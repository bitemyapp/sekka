# Repaired SIMD UTF-8 validator

The audited vendor is now integrated in the Hotaru workspace. Aizome's root
Cargo patch resolves transitive users to that same repaired source; both locks
contain no second stock-registry copy. The package source filter and benchmark
and Wayland source manifests include the vendor. Both original license texts
are installed with the application notices. The isolated measurements below
retain their original identities; combined protocol/native qualification is
recorded separately and must not be inferred from them.

The integrated validator uses `simdutf8::compat::from_utf8` through a small outlined function. It preserves the scanner's cached control-free extent and the standard-library retry on the valid prefix of malformed input. It introduces no unsafe code in Hotaru itself. The bundled dependency contains unsafe SIMD code and has a narrowly documented upstream repair; the unmodified registry release is unsuitable for this use.

The candidate is isolated at `/home/callen/work/hotaru-simdutf8-outlined`. Its source SHA-256 is `06f25a0842fb37a485bbea56813c1f4b4be01d31a6dc0dc32fd7b8ae92708977`, including the complete vendor tree. Its native replay SHA-256 is `236ef248a61e1e512eb638f372023a854812dacc6f22f088bf801ae681adbff5`. Raw evidence is under `upstream/utf8-simd/`. This document records that exact candidate; it does not claim full terminal or application performance parity.

## Dependency source and unsafe review

The vendor is the published 0.1.5 archive, SHA-256 `e3a9fe34e3e7a50316060351f37187a3f546bce95496156754b601a5fa71b76e`, corresponding to source revision `87ee8d9d20b849eae1974a82b248ee6fe7491bcc`. MIT and Apache license files are retained. The original file hash manifest and complete two-line source patch are preserved both in the vendor and in this evidence directory.

[Upstream repair 1890235e87c59a44d45efa6a44fcf9b7166183d4](https://github.com/rusticstuff/simdutf8/commit/1890235e87c59a44d45efa6a44fcf9b7166183d4) changes two prefetch pointer calculations from `add` to `wrapping_add`. With an exactly 128-byte non-ASCII allocation, the last block formerly formed a pointer at offset 192. The pointer arithmetic itself violated Rust's allocation bounds even though hardware prefetch did not fault.

The repaired pointer is used only for a prefetch hint. Rust 1.95's `_mm_prefetch` permits invalid pointers and lowers to `llvm.prefetch`; the [LLVM intrinsic contract](https://llvm.org/docs/LangRef.html#llvm-prefetch-intrinsic) permits the hint without trapping or producing a loaded value. Ordinary vector loads remain separately bounded. The valid slice length is at most `isize::MAX`, so adding 128 to the block index does not overflow `usize`.

Independent review checked that full vector loads occur only with at least 64 bytes remaining, and the final short tail uses an initialized 64-byte local buffer with bounded copying. The compatibility implementation exits at the first failing block. Its scalar error-location check starts at most three bytes before that block and stops at the first error. Combined with Hotaru's extent cache, malformed-input processing remains linear.

Default `std` feature dispatch selects AVX2 or SSE4.2 only after runtime CPU detection in generic binaries, falling back otherwise. Native AVX2 builds require the CPU feature promised by their compiler target. The published function-pointer cache contains only static functions of the correct signature. The `public_imp` feature is enabled only for dependency qualification, not the Hotaru dependency declaration. AArch64/WASM source is retained unchanged; those architectures were not executed on this x86 workstation.

## Validation

The complete workspace passed 106 core tests, 23 ordinary I/O tests, the replay fixture test, and strict Clippy. The three added validator tests compare exact success text and `(valid_up_to, error_len)` against the standard library:

* All 16,843,009 byte strings of length zero through three. These intentionally exercise the public API's scalar fallback for short input.
* 34,816 seeded long-input checks, including 64 KiB runs, different alignments, truncation and malformed bytes.
* 655,360 two-byte placements around 32/64-byte boundaries in 256-byte inputs, with a valid non-ASCII first block to exercise the SIMD path.
* 1,025 guarded-page cases over lengths zero through 512, so an actual load past the slice end would fault.

The tests pass in a generic debug build using runtime dispatch and in a native release build using direct AVX2 selection. The vendored upstream suite passes 11 tests and three doctests with `public_imp`, and again without default features to cover the fallback configuration.

Forced-AVX2 Miri on Rust `1.95.0-nightly (842bd5be2 2026-01-29)` demonstrates the original defect and the repair. The stock crate fails on the 128-byte boxed allocation at `algorithm.rs:306` with invalid pointer arithmetic. The repaired crate passes that exact test plus vector-tail and malformed-boundary cases through both compatibility and basic APIs. These logs and standalone reproduction manifests are preserved under `upstream/utf8-simd/miri/`. The fixed manifest's absolute dependency path should be adjusted to the local candidate vendor when rerunning it.

## Measured effects and limits

All four reviewed patterns passed real-Ghostty grid/effect gates in both legacy and Unicode modes before timing. The final outlined candidate's feed-only Rust/Ghostty ratios were:

| Pattern | Legacy width | Unicode width |
|---|---:|---:|
| ASCII | 1.202 | 1.254 |
| Mixed Unicode | 1.140 | 1.085 |
| Styled ASCII | 0.837 | 0.846 |
| Cursor operations | 0.114 | 0.114 |
| Separate dense Han diagnostic | 1.734 | 1.814 |

These are native builds, zero history, equal configuration, one CPU, serial timings, input reads outside the timer, alternating engine order, and measured reply-count checks. Dense Han remains substantially slower than Ghostty; ASCII and mixed Unicode also remain above parity. Rendering, large history, graphics and other unobserved features are outside this acceptance subset.

Against the same Rust source with the standard validator and extent cache, paired measurements show approximately 25% less dense Han time in legacy mode and 16% less in Unicode mode. ASCII differs by 0.3–1.1%; styled input is 2.8–3.6% slower; cursor input is 1.3–3.0% slower; mixed input differs by −4.9%/+0.3%. These small regressions remain explicit, and the data do not establish an across-the-board improvement.

The direct SIMD call increased styled ASCII time by 12–14%, despite that workload never invoking UTF-8 validation. Outlining restores the parser's original compiled function size and substantially reduces that code-generation effect. A separate lead-byte guard reduced some malformed-input costs but did not consistently improve the valid workloads. Those alternative measurements are retained, not substituted for the final candidate.

## Integration boundaries

`upstream/utf8-simd/outlined-integration.patch` contains the narrow Hotaru changes, validator tests, workspace vendor exclusion, and benchmark source-manifest support. It deliberately excludes the vendor copy, both generated workspace lockfiles, and Nix source-filter changes. Integration copied the exact audited `vendor/simdutf8` directory, included it in the package source, regenerated both Hotaru and Aizome locks, and confirmed that the dependency resolves to that path. The manifest helper hashes the vendor's sources and notices as part of the candidate identity. The [combined checkpoint](links-vt-checkpoint.md) records the separate package and native validation gates; the isolated measurements above remain evidence for their original candidate.
