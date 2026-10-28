# Scrollback storage and resize

Hotaru retains Linux terminal scrollback as owned rows grouped into bounded
blocks. The active screen remains directly editable. `RowRef` snapshots own
all cells, styles and graphemes they expose; resizing, eviction, terminal
teardown and history compression cannot invalidate an earlier snapshot.

## Logical-line resize

Primary-screen resizing reflows soft-wrapped logical lines when autowrap is
enabled. Alternate-screen resizing and disabled autowrap preserve physical
lines. Width growth happens before height adjustment; width shrink happens
after height adjustment, matching the pinned Ghostty implementation. Live and
saved cursor pins remap through reflow, including delayed wrap, blank positions,
wide-character spacers and clipped history. Erased blanks remain distinct from
explicitly printed spaces without enlarging the eight-byte cell.

`tools/reflow.py` drives the actual pinned `libghostty-vt` and the Rust replay
example through interleaved writes and resizes. `upstream/reflow-fixtures.json`
contains 272 reference sequences and 2,021 intermediate snapshots, including
history, cursor replies, protected cells, styles and graphemes. The Rust tests
replay those at three input chunk sizes (6,063 comparisons). An additional
1,003-sequence seeded stress run matched 20,012 reference snapshots. The
ordinary upstream stream replay corpus also continues to pass.

## Cold-history compression

`Terminal::compress_history_step` performs at most eight candidate inspections
and one compression attempt per call. It returns whether another idle slice
may be useful. The newest two screen-heights of history remain resident.
The worker should restart idle processing after terminal activity; the method
does not change the visible snapshot generation.

Resident blocks target 64 KiB and contain at most 64 rows. A single large row
may exceed that target. The raw archive limit is 4 MiB, including the worst
supported 8,192-column row with bounded graphemes and styles. A block that
exceeds the limit, cannot allocate scratch, or does not save space remains
resident. LZ4 encode/decode uses the safe implementations in `lz4_flex` with
checked decoding. No pointer, Rust container representation, or compiler
padding enters the archive.

The decoder checks the complete input length, dimensions, style indices,
Unicode scalar validity, cell flags, wide-cell pairs, sparse grapheme ordering,
UTF-8 and the 64-scalar/256-byte grapheme bound. It checks row payload sizes
before row allocations. A malformed block cannot request more than 4 MiB of
codec output. Internal archives are immutable and are never accepted from
external files or terminal escape sequences.

Snapshots decode only intersecting blocks and retain the resulting owned rows.
Reading does not promote cold history into permanently resident storage.
Eviction skips compressed rows without decoding them; a partially evicted
block is freed after its last logical row leaves history. Resize materializes
history for logical-line reflow, then allows later idle compression again.
Each compressed row also retains two bytes of width metadata outside the codec.
Recycling a row must preserve that state because it affects later non-reflow
resize behavior, even after the old cell contents are cleared. Discard-only
eviction allocates no replacement row. `compressed_bytes` includes this metadata.

Tests cover archive round trips, every truncation boundary, 20,000 malformed
row/LZ4 mutations, maximal-width grapheme rows, outstanding snapshots,
partial reads, incremental progress during eviction, and a compressed versus
resident terminal through scrolling, resizing and clearing.

## Measurements and present limitation

`tools/history_benchmark.py` measures fresh processes on `wx-workstation`, pinned
to CPU 8, with seven samples per corpus. Each engine retains exactly 7,977
history rows at 120 columns and 24 active rows; content checksums must agree.
The history-read measurement acquires row handles and reads the first cell of
every row. It includes application access overhead and is not a codec-only
microbenchmark. Process RSS comes from `/proc/self/status`; no allocator trim
or forced collection is performed.

`upstream/history-baseline.json` records the earlier uncompressed baseline.
`upstream/history-compressed.json` compares compression enabled and disabled in
the same Rust binary, plus the pinned Ghostty library. It includes source,
binary, library and corpus hashes. The populated Rust implementation in the
second report includes compact grapheme storage and strong-only row ownership.
These reports precede the retained-width recycling fix. Their measured values
and source hashes remain unchanged; the current representation adds two bytes
per compressed row to the accounted archive storage.

| Corpus | Rust compressed archive | Rust cold RSS | Ghostty cold RSS | Rust history read | Ghostty history read |
| --- | ---: | ---: | ---: | ---: | ---: |
| ASCII | 99,256 B | 12,092 KiB | 4,664 KiB | 1.90 ms | 1.19 ms |
| Styled | 1,620,211 B | 23,500 KiB | 15,116 KiB | 5.56 ms | 9.79 ms |
| Unicode | 130,788 B | 14,364 KiB | 5,692 KiB | 2.84 ms | 1.91 ms |
| Mixed | 209,309 B | 12,084 KiB | 4,836 KiB | 1.89 ms | 1.41 ms |

The Rust archive has low retained size, but glibc retains the freed small row
allocations in its heap. Consequently this implementation does **not** yet
match Ghostty's physical-memory reclamation, and most restore measurements are
slower. `HistoryMemory::resident_size_bound` is an archive-size upper bound,
not allocator accounting or RSS. Allocator and page-ownership experiments must
be measured before claiming memory or latency parity.
