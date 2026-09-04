//! The two things a waveform has to get right: loud reads tall, and bright
//! reads bright.

use audio::synth;
use waveform::{DETAIL_COLUMNS_PER_SECOND, PREVIEW_COLUMNS};

const RATE: u32 = 44100;

#[test]
fn a_preview_has_the_column_count_the_format_reads() {
    let signal = synth::sine(220.0, 10.0, RATE);
    let preview = waveform::preview(&signal.samples, RATE);
    assert_eq!(preview.len(), PREVIEW_COLUMNS);
    assert!(preview.height(0) > 25, "a full-scale sine should be tall");
}

#[test]
fn a_detail_waveform_is_read_back_at_exactly_150_columns_per_second() {
    let signal = synth::sine(220.0, 10.0, RATE);
    let detail = waveform::detail(&signal.samples, RATE);
    assert_eq!(detail.columns_per_second, DETAIL_COLUMNS_PER_SECOND);
    assert_eq!(detail.len(), 1500);
}

#[test]
fn silence_is_flat_and_a_fade_climbs() {
    let mut signal = synth::sine(220.0, 4.0, RATE);
    // Quiet for the first half, full scale for the second.
    let half = signal.samples.len() / 2;
    for sample in &mut signal.samples[..half] {
        *sample *= 0.01;
    }
    let rendered = waveform::render(&signal.samples, RATE, 100);
    assert!(rendered.height(10) < 3, "quiet half should be short");
    assert!(rendered.height(90) > 25, "loud half should be tall");
}

#[test]
fn shade_separates_bass_from_treble() {
    let bass = synth::sine(60.0, 4.0, RATE);
    let treble = synth::sine(6000.0, 4.0, RATE);
    let dark = waveform::render(&bass.samples, RATE, 20);
    let bright = waveform::render(&treble.samples, RATE, 20);
    assert!(
        dark.shade(10) < bright.shade(10),
        "60 Hz shaded {} against 6 kHz shaded {}",
        dark.shade(10),
        bright.shade(10)
    );
}
