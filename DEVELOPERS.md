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
nix build .#genkan .#tatami .#displays .#yukimi .#tools
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

Publishing is manual; the disabled release workflow does not touch the cache.
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

## Release workflow: deliberately disabled

`.github/workflows/release.yml` has only a manual trigger and an unconditional `if: ${{ false }}` on its sole job. No dispatch, push, tag, or schedule can currently build or publish an image through it. **Do not enable it as part of ordinary maintenance.**

When release readiness is established, a maintainer must review a change removing that inhibition. Provision a clean dedicated Linux x86_64 runner labeled `sekka-builder`, with enough storage, Python 3, Git, and GitHub CLI. The workflow installs Nix. Run the workflow from `main`, specifying an existing `v`-prefixed tag whose commit is reachable from `main`.

It checks and builds the tagged source without updating the lock, creates checksums and a source manifest, uploads assets into a draft release, then publishes it as a prerelease. The workflow uses its repository-scoped `GITHUB_TOKEN`; no personal token is required. It does not trigger on pull requests or create release tags automatically.

GitHub limits individual release assets to 2 GiB. `scripts/prepare-release.py` keeps a smaller ISO whole or splits a larger one into 1900 MiB parts. Release notes include Linux/macOS and Windows reassembly instructions. `SHA256SUMS` verifies downloadable parts; `ISO-SHA256SUMS` verifies the reconstructed image. Failed uploads leave a draft to inspect, not a partially published release. The script refuses to overwrite an existing output directory.

## Website

`site/` is a separate Cargo workspace whose program writes sekka.computer:
static HTML and CSS with no JavaScript, built from `docs/brand/tokens.json`,
the brand SVGs, and screenshots copied byte for byte. Each caption repeats the
provenance recorded in `docs/screenshots.md`. The design page's contrast
ratios are computed from the tokens when the site is built. Kanji are outlined
from Noto Serif CJK JP, so no CJK webfont ships; JetBrains Mono and Atkinson
Hyperlegible Next are served from the site with their OFL texts. Pages follow
the visitor's colour-scheme and contrast preferences, and every page states
the revision it was built from (a dirty tree says so).

```sh
nix build .#site --out-link result-site
nix run --inputs-from . nixpkgs#static-web-server -- --port 8787 --root result-site --page404 result-site/404.html
```

The site is the Cloudflare Pages project `sekka` (`sekka-esx.pages.dev`), with
the `sekka.computer` apex as its custom domain. Nothing deploys it
automatically. To publish a new version, build from a clean commit that is on
GitHub's `main` (the pages link the revision they were built from), then
upload `result-site` as a new deployment: in the dashboard, Workers & Pages ›
sekka › Create deployment (a zip of the directory works), or
`wrangler pages deploy result-site --project-name sekka`. Pages applies
`_headers` (the content security policy) and serves `404.html`. The pages
promise no cookies, scripts or analytics, so leave off the Cloudflare features
that add them: Web Analytics, Rocket Loader, Email Address Obfuscation and Bot
Fight Mode.

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
