//! The per-track analysis files, read back by the reference parser.

use binrw::BinRead;
use collection::{Beat, Cue, CueKind, Format, Section, SectionLabel, Track};
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
        trim_seconds: 0.0,
        seek_table: None,
        beats,
        cues: vec![Cue {
            kind: CueKind::Memory,
            time_seconds: 0.25,
            number: 1,
            comment: String::new(),
        }],
        sections: structure(bpm),
        preview: flat_waveform(0.9, 1200),
        detail: flat_waveform(150.0, 66_750),
    }
}

/// Four stretches on the bar lines of the fixture's grid, one of each label
/// that maps to a different phrase kind.
fn structure(bpm: f64) -> Vec<Section> {
    let beat = |number: usize| 0.25 + number as f64 * 60.0 / bpm;
    [
        (SectionLabel::Intro, 0, 64),
        (SectionLabel::Build, 64, 128),
        (SectionLabel::Drop, 128, 512),
        (SectionLabel::Outro, 512, 640),
    ]
    .into_iter()
    .map(|(label, start, end)| Section {
        label,
        start_seconds: beat(start),
        end_seconds: beat(end),
    })
    .collect()
}

/// The body of one section, past the fixed fields its header declares.
fn section_body<'a>(bytes: &'a [u8], kind: &[u8; 4], header_size: usize) -> &'a [u8] {
    let word = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let mut at = word(4);
    while at + 12 <= bytes.len() {
        let total = word(at + 8);
        if &bytes[at..at + 4] == kind {
            return &bytes[at + header_size..at + total];
        }
        if total == 0 {
            break;
        }
        at += total;
    }
    panic!("{} is missing", String::from_utf8_lossy(kind));
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

/// The order is the export's, and `PQT2` reads back as unknown because
/// rekordcrate has no layout for it.
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
            "ExtendedCueList",
            "ExtendedCueList",
            "Unknown([80, 81, 84, 50])",
            "WaveformColorDetail",
            "WaveformColorPreview",
            "SongStructure",
        ]
    );
}

/// The extended cue list carries the same cues as `PCOB`, with the comment.
///
/// rekordcrate parses these, which pins the entry lengths and the fields up to
/// the comment. It cannot say whether the five words past the hot cue colour
/// hold what rekordbox puts there: no populated `PCO2` has been read here.
#[test]
fn the_extended_cue_list_carries_the_cues_the_old_one_does() {
    let track = track();
    let parsed = ANLZ::read(&mut Cursor::new(rekordbox::anlz::ext(&track))).unwrap();
    let printed = format!("{parsed:?}");

    for cue in &track.cues {
        let time = (cue.time_seconds * 1000.0).round() as u32;
        assert!(
            printed.matches(&format!("time: {time}")).count() >= 2,
            "the cue at {time} ms is in both cue lists"
        );
    }
}

/// The phrases are the structure stage's stretches, masked the way rekordbox
/// masks them.
///
/// rekordcrate only reads a `PSSI` whose mask it can undo, so a parse that
/// returns the labels this tool wrote is the mask, the header and the phrase
/// layout all at once.
#[test]
fn the_song_structure_carries_the_sections_the_analysis_named() {
    let track = track();
    let parsed = ANLZ::read(&mut Cursor::new(rekordbox::anlz::ext(&track))).unwrap();
    let printed = format!("{parsed:?}");

    assert!(printed.contains("mood: High"), "the phrases are high-mood");
    assert!(
        printed.contains("bank: Default"),
        "nothing here picks a lighting bank"
    );
    // The fixture's stretches start at beats 0, 64, 128 and 512, and a phrase
    // counts beats from one.
    for (index, (beat, kind)) in [(1, 1), (65, 2), (129, 5), (513, 6)].iter().enumerate() {
        assert!(
            printed.contains(&format!("index: {}, beat: {beat}, kind: {kind}", index + 1)),
            "phrase {} starts at beat {beat} as kind {kind}",
            index + 1
        );
    }
    // The last stretch ends at beat 640, which is beat 641 counted from one.
    assert!(
        printed.contains("end_beat: 641"),
        "the last phrase ends where the last stretch does"
    );
}

