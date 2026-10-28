//! Eight-byte cells, row-local style indices, and sparse owned graphemes.
//! Unlike raw offsets into a Zig page, snapshots hold an Arc<Row>. Copy-on-write
//! retains the whole row's styles and graphemes, with no borrowed palette pointer.

use crate::Style;
use crate::grapheme::GraphemeMap;
use crate::hyperlink::{Hyperlink, LinkRef, RowLinks};
use compact_str::CompactString;
use std::ops::Range;

mod ascii;
mod history;
pub(crate) use history::RecycleLayout;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Cell {
    scalar: u32,
    style: u16,
    width: u8,
    _reserved: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Color, Rgb};

    #[test]
    fn inline_grapheme_overwrite_keeps_capacity_charge_and_heap_release_updates_peak() {
        let mut row = Row::new(8);
        let style = Style::default();
        for col in [0, 1, 3] {
            row.put(col, 'e', 1, style, false);
            row.combine(col, '\u{301}');
        }
        for _ in 0..24 {
            row.combine(3, '\u{301}');
        }
        let retained = row.clone();
        let signal = triomphe::Arc::new(crate::memory::BudgetSignal::default());
        row.attach_budget(&signal);
        let before = row.owned_bytes();
        signal.rebase(before);
        signal.take_change();

        row.put(0, 'x', 1, style, false);
        assert_eq!(row.owned_bytes(), before);
        assert_eq!(row.owned_bytes(), row.uncached_owned_bytes());
        assert_eq!(signal.current(), before);
        assert_eq!(signal.take_change(), None);

        row.put(3, 'y', 1, style, false);
        let after_heap = row.owned_bytes();
        assert!(after_heap < before);
        assert_eq!(after_heap, row.uncached_owned_bytes());
        assert_eq!(signal.current(), after_heap);
        assert_eq!(signal.take_change(), Some((after_heap, before)));

        row.put(1, 'z', 1, style, false);
        assert!(row.graphemes.is_empty());
        assert!(row.graphemes.capacity() > 0);
        assert_eq!(row.owned_bytes(), after_heap);
        assert_eq!(row.owned_bytes(), row.uncached_owned_bytes());
        assert_eq!(signal.take_change(), None);
        assert_eq!(retained.grapheme(0), Some("e\u{301}"));
        assert_eq!(retained.grapheme(1), Some("e\u{301}"));
        assert_eq!(retained.grapheme(3).unwrap().chars().count(), 26);
        assert_eq!(retained.owned_bytes(), before);
    }

    #[test]
    fn partial_reset_matches_full_clear_through_mixed_row_edits() {
        let mut actual = Row::new(80);
        let mut eager = actual.clone();
        let mut seed = 0x9c21_485du32;
        for step in 0..20_000 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let col = seed as usize % actual.cells.len();
            let end = (col + 1 + (seed as usize >> 8) % 17).min(actual.cells.len());
            let style = Style {
                foreground: Color::Rgb(Rgb(seed as u8, (seed >> 8) as u8, 127)),
                background: Color::Indexed((seed >> 16) as u8),
                ..Style::default()
            };
            let mutate = |row: &mut Row| match seed % 11 {
                0 => row.reset(row.cells.len(), style),
                1 => row.resize(1 + (seed as usize >> 9) % 120),
                2 => row.erase(col, end, style),
                3 => row.erase_selective(col, end, style),
                4 => row.shift_right(col, end - col, style),
                5 => row.shift_left(col, end - col, style),
                6 => row.combine(col, '\u{301}'),
                7 => row.set_spacer_head(row.cells.len() - 1, style),
                8 => row.clear_spacer_head(),
                9 if col + 1 < row.cells.len() => row.put(col, '雪', 2, style, step % 2 == 0),
                _ => row.put(col, 'x', 1, style, step % 2 == 0),
            };
            eager.dirty_end = eager.cells.len() as u16;
            mutate(&mut actual);
            mutate(&mut eager);
            assert_eq!(
                actual.owned_bytes(),
                actual.uncached_owned_bytes(),
                "cached charge at step {step}"
            );
            assert_eq!(
                actual.owned_bytes(),
                actual.clone().owned_bytes(),
                "clone charge at step {step}"
            );
            assert_eq!(actual.cells, eager.cells, "cells at step {step}");
            assert_eq!(actual.styles, eager.styles, "styles at step {step}");
            for col in 0..actual.cells.len() {
                assert_eq!(actual.grapheme(col), eager.grapheme(col));
            }
            assert!(
                actual.cells[usize::from(actual.dirty_end)..]
                    .iter()
                    .all(|&cell| cell == Cell::blank(0)),
                "dirty suffix at step {step}"
            );
        }
    }
}

