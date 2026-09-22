//! The per-track analysis files a player reads for beat grid, cues and
//! waveforms.
//!
//! Three files per track. `ANLZ0000.DAT` carries what every player since the
//! CDJ-2000 reads: the path, the seek index, the beat grid, the cue lists and
//! the monochrome waveforms. `ANLZ0000.EXT` carries the later additions: the
//! detailed waveform, and the colour pair a Nexus 2 player draws in preference
//! to the monochrome one. `ANLZ0000.2EX` carries the three-band waveform a
//! player from the XDJ-RX3 generation draws in preference to the colour pair,
//! and a stick without one showed no scrolling waveform on an RX3 whatever the
//! other two held.
//!
//! Everything here is big-endian, unlike the database, and every section is a
//! four-character kind, a header length, a total length, then content.
//!
//! Every layout below is a struct with the field widths declared on it, so the
//! bytes a player reads can be checked against the format analysis field by
//! field. Bulk payloads, the waveform bytes, are appended after their header
//! rather than described: they are already-encoded bytes rather than a layout,
//! and a 126 kB `Vec<u8>` routed through a bit writer costs time for nothing.
//!
//! Format reference: <https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/anlz.html>

use collection::{Beat, CueKind, SectionLabel, Track};
use deku::prelude::*;
use waveform::{COLOUR_PREVIEW_COLUMNS, Column, PREVIEW_COLUMNS};

/// Columns in the tiny preview a player draws on the track list.
const TINY_PREVIEW_COLUMNS: usize = 100;

/// Bytes in one `PSSI` phrase, which the section declares and every export
/// carries.
const PHRASE_BYTES: u32 = 24;

/// The mood whose phrases are intro, up, down, chorus and outro, which is the
/// one the export measured against uses and the one the six labels here fit.
const MOOD_HIGH: u16 = 1;

/// The `PMAI` file header, which is 28 bytes of which 12 carry anything.
#[derive(DekuWrite)]
#[deku(endian = "big", magic = b"PMAI")]
struct FileHeader {
    header_size: u32,
    /// Sixteen bytes follow that no export examined has ever filled.
    #[deku(pad_bytes_after = "16")]
    total_size: u32,
}

/// A section header: kind, header length, total length.
///
/// `header_size` counts these twelve bytes plus the fixed fields at the start
/// of the body, which is where the variable part of the section begins. It is
/// a number rather than a split in the bytes: a cue entry is all fixed fields
/// and still declares a header of 0x1c, so deriving one from the other puts
/// four bytes of nothing in the middle of the entry.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct SectionHeader {
    kind: [u8; 4],
    header_size: u32,
    total_size: u32,
}

/// `PPTH`: the length of the path in bytes. The UTF-16BE path follows.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct PathHeader {
    len_path: u32,
}

/// `PQTZ`: the fields before the beat list.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct BeatGridHeader {
    zero: u32,
    /// Constant in every file that has been examined. The format analysis and
    /// rekordcrate both record it as 0x00800000; the 324 files measured against
    /// here, rekordbox exports and an XDJ-RX3's own writes alike, all carry the
    /// value below.
    constant: u32,
    len_beats: u32,
}

/// One beat: where it falls in the bar, the tempo there, and when it happens.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct BeatEntry {
    number_in_bar: u16,
    /// Tempo in hundredths of a BPM, which is the resolution the format has.
    centi_bpm: u16,
    time_ms: u32,
}

/// `PQT2`: the fields before the per-beat microseconds.
///
/// The two beats are the first and the last of the grid, repeated from `PQTZ`,
/// which is how the export measured against carries them: beat one reads
/// (4, 12500, 10) there and (4, 12500, 10) here, and the last reads
/// (2, 12500, 442569) in both.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct ExtendedBeatGridHeader {
    zero: u32,
    one: u8,
    zero_byte: u8,
    /// Two, which is how many beat entries follow.
    beats_in_header: u16,
    zero_word: u32,
    first_number_in_bar: u16,
    first_centi_bpm: u16,
    first_time_ms: u32,
    last_number_in_bar: u16,
    last_centi_bpm: u16,
    last_time_ms: u32,
    len_beats: u32,
    /// The one field here nothing explains. The export read for this section
    /// carries 0x0cdcb1f5, which is neither a time, a beat count, a tempo nor a
    /// sample count of that track, so it reads as track-specific rather than as
    /// a constant and copying it would put another track's number in every
    /// file. Written zero until a second export says what it is.
    unknown: u32,
    zero_tail: [u32; 2],
}

