---
name: aizome-computer-use
description: "Operating Aizome for the person — aizomectl (windows, focus, spawn, screenshots, synthetic pointer and key input, binding actions), aizome-shell msg (palette with a query, panels), screen layout in points, AIZOME_KIT_INSPECT element files, typing limits (US key positions, ASCII only, clipboard workaround), verifying each step, and the rules (never type passwords, confirm destructive actions, never end the session). Load before clicking, typing or opening anything on their screen."
---

> In Tasuke, `sudo CMD` in this guide means CMD as root: call `run_as_root` with CMD, without the word `sudo` (Tasuke asks the person, then they type their password). When telling the person how to do it themselves in a terminal, keep the `sudo`.

# Operating Aizome for the person

Prefer, in order: a direct command (`timedatectl`, an app's CLI flag), an
IPC request (`aizomectl …`, `aizome-shell msg …`), then synthetic input
(click, type, keys). After every action, check that it did what you meant.

## Reaching the session

- Inside the person's session (their terminal, or a process the session
  started) `AIZOME_SOCKET` and `WAYLAND_DISPLAY` are set: run `aizomectl`
  directly, as the user.
- From a root shell: run as the user with the session's environment:
  `sudo -u USER env XDG_RUNTIME_DIR=/run/user/UID aizomectl --socket /run/user/UID/aizome.wayland-N.sock COMMAND`
  (find the socket: `ls /run/user/UID/aizome.*.sock`). `aizome-shell msg`
  needs `XDG_RUNTIME_DIR` and `WAYLAND_DISPLAY=wayland-N`, or
  `AIZOME_SHELL_SOCKET=/run/user/UID/aizome-shell.wayland-N.sock`.
- `aizomectl help` lists every command; `--json` prints raw responses.
  "can't reach the compositor" means wrong user/socket, or no session.

## Looking

| Command | Gives |
|---|---|
| `aizomectl --json windows` | every window: `id`, `app_id`, `title`, `pid`, `desk`, `focused`, `minimised`, `hidden`, `group`, `geometry {x,y,w,h}` (the content, in points; tab and frame sit outside) |
| `aizomectl --json focused-window` | the focused one |
| `aizomectl --json outputs` | displays: `name`, `scale`, `logical {x,y,w,h}` (points), `usable`, `primary` |
| `aizomectl --json desks` | Desks, the current one, which display shows each |
| `aizomectl input-state` | grabs, held keys/buttons, keyboard focus |
| `aizome-shell msg state` | the shell's state as JSON (open menus, palette, sharing…) |
| `aizomectl screenshot "$XDG_RUNTIME_DIR/shot.png"` | PNG of every display; `--output NAME`, `--window ID`, `--area X,Y,W,H` (points) |

- Give screenshots a path of your own (default is the person's
  `~/Pictures/Screenshots`); add `--clipboard` only if asked. Delete your
  temporary shots afterwards.
- A screenshot is in device pixels at the largest display scale:
  **points = pixels ÷ scale** (plus the layout's top-left when several
  displays are side by side). Pointer input takes points.

### Where things are (points, main display)

- Menu bar: top 24 pt of each display. Toolchest button at its far left;
  the focused app's name and menus after it; Desks strip right of centre;
  systray and clock at the right.
- Deskbar: under the menu bar (y from 24) at the right edge, 232 pt wide
  by default (`[deskbar] side`/`width` may differ), as tall as its content.
- Palette: 720 pt wide, up to 480 pt tall, centred horizontally on the
  focused display and vertically in the area below the menu bar.
- Desktop icons down the left edge.

### Element files (only if enabled)

If `AIZOME_KIT_INSPECT` names a directory in an Aizome app's environment
(off by default; test and evaluation machines set it), every Aizome-kit
window writes `DIR/<app_id>.<pid>.<window>.json` after it paints: `title`,
`kind` (`window`, `dialog`, `utility`, `surface`, `menu`, `popover`),
`elements` with `name`, `text`, `role`, `label`, `value`, `checked`,
`selected`, `focused`, `disabled`, `visible`, `rect`/`shown` `[x,y,w,h]` in
the **window's** points. Screen point = window `geometry.x/y` + element
centre. The shell's file (`org.aizome.Shell`, kind `surface`) has titles
`Menu bar` and `Desktop` (origin 0,0), `Deskbar` (top right, y 24) and
`Palette` (centred, while it has the keyboard). Menu and popover items
aren't listed: type to pick, or use keys.
To use it for one app: quit that app first (a second launch only hands
over to the running one), `mkdir -p "$XDG_RUNTIME_DIR/inspect"`, then
`aizomectl spawn -- env AIZOME_KIT_INSPECT="$XDG_RUNTIME_DIR/inspect" aizome-soroban`.
Non-Aizome apps (browsers, Electron, Qt) write nothing: use screenshots.

## Doing, without input

