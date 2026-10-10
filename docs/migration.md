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
