//! Experimental immutable mapped cells. Only a private builder has write
//! access; published ranges retain the mapping and materialize on mutation.
use crate::Cell;
use memmap2::{Mmap, MmapMut};
use std::io;
use std::ops::{Deref, DerefMut, Range};
use triomphe::Arc;

const MAX_CELLS: usize = 64 * crate::MAX_COLS;

#[derive(Debug)]
pub(crate) struct CellPage(Mmap);
impl CellPage {
    pub fn pack<'a>(rows: impl Clone + Iterator<Item = &'a [Cell]>) -> io::Result<Arc<Self>> {
        let count = rows
            .clone()
            .try_fold(0usize, |sum, row| sum.checked_add(row.len()))
            .filter(|&count| count > 0 && count <= MAX_CELLS)
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let bytes = count
            .checked_mul(size_of::<Cell>())
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        let mut mapping = MmapMut::map_anon(bytes)?;
        // The cast checks both alignment and whole-element length. Cell's Pod
        // derive proves no invalid bit patterns or padding; all fields are ints.
        let target = bytemuck::try_cast_slice_mut::<u8, Cell>(&mut mapping)
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        let mut start = 0;
        for row in rows {
            target[start..start + row.len()].copy_from_slice(row);
            start += row.len();
        }
        debug_assert_eq!(start, count);
        Ok(Arc::new(Self(mapping.make_read_only()?)))
    }
    fn cells(&self) -> &[Cell] {
        bytemuck::try_cast_slice(&self.0).expect("validated aligned cell mapping")
    }
}

#[derive(Debug)]
pub(crate) enum CellBuffer {
    Owned(Vec<Cell>),
    Frozen {
        page: Arc<CellPage>,
        start: u32,
        len: u16,
        capacity: u16,
    },
}
impl CellBuffer {
    pub fn frozen(page: Arc<CellPage>, range: Range<usize>, capacity: usize) -> Option<Self> {
        let len = range.end.checked_sub(range.start)?;
        if range.end > page.cells().len()
            || len == 0
            || len > crate::MAX_COLS
            || capacity < len
            || capacity > crate::MAX_COLS * 2
        {
            return None;
        }
        Some(Self::Frozen {
            page,
            start: range.start.try_into().ok()?,
            len: len.try_into().ok()?,
            capacity: capacity.try_into().ok()?,
        })
    }
    pub fn capacity(&self) -> usize {
        match self {
            Self::Owned(cells) => cells.capacity(),
            Self::Frozen { capacity, .. } => usize::from(*capacity),
        }
    }
    fn owned(&mut self) -> &mut Vec<Cell> {
        if matches!(self, Self::Frozen { .. }) {
            let mut cells = Vec::with_capacity(self.capacity());
            cells.extend_from_slice(self);
            *self = Self::Owned(cells);
        }
        let Self::Owned(cells) = self else {
            unreachable!()
        };
        cells
    }
    pub fn resize(&mut self, count: usize, cell: Cell) {
        self.owned().resize(count, cell);
    }
    pub fn is_frozen(&self) -> bool {
        matches!(self, Self::Frozen { .. })
    }
}
impl Clone for CellBuffer {
    fn clone(&self) -> Self {
        match self {
            Self::Owned(cells) => Self::Owned(crate::memory::clone_vec(cells)),
            Self::Frozen {
                page,
                start,
                len,
                capacity,
            } => Self::Frozen {
                page: page.clone(),
                start: *start,
                len: *len,
                capacity: *capacity,
            },
        }
    }
}
impl From<Vec<Cell>> for CellBuffer {
    fn from(cells: Vec<Cell>) -> Self {
        Self::Owned(cells)
    }
}
impl Deref for CellBuffer {
    type Target = [Cell];
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Owned(cells) => cells,
            Self::Frozen {
                page, start, len, ..
            } => &page.cells()[*start as usize..*start as usize + usize::from(*len)],
        }
    }
}
impl DerefMut for CellBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.owned()
    }
}
impl<'a> IntoIterator for &'a CellBuffer {
    type Item = &'a Cell;
    type IntoIter = std::slice::Iter<'a, Cell>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<'a> IntoIterator for &'a mut CellBuffer {
    type Item = &'a mut Cell;
    type IntoIter = std::slice::IterMut<'a, Cell>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}
impl PartialEq for CellBuffer {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}
impl Eq for CellBuffer {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_ranges_retain_mapping_and_materialize_without_aliasing() {
        let row = vec![Cell::new('x', 0, 1); 120];
        let page = CellPage::pack([row.as_slice(), row.as_slice()].into_iter()).unwrap();
        assert!(CellBuffer::frozen(page.clone(), 200..241, 120).is_none());
        assert!(CellBuffer::frozen(page.clone(), 0..120, 119).is_none());
        let reversed = Range { start: 120, end: 0 };
        assert!(CellBuffer::frozen(page.clone(), reversed, 120).is_none());
        let first = CellBuffer::frozen(page.clone(), 0..120, 256).unwrap();
        let second = CellBuffer::frozen(page, 120..240, 120).unwrap();
        let mut copy = first.clone();
        copy[0] = Cell::new('y', 0, 1);
        assert!(!copy.is_frozen());
        assert_eq!(copy.capacity(), 256);
        assert_eq!(first[0].character(), 'x');
        assert_eq!(second[0].character(), 'x');
        drop(first);
        std::thread::spawn(move || assert_eq!(second[119].character(), 'x'))
            .join()
            .unwrap();
    }
    #[test]
    fn mapping_bounds_and_cell_layout() {
        assert_eq!(size_of::<Cell>(), 8);
        assert_eq!(align_of::<Cell>(), align_of::<u32>());
        assert!(CellPage::pack(std::iter::empty()).is_err());
        let row = vec![Cell::default(); crate::MAX_COLS];
        assert!(CellPage::pack(std::iter::repeat_n(row.as_slice(), 65)).is_err());
        let page = CellPage::pack(std::iter::repeat_n(row.as_slice(), 64)).unwrap();
        assert_eq!(page.cells().len(), MAX_CELLS);
    }
}
