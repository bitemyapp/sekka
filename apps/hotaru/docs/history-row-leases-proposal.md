# Active cell leases: later design for review

This is a read-only design study, independently reviewed by the page-arena
subagent. It has not been implemented or approved for unsafe implementation.
The immediate next experiment is [immutable direct decode](history-direct-decode-proposal.md).
Active leases would address the separate measured sealing-copy cost.

## Ownership model

Give each `CellBuffer` one exclusive, validated slot lease from a bounded page
arena. Existing `RowRef` is the sharing boundary. Cloning a `RowRef` shares its
row and lease; cloning `Row` for `Arc::make_mut` allocates a different slot and
copies cells. Different cell buffers may own disjoint slots in the same page;
they never own overlapping slots. An `Arc<Lease>` per row is unnecessary.

The current 24-byte pointer/length/logical-capacity/page-owner representation
could remain. Physical slot capacity belongs to the page size class and is
separate from the validated logical capacity used by the budget, archive and
recycling semantics. Check actual layout and charge any growth honestly.
Resize across a slot boundary should allocate and initialize a replacement
before releasing the old lease; retain the original on allocation failure.

A terminal-owned pool maintains bounded allocation candidates and an explicitly
bounded empty-page cache. Pages hold a weak connection to their pool; buffers
hold strong page owners. This avoids a reference cycle and lets captures outlive
terminal destruction. Detached `Row::clone` must still allocate safely, using
its page's live pool or a standalone bounded allocation if that pool is gone.
Decoder workers should own their restore allocation policy, not mutate the live
terminal's accounting or require the terminal to outlive the capture.

## Unsafe boundary requiring review

After any lease is published, the page owner exposes no mapping-wide cell or
byte slice, mutable mapping accessor or content-scanning `Debug`. Slot pointers
derive from the owned allocation's raw base pointer, never integer addresses.
References cover only one lease's initialized prefix and borrow its buffer.
An exclusive buffer can mutably borrow that prefix because all sibling leases
are disjoint and shared rows go through ordinary row COW first.

The page remains writable: immutable historical rows and mutable active rows
may occupy separate slots in the same mapping. No whole-map `mprotect` is
permitted while mutable leases exist. Backing storage must express interior
mutability correctly. The Miri substitute should use raw or
`UnsafeCell<MaybeUninit<Cell>>` storage and the same ownership machinery; the
current immutable `Box<[Cell]>` substitute is inappropriate for this design.
`UnsafeCell` does not permit mutation while an ordinary shared slice to the
same cells remains alive.

Initialize every newly published cell before increasing visible length. Spare
capacity is never exposed as initialized data. Dirty-prefix reuse is valid only
while the row keeps its own exclusive lease. The existing active-budget transfer
must happen at COW replacement, not when an old captured row finally drops.

Slot allocation and release require synchronized metadata. Cell access itself
does not take an allocator lock. Explicit `Send`/`Sync` proofs must cover both
the disjoint-region rule and allocation/release ordering. The concrete pool
catalogue, lock order and panic/poison handling remain design work before code;
they must avoid acquiring pool and page locks in opposite orders.

## Sealing, compression and reclamation

History block boundaries become logical only. Sealing no longer copies cells
or changes mapping protection. Compression publishes an independently owned
encoded block and releases its old row owners. Reverse scrolling can reuse a
uniquely owned row lease; captured rows trigger normal COW.

Only `CellBuffer::drop` releases its slot, exactly once while retaining the page
owner. Safe slices cannot outlive that owner. Free-list pointers must not be
written into cell storage until the slot is exclusively free. Small size-class
slots should divide an OS page; large slots occupy whole OS pages. This avoids
rounding a reclamation request across an occupied neighboring slot.

Whole-chunk recycling alone is insufficient: one captured row could pin an
entire chunk. Useful RSS reclamation needs live-slot occupancy per OS page,
including slots reserved by allocations still under construction. Batch
`MADV_DONTNEED` only over completely free page runs. Recheck occupancy under the
page allocation lock and exclude new reservations until discard completes.
Next reuse initializes its visible prefix again. Unmap only after all leases
are gone and the mapping has left the pool/cache.

Report mapped virtual bytes, occupied-page bytes, bounded free-cache bytes and
capture-retained bytes separately. A bounded empty cache does not bound
fragmentation from partially occupied chunks. Arbitrarily many long-lived
captures can retain arbitrarily many COW versions; no allocator can promise a
fixed total bound independent of those external owners.

## Acceptance prerequisites

Before implementation, review the exact raw owner, slot layout, initialization,
failure cleanup, allocation catalogue and lock-order design. Miri should cover
concurrent reads of one lease and writes to another, same-row COW, resize/reuse,
last release on workers, teardown with captures, panic/allocation failure,
boundary slots and allocation competing with discard. Exercise real Linux
mapping and discard separately. Existing exact content, logical-capacity,
compression, reflow, search and chunk-budget invariants must still pass.

Measure zero-history printing, ingestion, sealing, compression, direct decode,
fragmented captures and cold RSS independently. The design is justified only if
it removes measured costs without weakening snapshot ownership or concealing
physical overhead in logical budget accounting.
