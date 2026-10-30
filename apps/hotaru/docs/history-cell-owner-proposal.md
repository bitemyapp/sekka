# Uniform cell access: proposed unsafe boundary

This is a proposal for review only. The safe enum prototype remains isolated
and is rejected for integration because of its measured regressions. No pointer
owner described here has been implemented.

The smaller next experiment is a private, uniformly accessed cell buffer:

```rust
struct CellBuffer {
    ptr: NonNull<Cell>,
    len: u16,
    capacity: u16,
    owner: Option<Arc<CellPage>>,
}
```

On the measured 64-bit target this should occupy 24 bytes, the same as `Vec`;
verify it with a compile-time assertion and charge any target-specific growth
honestly. Existing maximum width and vector-growth bounds fit the two counters.
Every conversion checks actual length and capacity before taking ownership;
there is no unchecked truncation. The fields and raw constructors stay private
to the storage module.

`owner == None` owns exactly one allocation originally created by a standard
`Vec<Cell>`. The cached pointer, length and capacity describe that allocation.
An empty buffer uses an aligned dangling pointer, zero length and zero capacity.
`owner == Some(page)` instead owns an immutable mapping; the pointer and length
come from a previously checked typed sub-slice. Its capacity is the remembered
logical capacity for later materialization, never a deallocation instruction.

## Auditable operations

- Construction from a vector checks all bounds before suppressing the vector's
  destructor. After that transfer there is exactly one owner of its allocation.
- Shared slice construction uses the cached pointer and length directly. The
  lifetime is tied to `&self`; the allocation or page owner cannot disappear
  while the returned slice is borrowed. No alignment or representation branch
  is repeated on this path.
- Mutable slice construction first checks `owner`. A mapped buffer copies into
  a new owned vector with the original capacity, installs that owner, then
  releases its page reference. It never returns a mutable reference into shared
  mapped cells. Ordinary owned writes need only the cold-owner test.
- Clone allocates and copies owned vectors, preserving capacity. Mapped clone
  increments the page reference and copies the validated range. A derived
  bitwise `Clone` would be invalid and must never replace this implementation.
- Resize replaces `self` with a valid empty sentinel before reconstructing an
  owned vector in a local RAII variable. Resizing or a later validation panic
  therefore drops that local allocation exactly once and leaves a valid empty
  owner. Reinstallation occurs only after successful validation; no pointer is
  cached across a reallocating vector operation.
- Drop reconstructs and drops a vector only for `owner == None`. A mapped
  buffer only drops its page reference. Its interior pointer is never passed
  to the global allocator.

The unsafe code would be confined to slice construction, transfer/recovery of
the owned vector's raw parts and explicit `Send`/`Sync` implementations. `Cell`
is Copy/Pod and cannot have drop glue; the map is immutable after publication.
An owned allocation is unique, shared references do not mutate it, and pointer
updates require exclusive `&mut self`. These invariants justify thread transfer
and shared reads. No arbitrary-address constructor, remapping method, interior
mutability or borrowed lifetime extension is exposed.

The alternative is to keep mutable `Row` and its `Vec<Cell>` entirely separate
from a frozen-history view. That removes representation dispatch from printing,
but needs a deliberate public row-handle boundary for render/copy/search and
must avoid allocating wrappers for every visible row on every frame. It is a
larger architectural experiment if the uniform-owner approach still regresses.

## Validation and benchmark plan

Before measuring, test raw-owner creation, empty/zero-capacity drop, growth,
shrink, swaps, clone, panic recovery, mapped COW and repeated teardown. Miri
should cover the owner logic with heap-backed immutable page owners where OS
mapping support is unavailable. Real anonymous mappings need separate lifetime,
thread, boundary and guard-page tests. Preserve the existing 107-test suite,
including captured rows across eviction/reflow and exact chunk-budget retention.

Use the same frozen baseline and corpora as the safe prototype. Compare
zero-history parse, history ingestion, mapping/sealing, compression and cold
read separately; keep full-content and capacity/charge equality as prerequisites.
Report current RSS and held-capture RSS independently. Do not claim that a
smaller unsafe wrapper fixes cold restore: the decoder still allocates rows on
the heap, so direct bounded-page decoding remains a separate possible phase.
