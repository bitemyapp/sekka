# Developing Sekka

Sekka is the source home for the ISO, Genkan installer, Tatami desktop, and Yukimi system manager. Work on a short-lived topic branch from `main`, open a pull request, squash-merge it, and delete the topic branch. There is no `stable`, `develop`, or release branch. Releases identify a commit with a version tag.

## Repository map

| Path | Responsibility |
| --- | --- |
| `flake.nix`, `flake.lock` | Public outputs and the authoritative external dependency lock |
| `installer/genkan/` | Rust installer, privileged helper, system modules, application catalog, storage tests |
| `desktop/tatami/` | Desktop module, configuration, Rust commands, Displays app, wallpapers |
| `apps/yukimi/` | GTK application, privileged helper, store/configuration/system libraries |
| `modules/`, `templates/` | Live ISO and the installed-system flake template |
| `nix/` | Component integration, packages, application resolution, prebuilt systems |
| `rust/`, `scripts/` | Image, VM, diagnostics, USB and release tooling |
| `site/` | The sekka.computer website generator |
| `tests/` | Generated-system configuration matrices and integration checks |
| `docs/` | Architecture, migration notes, screenshots, historical evidence, brand |

The initial import preserves component layouts and separate Cargo workspaces. There are no first-party submodules or remote first-party flake inputs. `nix/components.nix` adapts the local components to the existing integration interface; its `calamares` name and the `calamares.*` options are compatibility names, not another source repository.

Sidestep, Upleft, Omperor, upstream forks, and Arch/Omarchy packaging stay outside Sekka. Add package definitions and pinned dependencies when the distro needs them.

## Build and check

Use x86_64 Linux with Nix and flakes enabled. The full image includes several desktops and an application cache: allow substantial disk space (a dedicated builder with at least 250 GiB free and 32 GiB RAM is a useful starting point). Component builds are much smaller. Never run installer storage tests against host disks.

```sh
nix develop
nix flake check --no-build --no-update-lock-file
nix build .#genkan .#tatami .#displays .#network .#yukimi .#tools
nix build .#checks.x86_64-linux.storage
nix flake check --no-update-lock-file
nix build .#iso --no-update-lock-file --out-link result-iso
```

The ISO is under `result-iso/iso/`. `nix build .#installed-lock` exposes the lock shipped to the installed machine. `nix fmt` formats Nix files supplied to it.

The [migration record](docs/migration.md) explains retained work, exclusions,
compatibility tags and the limits of the migration validation. To exercise the
generated installed flake directly, build the fixture and run the template test:

```sh
cargo build --locked --manifest-path installer/genkan/tests/vm-fixture/Cargo.toml --target-dir .work/fixture-target
python3 tests/check_installed_template.py
python3 -m unittest discover -s tests -p test_release.py
```

For focused Rust work, run Cargo inside the development shell:

```sh
cargo test --locked --manifest-path installer/genkan/rust/Cargo.toml
cargo test --locked --manifest-path desktop/tatami/tool/Cargo.toml
cargo test --locked --manifest-path desktop/tatami/displays/Cargo.toml
cargo test --locked --manifest-path desktop/tatami/network/Cargo.toml
cargo test --locked --manifest-path apps/yukimi/Cargo.toml
cargo test --locked --manifest-path rust/Cargo.toml
```

Do not merge unrelated Cargo workspaces or broaden package source filters as part of routine edits. Preserve Genkan and Yukimi's unprivileged GUI/privileged helper boundaries. Keep slow I/O off the interface thread.

The retained VM tooling is available through `nix build .#tools`. Run `result/bin/respin-tools qemu-test --help` for its firmware, storage, desktop, and application options. The Genkan fixture now builds from `installer/genkan/tests/vm-fixture`, not a remote installer revision. The rootless builder's static guest tools require the Rust musl target. The older reports in `TESTING.md`, `installer/genkan/RUST-TESTING.md`, and `docs/test-results/` describe their named predecessor revisions only.

Before publishing an installable image, exercise the exact image through a disposable UEFI and BIOS install, disk-only reboot, desktop login, and Yukimi configuration/update checks. Record the ISO checksum, source revision, results and outstanding hardware limitations. Passing component tests alone is insufficient evidence of an end-to-end installation.

## Sources and installed-system updates

The root lock retains distinct base-system and application Nixpkgs inputs, plus Determinate Nix, FlakeHub CLI, AI applications, and upstream oh-my-pi. First-party code comes directly from this checkout.

