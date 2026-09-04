//! The transform and its two reductions, measured against signals whose
//! frequency content is known exactly.

use audio::synth;
use spectral::{ChromaMapper, LogBands, Stft, TuningEstimator};

const WINDOW: usize = 4096;
const HOP: usize = 1024;
const RATE: u32 = 44100;

/// Average magnitude spectrum over a whole signal.
fn average_spectrum(samples: &[f32]) -> Vec<f32> {
    let mut stft = Stft::new(WINDOW, HOP);
    let mut sum = vec![0.0f32; stft.bin_count()];
    let mut frames = 0.0f32;
    stft.for_each_frame(samples, |_, magnitudes| {
        for (slot, magnitude) in sum.iter_mut().zip(magnitudes) {
            *slot += magnitude;
        }
        frames += 1.0;
    });
    sum.iter().map(|s| s / frames).collect()
}

#[test]
fn a_sine_lands_in_its_own_bin_at_its_own_amplitude() {
    let signal = synth::sine(440.0, 2.0, RATE);
    let spectrum = average_spectrum(&signal.samples);
    let stft = Stft::new(WINDOW, HOP);

    let peak = spectrum
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(bin, _)| bin)
        .unwrap();
    let hz = stft.bin_frequency(peak, RATE);
    assert!((hz - 440.0).abs() < RATE as f64 / WINDOW as f64);
    // The window gain is divided out, so a full-scale sine reads near 1.0.
    assert!(
        (spectrum[peak] - 1.0).abs() < 0.1,
        "amplitude {}",
        spectrum[peak]
    );
}

#[test]
fn log_bands_ascend_and_cover_the_range() {
    let bands = LogBands::new(24, 30.0, 16_000.0, RATE, WINDOW);
    assert_eq!(bands.len(), 24);
    for band in 0..bands.len() {
        let bins = bands.bins(band);
        assert!(bins.start < bins.end, "band {band} is empty");
        let (low, high) = bands.range_hz(band);
        assert!(low < high);
    }
    assert!(bands.centre_hz(0) < bands.centre_hz(23));
}

#[test]
fn chroma_finds_the_notes_of_a_chord() {
    // C major: C4, E4, G4.
    let signal = synth::chord(&[60, 64, 67], 2.0, RATE);
    let mapper = ChromaMapper::new(RATE, WINDOW, 0.0);
    let mut stft = Stft::new(WINDOW, HOP);
    let mut chroma = [0.0f64; 12];
    stft.for_each_frame(&signal.samples, |_, magnitudes| {
        mapper.accumulate(magnitudes, &mut chroma);
    });

    let total: f64 = chroma.iter().sum();
    // C = 0, E = 4, G = 7.
    for class in [0, 4, 7] {
        assert!(
            chroma[class] / total > 0.15,
            "pitch class {class} holds only {:.3} of the energy",
            chroma[class] / total
        );
    }
    // Every note not in the chord and not one of its partials stays quiet.
    assert!(chroma[1] / total < 0.05);
    assert!(chroma[6] / total < 0.05);
}

#[test]
fn tuning_offset_is_measured_not_assumed() {
    // 440 Hz raised by 30 cents.
    let detuned = 440.0 * 2f64.powf(30.0 / 1200.0);
    let signal = synth::sine(detuned, 2.0, RATE);
    let mut estimator = TuningEstimator::default();
    let mut stft = Stft::new(WINDOW, HOP);
    stft.for_each_frame(&signal.samples, |_, magnitudes| {
        estimator.push(magnitudes, RATE, WINDOW);
    });
    assert!(
        (estimator.cents() - 30.0).abs() < 6.0,
        "measured {:.1} cents",
        estimator.cents()
    );
}
