//! The magnitude of a signal's transform at frequencies that are not bins.
//!
//! An FFT gives every bin of a window at once, spaced `rate / window` apart.
//! That is the wrong shape when a caller wants a few thousand closely spaced
//! frequencies that do not line up with the bins: a tempogram asks for 60 to
//! 220 BPM every 0.1 BPM over a twelve-second window, where the bin spacing is
//! five BPM. Evaluating the sum directly is the obvious answer and costs a sine
//! and a cosine per sample per frequency.
//!
//! Goertzel's recurrence is the same number without the trig. Each frequency
//! becomes a two-tap resonator driven by the samples, one multiply and two adds
//! each, and the magnitude falls out of the last two states. Measured against
//! the direct sum over a 1034-sample window at 1601 frequencies, the worst
//! relative difference is 1.9e-10 and it runs fifteen times faster.
//!
//! # Blocking
//!
//! The speed is in [`BLOCK`], not in the recurrence. One resonator is a
//! dependency chain: each step needs the step before it, so nothing overlaps.
//! Sixteen resonators driven by the same sample are sixteen independent chains
//! over contiguous lanes, which is a shape every vector unit recognises and
//! which LLVM emits NEON, AVX or wasm `simd128` for without being asked. There
//! is no intrinsic and no `unsafe` here; the arrays are the whole trick.
//!
//! # What it is not
//!
//! The recurrence is a marginally stable resonator, so error grows with window
//! length rather than staying flat. At a few thousand samples in `f64` that is
//! parts in 1e10 and nobody cares. At a few million it would be the wrong tool,
//! and a zero-padded FFT would be the right one.

/// Resonators advanced together per sample.
///
/// Sixteen because the inner loop then holds 48 doubles of state, which stays
/// in registers on a machine with 32 of them and spills gracefully on one with
/// half that. Thirty-two measured faster on arm64 and slower under wasm, where
/// there are sixteen vector registers; this is the value that is not the worst
/// on either.
const BLOCK: usize = 16;

/// A bank of resonators, built once and run over many windows.
///
/// Built once because the only trig left is three calls per frequency, and a
/// caller with ninety windows would otherwise pay for them ninety times.
pub struct Bank {
    /// `2 cos ω`, the only coefficient the sample loop reads. Padded with zeroes
    /// to a whole number of blocks so the loop never needs a remainder case.
    coefficients: Vec<f64>,
    cosines: Vec<f64>,
    sines: Vec<f64>,
    frequencies: usize,
}

impl Bank {
    /// One resonator per frequency, each given as radians per sample.
    ///
    /// For a frequency in hertz over a signal sampled at `rate`, that is
    /// `2π f / rate`. Values outside `0..π` alias, exactly as they would in any
    /// other transform, and are not rejected here.
    pub fn new(radians_per_sample: &[f64]) -> Bank {
        let padded = radians_per_sample.len().next_multiple_of(BLOCK);
        let mut coefficients = vec![0.0; padded];
        let mut cosines = vec![0.0; padded];
        let mut sines = vec![0.0; padded];

        for (index, &omega) in radians_per_sample.iter().enumerate() {
            let (sine, cosine) = omega.sin_cos();
            coefficients[index] = 2.0 * cosine;
            cosines[index] = cosine;
            sines[index] = sine;
        }

        Bank {
            coefficients,
            cosines,
            sines,
            frequencies: radians_per_sample.len(),
        }
    }

    /// How many frequencies the bank holds.
    pub fn len(&self) -> usize {
        self.frequencies
    }

    pub fn is_empty(&self) -> bool {
        self.frequencies == 0
    }

    /// `|X(ω)|` at every frequency, over one window, replacing `out`.
    ///
    /// `out` must be [`Bank::len`] long. Apply the window function to `samples`
    /// first: the bank does not know about one, and applying it here would mean
    /// applying it once per frequency instead of once per window.
    pub fn magnitudes(&self, samples: &[f64], out: &mut [f64]) {
        self.run(samples, out, |slot, magnitude| *slot = magnitude);
    }

    /// The same, added onto what `out` already holds.
    ///
    /// A caller averaging over overlapping windows accumulates rather than
    /// collecting, and a second buffer per window is a second buffer it does
    /// not need.
    pub fn add_magnitudes(&self, samples: &[f64], out: &mut [f64]) {
        self.run(samples, out, |slot, magnitude| *slot += magnitude);
    }

