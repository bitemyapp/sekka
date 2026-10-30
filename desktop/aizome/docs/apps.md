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
| Nisshi 日誌 | Logs | `apps/nisshi` | `aizome-nisshi` | `org.aizome.Nisshi` | log book, a day's record (a ship keeps a 航海日誌) |
| Shashin 写真 | Images | `apps/shashin` | `aizome-shashin` | `org.aizome.Shashin` | photograph |
| Annai 案内 | Welcome | `apps/annai` | `aizome-annai` | `org.aizome.Annai` | guidance |
| Koe 声 | Dictation | `apps/koe` | `aizome-koe` | `org.aizome.Koe` | voice |

Also part of the session, not separate apps: the Toolchest, the command
palette, notifications, the lock screen and the polkit dialog (all in
`aizome-shell`), and the file chooser other apps open and save with
(`aizome-portal`, below). The terminal is Hotaru (`apps/hotaru`); foot stays
installed, themed by Aizome's module, as a choice in Settei's Default Apps.

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
Tatami's GTK apps are under `desktop/tatami/`. Yukimi already shares its native
interface across desktops; its backend libraries remain under `apps/yukimi/`.
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
  fills the disk, opening Kura), Startup (Sekka's boot splash, on or off
  from the next start; the button proposes the change in Yukimi with
  `--choose calamares.bootSplash.enable=…`, and "Not offered" where the
  system doesn't declare it), Date & Time (automatic, timezone map or
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
- `nix build .#aizome-checks.settings` boots a VM and changes
  something in every pane by clicking and typing through the compositor
  (`nix/vm-settings-test.nix`, `nix/vm-settings-test.py`), then checks
  the system took it and that Settings shows it when opened again.

### Yukimi (Software)

Yukimi is Sekka's Nix software manager on every desktop: Overview,
Installed, Discover, Updates, History, Storage and Desktops
(read Sekka's `apps/yukimi/README.md`), built with the kit.
It shares the canonical UI-independent crates (`yukimi-config`,
`yukimi-store`, `yukimi-system`) under Sekka's `apps/yukimi/crates/`
with the helper and CLI, and uses Sekka's installed privileged
helper (`yukimi-helper`, with its polkit policy) for system changes, found
where Sekka's Yukimi package installs it.

As built: the helper is the program Sekka's polkit action
`io.github.bitemyapp.Yukimi.change-system` names (its
`org.freedesktop.policykit.exec.path`, read from the installed policy), run
through `/run/wrappers/bin/pkexec --disable-internal-agent`, so it needs
`programs.yukimi.enable` (Sekka sets
it on every desktop) and nothing
of Aizome's session. Its cache is `~/.cache/aizome/yukimi`, reading the former GTK
interface's package index and last check from `~/.cache/yukimi` until it has
its own. `aizome-yukimi --sample` shows a sample system;
`AIZOME_YUKIMI_ETC=/some/etc` looks at another system's configuration
without changing anything.

Yukimi waits up to five seconds through an unavailable authentication
agent by retrying only recognized pre-execution pkexec failures (complete
C-locale diagnostics, matching exit status, and no helper stdout). It never
retries a build or activation failure. An unavailable dialog offers **Try
Again**; denial and cancellation are distinct. Generic failures preserve
the helper's report without inventing a successful rollback.

`nix build .#aizome-checks.polkit-recovery` exercises real Yukimi,
polkit and PAM through two prebuilt system activations that change the
package profile, then shell replacement, an interrupted password dialog,
and retrying after a sustained agent outage. The fixture substitutes
prebuilt profiles for Nix evaluation/building.

It also changes what used to take typing `sudo` commands:

- **Early updates** (Updates): a switch to update Sekka from its `main`
  branch as soon as changes are merged, before the nightly checks
  (`change --sekka --follow sekka=main`; off goes back to `nightly`).
  Sekka keeps that choice through later updates.
- **Desktops**: the system's own choices, as Sekka declares them
  (`programs.yukimi.choices` in `/etc/yukimi/system.json`): which
  desktops are installed, which starts by default, the login screen, and
  the boot splash (a `boolean` choice, `true` or `false`, which the helper
  writes as a Nix boolean).
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
Open-Meteo rounded to 0.01°. **Search for a City** leaves a pending or
failed location search and focuses the sidebar. Disabling location or
closing Tenki cancels its request; a late answer cannot change or save
the position. The shared `aizome-sys` client gives connection setup,
service calls and the position lookup one 20-second deadline in Tenki,
then at most 250 ms to send cleanup without waiting for a service reply.
The shell uses the same client with its existing 30-second deadline.
The private-bus tests stall setup, property reads and cleanup separately;
they never contact the host's location service. Forecasts are cached per place in
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
`nix build .#aizome-checks.loop-images` checks these lifetimes against real
UDisks2 and kernel loop devices in a disposable VM, including error recovery.
Invalid or out-of-range typed
partition sizes stay in the create or resize dialog with an explanation;
they cannot queue the last valid size. Correct the text or choose a size with
the slider to continue. `aizome-checks.tansu-size` exercises the packaged
interface on its in-memory sample disks; it does not test real storage.
Ext2/3/4 resizing checks and repairs the unmounted filesystem before either
growing or shrinking it. A failed check stops before changing the partition
size; a failure in a later step is reported with any earlier changes retained.
`nix build .#aizome-checks.tansu-resize` exercises normal packaged Tansu
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
grouped. `nix build .#aizome-checks.myaku-grouping` exercises actual Hako,
Sumi, Firefox and Myaku launches, independent inspectors, pause/resume and Quit
cancel/confirm without signaling unrelated applications. It is a component test,
not an ISO qualification result.

Port Tatami's Activity (`desktop/tatami/activity` on Sekka's main): apps
and processes with CPU, memory, energy, disk and GPU use; sorting, search,
quit and force quit; graphs of CPU, memory, disk and network over time; the
inspector for a process. Through `aizome-sys::sysinfo`.

Application groups follow identified windows and their process trees: helpers
and commands belong with the app that started them, while a separately
identified app launched from a terminal starts its own group. The desktop
shell and session managers are boundaries. A dedicated application/service
unit can reunite orphaned workers only when its ownership is unambiguous;
sharing a login session or a slice is insufficient. Without compositor window
information, grouping falls back to parentage and explicit application units.
Labels use explicit identities or an unambiguous executable match, never a
generic interpreter name or whichever window happened to be focused.

The inspector's **Grouping** section explains the evidence separately from
each process's **Parent** link. Shared unit membership does not imply direct
parenthood. To inspect the same attribution without opening a window, run
`cargo run -p aizome-myaku --example groups -- NAME` inside `nix develop .#aizome`;
omit `NAME` to include every group. This probe only reads the system.

GPU readings use DRM statistics for AMD/Intel and NVML for NVIDIA (loaded
at runtime, with no NVIDIA dependency on other machines). NVIDIA graphics and
compute process lists are merged by PID without counting their memory twice;
card totals come directly from the driver. The newer NVML memory query separates
driver-reserved memory from allocations, matching `nvidia-smi`'s used figure;
the fallback for older drivers includes reservations. Unavailable counters show
a dash rather than zero. amdgpu leaves out the engines a client hasn't used,
so an amdgpu client listing none is idle, not unknown (the compositor's idle
context on an integrated Radeon, for one).

Across cards, % GPU and % Video are the busiest card's, as their column says:
each card's figure is a share of its own engines, so 18% of an integrated
Radeon and 2% of an RTX 5090 are not 20% of anything. Video memory adds up.
Shared GPU (system memory mapped through DRM's GTT) adds up from the cards
that report it; NVML has no such figure, so the column leaves when only
NVIDIA cards are shown. Most clients are idle at any moment, so at the same
% GPU or % Video the memory held decides: a browser holding 800 MiB comes
before the hundreds of processes that use no card.

In the GPU lens, each card's graph takes a place in the strip along the
bottom, Network's first, then Disk's: how busy it is (violet) and how full
its memory (cyan), over two minutes, with used and total memory above. The
discrete cards come first. With several cards, the table gains a **Card**
column saying which each application uses ("GeForce RTX 5090", "Both"), and
the table can show one card alone: from the toolbar's pop-up beside
Applications, from View ▸ GPUs (which opens the GPU lens from any other), or
by pressing the card's graph (again for every card). The card shown is
framed in the focus colour, as Tansu frames the partition selected; a
graph under the pointer is framed faintly. With one card shown, the table
lists its clients, idle ones holding memory included, with their use of
that card alone; applications on several cards say how many of their
processes use it ("1 of 14 processes"); the status line counts its clients
and gives its clock, temperature and power. The other lenses always show
every card's use, and the inspector gives the selection's use of each card
it uses whichever is shown. The choice follows the card's PCI address
across readings. Cards are named without their maker or codename ("Radeon
Graphics" for "AMD Radeon Graphics (Granite Ridge)"); alike cards add their
bus ("GeForce RTX 4090 · 41:00"), as Myaku's numbering of them would not
match `nvidia-smi`'s.
`aizome-myaku --print` also includes card and application video-memory usage.

The Energy lens is always available, including when no power meter can be
read. Unmeasured values show as dashes, with "No power meter" in the status
line; the selected lens stays open if readings stop.

Choose **View → Energy Meters…**, or click the Energy status, to open a
small live window listing the meters, their watts and whether each counts
toward the total. It explains overlapping readings: a graphics card's
power is already included when the battery supplies the total, and an AMD
package reading is not added on top of RAPL. Unavailable readings are
labelled without assuming why access failed. This optional window uses
the sampler's existing readings and closes independently of the main view.

The processor's meter is its RAPL counters, readable only by root, or else
the meter an AMD integrated Radeon reports: amdgpu's is the whole package's
on an APU, the processor included, and anyone may read it. Without either,
the battery's draw counts while it discharges. Each discrete card's own
meter adds to any of these: NVML's energy counter, or the card's hwmon
meter. A desktop with no battery and RAPL kept for root (the integrated
Radeon and RTX 5090 one) measures through these. Each meter is shared by
use of what it measures: the processor's (or the battery's draw less the
cards') by processor time, a card's by its clients' time on that card,
and what is left is the devices idling. With only the cards' meters,
what uses no card shows a dash.

`cargo test -p aizome-myaku --test gpu` drives the choice from the toolbar,
the strip and the menu on a made-up desktop with both cards
(`fixture::workstation`), and covers figures across cards, other lenses,
cards renumbered or gone, what can't be read, one card and five, and the
smallest window; `--test look` holds its pictures in Night and Day, and
`--test perf` a reading with one card shown (1.2 ms at the median, release,
600 processes). On a machine with NVIDIA compute clients running, `cargo
test -p aizome-myaku --test gpu live_ -- --ignored --nocapture` compares the
sampler with `nvidia-smi` (allowing allocations to change between readings)
and checks that idle amdgpu clients read as idle.

### Nisshi (Logs)

The system journal and the text logs under /var/log, filtered as you type,
with filters suggested by what wrote each message. Console's model in
Aizome's look: places in a sidebar, search with tokens, a message's
details below the list.

- **Places**: All Messages, Errors & Warnings, My Session (your own
  units and processes), Kernel, Crashes (core dumps, segfaults, traps,
  Rust panics), Logins & Security (the auth facilities, sudo, polkit, ssh,
  logind and the login screens), each counted; then the **Sources** that
  wrote the most, counted with the rest of the filter (one with errors
  has a warning glyph), and the **Log Files** under /var/log.
- **Search** matches a message, what wrote it and its unit, in any case
  unless a capital letter is typed. `-word` leaves out, `"a phrase"`,
  `/regex/` (Rust's syntax, over the message), and keys: `app:`, `unit:`
  (`.service` may be left out, `*` matches anything), `pid:`, `level:`
  (that level or worse), `since:` and `until:` (`10m`, `2h`, `14:30`,
  `yesterday`, `2026-10-28T14:30`), `id:` (the catalog id) and
  `FIELD=value`, which the journal itself matches (the boot is read again).
  Words found are drawn in the focus colour. While a key's value is being
  typed, the strip offers the values there are, counted, and Return takes
  the first.
- **Suggestions**: the strip under the toolbar. With nothing selected it
  says what stands out (errors and who wrote most of them, units that
  failed, what crashed, out-of-memory kills, a burst of errors, a message
  so frequent it's a fifth of what shows). With a message selected, it
  offers what fits that message: only its source, its source's
  problems, the topics Nisshi knows its program writes about
  (`src/known.rs`, 21 kinds of program: NetworkManager's Wi-Fi and DHCP,
  the kernel's USB, storage and graphics, sshd's failed logins,
  systemd's failed units, Aizome's displays and input, and more), this
  message's own topics first. Then messages like it and hiding them
  (a message's *shape* is its first line with numbers, addresses, ids and
  quoted names taken out: `src/shape.rs`), its process where its source
  ran as several, its unit, and for a program Nisshi doesn't know, the
  words it uses often and the rest of the journal rarely. Each is counted
  on what's read and offered only when it would narrow what shows.
  Applied, a suggestion is a chip; × removes it.
- **The timeline**: messages over the boot, errors and warnings at the
  foot of each bar, on a square-root scale so a few errors show beside
  thousands of messages. Dragging across it shows that stretch of time;
  a press selects the message nearest that moment.
- **Terminal colours**: what programs write with escape sequences (Aizome's
  compositor through `tracing`, test runners, build tools) shows in the
  theme's colours, red, yellow, green and the rest as Night and Day draw
  them, with bold, dim, italic and underline. Search, shapes, copying and
  export see the plain text; cursor movement, titles and other control
  characters are left out (`src/ansi.rs`). A service's output, which
  journald files at Info, takes the level its line was written with
  (`2026-…Z  WARN target: …`, `ERROR: …`, `[warn]`), and the details say so.
- **Launched apps**: the shell starts an app in a scope named for it
  (`app-aizome-<app ID>-<random>.scope`) and gives its output to the
  journal itself, under the app's ID or a launch's own identifier
  (`aizome-launch-…`, where the shell watches the launch). journald credits
  that output to whatever opened the stream (`cat`, `sh`). Nisshi names it
  after the app, from the scope and the app's desktop entry, so every
  launch of Koe is under Koe. Where an app was launched more than once,
  **This launch** narrows to one attempt (`src/launch.rs`).
- **Details**: the message whole (a core dump's stack trace included),
  when, how long ago, and every field the journal keeps with it, each
  with a button that filters by it. **Around This Time** clears every
  filter and shows a minute either side; **Activity** opens Myaku on its
  process while it runs.
- **Newest first**: the newest message is at the top, as the toolbar's
  **Newest First** says and the Time column's arrow shows; either turns the
  list round (**Oldest First**, the newest at the bottom), and Nisshi keeps
  the choice in `$XDG_CONFIG_HOME/aizome/nisshi.toml`.
- **Live**: new messages of the running boot come in as they're
  written, and the list follows the newest until a message is selected
  (the status line then counts what's new, and goes to it). The toolbar
  picks the boot (this one, the ones before, all of them) and the level.
  A boot with more than 50,000 messages is read newest first; **Read
  Earlier** reads 50,000 more, and **Search the Whole Boot** (Return in
  search) continues through older pages using the same search, including
  source names and text with terminal styling removed.
- **Copy** (❖C) copies the selected messages as journal lines; **Export**
  (⇧❖E) writes what shows as text, or JSON lines for a `.json` file.
- `aizome-nisshi --search TEXT --source NAME --unit NAME --level LEVEL
  --scope problems|mine|kernel|crashes|security --boot N --file PATH
  --cursor CURSOR --launch ID` opens on that, in the running window if
  there is one; `--launch` shows one launch's output, by its identifier;
  `--cursor` selects one message, by the cursor `journalctl --show-cursor`
  prints (another app's "show this in Logs").

As built: the window reads through `aizome-nisshi-helper`, which it runs
as you, and which runs `journalctl --output=json` (only the fields the
list uses; a message's others when it's selected, by its cursor) and
reads /var/log. Each request is data (`src/query.rs`, `src/protocol.rs`):
the helper checks it and builds every argument itself from options that
only read, with journal fields and cursors checked, the environment
cleared and journalctl taken from the system profile. Files are read only
when their real path is under /var/log, opened without following links,
the last 8 MiB of them.

Someone who isn't in `wheel`, `adm` or `systemd-journal` can read only
their own session's journal. The banner says so and offers **Unlock…**:
the window starts a second helper through
`/run/wrappers/bin/pkexec --disable-internal-agent` under the
`org.aizome.Nisshi.read-system-logs` action (`auth_admin_keep`), whose
policy names the helper by its store path in `libexec`. Aizome's polkit
dialog asks for an administrator's password once. That helper reads as
root until **Lock** or the window closes (closing its input ends it and
its reads), and a root-only file (`/var/log/boot.log`) asks the same
way. Cancelling, a refusal and an unavailable dialog are told apart
from pkexec's exit status and its whole message, as Yukimi does. The
Aizome module turns on NixOS's pkexec wrapper for it.

A large boot stays quick: messages are parsed on the reader's thread,
each is classified once as it's read, live arrivals add to the view
rather than making it again, and the overview's suggestions are tallied
as the view is made. A selection's topics are counted a moment after it
shows, and past 30,000 messages search waits for typing to pause before
it filters them all (the field shows each character at once).
`docs/performance.md` has the numbers.

`cargo test -p aizome-nisshi` covers the parser, search, shapes, the
helper's checks and the suggestions on a made-up boot
(`src/fixture.rs`: no message on the machine running the tests is shown),
the window headlessly (`--test ui`), its pictures in Night and Day
(`--test look`), and the real helper over a stand-in journalctl and a
stand-in pkexec (`--test reader`: reading, following, fields, a log
file, and unlocking cancelled, refused and given).
`cargo run -p aizome-nisshi --example sample -- DIR` makes that stand-in
and prints the line to open Nisshi on it. `nix build
.#aizome-checks.nisshi` boots a disposable VM: a user who isn't an
administrator reads only their own journal; with polkit's yes the
installed reader runs as root through the real pkexec wrapper and reads
the system's; another user, with no agent to ask, is refused before it
runs; and as root the reader refuses options, cursors and paths that
aren't reads under /var/log. Real polkit dialogs, and a person typing a
password, are the `polkit-recovery` check's and a desktop's.

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

### Koe (Dictation)

Speak, and the words are typed into the app in front: ❖⌥D to start and
again to finish, recognised on this computer by Whisper (whisper.cpp,
on the graphics card through Vulkan where there is one). Ported from Koe,
its own repository until it joined Aizome; see `docs/koe.md`.

As built: a window of two pages and a session service. **Dictation**
sets everything up without a terminal: Download a Model and Turn On (Base,
148 MB, checked against a pinned SHA-256; nothing is downloaded until
asked), the switch (the service now and at each login, through the unit's
`ExecCondition`, so it runs only for those who turned it on), the models
(download, use, remove), language, translation, the graphics card, what
happens to the words, finishing after a pause, and the shortcuts, which
turning it on adds to `[keys.bindings]` when ❖⌥D and ❖⌥⎋ are free. With
dictation off, the shortcut opens Koe. Settings are saved as they change
in `$XDG_CONFIG_HOME/aizome/koe.toml`, and the service takes them up
between dictations without restarting. **Vocabulary and Teaching** keeps
packs of words (hints and approved corrections, shareable as `.koe.json`)
and records reviewed examples for training later. The service
(`aizome-koe daemon`) puts its microphone in the systray, pastes through
the compositor's IPC after checking the focus and the lock, and keeps
audio and transcripts in memory. `aizome-koe` also has the commands
scripts and agents use (`record`, `status`, `watch`, `model`, `packs`,
`words`, `examples`, `enable`, `disable`).

### The file chooser (portal)

Not an app of its own: the window other apps open and save with. Apps
ask `xdg-desktop-portal` for a file chooser (Chromium, Chrome and
Electron apps such as VS Code, Obsidian, Slack and Signal; Zed; Flatpaks;
Firefox and GTK 3 apps, which the session's `GTK_USE_PORTAL=1` sends
there), and in Aizome sessions `aizome-portals.conf` routes
`org.freedesktop.impl.portal.FileChooser` to `aizome-portal`, which D-Bus
starts as `aizome-portal.service` when it's first asked.

It is Files in miniature, built from Hako's model and look: Hako's
sidebar (Recent when opening files; Home and the person's folders; the
current Desk's folder; the system's disk and mounted volumes), its path
bar from Home (or the Computer) down, and the folder as a list with
Name, Date Modified, Size and Kind, sorted by a header or View ▸ Sort
By, folders first. What the app asked for decides the rest:

- **Open**: files, several with ⇧ and ❖ when the app allows; or folders
  (Choose takes the folder selected, or the one the chooser is in). A
  folder activated is gone into, not chosen.
- **Save**: "Save as" on top, with the app's name for the file; a name
  can be a path (`Drafts/x.md`, `~/Documents/x.md`), and a folder's path
  goes there. A file that's already there is replaced only after asking.
  Save As starts in the file's folder with it selected.
- **Save Files** (several files at once): a folder; names already there
  get a number, as Files keeps both (`notes 2.md`).
- **Filters** in a pop-up (patterns matched without regard to case, and
  MIME types through the shared MIME-info database, subclasses
  included); saving, a name ending in one filter's extension takes the
  next one's. The app's **choices** show as pop-ups and checkboxes, and
  go back with the answer; so does the filter, and Open's files are
  writable.
- The Go menu: Back ❖[, Forward ❖], Enclosing Folder ❖↑, Recent ⇧❖F,
  Home ⇧❖H, Desktop ⇧❖D, Documents ⇧❖O, Downloads ⌥❖L and Go to Folder…
  ⇧❖G (a path, or a file to select). New Folder ⇧❖N where things are
  saved or folders chosen; View ▸ Show Hidden Files ⇧❖.; Return accepts,
  Escape cancels, as does closing the window.
- It remembers in `$XDG_STATE_HOME/aizome/portal/file-chooser.json`
  where each app last chose (apps outside a sandbox mostly have no app
  id, so theirs is the last folder chosen), whether hidden files show,
  and the order. Otherwise it starts where the app said, then in the
  current Desk's folder, then at home.

As built: the service (`aizome-portal`) holds no window. Each request
runs `aizome-portal --file-chooser` with the request as JSON on its
standard input and reads its answer (`null` for cancelled) from its
standard output; the frontend closing the request
(`org.freedesktop.impl.portal.Request.Close`) ends that process and
what it started. The chooser imports the window the app exported
(`parent_window` `wayland:<handle>`, xdg-foreign v2, through Sidestep's
`WindowOptions::foreign_parent` and the kit's
`WindowSpec::foreign_parent`), so the compositor places it over that
window as its dialog; X11 parents and unknown handles leave it on its
own. At most 16 choosers are open at once.

Qt apps keep Qt's dialogs (the session's `QT_QPA_PLATFORMTHEME=gtk3`
draws GTK's), and GTK 4 apps outside a sandbox keep GTK's unless they ask
the portal. Aizome's own apps keep the kit's file panel (`cx.open_panel`,
`cx.save_panel`), a sheet in their window.

`cargo test -p aizome-portal` covers the request and answer on the bus
(`tests/service.rs`, a private D-Bus with a stand-in chooser: the
request handed over, each answer, `Close`), the chooser in every mode on
the kit's stand-in platform (`tests/chooser.rs`), and its pictures in
Night and Day (`tests/goldens.rs`). With the binary built,
`AIZOME_PORTAL_TEST_BIN=target/debug/aizome-portal cargo test -p
aizome-comp --test portal -- --include-ignored` runs the chooser under
a headless compositor over a test window that exported itself: it is
that window's dialog, has the keys, and opens a file with ↓ and Return.
These are component checks, not a check of the installed session.