/// `PCOB`: the fields before a cue list.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct CueListHeader {
    /// 0 for the memory cues, 1 for the hot cues.
    list_type: u32,
    zero: u16,
    len_cues: u16,
    /// The format analysis calls this a count of the memory cues. It is all
    /// ones in each of the 924 cue lists measured against here, lists holding
    /// cues included, so it counts nothing.
    all_ones: u32,
}

/// `PCO2`: the fields before an extended cue list.
///
/// Four bytes shorter than [`CueListHeader`]: no word counting the memory cues,
/// and the cue count moves ahead of the padding. Both empty lists in the export
/// measured against read `PCO2 00000014 00000014 0000000t 0000`, which is this
/// with no entries after it.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct ExtendedCueListHeader {
    /// 0 for the memory cues, 1 for the hot cues, as in `PCOB`.
    list_type: u32,
    len_cues: u16,
    zero: u16,
}

/// `PCP2`: one extended cue, up to the comment.
///
/// A Nexus 2 reads these in preference to `PCPT`, and what it gets here that it
/// cannot get there is the comment and the colour. The entry runs on past the
/// comment, which is why this stops at its length: see [`extended_cue_list`].
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct ExtendedCueEntry {
    /// Hot cue slot, counted from 1, or 0 for a memory cue.
    hot_cue: u32,
    /// 1 for a point, 2 for a loop.
    kind: u8,
    zero: u8,
    /// Constant in every entry the format analysis records.
    constant_1000: u16,
    time_ms: u32,
    /// Where a loop ends. Zero when there is no loop, unlike `PCPT`, which
    /// writes all ones there.
    loop_end_ms: u32,
    /// Row of the colour table, which is the colour a memory cue is drawn in.
    colour_id: u8,
    /// The format analysis records this byte as 1 and the six after it as 0.
    one: u8,
    zero_pair: u16,
    zero_word: u32,
    /// The size of a quantised loop. Nothing here writes one.
    loop_numerator: u16,
    loop_denominator: u16,
}

/// `PCP2`: the fields after the comment.
///
/// The colour a hot cue lights its button with, then five words the format
/// analysis does not reach and rekordcrate reads as unknown. No populated
/// `PCO2` has been read here, so they are written zero and named.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct ExtendedCueColour {
    colour_index: u8,
    red: u8,
    green: u8,
    blue: u8,
    unknown: [u32; 5],
}

/// `PSSI`: the fields before the phrases, of which everything from `mood` on is
/// masked.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct SongStructureHeader {
    /// Twenty-four, the size of one phrase.
    len_entry_bytes: u32,
    len_entries: u16,
}

/// `PSSI`: the masked fields between the phrase count and the phrases.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct SongStructureBody {
    /// 1 high, 2 mid, 3 low. Which one decides what the phrase kinds mean.
    mood: u16,
    zero: u32,
    zero_word: u16,
    /// The beat the last phrase ends on.
    end_beat: u16,
    zero_pair: u16,
    /// The bank a player lights the track in. 0 is the default.
    bank: u8,
    zero_byte: u8,
}

/// `PSSI`: one phrase, 24 bytes.
///
/// The variant flags `k1`, `k2` and `k3` subdivide a high-mood phrase into
/// "Up 1", "Up 2" and the rest, and `beat2` to `beat4` mark where a phrase
/// changes inside itself. All of them are zero on all 31 phrases of the export
/// measured against, and this writes none of them: a phrase here is a stretch
/// the structure stage named, with nothing inside it that was measured.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct PhraseEntry {
    /// Counted from 1.
    index: u16,
    beat: u16,
    /// What the phrase is, in the vocabulary the mood picks.
    kind: u16,
    zero: u8,
    k1: u8,
    zero_2: u8,
    k2: u8,
    zero_3: u8,
    b: u8,
    beat2: u16,
    beat3: u16,
    beat4: u16,
    zero_4: u8,
    k3: u8,
    zero_5: u8,
    /// 1 when the phrase ends in beats belonging to no phrase.
    fill: u8,
    beat_fill: u16,
}

/// `PCPT`: one cue, fixed at 0x38 bytes of which this is the body.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct CueEntry {
    /// Hot cue slot, counted from 1, or 0 for a memory cue.
    hot_cue: u32,
    /// 4 marks an active loop. Nothing here writes one.
    status: u32,
    /// Constant in every file that has been examined.
    constant: u32,
    /// Two sort keys whose relationship nobody has explained. The values
    /// written are the ones rekordbox writes for the first and later cues.
    order_first: u16,
    order_last: u16,
    /// 1 for a point, 2 for a loop.
    kind: u8,
    zero: u8,
    /// Constant in every file that has been examined.
    constant_1000: u16,
    time_ms: u32,
    /// Where a loop ends, and all ones when there is no loop.
    ///
    /// Sixteen bytes of colour and comment fields follow, which a CDJ reads
    /// from the database instead and which rekordbox leaves zero here.
    #[deku(pad_bytes_after = "16")]
    loop_end_ms: u32,
}

