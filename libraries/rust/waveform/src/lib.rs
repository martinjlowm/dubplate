//! The waveform a player draws on its screen.
//!
//! One column is a height and a balance between three frequency bands. Pioneer
//! and Denon both draw the same two pictures, a whole-track preview and a
//! scrolling detail view, and both colour them by that balance, so the columns
//! are computed once here and each exporter packs them its own way.
//!
//! Height comes from the peak sample in the column. The bands come from three
//! one-pole filters run over the samples rather than from a second transform: at
//! 150 columns per second a column is 294 samples, which is shorter than a
//! window that would resolve the split properly, and the eye is being served
//! here rather than the analysis.

use deku::prelude::*;
use serde::{Deserialize, Serialize};

/// Columns per second in the detailed waveform.
///
/// Fixed by the format: a rekordbox detail waveform is read at 150 columns per
/// second and a player scrolls it against the beat grid, so a different rate
/// draws a track that drifts against its own beats.
pub const DETAIL_COLUMNS_PER_SECOND: f64 = 150.0;

/// Columns in the monochrome whole-track preview, also fixed by the format.
pub const PREVIEW_COLUMNS: usize = 400;

/// Columns in the colour whole-track preview.
pub const COLOUR_PREVIEW_COLUMNS: usize = 1200;

/// Crossovers between the three bands a column is coloured by, in hertz.
///
/// 200 Hz puts a kick and a bassline in the low band on its own. 2 kHz is above
/// everything with a fundamental and below most of what a hi-hat is made of.
const BAND_CROSSOVERS_HZ: [f64; 3] = [200.0, 2000.0, 10_000.0];

/// One column: a height, and how the energy in it splits three ways.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Column {
    /// 0 to 31, as every format that draws it stores five bits.
    pub height: u8,
    /// 0 to 7 each, scaled so the loudest band of the column reads 7. Colour is
    /// a balance rather than a level: the level is the height.
    pub low: u8,
    pub mid: u8,
    pub high: u8,
}

/// The two bytes a column is stored as, as the fields they are.
///
/// Written from the low bit of the first byte upwards: five bits of height,
/// three of low band, then three each of mid and high in the second byte, and
/// two that no format uses. `bit_order = "lsb"` is what says "from the low bit
/// up"; the default fills from the high bit down and produces a file a player
/// draws wrong rather than one it refuses.
#[derive(DekuRead, DekuWrite)]
#[deku(bit_order = "lsb")]
struct PackedColumn {
    #[deku(bits = 5)]
    height: u8,
    #[deku(bits = 3)]
    low: u8,
    #[deku(bits = 3)]
    mid: u8,
    #[deku(bits = 3)]
    high: u8,
    /// Unused by every format that reads these two bytes, and zero in what this
    /// writes. Named rather than left as a gap, so a reader of the struct sees
    /// the whole sixteen bits accounted for.
    #[deku(bits = 2)]
    spare: u8,
}

impl Column {
    /// Two bytes, which is what the JSON carries.
    ///
    /// A seven-minute track is 63 000 detail columns. As a JSON array of
    /// numbers that is a megabyte of digits and commas per track; packed and
    /// base64-encoded it is 170 kB.
    fn pack(&self) -> [u8; 2] {
        let column = self.clamped();
        let packed = PackedColumn {
            height: column.height,
            low: column.low,
            mid: column.mid,
            high: column.high,
            spare: 0,
        };
        // Infallible because every field came through `clamped`.
        let bytes = packed.to_bytes().expect("a clamped column fits its layout");
        [bytes[0], bytes[1]]
    }

    fn unpack(bytes: [u8; 2]) -> Self {
        let (_, packed) =
            PackedColumn::from_bytes((&bytes, 0)).expect("a fixed sixteen-bit layout");
        Column {
            height: packed.height,
            low: packed.low,
            mid: packed.mid,
            high: packed.high,
        }
    }

    /// The shade a monochrome waveform draws this column in, 0 to 7.
    ///
    /// Brightness follows the top of the spectrum, which is what makes a hi-hat
    /// read white and a kick read dark in the old two-colour views.
    pub fn shade(&self) -> u8 {
        self.high.max(self.mid.saturating_sub(2))
    }

    /// The same column with every field inside the range its bit width allows.
    ///
    /// Every packed layout declares five bits of height and three of each band,
    /// and a declared layout refuses a value too wide for its field rather than
    /// truncating it, so a column of unknown provenance passes through here
    /// before it is written. `render` already clamps, and the fields are public,
    /// so this states the range once instead of at each of the four places a
    /// column becomes bytes.
    pub fn clamped(self) -> Self {
        Column {
            height: self.height.min(31),
            low: self.low.min(7),
            mid: self.mid.min(7),
            high: self.high.min(7),
        }
    }
}

/// A waveform as columns.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Waveform {
    pub columns_per_second: f64,
    /// Two bytes per column, base64 in JSON.
    #[serde(rename = "columns", with = "base64_bytes")]
    packed: Vec<u8>,
}

impl Waveform {
    pub fn from_columns(columns_per_second: f64, columns: &[Column]) -> Self {
        Waveform {
            columns_per_second,
            packed: columns.iter().flat_map(|column| column.pack()).collect(),
        }
    }

    pub fn column(&self, index: usize) -> Column {
        let at = index * 2;
        match self.packed.get(at..at + 2) {
            Some(bytes) => Column::unpack([bytes[0], bytes[1]]),
            None => Column {
                height: 0,
                low: 0,
                mid: 0,
                high: 0,
            },
        }
    }

