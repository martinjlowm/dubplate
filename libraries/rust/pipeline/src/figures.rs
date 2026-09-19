//! The figures, and the note under each one saying what it would look like if
//! the stage above it had gone wrong.
//!
//! Everything comes back as bytes and strings rather than as files. The CLI
//! writes them into a directory; a browser hands them to the page. Neither
//! choice belongs to the code that draws them.

use crate::Outcome;
use report::html::{Figure, page};
use report::{BarChart, LinePlot, Marker, Series};

/// What a run draws: the page, the figures it references, and the spectrogram
/// raster the spectrogram figure points at.
///
/// Names are the file names the HTML references, so a caller that writes them
/// into one directory gets a page that works, and a caller that serves them
/// from memory has the keys it needs.
#[derive(Default)]
pub struct Artefacts {
    /// SVG and HTML, in the order they were drawn. `report.html` is last.
    pub text: Vec<(String, String)>,
    /// The PNG the spectrogram figure references.
    pub binary: Vec<(String, Vec<u8>)>,
}

const NOVELTY_COLOUR: &str = "#0f766e";
const COMB_COLOUR: &str = "#2563eb";
const FOURIER_COLOUR: &str = "#c2410c";
// Dark enough to separate a fitted beat from the axis grid behind it, which is
// #e2e8f0. The whole argument for a tempo is whether these lines sit on the
// peaks, so they have to be the most readable thing in the figure.
const BEAT_COLOUR: &str = "#475569";

