//! How hard a track hits, measured without asking what it is.
//!
//! A set is built along an axis that is not tempo. A 200 BPM hardcore track and
//! a 200 BPM drum and bass roller are the same number and nothing like each
//! other, and a 140 BPM hard trance record sits above a 152 BPM techno one. So
//! this reduces four things to one score, and a library sorted by it runs from
//! a soul ballad at the bottom to raw hardstyle at the top.
//!
//! # What goes in, and what deliberately does not
//!
//! Brightness, crest factor, onset density and the share above 4 kHz. Tempo is
//! left out, and so is anything read off the beat grid, because the score is
//! used to choose the tempo settings: a track reported at twice its real tempo
//! would otherwise carry that error into the measurement meant to catch it.
//!
//! Crest factor and, through it, dynamic range run the other way from the rest.
//! A brick-walled master has a low peak-to-RMS ratio, and that is a mark of the
//! loud end of the spectrum rather than the quiet one, so it is weighted
//! negative.
//!
//! # The constants
//!
//! [`WEIGHTS`] is the first principal component of those four over a
//! twenty-three track working set, with the means and deviations that
//! standardised them. They are fixed rather than fitted per run, so one track
//! scores the same alone as it does in a library. They are also therefore a
//! calibration: material far outside what they were derived from wants them
//! derived again.

use serde::{Deserialize, Serialize};

/// Where the low mid is read, against the top end, for brightness.
const LOW_MID: (f64, f64) = (100.0, 276.0);
const TOP: (f64, f64) = (2101.0, 16000.0);
/// Everything above this counts towards the high share.
const HIGH: f64 = 4000.0;

/// Mean, standard deviation and weight for each measurement, in the order
/// [`Energy`] lists them. Derived from the working set; see the module note.
const WEIGHTS: [(f64, f64, f64); 4] = [
    (-1.1877, 0.1940, 0.5828),  // brightness
    (10.1017, 1.5277, -0.3495), // crest, in dB
    (11.4666, 1.7940, 0.5096),  // onset density, per second
    (0.0437, 0.0320, 0.5277),   // share above 4 kHz
];

/// The tempo the band expects, and how far either side of it the tempo stage
/// keeps looking.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Weighting {
    pub centre_bpm: f64,
    /// How far the weighting reaches, in octaves.
    ///
    /// Not a caller's knob, because it is not a free one. Swept over the
    /// working set, 0.3 loses the correction on a 90 BPM R&B track and reports
    /// 120, and 1.5 loses the one on a 98 BPM soul ballad and reports 199.
    /// Between 0.5 and 1.0 every track reads the same, and 0.7 is the middle of
    /// that.
    pub width_octaves: f64,
}

/// The band a score falls in, and with it the settings it asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Band {
    /// Below -0.5. Soul, R&B, anything whose beat is slow and whose
    /// subdivisions are dense.
    Calm,
    /// Up to 1.0. House, trance, techno.
    Club,
    /// Above 1.0. Hardstyle, hardcore, and whatever arrives next.
    Hard,
}

impl Band {
    /// The tempo weighting this band asks the tempo stage for.
    ///
    /// `None` for [`Band::Hard`]: music that is genuinely fast is exactly what
    /// a weighting ruins, and the fast end is where the estimate needs no help.
    pub fn weighting(self) -> Option<Weighting> {
        let centre_bpm = match self {
            Band::Calm => 115.0,
            Band::Club => 140.0,
            Band::Hard => return None,
        };
        Some(Weighting {
            centre_bpm,
            width_octaves: 0.7,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Band::Calm => "calm",
            Band::Club => "club",
            Band::Hard => "hard",
        }
    }
}

/// One track's energy, and the four measurements behind it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Energy {
    /// The score. Zero is the middle of the working set, not of music.
    pub score: f64,
    pub band: Band,
    /// `log10` of the mean amplitude in the top end over the low mid.
    pub brightness: f64,
    /// Peak over RMS, in decibels. Low means heavily limited.
    pub crest_db: f64,
    /// Novelty peaks above the curve's own mean, per second.
    pub onset_density: f64,
    /// Share of the spectrum's power above 4 kHz.
    pub high_share: f64,
}

