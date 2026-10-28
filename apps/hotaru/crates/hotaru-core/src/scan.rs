//! Bounded byte classification for the parser's printable fast paths.
//!
//! x86-64 guarantees SSE2. Every load is preceded by a complete 16-byte range
//! check; the tail uses ordinary slice iteration. No alignment, padding, or
//! readable byte beyond the caller's slice is assumed. Other targets use the
//! scalar implementation. The scanner returns only an index, never a pointer.

#[inline]
pub(crate) fn printable_ascii(bytes: &[u8]) -> usize {
    #[cfg(all(target_arch = "x86_64", not(miri)))]
    {
        // SAFETY: SSE2 is mandatory on x86-64; scan checks each load's bounds.
        unsafe { scan_sse2::<false>(bytes) }
    }
    #[cfg(any(not(target_arch = "x86_64"), miri))]
    scalar::<false>(bytes)
}

#[inline]
pub(crate) fn printable_utf8_bytes(bytes: &[u8]) -> usize {
    #[cfg(all(target_arch = "x86_64", not(miri)))]
    {
        // SAFETY: SSE2 is mandatory on x86-64; scan checks each load's bounds.
        unsafe { scan_sse2::<true>(bytes) }
    }
    #[cfg(any(not(target_arch = "x86_64"), miri))]
    scalar::<true>(bytes)
}

#[inline]
fn scalar<const UTF8: bool>(bytes: &[u8]) -> usize {
    bytes
        .iter()
        .position(|&b| {
            if UTF8 {
                b < 0x20 || b == 0x7f
            } else {
                !(0x20..=0x7e).contains(&b)
            }
        })
        .unwrap_or(bytes.len())
}

#[cfg(all(target_arch = "x86_64", not(miri)))]
#[target_feature(enable = "sse2")]
unsafe fn scan_sse2<const UTF8: bool>(bytes: &[u8]) -> usize {
    use std::arch::x86_64::*;
    let mut at = 0;
    let low = _mm_set1_epi8(0x20);
    let high = _mm_set1_epi8(0x7e);
    let del = _mm_set1_epi8(0x7f);
    let negative_one = _mm_set1_epi8(-1);
    while bytes.len() - at >= 16 {
        // SAFETY: at <= len and at+16 <= len, so exactly these sixteen bytes
        // belong to the live slice. loadu accepts the slice's byte alignment.
        let value = unsafe { _mm_loadu_si128(bytes.as_ptr().add(at).cast()) };
        let invalid = if UTF8 {
            // Signed negative bytes (UTF-8 leads/continuations) are printable
            // candidates here; the standard library validates UTF-8 afterward.
            let c0 = _mm_and_si128(
                _mm_cmplt_epi8(value, low),
                _mm_cmpgt_epi8(value, negative_one),
            );
            _mm_or_si128(c0, _mm_cmpeq_epi8(value, del))
        } else {
            // High-bit bytes are signed negative and therefore below 0x20.
            _mm_or_si128(_mm_cmplt_epi8(value, low), _mm_cmpgt_epi8(value, high))
        };
        let mask = _mm_movemask_epi8(invalid) as u32;
        if mask != 0 {
            return at + mask.trailing_zeros() as usize;
        }
        at += 16;
    }
    at + scalar::<UTF8>(&bytes[at..])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_byte_alignment_and_tail_matches_scalar() {
        for offset in 0..16 {
            for len in 0..=64 {
                let mut data = vec![b'a'; offset + len + 32];
                // Invalid bytes just outside the slice must not affect results.
                data[offset + len..].fill(0);
                assert_eq!(printable_ascii(&data[offset..offset + len]), len);
                assert_eq!(printable_utf8_bytes(&data[offset..offset + len]), len);
                for at in 0..len {
                    for byte in 0..=255u8 {
                        data[offset + at] = byte;
                        let bytes = &data[offset..offset + len];
                        assert_eq!(
                            printable_ascii(bytes),
                            scalar::<false>(bytes),
                            "ASCII offset={offset} len={len} at={at} byte={byte}"
                        );
                        assert_eq!(
                            printable_utf8_bytes(bytes),
                            scalar::<true>(bytes),
                            "UTF8 offset={offset} len={len} at={at} byte={byte}"
                        );
                    }
                    data[offset + at] = b'a';
                }
            }
        }
    }
    #[test]
    fn long_runs_and_dense_controls_match_scalar() {
        let mut seed = 0x1480_3a99u32;
        let mut bytes = vec![b'x'; 65537];
        for _ in 0..200 {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let at = seed as usize % bytes.len();
            bytes[at] = (seed >> 24) as u8;
            assert_eq!(printable_ascii(&bytes), scalar::<false>(&bytes));
            assert_eq!(printable_utf8_bytes(&bytes), scalar::<true>(&bytes));
            bytes[at] = b'x';
        }
    }
}
