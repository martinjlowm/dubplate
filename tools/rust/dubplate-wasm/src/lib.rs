//! The analysis, the two databases and the image, as a browser can call them.
//!
//! Everything here is a boundary and nothing here is a decision. Each function
//! takes a file and settings and hands back what the equivalent CLI subcommand
//! writes to disk, so a page and a terminal measure the same file the same way
//! and there is no second implementation of anything to keep in step.
//!
//! A file, not its bytes. An input is a file the person picked or one in the
//! origin-private filesystem, an output is always the second, and both are read
//! and written here through [`std::io`]. Handing a gigabyte archive or a
//! three-gigabyte image across the boundary as an array means holding it twice
//! in an address space that is four gigabytes wide, and it is the job rather
//! than the track that is large: one track's samples are what the analysis
//! needs in memory, and one track's samples are all it ever holds.
//!
//! The analysis is stateless. A page that runs eight of these at once runs
//! eight workers, and one track in is one report out. The device is not, and
//! says why where it is declared.

mod archive;
mod convert;
mod opfs;
mod source;

use collection::{Collection, Playlist};
use pipeline::AnalysisOptions;
use serde::Serialize;
use std::io::{self, Read, Seek, SeekFrom, Write};
use wasm_bindgen::prelude::*;

/// How everything here crosses the boundary.
///
/// `json_compatible`, not the default. serde-wasm-bindgen writes a Rust map as a
/// JavaScript `Map` unless told otherwise, and the report carries one: it is a
/// `serde_json::Value`, so its every object arrives as a `Map`. A `Map` is not
/// what a caller reads a struct out of and not what `JSON.stringify` writes, so
/// the report would arrive unreadable and stringify to `{}`.
pub(crate) fn boundary() -> serde_wasm_bindgen::Serializer {
    serde_wasm_bindgen::Serializer::json_compatible()
}

pub use archive::{Archives, Entry};
pub use opfs::SyncHandle;
pub use source::Source;

/// Turn a Rust panic into something the console names.
///
/// Deliberately not `#[wasm_bindgen(start)]`. A module has one start slot, and it
/// belongs to whoever builds the binary: a library that claims it silently
/// replaces the caller's own entry point, so a worker whose `main` attaches a
/// message handler loads, initialises, and then answers nothing. Callers invoke
/// this themselves, or set the hook their own way.
pub fn start() {
    console_error_panic_hook::set_once();
}

/// The settings a run used, with every default filled in.
///
/// A page renders its form from this rather than from a copy of the numbers, so
/// a default changed in `pipeline` changes the form.
#[wasm_bindgen(js_name = defaultOptions)]
pub fn default_options() -> Result<JsValue, JsError> {
    Ok(AnalysisOptions::default().serialize(&boundary())?)
}

/// Analyse one track and hand back everything the report directory holds.
///
/// `source` is the track: a file the person picked, or one
/// [`Archives::extract`] wrote. Either is read once, and that read is the only
/// copy of the encoded file that exists: the decoder holds it until the samples
/// are out, and then the samples are what the stages work over.
///
/// `figures` decides whether the plots are drawn. Drawing them roughly doubles
/// the work, and a page that only wants a tempo and a key should not pay for a
/// spectrogram nobody opened.
#[wasm_bindgen]
pub fn analyze(
    file_name: &str,
    source: &Source,
    options: JsValue,
    figures: bool,
) -> Result<JsValue, JsError> {
    let options: AnalysisOptions = if options.is_undefined() || options.is_null() {
        AnalysisOptions::default()
    } else {
        serde_wasm_bindgen::from_value(options)?
    };

    let encoded = read_whole(source::open(source)?)
        .map_err(|e| JsError::new(&format!("{file_name}: {e}")))?;
    let decoded = audio::Audio::from_encoded(encoded, file_name)
        .map_err(|e| JsError::new(&format!("{file_name}: {e}")))?;
    let duration = decoded.duration_seconds();

    let outcome = pipeline::run(
        &decoded,
        &options,
        pipeline::Source {
            path: file_name.to_string(),
            duration_seconds: duration,
            start_seconds: 0.0,
        },
        if figures {
            pipeline::Figures::Drawn
        } else {
            pipeline::Figures::Skipped
        },
    )
    .map_err(|e| JsError::new(&format!("{file_name}: {e}")))?;

    let drawn = figures.then(|| {
        // The same window the CLI defaults to: a quarter of the way in, which
        // for most tracks is past the intro.
        let source = &outcome.report.source;
        let start = source.analysed_start_seconds + source.analysed_seconds * 0.25;
        convert::figures(pipeline::figures(&outcome, start, 12.0))
    });

    Ok(convert::Analysed {
        file_name: file_name.to_string(),
        export_name: collection::naming::target_name(
            file_name,
            outcome.report.tempo.bpm,
            &outcome.report.key.camelot,
        ),
        report: serde_json::from_str(&outcome.report.to_json()?)?,
        figures: drawn,
    }
    .serialize(&boundary())?)
}

