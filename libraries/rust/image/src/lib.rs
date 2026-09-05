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

/// The sector size `fatfs` formats with, and the unit a disk image is measured in.
const SECTOR_BYTES: u64 = 512;

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
    // Rounded up to a whole sector. A raw image is a disk, and a disk is a
    // number of sectors: macOS will not attach one whose length is not, and the
    // bytes past the last sector are outside the volume the BPB describes
    // anyway. Unrounded, this left up to 511 bytes hanging off the end.
    let size = (payload * (100 + SLACK_PERCENT) / 100 + SLACK_BYTES)
        .max(MINIMUM_BYTES)
        .next_multiple_of(SECTOR_BYTES);

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

    let mut image = storage.into_inner();
    repair_dot_entries(&mut image)?;
    Ok(image)
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

/// Put every subdirectory's `.` and `..` back where the format says they go.
///
/// `fatfs` 0.3.6 writes these two through the same path as any other name, so
/// each arrives behind a long-name entry, and it fills `..` with the parent's
/// first cluster even when the parent is the root, where the format says zero.
/// Both are violations a player is entitled to reject, and `fsck_msdos` reports
/// every directory in the image as not being one.
///
/// So the entries are rewritten here rather than there: the crate is four years
/// unmaintained, and a fork of a FAT driver is a larger thing to own than one
/// pass over the bytes it produced. Each directory begins
///
///   `[long "."] [short "."] [long ".."] [short ".."]`
///
/// and leaves as
///
///   `[short "."] [short ".."] [deleted] [deleted]`
///
/// The long entries become deleted slots rather than free ones. A free slot is
/// where a reader stops, so blanking them would truncate the directory instead.
fn repair_dot_entries(image: &mut [u8]) -> Result<(), Error> {
    let boot = Boot::read(image)?;

    let mut pending = vec![boot.root_cluster];
    let mut seen = std::collections::HashSet::new();

    while let Some(start) = pending.pop() {
        if !seen.insert(start) {
            continue;
        }
        if start != boot.root_cluster {
            boot.repair(image, start);
        }
        pending.extend(boot.children(image, start));
    }

    Ok(())
}

/// A directory entry is 32 bytes, and `.` and `..` occupy the first four slots.
const ENTRY: usize = 32;
/// Marks a slot deleted: skipped by a reader, unlike a zero, which ends the
/// directory.
const DELETED: u8 = 0xE5;
const ATTR_LONG_NAME: u8 = 0x0F;
const ATTR_DIRECTORY: u8 = 0x10;

/// The handful of BPB fields this pass needs to walk to a directory.
struct Boot {
    bytes_per_sector: u64,
    sectors_per_cluster: u64,
    reserved_sectors: u64,
    fats: u64,
    sectors_per_fat: u64,
    root_cluster: u32,
}

impl Boot {
    fn read(image: &[u8]) -> Result<Self, Error> {
        if image.len() < 512 {
            return Err(Error("the image is too small to hold a boot sector".into()));
        }
        let at16 = |at: usize| u64::from(u16::from_le_bytes([image[at], image[at + 1]]));
        let at32 = |at: usize| {
            u32::from_le_bytes([image[at], image[at + 1], image[at + 2], image[at + 3]])
        };

        let boot = Boot {
            bytes_per_sector: at16(0x0B),
            sectors_per_cluster: u64::from(image[0x0D]),
            reserved_sectors: at16(0x0E),
            fats: u64::from(image[0x10]),
            sectors_per_fat: u64::from(at32(0x24)),
            root_cluster: at32(0x2C),
        };

        if boot.bytes_per_sector == 0 || boot.sectors_per_cluster == 0 || boot.root_cluster < 2 {
            return Err(Error(
                "the filesystem just made does not describe itself".into(),
            ));
        }
        Ok(boot)
    }

    /// Where a cluster's data starts.
    fn offset(&self, cluster: u32) -> usize {
        let sector = self.reserved_sectors
            + self.fats * self.sectors_per_fat
            + (u64::from(cluster) - 2) * self.sectors_per_cluster;
        (sector * self.bytes_per_sector) as usize
    }

