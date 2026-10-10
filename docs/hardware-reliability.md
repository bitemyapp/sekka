# Live graphics/input reliability candidate

**Historical image:** the user subsequently confirmed that the 7.2.8 Plasma
entry resolved the reported display/input symptoms, but found no Wi-Fi SSIDs.
The current correction, firmware/unfree policy and new verification are in
[Wi-Fi backend and hardware-friendly defaults](wifi-hardware-defaults.md).
The results below remain evidence for the preceding image only.

2026-10-03, America/Chicago. Branch: `codex/live-hardware-reliability`.

## Reported failure and evidence

The preceding image (`f8c38600d0fc27de6cbfdddac191519f991a912c1f3285c2b4968d9057c41d5c`)
showed a black internal display with a pointer. Connecting an external display
revealed Plasma and the installer, but pointer/keyboard input appeared frozen.
Long boot labels hid the distinguishing kernel versions; the selected entry
therefore cannot be established. No journal from that live boot was retained.

Read-only inspection of the working CachyOS installation found a ThinkPad X1
Carbon Gen 14, Intel Panther Lake graphics (`8086:b090`, `xe`), Intel LPSS I2C,
an ELAN I2C HID touchpad, and i8042 keyboard/TrackPoint devices. CachyOS is running
kernel `7.2.8-2-cachyos`, Plasma Wayland and Plasma Login Manager. The failed
image defaulted to `6.18.54` and offered `7.2.8` as an alternate entry.

The older default kernel is a plausible compatibility factor, **not a proven
root cause**. The working OS also uses Wayland and Plasma Login Manager, so
neither is blamed merely by association. QEMU does not emulate this Intel
display engine, firmware or I2C touchpad.

The candidate's `7.2.8` module aliases match GPU `8086:b090` to `xe` and
controllers `8086:e450/e451` to `intel_lpss_pci`. The module closure includes
`i2c-hid-acpi`, `hid-multitouch` and `psmouse`; the firmware closure includes
`xe/ptl_guc_70.bin`, `xe/ptl_huc.bin`, `xe/ptl_gsc_1.bin` and
`i915/xe3lpd_dmc.bin` (compressed). Presence is checked, not physical operation.

## Changes

- Default to `7.2.8` from the **unchanged pinned Nixpkgs**, with a named `6.18.54`
  LTS fallback. The native installer preserves that kernel choice in the target.
- Put kernel/session distinctions first in each boot label; assert that full
  labels fit within 64 characters. Upstream diagnostic variants remain grouped
  in the separately named Options submenus.
- Add a latest-kernel Xfce/X11 recovery session using LightDM, software GTK/GL
  rendering, unaccelerated Xorg and no Xfce compositor. This does not constrain
  the installed desktop choices or copy these live-only workarounds to the OS.
- Explicitly enable libinput and show boot messages instead of Plymouth.
  Do not impose `nomodeset` on normal/recovery boots: native kernel modesetting
  remains available for the internal panel.
- Add a bounded, read-only `installer-live-diagnostics` command. It reports
  kernel, DRM connectors, device/seat metadata and display-manager/kernel logs.
  It does not capture raw input events, read saved Wi-Fi profiles or upload data.
- Add firmware-menu and live-input regression tests using actual guest-window
  mouse/keyboard events, not merely QMP acknowledgements or process existence.
  Installation tests now also compare the installed running kernel with the
  selected live kernel.

