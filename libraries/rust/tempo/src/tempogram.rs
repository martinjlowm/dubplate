//! Turning a novelty curve into ranked tempo candidates, and keeping the
//! evidence for every one of them.
//!
//! Two independent estimators run over the same curve. Autocorrelation asks
//! "how self-similar is this at a lag of one beat"; the Fourier tempogram asks
//! "how much energy sits at a beat frequency". They fail differently. The first
//! is confused by a shuffled or swung pulse, the second by tempo drift, so an
//! estimate the two agree on is worth more than either score alone, and a
//! disagreement names the thing to look at next.

use crate::novelty::Novelty;
use serde::Serialize;

/// A bell curve over tempo that multiplies the salience of every candidate,
/// pulling the ranking toward `centre_bpm`.
///
/// Nobody supplies one. The energy measured in `libraries/rust/energy` puts a
/// track in a band and the band names the centre, which is why this has no
/// default and no flag behind it.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct TempoWeighting {
    pub centre_bpm: f64,
    /// Standard deviation in octaves. Wide, because a narrow curve is exactly
    /// how a 174 BPM track gets reported as 87.
    pub width_octaves: f64,
}

impl TempoWeighting {
    pub fn weight(&self, bpm: f64) -> f64 {
        let octaves = (bpm / self.centre_bpm).log2() / self.width_octaves;
        (-0.5 * octaves * octaves).exp()
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct TempoSettings {
    pub min_bpm: f64,
    pub max_bpm: f64,
    pub resolution_bpm: f64,
    /// Number of comb teeth. Four covers a bar of 4/4, which is where the
    /// periodicity of most dance music actually lives.
    pub pulses: usize,
    /// Weight of the between-teeth penalty, in [0, 1].
    ///
    /// The penalty subtracts the autocorrelation halfway between two teeth,
    /// which is what argues against a candidate at half the true tempo: every
    /// gap in its comb sits on a real beat. It argues just as hard against the
    /// true tempo of anything with eighth-note movement, whose offbeats sit in
    /// the same gaps, so it is weighted rather than absolute.
    pub penalty: f64,
    /// Off unless the energy band names a centre. Pulling the ranking toward one
    /// improves the average case and is precisely what makes the hard cases fail
    /// silently, so a run that uses it reports `candidates_unweighted` and
    /// `bpm_unweighted` next to the answer.
    pub weighting: Option<TempoWeighting>,
    /// Metrical level the answer is reported at. See [`MetricalFloor`].
    pub floor: Option<MetricalFloor>,
    /// Largest gap, in BPM, the reported tempo may be moved by to land on a
    /// whole number.
    ///
    /// Produced music is written at an integer tempo, so a measurement of
    /// 137.99 is a measurement of 138 and the decimals are this tool's error
    /// rather than the track's. Closing that gap is worth doing because the
    /// grid, the file name and both device databases all take the reported
    /// number.
    ///
    /// The default is deliberately narrower than half a BPM. A track measured
    /// 0.4 BPM off an integer is a track that was played rather than rendered,
    /// and rounding it would write a grid that drifts a beat every two minutes.
    /// Those keep their measurement and raise `non-integer-tempo`. At 0 nothing
    /// is snapped, and `bpm_measured` carries the measurement either way.
    pub integer_snap_bpm: f64,
}

/// The slowest metrical level the reported tempo is allowed to sit at.
///
/// A harmonic sum scores a candidate at half the true tempo almost as highly as
/// the truth, since every one of its comb teeth still lands on a beat, and wins
/// whenever the track puts any weight on alternate beats. Doubling until the
/// answer clears the floor fixes that, and the report records the doubling with
/// both saliences rather than presenting the result as what the curve said.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct MetricalFloor {
    pub bpm: f64,
    /// How strong the doubled candidate must be relative to the original, as a
    /// fraction. At 1.0 the doubling only happens when it wins outright, which
    /// defeats the point; the default is well below that, because the truth
    /// scoring slightly under its own subharmonic is the failure being
    /// corrected.
    pub min_salience_ratio: f64,
}

/// A doubling that was applied, and the evidence for it.
#[derive(Clone, Debug, Serialize)]
pub struct OctaveShift {
    pub from_bpm: f64,
    pub to_bpm: f64,
    pub from_salience: f64,
    pub to_salience: f64,
}

impl Default for TempoSettings {
    fn default() -> Self {
        TempoSettings {
            // Wide enough for half-time at 70 and for hardcore at 200, so an
            // octave error stays visible inside the range instead of being
            // clipped out of it.
            min_bpm: 60.0,
            max_bpm: 220.0,
            resolution_bpm: 0.1,
            pulses: 4,
            // Off by default: a plain harmonic sum reports the metrical level
            // the track is most self-similar at, and the penalty argues against
            // the true level of anything with offbeat movement.
            penalty: 0.0,
            weighting: None,
            floor: Some(MetricalFloor {
                // Nothing in club music is counted below this; a track that
                // measures at 70 is being counted in half bars.
                bpm: 90.0,
                min_salience_ratio: 0.5,
            }),
            // Wider than the 0.1 BPM candidate grid and than the 0.065 BPM the
            // selftest misses a generated pulse train by at 200 BPM, so it
            // closes this tool's error. Narrow enough that a track genuinely
            // between two integers keeps its measurement.
            integer_snap_bpm: 0.25,
        }
    }
}

impl TempoSettings {
    pub fn grid(&self) -> Vec<f64> {
        let steps = ((self.max_bpm - self.min_bpm) / self.resolution_bpm).round() as usize;
        (0..=steps)
            .map(|i| self.min_bpm + i as f64 * self.resolution_bpm)
            .collect()
    }
}

/// Salience against tempo, on a shared BPM grid.
#[derive(Clone, Debug, Serialize)]
pub struct TempoCurve {
    pub bpm: Vec<f64>,
    pub salience: Vec<f64>,
}

impl TempoCurve {
    pub fn salience_at(&self, bpm: f64) -> f64 {
        if self.bpm.len() < 2 {
            return 0.0;
        }
        let step = self.bpm[1] - self.bpm[0];
        let index = (bpm - self.bpm[0]) / step;
        interpolate(&self.salience, index)
    }

