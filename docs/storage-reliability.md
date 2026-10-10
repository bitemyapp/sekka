# Filesystem selection and NVMe mount reliability

This report records the filesystem-only image. The newer application-enabled
image and its separate validation are recorded in [optional applications](applications.md).

On 2026-10-04 the user reported `mount` exit status 32 on
`/dev/nvme0n1p2` during installation on a ThinkPad X1 Carbon Gen 14.
The screenshot shows failure at the root mount stage, after the ext4 formatter
returned success. It does not include the kernel message or the disk's previous
filesystem, so the exact physical cause has not been established.

## Gaps in the preceding implementation and tests

The old installer wiped the whole disk's signatures, created partitions,
formatted ext4, then immediately used an untyped `mount`. Wiping a partition
table does not wipe filesystem signatures within partitions. Udev could probe
while formatting, and its cached type could describe the previous filesystem.
A successful formatter exit was not followed by an uncached type check.

The previous QEMU runner always used a blank 40 GiB VirtIO disk. It did not
exercise NVMe, 4Kn namespaces, old filesystem signatures or reinstallation.
Its historical successful installs do not contradict the physical failure.

These changes address those concrete gaps; they are not a claim that the
screenshot proves a specific race or that hardware failures are impossible.
See [wipefs](https://man7.org/linux/man-pages/man8/wipefs.8.html),
[blkid probing](https://man7.org/linux/man-pages/man8/blkid.8.html) and
[systemd block-device locking](https://systemd.io/BLOCK_DEVICE_LOCKING/).

## Changes

- The GUI and immutable reviewed plan carry ext4 (default), Btrfs or XFS.
  Btrfs uses a single root volume with Zstd compression retained after reboot.
  Automatic snapshots and subvolume layouts are not configured. UEFI uses FAT32.
- Before erasing, the helper checks the selected formatter and live-kernel
  filesystem support. All live profiles explicitly include ext4/Btrfs/XFS/VFAT.
- The whole-device lock spans partitioning and formatting. The helper rereads
  the table and checks actual kernel partition parents, numbers and geometry.
  It wipes signatures on each new partition, formats, and verifies the result
  with uncached `blkid --probe`, refusing ambiguity or a wrong type.
- After releasing the device lock, it triggers fresh udev probing and waits for
  pending events, rechecks availability, then mounts with an explicit type.
  No automatic reformat/retry hides an I/O or mount failure.
- It probes the newly mounted root and EFI UUIDs directly from their
  superblocks and pins those device paths with `lib.mkForce` in
  `configuration.nix`. This overrides hardware detection's possible choice of
  an obsolete UUID alias for the same partition.
- Failures save a root-only `/run/calamares-storage-*.log` containing bounded
  partition/probe/kernel diagnostics. The GUI displays selectable, scrollable
  failure details. Logs must be copied before rebooting.
- The ISO source pin is updated to the companion fix; the installed system's
  existing three-input lock remains unchanged.

Pinned installer: `2c80fed7af924d9b2a959a8bd4ed70c7f5834cf8`,
source hash `sha256-qSqyQTyHPhVXs8o+GlLMhvbbHw9AAnbEma4i4R9dJPU=`.
The source archive was fetched and hashed with Nix, then successfully imported
through the ISO's normal `fetchFromGitHub` path.

## Regression coverage

The package exposes a separate NixOS storage VM check, included in both
repositories' `nix flake check`. It runs real formatting, uncached probing,
mounting, writes, unmounting and remounting on newly created regular-file-backed
loop devices. It covers all nine old/new ext4/Btrfs/XFS pairs with 512-byte and
4096-byte sectors and FAT32 mounts at both sizes. Normal unprivileged Cargo
runs explicitly report this privileged test as ignored. The Nix check invokes
it explicitly; it can use emulation when nested KVM is unavailable.

Backend full-ISO tests now default to NVMe with an existing Btrfs filesystem,
installing ext4. Options are `--disk-bus virtio|nvme|nvme4k`,
`--filesystem ext4|btrfs|xfs`, and
`--previous-filesystem blank|ext4|btrfs|xfs`. The fixture first checks invalid
requests and preflight on the blank disposable disk, then seeds the old
filesystem and populates probe metadata before invoking the packaged helper.
The disk serial and size guards apply to both transports. Reboot verification
checks the requested filesystem, persistent device paths and Btrfs compression.
The runner preserves the controller and filesystem when resuming a boot test.

Run the complete release matrix against a rebuilt ISO on x86_64 Linux with KVM:

```sh
rust-script --force scripts/qemu_storage_matrix.rs /path/to/candidate.iso
```

It performs ten complete installs and disk-only boots: all three filesystems on
UEFI NVMe, UEFI 4Kn NVMe and BIOS VirtIO, plus blank VirtIO/ext4. It stops on the
first failure and preserves the normal per-run JSON, logs and screenshots.
GUI tests use a blank disk and verify the filesystem selected in the runner.

For hosts without KVM, set `RESPIN_QEMU_ACCEL=tcg`. This uses QEMU's emulated
CPU and allows up to 15 minutes for the guest agent to start; the installation
assertions are unchanged. Set `OVMF_CODE` and `OVMF_VARS` to the firmware files
provided by the host's QEMU package. The result records the chosen accelerator.
`RESPIN_QEMU_ACCEL=tcg-single` keeps four virtual CPUs but runs their emulation
on one host thread. It is slower and can isolate multicore-emulation failures
without changing the ISO or installed system.

## Validation on 2026-10-04

Completed in a disposable Linux container on the macOS development host
(OrbStack kernel `7.0.14-orbstack-00380-ga7e0a2dc9535`, x86_64 Rust userspace):

- All 18 real filesystem replacement/remount cases passed, plus both FAT32
  mounts. Only temporary image-backed loop devices were formatted.
  [Storage matrix output](test-results/filesystem-storage-matrix.log).
- Full GTK/network build: 35 ordinary runtime tests and six compile-fail
  doctests passed. The privileged matrix was run separately and passed.
  [Native test output](test-results/filesystem-native-tests.log).
- Clippy with warnings denied passed for all Linux targets; the separate VM
  fixture compiles. The orchestration suite passed all 15 tests.
- Rust formatting and patch-whitespace checks passed. The release-matrix
  script compiled and rejected a missing ISO argument before creating a VM.
- QEMU 7.2.22 accepted the 4096-byte NVMe device configuration in a paused
  launch. This is a device-configuration check, not an installed-system boot.
- The NixOS VM storage check built and passed the real matrix under QEMU
  emulation, then passed again through the ISO repository's exact published
  source pin (204 seconds, kernel 6.18.54), including UUID replacement checks.
  [NixOS VM result](test-results/filesystem-nixos-vm.json).
  The outer disposable container
  required disabling Nix's incompatible seccomp filter and sandbox; source
  hashes and normal cache verification remained enabled.
- The pinned NixOS configurations evaluate with all four filesystem drivers
  enabled in all three live profiles.
- The first candidate rebuilt successfully at `091af7b` with installer
  `dc4127509d37c0e0d8144159744af0282a165127`: 3,989,078,016 bytes, SHA-256
  `72ee216339133f0dc72d5e71826f4040217b5b9cfd353a546797b548e2145b5b`.
  The copied image independently matched the build's checksum.
  It was rejected during full-ISO testing and was never written to the USB.
  [Build record](test-results/filesystem-rejected-build.json),
  [rejected checksum](test-results/filesystem-rejected-iso.sha256).
- The corrected ISO also rebuilt successfully with installer `2c80fed7a`:
  3,989,078,016 bytes, SHA-256
  `2eb35dcb58f8104721a91a35e1e74625c2343a34bdf8adfcd922166806a2a39d`.
  The copied image independently matched its build checksum, and the complete
  Nix flake checks passed. [Current build](test-results/filesystem-build.json),
  [checksum](test-results/filesystem-iso.sha256).
- All 47 generated desktop configurations evaluated successfully with the
  current installer pin and unchanged lock.
  [Desktop matrix](test-results/filesystem-desktop-matrix.json).
- Cold-building Determinate Nix on this macOS ARM host exposed host translation
  limitations in upstream tests. The unmodified syscall, store, expression and
  functional test derivations passed inside full x86_64 NixOS VMs. Their
  successful outputs were exported back into the build store; no checks were
  removed to produce the ISO. Upstream skips and disabled tests remain recorded
  in the [test summary](test-results/filesystem-build-tests.log).
- The corrected ISO's actual GTK filesystem menu was opened and visually checked:
  [ext4, Btrfs and XFS choices](test-results/filesystem-choices.png).
- The corrected NVMe/ext4 installation booted without the ISO, accepted the
  test user's password at the graphical login screen, displayed Plasma and
  responded to opening its application menu. The unchanged installed-system
  checks passed again after login, followed by a clean shutdown.
  [Result](test-results/filesystem-installed-gui.json),
  [login screen](test-results/filesystem-installed-login.png),
  [desktop and application menu](test-results/filesystem-installed-desktop.png).
- All ten full ISO installation/disk-only reboot cases passed against the
  corrected image and exact installer pin. Each used-disk case replaces a
  different filesystem. Btrfs compression survived reboot. The original
  blank VirtIO/ext4 case also passed.
  [Complete per-case results](test-results/filesystem-iso-matrix.json).

| Installed target | ext4 | Btrfs | XFS |
| --- | --- | --- | --- |
| UEFI NVMe, 512-byte sectors | Passed | Passed | Passed |
| UEFI NVMe, 4096-byte sectors | Passed | Passed | Passed |
| BIOS VirtIO | Passed | Passed | Passed |
| UEFI VirtIO, initially blank | Passed | Not run | Not run |

Two completed installations had their disk-only boot checks resumed after a
temporary coordinator misread fixture output and interrupted the batch. The
results retain that lineage. An interrupted, incomplete Btrfs installation
was rerun in full. No failed or incomplete installation was counted as passed.

UBS was also run and reviewed. It is not a clean scanner gate: its five existing
critical matches flag the fixed-command wrapper, synthetic unit-test password,
UID/libnm return-code comparisons and GUI password confirmation. The added
warnings are principally assertions/unwraps in tests and bounded allocations;
they were reviewed with the command ordering and cleanup paths. No broad
suppression rules were added.

## Additional failure caught by the rebuilt ISO tests

The first candidate completed root formatting, mounting and installation on a
used 512-byte NVMe disk, then entered emergency mode on reboot. Its generated
hardware configuration referenced EFI UUID `6099-DDBC`; the actual FAT32
superblock contained `60F4-E5D4`. A 4Kn NVMe case passed, demonstrating why a
single successful VM run was insufficient.

The pinned NixOS hardware detector selects the first existing UUID alias with
the same device number. A stale alias can therefore enter the configuration
even when explicit mounts and udev settling succeed. The current installer pin
uses directly probed UUIDs as the authoritative device paths. The desktop
evaluation matrix now injects incorrect detected root/EFI identities and
asserts that the generated configuration overrides both. Reused-disk fixtures
also seed an earlier EFI UUID (`0000-0001`).

During a corrected-ISO NVMe install, the deliberately seeded `0000-0001` alias
still pointed at the EFI partition, alongside its new `171D-97F1` alias. The
installer's generated configuration used `171D-97F1`, matching the uncached
superblock probe. This directly exercises the condition that broke the first
candidate. [Live crosscheck](test-results/filesystem-uuid-live-crosscheck.log).

[Failure record](test-results/filesystem-uuid-rejection.json) and
[boot screenshot](test-results/filesystem-uuid-rejection.png).

One corrected-image disk-only boot under QEMU 11.1.2's multicore TCG mode
panicked before loading any modules or mounting the root filesystem. It raised
`int3` in `sched_clock_cpu` while the reported instruction bytes were a NOP.
This matches the symptom described in the
[kernel/QEMU discussion of stale translated instructions](https://lkml.rescloud.iu.edu/hypermail/linux/kernel/2307.0/04274.html);
the match is an inference, not a diagnosis of the physical laptop. The
[original panic log](test-results/filesystem-mttcg-panic.log) is retained. The
same installed disk then booted successfully and passed the unchanged
filesystem, authentication and service checks with single-threaded TCG.

Outstanding release evidence: written-USB checks and the user's physical NVMe
retest. Writing the explicitly authorized SanDisk is awaiting macOS
administrator authentication; no write has started. The completed VM tests do
not establish the physical ThinkPad controller's firmware behavior.
The previous ISO and USB verification reports remain historical evidence.