const _: () = assert!(std::mem::size_of::<Cell>() == 8);

impl Default for Cell {
    fn default() -> Self {
        Self::blank(0)
    }
}

impl Cell {
    pub(crate) fn new(ch: char, style: u16, width: u8) -> Self {
        Self {
            scalar: ch as u32,
            style,
            width,
            _reserved: 0,
        }
    }
    pub(crate) fn blank(style: u16) -> Self {
        // Bit 2 distinguishes a background-only erase from a codepoint-zero
        // cell written by the one-column wide-character path.
        Self {
            scalar: 0,
            style,
            width: 1,
            _reserved: 4,
        }
    }
    pub(crate) fn is_spacer_head(self) -> bool {
        self._reserved & 2 != 0
    }
    pub(crate) fn has_text(self) -> bool {
        self.scalar != 0
    }
    pub(crate) fn background_only(self) -> bool {
        self._reserved & 4 != 0
    }
    pub fn character(self) -> char {
        if self.scalar == 0 {
            return ' ';
        }
        char::from_u32(self.scalar).unwrap_or('\u{fffd}')
    }
    pub fn width(self) -> usize {
        self.width as usize
    }
    pub fn is_protected(self) -> bool {
        self._reserved & 1 != 0
    }
    pub(crate) fn protected(mut self, protected: bool) -> Self {
        self._reserved = (self._reserved & !1) | u8::from(protected);
        self
    }
}

#[derive(Debug)]
pub struct Row {
    pub(crate) cells: Vec<Cell>,
    styles: Vec<Style>,
    last_style: u16,
    pub(crate) graphemes: GraphemeMap,
    pub(crate) wrapped: bool,
    pub(crate) wrap_continuation: bool,
    retained_cols: u16,
    // Cells at/after this bound equal Cell::blank(0). Reset can leave that
    // already-blank suffix untouched even when style 0's value changes.
    dirty_end: u16,
    links: Option<Box<RowLinks>>,
    memory_charge: crate::memory::Charge,
}

impl Clone for Row {
    fn clone(&self) -> Self {
        Self {
            cells: crate::memory::clone_vec(&self.cells),
            styles: crate::memory::clone_vec(&self.styles),
            last_style: self.last_style,
            graphemes: self.graphemes.clone(),
            wrapped: self.wrapped,
            wrap_continuation: self.wrap_continuation,
            retained_cols: self.retained_cols,
            dirty_end: self.dirty_end,
            links: self.links.clone(),
            memory_charge: self.memory_charge.clone(),
        }
    }
}

