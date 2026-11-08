/// Borrow whole, aligned pixels from shared-memory bytes.
///
/// Panics if the slice is unaligned or ends inside a pixel. This is a safe
/// interface: callers outside the buffer pool need not uphold an unchecked
/// alignment contract.
pub fn as_pixels(bytes: &mut [u8]) -> &mut [u32] {
    // SAFETY: every u32 bit pattern is valid. align_to_mut supplies only
    // aligned, disjoint storage; reject bytes that cannot be part of it.
    let (prefix, pixels, suffix) = unsafe { bytes.align_to_mut::<u32>() };
    assert!(prefix.is_empty() && suffix.is_empty(), "pixel bytes must be whole and u32-aligned");
    pixels
}

#[cfg(test)]
mod tests {
    use super::*;

    #[repr(align(4))]
    struct Aligned([u8; 12]);

    #[test]
    fn pixels_borrow_the_original_storage() {
        let mut bytes = Aligned([0; 12]);
        as_pixels(&mut bytes.0)[1] = 0x12345678;
        assert_eq!(&bytes.0[4..8], &0x12345678u32.to_ne_bytes());
        assert!(as_pixels(&mut []).is_empty());
    }

    #[test]
    #[should_panic(expected = "whole and u32-aligned")]
    fn safe_callers_cannot_create_unaligned_references() {
        let mut bytes = Aligned([0; 12]);
        as_pixels(&mut bytes.0[1..9]);
    }

    #[test]
    #[should_panic(expected = "whole and u32-aligned")]
    fn partial_pixels_are_rejected() {
        let mut bytes = Aligned([0; 12]);
        as_pixels(&mut bytes.0[..7]);
    }
}
