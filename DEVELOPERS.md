# Developing Sekka

Sekka is the source home for the ISO, Genkan installer, Tatami desktop, Yukimi system manager, and Kakehashi USB writer. Work on a short-lived topic branch from `main`, open a pull request, squash-merge it, and delete the topic branch. There is no `stable`, `develop`, or release branch. Releases identify a commit with a version tag. The `nightly` branch, which installed systems follow, belongs to the nightly workflow alone; never push to it.

## Repository map

| Path | Responsibility |
| --- | --- |
| `flake.nix`, `flake.lock` | Public outputs and the authoritative external dependency lock |
| `installer/genkan/` | Rust installer, privileged helper, system modules, application catalog, storage tests |
| `desktop/tatami/` | Desktop module, configuration, Rust commands, Displays app, wallpapers |
| `apps/yukimi/` | GTK application, privileged helper, store/configuration/system libraries |
| `apps/kakehashi/` | Cross-platform USB writer: GPUI window, privileged helper, drive and image library |
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
nix build .#genkan .#tatami .#displays .#network .#activity .#yukimi .#tools
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
cargo test --locked --manifest-path desktop/tatami/activity/Cargo.toml
cargo test --locked --manifest-path apps/yukimi/Cargo.toml
cargo test --locked --manifest-path apps/kakehashi/Cargo.toml
cargo test --locked --manifest-path rust/Cargo.toml
```

Do not merge unrelated Cargo workspaces or broaden package source filters as part of routine edits. Preserve Genkan, Yukimi and Kakehashi's unprivileged GUI/privileged helper boundaries. Keep slow I/O off the interface thread.

The retained VM tooling is available through `nix build .#tools`. Run `result/bin/respin-tools qemu-test --help` for its firmware, storage, desktop, and application options. The Genkan fixture now builds from `installer/genkan/tests/vm-fixture`, not a remote installer revision. The rootless builder's static guest tools require the Rust musl target. The older reports in `TESTING.md`, `installer/genkan/RUST-TESTING.md`, and `docs/test-results/` describe their named predecessor revisions only.

Before publishing an installable image, exercise the exact image through a disposable UEFI and BIOS install, disk-only reboot, desktop login, and Yukimi configuration/update checks. Record the ISO checksum, source revision, results and outstanding hardware limitations. Passing component tests alone is insufficient evidence of an end-to-end installation.

## Sources and installed-system updates

The root lock retains distinct base-system and application Nixpkgs inputs, plus Determinate Nix, FlakeHub CLI, AI applications, and upstream oh-my-pi. First-party code comes directly from this checkout.

ChatGPT Desktop 26.1013.40934 copies `codex-app-tools` from the Nix store
into `~/.config/Codex/executor-plugins` with Node's `fs.cp`, preserving
read-only directories and files. Refreshing that copy fails with `EACCES`
while unlinking its launcher or rewriting `.mcp.json`, and the local Work
executor retries indefinitely. On gainzymede, after successful authentication,
the window stayed blank; with the repair the executor connected on its first
attempt and the welcome questionnaire rendered on the same account.

`nix/ai-packages.nix` applies `nix/chatgpt/` to that package alone. Both the
catalog module (including existing installed flakes) and the installation
media's catalog resolution use it; `packages.chatgpt` exposes the same build.
The helper makes the destination writable before and after copying, repairing
copies left by the old package, preserving executable bits and leaving symlink
targets alone. It serializes overlapping copies and leaves `.mcp.json` for the
caller to generate, so another refresh cannot make that file read-only during
a write. The ASAR patch preserves byte offsets and fails unless exactly
one expected copy call matches. The upstream package already repairs the
separate marketplace plugin copy; this covers the executor's copy. Review the
patch when updating the app, and remove it when upstream fixes that path.

```sh
nix build .#chatgpt .#checks.x86_64-linux.chatgpt-plugins --no-update-lock-file
```

The regression check calls the packaged app's actual executor-copy function
with read-only fixture files, including fresh, repeated, concurrent and already-broken
copies; the original `fs.cp` is its failing control. It also checks the generated
manifest, launcher permissions and symlink isolation. This is a component
check; it does not sign in, build an ISO or install a VM. The successful
authenticated launch on gainzymede is separate physical-machine evidence.
The app's existing downloaded-runtime patcher also raced its first executable
validation on this machine; a subsequent runtime download passed validation.
That independent startup race is not repaired by the plugin-copy change.

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

