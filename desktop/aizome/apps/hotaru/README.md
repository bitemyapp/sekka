# Hotaru native application

Hotaru (蛍, firefly) is a Linux terminal built with Aizome Kit and Sidestep.
It uses native application menus, tab controls, search fields, theme colors,
window activation and accessibility labels. It does not use GTK or a web view.

The native application is a separate Cargo workspace member from the portable
Rust terminal engine in `apps/hotaru`. It is a working component of an ongoing
Ghostty port, not a claim of Ghostty feature or performance parity. In particular,
graphics and renderer performance qualification remain
outstanding. Tabs contain independently focused, draggable split panes. Keyboard
and mouse protocols are encoded on the PTY owner using the running program's
negotiated modes; component tests cover Kitty press/release and SGR mouse input.

## Ownership and responsiveness

Find searches the active pane's captured scrollback and visible screen on a
bounded worker. Matching folds ASCII case, includes overlaps and spans soft
wraps and hard newlines. Next (❖G or Enter in Find) moves toward older matches;
Previous (❖⇧G) moves toward newer matches, wrapping at either end. Counts include
all matches, while navigation retains at most the newest 10,000. The selected
match copies from its full owned capture even when it extends beyond the visible
viewport. Ordinary pointer selection replaces that copy context.

Live output refreshes through a coalesced timer. Historical anchors survive
retained rows moving under append/eviction; changed active rows, reflow and
cleared history require fresh results. The controller retains only primary and
alternate captures for the active pane. Switching panes or queries cancels
obsolete work. Projection and multiline/wide-cell highlights run on the font
worker against the exact displayed snapshot.

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

Programs can use the clipboard themselves with OSC 52, as editors and tmux do,
even over SSH. Writing is allowed and reading is asked about by default, as in
Ghostty; Settings → General chooses Ask, Allow or Never for each. The question
names the program by its title and shows the first line of what it would get
or put, with its size. Return doesn't answer it, since it can arrive while
you're typing; Escape refuses. Refused reads get an empty answer so programs
don't wait; reads refused in Settings get none, as Ghostty's. The question is
about the bytes captured when the program asked: a read captures the
clipboard first, on a worker, so changing the clipboard while the question is
up doesn't change the answer. `c` names the clipboard; `p` and `s` name the
primary selection, for reads as well as writes (Ghostty reads the clipboard for
all three). Writes are decoded on the pane's PTY worker and offered byte for
byte, invalid UTF-8 and NUL included; reads answer with the selection's bytes as
offered. Each pane may have 16 requests waiting, holding 32 MiB at most, and a
read answers with 6 MiB at most (8 MiB once encoded, the most a write carries).
One question shows at a time; another program's request meanwhile is refused.

