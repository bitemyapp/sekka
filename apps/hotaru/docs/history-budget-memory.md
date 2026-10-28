# History memory with the byte budget

Hotaru's current logical byte budget bounds retained content, but compression
still does not release process RSS as effectively as Ghostty's page allocator.
This remains an acceptance gap. No global allocator change, `malloc_trim`, or
forced collection was used in these measurements.

The exact build is recorded in
[`history-budget-memory/build.json`](../upstream/history-budget-memory/build.json):
source manifest SHA-256
`2173606ace6a89542a27e35f2fd1b819b84fce09e96b1a9e533d91ce532fc387`,
based on checkpoint `c9801d6` plus owner-only budget atomics, the wide-overwrite
ordering correction, metadata row spans and OSC 7. These results precede the
later UTF-8 validator experiments. The reference is libghostty-vt at
`6ba144158577d033ccb6ed4b574e4013957c9161`.

Each sample runs in a fresh process on wx-workstation, CPU 8, under the shared
exclusive lock. Both engines use 120 columns, 24 active rows, a 10,000-row history
limit and a 50,000,000-byte budget. There are five samples per configuration,
with alternating engine order. The input allocation is freed before populated
RSS. The lock excludes other cooperating build jobs, but unrelated host activity
was not excluded or recorded for these runs.

Every common-content sample has equal retained row count and an equal FNV-1a
digest over all cell scalars, widths, protection, graphemes, resolved styles and
URI bytes. Internal style IDs are normalized. Blank scalar zero maps to a space;
the corpora do not exercise wide-character spacer heads. Each engine's digest
is checked again after compression and the timed read. These hashes are a
regression gate, not a substitute for the permanent cell-level oracle tests.

## Equal retained contents

RSS is process RSS in KiB, not a heap allocation estimate. Values are medians.
The resident and cold columns are independent fresh-process configurations.

| Workload | History rows | Hotaru resident RSS | Ghostty resident RSS | Hotaru cold RSS | Ghostty cold RSS | Hotaru cold read | Ghostty cold read |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| ASCII logs | 7,977 | 11,392 | 11,160 | 11,468 | 3,920 | 2.191 ms | 1.105 ms |
| RGB styled text | 7,977 | 18,256 | 18,112 | 18,576 | 9,952 | 4.212 ms | 9.106 ms |
| Wide and combining text | 7,977 | 13,568 | 13,960 | 13,592 | 4,748 | 3.467 ms | 1.926 ms |
| Mixed printable text | 7,977 | 11,336 | 11,156 | 11,536 | 4,084 | 2.192 ms | 1.423 ms |
| Four unique URIs per row | 2,977 | 14,256 | 17,120 | 14,520 | 5,636 | 1.785 ms | 3.040 ms |

The timed read acquires each row and reads its first codepoint. Hotaru restores
temporary bounded blocks through an owned `ContentSnapshot`; Ghostty's C API
restores the terminal's live pages. This measures those concrete access paths,
including allocation and row/reference acquisition. It is not a pure codec
comparison. Hotaru's resident reads are faster on these workloads, while cold
reads vary with the corpus. The measured short-run feed times are preserved in
the reports and are not the separate paired throughput acceptance benchmark.

Raw samples and commands are in
[`common-report.json`](../upstream/history-budget-memory/common-report.json) and
[`uri-report.json`](../upstream/history-budget-memory/uri-report.json).
The latter uses four unique links per row because the original eight-link
workload exposed the reference limitation below.

## Byte-limit stress and held captures

Fourteen thousand rows containing four distinct roughly 1-KiB URIs each retain
8,291 history rows in Hotaru and 3,915 in Ghostty. Both use the nominal 50 MB
setting, but their byte accounting and allocation granularity differ. Their RSS
or read times must not be compared as equal-content ratios. Compression on/off
preserves each engine's own retention and digest.

Resident and compressed Hotaru captures were also retained across history
clear, terminal mutation and terminal destruction. The full contents remained
equal. The ASCII capture's estimated owned storage is 9,653,518 bytes when
resident and 406,977 bytes when captured cold; the URI capture is 13,150,046 and
678,039 bytes respectively. Actual process RSS stays around 11–15 MiB even
after releasing the terminal and capture. That distinction demonstrates the
allocator reclamation issue rather than hiding it behind encoded byte counts.
These ownership scenarios are Hotaru-only measurements; the reference adapter
does not provide an equivalent owned snapshot API.

## Reference hyperlink capacity limitation

The pinned Ghostty loses the 1,664th unique live hyperlink within a page even
with short URIs and no scrollback. This is an intentional documented difference
for Hotaru: valid links remain present, subject to the normal history budget.

At 120×24, print one `x` for each of the OSC 8 URIs `https://x/0` through
`https://x/1662`. The full normalized contents match exactly. Add
`https://x/1663`: the glyph at zero-based row 13, column 103 is still present,
but Ghostty returns an empty URI. Hotaru returns the requested URI. Adding the
next link also loses that URI in Ghostty. Raw input fixtures, per-cell omitted
positions, reference commit/library hashes and both full digests are retained
under [`upstream/hyperlink-capacity`](../upstream/hyperlink-capacity/report.json).

The source mechanism is page-local: `page.Capacity.hyperlink_bytes` uses
`size.HyperlinkCountInt`, a `u16`, although the field counts bytes. Dividing its
65,535-byte maximum by the set item size and rounding the table capacity gives
2,048 buckets. `RefCountedSet` uses a 13/16 load factor and reserves ID zero,
leaving 1,663 live entries. `PageList.increaseCapacity` then returns `OutOfSpace`;
`Screen.startHyperlink` propagates it. This is independent of URI byte length.
The original 3,000-row eight-link input showed the same failure repeatedly at
page boundaries. Its unequal full digest was rejected before reporting ratios.

Relevant pinned source: [page capacity and layout](https://github.com/ghostty-org/ghostty/blob/6ba144158577d033ccb6ed4b574e4013957c9161/src/terminal/page.zig),
[set layout](https://github.com/ghostty-org/ghostty/blob/6ba144158577d033ccb6ed4b574e4013957c9161/src/terminal/ref_counted_set.zig),
[capacity growth](https://github.com/ghostty-org/ghostty/blob/6ba144158577d033ccb6ed4b574e4013957c9161/src/terminal/PageList.zig).

## Reproduction

Build `tools/history-budget-candidate.rs` as the replay example
`history_budget` in an isolated miniworkspace. Build
`tools/history-budget-reference.c` against the pinned shared library. The exact
native CPU/LTO flags, commands and source manifest are in `build.json`.

```sh
python3 tools/history-budget-benchmark.py --build /path/to/build.json \
  --output /path/to/results --samples 5 --cpu 8
python3 tools/history-hyperlink-capacity.py \
  --reference-root /path/to/ghostty-reference --build /path/to/build.json \
  --output /path/to/capacity-results
```

The current driver regenerates the common corpora identically and uses the
corrected four-link URI workload. `--case unique-uri --case byte-limit-uri`
runs only the latter two cases. Generated bulk corpora and binaries stay outside
the repository; only the small capacity boundary fixtures are retained.