/// `PWAV` and `PWV2`: the fields before a monochrome preview.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct PreviewHeader {
    len_data: u32,
    /// Constant in every file that has been examined. The format analysis
    /// records 0x00100000; all 324 files measured against here carry the value
    /// below.
    constant: u32,
}

/// `PWV6`: the fields before the three-band preview. Two words where a
/// scrolling waveform carries three, which is why it declares a 0x14 header.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct ThreeBandPreviewHeader {
    bytes_per_column: u32,
    len_data: u32,
}

/// The three bytes a three-band column is, one level per band.
///
/// Unlike [`ColourDetailColumn`] there is no height: each band carries its own
/// level and the player stacks them, which is what makes this waveform read as
/// three overlaid shapes rather than one shape tinted.
///
/// The order is the one [`ColourPreviewColumn`] uses, low band first. rekordbox
/// computes these from its own band split rather than from the colour waveform,
/// so the levels here are this tool's own and will not match it column for
/// column.
#[derive(DekuWrite)]
struct ThreeBandColumn {
    low: u8,
    mid: u8,
    high: u8,
}

/// `PWV3`, `PWV4` and `PWV5`: the fields before a scrolling waveform.
#[derive(DekuWrite)]
#[deku(endian = "big")]
struct DetailHeader {
    bytes_per_column: u32,
    len_data: u32,
    /// Constant per section kind, and a different constant in each.
    constant: u32,
}

/// The one byte a monochrome column is: five bits of height from the low bit
/// up, then three of shade.
#[derive(DekuWrite)]
#[deku(bit_order = "lsb")]
struct MonochromeColumn {
    #[deku(bits = 5)]
    height: u8,
    #[deku(bits = 3)]
    shade: u8,
}

/// The six bytes a colour preview column is: a height, its complement, the
/// loudest band, and then the three band levels.
///
/// Measured against the `PWV4` a rekordbox export wrote for a track this tool
/// also analysed, 1200 columns of it against the same track's audio. The last
/// three bytes track the low, mid and high band energy, at 0.97, 0.83 and 0.84.
/// `height` tracks the loudest detail column under it at 0.97 and never passes
/// 127. `height` and `inverse_height` sum to 255 give or take seven, which is
/// what says the second is the first subtracted from full scale rather than a
/// measurement of its own, and `loudest_band` sits within five of the largest
/// of the three levels.
#[derive(DekuWrite)]
struct ColourPreviewColumn {
    height: u8,
    inverse_height: u8,
    loudest_band: u8,
    low: u8,
    mid: u8,
    high: u8,
}

/// The two bytes a colour detail column is.
///
/// Sixteen bits from the high end of the first byte down: three each of low,
/// mid and high band, five of height, and two the format analysis calls a
/// sub-step, which the export measured against leaves zero on every one of its
/// 66 395 columns.
///
/// rekordcrate reads these two bytes the other way up, and this crate wrote
/// what rekordcrate read until an XDJ-RX3 drew no scrolling waveform from it.
/// In the rekordbox export of the same track the height in bits six to two of
/// the second byte matches the `PWV3` height column for column, at 0.97 over
/// the whole track; read where rekordcrate puts it, one bit lower, it matches
/// at 0.51 and is a different number on almost every column. The low band lands
/// in the field rekordcrate calls red, at 0.73 against the track's own audio
/// below 200 Hz, and the high band in the one it calls blue.
#[derive(DekuWrite)]
struct ColourDetailColumn {
    #[deku(bits = 3)]
    low: u8,
    #[deku(bits = 3)]
    mid: u8,
    #[deku(bits = 3)]
    high: u8,
    #[deku(bits = 5)]
    height: u8,
    #[deku(bits = 2)]
    sub_step: u8,
}

/// Every layout in this file is fixed, so a write cannot fail for any reason a
/// caller could fix. This is where that is asserted once instead of at each of
/// the twenty places a struct becomes bytes.
fn bytes(layout: &impl DekuContainerWrite) -> Vec<u8> {
    layout.to_bytes().expect("a fixed layout with no counts")
}

