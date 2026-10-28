# Hyperlinks, copy formats and scrollback search audit

This is an implementation proposal, not completed feature coverage. It audits
Ghostty `6ba144158577d033ccb6ed4b574e4013957c9161` and the current Hotaru worktree.
The reference library used for the small probes below has SHA-256
`70e1bdcf62801b735bb268c38c1e65fa727debff73d89ef1d3ed9fb5e23e7639`.
No product source changed during this audit.

## Current behavior and the missing ownership boundary

Hotaru's `Row` contains eight-byte cells, row-local styles and sparse owned
graphemes. `Snapshot` owns shared rows plus its palette/default colors. The
native clipboard worker already formats plain text off the interface thread,
with a replaceable pending job and generation-checked publication. Selection
endpoints are viewport row/column pairs with an exclusive end. It has no
rectangular selection, hyperlink metadata, VT or HTML formatter.

Native `matching_cells` currently performs case-sensitive, non-overlapping
matching separately on each displayed row. It does not search scrollback or
across soft wraps. Upstream does all three differently.

Cold history now stores compressed blocks, with at most 64 rows per block and
a validated four-MiB decode ceiling. Its compressed bytes are `Box<[u8]>`.
`History::range` decodes a block when visited, and `snapshot_all` materializes
all history rows on the terminal owner. Sending that snapshot to a worker does
not move the earlier decompression off the terminal owner. Large-history copy
and search therefore need an owned block snapshot interface before they can
meet the clipboard responsiveness requirement.

## Reference semantics

The relevant upstream sources are `terminal/osc/parsers/hyperlink.zig`,
`terminal/hyperlink.zig`, `Screen.zig`, `Terminal.zig`, `formatter.zig`,
`search/{sliding_window,pagelist,screen,terminal}.zig`, and
`Surface.zig::copySelectionToClipboards`.

OSC 8 stores a URI and either an explicit ID or a screen-local generated
implicit ID. Identity includes both ID and URI; equal URIs with distinct
implicit starts remain distinct links. URI and ID are byte sequences, not
necessarily valid UTF-8 strings. Empty `id=` means no explicit ID. Options are
colon-separated key/value pairs; a malformed pair stops option parsing and
unknown well-formed keys are skipped. Empty URI closes the current link only
when no nonempty explicit ID is present. Malformed/oversized commands do not
implicitly close a previously active link.

OSC 8 uses the fixed 2,048-byte trailing capture, then appends a sentinel NUL
while parsing. Consequently its accepted body is one byte shorter than OSC 4.
Direct `ghostty_grid_ref_hyperlink_uri` probes observed:

| Body after `8;` | URI on the next printed cell |
| --- | --- |
| `;` plus 2,046 `x` bytes (2,047 bytes total) | The 2,046-byte URI |
| `;` plus 2,047 `x` bytes (2,048 bytes total) | Previous active URI retained |
| `;` plus 2,048 `x` bytes | Previous active URI retained |
| `id=a;` | Previous active URI retained |
| `id=;` | No hyperlink |

Each probe first opened `original`, printed `A`, sent the tested command and
printed `B`; the URI was read at `B`. This separates rejection from an accepted
close. The ordinary 2,048-byte OSC bound alone is insufficient for this parser.

Upstream keeps hyperlink IDs outside cells in a sparse cell-offset map, with
reference-counted page-local link entries and owned strings. A cell and row
have presence flags for fast rejection. Printed wide characters attach metadata
to both halves; the special spacer head at a wrap boundary also matters.
Overwrites and erases remove associations; shifts, scrolling, cloning and
reflow migrate them. Switching screens ends the old screen's active link.
Saved-cursor operations do not save/restore hyperlink state. DECSTR leaves the
active hyperlink alone; RIS resets it with the screen. Existing upstream tests
exercise dense hyperlink rows, page movement, overwrite, wide-edge and VS16
expansion paths, not just the OSC parser.

Plain copy emits visible text without link destinations. VT copy emits SGR,
OSC 8 starts (retaining explicit IDs), link termination and CRLF line endings.
HTML emits escaped anchors and styled text. Link state must close before blank
cells, on transitions to unlinked text and at output completion. Selected
subranges must start with their own valid style/link state. Wrap joining,
rectangle boundaries, trailing spaces, blank rows, palette resolution and
wide/grapheme handling are shared formatter concerns.

