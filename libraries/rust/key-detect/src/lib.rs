//! Key estimation by correlating a chroma vector against key profiles.
//!
//! The chroma vector arrives from the spectral stage; everything here is twelve
//! rotations of a twelve-element correlation, which makes the whole stage
//! auditable by reading the ranked table it emits. The profile is a choice with
//! consequences, so it is named in the output rather than baked in.

pub mod profiles;

pub use profiles::Profile;

use diagnostics::Diagnostic;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Major,
    Minor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Key {
    /// 0 = C, 1 = C#, … 11 = B.
    pub tonic: u8,
    pub mode: Mode,
}

const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

impl Key {
    pub fn name(&self) -> String {
        let mode = match self.mode {
            Mode::Major => "major",
            Mode::Minor => "minor",
        };
        format!("{} {}", NAMES[self.tonic as usize % 12], mode)
    }

    /// Camelot notation, as printed on the wheel every DJ mixing program uses.
    ///
    /// Derived rather than tabulated: the wheel is the circle of fifths, so the
    /// number is a position on it and a minor key takes the number of its
    /// relative major.
    pub fn camelot(&self) -> String {
        let major_tonic = match self.mode {
            Mode::Major => self.tonic as i32,
            Mode::Minor => (self.tonic as i32 + 3).rem_euclid(12),
        };
        let number = ((major_tonic * 7).rem_euclid(12) + 7).rem_euclid(12) + 1;
        let letter = match self.mode {
            Mode::Major => 'B',
            Mode::Minor => 'A',
        };
        format!("{number}{letter}")
    }

    /// The relative major or minor: the confusion a correlation makes most
    /// often, because the two share every note.
    pub fn relative(&self) -> Key {
        match self.mode {
            Mode::Major => Key {
                tonic: ((self.tonic as i32 + 9).rem_euclid(12)) as u8,
                mode: Mode::Minor,
            },
            Mode::Minor => Key {
                tonic: ((self.tonic as i32 + 3).rem_euclid(12)) as u8,
                mode: Mode::Major,
            },
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct KeyScore {
    pub key: Key,
    /// Camelot notation, which is how every reader of this report mixes.
    pub camelot: String,
    /// The same key written as notes, for whoever wants it.
    pub name: String,
    /// Pearson correlation between the chroma vector and the rotated profile.
    pub correlation: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct KeyAnalysis {
    pub profile: Profile,
    /// The chroma vector the ranking was computed from, normalised to sum 1.
    pub chroma: [f64; 12],
    /// Deviation from A = 440 Hz the chroma mapping compensated for.
    pub tuning_cents: f64,
    pub key: Key,
    /// Camelot notation, which is what the terminal, the page and the device
    /// databases all show.
    pub camelot: String,
    pub name: String,
    /// Margin between the winner and the runner-up, as a fraction of the
    /// winner's correlation. Under about 0.05 the two are indistinguishable.
    pub margin: f64,
    pub ranked: Vec<KeyScore>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Rank all 24 keys against a chroma vector.
pub fn analyze(chroma: [f64; 12], tuning_cents: f64, profile: Profile) -> KeyAnalysis {
    let total: f64 = chroma.iter().sum();
    let normalised: [f64; 12] = if total > 0.0 {
        std::array::from_fn(|i| chroma[i] / total)
    } else {
        [0.0; 12]
    };

    let mut ranked: Vec<KeyScore> = [Mode::Major, Mode::Minor]
        .into_iter()
        .flat_map(|mode| {
            (0..12u8).map(move |tonic| {
                let key = Key { tonic, mode };
                KeyScore {
                    key,
                    camelot: key.camelot(),
                    name: key.name(),
                    correlation: correlation(&normalised, &profile.rotated(mode, tonic)),
                }
            })
        })
        .collect();
    ranked.sort_by(|a, b| b.correlation.total_cmp(&a.correlation));

    let best = ranked[0].clone();
    let runner_up = ranked[1].clone();
    let margin = if best.correlation.abs() > f64::MIN_POSITIVE {
        (best.correlation - runner_up.correlation) / best.correlation.abs()
    } else {
        0.0
    };

    let mut analysis = KeyAnalysis {
        profile,
        chroma: normalised,
        tuning_cents,
        key: best.key,
        camelot: best.camelot.clone(),
        name: best.name.clone(),
        margin,
        ranked,
        diagnostics: Vec::new(),
    };
    analysis.diagnostics = inspect(&analysis, &runner_up);
    analysis
}

fn inspect(analysis: &KeyAnalysis, runner_up: &KeyScore) -> Vec<Diagnostic> {
    let mut out = Vec::new();

    if analysis.margin < 0.05 {
        let relative = runner_up.key == analysis.key.relative();
        out.push(Diagnostic::warning(
            if relative {
                "relative-key-tie"
            } else {
                "key-tie"
            },
            format!(
                "{} scores within {:.1}% of {}{}",
                runner_up.camelot,
                analysis.margin * 100.0,
                analysis.camelot,
                if relative {
                    ": the two share every note, so the ranking is decided by which degree the track dwells on"
                } else {
                    ""
                }
            ),
        ));
    }

    if analysis.tuning_cents.abs() > 15.0 {
        out.push(Diagnostic::info(
            "tuning-offset",
            format!(
                "the track sits {:+.0} cents from A = 440 Hz and the chroma mapping was shifted to match; an uncompensated estimator reads this track differently",
                analysis.tuning_cents
            ),
        ));
    }

    // Chroma flatness: a percussive track puts equal energy in every pitch
    // class, and correlating that against 24 profiles still returns a winner.
    let mean = analysis.chroma.iter().sum::<f64>() / 12.0;
    let spread = (analysis
        .chroma
        .iter()
        .map(|c| (c - mean).powi(2))
        .sum::<f64>()
        / 12.0)
        .sqrt();
    let flatness = if mean > 0.0 { spread / mean } else { 0.0 };
    if flatness < 0.25 {
        out.push(Diagnostic::warning(
            "flat-chroma",
            format!(
                "pitch-class energy is nearly uniform (spread {:.2} of the mean), which is what a purely percussive track looks like; the ranking below is arbitrary",
                flatness
            ),
        ));
    }

    out
}

/// Pearson correlation between two twelve-element vectors.
fn correlation(a: &[f64; 12], b: &[f64; 12]) -> f64 {
    let mean_a = a.iter().sum::<f64>() / 12.0;
    let mean_b = b.iter().sum::<f64>() / 12.0;
    let mut covariance = 0.0;
    let mut variance_a = 0.0;
    let mut variance_b = 0.0;
    for i in 0..12 {
        let (da, db) = (a[i] - mean_a, b[i] - mean_b);
        covariance += da * db;
        variance_a += da * da;
        variance_b += db * db;
    }
    let denominator = (variance_a * variance_b).sqrt();
    if denominator > 0.0 {
        covariance / denominator
    } else {
        0.0
    }
}