Sekka backports Aquamarine's upstream null-connector teardown fix
[`fc77bd58`](https://github.com/hyprwm/aquamarine/commit/fc77bd58a2c9ef6d0760a7c9c3b7b6670f956f28)
onto Nixpkgs' 0.15.1 through `nix/overlays/aquamarine.nix`. Both Tatami and
vanilla Hyprland import the shared module, including Genkan's prebuilt installed
systems. The overlay also rebuilds Hyprland against the patched library; updating
an unrelated application package set alone would not change that runtime dependency.
`nix build .#checks.x86_64-linux.aquamarine` exercises the real library with null
connectors, confirms the original library crashes on the same test, and verifies
Hyprland's runtime linkage. It needs no GPU or running graphical session.
`nix build .#checks.x86_64-linux.hyprland-packages` checks package selection
through the installed-system, ISO reference and standalone desktop modules.
The backport keeps the 0.15.1 version string; its distinct Nix store path
identifies the patched build.
When Nixpkgs includes the fix, remove the backport and its negative control after
verifying the regression test passes with the new upstream package. A patch
failure during an input update requires that review; do not just skip the patch.

Tatami's Displays helper sends explicit `disabled = false` and `mirror = ""`
when enabling an independent screen: Hyprland's Lua monitor API merges omitted
fields with an earlier rule of the same literal `output`, and of several rules
matching a monitor the last one added wins outright. Mirror IDs from `hyprctl`
are resolved to connector names before the settings window submits another
change. The helper checks the resulting enabled/mirror state rather than
trusting an IPC acknowledgement alone. Monitor, lid and Night Shift reads and
all changes run off the GTK thread, with concurrent changes disabled until the
result arrives and stale reads discarded.

`tatami-display-recovery.service` (`tatami displays watch`) runs with the
desktop, independently of Displays, and carries out `display_policy.rs`:

- **Closed-display mode.** With the lid closed and another physical screen
  connected, built-in panels are turned off, after another screen is confirmed
  on. Opening the lid turns back on what the lid turned off (recorded by
  connector), with its kept settings. Undocked, closing the lid blanks the
  panel with DPMS once (logind normally suspends; Keep Awake can hold that
  off), and opening it wakes the screens. A panel turned on behind a closed
  lid because nothing else is connected is blanked the same way. Tatami does
  not change logind: suspending, or not, on lid close is still
  `HandleLidSwitch*`. The default `HandleLidSwitchDocked=ignore` keeps a docked
  laptop running, unlike macOS on battery power.
- **Remembered choices.** A display turned off in Displays or with
  Super+Ctrl+Delete is recorded against the set of connected displays (their
  `desc:` selectors), and turned off again whenever that set is connected again.
  The record lives in `$XDG_RUNTIME_DIR/tatami/displays-session.json` and ends
  with the session, so a new login starts with every connected display on. A
  session from before the record is started from `monitors-off`, read before
  any change rewrites it.
- **A screen always on.** If no connected physical screen would be on, the built-in
  panel (an external one while the lid is closed, then the main display) is
  restored with its kept settings, then its preferred mode, then each connected
  screen's preferred mode. Its stale mirror is cleared in the store; its
  arranged position is kept.
- **Mirrors.** A display mirroring one that is about to go is switched to its
  own picture first, and that is confirmed before the source is turned off:
  Hyprland 0.56 ends mirrors while iterating them in `onDisconnect`.

The lid reaches the watcher as `custom>>tatami-lid-closed`/`-open` socket events
from Tatami's `switch:on/off:Lid Switch` bindings (locked, universal across
submaps, not inhibitable). It also polls logind's `LidClosed` every two seconds
for switches named otherwise, with ACPI's report as the fallback. A poll
changes the lid only when logind's answer changes, so hardware whose lid only
libinput reports keeps the bindings' state. `tatami session` applies
closed-display mode before Hyprland starts, from logind and the connected DRM
connectors, so a docked login with the lid closed never lights the panel.

Hyprland 0.56 moves floating windows by the change in monitor origin when
their workspace changes monitor, and places orphaned workspaces modulo the
new monitor's size without its position (`WorkspacePlacementController.cpp`).
Unplugging the only lit display, or replugging one whose position is still
unset, can leave windows off every screen. For three seconds after each change
in the screen layout, the watcher centers floating windows that no longer
overlap their own monitor; at other times applications may place windows
where they like. `tatami display` (also the launcher entry) brings an open Displays
window to the focused screen's workspace, centered. It replaces an instance
whose main loop does not answer D-Bus, or one without a window.

