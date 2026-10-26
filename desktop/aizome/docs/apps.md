# Aizome's apps

Every app is a crate in `apps/<name>` built with `aizome-kit`. Names are
Japanese words that say what the app is for, in keeping with Aizome and Sekka
(Tatami, Yukimi, Genkan); each carries a plain English generic name, which
the Toolchest, the palette and the Deskbar show beside it and search by.

| App | Generic name | Crate | Binary | App ID | What the name means |
|---|---|---|---|---|---|
| Settei 設定 | Settings | `apps/settei` | `aizome-settei` | `org.aizome.Settei` | settings |
| Yukimi 雪見 | Software | `apps/yukimi` | `aizome-yukimi` | `org.aizome.Yukimi` | snow-viewing, through a shōji's low pane of glass (Sekka's Nix software manager, made native) |
| Hako 箱 | Files | `apps/hako` | `aizome-hako` | `org.aizome.Hako` | box |
| Sumi 墨 | Text Editor | `apps/sumi` | `aizome-sumi` | `org.aizome.Sumi` | ink |
| Fusen 付箋 | Notes | `apps/fusen` | `aizome-fusen` | `org.aizome.Fusen` | sticky notes |
| Koyomi 暦 | Calendar | `apps/koyomi` | `aizome-koyomi` | `org.aizome.Koyomi` | almanac, calendar |
| Tokei 時計 | Clock | `apps/tokei` | `aizome-tokei` | `org.aizome.Tokei` | clock |
| Soroban 算盤 | Calculator | `apps/soroban` | `aizome-soroban` | `org.aizome.Soroban` | abacus |
| Tenki 天気 | Weather | `apps/tenki` | `aizome-tenki` | `org.aizome.Tenki` | weather |
| Tansu 箪笥 | Disks | `apps/tansu` | `aizome-tansu` | `org.aizome.Tansu` | chest of drawers (partitions as drawers) |
| Kura 蔵 | Disk Usage | `apps/kura` | `aizome-kura` | `org.aizome.Kura` | storehouse |
| Myaku 脈 | Activity | `apps/myaku` | `aizome-myaku` | `org.aizome.Myaku` | pulse |
| Shashin 写真 | Images | `apps/shashin` | `aizome-shashin` | `org.aizome.Shashin` | photograph |
| Annai 案内 | Welcome | `apps/annai` | `aizome-annai` | `org.aizome.Annai` | guidance |

Also part of the session, not separate apps: the Toolchest, the command
palette, notifications, the lock screen and the polkit dialog (all in
`aizome-shell`). The terminal is foot, themed by Aizome's module.

## Conventions

- **Desktop entry** `org.aizome.<Name>.desktop`: `Name=<Name>`,
  `GenericName=<generic>`, `Comment`, `Keywords` (the generic name and
  common synonyms: Settings gets "preferences;control panel;system";
  Soroban gets "calculator;math;arithmetic"), `Categories`,
  `Icon=org.aizome.<Name>`, `Exec=aizome-<name> %F` (or `%U`),
  `StartupWMClass=org.aizome.<Name>`, `DBusActivatable=false`.
- **Icon**: `icons/apps/org.aizome.<Name>.svg` (drawn by `icons/genicons.py`),
  installed to `share/icons/hicolor/scalable/apps/`.
- **App ID** on every window (`xdg_toplevel.set_app_id`): the app ID above.
  The compositor treats `org.aizome.*` windows as speaking Aizome's command
  key natively.
- **One instance**: a second launch hands its arguments to the running one
  (a Unix socket in `$XDG_RUNTIME_DIR/aizome-app.<name>.sock`) and exits.
- **State** in `$XDG_STATE_HOME/aizome/<name>/`, **data** in
  `$XDG_DATA_HOME/aizome/<name>/`, **settings** in
  `$XDG_CONFIG_HOME/aizome/<name>.toml`; desktop-wide settings come from
  `aizome-config`.
- **Menus**: the kit's standard app, Edit, Window and Help menus, extended
  by the app; every command reachable from the palette.
- **Look**: only the kit's widgets and the theme's colours; Night and Day
  must both look finished; every target 24pt or more; all of it reachable
  by keyboard.
