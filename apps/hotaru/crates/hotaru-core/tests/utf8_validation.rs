#[test]
fn compat_validation_preserves_standard_error_boundaries() {
    fn compare(input: &[u8]) {
        let standard = std::str::from_utf8(input).map_err(|e| (e.valid_up_to(), e.error_len()));
        let simd = simdutf8::compat::from_utf8(input).map_err(|e| (e.valid_up_to(), e.error_len()));
        assert_eq!(
            simd,
            standard,
            "input prefix: {:?}",
            &input[..input.len().min(32)]
        );
    }
    compare(&[]);
    for first in 0..=255u8 {
        compare(&[first]);
        for second in 0..=255u8 {
            compare(&[first, second]);
            for third in 0..=255u8 {
                compare(&[first, second, third]);
            }
        }
    }
    let valid = "ASCII é 雪 👩🏽‍💻 e\u{301}".as_bytes();
    let mut seed = 0x94c3_b16du32;
    for case in 0..2048 {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        let len = if case % 8 == 0 {
            65536
        } else {
            64 + seed as usize % 4096
        };
        let offset = case % 32;
        let mut bytes = vec![0; offset + len];
        for (at, value) in bytes[offset..].iter_mut().enumerate() {
            *value = valid[at % valid.len()];
        }
        // Every prefix cut exercises an incomplete sequence at some boundary;
        // sparse and dense mutations also check early failure in long runs.
        compare(&bytes[offset..]);
        for mutation in 0..16 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let at = offset + seed as usize % len;
            bytes[at] = if mutation % 2 == 0 {
                0xff
            } else {
                (seed >> 24) as u8
            };
            compare(&bytes[offset..]);
        }
    }
}

#[test]
fn simd_error_offsets_match_across_vector_boundaries() {
    let mut data = [b'x'; 256];
    // Force a non-ASCII first block, then place every two-byte value around
    // 32/64-byte boundaries in later blocks. The short exhaustive test above
    // also covers the public API's intentionally scalar <64-byte fallback.
    data[..2].copy_from_slice("é".as_bytes());
    for at in [30, 31, 32, 62, 63, 64, 126, 127, 128, 254] {
        for first in 0..=255u8 {
            for second in 0..=255u8 {
                data[at] = first;
                data[at + 1] = second;
                let standard =
                    std::str::from_utf8(&data).map_err(|e| (e.valid_up_to(), e.error_len()));
                let simd = simdutf8::compat::from_utf8(&data)
                    .map_err(|e| (e.valid_up_to(), e.error_len()));
                assert_eq!(simd, standard, "at={at}, bytes={first:02x}/{second:02x}");
            }
        }
        data[at..at + 2].fill(b'x');
    }
}

#[cfg(target_os = "linux")]
#[test]
fn validator_loads_stay_before_guard_page() {
    // SAFETY: sysconf accepts no pointers; a positive page size is required.
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    assert!(page > 512);
    let page = page as usize;
    // SAFETY: create two private anonymous pages and check MAP_FAILED.
    let raw = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            page * 2,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    assert_ne!(raw, libc::MAP_FAILED);
    struct Mapping(*mut libc::c_void, usize);
    impl Drop for Mapping {
        fn drop(&mut self) {
            // SAFETY: this guard owns precisely the original mapping.
            assert_eq!(unsafe { libc::munmap(self.0, self.1) }, 0);
        }
    }
    let _mapping = Mapping(raw, page * 2);
    // SAFETY: the second page is a page-aligned subrange of the owned mapping.
    assert_eq!(
        unsafe { libc::mprotect(raw.cast::<u8>().add(page).cast(), page, libc::PROT_NONE) },
        0
    );
    // SAFETY: the first page is live, exclusive, writable and initialized below.
    let memory = unsafe { std::slice::from_raw_parts_mut(raw.cast::<u8>(), page) };
    memory.fill(0);
    let pattern = "é雪👩".as_bytes();
    for len in 0..=512 {
        let bytes = &mut memory[page - len..];
        for (at, byte) in bytes.iter_mut().enumerate() {
            *byte = pattern[at % pattern.len()];
        }
        let compare = |bytes: &[u8]| {
            let standard = std::str::from_utf8(bytes).map_err(|e| (e.valid_up_to(), e.error_len()));
            let simd =
                simdutf8::compat::from_utf8(bytes).map_err(|e| (e.valid_up_to(), e.error_len()));
            assert_eq!(simd, standard, "guarded length={len}");
        };
        compare(bytes);
        if let Some(last) = bytes.last_mut() {
            *last = 0xff;
            compare(bytes);
        }
    }
}
