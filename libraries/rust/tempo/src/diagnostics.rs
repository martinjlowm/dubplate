//! Rules that name why a tempo estimate should not be trusted.
//!
//! Each rule reads evidence the tempo stage already produced and states the
//! disagreement in the terms that caused it. No rule changes the answer: a
//! detector that quietly corrects itself is the thing this tool exists to
//! troubleshoot.

use crate::TempoAnalysis;
pub use diagnostics::{Diagnostic, Severity};

/// Relative salience above which a competing tempo counts as a real rival
/// rather than a sidelobe of the winner.
const RIVAL_THRESHOLD: f64 = 0.85;

pub fn inspect(analysis: &TempoAnalysis) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let bpm = analysis.bpm;

    let Some(strongest) = analysis.candidates.first() else {
        out.push(Diagnostic::warning(
            "no-candidate",
            "the salience curve has no peak: the novelty curve carries no periodicity in the searched range",
        ));
        return out;
    };

    for relative in &analysis.octave_relatives {
        if relative.label == "candidate" {
            continue;
        }
        // A relative the metrical floor already ruled out is not a competing
        // answer. Without this every track raises the half-tempo warning, since
        // a harmonic sum scores half as highly as the truth by construction.
        if analysis
            .settings
            .floor
            .is_some_and(|floor| relative.bpm < floor.bpm)
        {
            continue;
        }
        let ratio = relative.salience / analysis.salience.max(f64::MIN_POSITIVE);
        if ratio >= RIVAL_THRESHOLD {
            out.push(Diagnostic::warning(
                "octave-ambiguity",
                format!(
                    "{:.2} BPM ({}) scores {:.0}% of {:.2} BPM, so the octave is decided by a margin too small to defend",
                    relative.bpm,
                    relative.label,
                    ratio * 100.0,
                    bpm
                ),
            ));
        }
    }

    if let Some(shift) = &analysis.octave_shift {
        out.push(Diagnostic::info(
            "metrical-floor-applied",
            format!(
                "the strongest candidate was {:.2} BPM (salience {:.3}); the answer was doubled to {:.2} BPM (salience {:.3}) to clear the metrical floor",
                shift.from_bpm, shift.from_salience, shift.to_bpm, shift.to_salience
            ),
        ));
    }

    if let Some(runner_up) = analysis.candidates.get(1) {
        // The reported tempo is itself a candidate, and after the metrical
        // floor doubles the answer it is usually the runner-up rather than the
        // winner.
        let related = (runner_up.bpm - bpm).abs() < 1.0
            || [0.5, 2.0 / 3.0, 1.5, 2.0]
                .iter()
                .any(|r| (runner_up.bpm - bpm * r).abs() < 1.0);
        let ratio =
            runner_up.weighted_salience / strongest.weighted_salience.max(f64::MIN_POSITIVE);
        if !related && ratio >= RIVAL_THRESHOLD {
            out.push(Diagnostic::warning(
                "close-runner-up",
                format!(
                    "{:.2} BPM scores {:.0}% of the winner and is not an octave relative, which usually means two sections at different tempi",
                    runner_up.bpm,
                    ratio * 100.0
                ),
            ));
        }
    }

    if let Some((index, _)) = analysis
        .fourier_curve
        .salience
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
    {
        let fourier_bpm = analysis.fourier_curve.bpm[index];
        if (fourier_bpm - bpm).abs() / bpm > 0.01 {
            out.push(Diagnostic::warning(
                "estimators-disagree",
                format!(
                    "the Fourier tempogram peaks at {fourier_bpm:.2} BPM against {bpm:.2} from autocorrelation; both read the same curve, so one of them is being misled by the shape of the pulse"
                ),
            ));
        }
    }

    if analysis.grid.pulse_ratio < 1.5 {
        out.push(Diagnostic::warning(
            "weak-pulse",
            format!(
                "the beat grid sits on novelty only {:.2}x the track mean: the onsets this tempo was derived from are barely there",
                analysis.grid.pulse_ratio
            ),
        ));
    }

    if analysis.grid.matched_fraction < 0.5 {
        out.push(Diagnostic::warning(
            "grid-misfit",
            format!(
                "only {:.0}% of beats land within {:.0} ms of an onset, so a constant tempo does not describe this track",
                analysis.grid.matched_fraction * 100.0,
                analysis.grid.tolerance_ms
            ),
        ));
    }

    if analysis.stability.interquartile_range_bpm > 2.0 {
        out.push(Diagnostic::warning(
            "unstable-tempo",
            format!(
                "window estimates spread over {:.1} BPM (median {:.2}); the track either changes tempo or has sections with no pulse to measure",
                analysis.stability.interquartile_range_bpm, analysis.stability.median_bpm
            ),
        ));
    }

    if analysis.stability.octave_split_fraction > 0.15 {
        out.push(Diagnostic::info(
            "octave-split-windows",
            format!(
                "{:.0}% of windows chose half or double the reported tempo, which is where a whole-track average hides a section at another feel",
                analysis.stability.octave_split_fraction * 100.0
            ),
        ));
    }

    if let Some(unweighted) = &analysis.candidates_without_prior
        && let Some(first) = unweighted.first()
        && (first.bpm - bpm).abs() > 1.0
    {
        out.push(Diagnostic::warning(
            "prior-changed-answer",
            format!(
                "without the tempo prior the winner is {:.2} BPM, not {:.2}: the answer is the prior's, not the track's",
                first.bpm, bpm
            ),
        ));
    }

    let settings = &analysis.settings;
    if bpm - settings.min_bpm < 2.0 || settings.max_bpm - bpm < 2.0 {
        out.push(Diagnostic::warning(
            "range-edge",
            format!(
                "{bpm:.2} BPM sits at the edge of the {:.0}-{:.0} BPM search range, so a stronger candidate may lie outside it",
                settings.min_bpm, settings.max_bpm
            ),
        ));
    }

    if analysis.bar.contrast < 1.1 {
        out.push(Diagnostic::info(
            "flat-bar-phase",
            format!(
                "every beat of the bar carries the same low-end weight (contrast {:.2}), so beat one is a guess",
                analysis.bar.contrast
            ),
        ));
    }

    out
}
