# The shell

`aizome-shell` (crate `crates/aizome-shell`) is one sidestep-ui process that
draws everything of Aizome that isn't a window's frame or an app: the menu
bar, the Toolchest, the Desks strip, the systray and its panels, the
Deskbar and the Shelf, the command palette, menus at the pointer and
torn-off palettes, notifications, the OSD, the desktop with its icons and
wallpaper, the lock screen and the authentication dialog. One process keeps
one font cache and one render thread for all of it. The compositor starts it
and restarts it if it dies.

It is built with `aizome-kit`, talks to the compositor over `aizome-ipc`
(`$AIZOME_SOCKET`), serves apps over `aizome-shell-proto`
(`$AIZOME_SHELL_SOCKET`), reads the system through `aizome-sys`, and searches
with `aizome-search`.

The look is the boards': `aizome-design/boards/aizome-desktop-{night,day}@2x.png`
and `aizome-menus-{night,day}@2x.png` (2×, so halve pixels for points), with
the exact CSS in `aizome-design/reference/html/C2-Desktop.html` and
`C2-Menus.html`. The palette's structure is Strip's
(`reference/png/C3-Palette.png`, `reference/html/C3-Palette.html`) redrawn in
Aizome's look.

## Surfaces

| Surface | Layer | Namespace | Per | Anchors / size | Keyboard |
|---|---|---|---|---|---|
| Menu bar | top | `aizome-menubar` | output | top, left, right; 24pt; exclusive 24 | on demand |
| Deskbar | top | `aizome-deskbar` | the primary output | top, right; 232pt wide, as tall as its content; margin-top 24 | on demand |
| Menus, systray panels | popups of the bar or Deskbar | — | — | — | grab |
| Palette, Toolchest typed into | overlay | `aizome-palette` | the focused output | centred, 720 × up to 480pt | exclusive |
| Menu at the pointer, torn-off palettes | overlay / top | `aizome-menu` | — | positioned with margins | on demand |
| Notifications banner | top | `aizome-notifications` | the primary output | in the bar's line | none |
| OSD | overlay | `aizome-osd` | the focused output | bottom centre | none |
| Desktop | background | `aizome-desktop` | output | all four edges; exclusive -1 | on demand |
| Lock | ext-session-lock surface | — | output | full | yes |
| Authentication | overlay | `aizome-auth` | the focused output | centred | exclusive |

## Menu bar

24pt, `bar`, 1pt `bevel_lo` bottom edge, items 13pt Archivo. Left to right:

- **Toolchest button**: the mark at 16pt (module 4px at 2×, i.e. 2pt;
  draw it from `aizome_theme::Brand::mark_rects`), then "Toolchest" in
  Archivo 650. Opens the Toolchest.
- **The focused app's name** in bold, which opens its app menu, then its
  menus (from `aizome-shell-proto`, keyed by the focused window's pid) or the
  fallback menus for apps that publish none: an app menu (About, Hide ❖H,
  Hide Others ⌥❖H, Quit ❖Q — Quit closes all its windows through the
  compositor) and a Window menu (Minimise ❖M, Fill, Split Left, Split Right,
  Stack with…, Move to Desk ▸, Full Screen ⌃❖F, Close). With no window
  focused, the "app" is the desktop: Aizome's own menus (Aizome ▸ About
  Aizome, Settings…, Lock, Log Out; File ▸ New Folder on Desktop; Desks ▸;
  Help ▸).
- **The Desks strip**, right of centre: `1 Mail  2 Build  3 Write  +`, the
  current desk on `selection`, its number and name in Archivo 650; click to
  switch, `+` adds a desk, double-click renames in place, right-click for
  Rename, Set Folder…, Remove.
- **The recording indicator**, first in the systray while a screen
  recording goes: a `red` dot (8pt) and the time recorded in IBM Plex Mono
  600 tabular digits (`0:42`, `1:02:09`), ticking once a second and
  repainting only itself; a click stops the recording. While the recorder
  finishes the file the dot is a ring and the time reads "Saving".
