//! DeviceSQL strings.
//!
//! Two encodings in one type. A short ASCII string is a length byte and the
//! bytes; anything longer or non-ASCII gets a flag byte, a length, a pad byte
//! and the body, in ASCII or UTF-16LE. The player picks them apart by the low
//! bit of the first byte, which is set only in the short form.
//!
//! See <https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/exports.html#devicesql-strings>.

/// Longest string that fits the short form: the length is `(len + 1) << 1 | 1`
/// in one byte, so the length itself has seven bits minus the one it borrows.
const MAX_SHORT: usize = ((u8::MAX >> 1) - 1) as usize;

const FLAG_ASCII: u8 = 0x40;
const FLAG_UTF16: u8 = 0x90;

/// Encode a string the way DeviceSQL stores it.
pub fn encode(text: &str) -> Vec<u8> {
    if text.is_ascii() && text.len() <= MAX_SHORT {
        let mut out = Vec::with_capacity(text.len() + 1);
        out.push((((text.len() + 1) << 1) | 1) as u8);
        out.extend_from_slice(text.as_bytes());
        return out;
    }

    let (flags, body) = if text.is_ascii() {
        (FLAG_ASCII, text.as_bytes().to_vec())
    } else {
        (
            FLAG_UTF16,
            text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
        )
    };

    let mut out = Vec::with_capacity(body.len() + 4);
    out.push(flags);
    // The length counts the four bytes of its own header.
    out.extend_from_slice(&((body.len() + 4) as u16).to_le_bytes());
    out.push(0);
    out.extend_from_slice(&body);
    out
}

/// Alignment a string needs inside a row's offset array.
///
/// UTF-16 bodies are read as `u16` pairs and the parser seeks straight to the
/// offset, so an odd one splits every character. Four rather than two because
/// that is what rekordbox writes. Any non-ASCII string takes that encoding,
/// however short it is.
pub fn alignment(text: &str) -> usize {
    if text.is_ascii() { 1 } else { 4 }
}

/// The empty string, which is what most of a track row's twenty-one strings are.
pub fn empty() -> Vec<u8> {
    encode("")
}
