# Owned scrollback search

`hotaru_core::search::find` searches a `ContentSnapshot` on a worker. It owns
the exact capture in its result, so selections remain valid after compression,
eviction, reflow or terminal destruction. The PTY actor never decodes history
or searches text. Native Find uses these captures for whole-history search,
navigation, highlights and copying a complete match across viewport boundaries.

Matching uses KMP with ASCII-only case folding, including overlapping byte
matches. UTF-8 bytes and combining marks map to their owning cells; a match
inside a grapheme selects that complete cell. Wide-character endpoints include
both columns. Hard newlines remain searchable, soft wraps join, trailing spaces
and blank rows are trimmed, and interior blanks retain their actual coordinates.
Matches are returned newest first with exclusive end coordinates. The final
hard newline maps to the final emitted cell, matching the reference search API.

The implementation streams one owned row at a time, decoding at most one bounded
history block at once. It allocates a prefix table and coordinate ring bounded
by the 65,536-byte needle limit, plus retained matches. Delayed whitespace stores
one coordinate run per contiguous segment, without retaining decoded rows or
building a full-history text/byte map. A pathological soft-wrapped space run can
retain one small descriptor per physical row. The default result limit retains
the newest 10,000 matches and counts all matches; callers can request up to one
million, or zero for counts only. Truncation is explicit. Cancellation is checked
before each row/decode and every 256 emitted bytes, and discards partial results.

## Independent reference coverage

`tools/search-oracle.py` loads the pinned unchanged `libghostty-vt`, checks the
new C layouts against its ABI manifest and calls the actual search API. It
records 249 cases in `upstream/search-fixtures.json`: overlapping and ASCII-case
matches, Unicode case sensitivity, empty queries, hard/soft wraps, whitespace,
wide-character spacer heads, combining marks, emoji, history across pages,
primary/alternate screens, erasure, resize/reflow and deterministic mixed VT
streams. Rust tests compare all cases at input chunk sizes 1, 7 and 65,536.
The reference byte limit is explicitly set to the application's 50,000,000
bytes: the C library defaults to only 10,000 bytes. Large fixtures retain 3,000
rows and include a needle spanning every hard-line boundary across pages.

Three named cases deliberately correct an upstream coordinate bug. Ghostty's
`PageFormatter.appendBlankPoints` emits backwards coordinates for a forward run
of materialized spaces, so a match containing only those spaces can have reversed
endpoints. The fixture retains each raw selection and a separately asserted
forward coordinate correction. All other selections compare directly after
converting the reference's inclusive endpoint to the owning cell's exclusive end.

```sh
python3 tools/search-oracle.py \
  --library "$GHOSTTY_REFERENCE/zig-out/lib/libghostty-vt.so" \
  --output upstream/search-fixtures.json
cargo test --locked -p hotaru-core --test search
```

Additional tests retain captures through compression, thousands of evictions,
reflow and teardown before searching on another thread; check newest-result
retention and complete counts; and exercise cancellation and resource limits.
These checks do not establish performance parity or complete live-search
equivalence with Ghostty.

## Live results and row identity

Each history has an owned identity token and a monotonically advancing origin.
Captures retain both. Removing oldest rows advances the origin; reset, clear,
reflow and a newly initialized alternate screen change identity. A retained
token cannot compare equal to a newly allocated identity after its old terminal
is destroyed. Returning to the existing primary screen preserves its identity.

`Results::resolve_match` validates a result using only this metadata, without
decoding rows or comparing text on the PTY actor. Historical matches rebase as
old rows disappear. Matches touching the captured active screen require the
same generation. Deferred spaces and newlines can depend on later active text
even when their reported coordinates are historical; a compact result-prefix
bound also invalidates those matches when content changes. Newline coordinates
can depend on text preceding their reported endpoints, so any multiline query
requires a fresh capture after pruning. This conservative rule avoids selecting
a different piece of history when the coordinate's earlier context is gone.

Ten permanent position tests cover append/prune, active mutation, reset,
alternate-screen identity, reflow, projection, whitespace dependencies and
bounded result retention. An independent seeded review compared every accepted
old match against a fresh search through mutations and eviction. Exact source
hashes, commands and retained reproducers are recorded in
`upstream/search-anchor-validation.json`; this is a targeted invariant check,
not an exhaustive proof of all terminal behavior.

The application keeps at most primary and alternate result caches for its
active pane. Changing the query or pane cancels old work and releases those
caches. One worker has one active and one pending search. A 120 ms refresh timer
does not restart on every output frame, so continuous output still permits
completed results. Navigation and capture have separate coalescing actor
mailboxes that remain available when the ordinary input queue is full. Query
tokens and a separate navigation revision reject obsolete completions.

The font worker projects matches onto its exact immutable render snapshot,
including multiline and wide cells. Copying a selected result reads the owned
capture, not merely the visible rows. Pointer selection clears that search-copy
state. A real private Wayland session checks navigation through 120 historical
matches and copying the selected text; the script is
`desktop/aizome/apps/hotaru/tests/wayland/search.py`. Its result applies only to
the named tested binary, not automatically to later source or package builds.

## Initial same-host timing

`tools/search-benchmark.py` and the Rust `search` example compare fresh static
searches of identical resident history, checking every match coordinate and
both history counts before accepting timings. The initial record is
`upstream/search-benchmark-initial.json`: CPU 8 on wx-workstation, native CPU,
thin LTO, System allocator, 20 repeated searches per case. Ratios below are
Rust/Ghostty median latency, so values below one are faster:

| Retained text lines | ASCII needle | Unicode needle | Missing needle |
| ---: | ---: | ---: | ---: |
| 1,000 | 0.333 | 0.374 | 0.458 |
| 10,000 | 0.807 | 0.862 | 1.098 |
| 20,000 | 0.633 | 0.639 | 0.822 |

The 10,000-line miss remains slower. Both timed regions include capturing/feeding
and scanning; terminal parsing, result retrieval and destruction are excluded.
Reference timing includes Python ctypes dispatch, which matters most for short
searches. This exploratory checkpoint uses `1d3c9cf` plus the search implementation,
before the concurrent byte-budget and Unicode-write changes. It has no cold-history,
live-update, cross-process confidence or whole-application performance coverage.

The combined byte-budget, Unicode and live-anchor checkpoint was rebuilt and
measured separately in `upstream/search-checkpoint-build.json` and
`upstream/search-benchmark-checkpoint.json`. These static-search timings retain
the **legacy width configuration** in both engines, with the same resident
history, needle cases and exclusions as above:

| Retained text lines | ASCII needle | Unicode needle | Missing needle |
| ---: | ---: | ---: | ---: |
| 1,000 | 0.371 | 0.357 | 0.452 |
| 10,000 | 0.818 | 0.797 | 1.209 |
| 20,000 | 0.630 | 0.656 | 0.973 |

All selected coordinates and retained history counts matched. The 10,000-line
miss is still slower; neither this result nor the faster cases establish
overall performance parity. This checkpoint precedes the separate performance
experiments on the budget signal.

Refresh currently scans a new owned capture rather than incrementally updating
the old search stream. Remaining work includes broader live-search differential
coverage, bounded incremental updates, long-running output/navigation stress,
and final combined-source latency and memory comparisons against Ghostty.
