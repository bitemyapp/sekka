# Developing Hotaru

Start with the [usable checkpoint](docs/usable-checkpoint.md) and
[next work items](docs/next-work.md). The latter records the required migration
to Sekka main's first-party Sukumo toolkit before this branch can merge, along
with deferred prototype recovery and validation gates. The build layout below
describes this branch's preserved checkpoint.

Hotaru's terminal engine and Linux I/O live in this independent Cargo workspace.
The native application lives in `desktop/aizome/apps/hotaru`, within Aizome's
existing workspace, and uses `aizome-kit` and Sukumo (`desktop/aizome/sukumo`,
Aizome's toolkit) for chrome and drawing.
Maintain that boundary; the core has no UI dependencies. The core denies unsafe
Rust except for its bounded SIMD byte scanner and whole-cell ASCII stores.
The scanner's load bounds and CPU requirements are documented and tested
against the scalar implementation. ASCII stores use one possibly unaligned
eight-byte write, guarded by compile-time field type/layout assertions and
tested against ordinary constructors, boundary cases and strict-provenance Miri.
Each Linux I/O unsafe block documents its FFI and ownership invariants.
UTF-8 validation uses a bundled, audited simdutf8 release with the upstream
prefetch-pointer repair; see [the dependency audit](docs/utf8-simd.md).

The working Sekka branch is `codex/hotaru-rust-terminal`, based on
`3f4b5593c04b83ac533c7f3d48d7dc0a22839967`. The initial local worktree is
`/home/callen/work/sekka-hotaru`. Do not operate on this chat's unrelated
Aquamarine checkout or reset existing Sekka branches.

## Build

Use Rust 1.95 or newer. From Sekka's root:

```sh
cargo test --locked --manifest-path apps/hotaru/Cargo.toml
cargo clippy --locked --manifest-path apps/hotaru/Cargo.toml --all-targets -- -D warnings
cargo run --release --locked --manifest-path apps/hotaru/Cargo.toml -p hotaru-replay -- \
  --data /path/to/stream.vt --rows 80 --cols 120 --format json
nix develop .#aizome
cargo run --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hotaru
nix build .#hotaru --no-update-lock-file
```

The focused Nix package builds Hotaru without compiling the Aizome compositor.
The desktop's default terminal remains unchanged during qualification.
What Hotaru needed of the toolkit, once six patches over Sidestep (clipboard
MIME bundles and bounded reads, input, MIME, the font grid, sprites, and the
primary selection with raw-byte clipboard text), now lives in Sukumo's own
sources: edit them there, as any Aizome toolkit change. Kitty's clipboard
protocol added a snapshot's type listing and per-type reads
(`ClipboardRead::types`, `read_type`) and offers of a program's own types
(`PreparedClipboard::from_representations`). Git history keeps the
former `upstream/sidestep-*.patch` files that historical receipts name.

The latest Ghostty checkout is an **external reference**, not part of Hotaru's
source or build. `upstream/ghostty.json` pins its exact commit, the old fork's
clipboard fix, and the verified Zig toolchain. Preserve `LICENSE.ghostty` with
adapted logic and fixtures.

## Workstation coordination

All builds, tests and benchmarks run through `ssh wx-workstation`, including
GPU diagnostics. The user reaffirmed this on 2026-10-28 because local runs
were slowing the laptop; do not use local fallback runs. Local source editing,
review and evidence inspection remain available. Existing reference
and working directories for this effort are:

| Remote directory | Purpose |
| --- | --- |
| `/home/callen/work/ghostty-reference-20261028` | Unchanged upstream, ReleaseFast benchmark binary and libghostty-vt |
| `/home/callen/work/hotaru-core-tests-20261028` | Core/I/O tests and replay executable |
| `/home/callen/work/sekka-hotaru-20261028` | Native app mirror, necessary Aizome/Sidestep/Yukimi paths only |
| `/home/callen/work/hotaru-reference-tools` | Differential and benchmark runners |
| `/home/callen/work/hotaru-reference-fixtures` | Generated upstream oracle outputs and comparison reports |
| `/home/callen/work/hotaru-rich-copy-checkpoint-20261028` | Frozen core snapshot, formatter and hyperlink checkpoint |
| `/home/callen/work/sekka-hotaru-nix-20261028` | Exact source mirror for focused Nix package validation |
| `/home/callen/work/hotaru-gui-probes` | Private Wayland GUI reference probes and original captures |

All heavy jobs take `flock -x /home/callen/.cache/hotaru-workstation.lock`.
Timing trials are serial, with no concurrent builds from this effort. Record
host load and competing processes too; the lock cannot exclude unrelated jobs.
Keep generated corpora and build outputs out of Git. The filesystem had roughly
43 GiB available when setup began; check space before large builds.

## Acceptance and remaining scope

The machine-readable source inventory is `upstream/modules.json`. Most source
modules remain pending. Initial parser/grid implementations cover common DEC
controls, erasure, margins, styles, UTF-8, basic graphemes and primary/alternate
screens. The core uses eight-byte cells, sparse grapheme data and row-local
style indices. Immutable snapshots retain rows through a strong-reference-only
`RowRef`; mutations use copy-on-write. Sparse sorted grapheme entries retain
their allocation on row reuse, and short clusters fit in inline strings.
Resetting a recycled row clears its dirty prefix and preserves the already blank
suffix. Tests compare this optimization with eager full-row clearing.
Cold history uses bounded safe LZ4 blocks; see [history memory measurements](docs/history.md).
The current allocator still retains significant RSS after compression, so this
is not evidence of parity with Ghostty's page allocator.

Logical-line reflow, keyboard/mouse protocols, extended styles and several
device/status replies now have pinned-reference fixtures. Native splits and
basic saved settings are implemented. These are partial subsystem ports;
broader compatibility and platform qualification remain open.

OSC 8 links and plain/ANSI/HTML/mixed formatting now have independent upstream
fixtures. One explicit difference corrects invalid crossing HTML tag scopes;
raw upstream evidence is retained. Copy All captures owned history blocks in
O(blocks + visible rows), with bounded decoding and formatting on the clipboard
worker. The native selection publishes prepared plain and HTML representations
together. See [hyperlinks and formatting](docs/hyperlinks.md) and
[history capture](docs/history.md) for coverage and ownership details.
The [GUI probe](docs/gui-probe.md) documents the full Linux reference build and
the renderer/font gaps exposed by exploratory application measurements.
The [owned search core](docs/search.md) has 249 real-library selection fixtures
checked at three input chunk sizes. Native Find searches retained history on a
worker, validates navigation against row identities and projects highlights onto
the exact render snapshot. Broader live-search equivalence and final integrated
latency/memory acceptance remain open.

The [shell integration slice](docs/shell-integration.md) provides opt-out
Bash, Zsh and Fish hooks, validated local OSC 7 working directories and a
qualified terminfo subset. Default shell tabs and splits inherit the last
accepted local working directory. The [semantic metadata slice](docs/semantic-prompts.md)
now retains observed OSC 133 cell/row markers through edits, history and reflow,
and implements DECALN. Semantic shell hook emission, command navigation, complete semantic
extensions, remote shell installation and broader terminfo remain open. The packaged
checkpoint and four independent Wayland suites are recorded in
[`upstream/nix-shell-checkpoint.json`](upstream/nix-shell-checkpoint.json) and
[`upstream/native-shell/`](upstream/native-shell/).

The [history memory audit](docs/history-budget-memory.md) compares equal
retained content and documents Ghostty's per-page hyperlink-capacity loss.
The [page storage proposal](docs/history-page-storage.md) is a design, not a
shipped memory improvement. The [UTF-8 scanner audit](docs/utf8-scanning.md)
records the integrated linear-time malformed-input fix and rejected performance
experiments. The later [SIMD audit](docs/utf8-simd.md) records the repaired
dependency now included in both workspaces and the package.

Major outstanding work includes page/arena and compressed-history performance
parity, complete VT modes and replies, Kitty graphics,
tmux integration, incremental live-search equivalence, complete selection behavior, Ghostty config/theme
compatibility, complete shell integration and terminfo, accessibility and renderer/font
behavior. Unhandled protocols must not be advertised as implemented.

Full acceptance requires all relevant Linux features, real Aizome/Wayland
large-copy/paste stress while the terminal produces output, cancellation and
teardown races, long-running fuzzing, memory accounting, and same-host Zig/Rust
throughput, latency, frame pacing, startup and resident-memory comparisons.
No whole-application performance claim follows from a parser microbenchmark.
Do not mark the goal complete while these requirements remain unresolved.

The [VT control slice](docs/vt-controls.md) adds repetition and supported DEC
mode save/restore, with corrected alternate-screen behavior and decoded-C1
handling. Native OSC 8 activation uses the displayed snapshot and a bounded
desktop-opener worker; [its scope and tests](../../desktop/aizome/apps/hotaru/README.md)
remain distinct from full selection/URL-detection compatibility.

[Automatic URL and path detection](docs/automatic-links.md) now uses the pinned
default rule on bounded logical context from the exact displayed snapshot.
Worker results and path-helper completion are checked against the original
request and gesture before opening. Clipped scrollback context and configurable
rules remain open; the software Wayland qualification is not a new combined
package or whole-application performance checkpoint.

The [combined package checkpoint](docs/links-vt-checkpoint.md) records the
exact package and source hashes for this slice, 38 ordinary native tests and
five independent Wayland suites. Its software-compositor qualification does
not establish GPU, physical-desktop or whole-application performance parity.

The [cursor/cache package checkpoint](docs/cursor-cache-checkpoint.md) adds
[cursor shapes and bounded blink timers](docs/cursor.md),
[incremental shaped-row reuse](docs/renderer-row-cache.md),
[unchanged grapheme-charge avoidance](docs/grapheme-charge.md), and
[direct recycled-row construction](docs/history-row-construction.md).
The combined core passes 136 tests in both debug and release, strict Clippy,
and 6,174 pinned-library comparisons; exact identities and scope are in
[`upstream/combined-core-cursor/summary.json`](upstream/combined-core-cursor/summary.json).
The package passes 44 native tests and six independent Wayland suites.
The [frame harness](docs/frame-benchmark.md) measures actual private-compositor
captures and records observer overhead separately. Its tool qualification and
the isolated shaping improvements do not establish application performance parity.

Subsequent integrated changes add [native pixel scrolling](docs/pixel-scroll.md)
with exact partial/full framebuffer checks, and
[packed ASCII cell stores](docs/ascii-cell-store.md) with 139 standalone tests,
strict Clippy, Miri and direct reference comparisons. Their isolated validation
does not change the historical package identities above; the next combined
package must be qualified separately. The [GPU buffer probe](docs/gpu-buffer-capability.md)
now passes an offscreen Intel DMA-BUF round trip locally, while the workstation
NVIDIA export limitation remains. A production GPU renderer is still pending.

The subsequent [sprite package checkpoint](docs/sprites-checkpoint.md) qualifies
the combined ASCII, pixel-scroll and 153-sprite implementation: 56 native tests,
eight private Wayland suites, 226 local source comparisons and 220 prepared-source
matches. It records a separate package identity and the remaining styled
block-cursor decoration gap; earlier checkpoint identities remain historical.
