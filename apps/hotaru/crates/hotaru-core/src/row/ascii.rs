//! Whole-cell stores for the existing ASCII run path. No row metadata changes.
use super::Cell;

// The native-endian template below describes exactly these initialized fields.
// No padding, niche, reference, enum discriminant or destructor is involved.
const _: () = {
    assert!(std::mem::size_of::<Cell>() == 8);
    assert!(!std::mem::needs_drop::<Cell>());
    assert!(std::mem::offset_of!(Cell, scalar) == 0);
    assert!(std::mem::offset_of!(Cell, style) == 4);
    assert!(std::mem::offset_of!(Cell, width) == 6);
    assert!(std::mem::offset_of!(Cell, _reserved) == 7);
};
const _: fn(&Cell) = |cell| {
    let _: &u32 = &cell.scalar;
    let _: &u16 = &cell.style;
    let _: &u8 = &cell.width;
    let _: &u8 = &cell._reserved;
};

impl Cell {
    #[inline]
    #[allow(unsafe_code)] // One exact, possibly unaligned, eight-byte Cell store.
    pub(crate) fn write_ascii(cells: &mut [Self], bytes: &[u8], style: u16, protected: bool) {
        assert_eq!(cells.len(), bytes.len());
        let style = style.to_ne_bytes();
        let template = u64::from_ne_bytes([0, 0, 0, 0, style[0], style[1], 1, u8::from(protected)]);
        for (cell, &byte) in cells.iter_mut().zip(bytes) {
            let scalar = if cfg!(target_endian = "little") {
                u64::from(byte)
            } else {
                u64::from(byte) << 32
            };
            // SAFETY: Cell is repr(C) and the layout assertions prove that it
            // occupies exactly these eight initialized bytes. The template
            // encodes the same native-endian fields as Cell::new(byte as char,
            // style, 1).protected(protected); every u8 is a valid Unicode scalar.
            // Each pointer is derived from its exclusive, live Cell reference,
            // and the store touches only that Cell. No u64 reference is created:
            // Cell requires only four-byte alignment, so use write_unaligned.
            unsafe {
                (cell as *mut Self)
                    .cast::<u64>()
                    .write_unaligned(template | scalar);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_cell_store_matches_constructor_for_all_styles_and_bytes() {
        let styles: Box<dyn Iterator<Item = u16>> = if cfg!(miri) {
            Box::new([0, 1, 0xff, 0x100, 0x8000, u16::MAX].into_iter())
        } else {
            Box::new(0..=u16::MAX)
        };
        let bytes = [0, b' ', 0x7f, 0xff];
        let mut cells = [Cell::default(); 4];
        for style in styles {
            for protected in [false, true] {
                Cell::write_ascii(&mut cells, &bytes, style, protected);
                for (cell, &byte) in cells.iter().zip(&bytes) {
                    assert_eq!(
                        *cell,
                        Cell::new(byte as char, style, 1).protected(protected)
                    );
                }
            }
        }
        let bytes: Vec<u8> = (0..=u8::MAX).collect();
        let mut cells = vec![Cell::default(); bytes.len()];
        Cell::write_ascii(&mut cells, &bytes, 0xa57e, true);
        for (cell, &byte) in cells.iter().zip(&bytes) {
            assert_eq!(*cell, Cell::new(byte as char, 0xa57e, 1).protected(true));
        }
    }

    #[test]
    fn four_byte_aligned_cells_offsets_and_tails_keep_neighbors() {
        #[repr(C, align(8))]
        struct Storage {
            prefix: u32,
            cells: [Cell; 72],
        }
        let sentinel = Cell::new('雪', 0xbeef, 2).protected(true);
        let lengths: Vec<usize> = if cfg!(miri) {
            vec![0, 1, 2, 3, 4, 7, 8, 15, 16, 31, 32, 63, 64]
        } else {
            (0..=64).collect()
        };
        let mut storage = Storage {
            prefix: 0xabcdef12,
            cells: [sentinel; 72],
        };
        assert_eq!(storage.cells.as_ptr().addr() % 8, 4);
        for offset in 0..=7 {
            for &len in &lengths {
                storage.cells.fill(sentinel);
                let bytes: Vec<u8> = (0..len).map(|i| (i as u8).wrapping_mul(13)).collect();
                Cell::write_ascii(
                    &mut storage.cells[offset..offset + len],
                    &bytes,
                    0x1234,
                    false,
                );
                assert_eq!(storage.prefix, 0xabcdef12);
                assert!(storage.cells[..offset].iter().all(|cell| *cell == sentinel));
                assert!(
                    storage.cells[offset + len..]
                        .iter()
                        .all(|cell| *cell == sentinel)
                );
                for (cell, &byte) in storage.cells[offset..offset + len].iter().zip(&bytes) {
                    assert_eq!(*cell, Cell::new(byte as char, 0x1234, 1));
                }
            }
        }
    }

    #[test]
    fn mismatched_lengths_fail_before_any_store() {
        let mut cells = [Cell::new('x', 0x1234, 2); 2];
        let old = cells;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            Cell::write_ascii(&mut cells, b"a", 0, false);
        }));
        assert!(result.is_err());
        assert_eq!(cells, old);
    }
}
