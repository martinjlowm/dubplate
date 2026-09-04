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
