//! Reading the zip as it was downloaded.
//!
//! A handle rather than two functions over a byte slice, and the difference
//! matters at the size these archives are. A Beatport month is a gigabyte or
//! more; passing it to a listing call and then again to each of thirty
//! extractions copies it into wasm memory thirty-one times. Opened once, it is
//! copied once, and every entry after that is a read out of the copy.
//!
//! The page holds one of these in one worker and sends the extracted track to
//! whichever worker is free, so the archive exists once in the tab rather than
//! once per core.

use serde::Serialize;
use wasm_bindgen::prelude::*;

/// One file inside the archive that is worth analysing.
#[derive(Serialize)]
pub struct Entry {
    pub name: String,
    pub size: u64,
}

/// An archive held open.
#[wasm_bindgen]
pub struct Archive {
    zip: zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
}

#[wasm_bindgen]
impl Archive {
    #[wasm_bindgen(constructor)]
    pub fn new(bytes: Vec<u8>) -> Result<Archive, JsError> {
        let zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
            .map_err(|e| JsError::new(&format!("this is not a readable zip: {e}")))?;
        Ok(Archive { zip })
    }

    /// Every audio file in the archive, in the order the archive lists them.
    ///
    /// Directories, resource forks and the shop's PDF receipt are left out: what
    /// comes back is what the analyser can read.
    pub fn entries(&mut self) -> Result<JsValue, JsError> {
        let mut found = Vec::new();
        for index in 0..self.zip.len() {
            let file = self
                .zip
                .by_index(index)
                .map_err(|e| JsError::new(&format!("entry {index} could not be read: {e}")))?;
            if !file.is_file() {
                continue;
            }
            let name = file.name().to_string();
            if is_audio(&name) {
                found.push(Entry {
                    name,
                    size: file.size(),
                });
            }
        }
        Ok(serde_wasm_bindgen::to_value(&found)?)
    }

    /// One entry's bytes, by the name [`Archive::entries`] gave.
    pub fn entry(&mut self, name: &str) -> Result<Vec<u8>, JsError> {
        let mut file = self
            .zip
            .by_name(name)
            .map_err(|e| JsError::new(&format!("{name} is not in the archive: {e}")))?;
        let mut bytes = Vec::with_capacity(file.size() as usize);
        std::io::copy(&mut file, &mut bytes)
            .map_err(|e| JsError::new(&format!("{name} would not extract: {e}")))?;
        Ok(bytes)
    }
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
