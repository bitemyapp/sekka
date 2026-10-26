# Aizome architecture

Aizome 藍染 is Sekka's desktop environment, built on Sidestep's native Rust
toolkit (`sidestep-ui` on `sidestep-engine`). The design anchor is the
`aizome-design` repository (`~/work/aizome-design`, github.com/bitemyapp/aizome-design):
its `boards/` are the settled look, `tokens/aizome-tokens.json` is the source
of truth for colour, type and metrics, and `reference/html/C2-*.html` hold the
boards' exact CSS. This document is the contract between the parts.

## Parts and processes

| Part | Crate | Binary | What it is |
|---|---|---|---|
| Compositor core | `crates/aizome-comp-core` (`aizome-comp` is its binary) | `aizome` | smithay 0.7 Wayland compositor's stable part, a Rust dylib: clients, protocols, input, outputs, backends and rendering, IPC server, session startup and supervision, the recovery overlay; loads the logic and swaps it live (`docs/reload.md`) |
| Compositor logic | `crates/aizome-comp-logic` | `libaizome_comp_logic.so` | What happens on the desktop, reloadable without disconnecting a client: window management, Haiku tabs and frames (drawn server-side), tab groups, Desks, the Desks overview, the ❖Tab switcher, the tab marking menu, bindings, focus, motion |
| Shell | `crates/aizome-shell` | `aizome-shell` | One sidestep-ui process with layer-shell surfaces: global menu bar, Toolchest, Desks strip, systray widgets, Deskbar, Shelf, command palette, NeXT menus at the pointer, tear-off palettes, notifications (it is the `org.freedesktop.Notifications` server), OSD, desktop icons and wallpaper, lock screen (ext-session-lock), polkit agent |
| Control | `crates/aizome-ipc` | `aizomectl`, `aizome-dev` | IPC types for compositor and shell, a blocking client, the CLI (state, actions, screenshots, frame stats, reload), and `aizome-dev`, which builds a checkout as the session was built and reloads it live |
| Theme | `crates/aizome-theme` | — | Tokens: palettes (Night, Day, High Contrast), focus colours, effects tiers, metrics, type, the mark |
| Config | `crates/aizome-config` | — | `~/.config/aizome/settings.toml`: schema, defaults, load/save, change notification |
| Kit | `crates/aizome-kit` | — | Widget kit on sidestep-ui in Aizome's look: layout, controls, lists, text fields, menus (published to the global menu bar), sidebars, sheets, icons |
| System | `crates/aizome-sys` | — | System services over D-Bus and the kernel: NetworkManager, BlueZ, PipeWire volume, UPower, brightness, power profiles, timedate/locale/hostname, AccountsService, CUPS, logind, MPRIS, disks (UDisks2) |
| Search | `crates/aizome-search` | — | Fuzzy matching and the file index behind the palette and Find |
| Apps | `apps/*` | one each | Settei (Settings), Yukimi, Soroban, Koyomi, Tokei, Fusen, Sumi, Tenki, Tansu, Kura, Hako, Myaku |

Everything draws with Sidestep: the shell and apps are sidestep-ui programs,
and the compositor renders its tab labels, overview captions and switcher
text with sidestep-ui's offscreen `Canvas`, so text in a tab and text in a
window come from the same engine.

## Sidestep

Aizome builds against the `aizome` branch of Sidestep (worktree
`~/work/sidestep-aizome`; development happens on `aizome-dev` in
`~/work/sidestep-aizome-dev` and is fast-forwarded into `aizome` when its
tests pass). That branch adds, without changing AppKit's behaviour:

- layer-shell surfaces (`WindowOptions::layer(..)`), with popups;
- session-lock surfaces (`Cx::lock_session`, `WindowOptions::lock(..)`);
- font width (`Font::with_width`), for Archivo at 87.5%;
- offscreen rendering of a `Canvas` into pixels (`sidestep_ui::offscreen`);
- server-side decorations when the compositor offers them.

## Look

From the boards (`aizome-desktop-night@2x.png` is 2880 × 1800 at 2×, so
divide its pixels by two for points):

- **Menu bar** 24pt, `bar` colour, 1pt `bevel-lo` bottom edge. Left to right:
  the Toolchest button (the mark, then "Toolchest"), the focused app's name
  in bold, its menus; the Desks strip (`1 Mail  2 Build  3 Write  +`, the
  current desk on `selection`); then the systray (CPU and memory meters,
  network, sound, battery) and the clock in IBM Plex Mono semibold with
  tabular digits. With the Deskbar collapsed the strip shows `N apps ⌄`.