The watcher and display changes use Hyprland's command socket directly, with
a three-second deadline, rather than spawning `hyprctl`. They reconcile on
monitor and lid events and every second; normal DPMS blanking does not trigger
changes. Display changes share a bounded lock, and systemd reconnects a failed
watcher.

`nix build .#checks.x86_64-linux.display-recovery` exercises the packaged helper
in isolated namespaces against a fake Hyprland socket and a fake logind lid,
including lost events, stuck IPC, duplicate monitors, acknowledged changes
which never take effect, closed-display mode, remembered choices, mirrors,
window rescue and login preparation.
`nix build .#checks.x86_64-linux.display-recovery-vm` runs actual Hyprland in a
disposable VM and checks enabling, mirroring, extending, reloads, repeated
removal of a simulated external output, and a uinput lid switch seen by both
logind and Hyprland. That includes closed-display mode through a reload,
undocking with the lid closed, a Displays window stranded by an unplug, and
`tatami display` from another workspace. These tests do not replace physical
dock, cable, panel, lid sensor and GPU validation.

`nix/installed-lock.nix` extends that external lock with Sekka itself. A clean Git build pins its commit and source hash; a dirty development build pins the exact source copied into the Nix store. The source is included on the ISO. The installed flake follows `github:bitemyapp/sekka/main` for subsequent updates, with all external inputs explicitly following the installed system's top-level inputs.

Update the distro through **Yukimi → Updates → Update** or `sekka-update`.
Both use the privileged helper transaction: recognize/migrate the flake,
back up the configuration, update the selected inputs, and build the host once.
The ordinary GUI action includes Sekka and sources with available newer revisions;
the CLI updates only Sekka. Individual source rows retain their controls.
The default `auto` mode compares boot artifacts, kernel parameters, startup
compatibility, the declared display/graphics/sleep-manager identity, effective
session-critical systemd units (including aliases and drop-ins), and NixOS
switch inhibitors before choosing `switch` or `boot`. Configuration changes,
branch changes, and rollbacks use the same planner. A missing compatibility
manifest stages the transition for boot. D-Bus catalog trigger changes remain
live when both unit definitions specify reload-on-change and every other unit
setting matches; changes to its executable or reload policy still stage for boot.
`sekka-update --boot` explicitly
stages the update; `--switch` requests live activation but refuses an unsafe
transition before committing configuration edits. A target-side NixOS
pre-switch check also rejects unsafe activation from older updaters and other
rebuild frontends, before they can stop the desktop. Do not copy the root lock verbatim
into an installed machine: it lacks the Sekka input until the installed lock
is generated.

For installations without the new updater, `nix run github:bitemyapp/sekka/main#update`
bootstraps it without installing a user profile package. The CLI uses the
helper beside its own executable, including when run from the Nix store;
polkit asks for administrator authentication. `--print-flake` prints the
proposed configuration without mutation or authentication.

Migration recognizes the predecessor installer's explicit input declarations
and replaces only the three first-party inputs and the module reference.
It preserves external URLs and locks, configuration modules, user choices,
hardware settings, and `system.stateVersion`. Unrecognized customizations
require manual migration. Failed builds and cancellation before activation
restore the touched files. A helper process lock serializes Yukimi changes;
configuration snapshots under `/var/lib/yukimi/configurations` accompany
generations, including `flake.nix` for rollback across migration. Activation
is allowed to finish once started; it never initiates a reboot.

The authenticated helper launches the entire operation in an independent systemd
service. The client follows progress from regular files, rather than connecting
activation's output to a GUI-owned pipe. Closing Yukimi requests cancellation
before commit; after commit the worker finishes even if the client is killed.
Root-only logs and atomic results live under `/var/lib/yukimi/operations`, with
30-day tmpfiles retention. The worker holds the transaction lock until it has
finished; losing its client does not release the lock.

`/var/lib/yukimi/update.json` records the built target and activation outcome,
written atomically before and after activation. A failure cannot be mistaken
for a ready boot just because the system profile moved. Completion is recomputed
from selected, active and booted systems, rather than a sticky reboot flag.
Booting the prepared generation clears its request. `sekka-update --status --json`
exposes the same assessment used by the GUI. The GUI has one persistent completion
row, with reboot taking precedence over sign-out; it does not repeatedly pop up
dialogs or require Yukimi to autostart.

