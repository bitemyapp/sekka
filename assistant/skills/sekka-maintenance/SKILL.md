---
name: sekka-maintenance
description: "Everyday upkeep with exact commands and what needs root — updating, installing and removing software (Yukimi, nix profile, /etc/nixos), freeing disk space (store cleanup, generations, journal, Kura), services and logs, Wi-Fi (nmcli), sound (wpctl), Bluetooth, printers (CUPS), users and passwords, time zone, keyboard layout, language, host name, firewall, SSH, backups. Load for \"update my computer\", \"install X\", \"disk is full\", \"connect to Wi-Fi\", \"add a printer\", \"change the time zone\"."
---

# Maintaining Sekka

Commands below run as the desktop user unless marked **root** (prefix
`sudo`; the person types their password, never you). On Aizome a wheel
user at the computer is not asked for a password for time, time zone,
host pretty name, locale, keyboard and NetworkManager changes. Explain
what a command changes before running anything that isn't read-only.

## Updating

- GUI: Yukimi → Updates → **Update**. Terminal: `sekka-update` (asks for
  the administrator password itself; no sudo needed). Both fetch the nightly and
  rebuild, applying live when safe or at the next start.
- `sekka-update --status` — anything waiting for a restart or sign-out.
- `sekka-update --early` follows `main` (may build locally);
  `sekka-update --nightly` goes back.
- Never restarts on its own. If Yukimi says space is short (it stops below
  4 GB free), free space first (below).
- After an update a notification offers to reload Aizome; nothing is lost
  by reloading. A changed compositor core applies at the next login.
- Failed update: the running system is unchanged and the files the
  updater touched are put back. Read its message; Nisshi → Errors &
  Warnings around that time.

## Installing and removing software

| Want | How | Root |
|---|---|---|
| Try a program once | Yukimi → Discover → Try It | no |
| Install for this user | Yukimi → Discover → **Install for Me** (`nix profile install <system nixpkgs>#ATTR`) | no |
| Install for every user | Yukimi → Discover → **Install for Everyone** (adds the attribute to `/etc/nixos/yukimi.nix`, rebuilds) | password |
| Installer catalog apps (Firefox, Zed, Steam, AI tools…) | in `calamares.applications = [ … ];` in `/etc/nixos/configuration.nix`; Yukimi lists and removes them | rebuild |
| By hand | add to `environment.systemPackages = with pkgs; [ … ];` in configuration.nix, then `sekka-update` or `sudo nixos-rebuild switch` | yes |
| Remove | Yukimi → Installed → Remove from your profile / Remove for everyone | depends |

- Programs that need a NixOS module (Steam: 32-bit graphics) install only
  for everyone; Yukimi handles that.
- Unfree software needs `nixpkgs.config.allowUnfree = true` (Genkan's
  choice); Yukimi labels unfree packages.
- See what's in the user's profile: `nix profile list`.
- Flatpak is not set up by Sekka; don't assume it.

## Freeing disk space

Look first: `df -h / /home`, `df -i /`, Kura (`aizome-kura`, or
`aizome-kura hints` for suggestions), Yukimi → Storage (store split into
running system, old generations, user profiles, dev shells, garbage).

1. **Yukimi → Storage → Clean Up…**: deletes old system generations (you
   choose how old), collects garbage, updates the boot menu. Preferred.
   The same by hand (root), exactly what it does:
   `sudo nix-env -p /nix/var/nix/profiles/system --delete-generations 30d`
   `sudo nix-store --gc`
   `sudo /nix/var/nix/profiles/system/bin/switch-to-configuration boot`
   Only `sudo nix-store --gc` is risk-free (unreferenced paths only); deleting
   generations removes those rollback points.
2. Nix already collects unreferenced paths by itself when a build leaves
   under 5 GB free (until 20 GB is free); it never removes generations.
3. **Journal**: capped at 50 MB with Sekka's tuning
   (`journalctl --disk-usage`). Otherwise **root**:
   `sudo journalctl --vacuum-size=200M` or `--vacuum-time=2weeks`.
4. **Core dumps**: `/var/lib/systemd/coredump`, kept 3 days with tuning.
5. **User files**: Trash (`~/.local/share/Trash`; palette "Empty Trash"
   asks), `~/.cache`, old downloads. Kura's Collector moves them to the
   Trash after asking. Never delete documents, photos or backups to make
   room without the person choosing each item.
