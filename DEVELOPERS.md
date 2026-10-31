# Developing Sekka

Sekka is the source home for the ISO, Genkan installer, Aizome and Tatami desktops, Yukimi system manager, and Kakehashi USB writer. Work on a short-lived topic branch from `main`, open a pull request, squash-merge it, and delete the topic branch. There is no `stable`, `develop`, or release branch. Releases identify a commit with a version tag. The `nightly` branch, which installed systems follow, belongs to the nightly workflow alone; never push to it.

## Repository map

| Path | Responsibility |
| --- | --- |
| `flake.nix`, `flake.lock` | Public outputs and the authoritative external dependency lock |
| `installer/genkan/` | Genkan: backend and privileged helper (`rust/`), window on Aizome's kit (`ui/`), system modules, application catalog, storage tests |
| `desktop/aizome/` | Sekka's own desktop: compositor, shell, apps, NixOS module, VM checks, its docs |
| `desktop/aizome/sukumo/` | Sukumo, Aizome's toolkit: the Wayland engine and native Rust toolkit, from Sidestep |
| `desktop/tatami/` | Desktop module, configuration, Rust commands, Displays app, wallpapers |
| `apps/yukimi/` | Privileged helper, CLI, and store/configuration/system libraries |
| `apps/kakehashi/` | Cross-platform USB writer: GPUI window, privileged helper, drive and image library |
| `modules/`, `templates/` | Live ISO and the installed-system flake template |
| `agent-skills/` | Skills installed systems give coding agents, and the command that keeps them in each home |
| `nix/` | Component integration, packages, application resolution, prebuilt systems |
| `rust/`, `scripts/` | Image, VM, diagnostics, USB and release tooling |
| `site/` | The sekka.computer website generator |
| `tests/` | Generated-system configuration matrices and integration checks |
| `docs/` | Architecture, migration notes, screenshots, historical evidence, [incident records](docs/incidents/README.md), brand, patches sent upstream |