Tatami records its loaded component identities at session launch in
`$XDG_RUNTIME_DIR/tatami/update-session.json`. An unprivileged frontend invokes
`tatami update-session` after a verified live switch. Safe configuration reloads
advance only the fingerprints actually applied. Compositor/session environment
changes wait for login, and failed reloads remain explicitly unverified.
Unchanged pinned Hyprland configuration does not request sign-out. Active Keep
Awake holders survive both NixOS live activation and appearance reloads; their
implementation advances at a fresh session without dropping protection mid-update.
Session verification checks the boot, the active UWSM compositor service and its
session environment file, and (for new receipts) the service invocation ID.
UWSM deliberately omits `XDG_SESSION_ID` from the shared application environment;
its absence in Yukimi is not evidence of a different login session.

```sh
nix build .#checks.x86_64-linux.update-completion
python3 tests/test_tatami_update.py /nix/store/…-tatami/bin/tatami -v
```

The completion VM exercises the helper under real systemd, kills its client during
live activation, verifies persistent results and a surviving graphical session,
rejects a display-manager transition before service shutdown, and stages a rollback
with changed boot parameters. It boots that prepared generation through UEFI,
verifies that the restart request disappears, and captures the GUI. Its desktop
is IceWM; it does not reproduce physical GPU teardown or claim Hyprland hardware coverage.
Namespace fixtures test helper transactions and Tatami reload failures without
changing the host desktop.

`python3 tests/test_sekka_update.py result/bin -v` tests the packaged CLI and
helper in a rootless bubblewrap namespace. It covers migration, repeat updates,
failed builds, cancellation, concurrent operations, closed progress pipes, unsafe
explicit switches and rollbacks, and UWSM session identity. Nix and
systemd actions are stubbed; also evaluate/build a copy of the installed host
flake to check actual module and dependency compatibility before deployment.

Standalone users can import `sekka.nixosModules.tatami` with `programs.tatami.enable = true`, or `sekka.nixosModules.yukimi` with `programs.yukimi.enable = true`, without enabling the whole distro. Set `inputs.sekka.inputs.nixpkgs.follows = "nixpkgs"` in the consuming flake.

## Binary cache

`https://cache.sekka.computer` serves Sekka's own packages and the patched
Aquamarine and Hyprland, so installations and updates download them instead of
compiling them. It is the Cloudflare R2 bucket `sekka-cache` behind a custom
domain in the maintainer's Cloudflare account; `sekka.computer` is registered
at Porkbun with Cloudflare DNS. Paths are signed with `cache.sekka.computer-1`.

Installed systems and the live media use it through
`installer/genkan/rust/system/binary-cache.nix`
(`calamares.binaryCache.enable`, default on). The flake's `nixConfig`
advertises the same cache to people building this flake elsewhere: Nix asks
before accepting it (and ignores it non-interactively) unless
`accept-flake-config` is set, and only trusted users can add a substituter
that `trusted-substituters` does not already list.
`nix build .#checks.x86_64-linux.binary-cache` checks that the flake, the live
media and an installed system name the same cache and key.

Publishing is manual; the release workflow does not touch the cache.
From a clean checkout of reviewed `main`:

```sh
nix run .#push-cache
```

It builds `genkan`, `tatami`, `displays`, `wallpapers`, `yukimi`, `aquamarine`,
`hyprland` and `tools`, and signs their complete runtime closures into a
temporary local cache: Nix only adds a path to a binary cache that already
holds its references. It then uploads just the paths cache.nixos.org does not
serve (about 145 MiB at first), archives before the narinfos that point to
them, and skips objects already in the bucket. Clients substitute each
reference separately, so the shared libraries come from cache.nixos.org; the
Sekka cache is incomplete on its own, and `nix copy --from` it alone fails.
All of these packages are evaluated with Nixpkgs' default configuration, which
refuses unfree packages, so NVIDIA's driver and other unfree packages are never
published. Installed systems at the same lock use the identical store paths.
The command also writes `nix-cache-info` with priority 50, so Nix asks
cache.nixos.org (40) first.

The push needs two credentials, neither of which belongs in the repository:

- The signing key, `~/.config/sekka-cache/cache.sekka.computer-1.sec`
  (`SEKKA_CACHE_CONFIG` overrides the directory). Keep a backup: replacing it
  means a new key name in `binary-cache.nix` and `nixConfig`, with the old
  public key kept until installed systems have updated.
- An R2 API token with Object Read & Write on `sekka-cache` only, stored as the
  AWS profile `sekka-cache` (region `auto`):

  ```sh
  nix run nixpkgs#awscli2 -- configure --profile sekka-cache
  ```

