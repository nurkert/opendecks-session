//! DeviceSQL page layout (Deep Symmetry, "Database Exports": file header,
//! table pages, data pages, index pages).
//!
//! Layout written here, mirroring a fresh rekordbox export:
//!
//! * page 0: file header with one table pointer per table;
//! * for the table at position `t`: page `2t + 1` is its index page and
//!   page `2t + 2` its first data page (or an all-zero page that serves as
//!   the table's `empty_candidate` when the table has no rows);
//! * further data pages are appended after the last of those pages;
//! * the last data page of a non-empty table links to a fresh page index past
//!   the end of the file (its `empty_candidate`), as rekordbox does.
//!
//! Data pages: 0x28 byte header, rows growing up from 0x28 (4 byte aligned),
//! row index growing down from the page end in groups of 16 offsets
//! (`ofs[15..0], present_flags, transaction_flags`, 0x24 bytes per group).
//! `free_size` counts the heap minus rows minus `4 * groups + 2 * rows`.

use crate::bytes::*;
use crate::{Error, Result};

pub(crate) const PAGE_SIZE: usize = 4096;
const PAGE_HEADER: usize = 0x28;
const HEAP: usize = PAGE_SIZE - PAGE_HEADER;
const GROUP_BYTES: usize = 0x24;

const FLAGS_DATA: u8 = 0x24;
const SYNC_INFO_TYPE: u32 = 19;
const FLAGS_INDEX: u8 = 0x64;

/// One row ready for placement.
pub(crate) struct Row {
    /// Encoded row including trailing padding, length a multiple of 4.
    pub bytes: Vec<u8>,
    /// Whether bytes 2..4 hold `index_shift` (0x20 × row index in page).
    pub index_shift: bool,
}

/// A table to write.
pub(crate) struct TableSpec {
    pub page_type: u32,
    pub rows: Vec<Row>,
    /// rekordbox writes the default tables (colors, columns, menus) in one
    /// transaction (`transaction_row_count = rows`, all rows flagged) and
    /// fixed sequence numbers; other tables record the last row only.
    pub single_transaction: bool,
    pub fixed_seq: Option<u32>,
}

fn row_index_bytes(n: usize) -> usize {
    n.div_ceil(16) * 4 + n * 2
}

fn fits(used: usize, rows: usize, slot: usize) -> bool {
    used + slot + row_index_bytes(rows + 1) <= HEAP
}

/// Splits rows into pages, filling each page as far as it goes.
fn paginate(rows: Vec<Row>) -> Vec<Vec<Row>> {
    let mut pages: Vec<Vec<Row>> = Vec::new();
    let mut cur: Vec<Row> = Vec::new();
    let mut used = 0;
    for r in rows {
        assert!(r.bytes.len() % 4 == 0, "rows must be 4 byte aligned");
        assert!(
            fits(0, 0, r.bytes.len()),
            "row of {} bytes does not fit in a page",
            r.bytes.len()
        );
        if !fits(used, cur.len(), r.bytes.len()) || cur.len() >= 0x7ff {
            pages.push(std::mem::take(&mut cur));
            used = 0;
        }
        used += r.bytes.len();
        cur.push(r);
    }
    if !cur.is_empty() {
        pages.push(cur);
    }
    pages
}

fn page_header(p: &mut [u8], index: u32, page_type: u32, next: u32, seq: u32) {
    set_le32(p, 0x04, index);
    set_le32(p, 0x08, page_type);
    set_le32(p, 0x0c, next);
    set_le32(p, 0x10, seq);
}

fn write_index_page(p: &mut [u8], index: u32, page_type: u32, next: u32, first_data: Option<u32>) {
    page_header(p, index, page_type, next, 1);
    p[0x1b] = FLAGS_INDEX;
    set_le16(p, 0x20, 0x1fff); // unknown_a
    set_le16(p, 0x22, 0x1fff); // unknown_b
    set_le16(p, 0x24, 0x03ec); // magic
    set_le16(p, 0x26, 0); // next_offset
    set_le32(p, 0x28, index);
    set_le32(p, 0x2c, first_data.unwrap_or(0x03ff_ffff));
    set_le32(p, 0x30, 0x03ff_ffff);
    set_le32(p, 0x34, 0);
    set_le16(p, 0x38, 0); // num_entries
    set_le16(p, 0x3a, 0x1fff); // first_empty
    // Empty index entries up to the last 20 bytes of the page, which stay 0.
    let mut at = 0x3c;
    while at + 4 <= PAGE_SIZE - 20 {
        set_le32(p, at, 0x1fff_fff8);
        at += 4;
    }
}

