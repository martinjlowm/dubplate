//! The terminal summary: the answer, the reasons to doubt it, and where to look.

use diagnostics::Severity;
use report::AnalysisReport;
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
        "{:.2} BPM   {}   {}:{:02} analysed",
        tempo.bpm,
        report.key.camelot,
        analysed / 60,
        analysed % 60
    );
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
