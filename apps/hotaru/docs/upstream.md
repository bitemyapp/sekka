# Ghostty reference and Linux port ledger

Hotaru targets Ghostty main commit
[`6ba144158577d033ccb6ed4b574e4013957c9161`](https://github.com/ghostty-org/ghostty/commit/6ba144158577d033ccb6ed4b574e4013957c9161),
observed on October 28, 2026. That source identifies itself as **1.4.1-dev**
and requires **Zig 0.17.0**. The latest release observed was **1.4.0**, commit
[`83fb95624b0439ee660c2679b43b38c8b63769cd`](https://github.com/ghostty-org/ghostty/commit/83fb95624b0439ee660c2679b43b38c8b63769cd).
The main commit is the behavioral target; a release build is additional
evidence, not a substitute for the current source.

The exact source, fork and toolchain identities are recorded in
[`../upstream/ghostty.json`](../upstream/ghostty.json). Upstream's MIT license
is preserved byte-for-byte as
[`../upstream/LICENSE.ghostty`](../upstream/LICENSE.ghostty). Preserve that
notice when adapting its source, tests, shaders or data. The external reference
checkout contains upstream's original platforms so it can serve as an unchanged
oracle; none of that checkout is part of the Hotaru product or its build.
Hotaru's product scope is Linux, with native Aizome chrome.

## Coverage and acceptance

[`../upstream/modules.json`](../upstream/modules.json) inventories every tracked
Zig, C, C++, header, GLSL and Metal source file below upstream `src/`, including
its exact Git blob identity. Every Linux module starts **pending**. A Rust
implementation is not coverage until its row identifies the Rust source and
reproducible tests against the reference. Non-Linux exclusions are scope
decisions, not completed ports. Shared modules must shed their non-Linux
branches when ported. GTK module rows require replacement with native Aizome
behavior, never importing GTK as the new chrome.

The initial inventory has 481 Linux/shared modules, 58 GTK replacement modules,
35 build replacement modules, 19 benchmark modules and 37 platform exclusions.
These are source files, not a percentage of implementation effort. Binary
assets, themes, shell integration, terminfo, public C interfaces and external
integration suites need their own acceptance evidence; source-file counts do
not certify those features.

| Upstream subsystem | Rust obligations and evidence needed | Initial state |
| --- | --- | --- |
| `terminal/Parser.zig`, `parse_table`, `stream*`, `UTF8Decoder`, `csi`, `osc`, `dcs`, `apc` | Incremental parsing at every byte boundary; malformed UTF-8, overflow, cancellation, parameter/subparameter handling and bounded strings; SIMD/scalar equivalence | Pending |
| `terminal/page`, `PageList`, `Screen`, `ScreenSet`, `Terminal` | Packed cells, resize/reflow, primary/alternate screens, history limits, erasure, scroll regions, wide cells and graphemes; state snapshots against Zig | Pending |
| `terminal/bitmap_allocator`, `hash_map`, `ref_counted_set`, `StringMap`, `style`, `hyperlink` | Compact per-page metadata, allocation-failure behavior, reference-count integrity, clone/relocation correctness | Pending |
| `terminal/compress*`, `mem`, `snapshot*` | Cold-history compression, retained virtual mappings, exact decode bounds, malformed input and round-trip properties; memory ratio and restore latency | Pending |
| `terminal/formatter`, `Selection*`, `selection_codepoints`, `search*`, `highlight` | Plain/VT/HTML/mixed copy, rectangle and wrapped selections, search, hyperlinks and text maps; asynchronous owned clipboard snapshots | Pending |
| `terminal/kitty*`, `apc*`, `paste`, `clipboard`, `mouse`, `focus`, `size_report`, `tmux*` | Kitty graphics/keyboard/clipboard, bracketed paste, mouse/focus reports, tmux integration and resource limits | Pending |
| `terminal/modes`, `Tabstops`, `charsets`, `sgr`, colors, device/status/checksum modules | Full mode defaults and resets, reply encodings, color changes, tab stops and terminal queries | Pending |
| `termio/*`, `pty`, `Command`, shell integration | PTY creation and teardown, child reaping, nonblocking backpressure, resize, signals, environment/working directory, shell lifecycle | Pending |
| `input/*`, bindings and key encoders | Keyboard layouts, repeat, modifiers, compose/IME, shortcuts, Kitty keyboard and mouse encodings | Pending |
| `font/*` excluding CoreText | Fontconfig discovery, FreeType rasterization, HarfBuzz shaping, fallback, emoji, ligatures, metrics and sprite glyphs; deterministic font fixtures | Pending |
| `renderer/*` excluding Metal/DisplayLink | Glyph atlases, dirty rows, GPU buffers, shaders, cursor, decoration, images, selection, synchronization, fractional scaling and frame pacing | Pending |
| `apprt/gtk/*`, `App`, `Surface`, `apprt/action` | Aizome windows, tabs, splits, search, settings, menus, dialogs, accessibility, clipboard/primary selection and IME; real GUI tests | Pending |
| `config/*`, `cli/*` | Config defaults, parsing/reloading, keybindings, command-line behavior, diagnostics, themes and paths | Pending |
| `os/*` Linux/shared, `global`, `crash`, `inspector`, `datastruct`, `simd`, utility modules | Linux integration and applicable diagnostics; ownership, concurrency and safety evidence; remove platform-only branches | Pending |
| `terminal/c/*`, `lib/*`, `include/ghostty*` | Decide and document supported embedding/API surface; preserve relevant terminal functionality regardless of C API decision | Pending |
| `build*`, packaging, resources, `test/*` | Cargo/Nix packaging, terminfo, shell integration, licenses, reference fixtures, fuzzing and application tests | Pending |

Regenerate the inventory from the pin, outside the product tree:

```sh
git -C "$GHOSTTY_REFERENCE" ls-tree -r -l 6ba144158577d033ccb6ed4b574e4013957c9161 src > /tmp/ghostty-tree.txt
python3 tools/inventory.py --tree /tmp/ghostty-tree.txt \
  --revision 6ba144158577d033ccb6ed4b574e4013957c9161 --output upstream/modules.json
```

Regeneration retains evidence only where the exact upstream blob and scope are
unchanged. Keep evidence paths or commands in each row rather than claiming a
whole directory is complete because a small terminal demo runs.

## Clipboard hang audit

The user's fix is commit
[`108af5ed0a19800671e74421ec47699b6a35875e`](https://github.com/bitemyapp/ghostty/commit/108af5ed0a19800671e74421ec47699b6a35875e),
on `bitemyapp/fix-clipboard-hangs`. Its merge base with current upstream is
`51897c0cd51fee61fff824d616fb2901ac41e817`. The branch also contains an older
rewrite plan; the current request supersedes its macOS/GTK retention choices.

Current upstream still calls `copySelectionToClipboards` while holding the
renderer mutex in `Surface.zig`'s `copy_to_clipboard` action. Formatting walks
the selected screen, resolves styles and allocates plain/VT/HTML output before
calling the GTK clipboard setter. Copy-on-select paths also use this formatter.
This is work proportional to selection size on the UI thread and under the
terminal/render lock. The source demonstrates a stall mechanism; no compositor
deadlock cycle has yet been reproduced or proven in this investigation.

The fork shortens the locked operation to a selection snapshot, releases the
mutex, formats on a detached background thread and posts a GLib idle callback
to publish the clipboard. That separation is the design requirement to retain.
The implementation itself is not a correctness oracle:

- Its snapshot retains a raw pointer to the live color palette. The Rust
  snapshot must own the palette and all mutable formatting options.
- The worker retains a raw surface pointer before taking a GObject reference;
  teardown can race it. Rust workers must not dereference a destroyed window.
- Detached workers have no cancellation, generation ordering or concurrency
  bound. A slow old copy can overwrite a later copy, and repeated copies can
  accumulate workers and memory.
- The fallback returns to synchronous formatting on allocation/thread failure.
  Failure must be reported or retried without blocking the UI on a large copy.
- The new path always produces plain text, even for a VT-only copy. It records
  hyperlink URIs without emitting them, and its HTML differs from the existing
  formatter. Preserve current formatter semantics with differential tests.
- Its sole snapshot unit test is a compile check. The shell reproducer requires
  manual selection and visual observation; it proves no automated latency bound.

Latest upstream also supports OSC 5522 (Kitty clipboard), including multiple
MIME representations, authorization and limits. OSC 52 is not the entire
clipboard surface.

The Rust implementation should use immutable owned page/selection data, a
bounded formatting worker, monotonically increasing request IDs and explicit
window lifetime handling. UI publication must be quick. Wayland clipboard pipe
writes must be nonblocking and handle short writes, `EAGAIN`, `EINTR`, reader
disconnect and cancellation; moving formatting off-thread does not by itself
fix blocking clipboard transfers. Large pastes need the same PTY backpressure
discipline. Neither clipboard formatting nor pipe writes may hold the terminal
state/render mutex.

Required automated regressions include 256 KiB, 512 KiB, 1 MiB, 2 MiB, 8 MiB
and larger selections; exact bytes for all copy formats; combining characters,
wide cells, soft wraps, blank/trailing rows, rectangular selection, hyperlinks
and palette changes. Exercise simultaneous terminal output, repeated copies,
out-of-order completion, tab/window close, queue exhaustion, allocation failure,
cancelled/stalled/slow readers, primary selection and pasting to a stalled child.
Measure UI heartbeat/frame stalls and peak memory separately from formatter
throughput. A successful 8 MiB pipe unit test alone does not certify desktop
responsiveness.

## Memory and performance invariants

Current upstream stores both a cell and row descriptor in **8 bytes**. Page
memory is a single page-aligned anonymous mapping with cache-aligned cells and
offset-relative metadata. Styles and hyperlinks are shared within pages;
graphemes use bitmap-managed four-codepoint chunks and strings use 32-byte
chunks. Pages and nodes are pooled, with a pool/heap ownership discriminator.
Cold scrollback pages can be compressed while their address ranges remain
mapped; Linux `MADV_DONTNEED` reclaims resident memory and restores read-as-zero
semantics. A compressed page's virtual mapping remains reserved, so restoration
does not require a new address allocation.

Safe Rust indexes, transparent integer cell types, arena owners and explicit
enum states can preserve these patterns. Avoid a `String`/`Vec`/`Arc` in every
cell or per-character allocation. Any mmap/pointer/SIMD boundary needs local
safety invariants and adversarial tests; use `Drop` to own mappings and file
descriptors. Rust aliasing rules still apply while compacting or borrowing a
page. Keep ownership provenance explicit instead of inferring it from length.
Snapshot copies must preserve styles, graphemes and links without borrowing
mutable terminal storage.

Matching parser throughput alone is not the performance requirement. The final
gate also covers PTY throughput, idle CPU, startup, retained scrollback RSS,
resize/reflow, shaping/rasterization, GPU draw cost, keyboard-to-frame latency,
clipboard event-loop stalls and compression ratio/restore latency. Compare
identical content, dimensions, history, features and font/shader settings on the
same host. Feature omissions can make a prototype faster and invalidate a
parity claim.

## Remote reference builds and measurement

Heavy reference work runs on `ssh wx-workstation`. The current external checkout
is `/home/callen/work/ghostty-reference-20261028`; the isolated Zig installation
is under `/home/callen/.cache/hotaru-reference`. `tools/reference.py` verifies
the official archive SHA-256, rejects tracked reference edits, checks out the
exact pin and builds ReleaseFast terminal/benchmark binaries. Its `build`
action records commands, revisions, compiler and binary hashes in
`hotaru-baseline.json` in the external checkout.

Copy `tools/` and `upstream/ghostty.json` into a matching layout on the remote
host, then run:

```sh
python3 tools/reference.py prepare
python3 tools/reference.py build --jobs 16
python3 tools/corpus.py /home/callen/work/hotaru-corpora --bytes 8388608
```

The tools acquire an exclusive
`/home/callen/.cache/hotaru-workstation.lock`. All Hotaru build and timing work
must cooperate with that lock. Do not time alongside unrelated heavy work;
the lock cannot prevent activity from other users or projects.

Generate corpora before measurements. `tools/measure.py` executes argument
arrays directly, records corpus/binary hashes, warmups, every wall-time sample,
peak RSS, CPU affinity and machine details. It checks that the corpus and
binary did not change during measurement and stops the whole process group on
timeout. `{data}`, `{rows}` and `{cols}` placeholders keep the recorded
dimensions and executed commands together. Example from the remote tool root:

```sh
python3 tools/measure.py --label ghostty-terminal-stream \
  --revision 6ba144158577d033ccb6ed4b574e4013957c9161 \
  --data /home/callen/work/hotaru-corpora/ascii.vt \
  --output /home/callen/work/hotaru-results/ghostty-ascii.json -- \
  /home/callen/work/ghostty-reference-20261028/zig-out/bin/ghostty-bench \
  +terminal-stream --data={data} --terminal-rows={rows} --terminal-cols={cols}

python3 tools/measure.py --label hotaru-terminal-stream --revision RUST_COMMIT \
  --data /home/callen/work/hotaru-corpora/ascii.vt \
  --output /home/callen/work/hotaru-results/hotaru-ascii.json -- \
  /path/to/hotaru-replay --data {data} --rows {rows} --cols {cols} --bench
```

These are **whole-process** measurements, including startup, file reads and
teardown. They are not parser-only timings. Before interpreting a speed ratio,
verify equivalent terminal state and history behavior; report mismatches and
unsupported features. Use sufficiently large corpora for stable timings and
repeat any small differences. Upstream also provides formatter, resize,
snapshot, screen-clone, compression, Unicode and parser-specific benchmarks;
each needs a matching Rust workload, not an unrelated fast operation.

The source inventory, build scripts and a successful reference smoke test are
preparation. They do not establish a completed Rust port or performance parity.

## Differential snapshots

`tools/differential.py` calls the real reference `libghostty-vt.so` through
ctypes. It verifies every structure size, alignment and field offset against
the linked library's ABI manifest, and checks the shared library's SHA-256
against confirmed build metadata before creating a terminal. The library stays
outside the product. `upstream/fixtures/` holds actual input streams and
snapshots produced by that library, including its hash and source pin.

```sh
python3 tools/differential.py \
  --library /home/callen/work/ghostty-reference-20261028/zig-out/lib/libghostty-vt.so \
  --baseline-metadata upstream/baseline-build.json \
  --replay /path/to/hotaru-replay \
  --fixtures upstream/fixtures --report /tmp/hotaru-differential.json
```

The suite contains 148 deterministic streams replayed in chunks of 1,
7 and 65,536 bytes. It checks upstream's own chunk invariance before comparing
the Rust result. Comparison includes every active cell's text/grapheme, width,
symbolic colors, style flags, underline variants and overline, row wrapping, cursor position and cursor
visibility. It also compares PTY replies, application cursor/focus/bracketed
paste modes, active screen identity, the palette and effective foreground and
background. Both sides use the explicitly declared xterm fixture palette and
foreground/background, so application theme defaults do not affect the test.
Differences cause a nonzero exit and include exact JSON paths.
No expected snapshot is fabricated from the Rust implementation.

The report explicitly lists untested state. In particular, this harness
normalizes empty cells to spaces and wide-character spacer heads to blanks;
it does not compare pending wrap. It does not
yet compare scrollback, all modes/effects, hyperlinks, graphics, rendering or
clipboard transport.
These limitations must not disappear from reports when the basic cases pass.

The current passing expanded run is recorded in
[`../upstream/differential-report.json`](../upstream/differential-report.json):
444 matching comparisons against its exact Rust and Zig binary hashes. The
ledger records the tested core and input modules as **partial**. No module is
marked complete.

The additional eight reference fixtures cover restoring origin-mode cursors
above a changed scroll margin, DCS parameter overflow, and OSC 4 parsing and
capture limits. Ordinary OSC trailing data has a 2,048-byte capture limit;
overflow discards the entire command. At that boundary, 512 minimal palette
queries produce 12,800 reply bytes, whereas a 2,049-byte body produces none.
Invalid palette indices or colors stop parsing after earlier valid pairs;
empty fields are skipped. These are measured reference semantics, not a newly
invented request-count limit. The allocating OSC command path has a separate
8 MiB limit.

Two subsequent reference fixtures exercise color resets. RIS resets indexed
palette entries to configured defaults while preserving dynamic foreground and
background overrides; OSC 110/111 clear those overrides explicitly. Direct
C-API checks also confirm that changing configured defaults preserves explicit
OSC overrides even when their RGB equals the previous default. This requires
override tracking, rather than inferring intent from color equality. The earlier 297
comparisons passed with the configured-colors implementation; its exact source
and executable hashes are recorded in
[`../upstream/candidate-correctness-build.json`](../upstream/candidate-correctness-build.json).

## Linux keyboard and mouse input

`crates/hotaru-core/src/input.rs` implements legacy/fixterms/modifyOtherKeys
encoding, Kitty flags and its eight-slot stack, and X10, normal, button and any
mouse tracking with legacy, UTF-8, SGR, urxvt and pixel formats. The PTY owner
encodes normalized events using current terminal modes. Keyboard stacks belong
to each screen and survive alternate-screen clearing, matching upstream.

`tools/input-oracle.py` records actual bytes from the pinned C API in
`upstream/input-fixtures.json`. The recorded test pass covers 50,018 keyboard
encodings, 15,000 mouse scenarios encoded twice (including motion suppression),
and 32 negotiation streams in three chunk sizes. `input-report.json` identifies
the exact library, fixtures and tested Rust sources; strict core Clippy passes.
This verifies normalized events, not frontend layout/IME correctness. Hyper/Meta
modifier states absent from Ghostty's Linux input ABI remain unsupported, and
the functional-key table follows upstream through F25. Unsupported physical
keys retain the text fallback. No Zig runtime or foreign library is linked.

## Styles, device replies and reset behavior

The expanded screen corpus adds 49 streams covering underline styles and color,
overline, malformed and empty SGR parameters, canonical DECRQSS SGR replies,
primary/secondary/tertiary device attributes, DECID, strict malformed mode
queries and DECSTR. All 444 chunked comparisons pass; the tested source tree
and binary are identified in
[`../upstream/sgr-report.json`](../upstream/sgr-report.json).

Underline styles occupy bits in the existing compact style attributes, so
the core retains its cell and style layout. DECSTR resets style, margins,
selected modes and indexed palette entries while retaining screen contents,
cursor position and dynamic foreground/background overrides. Mode and query
support remains partial. XTVERSION deliberately identifies Hotaru rather than
Ghostty and is excluded from upstream identity equality. These checks cover
terminal state and emitted bytes; native decoration rendering has separate
validation.

## Comparable feed benchmarks

The current measured core checkpoint still misses the performance gate.
[`../upstream/candidate-checkpoint-build.json`](../upstream/candidate-checkpoint-build.json)
identifies the build with reflow, keyboard/mouse input, extended styles and the
compressed-row recycling fix. All four compared states matched. Rust/Zig median
feed ratios were **1.074 ASCII**, **1.165 Unicode**, **0.554 styled text** and
**0.084 cursor/erase**, recorded in `upstream/benchmarks/checkpoint-*.json`.
These are zero-history core measurements; history restoration, rendering,
desktop responsiveness and full feature parity have separate open gates.
The older measurements below describe their explicitly named checkpoints.

The built-in Ghostty stream benchmark's default history policy differs from
Hotaru's replay default, so do not compare those defaults and call the ratio
equivalent work. `reference.py build` also compiles the development-only
`tools/reference-replay.c` adapter against the external pinned library with
`-O3 -march=native`. It applies the exact zero-history and color configuration
used by the differential oracle. Its source and binary hashes are recorded in
the baseline metadata. No C adapter or Zig library is linked into Hotaru.

`tools/benchmark.py` first compares full common snapshots for the exact input
corpus, records canonical state hashes and refuses to time a mismatch. Only
then does it run both adapters serially, alternating their order across rounds
under the shared workstation lock. Each adapter reads the file once before all
iterations, constructs a fresh terminal before each measured feed, and reports
setup, feed and teardown times separately. Setup and teardown are outside the
feed timing. Both retain PTY reply bytes through teardown. Query workloads
require each iteration's reply count to match the oracle; the current approved
patterns contain no queries, so older Rust replay builds may use the verified
pre-timing zero-response snapshot. Reports identify this distinction. Both subprocesses run
on the same explicitly selected CPU. Use a Rust release build with
`-C target-cpu=native` for the native Zig comparison.

`tools/build-candidate.py` creates that build under the shared lock and records
the Rust toolchain, native optimization settings, executable hash, Cargo.lock
and every source-file hash. It rejects source changes during compilation.
It cleans the two first-party release packages before building: mirrored source
can retain older mtimes, which must not let Cargo reuse a stale executable.
The benchmark requires this manifest and verifies that all binaries, input and
metadata remain unchanged through the run. Source hashes identify uncommitted
work precisely; a base Git commit alone does not.

```sh
python3 tools/build-candidate.py \
  --workspace /path/to/synced/hotaru --revision 'BASE_COMMIT plus worktree edits' \
  --metadata /tmp/hotaru-candidate-build.json

python3 tools/benchmark.py \
  --library /home/callen/work/ghostty-reference-20261028/zig-out/lib/libghostty-vt.so \
  --reference /home/callen/work/ghostty-reference-20261028/zig-out/bin/ghostty-reference-replay \
  --replay /path/to/native-release/hotaru-replay --revision RUST_COMMIT \
  --candidate-metadata /tmp/hotaru-candidate-build.json --cpu 8 \
  --data /home/callen/work/hotaru-corpora/ascii.vt --report /tmp/hotaru-ascii.json
```

The resulting ratio is Rust feed time divided by Zig feed time; values above
one are slower. A matching snapshot covers only the state fields listed by the
differential harness. Setup cost, memory, full history/reflow, font rendering,
GUI latency and omitted terminal features still require separate benchmarks.
Only exact repetitions of the four reviewed ASCII, styled, Unicode and cursor
patterns from `corpus.py` are currently accepted for performance comparisons.
The hyperlink pattern is diagnostic only because Hotaru does not yet retain
hyperlinks. Unknown inputs are rejected for acceptance: matching visible state
does not prove that an unimplemented protocol performed equivalent work.

The historical native run before configured-color changes uses CPU 8, 20 timed iterations per engine
after warmup, and the exact candidate source/build manifest in
[`../upstream/candidate-build.json`](../upstream/candidate-build.json).
All four common snapshots matched. Median feed times for approximately 8 MiB
of input were:

| Workload | Zig | Rust | Rust / Zig |
| --- | ---: | ---: | ---: |
| ASCII | 6.474 ms | 9.127 ms | 1.410 |
| Styled text | 31.836 ms | 20.072 ms | 0.630 |
| Unicode | 31.215 ms | 37.725 ms | 1.209 |
| Cursor/erase | 752.875 ms | 205.971 ms | 0.274 |

ASCII and Unicode still fail the requested performance threshold. The shared
lock excludes our concurrent builds and UI tests, but unrelated host jobs and
frequency scaling were not disabled; the raw reports record host load and CPU
affinity. These are scoped terminal-feed measurements, not overall application
performance or complete feature parity.

The subsequent bounded SIMD scanner checkpoint, after configured colors and
before resize/input changes, is identified by
[`../upstream/candidate-simd-build.json`](../upstream/candidate-simd-build.json).
The same comparison method and all four common snapshots passed:

| Workload | Zig | Rust | Rust / Zig |
| --- | ---: | ---: | ---: |
| ASCII | 6.395 ms | 7.572 ms | 1.184 |
| Styled text | 32.103 ms | 20.203 ms | 0.629 |
| Unicode | 30.765 ms | 34.790 ms | 1.131 |
| Cursor/erase | 750.796 ms | 192.682 ms | 0.257 |

The raw `simd-*.json` reports are in `upstream/benchmarks/`. ASCII and Unicode
remain slower than the reference. Scanner verification compares every byte at
each alignment and tail position against the scalar implementation, and a Linux
guard-page test checks the caller's slice boundary. This evidence applies to
that checkpoint, not to later protocol or resize changes.

The integrated row-storage checkpoint includes keyboard/mouse encodings and
reflow. [`../upstream/candidate-row-storage-build.json`](../upstream/candidate-row-storage-build.json)
identifies its exact source. Its Rust/Zig ratios were 1.095 for ASCII, 1.051 for
Unicode, 0.610 for styled text and 0.167 for cursor/erase. Replacing row-local
tree nodes and separately allocated short graphemes with sparse inline strings,
and using row references without weak owners, reduced common allocation and
ownership costs while preserving the reference fixtures.

The subsequent dirty-prefix reset checkpoint is identified by
[`../upstream/candidate-dirty-row-build.json`](../upstream/candidate-dirty-row-build.json)
and the `dirty-row-*.json` reports. Its ratios were 1.077, 1.085, 0.614 and 0.090
respectively. Short-row reset and erase improved, but Unicode regressed from
the preceding checkpoint. These measurements predate the compressed-history
and extended-style changes. No checkpoint meets the complete performance gate;
later implementations require fresh measurements.

## Native package checkpoint

The integrated package is recorded in
[`../upstream/nix-native-checkpoint.json`](../upstream/nix-native-checkpoint.json).
Its sandbox build passed all 18 application tests. The packaged executable then
passed the private headless Wayland test with ambient font/library overrides
cleared: 1,152,000-byte copy, 1,150,000-byte paste, a stalled reader, split-pane
creation/teardown and 116 exact keyboard/mouse protocol bytes. Results, runtime
library/font evidence and source hashes are preserved in
[`../upstream/native-checkpoint/`](../upstream/native-checkpoint/).
The reported 0.164-second UI interaction includes 150 ms of input-fixture setup;
it demonstrates progress during the stalled read, not a frame-latency bound.
This is a component test in Sway, not Aizome desktop VM or ISO qualification.

[`../upstream/nix-package-checkpoint.json`](../upstream/nix-package-checkpoint.json)
records an earlier successful focused Nix package build on `wx-workstation`, its native
application tests, installed executable hash, immutable source NAR hash and
installed `--help` smoke check. That build predates splits, settings, reflow,
input protocols and SIMD changes. It establishes the Cargo/Nix packaging path;
it does not qualify later source or an Aizome desktop VM.
