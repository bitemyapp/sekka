# Hyperlinks, copy formats and scrollback search audit

This document records the implemented hyperlink/copy slice and the remaining
search proposal. It audits Ghostty
`6ba144158577d033ccb6ed4b574e4013957c9161` and the Hotaru worktree.
The reference library used for the small probes below has SHA-256
`70e1bdcf62801b735bb268c38c1e65fa727debff73d89ef1d3ed9fb5e23e7639`.

## Implemented hyperlink and formatting slice

`hyperlink.rs` stores owned raw URI/ID bytes behind shared immutable references.
Rows use optional sorted hyperlink ranges and a row-local reference table;
unlinked rows allocate no link metadata and cells remain eight bytes. Repeated
printing checks pointer identity before comparing URI bytes. Overwrite, wide
boundaries, erasure, insertion/deletion, reset, scrolling and reflow propagate
or remove ranges. Table compaction bounds stale references. History blocks
encode a shared dictionary plus validated ranges; oversized blocks stay
resident, preserving data instead of truncating metadata.

`ContentSnapshot` captures shared immutable history segments and active rows,
plus palette/default colors. Capturing does not decompress history. Its worker
iterator decodes bounded blocks and checks cancellation before each row/block.
`formatter::format_copy` produces `Plain`, `Vt`, `Html` or `Mixed` output from
this owned content, with exclusive selection endpoints, rectangles, trimming
and wrap options. The default is mixed with trim/unwrap enabled. A byte budget
applies to the aggregate MIME data. Cancellation, invalid selection and budget
exhaustion return errors; no partial or silently truncated clipboard is returned.
Mixed HTML omits outer default colors, while explicit HTML/VT includes captured
defaults. Palette colors resolve from the snapshot, even after later mutations
or terminal destruction. No terminal state is borrowed by formatting workers.
The `(rows, 0)` EOF sentinel is accepted for linear ranges only; rectangles
reject it so normalization cannot silently change their selected columns.

`upstream/hyperlink-fixtures.json` contains 134 cases obtained from the pinned
external C API: parser/body limits, raw URI bytes, identity transitions,
row/wide edits, resets/screens, resize, styles, whitespace, wrap options and
linear/rectangle selections. Tests compare every visible URI at three input
chunk sizes (402 comparisons), plus plain/VT/HTML output (402 comparisons).
391 output comparisons are byte-exact; eleven named HTML cases intentionally
repair a bug in the pinned formatter. Independent dense-array testing covers
20,000 sparse range mutations and dictionary restoration. Owned content tests
cover compressed-history capture, decode, eviction, resize and terminal teardown.
`upstream/hyperlink-report.json` identifies exact source and fixture hashes.

The intentional HTML correction is narrow: crossing terminal style and link
scopes can produce `<div style="...">a<a href="...">b</div>c</a>` or
`<a href="...">a<div style="...">b</a>c</div>` upstream. Hotaru closes and
reopens inner elements to retain both scopes with valid nesting. Raw reference
bytes are preserved. `tools/hyperlink-oracle.py` permits repaired expectations
only for `style-link-crossed`, `link-style-crossed`, and four explicitly named
partial-selection variants of `style-link-transitions`. Its independent HTML
parser verifies equal per-character CSS/link projections. All other HTML
fixtures compare unmodified raw reference bytes, except for the five URI
corrections below. Large output uses one outer
HTML wrapper rather than exposing Ghostty's internal page divisions.

The pinned formatter also converts each UTF8 URI byte into an HTML numeric
entity: `/é` becomes `/&#195;&#169;`, whose parsed destination is `/Ã©`.
Hotaru escapes valid UTF8 as Unicode scalars and preserves undecodable bytes as
`%XX`, leaving existing percent escapes intact. Unicode controls are encoded
as their UTF8 percent escapes because HTML remaps numeric C1 entities (for
example `&#128;` becomes a euro sign). The named `raw-uri`, `unicode-uri`,
`unicode-uri-percent`, `invalid-uri-truncated` and `unicode-uri-control` cases
retain the raw oracle HTML and explicitly corrected expectations. Independent
Python decoding verifies the expected destinations; all VT URI bytes remain
unchanged. Parser fixtures also verify the unusual upstream treatment of
empty option segments through exact explicit IDs in VT output, rather than
relying only on URI equality.
An independent review additionally checked 529 actual Rust outputs using
Python's HTML parser and a separate URI projection, including deterministic
mixtures of valid Unicode, malformed UTF8, controls and existing percent escapes;
see `upstream/formatter-uri-audit.json` and its reproducible driver scripts.

This is partial subsystem coverage. Search across scrollback, selection gesture
policies, configurable copy codepoint maps, hyperlink hover/activation policy,
full terminal state serialization and C API compatibility are separate work.
No broad performance acceptance is inferred from these correctness fixtures.

The serial diagnostic in `upstream/hyperlink-performance-verified.json` uses
four deterministic approximately 2 MiB streams, a pinned CPU, zero history,
native optimized builds, three warmups and twenty measured iterations per
engine. Full grid/effects, every visible URI and exact content VT transitions
match before timing. The same ASCII or dense wide-Unicode payload is run with
no link, then with one persistent 2,031-byte URI. Hotaru/Ghostty feed ratios are
1.383 / 0.060 for ASCII without/with the link and 4.715 / 0.407 for wide Unicode.
The link path improves on this reference workload, but unlinked wide text
remains substantially slower. These patterns differ from the earlier four
reviewed corpora and do not establish broad performance parity. Source, tool,
binary and build hashes are recorded, and checked unchanged across timing.

## Initial audit at checkpoint 1342344

The following audit and design were recorded before the implementation above;
statements about the then-missing copy features describe that checkpoint.

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
