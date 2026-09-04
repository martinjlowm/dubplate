//! Key profiles: what a major or minor key is expected to sound like, as
//! twelve weights starting on the tonic.
//!
//! Which profile is used changes the answer on real tracks, most often between a
//! key and its relative, so the choice is exposed on the command line and named
//! in the report rather than treated as an implementation detail.

use crate::Mode;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    /// Krumhansl and Kessler's probe-tone ratings. Derived from what listeners
    /// judged as fitting, on classical stimuli. Its minor profile weights the
    /// sixth and seventh degrees heavily, which reads a natural-minor loop as
    /// its relative major more often than the alternative does.
    Krumhansl,
    /// Temperley's revision, fitted to note counts in a corpus of scores rather
    /// than to listener ratings. Flatter, and the better default for music built
    /// on a repeating chord loop.
    Temperley,
}

impl Profile {
    pub fn weights(&self, mode: Mode) -> [f64; 12] {
        match (self, mode) {
            (Profile::Krumhansl, Mode::Major) => [
                6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
            ],
            (Profile::Krumhansl, Mode::Minor) => [
                6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
            ],
            (Profile::Temperley, Mode::Major) => [
                0.748, 0.060, 0.488, 0.082, 0.670, 0.460, 0.096, 0.715, 0.104, 0.366, 0.057, 0.400,
            ],
            (Profile::Temperley, Mode::Minor) => [
                0.712, 0.084, 0.474, 0.618, 0.049, 0.460, 0.105, 0.747, 0.404, 0.067, 0.133, 0.330,
            ],
        }
    }

    /// The profile rotated so that index 0 is C, for a key on `tonic`.
    pub fn rotated(&self, mode: Mode, tonic: u8) -> [f64; 12] {
        let weights = self.weights(mode);
        std::array::from_fn(|pitch_class| {
            weights[(pitch_class as i32 - tonic as i32).rem_euclid(12) as usize]
        })
    }
}

impl std::str::FromStr for Profile {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "krumhansl" => Ok(Profile::Krumhansl),
            "temperley" => Ok(Profile::Temperley),
            other => Err(format!("unknown key profile: {other}")),
        }
    }
}

/// The inverse of `from_str`, so a caller can print the default it is about to
/// use and get a string that parses back.
impl std::fmt::Display for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Profile::Krumhansl => "krumhansl",
            Profile::Temperley => "temperley",
        })
    }
}