    fn normalised(mut self) -> Self {
        let peak = self.salience.iter().cloned().fold(f64::MIN, f64::max);
        if peak > 0.0 {
            for v in &mut self.salience {
                *v /= peak;
            }
        }
        self
    }
}

/// One candidate tempo and everything that argues for or against it.
#[derive(Clone, Debug, Serialize)]
pub struct TempoCandidate {
    pub bpm: f64,
    /// Comb salience, the primary ranking score.
    pub salience: f64,
    /// Salience after the tempo weighting. Equal to `salience` when the energy
    /// band named no centre.
    pub weighted_salience: f64,
    /// Plain autocorrelation at this candidate's lag: periodicity with no comb
    /// and no penalty, so a candidate that scores well only through its
    /// harmonics is visible as a low number here.
    pub autocorrelation: f64,
    /// What the Fourier tempogram gives this tempo, normalised to its own peak.
    pub fourier_salience: f64,
}

/// One metrical level an octave error would land on, and what forcing it would
/// produce.
///
/// The saliences rank the level inside this run. The two grid numbers are what
/// a rerun at that tempo would print, so the octave comparison the
/// troubleshooting guide describes is on the page already.
#[derive(Clone, Debug, Serialize)]
pub struct OctaveRelative {
    pub label: &'static str,
    pub bpm: f64,
    pub salience: f64,
    pub fourier_salience: f64,
    /// Fraction of this level's beats with a novelty peak inside the tolerance.
    pub matched_fraction: f64,
    /// Mean novelty on this level's grid over the track mean.
    ///
    /// Reads higher at half the tempo whatever the truth is, because every beat
    /// of a half grid is a beat of the real one and half as many beats have to
    /// find an onset. Compare halves on `fourier_salience` and the window
    /// spread instead.
    pub pulse_ratio: f64,
}

/// Comb-filtered autocorrelation salience across the BPM grid.
///
/// Each tooth adds the autocorrelation at a whole multiple of the beat lag and
/// subtracts the value halfway between two teeth. The subtraction is what makes
/// the octave decision explicit rather than accidental: a track playing at half
/// the candidate tempo has a peak sitting in every gap, and a plain harmonic sum
/// scores it identically to the truth.
pub fn comb_salience(novelty: &Novelty, settings: &TempoSettings) -> TempoCurve {
    let grid = settings.grid();
    let longest_lag = 60.0 * novelty.frame_rate / settings.min_bpm;
    let max_lag = (longest_lag * settings.pulses as f64).ceil() as usize;
    let acf = novelty.autocorrelation(max_lag.min(novelty.values.len().saturating_sub(1)));

    let salience = grid
        .iter()
        .map(|&bpm| {
            let lag = 60.0 * novelty.frame_rate / bpm;
            let mut score = 0.0;
            let mut weight_total = 0.0;
            for k in 1..=settings.pulses {
                // 1/k: agreement at the beat itself is the claim being tested;
                // agreement four beats out is corroboration, not evidence of
                // equal weight.
                let weight = 1.0 / k as f64;
                let peak = interpolate(&acf, k as f64 * lag);
                let trough = interpolate(&acf, (k as f64 - 0.5) * lag);
                score += weight * (peak - settings.penalty * trough);
                weight_total += weight;
            }
            score / weight_total
        })
        .collect();

    TempoCurve {
        bpm: grid,
        salience,
    }
}

/// Windowed Fourier tempogram, averaged over the track.
///
/// Windowed rather than one transform over the whole curve: a set recorded from
/// vinyl, or a live edit, drifts by a beat over several minutes, and a global
/// transform smears that into a low plateau with no peak to pick.
///
/// The candidates are 0.1 BPM apart and the bins of a twelve-second window are
/// five BPM apart, so there is no FFT to read this off: every candidate is a
/// frequency between bins. `goertzel` evaluates them directly without the sine
/// and cosine per sample per candidate that doing so used to cost.
pub fn fourier_salience(
    novelty: &Novelty,
    settings: &TempoSettings,
    window_seconds: f64,
    hop_seconds: f64,
) -> TempoCurve {
    let grid = settings.grid();
    let window_frames = (window_seconds * novelty.frame_rate).round() as usize;
    let hop_frames = ((hop_seconds * novelty.frame_rate).round() as usize).max(1);
    let mut salience = vec![0.0f64; grid.len()];

    if novelty.values.len() < window_frames || window_frames == 0 {
        return TempoCurve {
            bpm: grid,
            salience,
        };
    }

    let window: Vec<f64> = (0..window_frames)
        .map(|i| {
            let phase = std::f64::consts::TAU * i as f64 / window_frames as f64;
            0.5 - 0.5 * phase.cos()
        })
        .collect();

    // One resonator per candidate, built once for the whole track. A BPM is a
    // frequency in beats per second, and the curve is sampled once per frame.
    let bank = goertzel::Bank::new(
        &grid
            .iter()
            .map(|bpm| std::f64::consts::TAU * (bpm / 60.0) / novelty.frame_rate)
            .collect::<Vec<f64>>(),
    );

    // The windowed segment, written once per window rather than once per
    // candidate. It used to be recomputed inside the frequency loop, which for
    // a four-minute track meant multiplying the same samples out sixteen
    // hundred times over.
    let mut windowed = vec![0.0f64; window_frames];

    let mut windows = 0.0;
    let mut start = 0;
    while start + window_frames <= novelty.values.len() {
        let segment = &novelty.values[start..start + window_frames];
        for ((slot, &value), &weight) in windowed.iter_mut().zip(segment).zip(&window) {
            *slot = value as f64 * weight;
        }
        bank.add_magnitudes(&windowed, &mut salience);
        windows += 1.0;
        start += hop_frames;
    }

    for v in &mut salience {
        *v /= windows;
    }
    TempoCurve {
        bpm: grid,
        salience,
    }
    .normalised()
}

/// The best `count` tempi, ranked by salience after the tempo weighting.
///
/// Candidates must be local maxima and are refined by fitting a parabola to the
/// three grid points around each: the grid is 0.1 BPM, and a 0.05 BPM error over
/// a seven-minute track is a beat and a half of drift by the end.
pub fn candidates(
    curve: &TempoCurve,
    fourier: &TempoCurve,
    novelty: &Novelty,
    settings: &TempoSettings,
    count: usize,
) -> Vec<TempoCandidate> {
    let mut peaks: Vec<TempoCandidate> = Vec::new();
    for i in 1..curve.salience.len().saturating_sub(1) {
        let (previous, here, next) = (
            curve.salience[i - 1],
            curve.salience[i],
            curve.salience[i + 1],
        );
        if here <= previous || here < next || here <= 0.0 {
            continue;
        }
        let denominator = previous - 2.0 * here + next;
        let offset = if denominator.abs() > f64::EPSILON {
            (0.5 * (previous - next) / denominator).clamp(-0.5, 0.5)
        } else {
            0.0
        };
        let bpm = curve.bpm[i] + offset * settings.resolution_bpm;
        let lag = 60.0 * novelty.frame_rate / bpm;
        let acf = novelty.autocorrelation((lag.ceil() as usize).max(1));
        let weight = settings.weighting.map(|p| p.weight(bpm)).unwrap_or(1.0);
        peaks.push(TempoCandidate {
            bpm,
            salience: here,
            weighted_salience: here * weight,
            autocorrelation: *acf.last().unwrap_or(&0.0),
            fourier_salience: fourier.salience_at(bpm),
        });
    }

    peaks.sort_by(|a, b| b.weighted_salience.total_cmp(&a.weighted_salience));
    // Two grid peaks 0.3 BPM apart are one tempo seen through a slightly ragged
    // curve, not two answers.
    let mut kept: Vec<TempoCandidate> = Vec::new();
    for peak in peaks {
        if kept.iter().any(|k| (k.bpm - peak.bpm).abs() < 1.0) {
            continue;
        }
        kept.push(peak);
        if kept.len() == count {
            break;
        }
    }
    kept
}

/// Salience at the tempi that an octave or triplet error lands on.
pub fn octave_relatives(
    curve: &TempoCurve,
    fourier: &TempoCurve,
    novelty: &Novelty,
    bpm: f64,
    tolerance_ms: f64,
) -> Vec<OctaveRelative> {
    [
        ("half", 0.5),
        ("two-thirds", 2.0 / 3.0),
        ("candidate", 1.0),
        ("three-halves", 1.5),
        ("double", 2.0),
    ]
    .into_iter()
    .map(|(label, ratio)| {
        let related = bpm * ratio;
        // A grid per level, which is the comparison that settles an octave.
        // Salience ranks candidates inside one run and says nothing across
        // levels; a grid fit is measured against the curve and does. Fitting
        // one costs a phase search over the novelty curve, so five of them are
        // cheaper than the one transform pass that produced the curve.
        let grid = crate::beats::align(novelty, related, tolerance_ms);
        OctaveRelative {
            label,
            bpm: related,
            salience: curve.salience_at(related),
            fourier_salience: fourier.salience_at(related),
            matched_fraction: grid.matched_fraction,
            pulse_ratio: grid.pulse_ratio,
        }
    })
    .collect()
}

/// One estimate per window, so a tempo that changes is visible as a change.
#[derive(Clone, Debug, Serialize)]
pub struct TempoWindow {
    pub start_seconds: f64,
    pub bpm: f64,
    pub salience: f64,
}

pub fn tempo_over_time(
    novelty: &Novelty,
    settings: &TempoSettings,
    window_seconds: f64,
    hop_seconds: f64,
) -> Vec<TempoWindow> {
    let window_frames = (window_seconds * novelty.frame_rate).round() as usize;
    let hop_frames = ((hop_seconds * novelty.frame_rate).round() as usize).max(1);
    let mut out = Vec::new();
    let mut start = 0;

    while start + window_frames <= novelty.values.len() {
        let segment = Novelty {
            frame_rate: novelty.frame_rate,
            values: novelty.values[start..start + window_frames].to_vec(),
        };
        let curve = comb_salience(&segment, settings);
        let best = curve
            .salience
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1));
        if let Some((index, &salience)) = best {
            // The same metrical floor as the whole-track answer, or the trace
            // reads as a permanent disagreement with it: a harmonic sum picks
            // the half-tempo peak in every window just as it does over the
            // whole track.
            let (bpm, _) = apply_floor(&curve, curve.bpm[index], settings.floor);
            out.push(TempoWindow {
                start_seconds: start as f64 / novelty.frame_rate,
                bpm,
                salience,
            });
        }
        start += hop_frames;
    }
    out
}

