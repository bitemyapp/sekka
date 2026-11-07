# Removal, replacement and other Macs

Status: phase 11 of the [implementation plan](../apple-silicon-installer-plan.md).
This release **recognizes** an existing or partial Sekka installation and
stops with an accurate explanation; it does not remove, replace or repair
one, and it supports one Mac.

## Recognition (implemented)

The engine's inspection reports every installation it finds on the internal
disk: Sekka (by its stub, its handoff on the ESP and its Recovery
configuration, or, for the developer image, by its ESP's FAT serial),
another system installed by Asahi's installer, or something unknown, each as
complete, waiting for Recovery, partial or unknown. Sekka Installer shows
them and offers nothing destructive:

- **Sekka is installed:** "Sekka is already on this Mac", how to start it,
  and that this version doesn't remove or replace an installation. How to
  start it is shown only when a Sekka installation has its startup volume
  and isn't missing parts.
- **Part of a Sekka installation is left** (an ESP no startup volume
  claims, an empty stub container, a stub missing parts): "Part of a Sekka
  installation is still on this Mac", with what the inspection said of it
  and Check Again.
- **Sekka's developer image** (below): "A developer installation of Sekka
  is on this Mac", with the inspection's details naming the partitions to
  delete, and Check Again.
- **An installation the helper is still running or that stopped part way:**
  the app reconnects to it (its journal), shows what changed and the next
  safe action the journal permits, and refuses to start another until it is
  looked at.
- **Something it can't describe with certainty:** an unrecognized layout is
  a blocking issue. The installer won't change a disk it can't describe.

Uninstalling the app (or deleting its helper) never removes Sekka's
partitions or the helper's records.

### A developer installation

Sekka's developer image, which `install-sekka.sh` installs with the
unmodified Asahi installer, has no handoff and no Recovery configuration,
and finds its partitions by fixed names (the root's label `sekka-root`, the
ESP's FAT serial `5ECC-A5E1`). The engine recognizes it by that serial
([engine](engine.md#inspect)) as Sekka without an installation ID, marked
as the developer image's, and installation stops: Sekka Installer doesn't
install beside it, and doesn't remove it. An ESP that holds Sekka's boot
record (`sekka/boot-bin.json`) with neither a handoff nor that serial stops
installation too, since which installation it belongs to can't be told.

Sekka Installer then says "A developer installation of Sekka is on this
Mac": that it was installed with `install-sekka.sh`, and that to install
from the app it must be removed in macOS first. Below that is what the
inspection found: the partitions to delete, by the stub's name, the ESP's
volume name and their sizes, with their `disk0sN` numbers as they were at
that moment. It doesn't ask to set up its helper, which keeps no record of
a developer installation. What is left of one, an ESP with the serial whose
stub was removed say, is shown the same way.

The serial is the only sign macOS can see: diskutil reports no label or UUID
for the Linux root. So a developer stub and root whose ESP is gone, or a
root left alone, aren't recognized: the stub is shown as another system
Asahi installed and the root as an unknown partition, and neither stops
installation. Without its ESP that installation can't start, and Sekka
installed beside it mounts none of its partitions; remove it the same way
when you like.

To install Sekka from the app on a Mac with a developer installation,
remove it yourself first, from macOS, with a backup. Partition numbers can
change once one is deleted (the cheatsheet below warns of it), so check
each one with `diskutil list` before deleting it, by its name and size. As
Asahi's
[partitioning cheatsheet](https://asahilinux.org/docs/sw/partitioning-cheatsheet/)
describes:

1. Copy off anything on the developer system you want to keep. Everything
   on it is deleted.
2. Make macOS the startup disk (System Settings › General › Startup Disk).
3. Delete its stub, the APFS container named after it (2.5 GB):
   `diskutil apfs deleteContainer disk0sN`.
4. Delete its ESP (`EFI - SEKKA`) and its Linux partition:
   `diskutil eraseVolume free free disk0sN` for each.
5. If you'd rather give the space back to macOS than install Sekka into
   it: `diskutil apfs resizeContainer disk0s2 0`, where `disk0s2` is
   macOS's container. Sekka Installer can install into the free space
   either way.

Never delete the first partition (the iBoot System Container) or the last
(`Apple_APFS_Recovery`), and use `diskutil`, not Disk Utility. Then choose
Check Again in Sekka Installer. These are Asahi's steps; Sekka's checks
have not run them, and the recognition itself has run only against the
engine's fake Mac.

## Removal and replacement (later)

A removal must be its own reviewed plan, bound like an installation:

- recognized narrowly, by the exact stub volume group, ESP and root
  partition UUIDs and the APFS container it returns space to, read from the
  installation's own records and checked against the disk;
- a fresh destructive confirmation that names what is deleted;
- a durable intent before each deletion, and a separate step (and record)
  for growing macOS's container afterwards. If growing fails, the space is
  reported as unallocated and the partitions are **not** deleted again;
- interruption reconciliation like the installation's, tested at every
  boundary on disposable disks.

Replacement (reinstalling in place) is removal followed by installation,
each with its own plan and confirmation.

## Other Macs

A new board needs its own platform work before the app can name it:

- a hardware profile (`installer/genkan/macos/crates/contract/profiles/`),
  with only observed and reported evidence;
- its addition to `KNOWN_BOARDS` in the contract, in a reviewed change: a
  catalog can't add a board the app's source doesn't know;
- a matched kernel, m1n1, U-Boot and graphics stack, and its boot and
  firmware policy;
- the whole qualification ledger on that board. Nothing transfers from the
  J514S because the app or the image builder are shared.
