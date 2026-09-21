//! A FAT32 filesystem, built where the caller puts it, that a player will mount.
//!
//! `nix/device.nix` does this with `truncate`, `mkfs.vfat` and `mcopy`, which is
//! the right answer when there is a shell. In a browser there is not, and the
//! image is the thing the person actually wanted: a file to write to a stick.
//! So the same layout is built here, in Rust, from the same two inputs the Nix
//! builder uses.
//!
//! [`build`] holds the whole image in a `Vec`, which is what a test wants and
//! what a caller with a few megabytes of audio wants. [`build_into`] writes it
//! through [`std::io`] instead, so a collection larger than the address space
//! can be assembled: a browser hands it a file in the origin-private
//! filesystem and never holds an image, or a track, in wasm memory. Neither
//! function opens anything; the storage and the audio are arguments.
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
use std::io::{Cursor, Read, Seek, SeekFrom, Write};

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

/// One audio file as it will sit on the device, held in memory.
pub struct Audio {
    /// The name under `/Contents`, which is what both databases stored.
    pub file_name: String,
    pub bytes: Vec<u8>,
}

/// Where the audio comes from while the image is written.
///
/// Two calls rather than a slice of buffers, because the volume is formatted
/// before a single track goes into it and the length of the payload is what
/// decides how many sectors it has. A caller holding every track in memory
/// answers both from the same `Vec`. A caller reading a browser's
/// origin-private filesystem answers `listing` from what it recorded when it
/// extracted each track, and `write` by streaming one file, so a thirty-track
/// collection never holds more than one track's bytes.
pub trait Tracks {
    /// The name and length of every file to place under `/Contents`, in the
    /// order they are written.
    fn listing(&self) -> Vec<(String, u64)>;

    /// Write the bytes of the file `listing` gave at `index`.
    fn write(&mut self, index: usize, into: &mut dyn Write) -> std::io::Result<()>;
}

impl Tracks for &[Audio] {
    fn listing(&self) -> Vec<(String, u64)> {
        self.iter()
            .map(|track| (track.file_name.clone(), track.bytes.len() as u64))
            .collect()
    }

    fn write(&mut self, index: usize, into: &mut dyn Write) -> std::io::Result<()> {
        into.write_all(&self[index].bytes)
    }
}

/// How far through the payload a build is, reported as it is written.
///
/// A caller drawing a bar wants `written_bytes` over `total_bytes`; a caller
/// naming what it is on wants `file_name`. Both count the bytes that reach the
/// volume, so neither includes the slack [`size`] adds: nothing writes those,
/// and a bar that stops at 89% because the volume is bigger than its contents is
/// a bar that looks stuck.
#[derive(Clone, Copy, Debug)]
pub struct Progress<'a> {
    /// Payload bytes written so far, databases included.
    pub written_bytes: u64,
    /// Payload bytes the whole build will write.
    pub total_bytes: u64,
    /// The file being written, by its path in the image.
    pub file_name: &'a str,
    /// Its position in the order the files are written, from 0.
    pub file_index: usize,
    /// How many files the build writes in total.
    pub file_count: usize,
}

/// How often progress is reported inside one file.
///
/// A track is written a megabyte at a time and every megabyte lands in the
/// volume as several writes, so reporting each one is thousands of calls into a
/// browser for a bar that moves a pixel. Four megabytes is a step a person sees
/// on a three-gigabyte image and a few hundred calls across the whole build.
const PROGRESS_STEP: u64 = 4 * 1024 * 1024;

/// A writer that counts what passes through it and reports as it goes.
///
/// One file's worth. `written` and `reported` are the build's running totals,
/// borrowed rather than owned, so a bar drawn from them climbs across the whole
/// image rather than restarting at every track.
struct Meter<'a> {
    into: &'a mut dyn Write,
    written: &'a mut u64,
    reported: &'a mut u64,
    total: u64,
    file_name: &'a str,
    file_index: usize,
    file_count: usize,
    report: &'a mut dyn FnMut(Progress<'_>),
}

impl Meter<'_> {
    fn announce(&mut self) {
        *self.reported = *self.written;
        (self.report)(Progress {
            written_bytes: *self.written,
            total_bytes: self.total,
            file_name: self.file_name,
            file_index: self.file_index,
            file_count: self.file_count,
        });
    }
}

