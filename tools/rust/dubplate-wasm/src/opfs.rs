//! A file in the origin-private filesystem, as `std::io` sees it.
//!
//! `FileSystemSyncAccessHandle` is the one file interface a browser answers
//! without a promise: reading, writing, measuring and truncating all return
//! rather than resolve. That is `std::io::Read + Write + Seek` once a position
//! is kept alongside it, and those are the traits every crate below this one
//! already reads and writes through, so an archive can be parsed out of a file
//! and an image written into one without either being a `Vec` in wasm memory.
//! A gigabyte archive, thirty tracks and the three-gigabyte image they build do
//! not fit in a 32-bit address space, and before this all three were a `Vec`.
//!
//! The page opens the handle and the page closes it. A worker cannot open one
//! itself, because `getFileHandle` and `createSyncAccessHandle` return promises
//! and nothing here can await; every function that wants a file takes one that
//! is already open. That is the division the CLI already has, where `main`
//! opens the file and the libraries take what is in it.

use std::io::{self, Read, Seek, SeekFrom, Write};
use wasm_bindgen::prelude::*;

/// The most one read or write hands to the browser at a time.
///
/// `read` is allowed to fill less of the buffer than it was given, and this
/// takes that permission: `read_to_end` over a ninety-megabyte track offers its
/// whole spare capacity, and answering that in one call would mean a
/// ninety-megabyte `Uint8Array` beside the `Vec` it is filling. A megabyte is
/// large enough that the per-call cost of crossing into JavaScript disappears
/// against the copy.
const CHUNK: usize = 1 << 20;

#[wasm_bindgen]
extern "C" {
    /// An open file in the origin-private filesystem.
    ///
    /// `Clone` clones the reference, not the file. Two of these address one
    /// file, which is what lets a track be read while the image it goes into is
    /// written, and it is the page that opened the handle that closes it.
    #[wasm_bindgen(typescript_type = "FileSystemSyncAccessHandle")]
    #[derive(Clone)]
    pub type SyncHandle;

    #[wasm_bindgen(method, js_name = getSize, catch)]
    fn get_size(this: &SyncHandle) -> Result<f64, JsValue>;

    #[wasm_bindgen(method, catch)]
    fn read(this: &SyncHandle, into: &js_sys::Uint8Array, at: &JsValue) -> Result<f64, JsValue>;

    #[wasm_bindgen(method, catch)]
    fn write(this: &SyncHandle, from: &js_sys::Uint8Array, at: &JsValue) -> Result<f64, JsValue>;

    #[wasm_bindgen(method, catch)]
    fn truncate(this: &SyncHandle, to: f64) -> Result<(), JsValue>;

    #[wasm_bindgen(method, catch)]
    fn flush(this: &SyncHandle) -> Result<(), JsValue>;
}

/// An open file, with a position of its own.
///
/// The handle has no cursor: every read and write says where it starts. So the
/// position lives here, which is what makes `Seek` mean anything and what lets
/// two of these sit over one file without either moving the other.
pub struct File {
    handle: SyncHandle,
    position: u64,
    /// The buffer both directions copy through, on the JavaScript heap, grown
    /// to the largest chunk asked for and then reused.
    ///
    /// A view onto wasm memory would save the copy and is what `Uint8Array::view`
    /// is for, but it is an `unsafe` call whose result any allocation in between
    /// can leave pointing at freed memory, and what it saves is one megabyte
    /// copied between two buffers of the same size.
    scratch: js_sys::Uint8Array,
    /// The `{ at }` the handle wants on every call, allocated once and
    /// rewritten, rather than one object per megabyte.
    options: js_sys::Object,
}

impl File {
    pub fn new(handle: SyncHandle) -> File {
        File {
            handle,
            position: 0,
            scratch: js_sys::Uint8Array::new_with_length(0),
            options: js_sys::Object::new(),
        }
    }

    /// The length of the file, read from the handle rather than remembered.
    ///
    /// One call per `SeekFrom::End`, which happens a handful of times per image
    /// rather than per chunk, and a remembered length is one the page can
    /// invalidate by writing through its own reference to the same handle.
    pub fn len(&self) -> io::Result<u64> {
        Ok(self
            .handle
            .get_size()
            .map_err(|e| failed("could not measure the file", e))? as u64)
    }

    /// Cut the file to `to` bytes, zero-filling if that grows it.
    pub fn truncate(&mut self, to: u64) -> io::Result<()> {
        self.handle
            .truncate(to as f64)
            .map_err(|e| failed(&format!("could not resize the file to {to} bytes"), e))?;
        self.position = self.position.min(to);
        Ok(())
    }

    /// `{ at: position }`, rewritten in place.
    fn at(&self, position: u64) -> io::Result<&JsValue> {
        js_sys::Reflect::set(
            &self.options,
            &JsValue::from_str("at"),
            &JsValue::from_f64(position as f64),
        )
        .map_err(|e| failed("could not set a file offset", e))?;
        Ok(self.options.as_ref())
    }

    /// A `Uint8Array` of exactly `len` bytes, over the buffer this file reuses.
    fn scratch(&mut self, len: usize) -> js_sys::Uint8Array {
        if (self.scratch.length() as usize) < len {
            self.scratch = js_sys::Uint8Array::new_with_length(len as u32);
        }
        self.scratch.subarray(0, len as u32)
    }
}

impl Read for File {
    fn read(&mut self, into: &mut [u8]) -> io::Result<usize> {
        let want = into.len().min(CHUNK);
        if want == 0 {
            return Ok(0);
        }
        let scratch = self.scratch(want);
        let read = self
            .handle
            .read(&scratch, self.at(self.position)?)
            .map_err(|e| failed("could not read the file", e))? as usize;
        scratch.subarray(0, read as u32).copy_to(&mut into[..read]);
        self.position += read as u64;
        Ok(read)
    }
}

impl Write for File {
    fn write(&mut self, from: &[u8]) -> io::Result<usize> {
        let want = from.len().min(CHUNK);
        if want == 0 {
            return Ok(0);
        }
        let scratch = self.scratch(want);
        scratch.copy_from(&from[..want]);
        let written = self
            .handle
            .write(&scratch, self.at(self.position)?)
            .map_err(|e| failed("could not write the file", e))? as usize;
        self.position += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.handle
            .flush()
            .map_err(|e| failed("could not flush the file", e))
    }
}

impl Seek for File {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let (from, by) = match to {
            SeekFrom::Start(at) => {
                self.position = at;
                return Ok(at);
            }
            SeekFrom::Current(by) => (self.position, by),
            SeekFrom::End(by) => (self.len()?, by),
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

/// What a rejected handle call says, as an `io::Error`.
///
/// A `JsValue` holding a `DOMException` has no `Display` and a `Debug` that
/// reads `JsValue(NotFoundError: …)`, so the message is read off the object and
/// the debug form is what is left when there is none.
fn failed(what: &str, error: JsValue) -> io::Error {
    let said = error
        .as_string()
        .or_else(|| {
            js_sys::Reflect::get(&error, &JsValue::from_str("message"))
                .ok()
                .and_then(|message| message.as_string())
        })
        .unwrap_or_else(|| format!("{error:?}"));
    io::Error::other(format!("{what}: {said}"))
}
