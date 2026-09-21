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

/// A track fixture. `key` is Camelot and `notes` the same key written out; the
/// two have to agree, since the database stores one and the report the other.
fn track(name: &str, bpm: f64, key: &str, notes: &str) -> Track_ {
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
        key_name: notes.to_string(),
        key_camelot: key.to_string(),
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
        preview: flat_waveform(1.0, 1200),
        detail: flat_waveform(150.0, 6000),
    }
}

fn collection() -> Collection {
    let tracks = vec![
        track("First Track (Extended Mix)", 138.0, "11A", "F# minor"),
        track("Second Track", 174.5, "8A", "A minor"),
        // Non-ASCII, because those strings take the other encoding and the
        // alignment rule that goes with it.
        track("Tredje Spår (Förlängd)", 128.0, "11A", "F# minor"),
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

    // Two distinct keys across the three tracks, stored the way a player shows
    // them.
    let keys: Vec<Key> = read(&bytes);
    assert_eq!(keys.len(), 2);
    field(&keys[0], "11A");

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
                "8B",
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

/// The colour table as rekordbox wrote it, which is the one table this crate
/// fills entirely from constants.
///
/// Both its pages come out of a real export: the index page that opens every
/// table, and the page holding the eight colour rows. Nothing about a library
/// changes either, so a byte comparison is available here and nowhere else, and
/// it covers the whole page layer rather than the colours alone: the flags that
/// mark an index page, the free and used counts, the data header, and the row
/// group with its bitmask written twice.
///
/// `rekordcrate` cannot catch any of that. It reads those fields as opaque
/// numbers and hands back whatever it read, so a database it round-trips
/// happily is still one a player may refuse.
mod against_a_real_export {
    use super::*;
    use rekordbox::pdb::PAGE_SIZE;

    /// Extracted from `PIONEER/rekordbox/export.pdb` of a 135-track export.
    /// The words saying where a page sat in that file are zeroed, because they
    /// are the only ones that depend on the library around it.
    const REAL_COLOUR_PAGES: &[u8] = include_bytes!("pages/colors.bin");

    /// Zero the page index, the next-page pointer and the sequence number, and
    /// on an index page the two places it repeats them.
    fn without_its_position(page: &[u8]) -> Vec<u8> {
        let mut page = page.to_vec();
        let is_index = page[27] & 0x40 != 0;
        let mut zero = |at: usize| page[at..at + 4].copy_from_slice(&0u32.to_le_bytes());
        zero(0x04);
        zero(0x0c);
        zero(0x10);
        if is_index {
            zero(0x28);
            zero(0x2c);
        }
        page
    }

    /// Every page of one table, in chain order.
    fn pages_of(database: &[u8], page_type: u32) -> Vec<Vec<u8>> {
        let word = |at: usize| u32::from_le_bytes(database[at..at + 4].try_into().unwrap());
        let tables = word(0x08) as usize;
        let mut out = Vec::new();
        for table in 0..tables {
            let entry = 0x1c + table * 16;
            if word(entry) != page_type {
                continue;
            }
            for page in word(entry + 8)..=word(entry + 12) {
                let at = page as usize * PAGE_SIZE;
                let bytes = &database[at..at + PAGE_SIZE];
                if word(at + 8) == page_type {
                    out.push(without_its_position(bytes));
                }
            }
        }
        out
    }

    #[test]
    fn the_colour_table_is_byte_for_byte_the_one_rekordbox_writes() {
        let written = write_database(&collection());
        let ours = pages_of(&written, 6);
        let real: Vec<Vec<u8>> = REAL_COLOUR_PAGES
            .chunks(PAGE_SIZE)
            .map(|page| page.to_vec())
            .collect();

        assert_eq!(
            ours.len(),
            real.len(),
            "the colour table is {} pages and rekordbox writes {}",
            ours.len(),
            real.len()
        );
        for (index, (ours, real)) in ours.iter().zip(&real).enumerate() {
            let differing: Vec<String> = ours
                .iter()
                .zip(real.iter())
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .map(|(at, (a, b))| format!("0x{at:03x}: wrote 0x{a:02x}, rekordbox 0x{b:02x}"))
                .collect();
            assert!(
                differing.is_empty(),
                "colour page {index} differs from the one rekordbox writes at {} bytes:\n  {}",
                differing.len(),
                differing.join("\n  ")
            );
        }
    }

    #[test]
    fn every_table_rekordbox_writes_is_present_and_numbered_without_a_gap() {
        let written = write_database(&collection());
        let word = |at: usize| u32::from_le_bytes(written[at..at + 4].try_into().unwrap());
        assert_eq!(word(0x08), 20, "a real export lists twenty tables");
        for table in 0..20u32 {
            let entry = 0x1c + table as usize * 16;
            assert_eq!(
                word(entry),
                table,
                "table {table} of the header list should be page type {table}"
            );
        }
    }

    /// The three tables whose rows rekordbox fills from its own menu rather
    /// than from a library, carrying the counts every export examined carries.
    #[test]
    fn the_browse_menu_tables_carry_the_rows_a_player_reads() {
        let written = write_database(&collection());
        for (page_type, expected) in [(16u32, 27usize), (17, 22), (18, 17)] {
            let rows: usize = pages_of(&written, page_type)
                .iter()
                .map(|page| {
                    (u32::from_le_bytes([page[24], page[25], page[26], 0]) & 0x1fff) as usize
                })
                .sum();
            assert_eq!(
                rows, expected,
                "page type {page_type} holds {rows} rows and rekordbox writes {expected}"
            );
        }
    }
}
