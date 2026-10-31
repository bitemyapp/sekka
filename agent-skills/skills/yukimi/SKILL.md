---
name: yukimi
description: Yukimi is Sekka's system manager for software, updates, earlier versions (generations) and disk space. Use this skill on a Sekka computer to install or remove software for everyone, or a catalog application such as Docker, Steam or an AI tool. It also covers updating, listing or comparing generations, going back to one, checking whether a restart is pending, and freeing space in the Nix store. It covers both Yukimi's window and the commands that do the same work from a shell, with root granted through the desktop's password dialog.
---

# Yukimi

Yukimi (`yukimi`; Software in Aizome's Toolchest) shows what is installed and
why, finds software, updates the system, goes back to earlier versions and
frees space. Everything it does as root goes through one privileged program,
`yukimi-helper`, which pkexec starts after the person types their password
into the desktop's dialog. The helper checks its own arguments. It builds
before switching, and if the build fails it puts the configuration back.
It keeps a copy of the configuration behind each generation, and a lock
stops two changes from running at once. The window, `sekka-update` and the
commands below all use it. One password covers the helper's next few
minutes. The `sekka` skill explains pkexec and the dialog.

When the person would rather click, open Yukimi and say where to go, for
example: Discover, search "gimp", Install for everyone.

## Where software goes

| For | How | Root |
| --- | --- | --- |
| Just this person | `nix profile add "path:$nixpkgs#NAME"` (below), `nix profile remove NAME`, `nix profile list` | No |
| Everyone: a Nixpkgs package | `scripts/software add NAME`, into `/etc/nixos/yukimi.nix` | Dialog |
| Everyone: a catalog application | `scripts/software add-app ID`, into `calamares.applications` | Dialog |
| Listed by hand in `configuration.nix` | Edit it and apply (`sekka` skill) | Dialog |

For one person, install from the Nixpkgs the system is built from, as
Yukimi does. That way the package matches the system and comes from the
binary cache:

```sh
nixpkgs=$(nix eval --raw --impure --expr '(builtins.fromJSON (builtins.unsafeDiscardStringContext (builtins.readFile /etc/yukimi/system.json))).nixpkgs')
nix profile add "path:$nixpkgs#ripgrep"
```

An unfree package also needs `NIXPKGS_ALLOW_UNFREE=1` and `--impure`. Plain
`nixpkgs#` is Determinate Nix's weekly Nixpkgs, which is newer than the
system's and isn't pinned.

Use the catalog when it has the application. Some catalog applications need
more than a package: Steam needs its 32-bit graphics drivers, and Docker its
rootless daemon. Those can only be installed for everyone.

To find a package's attribute name, run `nix search nixpkgs WORD`. The first
search downloads an index, and the name follows `legacyPackages.x86_64-linux.`.
https://search.nixos.org/packages also works. Unfree packages are allowed
unless `allowUnfree` in `/etc/yukimi/system.json` is false.

### scripts/software

In this skill's directory:

```sh
scripts/software list              # what is installed for everyone, by where it is listed
scripts/software catalog           # the catalog: id, name and description
scripts/software add NAME...       # Nixpkgs packages for everyone
scripts/software remove NAME...
scripts/software add-app ID...     # catalog applications
scripts/software remove-app ID...
```

It checks the names before anyone is asked for a password: each package
must evaluate in the system's own Nixpkgs, and each application id must be
in the catalog. Then it gives the helper an add/remove request. The helper
reads and
changes the current list under its operation lock, so another installation
completed during the password dialog is preserved. The helper switches to the
result at once when that is safe, or prepares it for the next start. Run it
in the background or with a long timeout, and don't interrupt it.

## Updates

- `sekka-update` updates Sekka, which moves NixOS, the applications and the AI
  tools together (`sekka` skill). Yukimi's Updates page does the same with
  Update.
- `sekka-update --status` says whether an update or change is waiting for a
  restart or sign-out. Tell the person; never restart for them.
- Sources the person has pointed elsewhere have rows of their own on the
  Updates page and update separately. Leave those to the window unless the
  person asks.

## Earlier versions

Each change makes a new generation of the system, and the boot menu lists
them. To list them without root:

```sh
for link in /nix/var/nix/profiles/system-*-link; do
  n=${link##*/system-}
  printf '%s\t%s\t%s\n' "${n%-link}" "$(stat -c %y "$link" | cut -d. -f1)" "$(cat "$link/nixos-version")"
done | sort -n
readlink /nix/var/nix/profiles/system   # the newest
readlink /run/current-system            # the running one
```

To see what changed between two of them:
`nix store diff-closures /nix/var/nix/profiles/system-41-link /nix/var/nix/profiles/system-42-link`.

To go back to one, use History in Yukimi or the helper:

```sh
pkexec --disable-internal-agent "$(readlink -f /run/current-system/sw/bin/yukimi-helper)" rollback 41 </dev/null
```

Going back also restores the configuration that generation was built from,
except for files edited by hand since. Like any change, it applies at once
when that is safe and otherwise at the next start. Ask before going back:
it undoes everything changed since that generation.

## Storage

- To see what takes the space: Yukimi's Storage page, `df -h /nix`, and
  `nix path-info -Sh /run/current-system` for the running system's size.
- Nix collects garbage by itself when less than 5 GB is free, and updates
  stop below 4 GB before they fill the disk.
- To free store paths nothing uses (no root needed):
  `nix-collect-garbage`. The person's own old profile versions go with
  `nix profile wipe-history --older-than 30d`.
- To remove system generations older than some days and then collect,
  ask first: those generations are the way back.

  ```sh
  pkexec --disable-internal-agent "$(readlink -f /run/current-system/sw/bin/yukimi-helper)" clean --older-than-days 30 </dev/null
  ```

  Without `--older-than-days` it only collects garbage, then updates the
  boot menu.

## The helper, directly

```sh
pkexec --disable-internal-agent "$(readlink -f /run/current-system/sw/bin/yukimi-helper)" ARGUMENTS </dev/null
```

- `readlink -f`: polkit knows the helper by its real path. That gives
  Yukimi's own prompt and the remembered password.
- `</dev/null`: if its input is a pipe and the pipe closes, the helper stops
  the operation and restores the configuration.
- Output: one JSON line on standard output, `{"ok":true,…}` or
  `{"ok":false,"error":"…"}`, and progress on standard error as Nix's
  `@nix {…}` log lines.

| Arguments | Does |
| --- | --- |
| `change --mode auto` | Builds and applies `/etc/nixos` as it is (`auto`: live when safe, else the next start; also `switch`, `boot`) |
| `change --add-packages a,b --mode auto` | Adds packages to the current `yukimi.nix` list under the operation lock |
| `change --remove-packages a,b --mode auto` | Removes only these packages from `yukimi.nix` |
| `change --setting calamares.applications --add-applications a,b --mode auto` | Adds catalog applications under the operation lock; `--remove-applications` removes them |
| `change --packages a,b --mode auto` | Sets the whole package list of `yukimi.nix` |
| `change --programs a,b --mode auto` | Sets `yukimi.nix`'s `programs.<name>.enable` list |
| `change --setting calamares.applications --applications a,b --mode auto` | Sets the whole catalog list |
| `change --unlist-system ATTR --mode auto` | Takes one entry out of `environment.systemPackages` in `configuration.nix` |
| `change --unlist-user USER:ATTR --mode auto` | Takes one entry out of `users.users.USER.packages` |
| `change --sekka --mode auto` | What `sekka-update` does |
| `change --choose SETTING=VALUE[,VALUE] --mode auto` | One of the system's declared choices (`sekka-update --choose`) |
| `rollback N` | Returns to generation N |
| `clean [--older-than-days N]` | Deletes older generations, then collects garbage |

The replacement-list options are for deliberately replacing an entire list;
use add/remove options for individual software changes.

If it fails with "Another Yukimi operation is running", wait for the other
change to finish. Don't remove the lock.
