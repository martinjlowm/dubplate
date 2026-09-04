//! The waveform a player draws on its screen.
//!
//! One column is a height and a shade. Both Pioneer and Denon draw the same two
//! things, at different resolutions and in different byte layouts, so the
//! columns are computed once here in a device-neutral form and each exporter
//! packs them its own way.
//!
//! Height comes from the peak sample in the column and shade from how much of
//! that column's energy is above 500 Hz, which is what makes a kick read as dark
//! and a hi-hat as bright. The split is a one-pole filter over the samples
//! rather than a second transform: at 150 columns per second a column is 294
//! samples, which is shorter than any window that would resolve the split
//! properly, and the eye is being served here rather than the analysis.

use serde::{Deserialize, Serialize};

/// Columns per second in the detailed waveform.
///
/// Fixed by the format: a rekordbox detail waveform is read at 150 columns per
/// second and a player scrolls it against the beat grid, so a different rate
/// draws a track that drifts against its own beats.
pub const DETAIL_COLUMNS_PER_SECOND: f64 = 150.0;

/// Columns in the whole-track preview, also fixed by the format.
pub const PREVIEW_COLUMNS: usize = 400;

/// Crossover between the dark and bright halves of the shade calculation.
const SHADE_CROSSOVER_HZ: f64 = 500.0;

/// A waveform as columns of height and shade.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Waveform {
    pub columns_per_second: f64,
    /// Height in `0..=31`, shade in `0..=7`, packed as rekordbox packs them:
    /// height in the low five bits, shade in the top three.
    #[serde(with = "base64_bytes")]
    pub columns: Vec<u8>,
}

impl Waveform {
    pub fn height(&self, column: usize) -> u8 {
        self.columns.get(column).map_or(0, |c| c & 0x1f)
    }

    pub fn shade(&self, column: usize) -> u8 {
        self.columns.get(column).map_or(0, |c| c >> 5)
    }

    pub fn len(&self) -> usize {
        self.columns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.columns.is_empty()
    }
}

/// Render `column_count` columns spanning the whole buffer.
pub fn render(samples: &[f32], sample_rate: u32, column_count: usize) -> Waveform {
    let column_count = column_count.max(1);
    let seconds = samples.len() as f64 / sample_rate as f64;
    let mut peaks = vec![0.0f32; column_count];
    let mut low_energy = vec![0.0f64; column_count];
    let mut high_energy = vec![0.0f64; column_count];

    // One-pole low pass. The coefficient is the usual exp(-2 pi fc / fs) form,
    // so the crossover stays at 500 Hz whatever the file's sample rate is.
    let coefficient = (-std::f64::consts::TAU * SHADE_CROSSOVER_HZ / sample_rate as f64).exp();
    let mut low_state = 0.0f64;

    for (index, &sample) in samples.iter().enumerate() {
        let column = index * column_count / samples.len().max(1);
        let column = column.min(column_count - 1);

        low_state = sample as f64 * (1.0 - coefficient) + low_state * coefficient;
        let high = sample as f64 - low_state;

        peaks[column] = peaks[column].max(sample.abs());
        low_energy[column] += low_state * low_state;
        high_energy[column] += high * high;
    }

    // Scaled against a high percentile rather than the maximum: one clipped
    // sample or one stray transient otherwise flattens the whole waveform to a
    // quarter of the height it should have.
    let mut sorted: Vec<f32> = peaks.iter().copied().filter(|p| *p > 0.0).collect();
    sorted.sort_by(f32::total_cmp);
    let reference = sorted
        .get(sorted.len().saturating_mul(99) / 100)
        .or(sorted.last())
        .copied()
        .unwrap_or(1.0)
        .max(1e-6);

    let columns = (0..column_count)
        .map(|column| {
            let height = ((peaks[column] / reference).clamp(0.0, 1.0) * 31.0).round() as u8;
            let total = low_energy[column] + high_energy[column];
            let shade = if total > 0.0 {
                ((high_energy[column] / total).clamp(0.0, 1.0) * 7.0).round() as u8
            } else {
                0
            };
            (shade << 5) | height.min(31)
        })
        .collect();

    Waveform {
        columns_per_second: column_count as f64 / seconds.max(f64::MIN_POSITIVE),
        columns,
    }
}

/// The whole-track preview a player shows above the detailed view.
pub fn preview(samples: &[f32], sample_rate: u32) -> Waveform {
    render(samples, sample_rate, PREVIEW_COLUMNS)
}

/// The scrolling waveform, at the fixed rate the format reads it back at.
pub fn detail(samples: &[f32], sample_rate: u32) -> Waveform {
    let seconds = samples.len() as f64 / sample_rate as f64;
    let mut waveform = render(
        samples,
        sample_rate,
        (seconds * DETAIL_COLUMNS_PER_SECOND).round() as usize,
    );
    // The renderer derives the rate from the column count, which rounds; the
    // player reads it as exactly 150 and would drift against the beat grid.
    waveform.columns_per_second = DETAIL_COLUMNS_PER_SECOND;
    waveform
}

/// Base64 in JSON, because a detailed waveform is one byte per column and a
/// seven-minute track has 63 000 of them. As a JSON array of numbers that is a
/// quarter of a megabyte of digits and commas per track.
mod base64_bytes {
    use serde::{Deserialize, Deserializer, Serializer};

    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn serialize<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let mut buffer = [0u8; 3];
            buffer[..chunk.len()].copy_from_slice(chunk);
            let packed =
                u32::from(buffer[0]) << 16 | u32::from(buffer[1]) << 8 | u32::from(buffer[2]);
            for shift in [18, 12, 6, 0] {
                out.push(ALPHABET[(packed >> shift) as usize & 0x3f] as char);
            }
            // Pad the characters that came from bytes the chunk did not have.
            let padding = 3 - chunk.len();
            out.truncate(out.len() - padding);
            out.push_str(&"=".repeat(padding));
        }
        serializer.serialize_str(&out)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        let mut out = Vec::with_capacity(text.len() / 4 * 3);
        let mut accumulator = 0u32;
        let mut bits = 0u32;
        for character in text.bytes().filter(|b| *b != b'=') {
            let value = ALPHABET
                .iter()
                .position(|c| *c == character)
                .ok_or_else(|| serde::de::Error::custom("invalid base64 character"))?;
            accumulator = (accumulator << 6) | value as u32;
            bits += 6;
            if bits >= 8 {
                bits -= 8;
                out.push((accumulator >> bits) as u8);
            }
        }
        Ok(out)
    }
}
