//! From band energies to a novelty curve: the one-dimensional signal every
//! tempo estimate is actually made of.
//!
//! Nothing downstream ever sees the audio again. If a tempo estimate is wrong,
//! the question is almost always whether the pulse is present in this curve, so
//! the curve is a first-class output and gets plotted next to the beat grid it
//! produced.

use serde::Serialize;

/// Positive spectral flux per band, accumulated frame by frame.
pub struct FluxAccumulator {
    compression: f32,
    previous: Vec<f32>,
    curves: Vec<Vec<f32>>,
    started: bool,
}

impl FluxAccumulator {
    /// `compression` is the gamma of a `ln(1 + gamma * energy)` compression.
    ///
    /// It is what makes a hi-hat in a quiet intro count as much as a kick in the
    /// drop. Without it the flux is dominated by whichever section is loudest,
    /// and a track that is mastered with a rising loudness curve reports the
    /// tempo of its last minute.
    pub fn new(bands: usize, compression: f32) -> Self {
        FluxAccumulator {
            compression,
            previous: vec![0.0; bands],
            curves: vec![Vec::new(); bands],
            started: false,
        }
    }

    pub fn push(&mut self, band_energies: &[f32]) {
        debug_assert_eq!(band_energies.len(), self.previous.len());
        for (band, &energy) in band_energies.iter().enumerate() {
            let compressed = (1.0 + self.compression * energy.max(0.0)).ln();
            let rise = if self.started {
                (compressed - self.previous[band]).max(0.0)
            } else {
                0.0
            };
            self.previous[band] = compressed;
            self.curves[band].push(rise);
        }
        self.started = true;
    }

    pub fn finish(self) -> Vec<Vec<f32>> {
        self.curves
    }
}

/// A novelty curve sampled at the STFT frame rate.
#[derive(Clone, Debug, Serialize)]
pub struct Novelty {
    pub frame_rate: f64,
    pub values: Vec<f32>,
}

impl Novelty {
    /// Subtract a moving average and rectify.
    ///
    /// The moving average is the whole trick: raw flux carries a slow swell that
    /// dwarfs the beat-to-beat structure, and an autocorrelation of it reports
    /// the arrangement, eight-bar phrases and all, instead of the pulse.
    /// `local_mean_seconds` sets what counts as slow; at 0.5 s it removes
    /// everything below about 2 Hz, which is under the slowest tempo measured.
    pub fn from_flux(flux: Vec<f32>, frame_rate: f64, local_mean_seconds: f64) -> Self {
        let half = ((local_mean_seconds * frame_rate) / 2.0).round().max(1.0) as usize;
        let mut values = Vec::with_capacity(flux.len());

        // Prefix sums, so the window cost does not grow with its width.
        let mut prefix = Vec::with_capacity(flux.len() + 1);
        prefix.push(0.0f64);
        for &v in &flux {
            prefix.push(prefix.last().unwrap() + v as f64);
        }

        for (i, &v) in flux.iter().enumerate() {
            let lower = i.saturating_sub(half);
            let upper = (i + half + 1).min(flux.len());
            let mean = (prefix[upper] - prefix[lower]) / (upper - lower) as f64;
            values.push((v as f64 - mean).max(0.0) as f32);
        }

        Novelty { frame_rate, values }
    }

    /// Sum of several band curves, each normalised to unit mean first.
    ///
    /// Normalising before the sum is what keeps a loud bass band from being the
    /// only voice in the mix: a broadband curve is wanted here, not a louder
    /// copy of band zero.
    pub fn sum(curves: &[Novelty]) -> Novelty {
        let frame_rate = curves.first().map(|c| c.frame_rate).unwrap_or(1.0);
        let length = curves.iter().map(|c| c.values.len()).min().unwrap_or(0);
        let mut values = vec![0.0f32; length];
        for curve in curves {
            let mean = curve.mean().max(f32::MIN_POSITIVE);
            for (slot, &v) in values.iter_mut().zip(&curve.values) {
                *slot += v / mean;
            }
        }
        Novelty { frame_rate, values }
    }

    pub fn mean(&self) -> f32 {
        if self.values.is_empty() {
            return 0.0;
        }
        self.values.iter().sum::<f32>() / self.values.len() as f32
    }

    pub fn duration_seconds(&self) -> f64 {
        self.values.len() as f64 / self.frame_rate
    }

    /// Value at a fractional frame index, linearly interpolated.
    pub fn at(&self, frame: f64) -> f32 {
        if frame < 0.0 {
            return 0.0;
        }
        let lower = frame.floor() as usize;
        let Some(&a) = self.values.get(lower) else {
            return 0.0;
        };
        let b = self.values.get(lower + 1).copied().unwrap_or(a);
        let t = (frame - lower as f64) as f32;
        a + (b - a) * t
    }

    /// Normalised autocorrelation over `0..=max_lag` frames.
    ///
    /// The curve is zero-meaned first. Skipping that leaves every lag dominated
    /// by the same constant offset, which flattens the ratios between candidate
    /// tempi to the point where a peak picker cannot separate them.
    pub fn autocorrelation(&self, max_lag: usize) -> Vec<f64> {
        let n = self.values.len();
        if n == 0 {
            return vec![0.0; max_lag + 1];
        }
        let mean = self.mean() as f64;
        let centred: Vec<f64> = self.values.iter().map(|&v| v as f64 - mean).collect();

        let mut acf = Vec::with_capacity(max_lag + 1);
        for lag in 0..=max_lag {
            if lag >= n {
                acf.push(0.0);
                continue;
            }
            let overlap = n - lag;
            let sum: f64 = (0..overlap).map(|i| centred[i] * centred[i + lag]).sum();
            // Divided by the overlap, not by n: a long lag correlates fewer
            // sample pairs, and the biased form tapers every candidate towards
            // zero in proportion to its period, which is a slow-tempo penalty
            // nobody asked for.
            acf.push(sum / overlap as f64);
        }
        let zero = acf[0];
        if zero > 0.0 {
            for v in &mut acf {
                *v /= zero;
            }
        }
        acf
    }
}
