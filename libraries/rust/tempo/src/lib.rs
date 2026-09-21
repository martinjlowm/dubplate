//! Tempo estimation, from spectral flux to a beat grid, with the intermediate
//! evidence kept at every step.
//!
//! The pipeline is deliberately shallow and each stage is separately
//! inspectable: band energies → per-band novelty → tempo salience → candidates →
//! grid fit. A wrong answer is diagnosed by reading down that list until the
//! first stage that already looks wrong.

pub mod beats;
pub mod diagnostics;
pub mod novelty;
pub mod tempogram;

pub use beats::{BarPhase, BeatGrid};
pub use diagnostics::{Diagnostic, Severity};
pub use novelty::{FluxAccumulator, Novelty};
pub use tempogram::{
    MetricalFloor, OctaveRelative, OctaveShift, TempoCandidate, TempoCurve, TempoPrior,
    TempoSettings, TempoWindow,
};

use serde::Serialize;

/// How much the per-window estimates agree with each other.
#[derive(Clone, Debug, Serialize)]
pub struct TempoStability {
    pub median_bpm: f64,
    /// Interquartile range of the window estimates. A steady track is under
    /// 1 BPM; anything over a few BPM is either a tempo change or a novelty
    /// curve too weak to measure.
    pub interquartile_range_bpm: f64,
    /// Fraction of windows within 1% of the chosen tempo.
    pub agreeing_fraction: f64,
    /// Fraction of windows that landed on half or double the chosen tempo.
    pub octave_split_fraction: f64,
}

/// Everything the tempo stage knows, including the parts that disagree.
#[derive(Clone, Debug, Serialize)]
pub struct TempoAnalysis {
    pub settings: TempoSettings,
    /// The reported tempo. Equal to the strongest candidate unless the metrical
    /// floor raised it, which `octave_shift` then records, or the snap to a
    /// whole number moved it, which `bpm_measured` then shows.
    pub bpm: f64,
    /// The tempo as measured, before the snap to a whole number.
    ///
    /// The two differ by at most `settings.integer_snap_bpm`. This is the
    /// number to read when a grid drifts and the number the selftest checks,
    /// since a rounded answer would hide the error it exists to measure.
    pub bpm_measured: f64,
    /// Salience of the reported tempo, which is what every ratio in the
    /// diagnostics is measured against.
    pub salience: f64,
    pub octave_shift: Option<OctaveShift>,
    pub candidates: Vec<TempoCandidate>,
    /// The ranking the prior would have produced had it been off. Present only
    /// when a prior is set, and the reason a prior is never applied silently.
    pub candidates_without_prior: Option<Vec<TempoCandidate>>,
    /// The answer the track would have had with no prior, floor and snap
    /// applied, so it compares against [`TempoAnalysis::bpm`] directly.
    ///
    /// The first entry of `candidates_without_prior` is not that answer: it is
    /// the salience ranking before the metrical floor has had its say, and on a
    /// track the floor doubles the two differ by an octave while the reported
    /// tempo does not move at all.
    pub bpm_without_prior: Option<f64>,
    pub octave_relatives: Vec<OctaveRelative>,
    pub grid: BeatGrid,
    pub bar: BarPhase,
    pub over_time: Vec<TempoWindow>,
    pub stability: TempoStability,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip)]
    pub comb_curve: TempoCurve,
    #[serde(skip)]
    pub fourier_curve: TempoCurve,
}

/// A tempo as it is written for a reader: `138` when it is a whole number,
/// `137.62` when it is not.
///
/// One function, because the findings, the terminal summary, the page and the
/// figure titles all state the same answer, and two decimals on a whole number
/// claim a precision the measurement does not have.
pub fn format_bpm(bpm: f64) -> String {
    if bpm.is_finite() && (bpm - bpm.round()).abs() < 1e-9 {
        format!("{bpm:.0}")
    } else {
        format!("{bpm:.2}")
    }
}

/// Window geometry for the stability trace, in seconds.
///
/// Twenty seconds holds roughly forty beats at club tempo, which is enough for
/// an autocorrelation to resolve a tempo; ten-second hops keep a tempo change
/// from hiding between two windows.
const STABILITY_WINDOW_SECONDS: f64 = 20.0;
const STABILITY_HOP_SECONDS: f64 = 10.0;
/// Shorter for the Fourier tempogram: it resolves tempo by frequency, so a long
/// window buys resolution the BPM grid cannot use and loses the ability to
/// follow drift.
const FOURIER_WINDOW_SECONDS: f64 = 12.0;
const FOURIER_HOP_SECONDS: f64 = 6.0;
/// A beat placed within 50 ms of an onset is heard as on time; the figure is the
/// usual bound on where a listener stops noticing.
const GRID_TOLERANCE_MS: f64 = 50.0;

