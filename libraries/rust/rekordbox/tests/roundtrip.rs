//! Everything this crate writes, read back by the reference parser.
//!
//! `rekordcrate` is an independent implementation of the same format analysis,
//! written to read what rekordbox produces. If it can parse a database this
//! crate wrote and hand back the values that went in, the layout is right. It
//! is the strongest check available without a CDJ on the desk, and it is why
//! that crate is a dev-dependency rather than a dependency: its row types
//! cannot be constructed from outside it, so it can verify but not write.

use collection::{Beat, Collection, Cue, CueKind, Format, Playlist, Track as Track_};
use fallible_iterator::FallibleIterator;
use rekordcrate::pdb::io::Database;
use rekordcrate::pdb::{
    Artist, Color, DatabaseType, Genre, Key, PlaylistEntry, PlaylistTreeNode, Track,
};
use std::io::Cursor;
use std::path::PathBuf;
use waveform::Waveform;

fn track(name: &str, bpm: f64, key: &str) -> Track_ {
    let beats = (0..64)
        .map(|index| Beat {
            time_seconds: index as f64 * 60.0 / bpm,
            bpm,
            number_in_bar: (index % 4) as u8 + 1,
        })
        .collect();
    Track_ {
        source: PathBuf::from(format!("/tmp/{name}.flac")),
        device_path: format!("/Contents/{name}.flac"),
        file_name: format!("{name}.flac"),
        title: name.to_string(),
        artist: "Test Artist".to_string(),
        album: None,
        genre: Some("Trance".to_string()),
        comment: String::new(),
        bpm,
        key_name: key.to_string(),
        key_camelot: "8A".to_string(),
        duration_seconds: 431.0,
        sample_rate: 44100,
        bit_depth: 16,
        bitrate_kbps: 1411,
        file_size: 76_000_000,
        format: Format::Flac,
        beats,
        cues: vec![Cue {
            kind: CueKind::Memory,
            time_seconds: 0.5,
            number: 1,
            comment: String::new(),
        }],
        preview: Waveform {
            columns_per_second: 1.0,
            columns: vec![0x2a; 400],
        },
        detail: Waveform {
            columns_per_second: 150.0,
            columns: vec![0x11; 6000],
        },
    }
}

fn collection() -> Collection {
    let tracks = vec![
        track("First Track (Extended Mix)", 138.0, "F# minor"),
        track("Second Track", 174.5, "A minor"),
        // Non-ASCII, because those strings take the other encoding and the
        // alignment rule that goes with it.
        track("Tredje Spår (Förlängd)", 128.0, "F# minor"),
    ];
    let playlists = vec![Playlist {
        name: "All tracks".to_string(),
        tracks: vec![0, 1, 2],
    }];
    Collection { tracks, playlists }
}

fn write_database(collection: &Collection) -> Vec<u8> {
    let options = rekordbox::Options {
        date: "2026-09-03".to_string(),
    };
    let mut bytes = Vec::new();
    rekordbox::build(collection, &options)
        .write(&mut bytes)
        .expect("writing the database");
    bytes
}

/// Read a database back with the reference parser and collect one table.
///
/// `Database::iter_rows` is the parser's own read path: it walks the header,
/// follows each table's page chain and decodes the rows. Anything malformed
/// fails here rather than being papered over.
fn read<RowT>(bytes: &[u8]) -> Vec<RowT>
where
    RowT: rekordcrate::pdb::RowVariant + Clone + 'static,
{
    let mut database =
        Database::open_non_persistent(Cursor::new(bytes.to_vec()), DatabaseType::Plain)
            .expect("the database must open");
    let mut rows = Vec::new();
    database
        .iter_rows::<RowT>()
        .expect("the table must be readable")
        .for_each(|row| {
            rows.push(row.clone());
            Ok(())
        })
        .expect("every row must parse");
    rows
}

/// Assert that a row prints a field with this value.
///
/// Used for the fields the parser keeps private, which are most of the numeric
/// ones. A field printed as `tempo: 13800` was decoded from the bytes this
/// crate wrote, which is the claim being tested.
#[track_caller]
fn field<T: std::fmt::Debug>(row: &T, printed: &str) {
    let text = format!("{row:?}");
    assert!(
        text.contains(printed),
        "expected to find {printed:?} in the parsed row:\n{text}"
    );
}

