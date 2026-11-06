---
name: aizome-settings
description: "Aizome's Settings (Settei, aizome-settei --pane ID) and ~/.config/aizome/settings.toml — every pane and option id, every section and key with defaults, how changes apply live, what an invalid file does (everything reverts; settings.toml.broken), and how to change a setting safely. Load to change or check theme, wallpaper, keyboard layout or modifier keys, lock/idle times, Deskbar, clock, notifications, displays, Night Light, mouse, trackpad or default apps, or when settings reverted or didn't stick."
---

# Aizome settings

All of Aizome reads one file, `~/.config/aizome/settings.toml`
(`$XDG_CONFIG_HOME/aizome/settings.toml`). Settings (Settei) writes it;
the compositor and the shell watch it (mtime and size, once a second) and
apply changes live. Nothing needs a logout except `[performance] renderer`
(next login).

## Changing a setting: in this order

1. **Settings app.** `aizome-settei --pane ID` opens a pane, or an option
   in one (`--pane keyboard.caps-lock`, also `keyboard/caps-lock`). The
   palette's Settings scope finds every option by name (❖Space, type
   "caps lock"). `aizome-settei --list-settings` prints every pane and
   option with its id (`--json` for machine use). Settei writes the file
   atomically, then asks the compositor to `reload-config`.
2. **Quick controls**: systray panels (Display: Night/Day/Auto, brightness;
   Sound; Network; Bluetooth; Battery: power mode, Keep Awake) and palette
   actions (Switch to Day/Night, Do Not Disturb, Keep Awake).