/// Run the whole tempo stage.
///
/// `broadband` drives the estimate and `low_band` decides only the bar phase.
pub fn analyze(broadband: &Novelty, low_band: &Novelty, settings: &TempoSettings) -> TempoAnalysis {
    let comb = tempogram::comb_salience(broadband, settings);
    let fourier = tempogram::fourier_salience(
        broadband,
        settings,
        FOURIER_WINDOW_SECONDS,
        FOURIER_HOP_SECONDS,
    );
    let candidates = tempogram::candidates(&comb, &fourier, broadband, settings, 5);

    let candidates_without_prior = settings.prior.map(|_| {
        let unweighted = TempoSettings {
            prior: None,
            ..settings.clone()
        };
        tempogram::candidates(&comb, &fourier, broadband, &unweighted, 5)
    });

    let strongest = candidates.first().map(|c| c.bpm).unwrap_or(f64::NAN);
    let (bpm_measured, octave_shift) = tempogram::apply_floor(&comb, strongest, settings.floor);

    // What the answer would have been without the prior, taken all the way
    // through the floor and the snap. Comparing anything earlier against the
    // reported tempo compares two different stages.
    let bpm_without_prior = candidates_without_prior.as_ref().and_then(|unweighted| {
        let first = unweighted.first()?.bpm;
        let (measured, _) = tempogram::apply_floor(&comb, first, settings.floor);
        Some(tempogram::snap_to_integer(measured, settings.integer_snap_bpm).unwrap_or(measured))
    });
    // Snapped here rather than where the number is printed. The grid, the
    // window comparison, the file name and both device databases all take the
    // reported tempo, and a grid fitted at 137.99 under an answer of 138 is a
    // grid a reader cannot check the answer against.
    let bpm =
        tempogram::snap_to_integer(bpm_measured, settings.integer_snap_bpm).unwrap_or(bpm_measured);
    let grid = beats::align(broadband, bpm, GRID_TOLERANCE_MS);
    let bar = beats::bar_phase(&grid, low_band, 4);
    let over_time = tempogram::tempo_over_time(
        broadband,
        settings,
        STABILITY_WINDOW_SECONDS,
        STABILITY_HOP_SECONDS,
    );
    let stability = stability(&over_time, bpm);
    let octave_relatives =
        tempogram::octave_relatives(&comb, &fourier, broadband, bpm, GRID_TOLERANCE_MS);

    let mut analysis = TempoAnalysis {
        settings: settings.clone(),
        bpm,
        bpm_measured,
        salience: comb.salience_at(bpm),
        octave_shift,
        candidates,
        candidates_without_prior,
        bpm_without_prior,
        octave_relatives,
        grid,
        bar,
        over_time,
        stability,
        diagnostics: Vec::new(),
        comb_curve: comb,
        fourier_curve: fourier,
    };
    analysis.diagnostics = diagnostics::inspect(&analysis);
    analysis
}

fn stability(windows: &[TempoWindow], bpm: f64) -> TempoStability {
    if windows.is_empty() {
        return TempoStability {
            median_bpm: f64::NAN,
            interquartile_range_bpm: f64::NAN,
            agreeing_fraction: 0.0,
            octave_split_fraction: 0.0,
        };
    }
    let mut sorted: Vec<f64> = windows.iter().map(|w| w.bpm).collect();
    sorted.sort_by(f64::total_cmp);
    let quantile = |q: f64| sorted[((sorted.len() - 1) as f64 * q).round() as usize];

    let within = |a: f64, b: f64| (a - b).abs() / b < 0.01;
    let agreeing = windows.iter().filter(|w| within(w.bpm, bpm)).count();
    let split = windows
        .iter()
        .filter(|w| within(w.bpm, bpm * 2.0) || within(w.bpm, bpm / 2.0))
        .count();

    TempoStability {
        median_bpm: quantile(0.5),
        interquartile_range_bpm: quantile(0.75) - quantile(0.25),
        agreeing_fraction: agreeing as f64 / windows.len() as f64,
        octave_split_fraction: split as f64 / windows.len() as f64,
    }
}

/// The strongest tempo in a single curve, used for the per-band comparison.
pub fn best_bpm(novelty: &Novelty, settings: &TempoSettings) -> Option<(f64, f64)> {
    let curve = tempogram::comb_salience(novelty, settings);
    curve
        .salience
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(i, &salience)| (curve.bpm[i], salience))
}