| Goal | Command |
|---|---|
| Open an app | `aizomectl spawn -- aizome-settei --pane appearance` (any argv; runs in the Desk's folder with the session's environment) |
| Open a Settings option | `aizomectl spawn -- aizome-settei --pane keyboard.modifiers` |
| Palette with a query | `aizome-shell msg '{"request":"palette","query":"Calculator"}'` (scope: `"scope":"apps"`, `commands`, `windows`, `files`, `settings`), then `aizomectl input key return`. With a query or scope it opens; a bare `aizome-shell msg palette` toggles (closes an open palette) |
| Run a key binding by name | `aizomectl action overview` (`palette`, `toolchest`, `go-to-desk-2`, `screenshot-screen`, `fill`, `terminal`, … see aizome-desktop) |
| Focus / close a window | `aizomectl focus ID` / `aizomectl close ID` (asks politely; the app may ask to save) |
| Fill, split, minimise, full screen | `aizomectl fill ID on`, `split left ID`, `minimise ID`, `fullscreen ID on` |
| Move / resize | `aizomectl move ID X Y`, `aizomectl resize ID X Y W H` (content rect, points) |
| Desks | `aizomectl set-desk 2`, `move-to-desk 2 ID [--follow]`, `add-desk NAME`, `rename-desk 2 NAME` |
| Systray panel / menu | `aizome-shell msg '{"request":"open","what":"sound"}'` (`network`, `sound`, `bluetooth`, `display`, `battery`, `clock`, or a menu title); `aizome-shell msg dismiss` closes |
| Apply edited settings | `aizomectl reload-config` (also validates settings.toml) |
| Stuck pointer/keys | `aizomectl recovery reset-input` |
| Menu bar/Deskbar gone | `aizomectl recovery restart-shell` (apps keep running) |

## Synthetic input

```
aizomectl input motion X Y        # pointer to a point (global layout, points)
aizomectl input click [left|right|middle]
aizomectl input button left down|up      # for drags: motion, down, motions, up
aizomectl input scroll DX DY
aizomectl input key NAME [down|up]       # tap, or one half
aizomectl input chord super+shift+3      # press in order, release in reverse
aizomectl input type 'plain ASCII text'
```

- Key names: letters, digits, `return` (`enter`), `escape` (`esc`), `tab`,
  `space`, `backspace`, `delete`, `up` `down` `left` `right`, `home`,
  `end`, `pageup`, `pagedown`, `f1`…`f12`, `print`, `super` (`cmd`),
  `shift`, `ctrl`, `alt`, `rightshift`, punctuation (`,` `.` `/` `;` `'`
  `[` `]` `\` `-` `=` `grave`), `volumeup`, `volumedown`, `mute`. There's
  no right Alt, Caps Lock or Insert.
- Keys are **key positions**, read through the active keymap:
  - `input type` taps US-layout positions (Shift for capitals and `!@#…`).
    With another layout (`[input.keyboard] layouts`, e.g. `de`) letters
    and symbols come out as that layout has them (`y`↔`z`).
  - With a modifier swap in the XKB options (`altwin:swap_lalt_lwin`,
    `altwin:swap_alt_win`, `ctrl:swap_*`; check `[input.keyboard] options`
    and `aizomectl keyboards`), `super` and `alt` (or `ctrl`) trade
    places. Prefer `aizomectl action …` over modifier chords, and verify.
  - Non-ASCII text (é, ü, ß, CJK, emoji) can't be typed: `input type`
    stops at the first such character with "unknown key". Paste instead:
    `printf %s 'Grüße' | wl-copy`, then ❖V (`aizomectl input chord super+v`;
    in non-Aizome apps ❖V becomes Ctrl+V). Restore the person's clipboard
    content if you replaced something they had.
  - One line at a time: `input type` per line, `input key return` between.
- In Aizome apps ❖ is the command key (❖S save, ❖W close, ❖Q quit). In
  other apps ❖ shortcuts are translated to Ctrl (Ctrl+Shift in terminals).
- Click targets are 24 pt or more; aim at the centre. After clicking a
  text field, type; after typing in the palette, `return`.

## Patterns

- **Open an app**: `aizomectl spawn -- aizome-NAME`, or palette query +
  Return. Then wait for `aizomectl --json windows` to list it (poll for a
  few seconds; a first launch can take a moment).
- **Change a setting**: open its Settings option (`--pane PANE.OPTION`),
  click the control, then verify in `~/.config/aizome/settings.toml` or by
  reopening the pane. A direct, verified file edit is fine when the person
  agrees (aizome-settings).
- **Menu command of the focused app**: palette Commands scope with its
  name (`{"request":"palette","scope":"commands","query":"Save As"}`) +
  Return, or ❖-right-click for the whole menu at the pointer.
- **Fill a form**: click the field, `input type`, `tab` to the next.
- **Verify**: re-list windows, take a screenshot, read the app's files
  (`~/Notes`, settings.toml), or query the system (`timedatectl`,
  `wpctl status`). Report what you saw, not what you expected.

## Rules

- **Never type a password, passphrase or one-time code**, and never read
  one from the screen into your answer. When a password dialog appears
  (polkit "Authenticate", the lock screen, a Wi-Fi password sheet, a
  browser login), stop and ask the person to type it.
- **Confirm before anything destructive or hard to undo**: deleting or
  trashing files, Empty Trash, Delete Immediately, formatting or
  partitioning (Tansu), removing software or accounts, forgetting a
  network, quitting an app with unsaved work, sending or posting anything,
  purchases. Name exactly what will happen.
- **Never end or disturb the session**: no `aizomectl quit`, no Log Out /
  Restart / Shut Down (palette, Toolchest, recovery overlay O or T), no
  `recovery log-out` or `text-console`, no `debug-stall`, no
  `input lid closed` (can suspend the machine), no `output NAME --off` or
  `power-off-monitors` (blank screens), no killing `aizome`,
  `aizome-shell`, `xwayland-satellite` or `aizome-session`. Settings apply
  by themselves; no restart is needed to "apply" anything in Aizome.
- Don't lock the screen unless asked (you can't unlock it). `lock` and
  the recovery overlay's L lock it.
- Stay in the task: don't close, move or rearrange the person's other
  windows, Desks or files; put back what you moved. Don't act on things
  that look odd but weren't asked about: mention them.
- If input seems to go nowhere, check `aizomectl input-state` (a grab, a
  modal dialog, the lock screen) before trying again; don't repeat the
  same click more than twice without a new look.
