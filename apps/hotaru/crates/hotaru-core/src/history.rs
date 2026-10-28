//! Cold scrollback blocks. Captures share immutable resident or compressed blocks;
//! eviction keeps a private logical prefix and never changes a captured view.
use crate::hyperlink::{Hyperlink, LinkIdentity, LinkRef};
use crate::{Row, RowRef};
use std::collections::{HashMap, VecDeque};
use std::mem::size_of;
use triomphe::Arc;

const TARGET_BYTES: usize = 64 * 1024;
const MAX_ROWS: usize = 64;
const MAX_RAW_BYTES: usize = 4 * 1024 * 1024;
const MAX_INSPECTED: usize = 8;
const MAX_LINK_BYTES: usize = 2048;
const ARCHIVE_MAGIC: &[u8; 4] = b"HHR2";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HistoryMemory {
    pub rows: usize,
    pub resident_rows: usize,
    pub compressed_rows: usize,
    /// Upper bound on serialized resident data, not allocator accounting or RSS.
    pub resident_size_bound: usize,
    /// Encoded bytes plus row-recycling metadata; excludes allocation headers
    /// and storage retained exclusively by outstanding content snapshots.
    pub compressed_bytes: usize,
    pub compressed_blocks: usize,
}

#[derive(Clone)]
enum Storage {
    // Empty slots are evicted prefixes of uniquely owned blocks. A shared
    // block is never mutated by eviction, so its captured prefix stays valid.
    Resident(Vec<Option<RowRef>>),
    Compressed {
        bytes: Box<[u8]>,
        raw_len: usize,
        rows: usize,
        recycle_widths: Box<[u16]>,
    },
}
#[derive(Clone)]
struct Block {
    storage: Storage,
}
impl Block {
    fn len(&self) -> usize {
        match &self.storage {
            Storage::Resident(rows) => rows.len(),
            Storage::Compressed { rows, .. } => *rows,
        }
    }
    fn read(&self, skip: usize) -> Vec<RowRef> {
        match &self.storage {
            Storage::Resident(rows) => rows[skip..]
                .iter()
                .map(|row| row.as_ref().expect("live row slot").clone())
                .collect(),
            Storage::Compressed {
                bytes,
                raw_len,
                rows,
                ..
            } => decode_block(bytes, *raw_len, *rows)
                .expect("internally encoded history is valid")
                .into_iter()
                .skip(skip)
                .collect(),
        }
    }
    fn compressed(&self, skip: usize, size_bound: usize) -> Option<Self> {
        let Storage::Resident(slots) = &self.storage else {
            return None;
        };
        let rows = &slots[skip..];
        let size_bound = size_bound.checked_add(8)?;
        if size_bound > MAX_RAW_BYTES || rows.is_empty() {
            return None;
        }
        let mut raw = Vec::new();
        raw.try_reserve_exact(size_bound).ok()?;
        let links = encode_links(
            rows.iter()
                .map(|row| &**row.as_ref().expect("live row slot")),
            &mut raw,
        )?;
        for row in rows {
            row.as_ref()
                .expect("live row slot")
                .encode_history(&mut raw, &links);
        }
        debug_assert!(raw.len() <= size_bound);
        let budget = lz4_flex::block::get_maximum_output_size(raw.len());
        let mut encoded = Vec::new();
        encoded.try_reserve_exact(budget).ok()?;
        encoded.resize(budget, 0);
        let written = lz4_flex::block::compress_into(&raw, &mut encoded).ok()?;
        if written + rows.len() * size_of::<u16>() + 64 >= raw.len() {
            return None;
        }
        encoded.truncate(written);
        let mut recycle_widths = Vec::new();
        recycle_widths.try_reserve_exact(rows.len()).ok()?;
        recycle_widths.extend(
            rows.iter()
                .map(|row| row.as_ref().expect("live row slot").recycle_width()),
        );
        Some(Self {
            storage: Storage::Compressed {
                bytes: encoded.into_boxed_slice(),
                raw_len: raw.len(),
                rows: rows.len(),
                recycle_widths: recycle_widths.into_boxed_slice(),
            },
        })
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
    let links = decode_links(&mut input)?;
    let mut result = Vec::with_capacity(rows);
    for _ in 0..rows {
        result.push(RowRef::new(Row::decode_history(&mut input, &links)?));
    }
    input.is_empty().then_some(result)
}

// A block dictionary shares long URIs across rows and runs. IDs remain raw
// bytes, since OSC 8 does not require either identity or URI to be UTF-8.
fn encode_links<'a>(
    rows: impl Iterator<Item = &'a Row>,
    raw: &mut Vec<u8>,
) -> Option<HashMap<LinkRef, u32>> {
    raw.extend_from_slice(ARCHIVE_MAGIC);
    raw.extend_from_slice(&0u32.to_le_bytes());
    let mut links = HashMap::new();
    for row in rows {
        for (_, link) in row.hyperlink_runs() {
            if links.contains_key(link) {
                continue;
            }
            if link.uri.is_empty() || link.uri.len() > MAX_LINK_BYTES {
                return None;
            }
            match &link.id {
                LinkIdentity::Explicit(id) => {
                    if id.len() > MAX_LINK_BYTES {
                        return None;
                    }
                    raw.push(0);
                    raw.extend_from_slice(&(id.len() as u16).to_le_bytes());
                    raw.extend_from_slice(id);
                }
                LinkIdentity::Implicit(id) => {
                    raw.push(1);
                    raw.extend_from_slice(&id.to_le_bytes());
                }
            }
            raw.extend_from_slice(&(link.uri.len() as u16).to_le_bytes());
            raw.extend_from_slice(&link.uri);
            let id = links.len() as u32;
            links.insert(link.clone(), id);
        }
    }
    raw[4..8].copy_from_slice(&(links.len() as u32).to_le_bytes());
    Some(links)
}
fn take<'a>(input: &mut &'a [u8], count: usize) -> Option<&'a [u8]> {
    let (head, rest) = input.split_at_checked(count)?;
    *input = rest;
    Some(head)
}
fn take_u16(input: &mut &[u8]) -> Option<usize> {
    Some(usize::from(u16::from_le_bytes(
        take(input, 2)?.try_into().ok()?,
    )))
}
fn decode_links(input: &mut &[u8]) -> Option<Vec<LinkRef>> {
    if take(input, 4)? != ARCHIVE_MAGIC {
        return None;
    }
    let count = u32::from_le_bytes(take(input, 4)?.try_into().ok()?) as usize;
    // Even the smallest dictionary entry takes six bytes. Check this before
    // allocating; the raw block and maximum cell count provide further caps.
    if count > MAX_ROWS * crate::MAX_COLS || count > input.len() / 6 {
        return None;
    }
    let mut links = Vec::new();
    links.try_reserve_exact(count).ok()?;
    for _ in 0..count {
        let id = match take(input, 1)?[0] {
            0 => {
                let len = take_u16(input)?;
                if len > MAX_LINK_BYTES {
                    return None;
                }
                LinkIdentity::Explicit(take(input, len)?.into())
            }
            1 => LinkIdentity::Implicit(u64::from_le_bytes(take(input, 8)?.try_into().ok()?)),
            _ => return None,
        };
        let len = take_u16(input)?;
        if len == 0 || len > MAX_LINK_BYTES {
            return None;
        }
        let uri = take(input, len)?.into();
        links.push(LinkRef::new(Hyperlink { id, uri }));
    }
    Some(links)
}
struct Entry {
    block: Arc<Block>,
    skip: usize,
    size_bound: usize,
    attempted: bool,
}
impl Entry {
    fn len(&self) -> usize {
        self.block.len() - self.skip
    }
}

