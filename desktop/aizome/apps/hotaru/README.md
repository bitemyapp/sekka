# Hotaru native application

Hotaru (蛍, firefly) is a Linux terminal built with Aizome Kit and Sidestep.
It uses native application menus, tab controls, search fields, theme colors,
window activation and accessibility labels. It does not use GTK or a web view.

The native application is a separate Cargo workspace member from the portable
Rust terminal engine in `apps/hotaru`. It is a working component of an ongoing
Ghostty port, not a claim of Ghostty feature or performance parity. In particular,
full scrollback search, graphics and renderer performance qualification remain
outstanding. Tabs contain independently focused, draggable split panes. Keyboard
and mouse protocols are encoded on the PTY owner using the running program's
negotiated modes; component tests cover Kitty press/release and SGR mouse input.

## Ownership and responsiveness

Each pane has one PTY worker. That worker owns terminal state, feeds output through
the Rust parser, applies input and resize commands, and publishes immutable row
snapshots. The renderer retains the preceding snapshot while comparing row
identity for damage, so recycled allocations cannot hide changed text.

The worker bounds aggregate pending user input to 32 MiB and reports rejection
when that budget or its command queue is full. Protocol replies have their own
64 KiB budget, plus at most one complete reply dispatch. When that budget fills,
the worker retains unconsumed PTY bytes and stops reading until writes make room.
It never discards replies to a live recipient. Shutdown uses an atomic stop flag
and eventfd wake independently of command queue capacity.

Copy and paste use one clipboard worker with a single replaceable pending job.
Copy All captures immutable history blocks without decompressing them on the PTY
owner. Formatting and block decoding happen on the clipboard worker. The UI
publishes all prepared MIME formats together in constant time. Reads have a two second deadline and a
size limit; obsolete jobs and offers are cancelled. Completion tokens prevent an
older copy from replacing a newer one, and paste remains attached to the pane that
requested it. Paste encoding runs on the PTY worker using its current mode and
strips embedded control sequences that could terminate bracketed paste.

Copy defaults to plain text plus HTML. Edit → Copy Format also offers plain text,
ANSI text with terminal formatting, and HTML alone. The choice is saved. A new
copy or a closed pane cancels obsolete work. Copies exceeding 128 MiB across all
requested formats report an error without replacing the clipboard or truncating
the selection. Plain aliases and the local text cache share one allocation.

OSC 52 requests are reported but do not grant programs clipboard access.
Colors are configured in the terminal engine itself, so OSC queries and rendered
colors agree. Aizome appearance changes preserve explicit program overrides;
OSC color resets restore the current configured defaults.

Older history is compressed in bounded worker slices after activity has settled
for 100 milliseconds. This retains owned snapshots and avoids doing compression
on the UI thread. Allocator behavior can retain freed pages, so this is not a
claim of reduced resident memory.

## Panes and settings

File → Split Right/Down divides the focused pane equally and focuses the new
session. Native dividers resize the panes. Close Pane removes just that session
and focuses its previous neighbor, or its next neighbor when it was first.
Close Tab removes all panes in that tab. View contains directional and sequential
pane focus actions. Shift-drag selects text while a running program captures the
mouse; Shift-wheel scrolls local history.

Settings live at `$XDG_CONFIG_HOME/aizome/hotaru.toml`, falling back to
`~/.config/aizome/hotaru.toml`. `--config-file FILE` selects the configuration for
the initial application instance. View saves text size, weight and appearance
changes through a bounded background worker. File → Reload Configuration applies
file edits; invalid settings leave the current configuration intact. Managed
symbolic links are readable but are not replaced by automatic saves.

```toml
font_size = 14.0
font_weight = 400
theme = "desktop" # desktop, night, day, high-contrast
scrollback = 10000 # Applies to newly created panes.
copy_format = "mixed" # plain, vt, html, mixed

[keybindings]
"super+d" = "split-right"
"super+shift+d" = "split-down"
"ctrl+shift+o" = "ignore"
```

The font uses Aizome's native monospace family. Arbitrary font-family selection
is not exposed by the current toolkit. Keybindings accept Super, Ctrl, Alt and
Shift modifiers plus a character, named navigation key or F1–F35. Actions include
`new-tab`, `close-pane`, `close-tab`, `split-right`, `split-down`, `previous-pane`,
`next-pane`, `focus-left/right/up/down`, `copy`, `paste`, `copy-all`, `find`,
`larger-text`, `smaller-text`, `actual-size`, `reload-config` and `ignore`.
Aizome's Super shortcuts are the defaults; familiar Linux Ctrl-Shift-O/E and
Ctrl-Shift-C/V aliases are also available. Plain Control-C remains terminal input.

## Component validation

Run from the repository root, using the repository's Rust toolchain:

```sh
SIDESTEP_BACKEND=null cargo test --manifest-path desktop/aizome/Cargo.toml -p aizome-hotaru --locked -- --test-threads=1
cargo clippy --manifest-path desktop/aizome/Cargo.toml -p aizome-hotaru --all-targets --no-deps -- -D warnings
cargo build --manifest-path desktop/aizome/Cargo.toml -p aizome-hotaru --locked
```

Tests cover native Night/Day rendering, explicit terminal colors, tabs and splits,
large asynchronous plain/HTML copies, cancellation and stale results, output delivery before shell exit,
copying after exit, settings validation/persistence, negotiated input and 50,000
terminal queries under PTY backpressure. Ignored child fixtures are executed as
isolated PTY children by the corresponding parent tests.

`tests/wayland/smoke.py BINARY EVIDENCE_DIRECTORY` creates a private headless Sway
session and tests exact plain text and HTML copies plus paste above 1 MiB, native UI input
while recipients stop reading, canceled receivers, repeated copies followed by
pane closure, nested split teardown, and actual
Wayland keyboard/mouse input. It records binary/source hashes,
result hashes, a screenshot and logs, then terminates its private processes. It
requires Sway, wl-clipboard, grim and the Wayland C development tools documented
in the script. These component checks do not qualify an Aizome desktop VM or ISO.
