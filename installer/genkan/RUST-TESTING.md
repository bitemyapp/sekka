> **Historical pre-monorepo evidence.** These results apply to the named predecessor revisions, not a released Sekka image. See the [migration record](../../docs/migration.md) for the migration checks.

# Rust installer verification

These results concern the NixOS-focused Rust implementation, not upstream
Calamares's distribution/plugin feature set. The first-release scope and
limitations are in [RUST-INSTALLER.md](RUST-INSTALLER.md).

## Parsed-plan refactor

Local locked Cargo tests pass: **26 runtime tests** (25 library, one GUI) and
**six compile-fail doctests**. The no-default-features build passes 20 runtime
tests and the same six compile-fail examples. Rustfmt and Clippy with warnings
denied pass for both feature sets and the separate VM fixture.

The tests cover the raw → parsed → confirmed → IPC → independently parsed helper
transitions; retargeting rejection; all 64 desktop subsets against every default;
hostname, user, password, ordinary-field and tzdata boundaries; and settings
snapshot consistency. Compile-fail tests prohibit using raw input for rendering,
deserializing either plan type, mutating the disk through shared access, invoking
installation with an unconfirmed plan, and constructing unchecked Wi-Fi collections.
Wi-Fi tests retain normalized bytes through writes and the IPC round trip,
including permissions, secret flags and duplicate connection identity.
The 47 generated configuration cases are byte-identical to the preceding
installer (`680c510`): JSON SHA-256
`0ed8a33800600707143798e9a99006be55ef2b1bef30e2b483413cdbc096227b`.

The separate VM fixture additionally sends invalid usernames, empty desktop
selections and opt-out/profile contradictions to the packaged helper, checking
that none creates a partition table. Full-image installation,
desktop matrix, Wi-Fi persistence and CST/CDT results are tracked with the exact
installer commit and ISO checksum in the graphical integration repository's
[TESTING.md](https://github.com/bitemyapp/determinate-nixos-graphical/blob/codex/rust-calamares-integration/TESTING.md).
That report, not the older component results below, identifies the latest tested
image. A synthetic guest-only Wi-Fi profile tests persistence, not radio access.

The remainder of this document preserves the original pre-integration results;
its package hashes and test counts are historical, not current release claims.

## Build and local checks

- Locked Nix build on Nixpkgs `c59305bab2065cfecc4944690d9eedbb56f3a9fa`.
- 12 backend unit tests and one GUI selection test pass in the Nix sandbox.
- Rustfmt and Clippy with warnings denied, including the separate VM fixture.
- Pre-integration package: `/nix/store/091zgxadxyww1yvmzkj59611n7cwp0jn-calamares-nixos-rust-0.1.0`.

## Disposable-machine integration tests

The baseline graphical ISO SHA-256 was
`3dd85d328e420af4c88c016b45dc9c4f69460c82b7d628218371b17110676447`.
It was attached read-only as USB mass storage. Each run used a new 40 GiB
regular-file-backed virtual target with serial `RESPIN_TEST_ONLY`, followed by
an orderly shutdown and a boot with **no ISO attached**.

| Firmware | Successful run | Packaged backend |
| --- | --- | --- |
| UEFI | `uefi-1791074916109-48874` | `f3h45hixda4zx0q87814gfrxz69in2rw` |
| BIOS | `bios-1791075333637-50870` | `wcgx3laar7k88rw0nfh1rcqnxfa61v4p` |
| UEFI, final package | `uefi-1791075670909-51959` | `091zgxadxyww1yvmzkj59611n7cwp0jn` |

All three runs exercised the real Nix-packaged helper, wrong-erase-confirmation
rejection without partitioning, non-destructive preflight, whole-disk
installation, installed-system boot, and Determinate Nix 3.23.0 / Nix 2.35.2
and `fh` 0.1.27. Installed-system checks included desktop services, root account
locking, the normal user's password hash, actual PAM rejection/acceptance of
wrong/correct passwords through a pseudo-terminal, protected password-file
permissions outside the flake, and absence of live installer settings.
The full name `Rust ${literal} Test` remained literal, testing Nix escaping.

The real GTK GUI was launched unprivileged and screenshots inspected, including
the unselected-disk placeholder and visible review button. Installed SDDM login
screens were visually checked. These component runs invoke installation through
the helper protocol, not by clicking every GUI control or logging into Plasma.
The later graphical ISO integration must verify its own autostart and Polkit
wiring; package-only testing cannot establish those properties.

## Development failures retained locally

- The first integrated ISO exposed a trust-check mismatch: its settings link
  into Nix's root-owned, sticky, group-writable store root. The helper now
  accepts only that specific 1775 store-root case, still rejects writable
  entries below it, and checks link ownership as well as resolved ownership.
  An added regression test brings the native test total to 14. Final image
  verification is recorded in the graphical installer repository.

- A test fixture assumed `/etc/localtime` existed on the live ISO; it now uses
  trusted tzdata. Production media supplies an explicit immutable tzdata path.
- Holding the device flock while waiting for udev stalled partition events;
  the lock is now released/closed before settling, while the installer-wide
  lock stays held. A fresh packaged install verified the fix.
- Initial PAM automation did not wait for the password prompt; it now does,
  keeps the PTY input open, and tests a wrong password first.
- Early screenshots exposed an automatic disk selection and a clipped review
  button. A real placeholder and a fixed footer corrected these UI issues.

Failed/development runs were not relabeled as passes. Their logs and disks
remain in ignored local artifact directories. No host sudo, host block device,
physical USB write or host reboot was used. Secure Boot, physical hardware,
encrypted/manual partitioning and upstream plugin compatibility are untested
and are not claimed.
