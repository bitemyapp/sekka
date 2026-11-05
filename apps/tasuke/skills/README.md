# Skills for Sekka's local assistant

On-demand knowledge for an assistant running on a Sekka computer: one
directory per skill, each a `SKILL.md` with YAML front matter (`name`,
`description`) and a Markdown body. The description is all the assistant
sees until it loads a skill, so each one names the requests it is for.
For oh-my-pi, point `skills.customDirectories` at this directory (it
scans `*/SKILL.md`, one level deep, and requires a description).

| Skill | Covers |
|---|---|
| [sekka-basics](sekka-basics/SKILL.md) | What Sekka is; `/etc/nixos` and the `calamares.*` options; per-user, live-daemon and declarative changes; applying them (`sekka-update`, Yukimi, `nixos-rebuild`); going back; where things live; what to ask before |
| [aizome-desktop](aizome-desktop/SKILL.md) | Menu bar, Toolchest, palette, Deskbar, Desks, tabs, every default binding and action name, screenshots and recording, notifications, lock and session, Keep Awake, Ctrl+Alt+Del, the file chooser |
| [aizome-settings](aizome-settings/SKILL.md) | Settei's panes and `--pane` ids, `settings.toml` sections, keys and defaults, invalid-file behaviour, safe ways to change settings |
| [aizome-apps](aizome-apps/SKILL.md) | The 19 Aizome apps: binaries, data, CLI flags, shortcuts, typical tasks |
| [sekka-maintenance](sekka-maintenance/SKILL.md) | Updating, software, disk space, logs, network, sound, Bluetooth, printers, users, time/locale/keyboard/host name, firewall, SSH, backups |
| [aizome-computer-use](aizome-computer-use/SKILL.md) | `aizomectl` and `aizome-shell msg` for operating the desktop, screen geometry, element files, input limits, verification, rules |
| [sekka-troubleshooting](sekka-troubleshooting/SKILL.md) | Investigation order, the session from root, Aizome's parts, a symptom table; extends `.agents/skills/sekka-troubleshooting` |

## Always-on system prompt

```text
You are the assistant built into this Sekka computer (NixOS with the
Aizome desktop). You help its user: answer how-to questions, do routine
maintenance, diagnose problems, and operate the desktop when asked.

Skills are listed by name and description. Before answering or acting in
an area a skill covers, load it and follow it; don't answer Sekka or
Aizome details from memory. If neither a skill nor the machine can
confirm something, say you aren't sure.

Rules:
- Look before you act; reading is always fine.
- Before changing anything (a setting, file, service, package or the
  system configuration), say what you will do and why, and wait for a yes.
- Prefer the least invasive fix that removes the cause: a Settings toggle
  before a file edit, a per-user change before a system one, a live
  change before a rebuild.
- Never delete, overwrite or move the person's files, notes or backups
  unless they asked for exactly that.
- Never end their session: no logging out, reboot, power off, or killing
  the compositor or shell. Aizome applies settings by itself.
- Never type, read out or keep passwords or other secrets; let the
  person type them.
- Never weaken security (firewall, SSH, polkit, permissions) to make
  something work.
- Mention, don't touch, unrelated things that look wrong.
- After a change, check it the way the person would notice, then say
  what was wrong, what you changed, and whether it lasts past the next
  update or login.
```

## How they were checked

Each statement was checked against the source in this repository at the
time of writing (docs were read, then confirmed or corrected from code).
Where code and docs disagreed, the skills follow the code. Two read-only
sub-agents checked the apps' `main.rs`/argument parsing, menu definitions
and path construction file by file; their findings were spot-checked.

- **sekka-basics**: `README.md`; `templates/flake.nix.in`;
  `installer/genkan/rust/src/config.rs` (the generated configuration.nix),
  `install.rs` (files written to `/etc/nixos`, `/etc/nixos-secrets`),
  `applications.nix`; `installer/genkan/rust/system/default.nix`,
  `desktops.nix`, `tuning.nix`, `store-space.nix`, `binary-cache.nix`,
  `usb-wake.nix`, `nvidia.nix`, `fish/default.nix`, `boot-splash/default.nix`;
  `desktop/aizome/nix/module.nix` (time zone override, polkit rule,
  `aizome-update.path`); `apps/yukimi/README.md`;
  `apps/yukimi/yukimi-helper/src/bin/sekka-update.rs`, `src/main.rs`
  (clean, rollback, 4 GB floor); `desktop/aizome/docs/reload.md`,
  `recovery.md`; DEVELOPERS.md ("Programs built for other distributions").
- **aizome-desktop**: `desktop/aizome/README.md`, `docs/shell.md`,
  `docs/ipc.md`, `docs/recovery.md`, `docs/keep-awake.md`, `docs/apps.md`
  (file chooser); `crates/aizome-comp-logic/src/bindings.rs` (default
  chords, action names), `input.rs`, `cmdkey.rs`;
  `crates/aizome-shell/src/menus/build.rs` (Toolchest, fallback and
  desktop menus), `launch.rs`, `palette/mod.rs` (actions),
  `recording_glue.rs` (Capture menu), `recording/mod.rs`,
  `systray/panels.rs`, `confirm.rs`; `crates/aizome-comp-core/src/screenshot.rs`;
  `crates/aizome-awake/src/main.rs`; `docs/incidents/2026-10-27-lock-screen-display-change.md`.
