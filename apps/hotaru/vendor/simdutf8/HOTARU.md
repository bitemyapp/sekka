# Hotaru's minimal simdutf8 repair

This directory is the published `simdutf8` 0.1.5 crate, dual licensed MIT/Apache-2.0, with only two Rust source changes. Both original license files are preserved. `HOTARU-PROVENANCE.json` records the registry archive checksum, original source revision, each original file hash, and the upstream repair revision. `HOTARU-PATCH.patch` contains the complete code delta.

Published crate archive SHA-256:
`e3a9fe34e3e7a50316060351f37187a3f546bce95496156754b601a5fa71b76e`.
Original crate source revision:
`87ee8d9d20b849eae1974a82b248ee6fe7491bcc`.
Backported upstream repair:
[1890235e87c59a44d45efa6a44fcf9b7166183d4](https://github.com/rusticstuff/simdutf8/commit/1890235e87c59a44d45efa6a44fcf9b7166183d4).

The two prefetch pointer calculations use `wrapping_add` instead of `add`. For a 128-byte non-ASCII slice, the second 64-byte block previously formed a pointer at offset192. Ordinary pointer addition requires the resulting pointer to remain within the allocation or one past it. A hardware prefetch not faulting does not make that prior pointer formation valid.

`wrapping_add` permits forming this pointer. The pointer is passed only to the prefetch intrinsic, never used by an ordinary load. Rust1.95's `_mm_prefetch` is a safe intrinsic that explicitly permits invalid pointers and lowers to `llvm.prefetch`; the [LLVM contract](https://llvm.org/docs/LangRef.html#llvm-prefetch-intrinsic) describes a hint that cannot trap and does not produce a value. The real SIMD loads are separate and retain their existing bounds checks.

Hotaru uses only `compat::from_utf8`, with default `std` enabled. On x86, inputs shorter than64 bytes use the standard-library fallback. Generic binaries select AVX2 or SSE4.2 after `is_x86_feature_detected!`, otherwise use the fallback. A build targeting AVX2 selects that implementation directly and therefore requires the target CPU promised at compilation. The published API's relaxed atomic function pointer stores point only to these static, correctly typed functions; concurrent selection does not change ownership or require an ordering relationship with other data.

Full blocks start at a multiple of64 below `len - len %64`, so each load stays within the input. The final partial block is copied into an initialized64-byte temporary; the copy breaks its length into bounded32/16/8/1-byte portions. Error recovery starts at the failed block or up to three preceding continuation bytes, and uses the standard validator to recover the exact offset. Thus error detection stops at the first failing block; Hotaru retains its separate control-free range cache to avoid rescanning malformed suffixes.

Qualification is external to the vendored source to preserve the original package files. It includes full Hotaru tests and Clippy; upstream tests with `public_imp` to exercise available AVX2/SSE4.2 implementations; generic and native builds of exhaustive short-input, vector-boundary, long malformed-input, and guarded-page tests; and forced-AVX2 Miri runs that fail on the stock pointer calculation and pass after this repair. These are targeted checks plus source review, not a claim that testing alone proves all unsafe Rust correct. See Hotaru's SIMD audit evidence for exact toolchains and results.
