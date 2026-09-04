//! Where a device's files are put, without saying what a file is.
//!
//! Both exporters used to call `std::fs` directly, which is what stopped either
//! of them running in a browser: those calls compile for wasm32 and then fail at
//! runtime, so the compiler never said anything. Writing through this trait
//! instead moves the decision to the caller. The CLI hands them a
//! [`Directory`], the Nix image builder hands them the same, and the browser
//! hands them a [`Memory`] whose files go on to a FAT32 image built in a tab.
//!
//! Paths are device-relative and always use forward slashes, because that is
//! what both databases store and what FAT32 wants. A sink joins them onto
//! whatever it writes into.

use std::io;

/// Somewhere a device's files can be put.
pub trait Sink {
    /// Write one file at a device-relative path, creating whatever the path
    /// implies. Called once per file; a second call for the same path replaces
    /// the first.
    fn file(&mut self, path: &str, bytes: &[u8]) -> io::Result<()>;
}

/// Files collected in memory, in the order they were written.
///
/// A `Vec` rather than a map: the FAT32 builder wants them in write order so
/// that the databases land before the audio they point at, and no exporter
/// writes the same path twice.
#[derive(Default, Debug)]
pub struct Memory {
    pub files: Vec<(String, Vec<u8>)>,
}

impl Memory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Total bytes held, which is what the image builder sizes a filesystem
    /// from before it makes one.
    pub fn len(&self) -> usize {
        self.files.iter().map(|(_, bytes)| bytes.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

impl Sink for Memory {
    fn file(&mut self, path: &str, bytes: &[u8]) -> io::Result<()> {
        match self.files.iter_mut().find(|(name, _)| name == path) {
            Some((_, existing)) => *existing = bytes.to_vec(),
            None => self.files.push((path.to_string(), bytes.to_vec())),
        }
        Ok(())
    }
}

/// Files written under a directory on the host filesystem.
#[cfg(not(target_family = "wasm"))]
pub struct Directory {
    root: std::path::PathBuf,
}

#[cfg(not(target_family = "wasm"))]
impl Directory {
    pub fn new(root: impl Into<std::path::PathBuf>) -> Self {
        Directory { root: root.into() }
    }
}

#[cfg(not(target_family = "wasm"))]
impl Sink for Directory {
    fn file(&mut self, path: &str, bytes: &[u8]) -> io::Result<()> {
        let destination = self.root.join(path.trim_start_matches('/'));
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(destination, bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::{Memory, Sink};

    #[test]
    fn a_repeated_path_replaces_rather_than_duplicates() {
        let mut sink = Memory::new();
        sink.file("PIONEER/rekordbox/export.pdb", b"first").unwrap();
        sink.file("PIONEER/rekordbox/export.pdb", b"second")
            .unwrap();
        assert_eq!(sink.files.len(), 1);
        assert_eq!(sink.files[0].1, b"second");
        assert_eq!(sink.len(), 6);
    }
}