3. **Hand edit** (when the GUI can't, or to repair the file):
   - `cp -a ~/.config/aizome/settings.toml ~/.config/aizome/settings.toml.bak-$(date +%s)`
   - edit only the key concerned, as the user (never with sudo: a
     root-owned file or folder can't be written by Settings afterwards);
   - `aizomectl reload-config` applies it now and **validates** it: on a
     parse error it prints `aizomectl: …/settings.toml: TOML parse error at
     line N, column M …` and exits 1, and the compositor keeps what it had.
   - Python may not be installed on Sekka; don't rely on `python3 tomllib`.

Never delete settings.toml to "reset": it holds all of the person's other
settings. Fix the one bad line.

## Invalid file behaviour

- Missing file or missing keys: defaults. Unknown keys: kept on every save.
- **Parse failure** (TOML syntax error, or a wrong type or unknown value in
  a typed section: `[appearance]`, `[input.*]`, `[[outputs]]`, `[[desks]]`,
  `[keys]`, `[windows]`, `[idle]`, `[night_light]`) fails the *whole* file:
  - at login the compositor uses all defaults and logs
    `/home/USER/.config/aizome/settings.toml: <error>; using the defaults`;
  - on a live edit the compositor keeps its previous settings (logs
    `settings: …`), but the shell and apps re-read it and fall back to
    defaults (theme goes to Night, wallpapers to the system's, …);
  - see it with `journalctl --user -b | grep -i 'settings.toml'`.
- In the shell's sections (`[menubar]`, `[deskbar]`, `[notifications]`,
  `[lock]`, `[palette]`, `[desktop]`, `[power]`, `[sound]`, `[recording]`)
  a key of the wrong type is ignored alone (`settings: ignoring KEY = …`).
- **If Settings changes something while the file doesn't parse**, it copies
  the file to `settings.toml.broken`, says so in its window, and writes its
  own (default) settings plus the change. The person's other settings are
  then only in `settings.toml.broken`: repair that copy and move it back,
  rather than starting over. Better: fix the syntax before using Settings.
- Ownership: the file and `~/.config/aizome` must belong to the user
  (`ls -ld ~/.config/aizome ~/.config/aizome/settings.toml`).

## Panes (`aizome-settei --pane ID`)

Aliases also work: `wifi` `vpn` → network, `audio` `volume` → sound,
`display` `monitors` → displays, `power` → battery, `datetime` `timezone`
→ general.datetime, `about`, `updates`, `storage`, `language`,
`default-apps`, `login-items`, `startup`, `boot-splash` → general.*,
`deskbar` `desks` → desktop, `systray` → menubar, `lock-screen` → lock,
`accounts` → users, `theme` `wallpaper` → appearance, `a11y`,
`shortcuts` → keyboard.shortcuts, `touchpad` → trackpad, `pointer` → mouse,
`renderer` → performance, `palette` → search.

| Pane id | Title | Option ids (`--pane PANE.ID`) | Writes |
|---|---|---|---|
| `network` | Network | `wifi` `airplane` `networks` `join-other` `known` `saved` `wired` `vpn` `add-vpn` `hotspot`; under Details: `auto-join` `metered` `private-address` `ipv4` `dns` `ipv6` `proxy` | NetworkManager |
| `bluetooth` | Bluetooth | `bluetooth` `name` `devices` `nearby` | BlueZ |
| `notifications` | Notifications | `dnd` `schedule` `banner-time` `previews` `apps` | `[notifications]` |
| `sound` | Sound | `output-device` `output-volume` `balance` `port` `input-device` `input-volume` `input-level` `apps` `alert-sound` `alert-volume` `feedback` | PipeWire; alerts in `[sound]` |
| `general` | General | `about` (`computer-name` `hostname` `hardware`) `software-update` `storage` `startup` (`boot-splash`) `datetime` (`auto-time` `timezone` `clock-24h` `clock-date` `clock-seconds`) `language` (`system-language` `region`) `default-apps` (`default-browser` `default-mail` `default-terminal` `default-files` `default-text-editor` `default-images` `default-video` `default-music` `default-pdf` `default-calendar` `default-archives`) `login-items` (`startup-apps`) | timedated, localed, hostnamed, mimeapps.list, autostart |
| `appearance` | Appearance | `theme` `follow-sunset` `focus` `effects` `size` `reduced-motion` `wallpaper` | `[appearance]` |
| `accessibility` | Accessibility | `high-contrast` `larger-text` `cursor-size` `reduced-motion` `flash` `sticky-keys` `slow-keys` | `[appearance]`, `[sound]`, `[input.keyboard]` |
| `desktop` | Desktop & Deskbar | `desks` `collapsed` `auto-hide` `pinned` `shelf` `deskbar-side` `deskbar-width` `desktop-icons` `stack-new` `double-click` `focus-follows` `remember` `command-key` | `[[desks]]`, `[deskbar]`, `[desktop]`, `[windows]`, `[keys]` |
| `menubar` | Menu Bar | `clock-24h` `clock-seconds` `clock-date` `battery-percent` `widget-cpu` `widget-memory` `widget-network` `widget-sound` `widget-bluetooth` `widget-display` `widget-battery` | `[menubar]` |
| `displays` | Displays | `arrangement` `use-as` `resolution` `fractional` `refresh-rate` `rotation` `brightness` `night-light` | `[[outputs]]`, `[night_light]` |
| `battery` | Battery & Power | `charge` `history` `health` `mode` `screen-off` `lock-after` `power-key` `lid` `percent` `devices` | power-profiles-daemon, `[idle]`, `[power]` |
| `performance` | Performance | `renderer` `effects` `frames` `refresh` | `[performance]`, `[appearance] effects` |
| `search` | Search | `scopes` `folders` `exclude` `rebuild` `forget` | `[palette]` |
| `lock` | Lock Screen | `lock-after` `screen-off` `before-sleep` `after-screen-off` `show-clock` `show-picture` | `[idle]`, `[lock]` |
| `users` | Users & Groups | `name` `picture` `password` `groups` `others` `auto-login` | AccountsService |
| `keyboard` | Keyboard | `layouts` `switch-layout` `repeat-delay` `repeat-rate` `keyboards` `setup` `modifiers` `caps-lock` `compose` `numlock` `shortcuts` `commands` | `[input.keyboard]`, `[[input.keyboards]]`, `[keys.bindings]` |
| `mouse` | Mouse | `speed` `acceleration` `natural-scroll` `scroll-speed` `primary-button` | `[input.pointer]` |
| `trackpad` | Trackpad | `tap` `right-click` `speed` `acceleration` `natural-scroll` `scroll-speed` `disable-while-typing` | `[input.touchpad]` |
| `printers` | Printers & Scanners | `default-printer` `printers` `add-printer` | CUPS |

Notes on panes:
- **Displays**: risky changes (resolution, scale, rotation, mirroring, on/
  off) ask "Keep these display settings?" and go back after 15 s
  unanswered.
- **General → Date & Time**: the time zone goes through timedated (a wheel
  user at the computer isn't asked for a password). Automatic time is
  read-only when NixOS manages it (`services.timesyncd.enable`).
- **Language & Region**, **lid**, **host name**: pinned by the NixOS
  configuration (`i18n.defaultLocale`, logind's lid settings,
  `networking.hostName`); Settings shows them as managed by the system.
  Change those in `/etc/nixos` (see sekka-basics). The pretty
  "Computer name" can change.
- **Keyboard**: until changed here, layouts and options come from the
  system (`services.xserver.xkb.*`, via `XKB_DEFAULT_*`). Settings' layouts
  apply to the Aizome session only, not the login screen or console.
- **Default Apps**: written to `~/.config/mimeapps.list` and the terminal to
  `~/.config/xdg-terminals.list`; ❖Return starts that terminal.

## settings.toml reference (defaults)

```toml
[appearance]
theme = "night"            # "night" | "day" | "high-contrast" | "auto"
follow_sunset = false      # with "auto": switch at sunset/sunrise (else 07:00–19:00 is Day)
focus = "amber"            # "amber" | "cyan" | "rose" | "lime"
effects = "lean"           # "full" | "lean" | "austere"
size = "medium"            # "small" | "medium" | "large"
reduced_motion = false
cursor_theme = "Adwaita"
cursor_size = 24
# wallpaper_night = "/path.jpg"   # unset: the system's; "" = no picture
# wallpaper_day = "/path.jpg"     # PNG, JPEG, GIF, WebP, BMP, TIFF, ICO; not SVG

[input.keyboard]
layouts = ["us"]           # from the system until set; e.g. ["us", "de"]
variant = ""               # one per layout, comma-separated
options = ""               # XKB options, comma-separated (below)
model = ""
repeat_delay = 600         # ms
repeat_rate = 25           # per second
numlock = false
# sticky_keys = false, slow_keys = false, slow_keys_ms = 300

# Optional, repeatable: one keyboard's own modifier/Caps/Compose keys
# [[input.keyboards]]
# name = "Apple Inc. Magic Keyboard"
# id = "05ac:029c"
# options = "compose:rwin"

[input.pointer]
accel_profile = "adaptive" # "adaptive" | "flat"
speed = 0.0                # -1.0 … 1.0
natural_scroll = false
left_handed = false
# scroll_factor = 1.0      # 0.1 … 10

[input.touchpad]
tap = true
natural_scroll = true
disable_while_typing = true
click_method = "clickfinger"   # or "button-areas"
accel_profile = "adaptive"
speed = 0.0

# One [[outputs]] entry per display, matched by make/model/serial, or by
# connector name when only `name` is given (Settings writes these):
# [[outputs]]
# make = "BOE"
# model = "…"
# serial = "…"
# mode = "2880x1800@60"
# scale = 2.0
# position = [0, 0]
# transform = "normal"     # "90" "180" "270" "flipped" "flipped-90" …
# enabled = true
# mirror = "eDP-1"         # show that output's picture
# primary = true           # the Deskbar's display

# No [[desks]] = four Desks named "1" … "4". Each entry:
# [[desks]]
# name = "Mail"
# folder = "~/Mail"

[keys]
command_key_for_all_apps = true   # ❖ shortcuts become Ctrl ones in non-Aizome apps
[keys.bindings]            # chord = action; "none" unbinds; "spawn PROG ARGS"
# "Ctrl+Super+Up" = "overview"

[windows]
stack_new_windows_of_same_app = true
focus_follows = "click"    # or "mouse"
remember_geometry = true
# tab_double_click = "fill"   # "fill" | "minimise" | "nothing"

[idle]
lock_after_minutes = 10        # 0 = never
monitors_off_after_minutes = 15  # 0 = never

[night_light]
enabled = false
temperature = 4000         # K, 1900–6500
schedule = "sunset"        # "sunset" | "manual" | "always"
from = "22:00"
to = "07:00"
# latitude = 48.85         # both or neither; unset = found automatically
# longitude = 2.35

[menubar]
clock = "24h"              # or "12h"
clock_seconds = false
clock_date = false
widgets = ["cpu", "memory", "network", "sound", "bluetooth", "display", "battery"]
battery_percent = true

[deskbar]
collapsed = false
auto_hide = false
pinned = []                # app ids
order = []
show_shelf = true
side = "right"             # or "left"
width = 232.0              # 120–400; below 84 snaps to 48 (Icons Only)

[desktop]
show_home = true
show_disks = true
show_trash = true

[notifications]
do_not_disturb = false
# quiet_from = "22:00"     # daily quiet hours (both)
# quiet_until = "07:00"
banner_seconds = 5
muted_apps = []            # filed silently
blocked_apps = []          # dropped
silent_apps = []           # no sound
previews_when_locked = false

[lock]
before_sleep = true
show_picture = true
show_clock = true
after_monitors_off = true
pam_service = "aizome"     # don't change

[power]
# power_key = "ask"        # "ask" | "sleep" | "lock" | "off" | "nothing"; unset = logind decides

[sound]
visual_alert = false
alert_sound = "bell"
alert_volume = 0.7
volume_feedback = false

[palette]
roots = ["~"]
exclude = []
scopes = ["commands", "windows", "files", "apps", "settings", "actions"]
reindex_minutes = 10
per_section = 4

[recording]                # no Settings pane yet
format = "mp4"             # or "mkv"
framerate = 60
quality = "very-high"      # "medium" | "high" | "very-high" | "ultra"
cursor = true
desktop_audio = false
microphone = false
# folder = "~/Videos/Recordings"
recorder = "auto"          # "auto" | "gpu-screen-recorder" | "wf-recorder"

[performance]
# renderer = "auto"        # "auto" | "gpu" | "software"; applies at next login
```

XKB options Settings uses: modifier swaps `altwin:swap_alt_win`,
`altwin:swap_lalt_lwin` (left Alt ↔ Super, AltGr kept), `altwin:swap_ralt_rwin`,
`ctrl:swap_lwin_lctl`, `ctrl:swap_rwin_rctl`, `ctrl:swap_lalt_lctl`,
`ctrl:swap_ralt_rctl`; Caps Lock `ctrl:nocaps` (Control), `caps:escape`,
`compose:caps`; Compose `compose:ralt`, `compose:rctrl`, `compose:menu`,
`compose:rwin`. A `[[input.keyboards]]` entry's own options replace only
these key options for that keyboard; layouts, repeat and shortcuts are
every keyboard's.

Binding action names for `[keys.bindings]` are listed in aizome-desktop.

## Common requests

| Request | Do |
|---|---|
| Dark/light | Settings → Appearance → Theme, or the Display panel's Night/Day/Auto, or palette "Switch to Day" (`theme = "day"`) |
| Wallpaper | Appearance → Wallpaper; or Shashin → Edit ▸ Set as Wallpaper ▸ Night/Day/Both |
| Lock after N minutes | Lock Screen → Lock the screen after (1, 2, 5, 10, 15, 20, 30, 60 min, Never) = `[idle] lock_after_minutes` |
| Keyboard layout | Keyboard → Keyboard layouts (`[input.keyboard] layouts`); Y/Z swapped means a `de`-style layout is set |
| Super under the thumb | Keyboard → Modifier keys → Swap left Alt and Super (`aizome-settei --pane keyboard.modifiers`) |
| 12-hour clock | Menu Bar → 24-hour clock off (`[menubar] clock = "12h"`) |
| Quiet notifications | palette "Do Not Disturb", or Notifications → Quiet hours |
| Night Light | Displays → Night Light (or `aizomectl night-light on` for this session only) |
| Terminal for ❖Return | General → Default Apps → Terminal |
