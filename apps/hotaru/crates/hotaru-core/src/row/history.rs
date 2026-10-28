//! Explicit, bounded archive representation. No raw Rust layout or pointer bytes
//! enter the codec, and decoding validates every offset before constructing Row.
use super::{Cell, CompactString, Row};
use crate::{Color, MAX_COLS, Rgb, Style};

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
    pub(crate) fn recycle_width(&self) -> u16 {
        self.retained_cols
    }
    pub(crate) fn new_recycled(cols: usize, retained_cols: u16) -> Self {
        let mut row = Self::new(cols);
        row.retained_cols = retained_cols.max(cols as u16);
        row.dirty_end = cols as u16;
        row
    }
    /// A constant-time upper bound on the archive size, including the bounded
    /// 64-scalar grapheme payloads. This also bounds compression-step work.
    pub(crate) fn history_size_bound(&self) -> usize {
        11 + self.cells.len() * 8 + self.styles.len() * 14 + self.graphemes.len() * 260
    }
    pub(crate) fn encode_history(&self, bytes: &mut Vec<u8>) {
        bytes.extend_from_slice(&(self.cells.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&self.retained_cols.to_le_bytes());
        bytes.push(u8::from(self.wrapped) | (u8::from(self.wrap_continuation) << 1));
        bytes.extend_from_slice(&(self.styles.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&self.last_style.to_le_bytes());
        bytes.extend_from_slice(&(self.graphemes.len() as u16).to_le_bytes());
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
        for (col, text) in self.graphemes.iter() {
            bytes.extend_from_slice(&(col as u16).to_le_bytes());
            bytes.extend_from_slice(&(text.len() as u16).to_le_bytes());
            bytes.extend_from_slice(text.as_bytes());
        }
    }
    pub(crate) fn decode_history(bytes: &mut &[u8]) -> Option<Self> {
        let mut r = Reader(bytes);
        let cols = usize::from(r.u16()?);
        let retained_cols = r.u16()?;
        let flags = r.u8()?;
        let styles = usize::from(r.u16()?);
        let last_style = r.u16()?;
        let graphemes = usize::from(r.u16()?);
        if !(1..=MAX_COLS).contains(&cols)
            || usize::from(retained_cols) < cols
            || usize::from(retained_cols) > MAX_COLS
            || flags > 3
            || styles == 0
            || styles > MAX_COLS * 2 + 1
            || usize::from(last_style) >= styles
            || graphemes > cols
        {
            return None;
        }
        // Validate the minimum complete payload before allocating any vectors.
        if r.0.len() < styles * 14 + cols * 8 + graphemes * 4 {
            return None;
        }
        let mut row = Row::new(cols);
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
            if col >= cols
                || previous.is_some_and(|v| col <= v)
                || len == 0
                || len > 256
                || row.cells[col].width == 0
            {
                return None;
            }
            let text = std::str::from_utf8(r.take(len)?).ok()?;
            if text.chars().count() > 64 || !text.starts_with(row.cells[col].character()) {
                return None;
            }
            row.graphemes.insert(col, CompactString::new(text));
            previous = Some(col);
        }
        *bytes = r.0;
        Some(row)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        row.encode_history(&mut bytes);
        assert!(bytes.len() <= row.history_size_bound());
        let mut input = bytes.as_slice();
        let decoded = Row::decode_history(&mut input).unwrap();
        assert!(input.is_empty());
        let mut restored = Vec::new();
        decoded.encode_history(&mut restored);
        assert_eq!(restored, bytes);
        for end in 0..bytes.len() {
            assert!(
                Row::decode_history(&mut &bytes[..end]).is_none(),
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
        row.encode_history(&mut source);
        let mut seed = 0x4f37a290u32;
        for _ in 0..10000 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let mut bytes = source.clone();
            let at = seed as usize % bytes.len();
            bytes[at] = (seed >> 16) as u8;
            let _ = Row::decode_history(&mut bytes.as_slice());
        }
        let mut large = source.clone();
        large[0..2].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(Row::decode_history(&mut large.as_slice()).is_none());
        large = source.clone();
        large[5..7].copy_from_slice(&u16::MAX.to_le_bytes());
        assert!(Row::decode_history(&mut large.as_slice()).is_none());
    }
}
