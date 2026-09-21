//! Decoding and excerpting, checked against a file this test writes.

use audio::Audio;
use std::path::PathBuf;

/// A stereo file whose two channels differ, so a broken downmix shows up as the
/// wrong amplitude rather than as the right one by luck.
fn write_stereo_wav(path: &PathBuf) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 8000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for _ in 0..8000 {
        writer.write_sample(16384i16).unwrap();
        writer.write_sample(-16384i16).unwrap();
    }
    writer.finalize().unwrap();
}

#[test]
fn downmixes_and_excerpts() {
    let path = std::env::temp_dir().join("dubplate-decode-test.wav");
    write_stereo_wav(&path);

    let decoded = Audio::from_wav(&path).unwrap();
    assert_eq!(decoded.sample_rate, 8000);
    assert_eq!(decoded.source_channels, 2);
    assert_eq!(decoded.samples.len(), 8000);
    // The two channels cancel exactly.
    assert!(decoded.samples.iter().all(|s| s.abs() < 1e-6));
    assert!((decoded.duration_seconds() - 1.0).abs() < 1e-9);

    let excerpt = decoded.excerpt(0.25, Some(0.5)).unwrap();
    assert_eq!(excerpt.samples.len(), 4000);
    assert!(decoded.excerpt(2.0, None).is_err());

    std::fs::remove_file(path).unwrap();
}

/// A stereo file whose channels differ, opening with `silent_seconds` of
/// digital black. The channels differ so a trim that dropped one, or swapped
/// them, shows up as the wrong sample rather than as the right one by luck.
fn write_padded_stereo_wav(path: &PathBuf, silent_seconds: f64) {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 8000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).unwrap();
    for _ in 0..(silent_seconds * 8000.0) as usize {
        writer.write_sample(0i16).unwrap();
        writer.write_sample(0i16).unwrap();
    }
    for _ in 0..8000 {
        writer.write_sample(20000i16).unwrap();
        writer.write_sample(-10000i16).unwrap();
    }
    writer.finalize().unwrap();
}

#[test]
fn the_lead_in_is_measured_and_cut_without_touching_the_samples_that_survive() {
    let path = std::env::temp_dir().join("dubplate-trim-test.wav");
    write_padded_stereo_wav(&path, 0.5);

    let decoded = Audio::from_wav(&path).unwrap();
    let lead_in = decoded.lead_in_seconds();
    assert!(
        (lead_in - 0.5).abs() < 0.02,
        "half a second of black at the head measured as {lead_in}s"
    );

    let whole = std::fs::read(&path).unwrap();
    let cut = audio::trim_wav(&whole, lead_in).unwrap();

    // The container survives: same rate, same two channels, same bit depth.
    let cut_path = std::env::temp_dir().join("dubplate-trim-test-cut.wav");
    std::fs::write(&cut_path, &cut).unwrap();
    let reader = hound::WavReader::open(&cut_path).unwrap();
    let spec = reader.spec();
    assert_eq!(spec.channels, 2, "a trim must not downmix");
    assert_eq!(spec.sample_rate, 8000);
    assert_eq!(spec.bits_per_sample, 16);
    assert_eq!(
        reader.duration(),
        8000,
        "one second of tone should survive the cut"
    );

    // The samples that survive are the ones that were there, both channels.
    let samples: Vec<i16> = hound::WavReader::open(&cut_path)
        .unwrap()
        .into_samples::<i16>()
        .map(|s| s.unwrap())
        .collect();
    assert_eq!(
        (samples[0], samples[1]),
        (20000, -10000),
        "the first frame after the cut is the first frame of the tone"
    );

    // A track that opens loud is left alone.
    let unpadded = std::env::temp_dir().join("dubplate-trim-test-loud.wav");
    write_padded_stereo_wav(&unpadded, 0.0);
    assert_eq!(
        Audio::from_wav(&unpadded).unwrap().lead_in_seconds(),
        0.0,
        "a track with no silent head has no lead-in"
    );

    for path in [path, cut_path, unpadded] {
        std::fs::remove_file(path).unwrap();
    }
}