impl Write for Meter<'_> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let written = self.into.write(buffer)?;
        *self.written += written as u64;
        if *self.written - *self.reported >= PROGRESS_STEP {
            self.announce();
        }
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.into.flush()
    }
}

/// How many bytes a volume holding this payload needs.
///
/// A caller writing into a file allocates exactly this much before calling
/// [`build_into`]: FAT32 describes a fixed number of sectors, and the number of
/// sectors comes from the length of what it is written into.
pub fn size(device: &Memory, audio_bytes: u64) -> u64 {
    let payload = device.len() as u64 + audio_bytes;
    // Rounded up to a whole sector. A raw image is a disk, and a disk is a
    // number of sectors: macOS will not attach one whose length is not, and the
    // bytes past the last sector are outside the volume the BPB describes
    // anyway. Unrounded, this left up to 511 bytes hanging off the end.
    (payload * (100 + SLACK_PERCENT) / 100 + SLACK_BYTES)
        .max(MINIMUM_BYTES)
        .next_multiple_of(SECTOR_BYTES)
}

/// Build the image and hand back its bytes.
///
/// `device` is what the two exporters wrote: `PIONEER/…` and
/// `Engine Library/…`, with their paths already device-relative. `audio` is
/// placed under `/Contents`, which is where those databases point.
///
/// `label` becomes the volume name, which is what a player shows in its source
/// list. FAT32 allows eleven characters and the name is truncated to fit rather
/// than refused.
pub fn build(device: &Memory, audio: &[Audio], label: &str) -> Result<Vec<u8>, Error> {
    let audio_bytes = audio.iter().map(|track| track.bytes.len() as u64).sum();
    let mut storage = Cursor::new(vec![0u8; size(device, audio_bytes) as usize]);
    let mut tracks = audio;
    build_into(&mut storage, device, &mut tracks, label, &mut |_| {})?;
    Ok(storage.into_inner())
}

