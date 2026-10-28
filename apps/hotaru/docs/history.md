# Scrollback storage and resize

Hotaru retains Linux terminal scrollback as owned rows grouped into bounded
blocks. The active screen remains directly editable. `RowRef` snapshots own
all cells, styles and graphemes they expose; resizing, eviction, terminal
teardown and history compression cannot invalidate an earlier snapshot.

## Live history budget

`Terminal::new` applies an independent row limit and a default **50,000,000
logical bytes**, including active content. `Terminal::with_history_limits`
accepts `HistoryLimits { rows, bytes }`; `bytes: None` explicitly disables
the byte limit for applications and controlled benchmarks. The native default
continues to request 10,000 history rows. The former eight-million-history-cell
ceiling has been removed. Oldest rows are pruned when either configured limit
is exceeded, and active content always fits even when it alone exceeds the
byte budget. Compression never increases permitted history depth.

Logical charges conservatively include row and block ownership, allocation
allowances, cell/style/grapheme vector capacities, heap grapheme capacity,
hyperlink run/table capacity, and every URI/identity in each row-local table,
including entries that await compaction. Shared link payloads are charged per
owning row, so this is not unique-allocation accounting. Charges are cached;
ordinary cell overwrites do not scan history or all active rows. Metadata
changes update an owner-side total and peak with relaxed atomics. Overwriting
metadata preserves scalar allocation order in both ASCII and Unicode paths,
so PTY read boundaries do not change byte-pruning decisions. Clean rows retain
batched writes. Resize rebases the active total after its atomic transformation.

Copy-on-write row clones preserve allocation capacities and transfer the
existing live charge without temporarily doubling it. Immutable captured rows
and worker-decoded rows cannot update the active total. Compressed sidecars
preserve row charges and recycling layout; pruning does not decode or allocate
replacement rows. Restoring capacity hints validates explicit bounds before
allocation. Compression-on/off therefore retains the same history and charge,
including after shrink, metadata churn and row recycling.

`HistoryMemory::logical_bytes` reports live historical charge;
`active_bytes` reports active charge, and `limit_bytes` reports the policy.
`retained_bytes_estimate` additionally includes shared block prefixes still
owned by the terminal, but excludes blocks retained exclusively by captures.
`ContentSnapshot::retention` estimates complete captured blocks and active
row storage, with both stored and uncompressed logical totals. Estimates from
overlapping captures must not be added and described as unique memory.
Consumers can retain arbitrarily many immutable snapshots; the terminal's
live budget cannot reclaim their storage. Native copy/search workers use
bounded pending work and cancellation, but allocator arenas, decoder scratch,
formatted output, renderer storage and external consumers are outside this
logical budget. It is not an RSS or process-memory ceiling.

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
exceeds that limit or 32 MiB of conservative logical ownership, cannot allocate
scratch, or does not save space remains
resident. LZ4 encode/decode uses the safe implementations in `lz4_flex` with
checked decoding. No pointer, Rust container representation, or compiler
padding enters the archive.

The decoder checks the complete input length, dimensions, style indices,
Unicode scalar validity, cell flags, wide-cell pairs, sparse grapheme ordering,
UTF-8 and the 64-scalar/256-byte grapheme bound. It checks row payload sizes
before row allocations. A malformed block cannot request more than 4 MiB of
codec output; restored dictionary/row allocations share a separate 64 MiB
conservative debit budget, checked before allocation. This also bounds
capacity restoration and highly shared hyperlink expansion. Internal archives
are immutable and are never accepted from
external files or terminal escape sequences.

`Terminal::content_snapshot` is the copy/search ownership boundary. Capture
costs O(history blocks + visible rows), with one reference per immutable
history block and one per active row. It never decodes history. Cloning the
result is constant-time. Resident blocks are shared too: appending after a
capture copies at most one block's 64 row handles. Eviction advances a private
prefix without copying a shared block. Outstanding captures can retain evicted
data until their worker releases them.

`ContentSnapshot::rows_range` uses frozen physical row coordinates and a binary
search over block ends. Workers decode only intersecting blocks, at most one
block at a time; `next_cancellable` checks before each row and before starting
the next decode. A decode is not preempted midway and is bounded by the 4 MiB
raw archive cap and 64 MiB shared row/dictionary allocation budget.
The same owned capture remains valid across compression, eviction, reflow and
terminal teardown. Palette, defaults and cursor coordinates are frozen with it.
Existing render `Snapshot` remains unchanged; `snapshot_all` is an eager
diagnostic compatibility path, not the native whole-history copy path.

The versioned archive uses a block-local hyperlink dictionary. Raw URI and
explicit identity bytes are preserved, implicit identities retain their numeric
value, and decoded runs share owned hyperlink objects. Dictionary counts,
2,048-byte identity/URI bounds, run indices and ordered nonoverlapping ranges
are validated. A hyperlink-dense row whose conservative archive bound exceeds
4 MiB stays resident with all data intact.