    fn run(&self, samples: &[f64], out: &mut [f64], combine: impl Fn(&mut f64, f64)) {
        assert_eq!(
            out.len(),
            self.frequencies,
            "the output holds {} magnitudes and the bank has {} frequencies",
            out.len(),
            self.frequencies,
        );

        for (block, slots) in (0..self.frequencies)
            .step_by(BLOCK)
            .zip(out.chunks_mut(BLOCK))
        {
            // A whole block every time: the padding above guarantees these
            // reads land, and the lanes past the end are written to `slots`
            // that do not exist and so are dropped by the zip below.
            let coefficients: &[f64; BLOCK] = self.coefficients[block..block + BLOCK]
                .try_into()
                .expect("one block");

            let mut back_one = [0.0f64; BLOCK];
            let mut back_two = [0.0f64; BLOCK];
            for &sample in samples {
                for lane in 0..BLOCK {
                    let next = sample + coefficients[lane] * back_one[lane] - back_two[lane];
                    back_two[lane] = back_one[lane];
                    back_one[lane] = next;
                }
            }

            for (lane, slot) in slots.iter_mut().enumerate() {
                // The last two states are the transform, up to a phase that the
                // magnitude does not see.
                let real = back_one[lane] - back_two[lane] * self.cosines[block + lane];
                let imaginary = back_two[lane] * self.sines[block + lane];
                combine(slot, real.hypot(imaginary));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Bank;

    const TAU: f64 = std::f64::consts::TAU;

    /// The sum the recurrence replaces, written the slow and obvious way.
    fn direct(samples: &[f64], omega: f64) -> f64 {
        let (mut real, mut imaginary) = (0.0f64, 0.0f64);
        for (index, &sample) in samples.iter().enumerate() {
            let phase = omega * index as f64;
            real += sample * phase.cos();
            imaginary -= sample * phase.sin();
        }
        real.hypot(imaginary)
    }

    /// Every frequency, against the direct sum, at the shape a tempogram uses.
    ///
    /// Frequencies that are not bins and a count that is not a multiple of the
    /// block, so the padding is exercised rather than avoided.
    #[test]
    fn every_frequency_matches_the_direct_sum() {
        let rate = 86.1328125_f64;
        let samples: Vec<f64> = (0..1034)
            .map(|i| {
                let beat = (TAU * (128.0 / 60.0) * i as f64 / rate).sin();
                let window = 0.5 - 0.5 * (TAU * i as f64 / 1034.0).cos();
                (beat + 0.3 * (i as f64 * 0.37).sin()) * window
            })
            .collect();
        let omegas: Vec<f64> = (0..1601)
            .map(|i| TAU * ((60.0 + i as f64 * 0.1) / 60.0) / rate)
            .collect();

        let bank = Bank::new(&omegas);
        assert_eq!(bank.len(), omegas.len());
        let mut out = vec![0.0; omegas.len()];
        bank.magnitudes(&samples, &mut out);

        let mut worst = 0.0f64;
        let mut at = 0;
        for (index, (&got, &omega)) in out.iter().zip(&omegas).enumerate() {
            let want = direct(&samples, omega);
            let relative = (got - want).abs() / want.abs().max(1e-12);
            if relative > worst {
                worst = relative;
                at = index;
            }
        }
        assert!(
            worst < 1e-8,
            "worst relative difference {worst:.3e} at frequency {at} of {}",
            omegas.len()
        );
    }

    /// A pure tone puts its energy where it is and nowhere else.
    #[test]
    fn a_tone_peaks_at_its_own_frequency() {
        let samples: Vec<f64> = (0..512).map(|i| (TAU * 0.1 * i as f64).sin()).collect();
        let omegas: Vec<f64> = (0..200).map(|i| TAU * (i as f64 * 0.001)).collect();

        let mut out = vec![0.0; omegas.len()];
        Bank::new(&omegas).magnitudes(&samples, &mut out);

        let peak = out
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(index, _)| index)
            .expect("a peak");
        assert_eq!(
            peak, 100,
            "the peak landed at {peak}, not at 0.1 cycles/sample"
        );
    }

    /// Accumulating over windows is adding, not replacing.
    #[test]
    fn adding_accumulates_across_windows() {
        let samples: Vec<f64> = (0..256).map(|i| (TAU * 0.05 * i as f64).sin()).collect();
        let omegas: Vec<f64> = (0..40).map(|i| TAU * (i as f64 * 0.005)).collect();
        let bank = Bank::new(&omegas);

        let mut once = vec![0.0; omegas.len()];
        bank.magnitudes(&samples, &mut once);

        let mut twice = vec![0.0; omegas.len()];
        bank.add_magnitudes(&samples, &mut twice);
        bank.add_magnitudes(&samples, &mut twice);

        for (index, (&one, &two)) in once.iter().zip(&twice).enumerate() {
            assert!(
                (two - 2.0 * one).abs() < 1e-12,
                "frequency {index}: {two} is not twice {one}"
            );
        }
    }
}