    fn cluster_bytes(&self) -> usize {
        (self.sectors_per_cluster * self.bytes_per_sector) as usize
    }

    /// The next cluster in a chain, while the chain continues.
    fn next(&self, image: &[u8], cluster: u32) -> Option<u32> {
        let at = (self.reserved_sectors * self.bytes_per_sector) as usize + cluster as usize * 4;
        let slot = image.get(at..at + 4)?;
        let entry = u32::from_le_bytes([slot[0], slot[1], slot[2], slot[3]]) & 0x0FFF_FFFF;
        (2..0x0FFF_FFF7).contains(&entry).then_some(entry)
    }

    /// Every cluster a directory occupies, in order.
    fn chain(&self, image: &[u8], start: u32) -> Vec<u32> {
        let mut chain = vec![start];
        let mut at = start;
        // Bounded by what the image could hold, so a FAT pointing back into its
        // own chain stops here rather than running forever.
        let limit = image.len() / self.cluster_bytes().max(1) + 2;
        while let Some(next) = self.next(image, at) {
            if chain.len() > limit || chain.contains(&next) {
                break;
            }
            chain.push(next);
            at = next;
        }
        chain
    }

    /// The first cluster of every subdirectory this one holds.
    fn children(&self, image: &[u8], start: u32) -> Vec<u32> {
        let mut children = Vec::new();
        for cluster in self.chain(image, start) {
            let base = self.offset(cluster);
            let Some(data) = image.get(base..base + self.cluster_bytes()) else {
                continue;
            };
            for slot in data.chunks_exact(ENTRY) {
                match slot[0] {
                    0 => return children,
                    DELETED => continue,
                    _ => {}
                }
                if slot[11] == ATTR_LONG_NAME || slot[11] & ATTR_DIRECTORY == 0 {
                    continue;
                }
                if slot[0] == b'.' {
                    continue;
                }
                let first = (u32::from(u16::from_le_bytes([slot[20], slot[21]])) << 16)
                    | u32::from(u16::from_le_bytes([slot[26], slot[27]]));
                if first >= 2 {
                    children.push(first);
                }
            }
        }
        children
    }

    /// Rewrite one directory's opening four slots, if they are the shape
    /// `fatfs` leaves behind. Anything else is left alone.
    fn repair(&self, image: &mut [u8], cluster: u32) {
        let base = self.offset(cluster);
        let Some(slots) = image.get_mut(base..base + 4 * ENTRY) else {
            return;
        };

        let long = |slot: &[u8]| slot[11] == ATTR_LONG_NAME;
        let dot = |slot: &[u8], name: &[u8]| {
            slot[0] != DELETED && slot[11] & ATTR_DIRECTORY != 0 && &slot[..11] == name
        };
        if !(long(&slots[0..ENTRY])
            && dot(&slots[ENTRY..2 * ENTRY], b".          ")
            && long(&slots[2 * ENTRY..3 * ENTRY])
            && dot(&slots[3 * ENTRY..4 * ENTRY], b"..         "))
        {
            return;
        }

        let here: [u8; ENTRY] = slots[ENTRY..2 * ENTRY].try_into().expect("one entry");
        let mut up: [u8; ENTRY] = slots[3 * ENTRY..4 * ENTRY].try_into().expect("one entry");

        // The root has a cluster number of its own, and `..` still has to say
        // zero when the root is what it points at.
        let parent = (u32::from(u16::from_le_bytes([up[20], up[21]])) << 16)
            | u32::from(u16::from_le_bytes([up[26], up[27]]));
        if parent == self.root_cluster {
            up[20..22].fill(0);
            up[26..28].fill(0);
        }

        slots[0..ENTRY].copy_from_slice(&here);
        slots[ENTRY..2 * ENTRY].copy_from_slice(&up);
        for slot in slots[2 * ENTRY..4 * ENTRY].chunks_exact_mut(ENTRY) {
            slot.fill(0);
            slot[0] = DELETED;
        }
    }
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