6. Space held by deleted-but-open files: `sudo lsof +L1`; restart the
   holder (with the person's agreement).

## Services and logs

- Nisshi (Logs): Errors & Warnings, Crashes, My Session; Unlock for the
  system journal. `aizome-nisshi --print --system --lines 50` prints.
- `systemctl --failed`, `systemctl --user --failed`
- `systemctl status UNIT --no-pager -l`, `journalctl -u UNIT -b --no-pager -n 100`
- User units: `systemctl --user status UNIT`, `journalctl --user -u UNIT -b`
- This boot's errors: `journalctl -b -p err --no-pager | tail -50`;
  previous boot: `journalctl -b -1 -p err`.
- Crashes: `coredumpctl list`, `coredumpctl info PID|NAME`.
- Restarting a system service is **root** (`sudo systemctl restart UNIT`)
  and needs a reason. A service defined by NixOS is changed in
  `/etc/nixos`, not in `/etc/systemd`.

## Network (NetworkManager)

- GUI: menu bar network panel (Wi-Fi switch, networks, password inline,
  VPNs, airplane mode) or Settings → Network (hidden networks, known
  networks/Forget, VPN import, hotspot, per-connection IPv4/IPv6/DNS/proxy,
  metered, private address). Passwords are typed by the person there.
- `nmcli general status`, `nmcli device status`, `nmcli device wifi list`,
  `nmcli connection show`, `nmcli connection up|down "NAME"`,
  `nmcli radio wifi on|off`, `nmcli device wifi rescan`.
- `nmcli connection delete "NAME"` forgets a network (ask first: it loses
  the saved password).
- Diagnose: `ip addr`, `ip route`, `getent hosts example.com`,
  `resolvconf -l`, `ping -c 3 1.1.1.1`, `curl -sS -o /dev/null -w '%{http_code}\n' https://example.com`.
- `nm-applet` doesn't run in Aizome; Aizome's agent
  (`aizome-network-agent.service`, user) asks for later passwords.

## Sound (PipeWire + WirePlumber)

- GUI: menu bar sound panel (volume, output device, input, media) or
  Settings → Sound (devices, balance, per-app volume, alert sound).
- `wpctl status` (lists sinks/sources with IDs; `*` marks the default)
- `wpctl set-default ID` — make an output/input the default
- `wpctl set-volume @DEFAULT_AUDIO_SINK@ 50%`, `wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle`
- `wpctl inspect @DEFAULT_AUDIO_SINK@`
- Units (user): `pipewire.socket`, `pipewire.service`,
  `pipewire-pulse.socket`, `pipewire-pulse.service`, `wireplumber.service`;
  `systemctl --user status pipewire wireplumber`.

## Bluetooth

Settings → Bluetooth (on/off, My Devices, Nearby Devices, pairing with a
code). Terminal: `bluetoothctl devices`, `bluetoothctl info MAC`,
`bluetoothctl connect MAC`. Radio blocked: `rfkill list`.

## Printers (CUPS)

Settings → Printers & Scanners → Add Printer (network printers are found
over mDNS; printers already set up are left out), default printer, test
page, queue. Terminal: `lpstat -p -d` (printers, default), `lpstat -o`
(jobs), `cancel JOB` (`cancel -a` all — ask first), `lpoptions -d NAME`
(default for this user).

## Users and passwords

- Settings → Users & Groups: your name, picture, password; other accounts
  (add, remove, administrator or standard), log in automatically.
  `users.mutableUsers = true`, so these changes persist.
- `passwd` changes your own password (the person types it). Never set or
  read a password for them.
- Removing an account (Users & Groups → Remove) offers "Also delete
  HOME and everything in it", off by default: leave it off unless the
  person explicitly wants that account's files gone.
- Declarative accounts: `users.users.NAME = { isNormalUser = true; extraGroups = [ "wheel" ]; };`
  in configuration.nix (wheel = administrator), then rebuild.

## Time, language, keyboard, host name

| Setting | Live (Aizome) | Durable system setting |
|---|---|---|
| Time zone | Settings → General → Date & Time, or `timedatectl set-timezone Asia/Tokyo` (`timedatectl list-timezones`) | owned by Settings on Aizome (`time.timeZone` in configuration.nix is overridden); persists across rebuilds |
| Clock format | Settings → Menu Bar → 24-hour clock | — |
| Automatic time | — | `services.timesyncd.enable` (on by default in NixOS) |
| Language | shown read-only in Settings | `i18n.defaultLocale = "de_DE.UTF-8";` + rebuild, next login |
| Keyboard layout | Settings → Keyboard (Aizome session only) | `services.xserver.xkb.layout = "de";` (login screen, console, other desktops) + rebuild |
| Computer name | Settings → General → About → Computer name (pretty name) | — |
| Host name | — | `networking.hostName` + rebuild; also rename `nixosConfigurations."OLD"` in `/etc/nixos/flake.nix` to match, or `nixos-rebuild` looks for the new name and fails |

Don't set the zone with `ln -sf … /etc/localtime`: a running timedated
keeps reporting the old one. Use `timedatectl`.

## Firewall, SSH

- NixOS's firewall is on by default. Open a port durably:
  `networking.firewall.allowedTCPPorts = [ 8080 ];` + rebuild. Never
  flush the ruleset; remove only the rule at fault.
- SSH server exists only if chosen at install (`services.openssh` in
  configuration.nix; key-only when GitHub keys were imported).
  `~/.ssh` must be 0700 and `authorized_keys` 0600, owned by the user.

## Power, sleep, displays

- Settings → Battery & Power: power mode, screen off, lock after, power
  key. Lid behaviour is logind's, from the NixOS configuration.
- Keep Awake: palette "Keep Awake" or `aizome-awake on|off|status`.
- Displays: Settings → Displays (15 s automatic revert for risky
  changes); `aizomectl outputs` lists them.

## Backup hygiene

Sekka sets up no backups, and generations restore the system, not files.
Suggest the person keep copies of `~/Documents`, `~/Notes`, `~/Pictures`,
`~/.ssh` and `~/.config/aizome` on another disk or service of their
choosing, and verify a restore. `/etc/nixos` is small and worth keeping
too (it rebuilds the whole system). Never delete or overwrite an existing
backup to free space or "tidy".
