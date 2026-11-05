---
name: sekka-troubleshooting
description: Diagnose and fix problems on a running Sekka system and its Aizome desktop session — where configuration, state and logs live, how to reach the user's session from a root shell, and a safe order of investigation. Use for user-reported faults on an installed or VM Sekka machine, not for building or qualifying images.
---

# Troubleshooting Sekka and Aizome

Sekka is NixOS with Genkan's installed configuration, the Aizome desktop and Yukimi. Most of the system is generated from the Nix store, and a fault is usually a local change on top of it: a hand-edited file, a stopped or masked unit, wrong ownership, a stray process, a full disk. Find that change.

## Ground rules

- Investigate before changing anything. Reproduce the symptom, then read the failing component's logs, unit state, configuration, permissions and processes until the evidence names the cause.
- Fix the cause with the smallest change. Don't disable security features (firewall, SSH `StrictModes`, password policy), don't delete the user's data, don't reboot or end their session.
- Verify the way the user would notice: the command they ran, the page they opened, the setting they changed.
- Report the root cause with its evidence and exactly what you changed. A durable fix on an installed machine belongs in `/etc/nixos` (then `sekka-update` or `nixos-rebuild switch`); say so when the running-system fix won't survive the next activation.

## Reading a NixOS machine

- `/etc` is mostly symlinks into `/etc/static` (the read-only store). A regular file where NixOS keeps a symlink (`ls -la /etc/hosts /etc/resolv.conf`) is a local edit; `readlink -f` shows what NixOS would have.
- Units: `systemctl status UNIT --no-pager -l`, `journalctl -u UNIT -b --no-pager -n 200`, `systemctl --failed`, `systemctl list-unit-files --state=masked`, and for the user's session the same with `--user`. A mask is a symlink to `/dev/null` in `/etc/systemd/system`, `/run/systemd/system` (runtime, `mask --runtime`) or `~/.config/systemd/user`; `systemctl unmask` (with `--runtime` for runtime masks) removes it.
- A process that comes back after `kill` is restarted by its unit (`Restart=`): find it with `systemctl status PID` or `systemd-cgls`, then stop and disable the unit rather than the process.
- Recent changes: `find /etc /home -newer /run/current-system -not -path '*/.cache/*' 2>/dev/null | head`, file mtimes, and `journalctl -b --since -2h`.

## The user's session from a root shell

The desktop user's systemd manager owns the session. Run its tools as the user with the session's environment: `XDG_RUNTIME_DIR=/run/user/UID`, `DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/UID/bus`, and for Wayland clients `WAYLAND_DISPLAY` from `/run/user/UID/wayland-*`. (On the evaluation VM, `as-alice CMD` does this.) Session logs: `journalctl _UID=UID -b` or `journalctl --user -u UNIT` as the user.

## Aizome

- **Session.** greetd starts `aizome-session`, which starts the user unit `aizome.service` (the compositor). It binds `graphical-session.target` through `aizome-session.target`, and starts and restarts `aizome-shell` (menu bar, Deskbar, notifications, lock screen, polkit agent) and `xwayland-satellite` (X11 apps). The compositor's watchdog restarts a stalled compositor; `coredumpctl` has its dumps.
- **IPC.** `aizomectl` talks to the compositor over `$XDG_RUNTIME_DIR/aizome.$WAYLAND_DISPLAY.sock`: `aizomectl --json windows`, `outputs`, `input-state`, `keyboards`, `screenshot PATH`, `reload-config`, `recovery restart-shell`.
- **Settings file.** `~/.config/aizome/settings.toml`, written by Settings (Settei) and watched by the compositor and shell. If it doesn't parse, Aizome ignores the whole file: at login the compositor logs `settings.toml: <error>; using the defaults`; on a live edit the compositor keeps its previous settings while the shell and apps revert to the defaults (the default theme is Night). `aizomectl reload-config` (as the user) prints the parse error with its line and column; Python isn't installed on every Sekka system. If Settings saved while the file was broken, the user's version is in `settings.toml.broken` beside it. Sections: `[appearance]` (`theme` night/day/high-contrast/auto, `focus`, `wallpaper_night`/`wallpaper_day`), `[input.keyboard]` (`layouts`, `variant`, `options`, repeat), `[input.pointer]`, `[idle]` (`lock_after_minutes`, `monitors_off_after_minutes`), `[keys]`, `[deskbar]`, `[[outputs]]`. Fix the file in place rather than deleting it: the user's other settings are in it.
- **Ownership.** Everything under the user's home must belong to the user. Running an app or a copy with `sudo` leaves root-owned files that the user's apps then can't write (`~/.config/aizome`, `~/Notes`): `find /home/USER -xdev -not -user USER`.
- **Apps.** Settings `aizome-settei` (`--pane NAME`), Notes `aizome-fusen` (Markdown files in `~/Notes`, index `~/Notes/.fusen.json`), Files `aizome-hako`, terminal `aizome-hotaru` (foot is the fallback), Logs `aizome-nisshi`, Calculator `aizome-soroban`, Software `aizome-yukimi`.
- **Portals.** Apps' file choosers and screen sharing go through the user unit `xdg-desktop-portal.service`, with Aizome's backend `aizome-portal.service` (file choosers), `xdg-desktop-portal-wlr` (screenshots, screencasts) and `xdg-desktop-portal-gtk` (the rest), chosen by `aizome-portals.conf`. Probe: `busctl --user call org.freedesktop.portal.Desktop /org/freedesktop/portal/desktop org.freedesktop.portal.Settings ReadOne ss org.freedesktop.appearance color-scheme`.
- **Sound.** PipeWire user units `pipewire.socket`/`.service`, `pipewire-pulse.socket`/`.service`, `wireplumber.service`; `wpctl status` lists devices, `wpctl inspect @DEFAULT_AUDIO_SINK@` the default output.
- **Recovery.** Ctrl+Alt+Del opens the recovery overlay (force quit, reset input, restart the shell).

## The system

- **Name resolution.** `getent hosts NAME` resolves as applications do (`/etc/nsswitch.conf`: files, then DNS). `/etc/hosts` comes from the configuration. `/etc/resolv.conf` is written by resolvconf from NetworkManager and `networking.nameservers`: `resolvconf -l` lists its sources, `resolvconf -u` rewrites it, and `dig @SERVER NAME` tests a server directly.
- **Connectivity.** `ip addr`, `ip route`, `ping`, `curl -v --max-time 5`. Listening sockets and their processes: `ss -ltnp`. The NixOS firewall is the `nixos-fw` chains (`iptables -S nixos-fw`); `nft list ruleset` also shows any other tables. Remove only the rule or table at fault, never the whole ruleset.
- **Nix.** Users reach the store through `nix-daemon.socket` (`/nix/var/nix/daemon-socket/socket`), which starts `nix-daemon.service`.
- **Time.** `timedatectl`; with Aizome, Settings owns the time zone (`/etc/localtime` is a link timedated writes).
- **Storage.** `df -h`, `df -i`, `du -xh --max-depth=2 DIR | sort -h | tail`, `lsof +L1` (deleted files still held open). Free space by removing what is safe to lose (logs, caches, partial files), not the user's data or backups.
- **SSH.** With `StrictModes` sshd refuses a user's keys when `~/.ssh`, `authorized_keys` or the home directory is writable by group or others, or owned by someone else; its journal says so (`journalctl -u sshd`). `sshd -T` prints the effective configuration.
- **Load.** `top -b -n 1 -o %CPU | head -20`, `ps -eo pid,user,pcpu,etime,cmd --sort=-pcpu | head`, `systemd-cgtop -b -n 1`.
