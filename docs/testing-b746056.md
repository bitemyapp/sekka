# Historical desktop, Wi-Fi and time-zone verification (b746056)

This report concerns `codex/rust-calamares-integration`. Historical results for
the [first native integration](testing-3ccbea8.md) and
[rust-script migration](testing-a207aa5.md) remain separate.

## Exact image and checks

- Nixpkgs: `c59305bab2065cfecc4944690d9eedbb56f3a9fa`.
- Rust installer: `680c5109daf18ae41f8a95ca4ce0869388fe189b`.
- Source hash: `sha256-q/xl5bJ5xSiAlqvVeJDrspbORHvfvC1A79giVqRuUUM=`.
- Unchanged system lock SHA-256: `2a1e300d41e32889d2f108a9405294cf89f0a196d755bfc4bf43b8eea9852d69`.
- ISO (preserved locally): `artifacts/native-rust-680c510/nixos-graphical-determinate-rust-26.11.20261001.c59305b-x86_64-linux.iso`.
- Size: **3,813,998,592 bytes**.
- SHA-256: `972687f69e81cce4f2d0a3895744ee95fa3ab903e2e561ba756869c7b047e96a`.

The host independently hashed the image and matched the builder's sidecar.
This is a custom image, not a vendor-signed ISO; the checksum identifies the
tested bytes. Nix fetched the exact revision with its pinned content hash.
Normal package signature/hash verification remained enabled.

All three flake checks passed: native installer, template/lock, and LTS/latest
settings. **32 tests** passed locally and in the Nix sandbox: 21 native installer
tests and 11 orchestration tests. Rustfmt, Nixfmt, Clippy with warnings denied,
RustScript entry-point compilation and process-cleanup regression checks passed.

Native package: `/nix/store/i47gjhnjmh1d1q7ryc1gz2lpn5qw9vlq-calamares-nixos-rust-0.1.0`.
ISO output: `/nix/store/i8bmqx1kmj94936yf8qb26fggxk0xriq-nixos-graphical-determinate-rust-26.11.20261001.c59305b-x86_64-linux.iso`.

## Image-level QEMU installations

Both final-image installations passed on 2026-10-04 UTC using the exact checksum above.

| Firmware | Installation path | Result |
| --- | --- | --- |
| UEFI | Actual GTK GUI → Polkit → packaged Rust helper; Xfce only | [PASS, including graphical login](test-results/desktop-uefi.json); `uefi-1791080602818-79429` |
| BIOS | Packaged helper protocol; Plasma + Xfce, default Plasma | [PASS](test-results/desktop-bios.json); `bios-1791080603134-79459` |

Visual evidence: [desktop choices](images/desktop-selector.png),
[Chicago detection](images/desktop-timezone.png),
[required confirmation](images/desktop-zone-confirmation.png),
[conflict rejection](images/desktop-conflict.png),
[erase guard](images/desktop-erase-guard.png),
[completion](images/desktop-install-complete.png),
[logged-in Xfce](images/desktop-installed-xfce.png), and
[BIOS login screen](images/desktop-bios-login.png).

Tests attach the full ISO read-only as USB mass storage, through OVMF or SeaBIOS.
Each creates a fresh 40 GiB regular-file disk with serial `RESPIN_TEST_ONLY`,
installs, shuts down, and boots the installed disk with **no ISO attached**.
No host block device, host sudo or host reboot is involved.
During the supervised UEFI disk boot, a late QMP key canceled the boot-menu
countdown. Enter selected the normal NixOS entry; no rescue or installed-system
changes were needed. This was operator input, not an unattended-boot failure.
A second [disk-only reboot](test-results/desktop-uefi-reboot.json),
`boot-uefi-1791081046172-81585`, then passed with no boot-menu input, repeating
all installed checks and the Xfce graphical login. All final VMs shut down cleanly.

The pinned fixture is absent from normal media. It checks that production
diagnostics are disabled, then enables guest-agent/serial diagnostics only in
the guarded VM, without editing the store or ISO. GUI mode fills the real form
through QMP input; it does not submit a fixture installation request.

The supervised GUI checks verified readable tabs, Xfce-only selection and
default session, GNOME+Cinnamon rejection, required timezone confirmation,
a one-profile Wi-Fi review, and a disabled erase button for the wrong disk
phrase even with consent checked. The exact phrase plus consent enabled it.

