---
name: sekka-troubleshooting
description: "Diagnosing and fixing faults on a running Sekka system and Aizome session — safe order of investigation, reaching the user's session from root, units, masks, ownership, logs, and known symptoms with causes (settings reverted, wrong keyboard layout, no sound, file dialogs dead, DNS/firewall, disk full, runaway process, wrong clock, SSH key refused, login refused \"already running\", black lock screen, frozen desktop, failed update). Load when something on the machine is broken."
---

> In Tasuke, `sudo CMD` in this guide means CMD as root: call `run_as_root` with CMD, without the word `sudo` (Tasuke asks the person, then they type their password). When telling the person how to do it themselves in a terminal, keep the `sudo`.

# Troubleshooting Sekka and Aizome

Sekka is NixOS with Genkan's installed configuration, the Aizome desktop
and Yukimi. Most of the system is generated from the Nix store, and a fault
is usually a local change on top of it: a hand-edited file, a stopped or
masked unit, wrong ownership, a stray process, a full disk, a typo in a
settings file. Find that change.

## Ground rules

- Investigate before changing anything. Reproduce the symptom, then read
  the failing component's logs, unit state, configuration, permissions and
  processes until the evidence names the cause.
- Once you know what is wrong, fix it. If you can't tell who made a bad
  change, say so; don't spend the session finding out.
- Fix the cause with the smallest change. Don't disable security features
  (firewall, SSH `StrictModes`, password policy, polkit), don't delete the
  user's data or backups, don't overwrite their files with test data,
  don't reboot or end their session.
- **Aizome applies its settings by itself** (the compositor and shell
  re-read `settings.toml` within a second; `aizomectl reload-config` forces
  it). Never kill `aizome`, `aizome-shell`, `xwayland-satellite`,
  `aizome-session` or greetd, and never `loginctl terminate-*`, to "apply"
  a change: that ends the person's session and loses their work. To get
  the shell back use `aizomectl recovery restart-shell`.
- **Stay on the reported problem.** Something that looks suspicious but
  isn't what the person asked about (an unfamiliar unit, process, cron job,
  open port, key) is reported to them, not stopped, deleted or "cleaned up".
- Verify the way the user would notice: the command they ran, the page
  they opened, the setting they changed.
- Report the root cause with its evidence and exactly what you changed,
  then stop. A durable fix on an installed machine belongs in `/etc/nixos`
  (then `sekka-update` or `sudo nixos-rebuild switch`); say so when the
  running-system fix won't survive the next activation.

## Reading a NixOS machine

- `/etc` is mostly symlinks into `/etc/static` (the read-only store). A
  regular file where NixOS keeps a symlink (`ls -la /etc/hosts
  /etc/resolv.conf`) is a local edit; `readlink -f` shows what NixOS would
  have.
- Units: `systemctl status UNIT --no-pager -l`, `journalctl -u UNIT -b
  --no-pager -n 200`, `systemctl --failed`, `systemctl list-unit-files
  --state=masked`, and for the user's session the same with `--user`. A
  mask is a symlink to `/dev/null` in `/etc/systemd/system`,
  `/run/systemd/system` (runtime, `mask --runtime`) or
  `~/.config/systemd/user`; `systemctl unmask` (with `--runtime` for
  runtime masks) removes it, then `systemctl daemon-reload` and start it.
- A process that comes back after `kill` is restarted by its unit
  (`Restart=`): find it with `systemctl status PID` or `systemd-cgls`, then
  stop and disable the unit rather than the process (`systemctl --user
  disable --now UNIT` for a user unit).
- Recent changes: `find /etc /home -newer /run/current-system -not -path
  '*/.cache/*' 2>/dev/null | head`, file mtimes, `journalctl -b --since
  -2h`.