- **Deskbar** 232pt wide at the top right under the bar, `chrome`: each app
  (icon, bold name, window count), its windows indented beneath (the focused
  one marked by a 3pt amber bar, hidden ones in italic `text3` with
  "hidden", stacked ones "stack", ones on another desk "desk N"), unread
  notifications under their app (count in `red`, newest previewed in a
  `sunk` card), then SHELF: parked drags and a dashed "Drop to park" target.
- **Windows**: a Haiku tab 22pt tall above the frame's top-left, only as wide
  as its title: a 10pt square close box, the title in Archivo 650 at 87.5%
  width, 12pt, and a zoom box at the right end. Focused: `focus` (amber)
  with `on-focus` text, and a 2pt `focus` rail along the frame's top edge.
  Unfocused: `tab-idle` with `tab-idle-text`, and an app glyph instead of
  the close box. Frames are square with a 1pt `bevel-lo` edge and a 1pt
  `bevel-hi` top highlight, and cast a hard shadow (3pt right and down,
  `slab`, never blurred) that includes the tab.
- **Tab groups**: stacked windows show their tabs side by side along one
  top edge; one window shows at a time. Dragging a tab over another
  window's tab shows a dashed amber slot and a "Release to stack with …"
  label. Tabs slide along the top edge (⇧-drag a tab, as in Haiku).
- **Menus**: `raised` rows 24pt, `selection` highlight, shortcuts right
  aligned in IBM Plex Mono with ❖ ⇧ ⌥ ⌃ from Noto Sans Symbols 2. Menus at
  the pointer and torn-off palettes have a `menu-title` strip with the
  menu's name, a close box and a pin. Hard 3pt shadows.
- **Desktop**: icons down the left (home, disks, mounted images, Trash),
  labels on `desktop-label` chips; wallpaper per theme.

Night is the default theme; Day is IRIX's nod; High Contrast exists for low
vision. Effects tier Lean is the default (hard shadows, 80ms motion, no
frost). Motion moves layers and never repaints; reduced motion means none.

## Keys

Super (❖) is the command key: ❖C, ❖V, ❖Z, ❖Q, ❖W and the rest belong to
apps, as ⌘ does on a Mac. The system takes only:

