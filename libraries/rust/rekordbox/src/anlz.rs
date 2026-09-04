//! The per-track analysis files a player reads for beat grid, cues and
//! waveforms.
//!
//! Two files per track. `ANLZ0000.DAT` carries what every player since the
//! CDJ-2000 reads: the path, the beat grid, the cue lists and the monochrome
//! waveforms. `ANLZ0000.EXT` carries the later additions, of which this writes
//! the detailed waveform.
//!
//! Everything here is big-endian, unlike the database, and every section is a
//! four-character kind, a header length, a total length, then content.
//!
//! Format reference: <https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/anlz.html>

use collection::{CueKind, Track};

/// Columns in the tiny preview a player draws on the track list.
const TINY_PREVIEW_COLUMNS: usize = 100;

/// The DAT file: path, beat grid, cues, and the two preview waveforms.
pub fn dat(track: &Track) -> Vec<u8> {
    let mut sections = Vec::new();
    sections.extend(path_section(&track.device_path));
    sections.extend(beat_grid(track));
    sections.extend(cue_list(track, CueKind::Memory));
    sections.extend(cue_list(track, CueKind::Hot));
    sections.extend(waveform_preview(track));
    sections.extend(tiny_waveform_preview(track));
    file(sections)
}

/// The EXT file: the path again, and the scrolling waveform.
///
/// The path is repeated because a player may read either file first and each
/// one has to identify the track it belongs to.
pub fn ext(track: &Track) -> Vec<u8> {
    let mut sections = Vec::new();
    sections.extend(path_section(&track.device_path));
    sections.extend(waveform_detail(track));
    file(sections)
}

/// Wrap sections in the `PMAI` file header.
fn file(sections: Vec<u8>) -> Vec<u8> {
    const HEADER_SIZE: u32 = 0x1c;
    let mut out = Vec::with_capacity(sections.len() + HEADER_SIZE as usize);
    out.extend_from_slice(b"PMAI");
    out.extend_from_slice(&HEADER_SIZE.to_be_bytes());
    out.extend_from_slice(&(HEADER_SIZE + sections.len() as u32).to_be_bytes());
    // The header is 28 bytes and only the first 12 carry anything.
    out.resize(HEADER_SIZE as usize, 0);
    out.extend_from_slice(&sections);
    out
}

/// A section: kind, header length, total length, then the body.
///
/// `header_size` counts the twelve bytes above plus the fixed fields at the
/// start of the body, which is where the variable part of the section begins.
/// It is a number rather than a split in the bytes: a cue entry is all fixed
/// fields and still declares a header of 0x1c, so deriving one from the other
/// puts four bytes of nothing in the middle of the entry.
fn section(kind: &[u8; 4], header_size: u32, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(12 + body.len());
    out.extend_from_slice(kind);
    out.extend_from_slice(&header_size.to_be_bytes());
    out.extend_from_slice(&(12 + body.len() as u32).to_be_bytes());
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

    let mut body = Vec::with_capacity(text.len() + 4);
    body.extend_from_slice(&(text.len() as u32).to_be_bytes());
    body.extend_from_slice(&text);
    section(b"PPTH", 0x10, &body)
}

/// `PQTZ`: the beat grid, as a beat number in the bar, a tempo and a time.
fn beat_grid(track: &Track) -> Vec<u8> {
    let mut body = Vec::with_capacity(12 + track.beats.len() * 8);
    body.extend_from_slice(&0u32.to_be_bytes());
    // Constant in every file that has been examined.
    body.extend_from_slice(&0x0080_0000u32.to_be_bytes());
    body.extend_from_slice(&(track.beats.len() as u32).to_be_bytes());
    for beat in &track.beats {
        body.extend_from_slice(&u16::from(beat.number_in_bar).to_be_bytes());
        body.extend_from_slice(&((beat.bpm * 100.0).round() as u16).to_be_bytes());
        body.extend_from_slice(&((beat.time_seconds * 1000.0).round() as u32).to_be_bytes());
    }
    section(b"PQTZ", 0x18, &body)
}