Upstream's application copy path supplies owned-equivalent effective palette,
foreground/background, trim preference and codepoint mapping. Plain and VT
use `text/plain`; HTML uses `text/html`; mixed supplies both plain and HTML.
Mixed HTML intentionally omits the outer foreground/background override.
The public C selection formatter does not expose every application-specific
formatting option, so its output alone cannot certify those options.

Search is byte-exact except ASCII case folding. It allows overlapping matches,
joins soft-wrapped rows, retains hard newlines, maps UTF-8 bytes back to cells,
and searches both screens' active areas and scrollback. It maintains screen
generations and recovers from pruning, resize/reflow and reset. The work splits
into copying bounded input under exclusive terminal access and searching owned
buffers without terminal access. It scans history from recent to old and keeps
only sufficient overlap to find a needle across page boundaries.

Direct C-API search probes, on a four-column/two-row terminal with default
history, produced these total counts:

| Text | Needle | Matches |
| --- | --- | ---: |
| `aaaa` | `aa` | 3 |
| `AbA` | `a` | 2 |
| `É` | `é` | 0 |
| `abcDEF` (soft-wrapped) | `cde` | 1 |
| `abcd\r\nEF` | `de` | 0 |
| `abcd\r\nEF` | `d\nE` | 1 |
| `old\r\n1\r\n2\r\n3` | `old` | 1 |

These probes used `ghostty_search_new`, `set(NEEDLE)`, `run`,
`get(TOTAL_MATCHES)` and `free`; new ctypes structures were checked against
the library's ABI manifest before use. They are audit observations, not yet
committed Rust acceptance fixtures.

## Proposed compact representation

Keep `Cell` unchanged at eight bytes. Add `Option<Box<RowLinks>>` to `Row`, so
unlinked rows pay one pointer of metadata and allocate nothing. `RowLinks`
holds sorted non-overlapping runs `{ start: u16, end: u16, link: u16 }` plus a
row-local table of shared immutable `Hyperlink` objects. End is exclusive;
adjacent runs of the same semantic link merge. A live row has at most one run
per column and at most that many referenced links. Row/table compaction removes
unreferenced entries after repeated repainting, analogous to style compaction.

`Hyperlink` owns URI bytes and `Explicit(id_bytes)` or `Implicit(screen_serial,
sequence)` identity. Share one allocation across a printed run and across row
boundaries, rather than cloning a URI or incrementing an Arc per cell. Explicit
identity comparisons include URI; implicit starts allocate a new sequence.
Do not use a permanent global URI interner: overwritten links must become
reclaimable, and retained snapshots may be their final owners. A wider internal
serial is acceptable only if observable wrap/identity behavior is documented
and tested; it must never alias old retained snapshot identities.

Printing batches merge/append runs once per batch. Rows with no links retain a
single fast rejection before existing ASCII/UTF-8 writes. Overwrite helpers
delete only intersecting runs and repair wide-boundary metadata alongside the
cells. Shift, resize and reflow copy associations with glyphs. Lookup uses
binary search; render/copy iterators can walk runs linearly alongside cells.
The run representation is a proposal to benchmark against a sorted sparse
cell map for adversarial alternating-link workloads, not a proven performance
improvement over Zig.

History serialization must include link identity, URI and runs with explicit
length/count bounds and validation of run order, overlap, column limits and
table indexes. Prefer a block-local dictionary to avoid repeating a long URI
for every wrapped row. Decode shares each dictionary entry across its rows.
The retained memory accounting includes link bytes and snapshot ownership;
the existing `history_size_bound` must remain a real upper bound. Snapshot
copy-on-write then retains links without borrowing a live terminal registry.
A row containing thousands of distinct long links can exceed the current
four-MiB raw-block limit. Preserve the existing resident fallback for oversized
blocks rather than truncating links or silently weakening the decode bound.

## Owned history and search plan

Make sealed history block payloads immutable shared objects, including
compressed bytes and their link dictionary. A worker can hold a block after
history eviction and decode it without touching the terminal. Keep the live
tail bounded (currently at most 64 rows), and freeze its row handles when
creating a request. Palette/defaults, formatting options and selection bounds
belong to the request, not the live terminal.

