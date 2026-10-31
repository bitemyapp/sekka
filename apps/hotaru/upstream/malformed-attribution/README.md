# Malformed UTF-8 instruction attribution

This diagnostic follows the directly measured 120×12 malformed-input gaps. It profiles the current qualified Hotaru source `7e36bc31…` on the exact `short-invalid-suffix` and `short-interior-error` inputs in both width modes. It changes no product code.

Callgrind cannot execute the pinned native Ghostty library's AVX-512 instruction during `PageList.MemoryPool.init`. Those four failed attempts and partial profiles are retained under `profiles/`; their counts are not valid workload comparisons. A separate pristine clone at the same commit was built with `-Dcpu=x86_64_v3 -Dstrip=false`, ReleaseFast. The adapter source is unchanged, rebuilt with `-march=x86-64-v3 -g`. This diagnostic library matches the unchanged native library's complete captured state in all four cases. The original native library hash remains unchanged.

Hotaru's diagnostic build likewise uses `x86-64-v3`, release optimization and debug information. Its source/vendor manifest matches the measured native baseline; only build settings and output path differ. All source, compiler, library and executable hashes and exact commands are preserved. No diagnostic executables or generated corpora are checked in.

Whole-process instruction totals include startup, setup and teardown. They are not feed-only wall times, native performance ratios, or cross-ISA acceptance evidence. The function percentages below aggregate exclusive entries displayed by the 99% annotation threshold; tiny omitted entries are not estimated.

| Hotaru path | Truncated legacy | Truncated Unicode | Interior legacy | Interior Unicode |
| --- | ---: | ---: | ---: | ---: |
| `State::print_utf8` including inlined char iteration | 56.99% | 56.32% | 29.42% | 28.41% |
| `core::str::from_utf8` | 9.77% | 9.59% | 9.58% | 9.12% |
| `Terminal::feed_inner` including inlined parser/scanner | 9.29% | 9.12% | 13.30% | 12.67% |
| Scalar `Handler::print` | 2.07% | 4.31% | 6.47% | 13.10% |

The truncated pattern contains one wide character followed by 48 ASCII bytes and an incomplete four-byte sequence before CRLF. Validation returns the valid prefix; `feed_inner` validates that prefix again and passes all of it to `print_utf8`. Its long ASCII suffix therefore follows per-character iteration, width checks and cell construction. A run beginning with ASCII instead uses `print_ascii` and the already-qualified packed cell stores. Instruction attribution makes this printing distinction the primary next target; removing duplicate validation alone cannot account for the largest cost.

The interior-error pattern also incurs more scalar replacement processing, row repairs and style lookups. Those costs and the exact error/continuation state must remain intact. The source already caches the control-free byte extent to avoid quadratic rescanning of malformed input; any change must preserve that linear behavior.

The reference profile instead attributes substantial work to SIMD UTF-8-to-UTF-32 conversion and `Terminal.printSlice`/`printSliceFill`. This supports testing a narrow handoff of sufficiently long printable ASCII subruns to Hotaru's existing ASCII path, with existing metadata ordering, Unicode boundaries, cursor/wrap and REP state preserved. No implementation or performance acceptance follows from instruction counts alone.

Generated input bytes remain at the recorded local/workstation paths and can be reconstructed from the adjacent complete-tail manifest and generator. All successful raw profiles, exclusive/inclusive annotations, profiler JSON/logs, failed native attempts and diagnostic build logs are compressed without changing their bytes.