/// The extended beat grid is the same beats to the microsecond.
///
/// rekordcrate reads `PQT2` as an unknown blob, so the check is the layout
/// measured off the export: a header naming the first and last beats and the
/// beat count, then one word per beat holding the microseconds past the
/// millisecond `PQTZ` rounds down to.
#[test]
fn the_extended_beat_grid_holds_the_microseconds_the_old_one_drops() {
    let track = track();
    let ext = rekordbox::anlz::ext(&track);
    let body = section_body(&ext, b"PQT2", 0x38);
    assert_eq!(
        body.len(),
        track.beats.len() * 2,
        "one word per beat and nothing else"
    );

    let dat = rekordbox::anlz::dat(&track);
    let grid = section_body(&dat, b"PQTZ", 0x18);
    for (index, beat) in track.beats.iter().enumerate().take(64) {
        let microseconds = (beat.time_seconds * 1_000_000.0).round() as u64;
        let millis = u32::from_be_bytes(grid[index * 8 + 4..index * 8 + 8].try_into().unwrap());
        let rest = u16::from_be_bytes(body[index * 2..index * 2 + 2].try_into().unwrap());
        assert_eq!(
            u64::from(millis) * 1000 + u64::from(rest),
            microseconds,
            "beat {index} at {} s",
            beat.time_seconds
        );
    }
}

/// The colour detail waveform is packed the way a rekordbox export packs it.
///
/// rekordcrate is not the oracle here, because it reads these two bytes the
/// other way up and this crate wrote what it read until an XDJ-RX3 drew no
/// scrolling waveform. The layout below was measured off the `PWV5` rekordbox
/// wrote for a track this tool also analysed: the height in bits six to two of
/// the second byte matches that file's `PWV3` height column for column at 0.97
/// over all 66 395 columns, the low band lands in the top three bits of the
/// first byte, and the bottom two bits of the second byte are zero everywhere.
#[test]
fn the_colour_detail_waveform_is_packed_from_the_high_bit_down() {
    let track = track();
    let bytes = rekordbox::anlz::ext(&track);
    let columns = section_body(&bytes, b"PWV5", 0x18);

    for (index, column) in track.detail.columns().take(64).enumerate() {
        let pair = [columns[index * 2], columns[index * 2 + 1]];
        let packed = u16::from_be_bytes(pair);
        let where_at = format!("column {index} of {column:?}");

        assert_eq!(
            packed >> 13,
            u16::from(column.low),
            "low band at {where_at}"
        );
        assert_eq!(
            (packed >> 10) & 0x07,
            u16::from(column.mid),
            "mid band at {where_at}"
        );
        assert_eq!(
            (packed >> 7) & 0x07,
            u16::from(column.high),
            "high band at {where_at}"
        );
        assert_eq!(
            (packed >> 2) & 0x1f,
            u16::from(column.height),
            "height at {where_at}"
        );
        assert_eq!(packed & 0x03, 0, "the sub-step stays zero at {where_at}");
    }
}

