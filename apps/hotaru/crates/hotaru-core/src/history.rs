//! Cold scrollback blocks. Resident snapshots and compressed history have
//! independent ownership; reading a block never changes its storage state.
use crate::{Row, RowRef};
use std::collections::VecDeque;
use std::mem::size_of;

const TARGET_BYTES: usize = 64 * 1024;
const MAX_ROWS: usize = 64;
const MAX_RAW_BYTES: usize = 4 * 1024 * 1024;
const MAX_INSPECTED: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HistoryMemory {
    pub rows: usize,
    pub resident_rows: usize,
    pub compressed_rows: usize,
    /// Upper bound on serialized resident data, not allocator accounting or RSS.
    pub resident_size_bound: usize,
    /// Encoded bytes plus the compact row-recycling metadata, excluding
    /// allocation headers and outstanding snapshots.
    pub compressed_bytes: usize,
    pub compressed_blocks: usize,
}

enum Storage {
    Resident {
        rows: VecDeque<RowRef>,
        size_bound: usize,
    },
    Compressed {
        bytes: Box<[u8]>,
        raw_len: usize,
        rows: usize,
        skip: usize,
        // Retained width affects non-reflow resize semantics even after a row
        // is cleared. Keep it readable when eviction skips the encoded cells.
        recycle_widths: Box<[u16]>,
    },
}
struct Block {
    storage: Storage,
    attempted: bool,
}
impl Block {
    fn len(&self) -> usize {
        match &self.storage {
            Storage::Resident { rows, .. } => rows.len(),
            Storage::Compressed { rows, skip, .. } => rows - skip,
        }
    }
    fn decode(&self) -> Vec<RowRef> {
        match &self.storage {
            Storage::Resident { rows, .. } => rows.iter().cloned().collect(),
            Storage::Compressed {
                bytes,
                raw_len,
                rows,
                skip,
                ..
            } => decode_block(bytes, *raw_len, *rows)
                .expect("internally encoded history is valid")
                .into_iter()
                .skip(*skip)
                .collect(),
        }
    }
    fn compress(&mut self) -> bool {
        self.attempted = true;
        let Storage::Resident { rows, size_bound } = &self.storage else {
            return false;
        };
        if *size_bound > MAX_RAW_BYTES || rows.is_empty() {
            return false;
        }
        let mut raw = Vec::new();
        if raw.try_reserve_exact(*size_bound).is_err() {
            return false;
        }
        for row in rows {
            row.encode_history(&mut raw);
        }
        debug_assert!(raw.len() <= *size_bound);
        let mut encoded = Vec::new();
        // The safe LZ4 encoder requires its maximum-size output buffer even
        // for compressible input. Both scratch buffers have an explicit bound.
        let budget = lz4_flex::block::get_maximum_output_size(raw.len());
        if encoded.try_reserve_exact(budget).is_err() {
            return false;
        }
        encoded.resize(budget, 0);
        let Ok(written) = lz4_flex::block::compress_into(&raw, &mut encoded) else {
            return false;
        };
        if written + rows.len() * size_of::<u16>() + 64 >= raw.len() {
            return false;
        }
        encoded.truncate(written);
        let count = rows.len();
        let mut recycle_widths = Vec::new();
        if recycle_widths.try_reserve_exact(count).is_err() {
            return false;
        }
        recycle_widths.extend(rows.iter().map(|row| row.recycle_width()));
        self.storage = Storage::Compressed {
            bytes: encoded.into_boxed_slice(),
            raw_len: raw.len(),
            rows: count,
            skip: 0,
            recycle_widths: recycle_widths.into_boxed_slice(),
        };
        true
    }
}

