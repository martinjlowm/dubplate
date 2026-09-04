//! Folding the spectrum onto the twelve pitch classes, and the tuning offset
//! that has to be measured before the fold is meaningful.

/// Cents between two frequencies.
fn cents(from_hz: f64, to_hz: f64) -> f64 {
    1200.0 * (to_hz / from_hz).log2()
}

/// Accumulates how far spectral peaks sit from equal temperament.
///
/// A track mastered a quarter-tone flat, or one produced against A = 432 Hz,
/// puts every partial halfway between two pitch classes. The fold then splits
/// each note across neighbours and the key estimate degrades into noise, so the
/// offset is measured first and the mapper is built around it.
#[derive(Clone, Debug)]
pub struct TuningEstimator {
    // One bucket per cent of deviation in [-50, 50).
    histogram: [f64; 100],
    min_hz: f64,
    max_hz: f64,
}

impl Default for TuningEstimator {
    fn default() -> Self {
        // Below 100 Hz an FFT bin spans more than a semitone, and above 4 kHz
        // partials are dense enough that the peak picker measures intermodulation
        // rather than notes.
        TuningEstimator {
            histogram: [0.0; 100],
            min_hz: 100.0,
            max_hz: 4000.0,
        }
    }
}

impl TuningEstimator {
    /// Feed one magnitude spectrum. Only local maxima count: every bin would
    /// weight the skirt of each peak as heavily as the peak itself, and the
    /// skirts are symmetric, so they wash the estimate towards zero.
    pub fn push(&mut self, magnitudes: &[f32], sample_rate: u32, window_size: usize) {
        let hz_per_bin = sample_rate as f64 / window_size as f64;
        let first = (self.min_hz / hz_per_bin).ceil() as usize;
        let last =
            ((self.max_hz / hz_per_bin).floor() as usize).min(magnitudes.len().saturating_sub(2));

        for bin in first.max(1)..last {
            let (prev, here, next) = (magnitudes[bin - 1], magnitudes[bin], magnitudes[bin + 1]);
            if here <= prev || here <= next {
                continue;
            }
            // Quadratic interpolation over the three points recovers the true
            // peak position; without it the estimate is quantised to bin width,
            // which is coarser than the deviation being measured.
            let offset = 0.5 * (prev - next) as f64 / (prev - 2.0 * here + next) as f64;
            let hz = (bin as f64 + offset) * hz_per_bin;
            if hz <= 0.0 {
                continue;
            }
            let semitones = 12.0 * (hz / 440.0).log2();
            let deviation = cents(440.0 * 2f64.powf(semitones.round() / 12.0), hz);
            let bucket = ((deviation + 50.0).floor() as isize).clamp(0, 99) as usize;
            self.histogram[bucket] += here as f64;
        }
    }

    /// The dominant deviation from equal temperament, in cents.
    pub fn cents(&self) -> f64 {
        let total: f64 = self.histogram.iter().sum();
        if total <= 0.0 {
            return 0.0;
        }
        // Circular mean over the three buckets around the mode: the histogram
        // wraps at ±50 cents, so a track sitting near the seam has half its mass
        // in each end bucket and a plain mean lands at zero.
        let mode = self
            .histogram
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i as isize)
            .unwrap_or(50);
        let mut weight = 0.0;
        let mut sum = 0.0;
        for delta in -1..=1 {
            let bucket = (mode + delta).rem_euclid(100) as usize;
            let value = self.histogram[bucket];
            sum += value * (mode + delta) as f64;
            weight += value;
        }
        (sum / weight.max(f64::MIN_POSITIVE)) - 49.5
    }
}

/// Folds spectral peaks onto the twelve pitch classes.
///
/// Peaks, not bins. An FFT bin is a fixed width in hertz and a semitone is not,
/// so mapping every bin assigns four times as many of them to the top pitch
/// class of an octave as to the bottom one. Broadband energy then lands in a
/// pattern set by the transform geometry, which is identical for every track,
/// and the ranking that follows reports the same key for all of them. Peaks
/// carry the partials and nothing else.
#[derive(Clone, Debug)]
pub struct ChromaMapper {
    reference_hz: f64,
    hz_per_bin: f64,
    first_bin: usize,
    last_bin: usize,
}

impl ChromaMapper {
    /// `tuning_cents` shifts the reference away from A = 440 Hz.
    ///
    /// The mapped range runs from 55 Hz to 3 kHz. Below, the fundamental of a
    /// kick drum carries no harmonic information but plenty of energy, and it
    /// would otherwise decide the key of every track in a club set; above,
    /// partials from different notes fall within one bin often enough that the
    /// peak picker measures intermodulation.
    pub fn new(sample_rate: u32, window_size: usize, tuning_cents: f64) -> Self {
        let hz_per_bin = sample_rate as f64 / window_size as f64;
        let bin_count = window_size / 2 + 1;
        ChromaMapper {
            reference_hz: 440.0 * 2f64.powf(tuning_cents / 1200.0),
            hz_per_bin,
            first_bin: ((55.0 / hz_per_bin).ceil() as usize).max(1),
            last_bin: ((3000.0 / hz_per_bin).floor() as usize).min(bin_count.saturating_sub(2)),
        }
    }

    /// Add one frame's peaks into a twelve-element chroma accumulator.
    ///
    /// The frame is normalised by its own total first, so a drop is not worth
    /// more to the key estimate than a breakdown of the same length.
    pub fn accumulate(&self, magnitudes: &[f32], chroma: &mut [f64; 12]) {
        let mut frame = [0.0f64; 12];
        for bin in self.first_bin..self.last_bin {
            let (previous, here, next) =
                (magnitudes[bin - 1], magnitudes[bin], magnitudes[bin + 1]);
            if here <= previous || here <= next {
                continue;
            }
            let denominator = previous - 2.0 * here + next;
            let offset = if denominator.abs() > f32::EPSILON {
                (0.5 * (previous - next) / denominator).clamp(-0.5, 0.5) as f64
            } else {
                0.0
            };
            let hz = (bin as f64 + offset) * self.hz_per_bin;
            let semitones = 12.0 * (hz / self.reference_hz).log2();
            // A peak more than a third of a semitone from equal temperament is
            // not a note of this scale: a snare, a sweep, or the skirt of
            // something louder nearby.
            if (semitones - semitones.round()).abs() > 0.33 {
                continue;
            }
            let class = ((semitones.round() as i64 + 9).rem_euclid(12)) as usize;
            // Squared, so the estimate follows energy rather than amplitude and
            // a partial 20 dB down contributes a hundredth, not a tenth.
            frame[class] += (here as f64).powi(2);
        }
        let total: f64 = frame.iter().sum();
        if total <= 0.0 {
            return;
        }
        for (slot, value) in chroma.iter_mut().zip(frame) {
            *slot += value / total;
        }
    }
}
