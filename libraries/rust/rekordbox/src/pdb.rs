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

use deku::prelude::*;
use std::io::{self, Write};

/// Every layout in this file is fixed, so a write cannot fail for anything a
/// caller could fix.
fn bytes(layout: &impl DekuContainerWrite) -> Vec<u8> {
    layout.to_bytes().expect("a fixed layout with no counts")
}

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
/// its entry and its page: every export examined carries all twenty, numbered
/// without a gap, and the six nobody has named are as present as the rest.
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
    Unknown9 = 9,
    Unknown10 = 10,
    HistoryPlaylists = 11,
    HistoryEntries = 12,
    Artwork = 13,
    Unknown14 = 14,
    Unknown15 = 15,
    Columns = 16,
    Unknown17 = 17,
    Unknown18 = 18,
    History = 19,
}

impl PageType {
    /// Every table rekordbox writes, in the order it writes them.
    pub const ALL: [PageType; 20] = [
        PageType::Tracks,
        PageType::Genres,
        PageType::Artists,
        PageType::Albums,
        PageType::Labels,
        PageType::Keys,
        PageType::Colors,
        PageType::PlaylistTree,
        PageType::PlaylistEntries,
        PageType::Unknown9,
        PageType::Unknown10,
        PageType::HistoryPlaylists,
        PageType::HistoryEntries,
        PageType::Artwork,
        PageType::Unknown14,
        PageType::Unknown15,
        PageType::Columns,
        PageType::Unknown17,
        PageType::Unknown18,
        PageType::History,
    ];

    /// The page rekordbox fills this table with whatever the library holds.
    ///
    /// Three tables carry the same rows in every export examined, an empty one
    /// and a 135-track one four years apart included: `Columns` is the browse
    /// menu a player draws, and the two beside it are whatever the menu needs.
    /// Nothing about a library changes them, so they are copied rather than
    /// derived. See `pages/`.
    fn boilerplate(self) -> Option<&'static [u8; PAGE_SIZE]> {
        match self {
            PageType::Columns => Some(include_bytes!("../pages/columns.bin")),
            PageType::Unknown17 => Some(include_bytes!("../pages/unknown-17.bin")),
            PageType::Unknown18 => Some(include_bytes!("../pages/unknown-18.bin")),
            _ => None,
        }
    }
}

/// The first page of every table, which holds no rows.
///
/// rekordbox writes an index page here, flagged 0x40 over the ordinary 0x24,
/// and leaves its entry array empty for every table but the three largest. The
/// bytes are one template in every export examined, so this exporter writes
/// that template and patches the four words that say where the page sits. An
/// empty entry array is what rekordbox itself writes for seventeen of its
/// twenty tables in a 135-track library.
const INDEX_PAGE: &[u8; PAGE_SIZE] = include_bytes!("../pages/index-page.bin");