#[derive(Default)]
pub(crate) struct History {
    blocks: VecDeque<Entry>,
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
        if let Some(entry) = self.blocks.back_mut()
            && !entry.attempted
            && entry.block.len() < MAX_ROWS
            && entry.size_bound + size <= TARGET_BYTES
            && matches!(entry.block.storage, Storage::Resident(_))
        {
            // This is the only COW block mutation: at most 64 RowRef slots.
            let Storage::Resident(rows) = &mut Arc::make_mut(&mut entry.block).storage else {
                unreachable!()
            };
            rows.push(Some(row));
            entry.size_bound += size;
        } else {
            self.blocks.push_back(Entry {
                block: Arc::new(Block {
                    storage: Storage::Resident(vec![Some(row)]),
                }),
                skip: 0,
                size_bound: size,
                attempted: false,
            });
        }
        self.len += 1;
    }
    pub fn pop_front(&mut self) -> Option<RowRef> {
        self.remove_front(false, 0)
    }
    pub fn pop_front_recycle(&mut self, cols: usize) -> Option<RowRef> {
        self.remove_front(true, cols)
    }
    fn remove_front(&mut self, recycle: bool, cols: usize) -> Option<RowRef> {
        let entry = self.blocks.front_mut()?;
        let index = entry.skip;
        let result = match &entry.block.storage {
            Storage::Resident(rows) => {
                entry.size_bound -= rows[index]
                    .as_ref()
                    .expect("live row slot")
                    .history_size_bound();
                if let Some(block) = Arc::get_mut(&mut entry.block) {
                    let Storage::Resident(rows) = &mut block.storage else {
                        unreachable!()
                    };
                    let row = rows[index].take();
                    if recycle {
                        row
                    } else {
                        drop(row);
                        None
                    }
                } else if recycle {
                    let Storage::Resident(rows) = &entry.block.storage else {
                        unreachable!()
                    };
                    rows[index].clone()
                } else {
                    None
                }
            }
            Storage::Compressed { recycle_widths, .. } => {
                recycle.then(|| RowRef::new(Row::new_recycled(cols, recycle_widths[index])))
            }
        };
        entry.skip += 1;
        self.len -= 1;
        if self.scan > 0 {
            self.scan_rows = self.scan_rows.saturating_sub(1);
        }
        if entry.len() == 0 {
            self.blocks.pop_front();
            self.scan = self.scan.saturating_sub(1);
        }
        result
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
        for entry in self.blocks {
            match Arc::try_unwrap(entry.block) {
                Ok(Block {
                    storage: Storage::Resident(rows),
                }) => result.extend(
                    rows.into_iter()
                        .skip(entry.skip)
                        .map(|row| row.expect("live row slot")),
                ),
                Ok(block) => result.extend(block.read(entry.skip)),
                Err(block) => result.extend(block.read(entry.skip)),
            }
        }
        result
    }
    /// Inspect at most eight candidates and compress at most one bounded block.
    pub fn compress_step(&mut self, hot_rows: usize) -> bool {
        let cold = self.len.saturating_sub(hot_rows);
        for _ in 0..MAX_INSPECTED {
            let Some(entry) = self.blocks.get_mut(self.scan) else {
                return false;
            };
            if self.scan_rows + entry.len() > cold {
                return false;
            }
            self.scan_rows += entry.len();
            self.scan += 1;
            if !entry.attempted {
                entry.attempted = true;
                if let Some(compressed) = entry.block.compressed(entry.skip, entry.size_bound) {
                    entry.block = Arc::new(compressed);
                    entry.skip = 0;
                    entry.size_bound = 0;
                }
                return self.scan < self.blocks.len();
            }
        }
        self.scan < self.blocks.len()
    }
    pub fn range(&self, start: usize, count: usize) -> Rows<'_> {
        let remaining = count.min(self.len.saturating_sub(start));
        if remaining == 0 {
            return Rows::empty(Source::Live(self));
        }
        let mut skip = start;
        let mut block = 0;
        while let Some(next) = self.blocks.get(block) {
            if skip < next.len() {
                break;
            }
            skip -= next.len();
            block += 1;
        }
        Rows {
            source: Source::Live(self),
            block,
            offset: skip,
            remaining,
            decoded: Vec::new().into_iter(),
        }
    }
    pub fn capture(&self) -> HistorySnapshot {
        let mut end = 0;
        let blocks = self
            .blocks
            .iter()
            .map(|entry| {
                end += entry.len();
                CapturedBlock {
                    block: entry.block.clone(),
                    skip: entry.skip,
                    end,
                }
            })
            .collect();
        HistorySnapshot {
            blocks: Arc::new(blocks),
            len: self.len,
        }
    }
    pub fn memory(&self) -> HistoryMemory {
        let mut memory = HistoryMemory {
            rows: self.len,
            ..HistoryMemory::default()
        };
        for entry in &self.blocks {
            match &entry.block.storage {
                Storage::Resident(_) => {
                    memory.resident_rows += entry.len();
                    memory.resident_size_bound += entry.size_bound;
                }
                Storage::Compressed {
                    bytes,
                    recycle_widths,
                    ..
                } => {
                    memory.compressed_rows += entry.len();
                    memory.compressed_bytes +=
                        bytes.len() + recycle_widths.len() * size_of::<u16>();
                    memory.compressed_blocks += 1;
                }
            }
        }
        memory
    }
}
struct CapturedBlock {
    block: Arc<Block>,
    skip: usize,
    end: usize,
}
/// Frozen history coordinates and owned immutable storage. Cloning this handle
/// is constant-time; capture clones one reference per block, never per row.
#[derive(Clone)]
pub struct HistorySnapshot {
    blocks: Arc<Vec<CapturedBlock>>,
    len: usize,
}
impl Default for HistorySnapshot {
    fn default() -> Self {
        Self {
            blocks: Arc::new(Vec::new()),
            len: 0,
        }
    }
}
impl std::fmt::Debug for HistorySnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HistorySnapshot")
            .field("rows", &self.len)
            .field("blocks", &self.blocks.len())
            .finish()
    }
}
impl HistorySnapshot {
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub(crate) fn range(&self, start: usize, count: usize) -> Rows<'_> {
        let remaining = count.min(self.len.saturating_sub(start));
        if remaining == 0 {
            return Rows::empty(Source::Captured(self));
        }
        let block = self.blocks.partition_point(|entry| entry.end <= start);
        let before = block
            .checked_sub(1)
            .map_or(0, |index| self.blocks[index].end);
        Rows {
            source: Source::Captured(self),
            block,
            offset: start - before,
            remaining,
            decoded: Vec::new().into_iter(),
        }
    }
}
enum Source<'a> {
    Live(&'a History),
    Captured(&'a HistorySnapshot),
}
impl Source<'_> {
    fn block(&self, index: usize) -> Option<(&Block, usize)> {
        match self {
            Self::Live(history) => history
                .blocks
                .get(index)
                .map(|entry| (&*entry.block, entry.skip)),
            Self::Captured(history) => history
                .blocks
                .get(index)
                .map(|entry| (&*entry.block, entry.skip)),
        }
    }
}
pub(crate) struct Rows<'a> {
    source: Source<'a>,
    block: usize,
    offset: usize,
    remaining: usize,
    decoded: std::vec::IntoIter<RowRef>,
}
impl<'a> Rows<'a> {
    fn empty(source: Source<'a>) -> Self {
        Self {
            source,
            block: 0,
            offset: 0,
            remaining: 0,
            decoded: Vec::new().into_iter(),
        }
    }
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
            let (block, skip) = self.source.block(self.block)?;
            let index = skip + self.offset;
            match &block.storage {
                Storage::Resident(rows) => {
                    if index < rows.len() {
                        self.offset += 1;
                        self.remaining -= 1;
                        return Some(rows[index].as_ref().expect("live row slot").clone());
                    }
                }
                Storage::Compressed {
                    bytes,
                    raw_len,
                    rows,
                    ..
                } => {
                    self.decoded = decode_block(bytes, *raw_len, *rows)
                        .expect("internally encoded history is valid")
                        .into_iter();
                    for _ in 0..index {
                        self.decoded.next();
                    }
                }
            }
            self.block += 1;
            self.offset = 0;
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
        let links = encode_links(std::iter::once(row), &mut bytes).unwrap();
        row.encode_history(&mut bytes, &links);
        bytes
    }
    #[test]
    fn captures_share_blocks_without_cloning_rows_and_tail_copy_is_bounded() {
        let mut history = History::default();
        for index in 0..100 {
            history.push_back(row(index));
        }
        let first = history.range(0, 1).next().unwrap();
        let owners = RowRef::strong_count(&first);
        let held = history.capture();
        assert_eq!(RowRef::strong_count(&first), owners);
        assert!(Arc::ptr_eq(&held.blocks[0].block, &history.blocks[0].block));
        let again = held.clone();
        assert!(Arc::ptr_eq(&held.blocks, &again.blocks));
        let tail = history.blocks.back().unwrap().block.clone();
        assert!(tail.len() < MAX_ROWS);
        history.push_back(row(100));
        assert!(!Arc::ptr_eq(&tail, &history.blocks.back().unwrap().block));
        assert!(Arc::ptr_eq(&held.blocks[0].block, &history.blocks[0].block));
        assert_eq!(held.range(0, usize::MAX).count(), 100);
        for _ in 0..17 {
            history.pop_front();
        }
        assert!(
            Arc::ptr_eq(&held.blocks[0].block, &history.blocks[0].block),
            "eviction must not COW a block"
        );
        let partial = history.capture();
        assert_eq!(partial.range(0, usize::MAX).count(), 84);
        history.clear();
        assert_eq!(encoded(&held.range(0, 1).next().unwrap()), encoded(&first));
        assert_eq!(
            encoded(&partial.range(0, 1).next().unwrap()),
            encoded(&row(17))
        );
    }
    #[test]
    fn capture_and_cancel_never_decode_a_compressed_block() {
        let mut history = History::default();
        for index in 0..128 {
            history.push_back(row(index));
        }
        while history.compress_step(0) {}
        assert!(history.memory().compressed_rows > 0);
        let Storage::Compressed { bytes, .. } =
            &mut Arc::get_mut(&mut history.blocks[0].block).unwrap().storage
        else {
            panic!("compressed block expected")
        };
        *bytes = vec![0xff].into_boxed_slice();
        // A decode would panic on the deliberately damaged bytes. Capture and
        // cancellation must only touch owned handles and frozen coordinates.
        let snapshot = crate::ContentSnapshot::capture(
            crate::Terminal::new(80, 2, 0).snapshot(),
            history.capture(),
        );
        drop(history);
        let cancel = std::sync::atomic::AtomicBool::new(true);
        assert!(snapshot.rows().next_cancellable(&cancel).is_err());
        assert_eq!(snapshot.rows_range(usize::MAX..usize::MAX).count(), 0);
    }
    #[test]
    fn hyperlink_dictionary_round_trip_is_shared_and_bounded() {
        let explicit = LinkRef::new(Hyperlink {
            id: LinkIdentity::Explicit(vec![0xff, 0, b'x'].into()),
            uri: vec![b'u'; 200].into(),
        });
        let implicit = LinkRef::new(Hyperlink {
            id: LinkIdentity::Implicit(u64::MAX),
            uri: b"https://example.test/".as_slice().into(),
        });
        let rows: Vec<_> = (0..32)
            .map(|index| {
                let mut row = RowRef::unwrap_or_clone(row(index));
                row.set_hyperlink_range(0..40, Some(&explicit));
                row.set_hyperlink_range(41..72, Some(&implicit));
                RowRef::new(row)
            })
            .collect();
        let mut raw = Vec::new();
        let links = encode_links(rows.iter().map(|row| &**row), &mut raw).unwrap();
        assert_eq!(links.len(), 2);
        let dictionary_len = raw.len();
        assert!(
            dictionary_len < 256,
            "long repeated URI must be encoded once"
        );
        for end in 0..dictionary_len {
            assert!(decode_links(&mut &raw[..end]).is_none());
        }
        let mut corrupt = raw.clone();
        corrupt[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_links(&mut corrupt.as_slice()).is_none());
        for row in &rows {
            row.encode_history(&mut raw, &links);
        }
        let restored =
            decode_block(&lz4_flex::block::compress(&raw), raw.len(), rows.len()).unwrap();
        for (before, after) in rows.iter().zip(&restored) {
            assert_eq!(encoded(before), encoded(after));
        }
        assert!(LinkRef::ptr_eq(
            restored[0].hyperlink_ref(0).unwrap(),
            restored[1].hyperlink_ref(0).unwrap()
        ));
        let mut history = History::default();
        for row in rows {
            history.push_back(row);
        }
        while history.compress_step(0) {}
        assert_eq!(history.memory().compressed_rows, 32);
        assert_eq!(
            history
                .range(0, 32)
                .map(|row| encoded(&row))
                .collect::<Vec<_>>(),
            restored.iter().map(|row| encoded(row)).collect::<Vec<_>>()
        );
    }
    #[test]
    fn oversized_hyperlink_rows_remain_resident_without_data_loss() {
        let link = LinkRef::new(Hyperlink {
            id: LinkIdentity::Implicit(1),
            uri: vec![b'x'; MAX_LINK_BYTES].into(),
        });
        let mut row = Row::new(crate::MAX_COLS);
        row.restore_hyperlink_runs(
            (0..crate::MAX_COLS)
                .step_by(2)
                .map(|col| (col..col + 1, link.clone()))
                .collect(),
        )
        .unwrap();
        assert!(row.history_size_bound() > MAX_RAW_BYTES);
        let mut history = History::default();
        history.push_back(RowRef::new(row));
        while history.compress_step(0) {}
        assert_eq!(history.memory().compressed_rows, 0);
        let held = history.capture();
        drop(history);
        assert_eq!(
            held.range(0, 1).next().unwrap().hyperlink_runs().count(),
            crate::MAX_COLS / 2
        );
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
        let Storage::Compressed { bytes, .. } =
            &mut Arc::get_mut(&mut history.blocks[0].block).unwrap().storage
        else {
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
        let row = row(1);
        let links = encode_links(std::iter::once(&*row), &mut raw).unwrap();
        row.encode_history(&mut raw, &links);
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