/// The DAT file: path, seek index, beat grid, the two preview waveforms, cues.
///
/// The order is the one every real DAT carries. A parser that reads the section
/// headers does not need it, and a player that seeks to a fixed offset does.
pub fn dat(track: &Track) -> Vec<u8> {
    let mut sections = Vec::new();
    sections.extend(path_section(&track.device_path));
    sections.extend(seek_index(track));
    sections.extend(beat_grid(track));
    sections.extend(waveform_preview(track));
    sections.extend(tiny_waveform_preview(track));
    sections.extend(cue_list(track, CueKind::Hot));
    sections.extend(cue_list(track, CueKind::Memory));
    file(sections)
}

/// The EXT file: the path again, the scrolling waveforms and the cues.
///
/// The path is repeated because a player may read either file first and each
/// one has to identify the track it belongs to.
///
/// The order is the one the export measured against carries, and it puts the
/// extended cue lists and the extended beat grid between the old cue lists and
/// the colour waveforms, with the phrases last.
pub fn two_ex(track: &Track) -> Vec<u8> {
    let mut sections = Vec::new();
    sections.extend(path_section(&track.device_path));
    sections.extend(three_band_detail(track));
    sections.extend(three_band_preview(track));
    sections.extend(three_band_scale(track));
    file(sections)
}

/// The EXT file: the path again, the scrolling waveforms and the cues.
pub fn ext(track: &Track) -> Vec<u8> {
    let mut sections = Vec::new();
    sections.extend(path_section(&track.device_path));
    sections.extend(waveform_detail(track));
    sections.extend(cue_list(track, CueKind::Hot));
    sections.extend(cue_list(track, CueKind::Memory));
    sections.extend(extended_cue_list(track, CueKind::Hot));
    sections.extend(extended_cue_list(track, CueKind::Memory));
    sections.extend(extended_beat_grid(track));
    sections.extend(colour_waveform_detail(track));
    sections.extend(colour_waveform_preview(track));
    sections.extend(song_structure(track));
    file(sections)
}

/// Wrap sections in the `PMAI` file header.
fn file(sections: Vec<u8>) -> Vec<u8> {
    const HEADER_SIZE: u32 = 0x1c;
    let mut out = bytes(&FileHeader {
        header_size: HEADER_SIZE,
        total_size: HEADER_SIZE + sections.len() as u32,
    });
    out.extend_from_slice(&sections);
    out
}

/// A section: the header, then the body.
fn section(kind: &[u8; 4], header_size: u32, body: &[u8]) -> Vec<u8> {
    let mut out = bytes(&SectionHeader {
        kind: *kind,
        header_size,
        total_size: 12 + body.len() as u32,
    });
    out.extend_from_slice(body);
    out
}

/// `PPTH`: the path of the audio file on the device, UTF-16BE with a
/// terminator.
fn path_section(device_path: &str) -> Vec<u8> {
    let mut text: Vec<u8> = device_path
        .encode_utf16()
        .flat_map(u16::to_be_bytes)
        .collect();
    text.extend_from_slice(&[0, 0]);

    let mut body = bytes(&PathHeader {
        len_path: text.len() as u32,
    });
    body.extend_from_slice(&text);
    section(b"PPTH", 0x10, &body)
}

/// `PVBR`: the seek index, a leading word and then 401 big-endian ones.
///
/// It lets a player seek in a file whose bitrate varies: 400 byte offsets, one
/// per slice of the track, then the sample count. A constant-bitrate file needs
/// none, and across the WAV tracks measured against here two or three of the
/// section's 1608 bytes were non-zero, so [`Track::seek_table`] is `None` for
/// those and this writes what rekordbox writes.
///
/// An MP3 with 401 zeros here tells a player every point in the track is at
/// byte zero, which is the state in which it throws the analysis away and reads
/// the file itself.
fn seek_index(track: &Track) -> Vec<u8> {
    const WORDS: usize = 401;
    let mut body = vec![0u8; 4];
    let table = track.seek_table.clone().unwrap_or_default();
    for index in 0..WORDS {
        let word = table.get(index).copied().unwrap_or(0);
        body.extend_from_slice(&word.to_be_bytes());
    }
    section(b"PVBR", 0x10, &body)
}

/// `PQTZ`: the beat grid, as a beat number in the bar, a tempo and a time.
fn beat_grid(track: &Track) -> Vec<u8> {
    let mut body = bytes(&BeatGridHeader {
        zero: 0,
        constant: 0x0008_0000,
        len_beats: track.beats.len() as u32,
    });
    for beat in track.beats.iter().map(beat_entry) {
        body.extend(bytes(&beat));
    }
    section(b"PQTZ", 0x18, &body)
}