Installed checks cover Determinate Nix 3.23.0 / Nix 2.35.2, `fh` 0.1.27,
unchanged lock, desktop/default session, password hash and real PAM rejection/
acceptance of wrong/correct passwords, locked root, no live user/installer,
and literal `Rust ${literal} Test` escaping. GUI mode additionally signs into
SDDM, checks the selected user's Xfce process and captures the desktop.
Backend mode tests wrong-confirmation rejection and read-only preflight.

## Desktop combinations

[Matrix evidence](test-results/desktop-matrix.json) evaluates full target
derivations for **all 47 supported nonempty combinations** of Plasma, GNOME,
Xfce, Cinnamon, MATE and LXQt, using the Rust backend's actual generated
configurations. It records available/default sessions and zone.
An explicit negative evaluation confirmed that GNOME+Cinnamon conflict on
`NIX_GSETTINGS_OVERRIDES_DIR` in the pinned NixOS modules.

The original matrix was generated at `393bfad55aba29dc7cd8e007576c921abdd0afca`.
The final installer produces byte-identical configuration JSON. The builder
reused the results only after all three fingerprints matched:

- Generated configurations: `0ed8a33800600707143798e9a99006be55ef2b1bef30e2b483413cdbc096227b`.
- System lock: the unchanged hash above.
- Evaluator: `b4af93fcbe3b5a825bb399c363d00c261b8415d4bdcf1b00c45ae610e2c01808`.

Evaluation is not installation/login testing for every desktop; this leg's
end-to-end cases are Xfce-only and Plasma+Xfce.

## Wi-Fi and US Central checks

Only a public synthetic Wi-Fi profile is seeded **inside the guest**, with a
saved WPA-PSK and restriction to the live `nixos` user. No host Wi-Fi data or
real credential is read. GUI review uses the real unprivileged NetworkManager
D-Bus snapshot and reports one saved profile. Installed checks require the
password to survive, restriction to become `rusttest`, root-owned mode 0600,
and successful NetworkManager loading. The live profile must remain unchanged.
Credentials must not appear in the generated Nix configuration.

QEMU has no wireless radio: this verifies persistence/loading, **not RF
connectivity or every Secret Agent implementation**. Unit tests cover missing
secrets, duplicate profiles, and unsupported external certificate/key references.
Enterprise certificate-file/token profiles require manual post-install setup.

The fixture sets the live zone to `America/Chicago`. GUI detection read that
zone and explained its live-system source. With no regional zone and unavailable
internet detection, the GUI left the field blank instead of choosing Eastern;
confirmation was required before review. Installed checks require Chicago,
`-0600 CST` on January 15 and `-0500 CDT` on July 15.
Internet detection remains approximate public-IP geolocation, not a guarantee
of physical location; users can override it and must review the result.

## Limits and development evidence

Only the default LTS kernel is installed end to end. The latest kernel's menu
and configuration are evaluated, not claimed as a completed installation.
Secure Boot and physical hardware compatibility are not established by VMs.
Whole-disk GPT/ext4 and network access are required. Manual partitioning,
dual boot, encryption, RAID/LVM, Btrfs, offline installation, translated UI and
upstream Calamares plugin parity are not implemented.

The preceding image (`09b97af45fe514acd05d724321824d424c0811321a25c8d7840657c82430730a`)
is retained in `artifacts/native-rust-6f43ed1/`. Its BIOS install passed.
Its UEFI install reached Xfce but the harness wrongly required a full `/bin/`
process path; that failed result is preserved. The corrected check passed a
separate disk-only reboot (`boot-uefi-1791080355818-77924`). An earlier resume
attempt exceeded the Unix socket path limit; run names are now short, with
lineage stored in JSON. Fresh final-image runs are separate evidence.
The final native source also fixes clipped notebook tab labels.

An interrupted development builder left 18 unreferenced cache entries damaged.
Nix's ordinary liveness-checked deletion removed them; a subsequent full
store-content audit completed cleanly. The builder now flushes and powers down
even on failure/cancellation, using a bounded cleanup grace. Audit, repair,
expected source-hash bootstrap mismatches and failed-run logs remain locally.
Prior artifacts are not relabeled as passes.

The physical Samsung USB stick is unchanged by this leg.
