//! A FAT32 filesystem, built in memory, that a player will mount.
//!
//! `nix/device.nix` does this with `truncate`, `mkfs.vfat` and `mcopy`, which is
//! the right answer when there is a shell. In a browser there is not, and the
//! image is the thing the person actually wanted: a file to write to a stick.
//! So the same layout is built here, in Rust, from the same two inputs the Nix
//! builder uses. The databases and the audio arrive as bytes and leave as one
//! `.img`.
//!
//! FAT32 because that is what a CDJ mounts. Denon players read exFAT as well,
//! but a stick that works in either booth is FAT32, and its four-gigabyte file
//! limit is not a constraint for a track.
//!
//! Everything here is deterministic. The volume id is fixed and every timestamp
//! is the epoch, so the same collection built twice is the same bytes twice,
//! which is the property the Nix output has and the reason a checksum means
//! anything.

use collection::sink::Memory;
use fatfs::{FatType, FileSystem, FormatVolumeOptions, FsOptions};
use std::io::{Cursor, Write};

/// Where audio sits on the device, as a directory name.
pub const CONTENTS: &str = "Contents";

/// The same, as the absolute path both databases store.
pub const CONTENTS_PATH: &str = "/Contents";

/// Slack over the payload, for the filesystem's own structures and for whoever
/// drags one more track onto the stick later. The same twelve percent and the
/// same 64 MB the Nix builder allows.
const SLACK_PERCENT: u64 = 12;
const SLACK_BYTES: u64 = 64 * 1024 * 1024;

/// FAT32 is not FAT32 below 65 525 clusters, and `mkfs.vfat` refuses to make one
/// that is smaller. 128 MB clears it with any cluster size worth using.
const MINIMUM_BYTES: u64 = 128 * 1024 * 1024;

/// A fixed volume id. `mkfs.vfat` derives one from the clock otherwise, and two
/// identical libraries would build to two different images.
const VOLUME_ID: u32 = 0xDEAD_BEEF;

#[derive(Debug)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error(e.to_string())
    }
}

/// One audio file as it will sit on the device.
pub struct Audio {
    /// The name under `/Contents`, which is what both databases stored.
    pub file_name: String,
    pub bytes: Vec<u8>,
}

/// Build the image.
///
/// `device` is what the two exporters wrote: `PIONEER/…` and
/// `Engine Library/…`, with their paths already device-relative. `audio` is
/// placed under `/Contents`, which is where those databases point.
///
/// `label` becomes the volume name, which is what a player shows in its source
/// list. FAT32 allows eleven characters and the name is truncated to fit rather
/// than refused.
pub fn build(device: &Memory, audio: &[Audio], label: &str) -> Result<Vec<u8>, Error> {
    let payload: u64 =
        device.len() as u64 + audio.iter().map(|a| a.bytes.len() as u64).sum::<u64>();
    let size = (payload * (100 + SLACK_PERCENT) / 100 + SLACK_BYTES).max(MINIMUM_BYTES);

    // With the `std` feature fatfs reads and writes through `std::io`, so a cursor
    // over a buffer is a disk as far as it is concerned.
    let mut storage = Cursor::new(vec![0u8; size as usize]);
    let mut volume_label = [b' '; 11];
    for (slot, byte) in volume_label.iter_mut().zip(label.bytes()) {
        *slot = byte.to_ascii_uppercase();
    }
    fatfs::format_volume(
        &mut storage,
        FormatVolumeOptions::new()
            .fat_type(FatType::Fat32)
            .volume_id(VOLUME_ID)
            .volume_label(volume_label),
    )
    .map_err(|e| Error(format!("could not make a FAT32 filesystem: {e}")))?;

    {
        // Every timestamp the epoch, so the same collection is the same bytes.
        let filesystem = FileSystem::new(&mut storage, FsOptions::new().time_provider(&EPOCH))
            .map_err(|e| Error(format!("could not open the filesystem just made: {e}")))?;
        {
            let root = filesystem.root_dir();

            // The databases first, then the audio at the path they point at,
            // which is the order the Nix builder uses and the order that keeps
            // the small files near the front of the volume.
            for (path, bytes) in &device.files {
                write_file(&root, path, bytes)?;
            }
            for track in audio {
                write_file(
                    &root,
                    &format!("{CONTENTS}/{}", track.file_name),
                    &track.bytes,
                )?;
            }
        }

        filesystem
            .unmount()
            .map_err(|e| Error(format!("the filesystem would not close cleanly: {e}")))?;
    }

    Ok(storage.into_inner())
}