/// One beat as both grid sections store it.
///
/// The time is the whole millisecond the beat falls on or after, not the
/// nearest one, because `PQT2` carries the rest of it as a positive remainder
/// and a rounded-up millisecond would need a negative one. The export measured
/// against floors: its first beat is 10 ms with a remainder of 884 µs against a
/// grid whose line through all 923 beats passes 10.884 ms.
fn beat_entry(beat: &Beat) -> BeatEntry {
    BeatEntry {
        number_in_bar: u16::from(beat.number_in_bar),
        centi_bpm: (beat.bpm * 100.0).round() as u16,
        time_ms: (beat_microseconds(beat) / 1000) as u32,
    }
}

/// A beat's time in whole microseconds, which both grid sections divide up.
fn beat_microseconds(beat: &Beat) -> u64 {
    (beat.time_seconds * 1_000_000.0).round().max(0.0) as u64
}

/// `PQT2`: the beat grid a Nexus 2 reads, which is the same beats to the
/// microsecond.
///
/// The body is one big-endian word per beat: the microseconds past the whole
/// millisecond `PQTZ` stores. Measured on the export beside this one, where
/// `time_ms + word / 1000` fits a straight line through all 923 beats to within
/// 0.046 ms, against 0.527 ms for the whole milliseconds alone. Its words run
/// 884, 793, 702 and on down through 68 to 977, which is the sub-millisecond
/// drift of a 479.99832 ms beat wrapping at 1000.
///
/// rekordcrate does not read this section at all, so nothing but that export
/// checks the layout.
fn extended_beat_grid(track: &Track) -> Vec<u8> {
    let edge = |beat: Option<&Beat>| {
        beat.map(beat_entry).unwrap_or(BeatEntry {
            number_in_bar: 0,
            centi_bpm: 0,
            time_ms: 0,
        })
    };
    let first = edge(track.beats.first());
    let last = edge(track.beats.last());

    let mut body = bytes(&ExtendedBeatGridHeader {
        zero: 0,
        one: 1,
        zero_byte: 0,
        beats_in_header: 2,
        zero_word: 0,
        first_number_in_bar: first.number_in_bar,
        first_centi_bpm: first.centi_bpm,
        first_time_ms: first.time_ms,
        last_number_in_bar: last.number_in_bar,
        last_centi_bpm: last.centi_bpm,
        last_time_ms: last.time_ms,
        len_beats: track.beats.len() as u32,
        unknown: 0,
        zero_tail: [0; 2],
    });
    for beat in &track.beats {
        body.extend_from_slice(&((beat_microseconds(beat) % 1000) as u16).to_be_bytes());
    }
    section(b"PQT2", 0x38, &body)
}

/// `PCOB`: one list of cues, either the memory cues or the hot cues.
///
/// A player wants both sections present even when one of them is empty.
fn cue_list(track: &Track, kind: CueKind) -> Vec<u8> {
    let cues: Vec<_> = track.cues.iter().filter(|cue| cue.kind == kind).collect();

    let mut body = bytes(&CueListHeader {
        list_type: u32::from(kind == CueKind::Hot),
        zero: 0,
        len_cues: cues.len() as u16,
        all_ones: 0xffff_ffff,
    });

    for (position, cue) in cues.iter().enumerate() {
        let entry = CueEntry {
            hot_cue: u32::from(if kind == CueKind::Hot { cue.number } else { 0 }),
            status: 0,
            constant: 0x0010_0000,
            order_first: if position == 0 {
                0xffff
            } else {
                position as u16 - 1
            },
            order_last: (position + 1) as u16,
            kind: 1,
            zero: 0,
            constant_1000: 1000,
            time_ms: (cue.time_seconds * 1000.0).round().max(0.0) as u32,
            loop_end_ms: 0xffff_ffff,
        };
        // A cue entry is fixed at 0x38 bytes and declares a 0x1c header, which
        // covers the fields up to the sort keys.
        body.extend(section(b"PCPT", 0x1c, &bytes(&entry)));
    }

    section(b"PCOB", 0x18, &body)
}

