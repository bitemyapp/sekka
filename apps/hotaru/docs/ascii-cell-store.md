# ASCII whole-cell stores

The integrated change writes each ASCII cell as one complete eight-byte value. All 88 source-manifest entries match the qualified candidate after correctness checks, memory-safety review and paired native measurements. This is a bounded terminal-core improvement, not an application-wide or universal Ghostty parity result.

The [patch and evidence](../upstream/ascii-cell-store/summary.json) identify source `7e36bc3139a9d6ee0617c30b602aa95f21bd732fba389c73ae176cfe49c74d6d`, workstation native replay `5755bcf3479b184ff8b05bcf68a22196d386a63258f99754caf380d90c576728`, and the unchanged baseline source `b9fcaa0ced20a2212e21685002a6da7b1fda89d3443394fe9afe82507c66ec3b`. Complete source/vendor manifests, compiler flags, raw samples, all paired rounds, host observations, logs and compressed profiles are retained under `upstream/ascii-cell-store/`.

## Why this change

The baseline's diagnostic disassembly emitted four field stores for every `Cell`. Those stores and their loop accounted for more than 20% of instructions in the zero-history ASCII corpus. The pinned Ghostty implementation's `printSliceStoreRun` instead combines cell metadata and codepoints into packed integer values. The Rust helper gives LLVM that complete value explicitly: the resulting bounded loop uses `vpmovzxbq`, `vpor` and unaligned vector stores on the diagnostic target.

The existing ownership check, wide-cell repair, style interning, metadata ordering, history-budget enforcement and cursor updates are unchanged. The helper replaces only the cell-assignment loop. It neither allocates nor changes the eight-byte representation.

## Unsafe boundary and verification

Compile-time assertions require `repr(C)`'s observed size and field offsets, the exact `u32/u16/u8/u8` field types, and absence of a destructor. The native-endian template initializes every byte. The scalar placement handles both endian orders, and every input byte is a valid Unicode scalar. One `write_unaligned` writes exactly the storage of each live, exclusively borrowed `Cell`; no potentially misaligned `u64` reference is created. A length assertion precedes mutation, and the checked zip iterator bounds every store.

Two independent source reviews checked the pointer, aliasing, layout, endian and metadata-order arguments without finding a concrete defect. Focused tests cover every style identifier, all byte values, both protection flags, deliberately four-byte-but-not-eight-byte-aligned cells, offsets, tails, unchanged neighbors and fail-before-write length mismatch. All three tests passed Miri with strict provenance. The native local Rust 1.99 helper tests also passed.

On the workstation, debug and release workspace suites each passed **139 tests with two existing intentional ignores**, and strict workspace all-target Clippy passed. The unchanged pinned Ghostty library matched **444 previous and 3,462 VT-control comparisons**. Its 756 freshly captured cursor cases matched the fixtures exercised at three chunk sizes in both test profiles. The ignores remain the external-shell startup fixture and its subprocess helper.

## Measured scope

Callgrind on a separate `x86-64-v3` diagnostic binary counted **17.95% fewer ASCII instructions**. Styled text decreased 1.69% and cursor controls 0.57%; mixed Unicode and malformed corpora were effectively unchanged. These instruction counts include initialization and teardown and are not wall-clock measurements.

The native paired run used the same workstation, Rust 1.95, native release flags, CPU 8, zero history, a 120×12 grid and 64 KiB feed chunks. Sibling CPU 72 was observed. Preflight activity was 3.85%/0%, and no compiler process was observed in preflight or 144 between-batch snapshots. Each engine received three warmups and 60 measured iterations per case in six alternating rounds. Every case first passed complete baseline/candidate JSON equality and the pinned reference's common-state comparison.

Candidate/baseline feed-time ratios are shown below. Lower is faster.

| Workload | Legacy median | Legacy p95 | Unicode median | Unicode p95 |
| --- | ---: | ---: | ---: | ---: |
| ASCII | 0.7172 | 0.7269 | 0.7101 | 0.6949 |
| Mixed Unicode | 0.9913 | 1.0518 | 0.9792 | 1.0236 |
| Styled text | 0.9460 | 1.0170 | 0.9486 | 0.9051 |
| Cursor controls | 1.0034 | 0.9501 | 1.0157 | 1.0517 |
| Malformed leading bytes | 0.9817 | 1.1241 | 0.9667 | 1.0324 |
| Truncated suffixes | 1.0031 | 1.0316 | 1.0046 | 1.0099 |