/// Measure one track.
///
/// `spectrum` is the long-term average as `(hertz, amplitude)` per bin, and
/// `novelty` is the onset curve at `frame_rate` frames per second. Neither the
/// tempo nor the beat grid is an argument, which is the point.
pub fn measure(
    samples: &[f32],
    spectrum: &[(f64, f64)],
    novelty: &[f32],
    frame_rate: f64,
) -> Energy {
    let brightness = (mean_amplitude(spectrum, TOP.0, TOP.1)
        / mean_amplitude(spectrum, LOW_MID.0, LOW_MID.1).max(f64::MIN_POSITIVE))
    .log10();

    let peak = samples.iter().fold(0f32, |m, v| m.max(v.abs())) as f64;
    let rms = if samples.is_empty() {
        0.0
    } else {
        (samples.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / samples.len() as f64).sqrt()
    };
    let crest_db = 20.0 * (peak / rms.max(f64::MIN_POSITIVE)).log10();

    // A peak above the curve's own mean, so a quiet passage contributes none
    // and a loud one does not contribute more than it has events.
    let mean = if novelty.is_empty() {
        0.0
    } else {
        novelty.iter().map(|v| *v as f64).sum::<f64>() / novelty.len() as f64
    };
    let peaks = novelty
        .windows(3)
        .filter(|w| w[1] > w[0] && w[1] >= w[2] && (w[1] as f64) > mean)
        .count();
    let seconds = novelty.len() as f64 / frame_rate.max(f64::MIN_POSITIVE);
    let onset_density = if seconds > 0.0 {
        peaks as f64 / seconds
    } else {
        0.0
    };

    // Power, not amplitude, and the same definition `SpectrumSummary` uses: a
    // share of energy weights a loud bin above a quiet one, and the weights
    // below were calibrated against that number.
    let power = |(_, amplitude): &(f64, f64)| amplitude * amplitude;
    let total: f64 = spectrum.iter().map(power).sum();
    let above: f64 = spectrum
        .iter()
        .filter(|(hz, _)| *hz >= HIGH)
        .map(power)
        .sum();
    let high_share = if total > 0.0 { above / total } else { 0.0 };

    let score = [brightness, crest_db, onset_density, high_share]
        .iter()
        .zip(WEIGHTS)
        .map(|(value, (mean, deviation, weight))| (value - mean) / deviation * weight)
        .sum();

    Energy {
        score,
        band: if score < -0.5 {
            Band::Calm
        } else if score < 1.0 {
            Band::Club
        } else {
            Band::Hard
        },
        brightness,
        crest_db,
        onset_density,
        high_share,
    }
}

/// Mean amplitude per bin between two frequencies.
///
/// Per bin rather than summed: 2 to 16 kHz holds six hundred bins where 100 to
/// 276 Hz holds eight, and a sum of the two compares bandwidths.
fn mean_amplitude(spectrum: &[(f64, f64)], low: f64, high: f64) -> f64 {
    let inside: Vec<f64> = spectrum
        .iter()
        .filter(|(hz, _)| *hz >= low && *hz < high)
        .map(|(_, amplitude)| *amplitude)
        .collect();
    if inside.is_empty() {
        0.0
    } else {
        inside.iter().sum::<f64>() / inside.len() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::{Band, measure};

    /// A bright, dense, limited signal outscores a dull, sparse, dynamic one.
    ///
    /// Synthesised rather than taken from a track, for the reason every test in
    /// this workspace is: a fixture whose energy is an assumption fails for two
    /// reasons and cannot separate them.
    #[test]
    fn a_loud_busy_signal_outscores_a_quiet_sparse_one() {
        let rate = 86.0;
        // Spectrum weighted to the top, and one weighted to the low mid.
        let bright: Vec<(f64, f64)> = (1..600).map(|b| (b as f64 * 21.5, 1.0)).collect();
        let dull: Vec<(f64, f64)> = (1..600)
            .map(|b| {
                let hz = b as f64 * 21.5;
                (hz, if hz < 400.0 { 1.0 } else { 0.02 })
            })
            .collect();

        // A limited signal: everything near full scale, so peak is close to RMS.
        let limited: Vec<f32> = (0..44100)
            .map(|i| if i % 2 == 0 { 0.9 } else { -0.9 })
            .collect();
        // A dynamic one: one spike in a quiet field.
        let mut dynamic = vec![0.02f32; 44100];
        dynamic[100] = 1.0;

        let busy: Vec<f32> = (0..860)
            .map(|i| if i % 4 == 0 { 1.0 } else { 0.1 })
            .collect();
        let sparse: Vec<f32> = (0..860)
            .map(|i| if i % 40 == 0 { 1.0 } else { 0.1 })
            .collect();

        let hot = measure(&limited, &bright, &busy, rate);
        let cold = measure(&dynamic, &dull, &sparse, rate);

        assert!(
            hot.score > cold.score,
            "bright, busy and limited scored {:.2} against {:.2} for dull, sparse and dynamic",
            hot.score,
            cold.score
        );
        assert_eq!(hot.band, Band::Hard, "scored {:.2}", hot.score);
        assert_eq!(cold.band, Band::Calm, "scored {:.2}", cold.score);
    }

    /// Every band asks for settings, and only the fast one declines a weighting.
    #[test]
    fn only_the_hard_band_declines_a_weighting() {
        assert_eq!(Band::Calm.weighting().map(|p| p.centre_bpm), Some(115.0));
        assert_eq!(Band::Club.weighting().map(|p| p.centre_bpm), Some(140.0));
        assert_eq!(Band::Hard.weighting(), None);
        // The width is the band's, not the caller's, and it is the same one.
        assert_eq!(Band::Calm.weighting().map(|p| p.width_octaves), Some(0.7));
    }
}
