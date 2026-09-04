//! End to end, through the binary the user runs.

use std::process::Command;

#[test]
fn selftest_recovers_a_generated_tempo() {
    let output = Command::new(env!("CARGO_BIN_EXE_dubplate"))
        .args(["selftest", "--bpm", "174", "--seconds", "45"])
        .output()
        .expect("running the binary");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "selftest failed:\n{text}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(text.contains("measured 17"), "unexpected output: {text}");
}

#[test]
fn a_short_file_is_refused_rather_than_guessed_at() {
    let output = Command::new(env!("CARGO_BIN_EXE_dubplate"))
        .args(["selftest", "--bpm", "128", "--seconds", "10"])
        .output()
        .expect("running the binary");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("30 seconds"),
        "expected the duration floor to be named"
    );
}

/// A pulse train on disk, which is the only way to reach `analyze` from here.
fn write_click_train(path: &std::path::Path, bpm: f64, seconds: f64) {
    let generated = audio::synth::click_train(bpm, seconds, 44100);
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: generated.sample_rate,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("writing the fixture");
    for sample in &generated.samples {
        writer.write_sample(*sample).expect("writing a sample");
    }
    writer.finalize().expect("closing the fixture");
}

#[test]
fn an_excerpt_still_draws_its_beat_grid() {
    let dir = std::env::temp_dir().join("dubplate-excerpt-figure");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("making the working directory");
    let wav = dir.join("click.wav");
    write_click_train(&wav, 128.0, 90.0);
    let out = dir.join("report");

    let output = Command::new(env!("CARGO_BIN_EXE_dubplate"))
        .args([
            "analyze",
            wav.to_str().unwrap(),
            "--start",
            "45",
            "--duration",
            "40",
            "-o",
            out.to_str().unwrap(),
        ])
        .output()
        .expect("running the binary");
    assert!(
        output.status.success(),
        "analysing an excerpt failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Every beat of the grid is one dashed line. The figure is titled after the
    // tempo it drew, so drawing none of them is the tool asserting a grid it
    // does not show. A twelve-second window at 128 BPM holds twenty-five beats.
    let svg = std::fs::read_to_string(out.join("novelty.svg")).expect("reading the figure");
    let lines = svg.matches("stroke-dasharray").count();
    assert!(
        lines > 10,
        "the novelty figure of an excerpt drew {lines} beat-grid lines"
    );

    std::fs::remove_dir_all(&dir).expect("cleaning up");
}
