//! Reading a track back out of the analyser's `report.json`.
//!
//! A deliberately narrow view of that file: only the fields an exporter needs,
//! declared here rather than derived from the report types. The report is the
//! contract between the two halves, and this module is where a change to it
//! fails loudly instead of silently exporting a default.

use crate::{Beat, Cue, CueKind, Format, Track};
use serde::Deserialize;
use std::error::Error;
use std::fmt;
#[cfg(not(target_family = "wasm"))]
use std::path::Path;
use std::path::PathBuf;
use waveform::Waveform;

#[derive(Deserialize)]
struct Report {
    source: Source,
    tempo: Tempo,
    key: Key,
    #[serde(default)]
    structure: Structure,
    waveforms: Waveforms,
}

/// The cues the structure stage placed. Defaulted rather than required, so a
/// report written before that stage existed still exports.
#[derive(Default, Deserialize)]
struct Structure {
    #[serde(default)]
    cues: Vec<ReportCue>,
}

#[derive(Deserialize)]
struct ReportCue {
    kind: String,
    number: u8,
    name: String,
    time_seconds: f64,
}

#[derive(Deserialize)]
struct Source {
    sample_rate: u32,
    channels: u16,
    duration_seconds: f64,
    /// Defaulted, so a report written before the analysis measured this still
    /// loads and exports the file whole.
    #[serde(default)]
    analysed_start_seconds: f64,
    #[serde(default)]
    trim_to_first_beat_seconds: f64,
}

#[derive(Deserialize)]
struct Tempo {
    bpm: f64,
    grid: Grid,
    bar: Bar,
}

#[derive(Deserialize)]
struct Grid {
    beats_seconds: Vec<f64>,
}

/// What to do about the silence the analysis skipped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trim {
    /// Write the file whole and add the skipped head back to every time, so the
    /// cues line up with the file as it is.
    Keep,
    /// Cut the file so beat one is sample zero, and leave every time counting
    /// from there.
    ToFirstBeat,
}

#[derive(Deserialize)]
struct Bar {
    phase: usize,
    beats_per_bar: usize,
}

#[derive(Deserialize)]
struct Key {
    name: String,
    camelot: String,
}

#[derive(Deserialize)]
struct Waveforms {
    preview: Waveform,
    detail: Waveform,
}

#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    Json(serde_json::Error),
    UnsupportedFormat(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "{e}"),
            LoadError::Json(e) => write!(f, "the report is not the shape this exporter reads: {e}"),
            LoadError::UnsupportedFormat(name) => {
                write!(f, "no exportable format for {name}")
            }
        }
    }
}

impl Error for LoadError {}

impl From<std::io::Error> for LoadError {
    fn from(e: std::io::Error) -> Self {
        LoadError::Io(e)
    }
}

impl From<serde_json::Error> for LoadError {
    fn from(e: serde_json::Error) -> Self {
        LoadError::Json(e)
    }
}

/// Build a track from an audio file and the report of analysing it.
///
/// `device_directory` is where the file will sit on the device, e.g.
/// `/Contents`, and decides the path both databases store.
///
/// A thin wrapper over [`parse`]: the three things this reads off the
/// filesystem are the name, the size and the report bytes, and a browser has
/// all three without a filesystem to read them from.
#[cfg(not(target_family = "wasm"))]
pub fn load(
    audio: &Path,
    report: &Path,
    device_directory: &str,
    trim: Trim,
) -> Result<Track, LoadError> {
    let file_name = audio
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let file_size = std::fs::metadata(audio)?.len();
    let mut track = parse(
        &file_name,
        file_size,
        &std::fs::read(report)?,
        device_directory,
        trim,
    )?;
    // The only field the filesystem knows and the bytes do not: where the file
    // came from, which is what the CLI copies or links from later.
    track.source = PathBuf::from(audio);
    Ok(track)
}

