//! Cache prefetch hints for memory the terminal is about to touch.
//!
//! A prefetch only starts a load into the cache. It never faults, its result
//! never reaches the program, and any address is sound, including dangling,
//! null or unmapped ones, so callers may pass addresses of storage they hold
//! no reference to. Pointers are only offset with wrapping arithmetic, never
//! dereferenced. x86-64 guarantees SSE, which provides PREFETCHT0; other
//! targets and Miri compile the hints away.

const LINE: usize = 64;

#[inline(always)]
pub(crate) fn line<T>(address: *const T) {
    #[cfg(all(target_arch = "x86_64", not(miri)))]
    // SAFETY: SSE is part of the x86-64 baseline. PREFETCHT0 is a hint that
    // dereferences nothing the program can observe and ignores bad addresses.
    unsafe {
        core::arch::x86_64::_mm_prefetch::<{ core::arch::x86_64::_MM_HINT_T0 }>(address.cast())
    }
    #[cfg(any(not(target_arch = "x86_64"), miri))]
    let _ = address;
}

/// Every cache line overlapping `bytes` bytes from `start`.
#[inline]
pub(crate) fn range<T>(start: *const T, bytes: usize) {
    let start = start.cast::<u8>();
    let offset = start.addr() % LINE;
    let first = start.wrapping_sub(offset);
    for index in 0..offset.saturating_add(bytes.max(1)).div_ceil(LINE) {
        line(first.wrapping_add(index * LINE));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hints_accept_any_address_and_length() {
        let cells = vec![0u64; 100];
        super::range(cells.as_ptr(), 800);
        super::range(cells.as_ptr().wrapping_add(3), 0);
        super::range(std::ptr::null::<u8>(), 64);
        super::range(std::ptr::dangling::<u64>(), 4096);
        super::range(std::ptr::without_provenance::<u8>(usize::MAX - 70), 200);
        super::line(std::ptr::without_provenance::<u8>(usize::MAX));
        assert_eq!(cells.iter().sum::<u64>(), 0);
    }
}
