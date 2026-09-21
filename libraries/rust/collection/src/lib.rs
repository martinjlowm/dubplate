//! What a player needs to know about a track, in a form no player owns.
//!
//! Pioneer and Denon want the same facts written two different ways. This is the
//! shape both exporters read: metadata, a beat grid, cues, and two waveforms.
//! Nothing here knows about either device, and the exporters know nothing about
//! how the facts were measured.

pub mod naming;
pub mod report;
pub mod sink;

pub use sink::Sink;

use std::path::PathBuf;
use waveform::Waveform;

/// The container the audio is in, which decides both the directory it is
/// exported into and the format field written into the database.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Wav,
    Flac,
    Mp3,
    Aiff,
}

impl Format {
    pub fn from_extension(extension: &str) -> Option<Self> {
        match extension.to_ascii_lowercase().as_str() {
            "wav" => Some(Format::Wav),
            "flac" => Some(Format::Flac),
            "mp3" => Some(Format::Mp3),
            "aiff" | "aif" => Some(Format::Aiff),
            _ => None,
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            Format::Wav => "wav",
            Format::Flac => "flac",
            Format::Mp3 => "mp3",
            Format::Aiff => "aiff",
        }
    }

    /// True when the format carries every sample, which decides whether a
    /// bitrate is a property of the file or an average worth reporting.
    pub fn is_lossless(&self) -> bool {
        !matches!(self, Format::Mp3)
    }
}

/// One beat of the grid.
#[derive(Clone, Copy, Debug)]
pub struct Beat {
    pub time_seconds: f64,
    pub bpm: f64,
    /// Position in the bar, 1 to 4. Beat one is where the low end lands, which
    /// is what the tempo stage measures as the bar phase.
    pub number_in_bar: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CueKind {
    /// Shown in the player's cue list and used by auto-cue.
    Memory,
    /// Assigned to a hot cue button.
    Hot,
}

#[derive(Clone, Debug)]
pub struct Cue {
    pub kind: CueKind,
    pub time_seconds: f64,
    /// Hot cue letter, 1 = A. Ignored for memory cues.
    pub number: u8,
    pub comment: String,
}

/// A track as a device will see it.
#[derive(Clone, Debug)]
pub struct Track {
    /// Where the audio is right now, on the machine doing the export.
    pub source: PathBuf,
    /// Where it will live on the device, as an absolute path from the device
    /// root, e.g. `/Contents/138_03A_Artist-Title.flac`. Both databases store
    /// this path, so it has to be decided before either is written.
    pub device_path: String,
    pub file_name: String,
    pub title: String,
    pub artist: String,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub comment: String,
    pub bpm: f64,
    /// Key as text, e.g. `F# minor`.
    pub key_name: String,
    /// Key on the Camelot wheel, e.g. `11A`.
    pub key_camelot: String,
    pub duration_seconds: f64,
    pub sample_rate: u32,
    pub bit_depth: u16,
    pub bitrate_kbps: u32,
    pub file_size: u64,
    pub format: Format,
    /// Seconds to cut from the head of [`Track::source`] before writing it to
    /// the device. Zero when the file goes across whole.
    ///
    /// Every time in this track counts from here, so a writer that copies the
    /// file without cutting puts every cue this many seconds early. One field
    /// rather than two, for the same reason [`Track::device_path`] is one: two
    /// places deciding where a file starts is two places disagreeing.
    pub trim_seconds: f64,
    pub beats: Vec<Beat>,
    pub cues: Vec<Cue>,
    pub preview: Waveform,
    pub detail: Waveform,
}

impl Track {
    /// Duration rounded to whole seconds, which is what both databases store.
    pub fn duration_rounded(&self) -> u32 {
        self.duration_seconds.round().max(0.0) as u32
    }

    /// Time of the first beat, which is where a player parks the cue when a
    /// track is loaded and nothing else says otherwise.
    pub fn first_beat_seconds(&self) -> f64 {
        self.beats.first().map_or(0.0, |beat| beat.time_seconds)
    }
}

/// A named list of tracks, by index into [`Collection::tracks`].
#[derive(Clone, Debug)]
pub struct Playlist {
    pub name: String,
    pub tracks: Vec<usize>,
}

/// Everything going onto one device.
#[derive(Clone, Debug, Default)]
pub struct Collection {
    pub tracks: Vec<Track>,
    pub playlists: Vec<Playlist>,
}

impl Collection {
    /// Distinct values of a field, in first-seen order, with the index each
    /// track should reference.
    ///
    /// Both databases store artists, genres and keys as their own rows and
    /// reference them by id, so both need the same de-duplication.
    pub fn index_by<'a, F>(&'a self, field: F) -> (Vec<&'a str>, Vec<Option<usize>>)
    where
        F: Fn(&'a Track) -> Option<&'a str>,
    {
        let mut values: Vec<&str> = Vec::new();
        let mut per_track = Vec::with_capacity(self.tracks.len());
        for track in &self.tracks {
            match field(track).filter(|value| !value.is_empty()) {
                Some(value) => {
                    let index = values.iter().position(|existing| *existing == value);
                    per_track.push(Some(index.unwrap_or_else(|| {
                        values.push(value);
                        values.len() - 1
                    })));
                }
                None => per_track.push(None),
            }
        }
        (values, per_track)
    }
}
