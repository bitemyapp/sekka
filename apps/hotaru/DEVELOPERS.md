# Developing Hotaru

Hotaru's terminal engine and Linux I/O live in this independent Cargo workspace.
The native application lives in `desktop/aizome/apps/hotaru`, within Aizome's
existing workspace, and uses `aizome-kit`/Sidestep for chrome and drawing.
Maintain that boundary; the core has no UI dependencies. The core denies unsafe
Rust except for its bounded SIMD byte scanner, whose load bounds and CPU
requirements are documented and tested against the scalar implementation.
Each Linux I/O unsafe block documents its FFI and ownership invariants.

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
Entering the Aizome development shell initializes Sidestep and applies the
tracked `upstream/sidestep-clipboard.patch`, `sidestep-input.patch`,
`sidestep-mime.patch` and `sidestep-grid-font.patch` in order. A private temporary index recognizes an already
applied prefix and validates the complete series before changing the worktree.
Conflicts preserve both staged and unstaged local changes. Nix applies the same
patches to its pinned copy. Added patch files are recognized without changing
the developer's real index or adding unrelated untracked files to Git objects.

The latest Ghostty checkout is an **external reference**, not part of Hotaru's
source or build. `upstream/ghostty.json` pins its exact commit, the old fork's
clipboard fix, and the verified Zig toolchain. Preserve `LICENSE.ghostty` with
adapted logic and fixtures.

## Workstation coordination

Heavy builds and tests run through `ssh wx-workstation`. Existing reference
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

Major outstanding work includes page/arena and compressed-history performance
parity, complete VT modes and replies, hyperlink activation, Kitty graphics/clipboard,
tmux integration, incremental live-search equivalence, complete selection behavior, Ghostty config/theme
compatibility, shell integration, terminfo, accessibility and renderer/font
behavior. Unhandled protocols must not be advertised as implemented.

Full acceptance requires all relevant Linux features, real Aizome/Wayland
large-copy/paste stress while the terminal produces output, cancellation and
teardown races, long-running fuzzing, memory accounting, and same-host Zig/Rust
throughput, latency, frame pacing, startup and resident-memory comparisons.
No whole-application performance claim follows from a parser microbenchmark.
Do not mark the goal complete while these requirements remain unresolved.
