//! Laying encoded rows out into a DeviceSQL database file.
//!
//! The file is a sequence of 4096-byte pages. Page zero is the header and lists
//! every table with the first and last page of its chain; each later page holds
//! a header, then rows packed forward from the start of its heap, then row
//! groups packed backward from the end of it. A row group carries sixteen
//! offsets and a bitmask saying which of them are real.
//!
//! Rows arrive here already encoded, because their layouts differ in every way
//! except that they are bytes. See `rows.rs` for those.
//!
//! Format reference: <https://djl-analysis.deepsymmetry.org/rekordbox-export-analysis/exports.html>

use std::io::{self, Write};

/// Page size rekordbox writes and players expect. Stored in the header, but
/// nothing is gained by varying it.
pub const PAGE_SIZE: usize = 4096;
const PAGE_HEADER_SIZE: usize = 0x20;
const DATA_HEADER_SIZE: usize = 0x08;
/// Bytes in a page available to rows and row groups together.
const HEAP_SIZE: usize = PAGE_SIZE - PAGE_HEADER_SIZE - DATA_HEADER_SIZE;
const ROWS_PER_GROUP: usize = 16;
const ROW_GROUP_SIZE: usize = ROWS_PER_GROUP * 2 + 4;
/// Rows are placed on four-byte boundaries so that the UTF-16 strings inside
/// them land on one too. The parser seeks to a string by an offset relative to
/// the row, so an unaligned row moves every string in it.
const ROW_ALIGNMENT: usize = 4;

/// Page types, as written in both the header's table list and each page.
///
/// A player looks tables up by this number, so the set below is what rekordbox
/// emits rather than what this exporter fills in. An empty table still needs
/// its entry and its page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum PageType {
    Tracks = 0,
    Genres = 1,
    Artists = 2,
    Albums = 3,
    Labels = 4,
    Keys = 5,
    Colors = 6,
    PlaylistTree = 7,
    PlaylistEntries = 8,
    HistoryPlaylists = 11,
    HistoryEntries = 12,
    Artwork = 13,
    Columns = 16,
    Menu = 17,
    Sync = 19,
}

impl PageType {
    /// Every table rekordbox writes, in the order it writes them.
    pub const ALL: [PageType; 15] = [
        PageType::Tracks,
        PageType::Genres,
        PageType::Artists,
        PageType::Albums,
        PageType::Labels,
        PageType::Keys,
        PageType::Colors,
        PageType::PlaylistTree,
        PageType::PlaylistEntries,
        PageType::HistoryPlaylists,
        PageType::HistoryEntries,
        PageType::Artwork,
        PageType::Columns,
        PageType::Menu,
        PageType::Sync,
    ];
}

/// One encoded row, and whether it carries an `index_shift` field.
///
/// That field is 0x20 times the row's position in its page, which no encoder
/// can know: which page a row lands on is decided here, after every row has
/// been encoded. So the encoders leave it zero and the page layer fills it in.
#[derive(Debug)]
struct RowBytes {
    bytes: Vec<u8>,
    has_index_shift: bool,
}

/// Rows for one table, in the order a player will list them.
#[derive(Debug, Default)]
pub struct Table {
    rows: Vec<RowBytes>,
}

impl Table {
    /// A row with no `index_shift`: genres, keys, playlists and the rest.
    pub fn push(&mut self, row: Vec<u8>) {
        self.rows.push(RowBytes {
            bytes: row,
            has_index_shift: false,
        });
    }