Selecting text with the pointer makes it the primary selection, which a middle
click pastes, here and in other programs; Copy on select in Settings turns
that off. A double click selects a word (Ghostty's word boundaries), a triple
click the whole line across soft wraps. A right click opens a menu of the
pane's commands, with Open Link and Copy Link over a link. While a program
tracks the mouse, these clicks are the program's; Shift takes them back.
Files dropped on a pane paste as their paths, quoted for the shell; dropped
text pastes as typed.

Programs can notify the desktop with OSC 9 (iTerm2's form) or OSC 777
`notify`, as long-running commands often do when they finish. Hotaru passes a
notification on while its pane isn't the one in use, once a second at most,
titled by the program or its pane; Settings turns them off.

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
Close Tab removes all panes in that tab. Window → Zoom Pane lets the focused
pane fill its tab until it's toggled again, a pane is split or closed, or focus
moves to another pane; the status bar says so while it lasts. The Window menu
also steps through tabs and panes and lists the tabs by title. Shift-drag
selects text while a running program captures the mouse; Shift-wheel scrolls
local history.

When the shell or program in a pane ends, Control-D at the prompt for one,
its pane closes, then its tab with its last pane, and the window with its last
tab; the panes beside it and the tab in front stay as they were. A program that
fails as it starts (a non-zero status within 250 ms, as Ghostty reckons it)
leaves its pane, so its error can be read, until a key is pressed. Keep ended
panes in Settings keeps every pane that way.

Edit → Clear Screen (❖K) does what Ghostty's `clear_screen` does: the scrollback
goes, and at a prompt marked by shell integration so does the screen, with a
form feed asking the shell to draw its prompt again; otherwise the cursor's
line moves to the top. A program's alternate screen is left alone. View →
Reset Terminal (⌥❖R) is a hard reset (RIS) for a terminal a program left in a
strange state, including any escape sequence it was partway through.

Packaged sessions use Hotaru's private terminfo entry. Default Bash, Zsh and
Fish shells can report their working directory without changing user dotfiles;
new tabs and splits inherit an accepted local directory. Explicit commands
keep their arguments and startup behavior. See the [shell integration notes](../../../../apps/hotaru/docs/shell-integration.md)
for startup details and the after-config fallback for Zsh frameworks that
replace prompt hooks.

Hotaru → Settings… (❖,) opens a window of four pages, laid out as Settei's:
General (shell integration, scrollback, keeping ended panes, notifications,
copy format, copy on select, what programs may do with the clipboard, the file
itself), Text
(family, size, weight, ligatures, fallback), Appearance (Follow Desktop, Night,
Day or High Contrast, each shown as a small terminal in its colours; cursor
shape and blinking) and Shortcuts. Changes reach every pane as they're made and
are saved at once. The family list holds the monospaced families installed,
found on a worker the first time Settings opens. ❖W in Settings closes Settings.

Shortcuts lists every command with its keys. The pencil opens a command's row:
the next key pressed with ❖, ⌃ or ⌥ (or a function key, or ⇧ with Page Up/Down,
Home or End) is added, taken from any other command that had it; × takes a key
away (a default is written as `ignore`); Escape or Done closes the row. While a
row is open the menus give their shortcuts up, so ❖T can be typed rather than
opening a tab. Keys the desktop keeps (⌥❖ and the arrows, ❖Tab, ❖Space, ⌃❖ with
digits…) and plain ⌃ letters, which programs need, are refused with a reason.
Restore Defaults empties `[keybindings]`.

Settings live at `$XDG_CONFIG_HOME/aizome/hotaru.toml`, falling back to
`~/.config/aizome/hotaru.toml`. `--config-file FILE` selects the configuration for
the initial application instance. Settings and the View menu's weight and
appearance choices save through a bounded background worker. ❖+ and ❖− zoom
the window's text for now, as Ghostty and foot do; Actual Size (❖0) returns to
the size Settings keeps. Hotaru → Reload Configuration (⇧❖,) applies file
edits; invalid settings leave the current configuration intact. A managed
symbolic link (a system configuration's) is read, never replaced: Settings says
changes last until Hotaru quits.

```toml
font_size = 13.0 # Typographic points, before output scale; Ghostty's default.
font_weight = 400
font_families = ["IBM Plex Mono"]
font_fallback = ["Noto Sans Mono CJK JP", "Noto Sans Symbols 2"]
font_emoji = ["Noto Color Emoji"]
font_features = [] # For example ["liga=0", "calt=0"], what Ligatures off writes.
theme = "desktop" # desktop, night, day, high-contrast
cursor_style = "block" # block, bar, underline; programs may choose another.
cursor_blink = true
scrollback = 10000 # Applies to newly created panes.
copy_format = "mixed" # plain, vt, html, mixed
shell_integration = true # Future default-shell panes only.
clipboard_read = "ask" # allow, ask, deny: programs reading it (OSC 52).
clipboard_write = "allow" # allow, ask, deny: programs writing it.
copy_on_select = true # Selections become the primary selection.
desktop_notifications = true # OSC 9 and OSC 777 notify.
wait_after_command = false # Keep panes whose program ended, until a key.

[keybindings]
"super+d" = "split-right"
"super+shift+d" = "split-down"
"ctrl+shift+o" = "ignore"
```

Terminal fonts are discovered and shaped on a separate worker. The primary
face controls a grid rounded in physical pixels; fallback faces, combining marks,
emoji and ligatures retain their terminal cell positions. Explicit families,
fallback order and OpenType features are configurable. The Nix wrapper supplies
licensed CJK and color emoji files through `HOTARU_FONT_DIR`, registered only in
this process without replacing system font discovery. Debug logs record the
resolved primary family, physical grid and unavailable requested families.
See [the font audit and implementation notes](../../../../apps/hotaru/docs/fonts.md)
for remaining limitations and fixture provenance.

Keybindings accept Super, Ctrl, Alt and Shift modifiers plus a character, named
navigation key or F1–F35; shifted punctuation is named by its unshifted key
(`super+shift+[` is ⇧❖[, whatever the layout types). Aizome's Super shortcuts are
the defaults; familiar Linux aliases (Ghostty's, GNOME Terminal's and those of
Aizome's foot configuration) also work. Plain Control-C remains terminal input.
⌥❖ and the arrows fill and split windows on Aizome's desktop, so moving between
panes is ⇧❖ and the arrows.

| Action | Default keys |
|---|---|
| `settings`, `reload-config` | ❖, · ⇧❖, |
| `new-tab`, `split-right`, `split-down` | ❖T, ⌃⇧T · ❖D, ⌃⇧O · ⇧❖D, ⌃⇧E |
| `close-pane`, `close-tab`, `zoom-pane` | ❖W, ⌃⇧W · ⌥❖W · ⇧❖↩ |
| `previous-tab`, `next-tab` | ⇧❖[, ❖PgUp, ⌃⇧Tab, ⌃PgUp · ⇧❖], ❖PgDn, ⌃Tab, ⌃PgDn |
| `select-tab-1` … `select-tab-8`, `last-tab` | ❖1 … ❖8 · ❖9 |
| `previous-pane`, `next-pane` | ❖[ · ❖] |
| `focus-left/right/up/down` | ⇧❖←→↑↓, ⌃⌥←→↑↓ |
| `copy`, `paste`, `copy-screen`, `copy-all` | ❖C, ⌃⇧C · ❖V, ⌃⇧V · none · ⇧❖C |
| `find`, `find-next`, `find-previous`, `find-selection` | ❖F, ⌃⇧F · ❖G · ⇧❖G · ❖E |
| `clear-screen`, `reset-terminal` | ❖K · ⌥❖R |
| `larger-text`, `smaller-text`, `actual-size` | ❖+, ❖=, ⌃+, ⌃= · ❖−, ⌃− · ❖0, ⌃0 |
| `scroll-to-top`, `scroll-to-bottom` | ❖Home · ❖End |
| `scroll-page-up`, `scroll-page-down` | ⇧PgUp · ⇧PgDn |

`ignore` silences a key, and Settings' × writes it for a default.

Hover an OSC 8 hyperlink to see its actual target in the status bar; hold Super
to underline it and click to open it in the desktop handler. Both mouse edges
are consumed, including when a gesture is cancelled. Movement, leaving the
widget, releasing Super, or changing the displayed terminal frame cancels the
press. Ordinary pointer selection and terminal mouse reporting keep working.
Targets must be valid UTF-8 URIs without control characters; they are passed as
one literal argument to `xdg-open`, never evaluated by a shell. Aizome Kit queues
at most eight outstanding opens, with at most four opener processes active, and
spawns/reaps them off the UI thread. Queue rejection and opener failures appear
in the status bar. Closing the application cancels queued opens; already started
desktop handlers are not terminated during normal shutdown.

Plain URLs and paths can also be opened with Super-click after their hover
match appears. Relative paths use the shell's last accepted working-directory
report. Detection follows complete logical lines within the displayed text;
clipped lines and lines beyond the matching limits are unavailable. Explicit
OSC 8 links take precedence. See the [automatic-link coverage and limits](../../../../apps/hotaru/docs/automatic-links.md).

## Component validation

Run on `wx-workstation` through SSH, from its repository mirror and using the
repository's Rust toolchain:

```sh
SIDESTEP_BACKEND=null cargo test --manifest-path desktop/aizome/Cargo.toml -p aizome-hotaru --locked -- --test-threads=1
cargo clippy --manifest-path desktop/aizome/Cargo.toml -p aizome-hotaru --all-targets --no-deps -- -D warnings
cargo build --manifest-path desktop/aizome/Cargo.toml -p aizome-hotaru --locked
```

Tests cover native Night/Day rendering, explicit terminal colors, tabs and splits,
the Settings window and shortcut recording, tab and pane navigation, zoom,
panes closing as their programs end, OSC 52 policies and exact answers, word and
line selection into the primary selection, middle-click paste, the pane menu,
dropped files,
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

`tests/wayland/fonts.py BINARY EVIDENCE_DIRECTORY` changes the private
compositor's scale through 1, 1.25, 2 and back to 1, verifies physical cell and
PTY pixel dimensions, and records CJK/emoji font hashes and screenshots. It
uses the wrapper's fallback bundle or an explicit `HOTARU_FONT_DIR` for Cargo
builds. The ignored native color test and Sidestep grid fixtures exercise the
same pinned fonts; see the linked font notes for their exact commands.

`tests/wayland/search.py BINARY EVIDENCE_DIRECTORY` types a query using the native
keyboard path, verifies whole-history counts, follows Next/Previous shortcuts
through wraparound, copies the selected historical match, and replaces the query.
It saves the native inspection, screenshots, binary hash and result evidence.

`tests/wayland/shell.py BINARY EVIDENCE_DIRECTORY` launches the packaged default
shell with an isolated home, types a directory change containing spaces and
Japanese characters, then verifies a new split's startup cwd. It checks the
installed resources and compares the private terminfo with a fresh compilation.

`tests/wayland/links.py BINARY EVIDENCE_DIRECTORY` uses a disposable fake
`xdg-open` on its explicit PATH. Native Super-click opens one exact URI argument,
including from a wide cell's continuation; plain clicks, movement, stale frames
and malformed UTF-8 do not open it. The fixture records hover rendering and
verifies a failed opener's status, without launching external applications.

`tests/wayland/settings.py BINARY EVIDENCE_DIRECTORY` drives the shortcuts and
the Settings window through a real XKB keymap and a virtual pointer: ❖K clears
history (checked through Copy All), ❖1, ❖2, ❖9, ⇧❖[, ⇧❖] (which type braces) and
⌃Tab move between three tabs, ⇧❖↩ zooms a split, ❖, opens Settings, the Day card
is clicked and the terminal window beside it is checked by its pixels, ⌥❖R is
recorded for New Tab and then opens one, ❖W in Settings leaves both panes, and
⇧❖, reloads an edited file into a new grid size. It saves screenshots, the
inspection of both windows and the result. For a Cargo build, pass a wrapper
that sets the development shell's `LD_LIBRARY_PATH` for Hotaru alone, and run
the script itself outside that shell so the system's Sway starts.

`tests/wayland/selections.py BINARY EVIDENCE_DIRECTORY` runs a raw-mode fixture
in each pane, driven through a FIFO, against the private compositor's real
clipboard and primary selection (wl-clipboard): OSC 52 writes arrive byte for
byte, `61 00 62 ff` included, and `p` on the primary selection; a read is asked
about with its preview, then allowed and answered exactly, then refused and
answered empty; a double click puts a word on the primary selection and a
middle click pastes another program's; Control-D closes a split, then a tab,
then Hotaru, which exits with status zero. The same Cargo-build wrapper applies.
