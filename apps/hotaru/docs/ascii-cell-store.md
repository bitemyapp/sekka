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

## Local setup

Fresh local baseline and candidate binaries were built with the same Rust 1.99 and native flags for the Intel Core Ultra 7 365. Its P-cores are CPUs 0–3, and the attempted benchmark was pinned to CPU 3. Native CPU-model and feature probes agreed between P- and E-cores. These binaries and their fingerprints are separate from the workstation's Rust 1.95 artifacts.

No local timing samples were taken. The first preflight rejected 11.76% activity; three later attempts observed 90.29%, 100% and 100% while an unrelated Nix evaluation saturated the P-cores. That exceeded the permitted mildly busy observational policy. The raw rejections are preserved. A trimmed-workspace test setup initially lacked the formatter fixture; copying the exact bundled JSON resolved that setup error, and all three native helper tests then passed.
