#[test]
fn non_ascii_final_block_prefetch() {
    // Exactly128 bytes: the second64-byte block's prefetch target is192,
    // beyond the allocation. Its pointer may be formed with wrapping_add,
    // but ordinary pointer add violates Rust's allocation-bound requirement.
    let bytes = "é".repeat(64).into_bytes().into_boxed_slice();
    assert!(simdutf8::compat::from_utf8(&bytes).is_ok());
}

#[test]
fn vector_tail_and_error_boundaries() {
    let pattern = "é雪👩".as_bytes();
    for len in [0, 1, 31, 32, 33, 63, 64, 65, 95, 96, 97, 127, 128, 129, 191, 192, 193, 255, 256, 257] {
        let mut bytes = (0..len).map(|i| pattern[i % pattern.len()]).collect::<Vec<_>>().into_boxed_slice();
        let compare = |bytes: &[u8]| {
            let expected = std::str::from_utf8(bytes).map_err(|e| (e.valid_up_to(), e.error_len()));
            let actual = simdutf8::compat::from_utf8(bytes).map_err(|e| (e.valid_up_to(), e.error_len()));
            assert_eq!(actual, expected);
            assert_eq!(simdutf8::basic::from_utf8(bytes).is_ok(), expected.is_ok());
        };
        compare(&bytes);
        if let Some(last) = bytes.last_mut() { *last = 0xff; compare(&bytes); }
    }
}
