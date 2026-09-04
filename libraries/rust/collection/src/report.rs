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
use std::path::{Path, PathBuf};
use waveform::Waveform;

#[derive(Deserialize)]
struct Report {
    source: Source,
    tempo: Tempo,
    key: Key,
    waveforms: Waveforms,
}

#[derive(Deserialize)]
struct Source {
    sample_rate: u32,
    channels: u16,
    duration_seconds: f64,
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
pub fn load(audio: &Path, report: &Path, device_directory: &str) -> Result<Track, LoadError> {
    let parsed: Report = serde_json::from_slice(&std::fs::read(report)?)?;
    let file_name = audio
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let extension = audio
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    let format = Format::from_extension(&extension)
        .ok_or_else(|| LoadError::UnsupportedFormat(file_name.clone()))?;
    let file_size = std::fs::metadata(audio)?.len();

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

    let beats_per_bar = parsed.tempo.bar.beats_per_bar.max(1);
    let beats = parsed
        .tempo
        .grid
        .beats_seconds
        .iter()
        .enumerate()
        .map(|(index, &time_seconds)| Beat {
            time_seconds,
            bpm: parsed.tempo.bpm,
            // The bar phase names which beat carries the low end; every fourth
            // beat from there is beat one.
            number_in_bar: ((index + beats_per_bar - parsed.tempo.bar.phase % beats_per_bar)
                % beats_per_bar) as u8
                + 1,
        })
        .collect::<Vec<_>>();

    // One memory cue on the first beat. Nothing here detects cue points, and a
    // player with no cue at all parks at the start of the file, which on a
    // track with a silent lead-in is the wrong place.
    let cues = beats
        .first()
        .map(|beat| {
            vec![Cue {
                kind: CueKind::Memory,
                time_seconds: beat.time_seconds,
                number: 1,
                comment: String::new(),
            }]
        })
        .unwrap_or_default();

    Ok(Track {
        source: PathBuf::from(audio),
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
    let stem = strip_analysis_prefix(stem);
    let spaced = stem.replace('_', " ");
    match spaced.split_once('-') {
        Some((artist, title)) => (artist.trim().to_string(), title.trim().to_string()),
        None => (String::new(), spaced.trim().to_string()),
    }
}

/// Drop a `138_03A_` prefix, which is this tool's own and not part of a title.
fn strip_analysis_prefix(stem: &str) -> &str {
    let mut parts = stem.splitn(3, '_');
    let (Some(bpm), Some(key), Some(rest)) = (parts.next(), parts.next(), parts.next()) else {
        return stem;
    };
    let bpm_shaped = bpm.len() == 3 && bpm.bytes().all(|b| b.is_ascii_digit());
    let key_shaped = key.len() == 3
        && key.as_bytes()[..2].iter().all(u8::is_ascii_digit)
        && matches!(key.as_bytes()[2], b'A' | b'B');
    if bpm_shaped && key_shaped { rest } else { stem }
}
