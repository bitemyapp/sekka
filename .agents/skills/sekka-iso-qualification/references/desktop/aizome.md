# Q4 — Aizome, through real input

Use the Aizome source pinned by this ISO, not whichever checkout happens to be
nearby. Inventory `docs/shell.md`, `docs/apps.md`, settings pages, menus and
shortcuts. Assign each feature its own ledger ID (Q4.shell.*, Q4.app.NAME.*,
Q4.settings.PAGE.*). All actual installed apps and pages are required, including
ones added after this reference was written. For each action record visible
success, cancellation, relevant invalid input, persistence and recovery.

## Compositor and shell

- Open native Sidestep, GTK, Qt and XWayland apps. Focus by click and keyboard,
  type, select, copy/cut/paste, scroll, resize, move, maximize/fullscreen,
  minimize/restore, close and relaunch. No blank surfaces, lost input, stale
  pixels or crashes. Check integer scaling and software/GPU rendering cases.
- Haiku tabs: slide a tab, stack and unstack windows from different apps,
  reorder, select, close one group member and recover the others.
- NeXT menus: focused app menu bar, nested menus, disabled items, mnemonics,
  command shortcuts, Super-right-click menu at the pointer, tear-off palettes,
  keyboard navigation, outside-click/Escape cancellation and focus restoration.
- Toolchest, Deskbar and all Desks/overview controls: create/switch/rename/
  remove supported Desks; move windows between them; overview zoom and parked
  Desks; window lists, application grouping, launch and restore. Test persistence
  according to the implementation, including logout/reboot.
- Shelf and drag/drop: native file/text drag, park and retrieve, cross-app drop,
  cancelled drop, and invalid target. Confirm data, not only animation.
- Command palette: app command, window, file, application, setting and arithmetic
  results; keyboard selection, execution, empty/no-result and Escape; shortcut
  hints match actual behavior. Test search after app/window state changes.
- Clipboard and Super command translation in native and nonnative apps;
  Unicode, multiline text, clipboard between desktops where supported.
- Notifications: send, view, dismiss, actions, grouping/history, images and DND;
  status/tray items and menus; clock/calendar and power/status indicators.
- Capture/portals: screenshot and screen/window sharing via a real portal
  consumer, choose/cancel, stop, start again; open/save file picker and permission
  prompts. Check only intended content is captured and cancellation is honored.
- Polkit authentication: cancel, wrong password, success; focus and modality;
  no competing agent. Lock/logout/reboot use the Q3 checks.

## Native applications

For each installed app, exercise every menu and primary control from its actual
inventory, including keyboard-only access and dialogs. Examples to expand:

| App | Required realistic journey |
| --- | --- |
| Settei | Every settings page; change/apply/cancel/reopen/relogin; unavailable devices and invalid inputs; see below |
| Yukimi | Discover/search/details/install/remove; installed list and updates; privileged prompt and error recovery (Q6) |
| Hako | Create folder/file, navigate/back/search, rename, copy/move/delete, trash/restore if offered, open-with, hidden files, permissions, missing paths, removable media |
| Sumi | Open, edit, select, undo/redo, find/replace, save/save-as, Unicode, unsaved-close prompt, read-only/error path, reopen exact content |
| Fusen | Create/edit/delete notes, cancel deletion, persistence, multiple notes and text selection |
| Koyomi | Calendar navigation and offered event CRUD, dates/timezone, invalid data, saved-state persistence |
| Tokei | Clock, timer and alarms actually offered; start/stop/reset, notification, persistence and cancellation |
| Soroban | Basic arithmetic, precedence, decimals, clear/backspace, keyboard input, divide-by-zero/invalid input and recovery |
| Tenki | Default/manual location, refresh, unit settings, no network/error, recovery and saved location |
| Tansu | Inspect only disposable devices; refresh, detail accuracy, supported operations and confirmation/cancel paths |
| Kura | Scan disposable files, navigate sizes, rescan after changes, permission errors and cancellation |
| Myaku | CPU/memory/process views, search/sort/details, safe test-process termination, denied operation, refresh responsiveness |
| foot | Typing, Unicode, shortcuts, selection/clipboard, resize, working directory, close and reopen |

## Settings inventory

Exercise every offered page. Typically this includes displays (mode, scale,
arrangement and recovery), input/keyboard/layout, mouse/touchpad, theme/appearance,
background, sound, network and Wi-Fi credentials, Bluetooth, power/idle,
notifications, default apps, accounts and system information. Match visible
values to backend state and persistence. Use VM audio sinks, hotplug displays,
virtual networking and simulated radios when available. Missing physical devices
must show a usable empty/error state; this is not proof of pairing or hardware
operation. Record a separate required hardware check when appropriate.

After each group inspect journal, coredumps and failed user units. Reproduce any
failure from a fresh session and retain steps and evidence. Never mark an app
passed just because its process stayed alive or its initial window appeared.
