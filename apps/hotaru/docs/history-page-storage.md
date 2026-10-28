# Proposed page-owned history storage

This is a design for review, not an implemented memory improvement. The
[byte-budget measurements](history-budget-memory.md) establish the current RSS
gap. Product storage and the global allocator remain unchanged.

## Why the ownership boundary matters

Pinned Ghostty allocates its page backing directly with anonymous `mmap`.
Compression retains the virtual mapping, stores an encoded copy and discards
the physical pages; restoration recommits the reserved mapping. See
[page allocation](https://github.com/ghostty-org/ghostty/blob/6ba144158577d033ccb6ed4b574e4013957c9161/src/terminal/page.zig)
and [node storage transitions](https://github.com/ghostty-org/ghostty/blob/6ba144158577d033ccb6ed4b574e4013957c9161/src/terminal/PageList.zig).
This gives it a reclamation boundary the system allocator cannot infer from
Hotaru's separate row, cell, style and string allocations.

Hotaru's existing `Arc<Block>` captures already give a suitable logical
boundary: at most 64 rows, with an immutable compressed block or resident row
handles. Captures can outlive the terminal. Eviction changes the live prefix;
compression replaces the live block; neither may invalidate a reader's storage.
The physical allocation should follow that boundary.

## First implementation boundary

Keep the current mutable active-row representation and print hot path. Add a
sealed resident-page representation for historical blocks. At sealing, pack
the cells into one private anonymous mapping and publish immutable ownership.
Use checked layout arithmetic and a row descriptor containing only bounded
offsets, counts and logical recycling capacities. Every row's cell interval is
within the mapping, aligned for `Cell`, and disjoint from every other interval.
Neither raw pointers nor references into a builder escape before it is sealed.

An owned row view holds an `Arc` to the page and its validated descriptor.
`Row::cells()` continues to return a slice with the view's lifetime. Access to a
frozen row's mutable representation first copies into an ordinary owned row,
preserving the cached logical capacity charge and stale metadata. Active
registration transfers at this COW replacement exactly as it does today;
captured rows never signal changes to the live budget.

Compression publishes a separate immutable encoded block, then releases the
terminal's old resident-page ownership. The mapping is unmapped only when its
last owner disappears. A held capture deliberately keeps it resident. There is
no `madvise(DONTNEED)` on shared mappings and no reserved mapping that a worker
may repopulate while another reader holds references. This uses more explicit
ownership than Ghostty's live-node restoration model and fits Hotaru's worker
API. A later unique-owner mapping reuse optimization would need separate review.

Anonymous mappings can use the safe
[`memmap2::MmapMut::map_anon`](https://docs.rs/memmap2/0.9.12/memmap2/struct.MmapMut.html#method.map_anon)
constructor. After filling, a builder can consume the mutable handle into an
immutable mapping. `Cell` is eight bytes of integer fields with no padding;
checked conversions such as
[`bytemuck::try_cast_slice`](https://docs.rs/bytemuck/1.25.2/bytemuck/fn.try_cast_slice.html)
can enforce size and alignment without unchecked casts in the core. These are
candidate dependencies only. Their exact versions, generated trait assumptions,
platform behavior and implementation still require review before adoption.

Sealing must not accumulate abandoned heap cells proportional to history. Keep
a bounded recycling pool for the previous owned cell buffers, or move one
bounded tail's buffers into a reusable builder before adding further history.
Do not fill the whole history with heap rows first and repack only when idle;
that would preserve the same allocator RSS problem. Mapping failures keep the
existing valid resident representation and report a measurable fallback.

## Metadata and restoration

Cell-only pages are an incremental experiment, not the final memory claim.
They can recover most of the plain-ASCII backing, but not the several MiB of URI
storage in the linked workload. The subsequent sealed layout must also own:

- Styles as validated packed integer records. Rust enum layouts are not byte
  cast. `Row::style()` already returns a value, so it can decode a record while
  active rows keep their current interned `Style` vector.
- Grapheme descriptors and UTF-8 bytes. Validate scalar count, byte length and
  UTF-8 before publishing a slice. Preserve the maximum 65-scalar/260-byte
  cluster and all stale capacity accounting.
- Hyperlink dictionary strings and row runs. Shared byte handles contain a
  page owner and a validated range; no page owns a handle back to itself.
  The run table and link objects can initially remain small heap allocations.
  The dictionary's URI and explicit-ID payloads must move into page storage to
  reclaim the large allocations. Public URI/ID access remains byte-oriented.

The current archive stays an explicit portable encoding. A physical page is
not a raw serialized Rust object. Decoding validates the existing raw-size,
row-count, column, style, link and total-allocation bounds before allocating the
destination page. Decode directly into one destination mapping and bounded
metadata tables, avoiding a fresh cell/style allocation per row. Iteration
still owns at most one restored block unless a caller deliberately retains rows.
Cancellation remains observable between bounded blocks; oversized rows follow
the existing resident policy instead of silently increasing decode limits.

Logical capacity and physical allocation must be separate. A compact frozen
page preserves the original row's conservative logical charge and recycle
layout, so compression, capture or chunking cannot change which history rows
survive. Physical retention estimates count each mapping once per owning block,
including a shared evicted prefix. Held captures report all the pages they keep
alive. No estimate is labeled RSS.

## Evidence required before integration

First compare cells-only sealed pages against the frozen current adapter with
identical full-content hashes, budgets, captures and corpus inputs. Measure feed,
sealing, compression, cold restore and RSS separately, including long-URI data
that the first stage cannot improve fully. Reject a throughput regression that
merely trades the measured parse bottleneck for copying at every line.

Run the existing 12,000-operation compression/residency invariant, byte-chunk
retention tests, reflow and hyperlink oracle fixtures with the new backing.
Add captures held across partial eviction, compression, reverse scrolling,
row mutation, resize and teardown; concurrent worker reads must remain stable.
Test invalid layouts, overflow, unaligned offsets, maximum-width rows, allocation
failure, cancellation and exact logical capacity preservation. A small typed
storage module should receive Miri tests where supported and guard-page tests
for every actual raw access. Full native and Nix validation follows the core
proof and same-host measurements, not before.
