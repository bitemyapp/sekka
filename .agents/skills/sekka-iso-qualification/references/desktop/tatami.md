# Q5 — Tatami, through real input

Inventory the pinned `desktop/tatami/README.md`, Rust command dispatch,
`config/hypr/hyprland.lua`, all Elephant menu files and desktop entries. Assign
Q5.menu.*, Q5.shortcut.*, Q5.panel.* and Q5.app.* IDs to every exposed action.
Execute menu and shortcut routes where both are provided. Confirm unavailable
hardware is described accurately; do not treat an empty panel as pairing proof.

- Session starts from Aizome's greeter. Hyprland, uwsm, Waybar, Walker/Elephant,
  Mako, OSD and polkit agent run once; unrelated desktop components stay out.
- Menu tree: Apps, Learn, Trigger, Setup, Style where present, About and System;
  all submenus and search. Correct app names/icons/descriptions, keyword search,
  launch, Escape/back, keyboard-only navigation. Super+K matches real bindings.
- Windows/workspaces: move/focus/resize, pop out, fullscreen, transparency, gaps,
  aspect, layouts, grouping if offered, workspace movement and persistence.
  Apps launched by bindings/menu survive launcher restart and remain isolated.
- Terminal, browser, editor and file-manager routes; terminal in the active
  directory; Super+C/V/X and ordinary terminal shortcuts; UTF-8 and multiline
  clipboard; GTK/Qt/Wayland/XWayland input and portal dialogs.
- Bar: workspaces, clock/calendar/hover, weather, tray, network, Bluetooth,
  volume/microphone, display, battery, power profile and all indicators. Check
  right-click/hover routes and panel open/close behavior.
- Network panel and full Network app: wired state, scan, connect correct/wrong
  password, saved reconnect, disconnect, radio switch, hidden/enterprise/VPN
  settings offered, DNS provider/custom validation and actual resolution,
  connection editor add/edit/delete/cancel and recovery after service restart.
  Use disposable virtual/simulated networks; never real credentials.
- Bluetooth: missing-adapter state plus simulated/passed-through test adapter
  pairing, passkey, connect/disconnect and removal when qualifying support.
- Sound: output/input volume and mute, defaults, streams and routing, devices,
  ports/profiles where present, hotplug/service restart and UI/backend agreement.
  Inspect the actual revision: it may ship wiremix or a dedicated Sound window.
- Displays: resolution/scale, arrange, main screen, mirror/unmirror, disable/
  enable, safe last-display recovery, hotplug/unplug, reload persistence and
  cursor visibility. Exercise Night Shift, zoom and supported keyboard/lid
  paths using suitable emulation; record hardware limitations explicitly.
- Background: choose each control type, global/per-screen, scaling/color,
  cancel/apply, lock screen consistency, logout/reboot and reset/default.
- Activity: process/system views, sort/search/details, controlled process kill,
  permissions/errors, charts and responsiveness under guest load.
- Weather: location, units if offered, refresh, no-network/cache/recovery,
  displayed and hover data; service errors do not hang the bar.
- Capture: full/window/region screenshot, freeze/cancel, clipboard, save, Satty
  edit; repeated captures clean temporary files. Record region/monitor with
  audio/mic and fallback path, start/stop indicator, playable result, cancellation.
  OCR and QR use known test text/codes and confirm the actual decoded result.
- Notifications/DND, volume/mic/brightness/media OSD, time/battery/backlight
  notifications, keep-awake and idle lock, touchpad and keyboard layout toggles.
- System actions: reload without losing work or display state, configuration
  edit through Setup, lock/wrong-password/unlock, logout, reboot, shutdown;
  suspend/resume and hibernate where advertised and supported by the VM.

For each persistent setting compare backend/config after apply, new session and
reboot; cancel must preserve the previous value. Inspect journals/coredumps and
user services throughout. Rotate back to Aizome and another installed desktop
to detect leaked theme, environment, notifications, keyring or portal state.