- **Frame budget**: 4 ms for a typical interaction; measure with
  `SIDESTEP_FRAME_STATS=1` and note exceptions in `docs/performance.md`.
- **Headless test**: each app has at least one test that runs it with
  `SIDESTEP_BACKEND=null`, drives it through messages and checks its state,
  and golden images of its main views through the offscreen renderer.

## What each app does

Sources to port from are in the Sekka repository (`~/work/sekka`, or
`git -C ~/work/sekka show origin/main:<path>` for what's only on main):
Tatami's GTK apps under `desktop/tatami/` and Yukimi under `apps/yukimi/`.
They are MIT OR Apache-2.0 by the same author: copy what helps and rewrite
the UI with the kit. Keep their behaviour and their care for edge cases.

### Settei (Settings)

The macOS System Settings model in Aizome's look: a sidebar (search field
on top, then the user's card, then panes in groups with Lucide glyphs on
small `focus`/dye-coloured tiles) and the pane on the right as forms.
`aizome-settei --pane <id>` opens a pane; `--list-settings --json`
prints every pane and searchable option (id, title, path, keywords) for the
palette. Search filters the sidebar and highlights options.

- **Network**: Wi-Fi (on/off, networks, join with password or hidden, known
  networks, forget, auto-join), wired, VPN and WireGuard (list, connect,
  import, remove), per-connection details (IPv4/IPv6 method, addresses,
  DNS, proxy read-out, metered, MAC), airplane mode, hotspot if supported.
  Port `desktop/tatami/network` (window.rs, details.rs, join.rs) onto
  `aizome-sys::network`.
- **Bluetooth**: on/off, discoverable name, nearby and paired devices,
  pairing with passkey confirmation, connect, disconnect, forget, battery.
- **Sound**: output and input devices with volume, balance and mute, level
  meter for input, per-app volumes, alert sound and volume.
- **Notifications**: Do Not Disturb with schedule, per-app allow/banners/
  sounds, show previews when locked.
- **Displays**: arrangement by dragging output rectangles, resolution and
  refresh, integer scale (1×, 2×, 3×) with fractional opt-in, rotation,
  enable/disable, mirror, primary; changes apply live through the
  compositor IPC (`output-config`) with a 15-second "Keep these settings?"
  revert. Night Light if the compositor supports it. Port the arrangement
  logic and care of `desktop/tatami/displays` (arrangement.rs, model.rs),
  replacing Hyprland calls with Aizome's IPC.
- **Appearance**: Night, Day, Auto (by sunset), High Contrast, with
  previews; focus colour (amber, cyan, rose, lime); effects tier (Full,
  Lean, Austere) with a description of each; Size (small, medium, large);
  wallpaper for Night and Day (a grid of `/run/current-system/sw/share/backgrounds`
  and `~/Pictures`); reduced motion. Wallpaper choices distinguish System
  default (the session's picture) from No picture (a plain background).
  Each theme remembers its own choice, including after logout.
- **Desktop & Deskbar**: Desks (names, folders, add/remove), Deskbar
  (collapsed, pinned apps, show the Shelf), desktop icons (show disks,
  Home, Trash), windows (stack new windows of the same app, double-click on a
  tab: fill or minimise, command key for all apps).
- **Menu Bar**: clock (24h, seconds, date), which systray widgets and meters
  show.
- **Search**: palette scopes on and off, file index folders and
  exclusions, rebuild the index, forget the palette's history.
- **Performance** (Display & Performance): the renderer (GPU or software,
  with which one is running), effects tier, frame statistics from
  `aizomectl stats` and the apps (p50/p95 against the 4 ms budget, live),
  the refresh rate.
- **Battery & Power**: power mode, battery health and a charge history,
  turn the display off after, lock after, what the power key does, sleep
  when the lid closes (shows the system configuration's value if NixOS
  manages it).
- **Lock Screen**: lock after idle, require the password after sleep or
  screen off, show the clock.
- **Users & Groups**: the current user's name, picture, password; other
  accounts (AccountsService), admin or standard, add and remove, automatic
  login.
- **Keyboard**: layouts (searchable list from `aizome-sys`'s xkb data, with
  variants), switching shortcut, key repeat delay and rate, modifier keys
  (swap Alt/Super, Control/Super or Control/Alt on both sides or just one),
  Caps Lock as (Caps Lock, Control, Escape, Compose), compose key, and the
  list of Aizome's shortcuts (from the compositor's `keys` config, editable).
  In Typing → Keys → Modifier keys, choose **Swap Alt and Super** for a
  Mac-style position, or the left-only swap to keep Right Alt for accents.
  Changes apply immediately to all keyboards and persist across logins;
  **Default** restores the modifier arrangement. Open this control directly
  with `aizome-settei --pane keyboard.modifiers`.
