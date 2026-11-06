---
name: tatami
description: Tatami is Sekka's keyboard-driven Hyprland desktop, inspired by Omarchy, with Waybar, Walker, Mako, hyprlock and its own tatami command. Use this skill when the desktop in use is Tatami (XDG_CURRENT_DESKTOP=Hyprland:Tatami). It covers changing Hyprland settings, key bindings or window rules in Tatami's Lua configuration, the bar, notifications, wallpaper, displays and Keep Awake, reloading the session, reading its logs, and fixing a stuck bar, launcher or authentication dialog. Use it for the installed desktop, not for developing Tatami's source.
---

# Tatami

Tatami is Hyprland set up after Omarchy 4: Tokyo Night throughout, Waybar
on top, Walker as the launcher and menus, Mako notifications, SwayOSD,
hyprlock and hypridle, and hyprpolkitagent for password dialogs. One Rust
program, `tatami`, does what Omarchy's scripts do. Super+K lists the key
bindings, Super+Space opens the Tatami menu, and Super+Alt+Space the
applications. Applications started from the launcher, menu or key bindings
each run as a systemd user service of their own.

You're in Tatami when `XDG_CURRENT_DESKTOP` is `Hyprland:Tatami`. `hyprctl`
talks to the running Hyprland: `hyprctl clients`, `hyprctl monitors`,
`hyprctl binds`, `hyprctl dispatch …`. Anything set with `hyprctl keyword`
lasts only until the next reload.

For root, `tatami-polkit` (hyprpolkitagent) shows the password dialog,
centred with the screen dimmed (`sekka` skill).

## Configuration

Tatami's configuration comes from the system and is read-only:
`/etc/xdg/tatami`. Its Hyprland configuration is
`/etc/xdg/tatami/hypr/hyprland.lua`. It is written in **Hyprland's Lua
configuration**, as `hl.*` calls, not in the older `hyprland.conf`
syntax. Other desktops on the same computer don't use any of it.

### Your Hyprland settings: ~/.config/tatami/hyprland.lua

Hyprland settings, key bindings and window rules of the person's own go in
`~/.config/tatami/hyprland.lua`. Setup › Hyprland in the menu
(`tatami edit-config`) creates it and opens it in an editor. To create it
yourself, start it the same way:

```lua
-- Your Tatami Hyprland configuration, used from the next login.
-- Tatami's defaults load first, so their updates keep reaching you;
-- settings below override them. See https://wiki.hypr.land/Configuring/Start/
dofile("/etc/xdg/tatami/hypr/hyprland.lua")
```

Then add calls after the `dofile`, written like Tatami's own; read
`/etc/xdg/tatami/hypr/hyprland.lua` for examples:

```lua
hl.config({ general = { gaps_out = 20 } })
hl.bind("SUPER + SHIFT + G", hl.dsp.exec_cmd("tatami launch gimp"), { description = "GIMP" })
hl.window_rule({ match = { class = "^org.gnome.Calculator$" }, float = true })
```

- Keep the `dofile` line first. Without it, none of Tatami's defaults load.
- When the session starts, it uses this file if it exists. A file created
  during a session applies from the next login. After that, `tatami reload`
  (System › Reload, or Super+R) re-reads it.
- Hyprland runs every binding of a chord. Before giving one of Tatami's
  chords a new action, check `hyprctl binds` and Hyprland's Lua
  documentation for removing the old binding.
- Start programs from bindings with `tatami launch PROGRAM` (or
  `tatami terminal`, `tatami browser`, `tatami editor`), so they run as
  services of their own, like the rest.
- Leave monitor rules alone. The Displays window keeps them in
  `~/.local/state/tatami/displays.lua`.
- If Hyprland reports an error after a reload, show the person and fix the
  file. A broken file can leave the next login without key bindings.

### Environment variables

- `hl.env(…)` in `hyprland.lua` reaches only what Hyprland starts itself
  (key bindings), not applications started from the launcher.
- Variables every application needs belong in the system configuration,
  as `environment.sessionVariables` in `/etc/nixos/configuration.nix`
  (`sekka` skill). They take effect from the next login, in every desktop.

### Bar, notifications, launcher, windows

- Waybar, Mako and Walker run as user services (`tatami-waybar`,
  `tatami-mako`, `tatami-walker`) with Tatami's configuration given
  explicitly. Files in `~/.config/waybar` or `~/.config/mako` don't change
  Tatami's. To use one of the person's own, override that unit's
  `ExecStart` in a drop-in under `~/.config/systemd/user/UNIT.service.d/`.
  Then run `systemctl --user daemon-reload` and `tatami reload`.
- Tatami's Displays, Network, Bluetooth, Sound and Activity windows take
  `tatami-NAME/style.css` or `style-dark.css` from `~/.config` before
  Tatami's own.
- foot reads `~/.config/foot/foot.ini` before Tatami's, but that file
  applies in every desktop.

## The tatami command

`tatami` with no arguments lists everything. The useful parts:

| Command | Does |
| --- | --- |
| `tatami reload` | Applies a rebuilt Tatami and your `hyprland.lua` to the running session: Hyprland's configuration, remembered toggles, and a restart of the bar, launcher and the other session units |
| `tatami awake on\|off\|status` | Keep Awake: no sleep, screen lock or lid-close sleep until turned off |
| `tatami background set IMAGE [--output NAME]`, `mode fill\|fit\|stretch\|center\|tile\|solid_color`, `color '#1a1b26'`, `status`, `reset` | Wallpaper, for every display or one |
| `tatami display` | Opens the Displays window (arrangement, resolution, scale, rotation, Night Shift) |
| `tatami nightlight on\|off\|status\|KELVIN` | Night Shift |
| `tatami dns dhcp\|cloudflare\|google\|custom` | DNS for the connection in use |
| `tatami network`, `bluetooth`, `audio` | Those settings windows |
| `tatami screenshot region\|windows\|fullscreen`, `screenrecord` | Capture; screenshots go to Pictures |
| `tatami toggle idle\|nightlight\|notifications\|bar\|gaps` | The same toggles as the menu |
| `tatami launch PROGRAM [ARGS]` | Starts a program as its own service |
| `tatami lock`, `logout`, `reboot`, `shutdown`, `suspend` | Session and power; ask first, as these close windows |

Take screenshots only when the person has asked, or has agreed to it for
the task at hand: the screen shows their private work.

## Logs

- Session units: `systemctl --user list-units 'tatami-*'`, and
  `journalctl --user -b -u tatami-waybar` (or `tatami-mako`, `tatami-walker`,
  `tatami-elephant`, `tatami-polkit`, `tatami-hypridle`, `tatami-swayosd`,
  `tatami-swaybg`).
- Hyprland: `hyprctl systeminfo`; the log is in
  `$XDG_RUNTIME_DIR/hypr/$HYPRLAND_INSTANCE_SIGNATURE/hyprland.log`.

## When something is stuck

- The bar, launcher or notifications are gone or stale: `tatami reload`
  restarts them. To restart just one, use
  `systemctl --user restart tatami-waybar`.
- No password dialog appears for pkexec: run
  `systemctl --user status tatami-polkit`, then
  `systemctl --user restart tatami-polkit`.
- A window is off screen after a display change: Tatami brings windows back
  by itself. Super+Ctrl+D always brings the Displays window to the current
  screen.
- Never stop `wayland-wm@*.service`, kill Hyprland, or run
  `hyprctl dispatch exit`, unless the person asks you to. Each of those
  ends the session and every application in it.
- Changes to the session's environment, and any change of the system's
  Tatami, need `tatami reload` or a new login. `sekka-update --status`
  says which.