/// `PCO2`: the same cues again, in the layout a Nexus 2 reads.
///
/// Both lists are written even when empty, as `PCOB` is: the export measured
/// against carries two empty ones beside two empty `PCOB`s, and an empty
/// extended list beside a populated `PCOB` is how a player is told this track's
/// cues are the old kind.
///
/// The entry layout past the comment is the one rekordcrate reads and the
/// format analysis does not reach. No populated `PCO2` has been read here, so
/// it is the least checked layout this crate writes.
fn extended_cue_list(track: &Track, kind: CueKind) -> Vec<u8> {
    let cues: Vec<_> = track.cues.iter().filter(|cue| cue.kind == kind).collect();

    let mut body = bytes(&ExtendedCueListHeader {
        list_type: u32::from(kind == CueKind::Hot),
        len_cues: cues.len() as u16,
        zero: 0,
    });

    for cue in cues {
        let mut entry = bytes(&ExtendedCueEntry {
            hot_cue: u32::from(if kind == CueKind::Hot { cue.number } else { 0 }),
            kind: 1,
            zero: 0,
            constant_1000: 1000,
            time_ms: (cue.time_seconds * 1000.0).round().max(0.0) as u32,
            loop_end_ms: 0,
            colour_id: 0,
            one: 1,
            zero_pair: 0,
            zero_word: 0,
            loop_numerator: 0,
            loop_denominator: 0,
        });
        entry.extend(wide_string(&cue.comment));
        entry.extend(bytes(&ExtendedCueColour {
            colour_index: 0,
            red: 0,
            green: 0,
            blue: 0,
            unknown: [0; 5],
        }));
        // The header covers the kind, the two lengths and the hot cue slot,
        // which is where the format analysis puts the split.
        body.extend(section(b"PCP2", 0x10, &entry));
    }

    section(b"PCO2", 0x14, &body)
}

/// A length-prefixed UTF-16BE string with a terminator, as a cue comment is.
///
/// The length counts the terminator, and an empty comment is a length of zero
/// with no bytes after it rather than a length of two with a terminator.
fn wide_string(text: &str) -> Vec<u8> {
    if text.is_empty() {
        return 0u32.to_be_bytes().to_vec();
    }
    let mut encoded: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
    encoded.extend_from_slice(&[0, 0]);

    let mut out = (encoded.len() as u32).to_be_bytes().to_vec();
    out.extend_from_slice(&encoded);
    out
}

/// What a high-mood phrase kind means, which is the vocabulary this tool's own
/// labels fit.
///
/// The five the export measured against uses, and the five the structure stage
/// names, land on each other: an intro is an intro, a build runs energy up, a
/// drop is the loudest stretch the track has, a breakdown takes the kick out,
/// and an outro is an outro. A stretch this tool could not place goes down as
/// "up", because the high mood has no word for one and the alternative is
/// leaving a hole in the middle of the track.
fn phrase_kind(label: SectionLabel) -> u16 {
    match label {
        SectionLabel::Intro => 1,
        SectionLabel::Build | SectionLabel::Steady => 2,
        SectionLabel::Breakdown => 3,
        SectionLabel::Drop => 5,
        SectionLabel::Outro => 6,
    }
}

/// The beat a time falls on, counted from 1, which is how a phrase names one.
fn beat_at(track: &Track, seconds: f64) -> u16 {
    let index = track
        .beats
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            let (a, b) = (
                (a.time_seconds - seconds).abs(),
                (b.time_seconds - seconds).abs(),
            );
            a.total_cmp(&b)
        })
        .map(|(index, _)| index)
        .unwrap_or(0);
    (index as u16).saturating_add(1)
}

/// `PSSI`: the phrases, which is what a player lights the track by and what
/// anything reading this stick gets instead of running its own analysis.
///
/// Masked, because every export since rekordbox 6 is: each byte from `mood`
/// onwards is XORed with a nineteen-byte pattern whose every byte has the
/// phrase count added to it. The pattern is the one the format analysis
/// records, and it decodes the export measured against into 31 phrases whose
/// beats land on its bar lines.
///
/// The phrases are this tool's own sections, one phrase each, which is coarser
/// than rekordbox's: it cut this track into 31 phrases where the structure
/// stage found 4 stretches. Both are claims about the same track; these are the
/// ones `report.json` carries the evidence for.
fn song_structure(track: &Track) -> Vec<u8> {
    let mut body = bytes(&SongStructureBody {
        mood: MOOD_HIGH,
        zero: 0,
        zero_word: 0,
        end_beat: track
            .sections
            .last()
            .map(|section| beat_at(track, section.end_seconds))
            .unwrap_or(0),
        zero_pair: 0,
        bank: 0,
        zero_byte: 0,
    });

    for (position, section) in track.sections.iter().enumerate() {
        body.extend(bytes(&PhraseEntry {
            index: position as u16 + 1,
            beat: beat_at(track, section.start_seconds),
            kind: phrase_kind(section.label),
            zero: 0,
            k1: 0,
            zero_2: 0,
            k2: 0,
            zero_3: 0,
            b: 0,
            beat2: 0,
            beat3: 0,
            beat4: 0,
            zero_4: 0,
            k3: 0,
            zero_5: 0,
            fill: 0,
            beat_fill: 0,
        }));
    }

    let count = track.sections.len() as u16;
    for (position, byte) in body.iter_mut().enumerate() {
        *byte ^= mask_byte(position, count);
    }

    let mut out = bytes(&SongStructureHeader {
        len_entry_bytes: PHRASE_BYTES,
        len_entries: count,
    });
    out.extend_from_slice(&body);
    section(b"PSSI", 0x20, &out)
}

