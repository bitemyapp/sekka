> **Historical pre-monorepo evidence.** These results apply to the named predecessor revisions, not a released Sekka image. See the [migration record](docs/migration.md) for the migration checks.

# Parsed-plan installer verification

**Current work (fast installation, RAM-sized swap with zswap, CachyOS-inspired
tuning, Hyprland and Tatami, libadwaita GUI):** results are in
[fast installation](docs/fast-install.md), [Hyprland](docs/hyprland.md) and
[tuning](docs/tuning.md). The sections below describe earlier images.

**Current application work:** [optional applications](docs/applications.md)
describes the catalog, source pins, pre-erasure preparation and the additional
configuration and installed-system tests. Application-enabled ISO results are
recorded separately from the earlier filesystem-only image below.

**Current storage work:** the user reported a root mount failure on a ThinkPad
X1 Carbon Gen 14 NVMe. See [storage reliability](docs/storage-reliability.md)
for the fix, filesystem choices, tests, and outstanding final-ISO/hardware checks.
The earlier [Wi-Fi and hardware-defaults report](docs/wifi-hardware-defaults.md)
records the preceding image; its passes did not cover used NVMe installation.

**Historical image:** the physical ThinkPad boot subsequently exposed a blank
internal display and unresponsive input, despite the virtual tests below.
The corrective candidate and stronger live-input tests are recorded in
[docs/hardware-reliability.md](docs/hardware-reliability.md). This report is
retained as evidence for the older image, not proof of physical compatibility.

This report concerns `codex/rust-calamares-integration` and the "parse, don't
validate" refactor. The preceding report is preserved in
[docs/testing-b746056.md](docs/testing-b746056.md) with its original evidence.
The subsequent fresh rebuild, disk cleanup and physical USB deployment are
recorded separately in [docs/usb-verification.md](docs/usb-verification.md).

## Exact image

- Nixpkgs: `c59305bab2065cfecc4944690d9eedbb56f3a9fa`.
- Rust installer: `d2fd201d94327a4aae0d91bb40de1963509f0573`.
- Source hash: `sha256-2X6MjHHNFXnHt4NoVU6CvqdXPvlcOchyc/Pk9bQeoxA=`.
- Unchanged lock SHA-256: `2a1e300d41e32889d2f108a9405294cf89f0a196d755bfc4bf43b8eea9852d69`.
- Retained historical ISO: `artifacts/native-rust-before-hardware-fix/nixos-graphical-determinate-rust-26.11.20261001.c59305b-x86_64-linux.iso`.
- Size: **3,813,998,592 bytes**.
- SHA-256: `f8c38600d0fc27de6cbfdddac191519f991a912c1f3285c2b4968d9057c41d5c`.
- Native package: `/nix/store/fj3sxzinw8q5kriw4rfq87gs32j9agfj-calamares-nixos-rust-0.1.0`.
- ISO output: `/nix/store/m75kq0zk2bl33bv6mnx71s1cdvwwrwmw-nixos-graphical-determinate-rust-26.11.20261001.c59305b-x86_64-linux.iso`.

The host independently hashed the image and matched the builder's sidecar.
This identifies custom tested bytes, not a vendor signature. Nix verified the
source hash and retained normal package verification. The expected source-hash
bootstrap mismatch is preserved in `artifacts/native-rust-pre-usb/source-hash-bootstrap-d2fd201.log`.
Older local images and obsolete disposable disks were removed during the
2026-10-04 cleanup; historical logs, screenshots and published evidence remain.

## Parsing contracts and build checks

Raw JSON/form input is consumed into an immutable `InstallPlan`; configuration
generation accepts only that type. Installation requires `ConfirmedInstall`,
produced by a separate exact erase-phrase transition. The helper independently
parses again at the privilege boundary. Desktop/default-session invariants and
normalized Wi-Fi bytes are retained rather than checked and discarded.
Names and zones have checked constructors. Ordinary values remain simple.
Live/root, firmware, disk identity and mount-state checks still run at the
point of use. Filesystem and libnm parsing remain off the GTK thread.

Verification completed:

- **26 native runtime tests and six compile-fail doctests**, locally and in the
  fresh Nix sandbox build. Compile-fail examples cover raw rendering, direct
  plan deserialization, disk mutation, unconfirmed execution and unchecked
  Wi-Fi collection construction.
- No-default-features build: 20 runtime tests and the same six doctests.
- All 64 desktop subsets against each of six defaults; name/password/ordinary
  field boundaries, TZif/path containment, IPC round trips, settings snapshots,
  and normalized Wi-Fi persistence and permissions.
