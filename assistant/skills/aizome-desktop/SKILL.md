---
name: aizome-desktop
description: "Using the Aizome desktop — menu bar, Toolchest, command palette (❖Space), Deskbar and Shelf, Desks, window tabs and groups, the ❖ command key, all default key bindings and action names, screenshots and screen recording, notifications, lock, sleep and log out, Keep Awake, the Ctrl+Alt+Del recovery overlay, the file chooser. Load for \"how do I … in Aizome\" (open an app, switch desks, take a screenshot, find a shortcut) or a stuck window, pointer, menu bar or lock screen."
---

# Using Aizome

❖ is the Super (logo) key, Aizome's command key, as ⌘ on a Mac. ⌥ Alt,
⇧ Shift, ⌃ Control. If the person swapped Alt and Super, ❖ is the key
beside the space bar.

## The screen

- **Menu bar** (top, 24 pt), left to right: **Toolchest** button; the
  focused app's name (its app menu) and its menus; the **Desks strip**
  (`1 2 3 4 +`); a red dot and time while recording; the **systray**
  (CPU and memory meters, apps' tray icons, network, sound, Bluetooth,
  display, battery; each opens a panel whose last row opens its Settings
  pane); the **clock** (opens the month, events, world clocks).
- **Deskbar** (top right under the bar by default; can stand at the left):
  each running app with its windows indented beneath (focused one marked,
  "hidden", "stack", "desk N"), unread notifications filed under their app,
  and the **Shelf** (park a dragged file/text/image halfway; drag it out
  later). Click an app: its last window; right-click: New Window, Hide,
  Quit, Keep in Deskbar, Move Up/Down. Middle-click a window row closes it.
  The bar's Hide Deskbar control folds it into an `N apps ⌄` control.
  Drag its top grip to the other side; drag its inner edge to resize;
  right-click the grip for At the Left/Right and Icons Only, Slim,
  Standard, Wide, Wider.
- **Desktop**: icons down the left: Home, disks (click mounts; right-click
  Eject/Unmount/Open in Tansu), Trash, then `~/Desktop`'s items.

## Opening apps and things

- **Palette**: ❖Space, type part of a name, Return. Also finds windows,
  files (home folder index), Settings options, menu commands of the
  focused app, system actions, and arithmetic (`= result`; Return copies
  it). Tab / ⇧Tab cycle scopes (All, Commands, Windows, Files, Apps,
  Settings); ↑↓ move; ❖K shows a row's actions (Open With, Show in Hako,
  Copy Path, Move to Trash, New Window, Quit, Move to Desk); ❖Return runs
  and keeps it open; Esc or ❖Space closes. Purpose words work for default
  apps: "term", "files", "editor", "browser".
- **Toolchest** (the button, or ❖⌥Space; typing turns it into the palette):
  Applications ▸ (by category), Places ▸ (Home, Desktop, Documents,
  Downloads, Pictures, Music, Videos, the Desk's folder, disks, Trash,
  Recent ▸), Desks ▸, Find ▸ (palette scopes), System ▸ (Settei, Yukimi,
  Myaku, Tansu, Kura, Application Launches…, Terminal ❖Return), Session ▸
  (Lock ⌃❖Q, Sleep, Restart…, Shut Down…, Log Out…).
- **Terminal**: ❖Return opens the default terminal (Hotaru) in the Desk's
  folder.
- Opening an app that's running raises its most recent window (switching
  Desk if needed); New Window starts another.
- **Application Launches…** (Toolchest → System) lists the last 64 launch
  attempts; a launch that fails or crashes posts a notification with Show
  Details and Open in Logs.

## Menus

- One menu bar for the focused app. **❖-right-click** anywhere (or
  right-click the empty desktop) opens the app's whole menu at the pointer.
