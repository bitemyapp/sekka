# The compositor

`aizome` (crate `aizome-comp`) is Aizome's Wayland compositor, on smithay
0.7: a floating, stacking window manager with Haiku tabs, tab groups,
Desks, the Desks overview, the ❖Tab switcher and the tab marking menu, the
IPC server (`docs/ipc.md`) and session start-up. This page is how to run
it, what it offers clients, and how it's built and tested. Its window
management reloads live, without disconnecting a client:
`docs/reload.md`.

## Running it

```sh
aizome                                   # udev on a TTY, winit inside another session
aizome --backend udev                    # DRM/KMS through libseat (aizome.service does this)
aizome --backend winit                   # a window in another session, for development
aizome --backend headless --headless-output 2880x1800@2 [--headless-output …]
aizome --renderer pixman                 # or AIZOME_RENDERER=pixman; GLES falls back to it
aizome --socket NAME --no-shell --no-xwayland --no-session -- foot
aizome --logic PATH                      # or AIZOME_LOGIC: another build of the logic library
aizome --build-info                      # how the core was built (what a logic must match)
```

- **Never run winit or udev on a developer's live session.** Use the
  headless backend (`scripts/headless-session` wraps it) with
  `WAYLAND_DISPLAY` and `DISPLAY` unset; `aizomectl screenshot` shows
  what it draws.
- Logging: `AIZOME_LOG` (or `RUST_LOG`), e.g. `AIZOME_LOG=debug,smithay=info`.
- Settings: `~/.config/aizome/settings.toml` (`--config PATH` for another),
  watched once a second and applied live (theme, focus colour, effects,
  keyboard, pointer and touchpad, outputs, Desks, key bindings, Night
  Light). Keys Settei keeps in the sections' extra tables are honoured
  too: `[windows] tab_double_click` (`fill`, `minimise`, `nothing`),
  `[input.pointer]` and `[input.touchpad]` `scroll_factor` (scales axis
  events, wheel and finger; `natural_scroll` turns them round in the
  compositor alone, libinput's own natural scrolling always off, so it's
  turned round once on every device), `[input.keyboard]` `sticky_keys` (a modifier
  tapped alone latches for the next key; tapped again it lets go) and
  `slow_keys` with `slow_keys_ms` (a key counts once held that long; a
  brush is dropped), and `[performance] renderer` (at the next start).
- The system's keyboard: until `[input.keyboard]` sets them, its layouts,
  variant, options and model are the session environment's
  `XKB_DEFAULT_LAYOUT`, `_VARIANT`, `_OPTIONS` and `_MODEL`, which
  Aizome's NixOS module sets from `services.xserver.xkb` (as the login
  screen's keyboard is): the layout Genkan writes, and its swap that puts
  ❖ beside the space bar. The keymap is always given its options, empty
  ones included, so libxkbcommon doesn't put `XKB_DEFAULT_OPTIONS` back
  for settings, or a keyboard of its own, that clear them.
- Keyboards of their own: each `[[input.keyboards]]` entry (`name` and
  `id`, `vendor:product`, as libinput gives them; `options`) has its own
  modifier swaps, Caps Lock and Compose key (the `altwin:`, `ctrl:`,
  `caps:` and `compose:` XKB options), in place of `[input.keyboard]`'s;
  layouts, layout switching, repeat and Num Lock are every keyboard's, and
  an entry without `options` types as other keyboards do. An exact name/ID
  match wins; otherwise a single profile for a nonzero ID covers its other
  interface names too. Multiple profiles for the same ID stay name-specific.
  The seat has one
  keymap, as Wayland has: before a key from a keyboard that types (one
  with a space bar and the letters) goes anywhere, the keymap becomes that
  keyboard's if it isn't already, and the focus hears it; Caps Lock, Num
  Lock and the layout in use carry over (`keyboards.rs`). Not yet under an
  input method: smithay gives an input method's keyboard grab
  (`zwp_input_method_keyboard_grab_v2`, IBus's or Fcitx 5's) the keymap
  only when the grab begins, and a change of keymap reaches `wl_keyboard`s
  alone. While IBus or Fcitx 5 holds the grab, it reads every keyboard's
  keys with the keymap in force when it took it, and the keys it passes on
  through its virtual keyboard carry that keymap to the app: one keyboard's
  own modifier swaps then go for all, or for none. Sending the keymap to
  the grab again after a change would need a change in smithay.