- **aizome-settings**: `crates/aizome-config/src/lib.rs`, `shell.rs`,
  `compositor.rs`; `crates/aizome-theme/src/lib.rs`;
  `apps/settei/src/main.rs`, `app.rs`, `shared.rs` (the `.broken` copy),
  `listing.rs`, `extra.rs`, `pane.rs`, `panes/mod.rs` (ids, aliases) and
  every `panes/*.rs` (option ids), `panes/keyboard/modifiers.rs`,
  `panes/lock.rs`, `panes/general/defaults.rs`, `panes/displays/mod.rs`;
  `crates/aizome-comp-core/src/lib.rs`, `config.rs`, `requests.rs`
  (load fallbacks, `reload-config` errors); `crates/aizome-shell/src/shell.rs`;
  `crates/aizome-apps/src/mime.rs`; `crates/aizome-sys/src/datetime/mod.rs`.
- **aizome-apps**: `desktop/aizome/docs/apps.md`, `koe.md`, `koyomi.md`,
  `kura-index.md`, `apps/hotaru/README.md`; for every app under
  `desktop/aizome/apps/`: `src/main.rs`, its argument parsing, menu
  builders, path helpers and `data/*.desktop`; `crates/aizome-kit/src/menu.rs`
  (standard menus); `desktop/aizome/apps/yukimi/src/ops.rs`, `ui/*.rs`;
  `desktop/aizome/nix/module.nix` (alarm, calendar and dictation units).
- **sekka-maintenance**: the sources above, plus `docs/applications.md`,
  DEVELOPERS.md ("Network DNS and IPv6", "Joining Wi-Fi networks"),
  `apps/settei/src/panes/users.rs`, `crates/aizome-sys/src/lib.rs`,
  `docs/agent-evals/README.md` (the timedated lesson) and
  `tests/agent-evals/harness/scenarios.py` (known fixes).
- **aizome-computer-use**: `crates/aizome-ipc/src/bin/aizomectl.rs`,
  `src/lib.rs`; `crates/aizome-comp-core/src/requests.rs` (key names,
  synthetic input), `screenshot.rs`; `crates/aizome-comp-logic/src/requests.rs`;
  `crates/aizome-shell/src/lib.rs` (`msg`), `apps/server.rs`, `shell.rs`;
  `crates/aizome-shell-proto/src/lib.rs`; `crates/aizome-kit/src/inspect.rs`,
  `docs/kit.md`; `tests/agent-evals/harness/vm.py`, `README.md`,
  `machine.nix`; `docs/agent-evals/README.md`.
- **sekka-troubleshooting**: `.agents/skills/sekka-troubleshooting/SKILL.md`
  (the base); `desktop/aizome/session/*`; `crates/aizome-comp-core/src/session.rs`;
  `docs/incidents/2026-10-28-aizome-orphaned-session.md`; the eval
  scenarios and results above; the settings and IPC sources above.

## Left out, because it couldn't be confirmed from this repository

NixOS option names for logind's lid handling (they depend on the Nixpkgs
release), boot-menu keys and timeouts, Tatami's own shortcuts, the full
list of kit element roles, and whether `dig` is installed. The effect of
an Alt/Super XKB swap on `aizomectl input` modifier names follows from
the code (synthetic keys are evdev positions read through the active
keymap) and XKB's behaviour, but hasn't been tried in a VM; the skill
says to prefer `aizomectl action` and to verify.

## Where the skills differ from existing docs

- `.agents/skills/sekka-troubleshooting`: `aizomectl recovery-action
  restart-shell` is `aizomectl recovery restart-shell`; the `python3`
  TOML check assumes Python, which a Sekka install doesn't have
  (`aizomectl reload-config` reports parse errors); a live edit that
  breaks settings.toml leaves the compositor on its previous settings
  while the shell and apps revert; the theme can also be `high-contrast`.
- `desktop/aizome/docs/shell.md`: wallpapers are `[appearance]
  wallpaper_night`/`wallpaper_day`, not `appearance.wallpaper.{night,day}`;
  the palette's theme actions are "Switch to Day"/"Switch to Night".
- `desktop/aizome/docs/apps.md`: Yukimi's desktop entry says "Package
  Manager", not "Software"; Hako's Quick Look has no PDF preview;
  Utsushi's Open Recent shows 12 of the 20 kept, and its resume margin at
  the end is the larger of 30 s and 5%.
- `docs/applications.md`: installed systems don't keep `applications.nix`
  beside the flake; Genkan writes only `flake.nix`, `flake.lock`,
  `configuration.nix` and `hardware-configuration.nix`, and the catalog
  comes through the `sekka` input. The generated configuration.nix's
  header (and a comment in `config.rs`) still names a `calamares` input.
- `apps/yukimi/README.md`: the button is "Update", and `sekka-update`
  applies live when it can (not always at the next restart).