- **Mouse** and **Trackpad**: pointer speed and acceleration, natural
  scrolling, tap to click, two-finger or corner right-click, disable while
  typing, scroll speed.
- **Printers & Scanners**: printers with status, add (discovered or by
  address), remove, default, test page, queue.
- **General**: About (the mark, Sekka version, hardware summary, hostname
  rename), Software Update (opens Yukimi's Updates), Storage (a bar of what
  fills the disk, opening Kura), Date & Time (automatic, timezone map or
  search, 24h), Language & Region (locale, formats preview), Default Apps
  (browser, mail, terminal, text editor, files, images, PDF, music, video),
  Login Items (autostart entries on and off).
- **Accessibility**: reduced motion, High Contrast, larger text (Size),
  cursor size, sticky and slow keys (xkb options), visual alert flash.

As built, beyond the above:

- Changes reach the system as soon as they're made; a name typed in a
  field (a Desk's, the computer's, yours, the Bluetooth name) is kept when
  the field loses the focus as well as on Return. The host name still
  waits for Return, since it asks first.
- **Displays** shows Night Light where the compositor says a display takes
  a gamma table (the switch is `[night_light] enabled`, applied at once),
  with today's sunset and sunrise where the computer is; where none does,
  it says why there's no switch.
- **Sound**: the alert sound and volume are the shell's: it plays the
  alert for the system bell (xdg-system-bell, through the compositor),
  notifications' sounds at the alert volume, and, with "Play feedback
  when volume is changed", the theme's `audio-volume-change` at the new
  volume after the volume keys or the Sound panel's slider; all from the
  freedesktop sound theme with `pw-play`. Flash the screen for alerts
  flashes its edge as well as sounding.
- **Appearance**: Auto following sunset and sunrise turns at them where
  the computer is, and says when and where ("Day from sunrise (08:15) to
  sunset (18:57) in Paris"). Where it is: what GeoClue tells the shell
  (asked as `org.aizome.Shell` while something follows the sun, kept in
  `[location]` to 0.01°), else the time zone's city from tzdata; the
  compositor's Night Light, the shell, the apps and Settings reckon it the
  same way (`aizome_config::location`).
- **Date & Time** shows automatic time as set by the system configuration
  where NixOS keeps systemd's units in the store (timedated can't switch
  it there): change `services.timesyncd.enable`.
- **Default Apps**: the terminal chosen is also `[keys] terminal`, what
  ❖Return starts.
- **Printers & Scanners**: Add Printer leaves out printers already set
  up, cups-browsed's queues among them.
- **Trackpad** says when no trackpad is connected; its settings are kept
  for one.
- **Users & Groups** reads a picture again when AccountsService replaces
  it at the same path. With `users.mutableUsers = false` the account
  types show but can't change; Remove names the account's own home
  folder.
- **Network**: search finds every option whatever the pane shows (with
  no VPNs, "Add VPN"; with Wi-Fi off, the network lists' options show the
  Wi-Fi switch). A password sheet keeps what's typed while another join
  goes, and a join that fails opens its sheet again as it was, with why.
  A saved network's new password or enterprise account is tried in
  memory and kept only once it joins, so a refused one costs nothing.
  The hotspot says the network's name others see. Airplane mode where
  `/dev/rfkill` can't be written switches NetworkManager's radios and
  BlueZ's adapters off, and those back on after.
- **Bluetooth**: a pairing sheet closes when BlueZ cancels it, when our
  pairing ends (a code only shown included), and a device that asked to
  pair (Authorize) leaves no spinner once answered.
- **Displays** previews mirroring live under "Keep these display
  settings?" as it does the other risky changes, and follows `[[outputs]]`
  changed elsewhere while it's open (panes hear when `settings.toml`
  changes on disk).