Nothing removes old paths from the bucket. Do not use R2 lifecycle rules for
that: unchanged paths keep their first upload date while still in use. R2
bills storage beyond its free 10 GB a month; downloads are free.

## Keep Awake and USB wake

`tatami awake on|off|toggle|status` controls a session-owned user service with
real logind `sleep:idle:handle-lid-switch` block inhibitors. The saved intent
is in `$XDG_STATE_HOME/tatami/keep-awake` (normally `~/.local/state/tatami`).
It is restored at Tatami login, released at logout, and independent of Waybar
and workload applications. The service reports readiness only after verifying
its own inhibitor, restarts on failure, and checks for lost inhibition every
second. The bar checks actual inhibition every two seconds and shows a warning
when the requested protection is unavailable. Appearance reloads exclude this
service. Hypridle stays running and respects the idle inhibitor, preserving
explicit lock/sleep hooks.

Keep Awake blocks normal suspend, hibernation, idle actions, and lid-close
sleep; administrators can still deliberately bypass inhibitors. It cannot
prevent power loss or firmware/thermal shutdown. Do not infer protection from
a stopped idle daemon or promise uninterrupted operation through those events.

The `calamares.usbWake.enable` option defaults to true. Udev enables wake only
along USB keyboard/mouse ancestor paths, including hubs and controllers; a
oneshot reapplies the rules at boot/activation and before sleep. It leaves
unrelated peripherals and autosuspend policy alone. Turning the option off
removes the policy; existing kernel wake attributes reset on reconnect/reboot.

```sh
nix build .#checks.x86_64-linux.keep-awake .#checks.x86_64-linux.usb-wake --no-update-lock-file
python3 -m unittest discover -s tests -p test_usb_wake.py -v
systemd-inhibit --list
tatami awake status
```

The VM check exercises real logind/polkit inhibition, rejected suspend, holder
failure recovery, logind restart, boot persistence, disable, and failed-start
indicators. It never suspends the host. USB fixtures check ancestor traversal,
unrelated-device isolation, repeated application, and reconnect-like resets.
Physical USB wake must still be checked on each machine with Keep Awake off;
record keyboard/mouse, hub, dock, firmware, and sleep mode when diagnosing it.

## Disk encryption must pass TRIM through