fn write_data_page(
    p: &mut [u8],
    index: u32,
    spec: &TableSpec,
    next: u32,
    seq: u32,
    rows: &mut [Row],
) {
    page_header(p, index, spec.page_type, next, seq);
    let n = rows.len();
    let counts = (n as u32 & 0x1fff) | ((n as u32 & 0x7ff) << 13);
    p[0x18..0x1b].copy_from_slice(&counts.to_le_bytes()[..3]);
    p[0x1b] = FLAGS_DATA;
    let mut used = 0usize;
    let mut offsets = Vec::with_capacity(n);
    for (i, r) in rows.iter_mut().enumerate() {
        if r.index_shift {
            set_le16(&mut r.bytes, 2, (i as u16) << 5);
        }
        p[PAGE_HEADER + used..PAGE_HEADER + used + r.bytes.len()].copy_from_slice(&r.bytes);
        offsets.push(used as u16);
        used += r.bytes.len();
    }
    let free = HEAP - used - row_index_bytes(n);
    set_le16(p, 0x1c, free as u16);
    set_le16(p, 0x1e, used as u16);
    let (trc, tri) = if spec.single_transaction {
        (n as u16, 0)
    } else {
        (1, n.saturating_sub(1) as u16)
    };
    set_le16(p, 0x20, trc);
    set_le16(p, 0x22, tri);
    for (g, chunk) in offsets.chunks(16).enumerate() {
        let base = PAGE_SIZE - g * GROUP_BYTES;
        let present: u16 = if chunk.len() == 16 {
            0xffff
        } else {
            (1u16 << chunk.len()) - 1
        };
        let txn = if spec.single_transaction {
            present
        } else if (g + 1) * 16 >= n {
            1u16 << (chunk.len() - 1)
        } else {
            0
        };
        set_le16(p, base - 4, present);
        set_le16(p, base - 2, txn);
        for (s, ofs) in chunk.iter().enumerate() {
            set_le16(p, base - 6 - 2 * s, *ofs);
        }
    }
}

/// Serializes a whole database.
pub(crate) fn write_database(tables: Vec<TableSpec>) -> Vec<u8> {
    let n = tables.len();
    let mut next_free = (1 + 2 * n) as u32;
    // Sequence numbers only need to be consistent (header above every page);
    // these reproduce a fresh rekordbox export.
    let mut seq = 8u32;
    // (spec, pages of rows, data page indices)
    let mut layout = Vec::with_capacity(n);
    for (t, spec) in tables.into_iter().enumerate() {
        let TableSpec {
            page_type,
            rows,
            single_transaction,
            fixed_seq,
        } = spec;
        let spec = TableSpec {
            page_type,
            rows: Vec::new(),
            single_transaction,
            fixed_seq,
        };
        let pages = paginate(rows);
        let mut indices = Vec::with_capacity(pages.len());
        for i in 0..pages.len() {
            if i == 0 {
                indices.push((2 * t + 2) as u32);
            } else {
                indices.push(next_free);
                next_free += 1;
            }
        }
        layout.push((spec, pages, indices));
    }
    let file_pages = next_free as usize;
    // Empty candidates of non-empty tables lie past the end of the file.
    // rekordbox hands the first one to the sync-info table (written first).
    let mut candidates: Vec<u32> = (0..n).map(|t| (2 * t + 2) as u32).collect();
    let mut order: Vec<usize> = (0..n).filter(|&t| !layout[t].1.is_empty()).collect();
    order.sort_by_key(|&t| layout[t].0.page_type != SYNC_INFO_TYPE);
    for t in order {
        candidates[t] = next_free;
        next_free += 1;
    }

    let mut out = vec![0u8; file_pages * PAGE_SIZE];
    for (t, (spec, pages, indices)) in layout.iter_mut().enumerate() {
        let index_page = (2 * t + 1) as u32;
        let first = indices.first().copied();
        let ip = index_page as usize * PAGE_SIZE;
        write_index_page(
            &mut out[ip..ip + PAGE_SIZE],
            index_page,
            spec.page_type,
            first.unwrap_or(candidates[t]),
            first,
        );
        for (i, rows) in pages.iter_mut().enumerate() {
            let next = indices.get(i + 1).copied().unwrap_or(candidates[t]);
            let page_seq = match spec.fixed_seq {
                Some(s) if i == 0 => s,
                _ => {
                    seq += 1;
                    seq
                }
            };
            let at = indices[i] as usize * PAGE_SIZE;
            write_data_page(
                &mut out[at..at + PAGE_SIZE],
                indices[i],
                spec,
                next,
                page_seq,
                rows,
            );
        }
    }

    // File header.
    let h = &mut out[..PAGE_SIZE];
    set_le32(h, 0x04, PAGE_SIZE as u32);
    set_le32(h, 0x08, n as u32);
    set_le32(h, 0x0c, next_free);
    set_le32(h, 0x10, 5);
    set_le32(h, 0x14, seq + 1);
    for (t, (spec, _, indices)) in layout.iter().enumerate() {
        let at = 0x1c + 16 * t;
        set_le32(h, at, spec.page_type);
        set_le32(h, at + 4, candidates[t]);
        set_le32(h, at + 8, (2 * t + 1) as u32);
        set_le32(
            h,
            at + 12,
            indices.last().copied().unwrap_or((2 * t + 1) as u32),
        );
    }
    out
}

