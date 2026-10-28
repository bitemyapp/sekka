//! Experimental uniform cell owner. Raw ownership is private to this module.
//! All cached pointers are backed by an owned global Vec allocation or an
//! immutable owned page. No pointer survives a reallocating Vec mutation.
use crate::Cell;
#[cfg(not(miri))]
use memmap2::{Mmap, MmapMut};
use std::io;
use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut, Range};
use std::ptr::NonNull;
use triomphe::Arc;

const MAX_CELLS: usize = 64 * crate::MAX_COLS;

// Miri exercises the exact owner, raw-slice, COW, resize and drop code below;
// only the OS mapping allocation is replaced by immutable heap backing.
#[cfg(not(miri))]
type PageBacking = Mmap;
#[cfg(miri)]
type PageBacking = Box<[Cell]>;

#[derive(Debug)]
pub(crate) struct CellPage(PageBacking);
impl CellPage {
    pub fn pack<'a>(rows: impl Clone + Iterator<Item = &'a [Cell]>) -> io::Result<Arc<Self>> {
        let count = rows
            .clone()
            .try_fold(0usize, |sum, row| sum.checked_add(row.len()))
            .filter(|&count| count > 0 && count <= MAX_CELLS)
            .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
        #[cfg(not(miri))]
        {
            let bytes = count
                .checked_mul(size_of::<Cell>())
                .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
            let mut mapping = MmapMut::map_anon(bytes)?;
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
        #[cfg(miri)]
        {
            let mut cells = Vec::with_capacity(count);
            for row in rows {
                cells.extend_from_slice(row);
            }
            assert_eq!(cells.len(), count);
            Ok(Arc::new(Self(cells.into_boxed_slice())))
        }
    }
    fn cells(&self) -> &[Cell] {
        #[cfg(not(miri))]
        {
            bytemuck::try_cast_slice(&self.0).expect("validated aligned cell mapping")
        }
        #[cfg(miri)]
        {
            &self.0
        }
    }
}

/// None owns a global Vec<Cell> allocation with these exact raw parts. Some
/// owns immutable mapped cells; capacity then records logical COW capacity only.
/// Fields must stay private. In particular, never derive Clone for this owner.
#[derive(Debug)]
pub(crate) struct CellBuffer {
    ptr: NonNull<Cell>,
    len: u16,
    capacity: u16,
    owner: Option<Arc<CellPage>>,
}
const _: () = assert!(size_of::<CellBuffer>() == size_of::<Vec<Cell>>());

// SAFETY: Cell is Copy/Pod, Send and Sync. An unshared Vec allocation is owned
// uniquely; a mapped allocation is immutable and retained through an atomic
// Arc. Pointer updates require &mut Self, and mapped mutation always copies.
// No reference to a mutable Vec or mutable mapping escapes this module.
unsafe impl Send for CellBuffer {}
// SAFETY: Shared access only returns immutable slices tied to &self. There is
// no interior mutability; a borrowed owner cannot be resized, dropped or moved.
unsafe impl Sync for CellBuffer {}

