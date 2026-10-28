//! Explicit, bounded archive representation. No raw Rust layout or pointer bytes
//! enter the codec, and decoding validates every offset before constructing Row.
use super::{Cell, CompactString, GraphemeMap, Row, RowLinks};
use crate::hyperlink::{LinkIdentity, LinkRef};
use crate::{Color, MAX_COLS, Rgb, Style};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RecycleLayout {
    pub retained_cols: u16,
    pub cells: u16,
    pub styles: u16,
    pub graphemes: u16,
}

pub(crate) struct Reader<'a>(pub &'a [u8]);
impl<'a> Reader<'a> {
    pub fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let (head, rest) = self.0.split_at_checked(count)?;
        self.0 = rest;
        Some(head)
    }
    pub fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    pub fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }
    pub fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
}
fn color_encode(color: Color, bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(&match color {
        Color::Default => [0, 0, 0, 0],
        Color::Indexed(index) => [1, index, 0, 0],
        Color::Rgb(Rgb(r, g, b)) => [2, r, g, b],
    });
}
fn color_decode(reader: &mut Reader<'_>) -> Option<Color> {
    match reader.take(4)? {
        [0, 0, 0, 0] => Some(Color::Default),
        [1, index, 0, 0] => Some(Color::Indexed(*index)),
        [2, r, g, b] => Some(Color::Rgb(Rgb(*r, *g, *b))),
        _ => None,
    }
}
impl Row {
    pub(crate) fn recycle_layout(&self) -> RecycleLayout {
        RecycleLayout {
            retained_cols: self.retained_cols,
            cells: self.cells.capacity() as u16,
            styles: self.styles.capacity() as u16,
            graphemes: self.graphemes.capacity() as u16,
        }
    }
    pub(crate) fn with_recycle_layout(cols: usize, layout: RecycleLayout) -> Self {
        let mut row = Self::new(cols);
        row.cells = Vec::with_capacity(usize::from(layout.cells).max(cols));
        row.cells.resize(cols, Cell::default());
        row.styles = Vec::with_capacity(usize::from(layout.styles).max(1));
        row.styles.push(Style::default());
        row.graphemes = GraphemeMap::with_capacity(usize::from(layout.graphemes));
        row.retained_cols = layout.retained_cols.max(cols as u16);
        row.dirty_end = cols as u16;
        row
    }
    /// Archive bound includes the full row-local link table, including entries
    /// retained for reuse but not currently referenced by a visible run.
    pub(crate) fn history_size_bound(&self) -> usize {
        let (links, runs) = self.links.as_ref().map_or((0, 0), |table| {
            let bytes = table
                .archive_table()
                .iter()
                .map(|link| {
                    let id = match &link.id {
                        LinkIdentity::Explicit(id) => 2 + id.len(),
                        LinkIdentity::Implicit(_) => 8,
                    };
                    4 + 1 + id + 2 + link.uri.len()
                })
                .sum::<usize>();
            (bytes, table.archive_runs().count() * 6)
        });
        25 + self.cells.len() * 8
            + self.styles.len() * 14
            + self.graphemes.len() * (crate::MAX_GRAPHEME_BYTES + 6)
            + links
            + runs
    }
    pub(crate) fn archive_links(&self) -> impl Iterator<Item = &LinkRef> {
        self.links.iter().flat_map(|links| links.archive_table())
    }
    pub(crate) fn encode_history(&self, bytes: &mut Vec<u8>, links: &HashMap<LinkRef, u32>) {
        let table = self.links.as_ref();
        let runs = table.map_or(0, |table| table.archive_runs().count());
        let count = table.map_or(0, |table| table.archive_table().len());
        let (run_capacity, table_capacity) =
            table.map_or((0, 0), |table| table.archive_capacities());
        bytes.extend_from_slice(&(self.cells.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&self.retained_cols.to_le_bytes());
        bytes.push(
            u8::from(self.wrapped)
                | (u8::from(self.wrap_continuation) << 1)
                | (u8::from(table.is_some()) << 2),
        );
        for value in [
            self.styles.len() as u16,
            self.last_style,
            self.graphemes.len() as u16,
            runs as u16,
            self.cells.capacity() as u16,
            self.styles.capacity() as u16,
            self.graphemes.capacity() as u16,
            count as u16,
            run_capacity as u16,
            table_capacity as u16,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for style in &self.styles {
            color_encode(style.foreground, bytes);
            color_encode(style.background, bytes);
            color_encode(style.underline_color, bytes);
            bytes.extend_from_slice(&style.attrs.to_le_bytes());
        }
        for cell in &self.cells {
            bytes.extend_from_slice(&cell.scalar.to_le_bytes());
            bytes.extend_from_slice(&cell.style.to_le_bytes());
            bytes.push(cell.width);
            bytes.push(cell._reserved);
        }
        for (col, text) in self.graphemes.archive_iter() {
            for value in [col as u16, text.len() as u16, text.capacity() as u16] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            bytes.extend_from_slice(text.as_bytes());
        }
        if let Some(table) = table {
            for link in table.archive_table() {
                bytes.extend_from_slice(&links[link].to_le_bytes());
            }
            for (start, end, index) in table.archive_runs() {
                for value in [start, end, index] {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn decode_history(bytes: &mut &[u8], links: &[LinkRef]) -> Option<Self> {
        {
            let mut budget = crate::memory::MAX_DECODE_BYTES;
            Self::decode_history_budget(bytes, links, &mut budget)
        }
    }
    pub(crate) fn decode_history_budget(
        bytes: &mut &[u8],
        links: &[LinkRef],
        budget: &mut usize,
    ) -> Option<Self> {
        let mut r = Reader(bytes);
        let cols = usize::from(r.u16()?);
        let retained_cols = r.u16()?;
        let flags = r.u8()?;
        let styles = usize::from(r.u16()?);
        let last_style = r.u16()?;
        let graphemes = usize::from(r.u16()?);
        let runs = usize::from(r.u16()?);
        let cells_capacity = usize::from(r.u16()?);
        let styles_capacity = usize::from(r.u16()?);
        let graphemes_capacity = usize::from(r.u16()?);
        let table_count = usize::from(r.u16()?);
        let run_capacity = usize::from(r.u16()?);
        let table_capacity = usize::from(r.u16()?);
        let has_links = flags & 4 != 0;
        if !(1..=MAX_COLS).contains(&cols)
            || usize::from(retained_cols) < cols
            || usize::from(retained_cols) > MAX_COLS
            || flags > 7
            || styles == 0
            || styles > MAX_COLS * 2 + 1
            || usize::from(last_style) >= styles
            || graphemes > cols
            || runs > cols
            || !(cols..=MAX_COLS * 2).contains(&cells_capacity)
            || !(styles..=MAX_COLS * 4 + 4).contains(&styles_capacity)
            || !(graphemes..=MAX_COLS * 2).contains(&graphemes_capacity)
            || table_count > MAX_COLS * 2 + 1
            || !(runs..=MAX_COLS * 2).contains(&run_capacity)
            || !(table_count..=MAX_COLS * 4 + 4).contains(&table_capacity)
            || (!has_links
                && (runs != 0 || table_count != 0 || run_capacity != 0 || table_capacity != 0))
        {
            return None;
        }
        // Validate the minimum complete payload before allocating any vectors.
        if r.0.len() < styles * 14 + cols * 8 + graphemes * 6 + table_count * 4 + runs * 6 {
            return None;
        }
        let base_charge =
            crate::memory::allocation(std::mem::size_of::<Row>() + std::mem::size_of::<usize>())
                + crate::memory::vector::<Cell>(cells_capacity)
                + crate::memory::vector::<Style>(styles_capacity)
                + crate::memory::vector::<(usize, CompactString)>(graphemes_capacity)
                + if has_links {
                    RowLinks::allocation_bytes(run_capacity, table_capacity)
                } else {
                    0
                };
        *budget = budget.checked_sub(base_charge)?;
        let mut row = Row::with_recycle_layout(
            cols,
            RecycleLayout {
                retained_cols,
                cells: cells_capacity as u16,
                styles: styles_capacity as u16,
                graphemes: graphemes_capacity as u16,
            },
        );
        row.dirty_end = cols as u16;
        row.styles.clear();
        for _ in 0..styles {
            row.styles.push(Style {
                foreground: color_decode(&mut r)?,
                background: color_decode(&mut r)?,
                underline_color: color_decode(&mut r)?,
                attrs: r.u16()?,
            });
        }
        row.last_style = last_style;
        row.retained_cols = retained_cols;
        row.wrapped = flags & 1 != 0;
        row.wrap_continuation = flags & 2 != 0;
        for col in 0..cols {
            let scalar = r.u32()?;
            let style = r.u16()?;
            let width = r.u8()?;
            let reserved = r.u8()?;
            if char::from_u32(scalar).is_none()
                || usize::from(style) >= styles
                || width > 2
                || reserved > 7
                || (reserved & 2 != 0 && (width != 1 || col + 1 != cols))
            {
                return None;
            }
            row.cells[col] = Cell {
                scalar,
                style,
                width,
                _reserved: reserved,
            };
        }
        for (col, cell) in row.cells.iter().enumerate() {
            if (cell.width == 0 && (col == 0 || row.cells[col - 1].width != 2))
                || (cell.width == 2 && (col + 1 == cols || row.cells[col + 1].width != 0))
            {
                return None;
            }
        }
        let mut previous = None;
        for _ in 0..graphemes {
            let col = usize::from(r.u16()?);
            let len = usize::from(r.u16()?);
            let capacity = usize::from(r.u16()?);
            if col >= cols
                || previous.is_some_and(|v| col <= v)
                || len == 0
                || len > crate::MAX_GRAPHEME_BYTES
                || capacity < len
                || capacity > 512
                || (capacity > std::mem::size_of::<CompactString>()
                    && capacity
                        < std::mem::size_of::<CompactString>() + std::mem::size_of::<usize>())
                || row.cells[col].width == 0
            {
                return None;
            }
            let text = std::str::from_utf8(r.take(len)?).ok()?;
            if text.chars().count() > crate::MAX_GRAPHEME_SCALARS
                || !text.starts_with(row.cells[col].character())
            {
                return None;
            }
            if capacity > std::mem::size_of::<CompactString>() {
                *budget = budget.checked_sub(crate::memory::allocation(capacity))?;
            }
            let mut restored = CompactString::with_capacity(capacity);
            restored.push_str(text);
            row.graphemes.insert(col, restored);
            previous = Some(col);
        }
        let mut table = Vec::with_capacity(table_capacity);
        for _ in 0..table_count {
            let index = r.u32()? as usize;
            let link = links.get(index)?;
            *budget = budget.checked_sub(link.owned_bytes())?;
            table.push(link.clone());
        }
        let mut restored_runs = Vec::new();
        restored_runs.try_reserve_exact(runs).ok()?;
        let mut previous_end = 0;
        for _ in 0..runs {
            let start = usize::from(r.u16()?);
            let end = usize::from(r.u16()?);
            let index = r.u16()?;
            if start < previous_end || start >= end || end > cols {
                return None;
            }
            table.get(usize::from(index))?;
            restored_runs.push((start as u16, end as u16, index));
            previous_end = end;
        }
        if has_links {
            row.links = Some(Box::new(RowLinks::restore_archive(
                table,
                restored_runs,
                run_capacity,
                cols,
            )?));
        }
        *bytes = r.0;
        Some(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capacities_and_allocation_budget_are_validated_before_restore() {
        let mut row = Row::new(40);
        for _ in 0..20 {
            row.combine(0, '\u{301}');
        }
        let mut bytes = Vec::new();
        row.encode_history(&mut bytes, &HashMap::new());
        for offset in [13, 15, 17, 19, 21, 23] {
            let mut corrupt = bytes.clone();
            corrupt[offset..offset + 2].copy_from_slice(&u16::MAX.to_le_bytes());
            assert!(
                Row::decode_history(&mut corrupt.as_slice(), &[]).is_none(),
                "capacity offset{offset}"
            );
        }
        let mut budget = 0;
        assert!(Row::decode_history_budget(&mut bytes.as_slice(), &[], &mut budget).is_none());
        let mut budget = row.owned_bytes();
        let restored = Row::decode_history_budget(&mut bytes.as_slice(), &[], &mut budget).unwrap();
        assert_eq!(budget, 0);
        assert_eq!(row.owned_bytes(), restored.owned_bytes());
        assert_eq!(row.owned_bytes(), row.clone().owned_bytes());
    }
    #[test]
    fn hyperlink_runs_round_trip_and_reject_corrupt_ranges_and_indices() {
        use crate::hyperlink::Hyperlink;
        let links = vec![
            LinkRef::new(Hyperlink {
                id: LinkIdentity::Implicit(7),
                uri: b"https://one.test/".as_slice().into(),
            }),
            LinkRef::new(Hyperlink {
                id: LinkIdentity::Explicit(b"same".as_slice().into()),
                uri: b"https://two.test/".as_slice().into(),
            }),
        ];
        let ids = links
            .iter()
            .cloned()
            .enumerate()
            .map(|(id, link)| (link, id as u32))
            .collect();
        let mut row = Row::new(8);
        row.set_hyperlink_range(1..3, Some(&links[0]));
        row.set_hyperlink_range(4..8, Some(&links[1]));
        let mut encoded = Vec::new();
        row.encode_history(&mut encoded, &ids);
        let restored = Row::decode_history(&mut encoded.as_slice(), &links).unwrap();
        for col in 0..8 {
            assert_eq!(row.hyperlink(col), restored.hyperlink(col));
        }
        for end in 0..encoded.len() {
            assert!(Row::decode_history(&mut &encoded[..end], &links).is_none());
        }
        let runs_start = encoded.len() - 12;
        for (at, bytes) in [
            (11, u16::MAX.to_le_bytes().to_vec()),
            (runs_start, 8u16.to_le_bytes().to_vec()),
            (runs_start + 2, 9u16.to_le_bytes().to_vec()),
            (runs_start + 4, u16::MAX.to_le_bytes().to_vec()),
            (runs_start + 6, 2u16.to_le_bytes().to_vec()),
        ] {
            let mut corrupt = encoded.clone();
            corrupt[at..at + bytes.len()].copy_from_slice(&bytes);
            assert!(
                Row::decode_history(&mut corrupt.as_slice(), &links).is_none(),
                "corrupt offset {at}"
            );
        }
    }
    #[test]
    fn exact_archive_round_trip_and_all_truncations() {
        let mut row = Row::new(17);
        row.put(
            0,
            'e',
            1,
            Style {
                foreground: Color::Rgb(Rgb(1, 2, 3)),
                attrs: 0x0dff,
                ..Style::default()
            },
            true,
        );
        row.combine(0, '\u{301}');
        row.put(3, '雪', 2, Style::default(), false);
        row.erase(
            8,
            12,
            Style {
                background: Color::Indexed(7),
                ..Style::default()
            },
        );
        row.set_spacer_head(16, Style::default());
        row.wrapped = true;
        row.wrap_continuation = true;
        let mut bytes = Vec::new();
        row.encode_history(&mut bytes, &HashMap::new());
        assert!(bytes.len() <= row.history_size_bound());
        let mut input = bytes.as_slice();
        let decoded = Row::decode_history(&mut input, &[]).unwrap();
        assert!(input.is_empty());
        let mut restored = Vec::new();
        decoded.encode_history(&mut restored, &HashMap::new());
        assert_eq!(restored, bytes);
        for end in 0..bytes.len() {
            assert!(
                Row::decode_history(&mut &bytes[..end], &[]).is_none(),
                "accepted truncated archive at {end}"
            );
        }
    }
    #[test]
    fn malformed_archives_are_bounded_and_never_panic() {
        let mut row = Row::new(13);
        row.put(0, '雪', 2, Style::default(), true);
        row.put(3, 'e', 1, Style::default(), false);
        row.combine(3, '\u{301}');
        let mut source = Vec::new();
        row.encode_history(&mut source, &HashMap::new());
        let mut seed = 0x4f37a290u32;
        for _ in 0..10000 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let mut bytes = source.clone();
            let at = seed as usize % bytes.len();
            bytes[at] = (seed >> 16) as u8;
            let _ = Row::decode_history(&mut bytes.as_slice(), &[]);
        }
        let mut large = source.clone();
        large[0..2].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(Row::decode_history(&mut large.as_slice(), &[]).is_none());
        large = source.clone();
        large[5..7].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(Row::decode_history(&mut large.as_slice(), &[]).is_none());
    }
}
