# Fast installation

Target: under one minute from clicking **Erase Disk and Install**, after all
inputs are supplied, to a finished installation on real hardware.

## What happens before the click

All expensive work that does not write to the target disk moves before the
confirmation. The installer's helper prepares the exact installed system while
the user reviews the summary and types the erase phrase:

1. **Identities first.** The helper chooses the root and swap UUIDs and the ESP
   serial, then detects hardware with
   `nixos-generate-config --no-filesystems`. That makes the complete
   `configuration.nix` known before erasure. Formatting later requests exactly
   these identities, and probing must read them back.
2. **One evaluation.** `nix build --dry-run --json` evaluates the system and
   reports the downloads it needs.
3. **Build in RAM, if it fits.** The live store is a tmpfs overlay, so
   downloads consume memory. If the unpacked downloads fit in available memory
   less a reserve (1 GiB or ⅛ of RAM, whichever is larger), the system is built
   in the live store before the click. Otherwise the build is deferred to the
   target store after formatting.
4. **Cache the closure.** Every file of the system's closure is read once
   through the live squashfs. Reading leaves its decompressed pages in RAM, up
   to the memory available at the start less the reserve. The compressed image
   pages read along the way are dropped immediately. Page cache stays
   reclaimable; nothing is pinned.

The installation media contain a **prebuilt reference system for every desktop**
(`rust/reference.nix` in the installer). An installation therefore shares almost
every store path with the media and only builds small per-machine derivations:
configuration files, users and the initrd. The media also carry each reference
system's and application's store path list at `/iso/calamares/closures/`.
The GUI uses those lists to warm RAM for the current selection while the user
is still choosing desktops and applications.

## What happens after the click

| Step | Work |
|---|---|
| Storage | One `parted` call; format with the chosen identities and verify them; mount with `noatime` |
| Files | `/etc/nixos` flake and modules, root-only password hash, Wi-Fi keyfiles |
| Copy | `nix copy --no-check-sigs` of the prepared closure from the (RAM-cached) live store into the target store |
| Bootloader | `nixos-install --system <prepared path>`: profile and bootloader only |
| Flush | `sync` the target filesystem and unmount |

## Measurements

Measured in KVM on the build workstation with 8 vCPUs and 16 GiB RAM. The
target is an emulated NVMe or VirtIO disk on the host's NVMe. The installation
ISO is attached as USB mass storage with reads **throttled to 200 MB/s** to
model a USB stick. Times come from the helper's own `timing` events, and
`CLICK_TO_COMPLETE_SECONDS` is measured by the test fixture from sending the
confirmation to `complete`. VM storage emulation is slower than a physical NVMe
drive, so treat these figures as conservative. They are **not** a substitute for
the physical ThinkPad result.

### Development images, 2026-10-04

Development ISOs built from the local installer tree. They contain Plasma and
Xfce (later all eight desktops) prebuilt, but not the optional-application
cache. "Verified" means the harness booted the installed disk without the ISO
and the fixture checked: the password hash and PAM login, locked root, time
zone and DST rules, migrated Wi-Fi, the requested filesystem pinned by the
booted UUID, `noatime`, desktops, Determinate Nix and the absence of live-only
files. With swap on it also checked the RAM-matched swap size, zswap, swappiness
100 and `resume=`; with tuning on, ananicy-cpp and the dirty limits.

| Firmware / disk | Filesystem (old) | Desktops, apps | Swap / tuning | Prepare (s) | Click → complete (s) | Copy (s) | Result |
|---|---|---|---|---|---|---|---|
| UEFI / NVMe | ext4 (Btrfs) | Plasma, Firefox | on / on | 23.3 | **42.4** (`nix copy`) | 39.2 | Verified |
| BIOS / VirtIO | Btrfs (XFS) | Xfce, none | off / off | 13.6 | **25.7** (`nix copy`) | 23.5 | Verified |
| UEFI / NVMe | ext4 (Btrfs) | Plasma, Firefox | on / on | 48.2 | 76.6 (direct copier, host busy building ISOs) | 69.6 | Verified |
| UEFI / VirtIO | Btrfs (ext4) | Omarchy + Hyprland, Firefox | on / on | 44.8 | 53.2 (direct copier, host busy) | 43.3 | Verified |

Preparation also includes a full preflight pass in these runs, so its time is
not representative of a first preparation. The Plasma closure is 10.2 GB and
9.99–10.12 GB of it was cached in RAM. Partitioning, formatting, verification
and mounting took 1.2 s; bootloader installation took 1.0–1.2 s.

