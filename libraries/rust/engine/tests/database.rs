//! A whole device database, written and then read back with SQL.
//!
//! There is no reference parser for this format the way rekordcrate is one for
//! rekordbox, so the check is the database itself: the schema's own constraints
//! and triggers have to accept every row, and the values have to come back.

use collection::{Beat, Collection, Format, Playlist, Track};
use rusqlite::Connection;
use std::path::PathBuf;
use waveform::{Column, Waveform};

fn track(name: &str, bpm: f64, camelot: &str) -> Track {
    let beats = (0..256)
        .map(|index| Beat {
            time_seconds: 0.5 + index as f64 * 60.0 / bpm,
            bpm,
            number_in_bar: (index % 4) as u8 + 1,
        })
        .collect();
    let columns: Vec<Column> = (0..1200)
        .map(|index| Column {
            height: (index % 32) as u8,
            low: (index % 8) as u8,
            mid: ((index / 3) % 8) as u8,
            high: ((index / 5) % 8) as u8,
        })
        .collect();
    Track {
        source: PathBuf::from(format!("/tmp/{name}.flac")),
        device_path: format!("/Contents/{name}.flac"),
        file_name: format!("{name}.flac"),
        title: name.to_string(),
        artist: "Test Artist".to_string(),
        album: None,
        genre: Some("Hard Dance".to_string()),
        comment: String::new(),
        bpm,
        key_name: "F minor".to_string(),
        key_camelot: camelot.to_string(),
        duration_seconds: 300.0,
        sample_rate: 44100,
        bit_depth: 16,
        bitrate_kbps: 1411,
        file_size: 52_000_000,
        format: Format::Flac,
        trim_seconds: 0.0,
        seek_table: None,
        beats,
        cues: Vec::new(),
        sections: Vec::new(),
        preview: Waveform::from_columns(4.0, &columns),
        detail: Waveform::from_columns(150.0, &columns),
    }
}

fn write(directory: &std::path::Path) -> Collection {
    let collection = Collection {
        tracks: vec![
            track("First", 150.0, "4A"),
            track("Second", 174.5, "8B"),
            track("Third", 128.0, "12A"),
        ],
        playlists: vec![Playlist {
            name: "All tracks".to_string(),
            tracks: vec![0, 1, 2],
        }],
    };
    engine::write_device(
        directory,
        &collection,
        &engine::Options {
            date: "2026-09-04".to_string(),
        },
    )
    .expect("writing the database");
    collection
}

fn open(directory: &std::path::Path) -> Connection {
    Connection::open(directory.join(engine::DATABASE_PATH)).expect("opening the database")
}

