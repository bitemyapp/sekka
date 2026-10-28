# Incremental shaped-row preparation prototype

This isolated prototype reuses completed row layouts before the worker builds
cell strings, style spans, hyperlink ranges, and toolkit layout-cache keys.
It preserves the existing Aizome/Sidestep shaping and drawing pipeline. It does
not add GPU presentation or establish whole-application Ghostty parity.

The baseline is Sekka `cb91125928563e11700f4c3f2034a00300578ca6`.
The original `font.rs` SHA-256 is
`677df2b032bab2e30766b600fbb6783d7a4247fe70799b1757f1325bbb758e99`.
The candidate lives at `/home/callen/work/hotaru-shaped-row-isolated`, with its
remote build at `wx-workstation:/home/callen/work/hotaru-shaped-row-20261028`.
[The patch](../upstream/renderer-row-cache/prototype.patch) changes `font.rs`,
span iteration in `terminal_view.rs`, a row-storage estimate accessor, and
test-only benchmark support. It applies cleanly to the current shared source;
the raw records describe that isolated prototype. The cache is now applied to
the working branch for combined qualification. Its test-only allocation counter
also uses saturating addition so diagnostic overflow cannot unwind from the
global allocator; measured counts are far below saturation.

## Ownership and invalidation

Each entry owns its source `RowRef`, shared immutable spans, and row-relative
hyperlink ranges. Pointer identity is therefore valid for the entry's entire
lifetime; allocator reuse cannot turn a stale entry into a hit. The completed
frame owns the exact input snapshot, including on cache hits. Moving a row during
scrolling reuses its spans and projects its links to the new row index.

Keys include row identity, column count, the existing cursor shaping boundary,
the row's selection interval, and its projected ordinary/current search-match
intervals. Changing the font configuration or scale clears the cache. Job epoch
alone does not invalidate unaffected rows: it also changes when overlays change.
Cursor shape and blink phase do not change the current shaping boundary policy.

Only entries used by the next completed frame survive. Cancellation or a shaping
error discards both generations. Admission is limited to an estimated 8 MiB per
generation; replacement can hold two generations briefly. An oversized entry
still draws completely but is not cached. The estimate includes row capacities,
keys, spans, links, and layout/glyph storage and deliberately double-counts
shared layouts. This is a conservative logical admission charge, **not a bound
on process RSS or exclusive allocation**. Displayed/queued frames and toolkit
font caches have separate ownership. A future mapped-history representation
must include the whole retained backing page in the row accessor; counting only
its visible cell slice would understate retention. Current measured rows use
the existing allocator-backed representation.

## Correctness and review

The test-only baseline preserves the original uncached algorithm and original
`Vec<Vec<Span>>` representation. Conversion to shared spans happens only after a
baseline timing call, when checking results. The comparison includes metrics,
every span's bounds/style/overlay flags, complete shaped glyph/run/cluster data,
font-family resolution, links, and snapshot ownership.

Three new ordinary tests cover moved-row reuse and hyperlink projection;
selection, search, cursor, scale and font invalidation; denied admission;
cancellation; and retained old frames across later mutations. One test runs 600
seeded snapshot transitions including one-column resize, wide characters,
combining marks, emoji, OSC 8, styles, scrolling and erase. The benchmark checks
another 1,920 workload snapshots against the original before measuring them.
All 41 ordinary native package tests passed; seven tests requiring separate
fixtures or explicit diagnostics remained ignored by the ordinary invocation.

Strict package Clippy passed with `--all-targets --all-features --no-deps -- -D
warnings`, as did package formatting. The broader invocation encountered five
existing style warnings in `aizome-kit`; its unmodified log is retained. The
native UI agent independently reviewed cache keys, immutable ownership,
cancellation and admission and found no concrete correctness defect. Packaged
Wayland smoke tests remain a combined-integration check, not evidence from this
isolated worker prototype.

## Matched worker measurements

