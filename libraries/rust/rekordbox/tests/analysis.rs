//! The per-track analysis files, read back by the reference parser.

use binrw::BinRead;
use collection::{Beat, Cue, CueKind, Format, Track};
use rekordcrate::anlz::ANLZ;
use std::io::Cursor;
use std::path::PathBuf;
use waveform::{Column, Waveform};

/// A waveform whose columns rise and fall, so a reader can tell one column from
/// the next.
fn flat_waveform(columns_per_second: f64, count: usize) -> Waveform {
    let columns: Vec<Column> = (0..count)
        .map(|index| Column {
            height: (index % 32) as u8,
            low: (index % 8) as u8,
            mid: ((index / 2) % 8) as u8,
            high: ((index / 4) % 8) as u8,
        })
        .collect();
    Waveform::from_columns(columns_per_second, &columns)
}

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
        preview: flat_waveform(0.9, 1200),
        detail: flat_waveform(150.0, 66_750),
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

/// The order is the one every DAT of the 135-track export carries, and the
/// hot cues come before the memory cues. A parser that reads section headers
/// does not care; a player that seeks to a fixed offset does.
#[test]
fn the_dat_file_carries_the_grid_the_cues_and_the_previews() {
    let bytes = rekordbox::anlz::dat(&track());
    let kinds = sections(&bytes);
    assert_eq!(
        kinds,
        vec![
            "Path",
            "VBR",
            "BeatGrid",
            "WaveformPreview",
            "TinyWaveformPreview",
            "CueList",
            "CueList"
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
fn the_ext_file_carries_the_scrolling_and_colour_waveforms() {
    let bytes = rekordbox::anlz::ext(&track());
    assert_eq!(
        sections(&bytes),
        vec![
            "Path",
            "WaveformDetail",
            "CueList",
            "CueList",
            "WaveformColorDetail",
            "WaveformColorPreview"
        ]
    );
}

#[test]
fn the_colour_detail_waveform_decodes_to_the_columns_it_was_given() {
    let track = track();
    let parsed = ANLZ::read(&mut Cursor::new(rekordbox::anlz::ext(&track))).unwrap();
    let printed = format!("{parsed:?}");

    // The first three columns of the fixture, as the writer packs them.
    let expected: Vec<String> = track
        .detail
        .columns()
        .take(3)
        .map(|column| {
            format!(
                "red: {}, green: {}, blue: {}, height: {}",
                column.high, column.mid, column.low, column.height
            )
        })
        .collect();
    for column in expected {
        assert!(
            printed.contains(&column),
            "the parser did not read back a column written as {column}"
        );
    }
}

#[test]
fn the_colour_preview_is_the_width_the_format_reads() {
    let bytes = rekordbox::anlz::ext(&track());
    let parsed = ANLZ::read(&mut Cursor::new(&bytes[..])).unwrap();
    let preview = parsed
        .sections
        .iter()
        .find(|section| format!("{:?}", section.header.kind) == "WaveformColorPreview")
        .expect("a colour preview section");
    // Six bytes per column over 1200 columns, plus the twelve of header the
    // section declares beyond the twelve every section has.
    assert_eq!(preview.header.total_size, 24 + 1200 * 6);
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
