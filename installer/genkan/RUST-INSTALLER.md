# NixOS-focused Rust Calamares

This branch contains a new native Rust installer for the Determinate NixOS
graphical respin, its window an Aizome app on Aizome's kit (`ui/`). It is **not** a translation of every upstream Calamares module,
and it is not compatible with Calamares's Python/C++ plugin API. The original
upstream source and licenses remain in the repository for provenance; the Nix
package builds only `rust/` and `ui/`.

The GitHub fork preserves Calamares history. Rust development starts at upstream
Codeberg tag `v3.4.2`, commit `36d30c492e5c7b5d6d32fed5c5d9790522e1eea3`.
The new implementation is GPL-3.0-or-later. It has no upstream endorsement.

## Supported first-release workflow

- x86_64 NixOS live ISO; network connection required. The live desktop is Aizome.
- Multi-select installed desktops: the Wayland desktops Aizome, Plasma, GNOME,
  vanilla Hyprland and Tatami (a Hyprland desktop inspired by Omarchy), and
  Xfce as the one X11 desktop, with an explicit default login session. Aizome
  and Tatami start selected, with Aizome the default session.
  MATE, LXQt and Cinnamon are no longer offered; the NixOS module still
  accepts them, so systems installed with them keep working.
- Every desktop offers Tatami's wallpapers from one copy of each image, and
  starts with its own: the ocean wave in Plasma (a Breeze Global Theme with
  that wallpaper), the foggy forest in GNOME, the mountains at dusk in Xfce
  (xfdesktop built with it as the default backdrop) and Da Nang at night in
  Tatami.
- Every desktop includes [Yukimi](../../apps/yukimi/README.md), the
  app for seeing what is installed, finding and installing packages,
  updating, returning to an earlier generation and cleaning up the store,
  without editing configuration.
- Guided erase of one whole disk, GPT, ext4 (default), Btrfs or XFS, EFI/systemd-boot or BIOS/GRUB.
- A swap partition matched to installed RAM, with zswap and hibernation resume
  (default on), and CachyOS-inspired kernel, memory, I/O and service defaults
  (default on). Both are reviewed choices with an opt-out.
- The installed system is evaluated, built and cached in RAM while the user
  reviews the plan. After the typed confirmation the installer only partitions,
  formats, copies the prepared system and installs the bootloader.
- Hostname, normal user with sudo, password, full name, time zone, a selection
  of eight system locales and keyboard layouts. Allowing unfree packages is
  enabled by default, with an explicit checkbox opt-out and review summary.
- A keyboard step that asks for the key just left of the space bar, through
  Aizome's compositor, and recommends swapping left Alt and Super when it's Alt
  (`altwin:swap_lalt_lwin` in `services.xserver.xkb.options`), preserving
  right Alt (AltGr); the person can
  keep the keys as they are. ❖ there needs nothing, and Genkan leaves any
  other key as it is.
