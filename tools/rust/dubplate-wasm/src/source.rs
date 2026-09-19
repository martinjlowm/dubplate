//! What a page can hand over as a file to read.
//!
//! Two things are a file in a browser and neither is the other. A
//! `FileSystemSyncAccessHandle` is one the page made, which is where an
//! extracted track and a built image live. A `File` from a picker is one the
//! person already has on disk, and copying a ninety-megabyte track into the
//! origin-private filesystem in order to read it is a copy for nothing:
//! `FileReaderSync` reads a slice of a `Blob` in a worker without a promise,
//! which is the same door the sync access handle opens.
//!
//! So a single track, a whole archive and an extracted entry all arrive the
//! same way, and every caller reads through [`std::io`]. Only the outputs are
//! handles and nothing else, because a picked file cannot be written to.

use crate::opfs;
use std::io::{self, Read, Seek, SeekFrom};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

/// The most one read hands to the browser at a time, for the same reason
/// [`opfs`] caps its own.
const CHUNK: usize = 1 << 20;

#[wasm_bindgen]
extern "C" {
    /// A file, as either of the two things a page has.
    #[wasm_bindgen(typescript_type = "FileSystemSyncAccessHandle | Blob")]
    #[derive(Clone)]
    pub type Source;
}

/// Whichever of the two a page handed over.
pub enum Reader {
    Handle(opfs::File),
    Picked(Picked),
}

impl Reader {
    /// The length of the file, which is what both databases record and what
    /// sizes the volume an image is written into.
    pub fn len(&self) -> io::Result<u64> {
        match self {
            Reader::Handle(file) => file.len(),
            Reader::Picked(picked) => Ok(picked.length),
        }
    }
}

impl Read for Reader {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        match self {
            Reader::Handle(file) => file.read(into),
            Reader::Picked(picked) => picked.read(into),
        }
    }
}

impl Seek for Reader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        match self {
            Reader::Handle(file) => file.seek(to),
            Reader::Picked(picked) => picked.seek(to),
        }
    }
}

/// Open whatever `source` is.
///
/// A `Blob` is recognised by its own type, a handle by the method no `Blob`
/// carries. Duck typing for the second because `FileSystemSyncAccessHandle` is
/// declared here rather than taken from `web-sys`, which gates it behind a flag
/// every crate in the build would have to be compiled with.
pub fn open(source: &Source) -> Result<Reader, JsError> {
    let value: &JsValue = source.as_ref();

    if let Some(blob) = value.dyn_ref::<web_sys::Blob>() {
        return Ok(Reader::Picked(Picked::new(blob.clone())?));
    }
    if js_sys::Reflect::get(value, &JsValue::from_str("getSize")).is_ok_and(|it| it.is_function()) {
        // Checked rather than assumed: the method that reads the length is the
        // one every later read depends on.
        return Ok(Reader::Handle(opfs::File::new(
            value.clone().unchecked_into(),
        )));
    }
    Err(JsError::new(
        "a file here is an open FileSystemSyncAccessHandle or a File from a picker",
    ))
}

/// A file the person picked, read a slice at a time.
pub struct Picked {
    blob: web_sys::Blob,
    reader: web_sys::FileReaderSync,
    /// A `Blob` cannot change length, so this is read once. The handle's is
    /// not, and is read on every call that needs it.
    length: u64,
    position: u64,
}

impl Picked {
    fn new(blob: web_sys::Blob) -> Result<Picked, JsError> {
        let reader = web_sys::FileReaderSync::new().map_err(|_| {
            JsError::new("FileReaderSync is a worker API; a picked file can only be read in one")
        })?;
        Ok(Picked {
            length: blob.size() as u64,
            blob,
            reader,
            position: 0,
        })
    }
}

impl Read for Picked {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        let want = into.len().min(CHUNK) as u64;
        let end = self.position.saturating_add(want).min(self.length);
        if end <= self.position {
            return Ok(0);
        }

        let slice = self
            .blob
            .slice_with_f64_and_f64(self.position as f64, end as f64)
            .map_err(|_| io::Error::other("the file could not be sliced"))?;
        let buffer = self.reader.read_as_array_buffer(&slice).map_err(|_| {
            io::Error::other(
                "the file could not be read; it may have changed on disk since it was picked",
            )
        })?;

        let got = js_sys::Uint8Array::new(&buffer);
        let read = got.length() as usize;
        got.copy_to(&mut into[..read]);
        self.position += read as u64;
        Ok(read)
    }
}

impl Seek for Picked {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let (from, by) = match to {
            SeekFrom::Start(at) => {
                self.position = at;
                return Ok(at);
            }
            SeekFrom::Current(by) => (self.position, by),
            SeekFrom::End(by) => (self.length, by),
        };
        self.position = from.checked_add_signed(by).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("a seek to {by} from {from} lands outside the file"),
            )
        })?;
        Ok(self.position)
    }
}
