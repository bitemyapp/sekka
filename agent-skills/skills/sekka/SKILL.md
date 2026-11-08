---
name: sekka
description: Sekka is a NixOS distribution with the Aizome and Tatami desktops and the Yukimi system manager. Use this skill on a Sekka computer before changing the system or running anything that needs administrator (root) rights, such as installing software, enabling a NixOS service or option, editing /etc/nixos, updating, rolling back, restarting system services or reading root-only files. It shows how to get root through the desktop's own password dialog (polkit) instead of sudo, or instead of asking the person to type commands. It also explains how the configuration is laid out, how to apply changes safely, and how to diagnose problems.
---

# Sekka

Sekka is NixOS, set up by its installer, Genkan, and looked after by Yukimi.
The whole system is declared in `/etc/nixos` and built by Nix. Nothing in
`/nix/store` can be edited, and most of `/etc` is generated from the
configuration: a change made there by hand is lost, or refused.

```sh
test -e /etc/yukimi/system.json && grep -q sekka /etc/nixos/flake.nix && echo Sekka
```

Related skills: `yukimi` covers software, updates, earlier versions and disk
space. `aizome` covers Sekka's own desktop and `tatami` its Hyprland desktop.
`echo "$XDG_CURRENT_DESKTOP"` names the desktop in use: `Aizome`,
`Hyprland:Tatami`, `GNOME`, `KDE` or `XFCE`.

## Administrator rights: the password dialog

Never run `sudo`, and never ask the person to type a command into a terminal
because it needs root. Your shell has no terminal for sudo's password prompt,
and handing the work back is what the person wanted to avoid. Sekka's
desktops run a polkit authentication agent: Aizome's own dialog, Tatami's
hyprpolkitagent, or GNOME's, Plasma's or Xfce's. Only vanilla Hyprland has
none. `pkexec` asks that agent; the person types their password into the
dialog on their screen, and the command runs as root.

1. Tell the person in one sentence that a password dialog is about to
   appear, and what it will do. The dialog shows the command line, which
   says little to most people.
2. Run the command through pkexec:

   ```sh
   pkexec --disable-internal-agent COMMAND ARGS... </dev/null
   ```

   - `--disable-internal-agent`: fail at once, rather than wait for a
     password on a terminal that nobody will type into.
   - `</dev/null`: nothing waits for your input. Yukimi's helper takes
     the end of a piped input as "stop".
   - The command runs in root's environment: the system's `PATH` and
     `HOME=/root`, without your own variables or working directory. Use
     absolute paths for files, and pass the variables it needs with
     `pkexec --disable-internal-agent env NAME=value COMMAND…`.
3. Wait. The dialog stays up until the person answers. Use a long timeout,
   or run the command in the background, and don't kill it.
4. Each pkexec call asks again. Put several root steps into one:
   `pkexec --disable-internal-agent sh -c 'first && second' </dev/null`.
   Yukimi's helper is the exception: one password covers its next few
   minutes.

| pkexec exits with | Meaning | Then |
| --- | --- | --- |
| the command's own status | It ran as root | Carry on |
| 126 | The person dismissed the dialog | Stop and ask whether to go ahead. Never retry in a loop |
| 127 | Not authorized, or no dialog could appear | See pkexec's message, below |

- **"pkexec must be setuid root"**: your own sandbox blocks setuid programs.
  Run the same command outside the sandbox, if your harness allows that
  with the person's approval.
- **"No authentication agent found"**, or no dialog appears: there is no
  desktop session to ask in, for example over SSH, on a text console or in
  a container. Only then, ask the person to run the one exact command in
  their own terminal, and say why.

Never ask for the person's password, pipe it to `sudo -S`, or set up an
askpass program. Never read or print `/etc/nixos-secrets`.

## How the system is described

`/etc/nixos` belongs to root and is readable by everyone. It is not a Git
repository. The system is its flake's `nixosConfigurations.<hostname>`.