Reading does not promote cold history into permanently resident storage.
Eviction skips compressed rows without decoding them; a partially evicted
block is freed after its last logical row leaves history. Resize materializes
history for logical-line reflow, then allows later idle compression again.
Each compressed row retains a 16-byte sidecar on 64-bit targets: its logical
charge, retained width, and cell/style/grapheme allocation capacities.
Recycling a row must preserve that layout because it affects later non-reflow
resize behavior and byte retention, even after old contents are cleared. Discard-only
eviction allocates no replacement row. `compressed_bytes` includes this metadata.

Tests cover archive round trips, every truncation boundary, 20,000 malformed
row/LZ4 mutations, maximal-width grapheme rows, outstanding snapshots,
partial reads, incremental progress during eviction, and a compressed versus
resident terminal through scrolling, resizing and clearing. Content tests also
cover immutable block sharing, bounded tail copy-on-write, range boundaries,
cancellation before deliberately damaged compressed content, hyperlink
dictionaries and concurrent reads after terminal teardown.
Budget regressions cover unique URI churn, shrinking/recycling, oversized
active content, compressed/resident equivalence, corrupt cold content pruned
without decoding, held render/content snapshots, and exact-budget metadata
overwrites split at one, seven and 65,536 bytes. Existing 12,000-operation
compressed/resident mutation stress also runs with the new default policy.

### Allocation-order audit and accounting cost

Checkpoint `c9801d6538de464819dc6e4d790a3095ff7df13d` still had a wide-cell
overwrite edge: replacing both halves in a bulk run could skip an erased-style
allocation performed by scalar writes. `tools/wide-budget-audit.rs` reproduced
active charges of 1,036 bytes with one-byte reads versus 980 bytes with seven-byte
reads. The correction preserves scalar metadata order when an overwritten wide
cell may allocate an erased style or trigger style compaction. It keeps batching
when the erased style is already guaranteed present. Exact-limit, unlimited,
compression, COW and style-compaction regressions cover ASCII and Greek writes.

The accounting signal has one writer, the terminal owner. Captured readers only
fill a separate immutable row charge cache, and decoded rows have no active
registration. Replacing locked atomic read-modify-write instructions with relaxed
loads/stores on that owner preserves Atomic storage and snapshot thread safety.
The isolated corrected checkpoint passed 98 core tests and strict Clippy.

`upstream/budget-atomics/audit.json` identifies the exact candidate sources,
patches, validation and raw same-host timing reports. In the controlled earlier
source, removing the clean-path locked dirty-flag operation improved the
Rust/Zig ratios from 1.645 to 1.207 for ASCII and 1.716 to 0.820 for styled text.
Replacing the remaining owner-only operations improved those to 1.070 and 0.777.
The corrected `c980` candidate, including the wide-cell guard, measured 1.179
for ASCII, 1.307 for mixed Unicode, 0.770 for styled text and 0.112 for cursor
updates in legacy width mode. These are zero-history feed measurements with
reference state checked before timing. They do not establish application-wide
parity, and reference timing varied between measurement windows. The report
also retains configured Unicode-mode results and all source/binary hashes.

## Content capture measurements

`upstream/content-capture.json` records the exact source and binary hashes for
the owned capture and the eager `snapshot_all` implementation from checkpoint
`13423440ef68b3633b41d267aea196d87b1007da`. Both use 80 columns and 24 visible
rows; 120 columns would hit the then-existing 8-million-history-cell cap before
100,000 rows. Measurements use CPU 8 on `wx-workstation`, native CPU code,
thin LTO, one codegen unit and the system allocator. Five fresh processes run
each case, with 500 owned captures or 30 eager captures per process. Values
below are medians of each process's median; release costs are measured
separately in the report.

| History rows | Owned resident capture | Owned cold capture | Checkpoint resident eager capture | Checkpoint cold eager capture | Cold worker restore after teardown |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1,000 | 0.231 µs | 0.231 µs | 6.840 µs | 181.675 µs | 0.271 ms |
| 10,000 | 0.831 µs | 0.831 µs | 64.919 µs | 1.850 ms | 2.838 ms |
| 100,000 | 6.671 µs | 6.690 µs | 665.789 µs | 22.155 ms | 28.470 ms |

The capture improvement comes from moving restoration to the worker and
cloning block handles. It does not establish faster decoding or total-terminal
performance parity. Capture still scales with block count. Worker restore
includes row access and destruction, and verifies a first-cell checksum after
the terminal owner has been dropped. Reproduce with the
`content_capture` example, `tools/capture-checkpoint.rs` installed as an
example in an isolated checkout of the named checkpoint, and
`tools/content-capture.py` under the shared workstation lock.

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
These reports precede the retained-width recycling fix and current byte-budget
accounting. Their measured values and source hashes remain unchanged. The
current representation includes 16 bytes of sidecar metadata per compressed
row and preserves additional allocation/table information in archive version
HHR3. No current RSS or restore-time parity claim follows from these older
measurements.

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
