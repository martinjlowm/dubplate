//! The analysis, the two databases and the image, as a browser can call them.
//!
//! Everything here is a boundary and nothing here is a decision. Each function
//! takes bytes and settings and hands back what the equivalent CLI subcommand
//! writes to disk, so a page and a terminal measure the same file the same way
//! and there is no second implementation of anything to keep in step.
//!
//! Stateless on purpose. A page that runs eight of these at once runs eight
//! workers, and a worker that holds an archive is a worker holding a copy of it.
//! One track in, one report out.

mod archive;
mod convert;

use collection::{Collection, Playlist};
use pipeline::AnalysisOptions;
use wasm_bindgen::prelude::*;

pub use archive::{Archive, Entry};

/// Turn a Rust panic into something the console names.
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
}

/// The settings a run used, with every default filled in.
///
/// A page renders its form from this rather than from a copy of the numbers, so
/// a default changed in `pipeline` changes the form.
#[wasm_bindgen(js_name = defaultOptions)]
pub fn default_options() -> Result<JsValue, JsError> {
    Ok(serde_wasm_bindgen::to_value(&AnalysisOptions::default())?)
}

/// Analyse one track and hand back everything the report directory holds.
///
/// `figures` decides whether the plots are drawn. Drawing them roughly doubles
/// the work, and a page that only wants a tempo and a key should not pay for a
/// spectrogram nobody opened.
#[wasm_bindgen]
pub fn analyze(
    file_name: &str,
    bytes: &[u8],
    options: JsValue,
    figures: bool,
) -> Result<JsValue, JsError> {
    let options: AnalysisOptions = if options.is_undefined() || options.is_null() {
        AnalysisOptions::default()
    } else {
        serde_wasm_bindgen::from_value(options)?
    };

    let decoded = audio::Audio::from_encoded_bytes(bytes, file_name)
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
    )
    .map_err(|e| JsError::new(&format!("{file_name}: {e}")))?;

    let drawn = figures.then(|| {
        // The same window the CLI defaults to: a quarter of the way in, which
        // for most tracks is past the intro.
        let source = &outcome.report.source;
        let start = source.analysed_start_seconds + source.analysed_seconds * 0.25;
        convert::figures(pipeline::figures(&outcome, start, 12.0))
    });

    Ok(serde_wasm_bindgen::to_value(&convert::Analysed {
        file_name: file_name.to_string(),
        export_name: collection::naming::target_name(
            file_name,
            outcome.report.tempo.bpm,
            &outcome.report.key.camelot,
        ),
        report: serde_json::from_str(&outcome.report.to_json()?)?,
        figures: drawn,
    })?)
}

/// A device being assembled, one analysed track at a time.
///
/// Stateful where the analysis is not, because the alternative is handing every
/// track's audio across the boundary again at the end. A track is added once,
/// and the image is built from what was added.
#[wasm_bindgen]
pub struct Device {
    tracks: Vec<collection::Track>,
    audio: Vec<image::Audio>,
}

#[wasm_bindgen]
impl Device {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Device {
        Device {
            tracks: Vec::new(),
            audio: Vec::new(),
        }
    }

    /// Add one analysed track: the audio as it was uploaded, and the report of
    /// analysing it.
    ///
    /// `export_name` is what the file is called on the device, which is what
    /// both databases store. Pass what `analyze` returned rather than the
    /// original name, or a player will show a listing that does not sort.
    pub fn add(&mut self, export_name: &str, bytes: Vec<u8>, report: &str) -> Result<(), JsError> {
        let track = collection::report::parse(
            export_name,
            bytes.len() as u64,
            report.as_bytes(),
            image::CONTENTS_PATH,
        )
        .map_err(|e| JsError::new(&format!("{export_name}: {e}")))?;

        self.audio.push(image::Audio {
            file_name: export_name.to_string(),
            bytes,
        });
        self.tracks.push(track);
        Ok(())
    }

    #[wasm_bindgen(getter)]
    pub fn count(&self) -> usize {
        self.tracks.len()
    }

    /// The finished `.img`: one FAT32 filesystem holding both databases and
    /// every track added.
    ///
    /// `date` is the date recorded against every track, as `YYYY-MM-DD`. It is
    /// a parameter rather than a clock reading so that the same collection
    /// built twice is the same file twice.
    pub fn image(
        &self,
        label: &str,
        playlist: &str,
        date: &str,
        target: &str,
    ) -> Result<Vec<u8>, JsError> {
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

        image::build(&device, &self.audio, label)
            .map_err(|e| JsError::new(&format!("building the image: {e}")))
    }
}

impl Default for Device {
    fn default() -> Self {
        Device::new()
    }
}