- Python is not necessarily installed; use shell tools (or
  `aizomectl reload-config` to check Aizome's TOML).

## The user's session from a root shell

The desktop user's systemd manager owns the session. Run its tools as the
user with the session's environment: `XDG_RUNTIME_DIR=/run/user/UID`,
`DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/UID/bus`, and for Wayland
clients `WAYLAND_DISPLAY` from `/run/user/UID/wayland-*` (e.g. `sudo -u
USER env XDG_RUNTIME_DIR=… DBUS_SESSION_BUS_ADDRESS=… systemctl --user
status`). On the evaluation VM, `as-alice CMD` does this. Session logs:
`journalctl _UID=UID -b`, or `journalctl --user -u UNIT` as the user.

## Aizome

- **Session.** The login screen (greetd + `aizome-greeter`) runs
  `aizome-session`, which starts the user unit `aizome.service` (the
  compositor, `Slice=session.slice`, `WatchdogSec=30s`). It binds
  `graphical-session.target` through `aizome-session.target`, which wants
  `aizome-network-agent.service`, `aizome-file-manager.service` and
  autostart entries. The compositor starts and restarts `aizome-shell`
  (menu bar, Deskbar, palette, notifications, lock screen, polkit agent)
  and `xwayland-satellite` (X11 apps). A stalled compositor is restarted
  by its watchdog up to three times (`coredumpctl info aizome`,
  `~/.local/state/aizome/stall.log`).
- **IPC.** `aizomectl` talks to the compositor over
  `$XDG_RUNTIME_DIR/aizome.$WAYLAND_DISPLAY.sock` (`--socket PATH`
  otherwise): `aizomectl --json windows`, `outputs`, `desks`,
  `input-state`, `keyboards`, `screenshot PATH`, `reload-config`,
  `recovery status`, `recovery restart-shell`, `recovery reset-input`.
  (The subcommand is `recovery ACTION`; there is no `recovery-action`.)
- **Recovery overlay.** Ctrl+Alt+Del, always: Close (W), Force Quit
  (Delete), Reset Input (R), Restart Shell (S; Restart Lock Screen while
  locked), Reload Aizome (A), Lock (L), Log Out (O twice), Text Console
  (T twice).
- **Settings file.** `~/.config/aizome/settings.toml`, written by Settings
  (Settei) and watched by the compositor and shell. If it doesn't parse
  (a syntax error, or a wrong type or unknown value in `[appearance]`,
  `[input.*]`, `[[outputs]]`, `[[desks]]`, `[keys]`, `[windows]`, `[idle]`,
  `[night_light]`), the whole file is ignored: at login the compositor logs
  `…/settings.toml: <error>; using the defaults`, on a live edit it keeps
  its previous settings while the shell and apps revert to defaults (the
  default theme is Night). `aizomectl reload-config` prints the parse error
  with its line and column. If Settings saved while it was broken, the
  person's version is in `settings.toml.broken` beside it. Fix the bad line
  in place (or repair `.broken` and move it back); never delete the file.
  Key sections: `[appearance]` (`theme` night/day/high-contrast/auto,
  `focus`, `wallpaper_night`/`wallpaper_day`), `[input.keyboard]`
  (`layouts`, `variant`, `options`, repeat), `[[input.keyboards]]`,
  `[input.pointer]`, `[input.touchpad]`, `[idle]` (`lock_after_minutes`,
  `monitors_off_after_minutes`), `[keys]`/`[keys.bindings]`, `[deskbar]`,
  `[[outputs]]`. Full reference: aizome-settings.
- **Ownership.** Everything under the user's home must belong to the user.
  Running an app or a copy with `sudo` leaves root-owned files that the
  user's apps then can't write (`~/.config/aizome`, `~/Notes`,
  `~/.local/state/aizome`): `find /home/USER -xdev -not -user USER`; fix
  with `chown -R USER:users PATH` on just that path.
- **Apps.** Settings `aizome-settei` (`--pane ID`), Notes `aizome-fusen`
  (Markdown in `~/Notes`, pins/colours in `~/Notes/.fusen.json`), Files
  `aizome-hako`, terminal `aizome-hotaru` (config `~/.config/aizome/hotaru.toml`;
  foot is the fallback), Logs `aizome-nisshi`, Calculator `aizome-soroban`,
  Software `aizome-yukimi`. A launch that fails shows in Toolchest → System
  → Application Launches…; its output: `aizome-nisshi --launch ID`, or
  `journalctl --user -b | grep -i APPNAME`.
- **Portals.** Apps' file choosers and screen sharing go through the user
  unit `xdg-desktop-portal.service`, with Aizome's backend
  `aizome-portal.service` (file choosers, D-Bus activated),
  `xdg-desktop-portal-wlr` (screenshots, screencasts) and
  `xdg-desktop-portal-gtk` (the rest), chosen by `aizome-portals.conf`.
  Probe: `busctl --user call org.freedesktop.portal.Desktop
  /org/freedesktop/portal/desktop org.freedesktop.portal.Settings ReadOne
  ss org.freedesktop.appearance color-scheme`.
- **Sound.** PipeWire user units `pipewire.socket`/`.service`,
  `pipewire-pulse.socket`/`.service`, `wireplumber.service`; `wpctl
  status` lists devices (`*` = default), `wpctl set-default ID` changes it,
  `wpctl inspect @DEFAULT_AUDIO_SINK@` the default output.
- **Network credentials.** `nm-applet` doesn't run in Aizome; Wi-Fi/VPN
  password prompts come from `aizome-network-agent.service` (user unit).

## Symptoms

| Symptom | Look at | Usual cause → fix |
|---|---|---|
| Theme, wallpaper or every setting reverted | `journalctl --user -b --no-pager -g settings.toml`; `aizomectl reload-config` | syntax error → fix the line |
| Settings "don't stick" | `ls -ld ~/.config/aizome ~/.config/aizome/settings.toml` | root-owned → chown to the user |
| Letters wrong (Y/Z swapped, symbols moved) | `[input.keyboard] layouts`/`variant`; `[[input.keyboards]]`; system `services.xserver.xkb.layout` | wrong layout in settings.toml → set it (Settings → Keyboard) |
| ❖ and Alt seem swapped | `[input.keyboard] options` (`altwin:…`), per-keyboard `options` | an Alt/Super swap set or missing → Settings → Keyboard → Modifier keys |
| No sound | `systemctl --user status pipewire pipewire-pulse wireplumber`; `systemctl --user list-unit-files --state=masked`; `wpctl status` | masked/stopped units → unmask, start; wrong default → `wpctl set-default` |
| File ▸ Open does nothing (browsers, Electron) | `systemctl --user status xdg-desktop-portal aizome-portal` | portal masked/failed → unmask and start |
| Notes can't save | `ls -ld ~/Notes`, Fusen's error | root-owned `~/Notes` → chown |
| Login returns to the greeter: "Aizome is already running in this user's session" | `systemctl --user status aizome.service` as the user (from a text console or SSH) | an orphaned compositor from an ended login. With the person's agreement (its windows close): `systemctl --user start --job-mode=replace-irreversibly aizome-shutdown.target` |
| Black lock screen, no password field (after a display change) | — | Ctrl+Alt+Del → S (Restart Lock Screen); the session stays locked |
| Pointer stuck to a window, clicks go nowhere | `aizomectl input-state` | `aizomectl recovery reset-input` (or Ctrl+Alt+Del → R) |
| Menu bar/Deskbar missing or frozen | `pgrep -u USER -x aizome-shell` | `aizomectl recovery restart-shell` |
| App frozen | `aizomectl recovery status` | Ctrl+Alt+Del → Force Quit, with consent if it has unsaved work |
| Hotaru won't open | `aizome-hotaru` in foot: "Hotaru configuration: …" | invalid `hotaru.toml` (unknown keys are errors) → fix it |
| Clock wrong by hours | `timedatectl` | wrong zone → `timedatectl set-timezone Area/City` (not `ln -sf`: timedated keeps the old one) |
| Software won't install/update | `systemctl status nix-daemon.socket`; `systemctl list-unit-files --state=masked`; `df -h /` | masked nix-daemon → `systemctl unmask --runtime nix-daemon.socket nix-daemon.service; systemctl daemon-reload; systemctl start nix-daemon.socket`; < 4 GB free → free space |
| Keep Awake cup red | `systemctl --user status aizome-keep-awake` | its inhibitor isn't held → turn it off and on |
| Desktop froze, then windows vanished | `~/.local/state/aizome/stall.log`, `coredumpctl info aizome` | compositor watchdog restart → report the backtrace |

## The system

- **Name resolution.** `getent hosts NAME` resolves as applications do
  (`/etc/nsswitch.conf`: files, then DNS). `/etc/hosts` comes from the
  configuration (`networking.hosts`/`extraHosts`). `/etc/resolv.conf` is
  written by resolvconf from NetworkManager and `networking.nameservers`:
  `resolvconf -l` lists its sources, `resolvconf -u` rewrites it, and `dig
  @SERVER NAME` tests a server directly (if installed). DNS servers per
  connection: Settings → Network → Details → DNS.
- **Connectivity.** `ip addr`, `ip route`, `nmcli device status`, `ping`,
  `curl -v --max-time 5`. Listening sockets and their processes: `ss
  -ltnp`. The NixOS firewall is the `nixos-fw` chains (`iptables -S
  nixos-fw`); `nft list ruleset` also shows any other tables. Remove only
  the rule or table at fault, never the whole ruleset.
- **Nix.** Users reach the store through `nix-daemon.socket`
  (`/nix/var/nix/daemon-socket/socket`), which starts `nix-daemon.service`.
- **Time.** `timedatectl`; with Aizome, Settings owns the time zone
  (`time.timeZone` in configuration.nix is overridden; `/etc/localtime` is
  a link timedated writes and activation preserves).
- **Storage.** In Tasuke, `system_usage` gives each disk's free space,
  labelled; then `df -i`, `du -xh --max-depth=2 DIR | sort -h | tail`,
  `lsof +L1` (deleted files still held open). Free space by
  removing what is safe to lose (runaway logs, caches, partial files, old
  generations via Yukimi → Storage), not the user's data or backups;
  truncate a log a process still writes (`truncate -s 0 FILE`) rather
  than deleting it.
- **SSH.** With `StrictModes` sshd refuses a user's keys when `~/.ssh`,
  `authorized_keys` or the home directory is writable by group or others,
  or owned by someone else; its journal says so (`journalctl -u sshd`).
  Fix with `chmod 700 ~/.ssh; chmod 600 ~/.ssh/authorized_keys` (as the
  user). `sshd -T` prints the effective configuration. Never replace the
  user's `authorized_keys`.
- **Load.** In Tasuke, `system_usage` names the processes using the most
  memory and processor time, every figure labelled (`ps` and `top` columns
  are easy to misread); then `ps -o pid,etime,cmd -p PID` for one of them,
  `systemd-cgtop -b -n 1`;
  Myaku (`aizome-myaku --print`). systemd-oomd kills the largest offender
  under sustained memory pressure (Sekka's tuning).
- **Updates.** `sekka-update --status`; a failed build leaves the running
  system unchanged. Nisshi → Errors & Warnings at that time; Yukimi stops
  below 4 GB free.