/// Where the index page repeats its own number, and where it points at the
/// first page holding rows.
const INDEX_SELF_AT: usize = 0x28;
const INDEX_NEXT_AT: usize = 0x2c;
/// What the index page points at when the table has no page holding rows.
const INDEX_NO_ROWS: u32 = 0x03ff_ffff;

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
        let mut front = bytes(&FileHeader {
            magic: 0,
            page_size: PAGE_SIZE as u32,
            tables: self.tables.len() as u32,
            next_unused_page: layout.total_pages as u32,
            unknown: 0,
            sequence: 1,
            gap: 0,
        });
        for table in &layout.tables {
            front.extend(bytes(&TableEntry {
                page_type: table.page_type as u32,
                empty_candidate: table.spare_page as u32,
                first_page: table.first_page as u32,
                last_page: table.last_page as u32,
            }));
        }

        let mut header = vec![0u8; PAGE_SIZE];
        header[..front.len()].copy_from_slice(&front);
        out.write_all(&header)?;

        for table in &layout.tables {
            for (position, page) in table.pages.iter().enumerate() {
                let index = table.first_page + position;
                let next = index + 1;
                let bytes = match page {
                    PagePlan::Index { has_rows } => {
                        index_page(index, table.page_type, next, *has_rows)
                    }
                    PagePlan::Boilerplate(bytes) => {
                        verbatim_page(index, table.page_type, next, bytes)
                    }
                    PagePlan::Rows {
                        first_row,
                        row_count,
                    } => build_page(
                        index,
                        table.page_type,
                        next,
                        &self.rows_of(table.page_type)[*first_row..*first_row + *row_count],
                    ),
                };
                out.write_all(&bytes)?;
            }
            // The spare, which is 4096 zero bytes in every real export.
            out.write_all(&[0u8; PAGE_SIZE])?;
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
            // The first page of a table is its index and holds no rows. A table
            // with nothing in it is that page on its own.
            let mut pages = Vec::new();
            if let Some(bytes) = page_type.boilerplate() {
                pages.push(PagePlan::Index { has_rows: true });
                pages.push(PagePlan::Boilerplate(bytes));
            } else {
                pages.push(PagePlan::Index {
                    has_rows: !table.rows.is_empty(),
                });
                let mut first_row = 0;
                while first_row < table.rows.len() {
                    let count = rows_that_fit(&table.rows[first_row..])?;
                    pages.push(PagePlan::Rows {
                        first_row,
                        row_count: count,
                    });
                    first_row += count;
                }
            }

            let first_page = next_index;
            next_index += pages.len();
            // One page this table owns and has not filled, which is what a
            // player takes when it needs to grow the table. See
            // [`TablePlan::spare_page`].
            let spare_page = next_index;
            next_index += 1;
            tables.push(TablePlan {
                page_type: *page_type,
                first_page,
                last_page: spare_page - 1,
                spare_page,
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
    /// A page holding nothing, belonging to this table alone.
    ///
    /// The header's entry for a table points a player here when the table has
    /// to grow, and a player takes it at its word. Pointing it at the page
    /// after the table means pointing it at the next table's first page: an
    /// XDJ-RX3 asked for one page of play history, was handed the page holding
    /// `history_entries`, and wrote over it. Two tables then claimed one page
    /// and rekordbox could no longer read the file.
    ///
    /// Every table therefore ends with a spare, the way both real exports
    /// measured here give one to every table they leave a page short of full.
    /// 4096 bytes of nothing per table, which is 80 kB on a stick.
    spare_page: usize,
    pages: Vec<PagePlan>,
}

enum PagePlan {
    /// The table's index page, which holds no rows.
    Index { has_rows: bool },
    /// A page copied from a real export.
    Boilerplate(&'static [u8; PAGE_SIZE]),
    /// A page of rows this exporter encoded.
    Rows { first_row: usize, row_count: usize },
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

/// The index page that opens a table, patched to say where it sits.
///
/// `has_rows` decides whether it points at the page after it or at the sentinel
/// rekordbox writes when the table holds nothing.
fn index_page(index: usize, page_type: PageType, next_page: usize, has_rows: bool) -> Vec<u8> {
    let mut page = INDEX_PAGE.to_vec();
    let mut put = |at: usize, value: u32| {
        page[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };
    put(0x04, index as u32);
    put(0x08, page_type as u32);
    put(0x0c, next_page as u32);
    put(0x10, 1);
    put(INDEX_SELF_AT, index as u32);
    put(
        INDEX_NEXT_AT,
        if has_rows {
            next_page as u32
        } else {
            INDEX_NO_ROWS
        },
    );
    page
}

/// A page copied whole from a real export, patched to say where it sits.
fn verbatim_page(
    index: usize,
    page_type: PageType,
    next_page: usize,
    bytes: &[u8; PAGE_SIZE],
) -> Vec<u8> {
    let mut page = bytes.to_vec();
    page[0x04..0x08].copy_from_slice(&(index as u32).to_le_bytes());
    page[0x0c..0x10].copy_from_slice(&(next_page as u32).to_le_bytes());
    page[0x10..0x14].copy_from_slice(&1u32.to_le_bytes());
    if page_type == PageType::Unknown17 {
        show_file_name(&mut page);
    }
    page
}

/// The column this exporter adds to the browse menu, and the slot it takes.
const FILE_NAME_COLUMN: u16 = 16;
const FILE_NAME_SLOT: u16 = 11;
/// Byte of a menu row that says the category is hidden.
const MENU_HIDDEN_AT: usize = 5;
/// Where the row carries its slot in the list a player draws.
const MENU_SLOT_AT: usize = 6;

/// Turn the file name on in the browse menu.
///
/// A row of `Unknown17` is a column id, a menu order, a flag byte and a slot in
/// the list a player draws. Byte five is 1 when the category is hidden, and the
/// slot is 0 exactly when it is: across the 64 rows of two real exports the two
/// agree without exception, and rekordbox turns a category on by clearing the
/// one and filling in the other.
///
/// rekordbox ships the file name hidden, which on this library is the wrong
/// default. A dubplate is named `<BPM>_<KEY>_<track> - Artist - Title`, so the
/// file name is the one browse axis carrying what this tool measured, and the
/// ten slots rekordbox does fill leave the eleventh free.
///
/// The page itself stays as it was copied. This is the single deliberate
/// difference from it, which is why it is here rather than edited into the
/// bytes in `pages/`.
fn show_file_name(page: &mut [u8]) {
    for row in menu_rows(page) {
        if u16::from_le_bytes([page[row], page[row + 1]]) != FILE_NAME_COLUMN {
            continue;
        }
        page[row + MENU_HIDDEN_AT] = 0;
        page[row + MENU_SLOT_AT..row + MENU_SLOT_AT + 2]
            .copy_from_slice(&FILE_NAME_SLOT.to_le_bytes());
    }
}

/// Where each row of a page starts, read out of its row groups.
fn menu_rows(page: &[u8]) -> Vec<usize> {
    let packed = u32::from_le_bytes([page[24], page[25], page[26], 0]);
    let rows = (packed & 0x1fff) as usize;
    (0..rows)
        .map(|row| {
            let group = row / ROWS_PER_GROUP;
            let slot = row % ROWS_PER_GROUP;
            let group_start = PAGE_SIZE - (group + 1) * ROW_GROUP_SIZE;
            let at = group_start + (ROWS_PER_GROUP - 1 - slot) * 2;
            let offset = u16::from_le_bytes([page[at], page[at + 1]]) as usize;
            PAGE_HEADER_SIZE + DATA_HEADER_SIZE + offset
        })
        .collect()
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
    //
    // The bitmask is written twice, the way the page header states its row
    // count twice: once for the rows present and once for the rows still valid.
    // Writing only the first leaves a group whose every row a player can read
    // and none it is told to trust.
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
        let present_at = group_start + ROWS_PER_GROUP * 2;
        page[present_at..present_at + 2].copy_from_slice(&flags.to_le_bytes());
        page[present_at + 2..present_at + 4].copy_from_slice(&flags.to_le_bytes());
    }

    let header = bytes(&PageHeader {
        magic: 0,
        index: index as u32,
        page_type: page_type as u32,
        next_page: next_page as u32,
        unknown1: 1,
        unknown2: 0,
        rows_present: rows.len() as u16,
        rows_valid: rows.len() as u16,
        page_flags: 0x24,
        free_size: free_size(used, rows.len()) as u16,
        used_size: used.next_multiple_of(ROW_ALIGNMENT) as u16,
        data_rows: rows.len() as u16,
        data_zero1: 0,
        data_zero2: 0,
        data_zero3: 0,
    });
    page[..header.len()].copy_from_slice(&header);

    page
}

/// The 0x20-byte page header and the 0x08-byte data header that follows it.
///
/// The two are one struct because they are always written together and the
/// second has no meaning without the first.
#[derive(DekuWrite)]
#[deku(endian = "little", bit_order = "lsb")]
struct PageHeader {
    magic: u32,
    index: u32,
    page_type: u32,
    next_page: u32,
    /// Purpose unknown; a small non-zero number in every file read.
    unknown1: u32,
    unknown2: u32,
    /// Thirteen bits of rows present, then eleven of rows still valid, packed
    /// into three bytes. Nothing here ever deletes a row, so the two are equal.
    #[deku(bits = 13)]
    rows_present: u16,
    #[deku(bits = 11)]
    rows_valid: u16,
    /// 0x24 is what rekordbox writes on a data page holding rows. The 0x40 bit
    /// marks an index page, which [`INDEX_PAGE`] carries instead of this.
    page_flags: u8,
    free_size: u16,
    used_size: u16,
    /// The data header: the row count again, then six zero bytes.
    data_rows: u16,
    data_zero1: u16,
    data_zero2: u16,
    data_zero3: u16,
}

/// Bytes on a page that neither a row nor a row group has claimed.
///
/// A group is 36 bytes of slots but only the slots a row filled count as used,
/// so a page of one row leaves the other fifteen slots free. Deriving it from
/// whole groups instead understates the figure by two bytes per empty slot,
/// which is how every export examined counts it.
fn free_size(used: usize, rows: usize) -> usize {
    let groups = rows.div_ceil(ROWS_PER_GROUP);
    let claimed = groups * 4 + rows * 2;
    HEAP_SIZE - used.next_multiple_of(ROW_ALIGNMENT) - claimed
}

/// The file header: page size, table count, and where the pages end.
#[derive(DekuWrite)]
#[deku(endian = "little")]
struct FileHeader {
    /// Always zero. Perhaps a signature nobody has needed.
    magic: u32,
    page_size: u32,
    tables: u32,
    /// First page past the end of the file.
    next_unused_page: u32,
    unknown: u32,
    /// Incremented by rekordbox on every export.
    sequence: u32,
    gap: u32,
}

/// One entry of the header's table list.
#[derive(DekuWrite)]
#[deku(endian = "little")]
struct TableEntry {
    page_type: u32,
    /// Where a player takes a page from when this table has to grow. Never a
    /// page another table owns: see [`TablePlan::spare_page`].
    empty_candidate: u32,
    first_page: u32,
    last_page: u32,
}

#[cfg(test)]
mod header_layout {
    use super::*;

    /// The page header as the cursor wrote it, kept as the oracle for the
    /// declared one. The row counts are a thirteen-bit field and an eleven-bit
    /// field sharing three bytes, and a derive macro filling those from the
    /// wrong end writes a page a player misreads rather than rejects.
    fn cursor_written(
        index: usize,
        page_type: PageType,
        next_page: usize,
        row_count: usize,
        used: usize,
    ) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(index as u32).to_le_bytes());
        out.extend_from_slice(&(page_type as u32).to_le_bytes());
        out.extend_from_slice(&(next_page as u32).to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        let packed = (row_count as u32 & 0x1fff) | ((row_count as u32 & 0x7ff) << 13);
        out.push((packed & 0xff) as u8);
        out.push(((packed >> 8) & 0xff) as u8);
        out.push(((packed >> 16) & 0xff) as u8);
        out.push(0x24);
        out.extend_from_slice(&(free_size(used, row_count) as u16).to_le_bytes());
        out.extend_from_slice(&(used.next_multiple_of(ROW_ALIGNMENT) as u16).to_le_bytes());
        out.extend_from_slice(&(row_count as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    /// The most rows a page can hold, which is what bounds the two packed row
    /// counts.
    ///
    /// The smallest row this exporter writes is a twelve-byte playlist entry,
    /// and every sixteen rows add a 36-byte group, so a full page of them is
    /// under 300 rows. Both counts have at least eleven bits, so neither can
    /// overflow its field.
    const MOST_ROWS_A_PAGE_CAN_HOLD: usize = HEAP_SIZE / 12;

    /// Checked at compile time, because it is what makes the eleven-bit field
    /// below unreachable rather than merely unlikely.
    const _: () = assert!(MOST_ROWS_A_PAGE_CAN_HOLD < 2048);

    #[test]
    fn a_row_count_too_large_for_its_field_is_refused_rather_than_truncated() {
        // The cursor masked this to eleven bits and wrote a page claiming 0
        // valid rows out of 2048. Unreachable either way, since a page holds a
        // few hundred rows at most, but refusing beats writing a wrong number.
        let over = PageHeader {
            magic: 0,
            index: 0,
            page_type: PageType::Tracks as u32,
            next_page: 1,
            unknown1: 1,
            unknown2: 0,
            rows_present: 2048,
            rows_valid: 2048,
            page_flags: 0x24,
            free_size: 0,
            used_size: 0,
            data_rows: 2048,
            data_zero1: 0,
            data_zero2: 0,
            data_zero3: 0,
        };
        assert!(
            over.to_bytes().is_err(),
            "2048 valid rows does not fit eleven bits and has to be refused"
        );
    }

    #[test]
    fn the_declared_page_header_is_the_header_the_cursor_wrote() {
        for row_count in [
            0usize,
            1,
            15,
            16,
            17,
            100,
            255,
            256,
            MOST_ROWS_A_PAGE_CAN_HOLD,
        ] {
            let used = row_count * 4;
            let declared = bytes(&PageHeader {
                magic: 0,
                index: 7,
                page_type: PageType::Tracks as u32,
                next_page: 8,
                unknown1: 1,
                unknown2: 0,
                rows_present: row_count as u16,
                rows_valid: row_count as u16,
                page_flags: 0x24,
                free_size: free_size(used, row_count) as u16,
                used_size: used.next_multiple_of(ROW_ALIGNMENT) as u16,
                data_rows: row_count as u16,
                data_zero1: 0,
                data_zero2: 0,
                data_zero3: 0,
            });
            assert_eq!(
                declared,
                cursor_written(7, PageType::Tracks, 8, row_count, used),
                "page header for {row_count} rows"
            );
            assert_eq!(
                declared.len(),
                PAGE_HEADER_SIZE + DATA_HEADER_SIZE,
                "a page header is 0x28 bytes with the data header"
            );
        }
    }
}
