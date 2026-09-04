//! The short-time Fourier transform and the two reductions of it every later
//! stage reads: energy in log-spaced frequency bands, and energy folded onto the
//! twelve pitch classes.
//!
//! One pass over the audio feeds every consumer. A ten-minute track at a 512
//! sample hop is roughly 52 000 frames of 1025 bins; keeping that matrix costs
//! 200 MB and no stage needs it whole, so `for_each_frame` hands each magnitude
//! spectrum out once and keeps nothing.

pub mod bands;
pub mod chroma;

pub use bands::LogBands;
pub use chroma::{ChromaMapper, TuningEstimator};

use rustfft::num_complex::Complex32;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

/// A windowed, hopped Fourier transform over a mono buffer.
pub struct Stft {
    window_size: usize,
    hop: usize,
    window: Vec<f32>,
    fft: Arc<dyn Fft<f32>>,
    buffer: Vec<Complex32>,
    magnitudes: Vec<f32>,
}

impl Stft {
    /// `window_size` need not be a power of two, since rustfft is mixed-radix,
    /// so it can be chosen for the frequency resolution the question needs.
    pub fn new(window_size: usize, hop: usize) -> Self {
        assert!(window_size >= 2 && hop >= 1, "degenerate STFT geometry");
        let mut planner = FftPlanner::new();
        Stft {
            window: hann(window_size),
            fft: planner.plan_fft_forward(window_size),
            buffer: vec![Complex32::new(0.0, 0.0); window_size],
            magnitudes: vec![0.0; window_size / 2 + 1],
            window_size,
            hop,
        }
    }

    pub fn window_size(&self) -> usize {
        self.window_size
    }

    pub fn hop(&self) -> usize {
        self.hop
    }

    pub fn bin_count(&self) -> usize {
        self.window_size / 2 + 1
    }

    /// How many magnitude spectra a buffer of `sample_count` samples yields.
    pub fn frame_count(&self, sample_count: usize) -> usize {
        if sample_count < self.window_size {
            0
        } else {
            (sample_count - self.window_size) / self.hop + 1
        }
    }

    /// Frames per second, which is the sample rate of every novelty curve.
    pub fn frame_rate(&self, sample_rate: u32) -> f64 {
        sample_rate as f64 / self.hop as f64
    }

    /// Centre frequency of a bin, in hertz.
    pub fn bin_frequency(&self, bin: usize, sample_rate: u32) -> f64 {
        bin as f64 * sample_rate as f64 / self.window_size as f64
    }

    /// Call `sink` with the magnitude spectrum of each frame, in order.
    ///
    /// The slice is reused between calls, so a consumer that needs to keep a
    /// frame copies it.
    pub fn for_each_frame(&mut self, samples: &[f32], sink: impl FnMut(usize, &[f32])) {
        self.for_each_frame_stepped(samples, 1, sink);
    }

    /// As `for_each_frame`, but transforming only every `step`-th frame.
    ///
    /// Measurements that average over the whole track, the tuning offset among
    /// them, converge long before every frame has been read, and the transform
    /// is the expensive part of the pass.
    pub fn for_each_frame_stepped(
        &mut self,
        samples: &[f32],
        step: usize,
        mut sink: impl FnMut(usize, &[f32]),
    ) {
        let frames = self.frame_count(samples.len());
        // The window sums to a constant; dividing it out here rather than per
        // frame keeps a hot loop out of the pass over a ten-minute track.
        let gain = 2.0 / self.window.iter().sum::<f32>();
        for frame in (0..frames).step_by(step.max(1)) {
            let start = frame * self.hop;
            for (i, slot) in self.buffer.iter_mut().enumerate() {
                *slot = Complex32::new(samples[start + i] * self.window[i], 0.0);
            }
            self.fft.process(&mut self.buffer);
            // Only the first half is independent for a real input, and the
            // window's coherent gain is divided out so magnitudes read as the
            // amplitude of the component rather than as a window-size artefact.
            for (bin, slot) in self.magnitudes.iter_mut().enumerate() {
                *slot = self.buffer[bin].norm() * gain;
            }
            sink(frame, &self.magnitudes);
        }
    }
}

/// Periodic Hann window. Periodic rather than symmetric because consecutive
/// frames overlap-add: the symmetric variant leaves a ripple at the hop rate,
/// which lands in the novelty curve as a tone at exactly the frame rate.
pub fn hann(size: usize) -> Vec<f32> {
    (0..size)
        .map(|i| {
            let phase = std::f64::consts::TAU * i as f64 / size as f64;
            (0.5 - 0.5 * phase.cos()) as f32
        })
        .collect()
}

/// Amplitude to decibels, floored so silence does not become negative infinity.
pub fn to_db(amplitude: f32) -> f32 {
    20.0 * amplitude.max(1e-10).log10()
}