#[test]
fn a_device_database_holds_every_track_with_its_analysis() {
    let directory = tempdir("engine-tracks");
    let collection = write(&directory);
    let connection = open(&directory);

    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM Track", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, collection.tracks.len() as i64);

    let (title, path, bpm, key, length, analysed): (String, String, f64, i32, i64, bool) =
        connection
            .query_row(
                "SELECT title, path, bpmAnalyzed, key, length, isAnalyzed FROM Track WHERE id = 1",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .unwrap();
    assert_eq!(title, "First");
    // Relative to the Engine Library directory, which is where the player
    // resolves it from.
    assert_eq!(path, "../Contents/First.flac");
    assert_eq!(bpm, 150.0);
    assert_eq!(key, 17, "4A is F minor, which Engine numbers 17");
    assert_eq!(length, 300);
    assert!(analysed);

    // Every analysis column has to be there, or the player treats the track as
    // unanalysed and grinds through it on load.
    type Blobs = (Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>, Vec<u8>);
    let (track_data, beat_data, quick_cues, loops, overview): Blobs = connection
        .query_row(
            "SELECT trackData, beatData, quickCues, loops, overviewWaveFormData \
             FROM Track WHERE id = 1",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .unwrap();
    for (name, blob) in [
        ("trackData", &track_data),
        ("beatData", &beat_data),
        ("quickCues", &quick_cues),
        ("overviewWaveFormData", &overview),
    ] {
        assert!(blob.len() > 4, "{name} is empty");
        let declared = u32::from_be_bytes(blob[..4].try_into().unwrap());
        assert!(declared > 0, "{name} declares no uncompressed length");
    }
    assert_eq!(
        loops.len(),
        8 + 8 * 23,
        "eight empty loop slots, uncompressed"
    );
}

#[test]
fn the_performance_data_view_reads_the_analysis_back() {
    let directory = tempdir("engine-performance");
    write(&directory);
    let connection = open(&directory);

    // A player reads the analysis through this view rather than from Track.
    let rows: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM PerformanceData WHERE trackData IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rows, 3);
}

#[test]
fn a_playlist_is_a_linked_list_ending_in_zero() {
    let directory = tempdir("engine-playlist");
    write(&directory);
    let connection = open(&directory);

    let (title, persisted): (String, bool) = connection
        .query_row(
            "SELECT title, isPersisted FROM Playlist WHERE id = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(title, "All tracks");
    assert!(persisted);

    let mut statement = connection
        .prepare("SELECT id, trackId, nextEntityId FROM PlaylistEntity ORDER BY id")
        .unwrap();
    let entries: Vec<(i64, i64, i64)> = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .unwrap()
        .map(|row| row.unwrap())
        .collect();
    assert_eq!(entries.len(), 3);

    // Walk the list the way a player does.
    let mut visited = Vec::new();
    let mut current = entries[0].0;
    while current != 0 {
        let entry = entries
            .iter()
            .find(|entry| entry.0 == current)
            .expect("a linked entry");
        visited.push(entry.1);
        current = entry.2;
    }
    assert_eq!(
        visited,
        vec![1, 2, 3],
        "the list walks every track in order"
    );
}

#[test]
fn the_information_row_names_the_schema_the_database_is() {
    let directory = tempdir("engine-information");
    write(&directory);
    let connection = open(&directory);

    let (uuid, major, minor, patch): (String, i32, i32, i32) = connection
        .query_row(
            "SELECT uuid, schemaVersionMajor, schemaVersionMinor, schemaVersionPatch \
             FROM Information",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!((major, minor, patch), engine::schema::VERSION);
    assert_eq!(uuid.len(), 36, "a uuid in the usual shape");

    // The same collection has to produce the same identifier, or a rebuild is a
    // different library to the player and to Nix.
    let second = tempdir("engine-information-again");
    write(&second);
    let repeated: String = open(&second)
        .query_row("SELECT uuid FROM Information", [], |row| row.get(0))
        .unwrap();
    assert_eq!(uuid, repeated);
}

fn tempdir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("dubplate-{name}"));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn the_database_can_be_built_without_a_filesystem() {
    // What a browser calls. `write_device` is this plus a write, so the bytes
    // here are the bytes on a stick.
    let collection = Collection {
        tracks: vec![track("Only", 126.0, "5A")],
        playlists: vec![Playlist {
            name: "All tracks".to_string(),
            tracks: vec![0],
        }],
    };
    let bytes = engine::build(
        &collection,
        &engine::Options {
            date: "2026-09-04".to_string(),
        },
    )
    .expect("building the database in memory");

    assert_eq!(
        &bytes[..15],
        b"SQLite format 3",
        "the bytes have to be a database, not a serialisation of our own"
    );

    // And SQLite has to agree, including the triggers a player checks for and
    // the analysis the view's INSTEAD OF trigger moved into the track row.
    let path = tempdir("engine-in-memory").join("m.db");
    std::fs::write(&path, &bytes).unwrap();
    let connection = Connection::open(&path).expect("reopening what build wrote");
    let triggers: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE type = 'trigger'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(triggers, 24, "the schema is what a player looks for");
    let (bpm, beat_data): (i64, Vec<u8>) = connection
        .query_row("SELECT bpm, beatData FROM Track WHERE id = 1", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    assert_eq!(bpm, 126);
    assert!(
        !beat_data.is_empty(),
        "the beat grid reached the row through the PerformanceData trigger"
    );
    let integrity: String = connection
        .query_row("PRAGMA integrity_check", [], |row| row.get(0))
        .unwrap();
    assert_eq!(integrity, "ok");
}