/// A table as found in a file: its type and the bytes of every present row
/// (from the row start to the end of its page).
pub(crate) struct ReadTable<'a> {
    pub page_type: u32,
    pub rows: Vec<&'a [u8]>,
}

/// Reads all present rows of all tables, following the page chains.
pub(crate) fn read_database(data: &[u8]) -> Result<Vec<ReadTable<'_>>> {
    let page_size = le32(data, 4)? as usize;
    if !(512..=65536).contains(&page_size) {
        return Err(Error::Invalid(format!("page size {page_size}")));
    }
    let num_tables = le32(data, 8)? as usize;
    let num_pages = data.len() / page_size;
    let mut tables = Vec::with_capacity(num_tables);
    for t in 0..num_tables {
        let at = 0x1c + 16 * t;
        let page_type = le32(data, at)?;
        let first = le32(data, at + 8)? as usize;
        let last = le32(data, at + 12)? as usize;
        let mut rows = Vec::new();
        let mut page = first;
        let mut visited = 0;
        loop {
            if page >= num_pages || visited > num_pages {
                break;
            }
            visited += 1;
            let p = &data[page * page_size..(page + 1) * page_size];
            let flags = p[0x1b];
            if le32(p, 0x08)? != page_type {
                break;
            }
            if flags & 0x40 == 0 {
                let counts = u32::from_le_bytes([p[0x18], p[0x19], p[0x1a], 0]);
                let num_offsets = (counts & 0x1fff) as usize;
                for r in 0..num_offsets {
                    let (g, s) = (r / 16, r % 16);
                    let base = page_size
                        .checked_sub(g * GROUP_BYTES)
                        .ok_or_else(|| Error::Invalid("row index overflows page".into()))?;
                    if base < PAGE_HEADER + 6 + 2 * s {
                        return Err(Error::Invalid("row index overflows page".into()));
                    }
                    let present = le16(p, base - 4)?;
                    if present >> s & 1 == 0 {
                        continue;
                    }
                    let ofs = le16(p, base - 6 - 2 * s)? as usize;
                    let start = PAGE_HEADER + ofs;
                    if start >= page_size {
                        return Err(Error::Invalid(format!(
                            "row offset {ofs:#x} outside page {page}"
                        )));
                    }
                    rows.push(&p[start..]);
                }
            }
            if page == last {
                break;
            }
            page = le32(p, 0x0c)? as usize;
        }
        tables.push(ReadTable { page_type, rows });
    }
    Ok(tables)
}
