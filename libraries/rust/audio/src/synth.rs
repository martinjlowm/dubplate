//! Signals with a known answer.
//!
//! Every test in this repo measures a stage against one of these rather than
//! against a track: a fixture whose true tempo is an assumption is a test that
//! fails for two different reasons and cannot tell them apart.

use crate::Audio;

/// A percussive pulse train at exactly `bpm`.
///
/// Each pulse is an exponentially decaying burst of a fixed pseudo-random
/// sequence, which gives the broadband onset a drum has and the deterministic
/// output a test needs.
pub fn click_train(bpm: f64, seconds: f64, sample_rate: u32) -> Audio {
    let len = (seconds * sample_rate as f64) as usize;
    let mut samples = vec![0.0f32; len];
    let period = 60.0 / bpm * sample_rate as f64;
    let decay = sample_rate as f64 * 0.02;

    let mut beat = 0usize;
    loop {
        let start = (beat as f64 * period).round() as usize;
        if start >= len {
            break;
        }
        let mut noise = 0x2545_F491_4F6C_DD1Du64 ^ beat as u64;
        for i in 0..(decay * 4.0) as usize {
            let Some(slot) = samples.get_mut(start + i) else {
                break;
            };
            noise = noise
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let white = ((noise >> 40) as f32 / 8_388_608.0) - 1.0;
            *slot += white * (-(i as f64) / decay).exp() as f32 * 0.8;
        }
        beat += 1;
    }

    Audio {
        sample_rate,
        source_channels: 1,
        samples,
    }
}

/// A steady sine, for checking that a frequency lands in the bin it should.
pub fn sine(frequency: f64, seconds: f64, sample_rate: u32) -> Audio {
    let len = (seconds * sample_rate as f64) as usize;
    let step = std::f64::consts::TAU * frequency / sample_rate as f64;
    Audio {
        sample_rate,
        source_channels: 1,
        samples: (0..len).map(|i| (i as f64 * step).sin() as f32).collect(),
    }
}

/// A sustained chord built from equal-temperament frequencies, each with two
/// harmonics. A pure-sine chord has no partials, so a chroma estimator scores
/// it perfectly for reasons no instrument reproduces.
pub fn chord(midi_notes: &[u8], seconds: f64, sample_rate: u32) -> Audio {
    let len = (seconds * sample_rate as f64) as usize;
    let mut samples = vec![0.0f32; len];
    for &note in midi_notes {
        let fundamental = 440.0 * 2f64.powf((note as f64 - 69.0) / 12.0);
        for (harmonic, gain) in [(1.0, 1.0), (2.0, 0.5), (3.0, 0.25)] {
            let step = std::f64::consts::TAU * fundamental * harmonic / sample_rate as f64;
            for (i, s) in samples.iter_mut().enumerate() {
                *s += ((i as f64 * step).sin() * gain) as f32;
            }
        }
    }
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs())).max(1e-9);
    for s in &mut samples {
        *s /= peak;
    }
    Audio {
        sample_rate,
        source_channels: 1,
        samples,
    }
}

/// Mix two signals sample-for-sample, truncating to the shorter one.
pub fn mix(a: &Audio, b: &Audio, gain_b: f32) -> Audio {
    let len = a.samples.len().min(b.samples.len());
    Audio {
        sample_rate: a.sample_rate,
        source_channels: 1,
        samples: (0..len)
            .map(|i| a.samples[i] + b.samples[i] * gain_b)
            .collect(),
    }
}
