// SPDX-License-Identifier: GPL-3.0-or-later
//! The helper's allocator: every block is zeroed before it's freed, and a
//! block that grows moves to a new one rather than leaving its old bytes
//! behind. The password passes through buffers the helper doesn't own,
//! serde_json's among them (a string with an escape, `"` or `\`, is
//! unescaped into its scratch buffer, which grows and is freed as any
//! other); with this, none of them leaves a copy in freed memory. The
//! helper allocates little, and seldom.

use std::alloc::{GlobalAlloc, Layout, System};
use std::mem::MaybeUninit;

use zeroize::Zeroize;

/// `A`, zeroing what it frees.
pub struct Zeroing<A = System>(pub A);

// Safety: every block comes from `A` and goes back to it with the layout
// it was made with; the only additions are writes of zeros to a block
// still owned, and a copy between two owned blocks.
unsafe impl<A: GlobalAlloc> GlobalAlloc for Zeroing<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { self.0.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        unsafe { self.0.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe {
            // Volatile writes, which freeing it can't optimise away.
            std::slice::from_raw_parts_mut(ptr.cast::<MaybeUninit<u8>>(), layout.size()).zeroize();
            self.0.dealloc(ptr, layout)
        }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe {
            // Never in place: the old block is zeroed as it's freed.
            let new = Layout::from_size_align_unchecked(new_size, layout.align());
            let moved = self.0.alloc(new);
            if !moved.is_null() {
                std::ptr::copy_nonoverlapping(ptr, moved, layout.size().min(new_size));
                self.dealloc(ptr, layout);
            }
            moved
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// The system's allocator, counting the blocks it's given back and
    /// those of them with anything but zeros in.
    struct Counting {
        freed: AtomicUsize,
        dirty: AtomicUsize,
    }

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            unsafe { System.alloc(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe {
                // Zeroed by Zeroing, so every byte is initialised.
                let bytes = std::slice::from_raw_parts(ptr, layout.size());
                self.freed.fetch_add(1, Ordering::SeqCst);
                if bytes.iter().any(|b| *b != 0) {
                    self.dirty.fetch_add(1, Ordering::SeqCst);
                }
                System.dealloc(ptr, layout)
            }
        }
    }

    #[test]
    fn freed_and_outgrown_blocks_hold_nothing() {
        let z = Zeroing(Counting { freed: AtomicUsize::new(0), dirty: AtomicUsize::new(0) });
        let secret = b"correct \"horse\" \\battery";
        unsafe {
            let layout = Layout::from_size_align(secret.len(), 1).unwrap();
            let p = z.alloc(layout);
            assert!(!p.is_null());
            std::ptr::copy_nonoverlapping(secret.as_ptr(), p, secret.len());
            // Grown, as a Vec grows: the contents move, the old block is freed.
            let q = z.realloc(p, layout, 4 * secret.len());
            assert!(!q.is_null());
            assert_eq!(std::slice::from_raw_parts(q, secret.len()), secret);
            z.dealloc(q, Layout::from_size_align(4 * secret.len(), 1).unwrap());
        }
        assert_eq!(z.0.freed.load(Ordering::SeqCst), 2, "the outgrown block and the last");
        assert_eq!(z.0.dirty.load(Ordering::SeqCst), 0);
    }
}
