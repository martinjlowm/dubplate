//! Rules that name why a tempo estimate should not be trusted.
//!
//! Each rule reads evidence the tempo stage already produced and states the
//! disagreement in the terms that caused it. No rule changes the answer: a
//! detector that quietly corrects itself is the thing this tool exists to
//! troubleshoot.

use crate::{TempoAnalysis, format_bpm};
pub use diagnostics::{Diagnostic, Severity};

/// Relative salience above which a competing tempo counts as a real rival
/// rather than a sidelobe of the winner.
const RIVAL_THRESHOLD: f64 = 0.85;

pub fn inspect(analysis: &TempoAnalysis) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    let bpm = analysis.bpm;
    // Every message that quotes the answer quotes it the way the headline does.
    let reported = format_bpm(bpm);

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
                    "{:.2} BPM ({}) scores {:.0}% of {} BPM, so the octave is decided by a margin too small to defend",
                    relative.bpm,
                    relative.label,
                    ratio * 100.0,
                    reported
                ),
            ));
        }
    }

    // A grid at another level fits the curve better than the one reported.
    //
    // Halves and doubles are excluded, and not because they cannot be wrong.
    // A half grid reads higher on both measures whatever the truth is, since
    // every one of its beats is a beat of the real grid and half as many beats
    // have to find an onset, so including them would fire this on every track
    // and say nothing. Their level is the metrical floor's decision, and the
    // Fourier salience is what argues about it. A two-thirds or three-halves
    // grid has beats that land on nothing when it is wrong, so a better fit
    // there is evidence.
    if let Some(answer) = analysis
        .octave_relatives
        .iter()
        .find(|r| r.label == "candidate")
    {
        for relative in &analysis.octave_relatives {
            if matches!(relative.label, "candidate" | "half" | "double") {
                continue;
            }
            let fits_better = relative.matched_fraction > answer.matched_fraction
                && relative.pulse_ratio > answer.pulse_ratio * 1.05;
            if fits_better {
                out.push(Diagnostic::warning(
                    "level-fits-better",
                    format!(
                        "a grid at {:.2} BPM ({}) fits this novelty curve better than the answer does, {:.0}% of beats matched at {:.2}x the track novelty against {:.0}% at {:.2}x, so the reported level is the salience ranking's and not the grid's",
                        relative.bpm,
                        relative.label,
                        relative.matched_fraction * 100.0,
                        relative.pulse_ratio,
                        answer.matched_fraction * 100.0,
                        answer.pulse_ratio
                    ),
                ));
            }
        }
    }

    // The snap was asked for and refused. Rounding here would have moved the
    // grid by more than the tolerance allows, which is a beat of drift every
    // few minutes on a file a player reads the grid straight out of.
    let snap = analysis.settings.integer_snap_bpm;
    let off_integer = (analysis.bpm_measured - analysis.bpm_measured.round()).abs();
    if snap > 0.0 && off_integer > snap {
        out.push(Diagnostic::warning(
            "non-integer-tempo",
            format!(
                "{:.2} BPM sits {:.2} BPM from the nearest whole number, further than the {snap:.2} BPM the snap allows, so the measurement is the answer: produced music is written on integers, so this was played rather than rendered, or the grid drifts across the file",
                analysis.bpm_measured, off_integer
            ),
        ));
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
                    "the Fourier tempogram peaks at {fourier_bpm:.2} BPM against {reported} from autocorrelation; both read the same curve, so one of them is being misled by the shape of the pulse"
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

    // The band between a grid that describes nothing and one that describes the
    // track. `grid-misfit` speaks for the first; this speaks for an answer that
    // stands on a grid fitting loosely. Across the working set a level that is
    // right puts 90% or more of its beats on an onset, and one reaching three
    // quarters is thin evidence whatever its salience says. Music played to no
    // click is what produced it here, which is also where the octave is hardest
    // to read.
    if (0.5..0.8).contains(&analysis.grid.matched_fraction) {
        out.push(Diagnostic::warning(
            "loose-grid",
            format!(
                "{:.0}% of beats land within {:.0} ms of an onset, where a programmed track reaches 90%; the level rests on a grid that fits loosely, so read the octave relatives before trusting it",
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
                "without the tempo prior the winner is {:.2} BPM, not {}: the answer is the prior's, not the track's",
                first.bpm, reported
            ),
        ));
    }

    let settings = &analysis.settings;
    if bpm - settings.min_bpm < 2.0 || settings.max_bpm - bpm < 2.0 {
        out.push(Diagnostic::warning(
            "range-edge",
            format!(
                "{reported} BPM sits at the edge of the {:.0}-{:.0} BPM search range, so a stronger candidate may lie outside it",
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