    /// A row that carries `index_shift`: tracks, artists and albums.
    pub fn push_indexed(&mut self, row: Vec<u8>) {
        self.rows.push(RowBytes {
            bytes: row,
            has_index_shift: true,
        });
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// The database under construction.
#[derive(Debug, Default)]
pub struct Database {
    tables: Vec<(PageType, Table)>,
}

impl Database {
    /// A database with every table rekordbox writes, all empty.
    pub fn new() -> Self {
        Database {
            tables: PageType::ALL
                .iter()
                .map(|page_type| (*page_type, Table::default()))
                .collect(),
        }
    }

    pub fn table(&mut self, page_type: PageType) -> &mut Table {
        let index = self
            .tables
            .iter()
            .position(|(existing, _)| *existing == page_type)
            .expect("every page type is created by Database::new");
        &mut self.tables[index].1
    }

    /// Serialise the whole file.
    pub fn write(&self, out: &mut impl Write) -> io::Result<()> {
        let layout = self.layout()?;
        let mut header = vec![0u8; PAGE_SIZE];
        let mut cursor = Cursor::new(&mut header);

        cursor.u32(0); // Always zero, perhaps a signature.
        cursor.u32(PAGE_SIZE as u32);
        cursor.u32(self.tables.len() as u32);
        cursor.u32(layout.total_pages as u32); // First page past the end.
        cursor.u32(0);
        cursor.u32(1); // Sequence, incremented by rekordbox on every export.
        cursor.u32(0); // Gap.
        for table in &layout.tables {
            cursor.u32(table.page_type as u32);
            // Purpose unknown; rekordbox appears to point it past the table.
            cursor.u32((table.last_page + 1) as u32);
            cursor.u32(table.first_page as u32);
            cursor.u32(table.last_page as u32);
        }
        out.write_all(&header)?;

        for table in &layout.tables {
            for (position, page) in table.pages.iter().enumerate() {
                let index = table.first_page + position;
                let next = index + 1;
                out.write_all(&build_page(
                    index,
                    table.page_type,
                    next,
                    &self.rows_of(table.page_type)[page.first_row..page.first_row + page.row_count],
                ))?;
            }
        }
        Ok(())
    }

    fn rows_of(&self, page_type: PageType) -> &[RowBytes] {
        self.tables
            .iter()
            .find(|(existing, _)| *existing == page_type)
            .map(|(_, table)| table.rows.as_slice())
            .unwrap_or(&[])
    }

    /// Decide which rows go on which page, and number the pages.
    fn layout(&self) -> io::Result<Layout> {
        // Page zero is the header.
        let mut next_index = 1;
        let mut tables = Vec::with_capacity(self.tables.len());

        for (page_type, table) in &self.tables {
            // The first page of a table holds no rows. Players expect it and
            // rekordbox writes it, so a single-page table has one empty page.
            let mut pages = vec![PagePlan {
                first_row: 0,
                row_count: 0,
            }];
            let mut first_row = 0;
            while first_row < table.rows.len() {
                let count = rows_that_fit(&table.rows[first_row..])?;
                pages.push(PagePlan {
                    first_row,
                    row_count: count,
                });
                first_row += count;
            }

            let first_page = next_index;
            next_index += pages.len();
            tables.push(TablePlan {
                page_type: *page_type,
                first_page,
                last_page: next_index - 1,
                pages,
            });
        }

        Ok(Layout {
            total_pages: next_index,
            tables,
        })
    }
}

struct Layout {
    total_pages: usize,
    tables: Vec<TablePlan>,
}

struct TablePlan {
    page_type: PageType,
    first_page: usize,
    last_page: usize,
    pages: Vec<PagePlan>,
}

struct PagePlan {
    first_row: usize,
    row_count: usize,
}

/// How many of these rows fit on one page, rows and their row groups together.
///
/// A row that does not fit an empty page has nowhere to go. Reaching that needs
/// a few thousand bytes of strings in one row, and every one of those strings
/// comes from a file name, so the caller is told rather than panicked at.
fn rows_that_fit(rows: &[RowBytes]) -> io::Result<usize> {
    let mut used = 0usize;
    for (count, row) in rows.iter().enumerate() {
        let start = used.next_multiple_of(ROW_ALIGNMENT);
        let groups = (count + 1).div_ceil(ROWS_PER_GROUP) * ROW_GROUP_SIZE;
        if start + row.bytes.len() + groups > HEAP_SIZE {
            if count == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "a row of {} bytes does not fit a {PAGE_SIZE}-byte page, which holds {HEAP_SIZE} bytes of rows",
                        row.bytes.len()
                    ),
                ));
            }
            return Ok(count);
        }
        used = start + row.bytes.len();
    }
    Ok(rows.len())
}