Both algorithms run in the same release test binary using Rust 1.95.0, thin LTO
and one codegen unit. Each workload has 160 immutable snapshots; 32 warm up the
font cache and 128 are measured. Four rounds alternate baseline/candidate order.
Timing uses CPUs 8–11 under the shared workstation lock. The previous frame is
held during shaping and retired outside the timed interval for both algorithms.
These numbers exclude input parsing, rasterization, presentation and frame
retirement. The allocation counter is a separate test-only binary and is not
used for accepted timing results.

| Grid | Workload | Original µs/frame | Cached µs/frame | Ratio |
| --- | --- | ---: | ---: | ---: |
| 120×40 | Unchanged | 273.3 | 3.6 | 0.013 |
| 120×40 | One cell | 286.6 | 18.9 | 0.066 |
| 120×40 | Cursor | 286.5 | 26.5 | 0.092 |
| 120×40 | One-row scroll | 292.3 | 18.8 | 0.064 |
| 120×40 | CJK/emoji edit | 310.1 | 68.3 | 0.220 |
| 120×40 | Full replacement | 285.6 | 287.9 | 1.008 |
| 240×80 | Unchanged | 1196.3 | 7.2 | 0.006 |
| 240×80 | One cell | 1147.5 | 35.1 | 0.031 |
| 240×80 | Cursor | 1163.7 | 53.4 | 0.046 |
| 240×80 | One-row scroll | 1159.0 | 35.5 | 0.031 |
| 240×80 | CJK/emoji edit | 1200.9 | 133.1 | 0.111 |
| 240×80 | Full replacement | 1195.4 | 1174.2 | 0.982 |

At 120×40, unchanged frames reduce Rust allocation calls from 10,005 to 10 and
requested bytes from 695,136 to 15,820. One-cell edits reduce calls from about
10,157 to 414 and requested bytes from about 710,627 to 48,743. Full replacement
adds about 85 calls and 17,964 requested bytes. These counters cover Rust
alloc/realloc calls on the shaping test thread; they exclude C-library
allocations, deallocation, allocator usable-size overhead and other threads.
The largest estimated two-generation cache charge in these workloads was
1,844,424 bytes, below the configured admission limit.

## Reproduction and evidence

The [source manifest](../upstream/renderer-row-cache/source-manifest.json)
records 648 source/lock files, both test binaries, compiler details and font
hashes. Its canonical source-map digest is
`981815b5e7cf771c52be316449b9ffee03ec1ed6dcf268151e8214a4ddce6eb8`.
The local candidate files match the built source hashes.

Run from the remote candidate's `desktop/aizome` directory, with
`CARGO_TARGET_DIR=/home/callen/work/hotaru-shaped-row-20261028/target`,
`CARGO_PROFILE_RELEASE_LTO=thin`, `CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1`,
`AIZOME_FONT_DIR=/nix/store/b9vp4qd3gz9vmsl20grvd80hniirn5n5-aizome-fonts`, and
`HOTARU_FONT_DIR=/nix/store/1rv68ixfzh8ki0jmanx7y846h2qdvzbg-hotaru-fonts`:

```sh
flock -x /home/callen/.cache/hotaru-workstation.lock \
  env HOTARU_ROW_BENCH_OUTPUT=/absolute/output.json \
  taskset -c 8-11 cargo +1.95.0 test --release --locked -p aizome-hotaru \
  --lib font::tests::benchmark_row_preparation -- --ignored --exact --test-threads=1
```

For allocation counts, add `--features shape-allocation-probe` to Cargo's
arguments and use a separate output filename. The custom allocator is compiled
only into the test module and forwards all pointer/layout operations to `System`.
It introduces no production unsafe code.

Raw [timing](../upstream/renderer-row-cache/worker-timing.json),
[allocation](../upstream/renderer-row-cache/worker-allocations.json), and
[summary](../upstream/renderer-row-cache/summary.json) reports accompany the
validation logs. The [combined package checkpoint](cursor-cache-checkpoint.md)
passes all six native Wayland suites and the allocation-probe correctness checks.
The next measurement step is matched application workloads using the controls in
[the renderer audit](renderer-audit.md). These worker improvements do not close
the GPU, raster cost, sprite generation or whole-application performance gaps.
