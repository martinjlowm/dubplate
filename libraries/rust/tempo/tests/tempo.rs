//! The tempo stage, measured against a pulse train whose tempo is exact.

use audio::synth;
use diagnostics::Severity;
use spectral::{LogBands, Stft};
use tempo::{FluxAccumulator, MetricalFloor, Novelty, TempoCurve, TempoSettings};

const WINDOW: usize = 2048;
const HOP: usize = 512;
const RATE: u32 = 44100;

/// The novelty curves the CLI builds, over a generated signal.
fn novelty(bpm: f64, seconds: f64) -> (Novelty, Novelty) {
    let signal = synth::click_train(bpm, seconds, RATE);
    let mut stft = Stft::new(WINDOW, HOP);
    let bands = LogBands::new(8, 30.0, 16_000.0, RATE, WINDOW);
    let mut energies = vec![0.0f32; bands.len()];
    let mut flux = FluxAccumulator::new(bands.len(), 1000.0);

    stft.for_each_frame(&signal.samples, |_, magnitudes| {
        bands.energies(magnitudes, &mut energies);
        flux.push(&energies);
    });

    let frame_rate = stft.frame_rate(RATE);
    let curves: Vec<Novelty> = flux
        .finish()
        .into_iter()
        .map(|c| Novelty::from_flux(c, frame_rate, 0.5))
        .collect();
    (Novelty::sum(&curves), Novelty::sum(&curves[..2]))
}

#[test]
fn reports_the_tempo_of_a_pulse_train_and_doubts_nothing() {
    let (broadband, low) = novelty(150.0, 60.0);
    let analysis = tempo::analyze(&broadband, &low, &TempoSettings::default());

    assert!(
        (analysis.bpm - 150.0).abs() < 0.5,
        "measured {:.2} BPM",
        analysis.bpm
    );
    assert!(analysis.grid.matched_fraction > 0.9);
    assert!(analysis.grid.pulse_ratio > 2.0);
    assert!(analysis.stability.interquartile_range_bpm < 1.0);

    let warnings: Vec<&str> = analysis
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .map(|d| d.code)
        .collect();
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");
}

#[test]
fn a_pulse_train_at_half_tempo_is_not_the_same_answer() {
    let settings = TempoSettings::default();
    let (fast, _) = novelty(160.0, 60.0);
    let (slow, _) = novelty(80.0, 60.0);
    let (fast_bpm, _) = tempo::best_bpm(&fast, &settings).unwrap();
    let (slow_bpm, _) = tempo::best_bpm(&slow, &settings).unwrap();
    assert!(
        (fast_bpm - 160.0).abs() < 0.5,
        "fast measured {fast_bpm:.2}"
    );
    assert!((slow_bpm - 80.0).abs() < 0.5, "slow measured {slow_bpm:.2}");
}

#[test]
fn the_metrical_floor_doubles_and_says_so() {
    // A curve with a strong peak at 70 BPM and a slightly weaker one at 140,
    // which is the shape a harmonic sum produces on a track counted in half
    // bars.
    let bpm: Vec<f64> = (0..=1600).map(|i| 60.0 + i as f64 * 0.1).collect();
    let salience = bpm
        .iter()
        .map(|&b| {
            let peak = |centre: f64, height: f64| height * (-((b - centre) / 0.6).powi(2)).exp();
            peak(70.0, 1.0) + peak(140.0, 0.8)
        })
        .collect();
    let curve = TempoCurve { bpm, salience };

    let floor = MetricalFloor {
        bpm: 90.0,
        min_salience_ratio: 0.5,
    };
    let (reported, shift) = tempo::tempogram::apply_floor(&curve, 70.0, Some(floor));
    assert!((reported - 140.0).abs() < 0.2, "reported {reported:.2}");
    let shift = shift.expect("the doubling must be recorded");
    assert!((shift.from_bpm - 70.0).abs() < 0.2);
    assert!(shift.to_salience < shift.from_salience);

    // With the floor off, the curve's own answer stands.
    let (unchanged, none) = tempo::tempogram::apply_floor(&curve, 70.0, None);
    assert_eq!(unchanged, 70.0);
    assert!(none.is_none());
}

