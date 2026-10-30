| Apps' status items | done | live watcher on the desktop's own session bus (Steam registered, its menu read); headless session on a private bus with an item carrying Steam's icon and menu: drawn, left click opens the menu, a row reaches the app; `statusnotifier` tests on a private bus | SVG icons are drawn at 16pt (resvg), in the Deskbar too; the wheel sends `Scroll` in whole steps (`statusnotifier` test hears it). No XEmbed: it embeds X11 windows, which a Wayland bar can't; KDE's `xembedsniproxy` would bridge it, at the cost of Plasma's workspace |
# Aizome against its design and brief

What the end-to-end walkthrough of 17–18 October 2026 found: every element
of the boards (`aizome-design/boards/aizome-{desktop,menus}-{night,day}@2x.png`,
`reference/png/C1-Switcher.png`, `C3-Palette.png`, `C4-Overview.png`,
`C4-Marking.png`) and of the brief, with its status, how it was checked and
what's left. The walkthrough ran the real session headless at 2880 × 1800
@ 2 from release builds (see `docs/screenshots/README.md`), drove it with
`aizomectl input` and looked at a screenshot after each step.

**done**: implemented and seen working in the live session.
**partial**: works, with a gap that's named. **missing**: not there.

"Live" below means the headless session; "tests" means the crate's own
tests, which were run after each fix.

## Windows and tabs (Haiku)

