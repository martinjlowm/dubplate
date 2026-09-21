//! Turning analysed tracks into a device a player can read.
//!
//! The audio is not copied here. This writes the databases that point at it, and
//! whoever assembles the device puts the files where the databases say they are.
//! In the Nix pipeline that is the image builder; locally it is `--audio-mode`.

use crate::ExportArgs;
use anyhow::{Context, Result, bail};
use collection::report::Trim;
use collection::{Collection, Playlist};
use std::path::{Path, PathBuf};

/// Which databases to write.
#[derive(Copy, Clone, PartialEq, Eq, clap::ValueEnum)]
pub enum Target {
    /// `PIONEER/` for CDJs, XDJs and rekordbox.
    Rekordbox,
    /// `Engine Library/Database2/` for Denon players, schema 2.21.2.
    Engine,
    /// Both, on one device. They live in separate directories and neither
    /// player reads the other's, so a stick can carry both.
    Both,
}

/// What to do with the audio files themselves.
#[derive(Copy, Clone, PartialEq, Eq, clap::ValueEnum)]
pub enum AudioMode {
    /// Leave them where they are. The databases still point at `/Contents`,
    /// which is where the image builder puts them.
    None,
    Copy,
    Symlink,
}

/// Where audio sits on the device. Both databases store paths under this.
const CONTENTS: &str = "/Contents";

pub fn run(args: ExportArgs) -> Result<()> {
    // Trimming means writing a new file, so there has to be a mode that writes
    // one. Refused rather than ignored: a run that silently exported untrimmed
    // audio against a trimmed grid puts every cue seconds early.
    if args.trim && args.audio_mode != AudioMode::Copy {
        bail!(
            "--trim writes a cut copy of each file, so it needs --audio-mode copy; \
             the source is never modified"
        );
    }
    let trim = if args.trim {
        Trim::ToFirstBeat
    } else {
        Trim::Keep
    };
    let tracks = discover(&args.audio, &args.reports, trim)?;
    if tracks.is_empty() {
        bail!(
            "no analysed tracks found: {} holds no audio with a matching report in {}",
            args.audio.display(),
            args.reports.display()
        );
    }

    let mut collection = Collection {
        tracks,
        playlists: Vec::new(),
    };
    collection.playlists.push(Playlist {
        name: args.playlist.clone(),
        tracks: (0..collection.tracks.len()).collect(),
    });

    std::fs::create_dir_all(&args.out)?;
    let date = args.date.clone().unwrap_or_else(today);

    // The audio first, then the databases that describe it. A trimmed file is
    // shorter and smaller than its source, and both databases state a length
    // and a byte count, so the numbers are read off what was written rather
    // than off what it was written from.
    place_audio(&args.out, &mut collection, args.audio_mode)?;

    if matches!(args.target, Target::Rekordbox | Target::Both) {
        rekordbox::write_device(
            &args.out,
            &collection,
            &rekordbox::Options { date: date.clone() },
        )
        .context("writing the rekordbox database")?;
    }

    if matches!(args.target, Target::Engine | Target::Both) {
        engine::write_device(&args.out, &collection, &engine::Options { date })
            .context("writing the Engine Library database")?;
    }

    println!(
        "{} tracks, {} playlist entries, written to {}",
        collection.tracks.len(),
        collection
            .playlists
            .iter()
            .map(|p| p.tracks.len())
            .sum::<usize>(),
        args.out.display()
    );
    Ok(())
}

/// Pair each audio file with the report of analysing it.
///
/// The pairing is by stem: `138_03A_Artist-Title.flac` next to
/// `138_03A_Artist-Title.json`. A file with no report is skipped loudly rather
/// than exported with a default beat grid, which would look analysed and be
/// wrong.
fn discover(audio_directory: &Path, reports: &Path, trim: Trim) -> Result<Vec<collection::Track>> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(audio_directory)
        .with_context(|| format!("reading {}", audio_directory.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| {
            path.extension()
                .and_then(|e| e.to_str())
                .and_then(collection::Format::from_extension)
                .is_some()
        })
        .collect();
    files.sort();

    let mut tracks = Vec::with_capacity(files.len());
    for file in files {
        let stem = file
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let report = reports.join(format!("{stem}.json"));
        if !report.exists() {
            eprintln!("no report for {stem}, skipping");
            continue;
        }
        tracks.push(
            collection::report::load(&file, &report, CONTENTS, trim)
                .with_context(|| format!("reading the analysis of {stem}"))?,
        );
    }
    Ok(tracks)
}

fn place_audio(root: &Path, collection: &mut Collection, mode: AudioMode) -> Result<()> {
    if mode == AudioMode::None {
        return Ok(());
    }
    let contents = root.join(CONTENTS.trim_start_matches('/'));
    std::fs::create_dir_all(&contents)?;
    for track in &mut collection.tracks {
        let destination = contents.join(&track.file_name);
        let _ = std::fs::remove_file(&destination);
        match mode {
            AudioMode::Copy if track.trim_seconds > 0.0 => {
                if track.format != collection::Format::Wav {
                    bail!(
                        "{} is a {} and this trims WAV only; a FLAC or an MP3 has to be cut \
                         where it is decoded, which is the archive pipeline and not here",
                        track.file_name,
                        track.format.extension()
                    );
                }
                // `trim_seconds` is the one number that says where the file
                // starts. The times on this track already count from it.
                let cut = audio::trim_wav(&std::fs::read(&track.source)?, track.trim_seconds)
                    .map_err(|error| anyhow::anyhow!("{error}"))
                    .with_context(|| format!("trimming {}", track.file_name))?;
                track.duration_seconds -= track.trim_seconds;
                track.file_size = cut.len() as u64;
                std::fs::write(&destination, cut)?;
            }
            AudioMode::Copy => {
                std::fs::copy(&track.source, &destination)?;
            }
            AudioMode::Symlink => {
                let target = std::fs::canonicalize(&track.source)?;
                std::os::unix::fs::symlink(target, &destination)?;
            }
            AudioMode::None => unreachable!(),
        }
    }
    Ok(())
}

/// Today, or the date a reproducible build was told to pretend it is.
///
/// `SOURCE_DATE_EPOCH` is the convention Nix and every other build system uses
/// for this. Without it the same collection exported twice differs in a field
/// nobody can hear, which is enough to make two identical images two different
/// store paths.
fn today() -> String {
    let seconds = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        });
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    format!("{year:04}-{month:02}-{day:02}")
}

/// Days since 1970-01-01 to a calendar date.
///
/// Howard Hinnant's algorithm, which is exact and shorter than the dependency
/// that would otherwise arrive to format one string.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_prime + 2) / 5 + 1) as u32;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::civil_from_days;

    #[test]
    fn the_epoch_and_a_leap_day_convert() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_417), (2023, 3, 1));
        // 2024-02-29, which a naive conversion gets wrong.
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }
}