Genkan does not create encrypted disks yet. When it does, the configuration it
writes must set `boot.initrd.luks.devices.<name>.allowDiscards = true`.
dm-crypt drops discard requests unless the mapping allows them, so the weekly
`fstrim` and Btrfs's default `discard=async` never reach the SSD, and the drive
keeps treating freed blocks as live data. `nixos-generate-config` emits only
the LUKS `device`, so relying on the generated hardware configuration repeats
the mistake. Omarchy shipped LUKS installs that way and had to retrofit them
([omacom/omarchy#5332](https://github.com/omacom/omarchy/pull/5332)); one
reporter's sync writes on a nearly full DRAM-less NVMe drive ran more than
twice as fast after the first trim.

Keep the Nix option as the only source of truth. That pull request's migration
also ran `cryptsetup --persistent refresh`, which writes the flag into the
LUKS2 header, where it outlives the configuration and takes another
interactive refresh to remove. Refreshing a passphrase-only mapping also needs
the passphrase, so it fails without a terminal. If installed systems ever need
this change, ship it as a configuration update that applies at the next boot.

Discards reveal which blocks are unused to anyone holding the raw disk. Say so
in the README's account of encryption. The encrypted-install VM check must show
a non-zero `DISC-GRAN` in `lsblk --discard` for the opened mapping and a
successful `fstrim -v /`; check that the virtual target disk itself advertises
discard first, or the result proves nothing.

## Releases

`.github/workflows/release.yml` builds and publishes an ISO. Its only trigger
is a manual dispatch from `main`, naming an existing `v`-prefixed tag whose
commit is reachable from `main`:

```sh
git tag v0.1.0-preview.1 <commit on main>
git push origin v0.1.0-preview.1
gh workflow run release.yml --ref main -f tag=v0.1.0-preview.1
```

It checks out the tag, runs `nix flake check` and builds `.#iso` without
updating the lock, creates checksums and a source manifest, uploads assets into
a draft release, then publishes it as a prerelease. It uses the repository's
`GITHUB_TOKEN` with `contents: write`. Nothing creates tags, and no push, pull
request or schedule starts it. It does not push to the binary cache.

GitHub limits individual release assets to 2 GiB. `scripts/prepare-release.py` keeps a smaller ISO whole or splits a larger one into 1900 MiB parts. Release notes include Linux/macOS and Windows reassembly instructions. `SHA256SUMS` verifies downloadable parts; `ISO-SHA256SUMS` verifies the reconstructed image. Failed uploads leave a draft to inspect, not a partially published release. The script refuses to overwrite an existing output directory.

### Nightly builds

`.github/workflows/nightly.yml` runs at 4 AM `America/Chicago` and from its
**Run workflow** button (Actions, "Build nightly Sekka ISO"), on `main` only:

```sh
gh workflow run nightly.yml --ref main
```

A job on a GitHub-hosted runner first compares `main`'s commit with the newest
build's `revision` in <https://downloads.sekka.computer/nightly/builds.json>. If
they match, nothing else runs and the builder is not touched. Otherwise the
builder runs `nix flake check`, builds `.#iso`, and `nix run .#publish-nightly`
uploads it to the R2 bucket `sekka-downloads` as a new build, in a folder of
its own that never changes, `nightly/builds/DATE-REVISION/`:

- `sekka-nightly-DATE-REVISION-x86_64-linux.iso`
- `SHA256SUMS`
- `flake.lock`
- `manifest.json`: revision, commit and upload times, size and SHA-256

Then it writes `nightly/builds.json`, which lists the last seven builds, newest
first, and removes everything else under `nightly/`, older builds and anything
a failed run left. A failed check, build or upload leaves the list as it was,
so the next night tries the same revision again. Seven builds of about 6 GB
cost a few dollars a year in R2 storage beyond its free 10 GB.

`downloads.sekka.computer` is the bucket's custom domain, served through
Cloudflare. Cloudflare's cache keeps no file over 512 MB on this plan, so ISOs
always come from R2, whose egress is free. A build's files are marked
immutable, and `builds.json` is marked `no-cache`.

sekka.computer/download/ lists the builds through the download worker (see
Website): it keeps `builds.json` in each Cloudflare data centre's cache for
five minutes, so a new build appears within five minutes of its upload without
a new deployment, and R2 sees at most one request for the list per data centre
in that time.

The upload uses an R2 API token with Object Read & Write on `sekka-downloads`
alone, kept as the secrets `R2_ACCESS_KEY_ID` and `R2_SECRET_ACCESS_KEY` of the
GitHub environment `nightly`, which only `main` may use. Only the publish step
receives them; they are never on the builder's disk. To set or replace them,
create the token in Cloudflare (R2, Manage API tokens), then:

```sh
gh secret set R2_ACCESS_KEY_ID --env nightly --repo bitemyapp/sekka
gh secret set R2_SECRET_ACCESS_KEY --env nightly --repo bitemyapp/sekka
```

`gh` asks for each value without echoing it.

### The builder

Jobs run on wx-workstation (Ubuntu, Docker, Determinate Nix) through
`nix/runner.nix`, the `runner` package:

- A supervisor container, which Docker restarts, asks GitHub for each job's
  just-in-time registration: a runner labelled `self-hosted`, `linux`, `x64`,
  `sekka-builder` that takes one job and is then removed. Only the supervisor
  reads the token that allows this; jobs never see it. Through Docker's socket
  the supervisor is as powerful as root on the builder, so it runs nothing but
  its own script.
- Each job gets a fresh container, as uid 30500, with every capability
  dropped, `no-new-privileges`, a read-only root, and a tmpfs `/tmp` and
  throwaway volume for its home and workspace, both gone with the job. It sees
  the host's Nix store read-only and the daemon's socket, and nothing else of
  the host: no Docker socket, `/dev/kvm` or home directories. Its builds run
  in the daemon's own sandbox as an untrusted user, which cannot add
  substituters or change trusted settings.
- Before the first step, the runner's job-started hook admits only
  `workflow_dispatch` and `schedule` on `refs/heads/main` of
  `bitemyapp/sekka`: workflows only maintainers can start or change. For
  anything else, such as a pull request's workflow, it kills every process of
  the job but the runner, so even `if: always()` steps never run, and the job
  fails.
- The repository requires approval before any workflow from an outside
  contributor's pull request runs (Settings, Actions, General: "Require
  approval for all external contributors"). The workflow token defaults to
  read-only and cannot approve pull requests.

Know its limits. Builds happen in the host's daemon, so the job container's
memory and CPU limits do not cap them. The daemon takes `max-jobs` and `cores`
from its client: the job image sets 32 builds at a time with 16 cores each.
Nix's default, one at a time, made the first nightly build its 3,400
derivations in sequence, an hour and a half where most of them could run
together.
A job's container is on Docker's default network: it reaches the internet, the
LAN and services on the host. The admission hook and pull request approval keep
untrusted code from getting that far. Each build leaves its closure in the
host's store, about 6 GiB for the ISO alone; the workflows keep no GC roots,
and the daemon collects garbage when free space runs low.

### Setting up the builder

Once, as root on the builder, from a checkout:

```sh
sudo bash scripts/runner-host-setup.sh
```

It reserves uid and gid 30500 as `sekka-runner`, lets Nix's builds open
`/dev/kvm`, has the daemon collect garbage when free space falls under 100 GiB
during a build (until 200 GiB are free), adds Sekka's binary cache and
`cache.numtide.com`, and restarts the daemon. numtide's cache serves the AI
applications from `numtide/llm-agents.nix`, which cache.nixos.org does not
have; without it the builder compiles Codex and the rest from source. Untrusted
clients can use only the daemon's caches, so these belong in its
configuration. Restarting the daemon stops builds in progress, so run the
script while no job is running. The daemon puts
`/dev/kvm` in the sandbox of builds that require `kvm`, such as the flake
check's NixOS tests, but the sandbox drops the build users' supplementary
groups, so membership of `kvm` does not help. A udev rule instead gives the
`nixbld` group read and write on the device with an ACL. The workflow's first
Nix step fails with a pointer here if a build cannot use KVM.

Then, as the maintainer, build the runner, load its images and start the
supervisor:

```sh
nix build .#runner --out-link ~/sekka-runner/current
docker load -i ~/sekka-runner/current/job-image.tar.gz
docker load -i ~/sekka-runner/current/supervisor-image.tar.gz
docker run -d --name sekka-runner-supervisor --restart unless-stopped \
  --user "$(id -u):$(id -g)" --group-add "$(getent group docker | cut -d: -f3)" \
  --read-only --tmpfs /tmp --cap-drop ALL --security-opt no-new-privileges \
  -v /var/run/docker.sock:/var/run/docker.sock \
  -v /nix/store:/nix/store:ro \
  -v "$HOME/.config/sekka-runner:/run/secrets:ro" \
  sekka-runner-supervisor:latest
```

The images refer to store paths without holding them; the out-link is the GC
root that keeps them. The supervisor waits for its token: a fine-grained
personal access token for `bitemyapp/sekka` alone, with Administration: Read
and write (which registering runners needs) and no other permission, saved as
`~/.config/sekka-runner/token` with mode 600. Replace the file when the token
expires; the supervisor reads it before each registration.

To update the runner, build and load again, then, while no job is running,
`docker stop sekka-runner-job` (the waiting runner, which would take the next
job in the old image) and `docker restart sekka-runner-supervisor`. A restarted
supervisor lets a running job finish first. To stop taking jobs,
`docker stop sekka-runner-supervisor sekka-runner-job`. The supervisor logs to
`docker logs sekka-runner-supervisor`.

## Website

`site/` is a separate Cargo workspace whose program writes sekka.computer:
static HTML and CSS, with no JavaScript for browsers, built from `docs/brand/tokens.json`,
the brand SVGs, and screenshots copied byte for byte. Each caption repeats the
provenance recorded in `docs/screenshots.md`. The design page's contrast
ratios are computed from the tokens when the site is built. Kanji are outlined
from Noto Serif CJK JP, so no CJK webfont ships; JetBrains Mono and Atkinson
Hyperlegible Next are served from the site with their OFL texts. Pages follow
the visitor's colour-scheme and contrast preferences, and every page states
the revision it was built from (a dirty tree says so).

The stylesheet, images, logos, fonts and licence texts are published under
names that include a hash of their bytes (`images/yukimi.<hash>.png`), and
`_headers` lets browsers and Cloudflare keep exactly those files for a year.
Pages keep Pages' default and are revalidated on every visit, so a changed
file arrives under a new URL and a deployment needs no cache purge. The build
fails if a page or the stylesheet links to a file it didn't write.

The one piece of server-side code is the download worker,
`site/download-worker`, a Cargo workspace of its own in Rust. Pages runs it
for `/download/` alone (`_routes.json`); every other page is a plain static
file. It fetches the static page, puts the list of nightly builds in its place
for them, and gives the response the headers `_headers` gives static files,
which Pages leaves off a worker's responses. The list comes from
`builds.json`, which the worker keeps in the data centre's cache (the Cache
API) for `BUILDS_TTL` seconds (`site/src/shared.rs`, which the generator and
the worker share), the news that there is no list yet included. The page's
`x-builds-cache` header is `HIT` when the cache answered and `MISS` when R2 was
asked. A `fetch` with Cloudflare's `cacheTtl` and `cacheEverything` options
did not cache the list: its `cf-cache-status` was `DYNAMIC` on every request. Before the
first nightly the page says there is none; if the list cannot be had, it
keeps its fallback, a link to `builds.json`. The rendering is plain Rust,
tested natively when the site is built; only the request handler is built for
Wasm.

`nix/site.nix` builds the worker with workers-rs' `worker-build` 0.8.5, the
version that pairs with the `worker` crate, and gives it Nixpkgs' wasm-bindgen
(exactly the version in the worker's `Cargo.lock`, 0.2.127) and esbuild rather
than letting it download its own. Two settings matter. `wasm-opt` is off:
`worker-build` runs it with `--all-features`, and Binaryen then uses Wasm
features Cloudflare's runtime refuses. And `pages-shim.js` replaces
`worker-build`'s loader: Pages' advanced mode calls `fetch` on a plain object,
where `worker-build` exports an entrypoint class. The result is
`_worker.js/index.js` and `_worker.js/index.wasm` in the site.

```sh
nix build .#site --out-link result-site
nix run --inputs-from . nixpkgs#static-web-server -- --port 8787 --root result-site --page404 result-site/404.html
```

A static server shows the download page's fallback. To run the worker as
Pages does, in Cloudflare's runtime, with any local server standing in for
downloads.sekka.computer (it needs no Cloudflare account):

```sh
cp -rL result-site /tmp/site && chmod -R u+w /tmp/site
nix run --inputs-from . nixpkgs#wrangler -- pages dev /tmp/site --port 8788 \
  --binding NIGHTLY_URL=http://127.0.0.1:8799/nightly
```

The site is the Cloudflare Pages project `sekka` (`sekka-esx.pages.dev`), with
the `sekka.computer` apex as its custom domain. Nothing deploys it
automatically. To publish a new version, build from a clean commit that is on
GitHub's `main` (the pages link the revision they were built from), then
deploy with Wrangler, logged in once with `wrangler login`:

```sh
nix run --inputs-from . nixpkgs#wrangler -- pages deploy result-site --project-name sekka --branch main
```

Only Wrangler uploads the worker's Wasm as a module. The dashboard's upload
publishes `_worker.js/` as plain files, and the download page then shows its
fallback. Pages applies `_headers` (the content security policy) to static
files and serves `404.html`. The pages promise no cookies, and no scripts or
analytics in the browser, so leave off the Cloudflare features that add them:
Web Analytics, Rocket Loader, Email Address Obfuscation and Bot Fight Mode.

Everything the site claims comes from this repository's READMEs and docs.
When a limitation, test or feature changes, change `site/src/pages.rs` with
it.

## Screenshots and attribution

The crest, lockups, family icons and colour tokens are in `docs/brand`, with
their usage rules in its README. `docs/brand/generate.py` draws every SVG, with
text outlined from JetBrains Mono and Noto Serif CJK taken from the root
flake's Nixpkgs; change the script and regenerate rather than editing the SVGs.
Nothing in the desktop, installer or image uses them yet.

Wallpapers retain their original published resolution and aspect ratio; desktop
fill/zoom modes handle cropping for each display. Preserve the downloaded bytes
and record their URL, dimensions and SHA-256 in
`desktop/tatami/wallpapers/wallpapers.json`. See the [wallpaper documentation](desktop/tatami/wallpapers/README.md)
and run `nix build .#checks.x86_64-linux.wallpapers` after changing the collection.
`nix build .#checks.x86_64-linux.tatami-background` exercises Tatami's saved
appearance settings, per-output inheritance, renderer arguments and menu commands
using isolated state, without changing the host desktop.

Keep original full-resolution PNGs and record how they were captured in `docs/screenshots.md`. Use real app captures, not generated mockups. Mark previews or historical images accurately. Preserve the original component licenses, SPDX declarations, artwork credits, and `docs/imports.json` provenance.

`nix build .#screenshots --out-link result-screenshots` captures Genkan's debug
preview and Yukimi in a disposable NixOS VM. This is a development output and
is not included in the ISO or the normal component checks.