- **The systray**: CPU meter (one 3pt bar per core, height = load, `blue`
  at rest, `amber` when hot), memory meter (one bar), then the network,
  sound, Bluetooth, display and battery widgets, each an icon (Lucide, 16pt,
  `text2`) and, for battery, the percentage in IBM Plex Mono. Hidden when
  their hardware is absent (no battery on a desktop, no Bluetooth adapter).
- **Apps' status items**, between the meters and the network widget: the
  icons apps put in a tray (Steam, Discord, Dropbox, Telegram), in the order
  they came. The shell serves `org.kde.StatusNotifierWatcher`, the
  StatusNotifierItem protocol libappindicator, libayatana-appindicator, Qt
  and Electron speak; XEmbed tray icons (old GTK 2 and Wine apps) aren't
  shown. Each is its icon at 16pt: by name from the app's own folder
  (`IconThemePath`), then the icon theme, else the pixels it sent (PNGs;
  SVGs don't read yet), a generic window glyph until one reads. Passive
  items (the app says there's nothing to show) are hidden; while one asks
  for attention it shows its attention icon. A click activates the item,
  or opens its menu when it is only a menu (Steam's); a right click opens
  its menu (`com.canonical.dbusmenu`, read whole as it opens, checks and
  radio marks and disabled rows as the app says), or asks the app to show
  its own when it has none; a middle click is its secondary action.
- **The clock**, IBM Plex Mono 600, tabular digits, 24h by default (config).
- When the Deskbar is collapsed: `N apps ⌄` before the clock, which expands
  it, in place of the widgets and status items.

Each systray widget opens a panel (a popup under it, `raised`, hard 3pt
shadow, 320pt wide) of quick controls, and its last row opens the matching
Settei pane (`aizome-settei --pane <name>`):

- **Network**: Wi-Fi switch; networks by strength with lock and signal
  glyphs, the connected one checked; picking a secured one asks for the
  password inline; VPN switches; airplane mode; "Network Settings…".
  Later password requests and VPN authentication use the session's
  [credential agent](network-authentication.md); `nm-applet` does not
  autostart in Aizome.
- **Sound**: output volume slider and mute, output device list, input
  volume and mute, the playing media (MPRIS: art, title, artist, ⏮ ⏯ ⏭);
  "Sound Settings…".
- **Bluetooth**: switch; paired devices with connect/disconnect and battery;
  "Bluetooth Settings…".
- **Display**: brightness slider (each built-in panel), Night / Day / Auto
  segmented control, Size (small, medium, large), Night Light switch if
  available; "Displays Settings…".
- **Battery & power**: charge, state and time left, a 24h charge sparkline
  if cheap, power mode (Power Saver, Balanced, Performance); "Battery
  Settings…".
- **Clock**: the month (today in `focus`), the next events from Koyomi's
  store if present, world clocks from Tokei's; "Date & Time Settings…".

## Menus

Menus are popups of the surface that opened them: `raised` rows 24pt, 13pt
labels, shortcuts right-aligned in IBM Plex Mono `text3` with ❖ ⇧ ⌥ ⌃
(Noto Sans Symbols 2 first in the fallback), submenu chevrons, separators as
1pt `line`, the highlighted row on `selection` with `on_selection` text,
disabled rows in `faint`, check marks. Hard 3pt `slab` shadow, 1pt
`bevel_lo` edge.

- **Aim prediction**: moving towards an open submenu doesn't close it.
- **Type-ahead**: typing in an open menu filters its rows (and its
  submenus', flattened with their paths) with `aizome-search`; Esc clears,
  then closes.
- **Keyboard**: ↑↓ move, → opens a submenu, ← closes, Return activates, Esc
  closes; F10 or ❖⌥M focuses the menu bar.
- **Tear-off**: every menu at the pointer and every submenu has a
  `menu-title` strip with its name; dragging the strip tears the menu off
  into a palette that stays (an overlay surface placed with margins, moved by
  dragging its strip), with a close box and a pin. Palettes are per app and
  show only while that app is focused, unless pinned. They are remembered
  across sessions.
- **The menu at the pointer** (NeXT): ❖-right-click anywhere (a `Binding`
  event with the pointer position) or right-click on the empty desktop opens
  the focused app's whole menu at the pointer: a vertical menu with the app's
  name in its title strip and every menu as a submenu, Hide and Quit at the
  bottom (C2-Menus).
- **Services**: the app menu's Services ▸ lists verbs for the selected text
  (the primary selection): New Note with Selection (Fusen), Open in Sumi,
  Search the Web, Open URL (when it is one), Look Up in Files, Calculate
  (when it is arithmetic), Copy as Plain Text.

## Toolchest

A menu from the Toolchest button (or ❖⌥Space), styled like the menus at the
pointer: title strip "Toolchest" with the mark, then:

- **Applications ▸** by category (Accessories, Development, Graphics,
  Internet, Office, Media, System, Utilities, Other), each row the app's
  icon, name and generic name in `text3`; Aizome's apps first in their
  categories. From `aizome-sys`'s desktop-entry index.
- **Places ▸**: Home, Desktop, Documents, Downloads, Pictures, Music,
  Videos, the Desk's folder, mounted disks, Trash, Recent ▸.
- **Desks ▸**: the desks (go to), New Desk, Desks Overview ⌃❖↑.
- **Find ▸**: Commands, Windows, Files, Apps, Settings — each opens the
  palette in that scope.
- **System ▸**: Settei (Settings), Yukimi (Software), Myaku (Activity),
  Tansu (Disks), Kura (Disk Usage), Terminal.
- **Session ▸**: Lock ⌃❖Q, Sleep, Restart…, Shut Down…, Log Out… (the last
  three confirm in a small dialog that counts down 60 s).

Typing while the Toolchest is open turns it into the palette with what was
typed.

## Command palette

❖Space (a `Binding` event) opens it on the focused output; Esc or ❖Space
closes it. Layout as Strip's: a query row (prompt glyph, the query in 20pt,
"in ‹App› · ‹window title›" right-aligned in mono `text3`), scope chips (All,
Commands, Windows, Files, Apps, Settings; Tab and ⇧Tab cycle, the current one
on `selection`), results on the left grouped under mono uppercase section
heads, a detail pane on the right, and a footer of key hints ("↑↓ move  ↩ run
⇥ scope  ❖K actions  esc close", and "Searches every menu of the focused app"
on the right). Aizome's look: square, `raised`, 1pt `bevel_lo` edge, hard
shadow, matched characters in `blue` and underlined, the selected row on
`selection` with a 2pt `focus` rail at its left.

Between openings its window is kept, hidden, and drawn ahead at idle as it
would open (the empty query, All): three seconds after the shell starts, a
second after the palette closes, and a second after what it opens with
changes (the focused window, the windows, an app's menus, apps, settings,
picks, the theme, the focused output's scale). Opening it then copies that
drawing in and draws only what differs, rather than rasterising the whole
surface (`Cx::prepare_window`; `docs/performance.md`, exception 4).

Results:

- **Commands**: every item of the focused app's menus (flattened, with its
  path "Edit › Find" in `text3` and its shortcut in keycaps), and the
  fallback Window menu verbs. Running one sends `activate` to the app.
- **Windows**: every window (title, app, "Desk N", "hidden"); running
  focuses it.
- **Files**: `aizome-search::files` over the home folder (index kept in
  `$XDG_CACHE_HOME/aizome/files.idx`, refreshed in the background when older
  than 10 minutes and at idle), recent files first when the query is empty,
  frecency boosts, folders open in Hako and files with their default app.
- **Apps**: desktop entries by name, generic name and keywords.
- **Settings**: every Settei pane and its main options ("Keyboard ›
  Remap Caps Lock"), opening Settei at that place. The list comes from
  `aizome-settei --list-settings --json` (cached), so it stays in step.
- **Actions**: Lock, Sleep, Restart, Shut Down, Log Out, Night/Day,
  Do Not Disturb, Screenshot, New Desk, Empty Trash, Show Desktop; Record
  Screen and Record Area, each also with Desktop Audio, with Microphone,
  and with both (plain, they follow `[recording]`'s audio); Stop Recording
  in their place while one goes.
- **Answer**: when the query is arithmetic (`calc::looks_like_math`), the
  first row is "= result"; running it copies the result.

The detail pane for a command shows its name, path, shortcut as keycaps and
the **shortcut coach**: when a command with a shortcut has been run from
menus or the palette three or more times this week (count them in the
frecency store with a `menu:` prefix), "You've run this from menus N times
this week. The keys above do it without leaving the text." Its actions: Run
(↩), Run and keep the palette open (❖↩), Copy Shortcut, Show in Menu. ❖K
opens the actions of any row (for a file: Open, Open With ▸, Show in Hako,
Copy Path, Move to Trash; for a window: Focus, Close, Move to Desk ▸; for an
app: Open, New Window, Quit if running).

Opening an app from the palette, Toolchest or Deskbar raises its most
recently used window on the current Desk (sticky windows count as local).
If it has no window here, it switches to its most recently used window's
Desk and raises it. Activation restores a minimised window, unhides its app,
selects its stack tab and gives it keyboard focus. Apps without windows are
launched; **New Window** explicitly launches another instance.

Selections are recorded in the frecency store (`app:`, `file:`, `cmd:`
keys), so the palette learns.

## Deskbar

At the top right under the bar (C2-Desktop): `chrome`, 232pt, hard shadow.
The menu bar's Hide Deskbar control collapses it; the `N apps ⌄` control
brings it back.

Its height is limited to the space below the menu bar on the owning
output. Scroll to reach overflowing app groups, notifications and Shelf
items.

Settings → Desktop & Deskbar can enable automatic hiding
(`[deskbar] auto_hide = true`). Hover the menu bar's app control to reveal
it; leaving the control and Deskbar hides it after 350 ms. Open menus and
active drags keep it visible. A temporary reveal overlays applications
without changing their layout. Manual collapse remains a separate saved
preference; click the app control to restore it.

- Each running app (by app ID, in launch order, pinned apps first): its
  icon (Lucide glyph for Aizome's apps by kind, the app's own icon at 16pt
  for others), name in Archivo 650 and window count right-aligned. Click
  activates its last window on this Desk, or another Desk if none are local;
  right-click: New Window,
  Hide, Quit, Keep in Deskbar, Move Up and Move Down. Drag an app header to
  rearrange groups. The `[deskbar] order` preference persists separately
  from pins, including an absent app's position for its next launch.
- Its windows indented beneath: the focused one with a 3pt `focus` bar at
  the left edge, hidden or minimised ones in italic `text3` with "hidden",
  stacked ones with "stack", ones on another desk with "desk N", urgent ones
  with a `red` dot. Click focuses, middle-click closes, drag onto a desk in
  the bar's strip moves it.
- Unread notifications filed under their app: the count in `red` ("3 new"),
  the newest previewed in a `sunk` card (summary in 600, body two lines);
  click opens it (default action), × dismisses, the count opens the list.
- **SHELF** (mono uppercase head): parked drags — files, folders, text,
  images, links — each a row with its glyph and name; drag one out to drop
  it anywhere; right-click: Open, Copy, Remove. A dashed "Drop to park" row
  at the end takes drops. Kept in `$XDG_STATE_HOME/aizome/shelf/` across
  sessions (files by path, text and images by content).

## Notifications

The shell is the `org.freedesktop.Notifications` server (`aizome-sys`). A new
notification appears as a line in the menu bar — the app's glyph, summary
and the first line of the body, in place of the app's menus for 5 s (Lean;
slides in over the motion time; Austere just appears) — then is filed under
its app in the Deskbar (or the `N apps ⌄` control shows a count). Critical
ones stay as a card under the bar until acted on. Actions appear as buttons
on the card and in the Deskbar preview. Do Not Disturb (from the palette,
the clock panel, or a schedule in settings) files them without the line.
Screenshots post a notification with the image's thumbnail and "Show in
Hako".

Opening a notification raises its application's most recent window on the
current Desk, falling back to another Desk if necessary. It reveals minimised
windows and still invokes
the notification's default action when it has one. With no default action
and no running window, the shell can launch a known desktop entry. App
identity comes from desktop IDs, declared window classes, the sender's
process, or an unambiguous declared name; similar-looking names alone do
not select another application.

## Screen recording

As Tatami does it (`desktop/tatami` in Sekka): a display or an area, with
what the computer plays and the microphone if asked, one recording at a
time.

- **Starting**: the palette's Record Screen (the focused display) and
  Record Area; ⌥Print (`record`), which picks; the Capture menu (❖⇧5,
  `screenshot-tool`, as ⌘⇧5 on a Mac: under the bar's middle, with
  screenshots of the screen, an area and the window, Record the Screen ▸
  and Record an Area ▸ by audio, Open Recordings, and Stop Recording
  while one goes); `aizome-shell msg '{"request":"record","target":"screen"}'`
  (`target` `screen`, `area` or a display's name; `desktop_audio`,
  `microphone`). Picking uses the compositor's screenshot picker (IPC
  `pick-area`): drag over an area, click for the whole display under the
  pointer, Space and a click for a window (its frame and tab, where it is
  now), Esc to give up.
- **Stopping**: the indicator, ⌥Print again, Stop Recording in the
  palette or the Capture menu, `aizome-shell msg stop-recording`. The
  recorder gets SIGINT and finishes the file; one that hasn't after 10 s
  gets SIGTERM, then SIGKILL, and the notification says the video may not
  play.
- **The recorder**: gpu-screen-recorder, capturing the display's planes
  through KMS (its `gsr-kms-server`, which the NixOS module gives
  CAP_SYS_ADMIN) and encoding H.264 on the GPU (VA-API, NVENC; x264 only
  where the GPU has no encoder), the pointer included: nothing per frame
  for the compositor or the CPU. Where it can't record (no hardware
  OpenGL, no capture rights) wf-recorder does, through the compositor's
  wlr-screencopy: with VA-API (dmabuf frames, still on the GPU), else with
  x264 (wl_shm, read back; the `superfast` preset). wf-recorder records
  one audio source (what the computer plays before the microphone) and
  always the pointer.
- **Which recorder**: found once a session, in the background as the
  shell starts, so a recording starts at once with one that works rather
  than seconds into ones that can't (gpu-screen-recorder takes seconds to
  give up on software OpenGL) while the indicator counts.
  `gpu-screen-recorder --info` says whether it has hardware OpenGL, an
  H.264 encoder and which displays it can capture (it's used for a
  display it lists); `vainfo` on `/dev/dri/renderD128` whether
  wf-recorder can encode there (without `vainfo` VA-API is tried). A
  recording asked for before the probe is done waits for it, uncounted.
  If the one chosen still fails (it exits within 1.5 s, or later without
  having written a frame) the next takes over, and it isn't tried again
  that session.
- **Audio**: through PipeWire's PulseAudio server: the default output's
  monitor for what the computer plays, the default input for the
  microphone; gpu-screen-recorder mixes both into one track (AAC in MP4,
  Opus in Matroska).
- **Files**: `~/Videos/Recordings` (`Recordings` in `XDG_VIDEOS_DIR`, or
  `user-dirs.dirs`' Videos), named as Tatami names them:
  `screenrecording-2026-10-18_14-05-09.mp4`, local time.
- **When it ends** a notification says so with the file's name, and its
  actions are Open (the default video player; also a click) and Show in
  Files (Hako, the file selected). A recorder that dies clears the
  indicator and says why (the last lines it wrote, kept in
  `/aizome/recorder.log`).
- **Robustness**: the recording isn't tied to a Desk or a window; the
  recorder runs in its own process group and is watched through a pidfd,
  and `/aizome/recording.json` notes it, so a shell that
  crashed and was restarted takes it back (indicator and Stop) instead of
  starting another.
- **Settings**: `[recording]` in `settings.toml` (Settei has no
  capture pane yet): `format` (`mp4`, `mkv`), `framerate` (60), `quality`
  (`medium`, `high`, `very-high`, `ultra`), `cursor` (true),
  `desktop_audio` and `microphone` (the defaults for Record Screen and
  Record Area, off), `folder`, and `recorder` (`auto`,
  `gpu-screen-recorder`, `wf-recorder`).

## Screen sharing

What apps share (browsers' screen and window sharing, through
xdg-desktop-portal-wlr):

- **Choosing**: the portal asks `aizome-share-picker`, which opens the
  compositor's picker with a banner across the top of each display
  ("Choose a screen or window to share — Esc to cancel", or just a screen
  or just a window, as the app asks) and the keys beneath it: click a
  screen; Space, then click a window; Esc to cancel, and the app is told
  the share was refused. A drag picks the screen it ends on, never an
  area.
- **The indicator**: while something is shared, the Deskbar's header
  shows a chip in the accent blue with the screen-share glyph (a count
  when several things are shared), left of the recording dot, as macOS
  shows its purple one. A share appears once it has lasted a second, so a
  screenshot tool's single frame never flashes it.
- **Stopping**: pressing the indicator opens a menu saying what's shared
  (the display, or the window's title) with Stop Sharing (one item per
  share, and Stop All Sharing, when several are). The compositor stops
  the share, the portal ends the stream, and the app hears that the
  screen or window is no longer available.
- `aizome-shell msg state` lists the shares under `sharing`; the
  compositor's `aizomectl shares` and `aizomectl stop-share [ID]` do the
  same from a terminal.

## OSD

Volume, mute, brightness and keyboard-backlight keys (`Binding` events from
the compositor) change the value through `aizome-sys` at once and show a
`raised` square at the bottom centre: a 32pt Lucide glyph and a 16-segment
level bar, for 1.2 s. Sound plays a click at each step unless muted
(optional).

## Desktop

A background surface per output:

- The wallpaper for the theme (config `appearance.wallpaper.{night,day}`,
  else `AIZOME_WALLPAPER_NIGHT`/`DAY`, else `desktop` colour), scaled to cover
  and cropped, decoded once per output size and cached.
- Icons down the left from the top (C2-Desktop): Home (the user's name),
  each mounted or mountable disk and image (UDisks2, from `aizome-sys`:
  label or model; click mounts; right-click Eject / Unmount / Open in Tansu),
  Trash (full or empty glyph; drop to trash; right-click Empty Trash), then
  the items in `~/Desktop`. Lucide glyphs at 48pt in `blue`, labels on
  `desktop-label` chips. Click selects (rubber band too), double-click opens
  (Hako for folders, default app for files), drag to move or drop on Trash
  or a disk; positions remembered.
- Right-click on empty desktop: the menu at the pointer.

## Lock screen

The compositor's recovery overlay (Ctrl+Alt+Del) can restart the shell
(Restart Shell; Restart Lock Screen while locked): it SIGKILLs and starts
it again without the usual backoff, and while the session is locked it
sends the new shell a `lock` binding once a second (for up to 20 s) until
a lock surface shows: the session stays locked, black, without one.

Locks on ⌃❖Q, on the compositor's `idle` event (config
`idle.lock_after_minutes`), before sleep (logind PrepareForSleep, holding a
delay inhibitor until locked), and on logind's Lock signal (`loginctl
lock-session`). Each output gets a lock surface: the wallpaper under a
`kachi` scrim at 70%, the time in IBM Plex Mono 64pt and the date, and on
the focused output the user's picture or initial, their name, a password
field (a 2pt `focus` rail when focused) and the mark. Caps Lock shows a
warning. Authentication runs PAM (service `aizome`, through the `pam`
crate or a small FFI) on a thread; a failure shakes the field (motion moves
the layer; none in Austere/reduced motion) and says so. Notifications and
media controls are not shown while locked. Unlocking restores everything.

## Authentication dialog

The polkit agent (`aizome-sys`) asks the shell: an overlay dialog with the
mark, the action's message, the identity (a dropdown if several admins), a
password field, Cancel and Authenticate, PAM's prompts and errors inline.

The shell owns a `PolkitAgent` handle for its lifetime. The handle watches
polkit's D-Bus owner before registering and registers again when the daemon
restarts, including during a system update. Registration failures retry
every two seconds; a healthy registration waits for owner changes without
polling. This also lets a replacement shell wait for the previous shell to
release its registration. Losing the authority cancels its pending dialogs
and helper conversations; requests from the old authority are rejected.
Initial setup failures retry, and losing the bus recreates the agent on
a fresh private connection. A lost registration reply is cleaned up before
retrying so the agent cannot collide with its own previous registration.
Dropping the handle stops the worker and unregisters from that specific
authority. The private-bus tests in `aizome-sys/tests/polkit.rs` exercise
restarts, interrupted authentication, deferred registration and handover.

## Real and headless sessions

Only the machine's Aizome session registers as polkit's agent, takes
logind's sleep delay and follows logind's Lock, Unlock and PrepareForSleep
(`real_session()` in `shell.rs`): the compositor says so with
`AIZOME_SESSION=1`, which it gives only on udev without `--no-session`.
Without the variable (a compositor older than it) `XDG_CURRENT_DESKTOP`
naming Aizome decides. Under a headless or winit compositor
(`scripts/headless-session`, the compositor's and shell's tests, a nested
development session) `AIZOME_SESSION` is `0` and the shell does none of
them, so it never competes with the real session's agent, holds up the
real machine's sleep, or locks when the real session does. ⌃❖Q and the
idle lock still lock it.

## Behaviour

- Everything follows `aizome-config` live: theme (Night, Day, High
  Contrast, Auto by sunset), focus colour, effects tier, size, reduced
  motion, clock format, Deskbar collapsed or not, which systray widgets show.
- When the theme changes, the shell also tells foot instances (SIGUSR1 for
  dark, SIGUSR2 for light) and updates the GTK color-scheme (`dconf write
  /org/gnome/desktop/interface/color-scheme`). Newly opened terminals and
  terminals present when the shell starts or reconnects receive the current
  palette too. Only Foot processes with windows on this compositor are
  signalled, including custom app IDs and foot server windows.
  Applications inside Foot may cache their own colours; see
  [Codex's theme-change limitation and deferred remedy](terminal-theme-changes.md).
- Apps launch through `aizome-sys::apps` (systemd user scopes) with an
  xdg-activation token, so their first window takes focus.
- The shell never blocks its loop: D-Bus, PAM, file indexing and searches
  run on threads and wake it.
- Frame budget: every surface's frames stay under 4 ms (measure with
  `SIDESTEP_FRAME_STATS=1`); the clock tick repaints only the clock, a meter
  only its bars.

## Checking Foot's live theme

With `aizome` and `foot` installed, build the shell and run the real-terminal
regression check:

```sh
nix develop --command cargo build -p aizome-shell
nix develop --command nix shell --inputs-from . nixpkgs#python3 --command \
  python3 scripts/check-foot-theme --shell target/debug/aizome-shell
```

It queries Foot's active background with OSC 11 in two isolated headless
sessions, covering existing and newly opened terminals, custom app IDs,
foot server windows, live Night/Day changes and a shell reload. The second
session shares the Wayland socket name but must keep its own palette.
Logs and screenshots are kept in the printed evidence directory.
