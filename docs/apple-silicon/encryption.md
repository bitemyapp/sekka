# Encrypting Sekka on a Mac: design and decision

Status: decided on 6 November 2026. **The first Mac release does not offer
encryption.** Sekka's root partition is not encrypted, and Sekka Installer
says so on its review screen. Nothing in the app, the helper, the engine or
first-boot setup offers a toggle that does nothing. This document is phase 8
of the [implementation plan](../apple-silicon-installer-plan.md): the design
comparison, why it is deferred, and what has to be true before it is offered.
Gate M7 of the qualification ledger is `not-applicable`, citing this file,
until then.

## What encryption would and wouldn't protect

Linux disk encryption would protect Sekka's files on the root partition
against someone with the Mac's disk and not the passphrase. It would not
change Apple's boot policy for the Sekka volume (the Recovery step sets that
to permissive so m1n1 can start), would not cover macOS (FileVault does
that, independently), and would not cover the ESP, which must stay readable
before unlock: m1n1's `boot.bin`, U-Boot, the kernels and initrds, and Apple's
firmware for this Mac (`vendorfw/`). The firmware archive is per Mac, not a
secret, but it is visible.

## The two routes

| | A. Encrypt before the system is written | B. Convert in place at first boot |
| --- | --- | --- |
| How | Sekka's root partition is LUKS2 from the start; the system is copied into it after the person chooses a passphrase | The engine writes the unencrypted image as today; before setup, the initrd encrypts the root partition in place with `cryptsetup reencrypt --encrypt` |
| Where the passphrase is chosen | Before any Sekka file exists | At first boot, before setup creates any user data |
| Temporary space | A second copy of the system: either a temporary partition the engine creates and later returns, or a provisioning initrd that copies from the ESP | None beyond LUKS2's header (16 MiB, made by shrinking ext4 first) |
| Interruption | Simple: an interrupted copy is repeated from the start into a fresh LUKS volume | LUKS2 reencryption keeps a checkpoint and resumes, but a power cut during the first segment or the ext4 shrink needs its own recovery path |
| Engine on macOS | Would need to create LUKS2 itself (no cryptsetup on macOS), or hand the job to a provisioning initrd | No change |
| Initrd | Needs the unlock prompt either way | Needs the unlock prompt, plus the reencryption and resume logic |
| Keyboard at the prompt | The Apple MTP keyboard needs its firmware in the initrd; it is (`vendorfw/firmware.cpio` is the first initrd) but this is untested at an unlock prompt | Same |
| Update and rollback | Every generation's initrd unlocks the same LUKS2 device; old generations must carry the unlock support too | Same, plus older generations can't predate the conversion |

## Recommendation for when it is built

Route B, restricted: convert at first boot, in the initrd, **before setup
runs and before any user data exists**, so a failed conversion can be
answered by reinstalling with nothing lost, and the passphrase prompt is the
first thing setup shows. Route A needs either a second Linux copy of the
system's size on the Mac (a large, temporary partition the engine must
create and return, which multiplies the APFS-resize risk this installer is
most careful about) or cryptography the engine can't do from macOS.

Before it can be offered, all of these need evidence, on the exact
candidate:

- the interruption matrix in a disposable VM: power loss during the ext4
  shrink, during the first reencryption segment, mid-way, and at the end;
  each restart must resume or stop with a clear message, never boot a
  half-converted root as if it were whole;
- the passphrase prompt in the real J514S initrd with the built-in keyboard
  and the Apple MTP firmware, a wrong passphrase, retries, and a non-US
  keyboard layout;
- swap: zram only (as today), so no encrypted swap partition is needed;
- cold boot, update, rollback to a generation from before an update, and
  booting macOS and returning;
- the recovery story: what someone does when they forget the passphrase
  (reinstall; there is no escrow), said plainly before they choose it.

## What the first release says

Sekka Installer's review screen: "Sekka's partition isn't encrypted. macOS
stays protected by FileVault; it doesn't protect Sekka. Encryption will come
in a later version." First-boot setup doesn't mention encryption. The README
lists it among the Mac's current limits.
