# History budget audit after owned content capture

This is a read-only follow-up for the next memory phase. It does not change
this checkpoint's limits or allocator. Source reviewed: Hotaru's owned content
capture implementation and pinned Ghostty
`6ba144158577d033ccb6ed4b574e4013957c9161`.

## Reference behavior

Ghostty's `src/config/Config.zig:1366` documents a default
`scrollback-limit-bytes` of 50,000,000 bytes, including active terminal pages.
The active screen is always allowed to fit, and remaining space is available
to history. The budget measures uncompressed logical page size; compression
does not permit additional history. The independent line limit defaults to
unlimited. `src/terminal/PageList.zig:596` and `:618` enforce both dimensions
at page granularity. These are logical terminal-page budgets, not exact RSS
limits or a process-global ceiling.

## Current gap

Hotaru currently limits history by configured physical rows and by
`8_000_000 / columns` in `State::new` and resize. The native default is 10,000
rows. This caps cell count but does not cap total retained bytes. Neither
`History::push_back` nor eviction has a byte budget.

Styles, graphemes and hyperlinks can make a row substantially larger than its
cells. For example, 8 million cells each bearing a distinct approximately
2 KiB OSC 8 URI can retain approximately 16 GB of live URI payload alone.
This is a source-derived upper-bound illustration, not a measured workload.
The parser's 2,048-byte ordinary OSC body cap bounds one link, not the sum of
links. Row-local hyperlink tables can also retain superseded entries until
compaction, and shrinking a row can leave unused styles/table capacity.
Archive size is therefore not an accurate measure of its live heap ownership.

The 4 MiB archive limit bounds individual compression/decode work. A row whose
conservative archive bound exceeds it intentionally remains resident, so that
limit does not close the total-memory gap. `resident_size_bound` describes
serializable data, while `compressed_bytes` describes codec data and recycle
sidecars. Neither includes all allocations, spare capacity, block headers,
row-local obsolete link entries, or outstanding content captures. They must
continue to be labeled distinctly from retained heap bytes and RSS.

## Proposed follow-up

Add an explicit uncompressed logical-byte limit alongside the line limit,
with a default consistent with Ghostty's 50 MB setting. Preserve enough space
for active content; prune oldest history when either constraint is exceeded.
Compression must retain the same logical charge and must never increase the
permitted history depth. Store charge and recycle metadata outside compressed
payloads so pruning stays allocation-free and decode-free.

Define the charge from owned data, not serialized length: cell/style/grapheme
storage, URI and identity payloads, sparse link runs and tables, and bounded
container overhead. Cache row/block charges so the hot scroll path does not
walk all history. Bound or compact obsolete per-row metadata before charging
new rows. A conservative charge that counts shared URI data more than once
is acceptable initially if documented; claiming exact unique-byte accounting
would require explicit shared-allocation ownership tracking. Never obtain an
apparently lower charge by omitting metadata from the accounting.

Content captures deliberately preserve data after terminal eviction. A
terminal's live budget cannot reclaim immutable storage still owned by a
consumer. Expose a separate capture-retention estimate, counting complete
shared blocks (including an otherwise evicted prefix) and active row storage.
Do not sum overlapping capture estimates and call the result unique memory.
The native latest-job worker already bounds pending copy jobs and cancels
superseded work; the next phase should verify that completed messages,
clipboard output buffers and any future history-search captures obey matching
ownership limits. Arbitrary library clients can retain unlimited snapshots,
so this limitation belongs in the public ownership documentation.

Required regressions: large unique URI streams; row-table churn and shrink;
compression on/off retaining identical history under a byte limit; oversized
rows forcing old-history eviction without truncating active content; eviction
without decode; captures held through byte pruning and teardown; and bounded
copy/search retention under rapid cancellation. Compare both retained logical
bytes and same-host process RSS, with independent labels and no allocator trim.