Hyprland itself carries `nix/patches/hyprland-screenshare-sessions.patch`
(`nix/overlays/hyprland.nix`), which is proposed upstream as the same commit.
wlr-screencopy and hyprland-toplevel-export clients, xdg-desktop-portal-hyprland
among them, ask for one frame at a time, so Hyprland keeps a "managed" session
per client and target. In 0.56.2 those sessions ended only when their monitor
disconnected or their window closed, and at any monitor scale other than 1 a
region share missed the lookup and made a new session for every frame: the
region was stored in pixels and compared with the client's logical box. Each
session is walked on every draw call, so an hour of a browser's region share
left about 144,000 sessions and Hyprland's main thread at 100% until the next
login. With the patch the requested box identifies a region session; a managed
session ends when its client disconnects, or after five seconds without a frame
request and with no frame pending; and a client holds at most 32, losing its
least recently used. `nix/overlays/default.nix` lists both overlays for the
flake's package set, `overlays.default` and `modules/hyprland-fixes.nix`.
`nix build .#checks.x86_64-linux.screenshare-sessions` runs the patched and
the unpatched Nixpkgs Hyprland in two disposable VMs, each with a headless
output at scale 2. It counts Hyprland's own session trace lines while a
screencopy client shares a region, the whole output and a window, holds many
regions at once, and connects repeatedly. The unpatched compositor must show
both leaks: that is the negative control. Remove the patch when a Hyprland
release includes the fix, after this check passes with the new upstream
package; review any patch failure rather than skipping it.

