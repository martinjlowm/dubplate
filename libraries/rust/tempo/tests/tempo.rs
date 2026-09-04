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