- **Language & Region**: the formats preview follows the locale localed
  has, whenever it changes.
- **Search**: Forget removes the palette's history for good: the shell
  records picks into the file and reads it again when it changed.
- GTK and libadwaita apps follow the theme and the focus colour through
  dconf (`color-scheme`, `gtk-theme`, `accent-color`, high contrast),
  which the shell writes.
- The NixOS module lets a local, active administrator (wheel) change the
  time zone, the computer's name, the language and keyboard, and
  networks without a password prompt, as GNOME does.
- `nix build .#checks.x86_64-linux.settings` boots a VM and changes
  something in every pane by clicking and typing through the compositor
  (`nix/vm-settings-test.nix`, `nix/vm-settings-test.py`), then checks
  the system took it and that Settings shows it when opened again.

### Yukimi (Software)

Yukimi, Sekka's Nix software manager, made native: the same Overview,
Installed, Discover, Updates, History and Storage views and the same
behaviour (read `apps/yukimi/README.md`), rebuilt with the kit. Copy the
UI-independent crates (`yukimi-config`, `yukimi-store`, `yukimi-system`)
into `apps/yukimi/crates/` and keep using Sekka's installed privileged
helper (`yukimi-helper`, with its polkit policy) for system changes, found
where Sekka's Yukimi package installs it.

As built: the helper is the program Sekka's polkit action
`io.github.bitemyapp.Yukimi.change-system` names (its
`org.freedesktop.policykit.exec.path`, read from the installed policy), run
through `/run/wrappers/bin/pkexec` with the GTK Yukimi's arguments, so it
needs `programs.yukimi.enable` (Sekka sets it on every desktop) and nothing
of Aizome's. Its cache is `~/.cache/aizome/yukimi`, reading the GTK
Yukimi's package index and last check from `~/.cache/yukimi` until it has
its own. `aizome-yukimi --sample` shows a sample system;
`AIZOME_YUKIMI_ETC=/some/etc` looks at another system's configuration
without changing anything.

Beyond the GTK Yukimi, it changes what used to take typing `sudo` commands:

- **Early updates** (Updates): a switch to update Sekka from its `main`
  branch as soon as changes are merged, before the nightly checks
  (`change --sekka --follow sekka=main`; off goes back to `nightly`).
  Sekka keeps that choice through later updates.
- **Desktops**: the system's own choices, as Sekka declares them
  (`programs.yukimi.choices` in `/etc/yukimi/system.json`): which
  desktops are installed, which starts by default, and the login screen.
  - Picking changes nothing. Apply asks, then builds once
    (`change --choose SETTING=VALUE[,VALUE]… --mode auto`).
  - A change the running system can't take now, such as another login
    screen, waits for a restart, and the page offers Restart Now.
  - A default desktop stays among the desktops. An option that needs
    another choice (Aizome's login screen needs Aizome) is offered only
    with it.
  - The helper refuses any setting or value the system doesn't declare.
- **Proposing from the command line**: `aizome-yukimi --choose
  calamares.loginScreen=aizome --early` opens Yukimi with that change
  proposed. A running Yukimi takes it over. Nothing changes until it's
  confirmed and an administrator password is entered.

### Hako (Files)

A file manager in NeXT's spirit: Miller columns as the main view (with list
and icon views as alternatives), a sidebar (Places, the Desk's folder,
disks, Recent, Trash), a path bar, Quick Look on Space (images, text,
PDF first page if cheap), search through `aizome-search`, file operations
with a progress panel (copy, move, trash, restore, delete, rename, new
folder, duplicate, compress and extract with `bsdtar`), Open With, Get Info
(size, kind, dates, permissions), drag and drop with other apps, the
Desktop and the Shelf, image thumbnails cached in
`$XDG_CACHE_HOME/thumbnails` (the freedesktop spec), hidden files toggle,
sorting. Tabs are Aizome's own tab groups, so no tabs inside.

Refreshing a listing keeps a still-selected folder's deeper columns
open. If that folder leaves the listing (including Put Back from Trash),
its obsolete columns close and keyboard focus returns to the surviving
parent when needed.