Tatami's Elephant (`desktop/tatami/elephant/`) carries `search.patch`, a Sekka
change that is not upstream. Stock Elephant searches applications either by
name alone (`only_search_title`, which Omarchy sets) or by fuzzy matching the
Exec line, the Comment and the keywords joined into one string. On NixOS the
Exec line is usually a store path, so "nix" and "store" listed most
applications, and a query could match across several keywords. The patch
matches the name, the generic name and each keyword on its own. It also keeps
the generic name as Elephant's subtext, leaving it empty when it repeats the
name; Walker shows it under each application's name
(`item_desktopapplications.xml`). Stock Elephant replaced it with whichever
field matched, such as the raw keyword list. Give a first-party application a
`GenericName` that says what it is and `Keywords` for what people search for,
including other systems' names for it ("task manager", "etcher"): a
multi-word query matches only a multi-word keyword or generic name.
`nix build .#checks.x86_64-linux.launcher-search` runs the patched Elephant
with Tatami's configuration over the first-party desktop entries and
stand-ins for third-party ones, without a graphical session; stock Elephant
with the same configuration is the negative control. The patch applies
without fuzz to Elephant 2.22.0 and 2.22.1. After an Elephant update, run the
check, and review any patch failure rather than skipping it.

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
- **XWayland's scale.** Tatami sets Hyprland's `xwayland:force_zero_scaling`,
  so X11 windows are drawn at 1×, sharp but small unless the application scales
  itself. Many read their scale from the `Xft.dpi` resource (SDL, which Steam
  uses; Chromium and Electron; Qt; GTK's text), so the watcher keeps it at 96 ×
  the main display's scale (`xwayland.rs`), rounded. The main display is
  chosen as Displays does: the kept one, else the one showing workspace 1, else
  the first one on, never a mirror. The watcher runs `xrdb -nocpp -merge`
  whenever that value changes, keeping XWayland's other resources, and retries
  every five seconds while XWayland is not up. It takes `DISPLAY` from its own
  environment, or else from the user service manager's. Applications read
  `Xft.dpi` when they start. If XWayland restarts, the value is set again only
  when the scale or the main display next changes. Hyprland's own `GDK_SCALE=2`
  (`hyprland.lua`) is unchanged.

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
`tatami display` from another workspace. Physical docking and the lid are in
daily use on the ThinkPad X1 Carbon Gen 14; these tests guard that behaviour
between changes. Don't describe it as untested on hardware.
`nix build .#checks.x86_64-linux.displays-cursor-vm` opens the Displays window
with `tatami display`, as the bar's icon does, and checks that the cursor stays
where it was.

`nix/installed-lock.nix` extends that external lock with Sekka itself. A clean Git build pins its commit and source hash; a dirty development build pins the exact source copied into the Nix store. The source is included on the ISO. The installed flake follows `github:bitemyapp/sekka/nightly` for subsequent updates, and every other input follows Sekka's own (`inputs.nixpkgs.follows = "sekka/nixpkgs"`, and likewise `determinate`, `fh`, `applications`, `ai-apps` and `omp`). Updating Sekka therefore moves them together, and only to revisions the nightly workflow has checked and confirmed a fresh system downloads entirely: it moves `nightly` only then, so nothing upstream publishes reaches an installed system untested, and no update compiles.

Update the distro through **Yukimi → Updates → Update** or `sekka-update`.
Both use the privileged helper transaction: recognize/migrate the flake,
back up the configuration, update the selected inputs, and build the host once.
A flake following Sekka's `main` (or its default branch) is moved to
`nightly`, and each of the other inputs still at the address Sekka's
installations have used is made to follow Sekka's (`yukimi_config::sekka`).
One given an address of one's own keeps it, and Sekka follows it; that
system may then build Sekka's packages itself. The build passes the binary
caches the new configuration names (as root, so the daemon accepts them), so
a cache an update adds already serves that update. While it builds or
downloads, the helper stops it if less than 4 GB stays free where Nix keeps
the store or builds (`/nix`, `/tmp`), and it doesn't start below 10 GB; the
configuration is put back either way. Installed systems also have Nix
collect garbage below 5 GB free (`installer/genkan/rust/system/store-space.nix`).
The ordinary GUI action updates Sekka, and with it everything that follows
it, along with any other source with a newer revision; the CLI updates only
Sekka. Sources that don't follow Sekka keep their own rows.
The default `auto` mode compares boot artifacts, kernel parameters, startup
compatibility, the declared display/graphics/sleep-manager identity, effective
session-critical systemd units (including aliases and drop-ins), and NixOS
switch inhibitors before choosing `switch` or `boot`. Tatami's login entry
starts `uwsm` and `tatami` from `/run/current-system/sw/bin`, as NixOS's own
uwsm sessions start their compositors, so a Tatami build leaves the display
manager's session list (`XDG_DATA_DIRS` in its unit) unchanged and can switch
live; the next login starts the new Tatami.
`nix build .#checks.x86_64-linux.tatami-session-entry` keeps store paths out
of the entry. After a live switch
(an update or a rollback) that adds, removes or changes a `.desktop` entry,
for everyone or in a user's own profile, the helper restarts
`tatami-elephant.service` in each signed-in user's session
(`systemctl --user --machine=<user>@.host try-restart`). Elephant watches the
application folders it found at sign-in through `/run/current-system` and
`/etc/profiles/per-user`, so it would otherwise miss the change until the next
sign-in; Walker reconnects to it by itself. Sessions without Tatami have no
such unit, and the restart never fails the update. Configuration changes,
branch changes, and rollbacks use the same planner. A missing compatibility
manifest stages the transition for boot. D-Bus catalog trigger changes remain
live when both unit definitions specify reload-on-change and every other unit
setting matches; changes to its executable or reload policy still stage for boot.
A display manager unit that says `X-RestartIfChanged=false` before and after,
and differs only in `XDG_DATA_DIRS` directories holding nothing but
`wayland-sessions` and `xsessions`, also stays live when every session entry it
may have read (the booted system's, or the active one's if it has restarted)
is unchanged in the target: sessions were only added or removed. NixOS leaves
it running, and it lists the new sessions from its next start; the completion
message says so. GDM, LightDM and Plasma Login Manager find sessions that way;
greetd's greeter finds them through its configuration file, whose changes still
stage. A changed entry still stages, and the reason names its desktop: most
entries start their desktop by store path (`startplasma-wayland`, Hyprland, an
X session script), so until a restart the running login screen would start the
previous version.
`sekka-update --boot` explicitly
stages the update; `--switch` requests live activation but refuses an unsafe
transition before committing configuration edits. A target-side NixOS
pre-switch check also rejects unsafe activation from older updaters and other
rebuild frontends, before they can stop the desktop. Do not copy the root lock verbatim
into an installed machine: it lacks the Sekka input until the installed lock
is generated.

For installations without the new updater, `nix run github:bitemyapp/sekka/nightly#update`
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
rejects a display-manager transition before service shutdown, switches live when
a session is only added to the login screen's list, stages one that changes an
entry the login screen started with, and stages a rollback with changed boot
parameters. It boots that prepared generation through UEFI,
verifies that the restart request disappears, and captures the GUI. Its desktop
is IceWM; it does not reproduce physical GPU teardown or claim Hyprland hardware coverage.
Namespace fixtures test helper transactions and Tatami reload failures without
changing the host desktop.

`python3 tests/test_sekka_update.py result/bin -v` tests the packaged CLI and
helper in a rootless bubblewrap namespace. It covers migration, repeat updates,
following Sekka and the new system's caches, a Nixpkgs of the system's own,
refusing to start without room, failed builds, cancellation, concurrent operations, closed progress pipes, unsafe
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
media and an installed system name the same caches and keys.

The same module adds numtide's `https://cache.numtide.com`
(`calamares.binaryCache.numtide.enable`, default on), signed with
`niks3.numtide.com-1`, as `numtide/llm-agents.nix` documents. It serves the
catalog's AI applications as built from that flake's own lock, which installed
systems keep, so an update that moves `ai-apps` downloads them instead of
compiling them. Its `nix-cache-info` asks for priority 30; the URL's `?priority=55` makes Nix ask
it last, only for paths neither cache.nixos.org nor Sekka's cache has. A
system takes a new substituter only once activated, so the first update to
this module still builds anything new itself.

The nightly workflow publishes every revision it moves `nightly` to (see
Nightly builds); the release workflow does not touch the cache. To publish by
hand, from a clean checkout of reviewed `main`:

```sh
nix run .#push-cache
```

It builds `genkan`, `tatami`, `displays`, `wallpapers`, `yukimi`, `kakehashi`,
`aquamarine`, `hyprland`, `tools` and `installed-systems`, and signs their
complete runtime closures into a temporary local cache: Nix only adds a path to
a binary cache that already holds its references. `installed-systems` is an
installed system for each desktop the media offers (`respin.referenceDesktops`)
and one with every free catalog application, so everything Sekka's modules
change and every application one can choose is covered without a list to
keep up:
Hyprland and what uses its patched Aquamarine (hyprlauncher among them),
Elephant, the wallpapers. It then uploads just the paths that none of
cache.nixos.org, Determinate's cache and numtide's cache serve (about 0.8 GiB),
archives before the narinfos that point to them, and skips objects already in
the bucket. Determinate Nixd is left to Determinate. Clients substitute each
reference separately, so the shared libraries come from cache.nixos.org; the
Sekka cache is incomplete on its own, and `nix copy --from` it alone fails.
All of these are evaluated refusing unfree packages (`installed-systems`
forces it), so NVIDIA's driver and other unfree packages are never published,
and one added to a desktop makes the push fail rather than publish it.
Installed systems at the same lock use the identical store paths.
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

  `AWS_ACCESS_KEY_ID` and `AWS_SECRET_ACCESS_KEY` in the environment take its
  place, as in the nightly workflow.

Nothing removes old paths from the bucket. Do not use R2 lifecycle rules for
that: unchanged paths keep their first upload date while still in use. R2
bills storage beyond its free 10 GB a month; downloads are free.

## Activity

Tatami's Activity window (`desktop/tatami/activity`, `tatami-activity`) is a
GTK 4 and libadwaita crate of its own, as Displays and Network are. A sampler
thread (`sampler.rs`) reads `/proc` and `/sys` every interval and sends the
window a plain `Frame`; the window updates its rows (GObjects, `entry.rs`) in
place and re-sorts, so selection, expansion and scrolling survive a reading.
Costly figures are rationed: `smaps_rollup` (PSS, private, shared) every
fifth reading unless the Memory view shows or the process is selected, and
each process's DRM file descriptors every tenth. Rows are grouped into
applications in `apps.rs` from the process tree and systemd units, with
process managers (init, systemd, kthreadd, the compositor) breaking the tree,
and named from Hyprland's clients and launcher entries; the unit tests there
cover Chromium moving itself into a scope and helpers reparented to systemd.

NVIDIA figures come from NVML, opened with `dlopen` (`nvml.rs`), so the crate
has no NVIDIA build dependency; NixOS's library is in `/run/opengl-driver/lib`.
AMD, Intel and other DRM drivers' per-process figures come from
`/proc/PID/fdinfo` (`gpu.rs`, the kernel's drm-usage-stats format), with a
client shared between processes counted once.

The processor's RAPL counters are root-only (CVE-2020-8694). The window stays
unprivileged: `tatami power-meter` (`tool/src/power_meter.rs`) runs as
`tatami-power-meter.service`, socket-activated by `/run/tatami/power.sock`,
with no capabilities, network or writable paths. It reads the package domains
at most once a second, answers every connection with "NAME ENERGY_UJ
RANGE_UJ" lines, and exits after a minute without one. Per-process power in
`power.rs` is a model, documented there and in Tatami's README; its tests pin
the split so it never attributes more than was measured.

`tatami-activity --print` prints one reading as text, useful on a machine
without a session. To look at the window without a desktop, run it on
GTK's Broadway backend (`gtk4-broadwayd :5`, then `GDK_BACKEND=broadway
BROADWAY_DISPLAY=:5 tatami-activity`) and open `http://127.0.0.1:8085`; set
`XDG_CONFIG_DIRS` to `desktop/tatami/config` for Tatami's look. For captures
at the screenshot VM's size, a headless sway (`WLR_BACKENDS=headless
WLR_RENDERER=pixman`, output 2560x1600 at scale 2) and `grim` work without a
GPU; a second `tatami-activity --pid PID`, `--lens ID` or `--search TEXT`
drives the open window, and its actions are on D-Bus at
`/org/tatami/Activity/window/1`. Component tests cover parsing, grouping and
the energy split. `nix build .#checks.x86_64-linux.activity-vm` boots a
disposable Tatami VM: Super+Ctrl+T must open the window floating, the power
meter must start from its socket as root and answer, a stand-in meter on the
same socket (a VM has no RAPL counters) must give a busy terminal processor
power in a reading, and Quit must end that process. Reading real RAPL
counters as root needs physical hardware.

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

## System tuning and responsiveness

`calamares.tuning.enable` (on by default) applies
`installer/genkan/rust/system/tuning.nix`. That covers CachyOS-inspired kernel,
memory, I/O and ananicy settings, plus Ubuntu's lowlatency boot options and
mq-deadline for NVMe. Sekka also has its own ananicy rules: shells run at
nice 0 and compilers at nice 3. [docs/tuning.md](docs/tuning.md) lists each
setting and why.

```sh
nix build .#checks.x86_64-linux.tuning --no-update-lock-file
tests/responsiveness/run.sh measure LABEL
tests/responsiveness/run.sh compare BEFORE AFTER
```

The VM check boots the installed kernel with an emulated NVMe disk. It checks
the boot options, the preemption mode, both disk schedulers, and the priorities
of a terminal, its shell, and a compiler. The responsiveness harness measures a
real machine's live session under build load for about 15 minutes.
[docs/responsiveness.md](docs/responsiveness.md) describes it and records
ganymede's results. Per-application time slices and sched-ext schedulers are
held for later, in [docs/interactive-scheduling.md](docs/interactive-scheduling.md).

## Omarchy's fish configuration

The catalog's fish choice builds Omarchy's `omarchy-fish` package
(`installer/genkan/rust/system/fish/package.nix`) as omarchy-pkgs'
`pkgbuilds/omarchy-fish/PKGBUILD` does: the omarchy-fish and fzf.fish releases
it names, whose tarballs match its SHA-256 sums, in fish's vendor directories,
with omarchy-fish's history search replacing fzf.fish's. The PKGBUILD's
dependencies are the catalog entry's packages. `starship.toml` beside it is
Omarchy's prompt, unchanged; fish exports `STARSHIP_CONFIG` for it unless the
person has a `~/.config/starship.toml`.

Omarchy keeps bash as the login shell, and `omarchy-setup-fish` gives
`~/.bashrc` a test that starts fish. `applications.nix` puts the same test at
the start of `/etc/bashrc`'s interactive part. It skips root, `bash -c`, bash
started from fish, and shells with `SHLVL` above 1, so `nix develop` and
`nix-shell` stay in bash. NixOS's `ls`, `ll` and `l` aliases are removed from
fish, where Omarchy has none and its `ls` function would be hidden. NixOS's
fish module still generates completions from each installed package's manual
pages. These are small local builds, which Determinate Nix downloads instead
when Sekka's cache has them; the all-applications reference system includes
fish.

```sh
nix build .#checks.x86_64-linux.fish --no-update-lock-file
```

The check logs in on a disposable VM's console. It checks that bash handed
over to fish, with Omarchy's settings, functions, fzf.fish bindings, zoxide,
mise, eza and prompt, and that `cd` falls back to zoxide. `bash -c`, root's
interactive bash, and bash started from fish stay in bash.

To follow Omarchy, compare the PKGBUILD and Omarchy's `config/starship.toml`
with these files. Update the tags and hashes
(`nix flake prefetch github:omacom-io/omarchy-fish/v<version>`), then run the
check.

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

The job runs on the builder. It first brings every input in `flake.lock` up
to date (`nix flake update`) and commits that, if anything moved, on top of
`main`. If the result is the newest build's `revision` in
<https://downloads.sekka.computer/nightly/builds.json>, nothing has changed and
it stops. Otherwise it runs `nix flake check` and builds `.#iso`, then, in this
order and only if each step succeeded:

1. `nix run .#push-cache` publishes Sekka's packages for that revision to
   cache.sekka.computer.
2. `scripts/check_cache_coverage.py .#installed-systems` follows, from each
   desktop's installed system and the free catalog applications, everything
   none of cache.nixos.org, Sekka's, numtide's and Determinate's caches has,
   as a system with an empty store would. It fails if any of it would be
   compiled rather than written locally (configuration), copied into place
   without a compiler (as Determinate Nixd is from its download) or fetched.
   NVIDIA's driver and the unfree applications aren't covered: they can't be
   published, so systems that have them build them (the driver for each
   kernel, in minutes; the applications by repackaging their vendors'
   downloads).
3. It pushes the revision to `main` (a fast-forward; if `main` moved during
   the build, the input update waits for the next night) and to `nightly`.
   Installed systems update to `nightly`, so they only ever reach a revision
   that passed and is cached. `nightly` is force-pushed when `main` moved
   past an earlier input update.
4. `nix run .#publish-nightly` uploads the ISO to the R2 bucket
   `sekka-downloads` as a new build, in a folder of its own that never
   changes, `nightly/builds/DATE-REVISION/`:

- `sekka-nightly-DATE-REVISION-x86_64-linux.iso`
- `SHA256SUMS`
- `flake.lock`
- `manifest.json`: revision, commit and upload times, size and SHA-256

Then it writes `nightly/builds.json`, which lists the last seven builds, newest
first, and removes everything else under `nightly/`, older builds and anything
a failed run left. A failed check, build or upload leaves the list as it was,
so the next night tries the same revision again. Seven builds of about 12 GB
cost about a dollar a month in R2 storage beyond its free 10 GB.

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

The cache push needs the cache's signing key and a second R2 token, with
Object Read & Write on `sekka-cache` alone, as the `nightly` environment's
`CACHE_SIGNING_KEY`, `CACHE_R2_ACCESS_KEY_ID` and `CACHE_R2_SECRET_ACCESS_KEY`.
The job writes the key to a private temporary file for `push-cache` and
removes it; without these secrets the nightly fails before moving anything.

```sh
gh secret set CACHE_SIGNING_KEY --env nightly --repo bitemyapp/sekka < ~/.config/sekka-cache/cache.sekka.computer-1.sec
gh secret set CACHE_R2_ACCESS_KEY_ID --env nightly --repo bitemyapp/sekka
gh secret set CACHE_R2_SECRET_ACCESS_KEY --env nightly --repo bitemyapp/sekka
```

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

## Kakehashi

`apps/kakehashi/` is its own Cargo workspace of three crates. `core` holds the
drive rules, image checksums, the nightly list and downloads, and the write
itself, with one platform module each for Linux, macOS and Windows. `helper`
is `kakehashi-helper`, the only part that writes a drive. It runs with
administrator rights on Linux and Windows; on macOS it receives an authorized
device handle from Apple’s `authopen` while remaining an ordinary user process.
`app` is the window, built on GPUI through `gpui-kit` with its `gpui-fast`
feature (gpui-component's widgets, re-exported as `gpui_kit::component`), in
Sekka's Night and Day colours from `docs/brand/tokens.json`. It replaces
`scripts/flash_usb.rs`, whose drive rules it carries over; the script still
works. On Sekka, Genkan and Yukimi's Discover offer it from the application
catalog (`"source": "sekka"`), as the same store path as `packages.kakehashi`,
which `push-cache` publishes.

The window runs as the user. To write, it makes a private temporary directory
holding `plan.json` (the image, its SHA-256 and the drive as the window saw
it) and an empty progress file, then starts the helper beside its own
executable through the system's prompt: `pkexec` on Linux (the setuid wrapper
in `/run/wrappers/bin` first) and `ShellExecuteExW` with the `runas` verb on
Windows. On macOS the GUI launches the helper directly; after validating and
unmounting the selected drive, the helper runs `/usr/libexec/authopen
-stdoutpipe -o FLAGS /dev/rdiskN`. Apple’s broker requests authorization and
passes the open descriptor over a private Unix socket. It uses read/write,
exclusive and nonblocking flags, with no creation or truncation. `authopen`
rejects `O_NOFOLLOW` with `EINVAL`, so the helper rejects symlinks with
`symlink_metadata` first and checks the returned raw device identity and size
before writing. Errors preserve the broker’s diagnostic without assuming
that every failure means missing privacy permission.
Cancelling while authorization is pending kills and reaps the broker. The
helper appends one JSON object per line to the progress file, which it opens
without following links and only while it has a single name; the window stops
it by creating `PROGRESS.cancel`, which the helper checks between 8 MiB chunks.

On macOS the GUI first stages the selected image in that private temporary
directory, on a worker thread. Administrator authorization alone does not
grant macOS file-privacy access. The former AppleScript elevation path lost
the GUI’s removable-volume consent: the root helper could fail to open the
raw device with `EPERM` even after both prompts were approved. `authopen`
has Apple’s entitlement to check removable-volume consent on the responsible
application. Keeping the helper in the GUI’s process ancestry lets it use
that consent; `NSRemovableVolumesUsageDescription` explains the request.
The GUI stays unprivileged and never receives a writable device handle.
Staging uses an independent APFS clone when possible, or a cancellable copy
across filesystems, and checks its SHA-256 before requesting device access.
The plan names the staged image; the original stays where the user put it.
The worker retains the directory until the helper exits and removes it on
completion or failure. A copy across filesystems requires space for the image
on the startup volume. Tests exercise preparation and descriptor transfer
using temporary files and private sockets, including rejected/missing
descriptors, broker failure and cancellation. macOS tests also run the real
`authopen` against `/dev/null`: they reproduce the rejected flag and verify
that the production flags return a writable descriptor. No tests write to
host devices. The opt-in macOS check
`KAKEHASHI_CHECK_USB=diskN cargo test --release --locked -p kakehashi-core
inspect_authorized_usb_handle_without_writing -- --ignored --nocapture`
checks the production device-open function on an explicitly selected USB disk
and closes the handle without unmounting, writing or ejecting. It requires
macOS authorization. These checks do not validate physical USB writing.

`kakehashi_core::write::execute` compares the plan's drive with the drive as
it is now (name, size, vendor, model, serial and bus) and rejects one the
rules now exclude, unmounts it, opens it exclusively, checks again, writes in
sector-padded chunks, zeroes the last MiB, syncs, drops the page cache where
it can, reads everything back against the image, checks the zeroed tail and
ejects. Windows locks and dismounts each volume on the disk, deletes the drive
layout, and writes with unbuffered, write-through I/O from sector-aligned
buffers. On Linux the helper finds `lsblk`, `umount` and `udisksctl` through
`KAKEHASHI_TOOL_PATH`, set when the Nix package is built, because `pkexec`
resets `PATH`.

macOS flushes the raw drive with `DKIOCSYNCHRONIZECACHE` before read-back
verification. Rust's `File::sync_all` uses `F_FULLFSYNC`, a filesystem operation
that raw disks reject with `ENOTTY` even after all image bytes were written.
Device flush failures remain fatal and include the failed stage; they are not
ignored or replaced with a plain `fsync`. Linux and Windows retain their
existing `File::sync_all` behavior. The opt-in
`flush_authorized_usb_without_writing` test uses `KAKEHASHI_CHECK_USB` like the
handle check above: it reproduces `ENOTTY` and flushes the existing device cache
through the production code, without writing bytes, unmounting or ejecting.

Nightly downloads read `builds.json` from `downloads.sekka.computer/nightly`,
resume a `.part` file, hash while downloading, and leave a `.sha256` file
beside the ISO so a later choice of the same file is checked
too. TLS is rustls on Linux and the system's own (`native-tls`) on macOS and
Windows.

Nightly downloads probe byte-range support, then start two concurrent requests.
They measure aggregate throughput over three-second windows, with one second
of settling after each increase, trying 4 and 8 connections before accepting a
plateau. They continue doubling up to 32 while throughput improves by at least
10%; two flat windows return to the best measured connection count. Completed
16 MiB ranges are appended and hashed in order. The outstanding window bounds
image buffers to 512 MiB, and only a contiguous prefix ever reaches `.part`,
including after cancellation or a worker failure. Existing partial downloads
remain compatible. Every range must match the requested offset, length and
complete object size; the final SHA-256 still gates the rename to `.iso`.
A server without range support uses a sequential download. Failed or invalid
ranges are retried twice with cancellable backoff, without committing their
bytes or making progress jump backwards.
Range requests have a 30-second deadline, which also bounds waiting for a
stalled connection when stopping. `KAKEHASHI_DOWNLOAD_TRACE=1` logs concurrency
changes for profiling; it does not change download behavior. The local HTTP
fixtures test out-of-order arrivals, cancellation/resume, corrupt and malformed
responses, sequential fallback and the controller's growth/backoff limits.

```sh
nix build .#kakehashi
nix develop .#kakehashi -c cargo test --locked --manifest-path apps/kakehashi/Cargo.toml
nix build .#checks.x86_64-linux.kakehashi-vm
```

`checks.kakehashi-vm` hot-plugs a 256 MiB virtual USB stick into a disposable
NixOS VM. It checks the helper refuses the VM's own disk and a stick whose
serial number differs from the plan's. Then an ordinary user starts the helper
through `pkexec` as the window does, with a polkit rule in place of a
password: once with the cancel file already present, which must leave the
stick unchanged, and once to write a random image, after which the stick's
bytes are compared from outside the VM and its last MiB must be clear. It
doesn't drive the window itself. NixOS 26.11 installs the setuid `pkexec`
wrapper only when `security.polkit.enablePkexecWrapper` is set, as Sekka's
modules do. Never point the helper at a host drive to test it.

`.github/workflows/kakehashi.yml` tests and builds on GitHub's hosted Linux,
macOS (Apple silicon) and Windows runners when `apps/kakehashi` changes, and
keeps a tarball, an ad-hoc-signed `Kakehashi.app` zip and a zip of the two
executables as artifacts. Nothing is signed with a developer identity or
published yet. The macOS and Windows platform code has been built there but
has not yet written a physical drive.

## Website

`site/` is a separate Cargo workspace whose program writes sekka.computer:
static HTML and CSS, with one small script for browsers, built from `docs/brand/tokens.json`,
the brand SVGs, and screenshots copied byte for byte. Each caption repeats the
provenance recorded in `docs/screenshots.md`. The design page's contrast
ratios are computed from the tokens when the site is built. Kanji are outlined
from Noto Serif CJK JP, so no CJK webfont ships; JetBrains Mono and Atkinson
Hyperlegible Next are served from the site with their OFL texts. Every page
states the revision it was built from (a dirty tree says so).

Pages open in Night whatever the visitor's colour-scheme preference, and in
High contrast when they ask for more contrast. The switch in the bar turns
Day on. It is a checkbox, so it works without script on the page it is on;
`site/theme.js`, loaded in each page's head so a remembered Day is drawn
from the start, keeps the choice in `localStorage` (`sekka-theme`) and sets
`data-theme` on the root element, which the stylesheet reads as it reads the
checkbox. That script is the only one, and the content security policy
allows scripts from the site alone.

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
nix run --inputs-from . nixpkgs#wrangler -- pages deploy result-site --project-name sekka --branch main --no-bundle
```

`--no-bundle` uploads `_worker.js/` as it was built, with `index.wasm` as a
compiled Wasm module. Without it, Wrangler (4.148) bundles the worker again
with esbuild, which has no loader for `.wasm`, and the deploy fails.
Only Wrangler uploads the worker's Wasm as a module. The dashboard's upload
publishes `_worker.js/` as plain files, and the download page then shows its
fallback. Pages applies `_headers` (the content security policy) to static
files and serves `404.html`. The pages promise no cookies or analytics, and
no script but their own theme switch, so leave off the Cloudflare features
that add them:
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

`nix build .#yukimi-discover --out-link result-yukimi-discover` runs Yukimi's
Discover the same way, on a Tatami VM with Genkan's application catalog and
unfree software allowed. Yukimi makes its package list from the VM's Nixpkgs,
searches for `steam`, opens the package's details and asks to install Steam
for everyone; the confirmation is cancelled, and the test checks that nothing
changed. It reads the expected text from part of each capture with Tesseract.
The pointer positions and those parts assume Yukimi's current layout. Its
captures are for review, not for `docs/screenshots.md`.
