//! Reading the archives as they were downloaded, however many there are.
//!
//! A handle rather than a function over a byte slice, and the difference
//! matters at the size these archives are. A Beatport month is a gigabyte or
//! more; handing it to a listing call and then again to each of thirty
//! extractions copies it into wasm memory thirty-one times, and the first copy
//! alone is a quarter of the address space. Here the zip is read out of the
//! file it already sits in, so nothing larger than one entry's buffer exists at
//! any point.
//!
//! Several archives rather than one, because a collection is not a download. A
//! month arrives in one zip, the two tracks bought the following week arrive in
//! another, and a device holds the lot. [`Archives::add`] appends; the listing
//! spans everything added and says which archive each entry came from, so the
//! page numbers tracks once across the whole set rather than once per file.

use crate::opfs;
use crate::source::{self, Source};
use serde::Serialize;
use std::io::{BufReader, BufWriter, Read, Seek, Write};
use wasm_bindgen::prelude::*;

/// How much of an archive's central directory and entry stream is read per call
/// into the browser. Parsing a zip is many small reads at known offsets, and
/// unbuffered that is one JavaScript call per field.
const BUFFER: usize = 256 * 1024;

/// One file inside an archive that is worth analysing.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    /// Which archive holds it, as an index into the order they were added.
    /// [`Archives::extract`] takes this back.
    pub archive: usize,
    /// What that archive was called, so a listing can say where a track came
    /// from without the page keeping a second index.
    pub archive_name: String,
    /// The entry as the archive keys it, directories and all, which is what
    /// [`Archives::extract`] takes back.
    pub name: String,
    /// The same without the directories the shop zipped it under.
    ///
    /// This is the name to analyse under and the one a device name is built
    /// from. `name` carries `Beatport/Artist-Title.wav`, and a device file
    /// called `128_07A_Beatport/Artist-Title.wav` is a directory on the stick
    /// rather than a track.
    pub file_name: String,
    pub size: u64,
}

/// One archive held open.
struct Open {
    name: String,
    zip: zip::ZipArchive<BufReader<source::Reader>>,
}

/// Every archive a session has been given, in the order they arrived.
#[wasm_bindgen]
#[derive(Default)]
pub struct Archives {
    open: Vec<Open>,
}

#[wasm_bindgen]
impl Archives {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Archives {
        Archives::default()
    }

    /// Add one archive, and hand back how many tracks it contributed.
    ///
    /// `name` is what the archive is called, which is the only thing that
    /// distinguishes two downloads holding a track of the same name. `file` is
    /// the zip as the person picked it or as the page put it in the
    /// origin-private filesystem, and it stays readable for as long as this
    /// object lives, because every extraction reads through it.
    pub fn add(&mut self, name: &str, file: &Source) -> Result<usize, JsError> {
        let reader = BufReader::with_capacity(BUFFER, source::open(file)?);
        let mut zip = zip::ZipArchive::new(reader)
            .map_err(|e| JsError::new(&format!("{name} is not a readable zip: {e}")))?;
        let tracks = audio_entries(&mut zip, self.open.len(), name)?.len();
        self.open.push(Open {
            name: name.to_string(),
            zip,
        });
        Ok(tracks)
    }

    /// How many archives are held.
    #[wasm_bindgen(getter)]
    pub fn count(&self) -> usize {
        self.open.len()
    }

    /// Every audio file in every archive, each archive in the order it was
    /// added and each entry in the order its archive lists them.
    ///
    /// Directories, resource forks and the shop's PDF receipt are left out:
    /// what comes back is what the analyser can read.
    pub fn entries(&mut self) -> Result<JsValue, JsError> {
        let mut found = Vec::new();
        for (index, open) in self.open.iter_mut().enumerate() {
            let name = open.name.clone();
            found.extend(audio_entries(&mut open.zip, index, &name)?);
        }
        Ok(found.serialize(&crate::boundary())?)
    }

    /// Extract one entry into `into`, and hand back how many bytes it wrote.
    ///
    /// The bytes never enter wasm memory whole: the decompressor reads from one
    /// file and writes into another a buffer at a time. `into` is left holding
    /// exactly the entry, so a handle reused from an earlier extraction carries
    /// nothing of it.
    pub fn extract(
        &mut self,
        archive: usize,
        name: &str,
        into: &opfs::SyncHandle,
    ) -> Result<f64, JsError> {
        let open = self
            .open
            .get_mut(archive)
            .ok_or_else(|| JsError::new(&format!("there is no archive {archive}")))?;
        let mut entry = open
            .zip
            .by_name(name)
            .map_err(|e| JsError::new(&format!("{name} is not in {}: {e}", open.name)))?;

        let mut file = opfs::File::new(into.clone());
        file.truncate(0)
            .map_err(|e| JsError::new(&format!("{name} could not be given a file: {e}")))?;
        let mut sink = BufWriter::with_capacity(BUFFER, file);
        let written = std::io::copy(&mut entry, &mut sink)
            .map_err(|e| JsError::new(&format!("{name} would not extract: {e}")))?;
        sink.flush()
            .map_err(|e| JsError::new(&format!("{name} would not extract: {e}")))?;
        Ok(written as f64)
    }
}

/// The audio entries of one archive, tagged with where they came from.
fn audio_entries<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
    archive: usize,
    archive_name: &str,
) -> Result<Vec<Entry>, JsError> {
    let mut found = Vec::new();
    for index in 0..zip.len() {
        let file = zip.by_index(index).map_err(|e| {
            JsError::new(&format!(
                "entry {index} of {archive_name} could not be read: {e}"
            ))
        })?;
        if !file.is_file() {
            continue;
        }
        let name = file.name().to_string();
        if is_audio(&name) {
            found.push(Entry {
                archive,
                archive_name: archive_name.to_string(),
                file_name: name.rsplit('/').next().unwrap_or(&name).to_string(),
                name,
                size: file.size(),
            });
        }
    }
    Ok(found)
}

/// Whether a name inside the archive is a track.
///
/// macOS writes `__MACOSX/._Artist-Title.aiff` beside every file it zips, which
/// carries the extension and none of the audio.
fn is_audio(name: &str) -> bool {
    let file_name = name.rsplit('/').next().unwrap_or(name);
    if file_name.starts_with("._") || name.starts_with("__MACOSX/") {
        return false;
    }
    matches!(
        file_name
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_ascii_lowercase())
            .as_deref(),
        Some("wav" | "aiff" | "aif" | "flac" | "mp3")
    )
}
