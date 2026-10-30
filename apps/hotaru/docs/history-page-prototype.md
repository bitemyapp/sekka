# Cells-only mapped history prototype

The isolated prototype demonstrates real RSS reclamation with owned immutable
history pages. It is **not ready to integrate**: history ingestion and cold
reads regress. The production core and allocator are unchanged.

Local and workstation source lives at `/home/callen/work/hotaru-page-cells-isolated`;
the matched baseline is `/home/callen/work/hotaru-page-cells-baseline`. Both were
frozen before the later saved-mode/REP protocol changes. Exact source manifests,
compiler flags, binary hashes, commands and raw samples are in
[`final-report.json`](../upstream/page-cells/final-report.json). Final candidate
source SHA-256 is
`d499ac0df01842a89572db2e98c2f436d2fd5ddb77094c26684b8bf76817c554`.
The reviewable implementation is
[`prototype.patch`](../upstream/page-cells/prototype.patch); it has not been
applied to the shared application source.

## Ownership and bounds

`CellBuffer` holds either the existing `Vec<Cell>` or an owned page reference,
validated cell range and remembered original capacity. A history block seals
when the next block starts, packing at most 64 rows into one anonymous mapping.
Mapping occurs per block, not per ordinary line. Only the unfinished tail keeps
heap cell buffers in the live history. Sealing releases the old buffers in a
bounded batch; there is no growing heap-buffer pool. A capture taken before
sealing can intentionally keep its original rows alive.

The builder owns a mutable anonymous mapping, copies typed cells, then consumes
it into a read-only mapping before publishing it. A row view owns the mapping
through `Arc`. Editing materializes an ordinary cell vector with the original
capacity; another capture's cells never change. Compression replaces the live
block and drops its ownership of the mapped cells. The last owner unmaps the
page. There is no decommit, remap or write through shared mappings.

Cell is still eight bytes of integer fields with no padding. The derived
`bytemuck::Pod` implementation verifies those prerequisites; checked casts
validate slice alignment and whole-element length. Mappings are bounded to
4 MiB by the builder, and the existing block policy normally bounds cell data
much more tightly. Invalid or reversed ranges and insufficient remembered
capacity are rejected before constructing a view. A mapping failure leaves
the valid heap representation intact.

The enum fits the existing layout: `Row` remains 112 bytes on the measured
x86-64 target. Logical charges, stale capacities, retained widths and archive
encoding stay unchanged. Partial eviction retains the mapped prefix in the
block's cached storage estimate; captures still avoid scanning individual rows.
These estimates describe requested retained storage, not exact kernel page
rounding or process RSS. A production version should report mapping granularity
and mapping-failure fallback counts explicitly.

The exact `memmap2` 0.9.12, `bytemuck` 1.25.2 and derive 1.12.1 sources were read
before use. The reviewed functions, crate archives and file hashes are recorded
in [`dependency-review.json`](../upstream/page-cells/dependency-review.json).
No new unsafe block or manual unsafe trait implementation is added to the core.
This review and measurement cover x86-64 Linux; no additional platform result
or Miri result is claimed.

## Validation and measured tradeoffs

All **107 core tests** and strict all-target Clippy pass. This includes the
12,000-operation resident/compressed invariant, chunk-dependent budget tests,
reflow, Unicode, hyperlink and search fixtures, concurrent captures, and new
tests for typed ranges, COW, layout limits, capacity/charge/codec preservation
and retained mapped prefixes. The log and exact source identity are in
[`validation.json`](../upstream/page-cells/validation.json).

Measurements use fresh processes, five samples, alternating baseline/candidate
order, CPU 8 and the shared workstation lock. Unlike the earlier audit, host
load and a process CPU snapshot are recorded for every case. There is no
allocator trim or host-setting change. All common cases and the 50 MB URI stress
case retain identical contents, checked by the complete normalized digest.
The stress case retains 8,291 history rows in both versions; there is no header
growth or near-budget retention difference on this target.

| Workload | Baseline cold RSS | Prototype cold RSS | Baseline cold read | Prototype cold read |
| --- | ---: | ---: | ---: | ---: |
| ASCII, 7,977 history rows | 11,532 KiB | 4,168 KiB | 2.239 ms | 2.622 ms |
| RGB styles, 7,977 rows | 18,512 KiB | 12,420 KiB | 4.262 ms | 4.582 ms |
| Wide/combining, 7,977 rows | 13,648 KiB | 6,224 KiB | 3.395 ms | 4.127 ms |
| Four long URIs/row, 2,977 rows | 14,460 KiB | 12,004 KiB | 1.785 ms | 1.902 ms |
| 50 MB URI stress, 8,291 rows | 49,116 KiB | 41,364 KiB | 5.362 ms | 5.702 ms |

The matched cold configuration's ASCII feed grows from 1.911 to 2.279 ms
(19.3%) and compression from 1.941 to 2.250 ms (15.9%). Cold reads also regress.
The zero-history ASCII parse check grows from 6.043 to 6.187 ms (2.4%). The final
zero-history mixed-Unicode sample is slightly faster, but an earlier paired
run was 4.8% slower, so it does not establish a throughput improvement.

A first implementation put materialization code on the ordinary mutable-access
path and regressed zero-history Unicode by 29%. Moving that slow allocation
path out of line removed most of the cost. Both initial and intermediate
reports are retained, with the exact initial source variant and intermediate
patch. None of those preliminary results is presented as final acceptance.

Held resident captures correctly keep their mapped pages resident: the final
ASCII held-resident case uses 11,628 KiB, while a capture taken after compression
uses 4,172 KiB. Both survive owner mutation and destruction with exact contents.
The remaining URI/style RSS gap is expected because only cells moved into maps;
metadata still uses the system allocator.

## Next decision

Retain this as evidence for the ownership design, and reject the current patch
for production integration. Profile the per-block mapping/protection work and
the added cell-access dispatch before changing representation further. Direct
decode into bounded page storage could remove per-row allocations during cold
restore, but requires a separate validated builder design. Moving URI and
grapheme bytes into pages remains necessary for the metadata-heavy RSS gap.
Neither optimization should weaken immutable capture lifetimes or logical
budget invariance to recover benchmark speed.