The initial import preserves component layouts and separate Cargo workspaces, and Aizome, which joined later, keeps its own workspace in `desktop/aizome` ([migration record](docs/migration.md#aizome)). There are no submodules or remote first-party flake inputs. Sukumo, Aizome's toolkit, is a workspace of its own inside `desktop/aizome` ([Sukumo](#sukumo)). `nix/components.nix` adapts the local components to the existing integration interface; its `calamares` name and the `calamares.*` options are compatibility names, not another source repository.

Sidestep (whose engine and native toolkit Sukumo continues), Upleft, Omperor, upstream forks, and Arch/Omarchy packaging stay outside Sekka. Add package definitions and pinned dependencies when the distro needs them.

## Build and check

Use x86_64 Linux with Nix and flakes enabled. The full image includes several desktops and an application cache: allow substantial disk space (a dedicated builder with at least 250 GiB free and 32 GiB RAM is a useful starting point). Component builds are much smaller. Never run installer storage tests against host disks.

```sh
nix develop
nix flake check --no-build --no-update-lock-file
nix build .#genkan .#tatami .#displays .#network .#bluetooth .#sound .#activity .#yukimi .#tools
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

The installed lock's root names only the inputs in `templates/flake.nix.in`.
Dependencies used only through Sekka, such as Superluminal, stay nested beneath Sekka;
adding one to the installed root makes Genkan reject every installation before
disk writes. `checks.x86_64-linux.template-lock` runs the real backend parser.
Genkan also seeds the selected time zone before the target's first activation.
Aizome preserves that initial zone while keeping Settings free to change it;
the VM fixture checks the installed zone after disk-only boot.
`check_installed_template.py` also evaluates Aizome, Aizome/Tatami and all-desktop
targets without changing the lock; `CALAMARES_FIXTURE` can select a packaged
`calamares-vm-fixture` instead of the local debug build.

For focused Rust work, run Cargo inside the development shell:

```sh
cargo test --locked --manifest-path installer/genkan/rust/Cargo.toml
cargo test --locked --manifest-path installer/genkan/ui/Cargo.toml
cargo test --locked --manifest-path desktop/tatami/tool/Cargo.toml
cargo test --locked --manifest-path desktop/tatami/displays/Cargo.toml
cargo test --locked --manifest-path desktop/tatami/network/Cargo.toml
cargo test --locked --manifest-path desktop/tatami/bluetooth/Cargo.toml
cargo test --locked --manifest-path desktop/tatami/sound/Cargo.toml
cargo test --locked --manifest-path desktop/tatami/activity/Cargo.toml
cargo test --locked --manifest-path apps/yukimi/Cargo.toml
cargo test --locked --manifest-path apps/kakehashi/Cargo.toml
cargo test --locked --manifest-path rust/Cargo.toml
```

Do not merge unrelated Cargo workspaces or broaden package source filters as part of routine edits. Preserve Genkan, Yukimi and Kakehashi's unprivileged GUI/privileged helper boundaries. Keep slow I/O off the interface thread.

The retained VM tooling is available through `nix build .#tools`. Run `result/bin/respin-tools qemu-test --help` for its firmware, storage, desktop, and application options. The Genkan fixture now builds from `installer/genkan/tests/vm-fixture`, not a remote installer revision. The rootless builder's static guest tools require the Rust musl target. The older reports in `TESTING.md`, `installer/genkan/RUST-TESTING.md`, and `docs/test-results/` describe their named predecessor revisions only.

BIOS fixtures keep PCI memory windows below 4 GiB with SeaBIOS's
`opt/org.seabios/pci64=0` setting. SeaBIOS 1.17 otherwise moves the NVMe
controller above that limit in an 8 GiB guest and cannot read the boot disk.
UEFI fixtures retain their normal firmware allocation.

Genkan refuses BIOS installation to disks with logical sectors other than
512 bytes during discovery and every pre-write revalidation. GRUB's BIOS
bootstrap encodes its next-stage location in 512-byte units, so a native
4 KiB BIOS read can miss the installed core image even after `grub-install`
reports success. UEFI and 512e disks remain supported. The storage VM check
exercises discovery and revalidation on disposable 4Kn and 512e disks.

Before publishing an installable image, exercise the exact image through a disposable UEFI and BIOS install, disk-only reboot, desktop login, and Yukimi configuration/update checks. Record the ISO checksum, source revision, results and outstanding hardware limitations. Passing component tests alone is insufficient evidence of an end-to-end installation.

Genkan verifies the live store's contents before evaluating the installation,
including paths needed by the low-memory, deferred-build route. After a normal
build it verifies the complete prepared runtime closure again. Nix's validity
records, successful builds and page-cache warming do not verify existing bytes:
without this check, a readable but corrupt package passes preparation, and
`nix copy` discovers its hash mismatch only after the target has been erased.
All nonzero verification results (including read errors) stop preparation.
Progress remains cancellable, and failures retain Nix's expected/actual hashes
plus kernel warnings in a root-only `/run/sekka-integrity-*.log`.
`--no-trust` skips signatures for locally built/ISO-registered paths; it does
not disable content hashing. Never use `--no-contents` or rewrite the expected
hash to make a damaged path pass.

The public `iso` package also verifies the actual compressed store before
packing it into the ISO. `nix/verified-iso.nix` extends the upstream builder's
squashfs command; `nix/verify-squashfs.nix` extracts it into a disposable local
store, compares its registration with the build's closure manifest and checks
every registered NAR hash. This runs inside the image derivation, so building
an ISO cannot skip it by omitting `flake check`. Allow temporary space for an
additional unpacked store. The upstream raw `nixosConfigurations.installer`
image is an implementation input; build and publish `.#iso`.

```sh
nix build .#genkan .#checks.x86_64-linux.genkan-store-integrity \
  .#checks.x86_64-linux.iso-store-integrity --no-update-lock-file
```

The installer regression uses the packaged helper and Determinate Nix in a
disposable VM. Its negative control shows an existing-path build and ordinary
read accepting corrupt bytes, followed by a real Nix copy reporting a hash
mismatch. The helper must reject confirmed installs with corrupt or missing
paths, preserve the entire 40 GiB target byte-for-byte and save private,
secret-free diagnostics. The image regression builds a small ISO through the
same mandatory gate and rejects changed contents, executable bits, symlinks,
missing directories/paths and altered registration. Neither check certifies
a complete release image or physical hardware.

Checks cannot guarantee against RAM faults, media changes or I/O failures
after verification. Keep Nix's copy-time checks enabled; deferred installations
still obtain unavailable downloads after formatting and can encounter network
or build failures at that point. The deferred route builds the system on the
target, so the complete system need not fit in physical RAM, but it does not
eliminate the RAM and temporary live-store space needed to evaluate the
configuration first. Do not describe the deferred route as immune to memory
exhaustion, or focused VM results as physical-hardware or full-release
qualification. The
[installer incident](docs/incidents/2026-10-24-installer-hash-mismatch-after-erase.md)
records the failure that led to this boundary and its memory-pressure tests.

The ISO embeds a prebuilt system and precache path list for every desktop
offered by Genkan, including default Aizome, and one more prebuilt system with
all of those desktops together. Choosing several desktops brings packages no
single desktop has: `calamares-session-env`, and Aizome's patched IBus frontend
when GNOME is also chosen. Without that system an installation compiled them:
for one choosing Aizome and GNOME, Genkan expected to fetch 1434 MiB
(unpacked), most of it rustc and cargo; with it, 16 MiB
([record](docs/test-results/several-desktops-media.json)). The cache's
`installed-systems` uses the same reference list. `nix build .#checks.x86_64-linux.iso-desktop-references`
compares that evaluated list with the actual GUI's `Desktop::ALL` inventory
and checks that each reference system and path list is included in the ISO
configuration. This is a packaging check; qualification still boots and
installs the resulting ISO.

Use the repository's [ISO qualification skill](.agents/skills/sekka-iso-qualification/SKILL.md)
for the complete workstation procedure. It routes agents through live-media,
storage, greeter, Aizome, Tatami and update checks with a per-feature evidence
ledger, plus the fix/rebuild/retest and publication gates. The retained fixture
harness changes installer settings, so the skill also includes a direct QEMU
path for an unmodified graphical installation and disk-only boot.

The live image's copy-to-RAM path checks available memory against the media size
plus 4 GiB for the desktop and installer. It keeps the original media mounted
until the copy is complete and preserves it on copy failure. A fallback boots
from the media, and Genkan warns when it opens to keep the USB drive or DVD
connected. `nix build .#checks.x86_64-linux.iso-copytoram` tests insufficient
memory, a real read error, and a successful switch to RAM in a disposable VM;
`nix build .#checks.x86_64-linux.iso-copytoram-initrd` separately unpacks the
actual installer initrd and runs its copy-to-RAM entrypoint and runtime tools
inside that filesystem. Initrd generation does not follow references inside
shell scripts, so both the helper and its runtime inputs must be included.
The exact ISO still needs both low-memory and successful copy-to-RAM boot tests.
Genkan accepts a RAM-backed `/iso` only after a `copytoram` boot with the
read-only squashfs store mounted from `/iso/nix-store.squashfs`; temporary
root and NixOS checks still apply. `nix build .#checks.x86_64-linux.genkan-live-media`
exercises this guard against real ISO, tmpfs and squashfs mounts in disposable
VMs, including unrelated media and installed-root refusals.

Tatami supplies mpv with system defaults that prefer OpenGL and permit its
software renderer, so recorded videos also play on a VM without 3D acceleration.
User `mpv.conf` and command-line preferences override those defaults.
`nix build .#checks.x86_64-linux.mpv-software` plays moving video to completion
in an Aizome VM using the shipped config and checks both override routes.
This component test does not replace playback through the exact ISO's normal UI.

### Aizome and IBus

When GNOME is coinstalled, its IBus dependency also needs a Wayland frontend
inside Aizome. `installer/genkan/rust/system/input-method.nix` starts that
frontend with `aizome-session.target`; GNOME keeps its own IBus service and
Xfce keeps the legacy frontend. Only Aizome's frontend package is patched;
the distribution daemon, engines and GTK/Qt modules remain unchanged. An
explicit custom IBus panel remains under the user's own integration.
Aizome's GTK/Qt input modules use Wayland only within its session environment.

The Aizome dconf profile defaults `org.freedesktop.ibus.general`'s
`use-system-keyboard-layout` to true. Its actual dconf path is
`/desktop/ibus/general/use-system-keyboard-layout`, **not** the schema ID
converted into a path. Aizome owns ordinary XKB layouts; IBus still handles
engine composition. With the backport, keys an engine does not consume reach
applications as key events instead of unsolicited text commits. This also
preserves application shortcuts without modifiers. The default is unlocked:
a user's explicit preference wins. No startup script writes the shared user
database, and GNOME/Xfce do not inherit this Aizome profile default.

The Chrome failure is reproducible by focusing a webpage input, opening a
new tab with Ctrl+T, closing it with Ctrl+W, and typing. Letters disappear
until a non-text key such as Shift is pressed. Protocol traces show text
reaching Chrome immediately; this is not queued input or compositor latency.
Chromium's browser-shortcut suppression discards character events until a
fresh keydown, while its single-character IME commit path can produce a
character without that keydown
([browser suppression](https://github.com/chromium/chromium/blob/155.0.8059.84/content/browser/renderer_host/render_widget_host_impl.cc#L1819-L1880),
[IME commit path](https://github.com/chromium/chromium/blob/155.0.8059.84/ui/base/ime/linux/input_method_auralinux.cc#L467-L499),
[related IBus report](https://github.com/ibus/ibus/issues/2855)). The traces and
source support this mechanism; the browser's internal flag was not inspected
with a debugger. Preserving ordinary key events fixes the reproduced failure
without disabling IME composition or injecting fake recovery keys.

The patch series in `nix/patches/` applies to pinned IBus 1.5.34 in this order:

| Patch | Upstream provenance and purpose |
| --- | --- |
| `ibus-wayland-forward-keycode.patch` | Physical-keycode subset of [58b74bd76d79](https://github.com/ibus/ibus/commit/58b74bd76d79bfb909eb011f5f444f474a9415c1); preserves Hangul's forwarded Space, digits and Enter through input-method-v2. |
| `ibus-wayland-serial.patch` | [a8ff75597150](https://github.com/ibus/ibus/commit/a8ff755971509f09e1d0ea52a10be1bfc13af39b); resets the commit counter only for a new input-method interface, not unrelated globals such as a newly connected display. |
| `ibus-wayland-system-keymap-dependencies.patch` | [18d8e37e112a](https://github.com/ibus/ibus/commit/18d8e37e112a155f3f80d36e4d6a845cc2e23bd8) guards removed seats; [a32d034f27c5](https://github.com/ibus/ibus/commit/a32d034f27c5f89813405e0a0f780b851b2cf893) bounds modifier-mask shifts; [ad28e5e42f4c](https://github.com/ibus/ibus/commit/ad28e5e42f4c89a4cbf546660db858f7ddd130b6) releases old resources. These are prerequisites for the keymap refactor, retained without functional changes. |
| `ibus-wayland-system-keymap.patch` | [cbaffca21e9e](https://github.com/ibus/ibus/commit/cbaffca21e9e9b61d081742058a9a6642b8d3bb4) separates compositor and engine keymaps; [6796a3dcd8e6](https://github.com/ibus/ibus/commit/6796a3dcd8e6d815bda838f7f2cb153e89f79b66) implements system-keymap/key-event forwarding; [ce066c8d9b17](https://github.com/ibus/ibus/commit/ce066c8d9b17f086cba3159eafbc4f7a92f93807) connects the existing preference to the Wayland frontend. |

Do not replace this series with **1.5.35-rc1 solely because it contains those
commits**. In the October 27, 2026 isolated tests with Chrome 155.0.8059.84,
the installed frontend failed 20/20 tab-return trials. Both the narrow
backport and 1.5.35-rc1 with the setting enabled passed 20/20, but the latter
corrupted Hangul 1.5.5's forwarded keys: Space became `n`, Enter became `t`,
and `1` disappeared. Its forwarding path subtracts eight from the supplied
physical evdev keycode. The existing adapted physical-keycode patch preserves
that code. Upgrading alone with the upstream default setting still failed
20/20 Chrome trials. The narrow series retains both previous fixes and avoids
the later forwarding/latch changes responsible for this observed regression.

Regression checks:

```sh
nix build .#checks.x86_64-linux.aizome-ibus-policy \
  .#checks.x86_64-linux.ibus-wayland-serial \
  .#checks.x86_64-linux.aizome-ibus --no-update-lock-file
```

The policy check evaluates complete system derivations for desktop combinations,
disabled IBus and custom panels, including the scope of the dconf default.
The serial check compiles the actual registry callback before and after its
patch; its negative control must reproduce the unrelated-global serial reset.
The component VM exercises real Hangul preedit, editing, spaces, digits and
Enter in Sumi, GTK and native-Wayland Google Chrome, then switches between
Aizome, Xfce and GNOME with a lingering user manager. The Chrome probe observes
a local textarea and contenteditable using CDP, but sends tested keys through
Aizome's input path. It checks both held and released Ctrl during tab cycling,
ordinary DOM keydowns, the user's ability to override and reset the default,
and composition after a tab cycle. It records whether disabling the option
reproduces lost text and Shift recovery; a future browser may fix that control.
The VM also verifies frontend failure/restart and normal logout behavior.
These are component tests, not ISO or physical docking qualification.

Known limits: keycode-zero synthetic keysyms remain unsupported. Sekka's
all-desktop configuration adds no IBus engines, so it offers the built-in
XKB/simple engines. The component test explicitly adds Hangul 1.5.5; it does
not establish support for arbitrary user-installed engines. Before adding an
engine, inspect its forwarding semantics and test native, GTK and browser
composition. Separately, changing the compositor layout while IBus is already
running can leave its virtual-keyboard path using the previous keymap. This
was reproduced with both the installed frontend and the narrow backport;
the no-IBus control updated correctly. US/German layouts and Compose configured
at session start passed isolated tests. Live keymap propagation is a separate
remaining integration issue, not fixed by this Chrome backport.

**Returning to vanilla IBus:**

1. On a Nixpkgs update, confirm the pinned source contains all the fixes above
   or documented equivalents, and check the physical-keycode units against
   the installed engines. The existence of a newer version or the original
   forward-key commit is insufficient; 1.5.35-rc1 is the known counterexample.
2. On a development branch, replace the `frontend` override in
   `input-method.nix` with `pkgs.ibus` and run the checks above using that exact
   package. Require Chrome tab-return typing and Hangul preedit, editing,
   spaces, digits and Enter to pass together, including desktop switches.
   Retain serial coverage, adapting the before/after harness when its patch
   is no longer applied. Check display changes and live layout propagation
   separately; do not claim those limitations are solved by the upgrade.
3. Once the vanilla package passes, delete the obsolete patch files and the
   override, update the provenance/validation record here, and retain the
   regression tests. Aizome still needs its frontend service and session
   environment. Keep the system-keyboard-layout policy unless a separate
   tested policy change replaces it: removing patches does not imply that
   upstream's false default has become appropriate for Aizome.

After activating a system containing this change, restarting
`aizome-ibus.service` replaces its frontend without restarting the compositor
or closing applications. An explicit false user preference remains false;
`DCONF_PROFILE=aizome dconf reset /desktop/ibus/general/use-system-keyboard-layout`
returns it to the session default. A system-generation rollback restores the
previous package and profile defaults without undoing user preferences.

## Sources and installed-system updates

The root lock retains distinct base-system and application Nixpkgs inputs, plus Determinate Nix, FlakeHub CLI, AI applications, and upstream oh-my-pi. First-party code comes directly from this checkout.

Plasma Login Manager's generic X11 startup script bypasses NixOS's session
wrapper. Xfce can then run while `graphical-session.target` remains inactive,
preventing portal activation. Sekka's default package sets the upstream CMake
`SESSION_COMMAND` to a parent around NixOS's wrapper, retaining its environment
setup and explicitly stopping both graphical targets on exit. This ends portals
before the X server closes, even when the user manager lingers. Wayland startup
and the X server command remain unchanged. An explicitly selected custom Plasma
Login package overrides this default and must provide its own X11 integration.
The XApp portal unit also follows `graphical-session.target`, like the GTK
backend, so it stops before X closes rather than failing on a stale display.
`checks.x86_64-linux.plasma-login-session` uses normal password login and Xfce
logout with a lingering user manager, checks real Settings portal calls and
clean teardown, then selects Plasma Wayland through the greeter. The original
package is both the failing control and an ordinary-priority custom-package
override check. This component test does not qualify an ISO.

Plasma Login's frontend also loads shipped defaults and packaged fragments
before administrator fragments, with the primary `/etc/plasmalogin.conf`
retaining highest priority. This lets Genkan's chosen default session override
the shipped Plasma default. `checks.x86_64-linux.plasma-login-config` compiles
the actual package's config loader against pinned KConfig and checks missing
files, fallback, fragment ordering, custom values and primary-file precedence;
the original loader reproduces the wrong default. The session VM checks fresh
Xfce preselection and logs in without changing it, then exercises a manual
Plasma selection. Custom Plasma Login packages must also supply this fix if
they use the affected upstream frontend.

With several desktops installed, `calamares-session-env.service` records the
user manager's initial environment, including custom variables. The login
screen's PAM hook restores that baseline before another desktop starts. The
snapshot service stays active after saving and does not restart on activation:
otherwise a switch or rollback in Tatami overwrites the baseline with its
`XDG_MENU_PREFIX=hyprland-`, breaking the next Xfce applications menu.
`nix build .#checks.x86_64-linux.session-environment` exercises real GDM password
login, activation and rollback in Tatami, Xfce's menu, and subsequent Wayland
sessions with one lingering manager; the original unit is its failing control.
The activation cases first observe natural retirement of GDM's previous greeter
and its runtime directory. A visible desktop can precede that delayed teardown;
activating while its GNOME targets stop can fail with a conflicting transaction.
The check covers activation in a settled session, retains every zero-exit-status
assertion, and does not certify activation concurrent with another session's
logout. It neither stops the greeter nor retries a failed activation.
After upgrading from the old inactive unit, start a new user manager, normally
by rebooting. Activation can start that old inactive unit in the current
desktop, and the fix does not repair an already contaminated snapshot. Manually
restarting the snapshot service during a desktop still replaces it and is not
a reset operation. Restart the user manager after ending its sessions instead.

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

XApp's Settings portal 1.1.3 can crash during Xfce login when an application
activates the portal before the settings daemon sends its theme and font.
Its synchronous GTK theme notification constructs a style context; reading
the new font invalidates that context before GTK has initialized its CSS node.
`nix/overlays/xapp.nix` carries a local source patch that coalesces the theme
notification at GLib idle priority, after the pending settings events. It
preserves Settings reads and change signals, without delaying portal startup.
`modules/portal-fixes.nix` applies it to installed desktops and to the live
media. Every live boot entry runs Aizome, the software one included, so the
media carry Xfce and the portal only in their prebuilt installed systems.
Review the patch when updating XApp; remove it when upstream handles this case
correctly.

`nix build .#checks.x86_64-linux.xapp-settings .#checks.x86_64-linux.xapp-packages`
checks the real backend under private X and D-Bus sessions. The original
package must crash for the same theme/font batch; the patched package must
read two distinct accent colors and emit their change notifications. Package
selection assertions cover installed Xfce and the media's prebuilt Xfce and
every-desktop systems, which must select the patched portal and no other XApp
package, and every live boot entry, where any XApp portal must be the patched
one. This is component evidence, not ISO qualification.

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
least recently used. `nix/overlays/default.nix` lists the shared overlays for the
flake's package set, `overlays.default` and `modules/hyprland-fixes.nix`.
`nix build .#checks.x86_64-linux.screenshare-sessions` runs the patched and
the unpatched Nixpkgs Hyprland in two disposable VMs, each with a headless
output at scale 2. It counts Hyprland's own session trace lines while a
screencopy client shares a region, the whole output and a window, holds many
regions at once, and connects repeatedly. The unpatched compositor must show
both leaks: that is the negative control. Remove the patch when a Hyprland
release includes the fix, after this check passes with the new upstream
package; review any patch failure rather than skipping it.

Hyprtoolkit 0.6.0 assumes its first advertised DMA-BUF format can be allocated
and dereferences a null buffer when it cannot. This crashes Hyprlauncher on
software graphics. `nix/patches/hyprtoolkit-buffer-allocation.patch` preserves
successful allocations, retries with advertised ARGB8888, and can use the same
device's primary DRM node when its render node cannot allocate software GBM
buffers. The renderer keeps its original device. Failed allocation never becomes
a null-buffer dereference. The shared overlay rebuilds Hyprlauncher against it.
`nix build .#checks.x86_64-linux.hyprlauncher-software` opens the launcher with
Super+R, searches for and launches an application, then reopens, rescales and
cancels on software graphics with both supported kernel variants.
The test asserts the applied monitor scale and actual buffer sizes, reads the
visible launcher/result text within its mapped layer, and preserves full-screen
captures. Its negative control rebuilds the launcher with only this toolkit
patch removed and must reproduce the original null-buffer crash. The package
selection check covers Hyprtoolkit and Hyprlauncher in ISO references, installed
systems and standalone desktop modules. These component checks do not certify
an ISO or accelerated graphics. Remove the patch after an upstream update
passes the regression; do not skip a patch failure during an input update.

`nix/patches/hyprland-capture-cursor.patch` is a separate local fix for
software cursors appearing in screenshots and OCR even when the capture
protocol excludes them. Hyprland drew the cursor into its shared monitor
image and assumed software captures already contained it. The patch uses
the existing separate unmodified image while sharing or mirroring, omits
cursor writes from that image, and adds the pointer explicitly when a
capture requests it or a display mirrors another output. It preserves the
display's render order and uses the existing GLES 3 draw-buffer support.
Cursor overlays use the zoom transform saved with that image; capture crops
are applied afterward, without advancing the detached zoom camera. Mirrored
cursors now fade their alpha separately during source DPMS transitions;
intermediate compositing can differ, while black and awake endpoints are
preserved.
`nix build .#checks.x86_64-linux.capture-cursor-vm` runs original and patched
compositors on software-rendered virtio GPUs. Pixel comparisons cover both
ext-image-copy-capture and wlr-screencopy, cursor inclusion and exclusion,
movement, scale and rotation, blank OCR, decoded software recordings,
ordinary and detached zoom with cropped captures, and the actual mirrored
VM display including destination rotation, zoom and DPMS sleep/wake. The
original must reproduce the cursor leak. These are component checks; they do
not qualify a release image or physical graphics hardware. Remove this patch once an upstream fix passes
the same regression, and review any patch conflict during input updates.

Tatami's screenshot completion notice has a one-shot `notify-send` action
owner, which exits when the card expires or closes. Its Mako rule therefore
excludes it from history: restoring it would advertise an Edit action with
no listener. The image remains saved in the user's Pictures directory;
ordinary notification history is unchanged. The
`screenshot-notifications` flake check runs real Mako, Tatami and Satty in a
VM: fresh Edit opens the saved image, expiry leaves no dead screenshot card,
and an ordinary notice still restores. Overriding just this history setting
reproduces the old dead action as the negative control.

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

Tatami's Sound window (`desktop/tatami/sound/`, `tatami-sound`) is a separate
GTK 4/libadwaita Cargo workspace, laid out after Aizome's Sound settings
(Settei's `panes/sound.rs`) on one page: each device list with the default
checked and its controls beneath, then applications and card profiles.
`tatami audio` (also `tatami sound`) opens it from the bar, menu or shortcut.
It uses the session's PipeWire PulseAudio interface through `pactl`, packaged
with the window. Moving an application's stream is the exception, through
`pw-metadata`, also on the window's path. pipewire-pulse's move only sets
the stream's `target.object` and `target.node` in PipeWire's default
metadata for WirePlumber to follow (-1 for the default device, so the stream
then follows the default), and PipeWire 1.6.9 acknowledges it before PipeWire
has those writes (`reply_simple_ack` in `do_move_stream`, where volume, mute
and default changes reply after a sync). `pactl` disconnects on the
acknowledgement, and the writes can go with its connection: about one run in
eight of the integration check, which makes five moves, left a stream where
it was, and two in four with PipeWire's debug logging on. `pw-metadata` sets
the same keys and syncs before it exits.
A stream with `node.dont-reconnect`, which pipewire-pulse refuses to move,
has its device menu disabled. Review this when PipeWire changes
`do_move_stream`. All reads and changes run off the GTK thread, with a
four-second deadline per command. Changes are sent one at a time in order, a
slider's values coalesced to its latest, and a reading is not shown while a
change waits. `pactl subscribe` prompts a reading after each change the
service reports, ignoring client connections (each reading makes six), and a
reading every four seconds catches anything else; the subscription restarts
every two seconds while the service is away. A section is rebuilt only when
what it lists changes; volumes, mute, balance, routing, defaults and active
profiles update in place.

A volume change keeps the balance: off center, it sets each channel as
`pa_cvolume_set_balance` would. The input level comes from the window's own
libpulse connection (`meter.rs`, after `aizome-sys`'s meter): a record stream
on the default input with `PA_STREAM_PEAK_DETECT`, 25 peaks a second, which
`pactl` cannot open. The package and the development shell link libpulse for
it. The stream is named "Peak detect"; the backend leaves level meters out of
Applications, with internal virtual-device streams, and an input counts as in
use only when an application records from it. `pactl` 17 reports card-profile
availability as a boolean and a monitor's sink as null, so monitors are
recognized by `device.class` and the parser accepts the older strings too.
Card profiles are ordered by priority, with Off last.

The window shares Tatami's GTK stylesheet through `tatami-sound/style-dark.css`,
which adds the tiles' and the level meter's Tokyo Night colors. Changing a
default does not explicitly move existing streams; routing is a separate
application control. It does not keep a second settings file: PipeWire and
WirePlumber own persistence and device policy.

```sh
nix build .#sound .#checks.x86_64-linux.sound-integration --no-update-lock-file
```

The integration check runs the packaged window under Xvfb with private
D-Bus, PipeWire and WirePlumber sessions and virtual audio devices. It checks
the device lists and defaults (the controls follow a new default), volume and
mute in both directions, balance and that a volume change keeps it, volume
above 100%, the level meter rising with sound and falling back, that the
meter is neither listed nor makes its input in use, that the window does not
read in a loop while the meter runs, playback and recording volume, mute and
routing (to the default device and to another), hotplug, service recovery, single-instance activation, and that
Escape closes the window without leaving `pactl` behind. It chooses from
popovers with the keyboard, as GTK reports a popover's items above where they
are drawn. Hardware monitors are disabled. Card/port parsing is covered by
unit fixtures shaped like `pactl` 17's output; physical ports and Bluetooth
profile changes still need hardware validation. Its captures are development
evidence using simulated devices, not screenshots of physical hardware. This
is a component check, not a Tatami session, image build, VM installation or
physical sound test.

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

Displays fits its floating window inside the logical workspace, excluding the
bar's reserved space, when opened and after monitor layout or scale changes.
Scale choices wrap and sliders sit below their labels; settings remain
scrollable at the larger text scales.
`nix build .#checks.x86_64-linux.displays-fit-vm` runs the shipped Tatami session
at 1280×800, selects scale 2 through the window, checks its bounds and scrolling,
and returns to scale 1 through the visible controls. It also checks the offered
scales and reopening the window. This component test does not qualify an ISO.

Network uses the same workspace geometry. The display watcher fits it for
three seconds at startup and after a Network window opens, covering initial
size negotiation from direct desktop-entry and panel launches. Layout changes
fit both settings windows; unrelated floating windows keep their policy.
Network's pages allow horizontal scrolling on very narrow workspaces.
`nix build .#checks.x86_64-linux.network-fit-vm` covers changing scale with an
open window, reopening through the network panel, usable bounds, scrolling to
the connection's bottom controls, and the header's Back and Close buttons at
1280×800 with scales 1, 1.25, 2 and 3.2.

Pop Out (Super+O) explicitly enables floating, fits its preferred 1300×900
size to the window's own logical monitor work area, and pins it. A second
invocation unpins and returns to tiling, including for windows that were
already floating before activation. It uses the same geometry calculation
as Displays, including scale, rotation, monitor origin and reserved panels;
other floating windows keep their existing policy.
`nix build .#checks.x86_64-linux.window-pop-vm` exercises the actual shortcut
from tiled and floating terminals at scales 1 and 2, pinning across workspaces,
return to tiling, and a simulated rotated monitor at a nonzero origin.
The test also runs against the predecessor desktop module as a negative
control. It is component GUI evidence, not ISO qualification.

Tatami's software recording fallback mixes desktop and microphone audio in a
temporary PipeWire source owned by the recording helper. It resolves the actual
default endpoints, connects both before starting, and removes the temporary
graph when the recorder or helper exits. Software recording requests continuous
frames so a static screen cannot prevent stop/error cleanup from finalizing the
video; this uses more capture work than damage-only recording. `nix build
.#checks.x86_64-linux.tatami-recording` checks decoded independent tones through
real PipeWire and wf-recorder, single-source/no-audio recording, cancellation,
startup failure, interrupted processes and recovery. The component fixture
supplies the capture rectangle; it does not qualify the exact ISO's GUI journey.

`nix/installed-lock.nix` extends that external lock with Sekka itself. A clean Git build pins its commit and source hash; a dirty development build pins the exact source copied into the Nix store. The source is included on the ISO. The installed flake follows `github:bitemyapp/sekka/nightly` for subsequent updates, and every other input follows Sekka's own (`inputs.nixpkgs.follows = "sekka/nixpkgs"`, and likewise `determinate`, `fh`, `applications`, `ai-apps` and `omp`). Sekka's `superluminal`, `tracy` and `uta` inputs are the exception: only Sekka's own outputs use them, as catalog packages (`nix/components.nix`), so the installed flake doesn't name them and they stay locked beneath Sekka, where updating it moves them just the same. Updating Sekka therefore moves them together, and only to revisions the nightly workflow has checked and confirmed a fresh system downloads entirely: it moves `nightly` only then, so nothing upstream publishes reaches an installed system untested, and no update compiles. The exception is an unfree application such as Superluminal: Sekka's binary cache carries only free ones, so updates download it from its maker and build its small wrapper on the machine. The `tracy` input is bitemyapp's fork of Quince-Pie's flake, which nothing updates on its own: the nightly signs and publishes Tracy's build, so only upstream changes reviewed and pushed to the fork reach it. The `uta` input names its revision, so the nightly's input update never moves it: Sekka builds Uta against its own Aizome (`nix/uta.nix`), so a new Uta is a change to `flake.nix`, built with the Aizome it will run with.

Update the distro through **Yukimi → Updates → Update** or `sekka-update`.
Both use the privileged helper transaction: recognize/migrate the flake,
back up the configuration, update the selected inputs, and build the host once.
A flake following Sekka's `main` (or its default branch) is moved to
`nightly`, and each of the other inputs still at the address Sekka's
installations have used is made to follow Sekka's (`yukimi_config::sekka`).
Early updates are the exception: `github:bitemyapp/sekka?ref=main`, written
only when chosen (Early updates in Aizome's Yukimi, `sekka-update --early`,
or the helper's `--follow sekka=main`), stays, so the system gets every change
as soon as it's merged, before the nightly checks; whatever the binary cache
doesn't have yet builds on the machine. `--nightly` (`--follow sekka=nightly`)
goes back.
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
The system's own choices travel with an update or go alone: the Nix module
declares which settings of the main configuration file Yukimi may write, and
to which values (`programs.yukimi.choices`, listed in `/etc/yukimi/system.json`;
Sekka declares `calamares.desktops`, `calamares.defaultDesktop` and
`calamares.loginScreen` in `installer/genkan/rust/system/desktops.nix`, and
`calamares.bootSplash.enable` in `installer/genkan/rust/system/boot-splash/`).
`yukimi-helper change --choose SETTING=VALUE[,VALUE]` (and `sekka-update
--choose`) refuses any other setting or value, writes a string, a list of
strings or a boolean as declared (a `boolean` choice's options are `true` and
`false`, given as strings so older readers of `system.json` still parse it),
and builds once with the rest of the change, so choosing
Aizome's login screen is a click and a password, with the same switch-or-boot
planning and Restart button as an update. A changed login screen always stages
for the next start.
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
early updates and the system's declared choices (and refusing undeclared
ones), refusing to start without room, failed builds, cancellation, concurrent operations, closed progress pipes, unsafe
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

The cache workflow publishes every commit that reaches `main` (see Caching
main), and the nightly workflow every revision it moves `nightly` to (see
Nightly builds); the release workflow does not touch the cache. To publish by
hand, from a clean checkout of reviewed `main`:

```sh
nix run .#push-cache
```

It builds `genkan`, `tatami`, `displays`, `wallpapers`, `yukimi`, `kakehashi`, `uta`,
`uta.source` (Uta's GPL-3.0-only build's sources, published beside it),
`aquamarine`, `hyprland`, `tools` and `installed-systems`, and signs their
complete runtime closures into a temporary local cache: Nix only adds a path to
a binary cache that already holds its references. `installed-systems` is an
installed system for each desktop the media offers (`respin.referenceDesktops`),
one with all of those desktops together, and one with every free catalog
application, so everything Sekka's modules
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

## Genkan

Genkan is two Cargo workspaces in `installer/genkan`:

- `rust/` (`calamares-nixos`): the backend (discovery, validation, the plan,
  the Wi-Fi snapshot, the helper's session protocol) and the privileged
  helper, `calamares-nixos-helper`. It has no interface dependencies; the
  helper, the VM fixture and the storage tests build from it alone.
- `ui/` (`genkan`, executable `calamares-nixos`): the window, an Aizome app
  on `aizome-kit`. It depends on the backend and on
  `desktop/aizome/crates/aizome-kit` by path, and Cargo reads Aizome's
  workspace manifest for the versions the kit inherits. It isn't a member of
  Aizome's workspace, so Aizome's package doesn't carry the installer and
  neither rebuilds the other except through the kit's crates and Sukumo.
  Its lock was seeded from Aizome's, giving the kit and Sukumo the
  versions Aizome builds them with; when Aizome's lock moves their
  dependencies, move Genkan's to match.

`installer/genkan/nix/package.nix` builds the helper from `rust/`'s lock and
the window from `ui/`'s with the kit and Sukumo, then installs the
helper beside the window: the window starts it through pkexec from its own
directory, and the Polkit policy names that path. The package keeps the GTK
interface's executable, desktop entry, application id and policy names.

The window is a kit program (`ui/src/app.rs`): Genkan's state and the
messages that change it. Disk discovery, memory, time zone detection and
GitHub run on the kit's workers; page cache warming, review (validation, the
Wi-Fi snapshot, starting the helper) and the helper's events come from
threads of their own as messages. Answers to a superseded request are
dropped, and a superseded helper session with them, which cancels it. The
views are in `ui/src/view/`; the world map and the partition bar are
canvases. On the live media the window opens with the Aizome session, and
says so when copy-to-RAM fell back to the media.

```sh
nix build .#genkan
nix develop
cargo test --locked --manifest-path installer/genkan/ui/Cargo.toml
```

The tests in `ui/tests/pages.rs` drive the program headlessly with the kit's
harness, on sample data: every page draws and says what it should, and the
checks between steps, the review's confirmation and the rule against closing
while installing hold. Set `AIZOME_FONT_DIR` (`nix build .#aizome-fonts`) for
Aizome's type and `TZDIR` for the map; the package's check phase sets both.
`GENKAN_PICTURES=dir` also writes each page in Night and Day.

In debug builds, `CALAMARES_UI_PREVIEW=welcome|disk|account|github|desktop|applications|location|review|ready|prepfail|install|done|failed`
opens the window on that page with sample data, touching no disks, network or
helper, and `CALAMARES_UI_SIZE=800x600` sizes it. Release builds read
neither. Outside the live media only previews are useful: reviewing reads the
media's root-owned settings and starts the helper through Polkit.

## Aizome

Aizome's source is in `desktop/aizome`, a Cargo workspace of its own with its
NixOS module, VM checks and documentation (`desktop/aizome/docs/`). It moved
here from [bitemyapp/aizome](https://github.com/bitemyapp/aizome), which keeps
its history ([migration record](docs/migration.md#aizome)). Changes to Aizome
are Sekka pull requests now, and reach the nightly with the rest of Sekka.

`desktop/aizome/nix/default.nix` gives what Aizome's flake gave: its packages,
module, checks and development shell, built with Sekka's Nixpkgs.
`nix/components.nix` hands them to the rest of Sekka as `inputs.aizome`, as it
does Tatami's and Yukimi's.

```sh
nix build .#aizome .#aizome-fonts
nix develop .#aizome
nix build .#aizome-checks.session -L
```

The shell observes application launch attempts through `aizome-sys`:
process creation and exit are separate events, and only a compositor window
event confirms that a window opened. D-Bus acceptance and successful
single-instance handoffs keep their narrower meaning. Desktop-entry launches
and commands launched by the shell share this path. Raw compositor `spawn`
bindings and processes started from terminals remain outside it.
Toolchest ▸ System ▸ Application Launches shows the last 64 attempts; early
failures and unconfirmed graphical launches offer Show Details. The shell
also exposes outcomes through `aizome-shell msg state`; `aizome-shell msg
'{"request":"open","what":"launches"}'` opens the same details window.

Each observed launch writes directly to journald with a unique
`SYSLOG_IDENTIFIER=aizome-launch-…`. The details window queries that exact
identifier on a worker, capped at 200 entries and 64 KiB with a five-second
deadline. Its **Open in Logs** runs `aizome-nisshi --launch` with
that identifier, through the same observed launch path. It never displays other attempts' output, captures the user's
environment, or pipes application output through the shell. The latter
preserves application lifetime when the shell is replaced. Accepted D-Bus
activation may have no output here because the bus owns that process.
History is bounded in memory and resets with the shell; the journal keeps
output according to its own retention. Failure details are local to the
user and are copied only on request.

The Aizome package's mandatory check phase runs the shell and system-library
unit tests, including delayed process failure, literal command arguments,
notification actions, details rendering and handoffs. For focused checks:

```sh
nix develop .#aizome -c cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-sys -p aizome-shell --lib
nix develop .#aizome -c cargo build --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shell
nix develop .#aizome -c cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shell --lib journal_output_is_isolated -- --ignored
nix shell --inputs-from . nixpkgs#python3 nixpkgs#grim -c python3 desktop/aizome/scripts/test-startup.py \
  --shell desktop/aizome/target/debug/aizome-shell --evidence .work/startup-evidence
```

The journal check writes only uniquely identified fixture output. The last
command uses the named shell with installed Aizome and Koe (override
`--compositor`, `--ctl`, `--koe` for other binaries), private home and runtime
directories, and disconnected session/system buses. It launches a delayed
failure and a missing program through the real palette, checks that stderr
is visible in Details, then opens a real Koe window without enabling
dictation. Its screenshots and JSON identify component evidence, not an ISO
or physical-hardware qualification.

Aizome's session launcher owns desktop cleanup even when logind terminates
only the login scope, as Yukimi's Sign Out does. The compositor runs in the
user manager, outside that scope; HUP, INT and TERM must therefore stop the
graphical session and clear its imported environment before the launcher
exits. Its `systemctl --wait` runs asynchronously so the shell can handle
signals immediately; cleanup terminates and reaps that client before stopping
the session, including when it has not yet submitted its start request.
The launcher holds a per-user runtime lock through startup and cleanup, and
refuses active, activating or deactivating services left by older launchers.
A rejected duplicate login never installs cleanup traps. See the
[orphaned-session incident](docs/incidents/2026-10-28-aizome-orphaned-session.md).

`nix build .#aizome-checks.session-launcher .#aizome-checks.session-launcher-systemd .#aizome-checks.greeter -L`
checks the launcher with isolated service stand-ins, startup/cancellation
ordering with real systemd and a stand-in compositor in a disposable VM,
then real logind termination and password login in the graphical greeter VM.
The startup check gates requests both before submission and before compositor
readiness, rejects overlapping launches and verifies subsequent login.
The greeter VM keeps the user manager alive, checks that the old desktop
and its environment end, and
logs the same user back in. These are component checks; they do not qualify
an ISO or physical suspend/resume. An uncatchable SIGKILL or a dead user
manager cannot run the launcher's cleanup trap.

`aizome-checks` holds the 24 VM checks Aizome's flake had, under the names it
gave them. They are in `legacyPackages`, not `checks`, so `nix flake check` and
CI don't run them, just as they didn't run in Aizome's repository. Sekka's own
Aizome checks (`aizome-login`, `aizome-greeter-desktops`, `aizome-ibus`,
`aizome-ibus-policy`) still run with the others.

The package version is a plain `0.1.0`. Aizome's flake added its own revision
to it. Inside Sekka that revision would be Sekka's, and every Sekka commit
would rebuild Aizome. The source hash already tells the builds apart.

The workspace builds against Sukumo, Aizome's toolkit, in
`desktop/aizome/sukumo` ([Sukumo](#sukumo)).

The catalog's Uta, from [bitemyapp/uta](https://github.com/bitemyapp/uta),
depends on `aizome-kit` by path, and Sekka builds it against the kit here
and Sukumo (`nix/uta.nix`), with Uta's own `Cargo.lock`. A change to the
interface or dependencies of the kit or Sukumo must keep
`nix build .#uta` building; the `applications` check builds it too. Uta's
source holds only the kit's and Sukumo's crates, so a crate the kit comes to
depend on joins the list in `nix/uta.nix`.

That `Cargo.lock` lives in bitemyapp/uta, at the revision `flake.nix` pins,
and Nix vendors exactly the crates it names. A kit or Sukumo change that adds
or bumps a crates.io dependency, or gives the kit a new path crate, therefore
breaks `.#uta` until Uta's lock is regenerated and re-pinned, even though
nothing in Sekka's own locks is wrong. `.#uta` is built by the cache
workflow's and the nightly's first step, by `nix flake check` and by
`push-cache`, so a broken Uta stops publication for that push to `main` and
holds `nightly`, with every other input update, until it is fixed. Build
`.#uta` before merging a kit or Sukumo change. When it fails, re-pin Uta in the
same pull request.

Re-pin from a `sekka` branch of bitemyapp/uta, never from its `main`. Uta's
own flake builds against Aizome's former repository with the same
`Cargo.lock`, and a lock regenerated against the kit here need not build
there. The first time, create `sekka` from the revision `flake.nix` pins.

1. In a scratch directory, put a checkout of bitemyapp/uta's `sekka` branch at
   `uta/` and this worktree's `desktop/aizome` at `aizome/` beside it (a
   symlink works), which is the layout Uta's path dependency
   `../aizome/crates/aizome-kit` and its `nix/package.nix` expect.
2. In `uta/`, inside this worktree's `nix develop .#aizome` shell, run
   `cargo update --workspace`, which adds what the kit and Sukumo now need
   and leaves the other locked versions alone, check that
   `cargo build --locked` works, and commit and push `Cargo.lock` (with any
   source change Uta needs) to `sekka`. Cargo applies only the `[patch]`
   sections of the workspace it builds, Uta's: a patch the kit or Sukumo
   relies on in `desktop/aizome/Cargo.toml` must be repeated in Uta's
   `Cargo.toml`.
3. Put the new `sekka` revision in `inputs.uta.url` in `flake.nix`, run
   `nix flake update uta`, update the pin in `docs/applications.md`, and build
   `.#uta`, `.#checks.x86_64-linux.uta-rustypipe` and
   `.#checks.x86_64-linux.uta-installation`.

To move to a newer Uta, merge its revision into `sekka` and run steps 2 and 3.
Sekka's lock reaches Uta's `main` only together with a change to Uta's flake
that builds it against the same kit, made and built in Uta.

With RustyPipe, Uta's build is GPL-3.0-only. `share/doc/aizome-uta` holds
RustyPipe's and EJS's notices and a `SOURCE` note naming Uta's repository,
revision and narHash, and where Sekka's recipe lives. The note leaves out
Sekka's own revision, which would rebuild Uta on every Sekka commit; it points
to the `sekka` input of an installed system's `/etc/nixos/flake.lock`
instead. `uta.source` links the exact sources it was built from: the pinned
Uta source (with its vendored, patched RustyPipe), the kit and Sukumo fileset
from `nix/uta.nix`, and that recipe.
`push-cache` publishes `uta.source` beside `uta`, so the cache that serves the
binary also serves its source; the crates.io sources are named, by checksum, in
Uta's `Cargo.lock`. `checks.x86_64-linux.uta-rustypipe` runs
`aizome-uta --version` in the build sandbox to confirm RustyPipe is enabled,
and checks the notices, the note and the source links. The workflows only
build Uta: no Uta code runs in a job that holds the cache's secrets.

Uta is an optional download in Genkan and Yukimi, with RustyPipe enabled. Its
catalog `preload: false` excludes it from `application-cache` and the ISO's
application closure lists; it remains in the free-application systems published
to the binary cache. New catalog entries preload by default. Keep the two
policies separate: catalog availability does not require offline media inclusion.
Run `nix build .#uta .#checks.x86_64-linux.uta-rustypipe .#checks.x86_64-linux.uta-installation`
after changing the pin or preload policy. This checks the package, package
selection and evaluated ISO contents; it does not certify an ISO boot or
installation. Because the ISO doesn't carry Uta, installing it from that ISO
needs the network and a cache.sekka.computer entry for the exact Uta that
ISO's revision builds; otherwise Genkan compiles it in the live session. ISO
qualification checks that entry first
(`.agents/skills/sekka-iso-qualification/references/prepare.md`).

Aizome's `just` recipes and `aizome-dev` enter the worktree's
`nix develop .#aizome` shell, including when called from a different Nix
shell. Without `--checkout`, `aizome-dev` uses `$AIZOME_CHECKOUT`, then the
current Sekka worktree, then `~/work/sekka/desktop/aizome`. `--checkout`
accepts either the Sekka root or its Aizome component. `CARGO_BUILD_JOBS`
can limit build concurrency without changing the core's ABI.

Yukimi has one native interface in `desktop/aizome/apps/yukimi`, used by
Aizome, Tatami and the other desktops. `apps/yukimi` keeps the privileged
helper, CLI and canonical backend libraries in its separate Cargo workspace.
Aizome's build exposes a `yukimi` runtime output containing only the window,
launcher, desktop entry and icon; the main output links to it. The root
`yukimi` package combines that output with the independently built helper.
Tatami therefore does not retain Aizome's compositor, shell or other apps.
The `yukimi` and `aizome-yukimi` commands launch the same app, with Aizome's
Archivo and IBM Plex Mono fonts available even outside its session.

Wayland sessions launch directly. On X11, the launcher opens a private Weston
X11 window with its kiosk shell and software renderer. Weston follows the
app's lifetime; closing either window ends the nested session. This adds
another process and buffers only on X11. `programs.yukimi.*`, `sekka-update`
and the helper's Polkit action remain compatible. The app uses bundled SQLite;
the helper workspace uses system SQLite. Slow reads and changes stay off the
interface thread, and only the authenticated helper changes the system.

Aizome's package still copies only its component (with Sukumo), shared backend
and workspace manifest. Helper-only changes do not rebuild the window.
`aizome-dev watch` includes the shared backend. Component GUI checks run with
`nix build .#checks.x86_64-linux.yukimi-tatami` and
`nix build .#checks.x86_64-linux.yukimi-x11`; these are not ISO qualification.

The [October 2026 resource comparison](docs/test-results/yukimi-unified-2026-10-27.json)
records three trials of each frontend in a private GPU-backed Tatami compositor
on Intel integrated graphics, plus a Tatami-only software-rendered VM with no
Aizome session. At 2560 × 1600, scale 2, the animated Overview's median app PSS
was 87.3 MiB for GTK and 64.9 MiB for native Yukimi. App CPU was 7.3% and 3.9%
of one logical core; compositor CPU was 1.5% and 2.7%. Render-engine activity
was 2.7%/1.8% for GTK/the compositor and 0%/3.3% for native/the compositor.
The native app draws into shared-memory buffers; the compositor still uses
the GPU. On Installed, app PSS was 93.2/65.7 MiB and app CPU 0.4%/0.0%.
Including compositor changes and subtracting each trial's empty compositor,
Overview added a median 84.2 MiB PSS for GTK and 66.7 MiB for native Yukimi.

This GPU has no dedicated VRAM. Overview's resident GTT buffers were
123.0/202.3 MiB for GTK/the compositor and 0/158.4 MiB for native/the compositor.
Shared buffers overlap between processes: these are not additive private
allocations or extra amounts to add to PSS. The isolated VM's app PSS was
306.3/28.3 MiB on Overview, confirming that native Yukimi does not require
another Aizome app to keep its RAM cost low; software rendering accounts for
much of GTK's higher VM cost. Startup, cold package indexing, discrete GPUs
and a physical Tatami login are outside these measurements.

The standalone uncompressed runtime closure grew from 896.2 to 1058.4 MiB,
including Weston for X11. Copying only the eight bundled font files avoids
retaining 382.3 MiB of unused font families. These are store sizes, not RAM or
the incremental size of an already installed Tatami system. Builds and VM
checks ran on `wx-workstation`; hardware sampling used the exact built ELFs.

The shared backend's Aizome completion check uses the local
`desktop/aizome/crates/aizome-ipc` protocol crate. The helper package includes only
that crate and its workspace manifest from Aizome, plus the session launcher
used by its environment-record regression. Keep these inputs narrow rather
than copying the whole desktop into the helper build. The frontend performs
session inspection without helper privileges; the helper's system-only check
never connects to a compositor or follows a user's PATH. See
[reload and completion advice](desktop/aizome/docs/reload.md#yukimis-update-advice)
for the evidence used and remaining limits.

Shiori, Aizome's PDF reader (`desktop/aizome/apps/shiori`), draws pages, and
reads their text, links and the outline, with hayro (Apache-2.0 OR MIT), a
PDF renderer in Rust, from [bitemyapp/hayro](https://github.com/bitemyapp/hayro)'s
`aizome-perf` branch. The fork is about twice as fast as PDFium over the
pdf.js corpus and gives an encrypted document's permissions
([record](docs/test-results/hayro-fork-2026-10-28.json)). Shiori's
`Cargo.toml` pins it by revision; it brings bitemyapp/zune-image's
`scaled-idct` zune-jpeg (pinned by `Cargo.lock`), and Aizome's workspace
patches moxcms to bitemyapp/moxcms's `gamma-table-cache-0.8`
(awxkee/moxcms#209). Each git dependency has a hash in `cargoLock.outputHashes`
in `desktop/aizome/nix/package.nix`. To move the fork, push it, change both
revisions in Shiori's `Cargo.toml`, run `cargo update -p hayro`, and put the
new `nix-prefetch-git --url https://github.com/bitemyapp/hayro --rev REV
--fetch-submodules` hash in `outputHashes`.

The zune-core and moxcms entries in the `[patch.crates-io]` of
`desktop/aizome/Cargo.toml` aren't Shiori's alone: Cargo applies a patch to
the whole workspace. Everything that reaches `image` builds it against the
forks: the compositor (`aizome-comp`, `aizome-comp-core` and
`aizome-comp-logic`, through Sukumo's `sidestep-ui` and `sidestep-engine`),
`aizome-kit` the same way, and so the shell and every app (Hako and Shashin
use `sidestep-engine` directly too, and Shashin `image`). `image` still
decodes JPEGs with crates.io's zune-jpeg, compiled against the forked
zune-core, and manages colour with the forked moxcms. Sukumo's own
workspace has its own `Cargo.lock` and no patch. A change to either fork
can change how wallpapers, icons and thumbnails decode, so moving one needs
Shashin's tests, the `aizome` package's check phase and a look at a
session, not only Shiori's tests:

- zune-core is named by its branch, `scaled-idct`, as hayro names its
  zune-jpeg, so a plain `cargo update` moves both (one repository) to the
  branch's head. Choose the commit with `cargo update -p zune-core
  --precise REV`, and replace the zune-image hash in `outputHashes`.
- moxcms is pinned by `rev`: change it, run `cargo update -p moxcms`, and
  replace its hash.
- The moxcms patch only saves work (its tables are the same), so dropping
  it with its hash costs Shiori time on ICC-based and Lab colour, nothing
  else; it can go once awxkee/moxcms#209 is released.
- The zune-core patch can go only once hayro's zune-jpeg and `image`'s
  agree on a zune-core Nix can vendor, as it can't hold two crates of one
  name and version: the fork's work released and hayro's fork moved to
  it, or the fork's zune-core given a version of its own.

Only the worker process, `aizome-shiori --worker`, one for each open
document, runs hayro: a document can panic it or keep it busy, and the
SIMD decoding Shiori turns on (hayro-syntax's `unsafe` feature) runs
`unsafe` code on the document's bytes. A panic costs the job it was on (a page it was drawing shows as
unreadable at that size; one whose text it was reading has none); the
reader restarts a worker that dies or spends too long on a page, and
refuses a drawing it couldn't have asked for (too large, or of a page the document hasn't). Fonts a document names but doesn't carry come from hayro's built-in
Foxit fonts, except Chinese, Japanese and Korean ones, which the worker asks
fontconfig for. Its tests start real workers over generated documents and a
qpdf-encrypted fixture:

```sh
cargo test -p aizome-shiori
cargo test -p aizome-shiori --release --test budget -- --nocapture
cargo run --release -p aizome-shiori --example corpus -- DIR
nix build .#aizome-checks.shiori -L
```

The last runs the packaged reader in an Aizome VM, including a worker
killed and replaced.

The engine evaluation (PDFium against hayro over the pdf.js corpus, and
why MuPDF and Poppler were ruled out by licence), the fork's work, and the
reader's design are in [Shiori's notes](desktop/aizome/docs/shiori.md).

The lock screen follows enabled Wayland outputs throughout lock acquisition
and the confirmed lock. Every display has a focused password field sharing
one authentication state; mirrors show their source's prompt and explicitly
disabled displays stay off. Output removal destroys obsolete lock surfaces,
returning outputs get new ones, and mode/scale/rotation changes reconfigure
existing ones. The compositor also retargets the keyboard when displays
change or the pointer moves between prompts. It keeps the session locked
while outputs are absent. The
[lock-screen display-change incident](docs/incidents/2026-10-27-lock-screen-display-change.md)
records the failure that led to this.

```sh
nix develop .#aizome -c cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shell -p aizome-kit
nix develop .#aizome -c cargo build --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shell
nix develop .#aizome -c env AIZOME_LOCK_TEST_SHELL="$PWD/desktop/aizome/target/debug/aizome-shell" \
  cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp --test lock_displays -- --include-ignored
```

The shell tests cover authentication, every display's field, output loss
during lock acquisition, no-output intervals, surface replacement and view
updates. `lock_displays` uses a test lock client to check that lock surfaces
refit mode/scale/rotation changes and that the keyboard follows the pointer
between displays' lock surfaces. Its desktop cases, ignored unless explicitly
run with the built shell, use the real shell and compositor over private
Wayland sockets to check repeated output removal/restoration, disabled
outputs, lid-close/undock/lid-open, DPMS, mirrors, input isolation and which
prompt has the keyboard. `aizomectl input-state` names the lock surface with
the keyboard (`the lock screen on <output>`). These are component checks,
not physical suspend/resume or ISO qualification.

Aizome's portal backend, `crates/aizome-portal`, answers
`org.freedesktop.impl.portal.FileChooser` in Aizome sessions
(`session/aizome-portals.conf`); D-Bus starts it as
`aizome-portal.service`, and the module adds the package to
`xdg.portal.extraPortals` so its activation file and `aizome.portal` are
found. Each request runs a chooser process built from Hako's library
(model, sidebar rows, path bar, icons), which the frontend's
`Request.Close` ends. It opens over the asking app's window through
xdg-foreign v2: Sukumo's `WindowOptions::foreign_parent`, which the
kit takes as `WindowSpec::foreign_parent`. The behaviour and its limits are in
[Aizome's apps](desktop/aizome/docs/apps.md#the-file-chooser-portal).

```sh
nix develop .#aizome -c cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-portal
nix develop .#aizome -c cargo build --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-portal
nix develop .#aizome -c env AIZOME_PORTAL_TEST_BIN="$PWD/desktop/aizome/target/debug/aizome-portal" \
  cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp --test portal -- --include-ignored
```

The first covers the bus (a private D-Bus with a stand-in chooser), the
chooser in each mode on the kit's stand-in platform, and its goldens. The
last runs the real chooser under a headless compositor over a test window
that exported itself, and checks that it is that window's dialog and takes
the keys. Neither runs a browser or the installed session.

### Sukumo

Sukumo, Aizome's toolkit, is in `desktop/aizome/sukumo`: the Wayland engine
and native Rust toolkit from Sidestep's `aizome` branch, without Sidestep's
Objective-C runtime and frameworks ([its README](desktop/aizome/sukumo/README.md),
[migration record](docs/migration.md#sukumo)). Its crates keep their names,
`sidestep-engine` and `sidestep-ui`, and so do its `SIDESTEP_*` environment
variables. It is a Cargo workspace of its own, as Sidestep's was. Aizome's
workspace excludes it and takes the crates by path, from inside its own
directory so Cargo hashes them relative to the workspace, which the
compositor's ABI fingerprint relies on ([reload](desktop/aizome/docs/reload.md)).
A change to Sukumo is a change to the compositor's core: it can't reload,
and runs at the next login.

Sukumo's lock is the subset of Aizome's its crates use. When Aizome's lock
moves one of their dependencies, prune a copy of it to make Sukumo's again
(the README has the command). Aizome's and Genkan's packages copy
`sukumo/Cargo.toml` and `sukumo/crates` with the kit. Its own tests run with:

```sh
nix develop .#aizome -c cargo test --locked --manifest-path desktop/aizome/sukumo/Cargo.toml
nix build .#checks.x86_64-linux.sukumo -L
```

The check also runs the toolkit's Wayland tests under a headless sway, and
fails if they skip. It replaces the Linux and Wayland jobs of Sidestep's CI
for this code. The frame and image budgets only measure, and run by hand
under `desktop/aizome/sukumo/scripts/headless-wayland`.

Sidestep stays a separate product for objc2 programs. It is not kept in
step with Sukumo. To bring a Sidestep change across, apply it with
`git apply --directory=desktop/aizome/sukumo`; the crates' paths are the
same.

Nisshi (`desktop/aizome/apps/nisshi`), Aizome's log viewer, reads the
journal and /var/log through `aizome-nisshi-helper`, a second binary of the
same crate that uses only the library's `query` and `protocol` modules.
The window runs it as the user. **Unlock** runs another copy through
`/run/wrappers/bin/pkexec --disable-internal-agent` under
`org.aizome.Nisshi.read-system-logs`, whose policy the package writes to
`share/polkit-1/actions` naming `$out/libexec/aizome-nisshi-helper`, the
path the window finds beside itself. Keep that boundary: the window sends
data, never arguments. The helper checks each query and builds journalctl's
arguments itself from options that only read. It reads files only under
/var/log, honours `NISSHI_JOURNALCTL` and `NISSHI_LOG_DIR` only when it
isn't root, and ends when its input closes. The Aizome module turns on
NixOS's pkexec wrapper (`security.polkit.enablePkexecWrapper`, where the
option exists).

```sh
nix develop .#aizome -c cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-nisshi
nix develop .#aizome -c cargo test --release --manifest-path desktop/aizome/Cargo.toml -p aizome-nisshi --test perf -- --nocapture
nix build .#aizome-checks.nisshi -L
```

The first runs the real helper over a stand-in journalctl and pkexec
(`tests/reader.rs`), besides the headless window and its pictures. The
last boots a disposable VM to check the installed policy, the setuid
wrapper and the helper as root through real polkit. Neither types a
password into a desktop's dialog.

Whole-boot search pages through older journal entries and uses the same
matcher as the visible messages, including friendly source names and text
with terminal styling removed. Raw `FIELD=value` terms are sent to the
helper as journal matches. Opening a second instance replaces the current
query and passes its `XDG_ACTIVATION_TOKEN` to the existing window; it does
not request a fresh token from that background window's old input.


## Tatami Bluetooth

`desktop/tatami/bluetooth` is a separate GTK 4/libadwaita Cargo workspace,
packaged as `bluetooth` (`tatami-bluetooth`). The bar, menu and Super+Ctrl+B
launch it through `tatami bluetooth`; its desktop entry also opens the same
single application instance. Its session stylesheet imports `tatami-gtk`
through `config/tatami-bluetooth/style-dark.css`, with the same XDG override
rules as Displays and Network.

The unprivileged window uses GIO's asynchronous D-Bus calls to BlueZ's
[Adapter1](https://bluez.readthedocs.io/en/latest/adapter-api/),
[Device1](https://bluez.readthedocs.io/en/latest/device-api/) and
[Agent1](https://bluez.readthedocs.io/en/latest/agent-api/) APIs. Reads and
ordinary property changes have five-second deadlines; pairing has two minutes
and connecting has thirty seconds. A snapshot is read every three seconds,
with reads serialized and stale action/session results discarded. Rows keep
their widgets across status and battery updates. Each BlueZ session is pinned
to its unique bus owner, which is also checked on incoming agent calls.
The application's KeyboardDisplay agent handles only a pairing explicitly
started here and never replaces the default agent. It rejects unanswered
prompts and closes display-only prompts when pairing finishes. Discovery
belongs to this connection; it never sets global Discoverable or Pairable.
Closing cancels pairing, releases discovery and unregisters the agent. BlueZ's
bus-owner changes clear old state and prepare a new session.

Aizome's Bluetooth pane informed the grouped device lists, device settings
and the pairing dialog lifecycle. No Aizome workspace or dependency is pulled
into Tatami. The root flake uses this local crate with its own narrow source
filter and lockfile.

```sh
nix build .#bluetooth .#tatami --no-update-lock-file
```

The Bluetooth package's checks run the real GTK controls and GIO marshalling
under Xvfb against a simulated BlueZ service on a private `dbus-daemon`.
They cover power and failed changes, multiple adapters, discovery ownership,
connect/disconnect, battery updates, PIN and passkey entry, comparison and
service authorization, display-only prompts, rejection and cancellation,
spoofed agent calls, rename/trust/block, confirmed removal, adapter loss,
service restarts and closing with a pending pairing.
The integration test is ignored by an ordinary `cargo test` because it needs
an isolated display and `dbus-daemon`; the package check explicitly includes it.
`TATAMI_BLUETOOTH_TEST_CAPTURES=/absolute/directory` optionally saves original
GTK captures with simulated devices for review, not hardware evidence.
These are component checks, not ISO builds, VM installations or physical
Bluetooth interoperability validation. Physical adapter/device pairing remains
to be checked; the test fixture does not emulate a Bluetooth controller.

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
power in a reading, and Quit must end that process. The inspector's actual
Pause/Resume button must stop and resume every process in an application
group, with a screenshot after each state change. Its label and action
come from the same whole-selection state as the context menu. Reading real RAPL
counters as root needs physical hardware.

The same VM check exercises process keyboard commands with real table focus:
Shift+F10 and Menu open the process menu, Ctrl+C copies its command, and Delete
and Shift+Delete open cancellable confirmations. These keys run before the
search entry's type-anywhere forwarding only while focus is in the table.
Search-field Delete, cut, copy and its context menu keep their editing behavior;
typing from the table still searches. The test also confirms keyboard Quit
actually ends its disposable process. These component checks do not qualify an
ISO's Activity window.

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

## Network DNS and IPv6

Network applies DNS and IPv6 mode changes together through NetworkManager's
`Update2` D-Bus API, on its existing worker thread. The version guard rejects
concurrent profile edits. Other controls retain their `nmcli` path.
NetworkManager forbids IPv6 DNS addresses with `method=disabled`, so Network
stores suspended addresses in the profile's `user.data` key
`org.sekka.tatami.ipv6-dns` while off. The versioned value binds them to the
current IPv4 DNS list and both families' automatic-DNS flags. Invalid or stale
metadata refuses restoration until the user selects DNS again; it never
overrides an external edit. Other applications' keys and settings are retained.
`GetSettings` excludes secrets, and NetworkManager preserves existing secrets
for a secret-free update. No password is read to change DNS.

`nix build .#checks.x86_64-linux.network-dns-vm` exercises the actual window and
daemon for both IPv6 transitions, automatic/preset/custom DNS, changes while
off, reopening and daemon restart. It also runs the ignored
`network_profile_transactions` Rust test inside that disposable VM: stored WPA
secret and unrelated-setting preservation, a real concurrent version rejection,
and atomic backend validation failure. The fixture package only retains the
normal Cargo test executable; the UI uses the production Network package.
Autologin, software graphics and the private NAT make this component evidence,
not qualification of an ISO or physical network hardware.

## Simulated Wi-Fi

`nix/simulated-wifi.nix` gives a test VM two mac80211_hwsim radios. The first,
in a network namespace of its own, serves the test's networks with hostapd and
dnsmasq. The second, wlan1, is the computer's, managed by NetworkManager. The
Network window's VM check and the screenshots use it.

```sh
nix build .#checks.x86_64-linux.network-vm --no-update-lock-file
```

NetworkManager waits 25 seconds for wpa_supplicant to associate. Joining a
network while connected to another takes two scans: NetworkManager's, as it
leaves the first network, then wpa_supplicant's own. Two settings keep that
well inside the wait:

- `regtest=6` gives wlan1 one of hwsim's own regulatory domains, with 32
  channels. Under the world domain it has 89, most of them passive (S1G at
  902–927 MHz among them), and a full scan takes 9.5 s instead of 2.3 s.
- `networking.networkmanager.wifi.scanRandMacAddress = false`. With
  NetworkManager 1.58 and wpa_supplicant 2.12, a join can otherwise never
  happen. NetworkManager requests its scan with `AllowRoam=false`, which makes
  wpa_supplicant treat that scan's results as scan-only, then takes wlan1 down
  to give it a random scanning address. If that drops the scan before it
  starts, wpa_supplicant keeps the scan-only mark (`wpas_trigger_scan_cb` frees
  only the parameters of work that never started). Its own scan finds the
  network, but the results are ignored, and nothing else scans during
  activation, so NetworkManager fails the join after 25 s with
  `ssid-not-found`. hostap's master branch has the same code. Installed
  systems keep NixOS's default, randomization on; whether they hit this on
  real hardware hasn't been checked.

## Disk encryption

Genkan offers opt-in LUKS2 encryption of root and the optional swap partition,
with the same separate disk passphrase on both. Passphrases are validated in
both processes, carried by the existing private stdin protocol, held in
zeroizing Rust owners and passed to cryptsetup through an anonymous memfd on
stdin. They never enter command arguments, the generated flake, or an installed
key file. The GUI clears disk passphrase fields when encryption is disabled
and when installation starts. GTK's own editable buffers are not guaranteed
to be zeroized.

The helper chooses outer LUKS UUIDs and inner filesystem UUIDs before erasure.
It uses LUKS2 with Argon2id and a 16 MiB payload offset, reserves that overhead
in the layout, verifies container and filesystem identities, and closes its
own mappings after unmounting (including failure cleanup). UEFI keeps the
unencrypted ESP; encrypted BIOS adds a 1 GiB ext4 `/boot` after the 2 MiB BIOS
boot area. Root remains partition 2, swap partition 3 when selected; BIOS
`/boot` is numbered 4 with swap or 3 without it. GRUB reads plain boot files,
so it does not need to unlock LUKS2. Encrypted BIOS installations retain GRUB's
framebuffer (`gfxpayloadBios = "keep"`): returning to VGA text hid the initrd
unlock prompt when the VM's DRM driver took over. Swap stays RAM-sized after header overhead,
and its inner UUID remains the resume device. Encrypted hibernation has been
tried once by hand, on UEFI with Plasma (see the encrypted GUI installation
record below). Early console setup uses the selected keyboard for unlock.

Swap opens after root in the initrd: the configuration adds `After=` on
`systemd-cryptsetup@cryptroot.service` to `systemd-cryptsetup@cryptswap` as a
drop-in (crypttab has no ordering option). systemd-cryptsetup tries the
passphrases already typed before it asks, so swap opens with root's and never
asks. Opened at once, swap was also tried with a mistyped root passphrase, and
then each disk asked again. Opening them in turn adds one Argon2id unlock, about
two seconds, to each boot, and the two unlocks no longer need their memory at
the same time.

Root's filesystem appears only once someone types the passphrase, so the
configuration gives root `x-systemd.device-timeout=0`, systemd's way to wait
without limit for a device that needs user input. systemd-cryptsetup's
generator does this only for `/dev/mapper/cryptroot`, and root is mounted by
its filesystem UUID; without the option, the initrd gave up on that device
after systemd's default 90 seconds and stopped at a locked emergency console
with the prompt still showing.

The configuration sets `boot.initrd.luks.devices.<name>.allowDiscards = true`.
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

Discards reveal which blocks are unused to anyone holding the raw disk; the
README and disk page explain this tradeoff. The installer opens mappings with
`--allow-discards` for formatting and testing, without a persistent header flag.

```sh
nix build .#genkan .#checks.x86_64-linux.storage .#checks.x86_64-linux.encrypted-storage --no-update-lock-file
```

The encrypted-storage component check runs the production partition/format/mount
phase on a marked disposable virtual disk for ext4, Btrfs and XFS, with and
without swap, and UEFI and BIOS layouts. It tests wrong passphrases, closing
and reopening containers, data persistence, and cleanup. It then boots an
encrypted root with Genkan's generated storage configuration and checks active
encrypted swap, non-zero discard granularity on both the virtual disk and
mapping, and `fstrim -v /`. The harness supplies the bootloader and Nix store:
this is not a full ISO installation or disk-only reboot, and does not validate
physical hardware or hibernation. The separate fixture accepts
`CALAMARES_TEST_ENCRYPT=1` for full image installation runs, and
`respin-tools qemu-test ISO --install` passes it through. On the installed boot
the tool waits for the splash's passphrase field (its amber edge, in a
screendump), keeps that frame as `unlock.png`, and types the fixture's public
test passphrase before it waits for the guest agent; `boot-installed` does the
same for an encrypted run. Genkan's test diagnostics add
`plymouth.ignore-serial-consoles` to the serial console they set up, so the
installed system shows the splash rather than Plymouth's text.

The [2026-10-21 full GUI installation record](docs/test-results/luks-gui-uefi.json)
covers an 8 GiB UEFI VM installed through Genkan with ext4, encrypted root and
swap, Aizome and Plasma. It includes two disk-only boots without the ISO or a
shared host store, wrong-passphrase rejection, graphical login to both desktops,
application use, file persistence and working TRIM. Installation used the ISO's
Xfce/X11 compatibility entry: Genkan's page rendering got stuck in the default
Plasma live session. That was GTK's rendering in Plasma; Genkan's window is now
Aizome's kit in an Aizome live session (see "Genkan"), which this record
predates. This run also found and fixed Aizome's omission from
`respin.referenceDesktops`; keeping its closure on the ISO avoids compiling the
default desktop in the live tmpfs.
It does not establish a full BIOS install, physical hardware support, encrypted
hibernation or an installed-system update. The record separates these limits
from the component checks above.

The [2026-10-24 encrypted installation record](docs/test-results/encrypted-splash-install.json)
covers installations from the ISO on UEFI and BIOS through `respin-tools
qemu-test --install` (the fixture driving Genkan's backend, not its window):
ext4, encrypted root and swap, then a disk-only boot that asked once for the
passphrase on the boot splash and passed the fixture's installed-system checks.

The [2026-10-24 encrypted GUI installation record](docs/test-results/encrypted-splash-gui.json)
installs through Genkan's window, in the default Plasma live session, which
rendered it on both firmwares: ext4 with Aizome and Plasma on UEFI, Btrfs with
Aizome alone on BIOS, encrypted root and swap on both. The installed systems
were then used through their own interfaces: the splash's prompt with a wrong
passphrase, Caps Lock and Esc, both greeters' sessions, applications, restart
and shutdown from Aizome's Toolchest and Plasma's launcher, and on UEFI a
hibernation to the encrypted swap and a resume through the splash. The UEFI
run found the 90-second device timeout described under "Disk encryption";
the BIOS run, from an ISO with the fix, waited 130 seconds at the prompt and
booted.

## System tuning and responsiveness

`calamares.tuning.enable` (on by default) applies
`installer/genkan/rust/system/tuning.nix`. That covers CachyOS-inspired kernel,
memory, I/O and ananicy settings, plus Ubuntu's lowlatency boot options and
mq-deadline for NVMe. Sekka also has its own ananicy rules: shells run at
nice 0, compilers at nice 3, and interactive AI harnesses at nice −4 with
normal CPU scheduling. A user service gives their child jobs separate systemd
scopes sharing a low-weight slice; the harness itself stays interactive.
`nix/scheduling/` also patches ananicy's thread/fork/exec handling, and
Aizome's keybinding/IPC launcher starts programs in `app.slice` rather than
leaving terminal builds in the compositor's scheduling group.
[docs/tuning.md](docs/tuning.md)
describes the policy, matching limits, patch maintenance and VM checks.

```sh
nix build .#checks.x86_64-linux.tuning .#checks.x86_64-linux.aizome-login --no-update-lock-file
nix build .#checks.x86_64-linux.tuning-policy --no-update-lock-file
python3 -m unittest discover -s tests -p test_agent_workloads.py
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

## Boot splash

Genkan writes `calamares.bootSplash.enable = true;` into every installation; the
installer has no choice to leave it out. The option turns on Plymouth with
Sekka's theme, from `installer/genkan/rust/system/boot-splash/`. Systems
installed before it keep booting without a splash until the option is set, and
an update doesn't set it. The module declares it as one of Yukimi's choices, so
Aizome's Settei (General → Startup, which proposes it in Yukimi), Aizome's
Yukimi Desktops page and `sekka-update --choose calamares.bootSplash.enable=true`
turn it on or off; turning it changes the initrd and kernel parameters, so it
stages for the next start. The splash shows the stacked lockup on the Night
ground, with a 2px rail under it in the crest's blue that fills as the computer
starts. When shutting down or
restarting, the label "Shutting down" or "Restarting" takes the rail's place. A
passphrase request gets the Aizome lock screen's field. Esc shows the boot
messages at any time, and after 15 seconds of starting, not counting time at a
passphrase prompt, the splash says so. The live media show the same splash
(`modules/installer.nix`), with `UseSimpledrm=2` as on encrypted systems: their
initrd has no GPU driver and may be copying the image to memory, so the splash
draws on the firmware's framebuffer at once and moves to the GPU when its
driver loads. They also name a serial console for boot logs, so they pass
`plymouth.ignore-serial-consoles`, without which Plymouth shows only text.

On a disk Genkan encrypted, the splash asks for the passphrase in the initrd.
Swap opens after root, with the passphrase root opened with, so one passphrase
opens both and a mistyped one is asked for again only once (see "Disk
encryption"). The initrd has no GPU driver on NixOS, and Plymouth waits
8 seconds (`DeviceTimeout`) for one before it draws on the firmware's
framebuffer, leaving the screen blank until then. It also passes over a
framebuffer at 640×480, 800×600 or 1024×768, expecting a native driver to
replace it, and GRUB leaves 1024×768 on BIOS. So the module sets
`UseSimpledrm=2` when `boot.initrd.luks.devices` is set: the prompt appears at
once at whatever mode the firmware set, and the splash moves to the GPU when
its driver loads after unlocking. Unencrypted systems keep Plymouth's default.
systemd's password agent doesn't pause Plymouth's clock or its progress
estimate the way `plymouth ask-for-password` does, so the theme stops the rail
and the hint's count while a prompt is up, then spreads what is left of the
estimate over what is left of the rail. Esc swaps the theme for Plymouth's text
view and back, which loads it again: the rail and the hint's count start over
from Plymouth's figures.

The module builds NixOS's Plymouth with three patches from `boot-splash/plymouth/`,
for what went wrong in the encrypted-boot-splash check when the GPU's driver took
over from the firmware's framebuffer or a passphrase was mistyped. None is in a
Plymouth release yet. [docs/upstream/plymouth](docs/upstream/plymouth/README.md)
has the first two as ported to Plymouth's `main` and sent upstream (plymouth!519
and !520), and says where each of the three stands and when Sekka can drop it.

- `keyboard-removal.patch`: Plymouth freed a display's keyboard without telling
  plymouthd or the splash, and the script plugin's `Plymouth.GetCapslockState()`
  went on reading it. plymouthd crashed in `ply_keyboard_get_capslock_state` in
  3 of 11 test boots with a wrong passphrase and Esc.
- `console-owner.patch`: Plymouth reads keys from the console terminal (NixOS's
  `vconsole.conf` has no `XKBLAYOUT` for evdev input), and only one renderer
  owns it. It was handed on only once no keyboards were left, so when the GPU's
  framebuffer device got a renderer before its DRM device, no renderer read the
  keyboard and nothing typed reached Plymouth, Esc included: 4 of 8 VM boots.
- `abandoned-prompts.patch`: a question stayed on the splash after whoever asked
  had hung up (`ply_boot_connection_on_hangup` only marked the connection). After
  a mistyped passphrase, systemd's agent gave up the request for a disk that got
  its passphrase another way, and the splash went on asking for an open disk.
  Plymouth's `main` already fixes this its own way (f3461296, e7fb0732), in no
  release yet.

`generate.py` outlines the theme's text from IBM Plex Mono, and `package.nix`
renders every piece at 1x, 2x and 3x. Plymouth's own scaling is off
(`DeviceScale=1`). The script picks 2x from 1600 pixels on the display's
shorter side and 3x from 3240, so nothing is stretched. Text it can't know
ahead, such as a passphrase prompt or a message, is set at run time in IBM
Plex Mono, the one font the initrd carries. Plymouth keeps the initrd's font
configuration after the switch to the installed system, so the module puts
the font at the same path there. Without it, that text comes out as empty
boxes.

`nix build .#checks.x86_64-linux.boot-splash-choice --no-update-lock-file`
evaluates complete installed Aizome configurations with the option absent,
off and on, using the local Aizome and Yukimi packages. It checks the JSON
choice Settei and Yukimi read, Plymouth's theme and boot parameters, and
standalone use without Yukimi. The packaged-helper tests in
`tests/test_sekka_update.py` also exercise enabling and disabling the splash,
restart planning when the initrd changes, invalid values, preserving custom
Nix expressions and restoring the configuration after a failed build. Their
Nix and activation commands are stubbed; these checks do not certify an
interactive Settei → Yukimi → Apply → restart or an ISO.

```sh
nix build .#checks.x86_64-linux.boot-splash --no-update-lock-file
```

The VM check boots at 1080p and at 4K (2x) and holds the splash until it has
looked at it. It covers the rail, a message, and a passphrase typed into the
field and handed back to whoever asked. It also covers Caps Lock, the Esc hint
and the messages Esc shows, the handoff to the login prompt, the restart and
shutdown labels, and a real power-off. Its screenshots are in the result.

```sh
nix build .#checks.x86_64-linux.encrypted-boot-splash --no-update-lock-file
```

The encrypted-boot-splash check runs Genkan's storage code on a disposable
disk, for UEFI and for BIOS, as the encrypted-storage check does. It then boots
the storage configuration Genkan generates, with the splash on and no GPU driver
in the initrd. It covers the prompt in the initrd, one passphrase opening root
and swap, the hint not counting time at the prompt, and the keyboard after the
GPU takes over: Caps Lock, an answer typed into a new prompt, and Esc. It also
covers a question going when whoever asked it does, the handoff to the login
prompt, and a restart through the splash. On the restart it covers a wrong
passphrase, asked for again for root alone, Caps Lock, Esc at the prompt, swap
opening without asking, no question left up, and an encrypted root left clean.
The harness supplies the bootloader and Nix store, so this isn't a full
installation.

## Programs built for other distributions

Installed systems enable nix-ld (`programs.nix-ld.enable`, with
`lib.mkDefault`, in `installer/genkan/rust/system/prebuilt-binaries.nix`), so
programs built for other Linux distributions start. Without it, NixOS puts a
stub at `/lib64/ld-linux-x86-64.so.2` that prints an error and exits 127.
Editors and version managers download such programs routinely. Zed fetches
rust-analyzer, CodeLLDB, package-version-server and its AI agents, and Node.js
when there is no suitable one on PATH. VS Code's extensions bring their own
programs, and mise installs whole runtimes. This is a property of the platform,
so Sekka handles it once rather than in each editor. A per-editor fix, such as
putting Nixpkgs' rust-analyzer on PATH for Zed, covers one server in one
editor. Nixpkgs' FHS variants (`zed-editor.fhs`, `vscode.fhs`) run the editor
in a bubblewrap sandbox. nix-ld's README notes that setuid programs and other
sandboxes, `nix build` among them, don't work inside one.

nix-ld (Nixpkgs' 2.0.6) starts the program with the Nix store's glibc and the
libraries linked in `/run/current-system/sw/share/nix-ld/lib`. NixOS sets
`NIX_LD` and `NIX_LD_LIBRARY_PATH` for sessions. When they are unset, nix-ld
falls back to the same paths. They are unset for Zed's downloads: Zed starts
those with its own environment (`env: None` in `crates/languages/src/rust.rs`),
and Zed started from a launcher has no login shell's environment. nix-ld passes
the libraries to the loader as `LD_LIBRARY_PATH`. On x86_64 it then restores
the environment the program was given before the program's entry point runs,
so the programs it starts, such as rust-analyzer's cargo and rustc, don't
inherit them.

The libraries are Nixpkgs' default set for the module: glibc, libstdc++ and
libgcc_s, zlib, zstd, xz, bzip2, OpenSSL, curl, libssh, libsodium, libxml2,
acl, attr, and util-linux's and systemd's libraries. Sekka adds none. `readelf -d`
on rust-analyzer 2026-10-19, CodeLLDB 1.12.3, codex-acp 0.16.0,
package-version-server 0.0.10 and Node.js 24.22.1 shows that they need nothing
beyond glibc, libgcc_s, libstdc++, zlib, OpenSSL 3 and the libraries CodeLLDB
ships beside itself. The set is kept small on purpose. The loader searches
`LD_LIBRARY_PATH` before a binary's `RUNPATH`, so a library in the set replaces
one with the same name that a program ships beside itself. A system that needs
more adds them to `programs.nix-ld.libraries`. `programs.nix-ld.enable = false`
brings back the stub. Main's Plasma reference system already contains every
library in the set. The change adds only nix-ld itself (75 KB, with no
references) and a directory of links.

Not covered:

- 32-bit programs (`/lib/ld-linux.so.2`).
- Scripts whose first line names `/bin/bash` or another path NixOS doesn't
  have. `/bin/sh` and `/usr/bin/env` exist, and envfs would add the rest.
- Native Python or Node.js modules loaded by Nixpkgs' interpreters, which
  don't go through nix-ld.
- Graphical programs that need libraries outside the set.

The live session keeps the stub.

```sh
nix build .#checks.x86_64-linux.prebuilt-binaries --no-update-lock-file
```

The VM check puts the GitHub and nodejs.org builds of rust-analyzer, Node.js,
package-version-server and CodeLLDB where Zed puts them, in
`~/.local/share/zed`. It starts them as an account with an empty environment,
as Zed does, and from a login shell. It checks that:

- every library each one asks for is found, the ones above in nix-ld's set;
- Zed's `--version` check of rust-analyzer passes;
- rust-analyzer loads a Cargo project and the standard library, running
  Nixpkgs' cargo and rustc;
- the processes Node.js starts don't see nix-ld's libraries.

With nix-ld disabled, the check fails at the first binary with the stub's
message. It does not start Zed itself, and codex-acp was checked only with
`readelf`.

## Joining Wi-Fi networks

Installed systems and the live session join Wi-Fi through NetworkManager and
wpa_supplicant 2.12. Sekka patches wpa_supplicant
(`installer/genkan/rust/system/wpa-supplicant`). Without the patch, a switch
from one network to another can fail. As it leaves the first network,
NetworkManager gives the interface a random address for scanning, taking it
down and up, and asks wpa_supplicant for a scan with `AllowRoam=false`. It
then restores the address for the new network, taking it down again. If that
removes the scan before it starts, wpa_supplicant keeps treating the next
scan's results as scan-only, including those of its own scan for the new
network. It finds the network but joins nothing, and NetworkManager gives up
after 25 s.

In VMs with simulated Wi-Fi and NetworkManager's default address
randomization, 15 of 720 switches failed this way, among hidden and visible
networks alike. None of 288 failed with the patch, and none of 72 with
randomization off. The failure hasn't been reproduced on physical hardware.
The patch clears the scan-only handler when the interface goes down, as
wpa_supplicant already does for scan results that arrive while it is down. It
isn't in hostap yet. If hostap takes it, the patch will stop applying to
Nixpkgs' wpa_supplicant, and the module can go.

```sh
nix build .#checks.x86_64-linux.wpa-supplicant-scan --no-update-lock-file
```

The check pauses wpa_supplicant in a disposable VM while a D-Bus scan request
with `AllowRoam=false` and wlan1 going down both wait for it, so the scan is
removed before it starts. It then requires `SelectNetwork` to join. It also
checks that each of the live session's boot entries and an installed system
start the patched wpa_supplicant over D-Bus.

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

## Agent skills

`agent-skills/skills` holds what installed systems tell coding agents, in
the [Agent Skills](https://agentskills.io/specification) format: a folder per
skill with a `SKILL.md`.

- `sekka`: root through the desktop's polkit dialog rather than sudo, the
  layout of `/etc/nixos`, and applying changes. Its script is
  `scripts/apply-configuration`.
- `yukimi`: software, generations and storage through Yukimi's helper. Its
  script is `scripts/software`.
- `aizome`: settings, bindings, `aizomectl`, logs and recovery.
- `tatami`: the Lua Hyprland configuration and its overrides, the `tatami`
  command, logs and recovery.

When a command, path, option or behaviour a skill describes changes, change
the skill in the same pull request.

`agent-skills/module.nix` is imported by the installed-system modules
(`installer/genkan/rust/system`), so the reference systems carry it too. Its
`calamares.agentSkills.enable` defaults to on. It runs
`sekka-agent-skills sync` from NixOS's user activation: at each login, and
for users already logged in, at every switch. A failed sync is reported and
never fails the activation. With the option disabled, user activation runs
`sekka-agent-skills disable` instead: managed copies and links are removed,
edited copies move to backups, and unrelated skills stay. The command remains
installed. Reenabling installs copies removed by disabling, while preserving
removals the person made beforehand.

Copies go in `~/.agents/skills/<name>`. That folder is read by Codex, by
the Codex inside the ChatGPT app, and by OpenCode, Gemini CLI, pi and
oh-my-pi, Copilot CLI, Cursor, Amp and Goose. Claude Code reads only
`~/.claude/skills` (`$CLAUDE_CONFIG_DIR/skills`), so it gets a symlink to
each copy's folder, which Claude Code follows.

An agent that reads both folders sees each skill once:

- Codex and pi dedupe by the real path.
- OpenCode keys skills by name.

These locations come from each agent's documentation and source as of
October 2026. They are not tested here.

The skills are copies, not links into the store, for two reasons:

- Codex skips a `SKILL.md` that is itself a symlink.
- People should be able to edit them.

The command keeps its records in `~/.local/state/sekka/agent-skills`:

- `installed/<name>` holds a hash of the version Sekka installed: paths,
  contents, symlink targets and executable bits.
- `linked/<name>` marks a Claude link Sekka made.

A sync acts on each skill as follows:

- **Never installed:** it installs the skill.
- **Copy still matches the record:** it replaces the copy with the new
  version.
- **Copy identical to Sekka's version:** it adopts the copy.
- **Changed by someone:** it leaves the copy.
- **Copy or link removed:** it leaves it removed.
- **Someone else's skill with the same name:** it leaves it, and doesn't
  link it.
- **Dropped from Sekka:** it removes the copy only if unchanged.

`restore` moves a changed copy to `backups/<time>/` before installing
Sekka's version again.

The two scripts make one root request each, through `pkexec` with
`--disable-internal-agent` and input from `/dev/null`. The input matters
because the helper takes a closing input pipe as Stop. Both reduce the
helper's `internal-json` progress to its stages and messages.

`apply-configuration` copies the original and edited configuration into one
immutable Nix store snapshot and holds a GC root while building its edited
side as the user. It calls `yukimi-helper change --configuration SNAPSHOT
--mode auto` directly through polkit. The helper copies the bounded snapshot
into its private operation directory before starting its independent worker,
so the agent can exit without removing the worker's inputs.

Under the existing operation lock, the worker checks that the original
snapshot still matches the complete configuration. It rejects stale plans,
symlinks, deletions and changes to files other than ordinary `.nix` modules
(the flake, hardware configuration and `yukimi.nix` are protected). It saves
the old generation before installing edits, includes every changed module in
its failure backup, then builds and activates through the normal transaction.
Cancellation before activation restores all edited files; after commit the
worker finishes and reports its result. History also restores imported
modules, preserving files changed by hand since their generation.

`software` reads the current selections and checks requested names without
root, then sends `--add-packages` / `--remove-packages` or
`--add-applications` / `--remove-applications`. The helper changes the current
list under its lock. Replacement-list options remain compatible with
existing clients, but these scripts no longer submit stale whole lists.

```sh
nix build .#checks.x86_64-linux.agent-skills .#checks.x86_64-linux.agent-skills-vm --no-update-lock-file
```

`agent-skills` runs shellcheck and checks each skill's frontmatter against
the format. It runs the packaged command in temporary homes and covers:

- installing, updating, keeping changes and removals
- restoring with a backup, disabling and reenabling
- `CLAUDE_CONFIG_DIR`
- dropped skills
- refusing to act without Sekka's skills

It also runs the scripts against a fake flake, pkexec and helper, with real
Nix evaluation of their expressions. `tests/test_sekka_update.py` runs the
packaged helper in disposable rootless namespaces and covers lock contention,
stale prepared configurations, cancellation, pre-activation failure, committed
failure, generation snapshots, imported-module rollback and software deltas:

```sh
nix build .#yukimi.helper --no-link --print-out-paths --no-update-lock-file
nix shell --inputs-from . nixpkgs#python3 nixpkgs#bubblewrap -c python3 tests/test_sekka_update.py /nix/store/…-yukimi-helper-0.1.0/bin
```

The prepared-configuration fixtures add their small snapshots to the Nix
store; all helper writes, builds, profiles and systemd actions are confined
to disposable namespaces, with builds and activation stubbed.

`agent-skills-vm` logs into Aizome in a disposable VM and checks that:

- the copies and links are made at login
- a switch syncs again, keeping a change and a removal
- `restore` brings back Sekka's versions
- an agent-like process, a user service with no terminal and a pipe for its
  input, outside the login session, installs a package with `software`
  through Aizome's real password dialog, with a fixture helper
- the same kind of process applies an edited `configuration.nix` with
  `apply-configuration`: it builds as the user, then the file is installed
  as root after one password
- a plain `pkexec` command, as the `sekka` skill gives it, runs as root with
  the system's programs (NixOS's PAM environment supplies `PATH`), in
  `/root`, without the caller's variables
- Escape in the dialog gives pkexec's 126

It doesn't cover Tatami's hyprpolkitagent or the GNOME, Plasma and Xfce
agents. It doesn't build a real system through the helper, and it doesn't
test the agents' own discovery.

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
**Run workflow** button (Actions, "Check and cache nightly Sekka"), on `main` only:

```sh
gh workflow run nightly.yml --ref main
```

The job runs on the builder. It first brings every input in `flake.lock` up
to date (`nix flake update`) and commits that, if anything moved, on top of
`main`. Every run checks the candidate with `nix flake check`; it doesn't build
`.#iso`, which no check uses and only qualification needs. Every check runs
even after one fails, and the failed ones run once more, so a VM test losing a
timing race once doesn't hold back upstream security fixes; a check that fails
twice stops the update. The run's warning names the checks that needed the
retry: they are flaky, and need fixing like any failure. Then, in this order
and only if each step succeeded:

1. `nix run .#push-cache` publishes Sekka's packages for that revision to
   cache.sekka.computer.
2. `scripts/check_cache_coverage.py .#installed-systems` follows, from each
   desktop's installed system, the one with every desktop and the free
   catalog applications, everything
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
CI stops after caching and moving the checked revision. It never invokes
`publish-nightly` and receives no credentials for `sekka-downloads`.
Nightly ISO publication is a separate workstation operation after testing the
exact image through installation, disk-only boots and desktop interaction.
A successful build or component test is not publication approval.

The workstation uploader requires an independently audited, machine-readable
[qualification report](.agents/skills/sekka-iso-qualification/references/release/report.md)
for the exact ISO. From the clean candidate checkout, whose commit is reachable
from the fetched `origin/main`:

```sh
nix run .#publish-nightly -- validate ISO_DIRECTORY QUALIFICATION_DIRECTORY
nix run .#publish-nightly -- publish ISO_DIRECTORY QUALIFICATION_DIRECTORY
```

Validation binds the ISO to that revision's evaluated immutable Nix output,
SHA-256/size, lock and workstation, checks all Q0–Q7 results and dynamic desktop
inventories, and rehashes referenced evidence. Keep the tested store output alive
with its build symlink; rebuilding after testing starts a new qualification.
Structural validation cannot decide whether the screenshots prove a result or
the feature inventory is complete; independent evidence review is still required.
CI is rejected. `validate` and `audit-digest` never access R2.

Each published image has an immutable `nightly/builds/DATE-REVISION/` folder
with ISO, `SHA256SUMS`, `flake.lock`, `manifest.json`, `publication.json` and
`qualification/` report/evidence. Existing prefixes are rejected. A conditional
claim prevents concurrent writers from reusing a path; failed/uncertain uploads
remain reserved and unindexed for review, not automatic retry or cleanup.
Object sizes/checksum metadata are checked before a conditional update of
`nightly/builds.json` exposes the build. Index conflicts fail without pruning.
Retention keeps seven builds and deletes only exact objects indexed by dropped
builds, preserving unrelated/unindexed objects. The manifest links its report
and lists its exact objects; existing Kakehashi/site fields remain unchanged.
Q8 still requires independently downloading/hashing the public bytes and checking
the actual download page and Kakehashi. Unpublished builds keep populating cache.

`nix build .#checks.x86_64-linux.publish-nightly` exercises the fail-closed gate,
source binding, transfer failures, concurrent-write conflicts and scoped retention
with synthetic data and no network or credentials. R2's conditional writes are
documented in its [S3 extensions](https://developers.cloudflare.com/r2/api/s3/extensions/).

`downloads.sekka.computer` is the bucket's custom domain, served through
Cloudflare. Cloudflare's cache keeps no file over 512 MB on this plan, so ISOs
always come from R2, whose egress is free. A build's files are marked
immutable, and `builds.json` is marked `no-cache`.

sekka.computer/download/ lists the builds through the download worker (see
Website): it keeps `builds.json` in each Cloudflare data centre's cache for
five minutes, so a new build appears within five minutes of its upload without
a new deployment, and R2 sees at most one request for the list per data centre
in that time.

The workstation upload uses an R2 API token with Object Read & Write on
`sekka-downloads` alone, supplied locally as `AWS_ACCESS_KEY_ID` and
`AWS_SECRET_ACCESS_KEY`. Keep it outside the repository and CI. Remove the
obsolete `R2_ACCESS_KEY_ID` and `R2_SECRET_ACCESS_KEY` secrets from the GitHub
`nightly` environment; keep all three `CACHE_*` secrets for nightly builds.

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

### Caching main

`.github/workflows/cache.yml` runs on the builder for every push to `main`
(each merged pull request) and from its **Run workflow** button (Actions,
"Check and cache main"). A push runs it once, for its last commit, however
many commits it brings: a rebase merge of several builds only the one it ends
on. One run builds at a time; while it does, GitHub keeps only the newest
waiting run and cancels older ones, so a burst of merges builds the first and
the last.

It builds `.#uta` first, so a kit or Sukumo change that breaks Uta fails by
name (see [Aizome](#aizome)), then runs `nix flake check` on the revision as
merged, without updating inputs; its `uta-rustypipe` check confirms, in the
build sandbox, that RustyPipe is enabled. Nothing of Uta runs in the job
itself, which goes on to hold the cache's secrets.
`nix run .#push-cache` signs and uploads the packages, including Uta and its
source, to the `sekka-cache` R2 bucket. `scripts/check_cache_coverage.py .#uta .#installed-systems`
then verifies that a fresh store can download Uta and the installed systems
without compiling them. The nightly uses the same build, upload and coverage
checks. Uta remains excluded from the ISO preload list. A failed build or check
stops the job. It builds no ISO, moves no
branch and publishes no download; machines following `main` substitute its
packages instead of building them, and the next nightly finds them already
built. It uses the `nightly` environment's three cache secrets, and fails
before building if any is missing. The nightly's own push to `main` starts no
run: the workflow token's pushes start no workflows, and the nightly publishes
that revision itself.

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
  `workflow_dispatch`, `schedule` and `push` on `refs/heads/main` of
  `bitemyapp/sekka`: workflows only maintainers can start or change, and
  pushes to `main`, which only they can make. For anything else, such as a
  pull request's workflow, it kills every process of the job but the runner,
  so even `if: always()` steps never run, and the job fails.
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

The runner's Nix client comes from Sekka's pinned Determinate input, matching
the host daemon's implementation. An upstream client cannot decode every build
status a Determinate daemon sends, and loses the original error when sibling
builds are cancelled
([incident](docs/incidents/2026-10-20-nightly-build-status-decode.md)). The
builder step prints the client version and only the daemon endpoint, version
and trust status; it does not print environment variables or configuration.
`checks.x86_64-linux.runner-protocol`
tests a cancelled sibling build through a real daemon as an untrusted user, with the upstream
client as a negative control. It does not certify an existing runner deployment.

After reviewing a runner change, rebuild and deploy its images using the steps
below while idle. For a fresh nightly attempt, dispatch `nightly.yml` on current
main after its fixes are merged; rerunning an old job uses its old workflow,
which may still publish ISOs. The current nightly intentionally updates inputs,
builds and checks, uploads the binary cache, then advances main/nightly; it does
not publish an ISO or qualify the workstation's already frozen image.

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
using isolated state, without changing the host desktop. Image selection decodes
the complete file with GdkPixbuf before any saved-state or renderer change; the
packaged loader set matches swaybg, including SVG, WebP and JPEG XL. The CLI check
uses real image data and proves malformed, truncated, unreadable and missing
files preserve the previous settings, symlink and renderer invocation count.
`nix build .#checks.x86_64-linux.background-validation-vm` exercises the normal
Walker menus in a disposable Tatami VM: invalid global and display-specific
images show an error without changing the rendered wallpaper or service PID,
then a valid image replaces it. This component check does not qualify an ISO.

Keep original full-resolution PNGs and record how they were captured in `docs/screenshots.md`. Use real app captures, not generated mockups. Mark previews or historical images accurately. Preserve the original component licenses, SPDX declarations, artwork credits, and `docs/imports.json` provenance.

`nix build .#screenshots --out-link result-screenshots` captures Genkan's debug
preview and Yukimi in a disposable NixOS VM. This is a development output and
is not included in the ISO or the normal component checks.

`nix build .#yukimi-discover --out-link result-yukimi-discover` runs Yukimi's
Discover the same way, on a Tatami VM with Genkan's application catalog and
unfree software allowed. Yukimi makes its package list from the VM's Nixpkgs,
searches for `steam`, opens the package's details and asks to install Steam
for everyone. It cancels both the confirmation and the real Polkit prompt,
checking that neither changes the configuration. The same check covers the
application launcher, single-instance page requests, clipboard commands,
fractional scaling and a smaller display. Tesseract reads the full captures
and locates clickable labels. Captures are component-test evidence; record
that provenance if using one in the gallery.
