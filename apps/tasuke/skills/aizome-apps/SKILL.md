---
name: aizome-apps
description: "Aizome's own apps — Settei (Settings), Yukimi (Software), Hako (Files), Sumi (Text Editor), Fusen (Notes), Koyomi (Calendar), Tokei (Clock, alarms), Soroban (Calculator), Tenki (Weather), Tansu (Disks), Kura (Disk Usage), Myaku (Activity), Nisshi (Logs), Shashin (Images), Shiori (PDF), Utsushi (Media Player), Koe (Dictation), Hotaru (Terminal), Annai (Welcome): binaries, data locations, CLI flags, menu shortcuts and typical tasks. Load when an app is named or for notes, files, alarms, logs, processes, disk usage, images, PDFs, media, dictation or the terminal."
---

# Aizome's apps

Every app: binary `aizome-<name>`, app ID `org.aizome.<Name>`, one
instance (a second launch hands its arguments to the running one over
`$XDG_RUNTIME_DIR/aizome-app.<name>.sock` and exits). Settings in
`~/.config/aizome/<name>.toml`, state in `~/.local/state/aizome/<name>/`,
data in `~/.local/share/aizome/<name>/` where an app has them. Every app
has Hide ❖H, Hide Others ⌥❖H, Quit ❖Q, Undo ❖Z, Redo ⇧❖Z, Cut/Copy/Paste
❖X/❖C/❖V, Select All ❖A, Minimise ❖M and Help ▸ Aizome Tour; most have
File ▸ Close Window ❖W (Hotaru's ❖W closes a pane).
Open any of them from the palette (❖Space, type its name or its English
name, Return). Files moved "to the Trash" go to `~/.local/share/Trash`
(Hako's Trash; recoverable with Put Back).

## Settei — Settings (`aizome-settei`)
`--pane ID[.OPTION]`, `--list-settings [--json]`, `--keyboard "Name (vendor:product)"`
(with `--pane keyboard.setup`), `--sample` (sample computer; still writes
settings.toml). Panes and keys: see aizome-settings.

## Yukimi — Software and updates (`aizome-yukimi`)
Pages (❖1–❖7): Overview, Installed, Discover, Updates, History, Storage,
Desktops (only where the system declares choices). Desktop entry's
generic name is "Package Manager". Outside Aizome its shortcuts use Ctrl.
- `--page overview|installed|discover|updates|history|storage|desktops`;
  `--choose SETTING=VALUE[,VALUE]` (repeatable) proposes a change, nothing
  happens until confirmed with a password; `--early` / `--nightly`
  propose following `main` / the nightly; `--sample`.
- **Discover**: search Nixpkgs and the catalog; **Try It** (runs it once,
  installs nothing), **Install for Me** (`nix profile`, no password),
  **Install for Everyone** (writes `/etc/nixos/yukimi.nix`, rebuilds,
  password). Apps needing a NixOS program (Steam) install for everyone only.
- **Installed**: grouped by why; Remove from your profile / Remove for
  everyone works for Yukimi's, `nix profile`, hand-listed
  `environment.systemPackages`/`users.users.<you>.packages` entries and the
  installer catalog (`calamares.applications`).
- **Updates**: Check for Updates ❖U; **Update** (same as `sekka-update`);
  Early updates switch; per-input branch choice (Follow).
- **History**: every generation; open one to see changes; **Return…** goes
  back (and restores that generation's configuration files).
- **Storage**: what fills the store; **Clean Up…** (System ▸ Clean Up the
  Store…) deletes old generations and collects garbage, then updates the
  boot menu.
- **Desktops**: desktops, default desktop, login screen, boot splash,
  terminal shell; Apply… builds; Restart Now when needed.
- Completion messages offer Restart Now or Sign Out; Sign Out ends the
  session (ask first). Cache: `~/.cache/aizome/yukimi`.

## Hako — Files (`aizome-hako [PATH…] [--select FILE]`)
Columns ❖1, List ❖2, Icons ❖3; Quick Look Space; Get Info ❖I; Rename F2;
New Folder ⇧❖N; New Window ❖N; Duplicate ❖D; Compress/Extract (bsdtar);
Move to Trash ❖⌫; in Trash: Put Back ❖⌫, Delete Immediately… ⌥❖⌫; Empty
Trash… ⇧❖⌫; Copy Path ⌥❖C; Move Here ⌥❖V (after Copy); Find… ❖F; Show
Hidden Files ⇧❖.; Go: Back ❖[, Forward ❖], Enclosing ❖↑, Home ⇧❖H,
Desktop ⇧❖D, Documents ⇧❖O, Downloads ⌥❖L, Recent ⇧❖F, Go to Folder… ❖L
or ⇧❖G. Settings `~/.config/aizome/hako.toml`. Unlocks encrypted USB
disks (asks the passphrase). Tabs are Aizome's window tabs.

## Sumi — Text Editor (`aizome-sumi [FILE…]`)
New ❖N, Open ❖O, Save ❖S, Save As ⇧❖S, Revert to Saved, Open Recent ▸,
Encoding ▸, Line Endings ▸, Find ❖F, Find and Replace ⌥❖F (regex), Find
Next ❖G / Previous ⇧❖G, Go to Line… ❖L, Paste as Plain Text ⌥⇧❖V, Zoom
❖= / ❖- / ❖0. Markdown Format menu: Bold ❖B, Italic ❖I, Code ❖E, Link ❖K,
Heading ⌥❖1–4, Quote ⇧❖'. **Quit keeps every document, saved or not**:
unsaved text lives in `~/.local/state/aizome/sumi/` (`session.json`,
`<id>.txt`) and returns next launch; closing a window with changes asks.
View settings `~/.config/aizome/sumi.toml`.

## Fusen — Notes (`aizome-fusen`)
Notes are Markdown files in **`~/Notes`** (made on the first note; file
named after the title; folders are subfolders). Pins and colours in
`~/Notes/.fusen.json`; sticky positions in
`~/.local/state/aizome/fusen/stickies.json`. Files changed by other
programs show within 3 s; Delete Note ❖⌫ moves to the Trash.
- `--new TEXT` (repeatable), `--new-from-stdin`, `--stickies`, `FILE` (a
  note, or text to make one).
- New Note ❖N, New Folder ⇧❖N, Search Notes ❖F, Checklist ⇧❖L, Pin ⌥❖P,
  Colour ▸ (Amber, Kamenozoki, Hanada, Green, Rose), Open as Sticky ❖T.
- A note can be created without the GUI: `aizome-fusen --new "Groceries
  - milk"`, or write a `.md` file in `~/Notes` as the user.
- "Can't save": check `~/Notes` belongs to the user; Fusen keeps the draft
  and offers Retry.

## Koyomi — Calendar (`aizome-koyomi`)
Calendars are `.ics` files in `~/.local/share/aizome/koyomi/<id>.ics`
(Home and Work at first); removed ones go to `.trash/` there. Day ❖1,
Week ❖2, Month ❖3, Year ❖4, Today ❖T, ❖←/❖→, New Event ❖N, New Calendar
⇧❖N, Import… ⇧❖I, Edit Event ❖I, Delete ❖⌫, Settings ❖,.
- CLI: `--upcoming [--json] [--days N] [--limit N]` (default 7 days),
  `--list [--json]`, `--export ID` (iCalendar to stdout),
  `--import FILE --calendar ID`, `--new`, `--show today|tomorrow|YYYY-MM-DD`.
- Alerts come from `aizome-koyomi-agent.service` (user unit), whether or
  not Koyomi is open.

## Tokei — Clock (`aizome-tokei`)
World Clock ❖1 (Add City ❖N, move ⇧❖↑/⇧❖↓), Alarms ❖2 (New Alarm ❖N,
Edit ❖E, On/Off ❖T, Delete ❖⌫), Timer ❖3 (Space start/pause, Esc
cancel), Stopwatch ❖4 (Space, L lap, R reset). Alarms in
`~/.local/share/aizome/tokei/alarms.json`; each is a transient user timer
`aizome-alarm-<id>` (fires with Tokei closed; an alarm set to wake the
computer wakes it from sleep).
`aizome-tokei --sync` reschedules them (runs at each login); check with
`systemctl --user list-timers 'aizome-alarm-*'`.

## Soroban — Calculator (`aizome-soroban`, no flags)
Basic ❖1, Scientific ❖2, Programmer ❖3 (hex ⇧❖H, dec ⇧❖D, oct ⇧❖O, bin
⇧❖B, word size), Paper Tape ❖T, Copy Tape ⇧❖C, Save Tape ❖S, Clear Tape
❖K, Clear Esc, Equals Return. Type expressions (`1234*5678`). The palette
also calculates (`= result`, Return copies).

## Tenki — Weather (`aizome-tenki`)
Open-Meteo forecasts (no account): Add Place ❖N, Previous/Next Place
❖[ / ❖], Refresh ❖R, Units (Metric/Imperial), Show Current Location
(GeoClue, asks once). Places `~/.local/share/aizome/tenki/places.json`,
settings `~/.config/aizome/tenki.toml`, cache `~/.cache/aizome/tenki/`.
Offline it shows the cache and says "Offline".

## Tansu — Disks (`aizome-tansu [IMAGE…] [--show=DEVICE] [--sample]`)
Partition map and table per disk; operations are **queued** and applied
only after Apply Pending Operations… ❖↩ shows them in plain words:
Mount/Unmount, Unlock/Lock (LUKS), New Partition ❖N, Delete ❖⌫, Format…
⇧❖F, Resize… ❖R, Label… ❖L, Flags, Check, Repair, New Partition Table,
SMART ❖I, Power Off; Attach Disk Image… ❖O (read-only option), Detach.
Undo Last Operation ❖Z removes a queued one. Through UDisks2. **Never
apply disk operations for the person without naming the disk, what will
be lost, and getting a clear yes.**

## Kura — Disk Usage (`aizome-kura [PATH]`)
Sunburst of a folder or disk; Collector gathers items, Move Collected to
Trash… ❖⌫ asks first; refuses the Nix store (opens Yukimi's Storage
instead). Scan Home ❖N, Scan Folder ❖O, Disks ❖D, Rescan ❖R, Stop ❖.,
Collect ❖K, Copy Path ⇧❖C. Suggestions list caches, build outputs, logs,
old downloads, duplicates, each with risk (safe, low risk, your call).
- CLI (first argument): `aizome-kura scan [PATH] [--json]`, `report
  [PATH]`, `hints [PATH] [--json]`, `trash [--json] PATH…` (freedesktop
  Trash, never the store); options `--depth N`, `-x`/`--cross-filesystems`,
  `--no-duplicates`, `--no-nix`, `--output DIR`, `-q`.
- Index for agents: `~/.cache/aizome/kura/index.json` and `index.md`.

## Myaku — Activity (`aizome-myaku`)
Apps (with all their processes) by CPU ❖1, Memory ❖2, Energy ❖3, GPU ❖4,
Disk ❖5; Applications / All Processes / My Processes; Inspector ❖I;
Find ❖F. Select a row, then Process ▸ **Quit… ⌥❖Q** (SIGTERM; its dialog
also offers Force Quit) or **Force Quit… ⇧⌥❖Q** (SIGKILL), or
Pause/Resume ⌥❖P. Refuses kernel threads and other users' processes.
- `--pid PID` (select and inspect), `--lens cpu|memory|energy|gpu|disk`,
  `--search TEXT`, `--print` (one reading to stdout: CPU, memory, GPUs,
  power, top 25 apps by CPU). Stores nothing.

## Nisshi — Logs (`aizome-nisshi`)
The journal and `/var/log`, newest first. Places ❖1–❖6: All Messages,
Errors & Warnings, My Session, Kernel, Crashes, Logins & Security; then
Sources and Log Files. Search tokens: `-word`, `"phrase"`, `/regex/`,
`app:` `unit:` `pid:` `level:` (that or worse) `since:` `until:` (`10m`,
`2h`, `14:30`, `yesterday`), `FIELD=value`.
- Users outside `wheel`/`adm`/`systemd-journal` see only their own
  session: Journal ▸ **Unlock the System's Logs…** (password once; Lock
  again from the same menu). Copy ❖C, Copy Message Only ⌥❖C, Export
  Messages Shown… ⇧❖E (`.json` → JSON lines), Around This Time ⌥❖T, Live
  ❖L, Search the Whole Boot ⇧❖F, Clear Filters ❖⌫.
- CLI: `--search TEXT`, `--source NAME` (`--app`), `--unit NAME`,
  `--level err|warning|…`, `--scope all|problems|mine|kernel|crashes|security`,
  `--boot N` (0 this boot, -1 the one before), `--file /var/log/…`,
  `--cursor C`, `--launch ID`; `--print [--lines N] [--system]` prints
  messages to stdout. Settings `~/.config/aizome/nisshi.toml`.

## Shashin — Images (`aizome-shashin FILE…|FOLDER`)
←/→ (Space, Home, End) through the folder; zoom ❖+ ❖− ❖0 (100%) ❖9 (fit);
Filmstrip F; Slideshow ⇧❖F; Full Screen ⌃❖F; Rotate ❖L/❖R then Save ❖S
(JPEG: orientation only, no re-encode); Get Info ❖I; Copy ❖C; Show in
Hako ⌥❖R; Move to Trash ❖⌫ (❖Z undoes); **Set as Wallpaper ▸ Night / Day
/ Both** (writes `[appearance] wallpaper_*`; SVG not supported as
wallpaper; the file must stay where it is).

## Shiori — Documents/PDF (`aizome-shiori FILE…`)
One window per document; opens where you left off
(`~/.local/state/aizome/shiori/places.json`). Zoom ❖+ ❖− ❖0, Fit Width ❖8,
Fit Page ❖9; Find ❖F, ❖G / ⇧❖G; Go to Page… ⌥❖G; First/Last Page ❖↑/❖↓;
Sidebar ⌥❖S; Full Screen ⌃❖F; Get Info ❖I; Reload ❖R. Asks for a locked
PDF's password. Not yet: printing, filling forms, annotations.

## Utsushi — Media Player (`aizome-utsushi FILE…|FOLDER|URL [--no-sound]`)
Space or K play/pause; ←/→ 5 s (⇧ 1 s, ⌥ 1 min); J/L 10 s; ↑/↓ volume;
M mute; F full screen; [ ] slower/faster; C next subtitles, Z/X shift
them; ⇧N/⇧P next/previous item; S saves the frame to
`~/Pictures/Screenshots`; ❖C copies it; Media Info ❖I; Open ❖O. Films
over 5 min resume where left. State in `~/.local/state/aizome/utsushi/`.
MPRIS name `org.mpris.MediaPlayer2.utsushi` (media keys, Sound panel).

## Koe — Dictation (`aizome-koe`)
Speak and the words are typed into the focused app, recognised locally
(Whisper). Off until turned on: Koe's window → **Download a Model and Turn
On** (Base, 148 MB), or `aizome-koe enable` (needs a model:
`aizome-koe model download base`). Then **❖⌥D** starts/finishes and
**❖⌥Esc** cancels (added to `[keys.bindings]` only if free). Service
`aizome-koe.service` (user; runs only when enabled). Models
`~/.local/share/aizome/koe/models/ggml-<name>.bin`; settings
`~/.config/aizome/koe.toml`. CLI: `status`, `record toggle|start|stop|cancel|last|copy-last|paste-last`,
`model list|download|verify|remove`, `enable`, `disable`, `doctor`,
`packs …`, `words …`. With dictation off, ❖⌥D opens Koe.

## Hotaru — Terminal (`aizome-hotaru`; ❖Return)
One window; tabs and split panes. New Tab ❖T, Split Right ❖D, Split Down
⇧❖D, Close Pane ❖W, Close Tab ⌥❖W, Copy ❖C, Paste ❖V, Find ❖F, Clear ❖K,
Larger/Smaller/Actual Text ❖+ ❖− ❖0, Reset Terminal ⌥❖R, tabs ❖1–❖9,
Previous/Next Tab ⇧❖[ ⇧❖], Settings… ❖,, Reload Configuration ⇧❖,.
Ctrl+Shift equivalents also work (⌃⇧T, ⌃⇧C, ⌃⇧V, ⌃⇧F). A second launch
opens a new tab. Flags: `--working-directory DIR`, `-e CMD…` or `-- CMD…`,
`--config-file FILE`.
- Config `~/.config/aizome/hotaru.toml` (unknown keys are an error; an
  invalid file makes Hotaru refuse to start with "Hotaru configuration:
  …"): `font_size` (11.0), `font_families` (["IBM Plex Mono"]), `theme`
  (desktop|night|day|high-contrast), `cursor_style` (block|bar|underline),
  `scrollback` (10000), `shell`, `copy_on_select`, `clipboard_read`
  (ask), `[keybindings]`.
- Shell: `shell` key, else `/etc/xdg/aizome/hotaru-shell` (fish or bash,
  Sekka's choice), else fish, `$SHELL`, `/bin/sh`.
- Doesn't do synchronized output, Kitty graphics and a few DEC modes; foot
  is installed for programs that need them. Make foot the default:
  Settings → General → Default Apps → Terminal.

## Annai — Welcome (`aizome-annai`)
The seven-page tour (Help ▸ Aizome Tour). Shown once at first login
(`--first-run`; "Show at login" checkbox); state
`~/.local/state/aizome/annai/state.json`. "Make it yours" opens Settings'
Appearance, Displays, Network and Keyboard.

## Typical requests

| Request | Do |
|---|---|
| "Make a note" | Fusen ❖N and type; or `aizome-fusen --new "TEXT"` |
| "Where are my notes?" | `~/Notes` (plain Markdown) |
| "Create a folder" | Hako → go there → New Folder ⇧❖N, type the name, Return |
| "Undo a delete" | Hako → Trash → select → Put Back ❖⌫ |
| "What's slowing the computer?" | Myaku, sorted by CPU (❖1); `aizome-myaku --print` |
| "Something crashed / failed" | Nisshi → Errors & Warnings or Crashes; `aizome-nisshi --print --system --lines 50` |
| "Free up space" | Kura (home), Yukimi → Storage (Nix store) |
| "Set an alarm for 7:00" | Tokei → Alarms ❖2 → New Alarm ❖N |
| "Calculate …" | palette (❖Space, type the sum) or Soroban |
| "Open this PDF/picture/video" | double-click in Hako; or `aizome-shiori` / `aizome-shashin` / `aizome-utsushi FILE` |
