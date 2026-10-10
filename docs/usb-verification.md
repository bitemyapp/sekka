# Fresh rebuild and Samsung USB verification

2026-10-04 UTC (2026-10-03 in America/Chicago).

**Historical deployment:** these exact bytes later failed the user's physical
ThinkPad live-session test. The checksum/read-back and QEMU results remain valid,
but did not prove that laptop's display/input compatibility. See the
[corrective candidate report](hardware-reliability.md).

## Disk cleanup

Removed only individually inspected, obsolete generated artifacts: 24 disposable
VM target disks and eight old or duplicate ISO files across this repository and
the Rust installer checkout. Deletions are permanent, but these build/test
outputs can be regenerated. Sources, Git history, checksums, logs, screenshots,
the reusable builder disk, and the latest successful installed BIOS/UEFI test
disks were retained. No general home-directory or Nix-store cleanup was used.

Net disk use fell from approximately 420 GiB to 196 GiB after the new export:
about **224 GiB reclaimed**, with **246 GiB available** (45% used).

## Fresh, reproducible image

The rootless builder now accepts `--rebuild-iso`. This passes `--rebuild` to the
ISO's Nix build, rebuilding and comparing that output while allowing its
dependencies to remain cached. This run completed fresh SquashFS and ISO
assembly; it was not merely an export of a previously cached ISO.

```sh
rust-script --force scripts/build_rootless.rs \
  /path/to/nixos-with-determinate.iso --rebuild-iso
```

The bootstrap image was checked against its pinned SHA-256. All three flake
checks passed; the tools package rebuilt and ran its 11 tests in the Nix sandbox.
The native installer check reused its previously tested package. The 47-desktop
matrix reused existing evaluations only after the configuration, evaluator and
lock fingerprints matched. Rustfmt, all 11 local tools tests and Clippy with
warnings denied also passed.

- Installer revision: `d2fd201d94327a4aae0d91bb40de1963509f0573`.
- Nixpkgs revision: `c59305bab2065cfecc4944690d9eedbb56f3a9fa`.
- Unchanged lock SHA-256: `2a1e300d41e32889d2f108a9405294cf89f0a196d755bfc4bf43b8eea9852d69`.
- Retained historical image: `artifacts/native-rust-before-hardware-fix/nixos-graphical-determinate-rust-26.11.20261001.c59305b-x86_64-linux.iso`.
- Size: **3,813,998,592 bytes**.
- SHA-256: **`f8c38600d0fc27de6cbfdddac191519f991a912c1f3285c2b4968d9057c41d5c`**.

Nix's rebuild comparison succeeded. The host independently checked the exported
sidecar and compared the fresh image with the preceding image byte-for-byte
before removing that redundant copy. This checksum identifies our custom build;
it is not a vendor signature. The fresh build log is retained locally at
`artifacts/native-rust-before-hardware-fix/build.log`.

## Physical write and read-back

The destination was the removable Samsung Flash Drive, serial ending **3525**,
capacity **32,080,200,192 bytes**. The deployment helper pinned the complete
by-id identity and checked vendor, model, serial, USB transport, removable flag,
capacity, mount state and device holders before opening it exclusively. The
internal NVMe disk was not a target.

The full image was written, the final 1 MiB cleared of stale backup partition
metadata, writes synchronized and the block cache invalidated. Every image byte
was then read back and compared against the source; the read-back SHA-256 also
matched. This replaced the old installer but is **not** a secure erase of the
drive's unused space. Kernel partition-table refresh succeeded. A second,
read-only pass independently repeated the complete byte comparison and hash.

The resulting media contains an ISO9660 partition labeled
`nixos-determinate-x86_64` and a FAT EFI partition labeled `EFIBOOT`.
Raw local evidence is retained under `.work/usb-samsung-final/` and
`.work/usb-samsung-boot/`.

## Direct boot verification

The first direct USB attempt stopped before boot: QEMU's default regular-file
driver rejected the inherited block-device descriptor. That harness failure is
preserved in `.work/usb-samsung-final/uefi/qemu.log`; it is not a successful test.
The corrected harness explicitly selects `host_device` for block devices and
`file` for regular-file preflights. No second flash was needed.

The helper opens only the pinned USB, then permanently drops root and
supplementary groups and enables `no_new_privs` before starting QEMU. QEMU runs
as the ordinary user and inherits only a read-only media descriptor. No device
permissions or persistent privilege rules are changed, and no host system disk
is attached. OVMF variable storage is a disposable regular file.

Both runs booted directly from the physical drive, not the exported ISO file:

| Firmware | Result | Inspected screenshot |
| --- | --- | --- |
| OVMF UEFI | [PASS: live checks and clean shutdown](test-results/usb-samsung-uefi.json) | [Plasma and Rust installer](images/usb-samsung-uefi.png) |
| SeaBIOS | [PASS: live checks and clean shutdown](test-results/usb-samsung-bios.json) | [Plasma and Rust installer](images/usb-samsung-bios.png) |

Each run verified a live tmpfs root, mounted ISO9660 media, active display
manager, the live user's Plasma and Rust installer processes, and the active
Determinate daemon socket. Nix reported **Determinate Nix 3.23.0 / Nix 2.35.2**.
The installed media's settings had `test_diagnostics: false`. Guest block-device
output showed the full 32,080,200,192-byte USB and both expected partition labels;
QMP independently confirmed its full capacity and read-only status. Both guests
powered off successfully, and no QEMU processes remained afterward.

The USB was left unmounted. A final host-side ISO checksum check passed, and
the repository lock remained unchanged. No host reboot or access to host Wi-Fi
credentials was needed.

## Limits

This verifies the actual flash drive's image bytes and their bootability under
virtual UEFI and BIOS hardware. It does not prove compatibility with every
physical firmware or Secure Boot. These were live-media boot tests, not another
installation run: the byte-identical image's prior end-to-end installation,
disk-only boot, Wi-Fi persistence and desktop checks are documented in
[TESTING.md](../TESTING.md).
