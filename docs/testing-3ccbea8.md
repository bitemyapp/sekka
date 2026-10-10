# Historical native Rust installer verification (3ccbea8)

This report preserves the first native integration at commit `3ccbea8`.
It is not evidence for the current desktop/Wi-Fi/time-zone image; see
[the current report](../TESTING.md).
Historical results for the completed rust-script migration are preserved in
[docs/testing-a207aa5.md](testing-a207aa5.md); they are not evidence for this ISO.

## Build and integrity

- Nixpkgs: `c59305bab2065cfecc4944690d9eedbb56f3a9fa`.
- Rust installer: `03fecd58c7bca8eb2df6774ec08962c6a7b89e74`.
- Installer source content hash: `sha256-6hR1djn5pVfd895Btmqi560GZvnua1LUuzremY0im0w=`.
- Unchanged system lock SHA-256: `2a1e300d41e32889d2f108a9405294cf89f0a196d755bfc4bf43b8eea9852d69`.
- ISO: `artifacts/native-rust-03fecd5/nixos-graphical-determinate-rust-26.11.20261001.c59305b-x86_64-linux.iso`.
- Size: **3,813,998,592 bytes**.
- SHA-256: `18a236b2593a75e6954430776f2bfdc0872c35ac3891e421a4107c109a0f135e`.

The host independently hashed the exported ISO. The rootless builder exports
atomically and generates a checksum sidecar. This is a custom-built image,
not a vendor-signed ISO; the checksum identifies these exact tested bytes.
Nix fetched the fork at the exact commit with the recorded content hash.
Normal Nix package verification was not disabled for the ISO build.

All three flake checks passed: native installer package, pinned template/lock,
and LTS/latest JSON settings. The last check also rejects legacy installer
packages/tools and old INI/handoff settings. The installer package passed
14 unit tests after the store-permissions fix; the orchestration crate passed 10 tests both locally and in the
Nix sandbox. Rustfmt, Nixfmt, Clippy with warnings denied, RustScript entry-point
compilation and process-cleanup regression checks passed.

## Image-level QEMU tests

Both final-image runs passed on 2026-10-04 UTC using the exact ISO hash above:

| Firmware | Installation path | Result and evidence |
| --- | --- | --- |
| UEFI | Actual GTK GUI → Polkit → Rust helper | [PASS, including graphical Plasma login](test-results/native-uefi.json), run `uefi-1791077035283-60781` |
| BIOS | Packaged Rust helper protocol | [PASS, including PAM authentication and visible SDDM login](test-results/native-bios.json), run `bios-1791077091994-60920` |

The UEFI GUI run verified the actual
review screen, disabled install button for a wrong erase phrase, enabled button
for the exact phrase plus consent, GUI-to-Polkit helper launch, visible progress,
and refusal to close normally while installation is active. Its completion
screen was visually checked, then the installed disk booted without the ISO.
QMP keyboard input signed into SDDM with the test user's password; the test
confirmed that user's Plasma shell and captured the logged-in desktop.

Both installed systems passed password-hash verification and actual PAM
rejection/acceptance of wrong/correct passwords, with root locked and the live
user/installer absent. Determinate Nix 3.23.0 / Nix 2.35.2, `fh` 0.1.27, the
desktop service and the unchanged flake lock were verified. Password-file
permissions and literal `Rust ${literal} Test` configuration escaping passed.
The BIOS backend run also verified bad-confirmation rejection and preflight
without disk writes. Both VMs shut down successfully after verification.

Visual evidence: [erase guard](images/native-erase-guard.png),
[installation complete](images/native-install-complete.png),
[logged-in Plasma](images/native-installed-desktop.png), and
[BIOS login screen](images/native-bios-login.png).

The tests attach the full ISO read-only as USB mass storage and boot through
OVMF or SeaBIOS. They create a fresh 40 GiB regular-file-backed disk with serial
`RESPIN_TEST_ONLY`, then shut down and boot it with the ISO physically absent
from the VM device configuration. No host block device, sudo or reboot is used.

Backend mode exercises bad-confirmation rejection, read-only preflight and the
normal packaged helper protocol. Supervised GUI mode uses QMP keyboard/mouse
input to fill the real form and invokes the helper through the actual GUI and
Polkit wrapper; it does not inject a fixture installation request. It also
attempts a graphical login after reboot, in addition to the backend/PAM checks.

The shared, pinned test fixture is absent from normal media. Before enabling
VM-only guest-agent/serial diagnostics, it validates the real media settings
and confirms diagnostics were disabled. It replaces the live `/etc` settings
link, never writes to the immutable Nix store, and never modifies the ISO.

## Limits and provenance

Only the default LTS kernel installation path was exercised end to end.
The latest-kernel menu/configuration is evaluated and checked, not claimed as
a complete tested installation. Secure Boot is not enabled.

The installer supports whole-disk GPT/ext4 with Plasma, BIOS/UEFI and network
access. No manual partitioning, dual boot, encryption, RAID/LVM, Btrfs, offline
installation, other desktop, translated interface or upstream plugin parity
is implemented or claimed. VM results do not guarantee physical hardware
compatibility.

The standalone fork had already passed packaged UEFI and BIOS installations
before integration; its detailed evidence is in
[RUST-TESTING.md](https://github.com/bitemyapp/calamares/blob/f09bcc246d7245bdceb09a0fcc5b27d340d877ad/RUST-TESTING.md).
The integration initially caught and corrected a misnamed NixOS Polkit option.
The first integrated UEFI (`uefi-1791076495267-57443`) and BIOS
(`bios-1791076545709-58030`) runs booted the native desktop but stopped before
any erase: the configuration guard rejected `/nix/store` mode `1775`. The fix
allows only that root-owned sticky store-root case, still rejects writable
entries below it, and checks link ownership as well as resolved ownership.
The first ISO (SHA-256 `21f6eda6b889ec32bb225cd9fa7d0c596ccbb6444be4b21bf7a6cbdaca2d5c52`)
is retained under `artifacts/native-rust-initial/`. A later build was interrupted
and restarted from the preserved builder store; its log is retained as well.
The source-fetch bootstrap deliberately used a placeholder hash to obtain and
then pin the real content hash; subsequent checks used the real hash.

Prior ISO images and failed/development run artifacts are retained locally,
not relabeled as passes. The physical Samsung USB stick is unchanged by this
leg of work.