fn decode_block(encoded: &[u8], raw_len: usize, rows: usize) -> Option<Vec<RowRef>> {
    if raw_len == 0 || raw_len > MAX_RAW_BYTES || rows == 0 || rows > MAX_ROWS {
        return None;
    }
    let mut raw = Vec::new();
    raw.try_reserve_exact(raw_len).ok()?;
    raw.resize(raw_len, 0);
    if lz4_flex::block::decompress_into(encoded, &mut raw).ok()? != raw_len {
        return None;
    }
    let mut input = raw.as_slice();
    let mut result = Vec::with_capacity(rows);
    for _ in 0..rows {
        result.push(RowRef::new(Row::decode_history(&mut input)?));
    }
    input.is_empty().then_some(result)
}

#[derive(Default)]
pub(crate) struct History {
    blocks: VecDeque<Block>,
    len: usize,
    scan: usize,
    scan_rows: usize,
}
impl History {
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn clear(&mut self) {
        self.blocks.clear();
        self.len = 0;
        self.scan = 0;
        self.scan_rows = 0;
    }
    pub fn push_back(&mut self, row: RowRef) {
        let size = row.history_size_bound();
        if let Some(Block {
            storage: Storage::Resident { rows, size_bound },
            attempted: false,
        }) = self.blocks.back_mut()
            && rows.len() < MAX_ROWS
            && *size_bound + size <= TARGET_BYTES
        {
            rows.push_back(row);
            *size_bound += size;
        } else {
            self.blocks.push_back(Block {
                storage: Storage::Resident {
                    rows: VecDeque::from([row]),
                    size_bound: size,
                },
                attempted: false,
            });
        }
        self.len += 1;
    }
    /// Evict without decoding. A resident old row can be recycled; a compressed
    /// row is skipped in-place until the block's final row is evicted.
    pub fn pop_front(&mut self) -> Option<RowRef> {
        let block = self.blocks.front_mut()?;
        let recycled = match &mut block.storage {
            Storage::Resident { rows, size_bound } => {
                let row = rows.pop_front()?;
                *size_bound -= row.history_size_bound();
                Some(row)
            }
            Storage::Compressed { skip, .. } => {
                *skip += 1;
                None
            }
        };
        self.len -= 1;
        if self.scan > 0 {
            self.scan_rows = self.scan_rows.saturating_sub(1);
        }
        if block.len() == 0 {
            self.blocks.pop_front();
            self.scan = self.scan.saturating_sub(1);
        }
        recycled
    }
    /// Preserve row-reuse semantics while avoiding a decode of evicted cells.
    /// Discard-only pruning uses pop_front instead, so it allocates no rows.
    pub fn pop_front_recycle(&mut self, cols: usize) -> Option<RowRef> {
        let retained_width = match &self.blocks.front()?.storage {
            Storage::Compressed {
                recycle_widths,
                skip,
                ..
            } => Some(recycle_widths[*skip]),
            Storage::Resident { .. } => None,
        };
        self.pop_front()
            .or_else(|| retained_width.map(|width| RowRef::new(Row::new_recycled(cols, width))))
    }
    pub fn from_rows(rows: VecDeque<RowRef>) -> Self {
        let mut history = Self::default();
        for row in rows {
            history.push_back(row);
        }
        history
    }
    pub fn into_rows(self) -> VecDeque<RowRef> {
        let mut result = VecDeque::with_capacity(self.len);
        for block in self.blocks {
            match block.storage {
                Storage::Resident { mut rows, .. } => result.append(&mut rows),
                Storage::Compressed {
                    bytes,
                    raw_len,
                    rows,
                    skip,
                    ..
                } => result.extend(
                    decode_block(&bytes, raw_len, rows)
                        .expect("internally encoded history is valid")
                        .into_iter()
                        .skip(skip),
                ),
            }
        }
        result
    }
    /// At most eight candidates and one bounded block per call. Keep recent
    /// rows hot; callers can keep scheduling while this returns true.
    pub fn compress_step(&mut self, hot_rows: usize) -> bool {
        let cold = self.len.saturating_sub(hot_rows);
        for _ in 0..MAX_INSPECTED {
            let Some(block) = self.blocks.get_mut(self.scan) else {
                return false;
            };
            if self.scan_rows + block.len() > cold {
                return false;
            }
            self.scan_rows += block.len();
            self.scan += 1;
            if !block.attempted {
                block.compress();
                return self.scan < self.blocks.len();
            }
        }
        self.scan < self.blocks.len()
    }
    pub fn range(&self, start: usize, count: usize) -> Rows<'_> {
        let remaining = count.min(self.len.saturating_sub(start));
        if remaining == 0 {
            return Rows {
                history: self,
                block: self.blocks.len(),
                skip: 0,
                remaining: 0,
                decoded: Vec::new().into_iter(),
            };
        }
        let mut skip = start.min(self.len);
        let mut block = 0;
        while let Some(next) = self.blocks.get(block) {
            if skip < next.len() {
                break;
            }
            skip -= next.len();
            block += 1;
        }
        Rows {
            history: self,
            block,
            skip,
            remaining,
            decoded: Vec::new().into_iter(),
        }
    }
    pub fn memory(&self) -> HistoryMemory {
        let mut memory = HistoryMemory {
            rows: self.len,
            ..HistoryMemory::default()
        };
        for block in &self.blocks {
            match &block.storage {
                Storage::Resident { rows, size_bound } => {
                    memory.resident_rows += rows.len();
                    memory.resident_size_bound += size_bound;
                }
                Storage::Compressed {
                    bytes,
                    rows,
                    skip,
                    recycle_widths,
                    ..
                } => {
                    memory.compressed_rows += rows - skip;
                    memory.compressed_bytes +=
                        bytes.len() + recycle_widths.len() * size_of::<u16>();
                    memory.compressed_blocks += 1;
                }
            }
        }
        memory
    }
}
pub(crate) struct Rows<'a> {
    history: &'a History,
    block: usize,
    skip: usize,
    remaining: usize,
    decoded: std::vec::IntoIter<RowRef>,
}
impl Iterator for Rows<'_> {
    type Item = RowRef;
    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        loop {
            if let Some(row) = self.decoded.next() {
                self.remaining -= 1;
                return Some(row);
            }
            let block = self.history.blocks.get(self.block)?;
            self.block += 1;
            self.decoded = block.decode().into_iter();
            for _ in 0..self.skip {
                self.decoded.next();
            }
            self.skip = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Rgb, Style};
    fn row(index: usize) -> RowRef {
        let mut row = Row::new(80);
        for col in 0..72 {
            row.put(
                col,
                char::from(b'a' + ((col + index) % 26) as u8),
                1,
                Style {
                    foreground: Color::Rgb(Rgb(index as u8, 17, 89)),
                    ..Style::default()
                },
                col % 7 == 0,
            );
        }
        row.combine(3, '\u{301}');
        RowRef::new(row)
    }
    fn encoded(row: &Row) -> Vec<u8> {
        let mut bytes = Vec::new();
        row.encode_history(&mut bytes);
        bytes
    }
    #[test]
    fn compressed_eviction_reads_only_owned_recycle_metadata() {
        let mut history = History::default();
        for index in 0..64 {
            let mut row = Row::new(8 + index % 3);
            row.resize(4);
            history.push_back(RowRef::new(row));
        }
        while history.compress_step(0) {}
        assert_eq!(history.memory().compressed_rows, 64);
        // Deliberately invalidate the compressed cells: discard/recycle must
        // still work because neither operation needs to restore old content.
        let Storage::Compressed { bytes, .. } = &mut history.blocks[0].storage else {
            panic!("expected a compressed block");
        };
        *bytes = vec![0xff].into_boxed_slice();
        assert!(history.range(0, 0).next().is_none());
        assert!(history.range(history.len(), usize::MAX).next().is_none());
        for _ in 0..13 {
            assert!(history.pop_front().is_none());
        }
        let mut recycled = history.pop_front_recycle(4).unwrap();
        let row = RowRef::make_mut(&mut recycled);
        row.reset(4, Style::default());
        row.wrapped = true;
        row.resize(9); // The thirteenth row originally retained nine columns.
        assert!(row.wrapped);
        row.resize(10);
        assert!(!row.wrapped);
        for _ in 0..50 {
            assert!(history.pop_front().is_none());
        }
        assert_eq!(history.len(), 0);
        assert!(history.pop_front_recycle(4).is_none());
    }
    #[test]
    fn compression_preserves_snapshots_partial_reads_eviction_and_restoration() {
        let mut history = History::default();
        for index in 0..1000 {
            history.push_back(row(index));
        }
        let held: Vec<_> = history.range(0, 1000).collect();
        let expected: Vec<_> = held.iter().map(|row| encoded(row)).collect();
        let mut steps = 0;
        while history.compress_step(48) {
            steps += 1;
            assert!(steps < 1000);
        }
        let cold = history.memory();
        assert!(cold.compressed_rows > 800);
        assert_eq!(
            history
                .range(111, 19)
                .map(|row| encoded(&row))
                .collect::<Vec<_>>(),
            expected[111..130]
        );
        assert_eq!(
            history.memory(),
            cold,
            "reading must not retain resident decoded pages"
        );
        for _ in 0..137 {
            history.pop_front();
        }
        assert_eq!(history.len(), 863);
        assert_eq!(
            history
                .range(0, 863)
                .map(|row| encoded(&row))
                .collect::<Vec<_>>(),
            expected[137..]
        );
        let restored = history.into_rows();
        assert_eq!(
            restored.iter().map(|row| encoded(row)).collect::<Vec<_>>(),
            expected[137..]
        );
        assert_eq!(
            held.iter().map(|row| encoded(row)).collect::<Vec<_>>(),
            expected
        );
    }
    #[test]
    fn maximum_width_grapheme_row_has_bounded_oversized_block() {
        let mut row = Row::new(crate::MAX_COLS);
        for col in 0..crate::MAX_COLS {
            row.put(col, 'a', 1, Style::default(), false);
            for _ in 0..63 {
                row.combine(col, '\u{301}');
            }
        }
        assert!(row.history_size_bound() > TARGET_BYTES);
        assert!(row.history_size_bound() < MAX_RAW_BYTES);
        let expected = encoded(&row);
        let mut history = History::default();
        history.push_back(RowRef::new(row));
        while history.compress_step(0) {}
        assert_eq!(history.memory().compressed_rows, 1);
        assert_eq!(encoded(&history.range(0, 1).next().unwrap()), expected);
        assert!(decode_block(&[], MAX_RAW_BYTES + 1, 1).is_none());
        assert!(decode_block(&[], 8, MAX_ROWS + 1).is_none());
    }
    #[test]
    fn corrupt_lz4_and_row_archives_are_rejected_without_panicking() {
        let mut raw = Vec::new();
        row(1).encode_history(&mut raw);
        let data = lz4_flex::block::compress(&raw);
        for end in 0..data.len() {
            assert!(decode_block(&data[..end], raw.len(), 1).is_none());
        }
        assert!(decode_block(&data, raw.len() - 1, 1).is_none());
        assert!(decode_block(&data, raw.len() + 1, 1).is_none());
        assert!(decode_block(&data, raw.len(), 2).is_none());
        let mut seed = 0xf7c44291u32;
        for _ in 0..10000 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let mut changed = data.clone();
            let at = seed as usize % changed.len();
            changed[at] = (seed >> 16) as u8;
            let _ = decode_block(&changed, raw.len(), 1);
        }
    }
    #[test]
    fn incremental_progress_survives_eviction_and_new_rows() {
        let mut history = History::default();
        for index in 0..300 {
            history.push_back(row(index));
        }
        for index in 300..1500 {
            history.pop_front();
            history.push_back(row(index));
            history.compress_step(48);
        }
        let mut steps = 0;
        while history.compress_step(48) {
            steps += 1;
            assert!(steps < 100);
        }
        assert!(history.memory().compressed_rows > 150);
        assert_eq!(
            history
                .range(0, 300)
                .map(|row| encoded(&row))
                .collect::<Vec<_>>(),
            (1200..1500)
                .map(|index| encoded(&row(index)))
                .collect::<Vec<_>>()
        );
    }
}
