# Sekka monorepo migration

The October 2026 migration brings the distro's first-party components into one
repository. [imports.json](imports.json) records the exact starting revisions
and imported paths. These are source snapshots, not rewritten copies of every
predecessor Git history. The original repositories and their histories remain
available; the local cleanup also retained full Git bundles and dirty patches.

| Predecessor | Included in Sekka | Reason |
| --- | --- | --- |
| determinate-nixos-graphical | Root flake, live image, templates, image/VM tooling and evidence | Owns image assembly and installation integration |
| calamares | `installer/genkan`: the NixOS Rust installer, helper, modules and test fixture | Its changes frequently need coordinated image and desktop changes |
| tatami | `desktop/tatami` | First-party desktop, helper, Displays app, settings and attributed wallpapers |
| yukimi | `apps/yukimi` | First-party package/system manager and its NixOS integration |

The upstream C++ Calamares application, distribution modules unrelated to this
Rust installer, and old upstream CI were not imported. Sidestep, Upleft and
Omperor remain separate products. Omarchy/Arch packaging and upstream forks
such as aquamarine and spectacle remain external dependencies or upstream
development repositories. Their branches and working trees were not changed.

## Recovered work and branch cleanup

- Kept the `zed` command wrapper, including correct forwarding of arguments,
  and `file` in the development tools selection. These were squash-merged in
  [calamares#2](https://github.com/bitemyapp/calamares/pull/2) and imported here.
- Kept `file` in both the live image and installed-system defaults. The source
  cleanup was squash-merged in
  [determinate-nixos-graphical#2](https://github.com/bitemyapp/determinate-nixos-graphical/pull/2).
  Sekka supplies the installed default through its shared system module.
- Discarded the stale wallpaper override in the old dirty checkout. It tried
  to replace the KDE package's wallpapers through old remote URLs; the current
  shared, attributed wallpaper setup already superseded it.
- Removed completed branches and duplicate integration branches. The two old
  filesystem/application draft PRs were closed because their head commits were
  already ancestors of the retained source, not because their work was dropped.
- The four predecessor repositories use `main` as their default and only
  remaining branch. Secondary local checkouts are detached at the retained
  `main` commit; primary checkouts track `origin/main`.
- Former `stable` heads remain as fixed tags so existing flake URLs still
  resolve. They no longer form an update channel. New work belongs in Sekka on
  topic branches from `main`; squash merges delete their topic branches.

## Integration changes

Sekka has one external dependency lock. The three remote first-party flake
inputs were replaced with local source adapters. Separate Cargo workspaces and
GUI/helper privilege boundaries remain intact. Genkan has its own name in the
interface; legacy executable names, desktop IDs, and `calamares.*` Nix options
remain compatible.

The installed-system flake adds a pinned `sekka` input and follows its external
dependencies explicitly. Its generated lock records the exact Git revision
for clean builds, or the exact store source for dirty development builds.
Both Genkan's parser and the image tooling validate the new input set. VM
fixtures build from local source. Configuration matrix checks no longer reuse
the old cross-repository revision cache.

Existing machines are not automatically migrated by this repository change.
See [Genkan's installed-system notes](../installer/genkan/RUST-INSTALLER.md#updating-installed-systems)
before changing an existing `/etc/nixos`.

## Validation and limits

Migration validation covers component builds and their unit tests, flake
evaluation, the installed template's actual backend validator, evaluation of
representative installed desktop configurations without a lock update, and
the real filesystem reformat matrix in a disposable VM. Release packaging
tests verify boundary sizes, checksums and exact reconstruction of split
images; actionlint and shellcheck inspect the inhibited workflow.

Genkan and Yukimi were rendered in a disposable NixOS VM for the README. See
[screenshots.md](screenshots.md) for capture details. Historical image and
hardware test reports remain historical evidence.

This migration does **not** claim a complete installation/reboot test of a new
Sekka ISO or validation on physical hardware. No ISO release was published.
The release workflow's sole job is unconditionally disabled until an explicit
reviewed change enables it. See [DEVELOPERS.md](../DEVELOPERS.md) for the checks
and release requirements.

## Aizome

Aizome joined Sekka in October 2026, after the first migration. Until then
Sekka took it from [bitemyapp/aizome](https://github.com/bitemyapp/aizome) as
a flake input. A change to Aizome reached Sekka's `main` only through a second
pull request that moved the pin, and the nightly built whatever Aizome's `main`
held that night. Its source is now in `desktop/aizome`, a snapshot like the
others. [imports.json](imports.json) records the revision, and bitemyapp/aizome
keeps the history.

The import includes Aizome main through `1a50f8e`: input-method keyboard
grabs, independent Desks and swapping, docked lid removal, the Desk underline
notification, and sandboxed applications surviving shell reloads. Sekka's
keybinding/IPC launch patch is folded into the local source.

- Aizome's flake became `desktop/aizome/nix/default.nix`. It gives the same
  packages, NixOS module, VM checks and development shell, built with Sekka's
  Nixpkgs, as Aizome's input did with `inputs.nixpkgs.follows`. The flake's
  own `flake.nix` and `flake.lock` were not imported: Sekka's lock is the only
  one.
- Sidestep, which the workspace builds against by path, remains a separate
  product. It is now Sekka's `sidestep` input, at the revision Aizome's lock
  pinned. `flake.nix` names that revision, so the nightly's input update
  doesn't move it.
- The installed flake's lock loses its nested `aizome` node and keeps
  `sidestep` beneath Sekka. The ISO qualification report no longer records
  an Aizome revision: the Sekka revision includes Aizome's source.
- Native Yukimi uses the existing `apps/yukimi/crates` backend instead of
  retaining a copied backend inside Aizome. Aizome's additions are preserved
  there; the GTK and Aizome Cargo workspaces and package boundaries remain
  separate. Aizome's filtered package source includes only the shared backend
  and its workspace metadata, not Yukimi's GTK interface or privileged helper.