- F10 (over Aizome's apps and the desktop) or ❖⌥M focuses the menu bar;
  ↑↓←→, Return, Esc. Typing in an open menu filters it.
- Drag a menu's title strip to tear it off into a palette (close box, pin).
- App menu ▸ **Services** acts on selected text: New Note with Selection,
  Open in Sumi, Search the Web, Open URL, Look Up in Files, Calculate, Copy
  as Plain Text.
- Apps without menus get: App (About, Hide ❖H, Hide Others ⌥❖H, Quit ❖Q)
  and Window (Minimise ❖M, Fill ⌥❖↑, Split Left/Right ⌥❖←/→, Stack with…
  ⌃❖S, Show Previous/Next Tab ⇧❖[ ⇧❖], Move Tab to New Window ⌃⇧❖S, Move
  to Desk ▸, Full Screen ⌃❖F, Close ❖W).
- With no window focused, the app is the desktop: Aizome ▸ About,
  Settings… ❖,, Lock, Log Out…; File ▸ New Folder on Desktop ⇧❖N, Find…;
  Desks ▸; Help ▸ Search, Aizome Help, Aizome Tour.

## Windows and tabs

- Each window has a tab as wide as its title (amber when focused).
  Double-click a tab: Fill (zoom to the usable area) by default.
- A new window of the focused app opens as another tab of its window
  (Settings → Desktop & Deskbar → Stack new windows of the same app).
- Drop one window's tab among another's tabs: they stack into a group
  (any apps). Drag a background tab out to unstack it; **Alt-drag** the
  showing tab to pull it out (a plain drag moves the group); **Shift**-drag
  slides a tab along the top edge.
- ⌃❖-drag moves the window under the pointer; ⌃❖-right-drag resizes from
  the nearest corner. ⌃❖Space opens the window's marking menu.

## Key bindings (defaults)

| Keys | Does |
|---|---|
| ❖Space / ❖⌥Space | palette / Toolchest |
| ❖Tab, ❖⇧Tab | app switcher (forward, back) |
| ❖\` | next window of the same app |
| ❖H, ❖⌥H, ❖M | hide app, hide others, minimise |
| ⌥❖↑ / ⌥❖↓ / ⌥❖← / ⌥❖→ | fill / minimise / split left / split right |
| ⌃❖F | full screen |
| ⌃❖S / ⌃⇧❖S | stack with the window behind / take the tab out |
| ❖⇧[ / ❖⇧] | previous / next tab in the group |
| ⌃⇧❖[ / ⌃⇧❖] | move the tab left / right |
| ⌃❖1…9 / ⌃⇧❖1…9 | go to Desk N / take the window to Desk N |
| ⌃❖← / ⌃❖→ (add ⇧ to take the window) | previous / next Desk |
| ⌃❖↑ | Desks overview |
| ❖Return | terminal |
| ⌃❖Q | lock |
| ❖⇧3 / ❖⇧4 or Print / ❖⇧5 | screenshot of the screen / an area / Capture menu |
| ⌥Print | start (pick area or click a display) or stop a recording |
| ⌃❖R | Reload Aizome (window management, shell, settings; windows stay) |
| ❖⌥D / ❖⌥Esc | dictation start-stop / cancel (only once Koe's dictation is on) |
| Ctrl+Alt+Del | recovery overlay (always works, even over games and the lock) |
| volume, mute, brightness, media keys | as labelled, with an on-screen level |

In Aizome's apps ❖C ❖V ❖X ❖Z ❖A ❖W ❖Q ❖F ❖S are Copy, Paste, Cut,
Undo, Select All, Close, Quit, Find, Save. In other apps Aizome turns ❖
shortcuts into the Ctrl ones they expect (Ctrl+Shift in terminals such as
foot or kitty, where ❖W and ❖Q close the window / quit). Turn that off in
Settings → Desktop & Deskbar → Command key for all apps.

Rebind in `[keys.bindings]` (aizome-settings): `"Ctrl+Super+Up" =
"overview"`, `"none"` unbinds, `"spawn PROG ARGS"`. Action names:
`palette` `toolchest` `menu-bar` `switcher` `switcher-reverse`
`next-window-of-app` `hide-app` `hide-others` `minimise` `fill`
`split-left` `split-right` `fullscreen` `stack-with-behind` `unstack`
`window-menu` `previous-tab` `next-tab` `move-tab-left` `move-tab-right`
`previous-desk` `next-desk` `take-to-previous-desk` `take-to-next-desk`
`go-to-desk-N` `take-to-desk-N` `overview` `terminal` `lock`
`screenshot-screen` `screenshot-area` `screenshot-window` `screenshot-tool`
`record` `record-screen` `record-area` `stop-recording` `reload`
`volume-up` `volume-down` `mute` `mic-mute` `brightness-up`
`brightness-down` `play-pause` `next` `previous` `stop`.
Ctrl+Alt+Del can't be rebound.

## Desks

- Four Desks by default, named 1–4. The strip in the menu bar shows them;
  click one to show it on this display (each display shows its own Desk).
  `+` adds one; double-click renames; right-click: Rename, Set Folder…
  (where its terminal and new files start), Remove (windows move to the
  Desk before it).
- Drag a window's row in the Deskbar onto a Desk in the strip to move it.
- Settings → Desktop & Deskbar → Desks: names, folders, add, remove.

## Screenshots and recording

- ❖⇧3 whole screen; ❖⇧4 or Print: drag an area, click for the display,
  Space then click for a window, Esc cancels. ❖⇧5: Capture menu
  (Screenshot of the Screen / an Area / the Window, Record the Screen ▸,
  Record an Area ▸ with or without desktop audio and microphone, Open
  Recordings, Stop Recording).
- Saved as `~/Pictures/Screenshots/Screenshot YYYY-MM-DD at HH.MM.SS.png`
  **and** copied to the clipboard; a notification offers Show in Hako.
- Recording: ⌥Print, the palette's Record Screen / Record Area (each also
  "with Desktop Audio", "with Microphone", or both), or the Capture menu.
  While recording a red dot and timer show in the bar; click it, ⌥Print or
  Stop Recording to stop. Files: `~/Videos/Recordings/screenrecording-YYYY-MM-DD_HH-MM-SS.mp4`.
  Defaults in `[recording]`.
- Screen sharing (browsers): Aizome asks which screen or window (click a
  screen; Space then click a window; Esc refuses). A blue chip in the
  Deskbar header shows sharing; press it for Stop Sharing.

## Notifications

- A new one shows in the menu bar's line for 5 s, then is filed under its
  app in the Deskbar ("3 new"); click to open it, × to dismiss. Critical
  ones stay as a card until acted on.
- Do Not Disturb: palette "Do Not Disturb", the clock panel, or quiet
  hours in Settings → Notifications (also per-app mute/block/silence).

## Theme, brightness, sound

- Display panel (systray): brightness, Night / Day / Auto, Size, Night
  Light. Palette: "Switch to Day" / "Switch to Night". Settings → Appearance.
- Sound panel: volume, output device, input, playing media.
- Battery panel (laptops): power mode, Keep Awake.

## Lock, sleep, log out

- Lock: ⌃❖Q, Toolchest → Session → Lock, palette "Lock". Also after the
  idle time (`[idle] lock_after_minutes`, default 10) and before sleep.
- Restart…, Shut Down…, Log Out… confirm in a dialog that counts down 60 s
  (Cancel or Esc stops it). Sleep is immediate.
- **Keep Awake** (no sleep, idle lock or display-off until turned off;
  survives logout): palette "Keep Awake", the Battery panel switch, or
  `aizome-awake on|off|toggle|status`. A red cup means it isn't holding:
  `systemctl --user status aizome-keep-awake`.

## Ctrl+Alt+Del: the recovery overlay

Opens over everything (games, the lock screen). Lists apps with CPU,
memory, "Not responding" or "Crashed". Esc or Ctrl+Alt+Del closes it.

| Action | Key | Effect |
|---|---|---|
| Close | W (Return on a responding app) | asks the app's windows to close |
| Force Quit | Delete (Return on an app in trouble) | kills the app (its whole scope) |
| Reset Input | R | ends stuck drags/grabs, releases held keys, frees a locked pointer |
| Restart Shell | S | restarts the menu bar/Deskbar/palette; apps keep running. Over the lock screen: Restart Lock Screen (session stays locked) |
| Reload Aizome | A | reloads window management in place; windows stay |
| Lock | L | locks |
| Log Out | O twice (within 4 s) | ends the session |
| Text Console | T twice | switches to a text console |

Over the lock screen only Reset Input, Restart Lock Screen and Text
Console are offered. Use the overlay before anything drastic: a pointer
stuck to a window → Reset Input; menu bar or Deskbar gone or frozen, or a
black lock screen with no password field → Restart Shell / Restart Lock
Screen; a hung app → Force Quit.

If the compositor itself freezes for 30 s, systemd's watchdog restarts it
(up to three times in a row); apps that were open lose their windows, and
the restarted compositor opens this overlay with "Aizome stopped
responding at HH:MM and was restarted" (details in
`~/.local/state/aizome/stall.log`, dumps in `coredumpctl info aizome`).

## The file chooser

Browsers, Electron apps, Zed and Flatpaks open and save in Aizome's own
chooser (Files in miniature over the app's window). "Save as" accepts a
path (`~/Documents/x.md`). Go menu: Back ❖[, Forward ❖], Enclosing Folder
❖↑, Recent ⇧❖F, Home ⇧❖H, Desktop ⇧❖D, Documents ⇧❖O, Downloads ⌥❖L, Go
to Folder… ⇧❖G; New Folder ⇧❖N; Show Hidden Files ⇧❖.; Return accepts,
Esc cancels. Qt apps and some GTK 4 apps show their own dialogs. Aizome's
own apps use a sheet in their window.

## Keyboards

Plugging in or pairing a keyboard: Settings can set it up (Keyboard → Set
Up…: press the key left of the space bar; it suggests where ❖ goes, for
that keyboard alone). Layouts and shortcuts are every keyboard's.