/// The colour preview carries a height a player can draw, not a whiteness.
///
/// The first byte of a `PWV4` column tracks the loudest detail column under it
/// at 0.97 in the rekordbox export measured against, and the last three carry
/// the band levels at 0.97, 0.83 and 0.84. This exporter wrote the smallest of
/// the three band energies in the first two bytes instead, which is zero
/// wherever one band is quiet, so a loud track arrived with a flat preview.
#[test]
fn the_colour_preview_carries_a_height_and_three_band_levels() {
    let track = track();
    let bytes = rekordbox::anlz::ext(&track);
    let columns = section_body(&bytes, b"PWV4", 0x18);

    let loud = columns
        .chunks(6)
        .find(|column| column[3].max(column[4]).max(column[5]) > 0)
        .expect("a column with energy in it");

    assert!(loud[0] > 0, "a column with energy in it has a height");
    assert_eq!(
        u16::from(loud[0]) + u16::from(loud[1]),
        255,
        "the second byte is the height taken from full scale"
    );
    assert_eq!(
        loud[2],
        loud[3].max(loud[4]).max(loud[5]),
        "the third byte is the loudest of the three bands"
    );
    assert!(
        columns
            .chunks(6)
            .all(|column| column[2..].iter().all(|level| *level <= 127)),
        "band levels run to 127, which is where rekordbox's stop"
    );
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

/// The 2EX file carries the three-band waveform, at the widths a player reads.
///
/// An XDJ-RX3 showed the preview waveform and no scrolling one from a stick
/// whose EXT held a populated PWV3, PWV5 and PWV4. A track copied onto the same
/// stick from a rekordbox export drew both, and the only file it had that this
/// exporter did not write was this one.
#[test]
fn the_2ex_file_carries_the_three_band_waveforms() {
    let track = track();
    let bytes = rekordbox::anlz::two_ex(&track);
    assert_eq!(
        sections(&bytes),
        vec![
            "Path",
            "Waveform3BandDetail",
            "Waveform3BandPreview",
            "Waveform3BandCalibration",
        ],
        "the 2EX is the path and the three-band pair, in the order rekordbox writes"
    );

    // Three bytes a column, at the same width as the detail waveform beside it,
    // and 1200 for the preview, which is what PWV4 carries too.
    let widths = |kind: &[u8; 4]| -> (u32, u32) {
        let mut at = u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize;
        while at + 24 <= bytes.len() {
            let total = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
            if &bytes[at..at + 4] == kind {
                return (
                    u32::from_be_bytes(bytes[at + 12..at + 16].try_into().unwrap()),
                    u32::from_be_bytes(bytes[at + 16..at + 20].try_into().unwrap()),
                );
            }
            if total == 0 {
                break;
            }
            at += total;
        }
        panic!("{} is missing", String::from_utf8_lossy(kind));
    };
    assert_eq!(
        widths(b"PWV7"),
        (3, track.detail.len() as u32),
        "the scrolling three-band waveform is three bytes over every detail column"
    );
    assert_eq!(
        widths(b"PWV6"),
        (3, 1200),
        "the three-band preview is three bytes over 1200 columns"
    );
}

/// The seek index is written when the track carries one and zeroed when it
/// does not.
///
/// Two callers fill `Track::seek_table`, the CLI off a disk and the browser out
/// of a tab, and a stick went out with 401 zeros on every one of 135 tracks
/// because only one of them did. The writer is what both reach, so this is
/// where the two shapes are pinned.
#[test]
fn the_seek_index_carries_the_table_the_track_was_given() {
    fn pvbr(bytes: &[u8]) -> Vec<u32> {
        let mut at = u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize;
        while at + 12 <= bytes.len() {
            let kind = &bytes[at..at + 4];
            let header = u32::from_be_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
            let total = u32::from_be_bytes(bytes[at + 8..at + 12].try_into().unwrap()) as usize;
            if kind == b"PVBR" {
                return bytes[at + header..at + total]
                    .chunks(4)
                    .map(|word| u32::from_be_bytes(word.try_into().unwrap()))
                    .collect();
            }
            if total == 0 {
                break;
            }
            at += total;
        }
        panic!("every DAT carries a PVBR section");
    }

    let mut without = track();
    without.seek_table = None;
    let words = pvbr(&rekordbox::anlz::dat(&without));
    assert_eq!(words.len(), 401, "the section is 401 words either way");
    assert!(
        words.iter().all(|word| *word == 0),
        "a format needing no index writes zeros, as rekordbox does for WAV"
    );

    let mut with = track();
    let table: Vec<u32> = (0..400)
        .map(|slice| slice as u32 * 1000)
        .chain([555_555])
        .collect();
    with.seek_table = Some(table.clone());
    assert_eq!(
        pvbr(&rekordbox::anlz::dat(&with)),
        table,
        "the table the track carries is the table the section holds"
    );
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
