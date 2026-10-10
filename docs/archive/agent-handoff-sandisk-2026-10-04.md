# Installer and SanDisk handoff

Updated October 4, 2026, America/Chicago. The rebuilt installer has been written
to the user's SanDisk, every image byte was compared successfully, and the drive
was ejected. Physical ThinkPad installation, installed application runtime, and
the user's new performance requirement remain unverified. The user is resuming
this work with another agent; do not restart the abandoned emulator matrix or
rewrite the USB merely because older local state says those tasks are pending.

## User requirements and immediate next work

The original failure was on a ThinkPad X1 Carbon Gen14 installing to NVMe:
`mount ... /dev/nvme0n1p2 ... wrong fs type, bad option, bad superblock`, exit 32.
The screenshot alone does not establish its precise cause. The user asked for
standard filesystem choices, prevention of this failure class, an application
selection menu, and a rebuilt installation USB.

The latest performance requirement is **less than one minute from clicking
Install, after supplying all inputs, to installation completion on real
hardware**. This is a requested target, not an achieved or measured result.
The user became frustrated with hours of emulated testing and explicitly asked
to stop waiting and flash the available image for immediate hardware testing.

The next agent should first obtain the user's physical boot/install result and
elapsed time for the exact image below. Record filesystem, selected applications,
desktop, and any error details. If performance misses the target, measure time
in package preparation, Nix evaluation, target copying, bootloader setup and
flushing before changing implementation. Do not describe the current image as
meeting the target, fully installation-tested, or guaranteed never to fail.

## Repositories and branches

Both repositories use `codex/filesystem-install-reliability`:

| Repository | Local checkout | Draft pull request |
| --- | --- | --- |
| bitemyapp/calamares | `/Users/callen/work/calamares` | [Calamares PR 1](https://github.com/bitemyapp/calamares/pull/1) |
| bitemyapp/determinate-nixos-graphical | `/Users/callen/work/determinate-nixos-graphical` | [Graphical ISO PR 1](https://github.com/bitemyapp/determinate-nixos-graphical/pull/1) |

The ISO uses Calamares source `8547c691978b76c39a6fcbdcaf67d782331d1adb`,
with source hash `sha256-fFx0MYCT97xPwvfZVDp43pZkUZm4YK3RIqnUDLKFiH0=`.
Application implementation is in the preceding `d7c7e9231` commit; `8547c6919`
adds the cold filesystem-module deadline fix. The graphical build records
source commit `1bcc838`; subsequent commits add test evidence and documentation.
The handoff commits are also documentation only. **Do not repin or rebuild solely
because Calamares HEAD now includes its handoff document.** Neither PR is merged.

## Exact image and completed USB write

- Image: `artifacts/native-rust/nixos-graphical-determinate-rust-26.11.20261001.c59305b-x86_64-linux.iso`
- Size: **9,560,510,464 bytes**.
- SHA-256: `e7ec2362952305081c6e5ed6332962a13a50159f1f10352806607b93bbc45c37`.
- Lock SHA-256: `1ab98c5950de5484c81f3d1c52c5a657d0247e3eecb21c0c9cada9b8472ee079`.
- Builder output: `/nix/store/1m4nwxx54gyjbn4gwr4wacd9s5vdwykq-nixos-graphical-determinate-rust-26.11.20261001.c59305b-x86_64-linux.iso/iso/` followed by the filename above.
- USB: SanDisk 3.2Gen1, **123,048,296,448 bytes**, serial suffix `be1c`, previously
  Batocera. It was `/dev/disk4` for this write; that identifier must not be reused
  without a fresh identity check. The old Samsung was replaced by this SanDisk.
- Write took **311.1 seconds**; full byte comparison/read-back took **87.2
  seconds**. These measure flashing from the Mac, not NixOS installation.
- The final 1 MiB of the device was zeroed and checked to remove the old backup
  partition table. This was not a secure erase of the entire device.
- The report records `passed: true`, the exact matching hash and length, and
  `ejected: true`. This pass concerns the write/read-back, not boot or installation.

Committed evidence: [USB report](test-results/sandisk-readback.json),
[write log](test-results/sandisk-hardware-flash.log),
[image build](test-results/application-build.json).
The successful user-run launcher remains local at `.work/flash-sandisk.command`.
It calls `.work/flash-sandisk-hardware-candidate.py`, which checks the exact USB
serial, vendor/product, capacity, physical/removable identity, unmounted state,
source checksum, opened device identity, complete write and complete read-back.

The first background attempt obtained administrator approval but failed with
`EPERM` opening the raw disk before any writes. macOS logs showed the background
Python writer lacked storage permission. The user then ran the launcher in their
own terminal successfully. Do not ask for new permissions or repeat the write:
it is complete. Original failure log: `artifacts/native-rust/sandisk-flash.log`;
successful log: `artifacts/native-rust/sandisk-hardware-flash.log`.

## Implemented behavior

Filesystem choices are ext4, Btrfs and XFS. Preparation locks/revalidates the
target, checks kernel filesystem support before disk writes, verifies the kernel
partition layout, removes old signatures, probes newly formatted filesystems
without cached metadata, and mounts with an explicit filesystem type. Fresh root
and EFI UUIDs override stale generated hardware aliases. Errors include bounded
storage diagnostics. Cold root/EFI module loading allows 120 seconds; preflight
failures explicitly say that no disk writes occurred.

The 31 application choices are Firefox (default), Chromium, Google Chrome stable,
Codex TUI, ChatGPT Desktop, Claude Code TUI, Claude Desktop, oh-my-pi TUI, OpenCode,
Zed, VS Code, Neovim, Ghostty, kitty, Development build tools, Rustup, Lazygit,
Docker Engine + Compose, LibreOffice, Obsidian, Bitwarden, Signal, Discord,
Telegram, Slack, VLC, Spotify, OBS Studio, GIMP, Inkscape and Steam.
Thunderbird, KeePassXC and Gemini CLI were removed. The user explicitly chose
Docker Engine + Compose as the Linux replacement for macOS-only OrbStack.

The UI has checkboxes, categories, search, short descriptions and reviewed
selections. Rustup selects and locks build tools; deselecting Rustup unlocks the
tools without silently removing them. Build tools include GCC C/C++ with headers,
Make, binutils, CMake, Ninja, pkg-config, Git and patch. Rustup uses the Nixpkgs
wrapper; a toolchain is downloaded after installation with `rustup default stable`.
Docker runs rootless with Compose, user lingering and the proper user socket;
the installed user is not added to the privileged Docker group.

The helper validates application IDs, duplicates, dependencies and proprietary
software consent. Explicit empty selection works. The installed flake keeps the
application catalog/module and exact lock; `/etc/installer-applications.json`
records IDs, versions and package paths. The ISO caches every offered package,
but only selected applications enter the target system. Preparation checks the
selected closure and capacity before erasure, reserving an additional 24 GiB.

Application pins researched on October 4 are Nixpkgs unstable
`a7868a727837f3c09cee2ce0ca671c76b1589fed`, Numtide llm-agents.nix
`372f0337e8170e55ff0c017cd43ab73b02a062ad`, and upstream oh-my-pi 18.6.1
`2a2c6dcbbb558c0f8145f67f28b3370984f2bf60`. Base Nixpkgs, Determinate and FlakeHub
CLI pins were preserved. ChatGPT and Claude package official Linux application
artifacts through community NixOS definitions. All 39 unique package outputs
built; their shared cache closure is 22,093,309,808 bytes. See
[applications](applications.md) for source details and evidence.

## Code entry points

In Calamares, `rust/src/install.rs` orchestrates preflight, erasure, formatting,
configuration and `nixos-install`; `filesystem.rs` handles filesystem support and
diagnostics; `disk.rs` validates disks; `config.rs` writes the installed flake;
`applications.json`, `applications.rs` and `applications.nix` define/validate the
catalog and packages; `ui.rs` implements GTK selection; `plan.rs` and `helper.rs`
carry and revalidate the request. `nix/storage-test.nix` exercises real storage.
`tests/vm-fixture` is the guarded disposable-disk integration fixture.

In the graphical repository, inspect `flake.nix`, `flake.lock`,
`nix/calamares-source.json`, `nix/calamares.nix`, `nix/applications.nix`,
`rust/src/config.rs`, `rust/src/vm.rs`, `scripts/qemu_storage_matrix.rs`,
`scripts/check_application_matrix.py`, `tests/application-matrix.nix` and
`tests/applications-runtime.sh`.

For performance, the current backend still resolves flake metadata, evaluates
and roots the selected application closure, runs hardware detection, then calls
`nixos-install` to evaluate/build/copy the target after the click. Caching
packages does not eliminate this work. `process.rs` drains subprocess output
concurrently but only retains bounded output and reports it on failure; the UI
does not stream detailed Nix build progress. Improving measurements/progress and
reducing post-click work are pending, not part of this handoff commit.

## Validation boundaries

Completed on the current source/image as applicable:

- Linux packaged installer: 41 library tests, one GUI model test, six compile-fail
  doctests and all-target Clippy; host no-default build: 36 tests and six doctests.
- Privileged NixOS storage VM: 18 ext4/Btrfs/XFS replacement/remount cases across
  512/4096-byte sectors, plus FAT32 checks.
- 47 Rust-generated desktop configurations and 34 application configurations
  evaluated; reports are `test-results/application-desktops.json` and
  `test-results/application-configurations.json`.
- Orchestration: 15 tests, Clippy and release build. All application package
  builds/install checks, complete flake checks, ISO build and independent copied
  image checksum passed.
- Application-menu interactions covered search, Rustup dependency locking and
  release, and clearing/disabling proprietary selections on opt-out. The current
  ISO's menu also rendered and was visually inspected; see
  `test-results/application-final-menu.json` and `images/applications-final-iso-menu.png`.
- SanDisk full write/read-back and ejection, as recorded above.

**Incomplete:** all ten final-image full installations were stopped at the user's
request. Every case passed preparation, formatting/probing/mounting and reached
NixOS build/install; none completed an installed-system boot. They covered UEFI
NVMe and 4Kn NVMe with each filesystem over a different old filesystem, BIOS
VirtIO with each filesystem, and a blank UEFI VirtIO/ext4 baseline. Selections
included all applications, none, Rustup and Firefox. See the exact run IDs and
last progress in [interrupted matrix](test-results/application-hardware-handoff.json).
No installed application runtime, compiler/Rustup/Compose workload, 22-application
GUI startup sweep, physical USB boot or ThinkPad install result is claimed.

Earlier evidence must not be promoted to current-image results:

- Filesystem-only image SHA `2eb35dcb58f8104721a91a35e1e74625c2343a34bdf8adfcd922166806a2a39d`
  passed ten full installs, but had no application feature and was not flashed to
  this SanDisk. Archived locally under `artifacts/filesystem-only-2eb35dcb/`.
- First application image SHA `231a39d4501b62dcaf721851861a1f8589f3ef364dc95177d30d52fdd5e0eff0`
  passed Rustup/4Kn/ext4 and Firefox/NVMe/Btrfs installed boots, then was rejected
  because cold XFS module loading exceeded the old 15-second deadline. The old
  all-applications case hit the old outer harness deadline while still installing.
  Records: `application-preflight-rejection.json`, `application-harness-timeout.json`.
- The harness deadline was raised from 7,500 to 15,000 seconds to accommodate
  separate preparation and installation limits. Each backend limit is still
  7,200 seconds. These limits are not performance targets or evidence of success.
- `artifacts/native-rust/application-iso-matrix.json` is the **old rejected image's
  report**. The current interrupted report is
  `artifacts/native-rust/final-application-iso-matrix.json`. Do not confuse them.

## Local environment and resumption cautions

This host is an ARM Mac with 128 GiB RAM and 18 CPU cores, using native ARM QEMU
11.1.2 to emulate x86 Linux. Ten parallel single-thread TCG installs were a poor
validation choice and consumed excessive time. Prefer physical hardware or a
native x86 Linux builder with hardware virtualization for further timing/work.
The all-applications guest was actively doing Nix work, with about 9.9 GiB used
after 60 minutes in the build phase; it was not waiting for user input.
Brief guest CPU affinity experiments were restored and showed no clear benefit.

All owned test supervisors, QEMU guests, matrix coordinators and the waiting
desktop helper are stopped. Docker containers `calamares-iso-build-20261004` and
`calamares-iso-20261004` are stopped; retain their `calamares-iso-nix` volume.
Do not resume old process IDs or assume a partially written VM disk boots.

Large ISOs, logs, VM disks and `.work` helpers are ignored local artifacts and
will **not** accompany a Git clone. The evidence under `docs/test-results` and
screenshots under `docs/images` are committed. Both repositories are local here;
the detailed handoff is intentionally in this graphical repository.

The normal build container mounts this repository at `/workspace` and keeps the
Nix store. The privileged container also mounts Calamares at `/calamares` read
only, has Linux musl/GTK build dependencies and the prior Cargo cache under
`/tmp/calamares-uuid-target`. The native fixture is local at
`.work/native-fixture/8547c691978b76c39a6fcbdcaf67d782331d1adb/bin/calamares-vm-fixture`.
Use the documented builder/configuration-check commands in the README rather
than assuming the Mac can build x86 Linux GTK directly.

If using Mac QEMU again, prior working settings were `RESPIN_QEMU_ACCEL=tcg-single`,
`OVMF_CODE=/opt/homebrew/share/qemu/edk2-x86_64-code.fd` and
`OVMF_VARS=/opt/homebrew/share/qemu/edk2-i386-vars.fd`. Multithread TCG previously
caused a guest kernel panic; do not treat it as a validated acceleration fix.
Only one QEMU guest-agent client may own a socket at a time.

The local `.work/verify-installed-desktop.py` waits for an actually successful
all-applications install, then boots its disk without the ISO, requires visual
greeter/desktop checks, runs regular-user development/CLI/runtime tests, and
requires visual checks for 22 GUI applications. It has not run those checks.
Do not fabricate its `login-ready`, `desktop-verified` or application verification
markers. The original strict `.work/flash-sandisk.py` still expects those checks;
the user-requested hardware candidate used the separate writer documented above.