/// Write one file, making the directories its path names.
///
/// `create_dir` on a directory that exists is not an error in this crate, which
/// is what lets every file just declare its whole path.
fn write_file<T>(root: &fatfs::Dir<'_, T>, path: &str, bytes: &[u8]) -> Result<(), Error>
where
    T: fatfs::ReadWriteSeek,
{
    let path = path.trim_start_matches('/');
    let (directories, name) = match path.rsplit_once('/') {
        Some((directories, name)) => (Some(directories), name),
        None => (None, path),
    };

    let mut directory = root.clone();
    if let Some(directories) = directories {
        let mut walked = String::new();
        for part in directories.split('/') {
            if !walked.is_empty() {
                walked.push('/');
            }
            walked.push_str(part);
            directory = root
                .create_dir(&walked)
                .map_err(|e| Error(format!("could not create {walked}: {e}")))?;
        }
    }

    let mut file = directory
        .create_file(name)
        .map_err(|e| Error(format!("could not create {path}: {e}")))?;
    file.truncate()
        .map_err(|e| Error(format!("could not truncate {path}: {e}")))?;
    file.write_all(bytes)
        .map_err(|e| Error(format!("could not write {path}: {e}")))?;
    Ok(())
}

/// The epoch, for every timestamp FAT wants.
///
/// FAT stores created, modified and accessed times, and none of them are a fact
/// about a track. Reading a clock for them is what would make two builds of one
/// collection differ.
#[derive(Debug, Clone, Copy)]
struct Epoch;

/// `FsOptions::time_provider` wants a `&'static`, so there is one of these.
static EPOCH: Epoch = Epoch;

impl fatfs::TimeProvider for Epoch {
    fn get_current_date(&self) -> fatfs::Date {
        // 1980-01-01, which is zero in the DOS date encoding FAT stores and the
        // earliest date it can hold.
        fatfs::Date {
            year: 1980,
            month: 1,
            day: 1,
        }
    }

    fn get_current_date_time(&self) -> fatfs::DateTime {
        fatfs::DateTime {
            date: self.get_current_date(),
            time: fatfs::Time {
                hour: 0,
                min: 0,
                sec: 0,
                millis: 0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Audio, CONTENTS, build};
    use collection::Sink;
    use collection::sink::Memory;

    fn device() -> Memory {
        let mut sink = Memory::new();
        sink.file("PIONEER/rekordbox/export.pdb", b"pdb").unwrap();
        sink.file("PIONEER/USBANLZ/a1b/c2d3e4f5/ANLZ0000.DAT", b"dat")
            .unwrap();
        sink.file("Engine Library/Database2/m.db", b"sqlite")
            .unwrap();
        sink
    }

    #[test]
    fn the_image_is_a_fat32_volume_holding_both_databases_and_the_audio() {
        let audio = vec![Audio {
            file_name: "126_05A_Lange-Out_Of_The_Sky.flac".into(),
            bytes: vec![7u8; 4096],
        }];
        let bytes = build(&device(), &audio, "MUSIC").unwrap();

        // The boot sector's signature, and FAT32's own marker in the extended
        // BPB. A player checks both before it looks for a directory.
        assert_eq!(&bytes[510..512], &[0x55, 0xAA]);
        assert_eq!(&bytes[82..87], b"FAT32");

        let mut storage = std::io::Cursor::new(bytes);
        let filesystem = fatfs::FileSystem::new(&mut storage, fatfs::FsOptions::new()).unwrap();
        let root = filesystem.root_dir();

        let names: Vec<String> = root
            .iter()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert!(names.contains(&"PIONEER".to_string()), "{names:?}");
        assert!(names.contains(&"Engine Library".to_string()), "{names:?}");
        assert!(names.contains(&CONTENTS.to_string()), "{names:?}");

        // A long name survives, which is the whole reason LFN is on: a track
        // truncated to 8.3 is a track a player lists as OUT_OF~1.FLA.
        let contents: Vec<String> = root
            .open_dir(CONTENTS)
            .unwrap()
            .iter()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert!(
            contents.contains(&"126_05A_Lange-Out_Of_The_Sky.flac".to_string()),
            "{contents:?}"
        );
    }

    #[test]
    fn the_same_collection_builds_to_the_same_bytes() {
        let audio = vec![Audio {
            file_name: "128_08A_Track.wav".into(),
            bytes: vec![3u8; 1024],
        }];
        assert_eq!(
            build(&device(), &audio, "MUSIC").unwrap(),
            build(&device(), &audio, "MUSIC").unwrap()
        );
    }
}