    /// A raw image is a disk, and a disk is a whole number of sectors.
    ///
    /// macOS refuses to attach one that is not, which is what a person meets
    /// first: the image downloads, and the volume will not open.
    #[test]
    fn the_image_is_a_whole_number_of_sectors() {
        let audio = vec![Audio {
            file_name: "128_08A_Track.wav".into(),
            bytes: vec![3u8; 5000],
        }];
        let bytes = build(&device(), &audio, "MUSIC").unwrap();
        assert_eq!(bytes.len() % 512, 0, "{} bytes", bytes.len());
    }

    /// Every subdirectory opens with `.` and `..`, and nothing before them.
    ///
    /// `fatfs` puts a long-name entry in front of each, which is not a thing the
    /// format allows and which `fsck_msdos` reads as the directory not being a
    /// directory. `..` also has to be zero when it points at the root, whatever
    /// cluster the root happens to occupy.
    #[test]
    fn a_subdirectory_opens_with_its_dot_entries() {
        let bytes = build(&device(), &[], "MUSIC").unwrap();

        let sector = u64::from(u16::from_le_bytes([bytes[0x0B], bytes[0x0C]]));
        let per_cluster = u64::from(bytes[0x0D]);
        let reserved = u64::from(u16::from_le_bytes([bytes[0x0E], bytes[0x0F]]));
        let fats = u64::from(bytes[0x10]);
        let per_fat = u64::from(u32::from_le_bytes([
            bytes[0x24],
            bytes[0x25],
            bytes[0x26],
            bytes[0x27],
        ]));
        let root = u32::from_le_bytes([bytes[0x2C], bytes[0x2D], bytes[0x2E], bytes[0x2F]]);
        let offset = |cluster: u32| {
            ((reserved + fats * per_fat + (u64::from(cluster) - 2) * per_cluster) * sector) as usize
        };
        let start = |slot: &[u8]| {
            (u32::from(u16::from_le_bytes([slot[20], slot[21]])) << 16)
                | u32::from(u16::from_le_bytes([slot[26], slot[27]]))
        };

        // PIONEER sits in the root, so its `..` is the case that has to read zero.
        let root_dir = &bytes[offset(root)..offset(root) + (per_cluster * sector) as usize];
        let pioneer = root_dir
            .chunks_exact(32)
            .find(|slot| &slot[..11] == b"PIONEER    " && slot[11] & 0x10 != 0)
            .map(start)
            .expect("the root lists PIONEER");

        let dir = &bytes[offset(pioneer)..offset(pioneer) + 4 * 32];
        assert_eq!(&dir[..11], b".          ", "the first slot is not `.`");
        assert_eq!(dir[11] & 0x10, 0x10, "`.` is not marked a directory");
        assert_eq!(start(&dir[..32]), pioneer, "`.` does not point at itself");

        assert_eq!(&dir[32..43], b"..         ", "the second slot is not `..`");
        assert_eq!(dir[43] & 0x10, 0x10, "`..` is not marked a directory");
        assert_eq!(
            start(&dir[32..64]),
            0,
            "`..` names the root's cluster where the format says zero",
        );

        for slot in dir[64..128].chunks_exact(32) {
            assert_eq!(slot[0], 0xE5, "the slot the long name left is not deleted");
        }
    }

    /// The repair leaves the filesystem readable, which is the point of doing it
    /// in place rather than shifting every entry along.
    #[test]
    fn the_files_survive_the_repair() {
        let audio = vec![Audio {
            file_name: "126_05A_Lange-Out_Of_The_Sky.flac".into(),
            bytes: vec![7u8; 4096],
        }];
        let bytes = build(&device(), &audio, "MUSIC").unwrap();

        let mut storage = std::io::Cursor::new(bytes);
        let filesystem = fatfs::FileSystem::new(&mut storage, fatfs::FsOptions::new()).unwrap();
        let root = filesystem.root_dir();

        let anlz: Vec<String> = root
            .open_dir("PIONEER/USBANLZ/a1b/c2d3e4f5")
            .unwrap()
            .iter()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert!(anlz.contains(&"ANLZ0000.DAT".to_string()), "{anlz:?}");

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
}