impl Row {
    pub(crate) fn new(cols: usize) -> Self {
        Self {
            cells: vec![Cell::default(); cols],
            styles: vec![Style::default()],
            last_style: 0,
            graphemes: GraphemeMap::new(),
            wrapped: false,
            wrap_continuation: false,
            retained_cols: cols as u16,
            dirty_end: 0,
            links: None,
            memory_charge: crate::memory::Charge::default(),
        }
    }
    pub(crate) fn refresh_memory_charge(&mut self) {
        let bytes = self.uncached_owned_bytes();
        self.memory_charge.update(bytes);
    }
    pub(crate) fn budget_attached(
        &self,
        signal: &triomphe::Arc<crate::memory::BudgetSignal>,
    ) -> bool {
        self.memory_charge.attached_to(signal)
    }
    pub(crate) fn attach_budget(&mut self, signal: &triomphe::Arc<crate::memory::BudgetSignal>) {
        self.owned_bytes();
        self.memory_charge.attach(signal);
    }
    /// Conservative logical owned-storage estimate, including retained table
    /// capacities. This is an admission charge, not RSS or uniquely allocated
    /// bytes; multiple holders can count the same immutable row. Shared page
    /// backing must also be accounted for if the storage representation changes.
    pub fn storage_bytes_estimate(&self) -> usize {
        self.owned_bytes()
    }
    pub(crate) fn owned_bytes(&self) -> usize {
        self.memory_charge.get(|| self.uncached_owned_bytes())
    }
    pub(crate) fn has_dynamic_metadata(&self) -> bool {
        !self.graphemes.is_empty() || self.links.is_some()
    }
    pub(crate) fn needs_scalar_write_order(&self, range: Range<usize>, style: Style) -> bool {
        if self.has_dynamic_metadata()
            || self.styles.len() >= (self.cells.len() * 2 + 1).min(u16::MAX as usize)
        {
            return true;
        }
        // Scalar replacement of an interior wide glyph briefly erases its
        // other half. A batched replacement can skip that cell write, but must
        // not skip allocating the erased style. The base style slot makes the
        // common default-background path constant-time; it cannot disappear
        // through compaction because the threshold above selects scalar order.
        self.styles[0] != style.erased() && self.cells[range].iter().any(|cell| cell.width() != 1)
    }
    fn uncached_owned_bytes(&self) -> usize {
        crate::memory::allocation(std::mem::size_of::<Self>() + std::mem::size_of::<usize>())
            + crate::memory::vector::<Cell>(self.cells.capacity())
            + crate::memory::vector::<Style>(self.styles.capacity())
            + self.graphemes.owned_bytes()
            + self.links.as_ref().map_or(0, |links| links.owned_bytes())
    }
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }
    pub fn style(&self, cell: Cell) -> Style {
        self.styles[cell.style as usize]
    }
    pub fn grapheme(&self, col: usize) -> Option<&str> {
        self.graphemes.get(&col).map(CompactString::as_str)
    }
    pub fn wrapped(&self) -> bool {
        self.wrapped
    }
    pub fn hyperlink(&self, col: usize) -> Option<&Hyperlink> {
        self.hyperlink_ref(col).map(|link| &**link)
    }
    pub(crate) fn hyperlink_ref(&self, col: usize) -> Option<&LinkRef> {
        self.links.as_ref()?.get(col)
    }
    #[cfg(test)]
    pub(crate) fn hyperlink_runs(&self) -> impl Iterator<Item = (Range<usize>, &LinkRef)> {
        self.links.iter().flat_map(|links| links.iter())
    }
    /// Restore sparse metadata in linear expected time during history decode.
    #[cfg(test)]
    pub(crate) fn restore_hyperlink_runs(
        &mut self,
        runs: Vec<(Range<usize>, LinkRef)>,
    ) -> Option<()> {
        let links = RowLinks::from_ordered_runs(runs, self.cells.len())?;
        self.links = (!links.is_empty()).then(|| Box::new(links));
        self.refresh_memory_charge();
        Some(())
    }
    pub(crate) fn set_hyperlink_range(&mut self, range: Range<usize>, link: Option<&LinkRef>) {
        let range = range.start.min(self.cells.len())..range.end.min(self.cells.len());
        if range.is_empty() || (link.is_none() && self.links.is_none()) {
            return;
        }
        let links = self
            .links
            .get_or_insert_with(|| Box::new(RowLinks::default()));
        links.set(range, link, self.cells.len());
        if links.is_empty() {
            self.links = None;
        }
        self.refresh_memory_charge();
    }
    pub(crate) fn style_id(&mut self, style: Style) -> u16 {
        if self.styles[self.last_style as usize] == style {
            return self.last_style;
        }
        if let Some(id) = self.styles.iter().rposition(|s| *s == style) {
            self.last_style = id as u16;
            return self.last_style;
        }
        // No unbounded style leak from a process repeatedly repainting one row.
        // At most cols + 1 live styles survive compaction. cols is <= 8192.
        if self.styles.len() >= (self.cells.len() * 2 + 1).min(u16::MAX as usize) {
            self.compact_styles();
        }
        let id = self.styles.len() as u16;
        self.styles.push(style);
        self.last_style = id;
        self.refresh_memory_charge();
        id
    }
    fn compact_styles(&mut self) {
        // Renumbering can change style indices in the entire blank suffix.
        self.dirty_end = self.cells.len() as u16;
        let mut map = vec![u16::MAX; self.styles.len()];
        let mut live = Vec::with_capacity(self.cells.len() + 1);
        for cell in &mut self.cells {
            let old = cell.style as usize;
            if map[old] == u16::MAX {
                map[old] = live.len() as u16;
                live.push(self.styles[old]);
            }
            cell.style = map[old];
        }
        self.styles = live;
        self.last_style = 0;
        self.refresh_memory_charge();
    }
    pub(crate) fn reset(&mut self, cols: usize, style: Style) {
        self.styles.clear();
        self.styles.push(style.erased());
        self.last_style = 0;
        self.cells.resize(cols, Cell::default());
        self.cells[..usize::from(self.dirty_end).min(cols)].fill(Cell::blank(0));
        self.dirty_end = 0;
        self.graphemes.clear();
        self.links = None;
        self.wrapped = false;
        self.wrap_continuation = false;
        self.refresh_memory_charge();
    }
    pub(crate) fn erase(&mut self, start: usize, end: usize, style: Style) {
        if start >= end {
            return;
        }
        let id = self.style_id(style.erased());
        // Erasing either half of a wide glyph erases both halves.
        let start = if self.cells[start].width == 0 {
            start.saturating_sub(1)
        } else {
            start
        };
        let end = if end < self.cells.len() && self.cells[end].width == 0 {
            end + 1
        } else {
            end
        };
        self.dirty_end = self.dirty_end.max(end as u16);
        self.cells[start..end].fill(Cell::blank(id));
        self.graphemes.retain(|&col, _| col < start || col >= end);
        self.set_hyperlink_range(start..end, None);
        self.refresh_memory_charge();
    }
    pub(crate) fn erase_selective(&mut self, start: usize, end: usize, style: Style) {
        let mut col = start;
        while col < end {
            if !self.cells[col].is_protected() {
                self.erase(col, col + 1, style);
            }
            col += 1;
        }
    }
    /// Prepare an overwrite without filling cells that the caller immediately
    /// replaces. Only a wide glyph straddling either boundary needs a blank.
    pub(crate) fn prepare_write(&mut self, start: usize, end: usize, style: Style) {
        let left = start > 0 && self.cells[start].width == 0;
        self.prepare_write_with_left(start, end, left, style);
    }
    fn prepare_write_with_left(&mut self, start: usize, end: usize, left: bool, style: Style) {
        let right = end < self.cells.len() && self.cells[end].width == 0;
        self.dirty_end = self.dirty_end.max((end + usize::from(right)) as u16);
        if left || right {
            let id = self.style_id(style.erased());
            if left {
                self.cells[start - 1] = Cell::blank(id);
            }
            if right {
                self.cells[end] = Cell::blank(id);
            }
        }
        if !self.graphemes.is_empty() {
            let first = if left { start - 1 } else { start };
            let last = end + usize::from(right);
            if self.graphemes.retain(|&col, _| col < first || col >= last) {
                self.refresh_memory_charge();
            }
        }
        if self.links.is_some() {
            self.set_hyperlink_range(
                if left { start - 1 } else { start }..end + usize::from(right),
                None,
            );
        }
    }
    pub(crate) fn put(
        &mut self,
        col: usize,
        ch: char,
        width: usize,
        style: Style,
        protected: bool,
    ) {
        self.prepare_write(col, col + width, style);
        let id = self.style_id(style);
        self.cells[col] = Cell::new(ch, id, width as u8).protected(protected);
        if width == 2 {
            self.cells[col + 1] = Cell::new(' ', id, 0).protected(protected);
        }
    }
    /// Finish a positive-width print run after its interior cells were replaced.
    /// The left-wide flag was captured before writing; the right neighbor is
    /// still untouched. Only exterior wide halves and overwritten metadata
    /// are cleared here. No snapshot can observe the exclusive mutable run.
    pub(crate) fn finish_print_run(
        &mut self,
        start: usize,
        end: usize,
        left: bool,
        style: Style,
        link: Option<&LinkRef>,
    ) {
        debug_assert!(start < end && end <= self.cells.len());
        debug_assert!(!left || start > 0);
        self.prepare_write_with_left(start, end, left, style);
        if let Some(link) = link {
            self.set_hyperlink_range(start..end, Some(link));
        }
    }
    pub(crate) fn combine(&mut self, col: usize, ch: char) {
        let col = if self.cells[col].width == 0 {
            col.saturating_sub(1)
        } else {
            col
        };
        let first = self.cells[col].character();
        self.graphemes.modify(
            col,
            || {
                let mut text = CompactString::default();
                text.push(first);
                text
            },
            |text| {
                // Ghostty permits 64 suffix scalars in addition to the base.
                if text.chars().count() < crate::MAX_GRAPHEME_SCALARS {
                    text.push(ch);
                }
            },
        );
        self.refresh_memory_charge();
    }
    /// Change the retained base cell's width while writing a new tail with the
    /// current style/link. The base keeps its original style and cluster text.
    pub(crate) fn widen_grapheme(
        &mut self,
        col: usize,
        style: Style,
        protected: bool,
        link: Option<&LinkRef>,
    ) {
        debug_assert!(col + 1 < self.cells.len());
        self.prepare_write(col + 1, col + 2, style);
        let id = self.style_id(style);
        self.cells[col].width = 2;
        self.cells[col + 1] = Cell::new('\0', id, 0).protected(protected);
        if let Some(link) = link {
            self.set_hyperlink_range(col + 1..col + 2, Some(link));
        }
    }
    pub(crate) fn narrow_grapheme(&mut self, col: usize) {
        self.cells[col].width = 1;
        if col + 1 < self.cells.len() {
            self.cells[col + 1].width = 1;
            self.cells[col + 1].scalar = 0;
        }
        self.dirty_end = self.dirty_end.max((col + 2).min(self.cells.len()) as u16);
    }
    pub(crate) fn shift_right(&mut self, col: usize, count: usize, style: Style) {
        self.dirty_end = self.cells.len() as u16;
        let count = count.min(self.cells.len() - col);
        let len = self.cells.len();
        self.cells.copy_within(col..len - count, col + count);
        self.graphemes.shift_right(col, count, len);
        if let Some(links) = &mut self.links {
            links.shift_right(col, count, len);
        }
        let id = self.style_id(style.erased());
        self.cells[col..col + count].fill(Cell::blank(id));
        self.repair_wide(style);
        self.refresh_memory_charge();
    }
    pub(crate) fn shift_left(&mut self, col: usize, count: usize, style: Style) {
        self.dirty_end = self.cells.len() as u16;
        let count = count.min(self.cells.len() - col);
        let len = self.cells.len();
        self.cells.copy_within(col + count..len, col);
        self.graphemes.shift_left(col, count);
        if let Some(links) = &mut self.links {
            links.shift_left(col, count);
        }
        let id = self.style_id(style.erased());
        self.cells[len - count..].fill(Cell::blank(id));
        self.repair_wide(style);
        self.refresh_memory_charge();
    }
    /// Trailing erased cells are nonsemantic, unlike explicitly printed spaces.
    /// A background fill is meaningful even without a codepoint.
    pub(crate) fn reflow_len(&self) -> usize {
        if self.wrapped {
            return self.cells.len();
        }
        self.cells
            .iter()
            .rposition(|cell| {
                cell.scalar != 0
                    || cell.width != 1
                    || cell.is_spacer_head()
                    || (cell._reserved & 4 != 0
                        && self.style(*cell).background != crate::Color::Default)
            })
            .map_or(0, |col| col + 1)
    }
    pub(crate) fn has_text(&self) -> bool {
        self.cells
            .iter()
            .any(|cell| cell.scalar != 0 && cell.width != 0)
    }
    pub(crate) fn set_spacer_head(&mut self, col: usize, style: Style) {
        self.dirty_end = self.dirty_end.max((col + 1) as u16);
        let id = self.style_id(style);
        self.cells[col] = Cell::blank(id);
        self.cells[col]._reserved |= 2;
        self.graphemes.remove(&col);
        self.set_hyperlink_range(col..col + 1, None);
        self.refresh_memory_charge();
    }
    pub(crate) fn clear_spacer_head(&mut self) {
        if let Some(cell) = self.cells.last_mut()
            && cell.is_spacer_head()
        {
            cell._reserved &= !(2 | 4);
        }
    }
    /// Move a cell's value into another row, translating its row-local style.
    /// Sparse graphemes stay owned; outstanding snapshots retain their originals.
    pub(crate) fn copy_cell(&mut self, col: usize, source: &Row, source_col: usize) {
        self.dirty_end = self.dirty_end.max((col + 1) as u16);
        let mut cell = source.cells[source_col];
        cell.style = self.style_id(source.style(cell));
        self.cells[col] = cell;
        if let Some(grapheme) = source.graphemes.get(&source_col) {
            self.graphemes.insert(col, grapheme.clone());
        }
        self.set_hyperlink_range(col..col + 1, source.hyperlink_ref(source_col));
        self.refresh_memory_charge();
    }
    pub(crate) fn resize(&mut self, cols: usize) {
        if cols < self.cells.len() {
            self.set_hyperlink_range(cols..self.cells.len(), None);
        }
        // A partial row copy into a wider allocation does not copy wrap
        // metadata. Growing back inside retained storage keeps that metadata,
        // matching Ghostty's non-reflow screen resize.
        if cols > usize::from(self.retained_cols)
            || (cols > self.cells.len() && self.cells.last().is_some_and(|c| c.is_spacer_head()))
        {
            self.wrapped = false;
            self.wrap_continuation = false;
            self.retained_cols = cols as u16;
        }
        if cols > self.cells.len() && self.cells.last().is_some_and(|c| c.is_spacer_head()) {
            let last = self.cells.len() - 1;
            self.cells[last]._reserved &= !2;
        }
        let id = self.style_id(Style::default());
        self.cells.resize(cols, Cell::blank(id));
        self.graphemes.retain(|&c, _| c < cols);
        self.repair_wide(Style::default());
        self.dirty_end = cols as u16;
        self.refresh_memory_charge();
    }
    fn repair_wide(&mut self, style: Style) {
        let id = self.style_id(style.erased());
        for col in 0..self.cells.len() {
            let w = self.cells[col].width;
            if (w == 0 && (col == 0 || self.cells[col - 1].width != 2))
                || (w == 2 && (col + 1 == self.cells.len() || self.cells[col + 1].width != 0))
            {
                self.cells[col] = Cell::blank(id);
                self.graphemes.remove(&col);
                self.set_hyperlink_range(col..col + 1, None);
            }
        }
    }
    pub fn append_text(&self, output: &mut String, start: usize, end: usize, trim: bool) {
        let end = end.min(self.cells.len());
        let mut last = end;
        if trim {
            while last > start
                && self.cells[last - 1].character() == ' '
                && self.cells[last - 1].width != 0
                && !self.graphemes.contains_key(&(last - 1))
            {
                last -= 1;
            }
        }
        for col in start.min(last)..last {
            if self.cells[col].width == 0 || self.cells[col].is_spacer_head() {
                continue;
            }
            if let Some(text) = self.grapheme(col) {
                output.push_str(text);
            } else {
                output.push(self.cells[col].character());
            }
        }
    }
}
