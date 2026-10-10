# Wi-Fi backend and hardware-friendly defaults

2026-10-04, America/Chicago. Branch: `codex/live-hardware-reliability`.

## Physical feedback and root cause

The user booted the preceding `a06e8605…` image's **7.2.8 Plasma** entry and
reported that the earlier display/input problems were resolved. Plasma's Wi-Fi
menu showed no SSIDs, including after turning Wi-Fi off and on. No journal from
that physical live session was retained.

The ISO forcibly disabled `networking.wireless.enable`, removing
`wpa_supplicant` even though NetworkManager selected that backend. The pinned
[NetworkManager module](https://github.com/NixOS/nixpkgs/blob/c59305bab2065cfecc4944690d9eedbb56f3a9fa/nixos/modules/services/networking/networkmanager.nix#L653-L658)
normally enables the supplicant in D-Bus-controlled mode. Removing the forced
disable restores that arrangement; this does not start a competing standalone
Wi-Fi manager.

The working host has an Intel BE211 (`8086:e440`, subsystem `8086:0110`), using
`iwlwifi`/`iwlmld`. Read-only inspection found the matching driver alias and
`iwlwifi-sc-a0-wh-b0-c106.ucode` in the ISO. Its decompressed firmware SHA-256,
`253876486e3b45267c9b67b510b90b2a6dc67e7d9b51ef4b367bf4813db693d3`,
matches the working host's firmware. Missing firmware is not the demonstrated
defect here.

The [old-image regression result](test-results/wifi-before.json) is an expected
**failure**, not a pass: the real ISO rejects D-Bus activation with “The name is
not activatable.” A separate simulated-radio diagnostic also reproduced
NetworkManager's unavailable radio/failed scan. The correction passes actual
scan/authentication tests below. This establishes a concrete software defect,
but the physical BE211 still needs a retest after the fix.

Earlier tests proved Wi-Fi **profile transfer and loading**, not radio scans or
authentication. That coverage gap allowed this live-image bug to escape.

## Firmware and unfree policy

- Live image: redistributable firmware, Intel **and** AMD early CPU microcode,
  unfree-package permission, and NetworkManager's working supplicant backend.
  Both microcode file names were found in the final image's latest-kernel early
  initrd. The profile checks require both update options for all three entries.
- Installed system: explicitly enables redistributable firmware, uses upstream
  hardware/CPU detection, and defaults the GUI's unfree-package checkbox to on.
  An explicit user opt-out is retained and tested through parsed-plan round trips.
- Opting out of additional unfree packages **does not remove firmware blobs**
  or produce a strictly free-software-only system. This is explained in the UI.
- `hardware.enableAllFirmware` remains false. The normal redistributable bundle
  is enabled; this is not a claim to include every restricted/special-purpose
  firmware package. See the pinned [firmware module](https://github.com/NixOS/nixpkgs/blob/c59305bab2065cfecc4944690d9eedbb56f3a9fa/nixos/modules/hardware/all-firmware.nix).
- NVIDIA GPUs get NVIDIA's driver (latest release, open kernel modules) on the
  live system and on installed systems, never nouveau. The installer reads the
  bus IDs from sysfs and adds PRIME offload on hybrid laptops; see the installer's
  RUST-INSTALLER.md. Unusual out-of-tree drivers still need hardware-specific
  setup.

The existing latest/LTS kernel choices, display/input recovery profile and
readable boot labels are preserved. The system lock is unchanged.

## Exact image and source

- Image: `artifacts/native-rust/nixos-graphical-determinate-rust-26.11.20261001.c59305b-x86_64-linux.iso`.
- Size: **3,989,078,016 bytes**.
- SHA-256: `26444afb6b01cc35b6e226de06308ba3301570888afe0ef4ab139d9b7b1f5ef5`.
- Nix output: `/nix/store/925gnfj20dmgw7xdyldlz213n7cf022y-nixos-graphical-determinate-rust-26.11.20261001.c59305b-x86_64-linux.iso`.
- Native installer: `ffe4d9faad2c99220de0b6855c215fed209a63f7`.
- Source hash: `sha256-StMw1Ezb2C9472KEuLc6Dh+OI5p9FlelR0R2QkxYZZc=`.
- System lock SHA-256: `2a1e300d41e32889d2f108a9405294cf89f0a196d755bfc4bf43b8eea9852d69`.

The host independently checked the exported ISO against its checksum sidecar.
These identify this custom build's bytes, not a vendor signature. Nix source
hash/package verification remains enabled.

## Verification

All four flake checks passed. Native tests passed: **26 library tests, one GUI
unit test and six compile-fail doctests**. The **14 orchestration tests**,
Rustfmt, Clippy with warnings denied, and child-process cleanup checks passed.
The [47 supported desktop combinations](test-results/wifi-desktop-matrix.json)
were evaluated from the actual newly generated configurations: each enables
NetworkManager's supplicant, redistributable firmware and default unfree policy.
These are Nix evaluations, not 47 installation or physical-radio tests.

### Real Wi-Fi backend, simulated radios

The new `scripts/test_wifi.rs` uses
[`mac80211_hwsim`](https://wireless.docs.kernel.org/en/latest/en/users/drivers/mac80211_hwsim.html)
inside disposable QEMU guests. It uses the actual ISO's NetworkManager and
supplicant, a WPA2 access point in a separate network namespace, and public
synthetic credentials. No host radio, saved credentials or target disk is
attached. Test-only AP tools are imported separately, not shipped on the ISO.

| Final-image profile | Result |
| --- | --- |
| 7.2.8 Plasma default | [PASS](test-results/wifi-default.json) |
| 7.2.8 Xfce/X11 recovery | [PASS](test-results/wifi-recovery.json) |
| 6.18.54 Plasma LTS | [PASS](test-results/wifi-lts.json) |

Each pass requires SSID discovery, WPA2 authentication, three packets explicitly
bound to the Wi-Fi interface, disconnect, reconnect and another three successful
packets, plus clean shutdown. All three final-image results contain the SHA-256
above. The supplicant is active before teardown and running afterward. The
namespace teardown moves the AP radio back and triggers a service stop/start;
the captured post-teardown state reports success and zero failure restarts.

The intermediate `12691962…` candidate passed Wi-Fi testing before the explicit
microcode policy was added. It is retained under
`artifacts/native-rust-before-microcode/`, but is **not** the final image and is
not counted in the table above.

### Graphical installation and media deployment

Two representative final-image input regressions also passed:
[BIOS/PS2 Plasma](test-results/wifi-live-bios-ps2.json) and
[UEFI/USB Xfce recovery](test-results/wifi-recovery-input.json). They require
actual guest-window pointer/keyboard events, correct sessions and clean
shutdown. The earlier six-way matrix remains evidence for the preceding image,
not six additional tests of this build.
The final [BIOS](images/wifi-menu-bios.png) and
[UEFI](images/wifi-menu-uefi.png) menu screenshots were inspected; all three
kernel/session labels remain readable.

The supervised final-image [UEFI GUI installation passed](test-results/wifi-install-uefi.json).
It confirmed the unchanged
[enabled unfree default](images/wifi-unfree-default.png),
[live Chicago detection](images/wifi-timezone.png), and the
[reviewed Plasma installation with Wi-Fi transfer](images/wifi-review.png).
Only the disposable 40 GiB regular-file disk, serial `RESPIN_TEST_ONLY`, was
erased. Actual GUI submission launched the packaged privileged helper; no
fixture installation request bypassed the form.

The [completion screen](images/wifi-install-complete.png) and
[installed Plasma session](images/wifi-installed-plasma.png) were inspected.
The installed disk booted without the ISO, authenticated at the graphical login
and shut down cleanly. Live and installed kernels both report **7.2.8**.
Assertions cover:

- Enabled firmware/unfree settings and working supplicant D-Bus activation.
- Determinate services, unchanged system lock, expected desktop/session.
- Actual PAM rejection/acceptance, locked root, protected password storage,
  literal full-name escaping and absence of live-only settings.
- Synthetic Wi-Fi secret persistence, root-only mode 0600, remapped user
  restriction and successful NetworkManager loading; the live profile remained
  unchanged. No personal host credentials were used.
- Chicago, including winter CST and summer CDT.

### Samsung deployment

Read-only identification confirmed the unmounted Samsung Flash Drive, serial
ending **3525**, capacity **32,080,200,192 bytes**, at its exact by-id path.
The initial Polkit authorization attempt failed at **00:45:42 America/Chicago**.
The deployment process exited with status 143 before the writer started;
no USB bytes were written by that attempt. The user explicitly authorized a
retry, and the next single authorization succeeded.

The Samsung now contains the final `26444afb…` image, replacing `a06e8605…`.
The helper checked vendor/model/serial, capacity, removable USB transport,
mount state and holders before opening the exact device exclusively. The
internal NVMe was not a target.

All **3,989,078,016 bytes** were written and synchronized. The final 1 MiB of
the device was cleared of stale backup partition metadata; this was not a
secure erase of unused space. After invalidating the block cache, the helper
read back and compared every ISO byte and verified SHA-256
`26444afb6b01cc35b6e226de06308ba3301570888afe0ef4ab139d9b7b1f5ef5`.
The [read-back evidence](test-results/wifi-usb-readback.json) records the
matching size/hash and successful byte comparison. Kernel partition-table
refresh succeeded.

Both following tests booted the **physical Samsung device**, not the ISO file:

| Firmware | Result | Inspected screenshot |
| --- | --- | --- |
| OVMF UEFI | [PASS](test-results/wifi-usb-uefi.json) | [Plasma and Rust installer](images/wifi-usb-uefi.png) |
| SeaBIOS | [PASS](test-results/wifi-usb-bios.json) | [Plasma and Rust installer](images/wifi-usb-bios.png) |

Both reached a live tmpfs root with the expected ISO9660 and FAT `EFIBOOT`
partitions, active display manager, Plasma, the Rust installer and Determinate
Nix. Media settings reported `kernel: latest` and `test_diagnostics: false`.
Both guests shut down cleanly. These are default-profile physical-media boot
checks; the three-profile simulated-radio tests above used the byte-identical
ISO file.

The successful authorization covered writing, complete read-back and both
boots. Before starting QEMU, the helper permanently dropped root and
supplementary groups and enabled `no_new_privs`. QEMU ran as UID 1000 with only
a read-only media descriptor, no host system disk and no installation target.
QMP independently confirmed the full Samsung capacity and read-only status.
No persistent permission rules were changed.

Raw evidence is retained in `.work/usb-wifi-final/`. The Samsung was left
unmounted, all test VMs stopped, and the host was not rebooted.

## Remaining physical test

QEMU cannot establish Intel BE211 RF/firmware behavior or every desktop's secret
agent behavior. The final physical check is to boot **7.2.8 Plasma (default)**,
confirm nearby SSIDs appear, connect and browse, then disconnect/reconnect.
No host reboot or host network-configuration change is part of this verification.
