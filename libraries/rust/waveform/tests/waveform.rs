//! What a waveform has to get right: loud reads tall, and the colour follows
//! the part of the spectrum the sound is actually in.

use audio::synth;
use waveform::{COLOUR_PREVIEW_COLUMNS, DETAIL_COLUMNS_PER_SECOND};

const RATE: u32 = 44100;

#[test]
fn a_preview_has_the_column_count_the_colour_format_reads() {
    let signal = synth::sine(220.0, 10.0, RATE);
    let preview = waveform::preview(&signal.samples, RATE);
    assert_eq!(preview.len(), COLOUR_PREVIEW_COLUMNS);
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
fn quiet_reads_short_and_loud_reads_tall() {
    let mut signal = synth::sine(220.0, 4.0, RATE);
    let half = signal.samples.len() / 2;
    for sample in &mut signal.samples[..half] {
        *sample *= 0.01;
    }
    let rendered = waveform::render(&signal.samples, RATE, 100);
    assert!(rendered.height(10) < 3, "the quiet half should be short");
    assert!(rendered.height(90) > 25, "the loud half should be tall");
}

#[test]
fn colour_follows_the_band_the_sound_is_in() {
    for (hertz, name) in [(60.0, "low"), (800.0, "mid"), (8000.0, "high")] {
        let signal = synth::sine(hertz, 4.0, RATE);
        let column = waveform::render(&signal.samples, RATE, 20).column(10);
        let strongest = match name {
            "low" => (column.low, column.mid.max(column.high)),
            "mid" => (column.mid, column.low.max(column.high)),
            _ => (column.high, column.low.max(column.mid)),
        };
        assert!(
            strongest.0 > strongest.1,
            "{hertz} Hz should be strongest in the {name} band, got low {} mid {} high {}",
            column.low,
            column.mid,
            column.high
        );
    }
}

#[test]
fn a_monochrome_shade_still_separates_bass_from_treble() {
    let bass = waveform::render(&synth::sine(60.0, 4.0, RATE).samples, RATE, 20);
    let treble = waveform::render(&synth::sine(8000.0, 4.0, RATE).samples, RATE, 20);
    assert!(
        bass.column(10).shade() < treble.column(10).shade(),
        "60 Hz shaded {} against 8 kHz shaded {}",
        bass.column(10).shade(),
        treble.column(10).shade()
    );
}

#[test]
fn resampling_keeps_the_peaks() {
    let mut signal = synth::sine(440.0, 4.0, RATE);
    // One loud column in an otherwise quiet track.
    let spike = signal.samples.len() / 2;
    for sample in &mut signal.samples {
        *sample *= 0.05;
    }
    for sample in &mut signal.samples[spike..spike + 2000] {
        *sample = 1.0;
    }
    let rendered = waveform::render(&signal.samples, RATE, 1200);
    let reduced = waveform::resample(&rendered, 100);
    assert_eq!(reduced.len(), 100);
    assert!(
        reduced.iter().map(|c| c.height).max().unwrap_or(0) > 25,
        "the spike survived the reduction"
    );
}

/// The column a sample lands in comes out of a product that leaves 32 bits.
///
/// usize is 32 bits in a browser and 64 on the machines these tests run on, so
/// this passed everywhere while the analysis died on every real track it was
/// given in a tab. Ninety seconds against the colour preview's 1200 columns puts
/// the last sample's product at 4.8 billion, and the arithmetic has to survive
/// it here for the browser to survive it there.
#[test]
fn a_track_too_long_for_a_32_bit_product_still_fills_its_last_column() {
    let seconds = 90.0;
    let samples = (seconds * f64::from(RATE)) as u64;
    assert!(
        samples * COLOUR_PREVIEW_COLUMNS as u64 > u64::from(u32::MAX),
        "this test is only meaningful while the product passes 32 bits",
    );

    let signal = synth::sine(220.0, seconds, RATE);
    let preview = waveform::preview(&signal.samples, RATE);

    assert_eq!(preview.len(), COLOUR_PREVIEW_COLUMNS);
    assert!(
        preview.height(COLOUR_PREVIEW_COLUMNS - 1) > 25,
        "the last column was never written, so the index wrapped",
    );
}