- Fonts: Archivo, IBM Plex Mono and Noto Sans Symbols 2 from fontconfig,
  else from `$AIZOME_FONT_DIR` or `../share/aizome/fonts` beside the
  binary.
- `AIZOME_DRM_DEVICE` picks the primary GPU on udev; `AIZOME_RENDER_NODE`
  the render node for headless GLES.

## Session

`aizome-session` (from `session/aizome.desktop`) starts `aizome.service` in
the user's systemd manager (or runs the compositor in a D-Bus session
without one). Once its Wayland socket and IPC are up the compositor:

- imports `WAYLAND_DISPLAY`, `DISPLAY`, `XDG_CURRENT_DESKTOP=Aizome`,
  `XDG_SESSION_DESKTOP=aizome`, `XDG_SESSION_TYPE`, `AIZOME_SESSION`,
  `AIZOME_SOCKET` and `AIZOME_SHELL_SOCKET` into systemd and D-Bus
  activation;
- tells systemd it's ready (`READY=1` on `$NOTIFY_SOCKET`) and starts
  `aizome-session.target` (bound to `graphical-session.target`);
- starts `xwayland-satellite :N` (a free display, exported as `DISPLAY`)
  and `aizome-shell` when they're on PATH, restarting them with backoff
  (1 s doubling to 30 s, reset after a minute's run); `aizomectl reload
  --shell` starts a new shell beside the running one, which takes over its
  surfaces in the same frame and its notifications (`docs/reload.md`);
- runs the command after `--`, if any.

`aizome-update.path` watches `/run/current-system`: after `nixos-rebuild
switch` a notification offers to reload what the update changed.

Every program it starts (`spawn`, ❖Return, the shell) gets that
environment.

`AIZOME_SESSION` is `1` only on udev without `--no-session`, the
machine's login session, and `0` under headless, winit or `--no-session`.
Those children still see `XDG_CURRENT_DESKTOP=Aizome`, but they run as the
same user inside the user's real session, so the shell reads
`AIZOME_SESSION` and there doesn't register as the polkit agent, take
logind's sleep delay or lock for logind (`docs/shell.md`, "Real and
headless sessions"): `scripts/headless-session` and the tests leave the
developer's session alone.

Under `aizome.service` it feeds systemd's watchdog (`WatchdogSec=10s`)
from its main loop; a loop stalled that long is stopped (SIGABRT, with a
core dump) and `aizome-session` starts it again, and a watcher thread logs
the main thread's backtrace to `~/.local/state/aizome/stall.log` first.
The service weighs `CPUWeight=1000`/`IOWeight=1000` within
`session.slice`. See `docs/recovery.md`.

## Recovery

**Ctrl+Alt+Del** opens the recovery overlay, checked before every other
key handling (bindings, grabs, shortcut inhibitors, the lock): the apps
with CPU, memory and whether they answer pings, Force Quit, Close, Reset
Input, Restart Shell, Reload Aizome, Lock, Log Out and Text Console. It
shows over full-screen clients and the lock screen (safe actions only
there) and has the keyboard and pointer while open. `docs/recovery.md`
has the details, the stall watchdog and the SysRq last resort.

Grabs end on release: the compositor gives the seat the release of every
press it gave it, whatever overlay came up between, and its own grabs
(moves, resizes, tab slides, the boxes) end when their button comes up or
is pressed again, whatever else is held and whatever any client does.
The release of a press an overlay took is the overlay's, and nobody's
once that press has closed it (a window pick ends on the press), not
the client's under the pointer, which never saw the press.

## What clients get

Globals: wl_compositor (6), wl_subcompositor, wl_shm, linux-dmabuf (GLES;
with feedback), wl_output (4) and xdg-output, xdg-shell, xdg-decoration
(server-side by default; clients may insist on client-side and then get
the tab only), xdg-dialog, xdg-activation, xdg-foreign, wlr-layer-shell,
ext-session-lock, wl_seat (keyboard, pointer, touch), data-device,
primary-selection, wlr and ext data-control, viewporter,
fractional-scale, presentation-time, single-pixel-buffer, cursor-shape,
idle-notify, idle-inhibit, keyboard-shortcuts-inhibit, pointer-constraints,
relative-pointer, pointer-gestures, text-input-v3, input-method-v2,
virtual-keyboard, security-context, content-type, tablet, alpha-modifier,
fifo, commit-timing, xdg-toplevel-icon, xdg-system-bell (rung as the
shell's `bell` binding), ext-foreign-toplevel-list,
wlr-foreign-toplevel-management (3), wlr-output-management (4),
wlr-screencopy (3), ext-image-copy-capture (1) with ext-image-capture-source
(1), wlr-gamma-control (1) and Aizome's own toplevel identity (1), which
tells a client the ext-foreign-toplevel-list identifier of each of its
toplevels as it maps (`aizome-toplevel-identity-v1.xml`, in Sukumo's
`sidestep-engine`). Privileged ones (layer shell,
session lock, data control, input method, virtual keyboard, screencopy,
image copy capture and its sources,
foreign toplevel management, output management, foreign toplevel list,
gamma control) aren't offered
inside a security context.

Notes for client authors:

- Outputs use integer scales (2 for panels over ~180 dpi) unless the
  settings give a fractional one.
- Every surface hears its output's scale and transform, and hears them
  again when they change or the window moves to another output:
  wp_fractional_scale's preferred_scale, and wl_surface 6's
  preferred_buffer_scale and preferred_buffer_transform (the logic's
  `scales.rs`, for windows with their subsurfaces and popups, layer
  surfaces and lock surfaces). `tests/scales.rs` checks an open window
  follows its output from 1× to 2×, 1.5× and back.
- wl_shm takes every format the renderer reads (ABGR8888 and XBGR8888
  besides ARGB and XRGB). Under GLES a shm buffer is released as soon as
  its commit has been uploaded into a texture, not at the next commit, so
  a client can draw into one buffer again at once (smithay adds a second
  release when the next commit replaces it); under pixman, which draws
  from the client's memory, it is kept until then.
- Drag and drop: wl_data_device_manager 3 (source and offer actions),
  and the drag icon moves with its wl_surface.offset (or attach x, y)
  relative to the pointer; it shows above everything, in screenshots
  too.
- Frame callbacks fire after a frame that showed the surface, and roughly
  once a second for surfaces that don't show (other Desks, minimised).
  Nothing draws while nothing changes.
- wp_fifo_v1 barriers clear the same way: after a frame on the surface's
  output, so a Vulkan swapchain presenting with vsync (FIFO) gets one
  frame a refresh, and within about a second where no frame shows the
  surface (it doesn't show, or its display is off). A commit with a
  wp_commit_timing_v1 timestamp goes into the first frame that reaches
  the screen at or after it (counted from the display's last vblank), or
  applies at that time if the surface doesn't show (`pacing.rs`;
  `tests/pacing.rs`).
- The shell's notification cards (layer namespace `aizome-notifications`)
  slide in from the screen's right edge when they come up and back out
  when they go, over the motion time with its ease-out and no fade (120 ms
  Full, 80 ms Lean, none in Austere or with reduced motion). Only the
  drawing moves: where the cards take the pointer is their place
  throughout. Going out shows the surface's last frame, which the core
  draws into memory as the surface goes (the logic names such namespaces
  in its view, `keep_on_unmap`).
- In the Full effects tier, under GLES, what's behind the shell's frosted
  surfaces (the Deskbar, torn-off menus, the palette, the notification
  cards, and every popup of the shell's surfaces) is blurred where the
  surface's own pixels are at least 75% opaque, so a 90%-opaque face shows
  a blur through it and its hard shadow stays a plain shadow
  (`docs/rendering.md`). Other clients' translucency is blended as drawn.
- A toplevel's app_id should be set before its first commit; it picks the
  remembered size and place, the icon, and whether ❖ reaches it untouched
  (`org.aizome.…` apps) or as Ctrl.
- Layer surfaces: `aizome-deskbar` is placed around (new windows open clear
  of it). Exclusive keyboard interactivity on the top and overlay layers
  takes the keyboard while mapped; on-demand takes it on a click; a
  grabbing popup has it until it closes, then it goes back (the order is
  in `docs/ipc.md`).
- Popups (menus) of windows and layer surfaces show above every window
  and layer surface, newest on top, anywhere on their parent's output.
- xdg-activation: a token made from an input event of the client with the
  keyboard or pointer focus moves focus; any other request marks the
  window urgent (IPC `urgent`) instead.
- ext-session-lock: `locked` is sent once every output has drawn a frame
  without the desktop; lock surfaces get the output's size and all input.
  A locker that dies leaves the session locked. Locking closes the
  overview, the switcher, the marking menu and the area picker (one
  borrowed with `pick-area` reports nothing picked), so a menu left open
  can't act on the windows behind the prompt.
- wlr-screencopy (what wf-recorder and grim use): each frame offers
  wl_shm XRGB8888 and, under GLES on a render node (or a GPU on udev), a
  linux-dmabuf XRGB8888 buffer too (`linux_dmabuf`, then `buffer_done`).
  A dmabuf, in any modifier the GPU renders to, is bound as the render
  target, so a screencast stays on the GPU end to end (wf-recorder with a
  VA-API codec hands it straight to the encoder); `ready` is sent when the
  GPU's fence signals, and the compositor never waits for it. wl_shm goes
  through an offscreen buffer kept per stream, read back as BGRA (GLES
  swizzles as it reads; pixman copies from the image). Frames are
  upright (the output's transform undone, no `y_invert`). A stream (one
  client's captures of one output region, with or without the pointer)
  keeps its own damage tracker: `copy_with_damage` answers at once for its
  first frame, then after the first output frame that changed something
  in its region, with `damage` events for what did; an idle screen copies
  nothing. `aizomectl stats` reports each copy's CPU time (both capture
  protocols', as `screencopy`).
- ext-image-copy-capture (1) with ext-image-capture-source (1), for
  outputs and for windows (from ext-foreign-toplevel-list handles): what
  xdg-desktop-portal-wlr 0.8 uses, so browsers' screen and window
  sharing. Its wlr-screencopy path froze after the first frame with
  PipeWire 1.x: it drives its stream and, after a frame, only queues the
  buffer, which runs one cycle and never asks it for the next; its
  ext-image-copy-capture path triggers each cycle itself. Sessions take
  the same copies as screencopy: constraints offer wl_shm XRGB8888 and
  ARGB8888 and, under GLES, the render node (`dmabuf_device`) with
  XRGB8888 and ARGB8888 in every modifier the GPU renders to. A session's
  first frame comes at once with full damage, later ones once its source
  changes (an output drew something, a window's surfaces committed), with
  damage; frames are upright (`transform` normal). A window's frames hold
  its surfaces and its popups, without its frame or what covers it, at its
  output's scale, sized to its geometry; the pointer is painted onto
  outputs only (`paint_cursors`). A resize sends new constraints (a frame
  waiting with an old buffer fails with `buffer_constraints`); a source
  that goes stops the session. Cursor sessions are accepted but stop at
  once. The portal can make a frame before destroying its last when its
  stream pauses and resumes as a consumer links: the newer frame takes
  over (and the older is never answered) instead of a `duplicate_frame`
  error ending its connection. The NixOS module's portal is patched not
  to (`nix/xdg-desktop-portal-wlr-captures.patch`): it cancels a capture
  when its stream pauses and captures into the same buffer on resuming,
  where 0.8.4 queued that buffer unfilled with a timestamp of zero, after
  which a reader syncing to the timestamps (GStreamer's `pipewiresrc`)
  stalled after one frame. A session that has shown frames for a
  second is a share (reported then, from a timer: an IPC event at the
  first frame, while the portal and the app's PipeWire stream were still
  starting, left the stream stuck after one frame in the VM): IPC `shares`
  lists them, `shares-changed` tells the shell (its Deskbar indicator),
  and `stop-share` stops one as if its source had gone. In Aizome sessions
  the portal asks `aizome-share-picker` what to share (the NixOS module
  sets it as the portal's `chooser_cmd`, with `chooser_type=dmenu`): it
  reads the displays and windows the portal offers, opens the picker
  (`pick-area` with displays and windows only, and a banner saying what's
  being chosen), and prints the portal's line
  for the display clicked or the window clicked after Space (matched by
  its ext-foreign-toplevel identifier). Esc prints nothing, and the
  portal refuses the share. The portal reads its config only from
  `~/.config` or its own package, not `XDG_CONFIG_DIRS`, so the module's
  drop-in for its user service starts it with
  `/etc/xdg/aizome/xdg-desktop-portal-wlr/config` in Aizome sessions,
  unless the person has a config of their own.

## Displays

- **On and off**: `output-config` with `enabled` (or `enabled = false` in an
  `[[outputs]]` entry, or wlr-output-management) takes an output out of the
  layout and, on udev, disables its CRTC; it stays listed (`enabled:
  false`) to be turned on again. Its Desk goes as a display's does when
  unplugged (see Desks below), and comes back with it.
- **Mirroring**: `mirror = "eDP-1"` in an `[[outputs]]` entry (or
  `mirror-output`) shows another output's picture, rendered afresh and
  scaled to fit with black bars, pointer included; the mirror has no place
  in the layout and no wl_output. It costs a second render of the source
  and a read-back per frame, and redraws when the source does.
- **Main display**: `primary = true` (or `set-primary-output`); else the
  first output. The pointer starts in its middle, it shows the first Desk
  at login, and the current Desk comes there when its own display goes;
  the IPC reports it, and the shell puts the Deskbar and banners there
  (again, when it changes).
- **Input methods**: IBus's Wayland frontend (and any other on
  zwp_input_method_v2) types into text-input-v3 clients. Every commit an
  input method makes reaches the text input with the text input's own
  serial, whatever serial the input method gives, as under wlroots: IBus
  1.5 counts its serial from zero again on every global the registry
  announces (a display plugged in), and foot drops a `done` whose serial
  isn't its own, so once smithay discarded such commits (sending
  `done(0)`) nothing but Return reached foot.
- **The lid**: while a laptop's lid is closed (libinput's lid switch), its
  own panel (`eDP-*`, `LVDS-*`, `DSI-*`) is off, its CRTC disabled as
  idleness does, and other monitors stay on. Opening the lid turns it on
  again and counts as input. With an external display in the layout,
  the panel leaves the layout too: its focused Desk moves to the display
  left. Opening the lid restores the panel's live mode, scale and position,
  and its Desk returns. Undocking while closed restores its layout but
  keeps it dark until opened. An explicitly disabled panel stays disabled.
  That matters when logind doesn't suspend on the lid (Keep Awake,
  docked); a panel another output mirrors stays on, since its frames
  drive the mirror's. `powered` in the IPC's outputs says which are off.
- **Night Light**: `[night_light]` `enabled`, `temperature` (K, default
  4000), `schedule` (`sunset`: from sunset to sunrise where the computer
  is, which is `latitude` and `longitude` when set by hand, else
  `[location]` (what GeoClue told the shell, while the time zone is the
  one it was found in), else the time zone's city from tzdata, and
  19:00–07:00 without any; `manual` from `from` to `to`, `"HH:MM"`;
  `always`), checked every minute (the time zone too) and on changes;
  the IPC's state says today's `from` and `to` and the `place`; Auto's
  theme on the tabs turns in the same minute; `night-light` over IPC changes it
  for the session. It goes through the CRTC's gamma LUT (legacy gamma
  ioctl), the same under GLES and pixman, and re-applies after a VT
  switch; outputs without gamma (headless, winit, virtio-gpu) can't show
  it, and the IPC says which can. Screenshots are untinted. A
  wlr-gamma-control client (gammastep, wlsunset) holding an output's ramps
  takes it over from Night Light until it lets go; outputs without gamma
  answer it `failed`.
- **Renderer**: `[performance] renderer` (`auto`, `gpu`, `software`) picks
  GLES or pixman at start-up unless `--renderer` or `AIZOME_RENDERER` say
  otherwise; `stats` reports when it differs from the running one.

## Window management in brief

- **Placement**: centred on the usable area (inside exclusive zones),
  beside the Deskbar, cascading off windows already there; dialogs centre
  on their parent; apps reopen where they last were
  (`~/.local/state/aizome/windows.json`).
- **Tabs**: as wide as their titles (at most 360pt, and 32pt short of the
  frame); paths lose their middle, other titles their end. Click focuses
  and raises; drag moves; ⇧-drag slides the tab along the top; a double
  click or the zoom box fills; the close box closes; middle-click sends to
  the back; right-click opens the marking menu.
- **Frame**: edges and corners resize, 6pt beyond the edge; ⌃❖-drag moves
  and ⌃❖-right-drag resizes from anywhere in a window.
- **Stacks**: drag a tab over another window's tabs to stack: they part
  where it would go, before the first whose middle is past the pointer or
  after them all (a dashed slot and "Release to stack with ‹title›"
  show); click a background tab to show it, drag it away to tear it out
  (⌥-drag tears out the showing one), and drop it back on its group to
  move it along; ⌃❖S stacks with the window behind and ⌃⇧❖S takes the
  showing tab out; ❖⇧[ and ❖⇧] show the previous and next tab, and
  ⌃⇧❖[ and ⌃⇧❖] move the showing one along; IPC `stack`/`unstack`
  (`stack` adds at the end).
- **Desks**: four by default (settings `[[desks]]`). Each display shows a
  Desk of its own, as XMonad's screens show workspaces: the main display
  starts on the first, the others on the next ones. The current Desk is
  the one of the display with the focus, which moves with the focused
  window and with a click on a display's desktop. ⌃❖N, a Desk clicked in
  a bar's strip (on that display) or in the overview (on its display), and
  `set-desk` (with `output`, on that display) show a Desk there; a Desk
  another display shows swaps places with it (greedyView), and only those
  two displays change. ⌃❖←/→ step past Desks other displays show. A new
  window opens on the display with the focus (a dialog on its parent's),
  and focus never leaves a display by itself: closing its last window
  leaves it with none. A window dragged (or `move`d) mostly onto another
  display joins the Desk there. Sticky windows show on every Desk, on
  their display. Switching slides that display's Desk over the motion
  time (none in Austere or with reduced motion).
- **Desks between displays**: a window remembers the usable area it was
  laid out in. Shown on a display of another size or place, it scales to
  it, its frame's edges keeping their places across the area (a window
  flush with an edge stays flush; two side by side stay so), and fill,
  split and full screen take that display's area; it keeps where it was
  before (its home), so a display like the first puts it back exactly, to
  the point, and others scale from there, without rounding piling up.
  Moving or resizing it forgets its home. A display that goes takes its
  Desk out of sight, unless it had the focus: then the Desk comes to the
  main display, whose own hides. A display that comes back shows the
  Desk it showed (taking it back from where it went, and giving that
  display its own back); a new one shows the first Desk no display shows,
  a Desk being added when there's none. Windows of Desks no display shows
  keep their places for when one does. Every display keeps a Desk, so
  removing Desks stops at one each.
- **Overview** (⌃❖↑), **switcher** (❖Tab), **marking menu** (right press
  on a tab, or ⌃❖Space for the focused window: arrows, letters and digits
  pick, and the pointer only what it's over), **area screenshots** (❖⇧4; the shell borrows the picker with
  IPC `pick-area` to choose what to record, and `aizome-share-picker` to
  choose what to share, a display or a window, under a banner saying so):
  see `docs/ipc.md` for their
  keys.

## Inside

The compositor is a stable core and a logic library the core loads at
run time and swaps for a new build without disconnecting anyone
(`aizomectl reload`, ⌃❖R, `aizome-dev watch`: `docs/reload.md`). The
binary is `fn main() { aizome_comp_core::run() }`.

**`crates/aizome-comp-core`** (`libaizome_comp_core.so`): what clients'
connections and the way out depend on.

| Module | What |
|---|---|
| `logic.rs`, `reload.rs` | Loading, checking (the ABI fingerprint from `build.rs`), swapping and rolling back the logic; the hook table (`LogicVTable`); `reload` |
| `view.rs`, `scene.rs` | What the logic publishes (windows' places, focus, stacking) and the renderer-free scene it returns each frame |
| `render.rs` | Turning a scene into render elements: windows, layer surfaces, popups, thumbnails, the cursor and drag icon, the lock screen |
| `thumbs.rs` | Under pixman, thumbnails (the overview's, the switcher's) kept as pictures at their size, scaled again only where their window drew |
| `blur.rs` | Under GLES, the blur behind a scene's `Blur` items (dual Kawase, masked by the surface's own alpha), taken again only when what's under it changed |
| `ghosts.rs` | Layer surfaces' last frames, kept for the logic to move away as they go |
| `input.rs`, `focus.rs` | Input from libinput, winit or IPC; the core's keys (VT switching, ⌃❖R, Ctrl+Alt+Del) before the logic's, and a key `catch-key` waits for; keyboard focus, popup grabs, grab wrappers |
| `keyboards.rs` | The keyboards that type, who each is, and the keymap of the one typing (its own keys from `[[input.keyboards]]`) |
| `shell.rs`, `handlers.rs`, `popups.rs` | Surfaces, xdg-shell, layer shell, session lock and the smaller protocols; the window registry |
| `protocols/` | wlr-screencopy and ext-image-copy-capture with its output and window sources (`capture`: the streams and copies they share, wl_shm and dmabuf, per-stream damage; an output's copy draws the logic's scene), wlr foreign toplevel management, wlr output management, wlr gamma control, xdg-toplevel-icon |
| `backend/` | headless (GLES on a render node or pixman, 60 Hz pacing only while damaged), winit, udev (DRM/KMS, multi-GPU GLES or pixman dumb buffers) |
| `recovery/` | Ctrl+Alt+Del's overlay: state, keys and actions (`mod.rs`), drawing (`view.rs`, with the core's painter), apps' processes on a thread (`procinfo.rs`, `x11.rs`), pings (`ping.rs`), Reset Input (`reset.rs`). The core calls it before the logic: the first check in `decide_key` and `pointer_button`, `pointer_moved` and `scroll` in input, `may_constrain` (pointer constraints), the first items of `State::scene`, `frame_flags` (udev), `exclusive_keyboard_target`, `handle_request`, `client_pong` and `constraint_requested`, and `start` in `run`. |
| `watchdog.rs` | systemd's watchdog keepalive from the main loop and the stall watcher (backtrace, `stall.log`, the notice after a restart). |
| `fallback.rs` | The window management that runs while no logic does (none loaded, or set aside after repeated panics) |
| `text.rs`, `icons.rs`, `svg.rs`, `painter.rs`, `pieces.rs`, `cursor.rs` | Labels (Sidestep's offscreen canvas), app icons (desktop files, icon themes, resvg), the overlays' painter, decoration pieces, xcursor |
| `ipc.rs`, `requests.rs` | The IPC socket and the requests the core answers (the rest go to the logic) |
| `session.rs`, `idle.rs`, `power.rs`, `config.rs`, `outputs.rs`, `night.rs`, `timers.rs` | Start-up, supervising the shell (and restarting it beside the old one), idle events, display power (idleness, IPC, the panel behind a closed lid), live settings, outputs, Night Light, timers for the logic |

**`crates/aizome-comp-logic`** (`libaizome_comp_logic.so`): what happens on
the desktop.

| Module | What |
|---|---|
| `wm/` | The model: windows, stacking, focus history, tab groups, Desks, placement, fill/split/full screen. Plain Rust, unit-tested; saved and restored across reloads. |
| `actions.rs`, `hooks.rs` | Window lifecycle, placement and the hook table the core calls |
| `decor.rs`, `render.rs` | Tab and frame layout and hit testing; the scene per output, front to back |
| `overlays/` | Stack-drop feedback, marking menu, switcher, overview, area picker (screenshots, and the picks the shell and the share picker ask for, with the sharing banner) |
| `input.rs`, `grabs.rs`, `bindings.rs`, `cmdkey.rs` | Key and pointer handling after the core's; move, resize, tab slide and box grabs; the binding table and ❖ translation |
| `anim.rs`, `memory.rs`, `requests.rs`, `config.rs` | Motion; remembered places; the IPC requests about windows and Desks; settings |
| `slides.rs` | The shell's notification cards sliding in and out (carried across reloads) |
| `frost.rs` | Which surfaces get a blur behind them in Full (the shell's frosted ones and its menus) |

## Tests

```sh
nix develop .#aizome --command cargo test -p aizome-comp
```

- Unit tests: the model (placement, focus order, stacks, Desks), tab
  layout and hit testing, bindings, EDID parsing, statistics.
- `tests/headless.rs`: a headless compositor in a child process with
  solid-colour Wayland test clients, checking IPC state and screenshot
  pixels (the focused tab, the rail, the frame, the 3pt hard shadow,
  idle tabs, stacks, Desks, clicks, drags, resizing, the close box,
  fill, client-side decorations, idle frames, live settings, remembered
  places, ❖⇧3), under GLES and pixman; and grim, wlr-randr and lswt when
  installed.
- `tests/overlays.rs`: the switcher, the overview, the marking menu, and
  the area picker reporting its pick to the shell (`pick-area`): a drag,
  a click for the display, Space for a window, Esc; and for sharing,
  displays and windows only (a drag is the display, a window comes with
  its ext-foreign-toplevel identifier, a click on no window keeps
  picking, windows alone start at windows); and a window pick, for a
  share or ❖⇧4, keeps its release from the window under it.
- `tests/keys.rs`: ❖ as Ctrl, Ctrl+Shift in terminals, ❖ for kit apps,
  system and shell chords.
- `tests/settings.rs`: tab double-click, scroll factor, sticky and slow
  keys.
- `tests/displays.rs`: outputs off and on (from IPC and the settings),
  mirroring scaled and letterboxed, the main display, Night Light's IPC
  and schedule, the renderer setting, and a laptop's panel (a headless
  output named `eDP-1`) dark while its lid is closed.
- `tests/screens.rs`: displays with Desks of their own: switching on one
  leaves the other alone, a Desk another display shows swaps with it and
  its window scales to the larger display and back exactly, a window
  dragged by its tab to another display joins its Desk, and an unplugged
  display's Desk comes along and goes back when it's plugged in again.
  The model's rules (swaps, homes, hotplug, stepping, removing Desks) are
  unit tests in `wm/tests.rs`.
- `tests/input_method.rs`: an input method counting its serial as IBus
  does types into a text input that checks serials as foot does, across a
  display turned off and on.
- `tests/protocols.rs`: wl_shm formats, shm buffers released after upload
  (GLES) or kept (pixman), drag and drop with an icon and actions.
- `tests/chromium.rs`: a window opened the way Chromium and Electron
  open theirs (an xdg-toplevel-icon set and destroyed before its buffer,
  either decoration mode, activation with their own token), and the icon
  protocol's errors.
- `tests/screencopy.rs`: wlr-screencopy into wl_shm, a whole output and a
  region, under pixman and GLES; `copy_with_damage` answering at once
  for a stream's first frame and then only after a change (not for the
  pointer when it isn't captured); and, with a render node, a GBM dmabuf
  rendered into on the GPU and read back upright.
- `tests/image_copy_capture.rs`: ext-image-copy-capture as
  xdg-desktop-portal-wlr uses it (the regression test for the frozen
  screen sharing): an output's and a window's sessions, constraints, a
  first frame at once with full damage, then a frame per change for
  several changes and none without one, into wl_shm under pixman and GLES
  and into a dmabuf on the render node; a window's content alone, new
  constraints on a resize and a stop when it closes; a second frame
  taking over from the first; shares reported (`shares`,
  `shares-changed`) and stopped (`stop-share`).
- `tests/focus.rs`: a shell-like wayland-client (layer surfaces and their
  popups): exclusive and on-demand keyboard interactivity, grabbing menus
  getting the keys and giving them back, menus closing for an exclusive
  surface or a click elsewhere, and the popup layer's stacking.
- `tests/context_menus.rs`: an app with a text field and an input method
  that grabs the keyboard while a field has it (as Telegram and ibus):
  the field's context menu opening on a right-press, keeping the keys
  once the input method lets go, and a field in the menu typing through
  it; an input method that keeps its grab having the keys again after
  the menu; another client's menu refused over the grab and the shell's
  (serial 0, as for F10) opening; and a menu refused for a pointer grab
  leaving the field and the input method be.
- `tests/sidestep.rs`: a sidestep-ui client (`examples/sidestep_client`):
  server-side decorations, frame callbacks on the content subsurface, a
  layer-shell panel's exclusive zone, the session lock, a see-through
  surface showing what's under it, and (with a render node) a frosted
  panel in Full blurring a window's edge under its face while its shadow
  and everything off it stay as in Lean.
- `tests/recovery.rs`: Ctrl+Alt+Del over a full-screen client holding a
  keyboard-shortcuts inhibitor, Force Quit killing a client in a process
  of its own (and never the compositor's parent), Not responding, the
  lock screen's safe actions, Reset Input ending a stuck grab and freeing
  a locked pointer, and drags that end on release (after an overlay took
  a release, with a frozen client, and a client's late move request),
  and the overlay's Reload Aizome reloading the logic.
- `tests/reload.rs`: reloading the logic under a test client, foot and
  Electron (windows, groups, Desks and focus kept, the screen changed), a
  panic rolled back, a session without a logic, a library built against
  another core refused, and restarting the shell with apps and
  notifications kept (`docs/reload.md`).

Screenshots the tests keep are in `target/tmp/aizome-shots/`; examples
are in `docs/screenshots/`. Frame times: `docs/performance.md`.

The udev backend is tested in a VM: `crates/aizome-comp/vm/udev.nix`
boots NixOS with a virtio GPU without 3D, logs a user in on tty1, starts a
development build there (libseat through logind, libinput, KMS), and
checks the output, ❖Return opening foot (the default terminal there, the
only one installed), typing into it and the frame statistics;
`docs/screenshots/compositor-udev-vm.png` is its scan-out.
Without a hardware GPU the backend renders with pixman into dumb buffers.