/// `PCOB`: one list of cues, either the memory cues or the hot cues.
///
/// A player wants both sections present even when one of them is empty.
fn cue_list(track: &Track, kind: CueKind) -> Vec<u8> {
    let cues: Vec<_> = track.cues.iter().filter(|cue| cue.kind == kind).collect();

    let mut body = Vec::with_capacity(12 + cues.len() * 56);
    body.extend_from_slice(&(u32::from(kind == CueKind::Hot)).to_be_bytes());
    body.extend_from_slice(&0u16.to_be_bytes());
    body.extend_from_slice(&(cues.len() as u16).to_be_bytes());
    body.extend_from_slice(&(cues.len() as u32).to_be_bytes());

    for (position, cue) in cues.iter().enumerate() {
        let time_ms = (cue.time_seconds * 1000.0).round().max(0.0) as u32;
        let mut entry = Vec::with_capacity(44);
        entry.extend_from_slice(
            &u32::from(if kind == CueKind::Hot { cue.number } else { 0 }).to_be_bytes(),
        );
        entry.extend_from_slice(&0u32.to_be_bytes()); // status, 4 for an active loop
        entry.extend_from_slice(&0x0010_0000u32.to_be_bytes()); // constant
        // Two sort keys whose relationship nobody has explained. The values
        // below are the ones rekordbox writes for the first and following cues.
        entry.extend_from_slice(
            &(if position == 0 {
                0xffffu16
            } else {
                position as u16 - 1
            })
            .to_be_bytes(),
        );
        entry.extend_from_slice(&((position + 1) as u16).to_be_bytes());
        entry.push(1); // cue type: a point rather than a loop
        entry.push(0);
        entry.extend_from_slice(&1000u16.to_be_bytes()); // constant
        entry.extend_from_slice(&time_ms.to_be_bytes());
        entry.extend_from_slice(&0xffff_ffffu32.to_be_bytes()); // loop end, unset
        for _ in 0..4 {
            entry.extend_from_slice(&0u32.to_be_bytes());
        }
        // A cue entry is fixed at 0x38 bytes and declares a 0x1c header, which
        // covers the fields up to the sort keys.
        body.extend(section(b"PCPT", 0x1c, &entry));
    }

    section(b"PCOB", 0x18, &body)
}

/// `PWAV`: the 400-column preview, one byte per column.
fn waveform_preview(track: &Track) -> Vec<u8> {
    let mut body = Vec::with_capacity(8 + track.preview.len());
    body.extend_from_slice(&(track.preview.len() as u32).to_be_bytes());
    body.extend_from_slice(&0x0010_0000u32.to_be_bytes());
    body.extend_from_slice(&track.preview.columns);
    section(b"PWAV", 0x14, &body)
}

/// `PWV2`: the same shape at a quarter of the width, drawn on the track list.
///
/// Only the height matters here, in the low four bits, so the columns are
/// re-quantised rather than reused.
fn tiny_waveform_preview(track: &Track) -> Vec<u8> {
    let source = &track.preview;
    let content: Vec<u8> = (0..TINY_PREVIEW_COLUMNS)
        .map(|column| {
            let from = column * source.len() / TINY_PREVIEW_COLUMNS;
            let to = ((column + 1) * source.len() / TINY_PREVIEW_COLUMNS).max(from + 1);
            let peak = (from..to).map(|i| source.height(i)).max().unwrap_or(0);
            peak / 2 // 0..=31 becomes 0..=15
        })
        .collect();

    let mut body = Vec::with_capacity(8 + content.len());
    body.extend_from_slice(&(content.len() as u32).to_be_bytes());
    body.extend_from_slice(&0x0010_0000u32.to_be_bytes());
    body.extend_from_slice(&content);
    section(b"PWV2", 0x14, &body)
}

/// `PWV3`: the scrolling waveform, 150 columns per second.
fn waveform_detail(track: &Track) -> Vec<u8> {
    let mut body = Vec::with_capacity(12 + track.detail.len());
    body.extend_from_slice(&1u32.to_be_bytes()); // bytes per column
    body.extend_from_slice(&(track.detail.len() as u32).to_be_bytes());
    // Constant in every file that has been examined.
    body.extend_from_slice(&0x0096_0000u32.to_be_bytes());
    body.extend_from_slice(&track.detail.columns);
    section(b"PWV3", 0x18, &body)
}