- Rustfmt and Clippy with warnings denied, including both feature sets and
  the separate VM fixture. The 11 orchestration tests passed locally; the
  unchanged Nix tools package also built successfully from cache.
- All three flake checks: native installer, template/lock and kernel settings.

The 47 supported desktop configurations remain byte-identical to the preceding
installer. The builder reused the [matrix](docs/test-results/desktop-matrix.json)
only after all fingerprints matched:

- Generated JSON: `0ed8a33800600707143798e9a99006be55ef2b1bef30e2b483413cdbc096227b`.
- Evaluator: `b4af93fcbe3b5a825bb399c363d00c261b8415d4bdcf1b00c45ae610e2c01808`.
- System lock: the unchanged hash above.

These are reused evaluation results, not 47 new installation/login tests.

## QEMU results — 2026-10-04 UTC

The ISO boots as read-only USB mass storage through OVMF or SeaBIOS. Fresh
targets are 40 GiB regular files, serial `RESPIN_TEST_ONLY`. Tests install,
power down and boot the installed disk without the ISO. Guest-only diagnostics
and public synthetic Wi-Fi credentials are absent from normal media.

| Run | Path and result |
| --- | --- |
| `uefi-1791082926111-92924` | [PASS](docs/test-results/parsed-plan-uefi.json): fresh GUI → Polkit → helper install, disk-only boot, Xfce login and clean shutdown. |
| `bios-1791082634701-91052` | Plasma+Xfce helper install and installed checks completed; interrupted during shutdown. [Original incomplete result](docs/test-results/parsed-plan-bios-interrupted.json). |
| `boot-bios-1791082923700-92938` | [PASS](docs/test-results/parsed-plan-bios-reboot.json): disk-only retest, repeated installed checks and clean shutdown. |

The BIOS [helper log](docs/test-results/parsed-plan-bios-install.log) records
rejection of a reserved username, empty desktop selection, opt-out with supplied
Wi-Fi profiles, and wrong erase phrase. After each request, the fixture required
the disk to remain uninitialized. Read-only preflight passed before installation.

The first GUI run verified required zone confirmation,
[live Chicago detection](docs/images/parsed-plan-timezone.png),
[GNOME+Cinnamon rejection](docs/images/parsed-plan-conflict.png),
an Xfce-only review with one saved Wi-Fi profile, and a
[disabled erase button for the wrong disk phrase](docs/images/parsed-plan-erase-guard.png).
The [exact phrase plus consent](docs/images/parsed-plan-review.png) enabled it
and launched the packaged helper through Polkit.

Both initial runners received an interrupt almost simultaneously. Its source
was not established. BIOS assertions had passed but shutdown was unfinished;
the [first UEFI run](docs/test-results/parsed-plan-uefi-interrupted.json) was still
installing. Neither is relabeled a pass. UEFI was retried on a fresh disk;
the BIOS disk-only retest preserves the original run as lineage.

The fresh UEFI run completed with no boot-menu intervention. Its
[completion screen](docs/images/parsed-plan-complete.png) and
[logged-in Xfce desktop](docs/images/parsed-plan-installed-xfce.png) were inspected.
The fixture authenticated at SDDM and checked the installed user's Xfce process,
not just the presence of a login screen. All final VMs have stopped.

Installed assertions cover Determinate Nix 3.23.0 / Nix 2.35.2, `fh` 0.1.27,
unchanged lock, selected/default desktop, actual PAM rejection/acceptance of
wrong/correct passwords, locked root, no live user/installer, literal full-name
escaping and private password hashes outside the flake. Wi-Fi checks require
the synthetic PSK to survive, live-user restriction to become `rusttest`,
root ownership/mode 0600, successful NetworkManager loading and an unchanged
live profile. Time-zone checks require Chicago, winter CST and summer CDT.

## Limits

QEMU tests profile persistence/loading, not radio connectivity or every desktop's
Secret Agent implementation. Enterprise certificate-file/token profiles still
require manual setup. Public-IP time-zone detection is advisory. Physical
hardware, Secure Boot and every desktop login are not covered. Whole-disk
GPT/ext4 and network access remain required. Manual partitioning, dual boot,
encryption, offline installation and upstream plugin parity remain unsupported.
Only the LTS kernel is installed end to end.

The parsed-plan tests above used no host Wi-Fi credentials, host sudo, physical
USB writes or host reboot. The subsequent USB deployment is documented separately.