/// Quadratic interpolation at a fractional index, zero outside the slice.
///
/// A beat lag is almost never a whole number of frames: at 44.1 kHz with a 512
/// sample hop, 174 BPM is 29.7 frames. Interpolating the autocorrelation
/// linearly across such a peak pulls the estimate towards the nearer whole
/// frame. At club tempo that bias is a fifth of a BPM, which is a beat of drift
/// over a five-minute track.
fn interpolate(values: &[f64], index: f64) -> f64 {
    if index < 0.0 || values.is_empty() {
        return 0.0;
    }
    let nearest = index.round();
    if nearest < 1.0 || nearest as usize + 1 >= values.len() {
        // No room for three points: fall back to linear, which at the ends of
        // the range is all the data supports anyway.
        let lower = index.floor() as usize;
        let Some(&a) = values.get(lower) else {
            return 0.0;
        };
        let b = values.get(lower + 1).copied().unwrap_or(a);
        return a + (b - a) * (index - lower as f64);
    }
    let centre = nearest as usize;
    let (before, here, after) = (values[centre - 1], values[centre], values[centre + 1]);
    let t = index - nearest;
    here + 0.5 * t * (after - before) + 0.5 * t * t * (after - 2.0 * here + before)
}

/// Raise a candidate to the metrical level the report is stated at.
///
/// Returns the reported tempo and, when the level changed, what it changed from.
pub fn apply_floor(
    curve: &TempoCurve,
    bpm: f64,
    floor: Option<MetricalFloor>,
) -> (f64, Option<OctaveShift>) {
    let Some(floor) = floor else {
        return (bpm, None);
    };
    let mut current = bpm;
    let mut shift = None;
    // Two doublings at most: from 60 BPM that reaches 240, past anything the
    // search range admits.
    for _ in 0..2 {
        if current >= floor.bpm {
            break;
        }
        let doubled = refine_peak(curve, current * 2.0);
        let (here, there) = (curve.salience_at(current), curve.salience_at(doubled));
        if there < floor.min_salience_ratio * here {
            break;
        }
        shift = Some(OctaveShift {
            from_bpm: shift.map(|s: OctaveShift| s.from_bpm).unwrap_or(current),
            to_bpm: doubled,
            from_salience: here,
            to_salience: there,
        });
        current = doubled;
    }
    (current, shift)
}