/// One whole file, with the buffer sized before it is filled.
///
/// The decoder needs the encoded file in one piece: symphonia reads through a
/// `MediaSource`, which is `Send + Sync` and so cannot borrow a browser's file.
/// This is the one place a whole file is held, and it is one track rather than a
/// collection.
fn read_whole(mut file: source::Reader) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(file.len()? as usize);
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// One track as it sits on the device, and the file its bytes are still in.
struct Held {
    file_name: String,
    size: u64,
    file: source::Reader,
}

/// What the image builder reads the audio out of.
///
/// A named type rather than a `Vec` because the trait and the `Vec` are both
/// somebody else's, and the order it hands out matters: it is the order the
/// tracks were added, which is the order both databases numbered them in.
#[derive(Default)]
struct Placed(Vec<Held>);

impl image::Tracks for Placed {
    fn listing(&self) -> Vec<(String, u64)> {
        self.0
            .iter()
            .map(|held| (held.file_name.clone(), held.size))
            .collect()
    }

    fn write(&mut self, index: usize, into: &mut dyn Write) -> io::Result<()> {
        let held = &mut self.0[index];
        held.file.seek(SeekFrom::Start(0))?;

        // A megabyte at a time rather than through `io::copy`, whose buffer is
        // eight kilobytes: a ninety-megabyte track is twelve thousand calls
        // into the browser that way and ninety this way.
        let mut buffer = vec![0u8; 1 << 20];
        let mut copied = 0u64;
        loop {
            let read = held.file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            into.write_all(&buffer[..read])?;
            copied += read as u64;
        }

        // Both databases store the length this track was measured at, and a
        // player reading past the end of a short file is the symptom. Caught
        // here, where the two numbers are side by side.
        if copied != held.size {
            return Err(io::Error::other(format!(
                "{} is {copied} bytes and the databases say {}",
                held.file_name, held.size
            )));
        }
        Ok(())
    }
}

/// A device being assembled, one analysed track at a time.
///
/// Stateful where the analysis is not, because the alternative is handing every
/// track across the boundary again at the end. What it keeps per track is a
/// reference to the file and a length, not the audio: thirty tracks are three
/// gigabytes and the image is another three, and neither is ever in wasm
/// memory.
///
/// Every file added has to stay readable until the image is built, which for a
/// sync access handle means the page leaves it open. That is what lets the
/// image be written without extracting anything a second time.
#[wasm_bindgen]
pub struct Device {
    tracks: Vec<collection::Track>,
    audio: Placed,
}

#[wasm_bindgen]
impl Device {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Device {
        Device {
            tracks: Vec::new(),
            audio: Placed::default(),
        }
    }