The NixOS modules used for these profiles are the pinned upstream
[graphical base](https://github.com/NixOS/nixpkgs/blob/c59305bab2065cfecc4944690d9eedbb56f3a9fa/nixos/modules/installer/cd-dvd/installation-cd-graphical-base.nix),
[Xfce module](https://github.com/NixOS/nixpkgs/blob/c59305bab2065cfecc4944690d9eedbb56f3a9fa/nixos/modules/services/x11/desktop-managers/xfce.nix),
and [ISO menu generator](https://github.com/NixOS/nixpkgs/blob/c59305bab2065cfecc4944690d9eedbb56f3a9fa/nixos/modules/installer/cd-dvd/iso-image.nix).

## Verification status

Final candidate:

- Retained image: `artifacts/native-rust-before-wifi/nixos-graphical-determinate-rust-26.11.20261001.c59305b-x86_64-linux.iso`.
- Size: **3,927,244,800 bytes**.
- SHA-256: `a06e860511db656850c7dcc4b91b348dcaebdfa2f321a7105ec7bd3df59e1978`.
- Nix output: `/nix/store/3ncxgiaalbqyhhjz8650fzhni8qkvw9p-nixos-graphical-determinate-rust-26.11.20261001.c59305b-x86_64-linux.iso`.
- Unchanged system lock SHA-256: `2a1e300d41e32889d2f108a9405294cf89f0a196d755bfc4bf43b8eea9852d69`.
- Unchanged native installer: `d2fd201d94327a4aae0d91bb40de1963509f0573`.

All four flake checks passed, including the new profile/menu assertions and the
then-13 orchestration tests in the Nix sandbox. The final test-runner focus guard
adds a fourteenth unit test; all **14** passed locally, along with Rustfmt,
Clippy with warnings denied, and the process-cleanup tests. The unchanged
native package and 47-desktop evaluation matrix were reused, not represented as
new native unit tests or 47 new installation runs.

The [800×600 BIOS menu](images/hardware-menu-bios.png) and
[UEFI menu](images/hardware-menu-uefi.png) were inspected: all three complete
main labels are readable, with kernel/session at the left edge.

During harness development, failed attempts exposed monitor-socket contention,
Unix socket path length, early firmware keystrokes and executable-name assumptions.
Two final-image attempts sent their marker to the installer, which had appeared
over the probe; the saved screenshots show `input42` in the actual GUI. Those
attempts were correctly rejected by the probe assertion. The runner now waits
for the loader, accepts bare or wrapped executable names, and checks actual
probe focus before sending test input. Failed/interrupted attempts remain under
`artifacts/live-*`; they are not counted as successful matrix entries.

### Final-image live-boot matrix

| Profile | OVMF UEFI, USB-only input | SeaBIOS, PS/2-only input |
| --- | --- | --- |
| 7.2.8 Plasma default | [PASS](test-results/hardware-default-uefi-usb.json) | [PASS](test-results/hardware-default-bios-ps2.json) |
| 7.2.8 Xfce/X11 software | [PASS](test-results/hardware-recovery-uefi-usb.json) | [PASS](test-results/hardware-recovery-bios-ps2.json) |
| 6.18.54 Plasma LTS | [PASS](test-results/hardware-lts-uefi-usb.json) | [PASS](test-results/hardware-lts-bios-ps2.json) |

Each boot used the full final ISO as read-only USB mass storage, with **no target
disk** attached. Assertions require the requested kernel policy, correct active
seat/session, running installer, actual window-received click and ordered typed
marker, diagnostics, and clean shutdown. USB tests disable the emulated i8042
controller; PS/2 tests omit USB input devices and disable the VMware pointer shim.
The recovery tests also check the installer's software-rendering environment and
the running Xfce compositor setting. Examples of received events are retained for
[Plasma](test-results/hardware-default-input.log) and
[Xfce](test-results/hardware-recovery-input.log), with trailing whitespace
normalized in the published copies; original logs remain in `artifacts/`.

The [Plasma](images/hardware-plasma.png) and
[Xfce recovery](images/hardware-xfce.png) desktop screenshots were inspected.
This tests virtual graphics/input delivery, not actual Wi-Fi radio connectivity
or a full graphical installation under every firmware/desktop combination.

The first final-image installation attempt was
[interrupted during the target build](test-results/hardware-install-interrupted.json).
The signal's source was not established; that attempt is incomplete, not a pass.

The fresh-disk [UEFI retry passed](test-results/hardware-install-uefi.json): the
packaged Rust helper installed Plasma+Xfce, shut down, and booted the target with
no ISO attached. Both live and installed kernels are **7.2.8**. Checks passed for
Determinate services, unchanged lock, real PAM rejection/acceptance, locked root,
root-only password storage, absent live-only settings, synthetic Wi-Fi secret
and user-restriction migration, NetworkManager profile loading, Chicago with
winter CST/summer CDT, and clean shutdown. This was a helper-driven installation,
not another full GUI form/login test. It used only a fresh 40 GiB regular-file
disk with the guarded `RESPIN_TEST_ONLY` serial. All test VMs have stopped.

### Samsung USB deployment

The final candidate above has now replaced the preceding image on the Samsung
Flash Drive, serial ending **3525**, capacity **32,080,200,192 bytes**. The
deployment helper checked the complete by-id identity, vendor/model/serial,
capacity, removable USB transport, mount state and holders before exclusively
opening the drive. The internal NVMe was not a target.

All **3,927,244,800 image bytes** were written. The final 1 MiB of the device was
cleared of stale backup partition metadata; this is not a secure erase of unused
space. After synchronizing writes and invalidating the block cache, every image
byte was read back and compared with the source. The [read-back evidence](test-results/hardware-usb-readback.json)
records a matching SHA-256 of
`a06e860511db656850c7dcc4b91b348dcaebdfa2f321a7105ec7bd3df59e1978`.
Kernel partition-table refresh succeeded. The media has the expected ISO9660
`nixos-determinate-x86_64` and FAT `EFIBOOT` partitions.

Both following tests booted the **physical Samsung device**, not the ISO file:

| Firmware | Result | Inspected screenshot |
| --- | --- | --- |
| OVMF UEFI | [PASS](test-results/hardware-usb-uefi.json) | [Plasma and Rust installer](images/hardware-usb-uefi.png) |
| SeaBIOS | [PASS](test-results/hardware-usb-bios.json) | [Plasma and Rust installer](images/hardware-usb-bios.png) |

A single authorization covered writing, read-back and both boots. Before starting
QEMU, the helper permanently dropped root and supplementary groups and enabled
`no_new_privs`. QEMU ran as UID 1000 with only a read-only media descriptor, no
host system disk and no installation target disk. No persistent permission rules
were changed. QMP independently confirmed the full USB capacity and read-only
status in both runs.

Both guests reached a live tmpfs root with ISO9660 media, active display manager,
Plasma and Rust installer processes, and the Determinate daemon socket. They
reported Determinate Nix 3.23.0 / Nix 2.35.2, with `kernel: latest` and
`test_diagnostics: false` in the media settings. Both powered off cleanly. These
are default-profile boot checks; the six-profile/input matrix above separately
tested the byte-identical ISO file. Raw deployment evidence remains locally in
`.work/usb-hw-final/`.

The Samsung was left unmounted and all test VMs stopped. The host was not rebooted.
Physical laptop compatibility remains **unverified** until the user boots this
candidate. Select **7.2.8 Plasma (default)** first; **7.2.8 Xfce/X11 (software)**
is the recovery option if the normal session still has problems.

For a failed GUI with a working `Ctrl+Alt+F3` console:

```sh
sudo installer-live-diagnostics > /tmp/installer-hardware-report.txt
```

Save the report before rebooting. Review it before sharing: it can contain
hardware identifiers. If even the console cannot accept input, report which
short boot label was selected and whether both built-in and external USB input
devices fail; do not infer the kernel from a clipped label.