/// The whole number `bpm` should be reported as, when one sits within
/// `tolerance` of it.
///
/// Returns `None` when nothing is close enough, which leaves the measurement as
/// the answer and lets `non-integer-tempo` say why.
pub fn snap_to_integer(bpm: f64, tolerance: f64) -> Option<f64> {
    if !bpm.is_finite() || tolerance <= 0.0 {
        return None;
    }
    let whole = bpm.round();
    ((bpm - whole).abs() <= tolerance).then_some(whole)
}

/// The local maximum of the salience curve nearest `bpm`, refined between grid
/// points. Doubling a candidate lands close to the peak, not on it, and the
/// difference is a tenth of a BPM the answer is stated to.
fn refine_peak(curve: &TempoCurve, bpm: f64) -> f64 {
    if curve.bpm.len() < 3 {
        return bpm;
    }
    let step = curve.bpm[1] - curve.bpm[0];
    let centre = ((bpm - curve.bpm[0]) / step).round() as isize;
    // Search 1% either side: wide enough to cover the grid rounding, narrow
    // enough that it cannot wander to a different candidate.
    let reach = ((bpm * 0.01 / step).round() as isize).max(1);
    let mut best = centre;
    for index in (centre - reach)..=(centre + reach) {
        if index <= 0 || index as usize + 1 >= curve.salience.len() {
            continue;
        }
        if curve.salience[index as usize] > curve.salience[best.max(0) as usize] {
            best = index;
        }
    }
    let i = best.clamp(1, curve.salience.len() as isize - 2) as usize;
    let (previous, here, next) = (
        curve.salience[i - 1],
        curve.salience[i],
        curve.salience[i + 1],
    );
    let denominator = previous - 2.0 * here + next;
    let offset = if denominator.abs() > f64::EPSILON {
        (0.5 * (previous - next) / denominator).clamp(-0.5, 0.5)
    } else {
        0.0
    };
    curve.bpm[i] + offset * step
}
