//! What the analysis can be told to do, and what it does when told nothing.
//!
//! Plain data. The CLI derives its flags from this and a browser fills it in
//! from a form, so the defaults live here rather than in either caller: two
//! copies of `0.25` would be two answers to what the snap tolerance is.

use key_detect::Profile;
use serde::{Deserialize, Serialize};

// Serde, because the CLI is no longer the only caller: a browser form is a JSON
// object on the way in and the report names the settings a run used on the way
// out. `default` on every field means a page that knows about half of these
// still produces the same defaults as `--help` prints for the rest.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct AnalysisOptions {
    /// STFT window, in samples. Larger resolves frequency, smaller resolves
    /// time; onsets need time and key needs frequency, which is the tension the
    /// default sits in the middle of.
    pub window: usize,
    /// STFT hop, in samples. Sets the frame rate of every novelty curve, and so
    /// the finest tempo difference that can be resolved.
    pub hop: usize,
    /// Frequency bands the onset detector splits the spectrum into.
    pub onset_bands: usize,
    /// Gamma of the logarithmic compression applied before the flux.
    pub compression: f32,
    /// Width of the moving average subtracted from the flux, in seconds. Stays
    /// wider than a beat period or it removes the pulse being measured.
    pub local_mean: f64,
    /// Slowest tempo searched. Wide enough that an octave error stays inside
    /// the range and visible, rather than being clipped out of it.
    pub min_bpm: f64,
    /// Fastest tempo searched.
    pub max_bpm: f64,
    /// Spacing of the tempo grid, in BPM. The answer is refined between grid
    /// points, so this sets the cost of the search rather than the precision.
    pub bpm_resolution: f64,
    /// Comb teeth used by the tempo salience.
    pub pulses: usize,
    /// Weight of the penalty applied between comb teeth, in [0, 1]. At 0 the
    /// salience is a plain harmonic sum; at 1 it argues hardest against slow
    /// metrical levels, and against any tempo whose offbeats carry weight.
    pub comb_penalty: f64,
    /// Slowest metrical level the answer may be reported at, in BPM. At 0 the
    /// salience curve's own answer stands, subharmonic and all.
    pub metrical_floor: f64,
    /// How strong the doubled candidate must be, relative to the original, for
    /// the metrical floor to double it.
    pub metrical_floor_ratio: f64,
    /// Largest gap, in BPM, the answer may be moved by to reach a whole number.
    pub integer_snap: f64,
    /// Centre of a log-normal tempo prior, in BPM. Off unless set, because a
    /// prior is how a detector reports the tempo it expected.
    pub tempo_prior: Option<f64>,
    /// Width of the tempo prior, in octaves.
    pub tempo_prior_width: f64,
    /// Key profile the chroma is correlated against.
    pub key_profile: Profile,
    /// Override the measured tuning offset, in cents from A = 440 Hz.
    pub tuning_cents: Option<f64>,
}

impl Default for AnalysisOptions {
    fn default() -> Self {
        AnalysisOptions {
            window: 2048,
            hop: 512,
            onset_bands: 8,
            compression: 1000.0,
            local_mean: 0.5,
            min_bpm: 60.0,
            max_bpm: 220.0,
            bpm_resolution: 0.1,
            pulses: 4,
            comb_penalty: 0.0,
            metrical_floor: 90.0,
            metrical_floor_ratio: 0.5,
            integer_snap: 0.25,
            tempo_prior: None,
            tempo_prior_width: 0.7,
            key_profile: Profile::Temperley,
            tuning_cents: None,
        }
    }
}
