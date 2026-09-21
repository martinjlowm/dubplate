//! Encoding one row of each table this exporter writes.
//!
//! Every layout here is little-endian and fixed-width except for the strings,
//! which sit after the fixed part and are reached through an array of offsets
//! relative to the start of the row. Each fixed part is a struct with its field
//! widths declared, so it can be read against the format analysis field by
//! field; the offset array is computed rather than declared, because where a
//! string lands depends on how long the ones before it were.
//!
//! Field names follow the community analysis at
//! <https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/exports.html>.
//! Fields named `unknown` there are written with the values rekordbox writes;
//! they are not padding, and a player may well read them.

use crate::string;
use collection::Format;
use deku::prelude::*;

/// Where `index_shift` sits in the rows that carry one. The page layer fills it
/// in, because its value is derived from the row's position in its page.
pub const INDEX_SHIFT_AT: usize = 2;

/// Every layout here is fixed, so a write cannot fail for anything a caller
/// could fix.
fn bytes(layout: &impl DekuContainerWrite) -> Vec<u8> {
    layout.to_bytes().expect("a fixed layout with no counts")
}

/// The fixed part of a track row, 0x5C bytes before the string offsets.
#[derive(DekuWrite)]
#[deku(endian = "little")]
struct TrackRowFixed {
    /// 0x24: the 0x04 bit is what makes the string offsets `u16` rather than
    /// `u8`, which a track row needs because its strings run well past 255
    /// bytes.
    subtype: u16,
    /// Filled in by the page layer once the row's position is known.
    index_shift: u16,
    bitmask: u32,
    sample_rate: u32,
    composer_id: u32,
    file_size: u32,
    unknown2: u32,
    unknown3: u16,
    unknown4: u16,
    artwork_id: u32,
    key_id: u32,
    original_artist_id: u32,
    label_id: u32,
    remixer_id: u32,
    bitrate_kbps: u32,
    track_number: u32,
    tempo_centi_bpm: u32,
    genre_id: u32,
    album_id: u32,
    artist_id: u32,
    id: u32,
    disc_number: u16,
    play_count: u16,
    year: u16,
    sample_depth: u16,
    duration_seconds: u16,
    unknown5: u16,
    colour: u8,
    rating: u8,
    /// What a player reads to decide which decoder to use.
    file_type: u16,
}
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
        let mut row = bytes(&TrackRowFixed {
            subtype: 0x0024,
            index_shift: 0,
            bitmask: 0x000c_0700,
            sample_rate: self.sample_rate,
            composer_id: 0,
            file_size: self.file_size,
            unknown2: 0,
            unknown3: 0,
            unknown4: 0,
            artwork_id: 0,
            key_id: self.key_id,
            original_artist_id: 0,
            label_id: 0,
            remixer_id: 0,
            bitrate_kbps: self.bitrate_kbps,
            track_number: 0,
            tempo_centi_bpm: self.tempo_centi_bpm,
            genre_id: self.genre_id,
            album_id: self.album_id,
            artist_id: self.artist_id,
            id: self.id,
            disc_number: 0,
            play_count: 0,
            year: 0,
            sample_depth: self.sample_depth,
            duration_seconds: self.duration_seconds,
            unknown5: 0x0029,
            colour: 0,
            rating: 0,
            file_type: file_type(self.format),
        });
        // A mistyped field width would still write a struct, just a shorter or
        // longer one, and the strings after it are placed by offsets counted
        // from here.
        debug_assert_eq!(
            row.len(),
            0x5C,
            "the fixed part of a track row is 0x5C bytes"
        );

        let strings = [
            "",                 // ISRC
            "",                 // lyricist
            "1",                // increments when rekordbox re-exports a track
            "2",                // unknown, this number in every file we have seen
            "",                 // unknown
            "",                 // message
            "",                 // publish track information
            "ON",               // autoload hotcues, "ON" in every export examined
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

/// An id on its own, which is how a genre, label or key row starts.
#[derive(DekuWrite)]
#[deku(endian = "little")]
struct RowId {
    id: u32,
}

/// A row that is an id and a name, which covers genres, labels and keys.
pub fn named(id: u32, name: &str, duplicate_id: bool) -> Vec<u8> {
    let mut row = bytes(&RowId { id });
    // A key row carries its id twice. Nobody knows why, and rekordbox does it.
    if duplicate_id {
        row.extend(bytes(&RowId { id }));
    }
    row.extend_from_slice(&string::encode(name));
    row
}

/// The fixed part of an artist row: eight bytes before the offset array.
#[derive(DekuWrite)]
#[deku(endian = "little")]
struct ArtistRowFixed {
    /// 0x60 keeps the offset a single byte, which is all the one string in this
    /// row can need: it always starts ten bytes in.
    subtype: u16,
    index_shift: u16,
    id: u32,
}

/// An artist row: eight fixed bytes, then a one-entry offset array.
pub fn artist(id: u32, name: &str) -> Vec<u8> {
    let mut row = bytes(&ArtistRowFixed {
        subtype: 0x0060,
        index_shift: 0,
        id,
    });
    append_offset_array(&mut row, &[name], OffsetWidth::U8);
    row
}

/// The fixed part of an album row.
#[derive(DekuWrite)]
#[deku(endian = "little")]
struct AlbumRowFixed {
    subtype: u16,
    index_shift: u16,
    unknown1: u32,
    artist_id: u32,
    id: u32,
    unknown2: u32,
}

/// An album row.
pub fn album(id: u32, artist_id: u32, name: &str) -> Vec<u8> {
    let mut row = bytes(&AlbumRowFixed {
        subtype: 0x0080,
        index_shift: 0,
        unknown1: 0,
        artist_id,
        id,
        unknown2: 0,
    });
    append_offset_array(&mut row, &[name], OffsetWidth::U8);
    row
}

/// The fixed part of a colour row.
///
/// Every real export writes the colour twice, once in the byte before the id
/// and once as the id itself. The byte is not padding: zeroing it is the one
/// thing that separated this row from the one rekordbox writes.
#[derive(DekuWrite)]
#[deku(endian = "little")]
struct ColourRowFixed {
    unknown1: u32,
    colour_again: u8,
    id: u16,
    unknown3: u8,
}

/// A colour row. Written even though nothing here assigns colours, because a
/// player shows the colour menu whether or not any track uses it.
pub fn colour(index: u8, name: &str) -> Vec<u8> {
    let mut row = bytes(&ColourRowFixed {
        unknown1: 0,
        colour_again: index,
        id: u16::from(index),
        unknown3: 0,
    });
    row.extend_from_slice(&string::encode(name));
    row
}

/// The fixed part of a playlist tree node.
#[derive(DekuWrite)]
#[deku(endian = "little")]
struct PlaylistNodeFixed {
    parent_id: u32,
    unknown: u32,
    sort_order: u32,
    id: u32,
    /// Non-zero means a folder. The field name says so and the reference parser
    /// reads it that way, against a stale comment in its own source claiming
    /// the opposite.
    is_folder: u32,
}

/// A node of the playlist tree: either a folder or a playlist.
pub fn playlist_node(
    id: u32,
    parent_id: u32,
    sort_order: u32,
    is_folder: bool,
    name: &str,
) -> Vec<u8> {
    let mut row = bytes(&PlaylistNodeFixed {
        parent_id,
        unknown: 0,
        sort_order,
        id,
        is_folder: u32::from(is_folder),
    });
    row.extend_from_slice(&string::encode(name));
    row
}

/// One track in one playlist, at one position.
#[derive(DekuWrite)]
#[deku(endian = "little")]
struct PlaylistEntryRow {
    entry_index: u32,
    track_id: u32,
    playlist_id: u32,
}

/// One track in one playlist, at one position.
pub fn playlist_entry(entry_index: u32, track_id: u32, playlist_id: u32) -> Vec<u8> {
    bytes(&PlaylistEntryRow {
        entry_index,
        track_id,
        playlist_id,
    })
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
