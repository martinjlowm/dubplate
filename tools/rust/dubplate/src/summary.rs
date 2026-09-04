//! The terminal summary: the answer, the reasons to doubt it, and where to look.

use diagnostics::Severity;
use report::{AnalysisReport, format_bpm};
use std::io::Write;
use std::path::Path;

/// Print the summary.
///
/// `to` is stderr when the report JSON is going to stdout, so a caller piping
/// the JSON into `jq` still sees the findings instead of a parse error.
pub fn print(report: &AnalysisReport, out: &Path, no_figures: bool, to: &mut dyn Write) {
    let tempo = &report.tempo;
    let analysed = report.source.analysed_seconds.round() as i64;
    let _ = writeln!(
        to,
        "{} BPM   {}   {}:{:02} analysed",
        format_bpm(tempo.bpm),
        report.key.camelot,
        analysed / 60,
        analysed % 60
    );
    // The measurement, whenever the snap moved the answer. Printed on the
    // headline rather than left in the JSON: an answer of 138 that came from
    // 137.62 is a different claim from one that came from 137.99, and the
    // reader deciding whether to trust the grid needs the difference here.
    // Half of the smallest move the line can show. Below that the snap closed
    // a gap the print would round to 0.00, and a line saying so reads as a bug.
    if (tempo.bpm - tempo.bpm_measured).abs() >= 0.005 {
        let _ = writeln!(
            to,
            "  measured  {:.2} BPM, snapped {:+.2} BPM to a whole number",
            tempo.bpm_measured,
            tempo.bpm - tempo.bpm_measured
        );
    }
    let _ = writeln!(
        to,
        "  grid      {:.0}% of beats within {:.0} ms, pulse {:.2}x the mean, bar phase {} (contrast {:.2})",
        tempo.grid.matched_fraction * 100.0,
        tempo.grid.tolerance_ms,
        tempo.grid.pulse_ratio,
        tempo.bar.phase + 1,
        tempo.bar.contrast
    );
    let _ = writeln!(
        to,
        "  windows   median {:.2} BPM, spread {:.2} BPM, {:.0}% agree",
        tempo.stability.median_bpm,
        tempo.stability.interquartile_range_bpm,
        tempo.stability.agreeing_fraction * 100.0
    );

    // What a rerun at each metrical level would measure, which is the
    // comparison that settles an octave. The reported level is left out: its
    // grid is the line above.
    let levels: Vec<String> = tempo
        .octave_relatives
        .iter()
        .filter(|o| o.label != "candidate" && o.pulse_ratio > 0.0)
        .map(|o| {
            format!(
                "{} {:.0}%/{:.1}x",
                format_bpm(o.bpm),
                o.matched_fraction * 100.0,
                o.pulse_ratio
            )
        })
        .collect();
    if !levels.is_empty() {
        let _ = writeln!(to, "  levels    {}", levels.join(", "));
    }

    // The reported tempo is one of the candidates and is already on the first
    // line; the point of this line is what else the curve offered.
    let runners: Vec<String> = tempo
        .candidates
        .iter()
        .filter(|c| (c.bpm - tempo.bpm).abs() > 1.0)
        .take(3)
        .map(|c| format!("{:.2} ({:.2})", c.bpm, c.salience))
        .collect();
    if !runners.is_empty() {
        let _ = writeln!(to, "  also      {}", runners.join(", "));
    }

    for finding in report.diagnostics() {
        let mark = match finding.severity {
            Severity::Warning => "!",
            Severity::Info => "-",
        };
        let _ = writeln!(to, "  {mark} {}: {}", finding.code, finding.message);
    }

    if no_figures {
        let _ = writeln!(to, "  wrote     {}", out.join("report.json").display());
    } else {
        let _ = writeln!(to, "  wrote     {}", out.join("report.html").display());
    }
}