| Element | Status | How it was checked | Notes |
|---|---|---|---|
| Tab only as wide as its title, Archivo semi-condensed 650, close box and zoom box | done | live, `01`, `02` | Long titles keep the tab short; paths lose their middle (`/h/c/w/aizome`) |
| Focused tab amber with the 2pt rail along the frame | done | live | |
| Unfocused tabs `tab-idle` with an app glyph | done | live | **Fixed** (`de92f1b`): the glyph was the app icon's silhouette, a solid square for tiled icons; now Lucide line glyphs as on the boards |
| Square frame, 1pt bevel, hard 3pt shadow including the tab | done | live, `compositor` tests | |
| ⇧-drag slides a tab along the top | done | live (`01`: foot/Hako's tabs slid right) | |
| Drag a tab onto another: dashed amber slot, "Release to stack with …" | done | live, screenshot during the drag | |
| Stacked tabs side by side, one window showing; click a tab to switch | done | live (Tokei/Soroban, foot/Hako) | **Fixed** (`9a2bc77`): a fixed-size window (Soroban) stacked with a larger one left a hole onto the desktop; the frame now fills under the client |
| Unstack by dragging a tab out | done | live | |
| Double-click a tab, or the zoom box: fill | done | live | **Fixed** (`4b3fd67`): fill and split ran under the Deskbar, hiding windows' right edges |
| Close box | done | live (Settei) | |
| Resize by the edges | done | live (right and bottom edges, Settei) | |
| ⌃❖-drag moves, ⌃❖-right-drag resizes | done | live | |
| ❖H hide, ❖M minimise | done | live, Deskbar shows "hidden" | |

## The marking menu (C4-Marking)

| Element | Status | How | Notes |
|---|---|---|---|
| Right press and pause on a tab shows eight verbs | done | live, `11` | |
| Flick without pausing runs the verb | done | live (flick east: split right, and again to restore) | |
| "to Desk ▸" opens a ring of Desk names | done | live | |

## Desks and the overview

| Element | Status | How | Notes |
|---|---|---|---|
| ⌃❖←/→, ⌃❖1–4 | done | live | |
| Desks strip in the bar, current on `selection`, click to switch, `+` adds | done | live | Double-click rename not tried |
| ❖Return opens the terminal (Hotaru; foot before #142) | done | live; Hotaru in `vm-terminal-shortcuts` | In the Desk's folder: not tried live (the Desk had none then) |
| Overview (⌃❖↑): Desks with thumbnails, counts, "here", new Desk slot | done | live, `04` | |
| Drag a window to another Desk in the overview | done | live (Tokei to Write) | |
| Type to filter, 1–9 to go, Esc | done | live | |
| Park a Desk (p): its windows suspended, "parked 3 days · draws nothing", woken on arrival | done | `overlays` test (a client told it's suspended, then woken), `wm` tests | Added 20 October after comparing the installed build with C4-Overview |
| What each Desk is doing: here, "12 min ago", "● 3 new" (windows asking for attention), "audio playing" | done | `wm`, `overview` and logic tests; laptop's build (`scripts/validate-installed`) | "audio playing" from the shell's sound streams (`set-audible`), matched to windows through parent processes |
| ❖-scroll zooms, "zoom 28%" in the heading | done | laptop's build | |
| A dragged window's label: "Notes → Mail" | done | | |

## The switcher (C1-Switcher)

| Element | Status | How | Notes |
|---|---|---|---|
| ❖Tab: apps row with icons and window counts | done | live, `05` | |
| ↓ or keep holding: that app's windows with live thumbnails | done | live, `05` | |
| ❖`: next window of the app | done | live (three Sumi windows) | |

## The menu bar, menus and tear-offs

| Element | Status | How | Notes |
|---|---|---|---|
| Bar 24pt: Toolchest button with the mark, app name bold, its menus | done | live | |
| Menus from the bar, shortcuts in mono with ❖⇧⌥⌃ | done | live | |
| Type-ahead in a menu | done | live (Edit, "sel") | |
| Submenus, keyboard (↑↓→←, Return, Esc) | done | live | |
| F10 / ⌥❖M focuses the bar | done | live, `keys` tests | |
| ❖-right-click: the whole menu at the pointer (NeXT) | done | live, `02` | |
| Services ▸ (New Note with Selection, Open in Sumi, …) | done | live: New Note with Selection made a Fusen note from Sumi's selection | Only that service tried |
| Tear-off by the strip or its pin; palettes move by their strip | done | live, `02` | **Fixed** (`31cbcc9`): dragging a palette's strip moved it by the drag again on every move, sending it off screen, where the shell crashed (a negative layer size); now it follows the pointer and stays on its output |
| Palettes remembered, per app, pinned | done | live (`palettes.json`), `menus` tests | |
| The boards' pin glyph on title strips | done | live | **Fixed** (`a50dde6`): unpinned strips drew a struck-through pin |
| Fallback menus for apps that publish none (foot) | done | live | |

## The Toolchest

| Element | Status | How | Notes |
|---|---|---|---|
| Mouse, ❖⌥Space | done | live | |
| Applications ▸ by category, Places, Desks, Find, System, Session | done | live, `07` | **Fixed** (`a50dde6`): the rows' glyphs were dropped by the kit for submenus |
| Typing opens the palette with what was typed | done | live | **Fixed** (`f5951ea`): only the first key reached the palette; the rest went to the window under it |
| Tear-off | done | live, `02` | |

## The palette (C3-Palette, Strip's)

| Element | Status | How | Notes |
|---|---|---|---|
| ❖Space; query row with "in ‹App› · ‹title›" | done | live, `03` | |
| Commands of the focused app with paths and keycaps | done | live | |
| Shortcut coach after three runs | done | live, `03` | |
| Windows, Files, Apps, Settings, Actions scopes; Tab cycles | done | live (opened Shashin from Windows; "caps lock" opened Settei's Keyboard; "night" switched theme) | |
| Arithmetic: "= result" | done | live (`12*(3+4)/2` = 42) | |
| ❖K actions | done | live (a file's actions) | |
| Matched characters in blue, underlined; selected row with the amber rail | done | live | |

## The systray

| Element | Status | How | Notes |
|---|---|---|---|
| CPU and memory meters | done | live | |
| Network panel | done | live (this laptop's NetworkManager through a read-only D-Bus filter); VM (`checks.settings`: hostapd WPA2 join, airplane mode, wired) | Its access points match `GetAllAccessPoints` and `nmcli --rescan no` exactly; an empty list earlier was NM's empty cache, not a bug |
| Sound panel | done | live (private PipeWire: three null sinks, a test microphone, a test MPRIS player); VM (`wpctl`) | Device switching, volume, mute, the input meter, title and ⏯ ⏭ ⏮ following the player live. Real devices and Bluetooth audio wait for a real session |
| Bluetooth panel | done | live (this laptop's BlueZ, read only); VM (`btvirt`: power, rename, discovery while open) | This laptop knows no devices, so its list is rightly empty; pairing between two virtual controllers didn't work in the VM |
| Display panel | done | live | Night/Day/Auto and Size. Brightness hides on outputs without a built-in panel (**fixed**, `6fd37f5`); Night Light needs gamma, which headless lacks |
| Battery panel | done | live (real UPower) | Charge, state, power mode |
| Clock panel | done | live | Month with today in amber, Do Not Disturb; agenda empty |
| Escape closes the panels | done | live | **Fixed** (`130ca23`): after closing, keys went to the bar, not the window; (`441f0c8`): slivers of the highlight were left beside the glyphs |
| Clock, IBM Plex Mono tabular | done | live | |

## The Deskbar and the Shelf

| Element | Status | How | Notes |
|---|---|---|---|
| Apps with their windows; focused bar, "hidden", "stack", "desk N" | done | live, `01`, `05` | |
| Click a row focuses | done | live | |
| Notifications under their app: count in red, newest in a card, × dismisses | done | live (`notify-send` on the private bus), `01` | Clicking for the default action not tried |
| Notification line in the bar | done | live | |
| Fold to "N apps ⌄" and back | done | live | |
| Shelf: drag to park, "Drop to park" | done | live (Hako's files, a desktop icon, Sumi's selected text, `13`) | **Added** (`67a0431`): a press on the selection in any text editor or field that moves 4pt drags the text out (to copy); a press that doesn't move places the caret; passwords never drag. Dragging out from the Shelf is covered by `deskbar` tests, not tried live |
| App glyphs | done | live | **Fixed** (`588b55b`): Shashin and Annai had category fallbacks |

## The desktop

| Element | Status | How | Notes |
|---|---|---|---|
| Wallpaper per theme | done | live, `01`, `07` | |
| Icons: home, Trash, `~/Desktop` items, labels on chips | done | live | |
| Disk and image icons | done | live (this laptop's disk read from UDisks, read only, and from the mounts without it), `13`; `desktop`, `aizome-sys` and Tansu tests | **Added** (`bdf8ed1`, `10cf897`, `f0e3712`): the system's disk as `nvme0` (its drive's short name), opened in Hako at `/`; its menu offers Open, Show in Disks (Tansu on that drive) and Disk Usage (Kura), never Unmount or Eject, which the shell refuses for it anyway. Images show the disc and their file's name (covered by tests; attaching one wasn't tried, no loop devices outside the VM). `[desktop] show_disks` hides both |
| Double-click opens; drag moves (remembered) | done | live (opened a folder in Hako) | |
| Right-click: the menu at the pointer | done | live | |

## The rest of the shell

| Element | Status | How | Notes |
|---|---|---|---|
| OSD (volume, mute, brightness) | done | live (`aizomectl action volume-up`, `mute`, `brightness-up`) | **Fixed** (`6fd37f5`, `27b4755`): the brightness key in a headless session changed the laptop's own panel, stepping from 0 since a blanked panel reads 0 |
| Lock screen: wallpaper under a scrim, time, date, initial, name, password field, mark | done | live, `12` | PAM's `aizome` service doesn't exist outside the VM, so a password is refused ("That password didn't work") as expected; the shake wasn't looked at |
| ❖⇧3, ❖⇧4 screenshots | done | live (files in `~/Pictures/Screenshots`, a notification) | |
| Night, Day, High Contrast live | done | live, `07`; foot follows (SIGUSR1/2) | **Fixed** (`5ea3273`): the theme signal went to every foot of the user's, including the live desktop's |
| Effects tiers Full, Lean, Austere | done | live: switched in Settings, shadows go in Austere; Full examined over the busiest parts of both wallpapers (menus, a systray panel, the palette, the Deskbar, the Toolchest), `13` | **Fixed** (`992e3dc` in the Sidestep fork): see-through windows had buffers without alpha, so Full's frost was opaque on screen (only the test composites showed it) and menus' shadow corners were black. **Tuned** (`d488677`): faces 90% opaque in Night, 94% in Day, the grain at device pixels; text stays legible over the city lights (primary text 5:1 or better over a white light, `text3` 3:1). There's no blur, so text under a frosted face shows as a faint ghost |
| Shell restart keeps apps working | done | live (killed the shell) | **Fixed** (`4b8f943`): apps lost their keyboard focus when the shell came back |

## Settings (Settei)

| Element | Status | How | Notes |
|---|---|---|---|
| Sidebar with search, the user's card, grouped panes | done | live, `06` | |
| Panes | done | live: Network, Appearance, Displays, Performance, Keyboard, General opened; the rest by their tests (goldens of 13 panes) | Changes that touch the system (joins, users, printers) weren't tried |
| `--pane`, palette's Settings scope | done | live | |

## Apps

All fourteen opened from the session and were looked at:

| App | Status | Notes |
|---|---|---|
| Settei (Settings) | done | `06` |
| Yukimi (Software) | done | `08`, this machine's configuration |
| Hako (Files) | done | Miller columns, sidebar, drag out to the Shelf |
| Sumi (Text Editor) | done | Markdown styling, ❖N/❖W/❖Z. **Fixed** (`7744b6e`): File ▸ New's window didn't take the keys |
| Fusen (Notes) | done | Notes from `~/Notes`, Services' new note |
| Koyomi (Calendar) | done | Month view |
| Tokei (Clock) | done | `10` |
| Soroban (Calculator) | done | Typed expressions with the tape |
| Tenki (Weather) | done | `10`, fetched from Open-Meteo. **Fixed** (`0a6ee49`): the location question's words were squeezed to a letter a line in a narrow window |
| Tansu (Disks) | done | Opened on the real disks, read only; no operation tried |
| Kura (Disk Usage) | done | `09`, sunburst and suggestions; `index.json`/`index.md` written |
| Myaku (Activity) | done | |
| Shashin (Images) | done | |
| Annai (Welcome) | done | |

## The brief

| Item | Status | Notes |
|---|---|---|
| Haiku tabs and tab groups | done | above |
| Deskbar with app → windows, notifications and Shelf | done | |
| Global menu bar, NeXT menu at the pointer, tear-offs | done | |
| Toolchest, Desks, desktop icons | done | |
| Frames, type, themes (Night/Day) | done | High Contrast too |
| Strip's palette: scopes, shortcut coach, Toolchest typing, ❖Space, Services | done | |
| Two-tier switcher, Desks overview, marking menu | done | |
| Systray: sound, Bluetooth, network, display, battery, power | done | checked against this laptop's real devices, read only, and in VMs (above); a sharing indicator and a recording indicator join them while in use |
| Settings, all panes | done | `checks.settings`: a VM drives every pane that has state, checks the system took the change, then reopens Settings and checks the pane shows it (`nix/vm-settings-test.py`, `docs/apps.md`) |
| Apps (Yukimi, Calculator, Calendar, Clock, Disks, Disk Usage with its agent index, Notes, TextEdit, Weather, Files, Activity, Images, Welcome) | done | |
| Hotaru as the terminal (foot before #142), fish as the shell | done | foot with Aizome's `foot.ini`; fish from nixpkgs here (Sekka's module installs it) |
| GPU and software rendering | done | GLES for the walkthrough; the same scene under pixman (`AIZOME_RENDERER=pixman`) |
| The 4 ms budget | done, with exceptions | `docs/performance.md`: every interaction within it, except once-per-session first frames (the palette's first opening, the wallpaper) and the pixman overview's opening and closing frames, at about 4.5 ms |
| Recovery (Ctrl+Alt+Del), live reload, screen recording and sharing | done | asked for after the first login: `docs/recovery.md`, `docs/reload.md`, `docs/shell.md`; each has its VM test step |
| Notification cards' slide, true blur in Full | done | the slide under both renderers; blur under GLES only (pixman keeps the frost), `docs/rendering.md` |
| Super as the command key | done | ❖N, ❖W, ❖Z, ❖Space in the session; ❖ → Ctrl translation for foot and others in `keys` tests |

## Frame times

From `aizomectl stats` and `SIDESTEP_FRAME_STATS=1`, release builds, the
final screenshot run (GLES, 2880 × 1800 @ 2):

- **Compositor (GLES)**: 617 frames drawn, 271 redraws skipped for want
  of damage; CPU p50 0.31 ms, p95 0.87 ms, p99 1.94 ms, max 6.4 ms; GPU
  p50 0.68 ms, p95 1.92 ms. Over the longer walkthrough session (1,644
  frames): CPU p95 0.49 ms, p99 2.0 ms, max 30 ms (the first frames,
  uploading the wallpaper).
- **Compositor (pixman)**, the desktop scene with the overview, switcher,
  palette and a window drag: p50 0.18 ms, p95 4.8 ms, p99 8.4 ms, max
  16 ms. Full-output damage at 2880 × 1800 in software is over budget; it
  is the documented exception in `docs/performance.md`.
- **Clients** (the shell and the apps, Sidestep's five-second summaries):
  153 summaries, 143 with p95 within 4 ms. The ten over it were whole-window
  repaints (67–100% damage): windows' first frames, the switch to Day and
  back, Kura's first drawing after its scan, a page switch, 4.1–14.2 ms at
  p95; two had p50 over 4 ms (single-frame summaries of new windows).
- The shell's `perf` test (`window opens and closes`) failed once at p95
  4.5 ms with the machine loaded (load average 14, three builds running)
  and passed three times when quiet.

## On the laptop, from the installed build (20 October)

`scripts/validate-installed` runs a second, headless session from the
binaries installed in `/run/current-system` (Aizome `35f3015`, generation
33 on ganymede). It uses its own runtime directory, home and session bus,
and reads the system bus without changing it, so the menu bar shows the
laptop's own Wi-Fi, Bluetooth and battery. It drives that session through
`aizomectl` and photographs only its output:

```sh
dbus-run-session -- scripts/validate-installed /tmp/out gles
dbus-run-session -- scripts/validate-installed /tmp/out-pixman pixman
```

It checked:

- **Tab groups:** Hako and foot are stacked as one tab group (`stack 1`),
  and switching tabs works.
- **The palette:** over Sumi it lists Sumi's own menu commands with their
  paths and keycaps, scopes, apps and the shortcut coach.
- **The rest of the session:** the Desks overview, the switcher, desktop
  icons, the Deskbar and the Shelf.
- **Every app of the suite:** each opens a window (Soroban, Koyomi, Tokei,
  Tenki, Fusen, Kura, Tansu, Settei, Yukimi, Myaku, Shashin, Annai, with
  Hako and Sumi). Yukimi reads the laptop's own generations. Each is also
  photographed filled on its own (`app-*.png`). On the build pinned as
  Sekka `99bda5d` they showed:
  - Tansu: the laptop's NVMe drive and its partitions.
  - Myaku: the processes, with live graphs.
  - Tenki: Chicago's forecast.
  - Tokei: the world clocks.
  - Kura: the disks and the Nix store, ready to scan.
  - Koyomi: the month.
  - Fusen: a note.
  - Soroban: the keypad and its tape.
  - Shashin: its empty state.
  - Annai: the welcome.
  - Settei: Network.
  - Yukimi: the Overview.
- **Both renderers:** the same scene draws under GLES and under pixman.
- **The systray:** every widget the brief names (network, sound,
  Bluetooth, display, battery and power) next to the CPU and memory
  meters and the clock, as on the desktop board. Sound comes from a
  PipeWire of the session's own: a null sink, with WirePlumber's hardware
  monitors off.

Compositor frame times at 2880 × 1800, scale 2:

| Renderer | Frames | p50 | p95 | p99 | Max |
|---|---|---|---|---|---|
| GLES (CPU) | 223 | 0.51 ms | 0.90 ms | 2.5 ms | 4.1 ms |
| GLES (GPU) | 223 | 0.78 ms | 2.4 ms | 3.0 ms | 4.4 ms |
| pixman | 216 | 0.89 ms | 4.7 ms | 8.3 ms | 12 ms |

Pixman's tail is its documented exception (`docs/performance.md`:
whole-output frames at this size, as when twelve apps open at once).

The person's own session, checked read only:

- they log in through greetd and Aizome's greeter;
- foot runs fish;
- Keep Awake holds logind's block;
- the panels' sources read on the real devices (below).

## Fixes made during the walkthrough

`5ea3273`, `7744b6e`, `9a2bc77`, `4b3fd67`, `130ca23`, `45f1e85`,
`a50dde6`, `f5951ea`, `441f0c8`, `31cbcc9`, `27b4755`, `6fd37f5`,
`588b55b`, `0a6ee49`, `91ca828`, `4b8f943`, `de92f1b`. Most came with a
test that fails without the fix; the glyph, pin and highlight ones were
checked by eye.

Then three partials, finished: the system's disk on the desktop
(`bdf8ed1`, `10cf897`, `f0e3712`), dragging selected text out of the
kit's editor and fields (`67a0431`), and Full's frost (`992e3dc` in the
Sidestep fork, with its compositor test `a5e8ce4`, then `d488677`).

## Still to do

- *Done since*: frost with blur (`0f8319d`, GLES; pixman keeps the tint),
  and the Sidestep pin carries see-through windows' alpha (`992e3dc`,
  rebased as `9f56003`).
- The systray panels' sources on the laptop: *checked* 20 October with
  `cargo run -p aizome-sys --example probe`, which reads through Aizome's
  own clients and changes nothing. It found:
  - **Network:** one Wi-Fi network as the panel lists it, five access
    points across 2.4, 5 and 6 GHz, known and connected.
  - **Bluetooth:** BlueZ's adapter is powered, with nothing paired.
  - **Battery:** UPower's battery at 79%, with charge thresholds 75–80%
    and health 100%. The power profiles and the keyboard light read too.
  - **Night Light:** the internal display's CRTC takes 1024-entry gamma
    tables.
  - **Not covered:** sound and media need a device playing; the `media`
    and `mpris` tests cover them on a private bus.
- Whole-window repaints of the apps at 2×: *done since*. Sidestep
  repaints a whole window at 2× in about a millisecond (5–7 ms before;
  `docs/performance.md`, "The kit"). Once-per-session first frames stay
  the budget's documented exceptions there.
