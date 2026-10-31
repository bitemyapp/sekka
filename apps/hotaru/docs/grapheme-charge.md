# Avoiding unchanged grapheme ownership updates

Overwriting an inline grapheme can remove text without releasing any allocated
storage: `Vec::retain` preserves its capacity, and an inline `CompactString`
has no separately charged heap allocation. `GraphemeMap::retain` now reports
whether it released charged heap storage. The row overwrite path skips full-row
charge recomputation only when that value is unchanged. Style interning and
hyperlink removal still update their charges independently and in their
original order. This changes no representation, allocation order or budget
policy and adds no unsafe code.

The [evidence](../upstream/grapheme-charge/) records a broader experiment that
also skipped unchanged charges while extending graphemes, separate experiments
for each operation, and the selected retain-only implementation. The broader
version regressed other paths and was rejected. Independent review found no
concrete capacity/accounting defect in the selected change.

The selected isolated source is
`05772703d10388304fe02e2c9a67b275106fb894a60f61b0258e08a6387134be`, with replay
SHA-256 `1b721257f1c937b7623cb83e897239965d61b504c407381dd3389a1ee2ad3d01`.
It passes 109 core tests, 23 ordinary I/O tests, the replay fixture test, strict
workspace Clippy and 3,462 actual-library VT comparisons. The new regression
checks inline removal, release of heap text, removal of the last inline entry,
retained vector capacity, cached charges, budget peaks and a retained copy.
Existing tests cover chunk-dependent budget pruning, COW captures, compression,
reflow and long randomized row edits.

Paired diagnostics on the final isolated build reduce mixed Unicode time by
about 10% with legacy widths and 8% with Unicode widths. Other timings vary:
the last Unicode styled/cursor samples were about 4%/6% slower, while earlier
trials were near equal or faster. Unrelated Uta builds were active on the same
host; process/load observations are retained. These are provisional measurements,
not evidence of an improvement in every workload. The explicit CPU selection
and observations added to the harness do not exclude unrelated work.

The reviewed zero-history Ghostty comparisons still show remaining gaps:

| Feed workload | Hotaru / Ghostty, legacy | Hotaru / Ghostty, Unicode |
| --- | ---: | ---: |
| ASCII | 1.199 | 1.215 |
| Mixed Unicode | 1.064 | 1.065 |
| Styled ASCII | 0.848 | 0.832 |
| Cursor operations | 0.118 | 0.123 |

All eight streams passed their declared state/effect gates before timing.
ASCII and mixed Unicode remain slower than Ghostty. These ratios exclude
history, rendering and startup. The [combined package](cursor-cache-checkpoint.md)
now passes its behavior checks; performance measurement on a quieter host
remains required. Integration adds only a comment to the selected grapheme
method beyond the measured patch.
