#[test]
fn non_ascii_final_block_prefetch() {
    // Exactly128 bytes: the second64-byte block's prefetch target is192,
    // beyond the allocation. Its pointer may be formed with wrapping_add,
    // but ordinary pointer add violates Rust's allocation-bound requirement.
    let bytes = "é".repeat(64).into_bytes().into_boxed_slice();
    assert!(simdutf8::compat::from_utf8(&bytes).is_ok());
}