/// The mask over a `PSSI` body, which starts at the mood and runs to the end.
fn mask_byte(position: usize, len_entries: u16) -> u8 {
    /// The pattern the format analysis records, before the count is added.
    const PATTERN: [u8; 19] = [
        0xCB, 0xE1, 0xEE, 0xFA, 0xE5, 0xEE, 0xAD, 0xEE, 0xE9, 0xD2, 0xE9, 0xEB, 0xE1, 0xE9, 0xF3,
        0xE8, 0xE9, 0xF4, 0xE1,
    ];
    (u16::from(PATTERN[position % PATTERN.len()]) + len_entries) as u8
}

/// One byte of a monochrome waveform.
fn monochrome(column: &Column) -> u8 {
    let column = column.clamped();
    bytes(&MonochromeColumn {
        height: column.height,
        shade: column.shade(),
    })[0]
}

/// `PWAV`: the 400-column monochrome preview, one byte per column.
fn waveform_preview(track: &Track) -> Vec<u8> {
    let columns = waveform::resample(&track.preview, PREVIEW_COLUMNS);
    let content: Vec<u8> = columns.iter().map(monochrome).collect();

    let mut body = bytes(&PreviewHeader {
        len_data: content.len() as u32,
        constant: 0x0001_0000,
    });
    body.extend_from_slice(&content);
    section(b"PWAV", 0x14, &body)
}

/// `PWV2`: the same shape at a quarter of the width, drawn on the track list.
///
/// Only the height matters here, in the low four bits, so the columns are
/// re-quantised rather than reused.
fn tiny_waveform_preview(track: &Track) -> Vec<u8> {
    let content: Vec<u8> = waveform::resample(&track.preview, TINY_PREVIEW_COLUMNS)
        .iter()
        .map(|column| column.height / 2) // 0..=31 becomes 0..=15
        .collect();

    let mut body = bytes(&PreviewHeader {
        len_data: content.len() as u32,
        constant: 0x0001_0000,
    });
    body.extend_from_slice(&content);
    section(b"PWV2", 0x14, &body)
}

/// `PWV3`: the scrolling waveform, 150 columns per second.
fn waveform_detail(track: &Track) -> Vec<u8> {
    let content: Vec<u8> = track.detail.columns().map(|c| monochrome(&c)).collect();

    let mut body = bytes(&DetailHeader {
        bytes_per_column: 1,
        len_data: content.len() as u32,
        constant: 0x0096_0000,
    });
    body.extend_from_slice(&content);
    section(b"PWV3", 0x18, &body)
}

/// `PWV4`: the colour preview, six bytes per column over 1200 columns.
fn colour_waveform_preview(track: &Track) -> Vec<u8> {
    let columns = waveform::resample(&track.preview, COLOUR_PREVIEW_COLUMNS);
    let mut content = Vec::with_capacity(columns.len() * 6);
    for column in &columns {
        let [low, mid, high] = band_levels(column);
        let height = level(f64::from(column.clamped().height) / 31.0);
        content.extend(bytes(&ColourPreviewColumn {
            height,
            inverse_height: 255 - height,
            loudest_band: low.max(mid).max(high),
            low,
            mid,
            high,
        }));
    }

    let mut body = bytes(&DetailHeader {
        bytes_per_column: 6,
        len_data: columns.len() as u32,
        constant: 0,
    });
    body.extend_from_slice(&content);
    section(b"PWV4", 0x18, &body)
}

/// A share of full scale as a waveform field that stores a level rather than a
/// balance holds it.
///
/// Levels run to 127 rather than 255: across the three-band waveforms and the
/// colour preview measured here no band ever exceeds it, and a value a player
/// clamps is a value it draws wrong. The colour preview this exporter wrote
/// before scaled its bands to 255 and spent most of a loud track pinned there.
fn level(share: f64) -> u8 {
    const FULL: f64 = 127.0;
    (share * FULL).round() as u8
}

/// The three band levels of a column, each scaled by the column's height.
///
/// A [`Column`] carries a balance, where the loudest band of the column reads
/// 7, and the level a waveform stores is that balance times the height. `PWV4`,
/// `PWV6` and `PWV7` all store it, so it is computed once.
fn band_levels(column: &Column) -> [u8; 3] {
    let column = column.clamped();
    let height = f64::from(column.height) / 31.0;
    [column.low, column.mid, column.high].map(|band| level(f64::from(band) / 7.0 * height))
}

