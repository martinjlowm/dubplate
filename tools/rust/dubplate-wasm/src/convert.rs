//! What crosses the boundary, and how the binary parts of it get there.

use serde::Serialize;
use std::collections::BTreeMap;

/// One analysed track, as a page reads it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Analysed {
    pub file_name: String,
    /// What the file is called on a device: `126_05A_Artist-Title.flac`. `None`
    /// when the tempo or the key could not be written into a name, which is a
    /// measurement that failed rather than a naming problem.
    pub export_name: Option<String>,
    /// The whole of `report.json`, unaltered. A page reads what it needs and the
    /// rest is there for the ones that want more.
    pub report: serde_json::Value,
    pub figures: Option<Figures>,
}

/// The plots, ready to put in a page.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Figures {
    /// SVG source by file name, e.g. `novelty.svg`. Inline rather than a blob
    /// URL: an SVG in the document inherits the page's own colours.
    pub svg: BTreeMap<String, String>,
    /// The spectrogram, as a `data:` URL. A PNG is the one figure that is a
    /// pixel grid rather than a drawing, and a URL is what an `img` takes.
    pub png: BTreeMap<String, String>,
}

pub fn figures(artefacts: pipeline::Artefacts) -> Figures {
    Figures {
        svg: artefacts.text.into_iter().collect(),
        png: artefacts
            .binary
            .into_iter()
            .map(|(name, bytes)| (name, format!("data:image/png;base64,{}", base64(&bytes))))
            .collect(),
    }
}

/// Base64, because pulling in a crate to write twenty lines that have no
/// decoder and no configuration is a dependency for its own sake.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let block = chunk.iter().enumerate().fold(0u32, |block, (index, byte)| {
            block | (u32::from(*byte) << (16 - 8 * index))
        });
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(ALPHABET[(block >> (18 - 6 * index)) as usize & 0x3F] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn every_padding_case_matches_the_alphabet() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        // The two bytes a PNG header starts with, which is what this is for.
        assert_eq!(base64(&[0x89, 0x50, 0x4E, 0x47]), "iVBORw==");
    }
}
