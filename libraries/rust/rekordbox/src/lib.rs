//! Writing the rekordbox half of a USB stick: `export.pdb` and the per-track
//! analysis files a Pioneer player reads.
//!
//! Nothing here copies audio. The caller decides how files get onto the device,
//! and this writes the database that points at them, so the two never disagree
//! about a path: [`Track::device_path`](collection::Track::device_path) is the
//! single source of it.
//!
//! No open tool writes this database, and the format is known from the
//! community analysis at
//! <https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/>. The
//! writer is therefore verified against `rekordcrate`, the reference parser: the
//! tests here write a database and read it back with that crate. That proves
//! the file is well formed and that the values survive the trip. It does not
//! prove a player accepts it, and the fields whose purpose nobody has
//! established are written with the constants real exports carry.

pub mod anlz;
pub mod pdb;
pub mod rows;
pub mod string;

use collection::{Collection, Sink, Track};
use pdb::{Database, PageType};
use std::io;
#[cfg(not(target_family = "wasm"))]
use std::path::Path;

/// Where the database and the analysis files live on the device.
pub const DATABASE_PATH: &str = "PIONEER/rekordbox/export.pdb";
const ANALYSIS_ROOT: &str = "PIONEER/USBANLZ";

/// Options that would otherwise be guessed.
pub struct Options {
    /// The date written into every track's `date_added` and `analyze_date`, as
    /// `YYYY-MM-DD`.
    ///
    /// Taken from the caller rather than the clock: this exporter runs inside
    /// Nix builds, where reading the clock makes the output differ from one run
    /// to the next for no reason a listener could hear.
    pub date: String,
}

/// Write `PIONEER/` under `root` for this collection.
#[cfg(not(target_family = "wasm"))]
pub fn write_device(root: &Path, collection: &Collection, options: &Options) -> io::Result<()> {
    write_device_to(
        &mut collection::sink::Directory::new(root),
        collection,
        options,
    )
}

/// Write `PIONEER/` into a sink for this collection.
///
/// The analysis files first and the database last, which is the order a FAT32
/// image wants them in and the order a directory does not care about.
pub fn write_device_to(
    sink: &mut dyn Sink,
    collection: &Collection,
    options: &Options,
) -> io::Result<()> {
    for (index, track) in collection.tracks.iter().enumerate() {
        let directory = analysis_directory(track_id(index));
        let directory = directory.trim_start_matches('/');
        sink.file(&format!("{directory}/ANLZ0000.DAT"), &anlz::dat(track))?;
        sink.file(&format!("{directory}/ANLZ0000.EXT"), &anlz::ext(track))?;
        sink.file(&format!("{directory}/ANLZ0000.2EX"), &anlz::two_ex(track))?;
    }

    let mut database_bytes = Vec::new();
    build(collection, options).write(&mut database_bytes)?;
    sink.file(DATABASE_PATH, &database_bytes)
}