    /// Add one analysed track: the file its audio is in, and the report of
    /// analysing it.
    ///
    /// The same file `analyze` read, whether that is one the person picked or
    /// one [`Archives::extract`] wrote. It is read again when the image is
    /// built and not before, so what this keeps is a reference and a length.
    ///
    /// `export_name` is what the file is called on the device, which is what
    /// both databases store. Pass what `analyze` returned rather than the
    /// original name, or a player will show a listing that does not sort.
    pub fn add(&mut self, export_name: &str, file: &Source, report: &str) -> Result<(), JsError> {
        // `Track::device_path` is the only thing that decides where a file sits
        // on the device, and a name carrying a slash decides it a second time:
        // the image builder makes the directory it names and a player lists a
        // folder where the track should be. An archive keys its entries by
        // path, so this is one `entry.name` away at all times.
        if export_name.is_empty() || export_name.contains('/') {
            return Err(JsError::new(&format!(
                "{export_name} is a path rather than a file name; pass the entry's fileName"
            )));
        }

        let file = source::open(file)?;
        let size = file
            .len()
            .map_err(|e| JsError::new(&format!("{export_name}: {e}")))?;

        let track =
            collection::report::parse(export_name, size, report.as_bytes(), image::CONTENTS_PATH)
                .map_err(|e| JsError::new(&format!("{export_name}: {e}")))?;

        self.audio.0.push(Held {
            file_name: export_name.to_string(),
            size,
            file,
        });
        self.tracks.push(track);
        Ok(())
    }

    #[wasm_bindgen(getter)]
    pub fn count(&self) -> usize {
        self.tracks.len()
    }

    /// Write the finished `.img` into `into`, and hand back its length.
    ///
    /// One FAT32 filesystem holding both databases and every track added. The
    /// file is cut to nothing and then grown to the size the payload needs,
    /// which is what makes the free clusters read as zeroes: they are the one
    /// part of the volume nothing writes, and leftovers in them are what make
    /// one collection build to two different images.
    ///
    /// `date` is the date recorded against every track, as `YYYY-MM-DD`. It is
    /// a parameter rather than a clock reading so that the same collection
    /// built twice is the same file twice.
    pub fn image(
        &mut self,
        into: &SyncHandle,
        label: &str,
        playlist: &str,
        date: &str,
        target: &str,
    ) -> Result<f64, JsError> {
        if self.tracks.is_empty() {
            return Err(JsError::new("no analysed tracks to write"));
        }

        let collection = Collection {
            tracks: self.tracks.clone(),
            playlists: vec![Playlist {
                name: playlist.to_string(),
                tracks: (0..self.tracks.len()).collect(),
            }],
        };

        let mut device = collection::sink::Memory::new();
        if matches!(target, "rekordbox" | "both") {
            rekordbox::write_device_to(
                &mut device,
                &collection,
                &rekordbox::Options {
                    date: date.to_string(),
                },
            )
            .map_err(|e| JsError::new(&format!("writing the rekordbox database: {e}")))?;
        }
        if matches!(target, "engine" | "both") {
            engine::write_device_to(
                &mut device,
                &collection,
                &engine::Options {
                    date: date.to_string(),
                },
            )
            .map_err(|e| JsError::new(&format!("writing the Engine Library database: {e}")))?;
        }

        let audio_bytes = self.audio.0.iter().map(|held| held.size).sum();
        let size = image::size(&device, audio_bytes);

        let mut storage = opfs::File::new(into.clone());
        storage
            .truncate(0)
            .and_then(|()| storage.truncate(size))
            .map_err(|e| JsError::new(&format!("could not make room for the image: {e}")))?;

        image::build_into(&mut storage, &device, &mut self.audio, label)
            .map_err(|e| JsError::new(&format!("building the image: {e}")))?;
        storage
            .flush()
            .map_err(|e| JsError::new(&format!("could not finish writing the image: {e}")))?;

        Ok(size as f64)
    }
}

impl Default for Device {
    fn default() -> Self {
        Device::new()
    }
}