- An optional GitHub step: the public SSH keys of a GitHub account as the
  user's authorized keys, the OpenSSH server, and Git's name and email address
  (see [GitHub, SSH and Git](#github-ssh-and-git)).
- Pinned Nixpkgs, Determinate Nix and `fh` inputs supplied by the installation
  media. The installed system keeps the same lock file.

No manual partitioning, existing-OS preservation, encryption, RAID/LVM,
offline installation, upstream plugins or interface
translations are implemented. The GUI says this before offering installation.
Treat this as an experimental, NixOS-focused fork; VM verification is not a
guarantee for every physical machine. Do not erase irreplaceable data without a
backup.

### GitHub, SSH and Git

The step after the account (`github.rs`, all optional) works like Ubuntu's
server installer:

- **SSH keys.** For a GitHub username, the GUI fetches
  `https://github.com/USER.keys` over HTTPS (curl, TLS 1.2+, the media's CA
  bundle) and lists each key's type and `SHA256:` fingerprint, as
  `ssh-keygen -l` prints them. Keys the user allows become
  `users.users.USER.openssh.authorizedKeys.keys`. Only the algorithm and the
  base64 key are kept; each key must name the same algorithm inside as
  outside, and types OpenSSH no longer accepts are left out and counted.
- **SSH server.** A switch on the same page, not in the applications list,
  enables `services.openssh` (open in the firewall). With keys authorized it
  refuses passwords and keyboard-interactive login; with none, the account's
  password is the only way in, so it is accepted.
- **Git identity.** Name and email address for `programs.git.config.user`
  (`/etc/gitconfig`, for every account; `git config --global` overrides it).
  The name follows the account's full name, or the GitHub profile's name when
  the account has none; a lookup fills in the profile's public address, or
  GitHub's private commit address `ID+USER@users.noreply.github.com`. Fields
  the user edits are left alone. Both are set, or neither.

The GitHub username is the only thing sent, to github.com and to
api.github.com for the profile; the profile is best effort (the API allows 60
unauthenticated requests an hour). A username that was not looked up, or was
changed after its lookup, cannot reach review: Next looks it up first. As with
every other field, the helper parses the GitHub username, each key and the Git
identity again from the request, and `configuration.nix` receives only the
parsed values.

### Filesystems and storage verification

The root filesystem is an explicit reviewed choice: ext4, Btrfs or XFS. Btrfs
uses a single root volume with `compress=zstd`; automatic snapshots and a
subvolume layout are not configured. UEFI always gets a separate FAT32 ESP.
Older JSON requests default to ext4; unknown filesystem names are rejected.

Before erasure the helper checks the selected formatter and live-kernel support.
It chooses the new filesystem identities itself: random UUIDs for root and swap
and a FAT serial for the ESP. Hardware detection runs with
`nixos-generate-config --no-filesystems`, and `configuration.nix` declares the
filesystems and swap by those identities, pinned with `lib.mkForce`. Hardware
detection run later can therefore never select an obsolete
`/dev/disk/by-uuid` alias. Choosing identities first is what allows the complete
system to be built before anything is written.

The GPT layout is ESP (EFI) or a BIOS boot partition, then root, then optional
swap at the end, created by a single `parted` invocation with exact MiB
boundaries. The helper holds the whole-device lock through partitioning and
formatting. It checks the kernel's partition count, parents, numbers and exact
geometry, and wipes signatures inside the new partitions. Each formatter is
given the chosen identity; uncached `blkid --probe` must then report the
expected type **and** that exact identity, otherwise installation stops. The
lock is released before triggering udev (udev postpones events for locked
disks), then filesystems are mounted with an explicit type and `noatime`
(Btrfs also uses `compress=zstd:1`).
Failures preserve bounded storage/kernel diagnostics in a root-only
`/run/calamares-storage-*.log`; copy that file before rebooting. The GUI displays
scrollable, selectable failure details.

`nix flake check` includes a separate Linux VM storage test. It runs the real
format/probe/mount implementation against temporary loop images, covering all
nine old/new filesystem pairs at both 512-byte and 4096-byte sector sizes,
checking new UUIDs and data after remount, and checking FAT32 replacement and ESP
mounts. The test is ignored
in ordinary unprivileged Cargo runs; it must run explicitly inside disposable
Linux infrastructure with loop/mount access:

```sh
cargo test --manifest-path rust/Cargo.toml --no-default-features --locked \
  filesystem::tests::real_reformat_mount_matrix -- --ignored --nocapture
```

The companion ISO runner additionally tests actual NVMe and VirtIO controllers,
used disks, firmware boot and the installed filesystem after reboot. Successful
loop tests alone do not establish that a rebuilt ISO or a physical NVMe boots.

### Wi-Fi and time zone

Every generated desktop configuration enables NetworkManager and redistributable
device firmware. That firmware can be proprietary; unchecking the additional
unfree-packages option does not promise a strictly free-software-only system.
The upstream hardware generator still supplies detected storage, CPU microcode
and device settings.

NVIDIA GPUs use NVIDIA's own driver rather than nouveau (`rust/system/nvidia.nix`,
`src/graphics.rs`). The GUI reads display controllers and connected displays
from sysfs and sends the result with the request; the helper accepts only
well-formed bus IDs. The driver is the latest release with open kernel modules,
which support Turing and newer GPUs, the only ones the latest driver supports.
Video memory is preserved across suspend and hibernation.

Which GPU is primary follows where the displays are connected: the GPU above
the connected eDP, LVDS or DSI panel connector in `/sys/class/drm`, otherwise
the one with any connected display.

- **Displays on the Intel or AMD GPU:** PRIME offload. `nvidia-offload` runs a
  program on the NVIDIA GPU, and fine-grained power management turns it off
  when idle.
- **Displays on the NVIDIA GPU:** the NVIDIA GPU is primary. This covers a
  laptop MUX switch in discrete mode, and a desktop's monitor on the graphics
  card.
- **No connected display visible:** laptops (SMBIOS chassis types 8, 9, 10, 14,
  31 and 32) assume offload, and desktops assume the NVIDIA GPU.

The choice matters only for X11 sessions (Xfce, and on older installations
MATE, LXQt and Cinnamon). The login screen (GDM when GNOME is installed, Plasma
Login Manager otherwise) and the Wayland desktops find the displays on any GPU
by themselves. Under offload, X draws only on the
integrated GPU; otherwise, only on the NVIDIA GPU. On an Acer Predator Helios
Neo 14 with its MUX in discrete mode, offload under SDDM's X11 login screen
left the screen black.
`examples/detect-graphics.rs` prints the detection on any machine without
privileges.

The review page shows the choice. If unfree packages are declined, nouveau
remains. The installation media carry the same driver build, so an
installation copies it instead of downloading NVIDIA's installer and building
it.

Wi-Fi transfer is enabled by default, with an opt-out and a profile count on
the review page. The GUI worker queries NetworkManager as the live user, so
its unlocked Secret Agent/wallet can supply saved passwords. The helper
independently parses and validates the profiles with libnm before erasing.
It writes root-owned mode-0600 keyfiles under
`/etc/NetworkManager/system-connections`, never into the flake or Nix store.
Live-user restrictions are remapped to the installed username; agent-owned
credentials become system-keyfile secrets. Ethernet and VPN profiles are not
copied. Live settings are never modified. Missing secrets or external EAP
certificate/key references stop review rather than silently installing a
broken connection: unlock/save the live connection or disable transfer and
configure that enterprise network afterwards. No actual host Wi-Fi profiles
are accessed by the tests; they use public synthetic credentials in guests.

The time zone is chosen on a world map (`src/zonemap.rs`, `../ui/src/view/map.rs`)
or from a searchable city list. A click resolves to a zone from the live
tzdata's `zone.tab`. Within the clicked country, only zones in the clicked
standard-time band are considered, and the nearest of those is chosen. So
western Kentucky gets Central time even though Louisville (Eastern) is closer.
Overseas regions drawn as part of their country (French Guiana) match the
nearest zone in their band. Kosovo and other areas without a zone of their own
use the nearest zone in their band. Open ocean far from every zone selects
nothing. Natural Earth supplies country outlines and 2012 standard-time bands;
CLDR supplies names. Both are embedded and regenerated by
`examples/zonemap-data.rs` from the pinned inputs in `data/zonemap-sources.md`.

Detection, on a worker, proposes a zone in this order:

1. The live system's regional zone.
2. IP geolocation through geoip.kde.org (the service upstream Calamares uses),
   then ipinfo.io. Each is a bounded HTTPS request, and the service sees the
   public IP address. Internet detection can be disabled.
3. The hardware clock's lead over an NTP-synchronized system clock, rounded to
   quarter hours, when it keeps local time. The most populous zone with that
   current offset is proposed.
4. `America/New_York`.

The page names the source. Manual choices win over late responses, and on
entering the page an earlier fallback is retried. Selecting a zone does not
change the live OS.

## Architecture and safeguards

`calamares-nixos` runs as the live user. Its window (`../ui`) only updates its
state from messages and draws it. Disk discovery, filesystem reads, validation,
authorization and helper communication run on workers; hashing, downloads,
partitioning and installation run in the privileged Rust helper. The window
shows a moving progress bar throughout work. No shell command is assembled from user
input by the installer.

### Preparation before confirmation

The GUI starts the helper as soon as review succeeds, using the `session`
protocol (`src/session.rs`). The protocol is newline-delimited JSON on stdin:
first the reviewed plan without a confirmation, later at most one
confirmation line. While the user reads the summary and types the erase phrase,
the helper prepares without writing to any disk (`install::prepare`):

1. Repeats the live-media, firmware, user and disk checks and filesystem
   preflight, then takes the installer lock.
2. Computes the RAM-matched swap size (`MemTotal` rounded up to whole GiB) and
   the exact partition layout. A disk too small for the root minimum fails here.
3. Chooses filesystem identities, detects hardware and writes the complete
   target flake into a root-only staging directory.
4. Runs one `nix build --dry-run --json`. That evaluates the system and reports
   how much must be downloaded.
5. Builds the system in the live store if the unpacked downloads fit in
   available memory less a reserve. Otherwise it defers the build to the target
   store after formatting, which is slower but never exhausts the RAM-backed
   live store.
6. Checks the root partition against the closure size plus 8 GiB.
7. Warms the page cache with the whole closure (`precache.rs`). It reads at most
   the memory available at the start less a reserve, and drops the compressed
   squashfs pages behind it, so the later copy is not limited by the USB stick.

Closing stdin before confirming cancels preparation (`Event::Cancelled`):
running Nix commands are killed and nothing was written. The confirmation line
is checked by the helper against its own parsed plan (`InstallPlan::confirm`);
only the resulting `ConfirmedInstall` can be executed. `install::execute`
re-renders the configuration from the confirmed plan and requires it to equal
the prepared one. It then partitions, formats with the chosen identities, writes
files, copies the closure and installs the bootloader with
`nixos-install --system`.

The closure is copied with `nix copy --no-check-sigs` from the live store.
A direct parallel file copy with database registration was measured and
removed: it was slower in VMs (48.0 s against 30.5 s click-to-complete) and on
the host. The helper reports each stage's duration as `Event::Timing`, the GUI
shows the installation time, and `/var/log/calamares-nixos/install.json` on the
installed system keeps the stage timings and preparation summary for
measurements on real hardware.

The GUI separately warms the page cache while the user is still choosing. Two
seconds after desktop or application choices settle, it reads the store paths
listed in `/iso/calamares/closures/` for the selection. The installation media
generate those lists from prebuilt reference systems (`reference.nix`).

`calamares-nixos-helper` accepts a strict JSON request on stdin. The GUI starts
it through NixOS's setuid Polkit wrapper, using a policy restricted to this
executable. Passwords never appear in argv, process titles or progress messages.
The backend independently parses the request and requires root, NixOS,
a temporary root filesystem, a mounted live ISO and root-owned settings.

### Parse once per process boundary

The strict JSON `RawRequest` is only a transport/form object. Consuming it with
`parse` produces an immutable `InstallPlan`; configuration generation accepts
only that plan. `Hostname`, `Username`, and `TimeZone` retain checked values.
`DesktopSelection` keeps the nonempty, unique, compatible selection together
with a default that belongs to it. `WifiTransfer` represents either opt-out or
a bounded collection of parsed, normalized profiles with unique connection
identities. The writer uses those retained bytes, without parsing them again.

Review does not fabricate an erase phrase. A separate consuming `confirm`
transition binds the exact phrase to the plan's disk and returns
`ConfirmedInstall`, the only input accepted by the executor. Serializing for
IPC deliberately returns to raw data: the privileged helper independently
parses and confirms it using its own root-owned settings. Neither plan type
implements `Deserialize`, `Debug`, or `Clone`; password/profile owners zeroize
their Rust buffers when dropped. External libnm allocations, and the copies the window's
text fields and messages make, are not claimed to be zeroized.

This is intentionally not a type for every string or process step. Full names
remain strings; locale/keyboard choices resolve to allowed static values;
cross-field rules belong in aggregate constructors. Runtime checks still guard
live/root status, current firmware, mount state and disk identity immediately
before writes. A parsed plan describes intent, not permanently safe hardware.
Parsing that reads tzdata or calls libnm runs on workers, not the interface thread.

No target is selected automatically. The review page requires both a destructive
checkbox and typing `ERASE /dev/<device>`. Before writing, the helper rechecks
the device path, major/minor, size, serial, WWN and model. Mounted filesystems,
live media, active swap, device-mapper/RAID holders, read-only devices, virtual
memory devices and disks below 24 GiB are rejected. The `lsblk` device tree is
explicitly requested; a flat partition list fails closed. Installer and device
locks prevent concurrent cooperating instances. Disconnecting disks during an
installation is unsupported.

The helper hashes the password with a random salt and SHA-512 crypt (100,000
rounds), then zeroizes its plaintext buffer. A root-only hash file lives at
`/etc/nixos-secrets/user-password.hash`, **outside** the `/etc/nixos` flake
source. Nix references it as a runtime string path, not a store path. Root login
is locked. The normal user belongs to `wheel`. To make a password change persist
through subsequent NixOS activation, update/remove the declarative
`hashedPasswordFile` setting as appropriate; protect the hash file and backups.

Once destructive work starts the window cannot be closed normally. Failures are
reported as incomplete installation, never success. Command timeouts terminate
only the process group spawned by that operation. Cleanup attempts to unmount;
it never recursively deletes a target mount directory. There is no automatic
reboot. The installer cannot promise rollback after a partition table is erased.

## Updating installed systems

The installed `/etc/nixos` contains `flake.nix`, `flake.lock`,
`configuration.nix` and `hardware-configuration.nix`. Sekka supplies the
installer's system modules, Tatami and Yukimi through one `sekka` input.
The [installed template](../../templates/flake.nix.in) records its external
inputs and follows relationships. The generated lock pins the exact source
used by the image.

```sh
sudo nix flake update sekka --flake /etc/nixos
sudo nixos-rebuild boot --flake /etc/nixos   # then reboot
```

Updating all inputs also updates Nixpkgs and application sources. Development
uses `main` and topic branches; releases use tags. No moving `stable` branch
exists in Sekka.

`sekka.nixosModules.default` provides the existing `calamares.*` options and
imports Tatami and Yukimi from the monorepo. Their individual modules are
also available as `sekka.nixosModules.tatami` and `sekka.nixosModules.yukimi`.
See [DEVELOPERS.md](../../DEVELOPERS.md) for the public outputs and migration.

Existing machines with separate `calamares`, `tatami` and `yukimi` inputs
need a deliberate configuration migration. Back up `/etc/nixos`, replace those
inputs and their follows declarations with the `sekka` input from the current
template, and replace their module imports with `sekka.nixosModules.default`.
Keep hardware configuration, user settings, secrets and `system.stateVersion`.
Review the new lock and evaluate/build before switching. The former `stable`
references remain fixed compatibility tags; they no longer receive updates.
Do not delete local modules until their settings have been accounted for.

## Build and test

```
cargo test --manifest-path installer/genkan/rust/Cargo.toml --locked
cargo clippy --manifest-path installer/genkan/rust/Cargo.toml --all-targets --all-features -- -D warnings
cargo fmt --manifest-path installer/genkan/rust/Cargo.toml --check
cargo test --manifest-path installer/genkan/ui/Cargo.toml --locked
cargo clippy --manifest-path installer/genkan/ui/Cargo.toml --all-targets -- -D warnings
cargo fmt --manifest-path installer/genkan/ui/Cargo.toml --check
nix build .#genkan --no-update-lock-file
```

Local window builds need Wayland, xkbcommon, fontconfig and libnm development
libraries; Nix supplies them. The window's crate (`ui/`) builds against
Aizome's kit by path (DEVELOPERS.md, "Genkan"). The Nix package includes the
window, helper, desktop entry and Polkit policy; it deliberately does not
contain the separate `tests/vm-fixture` crate.

Root-owned `/etc/calamares-nixos/settings.json` supplies `template_dir`,
`zoneinfo`, `state_version` (`26.11` or `26.05`), and `kernel` (`lts` or `latest`).
The template directory contains `flake.nix.in` with exactly one `@HOSTNAME@`
placeholder and a version-7 lock with exactly `nixpkgs`, `determinate`, `fh` root
inputs. `test_diagnostics` defaults false; it enables QEMU diagnostics only when
the guarded disposable test disk is present. It is not accepted in UI requests.

`scripts/build-rootless.rs` reuses the graphical respin's existing 100 GiB
regular-file-backed builder VM. `scripts/verify-qemu.rs` boots the verified
baseline ISO read-only as USB media and creates a fresh 40 GiB regular-file disk.
Both scripts use pinned Rust tooling from the published respin migration commit.
They require rust-script, QEMU/KVM, and the prerequisite respin builder setup.

```
scripts/build-rootless.rs /path/to/determinate-nixos-graphical /path/to/bootstrap.iso
scripts/verify-qemu.rs /path/to/verified-graphical.iso uefi
scripts/verify-qemu.rs /path/to/verified-graphical.iso bios
```

For faster backend iteration, append `--dev-backend` to use a newly compiled
static Rust helper with the last packaged GUI. Such runs are explicitly marked
development-only in their result JSON and do not count as release verification.
Repeat without that flag against a fresh Nix build before publishing a release.

Use executable shebangs or `rust-script --force` so shared-source edits are
recompiled. Tests launch the actual Nix-packaged GUI and helper. The automation
invokes the helper's normal stdin protocol for installation; a GUI screenshot
alone does not verify every interactive widget path. The installed system boots
without the ISO, and tests check its password hash, PAM authentication with wrong
and correct passwords through a pseudo-terminal, locked root, desktop,
Determinate services and absence of live-only installer configuration. This is
not itself an interactive graphical desktop-login test. The integrated ISO's
supervised GUI tests and desktop-login evidence are recorded in the graphical
repository's [verification report](https://github.com/bitemyapp/determinate-nixos-graphical/blob/codex/rust-calamares-integration/TESTING.md).

Artifacts and disposable disks stay under ignored `artifacts/` and `.work/`.
No host sudo, real block-device write, physical USB modification or host reboot
is needed for these tests.

## Optional applications

The Applications tab offers a searchable, categorized list with short descriptions.
Firefox is selected initially; an empty selection is valid. The packaged catalog
in `rust/src/applications.json` is shared by the GUI, privileged parser, and Nix
module. Unknown IDs, duplicates, and proprietary choices without consent are
rejected before installation. Rustup adds Development build tools before review
and again when the helper parses the request. Required build tools cannot be
deselected in the GUI while Rustup is selected.

The build-tools choice includes wrapped GCC/G++ and libc headers, GNU Make,
binutils, CMake, Ninja, pkg-config, Git, patch, and `file`. Rustup uses Nixpkgs' NixOS-aware
package; users select a toolchain with `rustup default stable` after installation.
Additional native libraries should be provided by a project development shell.
Build tools, Lazygit and the GitHub CLI each bring Git, and with it Git LFS
through `programs.git.lfs`, which sets the LFS filter in `/etc/gitconfig`.
Docker Engine + Compose uses the rootless user service with lingering enabled;
it does not grant membership in the privileged `docker` group. OrbStack itself
is macOS-only.

The target flake supplies independently pinned application Nixpkgs, Numtide's
`llm-agents.nix`, and upstream oh-my-pi. Its full lock is preserved. Kakehashi
(`"source": "sekka"`) comes from Sekka itself, built with the system's Nixpkgs:
`nix/components.nix` gives the module `sekkaPackages`. Both app
catalog and module come from the flake's `calamares` input (see "Updating
installed systems"), with the selected IDs in `configuration.nix`; installed
versions and store paths are recorded in
`/etc/installer-applications.json`. Terminal applications receive menu launchers.

Before the first disk write, the helper builds the complete installed system,
including the selected applications, and checks that the root partition holds
its closure plus 8 GiB of headroom. Failure at this stage leaves disk contents
unchanged. The integrated ISO caches the complete catalog and a prebuilt system
for every desktop in its read-only store, so preparation normally downloads
almost nothing into live-session RAM. Selections whose downloads would not fit
in memory are built on the target store after formatting instead. This does not
make the complete OS installation offline.

Application test runs may use an 80 GiB image in addition to the original 40 GiB
image. Both require the exact `RESPIN_TEST_ONLY` serial and expected VM device
path before privileged diagnostics can run.
