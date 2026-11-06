# Removal, replacement and other Macs

Status: phase 11 of the [implementation plan](../apple-silicon-installer-plan.md).
This release **recognizes** an existing or partial Sekka installation and
stops with an accurate explanation; it does not remove, replace or repair
one, and it supports one Mac.

## Recognition (implemented)

The engine's inspection reports every installation it finds on the internal
disk: Sekka (by its stub, its handoff on the ESP and its Recovery
configuration), another system installed by Asahi's installer, or something
unknown, each as complete, waiting for Recovery, partial or unknown. Sekka
Installer shows them and offers nothing destructive:

- **Sekka is installed:** "Sekka is already on this Mac", how to start it,
  and that this version doesn't remove or replace an installation.
- **An installation the helper is still running or that stopped part way:**
  the app reconnects to it (its journal), shows what changed and the next
  safe action the journal permits, and refuses to start another until it is
  looked at.
- **Something it can't describe with certainty:** an unrecognized layout is
  a blocking issue. The installer won't change a disk it can't describe.

Uninstalling the app (or deleting its helper) never removes Sekka's
partitions or the helper's records.

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
