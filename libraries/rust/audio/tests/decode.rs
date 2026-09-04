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