/// Build the image into `storage`, which is already [`size`] bytes long and
/// reads as zeroes.
///
/// The length is an input rather than a parameter: `fatfs` counts the sectors
/// it was handed rather than being told how many to make, so a caller that
/// allocated too little gets a volume it cannot fill. That is caught here, with
/// both numbers, rather than as a write failing part way through the audio.
///
/// Zeroes because the free clusters are the one part of the volume nothing
/// writes, and leftovers in them are what make one collection build to two
/// different images.
///
/// `progress` is called as the payload lands, at most every few megabytes and
/// at least once per file, and a last time with `written_bytes` equal to
/// `total_bytes`. It reports bytes that were actually written rather than a
/// share of the files placed, because the files differ by two orders of
/// magnitude in size and a browser writing three gigabytes is the caller this
/// exists for. Pass `&mut |_| {}` to ignore it.
pub fn build_into<S, T>(
    storage: &mut S,
    device: &Memory,
    audio: &mut T,
    label: &str,
    progress: &mut dyn FnMut(Progress<'_>),
) -> Result<(), Error>
where
    S: Read + Write + Seek,
    T: Tracks + ?Sized,
{
    let listing = audio.listing();
    let payload = device.len() as u64 + listing.iter().map(|(_, size)| *size).sum::<u64>();
    let length = storage.seek(SeekFrom::End(0))?;
    if length < payload {
        return Err(Error(format!(
            "the image is {length} bytes and the files to write into it are {payload}"
        )));
    }
    storage.seek(SeekFrom::Start(0))?;

    let mut volume_label = [b' '; 11];
    for (slot, byte) in volume_label.iter_mut().zip(label.bytes()) {
        *slot = byte.to_ascii_uppercase();
    }
    fatfs::format_volume(
        &mut *storage,
        FormatVolumeOptions::new()
            .fat_type(FatType::Fat32)
            .volume_id(VOLUME_ID)
            .volume_label(volume_label),
    )
    .map_err(|e| Error(format!("could not make a FAT32 filesystem: {e}")))?;

    {
        // Every timestamp the epoch, so the same collection is the same bytes.
        let filesystem = FileSystem::new(&mut *storage, FsOptions::new().time_provider(&EPOCH))
            .map_err(|e| Error(format!("could not open the filesystem just made: {e}")))?;
        {
            let root = filesystem.root_dir();
            let file_count = device.files.len() + listing.len();
            let mut written = 0u64;
            let mut reported = 0u64;

            // The databases first, then the audio at the path they point at,
            // which is the order the Nix builder uses and the order that keeps
            // the small files near the front of the volume.
            for (file_index, (path, bytes)) in device.files.iter().enumerate() {
                write_file(&root, path, |file| {
                    let mut meter = Meter {
                        into: file,
                        written: &mut written,
                        reported: &mut reported,
                        total: payload,
                        file_name: path,
                        file_index,
                        file_count,
                        report: progress,
                    };
                    meter.write_all(bytes)?;
                    meter.announce();
                    Ok(())
                })?;
            }
            for (index, (file_name, _)) in listing.iter().enumerate() {
                let path = format!("{CONTENTS}/{file_name}");
                write_file(&root, &path, |file| {
                    let mut meter = Meter {
                        into: file,
                        written: &mut written,
                        reported: &mut reported,
                        total: payload,
                        file_name: &path,
                        file_index: device.files.len() + index,
                        file_count,
                        report: progress,
                    };
                    audio.write(index, &mut meter)?;
                    meter.announce();
                    Ok(())
                })?;
            }
        }

        filesystem
            .unmount()
            .map_err(|e| Error(format!("the filesystem would not close cleanly: {e}")))?;
    }

    repair_dot_entries(storage)
}

/// Write one file, making the directories its path names.
///
/// `create_dir` on a directory that exists is not an error in this crate, which
/// is what lets every file just declare its whole path.
///
/// The bytes arrive through a closure rather than as a slice so that a caller
/// streaming a track does not have to hold it: what it writes lands in the
/// image a buffer at a time.
fn write_file<T, F>(root: &fatfs::Dir<'_, T>, path: &str, write: F) -> Result<(), Error>
where
    T: fatfs::ReadWriteSeek,
    F: FnOnce(&mut dyn Write) -> std::io::Result<()>,
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
    write(&mut file).map_err(|e| Error(format!("could not write {path}: {e}")))?;
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
///
/// The pass reads and writes through seeks rather than over a slice, because
/// the image it walks may be a file the size of a stick rather than a buffer.
/// Nothing larger than one cluster is held at a time.
fn repair_dot_entries<S: Read + Write + Seek>(storage: &mut S) -> Result<(), Error> {
    let boot = Boot::read(storage)?;

    let mut pending = vec![boot.root_cluster];
    let mut seen = std::collections::HashSet::new();

    while let Some(start) = pending.pop() {
        if !seen.insert(start) {
            continue;
        }
        if start != boot.root_cluster {
            boot.repair(storage, start)?;
        }
        pending.extend(boot.children(storage, start)?);
    }

    Ok(())
}

/// Read `into.len()` bytes from `at`.
fn read_at<S: Read + Seek>(storage: &mut S, at: u64, into: &mut [u8]) -> Result<(), Error> {
    storage.seek(SeekFrom::Start(at))?;
    storage.read_exact(into)?;
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
    /// The length of the image, which bounds every offset this pass computes
    /// out of the FAT. A slice answered that question by itself.
    length: u64,
}

impl Boot {
    fn read<S: Read + Seek>(storage: &mut S) -> Result<Self, Error> {
        let length = storage.seek(SeekFrom::End(0))?;
        if length < SECTOR_BYTES {
            return Err(Error("the image is too small to hold a boot sector".into()));
        }
        let mut sector = [0u8; SECTOR_BYTES as usize];
        read_at(storage, 0, &mut sector)?;

        let at16 = |at: usize| u64::from(u16::from_le_bytes([sector[at], sector[at + 1]]));
        let at32 = |at: usize| {
            u32::from_le_bytes([sector[at], sector[at + 1], sector[at + 2], sector[at + 3]])
        };

        let boot = Boot {
            bytes_per_sector: at16(0x0B),
            sectors_per_cluster: u64::from(sector[0x0D]),
            reserved_sectors: at16(0x0E),
            fats: u64::from(sector[0x10]),
            sectors_per_fat: u64::from(at32(0x24)),
            root_cluster: at32(0x2C),
            length,
        };

        if boot.bytes_per_sector == 0 || boot.sectors_per_cluster == 0 || boot.root_cluster < 2 {
            return Err(Error(
                "the filesystem just made does not describe itself".into(),
            ));
        }
        Ok(boot)
    }

    /// Where a cluster's data starts.
    fn offset(&self, cluster: u32) -> u64 {
        let sector = self.reserved_sectors
            + self.fats * self.sectors_per_fat
            + (u64::from(cluster) - 2) * self.sectors_per_cluster;
        sector * self.bytes_per_sector
    }

    fn cluster_bytes(&self) -> usize {
        (self.sectors_per_cluster * self.bytes_per_sector) as usize
    }

    /// The next cluster in a chain, while the chain continues.
    fn next<S: Read + Seek>(&self, storage: &mut S, cluster: u32) -> Result<Option<u32>, Error> {
        let at = self.reserved_sectors * self.bytes_per_sector + u64::from(cluster) * 4;
        if at + 4 > self.length {
            return Ok(None);
        }
        let mut slot = [0u8; 4];
        read_at(storage, at, &mut slot)?;
        let entry = u32::from_le_bytes(slot) & 0x0FFF_FFFF;
        Ok((2..0x0FFF_FFF7).contains(&entry).then_some(entry))
    }

    /// Every cluster a directory occupies, in order.
    fn chain<S: Read + Seek>(&self, storage: &mut S, start: u32) -> Result<Vec<u32>, Error> {
        let mut chain = vec![start];
        let mut at = start;
        // Bounded by what the image could hold, so a FAT pointing back into its
        // own chain stops here rather than running forever.
        let limit = (self.length / self.cluster_bytes().max(1) as u64 + 2) as usize;
        while let Some(next) = self.next(storage, at)? {
            if chain.len() > limit || chain.contains(&next) {
                break;
            }
            chain.push(next);
            at = next;
        }
        Ok(chain)
    }

    /// The first cluster of every subdirectory this one holds.
    fn children<S: Read + Seek>(&self, storage: &mut S, start: u32) -> Result<Vec<u32>, Error> {
        let mut children = Vec::new();
        let mut data = vec![0u8; self.cluster_bytes()];
        for cluster in self.chain(storage, start)? {
            let base = self.offset(cluster);
            if base + data.len() as u64 > self.length {
                continue;
            }
            read_at(storage, base, &mut data)?;
            for slot in data.chunks_exact(ENTRY) {
                match slot[0] {
                    0 => return Ok(children),
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
        Ok(children)
    }

    /// Rewrite one directory's opening four slots, if they are the shape
    /// `fatfs` leaves behind. Anything else is left alone.
    fn repair<S: Read + Write + Seek>(&self, storage: &mut S, cluster: u32) -> Result<(), Error> {
        let base = self.offset(cluster);
        let mut slots = [0u8; 4 * ENTRY];
        if base + slots.len() as u64 > self.length {
            return Ok(());
        }
        read_at(storage, base, &mut slots)?;

        let long = |slot: &[u8]| slot[11] == ATTR_LONG_NAME;
        let dot = |slot: &[u8], name: &[u8]| {
            slot[0] != DELETED && slot[11] & ATTR_DIRECTORY != 0 && &slot[..11] == name
        };
        if !(long(&slots[0..ENTRY])
            && dot(&slots[ENTRY..2 * ENTRY], b".          ")
            && long(&slots[2 * ENTRY..3 * ENTRY])
            && dot(&slots[3 * ENTRY..4 * ENTRY], b"..         "))
        {
            return Ok(());
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

        storage.seek(SeekFrom::Start(base))?;
        storage.write_all(&slots)?;
        Ok(())
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
    use super::{Audio, CONTENTS, Tracks, build, build_into, size};
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

    /// Streaming the audio in builds the same volume as handing it over whole.
    ///
    /// `build_into` is what a browser calls, and the file it streams out of is
    /// the one thing this crate cannot hold a copy of to compare. A reader over
    /// the same bytes stands in: if the image differs, the difference is in how
    /// the bytes were written rather than in what they were. One byte per
    /// write, so a builder that assumed one write per file fails here.
    #[test]
    fn streaming_the_audio_builds_the_same_image() {
        struct Streamed(Vec<(String, Vec<u8>)>);

        impl Tracks for Streamed {
            fn listing(&self) -> Vec<(String, u64)> {
                self.0
                    .iter()
                    .map(|(name, bytes)| (name.clone(), bytes.len() as u64))
                    .collect()
            }

            fn write(
                &mut self,
                index: usize,
                into: &mut dyn std::io::Write,
            ) -> std::io::Result<()> {
                for byte in &self.0[index].1 {
                    into.write_all(&[*byte])?;
                }
                Ok(())
            }
        }

        let name = "126_05A_Lange-Out_Of_The_Sky.flac";
        let bytes: Vec<u8> = (0..40_000u32).map(|n| n as u8).collect();
        let whole = build(
            &device(),
            &[Audio {
                file_name: name.into(),
                bytes: bytes.clone(),
            }],
            "MUSIC",
        )
        .unwrap();

        let mut tracks = Streamed(vec![(name.into(), bytes.clone())]);
        let mut storage =
            std::io::Cursor::new(vec![0u8; size(&device(), bytes.len() as u64) as usize]);
        build_into(&mut storage, &device(), &mut tracks, "MUSIC", &mut |_| {}).unwrap();

        let streamed = storage.into_inner();
        assert_eq!(
            streamed.len(),
            whole.len(),
            "{} bytes streamed against {} held",
            streamed.len(),
            whole.len()
        );
        assert_eq!(
            streamed.iter().zip(&whole).position(|(a, b)| a != b),
            None,
            "the two images differ"
        );
    }

    /// What a bar drawn from the build shows: every byte of the payload, once.
    ///
    /// The numbers a caller renders, so the test is the caller's: they only
    /// climb, they end on the payload, and they arrive often enough during one
    /// large file that a bar moves while a track is being written rather than
    /// jumping when it finishes.
    #[test]
    fn progress_climbs_to_the_payload_and_reports_inside_a_file() {
        let name = "174_11A_Rebelion-Bonkerz.wav";
        let bytes = vec![7u8; 10 * 1024 * 1024];
        let audio = [Audio {
            file_name: name.into(),
            bytes: bytes.clone(),
        }];
        let payload = device().len() as u64 + bytes.len() as u64;

        let mut reports: Vec<(u64, u64, String, usize, usize)> = Vec::new();
        let mut tracks: &[Audio] = &audio;
        let mut storage =
            std::io::Cursor::new(vec![0u8; size(&device(), bytes.len() as u64) as usize]);
        build_into(&mut storage, &device(), &mut tracks, "MUSIC", &mut |p| {
            reports.push((
                p.written_bytes,
                p.total_bytes,
                p.file_name.to_string(),
                p.file_index,
                p.file_count,
            ));
        })
        .unwrap();

        assert!(
            reports.windows(2).all(|pair| pair[0].0 <= pair[1].0),
            "the count went backwards: {:?}",
            reports.iter().map(|r| r.0).collect::<Vec<_>>()
        );
        let last = reports.last().expect("the build reported nothing");
        assert_eq!(
            (last.0, last.1),
            (payload, payload),
            "ended at {} of {}, with a payload of {payload}",
            last.0,
            last.1
        );
        assert_eq!(
            last.4,
            device().files.len() + 1,
            "counted {} files against {} databases and one track",
            last.4,
            device().files.len()
        );

        let inside = reports.iter().filter(|r| r.2.ends_with(name)).count();
        assert!(
            inside >= 2,
            "a {} MB track reported {inside} times, so a bar sits still while it is written",
            bytes.len() / (1024 * 1024)
        );
    }

    /// Storage shorter than the payload is refused before it is formatted.
    ///
    /// `fatfs` counts the sectors it was handed, so the alternative is a volume
    /// that formats, half fills, and fails on a track with no space left.
    #[test]
    fn storage_too_small_for_the_payload_says_both_numbers() {
        let audio = [Audio {
            file_name: "128_08A_Track.wav".into(),
            bytes: vec![3u8; 8192],
        }];
        let mut storage = std::io::Cursor::new(vec![0u8; 4096]);
        let mut tracks: &[Audio] = &audio;
        let error = build_into(&mut storage, &device(), &mut tracks, "MUSIC", &mut |_| {})
            .expect_err("a 4 KB image accepted 8 KB of audio")
            .0;
        assert!(error.contains("4096") && error.contains("8204"), "{error}");
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