Capturing a flat vector of every block handle is still O(block count), even
though it avoids decompression. Measure that separately; do not describe it as
constant time. For a bounded owner step, use an immutable segmented block index
with a shared snapshot root and copy-on-write along the modified path. A simpler
initial flat handle capture is acceptable only as an explicitly measured
intermediate, with a practical history-size limit and no full responsiveness
claim. Never feed mutable history iterators to a detached worker.

Build one text projection shared by plain formatting and search: UTF-8 chunks
plus compact byte-span-to-cell mappings, with soft/hard boundary metadata.
Search ASCII-folds only ASCII bytes, advances one byte after a match to preserve
overlaps and retains `needle.len() - 1` overlap bytes between chunks. It must not
assemble a second full-history String. Compressed blocks decode one at a time
off the PTY owner. Cancellation is checked between bounded decode/format/search
chunks; query changes replace pending work and stale generations cannot publish.

Results carry screen generation and stable content anchors, not current
viewport row numbers or pointers. Add a content revision separate from the
existing general generation: output revisions rescan changed active rows;
history pruning invalidates only discarded anchors; reflow remaps anchors or
restarts the affected screen search. Keep separate primary/alternate results.
An immutable snapshot search can provide an initial correct static result,
but matching upstream's continuing live search requires this reconciliation.
Enumerate results in bounded batches rather than allocating one giant vector
for a one-character needle across huge history.

## Integration and acceptance sequence

1. Add OSC 8 parser/state and row metadata, then update every row movement and
   compressed-history codec. Public read API: `Row::hyperlink(col)` returning
   an immutable borrowed value owned by that row. Keep C APIs external.
2. Add a formatter module over owned row/block snapshots with explicit
   `Plain`, `Vt`, `Html`, trim, unwrap and rectangle options. Snapshot palette
   and codepoint-map settings. Extend clipboard publication to prepared MIME
   bundles and keep both formatting and Wayland writes outside UI/terminal
   critical sections. Preserve generation ordering for every format.
3. Add bounded history snapshots, text projection and a search worker.
   Integrate `Find next/previous`, result count and scroll-to-match through the
   PTY actor; the widget receives already-mapped visible highlights. Retain
   current normalized input processing on that actor.
4. Extend the reference harness with hyperlink URI reads,
   `ghostty_terminal_selection_format_buf` (plain/VT/HTML, linear/rectangle),
   `ghostty_formatter_format_buf` and the search API. Convert upstream inclusive
   selection endpoints carefully to Rust's exclusive endpoints, especially for
   wide characters. URI reads cannot prove explicit/implicit identity, so VT
   transitions and selected identity cases need formatter/snapshot evidence.
5. Generate actual fixtures for parser boundaries and malformed options;
   repeat IDs and URI changes; overwrite/erase/insert/delete; wide halves and
   combining/VS16; alternate/reset/cursor semantics; resize and compressed
   history. Format fixtures must cover link/style changes, quotes/ampersands,
   non-ASCII URI bytes, partial links, palette mutation after capture, soft
   wraps, rectangles and trailing blanks. Search fixtures must cover overlaps,
   ASCII case, Unicode byte exactness, grapheme maps, soft/hard boundaries,
   block edges, history eviction, alternate screens and live reflow/reset.
6. Repeat the large-copy and stalled-reader UI tests for each format, including
   cancellation, window close and concurrent output. Benchmark plain unlinked
   feed for regression, dense-link feed, retained history bytes/RSS, block
   decode, copy throughput/peak memory, search first-result/cancellation latency
   and terminal-owner stall time. Same-corpus feature parity gates precede
   timing; the current hyperlink corpus remains ineligible for performance
   acceptance until storage and effects are implemented and compared.

Core integration touches `row.rs`, `row/history.rs`, `history.rs`, parser/OSC
dispatch, all screen printing/reflow helpers, and the snapshot interface.
Native integration touches `session.rs`, `clipboard.rs`, selection endpoints
and `terminal_view.rs`. The compressed-history and UI owners should agree on
these interfaces before parallel implementation to avoid losing metadata in
an otherwise correctly tested resize or clipboard path.