| File | What it holds | How it changes |
| --- | --- | --- |
| `flake.nix` | Sekka's flake, written by Genkan | Leave it: `sekka-update` rewrites it |
| `flake.lock` | Exact versions of Sekka, Nixpkgs and the applications | Updates only. Never run `nix flake update` here |
| `configuration.nix` | This computer: Sekka's `calamares.*` choices and ordinary NixOS options | Yours to edit, then apply (below) |
| `hardware-configuration.nix` | Disks, kernel modules, firmware | Leave it |
| `yukimi.nix` | Packages and programs installed for everyone with Yukimi | Only through Yukimi (`yukimi` skill) |

Sekka's options keep the installer's earlier name, `calamares`:
`calamares.applications` (catalog application ids), `calamares.desktops`,
`calamares.defaultDesktop`, `calamares.loginScreen`,
`calamares.bootSplash.enable`, `calamares.nvidia.enable`,
`calamares.tuning.enable`, `calamares.zswap.enable` and
`calamares.agentSkills.enable`. `/etc/yukimi/system.json` records which
Nixpkgs the system is built from, whether unfree packages are allowed, the
application catalog, and the choices `sekka-update --choose` accepts.

Read any option without root:

```sh
nix eval /etc/nixos#nixosConfigurations.$(hostname).config.services.openssh.enable
```

## Changing the system

Take the narrowest route:

| Goal | How | Root |
| --- | --- | --- |
| Use a tool once | `nix run nixpkgs#cowsay -- hi`, `nix shell nixpkgs#jq -c jq …` | No |
| A tool for this person only | `nix profile add` (`yukimi` skill) | No |
| Software for everyone, or a catalog application (Docker, Steam…) | `yukimi` skill | Dialog |
| A NixOS option or service | Edit `configuration.nix`, then `scripts/apply-configuration` | Dialog |
| Update Sekka | `sekka-update` | Dialog |
| Desktops, login screen, boot splash | `sekka-update --choose SETTING=VALUE` | Dialog |
| Undo a change | An earlier generation (`yukimi` skill) | Dialog |

`nixpkgs#` is Determinate Nix's weekly Nixpkgs, which is fine for running a
tool; the system itself is built from the Nixpkgs that Sekka pins. `jq` and
`python3` aren't installed by default: reach them through `nix shell` or
`nix run` as above.

### NixOS options and services

`scripts/apply-configuration`, in this skill's directory, applies edited
copies of files in `/etc/nixos` the way Yukimi applies its own changes:

```sh
work=$(mktemp -d)
cp /etc/nixos/configuration.nix "$work/configuration.nix"
# edit "$work/configuration.nix"
scripts/apply-configuration configuration.nix="$work/configuration.nix"
```

It shows the changes, freezes the original and edited files in the Nix
store, and builds that exact edited snapshot as you, without root. Then it
asks once through the dialog for Yukimi's helper to apply it. Under its
operation lock, the helper checks that `/etc/nixos` still matches the
original snapshot and saves the previous generation before installing any
edits. If another change finished meanwhile, it refuses the stale plan;
read the current files and prepare the edit again. The helper switches to
the new system at once when that is safe.
Otherwise it prepares the system for the next start, and Yukimi and Settei
offer the restart. Failures before activation restore the original files;
after activation begins, follow the helper's reported outcome.

- `--check` stops after the build. With no files named, it applies
  `/etc/nixos` as it is.
- Snapshots contain regular files, not symlinks. Only `.nix` modules can
  change; the flake, hardware configuration and `yukimi.nix` stay managed
  by Sekka and Yukimi.
- A new module goes in beside the configuration, with its name added to
  `imports`: `configuration.nix=… my-module.nix=…`.
- Edit narrowly. Add options inside the top-level attribute set, keep the
  `calamares.*` lines and the existing `imports`, and prefer NixOS options
  (`services.…`, `programs.…`, `virtualisation.…`) to imperative setup.
  Look options up with `nix eval` as above, or at https://search.nixos.org/options.
