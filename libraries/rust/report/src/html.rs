//! One page per run, linking every figure to the numbers it was drawn from.
//!
//! Static HTML with no script and no external asset: the output directory is
//! meant to be copied into a ticket or a chat thread and still open years later.

use crate::AnalysisReport;
use crate::plot::escape;
use diagnostics::Severity;
use std::fmt::Write as _;

/// A figure on the page.
pub struct Figure {
    pub caption: String,
    /// The file written next to the page, which is what a reader opens on its
    /// own or attaches to a ticket.
    pub file: String,
    /// What to look at, and what it would mean if it looked wrong.
    pub note: String,
    /// Markup embedded in the page instead of being linked.
    ///
    /// An SVG loaded through an img element may not fetch anything, so a figure
    /// that references a PNG beside it renders as empty axes unless its markup
    /// sits in the page itself. The file is still written, for reading alone.
    pub inline: Option<String>,
}

pub fn page(report: &AnalysisReport, figures: &[Figure]) -> String {
    let mut html = String::new();
    let _ = write!(
        html,
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
:root {{ color-scheme: light; }}
body {{ margin: 0 auto; padding: 32px 24px 64px; max-width: 1024px; background: #f8fafc; color: #0f172a;
        font-family: ui-sans-serif, system-ui, -apple-system, sans-serif; line-height: 1.55; }}
h1 {{ font-size: 22px; margin: 0 0 4px; }}
h2 {{ font-size: 16px; margin: 40px 0 12px; padding-bottom: 6px; border-bottom: 1px solid #e2e8f0; }}
.source {{ color: #475569; font-size: 13px; margin: 0 0 24px; word-break: break-all; }}
.headline {{ display: flex; gap: 32px; flex-wrap: wrap; background: #fff; border: 1px solid #e2e8f0;
             border-radius: 8px; padding: 16px 20px; }}
.headline div span {{ display: block; font-size: 12px; color: #64748b; text-transform: uppercase; letter-spacing: .04em; }}
.headline div strong {{ font-size: 24px; font-weight: 600; }}
table {{ border-collapse: collapse; width: 100%; font-size: 13px; background: #fff; }}
th, td {{ text-align: right; padding: 6px 10px; border-bottom: 1px solid #e2e8f0; }}
th:first-child, td:first-child {{ text-align: left; }}
th {{ color: #475569; font-weight: 600; }}
figure {{ margin: 24px 0; }}
figure img, figure svg {{ display: block; width: 100%; height: auto; border: 1px solid #e2e8f0; border-radius: 6px; background: #fff; }}
figcaption {{ font-size: 13px; color: #475569; margin-top: 8px; }}
.finding {{ border-left: 3px solid #cbd5e1; padding: 8px 14px; margin: 10px 0; background: #fff; font-size: 14px; }}
.finding.warning {{ border-left-color: #c2410c; }}
.finding code {{ font-size: 12px; color: #64748b; }}
footer {{ margin-top: 48px; font-size: 12px; color: #64748b; }}
</style>
</head>
<body>
<h1>{tempo:.2} BPM &middot; {key} ({camelot})</h1>
<p class="source">{path}</p>
<div class="headline">
  <div><span>tempo</span><strong>{tempo:.2}</strong></div>
  <div><span>key</span><strong>{key}</strong></div>
  <div><span>camelot</span><strong>{camelot}</strong></div>
  <div><span>grid fit</span><strong>{matched:.0}%</strong></div>
  <div><span>analysed</span><strong>{analysed}</strong></div>
</div>
"#,
        title = escape(&file_name(&report.source.path)),
        path = escape(&report.source.path),
        tempo = report.tempo.bpm,
        key = escape(&report.key.name),
        camelot = escape(&report.key.camelot),
        matched = report.tempo.grid.matched_fraction * 100.0,
        analysed = clock(report.source.analysed_seconds),
    );

    let findings = report.diagnostics();
    let _ = write!(html, "<h2>Findings</h2>");
    if findings.is_empty() {
        let _ = write!(
            html,
            r#"<div class="finding">Nothing disagreed: the estimators, the per-window estimates and the grid fit all point the same way.</div>"#
        );
    }
    for finding in findings {
        let class = match finding.severity {
            Severity::Warning => "finding warning",
            Severity::Info => "finding",
        };
        let _ = write!(
            html,
            r#"<div class="{class}">{}<br><code>{}</code></div>"#,
            escape(&finding.message),
            escape(finding.code)
        );
    }

    let _ = write!(html, "<h2>Tempo candidates</h2>");
    table(
        &mut html,
        &[
            "BPM",
            "salience",
            "after prior",
            "autocorrelation",
            "Fourier",
        ],
        report.tempo.candidates.iter().map(|c| {
            vec![
                format!("{:.2}", c.bpm),
                format!("{:.3}", c.salience),
                format!("{:.3}", c.weighted_salience),
                format!("{:.3}", c.autocorrelation),
                format!("{:.3}", c.fourier_salience),
            ]
        }),
    );

    let _ = write!(html, "<h2>Octave relatives</h2>");
    table(
        &mut html,
        &["relation", "BPM", "salience", "Fourier"],
        report.tempo.octave_relatives.iter().map(|o| {
            vec![
                o.label.to_string(),
                format!("{:.2}", o.bpm),
                format!("{:.3}", o.salience),
                format!("{:.3}", o.fourier_salience),
            ]
        }),
    );

    let _ = write!(html, "<h2>Tempo per frequency band</h2>");
    table(
        &mut html,
        &["band (Hz)", "BPM", "salience"],
        report.bands.iter().map(|b| {
            vec![
                format!("{:.0} to {:.0}", b.low_hz, b.high_hz),
                format!("{:.2}", b.bpm),
                format!("{:.3}", b.salience),
            ]
        }),
    );

    let _ = write!(html, "<h2>Key ranking</h2>");
    table(
        &mut html,
        &["key", "Camelot", "correlation"],
        report.key.ranked.iter().take(6).map(|k| {
            vec![
                k.name.clone(),
                k.camelot.clone(),
                format!("{:.3}", k.correlation),
            ]
        }),
    );

    let _ = write!(html, "<h2>Spectrum</h2>");
    table(
        &mut html,
        &["measure", "value"],
        [
            ("peak", format!("{:.0} Hz", report.spectrum.peak_hz)),
            ("centroid", format!("{:.0} Hz", report.spectrum.centroid_hz)),
            (
                "95% rolloff",
                format!("{:.0} Hz", report.spectrum.rolloff_95_hz),
            ),
            (
                "upper cutoff",
                format!("{:.0} Hz", report.spectrum.high_cutoff_hz),
            ),
            (
                "below 200 Hz",
                format!("{:.0}%", report.spectrum.low_share * 100.0),
            ),
            (
                "200 Hz to 4 kHz",
                format!("{:.0}%", report.spectrum.mid_share * 100.0),
            ),
            (
                "above 4 kHz",
                format!("{:.0}%", report.spectrum.high_share * 100.0),
            ),
        ]
        .into_iter()
        .map(|(name, value)| vec![name.to_string(), value]),
    );

    let _ = write!(html, "<h2>Figures</h2>");
    for figure in figures {
        let body = match &figure.inline {
            Some(markup) => markup.clone(),
            None => format!(
                r#"<img src="{}" alt="{}">"#,
                escape(&figure.file),
                escape(&figure.caption)
            ),
        };
        let _ = write!(
            html,
            r#"<figure>{body}<figcaption><strong>{}</strong> {}</figcaption></figure>"#,
            escape(&figure.caption),
            escape(&figure.note)
        );
    }

    let _ = write!(
        html,
        r#"<footer>{tool} {version} &middot; {rate} Hz, {channels} channel(s) &middot; window {window}, hop {hop} ({frame_rate:.1} frames/s) &middot; key profile {profile:?} &middot; every figure above is derived from report.json in this directory.</footer>
</body>
</html>
"#,
        tool = report.tool,
        version = report.version,
        rate = report.source.sample_rate,
        channels = report.source.channels,
        window = report.settings.window_size,
        hop = report.settings.hop,
        frame_rate = report.settings.frame_rate,
        profile = report.key.profile,
    );
    html
}

fn table(html: &mut String, headers: &[&str], rows: impl Iterator<Item = Vec<String>>) {
    let _ = write!(html, "<table><thead><tr>");
    for header in headers {
        let _ = write!(html, "<th>{}</th>", escape(header));
    }
    let _ = write!(html, "</tr></thead><tbody>");
    for row in rows {
        let _ = write!(html, "<tr>");
        for cell in row {
            let _ = write!(html, "<td>{}</td>", escape(&cell));
        }
        let _ = write!(html, "</tr>");
    }
    let _ = write!(html, "</tbody></table>");
}

fn file_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

fn clock(seconds: f64) -> String {
    let total = seconds.round() as i64;
    format!("{}:{:02}", total / 60, total % 60)
}
