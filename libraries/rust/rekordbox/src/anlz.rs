//! The per-track analysis files a player reads for beat grid, cues and
//! waveforms.
//!
//! Two files per track. `ANLZ0000.DAT` carries what every player since the
//! CDJ-2000 reads: the path, the beat grid, the cue lists and the monochrome
//! waveforms. `ANLZ0000.EXT` carries the later additions: the detailed
//! waveform, and the colour pair a Nexus 2 or newer player draws in preference
//! to the monochrome one.
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

use collection::{CueKind, Track};
use deku::prelude::*;
use waveform::{COLOUR_PREVIEW_COLUMNS, Column, PREVIEW_COLUMNS};

/// Columns in the tiny preview a player draws on the track list.
const TINY_PREVIEW_COLUMNS: usize = 100;

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

/// The six bytes a colour preview column is, one per band plus the pair the
/// format analysis calls the whiteness.
///
/// There is no height field: the player draws the column from the energies
/// themselves, so each band arrives scaled by the column's height as well as by
/// its share of the column.
#[derive(DekuWrite)]
struct ColourPreviewColumn {
    whiteness: u8,
    whiteness_again: u8,
    /// Energy below 10 kHz, which the player uses for the body of the column.
    below_10k: u8,
    low: u8,
    mid: u8,
    high: u8,
}

/// The two bytes a colour detail column is.
///
/// Sixteen bits from the low end up: three each of high, mid and low band, five
/// of height, and two the format analysis calls a sub-step, which no export
/// examined has ever set. `low` is the field that crosses the byte boundary,
/// bits six to eight, which is the reason this is declared rather than shifted:
/// `bit_order = "lsb"` places it, and the default would fill from the high bit
/// down and write a file a player draws wrong rather than one it rejects.
///
/// Red is the top of the spectrum and blue the bottom, which is the way every
/// player draws it.
#[derive(DekuWrite)]
#[deku(bit_order = "lsb")]
struct ColourDetailColumn {
    #[deku(bits = 3)]
    high: u8,
    #[deku(bits = 3)]
    mid: u8,
    #[deku(bits = 3)]
    low: u8,
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
/// A real EXT carries four more sections this one does not: `PCO2` twice, the
/// cue lists a Nexus 2 reads in preference to `PCOB`, `PQT2` for the extended
/// beat grid, and `PSSI` for the phrases. The first three are cues and beats
/// this exporter already measures in a layout nothing here has been checked
/// against; `PSSI` is phrase detection it does not do at all.
pub fn ext(track: &Track) -> Vec<u8> {
    let mut sections = Vec::new();
    sections.extend(path_section(&track.device_path));
    sections.extend(waveform_detail(track));
    sections.extend(cue_list(track, CueKind::Hot));
    sections.extend(cue_list(track, CueKind::Memory));
    sections.extend(colour_waveform_detail(track));
    sections.extend(colour_waveform_preview(track));
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
    for beat in &track.beats {
        body.extend(bytes(&BeatEntry {
            number_in_bar: u16::from(beat.number_in_bar),
            centi_bpm: (beat.bpm * 100.0).round() as u16,
            time_ms: (beat.time_seconds * 1000.0).round() as u32,
        }));
    }
    section(b"PQTZ", 0x18, &body)
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
        let column = column.clamped();
        let energy = |level: u8| {
            ((f64::from(level) / 7.0) * (f64::from(column.height) / 31.0) * 255.0).round() as u8
        };
        let (low, mid, high) = (energy(column.low), energy(column.mid), energy(column.high));
        // A column with energy in every band is the white one, so the smallest
        // of the three is what both whiteness bytes carry.
        let whiteness = low.min(mid).min(high);
        content.extend(bytes(&ColourPreviewColumn {
            whiteness,
            whiteness_again: whiteness,
            below_10k: low.saturating_add(mid / 2),
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

/// `PWV5`: the colour detail waveform, two bytes per column.
///
/// See [`ColourDetailColumn`] for what those two bytes hold.
fn colour_waveform_detail(track: &Track) -> Vec<u8> {
    let mut content = Vec::with_capacity(track.detail.len() * 2);
    for column in track.detail.columns() {
        let column = column.clamped();
        content.extend(bytes(&ColourDetailColumn {
            high: column.high,
            mid: column.mid,
            low: column.low,
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
        let packed: u16 = u16::from(column.high & 0x07)
            | (u16::from(column.mid & 0x07) << 3)
            | (u16::from(column.low & 0x07) << 6)
            | (u16::from(column.height & 0x1f) << 9);
        [packed as u8, (packed >> 8) as u8]
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
                            high: column.high,
                            mid: column.mid,
                            low: column.low,
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
