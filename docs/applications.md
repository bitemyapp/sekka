# Optional applications

The installer presents 35 choices with checkboxes, categories, search and short
descriptions. Firefox starts selected. Empty selection is supported. The review
screen lists the normalized selection, and the privileged helper validates it
again. Unknown IDs, duplicate requests and proprietary selections without
unfree-software consent are rejected before disk writes.

The catalog lives in the pinned installer's `applications.json`, shared by its
Rust UI/parser and generated NixOS module. Each installed system retains that
catalog and `applications.nix` beside its flake. `/etc/installer-applications.json`
records selected IDs and exact package versions and store paths.

Every application is on the installation media except those marked
`"media": false`, whose licences forbid redistributing them. Neither the media
nor Sekka's binary cache carries those; preparing an installation downloads
them from their makers (installing always needs a network connection), and the
installer marks them Downloaded. The `applications` check fails if another
choice names one of their packages. It compares the packages choices name, not
those packages' dependencies.

## Sources

All sources are pinned in `flake.lock`. Adding the application inputs preserves
the existing base Nixpkgs, Determinate and FlakeHub CLI revisions. The application
pins were selected on 2026-10-04:

| Source | Pin | Use |
| --- | --- | --- |
| [NixOS/nixpkgs](https://github.com/NixOS/nixpkgs) | `a7868a727837f3c09cee2ce0ca671c76b1589fed` | Browsers, editors, terminals, developer tools, productivity, communication and media |
| [Numtide llm-agents.nix](https://github.com/numtide/llm-agents.nix) | `372f0337e8170e55ff0c017cd43ab73b02a062ad` | Codex, ChatGPT, Claude Code, Claude Desktop and OpenCode |
| [oh-my-pi upstream](https://github.com/can1357/oh-my-pi) | v18.6.1, `2a2c6dcbbb558c0f8145f67f28b3370984f2bf60` | oh-my-pi |
| [bitemyapp/superluminal-nix-linux](https://github.com/bitemyapp/superluminal-nix-linux) | `eacbc06314c5c590e2cc74d95d082911759b59f0` (1.0.7705.679-alpha), added 2026-10-16 | Superluminal |
| [bitemyapp/tracy-flake](https://github.com/bitemyapp/tracy-flake), forked from [Quince-Pie/tracy-flake](https://github.com/Quince-Pie/tracy-flake) | `1f0bf6eaee8e965848274caa7f4ab6d2ff877f97` (0.14.1), added 2026-10-16 | Tracy Profiler |

The AI definitions use upstream release artifacts or source and adapt them for
NixOS. The ChatGPT and Claude entries package their official Linux applications;
they are not unofficial web wrappers. Their vendor-supported Linux distributions
and this community NixOS packaging have different support scopes. Account login
is performed by the user after installation. The upstream oh-my-pi release was
newer than the Numtide package at the time of pinning.

OrbStack is macOS-only. At the user's request its Linux replacement is Docker
Engine + Compose. Thunderbird, KeePassXC and Gemini CLI are absent.

## Development tools

Development build tools includes GCC's C/C++ compilers and libc headers, Make,
binutils, CMake, Ninja, pkg-config, Git and patch. This covers the usual
`build-essential` role with common additional build drivers; projects can still
need their own libraries or a Nix development shell.

Every choice that brings Git (build tools, Lazygit and the GitHub CLI) also
brings Git LFS, through `programs.git.lfs`. Its filter is in `/etc/gitconfig`
for every account, so LFS files clone as their contents rather than pointer
files without anyone running `git lfs install`. Git LFS adds each repository's
hooks itself when its filter first runs there.

Selecting Rustup also selects and locks the build-tools checkbox. Deselecting
Rustup makes that checkbox optional again. Nixpkgs provides the NixOS-compatible
Rustup wrapper. Run `rustup default stable` after installation to download a
toolchain. Rustup is intentionally not accompanied by a conflicting system-wide
`rustc`/`cargo` package.

Superluminal and Tracy are profilers packaged by other flakes, which Sekka
takes as inputs and builds with the applications' Nixpkgs; the catalog lists
them as Sekka's own packages (`"source": "sekka"`). Superluminal repackages
the vendor's proprietary Linux alpha, so it needs unfree-software consent and
a trial or license. Its end-user licence (section 1.9) forbids redistributing
it, so it is marked `"media": false`: the installer, Yukimi and every update
download it from Superluminal's own servers and build its small wrapper on the
machine. The wrapper depends on the applications' Nixpkgs, so most updates
rebuild it, and installing or updating a system with Superluminal fails while
Superluminal's servers are unreachable or no longer serve the locked release.
A daily workflow in the
[superluminal-nix-linux fork](https://github.com/bitemyapp/superluminal-nix-linux)
adds each new release from superluminal.eu once it builds, and Sekka's nightly
input update picks it up. Superluminal ships no Qt Wayland plugin and runs
through XWayland. Capturing starts its capture service through `pkexec`, so it
needs the setuid wrapper (Yukimi's module enables it) and a graphical polkit
agent: Tatami starts hyprpolkitagent and the full desktop environments bring
their own, but the plain Hyprland session starts none. Sekka's tuning sets
`kernel.kptr_restrict` to 2, which Superluminal names as the reason captures
lack kernel symbols; a system that needs them can set it to 1 with
`lib.mkForce`. Tracy comes from a
[fork of Quince-Pie's tracy-flake](https://github.com/bitemyapp/tracy-flake)
that nothing updates on its own, so upstream changes reach Sekka's nightly only
once they are reviewed and pushed there. Tracy's profiler runs natively on
Wayland. Its network protocol changes between releases, so a program must be
built with the same Tracy client version as the installed profiler; Nixpkgs
keeps older versions as `tracy_0_13` and the like. Tracy is free and compiled
from source, so `push-cache` publishes it.

Docker runs as the installed user's rootless service, with a persistent user
manager and its socket selected in the user environment. The user is not added
to the privileged `docker` group. Compose is installed with the engine.

## Sign-ins and the keyring

Chromium and Electron seal cookies, saved passwords and Electron's
`safeStorage` with a key kept in the keyring of a desktop they recognise:
GNOME Keyring in GNOME, Xfce or Cinnamon, KWallet in Plasma. In a desktop they
don't recognise (Tatami, Hyprland, Aizome) they fall back to a fixed key, which
Electron's `safeStorage` reports as no encryption. Claude and ChatGPT then keep
no sign-in between launches.

Every Sekka desktop runs GNOME Keyring, unlocked at login, so the catalog's
Chromium, Chrome, ChatGPT, Claude, VS Code, Obsidian, Discord and Slack start with
`--password-store=gnome-libsecret` everywhere except Plasma
(`installer/genkan/rust/src/keyring.nix`). They keep a sign-in across launches
and logins, and what they sealed with the fixed key still opens.

- **Signal** is left out: it records the keyring it used and stops if that
  changes.
- **Bitwarden** reaches the keyring by itself.
- **The ISO:** its per-application closures carry the same wrapped packages, so
  offline installations find them on the medium.

## Preparation and regression checks

The ISO includes a read-only store closure of all offered application packages.
They become installed applications only when selected. Before changing the
target disk, the helper resolves the selected closure against the immutable
lock, keeps it rooted, and checks target capacity against that closure plus a
24 GiB reserve for the base system and installation workspace. Package-resolution
failures stop before erasure.

`nix flake check` evaluates all choices and none, and builds the complete package
cache. The rootless builder's additional configuration matrix evaluates
actual Rust-generated configurations for every choice, all, none and free-only,
including dependency normalization, rootless Docker, Firefox/Steam integration
and freshly probed filesystem UUID overrides. It can also be run separately:

```sh
# In the /workspace Linux build environment, checks both desktop and app cases:
respin-tools check-selections
# Standalone application-only driver:
python3 scripts/check_application_matrix.py
```

The pinned Linux `calamares-vm-fixture` must first exist under
`.work/native-fixture/<revision>/bin`. Each configuration evaluates in its own
Nix process to bound memory usage.

The standard `scripts/qemu_storage_matrix.rs` release matrix now includes
all-applications, no-applications and Rustup-only selections within its ten
filesystem/controller cases. The full ISO runner also accepts
`--applications all`, `none`, or comma-separated catalog IDs. Additional application selections use an 80 GiB disposable disk;
Firefox-only and empty selections retain the 40 GiB disk. After booting without
the ISO, the fixture compares the installed manifest and verifies package paths.

`tests/applications-runtime.sh` is run as the disposable VM's regular test user
after an all-applications installation. It compiles and runs C/C++ programs with
Make and CMake/Ninja, commits and clones a Git LFS file, downloads Rust stable
using Rustup, builds Rust linked to a
C library, runs a rootless Compose workload, and checks the terminal assistants'
startup. It requires network access for the Rust toolchain and container image.
Desktop launch and visual checks are recorded separately from authenticated
service use, which requires the user's accounts.

See [filesystem reliability](storage-reliability.md) for the original NVMe
failure and the used-disk, 4Kn and legacy BIOS regression cases. Emulator passes
do not establish the behavior of the physical ThinkPad's controller or firmware.

## Validation and hardware handoff

The packaged installer passed 41 library tests, one GUI model test and six
compile-fail doctests. The separately invoked storage VM passed all 18 real
ext4/Btrfs/XFS replacement and remount cases (512-byte and 4096-byte sectors),
plus FAT32 checks. All 34 generated application configurations and all 47
desktop configurations evaluated successfully. The native orchestration tools
passed 15 tests and Clippy with warnings denied. Both configuration matrices
also passed through the normal native build driver.

The new packaged GTK menu was visually exercised in a disposable live session:
search, the default Firefox selection, Rustup dependency selection and locking,
releasing that dependency, and clearing/disabling proprietary choices on opt-out.
The session imported the exact new installer package into the earlier live ISO;
this isolates UI checks from the still-pending new image validation.
[Menu result](test-results/application-menu.json),
[menu](images/applications-menu.png),
[Rustup dependency](images/applications-rustup-dependency.png),
[proprietary opt-out](images/applications-proprietary-disabled.png),
[Docker option](images/applications-docker.png). The corrected application ISO also rendered the
[application menu](images/applications-final-iso-menu.png) during its own
full installation tests: [exact-image result](test-results/application-final-menu.json).

All selected application packages built successfully, including their upstream
install checks. The complete Nix flake checks passed. The corrected application ISO is
9,560,510,464 bytes with SHA-256
`e7ec2362952305081c6e5ed6332962a13a50159f1f10352806607b93bbc45c37`.
An independent hash of the copied image matched the build output.
[Build record](test-results/application-build.json),
[checksum](test-results/application-iso.sha256). The previous application
candidate also passed a separate [BIOS live-boot smoke test](test-results/application-bios-live.json),
checking Linux, desktop/installer processes and Determinate Nix startup. This
process check does not claim rendered GUI readiness or an installed-system boot.

At the user's request, the ten final-image installation tests were stopped to
prepare the SanDisk for immediate physical-hardware testing. All ten had reached
the NixOS build/install phase, past filesystem preparation, formatting and
mounting; none had completed an installed-system boot. These are incomplete
tests, not passes. Installed application runtime and desktop startup tests have
not run. Logs and disposable disks are retained in the local artifacts.
[Handoff record](test-results/application-hardware-handoff.json).

The requested performance target is less than 60 seconds from pressing Install
after providing all inputs to installation completion on physical hardware.
This image has not demonstrated that target. Cross-architecture QEMU timing on
the ARM Mac is not a physical-hardware benchmark. The all-applications guest was
actively copying/building the target system when interrupted, with about 9.9 GiB
used after 60 minutes in the NixOS build phase. Full-system evaluation and copying
still happen after pressing Install; package caching alone does not establish
the requested completion time.

The SanDisk write completed: every one of the 9,560,510,464 image bytes matched
the source and checksum, the stale backup partition-table area was cleared and
checked, and the drive was ejected. Writing took 311.1 seconds and full read-back
87.2 seconds; these are Mac USB-transfer timings, not NixOS installation timings.
[USB report](test-results/sandisk-readback.json),
[write log](test-results/sandisk-hardware-flash.log).
Physical boot, installation completion and elapsed time remain for the user's
hardware test. The earlier filesystem-only image was archived and was never
written to this SanDisk. See the [agent handoff](agent-handoff.md) for resumption.

## Cold-module preflight regression

The first application-enabled candidate was rejected by a full-ISO NVMe/XFS
case before disk erasure: cold `modprobe xfs` exceeded a 15-second deadline
while the emulated live desktop was starting. The filesystem-only image's
passes did not expose that timing condition.
[Failure record](test-results/application-preflight-rejection.json).

The installer now allows 120 seconds for root/EFI filesystem module loading,
reports filesystem preparation in its progress text, and preserves the
underlying error with an explicit statement that no disk writes occurred.
A regression test checks both root and EFI module failures. The corrected ISO
has been rebuilt; its full installation matrix was interrupted for the requested
hardware handoff described above. The original
[rejected build record](test-results/application-rejected-build.json) is retained.

The full-install harness also allows both of the installer's separate
two-hour preparation and installation deadlines, plus test overhead. Its former
125-minute overall limit could otherwise interrupt a valid slow installation.
This changes test supervision only; installer deadlines remain unchanged.
The original all-applications run subsequently reached that old outer deadline
while the backend was still installing and is retained as an
[interrupted test](test-results/application-harness-timeout.json), never a pass.
The corrected image's unfinished tests are recorded separately in the hardware
handoff record.

## Profiler validation (2026-10-16)

Component checks: Superluminal 1.0.7705.679-alpha and Tracy 0.14.1 built with
the applications' Nixpkgs, Superluminal's install check resolving every
library of its 37 binaries and Tracy's own client, pkg-config and capture
round-trip tests passing. Genkan's, `rust/`'s and Yukimi's Cargo tests, the
`applications`, `template-lock` and `binary-cache` checks, and
`tests/check_installed_template.py` (Plasma, Tatami and GNOME/Xfce) passed, and
`nix flake check --no-build` evaluated all 70 checks.
The free systems `push-cache` publishes select Tracy and not Superluminal.

Image build and VM installation: an ISO built from the working tree of this
change, equal to its commit but for `nix fmt` whitespace, carried both
applications' closure lists. `respin-tools qemu-test --install` with
`--applications superluminal,tracy` and Tatami installed onto a disposable
80 GiB NVMe disk in 47.4 seconds from Install, downloading nothing. The
installed system booted without the ISO. The harness recorded a failure: its
fixture expects `sshd -T` to print `passwordauthentication no`, and this
OpenSSH prints `PasswordAuthentication no`; the GitHub step was otherwise
right (authorized key, `sshd` active, Git identity). On the same disk, the
fixture's other checks passed with that step skipped, and
`/etc/installer-applications.json` listed both packages at the tested store
paths, with their inputs locked beneath `sekka` in `/etc/nixos/flake.lock`.

Runtime, in that installed system's Tatami session: Tracy started from its
launcher command on Wayland and captured a live trace from a program built
against the same Tracy client (the virtual CPU needed
`TRACY_NO_INVARIANT_CHECK=1`). Superluminal started under XWayland and opened
its first-run wizard; activation was not attempted, so no capture ran.
`superluminalcmd` ran, and the setuid `pkexec` and Tatami's polkit agent were
present. Not tested: Superluminal's capture service, other desktops' sessions,
and physical hardware.

That installation predates keeping Superluminal off the media (2026-10-17):
it installed Superluminal from the ISO. With `"media": false`, the media's
application cache and closure lists hold Tracy and not Superluminal, the
`applications` check's assertion holds (and fails if Superluminal is put back
on the media), and Genkan's applications page, in its preview, marks
Superluminal Downloaded. Installing Superluminal by download while an
installation is prepared has not yet been run in a VM.
