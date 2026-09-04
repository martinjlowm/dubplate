//! Log-spaced frequency bands over the FFT bins.
//!
//! Two callers, one shape: the onset stage sums a handful of wide bands so a
//! kick and a hi-hat produce separate novelty curves, and the spectrogram
//! renderer uses several hundred narrow ones as image rows. Both want a
//! logarithmic axis, because a linear one spends nine tenths of its pixels above
//! 2 kHz where nothing about tempo or key happens.

use std::ops::Range;

/// Bin ranges for log-spaced bands, in ascending frequency order.
#[derive(Clone, Debug)]
pub struct LogBands {
    edges_hz: Vec<f64>,
    bins: Vec<Range<usize>>,
}

impl LogBands {
    /// `count` bands between `min_hz` and `max_hz`.
    ///
    /// Below roughly 100 Hz an FFT bin is wider than a band, so several bands
    /// would resolve to the same bin and the lowest would come out empty. Each
    /// band is widened to at least one bin instead: an empty band reads as
    /// silence, and silence in the bass is a claim about the track rather than
    /// about the analysis geometry.
    pub fn new(
        count: usize,
        min_hz: f64,
        max_hz: f64,
        sample_rate: u32,
        window_size: usize,
    ) -> Self {
        assert!(count >= 1 && min_hz > 0.0 && max_hz > min_hz);
        let nyquist = sample_rate as f64 / 2.0;
        let max_hz = max_hz.min(nyquist);
        let bin_count = window_size / 2 + 1;
        let hz_per_bin = sample_rate as f64 / window_size as f64;

        let edges_hz: Vec<f64> = (0..=count)
            .map(|i| min_hz * (max_hz / min_hz).powf(i as f64 / count as f64))
            .collect();

        let bins = (0..count)
            .map(|i| {
                let lower = (edges_hz[i] / hz_per_bin).floor() as usize;
                let upper = (edges_hz[i + 1] / hz_per_bin).ceil() as usize;
                let lower = lower.min(bin_count - 1);
                let upper = upper.clamp(lower + 1, bin_count);
                lower..upper
            })
            .collect();

        LogBands { edges_hz, bins }
    }

    pub fn len(&self) -> usize {
        self.bins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bins.is_empty()
    }

    pub fn bins(&self, band: usize) -> Range<usize> {
        self.bins[band].clone()
    }

    /// Lower and upper edge of a band, in hertz.
    pub fn range_hz(&self, band: usize) -> (f64, f64) {
        (self.edges_hz[band], self.edges_hz[band + 1])
    }

    pub fn centre_hz(&self, band: usize) -> f64 {
        (self.edges_hz[band] * self.edges_hz[band + 1]).sqrt()
    }

    /// Mean magnitude per band. Mean rather than sum: with log spacing an upper
    /// band covers a hundred times as many bins as a lower one, and summing
    /// would rank bands by their width.
    pub fn energies(&self, magnitudes: &[f32], out: &mut [f32]) {
        debug_assert_eq!(out.len(), self.bins.len());
        for (band, slot) in out.iter_mut().enumerate() {
            let bins = &magnitudes[self.bins[band].clone()];
            *slot = bins.iter().sum::<f32>() / bins.len() as f32;
        }
    }
}