impl CellBuffer {
    fn empty() -> Self {
        Self {
            ptr: NonNull::dangling(),
            len: 0,
            capacity: 0,
            owner: None,
        }
    }
    fn try_from_vec(cells: Vec<Cell>) -> Result<Self, Vec<Cell>> {
        // Validate ACTUAL vector length/capacity before suppressing its Drop.
        // Codec capacity hints and amortized growth use this same constructor.
        let (Ok(len), Ok(capacity)) = (u16::try_from(cells.len()), u16::try_from(cells.capacity()))
        else {
            return Err(cells);
        };
        if cells.len() > crate::MAX_COLS || cells.capacity() > crate::MAX_COLS * 2 {
            return Err(cells);
        }
        let mut cells = ManuallyDrop::new(cells);
        let ptr = NonNull::new(cells.as_mut_ptr()).expect("Vec pointers are nonnull");
        Ok(Self {
            ptr,
            len,
            capacity,
            owner: None,
        })
    }
    pub fn frozen(page: Arc<CellPage>, range: Range<usize>, capacity: usize) -> Option<Self> {
        let len = range.end.checked_sub(range.start)?;
        if len == 0 || len > crate::MAX_COLS || capacity < len || capacity > crate::MAX_COLS * 2 {
            return None;
        }
        let len = u16::try_from(len).ok()?;
        let capacity = u16::try_from(capacity).ok()?;
        let cells = page.cells().get(range)?;
        // This pointer comes from a checked, aligned sub-slice. The cast only
        // changes pointer mutability; no mapped pointer is ever written through.
        let ptr = NonNull::new(cells.as_ptr().cast_mut())?;
        Some(Self {
            ptr,
            len,
            capacity,
            owner: Some(page),
        })
    }
    #[inline]
    pub fn capacity(&self) -> usize {
        usize::from(self.capacity)
    }
    fn into_vec(self) -> Vec<Cell> {
        if self.owner.is_some() {
            let mut cells = Vec::with_capacity(self.capacity());
            cells.extend_from_slice(&self);
            // self (and its page owner) remains alive until copying finishes.
            return cells;
        }
        let this = ManuallyDrop::new(self);
        // SAFETY: None identifies the sole owner of the exact original global
        // Vec<Cell> allocation. No live slice borrows self (it was consumed).
        // Its initialized prefix and allocation layout are unchanged. Empty
        // owners have an aligned dangling pointer, len=cap=0 as Vec requires.
        unsafe { Vec::from_raw_parts(this.ptr.as_ptr(), usize::from(this.len), this.capacity()) }
    }
    #[cold]
    #[inline(never)]
    fn materialize(&mut self) {
        let mut cells = Vec::with_capacity(self.capacity());
        cells.extend_from_slice(self);
        let replacement = Self::from(cells);
        // The old page remains alive throughout the copy and validation.
        *self = replacement;
    }
    fn mutate_vec(&mut self, mutate: impl FnOnce(&mut Vec<Cell>)) {
        let old = std::mem::replace(self, Self::empty());
        let mut cells = old.into_vec();
        // A panic here drops this ordinary local Vec exactly once, leaving a
        // valid empty self. No cached pointer is used across any reallocation.
        mutate(&mut cells);
        *self = Self::from(cells);
    }
    #[inline]
    pub fn resize(&mut self, count: usize, cell: Cell) {
        self.mutate_vec(|cells| {
            assert!(
                count <= crate::MAX_COLS,
                "cell width exceeds terminal bounds"
            );
            cells.resize(count, cell);
        });
    }
    pub fn is_frozen(&self) -> bool {
        self.owner.is_some()
    }
    pub fn retained_extra(&self) -> usize {
        self.owner.as_ref().map_or(0, |page| {
            (page.cells().len() - usize::from(self.len)) * size_of::<Cell>()
        })
    }
}
impl Drop for CellBuffer {
    fn drop(&mut self) {
        if self.owner.is_none() {
            // SAFETY: See into_vec. This destructor owns the exact original
            // allocation, and runs once. A mapped interior pointer never enters
            // this branch. owner=None itself has no additional drop work.
            drop(unsafe {
                Vec::from_raw_parts(self.ptr.as_ptr(), usize::from(self.len), self.capacity())
            });
        }
        // For mapped storage the owner field's normal Drop releases the Arc.
    }
}
impl Clone for CellBuffer {
    fn clone(&self) -> Self {
        if let Some(page) = &self.owner {
            return Self {
                ptr: self.ptr,
                len: self.len,
                capacity: self.capacity,
                owner: Some(page.clone()),
            };
        }
        let mut cells = Vec::with_capacity(self.capacity());
        cells.extend_from_slice(self);
        Self::from(cells)
    }
}
impl From<Vec<Cell>> for CellBuffer {
    fn from(cells: Vec<Cell>) -> Self {
        Self::try_from_vec(cells).unwrap_or_else(|_| panic!("cell vector exceeds checked bounds"))
    }
}
impl Deref for CellBuffer {
    type Target = [Cell];
    #[inline]
    fn deref(&self) -> &Self::Target {
        // SAFETY: The owner invariants guarantee aligned initialized cells in
        // one allocation for len elements. self retains that allocation, and
        // this shared slice's lifetime prevents simultaneous owner mutation.
        unsafe { std::slice::from_raw_parts(self.ptr.as_ptr(), usize::from(self.len)) }
    }
}
impl DerefMut for CellBuffer {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        if self.owner.is_some() {
            self.materialize();
        }
        // SAFETY: materialize ensures unique Vec ownership. &mut Self excludes
        // other live slices into this allocation; mapped siblings were copied.
        unsafe { std::slice::from_raw_parts_mut(self.ptr.as_ptr(), usize::from(self.len)) }
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
        assert!(CellBuffer::frozen(page.clone(), Range { start: 120, end: 0 }, 120).is_none());
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
    #[cfg_attr(
        miri,
        ignore = "large OS-mapping boundary test; owner tests use the same code on small heap pages"
    )]
    fn mapping_bounds_and_cell_layout() {
        assert_eq!(size_of::<Cell>(), 8);
        assert_eq!(align_of::<Cell>(), align_of::<u32>());
        assert!(CellPage::pack(std::iter::empty()).is_err());
        let row = vec![Cell::default(); crate::MAX_COLS];
        assert!(CellPage::pack(std::iter::repeat_n(row.as_slice(), 65)).is_err());
        let page = CellPage::pack(std::iter::repeat_n(row.as_slice(), 64)).unwrap();
        assert_eq!(page.cells().len(), MAX_CELLS);
    }
    #[test]
    fn actual_vector_bounds_are_checked_before_ownership_transfer() {
        let too_long = vec![Cell::default(); crate::MAX_COLS + 1];
        assert!(CellBuffer::try_from_vec(too_long).is_err());
        let too_large = Vec::with_capacity(crate::MAX_COLS * 2 + 1);
        assert!(CellBuffer::try_from_vec(too_large).is_err());
        let truncated = Vec::with_capacity(usize::from(u16::MAX) + 1);
        assert!(CellBuffer::try_from_vec(truncated).is_err());
        let mut empty = CellBuffer::from(Vec::new());
        assert_eq!(empty.len(), 0);
        assert_eq!(empty.capacity(), 0);
        empty.resize(3, Cell::new('e', 0, 1));
        assert!(empty.iter().all(|cell| cell.character() == 'e'));
    }
    #[test]
    fn resize_unwind_drops_local_vector_and_leaves_valid_sentinel() {
        let mut cells = CellBuffer::from(vec![Cell::new('a', 0, 1); 4]);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cells.mutate_vec(|vec| {
                vec.reserve(500);
                vec.resize(30, Cell::default());
                panic!("injected after growth");
            });
        }));
        assert!(result.is_err());
        assert!(cells.is_empty());
        assert_eq!(cells.capacity(), 0);
        cells.resize(7, Cell::new('b', 0, 1));
        assert_eq!(cells[6].character(), 'b');
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cells.resize(crate::MAX_COLS + 1, Cell::default())
        }));
        assert!(result.is_err());
        assert!(cells.is_empty());
        cells.resize(2, Cell::default());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            cells.mutate_vec(|vec| vec.reserve_exact(crate::MAX_COLS * 2 + 1));
        }));
        assert!(result.is_err());
        assert!(cells.is_empty());
        assert_eq!(cells.capacity(), 0);
    }
    #[test]
    fn repeated_growth_shrink_swap_clone_and_thread_drop_match_vectors() {
        let mut cells = CellBuffer::from(Vec::new());
        let mut expected = Vec::new();
        for round in 0..128usize {
            let len = (round * 37) % 201;
            let cell = Cell::new(char::from(b'a' + (round % 26) as u8), 0, 1);
            cells.resize(len, cell);
            expected.resize(len, cell);
            assert_eq!(&*cells, expected.as_slice());
            let mut copied = cells.clone();
            if !copied.is_empty() {
                copied[0] = Cell::new('Z', 0, 1);
            }
            assert_eq!(&*cells, expected.as_slice());
            std::mem::swap(&mut copied, &mut cells);
            std::mem::swap(&mut copied, &mut cells);
            assert_eq!(&*cells, expected.as_slice());
        }
        std::thread::spawn(move || drop(cells)).join().unwrap();
    }
    #[test]
    fn mapped_resize_and_unwind_keep_sibling_page_alive() {
        let data = vec![Cell::new('p', 0, 1); 16];
        let page = CellPage::pack([data.as_slice()].into_iter()).unwrap();
        let mut first = CellBuffer::frozen(page.clone(), 0..8, 32).unwrap();
        let second = CellBuffer::frozen(page, 8..16, 32).unwrap();
        first.resize(20, Cell::new('n', 0, 1));
        assert_eq!(first[0].character(), 'p');
        assert_eq!(first[19].character(), 'n');
        let mut third = second.clone();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            third.mutate_vec(|_| panic!("mapped ownership transferred"))
        }));
        assert!(result.is_err());
        assert!(third.is_empty());
        assert_eq!(second[7].character(), 'p');
        drop(first);
        drop(third);
        assert_eq!(second[0].character(), 'p');
    }
}
