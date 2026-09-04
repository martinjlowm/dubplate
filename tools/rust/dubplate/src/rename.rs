//! Naming a file after what it measured.
//!
//! The format is `<BPM>_<KEY>_<rest of the original name>`, with the tempo
//! zero-padded to three digits and the Camelot number to two, so a plain
//! alphabetical listing sorts by tempo first and then around the wheel. Sorting
//! is the whole point of the padding: `96` sorts before `138` as text, and `8A`
//! sorts between `12A` and `1A`.
//!
//! Nothing here renames the original. A run produces links or copies under a
//! directory you name, and the source tree is left alone.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// The name a file measured at `bpm` in `camelot` should carry.
///
/// `camelot` is the short form the report prints, such as `8A`. The rule lives
/// in `collection::naming` because a browser writes these names into a FAT32
/// image and both databases store the result; two spellings of the prefix would
/// be a stick whose listing does not sort.
pub fn target_name(original: &Path, bpm: f64, camelot: &str) -> Result<String> {
    let file_name = original
        .file_name()
        .and_then(|n| n.to_str())
        .context("the path has no file name")?;
    collection::naming::target_name(file_name, bpm, camelot)
        .with_context(|| format!("cannot name a file after {bpm} BPM in {camelot}"))
}

/// Every WAV under `path`, or `path` itself when it is a file.
///
/// One level deep, not a recursive walk: a nested tree carries a structure
/// somebody chose, and flattening it into one directory of renamed files would
/// discard that silently.
pub fn wav_files(path: &Path) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    let mut files: Vec<PathBuf> = std::fs::read_dir(path)
        .with_context(|| format!("reading {}", path.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
        })
        .collect();
    // Sorted, so a run over a directory is reproducible and its log is diffable.
    files.sort();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_both_numbers_so_a_listing_sorts() {
        let name = target_name(Path::new("Artist-Title (Extended Mix).wav"), 96.4, "8A").unwrap();
        assert_eq!(name, "096_08A_Artist-Title (Extended Mix).wav");

        let fast = target_name(Path::new("x.wav"), 199.84, "12B").unwrap();
        assert_eq!(fast, "200_12B_x.wav");
    }

    #[test]
    fn renaming_twice_replaces_the_prefix_instead_of_stacking_one() {
        let once = target_name(Path::new("Artist-Title.wav"), 138.0, "3A").unwrap();
        let twice = target_name(Path::new(&once), 140.0, "5A").unwrap();
        assert_eq!(twice, "140_05A_Artist-Title.wav");
    }

    #[test]
    fn a_name_that_only_looks_like_a_prefix_is_left_alone() {
        // Three digits and something ending in A, but not a Camelot key.
        let name = target_name(Path::new("808_MIA_State-Pacific.wav"), 120.0, "8B").unwrap();
        assert_eq!(name, "120_08B_808_MIA_State-Pacific.wav");
    }

    #[test]
    fn an_unmeasurable_tempo_is_refused() {
        assert!(target_name(Path::new("x.wav"), f64::NAN, "8A").is_err());
    }
}