On the workstation host itself, with no disk or CPU emulation, copying the same
10.2 GB closure took 6.1 s with `nix copy` and 8.1 s with an experimental direct
copier (parallel file copy plus database registration). In a sequential A/B on
a quiet host, with the same image apart from the copier, `nix copy` won:

| Copy method | Copy (s) | Click → complete (s) | Result |
|---|---|---|---|
| Direct copier | 43.9 | 48.0 | Verified |
| `nix copy` | 27.1 | **30.5** | Verified |

The direct copier was removed.

### GUI-driven installs of the release candidates

Driven through the real GUI over QMP, on full release images that include the
application cache and all eight prebuilt desktops:

| Image | Firmware / disk | Filesystem | Desktops (default) | Installer's own result | Result |
|---|---|---|---|---|---|
| `201a7c40…` | UEFI / NVMe | Btrfs | Plasma, Hyprland, Omarchy (Omarchy) | "Installed in 57 seconds" for an 11.4 GiB system (direct copier) | Verified; automated login to Omarchy; manual logins to Hyprland and Plasma |
| `1e1253a4…` (final) | UEFI / NVMe | XFS | Plasma, Hyprland (Hyprland) | Install clicked before preparation finished; after preparation: storage 1.1 s, copy 40.6 s (9.8 GiB), bootloader 0.9 s | Verified; automated login to vanilla Hyprland |

On the final image, preparation found 49 small per-machine derivations to
build and 16 MiB to download. Every package came from the media.

### Further verified installs

| Image | VM | Firmware / disk | Filesystem (old) | Desktops, apps | Click → complete (s) | Result |
|---|---|---|---|---|---|---|
| final `1e1253a4…` | 16 GiB | BIOS / VirtIO | ext4 (XFS) | Omarchy only, Firefox | **25.4** (9.0 GB system, fully cached) | Verified |
| final `bce81f55…` | 16 GiB | UEFI / NVMe | ext4 (Btrfs) | Plasma + Omarchy, Firefox | **42.1** (12.1 GB system, 9.9 GB cached) | Verified |
| dev, with zram | 6 GiB | UEFI / NVMe | XFS (ext4) | GNOME, Firefox | 39.4 (8.1 GB system, 2.3 GB cached; 6 GiB swap matched) | Verified, no out-of-memory |
| dev, no app cache | 6 GiB | UEFI / VirtIO | ext4 (blank) | Xfce + 10 free apps not on the media | **42.6**: 7.4 GiB of downloads exceeded memory, so the system was built on the target after formatting (deferred path) | Verified; all 10 applications present |

A dev image without the application cache, installing every catalog
application, also took the deferred path correctly. Its post-erase build then
compiled the AI applications from source (they are not on cache.nixos.org) and
one build failed. Release images carry those packages on the media. This run
led to readable failure reports: Nix's own error messages and the last build
output replace the raw `--log-format internal-json` stream. The earlier runs in the table above were made
while other VMs or ISO builds shared the host and are slower for that reason.

After installation, `/var/log/calamares-nixos/install.json` on the installed
system holds the stage timings and preparation summary. Read it after the first
boot to measure physical hardware.

A 6 GiB VM installing GNOME with Firefox ran out of memory twice while
evaluating, before any disk write, so nothing was written. The live session had
no swap. The live media now enable zram with CachyOS's values: zstd, 100% of
RAM, swappiness 150, page-cluster 0. The same 6 GiB GNOME installation then
completed and verified (below).

## Reproducing

From the graphical repository on a Linux host with KVM, Docker access to
`/dev/kvm` and Nix:

```sh
# Development image: local installer checkout, Plasma + Xfce prebuilt, no
# application cache (a scratch flake overriding respin.referenceDesktops and
# respin.cacheApplications).
RESPIN_CALAMARES_SRC=/abs/path/to/calamares nix build --impure path:/path/to/dev-flake#iso

# Static guest fixture from the same checkout.
cargo build --release --target x86_64-unknown-linux-musl \
  --manifest-path /abs/path/to/calamares/tests/vm-fixture/Cargo.toml

# Harness; QEMU and OVMF from Nix. Run inside a container with /dev/kvm when the
# user is not in the kvm group.
RESPIN_FIXTURE=/path/to/calamares-vm-fixture RESPIN_QEMU_MEMORY=16384 \
RESPIN_QEMU_CPUS=8 RESPIN_USB_BPS=200000000 \
CALAMARES_TEST_DESKTOPS=plasma CALAMARES_TEST_APPLICATIONS=firefox \
  respin-tools qemu-test ISO --install --firmware uefi --disk-bus nvme \
  --filesystem ext4 --previous-filesystem btrfs
```

`result.json` in the run's artifact directory contains `timing`: each helper
stage, the preparation summary, and the click-to-complete time.
