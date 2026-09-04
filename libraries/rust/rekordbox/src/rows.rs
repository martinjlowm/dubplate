//! Encoding one row of each table this exporter writes.
//!
//! Every layout here is little-endian and fixed-width except for the strings,
//! which sit after the fixed part and are reached through an array of offsets
//! relative to the start of the row.
//!
//! Field names follow the community analysis at
//! <https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/exports.html>.
//! Fields named `unknown` there are written with the values rekordbox writes;
//! they are not padding, and a player may well read them.

use crate::string;
use collection::Format;

/// Where `index_shift` sits in the rows that carry one. The page layer fills it
/// in, because its value is derived from the row's position in its page.
pub const INDEX_SHIFT_AT: usize = 2;

/// A track row.
///
/// The fixed part is 0x5C bytes, then an array of twenty-one string offsets,
/// then the strings themselves.
#[allow(clippy::too_many_arguments)]
pub struct TrackRow {
    pub id: u32,
    pub artist_id: u32,
    pub album_id: u32,
    pub genre_id: u32,
    pub key_id: u32,
    pub sample_rate: u32,
    pub sample_depth: u16,
    pub bitrate_kbps: u32,
    pub file_size: u32,
    pub tempo_centi_bpm: u32,
    pub duration_seconds: u16,
    pub format: Format,
    pub title: String,
    pub comment: String,
    pub file_name: String,
    pub file_path: String,
    pub analyze_path: String,
    pub date_added: String,
    pub analyze_date: String,
}

impl TrackRow {
    pub fn encode(&self) -> Vec<u8> {
        let mut row = Vec::with_capacity(256);
        // Subtype 0x24: the 0x04 bit is what makes the string offsets u16
        // rather than u8, which a track row needs because its strings run well
        // past 255 bytes.
        row.extend_from_slice(&0x0024u16.to_le_bytes());
        row.extend_from_slice(&0u16.to_le_bytes()); // index_shift, filled in later
        row.extend_from_slice(&0x000c_0700u32.to_le_bytes()); // bitmask
        row.extend_from_slice(&self.sample_rate.to_le_bytes());
        row.extend_from_slice(&0u32.to_le_bytes()); // composer
        row.extend_from_slice(&self.file_size.to_le_bytes());
        row.extend_from_slice(&0u32.to_le_bytes()); // unknown2
        row.extend_from_slice(&0u16.to_le_bytes()); // unknown3
        row.extend_from_slice(&0u16.to_le_bytes()); // unknown4
        row.extend_from_slice(&0u32.to_le_bytes()); // artwork
        row.extend_from_slice(&self.key_id.to_le_bytes());
        row.extend_from_slice(&0u32.to_le_bytes()); // original artist
        row.extend_from_slice(&0u32.to_le_bytes()); // label
        row.extend_from_slice(&0u32.to_le_bytes()); // remixer
        row.extend_from_slice(&self.bitrate_kbps.to_le_bytes());
        row.extend_from_slice(&0u32.to_le_bytes()); // track number
        row.extend_from_slice(&self.tempo_centi_bpm.to_le_bytes());
        row.extend_from_slice(&self.genre_id.to_le_bytes());
        row.extend_from_slice(&self.album_id.to_le_bytes());
        row.extend_from_slice(&self.artist_id.to_le_bytes());
        row.extend_from_slice(&self.id.to_le_bytes());
        row.extend_from_slice(&0u16.to_le_bytes()); // disc number
        row.extend_from_slice(&0u16.to_le_bytes()); // play count
        row.extend_from_slice(&0u16.to_le_bytes()); // year
        row.extend_from_slice(&self.sample_depth.to_le_bytes());
        row.extend_from_slice(&self.duration_seconds.to_le_bytes());
        row.extend_from_slice(&0x0029u16.to_le_bytes()); // unknown5
        row.push(0); // colour
        row.push(0); // rating
        row.extend_from_slice(&file_type(self.format).to_le_bytes());
        debug_assert_eq!(
            row.len(),
            0x5C,
            "the fixed part of a track row is 0x5C bytes"
        );

        let strings = [
            "",                 // ISRC
            "",                 // lyricist
            "1",                // increments when rekordbox re-exports a track
            "3",                // unknown, a number in every file we have seen
            "",                 // unknown
            "",                 // message
            "",                 // publish track information
            "",                 // autoload hotcues, "ON" or empty
            "",                 // unknown
            "",                 // unknown
            &self.date_added,   //
            "",                 // release date
            "",                 // mix name
            "",                 // unknown
            &self.analyze_path, //
            &self.analyze_date, //
            &self.comment,      //
            &self.title,        //
            "",                 // unknown
            &self.file_name,    //
            &self.file_path,    //
        ];
        append_offset_array(&mut row, &strings, OffsetWidth::U16);
        row
    }
}

/// A row that is an id and a name, which covers genres, labels and keys.
pub fn named(id: u32, name: &str, duplicate_id: bool) -> Vec<u8> {
    let mut row = Vec::new();
    row.extend_from_slice(&id.to_le_bytes());
    // A key row carries its id twice. Nobody knows why, and rekordbox does it.
    if duplicate_id {
        row.extend_from_slice(&id.to_le_bytes());
    }
    row.extend_from_slice(&string::encode(name));
    row
}

