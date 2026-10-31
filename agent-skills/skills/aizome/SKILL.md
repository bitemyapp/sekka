---
name: aizome
description: Aizome is Sekka's own Wayland desktop, with its compositor, shell, menu bar, Deskbar, Desks, command palette and apps such as Settei, Hako, Sumi and Myaku. Use this skill when the desktop in use is Aizome (XDG_CURRENT_DESKTOP=Aizome) and you need to change its settings or key bindings, set session environment variables, start or arrange windows, take a screenshot with the person's consent, read its logs, or recover a stuck window, shell or session. Use it for the installed desktop, not for developing Aizome's source.
---

# Aizome

Aizome is Sekka's desktop, written in Rust from the compositor up. One menu
bar shows the focused app's menus. Each window has a tab, and tabs stack
into groups. Desks keep work apart, and the Deskbar lists each app with its
windows. Super (❖) is the command key in every app: ❖C, ❖V, ❖Z. The
terminal is Hotaru (`aizome-hotaru`), running fish; foot is still installed.

You're in Aizome when `XDG_CURRENT_DESKTOP` is `Aizome`. The compositor's
socket is in `AIZOME_SOCKET`, which `aizomectl` uses. A shell started outside
the session (over SSH, say) can export
`AIZOME_SOCKET=$XDG_RUNTIME_DIR/aizome.wayland-1.sock`; check the actual
name with `ls "$XDG_RUNTIME_DIR"`.

For root, Aizome's shell is the polkit agent. pkexec opens its password
dialog over the desktop (`sekka` skill).

## Settings

Settei (Settings, `aizome-settei`) covers most things. When the person would
rather click, point them to the right pane. When asked to make a change,
edit the relevant setting and apply it. Shared desktop settings live in
`~/.config/aizome/settings.toml`. Settei writes that file, and the
compositor, shell and apps read it.

- Its sections are `appearance` (theme, follow sunset, focus color, effects,
  size, reduced motion, wallpaper), `input` (keyboard layouts and options,
  key repeat, pointer, touchpad), `outputs` (displays, by make, model and
  serial), `desks`, `keys`, `windows`, `idle`, `night_light`, `menubar`,
  `deskbar`, `notifications`, `lock`, `power`, `apps`, `sound`, `recording`
  and `location`. Each keyboard's own modifier keys, Caps Lock and Compose
  key live in `[[input.keyboards]]`; Settei's Keyboard pane sets them up.
  `aizomectl keyboards` lists the keyboards.
- Edit narrowly. Change or add only the keys you mean to; missing keys take
  their defaults, and unknown keys are kept. Write values the way the file
  already writes them. Don't edit while Settei is changing the same setting.
- Apply the edit: `aizomectl reload-config`. The compositor applies it and
  tells the shell and apps, and kit apps also notice the file changing.
- Apps keep their own files beside it: `sumi.toml`, `koyomi.toml`,
  `tenki.toml`, `koe.toml`, `hako.toml`.
- Displays: Settei's Displays pane writes `[[outputs]]`. For this session
  only, `aizomectl output NAME --scale 2`, `--mode 2560x1440@144`,
  `--position X,Y`, `--off` or `--on`, and `aizomectl outputs` lists them.
- Everything in `/etc/xdg/aizome` comes from the system configuration and
  is read-only.

### Key bindings

Overrides go in `[keys.bindings]`, one chord to one action:

```toml
[keys.bindings]
"Ctrl+Super+Up" = "overview"
"Super+Alt+T" = "spawn foot --working-directory /home/me/src"
"Super+H" = "none"
```

- A chord is modifiers and a key joined by `+`: `Super`, `Ctrl`, `Alt`,
  `Shift`, then a key name such as `Return`, `Space`, `Up`, `Print`, `T`.
- An action is a built-in action name, `spawn PROGRAM ARGS`, or `none` to
  unbind.
- Built-in actions handled by the compositor: `switcher`,
  `next-window-of-app`, `hide-app`, `hide-others`, `minimise`,
  `go-to-desk-N`, `take-to-desk-N`, `previous-desk`, `next-desk`,
  `overview`, `fill`, `split-left`, `split-right`, `window-menu`,
  `stack-with-behind`, `unstack`, `previous-tab`, `next-tab`,
  `move-tab-left`, `move-tab-right`, `fullscreen`, `terminal`,
  `screenshot-screen`, `screenshot-area` and `reload`.
- Actions for the shell: `palette`, `toolchest`, `menu-bar`, `lock`,
  `screenshot-tool`, `record`, `record-screen`, `record-area` and
  `stop-recording`.
