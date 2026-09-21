//! Reducing the transform pass to something a segmentation can hold.
//!
//! A ten-minute track is about 52 000 frames, and comparing every frame with
//! every other one is a matrix with two and a half billion cells in it. So the
//! pass writes into a coarse grid instead, one slot every tenth of a second,
//! and the segmentation compares bars rather than frames: a six-minute track at
//! 128 BPM is 192 bars, and 192 squared is a matrix that fits in a cache line
//! budget nobody has to think about.
//!
//! The accumulator takes the band energies the onset detector already computed,
//! so this costs one add per band per frame and no second transform.

/// Seconds of audio per slot of the coarse grid.
///
/// A tenth of a second is four times finer than the fastest beat this tool
//  searches for, so a bar boundary lands within one slot of where it belongs.
const SLOT_SECONDS: f64 = 0.1;

/// Band energies on a coarse time grid, filled in as the transform runs.
///
/// Mirrors `tempo::FluxAccumulator`: the pass hands it what it already has, and
/// nothing is kept per frame.
pub struct SectionFeatures {
    bands: usize,
    frames_per_slot: f64,
    sums: Vec<f64>,
    counts: Vec<f64>,
}

impl SectionFeatures {
    pub fn new(bands: usize, frame_rate: f64) -> Self {
        SectionFeatures {
            bands,
            frames_per_slot: (frame_rate * SLOT_SECONDS).max(1.0),
            sums: Vec::new(),
            counts: Vec::new(),
        }
    }

    /// Add one frame's band energies to the slot it falls in.
    pub fn push(&mut self, frame: usize, energies: &[f32]) {
        let slot = (frame as f64 / self.frames_per_slot) as usize;
        if slot >= self.counts.len() {
            self.counts.resize(slot + 1, 0.0);
            self.sums.resize((slot + 1) * self.bands, 0.0);
        }
        let at = slot * self.bands;
        for (slot_band, energy) in self.sums[at..at + self.bands].iter_mut().zip(energies) {
            *slot_band += f64::from(*energy);
        }
        self.counts[slot] += 1.0;
    }

    /// Mean energy per band per slot.
    pub fn finish(self) -> Grid {
        let mut means = self.sums;
        for (slot, count) in self.counts.iter().enumerate() {
            if *count > 0.0 {
                let at = slot * self.bands;
                for value in &mut means[at..at + self.bands] {
                    *value /= count;
                }
            }
        }
        Grid {
            bands: self.bands,
            slots: self.counts.len(),
            means,
        }
    }
}

/// The coarse grid, once the pass is over.
pub struct Grid {
    pub bands: usize,
    pub slots: usize,
    means: Vec<f64>,
}

impl Grid {
    fn slot(&self, index: usize) -> &[f64] {
        let at = index * self.bands;
        &self.means[at..at + self.bands]
    }

    /// Mean energy per band over a span of seconds, as amplitudes.
    fn between(&self, start_seconds: f64, end_seconds: f64) -> Vec<f64> {
        let first = (start_seconds / SLOT_SECONDS).floor().max(0.0) as usize;
        let last = (end_seconds / SLOT_SECONDS).ceil().max(0.0) as usize;
        let last = last.min(self.slots);
        let mut sum = vec![0.0; self.bands];
        let mut count = 0.0;
        for index in first..last {
            for (slot, value) in sum.iter_mut().zip(self.slot(index)) {
                *slot += value;
            }
            count += 1.0;
        }
        if count > 0.0 {
            for value in &mut sum {
                *value /= count;
            }
        }
        sum
    }

    /// One feature vector per bar, given where the bars start.
    ///
    /// `bar_starts` carries one more entry than there are bars, so the last bar
    /// has an end.
    pub fn per_bar(&self, bar_starts: &[f64]) -> Vec<Bar> {
        bar_starts
            .windows(2)
            .map(|pair| {
                let energies = self.between(pair[0], pair[1]);
                Bar {
                    start_seconds: pair[0],
                    end_seconds: pair[1],
                    energies,
                }
            })
            .collect()
    }
}

/// What one bar of the track sounds like, in as few numbers as will separate it
/// from the next one.
#[derive(Clone, Debug)]
pub struct Bar {
    pub start_seconds: f64,
    pub end_seconds: f64,
    /// Mean amplitude per onset band, lowest band first.
    pub energies: Vec<f64>,
}

impl Bar {
    /// The bar as a direction rather than a loudness, which is what a
    /// similarity between two bars has to compare.
    ///
    /// Compressed first: a drop is thirty decibels above an intro, and on a
    /// linear scale the quiet bars all look alike to a cosine.
    pub fn shape(&self) -> Vec<f64> {
        let compressed: Vec<f64> = self
            .energies
            .iter()
            .map(|energy| (1.0 + 1000.0 * energy.max(0.0)).ln())
            .collect();
        let norm = compressed.iter().map(|v| v * v).sum::<f64>().sqrt();
        if norm > 0.0 {
            compressed.into_iter().map(|v| v / norm).collect()
        } else {
            vec![0.0; self.energies.len()]
        }
    }

    /// Energy across every band, in decibels.
    pub fn broadband_db(&self) -> f64 {
        to_db(self.energies.iter().sum::<f64>())
    }

    /// Energy in the bands below roughly 130 Hz, which in a club production is
    /// the kick and nothing else. The tempo stage reads the same two bands.
    pub fn low_band_db(&self, bands: usize) -> f64 {
        to_db(self.energies.iter().take(bands).sum::<f64>())
    }
}

/// Amplitude to decibels, floored so a silent bar is a number rather than an
/// infinity that poisons every mean it lands in.
pub fn to_db(amplitude: f64) -> f64 {
    const FLOOR_DB: f64 = -120.0;
    if amplitude <= 0.0 {
        FLOOR_DB
    } else {
        (20.0 * amplitude.log10()).max(FLOOR_DB)
    }
}
