//! Immutable copy/search capture. The PTY owner clones block handles; consumers
//! restore bounded blocks on their own worker, without borrowing terminal state.
use crate::history::{HistorySnapshot, Rows};
use crate::{Cursor, Modes, Rgb, RowRef, Snapshot};
use std::ops::Range;
use std::sync::atomic::{AtomicBool, Ordering};
use triomphe::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ContentRetention {
    /// Conservative retained storage, counting shared allocations in full.
    pub storage_bytes_estimate: usize,
    /// Uncompressed logical ownership, including complete shared blocks.
    pub logical_bytes: usize,
}

#[derive(Clone, Debug)]
/// Capturing costs O(history blocks + visible rows), without decoding history.
/// Cloning a capture is O(1). Later appends copy at most one 64-row block of
/// handles; eviction never copies a block, and captured rows remain immutable.
pub struct ContentSnapshot {
    pub position: crate::ContentPosition,
    pub cols: usize,
    /// Total retained physical rows, including history and active screen.
    pub rows: usize,
    /// Frozen history prefix length. Coordinate zero is the oldest captured row.
    pub history_rows: usize,
    /// Cursor row is relative to the complete captured content.
    pub cursor: Cursor,
    pub palette: [Rgb; 256],
    pub default_foreground: Rgb,
    pub default_background: Rgb,
    pub modes: Modes,
    pub generation: u64,
    history: HistorySnapshot,
    active: Arc<Vec<RowRef>>,
    retention: ContentRetention,
}
impl ContentSnapshot {
    pub(crate) fn capture(screen: Snapshot, history: HistorySnapshot) -> Self {
        let history_rows = history.len();
        let active_bytes = screen
            .lines
            .iter()
            .map(|row| row.owned_bytes())
            .sum::<usize>()
            + crate::memory::vector::<RowRef>(screen.lines.capacity())
            + crate::memory::allocation(
                std::mem::size_of::<Vec<RowRef>>() + std::mem::size_of::<usize>(),
            )
            + std::mem::size_of::<Self>();
        let retention = ContentRetention {
            storage_bytes_estimate: active_bytes + history.retained_bytes_estimate(),
            logical_bytes: active_bytes + history.logical_retained_bytes(),
        };
        let mut cursor = screen.cursor;
        cursor.row += history_rows;
        Self {
            position: crate::ContentPosition {
                first_row: 0,
                ..screen.position
            },
            cols: screen.cols,
            rows: history_rows + screen.lines.len(),
            history_rows,
            cursor,
            palette: screen.palette,
            default_foreground: screen.default_foreground,
            default_background: screen.default_background,
            modes: screen.modes,
            generation: screen.generation,
            history,
            active: Arc::new(screen.lines),
            retention,
        }
    }
    pub fn rows(&self) -> ContentRows<'_> {
        self.rows_range(0..self.rows)
    }
    /// Estimates remain stable after terminal mutation or destruction. Captures
    /// may overlap: adding their estimates overcounts shared ownership, and an
    /// external client retaining unbounded captures can retain unbounded data.
    pub fn retention(&self) -> ContentRetention {
        self.retention
    }
    /// Half-open physical row range. Out-of-range and reversed ranges are empty.
    pub fn rows_range(&self, range: Range<usize>) -> ContentRows<'_> {
        let start = range.start.min(self.rows);
        let end = range.end.min(self.rows).max(start);
        let history_start = start.min(self.history_rows);
        let history_end = end.min(self.history_rows);
        let active_start = start
            .saturating_sub(self.history_rows)
            .min(self.active.len());
        let active_end = end.saturating_sub(self.history_rows).min(self.active.len());
        ContentRows {
            history: self
                .history
                .range(history_start, history_end - history_start),
            active: self.active[active_start..active_end].iter(),
        }
    }
}
impl From<Snapshot> for ContentSnapshot {
    fn from(snapshot: Snapshot) -> Self {
        let position = snapshot.position.clone();
        let mut capture = Self::capture(snapshot, HistorySnapshot::default());
        capture.position = position;
        capture
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentCancelled;
impl std::fmt::Display for ContentCancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("content iteration cancelled")
    }
}
impl std::error::Error for ContentCancelled {}

pub struct ContentRows<'a> {
    history: Rows<'a>,
    active: std::slice::Iter<'a, RowRef>,
}
impl ContentRows<'_> {
    /// Checks cancellation before every row and therefore before every decode.
    /// A single block decode is bounded by the history codec's 4MiB raw cap.
    pub fn next_cancellable(
        &mut self,
        cancel: &AtomicBool,
    ) -> Result<Option<RowRef>, ContentCancelled> {
        if cancel.load(Ordering::Acquire) {
            return Err(ContentCancelled);
        }
        Ok(self.next())
    }
}
impl Iterator for ContentRows<'_> {
    type Item = RowRef;
    fn next(&mut self) -> Option<Self::Item> {
        self.history.next().or_else(|| self.active.next().cloned())
    }
}
