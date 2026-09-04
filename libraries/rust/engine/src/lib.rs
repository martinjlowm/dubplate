//! Writing the Denon half of a device: `Engine Library/Database2/m.db`.
//!
//! One SQLite database, schema 2.21.2, which is the Engine DJ 2.x and 3.x
//! layout. Everything a player needs about a track is a row in `Track`, with
//! the analysis in five binary columns beside the metadata.
//!
//! The schema is Denon's, reproduced here because a player checks it: a
//! database with a column missing or a trigger absent is not read. It was taken
//! from [libdjinterop](https://github.com/xsco/libdjinterop), which is the open
//! implementation of these formats and where each version's DDL is recorded.
//!
//! Like the rekordbox exporter, nothing here copies audio. The database stores
//! the path and whoever assembles the device puts the file there.

pub mod blob;
pub mod schema;

use blob::{BeatGridMarker, WaveformPoint};
use collection::{Collection, Sink, Track};
use rusqlite::{Connection, params};
#[cfg(not(target_family = "wasm"))]
use std::path::Path;

/// Where the database lives on the device.
pub const DATABASE_PATH: &str = "Engine Library/Database2/m.db";

/// Points in the whole-track overview waveform.
///
/// Engine's own analysis produces a few hundred; the player scales whatever it
/// finds to the width of its screen.
const OVERVIEW_POINTS: usize = 1024;

/// Hot cue and loop slots a player draws, filled or not.
const SLOTS: usize = 8;

pub struct Options {
    /// Date recorded against every track, as `YYYY-MM-DD`.
    pub date: String,
}

/// Write `Engine Library/` under `root` for this collection.
#[cfg(not(target_family = "wasm"))]
pub fn write_device(
    root: &Path,
    collection: &Collection,
    options: &Options,
) -> Result<(), rusqlite::Error> {
    write_device_to(
        &mut collection::sink::Directory::new(root),
        collection,
        options,
    )
}

