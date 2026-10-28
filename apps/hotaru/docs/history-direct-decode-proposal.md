# Direct immutable-page history decoding: review proposal

This is a proposed isolated follow-up to the measured
[uniform owner](history-page-owner.md). No decoder change is implemented yet.
It targets the per-row cell allocation during cold restoration, while retaining
the existing immutable page publication and copy-on-write invariants. It does
not introduce a mutable active arena or change the archive format.

## Bounded layout pass

Keep LZ4 output in the existing bounded raw buffer (maximum 4 MiB) and decode
the block hyperlink dictionary with the existing 64 MiB allocation budget.
Before allocating a cell mapping, scan the remaining row framing without
allocating per-row cell vectors. Share a checked header parser with the actual
decoder so that width, counts, capacity ranges, flags and minimum payload bounds
have one implementation.

The layout scan checks every slice advance: header, fixed-size styles/cells,
each length-prefixed grapheme, table indices and link runs. It records at most
64 encoded row ranges and their cell widths. All arithmetic is checked. It
requires exactly the declared row count and no trailing bytes. The sum of cell
widths must be positive and at most 64 times `MAX_COLS`, hence at most 4 MiB of
cell storage. Capacity hints never determine map length; original validated
capacities remain separate logical metadata for later COW/recycling.

This first pass validates framing and sizing, not semantic cell/style/link
correctness. The second pass retains every current semantic check and allocation
budget subtraction. Malformed input may cause one bounded mapping allocation
before a later semantic rejection; it cannot publish an invalid row.

## Private construction and publication

Allocate one anonymous writable mapping of exactly the checked cell length.
A private `CellPageBuilder` owns it exclusively. Its checked bytemuck cast yields
initialized integer `Cell` storage; anonymous mappings start zeroed. Each decode
receives only its validated mutable row range. No range, row or pointer escapes
the builder before all rows have passed validation.

Refactor row decoding to produce private `DecodedRowParts`, containing owned
styles, graphemes, links, flags and retained capacities, with cell data supplied
by the caller. The decoder writes cells directly into the provided range and
validates wide-cell neighbors, grapheme bases, scalar values, style indices and
all existing bounds there. `DecodedRowParts` cannot be used as a public `Row`.
It becomes a `Row` only through a finalizer requiring a validated `CellBuffer`.

After the exact row count validates and input is exhausted, end all mutable
borrows and consume the builder into the existing read-only `CellPage`. Only
then create checked `CellBuffer` views and complete the rows. Set `dirty_end` to
the row width and leave charge caches detached from live-terminal accounting.
The page stays alive through each row owner; COW still materializes a vector
with the remembered capacity. Existing heap decoding remains available for
focused codec tests and as a semantic comparison path, using the same header
and metadata validation code.

Every rejection drops only owned locals: partial row metadata, dictionary,
builder mapping and raw buffer. Mapping/protection failure returns a decode
failure or retries the existing heap decoder from unchanged input with a fresh
budget; it must never reuse a partially depleted budget or partially published
rows. Prefer explicit heap fallback to changing successful capture behavior
under ordinary map pressure. Allocation failure injection must exercise this
path. No new unsafe code is necessary beyond the already reviewed cell owner;
the builder can use checked slices throughout.

## Validation and measurements

Compare heap and mapped decode for every existing codec fixture, all truncation
points, corrupt widths/capacities/counts/ranges and seeded valid rows. Check
exact cells, styles, graphemes, links, capacities, retained widths, cached
charges and dirty-prefix invariants. Exercise multirow page boundaries, one
maximum-width row, maximum 64-row storage, malformed late rows, map/protection
failure, clone/COW and independent worker drop. Run existing compression/history,
reflow, capture, search and chunk-budget stress tests. Miri uses the same builder
and owner API with heap backing; Linux mapping tests remain separate.

Preserve the measured frozen baseline and uniform-owner binary. First compare
only owner-with-heap-decode versus owner-with-direct-decode, with full-content
and equal-retention gates. Report cold RSS, temporary restored RSS, capture
retention, decode phase time and syscall counts separately. Keep five fresh
samples, alternating order, CPU 8 and the shared lock. Direct maps add one
mapping/protection/unmap group per decoded block; they may lose on small blocks.
Reject the experiment if added syscalls or a second framing pass outweigh the
allocation savings. A typed row arena and metadata-page layout are later,
separate experiments, not assumptions required to call this one successful.