ASCII improved in every paired round: median-ratio ranges were 0.7079–0.7314 for legacy width and 0.6937–0.7203 for Unicode width. Other workloads had varying medians and tails; they are not blanket improvement claims. For example, the legacy malformed-leading-byte maximum ratio was 1.1314, and the Unicode cursor maximum was 1.0892. All raw samples, maxima and per-round ratios are retained without outlier trimming. Observed CPU 8 frequencies ranged from 4.687 to 5.047 GHz. A direct latest-candidate comparison against Ghostty is a separate measurement; older ratios must not be multiplied to infer parity.

## Direct comparison with pinned Ghostty

The [matched 80-row run](../upstream/ascii-cell-store/direct-ghostty-80rows/summary.json) compares this exact native candidate with unchanged Ghostty revision `6ba144158577d033ccb6ed4b574e4013957c9161`, adapter `dc687741377b3058f7b06bcdf287df9990c183eadd571a1fbe835fb3869625cd` and library `70e1bdcf62801b735bb268c38c1e65fa727debff73d89ef1d3ed9fb5e23e7639`. It uses the exact roughly 8 MiB corpus hashes and 120×80 dimensions from the earlier grapheme-charge report. Both width modes pass all four declared state/effect gates before timing.

Hotaru/Ghostty feed-time ratios follow; lower is faster.

| Workload | Legacy median | Legacy p95 | Unicode median | Unicode p95 |
| --- | ---: | ---: | ---: | ---: |
| ASCII | 0.8561 | 0.8388 | 0.8488 | 0.8556 |
| Mixed Unicode | 1.0859 | 1.0574 | 1.0709 | 1.1309 |
| Styled text | 0.8437 | 0.8593 | 0.8303 | 0.8219 |
| Cursor controls | 0.1219 | 0.1267 | 0.1222 | 0.1245 |

ASCII, styled text and cursor controls are faster in this bounded workload. Mixed Unicode remains **7.1–8.6% slower by median**, and every mixed-Unicode round remains slower. Its maximum ratios are 1.0881 and 1.0919. This does not satisfy the broader performance goal across all workloads.

The run used CPU 8, zero history, 64 KiB chunks, three warmups and six alternating rounds of ten samples per engine. Preflight activity on CPU 8 and sibling 72 was 2.91% and 0%; no compiler was detected in 244 observations. CPU 8 frequencies ranged from 4.689 to 5.047 GHz. Observation intervals ranged from 0.527 to 0.547 seconds; short external work between observations cannot be excluded. All raw samples, maxima and round ratios remain available without trimming.

The [protocol audit](../upstream/ascii-cell-store/direct-ghostty-80rows/prior-comparison.json) confirms identical adapter/library hashes, data, dimensions, chunking, zero-history policy, CPU, native Rust 1.95 release flags and feed timing boundaries against the older table. Rust replay timing source is byte-identical: input reads, setup, width configuration and teardown remain outside the feed clock. The new run has 60 samples per engine instead of 20, a different candidate core and a new observed host window; older Uta compilation was present. Differences between those windows are not a controlled causal estimate for the code changes.

A [separate smaller-grid run](../upstream/ascii-cell-store/direct-ghostty-12rows/summary.json) used 120×12 and roughly 2 MiB. Its legacy/Unicode median ratios were ASCII 0.9330/0.9454, mixed Unicode 1.2765/1.2062, styled text 0.8375/0.8487 and cursor controls 0.3589/0.3568. Those figures are retained as a distinct workload. Repeated full-screen erases depend on the grid height; the smaller-grid ratios must not be presented as a code regression from the older 80-row table. Both comparisons measure zero-history terminal feed only, excluding rendering, startup and retained-history costs.

## Local setup

Fresh local baseline and candidate binaries were built with the same Rust 1.99 and native flags for the Intel Core Ultra 7 365. Its P-cores are CPUs 0–3, and the attempted benchmark was pinned to CPU 3. Native CPU-model and feature probes agreed between P- and E-cores. These binaries and their fingerprints are separate from the workstation's Rust 1.95 artifacts.

No local timing samples were taken. The first preflight rejected 11.76% activity; three later attempts observed 90.29%, 100% and 100% while an unrelated Nix evaluation saturated the P-cores. That exceeded the permitted mildly busy observational policy. The raw rejections are preserved. A trimmed-workspace test setup initially lacked the formatter fixture; copying the exact bundled JSON resolved that setup error, and all three native helper tests then passed.