/// Build the database in memory, which is what the tests read back.
pub fn build(collection: &Collection, options: &Options) -> Database {
    let mut database = Database::new();

    let (artists, artist_of_track) = collection.index_by(|track| Some(track.artist.as_str()));
    let (genres, genre_of_track) = collection.index_by(|track| track.genre.as_deref());
    let (albums, album_of_track) = collection.index_by(|track| track.album.as_deref());
    // Keys are stored in Camelot notation, which is what a player then shows and
    // sorts by, and matches the file names. Two tracks in the same key share one
    // row, which is what makes "same key" browsing work.
    let (keys, key_of_track) = collection.index_by(|track| Some(track.key_camelot.as_str()));

    for (index, name) in artists.iter().enumerate() {
        database
            .table(PageType::Artists)
            .push_indexed(rows::artist(index as u32 + 1, name));
    }
    for (index, name) in genres.iter().enumerate() {
        database
            .table(PageType::Genres)
            .push(rows::named(index as u32 + 1, name, false));
    }
    for (index, name) in albums.iter().enumerate() {
        database
            .table(PageType::Albums)
            .push_indexed(rows::album(index as u32 + 1, 0, name));
    }
    for (index, name) in keys.iter().enumerate() {
        database
            .table(PageType::Keys)
            .push(rows::named(index as u32 + 1, name, true));
    }
    // The eight colours a player offers, which exist whether or not a track
    // uses one. Index 0 means "no colour" and gets no row.
    for (index, name) in [
        "Pink", "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple",
    ]
    .iter()
    .enumerate()
    {
        database
            .table(PageType::Colors)
            .push(rows::colour(index as u8 + 1, name));
    }

    for (index, track) in collection.tracks.iter().enumerate() {
        let id = track_id(index);
        let row = rows::TrackRow {
            id,
            artist_id: artist_of_track[index].map_or(0, |i| i as u32 + 1),
            album_id: album_of_track[index].map_or(0, |i| i as u32 + 1),
            genre_id: genre_of_track[index].map_or(0, |i| i as u32 + 1),
            key_id: key_of_track[index].map_or(0, |i| i as u32 + 1),
            sample_rate: track.sample_rate,
            sample_depth: track.bit_depth,
            bitrate_kbps: track.bitrate_kbps,
            // A player reads this as a 32-bit count, so a file over four
            // gigabytes cannot be described here. FAT32 cannot hold one either.
            file_size: u32::try_from(track.file_size).unwrap_or(u32::MAX),
            tempo_centi_bpm: (track.bpm * 100.0).round().max(0.0) as u32,
            duration_seconds: u16::try_from(track.duration_rounded()).unwrap_or(u16::MAX),
            format: track.format,
            title: track.title.clone(),
            comment: track.comment.clone(),
            file_name: track.file_name.clone(),
            file_path: track.device_path.clone(),
            analyze_path: format!("{}/ANLZ0000.DAT", analysis_directory(id)),
            date_added: options.date.clone(),
            analyze_date: options.date.clone(),
        };
        database.table(PageType::Tracks).push_indexed(row.encode());
    }

    for (index, playlist) in collection.playlists.iter().enumerate() {
        let playlist_id = index as u32 + 1;
        database
            .table(PageType::PlaylistTree)
            .push(rows::playlist_node(
                playlist_id,
                0,
                index as u32,
                false,
                &playlist.name,
            ));
        for (position, track_index) in playlist.tracks.iter().enumerate() {
            database
                .table(PageType::PlaylistEntries)
                .push(rows::playlist_entry(
                    position as u32 + 1,
                    track_id(*track_index),
                    playlist_id,
                ));
        }
    }

    database
}

/// Row ids start at one: zero means "not set" in every field that holds one.
fn track_id(index: usize) -> u32 {
    index as u32 + 1
}

/// Where a track's analysis files live.
///
/// rekordbox spreads them over two levels so no directory holds thousands of
/// entries, which FAT32 handles badly. The names are derived from the track id
/// rather than hashed, so the same collection exports to the same paths.
fn analysis_directory(track_id: u32) -> String {
    format!("/{ANALYSIS_ROOT}/P{:03}/{:08X}", track_id / 1000, track_id)
}

/// The default playlist a device gets when the caller names none.
pub fn all_tracks_playlist(collection: &Collection) -> collection::Playlist {
    collection::Playlist {
        name: "All tracks".to_string(),
        tracks: (0..collection.tracks.len()).collect(),
    }
}

/// The path this exporter will write a track's analysis file to, relative to
/// the device root. Exposed so a caller can check what it is about to write.
pub fn analysis_path(index: usize) -> String {
    format!("{}/ANLZ0000.DAT", analysis_directory(track_id(index)))
}

/// Trait-free helper for callers that hold tracks rather than a collection.
pub fn device_paths(tracks: &[Track]) -> Vec<&str> {
    tracks
        .iter()
        .map(|track| track.device_path.as_str())
        .collect()
}