- `aizomectl action NAME` runs an action now, as if its keys were pressed.
- Ctrl+Alt+Del always opens recovery and can't be rebound.
- `[keys] terminal` is the program ❖Return starts: `aizome-hotaru` unless
  the person chose another in Settei (General › Default Apps › Terminal).
- `command_key_for_all_apps` turns ❖ shortcuts into the Ctrl ones that
  apps outside Aizome expect, and into Ctrl+Shift for the app ids listed in
  `[keys] terminals`.

### Session environment

To set variables for everything in the session, put `KEY=VALUE` lines in
`~/.config/aizome/environment`, one per line, without `export` or quotes.
They apply from the next sign-in, after the system's
`/etc/xdg/aizome/environment`. The session type, `XDG_SESSION_DESKTOP`
and the launcher's own bookkeeping variables can't be changed there.

## Driving the session: aizomectl

`aizomectl help` lists every command, and `--json` prints raw replies.

- State: `aizomectl windows`, `focused-window`, `desks`, `outputs`, `version`.
- Windows (ID from `windows`; the focused window by default): `focus ID`,
  `close ID`, `minimise`, `fill`, `split left|right`, `fullscreen`,
  `move-to-desk DESK ID`, `stack A B`.
- Start graphical programs with
  `aizomectl spawn [--cwd DIR] -- PROGRAM ARGS…`. They get the session's
  environment and keep running after your shell exits. Starting one as a
  child of your own shell ties it to your shell instead.
- Desks: `set-desk DESK`, `add-desk NAME [--folder DIR]`, `rename-desk`.
- Screenshots: `aizomectl screenshot PATH` takes every display, and
  `--output NAME`, `--window ID` or `--area X,Y,W,H` take part of it. The
  screen shows the person's private work. Take one only when they have
  asked, or have agreed to it for the task at hand, and say where you
  saved it.
- `aizomectl events` prints what happens (focus, windows, displays) until
  it is stopped.

Aizome's own apps are `aizome-NAME`: `aizome-settei` (Settings),
`aizome-yukimi` (Software), `aizome-hako` (Files), `aizome-sumi` (Text
Editor), `aizome-fusen` (Notes), `aizome-koyomi` (Calendar), `aizome-tokei`
(Clock), `aizome-soroban` (Calculator), `aizome-tenki` (Weather),
`aizome-tansu` (Disks), `aizome-kura` (Disk Usage), `aizome-myaku`
(Activity), `aizome-nisshi` (Logs), `aizome-shashin` (Images), `aizome-shiori`
(Documents), `aizome-utsushi` (Media Player), `aizome-hotaru` (Terminal) and
`aizome-koe` (Dictation).

Apps that open and save files through the desktop portal get Aizome's file
chooser. Its backend is `aizome-portal.service`.

## Logs

Nisshi (Logs) shows the person the system's journal and `/var/log`, with
filters, when they would rather look themselves. From a shell:

- The compositor and shell: `journalctl --user -b -u aizome.service`.
- Other session services: `aizome-portal`, `aizome-network-agent`,
  `aizome-keep-awake`, `aizome-koyomi-agent` and `aizome-ibus`, each with
  `-u NAME.service`. `systemctl --user list-units 'aizome*'` lists them.
- Frames that ran long are logged in `~/.local/state/aizome/stall.log`.
- `aizomectl stats` shows frame times per display.

## When something is stuck

Prefer the gentlest step that works, and tell the person what you did.

1. One app: `aizomectl recovery status` lists apps, their CPU and memory,
   and which are not responding. `aizomectl recovery close APP_ID` asks its
   windows to close, and `aizomectl recovery force-quit APP_ID` kills it.
2. The shell (menu bar, Deskbar, notifications):
   `aizomectl recovery restart-shell`.
3. Aizome itself, after a system update or when it misbehaves:
   `aizomectl reload` (also ❖⌃R) reloads the compositor's logic, the shell
   and the settings without touching apps. `aizomectl reload --check`
   says what it would do.
4. The person can always press Ctrl+Alt+Del for the recovery overlay. It
   offers the same actions, plus lock, sign out and a text console.

Never `systemctl --user restart aizome.service` or `stop` it, `kill` the
compositor, or run `aizomectl quit`, unless the person asks you to. Each of
those ends the session and every app in it, losing unsaved work.

Some system updates change Aizome itself. A notification then offers to
reload it, or a sign-out when the change needs one; `sekka-update --status`
says which.