/// One three-band column, one level per band.
fn three_band(column: &Column) -> Vec<u8> {
    let [low, mid, high] = band_levels(column);
    bytes(&ThreeBandColumn { low, mid, high })
}

/// `PWV7`: the three-band scrolling waveform, three bytes per column.
fn three_band_detail(track: &Track) -> Vec<u8> {
    let content: Vec<u8> = track
        .detail
        .columns()
        .flat_map(|c| three_band(&c))
        .collect();

    let mut body = bytes(&DetailHeader {
        bytes_per_column: 3,
        len_data: track.detail.len() as u32,
        constant: 0x0096_0000,
    });
    body.extend_from_slice(&content);
    section(b"PWV7", 0x18, &body)
}

/// `PWV6`: the three-band preview, over the same 1200 columns as `PWV4`.
fn three_band_preview(track: &Track) -> Vec<u8> {
    let columns = waveform::resample(&track.preview, COLOUR_PREVIEW_COLUMNS);
    let content: Vec<u8> = columns.iter().flat_map(three_band).collect();

    let mut body = bytes(&ThreeBandPreviewHeader {
        bytes_per_column: 3,
        len_data: columns.len() as u32,
    });
    body.extend_from_slice(&content);
    section(b"PWV6", 0x14, &body)
}

/// `PWVC`: what rekordcrate calls the three-band calibration.
///
/// Two bytes of header tail and then three big-endian words, one per band. In
/// the real file measured here they sit about a fifth below each band's loudest
/// column; in two others they run past 255, which no band level does, so the
/// scale is not fixed and nothing establishes what a player does with them.
/// This writes the loudest column per band, which is a measurement of the
/// waveform beside it rather than a number copied out of somebody else's track.
fn three_band_scale(track: &Track) -> Vec<u8> {
    let mut loudest = [0u8; 3];
    for column in track.detail.columns() {
        for (slot, level) in loudest.iter_mut().zip(three_band(&column)) {
            *slot = (*slot).max(level);
        }
    }

    let mut body = vec![0u8; 2];
    for level in loudest {
        body.extend_from_slice(&u16::from(level).to_be_bytes());
    }
    section(b"PWVC", 0x0e, &body)
}

/// `PWV5`: the colour detail waveform, two bytes per column.
///
/// See [`ColourDetailColumn`] for what those two bytes hold.
fn colour_waveform_detail(track: &Track) -> Vec<u8> {
    let mut content = Vec::with_capacity(track.detail.len() * 2);
    for column in track.detail.columns() {
        let column = column.clamped();
        content.extend(bytes(&ColourDetailColumn {
            low: column.low,
            mid: column.mid,
            high: column.high,
            height: column.height,
            sub_step: 0,
        }));
    }

    let mut body = bytes(&DetailHeader {
        bytes_per_column: 2,
        len_data: track.detail.len() as u32,
        constant: 0x0096_0305,
    });
    body.extend_from_slice(&content);
    section(b"PWV5", 0x18, &body)
}

#[cfg(test)]
mod packing {
    use super::*;

    /// The two bit-packed layouts as they were written before they were
    /// declared, kept as the oracle. A derive macro that fills bits from the
    /// wrong end writes bytes of the right length holding the wrong numbers,
    /// which a player draws rather than refuses, so the proof is exhaustive
    /// over every value the fields can hold.
    fn shifted_monochrome(column: &Column) -> u8 {
        (column.shade() << 5) | (column.height & 0x1f)
    }

    fn shifted_colour_detail(column: &Column) -> [u8; 2] {
        let packed: u16 = (u16::from(column.low & 0x07) << 13)
            | (u16::from(column.mid & 0x07) << 10)
            | (u16::from(column.high & 0x07) << 7)
            | (u16::from(column.height & 0x1f) << 2);
        packed.to_be_bytes()
    }

    #[test]
    fn the_declared_layouts_are_the_layouts_the_shifts_wrote() {
        for height in 0..32u8 {
            for low in 0..8u8 {
                for mid in 0..8u8 {
                    for high in 0..8u8 {
                        let column = Column {
                            height,
                            low,
                            mid,
                            high,
                        };
                        let where_at = format!("height {height} low {low} mid {mid} high {high}");

                        assert_eq!(
                            monochrome(&column),
                            shifted_monochrome(&column),
                            "monochrome at {where_at}"
                        );

                        let declared = bytes(&ColourDetailColumn {
                            low: column.low,
                            mid: column.mid,
                            high: column.high,
                            height: column.height,
                            sub_step: 0,
                        });
                        assert_eq!(
                            declared.as_slice(),
                            shifted_colour_detail(&column).as_slice(),
                            "colour detail at {where_at}"
                        );
                    }
                }
            }
        }
    }
}