#[test]
fn the_answer_is_a_whole_number_and_the_measurement_survives() {
    // 128 BPM generated exactly. The estimator lands a few hundredths off,
    // which is its own error rather than the track's, and that is the gap the
    // snap closes.
    let (broadband, low) = novelty(128.0, 60.0);
    let analysis = tempo::analyze(&broadband, &low, &TempoSettings::default());

    assert_eq!(
        analysis.bpm,
        analysis.bpm.round(),
        "reported {:.4} BPM, which is not a whole number",
        analysis.bpm
    );
    assert_eq!(analysis.bpm, 128.0, "reported {:.4} BPM", analysis.bpm);
    assert!(
        (analysis.bpm_measured - 128.0).abs() > 0.0 && (analysis.bpm_measured - 128.0).abs() < 0.25,
        "measured {:.4} BPM, which is either exact or outside the snap",
        analysis.bpm_measured
    );
    assert_eq!(
        tempo::format_bpm(analysis.bpm),
        "128",
        "the headline writes {}",
        tempo::format_bpm(analysis.bpm)
    );

    // With the snap off the measurement is the answer, decimals and all.
    let measured_only = TempoSettings {
        integer_snap_bpm: 0.0,
        ..TempoSettings::default()
    };
    let unsnapped = tempo::analyze(&broadband, &low, &measured_only);
    assert_eq!(unsnapped.bpm, unsnapped.bpm_measured);
    assert_ne!(unsnapped.bpm, unsnapped.bpm.round());
}

#[test]
fn a_tempo_between_two_integers_keeps_its_measurement_and_says_why() {
    // 127.5 BPM is a tempo no producer types in, and the snap has to leave it
    // alone rather than move the grid a quarter of a beat per bar.
    let (broadband, low) = novelty(127.5, 60.0);
    let analysis = tempo::analyze(&broadband, &low, &TempoSettings::default());

    assert!(
        (analysis.bpm - 127.5).abs() < 0.2,
        "reported {:.2} BPM instead of the measurement",
        analysis.bpm
    );
    let codes: Vec<&str> = analysis.diagnostics.iter().map(|d| d.code).collect();
    assert!(
        codes.contains(&"non-integer-tempo"),
        "a tempo half way between two integers raised {codes:?}"
    );
}

#[test]
fn every_metrical_level_carries_the_grid_a_rerun_would_measure() {
    let (broadband, low) = novelty(150.0, 60.0);
    let analysis = tempo::analyze(&broadband, &low, &TempoSettings::default());

    let level = |label: &str| {
        analysis
            .octave_relatives
            .iter()
            .find(|r| r.label == label)
            .unwrap_or_else(|| panic!("no {label} level in the report"))
    };

    // The reported level's row is the grid the run already fitted, so the two
    // cannot disagree without one of them being computed from something else.
    let answer = level("candidate");
    assert!(
        (answer.pulse_ratio - analysis.grid.pulse_ratio).abs() < 1e-9
            && (answer.matched_fraction - analysis.grid.matched_fraction).abs() < 1e-9,
        "the candidate row reads {:.4}x and {:.4} against the fitted grid's {:.4}x and {:.4}",
        answer.pulse_ratio,
        answer.matched_fraction,
        analysis.grid.pulse_ratio,
        analysis.grid.matched_fraction
    );

    // Half the tempo reads higher on pulse whatever the truth is, which is why
    // `level-fits-better` ignores it. Documented here because a future reader
    // will otherwise take the number as evidence.
    let half = level("half");
    assert!(
        half.pulse_ratio > answer.pulse_ratio,
        "half reads {:.2}x against the answer's {:.2}x, so the caveat on this column is stale",
        half.pulse_ratio,
        answer.pulse_ratio
    );

    // A level with beats landing on nothing reads lower on both.
    let three_halves = level("three-halves");
    assert!(
        three_halves.matched_fraction < answer.matched_fraction,
        "three-halves matched {:.2} against the answer's {:.2}",
        three_halves.matched_fraction,
        answer.matched_fraction
    );
}

#[test]
fn a_level_that_fits_better_than_the_answer_is_named() {
    // The search range is pinned to three-halves of the generated tempo, so the
    // answer is 180 BPM on a 120 BPM signal and the two-thirds level is the
    // truth. This is the case the octave comparison exists to catch.
    let (broadband, low) = novelty(120.0, 60.0);
    let forced = TempoSettings {
        min_bpm: 175.0,
        max_bpm: 185.0,
        ..TempoSettings::default()
    };
    let analysis = tempo::analyze(&broadband, &low, &forced);

    let codes: Vec<&str> = analysis.diagnostics.iter().map(|d| d.code).collect();
    assert!(
        codes.contains(&"level-fits-better"),
        "reported {:.2} BPM on a 120 BPM signal and raised {codes:?}",
        analysis.bpm
    );
}