/// An artist row: eight fixed bytes, then a one-entry offset array.
pub fn artist(id: u32, name: &str) -> Vec<u8> {
    let mut row = Vec::new();
    // Subtype 0x60 keeps the offset a single byte, which is all the one string
    // in this row can need: it always starts ten bytes in.
    row.extend_from_slice(&0x0060u16.to_le_bytes());
    row.extend_from_slice(&0u16.to_le_bytes()); // index_shift, filled in later
    row.extend_from_slice(&id.to_le_bytes());
    append_offset_array(&mut row, &[name], OffsetWidth::U8);
    row
}

/// An album row.
pub fn album(id: u32, artist_id: u32, name: &str) -> Vec<u8> {
    let mut row = Vec::new();
    row.extend_from_slice(&0x0080u16.to_le_bytes()); // subtype
    row.extend_from_slice(&0u16.to_le_bytes()); // index_shift, filled in later
    row.extend_from_slice(&0u32.to_le_bytes()); // unknown
    row.extend_from_slice(&artist_id.to_le_bytes());
    row.extend_from_slice(&id.to_le_bytes());
    row.extend_from_slice(&0u32.to_le_bytes()); // unknown
    append_offset_array(&mut row, &[name], OffsetWidth::U8);
    row
}

/// A colour row. Written even though nothing here assigns colours, because a
/// player shows the colour menu whether or not any track uses it.
pub fn colour(index: u8, name: &str) -> Vec<u8> {
    let mut row = Vec::new();
    row.extend_from_slice(&0u32.to_le_bytes()); // unknown
    row.push(0); // unknown
    row.push(index);
    row.extend_from_slice(&0u16.to_le_bytes()); // unknown
    row.extend_from_slice(&string::encode(name));
    row
}

/// A node of the playlist tree: either a folder or a playlist.
pub fn playlist_node(
    id: u32,
    parent_id: u32,
    sort_order: u32,
    is_folder: bool,
    name: &str,
) -> Vec<u8> {
    let mut row = Vec::new();
    row.extend_from_slice(&parent_id.to_le_bytes());
    row.extend_from_slice(&0u32.to_le_bytes()); // unknown
    row.extend_from_slice(&sort_order.to_le_bytes());
    row.extend_from_slice(&id.to_le_bytes());
    // Non-zero means a folder. The field name says so and the reference parser
    // reads it that way, against a stale comment in its own source claiming the
    // opposite.
    row.extend_from_slice(&u32::from(is_folder).to_le_bytes());
    row.extend_from_slice(&string::encode(name));
    row
}

/// One track in one playlist, at one position.
pub fn playlist_entry(entry_index: u32, track_id: u32, playlist_id: u32) -> Vec<u8> {
    let mut row = Vec::new();
    row.extend_from_slice(&entry_index.to_le_bytes());
    row.extend_from_slice(&track_id.to_le_bytes());
    row.extend_from_slice(&playlist_id.to_le_bytes());
    row
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum OffsetWidth {
    U8,
    U16,
}

/// Append the offset array and the strings it points at.
///
/// The array starts with the constant 3, then one offset per string measured
/// from the start of the row, then the encoded strings. A UTF-16 string is
/// padded to a four-byte boundary first, because the parser seeks straight to
/// the offset and reads `u16` pairs from there.
fn append_offset_array(row: &mut Vec<u8>, strings: &[&str], width: OffsetWidth) {
    let magic_and_offsets = match width {
        OffsetWidth::U8 => 1 + strings.len(),
        OffsetWidth::U16 => 2 + strings.len() * 2,
    };
    let body_start = row.len() + magic_and_offsets;

    let mut bodies = Vec::new();
    let mut offsets = Vec::with_capacity(strings.len());
    for text in strings {
        let alignment = string::alignment(text);
        let padding =
            (body_start + bodies.len()).next_multiple_of(alignment) - (body_start + bodies.len());
        bodies.resize(bodies.len() + padding, 0u8);
        offsets.push(body_start + bodies.len());
        bodies.extend_from_slice(&string::encode(text));
    }

    match width {
        OffsetWidth::U8 => {
            row.push(0x03);
            for offset in offsets {
                row.push(u8::try_from(offset).expect("a one-byte offset array outgrew a byte"));
            }
        }
        OffsetWidth::U16 => {
            row.extend_from_slice(&0x0003u16.to_le_bytes());
            for offset in offsets {
                row.extend_from_slice(
                    &u16::try_from(offset)
                        .expect("a row outgrew the sixteen-bit offsets it stores")
                        .to_le_bytes(),
                );
            }
        }
    }
    row.extend_from_slice(&bodies);
}

/// The file type number a player reads to decide which decoder to use.
fn file_type(format: Format) -> u16 {
    match format {
        Format::Mp3 => 0x1,
        Format::Flac => 0x5,
        Format::Wav => 0xb,
        Format::Aiff => 0xc,
    }
}