Encrypted removable volumes stay in the file sidebar after unlocking. Hako asks for a volume passphrase before mounting a locked one; Cancel leaves it locked, and an incorrect passphrase can be retried. While unlocking, the prompt waits for the device operation to finish. System volumes and devices hidden by UDisks policy stay excluded from the removable list.

### Sumi (Text Editor)

The board's editor: a toolbar with the language pop-up ("Markdown ⌄"), B,
I and </> buttons, and the status "Ln 48, Col 12 · UTF-8 · LF"; line numbers
in `faint`; the current line on the board's 10% `focus` band; Markdown styled as you type
(headings in `blue` 600, code in `green`, emphasis); syntax colouring for
common languages (Rust, Python, JS/TS, C, Nix, shell, TOML, JSON, YAML)
with a small built-in highlighter; Format menu as on the board (Font ▸,
Paragraph ▸, Bold ❖B, Italic ❖I, Code ❖E, Link… ❖K, Heading ▸, Quote,
Copy Style ⌥❖C, Paste Style ⌥❖V); Find and Replace with regex; encodings
and line endings; autosave to a sidecar and restore after a crash; recent
files; open large files quickly.

As built (`apps/sumi`): a window per document. Code keeps the board's ❖E,
so Use Selection for Find (❖E on a Mac) is ⌥❖E, beside Find and Replace's
⌥❖F; Quote is ⇧❖' (heard as ⇧❖" too, which the key types on most
layouts). Unsaved text is kept in `$XDG_STATE_HOME/aizome/sumi/<id>.txt`
and the open documents in `session.json` there, written 0.8 s after the
last change: after a crash or a logout, and after Quit (which keeps
everything rather than asking), the documents come back as they were,
Untitled ones included; closing a window with unsaved changes asks.
Files are read as UTF-8, UTF-16 (by its byte order mark or its zeros) or
Latin-1, with LF, CRLF or CR line endings kept (File ▸ Encoding and Line
Endings switch them; Reopen with Encoding reads again), and saved
atomically, through symbolic links and with the file's permissions.
Files it opens and saves go to `recently-used.xbel` (and Open Recent).
View settings are in `$XDG_CONFIG_HOME/aizome/sumi.toml`.

### Fusen (Notes)