/// Write `Engine Library/` into a sink for this collection.
///
/// The database is one file wherever it goes, so this is [`build`] plus a name.
/// It exists so that a caller assembling a device does not have to know that the
/// Engine half is a single path while the Pioneer half is a tree.
pub fn write_device_to(
    sink: &mut dyn Sink,
    collection: &Collection,
    options: &Options,
) -> Result<(), rusqlite::Error> {
    let database = build(collection, options)?;
    sink.file(DATABASE_PATH, &database)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

/// The database as bytes, built in memory.
///
/// This is what a browser can use: there is nowhere to put a path there, and
/// SQLite is the same SQLite either way, so the schema, the inserts and the
/// twenty-four triggers that do work on insert stay here rather than being
/// transcribed into whatever the caller is written in. `write_device` is this
/// plus a write.
pub fn build(collection: &Collection, options: &Options) -> Result<Vec<u8>, rusqlite::Error> {
    let mut connection = Connection::open_in_memory()?;
    schema::create(&connection)?;

    // Row zero of AlbumArt is "no artwork", and every track points at it. The
    // column carries a foreign key, so the row has to exist before any track
    // does even though it holds nothing.
    connection.execute(
        "INSERT INTO AlbumArt (id, hash, albumArt) VALUES (0, NULL, NULL)",
        [],
    )?;

    let uuid = library_uuid(collection);
    connection.execute(
        "INSERT INTO Information (uuid, schemaVersionMajor, schemaVersionMinor, \
         schemaVersionPatch, currentPlayedIndiciator, lastRekordBoxLibraryImportReadCounter) \
         VALUES (?1, ?2, ?3, ?4, 0, 0)",
        params![
            uuid,
            schema::VERSION.0,
            schema::VERSION.1,
            schema::VERSION.2
        ],
    )?;

    let transaction = connection.transaction()?;
    for (index, track) in collection.tracks.iter().enumerate() {
        insert_track(&transaction, index as i64 + 1, track, options)?;
    }

    for (index, playlist) in collection.playlists.iter().enumerate() {
        let list_id = index as i64 + 1;
        transaction.execute(
            "INSERT INTO Playlist (id, title, parentListId, isPersisted, nextListId, \
             lastEditTime, isExplicitlyExported) VALUES (?1, ?2, 0, 1, 0, ?3, 1)",
            params![list_id, playlist.name, format!("{} 00:00:00", options.date)],
        )?;

        // Entities are a linked list: each row names the next, and the last
        // names zero. A player walks it rather than sorting by id.
        let entity_base = list_id * 1_000_000;
        for (position, track_index) in playlist.tracks.iter().enumerate() {
            let entity_id = entity_base + position as i64 + 1;
            let next = if position + 1 == playlist.tracks.len() {
                0
            } else {
                entity_id + 1
            };
            transaction.execute(
                "INSERT INTO PlaylistEntity (id, listId, trackId, databaseUuid, nextEntityId, \
                 membershipReference) VALUES (?1, ?2, ?3, ?4, ?5, 0)",
                params![entity_id, list_id, *track_index as i64 + 1, uuid, next],
            )?;
        }
    }
    transaction.commit()?;

    // `serialize` hands back the pages SQLite would have written to a file, so
    // what a player reads is what the database engine produced rather than
    // anything assembled here.
    Ok(connection.serialize("main")?.to_vec())
}

fn insert_track(
    connection: &Connection,
    id: i64,
    track: &Track,
    options: &Options,
) -> Result<(), rusqlite::Error> {
    let sample_rate = f64::from(track.sample_rate);
    let samples = track.duration_seconds * sample_rate;

    let grid = beat_grid(track, sample_rate);
    let main_cue = track.first_beat_seconds() * sample_rate;
    let overview = overview_points(track);

    connection.execute(
        "INSERT INTO Track (id, playOrder, length, bpm, year, path, filename, bitrate, \
         bpmAnalyzed, albumArtId, fileBytes, title, artist, album, genre, comment, label, \
         composer, remixer, key, rating, albumArt, isPlayed, fileType, isAnalyzed, \
         dateCreated, dateAdded, isAvailable, isMetadataOfPackedTrackChanged, \
         isPerfomanceDataOfPackedTrackChanged, playedIndicator, isMetadataImported, \
         pdbImportKey, streamingSource, uri, isBeatGridLocked, trackData, \
         overviewWaveFormData, beatData, quickCues, loops, thirdPartySourceId, \
         streamingFlags, explicitLyrics, activeOnLoadLoops, lastEditTime) \
         VALUES (?1, NULL, ?2, ?3, NULL, ?4, ?5, ?6, ?7, 0, ?8, ?9, ?10, NULL, ?11, '', NULL, \
         NULL, NULL, ?12, 0, NULL, 0, ?13, 1, ?14, ?14, 1, 0, 0, 0, 1, 0, NULL, NULL, 0, \
         ?15, ?16, ?17, ?18, ?19, NULL, 0, 0, 0, ?20)",
        params![
            id,
            track.duration_rounded() as i64,
            track.bpm.round() as i64,
            device_relative_path(&track.device_path),
            track.file_name,
            i64::from(track.bitrate_kbps),
            track.bpm,
            track.file_size as i64,
            track.title,
            track.artist,
            track.genre,
            camelot_to_key(&track.key_camelot),
            track.format.extension(),
            format!("{} 00:00:00", options.date),
            blob::track_data(
                sample_rate,
                samples as i64,
                camelot_to_key(&track.key_camelot).unwrap_or(0),
                loudness(track),
            ),
            blob::overview_waveform(&overview, samples / OVERVIEW_POINTS.max(1) as f64),
            blob::beat_data(sample_rate, samples, &grid),
            blob::quick_cues(&vec![None; SLOTS], main_cue),
            blob::loops(SLOTS),
            format!("{} 00:00:00", options.date),
        ],
    )?;
    Ok(())
}

/// The grid Engine stores: two markers, the first four beats before the track
/// and the last one beat past its end.
///
/// The format keeps a marker every so often rather than every beat, and derives
/// the tempo between two of them. A constant tempo needs exactly two.
fn beat_grid(track: &Track, sample_rate: f64) -> Vec<BeatGridMarker> {
    let Some(first) = track.beats.first() else {
        return Vec::new();
    };
    let beats = track.beats.len() as i64;
    let beat_samples = 60.0 / track.bpm * sample_rate;
    let first_sample = first.time_seconds * sample_rate;

    vec![
        BeatGridMarker {
            sample_offset: first_sample - 4.0 * beat_samples,
            beat_number: -4,
            beats_to_next_marker: (beats + 4) as i32,
        },
        BeatGridMarker {
            sample_offset: first_sample + beats as f64 * beat_samples,
            beat_number: beats,
            beats_to_next_marker: 0,
        },
    ]
}

/// The whole-track waveform, one point per band.
fn overview_points(track: &Track) -> Vec<WaveformPoint> {
    waveform::resample(&track.preview, OVERVIEW_POINTS)
        .iter()
        .map(|column| {
            // Each band scaled by the column's height, since this format has no
            // separate height and draws the bands themselves.
            let level = |band: u8| {
                ((f64::from(band) / 7.0) * (f64::from(column.height) / 31.0) * 255.0).round() as u8
            };
            WaveformPoint {
                low: level(column.low),
                mid: level(column.mid),
                high: level(column.high),
            }
        })
        .collect()
}

/// Average loudness per band, which `trackData` carries as three numbers in
/// `0..1`.
fn loudness(track: &Track) -> (f64, f64, f64) {
    let mut totals = (0.0, 0.0, 0.0);
    let mut count = 0.0;
    for column in track.preview.columns() {
        let scale = f64::from(column.height) / 31.0;
        totals.0 += f64::from(column.low) / 7.0 * scale;
        totals.1 += f64::from(column.mid) / 7.0 * scale;
        totals.2 += f64::from(column.high) / 7.0 * scale;
        count += 1.0;
    }
    if count == 0.0 {
        return (0.0, 0.0, 0.0);
    }
    (totals.0 / count, totals.1 / count, totals.2 / count)
}

/// The path a track is stored at, relative to the Engine Library directory.
///
/// The database sits two levels down in `Engine Library/Database2`, and the
/// audio is at `/Contents` from the device root, so the stored path climbs out.
fn device_relative_path(device_path: &str) -> String {
    format!("..{device_path}")
}

/// Camelot notation to the key number Engine stores.
///
/// The numbering is the Camelot wheel read from 8B: even numbers are the major
/// keys, odd the minor, and each pair steps one place clockwise.
pub fn camelot_to_key(camelot: &str) -> Option<i32> {
    let (number, letter) = camelot.split_at(camelot.len().checked_sub(1)?);
    let number: i32 = number.parse().ok()?;
    if !(1..=12).contains(&number) {
        return None;
    }
    let minor = match letter {
        "A" => 1,
        "B" => 0,
        _ => return None,
    };
    Some(((number - 8).rem_euclid(12)) * 2 + minor)
}

/// An identifier for this library, derived from what is in it.
///
/// Engine wants a UUID here to tell one stick from another. Deriving it from
/// the track paths rather than drawing a random one keeps a rebuild of the same
/// collection identical, which is what makes the Nix output reproducible.
fn library_uuid(collection: &Collection) -> String {
    let mut hash: u128 = 0x6c72_9f8a_1d3e_5b47_0000_0000_0000_0001;
    for track in &collection.tracks {
        for byte in track.device_path.as_bytes() {
            hash = hash.wrapping_mul(0x0100_0000_0000_0000_0000_0000_0000_0013) ^ u128::from(*byte);
        }
    }
    let bytes = hash.to_be_bytes();
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-4{}-a{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[13..16],
        &hex[17..20],
        &hex[20..32]
    )
}

#[cfg(test)]
mod tests {
    use super::camelot_to_key;

    #[test]
    fn the_key_numbering_starts_at_c_major() {
        // The four corners of the mapping the format analysis documents.
        assert_eq!(camelot_to_key("8B"), Some(0)); // C major
        assert_eq!(camelot_to_key("8A"), Some(1)); // A minor
        assert_eq!(camelot_to_key("1B"), Some(10)); // B major
        assert_eq!(camelot_to_key("7A"), Some(23)); // D minor
        assert_eq!(camelot_to_key("12A"), Some(9)); // Db minor
        assert_eq!(camelot_to_key("13A"), None);
        assert_eq!(camelot_to_key("8C"), None);
    }
}