    pub fn columns(&self) -> impl Iterator<Item = Column> + '_ {
        (0..self.len()).map(|index| self.column(index))
    }

    pub fn len(&self) -> usize {
        self.packed.len() / 2
    }

    pub fn is_empty(&self) -> bool {
        self.packed.is_empty()
    }

    pub fn height(&self, index: usize) -> u8 {
        self.column(index).height
    }
}

/// Render `column_count` columns spanning the whole buffer.
pub fn render(samples: &[f32], sample_rate: u32, column_count: usize) -> Waveform {
    let column_count = column_count.max(1);
    let seconds = samples.len() as f64 / sample_rate as f64;
    let mut peaks = vec![0.0f32; column_count];
    // Low, mid, high and everything above the top crossover, which is folded
    // into the high band for colour but kept separate while filtering.
    let mut energy = vec![[0.0f64; 4]; column_count];

    // One-pole low passes at each crossover. The coefficient is the usual
    // exp(-2 pi fc / fs) form, so the crossovers stay where they are whatever
    // the file's sample rate is.
    let coefficients =
        BAND_CROSSOVERS_HZ.map(|hz| (-std::f64::consts::TAU * hz / sample_rate as f64).exp());
    let mut states = [0.0f64; 3];

    for (index, &sample) in samples.iter().enumerate() {
        // Widened, because usize is 32 bits in a browser. The product is a
        // sample index times a column count, and a 1200-column preview passes
        // four billion 81 seconds into a 44.1 kHz track; the detail render, at
        // 150 columns a second, passes it after 26. Every track a person owns
        // is longer than that, so on wasm32 this multiplied over on all of them
        // and the analysis died on the first one.
        let column =
            usize::try_from(index as u64 * column_count as u64 / samples.len().max(1) as u64)
                .unwrap_or(usize::MAX)
                .min(column_count - 1);
        let value = sample as f64;

        for (state, coefficient) in states.iter_mut().zip(coefficients) {
            *state = value * (1.0 - coefficient) + *state * coefficient;
        }
        let bands = [
            states[0],
            states[1] - states[0],
            states[2] - states[1],
            value - states[2],
        ];

        peaks[column] = peaks[column].max(sample.abs());
        for (accumulated, band) in energy[column].iter_mut().zip(bands) {
            *accumulated += band * band;
        }
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

    let columns: Vec<Column> = (0..column_count)
        .map(|column| {
            let height = ((peaks[column] / reference).clamp(0.0, 1.0) * 31.0).round() as u8;
            let [low, mid, high, air] = energy[column];
            let high = high + air;
            let loudest = low.max(mid).max(high);
            let level = |band: f64| {
                if loudest > 0.0 {
                    ((band / loudest).clamp(0.0, 1.0) * 7.0).round() as u8
                } else {
                    0
                }
            };
            Column {
                height: height.min(31),
                low: level(low),
                mid: level(mid),
                high: level(high),
            }
        })
        .collect();

    Waveform::from_columns(
        column_count as f64 / seconds.max(f64::MIN_POSITIVE),
        &columns,
    )
}

/// The whole-track preview a player shows above the detailed view.
///
/// Rendered at the colour preview's width, which is three times the monochrome
/// one, so both can be written from the same columns.
pub fn preview(samples: &[f32], sample_rate: u32) -> Waveform {
    render(samples, sample_rate, COLOUR_PREVIEW_COLUMNS)
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

/// Resample a waveform to a fixed number of columns, keeping peaks.
///
/// The formats disagree about width: 400 columns for the monochrome preview,
/// 1200 for the colour one, 100 for the tiny view on the track list. Peaks are
/// kept rather than averaged, because a waveform that loses them looks like a
/// track with no transients.
pub fn resample(waveform: &Waveform, column_count: usize) -> Vec<Column> {
    (0..column_count)
        .map(|column| {
            let from = column * waveform.len() / column_count.max(1);
            let to = ((column + 1) * waveform.len() / column_count.max(1)).max(from + 1);
            (from..to)
                .map(|index| waveform.column(index))
                .max_by_key(|column| column.height)
                .unwrap_or(Column {
                    height: 0,
                    low: 0,
                    mid: 0,
                    high: 0,
                })
        })
        .collect()
}

/// Base64 in JSON.
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

#[cfg(test)]
mod packing {
    use super::*;

    /// The layout these two bytes had when they were written as shifts, kept as
    /// the oracle for the derived one.
    ///
    /// Bit order is the one thing a derive macro can get wrong without failing:
    /// filling from the high bit down produces two bytes of the right length
    /// holding the wrong numbers, which a player draws rather than rejects. So
    /// the proof is exhaustive over every value the four fields can hold.
    fn shifted(column: &Column) -> [u8; 2] {
        [
            (column.height & 0x1f) | ((column.low & 0x07) << 5),
            (column.mid & 0x07) | ((column.high & 0x07) << 3),
        ]
    }

    #[test]
    fn the_declared_layout_is_the_layout_the_shifts_wrote() {
        for height in 0..32u8 {
            for low in 0..8u8 {
                for mid in 0..8u8 {
                    for high in 0..8u8 {
                        let column = Column {
                            height,
                            low,
                            mid,
                            high,
                        };
                        assert_eq!(
                            column.pack(),
                            shifted(&column),
                            "height {height} low {low} mid {mid} high {high}"
                        );
                        assert_eq!(
                            Column::unpack(column.pack()),
                            column,
                            "height {height} low {low} mid {mid} high {high} did not survive"
                        );
                    }
                }
            }
        }
    }
}