- It takes minutes whenever something must be built or downloaded. Run it in
  the background or with your longest timeout, and wait. Interrupting it
  by signalling the helper before activation cancels the change and
  restores the files; once activation has begun, it finishes regardless.
  If the agent loses contact, check `sekka-update --status` and the operation
  log before retrying. Never restore configuration files independently of
  the helper.

Don't use `nixos-rebuild` on Sekka. It needs sudo, and it skips Yukimi's
side of a change: the free-space check, putting files back after a failed
build, the record behind History, and the choice between switching now and
at the next start. When a live switch would replace the login screen or
D-Bus under the running desktop, Sekka refuses `nixos-rebuild switch`.
Yukimi prepares that change for the next start instead.

### Updating Sekka

`sekka-update` brings the newest nightly: NixOS, the applications and the AI
tools together, at the versions that nightly built and tested. It asks
through the dialog, prints its progress, and ends by saying whether a
restart or sign-out is needed. It never restarts the computer itself, and
you mustn't either; tell the person instead. Run it in the background: it
can take a long time.

- `sekka-update --status` says whether a prepared change is waiting for a
  restart.
- `sekka-update --early` follows Sekka's `main` branch, before the nightly
  checks; what isn't cached yet builds here. `--nightly` goes back.
- `sekka-update --choose calamares.desktops=aizome,tatami`,
  `--choose calamares.defaultDesktop=aizome`, `--choose calamares.loginScreen=auto`
  or `--choose calamares.bootSplash.enable=true` changes the system's
  choices as part of an update. The values allowed are in the `choices` of
  `/etc/yukimi/system.json`.

## Working on NixOS

- Programs built for other distributions usually run: nix-ld provides the
  usual loader and common libraries for downloaded binaries, such as
  language servers and rustup toolchains.
- Scripts need `#!/usr/bin/env bash`; `/bin/bash` doesn't exist. `/bin/sh`
  and `/usr/bin/env` do.
- There's no global `pip install`, `npm install -g` into `/usr`, or
  `make install`. Use a virtual environment, `nix shell`, or a project's
  `flake.nix` with `nix develop`. Compilers and headers come the same way
  (`nix shell nixpkgs#gcc nixpkgs#pkg-config nixpkgs#openssl`), or from the
  catalog's `build-tools` application.
- Services: `systemctl status NAME` and `journalctl -b -u NAME` for the
  system's, and `systemctl --user` and `journalctl --user` for the person's
  own. Members of `wheel` can read the whole journal without root. A unit's
  definition lives in the configuration: change it there, not in
  `/etc/systemd`.
- Docker is the catalog's `docker` application: rootless Docker, which needs
  no root once it's installed.

## When something goes wrong

- `journalctl -b -p warning` shows this boot's warnings and errors, and
  `journalctl -b -1` the previous boot.
- After a change that broke something, choose an earlier generation in
  Yukimi's History, or from the boot menu (`yukimi` skill).
- "Another Yukimi operation is running": Yukimi, Settei or `sekka-update` is
  already changing the system. Wait for it to finish and try again.
- If the disk is full, see the `yukimi` skill's Storage section.
- Sekka's bugs go to https://github.com/bitemyapp/sekka/issues, with what was
  tried, what happened and `nixos-version`. Check logs for personal
  information before sharing them.

## These skills

Sekka keeps its skills in `~/.agents/skills`, with links in
`~/.claude/skills`. They update with the system unless they were changed
or removed, and then they are left as they are. `sekka-agent-skills status` lists them,
`sekka-agent-skills diff` shows what changed, and `sekka-agent-skills restore`
brings back Sekka's versions, keeping a copy of the changed ones.

Disabling `calamares.agentSkills.enable` removes Sekka's managed copies and
links at each user's next login or system switch. Edited copies are backed
up outside the skill folders; unrelated skills are left alone. The
`sekka-agent-skills` command remains available to inspect or restore them.
