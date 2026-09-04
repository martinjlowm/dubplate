//! The whole pass, over a signal whose tempo is exact.
//!
//! One test per thing the crate promises: the report, the artefacts, and the
//! refusal. Everything here goes through the same entry points the CLI and the
//! browser build call, so a change that breaks either breaks this.

use pipeline::{AnalysisOptions, Source};

fn source(seconds: f64) -> Source {
    Source {
        path: "generated".into(),
        duration_seconds: seconds,
        start_seconds: 0.0,
    }
}

#[test]
fn a_generated_tempo_survives_the_whole_pass() {
    let audio = audio::synth::click_train(128.0, 60.0, 44100);
    let outcome = pipeline::run(&audio, &AnalysisOptions::default(), source(60.0))
        .expect("60 seconds is enough to analyse");

    let report = &outcome.report;
    assert_eq!(report.tempo.bpm, 128.0, "reported {}", report.tempo.bpm);
    assert!(
        (report.tempo.bpm_measured - 128.0).abs() < 0.25,
        "measured {:.4} BPM",
        report.tempo.bpm_measured
    );
    assert_eq!(report.settings.window_size, 2048);
    assert_eq!(report.bands.len(), 8, "one tempo per onset band");
    assert!(!report.waveforms.detail.is_empty());
    // The JSON is the contract, so it has to serialise.
    let json = report.to_json().expect("the report serialises");
    assert!(json.contains("\"bpm_measured\""), "{}", &json[..200]);
}

#[test]
fn the_figures_come_back_as_bytes_under_the_names_the_page_references() {
    let audio = audio::synth::click_train(128.0, 45.0, 44100);
    let outcome = pipeline::run(&audio, &AnalysisOptions::default(), source(45.0))
        .expect("45 seconds is enough to analyse");
    let artefacts = pipeline::figures(&outcome, 10.0, 6.0);

    let text: Vec<&str> = artefacts
        .text
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(
        text,
        [
            "spectrogram.svg",
            "spectrum.svg",
            "novelty.svg",
            "tempo-salience.svg",
            "tempo-over-time.svg",
            "chroma.svg",
            "key-correlations.svg",
            "report.html",
        ],
        "the page is written last, after everything it references"
    );
    let binary: Vec<&str> = artefacts
        .binary
        .iter()
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(binary, ["spectrogram.png"]);

    // Every SVG is an SVG, the page references every figure it was given, and
    // the PNG is a PNG.
    for (name, text) in &artefacts.text {
        if name.ends_with(".svg") {
            assert!(text.starts_with("<svg"), "{name} is not an SVG");
            assert!(
                artefacts
                    .text
                    .iter()
                    .any(|(page, body)| page == "report.html" && body.contains(name.as_str()))
                    || name == "spectrogram.svg",
                "{name} is drawn but the page never mentions it"
            );
        }
    }
    let png = &artefacts.binary[0].1;
    assert_eq!(&png[1..4], b"PNG", "the spectrogram is not a PNG");
}

#[test]
fn a_track_too_short_to_measure_twice_is_refused() {
    let audio = audio::synth::click_train(128.0, 10.0, 44100);
    let Err(error) = pipeline::run(&audio, &AnalysisOptions::default(), source(10.0)) else {
        panic!("ten seconds is not enough to analyse and should have been refused");
    };
    assert!(
        error.to_string().contains("30 seconds"),
        "the refusal has to name the bound: {error}"
    );
}