#[test]
fn the_reference_parser_reads_back_every_track() {
    let bytes = write_database(&collection());
    assert_eq!(bytes.len() % 4096, 0, "the file is whole pages");

    let tracks: Vec<Track> = read(&bytes);
    assert_eq!(tracks.len(), 3);

    let first = tracks
        .iter()
        .find(|track| track.id.0 == 1)
        .expect("track 1");
    assert_eq!(
        first.offsets.title.to_string(),
        "First Track (Extended Mix)"
    );
    assert_eq!(
        first.offsets.file_path.to_string(),
        "/Contents/First Track (Extended Mix).flac"
    );
    field(first, "tempo: 13800");
    field(first, "duration: 431");
    field(first, "sample_rate: 44100");
    field(first, "file_size: 76000000");
    field(first, "bitrate: 1411");
    field(first, "sample_depth: 16");
    field(first, "file_type: Flac");
    field(first, "/PIONEER/USBANLZ/P000/00000001/ANLZ0000.DAT");
    field(first, "2026-09-03");

    let second = tracks
        .iter()
        .find(|track| track.id.0 == 2)
        .expect("track 2");
    field(second, "tempo: 17450");

    // A non-ASCII title takes the UTF-16 encoding and the four-byte alignment
    // that goes with it. Coming back unchanged means both were right.
    let third = tracks
        .iter()
        .find(|track| track.id.0 == 3)
        .expect("track 3");
    assert_eq!(third.offsets.title.to_string(), "Tredje Spår (Förlängd)");
    assert_eq!(
        third.offsets.file_path.to_string(),
        "/Contents/Tredje Spår (Förlängd).flac"
    );
}

#[test]
fn artists_genres_and_keys_are_shared_between_tracks() {
    let bytes = write_database(&collection());

    let artists: Vec<Artist> = read(&bytes);
    assert_eq!(
        artists.len(),
        1,
        "one artist, referenced by all three tracks"
    );
    assert_eq!(artists[0].offsets.name.to_string(), "Test Artist");
    assert_eq!(artists[0].id.0, 1);

    let genres: Vec<Genre> = read(&bytes);
    assert_eq!(genres.len(), 1);
    field(&genres[0], "Trance");

    // Two distinct key names across the three tracks.
    let keys: Vec<Key> = read(&bytes);
    assert_eq!(keys.len(), 2);
    field(&keys[0], "F# minor");

    let colours: Vec<Color> = read(&bytes);
    assert_eq!(colours.len(), 8);

    for track in read::<Track>(&bytes) {
        assert_eq!(track.artist_id.0, 1, "every track points at the one artist");
        field(&track, "genre_id: GenreId(1)");
    }
}

#[test]
fn a_playlist_lists_its_tracks_in_order() {
    let bytes = write_database(&collection());

    let nodes: Vec<PlaylistTreeNode> = read(&bytes);
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].name.to_string(), "All tracks");
    assert_eq!(nodes[0].parent_id.0, 0, "a top-level playlist");
    assert!(!nodes[0].is_folder(), "a playlist, not a folder");

    let mut entries: Vec<PlaylistEntry> = read(&bytes);
    entries.sort_by_key(|entry| entry.entry_index);
    assert_eq!(entries.len(), 3);
    for (position, entry) in entries.iter().enumerate() {
        assert_eq!(entry.entry_index, position as u32 + 1);
        assert_eq!(entry.track_id.0, position as u32 + 1);
        assert_eq!(entry.playlist_id.0, 1);
    }
}

#[test]
fn a_collection_larger_than_one_page_still_reads_back() {
    // A track row runs to roughly 300 bytes, so a page holds about a dozen.
    // Fifty tracks crosses several pages and several row groups, which is where
    // the offsets and the presence bitmask have to agree.
    let tracks: Vec<Track_> = (0..50)
        .map(|index| {
            track(
                &format!("Track number {index}"),
                120.0 + index as f64,
                "C major",
            )
        })
        .collect();
    let collection = Collection {
        playlists: vec![Playlist {
            name: "Everything".to_string(),
            tracks: (0..tracks.len()).collect(),
        }],
        tracks,
    };

    let bytes = write_database(&collection);
    let parsed: Vec<Track> = read(&bytes);
    assert_eq!(
        parsed.len(),
        50,
        "every row survived being split over pages"
    );

    // Each row's tempo has to match the title it was written with, or rows have
    // been crossed between pages.
    for row in &parsed {
        let index = row.id.0 - 1;
        assert_eq!(
            row.offsets.title.to_string(),
            format!("Track number {index}")
        );
        field(row, &format!("tempo: {}", 12000 + index * 100));
    }

    assert_eq!(read::<PlaylistEntry>(&bytes).len(), 50);
}