Notes as Markdown files in `~/Notes` (so they're plain, searchable and
agent-readable): a list with search and pinned notes, folders, checklists,
a preview line; the editor is the kit's text editor with light Markdown
styling. Any note can float as a sticky (a small always-on-top window in
the note's colour). Takes "New Note with Selection" from the Services menu.

As built (`apps/fusen`): `~/Notes` is made when the first note is
written; a note's file is named after its title (and renamed with it,
when Fusen named it), folders are its subdirectories, and pins and
colours (amber, kamenozoki, hanada, green, rose) are kept in
`~/Notes/.fusen.json`, keyed by path, so the notes stay plain. Files other
programs (agents) change or add show within 3 s; deleting puts a note in
the freedesktop Trash. A click on a task's `[ ]` ticks it; ❖-click opens a
link. A sticky is a utility window on its colour's paper; under Aizome
the compositor's IPC (`float`, `resize`, `move-to-desk`) keeps it above
other windows and puts it back where it was, and Fusen asks where its
stickies are every 4 s, keeping that in `$XDG_STATE_HOME/aizome/fusen/stickies.json`.
While there are stickies, `$XDG_CONFIG_HOME/autostart/org.aizome.Fusen.Stickies.desktop`
runs `aizome-fusen --stickies` at login (the stickies only). `aizome-fusen
--new TEXT`, `--new-from-stdin` and a file (the shell's Services hand
selected text over in one) make new notes; a second launch hands its
request to the running Fusen. Quit keeps the stickies; closing one puts
it away. A failed note write keeps the draft dirty and shows a persistent error
with Retry. Close (including a sticky's decoration) and Quit retry saving first;
if that fails, Keep Open preserves the draft and Retry saves before finishing
the requested exit. Restoring folder access allows recovery without retyping.
Rescans retain dirty notes even when their backing directory is unreadable.
Force termination and session shutdown are not a durable draft-recovery system.
`checks.fusen-save` exercises permission loss, visible errors, cancelled exits,
restored-access retry, sticky close and exact-byte persistence in a packaged VM;
these diagnostic fault injections are component evidence, not ISO qualification.

### Koyomi (Calendar)

Month, week and day views; events with time, all-day, location, notes,
repeat (RRULE subset), alerts; calendars as `.ics` files in
`$XDG_DATA_HOME/aizome/koyomi/`, import and export `.ics`; a small
`aizome-koyomi --agent` user service fires alerts as notifications. Week
numbers and the first day of the week from the locale.

### Tokei (Clock)

World clocks (search cities, with day/night and offset), alarms (repeat
days, label, sound; scheduled as systemd user timers so they fire when the
app is closed, and wake from sleep when asked), a timer and a stopwatch with
laps. Tabular IBM Plex Mono digits.

On World Clock, Shift+Super+Up/Down moves the selected city. The Clock menu
shows those shortcuts and disables each direction at its list boundary.
Alt+Super+Up/Down remains the desktop's Fill/Minimise shortcuts. The
`tokei-reorder` VM check exercises actual packaged input, both boundaries,
unchanged window geometry during city moves, global window controls, and
the saved order after Quit and relaunch.

### Soroban (Calculator)

Basic, Scientific and Programmer (hex, dec, oct, bin; bitwise; word size)
modes; typed expressions through `aizome-search::calc`; a paper tape of the
history that can be copied or saved; keyboard first.

### Tenki (Weather)

Current conditions, hourly for 48 hours and ten days ahead from Open-Meteo
(no key needed), cities found with Open-Meteo's geocoding and the current
place through geoclue; saved places; units (metric, imperial); icons drawn
from Lucide's weather set; a gentle failure state offline.

As built: a sidebar of places (search on top, rows dragged to reorder, the
current location pinned first) and the place showing: the temperature
large in Archivo light, the condition, high and low, an hourly strip with
rain bars, ten days with ranges in the dye ramp (kamenozoki to kohaku) on
one scale, and tiles for feels-like, UV, wind (with a compass), humidity,
the sun and precipitation. The first launch keeps the city of the
machine's time zone and asks once whether to use the location; GeoClue is
asked at city accuracy as `org.aizome.Tenki`, and positions go to
Open-Meteo rounded to 0.01°. Forecasts are cached per place in
`$XDG_CACHE_HOME/aizome/tenki/` (the window opens on the cache and says
"Updated 12 min ago", or "Offline"), places are kept in
`$XDG_DATA_HOME/aizome/tenki/places.json`, and units and the location
choice in `$XDG_CONFIG_HOME/aizome/tenki.toml`. Tests run on answers
recorded once from Open-Meteo (`apps/tenki/tests/fixtures`, re-recorded
with `aizome-tenki --record-fixtures DIR`). NixOS needs
`services.geoclue2.enable = true` and
`services.geoclue2.appConfig."org.aizome.Tenki" = { isAllowed = true; isSystem = false; }`
for the current location (and a working location source, such as
`services.geoclue2.geoProviderUrl` pointed at BeaconDB).

City search debounces each query separately: editing while an older request
waits starts the new query, and old results or errors cannot replace it or
leave it stuck searching. `search_generation` tests hold background replies
and release them in both orders, including Return before the current answer
and clearing the field. `nix/vm-tenki-live-search-test.nix` is a separate
manual packaged UI regression with delayed networking; build its `.driver`
and run it with private `XDG_RUNTIME_DIR` and `TMPDIR` directories. It needs
a reachable Open-Meteo and is deliberately outside the offline flake checks.

### Tansu (Disks)

GParted's job with Aizome's care: pick a disk; a partition map bar
(segments sized by bytes, coloured by filesystem, used space shaded); the
partition table (name, filesystem, label, mount point, size, used, flags);
operations queued and listed (create, delete, format, resize, label, flags,
check, mount, unmount, create a partition table, unlock LUKS) and applied
only after a confirmation that lists them in plain words; SMART health;
images (attach `.iso`/`.img` as loop devices). Manual attachments stay until
Detach, including blank images opened for partitioning; read-only attachments
cannot be written. The system image-mount helper enables automatic detach only
after mounting succeeds, and releases its own loop if probing or mounting fails.
`nix build .#checks.x86_64-linux.loop-images` checks these lifetimes against real
UDisks2 and kernel loop devices in a disposable VM, including error recovery.
Invalid or out-of-range typed
partition sizes stay in the create or resize dialog with an explanation;
they cannot queue the last valid size. Correct the text or choose a size with
the slider to continue. `checks.x86_64-linux.tansu-size` exercises the packaged
interface on its in-memory sample disks; it does not test real storage.
Ext2/3/4 resizing checks and repairs the unmounted filesystem before either
growing or shrinking it. A failed check stops before changing the partition
size; a failure in a later step is reported with any earlier changes retained.
`nix build .#checks.x86_64-linux.tansu-resize` exercises normal packaged Tansu
mount, unmount, grow and shrink controls on a disposable VM disk, checking the
actual filesystem size and preservation of ten files after each resize.
Everything goes through UDisks2 (`aizome-sys::disks`). Tests never use host
disks: unit tests use mocks, and storage integration tests use disposable disks
and loop images inside VMs.

### Kura (Disk Usage)

DaisyDisk's job: scan a disk or folder (fast, parallel, showing progress),
show it as a sunburst of rings by size with the largest items labelled,
drill in and out, a list beside it, a Collector to gather items and move
them to the Trash in one go, Nix-aware (the store's size, with a button
that opens Yukimi's Storage rather than deleting store paths). Every scan
is also written for agents: `$XDG_CACHE_HOME/aizome/kura/index.json` (a
stable, documented schema: the tree to a depth, the largest files and
folders, sizes, modified dates, kinds, and hints such as caches, build
outputs, old downloads and duplicates) and `index.md` (a readable summary of
what could be freed and how), plus `aizome-kura scan [PATH] --json` and
`aizome-kura report` on the command line. Document the format in
`docs/kura-index.md`.

As built: the window opens on the disks (each mounted filesystem with its
used and free space, and the Nix store's size with "Open Yukimi's
Storage", which runs `aizome-yukimi --page storage`), or scans the folder
given on the command line. A scan shows its bytes, counts and the folder
being read, and stops with ❖. ; it runs on a thread per CPU into a flat
arena (allocated and apparent sizes, hard links once, no symlinks
followed, one filesystem, unreadable folders marked): about half a second
for this laptop's 900,000-file home with a warm cache. The results are a
sunburst on the kit's `canvas` (five rings, the dye ramp by top-level
branch and lighter outwards, labels on the largest segments, along the arc
where that's where they fit; hovering lights a segment and names it in the
middle; a click goes in, the middle goes up; the rings move over the
motion time) beside the folder's list (largest first, with each row's
colour, share and size; the keyboard is there after a scan: arrows,
Return to go in, ❖↑ out). Items reach the Collector by dragging a segment
or a row onto it, by Collect in their menu or ❖K; it holds no item inside
another, refuses anything in the Nix store, totals what it holds, and
"Move to Trash…" (❖⌫) asks, then moves it all by the freedesktop Trash
specification. Suggestions (the side panel's other half, and the
toolbar's "179 GB to free") show the index's hints by kind, most
reclaimable first: each with what it frees, its risk as a badge (safe,
low risk, your call), what it is in plain words, its paths (ticked by
default when safe or low risk, never when they're the person's to
decide) and its action: Move to Trash for caches, build outputs, logs and
old downloads; Empty Trash; Open Yukimi's Storage for Nix; the command to
run, with Copy, for containers and Flatpak; and for duplicates, the
copies of each set side by side to keep one. Every action asks first.
Picking a hint moves the rings to the folder holding its paths and dims
everything else; after an action, the folders it touched (and the Trash)
are scanned again and grafted into the tree, and the hints are found
anew. The command line also has `aizome-kura hints [--json]` and
`aizome-kura trash PATH…` (the same Trash, for agents), and `scan`,
`report` and `hints` take `--output DIR` to keep the index elsewhere.
Nothing in NixOS needs configuring; `nix` on the `PATH` lets Kura measure
the store.

### Myaku (Activity)

Independent Aizome application scopes bound process groups, including an editor
opened by Hako. The desktop shell is a launcher, not one application containing
everything it starts. Ordinary descendants and browser helper scopes remain
grouped. `nix build .#checks.x86_64-linux.myaku-grouping` exercises actual Hako,
Sumi, Firefox and Myaku launches, independent inspectors, pause/resume and Quit
cancel/confirm without signaling unrelated applications. It is a component test,
not an ISO qualification result.

Port Tatami's Activity (`desktop/tatami/activity` on Sekka's main): apps
and processes with CPU, memory, energy, disk and GPU use; sorting, search,
quit and force quit; graphs of CPU, memory, disk and network over time; the
inspector for a process. Through `aizome-sys::sysinfo`.

### Shashin (Images)

The image viewer the desktop opens pictures with: PNG, JPEG, GIF
(animated), WebP, BMP, TIFF and ICO through the `image` codecs Sidestep
reads with, and SVG through resvg, rendered at the zoom.

- **The window**: the picture on `sunk`, fitted (never enlarged past
  100%); zoom with ❖+/❖−, ❖-scroll and pinches about the pointer, ❖0 for
  100% (a picture pixel to a device pixel, so crisp; nearest pixel above
  200%), ❖9 to fit, double-click between the two; pan by dragging or
  scrolling. The toolbar has ‹ ›, the zoom, the turns, the filmstrip, the
  slideshow and Get Info, with `4032 × 3024 · 37% · 12 of 48` at its right.
- **The folder**: ←/→ (Space, Home, End) step through the folder's
  pictures in Hako's natural order; several files opened together are
  stepped through on their own. F shows a filmstrip of thumbnails kept in
  the freedesktop cache (`$XDG_CACHE_HOME/thumbnails/normal`, shared with
  Hako).
- **Edit**: Rotate Left/Right (❖L/❖R) turn the view until Save (❖S): a
  JPEG's EXIF orientation is rewritten (or added) and not a pixel is
  re-encoded; other kinds are decoded, turned and written again in their
  format (not SVG, nor animated WebP and PNG). Closing with turns unsaved
  asks. Copy (❖C) offers the picture as `image/png`; Set as Wallpaper ▸
  Night, Day or Both first decodes the file with the desktop's renderer,
  then writes `appearance.wallpaper_night`/`_day` and asks the compositor
  to `reload-config`. Wallpapers support PNG, JPEG, GIF (the first frame),
  WebP, BMP, TIFF and ICO; SVG viewing works, but SVG wallpapers are not
  supported. Unreadable, corrupt or unsupported files leave both existing
  wallpapers unchanged and show an error. The setting keeps the source
  path, so the image must remain there. Show in Hako (⌥❖R) runs
  `aizome-hako --select`; Move to Trash (❖⌫) follows the freedesktop
  Trash, and Undo (❖Z) puts the picture back. Get Info (❖I): kind,
  dimensions, size, dates, where, and the camera, lens and settings from
  the EXIF (`kamadak-exif`).
- **Viewing**: full screen (⌃❖F, or the desktop's), and a slideshow
  (⇧❖F, five seconds a picture, Esc ends it).
- **Speed**: pictures decode on the kit's workers; a camera's EXIF
  thumbnail shows first; the next and previous pictures are read ahead;
  every frame at rest is a drawing made for exactly that zoom, drawn one
  to one (see `docs/performance.md` for what that costs).
- One instance: a second launch hands its files over
  (`aizome-app.shashin.sock`).

### Annai (Welcome)

A welcome window shown once at first login and from Help ▸ Aizome Tour:
short pages that make Aizome's ideas plain, each with a small live
illustration drawn with the kit's canvas in Aizome's look (no
screenshots): Welcome (the mark, the name and 藍染), Tabs, the menu bar and
the menu at the pointer, the palette, the Toolchest, Desks and the
Deskbar, Super as the command key, and Make it yours (buttons that open
Settei's Appearance, Displays, Network and Keyboard). ←/→ and Return
step; a "Show at login" checkbox.

`aizome-annai --first-run` shows it only if it has never been shown (or
"Show at login" is on) and exits at once otherwise; what it remembers is
`$XDG_STATE_HOME/aizome/annai/state.json`. The session starts it through
`apps/annai/data/org.aizome.Annai.Welcome.desktop`, an XDG autostart entry
(`OnlyShowIn=Aizome`) installed into `/etc/xdg/autostart`.