/// Serialise one page.
fn build_page(index: usize, page_type: PageType, next_page: usize, rows: &[RowBytes]) -> Vec<u8> {
    let mut page = vec![0u8; PAGE_SIZE];
    let heap_start = PAGE_HEADER_SIZE + DATA_HEADER_SIZE;

    // Rows forward from the start of the heap, remembering where each landed.
    let mut offsets = Vec::with_capacity(rows.len());
    let mut used = 0usize;
    for (position, row) in rows.iter().enumerate() {
        let start = used.next_multiple_of(ROW_ALIGNMENT);
        let at = heap_start + start;
        page[at..at + row.bytes.len()].copy_from_slice(&row.bytes);
        if row.has_index_shift {
            let shift = (position as u16) * 0x20;
            let field = at + crate::rows::INDEX_SHIFT_AT;
            page[field..field + 2].copy_from_slice(&shift.to_le_bytes());
        }
        offsets.push(start as u16);
        used = start + row.bytes.len();
    }

    // Row groups backward from the end of the heap. Group zero is the last
    // thirty-six bytes of the page, and within a group the offsets are stored in
    // reverse, so the first row of the group sits closest to the flags.
    let group_count = rows.len().div_ceil(ROWS_PER_GROUP);
    for group in 0..group_count {
        let group_start = PAGE_SIZE - (group + 1) * ROW_GROUP_SIZE;
        let mut flags = 0u16;
        for slot in 0..ROWS_PER_GROUP {
            let Some(offset) = offsets.get(group * ROWS_PER_GROUP + slot) else {
                break;
            };
            let position = group_start + (ROWS_PER_GROUP - 1 - slot) * 2;
            page[position..position + 2].copy_from_slice(&offset.to_le_bytes());
            flags |= 1 << slot;
        }
        let flags_at = group_start + ROWS_PER_GROUP * 2;
        page[flags_at..flags_at + 2].copy_from_slice(&flags.to_le_bytes());
    }

    let group_bytes = group_count * ROW_GROUP_SIZE;
    let mut cursor = Cursor::new(&mut page);
    cursor.u32(0); // Page magic.
    cursor.u32(index as u32);
    cursor.u32(page_type as u32);
    cursor.u32(next_page as u32);
    cursor.u32(1); // Purpose unknown; small non-zero in the files we have read.
    cursor.u32(0);
    // Row counts packed into three bytes: thirteen bits of rows present, eleven
    // of rows still valid. Nothing here ever deletes a row, so they are equal.
    let packed = (rows.len() as u32 & 0x1fff) | ((rows.len() as u32 & 0x7ff) << 13);
    cursor.u8((packed & 0xff) as u8);
    cursor.u8(((packed >> 8) & 0xff) as u8);
    cursor.u8(((packed >> 16) & 0xff) as u8);
    // Page flags. 0x24 is what rekordbox writes on a data page holding rows;
    // the 0x40 bit would mark it an index page, which this exporter never
    // writes.
    cursor.u8(0x24);
    cursor.u16((HEAP_SIZE - used - group_bytes) as u16);
    cursor.u16(used as u16);
    // Data page header.
    cursor.u16(1);
    cursor.u16(rows.len() as u16);
    cursor.u16(0);
    cursor.u16(0);

    page
}

/// A cursor that writes little-endian integers forward from the start of a
/// buffer. Enough for a header; the rows do their own encoding.
struct Cursor<'a> {
    buffer: &'a mut [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    fn new(buffer: &'a mut [u8]) -> Self {
        Cursor {
            buffer,
            position: 0,
        }
    }

    fn u8(&mut self, value: u8) {
        self.buffer[self.position] = value;
        self.position += 1;
    }

    fn u16(&mut self, value: u16) {
        self.buffer[self.position..self.position + 2].copy_from_slice(&value.to_le_bytes());
        self.position += 2;
    }

    fn u32(&mut self, value: u32) {
        self.buffer[self.position..self.position + 4].copy_from_slice(&value.to_le_bytes());
        self.position += 4;
    }
}