/// Build a track from a file name, its size, and the bytes of its report.
///
/// `source` on the returned track is the file name alone, since there is no
/// path to record. Everything an exporter reads is set.
pub fn parse(
    file_name: &str,
    file_size: u64,
    report: &[u8],
    device_directory: &str,
    trim: Trim,
) -> Result<Track, LoadError> {
    let parsed: Report = serde_json::from_slice(report)?;
    let file_name = file_name.to_string();
    let extension = file_name
        .rsplit_once('.')
        .map_or(String::new(), |(_, e)| e.to_string());
    let format = Format::from_extension(&extension)
        .ok_or_else(|| LoadError::UnsupportedFormat(file_name.clone()))?;

    let (artist, title) = split_artist_and_title(&file_name);

    // Lossless formats carry a bitrate that follows from the sample format;
    // for a lossy one the file is the only evidence of what it was encoded at.
    let bitrate_kbps = if format.is_lossless() {
        parsed.source.sample_rate * 16 * u32::from(parsed.source.channels.max(1)) / 1000
    } else if parsed.source.duration_seconds > 0.0 {
        ((file_size as f64 * 8.0) / parsed.source.duration_seconds / 1000.0).round() as u32
    } else {
        0
    };

    // The report counts from where the analysis started, which is past
    // whatever silence it skipped. Either the file goes across whole and that
    // head is added back, or it is cut and the grid's own offset comes off too.
    // One number either way, applied to every time below.
    let (shift_seconds, trim_seconds) = match trim {
        Trim::Keep => (parsed.source.analysed_start_seconds, 0.0),
        Trim::ToFirstBeat => (
            parsed.source.analysed_start_seconds - parsed.source.trim_to_first_beat_seconds,
            parsed.source.trim_to_first_beat_seconds,
        ),
    };

    let beats_per_bar = parsed.tempo.bar.beats_per_bar.max(1);
    let beats = parsed
        .tempo
        .grid
        .beats_seconds
        .iter()
        .enumerate()
        .map(|(index, &time_seconds)| Beat {
            time_seconds: time_seconds + shift_seconds,
            bpm: parsed.tempo.bpm,
            // The bar phase names which beat carries the low end; every fourth
            // beat from there is beat one.
            number_in_bar: ((index + beats_per_bar - parsed.tempo.bar.phase % beats_per_bar)
                % beats_per_bar) as u8
                + 1,
        })
        .collect::<Vec<_>>();

    // The cues the structure stage placed, which the report carries as the
    // contract between the two halves. A player with no cue at all parks at the
    // start of the file, so a report that named none still gets the first beat.
    let cues: Vec<Cue> = if parsed.structure.cues.is_empty() {
        beats
            .first()
            .map(|beat| {
                vec![Cue {
                    kind: CueKind::Memory,
                    time_seconds: beat.time_seconds,
                    number: 1,
                    comment: String::new(),
                }]
            })
            .unwrap_or_default()
    } else {
        parsed
            .structure
            .cues
            .iter()
            .map(|cue| Cue {
                kind: if cue.kind == "hot" {
                    CueKind::Hot
                } else {
                    CueKind::Memory
                },
                time_seconds: cue.time_seconds + shift_seconds,
                number: cue.number,
                comment: cue.name.clone(),
            })
            .collect()
    };

    Ok(Track {
        source: PathBuf::from(&file_name),
        device_path: format!("{}/{}", device_directory.trim_end_matches('/'), file_name),
        file_name,
        title,
        artist,
        album: None,
        genre: None,
        comment: String::new(),
        bpm: parsed.tempo.bpm,
        key_name: parsed.key.name,
        key_camelot: parsed.key.camelot,
        duration_seconds: parsed.source.duration_seconds,
        sample_rate: parsed.source.sample_rate,
        bit_depth: 16,
        bitrate_kbps,
        file_size,
        format,
        trim_seconds,
        beats,
        cues,
        preview: parsed.waveforms.preview,
        detail: parsed.waveforms.detail,
    })
}

/// Pull an artist and a title out of a file name.
///
/// Beatport names a download `Artist_1,_Artist_2-Title_(Mix).wav`, and the
/// rename step prefixes tempo and key to that. There are no tags to read here:
/// a WAV carries none worth trusting, and the name is what the shop wrote.
pub fn split_artist_and_title(file_name: &str) -> (String, String) {
    let stem = file_name
        .rsplit_once('.')
        .map_or(file_name, |(stem, _)| stem);
    let stem = crate::naming::strip_prefix(stem);
    let spaced = stem.replace('_', " ");
    match spaced.split_once('-') {
        Some((artist, title)) => (artist.trim().to_string(), title.trim().to_string()),
        None => (String::new(), spaced.trim().to_string()),
    }
}