pub fn figures(outcome: &Outcome, plot_start: f64, plot_window: f64) -> Artefacts {
    let report = &outcome.report;
    let mut figures = Vec::new();

    let mut artefacts = Artefacts::default();

    // Absent when the run was asked for `Figures::Skipped`, which is the one
    // figure a caller can decline. Everything below is drawn from the report
    // and from curves the tempo estimate needed anyway, so those cost nothing
    // to have kept.
    if let Some(heatmap) = &outcome.spectrogram {
        artefacts.binary.push((
            "spectrogram.png".into(),
            // The encoder writes to a Vec, so this cannot fail for a reason a
            // caller could act on.
            heatmap
                .to_png_bytes()
                .expect("a PNG encoder writing to memory"),
        ));
        let spectrogram = heatmap.to_svg("spectrogram.png");
        artefacts
            .text
            .push(("spectrogram.svg".into(), spectrogram.clone()));
        figures.push(Figure {
            caption: "Spectrogram".into(),
            file: "spectrogram.svg".into(),
            note: "Time against frequency, on a logarithmic frequency axis. Horizontal bands are sustained tones, vertical stripes are onsets, and a flat ceiling across the top marks the cutoff of a lossy source.".into(),
            inline: Some(spectrogram),
        });
    }

    artefacts.text.push((
        "spectrum.svg".into(),
        LinePlot::new("Long-term average spectrum", "frequency (Hz)", "level (dB)")
            .log_x()
            .x_range(
                20.0,
                outcome
                    .average_spectrum
                    .last()
                    .map(|p| p.0)
                    .unwrap_or(20_000.0),
            )
            .series(Series::new(
                "average",
                COMB_COLOUR,
                outcome.average_spectrum.clone(),
            ))
            .to_svg(),
    ));
    figures.push(Figure {
        caption: "Long-term average spectrum".into(),
        file: "spectrum.svg".into(),
        note: "The whole track averaged into one spectrum. The cliff at the right is where the file stops carrying content, and a peak in the bass names the fundamental the kick sits on.".into(),
        inline: None,
    });

    let frame_rate = outcome.broadband.frame_rate;
    let offset = report.source.analysed_start_seconds;
    let first = ((plot_start - offset).max(0.0) * frame_rate) as usize;
    let last = (first + (plot_window * frame_rate) as usize).min(outcome.broadband.values.len());
    let novelty_points: Vec<(f64, f64)> = (first..last)
        .map(|i| {
            (
                offset + i as f64 / frame_rate,
                outcome.broadband.values[i] as f64,
            )
        })
        .collect();

    let mut novelty_plot = LinePlot::new(
        format!(
            "Onset novelty and the {} BPM grid",
            report::format_bpm(report.tempo.bpm)
        ),
        "time (s)",
        "novelty",
    )
    .series(Series::new("novelty", NOVELTY_COLOUR, novelty_points).filled());
    for beat in &report.tempo.grid.beats_seconds {
        let time = offset + beat;
        if time >= plot_start && time <= plot_start + plot_window {
            novelty_plot = novelty_plot.marker(Marker {
                x: time,
                label: None,
                colour: BEAT_COLOUR.into(),
                dashed: true,
            });
        }
    }
    artefacts
        .text
        .push(("novelty.svg".into(), novelty_plot.to_svg()));
    figures.push(Figure {
        caption: "Onset novelty against the beat grid".into(),
        file: "novelty.svg".into(),
        note: "The curve every tempo estimate is made from, with the chosen grid drawn over it. Lines that sit beside the peaks rather than on them mean the tempo is close but wrong; peaks with no line between them mean the grid is at half the tempo of the track.".into(),
        inline: None,
    });

    let comb = &report.tempo.comb_curve;
    let comb_peak = comb
        .salience
        .iter()
        .cloned()
        .fold(f64::MIN, f64::max)
        .max(f64::MIN_POSITIVE);
    let mut salience_plot = LinePlot::new("Tempo salience", "tempo (BPM)", "salience (peak = 1)")
        .series(Series::new(
            "autocorrelation comb",
            COMB_COLOUR,
            comb.bpm
                .iter()
                .zip(&comb.salience)
                .map(|(&bpm, &value)| (bpm, value / comb_peak))
                .collect(),
        ))
        .series(Series::new(
            "Fourier tempogram",
            FOURIER_COLOUR,
            report
                .tempo
                .fourier_curve
                .bpm
                .iter()
                .zip(&report.tempo.fourier_curve.salience)
                .map(|(&bpm, &value)| (bpm, value))
                .collect(),
        ));
    for candidate in &report.tempo.candidates {
        salience_plot = salience_plot.marker(Marker {
            x: candidate.bpm,
            label: Some(format!("{:.1}", candidate.bpm)),
            colour: "#334155".into(),
            dashed: true,
        });
    }
    artefacts
        .text
        .push(("tempo-salience.svg".into(), salience_plot.to_svg()));
    figures.push(Figure {
        caption: "Tempo salience".into(),
        file: "tempo-salience.svg".into(),
        note: "Both estimators over the same BPM axis, each scaled to its own peak. Peaks at half and double the winner are the octave decision; the two lines peaking in different places is the disagreement the findings report.".into(),
        inline: None,
    });

    if !report.tempo.over_time.is_empty() {
        artefacts.text.push((
            "tempo-over-time.svg".into(),
            LinePlot::new("Tempo per window", "time (s)", "tempo (BPM)")
                .series(Series::new(
                    "window estimate",
                    "#7c3aed",
                    report
                        .tempo
                        .over_time
                        .iter()
                        .map(|w| (offset + w.start_seconds, w.bpm))
                        .collect(),
                ))
                .marker(Marker {
                    x: offset,
                    label: Some(format!("reported {}", report::format_bpm(report.tempo.bpm))),
                    colour: "#334155".into(),
                    dashed: false,
                })
                .to_svg(),
        ));
        figures.push(Figure {
            caption: "Tempo per window".into(),
            file: "tempo-over-time.svg".into(),
            note: "Each point is an independent estimate over twenty seconds. A flat line is a track at one tempo; a line that steps between two values an octave apart is a whole-track average hiding two different feels.".into(),
            inline: None,
        });
    }

    const PITCH_CLASSES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    artefacts.text.push((
        "chroma.svg".into(),
        BarChart::new(
            format!(
                "Pitch-class energy for {} ({:+.0} cents from A = 440 Hz)",
                report.key.camelot, report.key.tuning_cents
            ),
            "share of energy",
            PITCH_CLASSES
                .iter()
                .zip(report.key.chroma)
                .map(|(name, value)| (name.to_string(), value))
                .collect(),
        )
        .highlight(report.key.key.tonic as usize)
        .to_svg(),
    ));
    figures.push(Figure {
        caption: "Pitch-class energy".into(),
        file: "chroma.svg".into(),
        note: "The twelve pitch classes, tonic highlighted. A profile with no shape belongs to a track with no tonal content, and the key ranking below it is then meaningless whatever its correlations say.".into(),
        inline: None,
    });

    artefacts.text.push((
        "key-correlations.svg".into(),
        BarChart::new(
            "Key correlations",
            "correlation",
            report
                .key
                .ranked
                .iter()
                .take(8)
                .map(|k| (k.camelot.clone(), k.correlation))
                .collect(),
        )
        .highlight(0)
        .to_svg(),
    ));
    figures.push(Figure {
        caption: "Key correlations".into(),
        file: "key-correlations.svg".into(),
        note: "The eight best-scoring keys, in Camelot notation. Two bars of nearly equal height are a tie the correlation cannot break, and the pair is usually a key and its relative.".into(),
        inline: None,
    });

    artefacts
        .text
        .push(("report.html".into(), page(report, &figures)));
    artefacts
}
