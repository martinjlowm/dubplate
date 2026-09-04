//! The per-track analysis files, read back by the reference parser.

use binrw::BinRead;
use collection::{Beat, Cue, CueKind, Format, Track};
use rekordcrate::anlz::ANLZ;
use std::io::Cursor;
use std::path::PathBuf;
use waveform::Waveform;

fn track() -> Track {
    let bpm = 138.0;
    let beats = (0..1024)
        .map(|index| Beat {
            time_seconds: 0.25 + index as f64 * 60.0 / bpm,
            bpm,
            number_in_bar: (index % 4) as u8 + 1,
        })
        .collect();
    Track {
        source: PathBuf::from("/tmp/track.flac"),
        device_path: "/Contents/138_03A_Artist-Title.flac".to_string(),
        file_name: "138_03A_Artist-Title.flac".to_string(),
        title: "Title".to_string(),
        artist: "Artist".to_string(),
        album: None,
        genre: None,
        comment: String::new(),
        bpm,
        key_name: "F minor".to_string(),
        key_camelot: "4A".to_string(),
        duration_seconds: 445.0,
        sample_rate: 44100,
        bit_depth: 16,
        bitrate_kbps: 1411,
        file_size: 78_000_000,
        format: Format::Flac,
        beats,
        cues: vec![Cue {
            kind: CueKind::Memory,
            time_seconds: 0.25,
            number: 1,
            comment: String::new(),
        }],
        preview: Waveform {
            columns_per_second: 0.9,
            columns: (0..400).map(|i| (i % 32) as u8).collect(),
        },
        detail: Waveform {
            columns_per_second: 150.0,
            columns: (0..66_750).map(|i| (i % 32) as u8).collect(),
        },
    }
}

fn sections(bytes: &[u8]) -> Vec<String> {
    let parsed = ANLZ::read(&mut Cursor::new(bytes)).expect("the analysis file must parse");
    parsed
        .sections
        .iter()
        .map(|section| format!("{:?}", section.header.kind))
        .collect()
}

#[test]
fn the_dat_file_carries_the_grid_the_cues_and_the_previews() {
    let bytes = rekordbox::anlz::dat(&track());
    let kinds = sections(&bytes);
    assert_eq!(
        kinds,
        vec![
            "Path",
            "BeatGrid",
            "CueList",
            "CueList",
            "WaveformPreview",
            "TinyWaveformPreview"
        ]
    );

    let parsed = ANLZ::read(&mut Cursor::new(&bytes[..])).unwrap();
    let printed = format!("{parsed:?}");
    assert!(
        printed.contains("/Contents/138_03A_Artist-Title.flac"),
        "the path the database points at has to be in the file too"
    );
    // 138 BPM in centi-BPM, and a first beat a quarter of a second in.
    assert!(printed.contains("tempo: 13800"), "beats carry the tempo");
    assert!(printed.contains("time: 250"), "the first beat is at 250 ms");
}

#[test]
fn the_ext_file_carries_the_scrolling_waveform() {
    let bytes = rekordbox::anlz::ext(&track());
    assert_eq!(sections(&bytes), vec!["Path", "WaveformDetail"]);
}

#[test]
fn a_track_with_no_beats_still_produces_readable_files() {
    // A file the tempo stage could not measure still has to export, or one bad
    // track takes the whole device with it.
    let mut silent = track();
    silent.beats.clear();
    silent.cues.clear();
    assert!(!sections(&rekordbox::anlz::dat(&silent)).is_empty());
    assert!(!sections(&rekordbox::anlz::ext(&silent)).is_empty());
}