| Keys | Action |
|---|---|
| ❖Space | Command palette (Strip's): every menu command of the focused app, windows, files, apps, settings |
| ❖⌥Space | Toolchest |
| ❖Tab, ❖⇧Tab | Switcher: apps, then (↓ or keep holding) that app's windows |
| ❖` | Next window of this app |
| ❖H, ❖⌥H, ❖M | Hide app, hide others, minimise window |
| ⌃❖1…9 | Go to Desk N; with ⇧, take the window along |
| ⌃❖←, ⌃❖→ | Previous, next Desk; with ⇧, take the window along |
| ⌃❖↑ | Desks overview |
| ⌥❖↑, ⌥❖↓, ⌥❖←, ⌥❖→ | Fill, park (minimise), split left, split right: the marking menu's verbs |
| ⌥❖S | Stack with the window behind |
| ⌃❖F | Full screen |
| ❖Return | Terminal (foot) in the Desk's folder |
| ⌃❖Q | Lock |
| ⌃⌥Del | Recovery: force quit stuck apps, reset input, restart the shell (`docs/recovery.md`) |
| ⌃❖R | Reload what changed: the compositor's logic, the shell, the settings (`docs/reload.md`) |
| ❖⇧3; ❖⇧4 or Print; ❖⇧5 | Screenshot: screen; area; window/tool (❖⇧5: the Capture menu, recordings too) |
| ⌥Print | Record an area or a display; again, stop |
| ❖-right-click | The focused app's whole menu at the pointer (NeXT) |
| ⌃❖-drag, ⌃❖-right-drag | Move, resize |
| media keys | Volume, mute, brightness, players, with the OSD |

Right-click on a tab opens the marking menu of window verbs (fill, park,
split ←/→, close, stack, float, to desk ▸). Right-click on empty desktop opens
the NeXT menu.

For apps that don't use Aizome's kit, the compositor translates ❖ shortcuts
into the Ctrl ones they expect (❖C → Ctrl+C; in terminals Ctrl+Shift+C), so
the command key works everywhere. The setting is "Command key for all apps"
(on by default). In configured terminals, ❖W and ❖Q instead perform the
fallback menu's Close and Quit: a polite close request to the current window
or every window of that terminal app. Other terminal keys keep their normal
translation; native and GTK apps keep their own Close/Quit handling. The
`terminal-close` / `terminal-quit` bindings can be overridden or unbound in
`keys.bindings`, and shortcut inhibitors still take precedence.

## IPC

### Compositor: `$AIZOME_SOCKET`

`$XDG_RUNTIME_DIR/aizome.$WAYLAND_DISPLAY.sock`, newline-delimited JSON. A
client sends `Request`s and gets one `Response` each; after `subscribe` the
connection also streams `Event`s. Types live in `aizome-ipc` (`docs/ipc.md`
lists them). The shell, Settings, `aizomectl` and tests use it.

### Shell: `$AIZOME_SHELL_SOCKET`

`$XDG_RUNTIME_DIR/aizome-shell.$WAYLAND_DISPLAY.sock`, the same framing. Apps
built with the kit publish their menus here (keyed by process ID, so the
menu bar shows the menus of the app whose window has focus), keep item
states current, and get `activate` back when someone picks an item from the
bar, the pointer menu, a torn-off palette or the palette. The shell also
takes `notify`, `osd`, `palette`, `lock` and `toolchest` requests from the
compositor's key bindings and from `aizomectl`.

Apps that don't publish menus still get an app menu (About, Hide, Quit) and
a Window menu (minimise, fill, stack, move to desk, close) from the shell,
which acts through the compositor.

## Config

`~/.config/aizome/settings.toml`, read by every part through
`aizome-config`. Settings (Settei) writes it and sends `reload-config` to
the compositor, which applies it and broadcasts `config-changed`; the shell
and kit apps reload on that event (kit apps also watch the file, so they
need no socket). Missing keys take defaults; unknown keys are kept.
Sections: `appearance` (theme, follow sunset, focus colour, effects, size,
reduced motion, wallpaper), `input` (keyboard layouts, repeat, pointer,
touchpad), `outputs` (by make/model/serial: mode, scale, position,
transform), `desks` (names, folders), `keys` (overrides), `menubar`,
`deskbar`, `notifications`, `lock`, `power`, `apps` (defaults), `sound`,
`recording` (screen recordings), `location` (where the computer is, for
sunrise and sunset: the shell writes what GeoClue says).

## Rendering and the 4ms budget

- The compositor renders with GLES (smithay's `GlesRenderer`) and falls back
  to pixman (software) when there is no usable GPU, or when
  `AIZOME_RENDERER=pixman` / `--renderer pixman`. Both use smithay's damage
  tracking: idle means no frames, and moving a window composites without
  re-rasterising.
- Apps and the shell rasterise on the CPU in Sidestep's render thread
  (tiny-skia, damage-tracked), so they work the same over either renderer.
- Every frame's CPU time is measured: the compositor per output (render
  plus submit), Sidestep per window (record plus rasterise). `aizomectl
  stats` prints percentiles. The budget is 4ms; exceptions are written up in
  `docs/performance.md` with the reason.

## Session

- `session/aizome.desktop` (wayland-sessions) runs `aizome-session`, which
  starts `aizome.service` in the user manager and waits for it (as niri
  does). The compositor imports its environment into systemd and D-Bus
  (`WAYLAND_DISPLAY`, `DISPLAY`, `XDG_CURRENT_DESKTOP=Aizome`, the IPC
  sockets), notifies systemd that it is ready, and starts
  `aizome-session.target`, which binds `graphical-session.target`.
- The compositor starts and keeps alive `aizome-shell` and
  `xwayland-satellite` (X11 apps).
- Live reload (`docs/reload.md`): the compositor's logic swaps between
  frames and the shell restarts beside the old one, keeping every window,
  its notifications and the apps' menus; `aizome-dev watch` does it on
  every save of a checkout. Changes to the compositor's core need a new
  session; after `nixos-rebuild switch` a notification offers the rest.
- Portals: xdg-desktop-portal-wlr for screenshots and screencasts (over
  ext-image-copy-capture, for outputs and windows, chosen with
  `aizome-share-picker`), xdg-desktop-portal-gtk
  for the rest (`aizome-portals.conf`).
- Terminal: foot. Shell: Sekka's fish (set up as Omarchy sets it up).
- The lock screen uses PAM service `aizome`.

## Repository layout

```
Cargo.toml            workspace
crates/               the parts above
apps/                 one crate per app
session/              aizome-session, aizome.desktop, systemd units (aizome-update.path), portals.conf
scripts/              rustc-wrapper (-C prefer-dynamic for the compositor's core), headless-session
nix/                  package.nix, module.nix (NixOS), fonts.nix, vm-test.nix
docs/                 this file, ipc.md, reload.md, performance.md, software-rendering.md, apps.md
```

## Validation

- Unit tests per crate.
- The compositor's headless backend (`aizome --backend headless`) renders
  outputs offscreen; `aizomectl screenshot out.png` captures them. Clients
  connect to it as to any compositor, so the shell and apps run under it in
  CI and here without touching a live session.
- `nix/vm-test.nix` boots a NixOS VM that logs into the Aizome session and
  takes screenshots (software rendering on virtio-gpu, the fallback path).
- Never open windows on the developer's live session: unset
  `WAYLAND_DISPLAY`/`DISPLAY` or point them at a headless Aizome.
