# Uniform cell owner: isolated result

The uniform pointer owner passes the ownership checks and removes most of the
safe enum's ordinary printing overhead. It is **not accepted for production**:
the measured ASCII history feed is still 18% slower and cold history reading is
16% slower. Production source and its global allocator are unchanged.

This implements the previously reviewed
[owner proposal](history-cell-owner-proposal.md) in
`/home/callen/work/hotaru-page-owner-isolated`, locally and on `wx-workstation`.
The proposal is retained as the historical, pre-implementation design.
The [reviewable patch](../upstream/page-owner/owner-hoisted.patch) applies to the
same frozen baseline as the [safe prototype](history-page-prototype.md), before
later saved-mode and REP changes. Reconstructing that baseline plus the patch
matches every source hash in the final build manifest.

## Ownership proof and validation

`CellBuffer` caches an aligned pointer, initialized length, original logical
capacity and optional immutable page owner. It remains 24 bytes; `Row` remains
112 bytes on the measured x86-64 target. Cell remains eight bytes of POD integer
fields. Vector length and actual capacity are checked before `ManuallyDrop`.

Only the no-page variant reconstructs the original global `Vec<Cell>` raw
allocation, with its exact type, alignment, length and capacity. The page
variant retains an immutable mapping through `Arc`; its interior pointer is
never passed to a deallocator. Shared slices borrow their owner. Mutable access
copies mapped cells while the old owner is still alive, then exposes the unique
vector. Manual clone copies vectors and shares immutable pages. Resize first
installs an aligned empty sentinel and operates on an ordinary local vector;
growth or validation unwinding therefore drops that vector exactly once.

The unsafe boundary is four small raw-vector/raw-slice blocks and explicit
`Send`/`Sync` implementations in the private cells module. The implementation
has no mutable mapped references, remapping, interior cell mutation, integer
pointer reconstruction or escaping borrowed lifetimes. An independent source
review by the upstream/protocol agent found no concrete ownership, aliasing,
drop or trait-safety defect. That is a code review, not a safety guarantee.

All **111 core tests** and strict all-target Clippy pass. The six native owner
tests exercise real mappings, range/layout bounds, COW, actual vector capacity,
repeated growth/shrink/swap/clone/thread drop and panic cleanup. The same owner,
raw-slice, COW and destructor code also passes five tests under each of Miri's
default and Tree Borrows models with strict provenance. Miri substitutes an
immutable boxed cell slice for the OS mapping; the large OS-mapping boundary
test is deliberately ignored there and passes natively. Miri does not validate
the Linux mapping syscalls. Exact commands and logs are in
[validation.json](../upstream/page-owner/validation.json).

## Paired measurements

The final variant also hoists one mutable cell-slice borrow per positive-width
row span and avoids reconstructing a vector for a same-length resize. This
changes neither metadata ordering nor logical capacity. Source SHA-256 is
`2f8ad97e7e82766b99655fdc591658ad45a8ba40c59fb879bf9934a75a30f774`;
binary SHA-256 is
`7a08e8c182ed46704edace87d25a241b734eaa15bed9175982338ffff4eaa254`.
The [final report](../upstream/page-owner/hoisted-report.json) contains source
manifests, build flags, commands, load snapshots and all raw samples.

Five fresh-process samples alternate candidate/baseline order on CPU 8 under
the workstation lock. There is no allocator trim or machine-setting change.
Every common workload and the 50 MB stress case passes the full normalized
content digest and equal-retention gate. The stress case retains 8,291 history
rows in both versions. Logical charges, stale capacities and row layout match.

| Workload | Baseline cold RSS | Owner cold RSS | Baseline cold read | Owner cold read |
| --- | ---: | ---: | ---: | ---: |
| ASCII, 7,977 history rows | 11,468 KiB | 4,160 KiB | 2.248 ms | 2.613 ms |
| RGB styles, 7,977 rows | 18,512 KiB | 12,348 KiB | 4.269 ms | 4.813 ms |
| Wide/combining, 7,977 rows | 13,640 KiB | 6,284 KiB | 3.389 ms | 3.749 ms |
| Four long URIs/row, 2,977 rows | 14,524 KiB | 12,048 KiB | 1.798 ms | 1.934 ms |
| 50 MB URI stress, 8,291 rows | 49,116 KiB | 41,356 KiB | 5.382 ms | 5.776 ms |

ASCII history feed grows from 1.905 to 2.249 ms; compression grows from 1.926
to 2.180 ms. Zero-history ASCII is 6.038 versus 6.088 ms (+0.8%), and mixed
Unicode is 65.441 versus 65.636 ms (+0.3%). These near-equal zero-history results
do not establish a throughput win. Initial-owner results are preserved
separately with their exact source files and build manifest.

## Sealing and restoration attribution

Separate feature-gated clocks measure phases; they are diagnostic measurements,
not acceptance timings. The [diagnostic report](../upstream/page-owner/profile-report.json),
[instrumentation patch](../upstream/page-owner/diagnostic-hooks.patch) and
[driver](../upstream/page-owner/profile-pages.py) preserve exact source and binary
hashes. Decode counters exclude the later validation digest.

For the ASCII case, 124 blocks map 7,618,560 cell bytes. Median total sealing is
1.170 ms: 0.858 ms copies cells and faults the new mapping, 0.066 ms creates
mappings, and 0.153 ms makes them read-only. The residual includes row-owner
replacement and instrumentation. These phase times cannot be subtracted
directly from an uninstrumented baseline to claim a causal speedup.

Uninstrumented binaries traced only for syscall counts show exactly 124 extra
calls each to `mmap`, `mprotect` and `munmap`: baseline counts are 14/5/3 and
owner counts 138/129/127. Trace timings are not used. This demonstrates the
per-sealed-block syscall cost without inventing a line-per-map explanation.

Cold ASCII decode restores 123 blocks. Median raw-buffer allocation plus LZ4 is
0.273 ms, link dictionaries 0.003 ms and row reconstruction 2.188 ms, versus
2.477 ms total decoding. Unicode row reconstruction is 3.177 of 3.631 ms.
URI rows spend 0.960 of 1.698 ms reconstructing rows, 0.442 ms in raw allocation
plus LZ4 and 0.279 ms in dictionaries. Row reconstruction includes parsing,
validation, allocation and cell writes; this measurement does not isolate any
one of those costs.

The next smaller experiment is direct bounded decode into a private immutable
block page, preserving every codec bound and publishing only after validation.
Active page leases could later eliminate sealing copies, but introduce a
different disjoint-mutation proof and need separate review before implementation.
