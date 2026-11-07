# The Mac installer's engine

`installer/genkan/macos/engine/` is the engine of Sekka's installer for Apple
Silicon Macs: Sekka's adapter (`sekka_engine`, GPL-3.0-or-later) over the
Asahi installer v0.9.2, which stays a pinned external dependency. The
adapter imports Asahi's modules from the extracted, SHA-256-pinned release
tarball at run time; none of Asahi's source is copied into Sekka's tree. It
speaks the `sekka.mac.engine/1` transcript that
`installer/genkan/macos/crates/contract/src/engine.rs` defines, and is part of
phase 4 of the [installer plan](../apple-silicon-installer-plan.md).

Nothing here has run on a Mac. Every result below comes from a fake Mac in the
test suite; the last section lists what only the J514S can show.

## Running it

The privileged helper extracts `sekka-engine-0.9.2-sekka.1.tar.gz` into a
private directory ROOT and starts the pinned runtime, isolated, by the entry
point's path:

```sh
ROOT/Frameworks/Python.framework/Versions/3.13/bin/python3.13 -I -B \
    ROOT/sekka_engine/__main__.py inspect [--owner SHORTNAME  3< password]
ROOT/Frameworks/Python.framework/Versions/3.13/bin/python3.13 -I -B \
    ROOT/sekka_engine/__main__.py execute --plan FILE --assets DIR \
    --installation UUID --owner SHORTNAME     3< password
ROOT/Frameworks/Python.framework/Versions/3.13/bin/python3.13 -I -B \
    ROOT/sekka_engine/__main__.py startup-disk --plan FILE \
    --installation UUID --owner SHORTNAME --esp-inventory DIGEST  3< password
```

`startup-disk` is the helper's bounded retry after execute's startup-disk
stage failed: the same plan file and installation, the `esp-inventory` fact
of that run's Esp checkpoint from the journal, and only that stage (below).

- **Environment.** `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, `LC_ALL=C`, and the
  runtime's own `DYLD_FRAMEWORK_PATH=ROOT/Frameworks` and
  `DYLD_LIBRARY_PATH=ROOT/Frameworks/Python.framework/Versions/Current/lib`,
  as Asahi's `install.sh` sets them. The engine then replaces its environment
  with `PATH`, `LC_ALL=C`, `LANG=C` and `SSL_CERT_FILE` naming the runtime's CA
  bundle; nothing else is read from it.
- **Bytecode.** `-B`: the first run would otherwise write bytecode into the
  unpacked engine and the runtime's sealed framework, which the engine cache
  then trusts as unpacked. Under `-I` Python ignores
  `PYTHONDONTWRITEBYTECODE`, so only the flag keeps it out.
- **Why not `-m sekka_engine`.** Under `-I` the package would have to be on the
  runtime's own `sys.path`, which means writing into
  `Python.framework/Versions/3.13/lib/python3.13/site-packages`. That
  directory is sealed by the framework's code signature (its `CodeResources`
  lists `site-packages/README.txt`), so the archive leaves it alone. `python3
  -I -m sekka_engine …` runs the same code wherever the package is importable,
  as in the tests.
- **Arguments.** Exactly these, each once. `--installation` is the lower-case
  UUID the helper's journal uses for this installation; execute writes it
  into the first-boot handoff and the Recovery configuration, and
  startup-disk finds the installation by it. `--owner` is the macOS short
  name whose password the helper verified, as the directory records it (the
  helper looks up the name typed, in whatever case). There is no option for
  simulation, another installer, profile, command or path; the tests inject
  their fakes into the `Engine` class, which no argument or variable reaches.
- **Descriptors.** Standard output is the transcript: one canonical JSON
  message per line, numbered from 1. Standard input carries the helper's
  acknowledgements. Standard error is a diagnostic log: the helper and the
  app drain it as it is written and keep its last 500 lines, the helper for
  Copy Diagnostics (with the owner's name and home folders taken out, and
  Asahi's list of administrators left out by the engine). The log decides
  nothing: once nothing reads it (the helper stopped), what the engine
  writes there is dropped and standard output and error point at
  `/dev/null`. Nor does the transcript's progress decide whether a stage
  that changes the disk ends: such a stage goes on to its end whether or
  not its progress can be written, and the checkpoint after it, which then
  can't be written either, ends the run before another stage starts. In
  `execute`, `startup-disk` and `inspect --owner`, file descriptor 3 carries
  the owner's password, read once to end of file and closed; it is kept in a
  `bytearray` only until `bless` has it (or the check has passed), then
  overwritten.
  (Python may hold other copies, such as the bytes handed to `bless`'s
  standard input; no complete erasure is claimed.)
  Descriptor 4 is the helper's install lock, a `flock` the engine holds by
  leaving it open until it exits, so nothing else installs or inspects for
  the helper while it may still be changing the disk, even if the helper
  stops first; its child processes don't inherit it. Before anything
  else runs, the engine takes private copies of descriptors 0 and 1 and points
  them at `/dev/null` and standard error, so neither Asahi's progress printing
  nor any child process can write into the transcript or read an
  acknowledgement.
- **Acknowledgements.** After each `intent` with sequence number N, the engine
  reads exactly `{"ack":N}` and a newline. Anything else, or end of input,
  stops it: it writes a `failure` for that stage (`not-acknowledged`) and a
  failed `completion`, and the stage never starts.
- **Exit status.** 0 after `inspected` or `waiting-for-recovery`, 1 after
  `failed`, 2 for a usage error (nothing is written to standard output),
  143 when stopped by SIGTERM (or ended by SIGALRM, below).
- **SIGTERM.** The helper's Cancel and its inspection's time limit send it.
  Outside a stage that changes the disk the engine stops where it is,
  through the run's own cleanup: what it mounted is unmounted and its
  working folder, with Apple's files in it, is deleted; nothing more is
  written to the transcript. That cleanup has 30 seconds (`STOP_LIMIT`):
  it unmounts through `diskutil`, which may be stuck on what the run was
  stuck on, so after that SIGALRM, or a second SIGTERM, ends the engine at
  once, wherever it is, and a Cancel or the time limit always ends it.
  During such a stage nothing is interrupted (Apple's tools are never
  stopped part way): the stage finishes and no other starts.
- **Commands.** Every command, the adapter's and Asahi's, goes through one
  `Host`: only `bless bputil chflags diskutil fdesetup hdiutil ioreg log mount
  newfs_msdos nvram pmset scutil sync sysctl tar xattr`, found only in the
  system directories, with output captured, standard input empty unless given,
  and the fixed environment. Asahi's modules reach it because the adapter
  replaces their `subprocess` with a shim.
- **Self-check.** At start the engine computes the digest of its own source
  files and compares it, and the bundled profile's digest, with
  `engine-lock.json`; it refuses (`engine-damaged`) on any difference. The
  Hello message names that adapter digest.

## inspect

`inspect` builds the contract's `Inspection` from Asahi's own model:
`system.SystemInfo` (ioreg's device tree and service planes, nvram, bputil,
the SystemVersion and SFR plists), `diskutil.DiskUtil` (diskutil's plists and
`get_partitions`) and `osenum.OSEnum` (each container's systems). The app runs
it unprivileged, before it asks for a password; the helper and execute run it
as root. Either way:

- every volume it mounts, Asahi's OSEnum's and other systems' ESPs included,
  is mounted read-only (`diskutil mount readOnly`) and unmounted again before
  it returns, also when it is stopped part way; what macOS had mounted is
  left as it was; nothing is written;
- what only root can read is reported in a `log` line rather than guessed:
  without `bputil` the boot mode is taken from the booted volume group, and a
  stub's state is `unknown`.

The helper's inspection at Begin, before it opens an installation, names
the owner (`inspect --owner`). The engine then checks that account and the
password on descriptor 3 by the same code `execute` and `startup-disk` use
(below: a short name, a password `bless` can take, and the owner in the
running macOS's `AdminUserRecoveryInfo.plist`) and refuses, with a stage-less
`failure` whose code is one of the contract's `OWNER_REFUSALS` and no
inspection, what they would refuse. The helper refuses Begin with that
reason before it records or imports anything, so the rule lives in the
engine alone; the tests hold all three modes to the same cases
(`support.UNUSABLE_OWNERS`).

What it fills in:

| Field | Source |
| --- | --- |
| machine | device tree (`compatible`, `chosen`, `product`); binding is `machine_binding` of IOPlatformUUID |
| macos | SystemVersion, SFR (`/System/Volumes/iSCPreboot/SFR/current`), the booted volume group |
| disk.uuid | the GPT disk GUID, from IOGUIDPartitionScheme's `UUID` property under the whole disk's IOMedia (IOStorageFamily publishes it; diskutil doesn't) |
| partitions | Asahi's `get_partitions`; name is the GPT name (`IORegistryEntryName`), or the FAT volume name for an ESP |
| containers | `diskutil apfs list` and `listVolumeGroups`; `snapshots` counts `apfs listSnapshots` entries except a System volume's own sealed snapshot |
| resize | Asahi's `action_resize` rule: `minimum` = max(align_up(used + 38 GB, 1 MiB), diskutil's `MinimumSizePreferred`); `recommended` = max(minimum, preferred), so equal to it |
| candidates | free regions Asahi lists that hold a stub; macOS containers Asahi's `can_resize` accepts, with `available` = current − minimum (offered only from 2 MiB) |
| power | `pmset -g batt` |
| filevault | `fdesetup status`, else the data volume's state in Asahi's apfs list |

Installations are recognized, never repaired:

- **Sekka**: a stub whose System volume has `Sekka/InstallData/recovery.json`,
  or whose ESP has `sekka/install.json`. `complete` or `waiting-for-recovery`
  from the boot policy (`coih`) when every part is present and the records
  agree; `partial` when parts are missing; `unknown` when the records disagree
  or the policy can't be read. Either way a blocking `existing-sekka` or
  `partial-installation` issue: the initial release installs once.
- **Sekka, the developer image** (below): a stub without those records whose
  ESP has the developer image's FAT serial. It is `sekka` with no
  `installation_id` and `developer` true, its state Asahi's (`coih`, `Finish
  Installation.app`), or `partial` when its root is missing or its ESP holds
  neither `m1n1/boot.bin` nor `sekka/boot-bin.json`, `unknown` when the ESP
  can't be mounted. A blocking `existing-sekka` (or `partial-installation`)
  issue says that it is a developer installation and which partitions to
  delete in macOS before installing, each by what `diskutil list` shows of
  it (the stub's name, the ESP's volume name, sizes) with its number at the
  time, since numbers can change once one is deleted; and to make macOS the
  startup disk first
  ([removal](removal-and-boards.md#a-developer-installation)).
- **Sekka, unidentified**: a stub without those records whose ESP holds
  `sekka/boot-bin.json` (the record only Sekka's boot-bin writes) but not the
  developer serial. It is Sekka's, but whether the developer image's or an
  installation whose handoff is gone isn't guessed: `sekka`, `unknown`, no
  `installation_id`, blocking `existing-sekka`.
- **other-linux**: a stub with Asahi's marks (m1n1 version, `Finish
  Installation.app`, an ESP with `asahi/`) and none of the above.
- **unknown**: any other stub, an APFS container with no system (blocking
  `partial-installation` if it is named Sekka, otherwise a non-blocking
  `unrecognized-layout`), and EFI or Linux partitions no stub claims. These
  are `sekka` instead, `partial`, with a blocking `partial-installation`:
  - an empty container that could be a stub (at most 4 volumes, and named
    Sekka or no larger than a stub) followed by an ESP with the developer
    serial: the developer image's (`developer`), with that ESP and the
    Linux partitions OSEnum would attach. Any other container in front of
    such an ESP (one the owner made where a stub was, say) is left
    `unknown`, and the ESP is reported on its own, as below;
  - an ESP no stub claims with the developer serial and no handoff: the
    developer image's, with the Linux partitions directly after it (up to
    three, stopping at anything else), which are named to delete with it;
  - an ESP no stub claims with a handoff, or with Sekka's boot record.

Sekka's developer image (`nix/apple-silicon/developer/`, installed by
`install-sekka.sh` with the unmodified Asahi installer, as on the reference
J514S) writes no handoff and no Recovery configuration, and mounts its root
by the label `sekka-root` and its ESP by the FAT serial `5ECC-A5E1`
(`identities.json`). Of those, macOS can see only the serial: diskutil has
no ext4 support and reports no volume name or UUID for a Linux partition
(the recorded M1 Pro's root has neither), and the inspection reads volumes
only through diskutil and read-only mounts, never raw devices. Nor does
diskutil report a FAT serial; it reports the `VolumeUUID` macOS's msdosfs
derives from the boot sector (`msdosfs_generate_volume_uuid`: a version 3
UUID, the MD5 of `kFSUUIDNamespaceSHA1`, the volume ID as stored and the
total sector count, little-endian). The engine compares an ESP's
`VolumeUUID` with that UUID for serial `0x5ecca5e1` and the partition's
size in its 4 KiB blocks, which is how newfs_msdos counts it: the recorded
M1 Pro's ESP, 128,000 blocks, has the UUID of serial `CEF8-B598`, the only
32-bit serial that gives it (`test_msdosfs_uuids_are_the_ones_macos_reports`).
The derivation is msdosfs's source and that recording; no developer
installation's `diskutil info` has been recorded. An ESP whose sectors
newfs_msdos counted otherwise wouldn't match, which is why Sekka's boot
record alone still stops installation. A handoff wins over the serial: an
ESP with both is the installation the handoff identifies. No product
release has the serial (its installer data gives the ESP no `volume_id`,
which `manifest.py` holds it to), and neither does the tests' release.

So a developer installation without its ESP isn't recognized from macOS:
its stub, with Asahi's marks, is `other-linux`, and its root, or a root
left alone, an `unknown` partition with a non-blocking
`unrecognized-layout` (`test_a_developer_install_without_its_esp_is_not_recognized`).
Neither stops installation. Such an installation can't start (its stub
chain-loads from the ESP), and the product mounts its own partitions by
identities of its own, so a product installation beside it mounts nothing
of it; the owner can remove it with the same steps
([removal](removal-and-boards.md#a-developer-installation)).

The `developer` flag is what the app goes by. Sekka Installer never makes
a developer installation, so where every Sekka installation on the disk is
one, the app doesn't ask for its helper, which has no record of one to
check; and wherever one is, its Existing page says that a developer
installation is on the Mac, with the issue's details and Check Again.

Issues come from the J514S profile's admission (board, chip, system firmware
`block`, macOS minimum, the macOS 27.0 resize warning), Asahi's own device
table, minimum macOS (13.5) and ProMotion check (`bugs.run_checks`), boot mode,
power (battery below 20% is a warning, battery power blocks), FileVault
converting, snapshots, unavailable resize limits, space (Asahi's 10 GB
`MIN_INSTALL_FREE`) and more than one internal Apple disk.

## execute

### Before anything changes

The engine refuses, with a `failure` whose stage is null, `disk_changed: no`
and nothing changed, unless all of these hold:

- the plan is canonical `sekka.mac.plan/1`, every field as `plan.rs` declares
  it, and `Plan::check`'s arithmetic holds; the installation UUID, owner name
  and password are usable (a password with a leading or trailing space can't go
  through `bless --stdinpass`, as Asahi's `bless()` notes);
- the bundled profile is the plan's (id and canonical digest);
- each asset is a regular file, opened without following links, whose SHA-256
  is the plan's; then their contents (below);
- a fresh inspection as root agrees: machine binding and identifiers, GPT disk
  UUID, layout digest, the running macOS's volume group, profile admission,
  and every acknowledgement the admission needs is in the plan; no blocking
  issue;
- every action agrees with Asahi's arithmetic: the stub is Asahi's
  `STUB_SIZE`; Sekka has at least the stub plus the template's partitions; a
  shrink starts from the container's current size, ends at or above Asahi's
  minimum, and leaves Sekka's extent directly behind the container; a
  free-space plan lies inside a free region Asahi sees;
- the owner is in the running macOS's `AdminUserRecoveryInfo.plist` (Asahi's
  `admin_users`), the list the stub's Recovery gets. macOS updates it when
  it updates, so an account made an administrator or given a secure token
  since may be missing; the refusal says so and how to bring it up to date
  (`diskutil apfs updatePreboot /`, which Asahi's installer runs itself and
  the engine doesn't, since it changes a volume outside the plan);
- Asahi's `choose_ipsw` rule, without prompts, gives an OS firmware the
  profile lists (14.8.3).

The assets directory holds exactly:

| File | Checked |
| --- | --- |
| `payload.zip` | only `esp/…` and `root.img`; no links, devices, encryption or case-twins; every ESP file's SHA-256 is the manifest's; `root.img`'s size is the manifest's; the boot entry has each placeholder once, as the value of its own argument on an `options` line (`sekka.root=PARTUUID=@SEKKA_ROOT_PARTUUID@`, `sekka.esp=PARTUUID=@SEKKA_ESP_PARTUUID@`), anywhere and in either order, as first-boot setup reads the command line |
| `installer_data.json` | one OS; partitions exactly a FAT ESP from `esp/` with `copy_firmware`, then an expanding Linux root from `root.img`; `m1n1.bin` chainloading `m1n1/boot.bin`; supports 14.8.3; no `extras` or `icon`; the ESP's size is the plan's |
| `manifest.json` | `sekka.mac.manifest/1` (below); release, revision and root-image digest are the plan's |
| `recovery.zip` | every member a plain path inside `Sekka Recovery.app/`: no `..`, `.`, empty or absolute components, backslashes, links, devices, `__MACOSX` or `._` files, name twice ignoring case, encryption or unknown compression; at most 4096 entries and 512 MiB; Info.plist and an executable in Contents/MacOS |

The manifest is not yet defined in the contract crate. The engine reads this
much of it, and allows other top-level fields:

```json
{
  "schema": "sekka.mac.manifest/1",
  "release": "sekka-j514s-…",
  "source_revision": "<40 hex>",
  "root_image": { "path": "root.img", "size": 0, "digest": "sha256:…" },
  "esp": {
    "boot_entry": "loader/entries/nixos-generation-1.conf",
    "files": [ { "path": "m1n1/boot.bin", "size": 0, "digest": "sha256:…" } ]
  }
}
```

ESP paths are relative to `esp/`. The ESP inventory digest the engine reports
is the canonical digest of `{"domain":"sekka.mac.esp-inventory/1","files":[…]}`
with the files sorted by path; the image producer's `image.esp_inventory`
should use the same form.

### Stages

The stages are `macos_stages()`'s, in order. Each starts only after its
acknowledgement and ends with a checkpoint whose facts are what the engine
found afterwards, by identity (GPT and APFS UUIDs), never by `diskN`.

| Stage | What it does | Facts | Verification |
| --- | --- | --- | --- |
| apple-inputs | Asahi's `StubInstaller.load_ipsw` for the 14.8.3 image and `load_identity` for this board; every member the stub needs is fetched into a private spool (below) | os-firmware, restore-url, restore-bytes, build, build-manifest (SHA-256 of BuildManifest.plist), members, member-bytes, fetched-bytes, authenticity | inspected |
| shrink | limits rechecked, then only `diskutil apfs resizeContainer PART BYTES` through Asahi's DiskUtil, output captured | container, partition, size-before, size-after, free-offset, free-size | inspected |
| stub-partition | Asahi's `addPartition(after, "apfs", "Sekka", STUB_SIZE)`; the new partition must be APFS, STUB_SIZE, and start within 1 MiB before the plan's offset (exactly at it after a shrink) | stub-partition, stub-container, stub-offset, stub-size | inspected |
| stub-system | Asahi's `prepare_volume`, `check_volume`, `install_files` (including its admin-user copy for paired Recovery), then Asahi's `Finish Installation.app` and `.IAPhysicalMedia` are removed | stub-volume-group, stub-system-volume, stub-data-volume, stub-macos, build, admin-users | inspected |
| partitions | Asahi's `OSInstaller.partition_disk` from the template: the FAT ESP, with the serial `newfs_msdos` chooses (the product's installer data gives no `volume_id`; esp-volume-id is `none`), and the root, sizes and places checked | esp-partition, esp-offset, esp-size, esp-volume-id, root-partition, root-offset, root-size | inspected |
| firmware | Asahi's `collect_firmware` into a `FWPackage`, copied to the ESP's `vendorfw/`; unmounted, mounted, read back and hashed | firmware-files, firmware-cpio, firmware-cpio-size, firmware-inventory | read-back |
| esp | the payload's `esp/` tree (Asahi's `extract_tree`, CRC-checked); the boot entry's `@SEKKA_ROOT_PARTUUID@`/`@SEKKA_ESP_PARTUUID@` filled with the real GPT UUIDs; `sekka/install.json`; Asahi's `asahi/` installer data; AppleDouble files removed; unmounted, mounted, every file read back and compared with what was written, nothing extra | esp-inventory, esp-files, esp-bytes, boot-entry, kernel-arguments (as the filled entry passes them), handoff, installation, setup-hints | read-back |
| root | `root.img` streamed from the package to `/dev/rDISKsN`, hashed as read, flushed (`F_FULLFSYNC`, else `DKIOCSYNCHRONIZECACHE`, else `fsync`), then read back from the raw device and hashed; both must be the manifest's digest | root-partition, root-image-size, root-image, root-read-back, flush | read-back |
| recovery | `Sekka Recovery.app` from the checked zip, staged and renamed into place on the stub's System volume; `Sekka/InstallData/` with `recovery.json`, `boot.bin` (Asahi's `m1n1.build` of `boot/m1n1.bin` with `chosen.asahi,efi-system-partition` and `chainload=ESP;m1n1/boot.bin`, as `osinstall.install` builds it) and the firmware baseline under `firmware/`; `.IAPhysicalMedia` from Asahi's plist with `AppName` `Sekka Recovery.app`; unmounted, mounted, read back | recovery-app, recovery-app-files, recovery-config, boot-object, boot-object-size, firmware-baseline, iaphysicalmedia | read-back |
| startup-disk | Asahi's `prepare_for_bless` (SystemVersion visible), then `bless --setBoot --device /dev/SYSTEM --user OWNER --stdinpass`, as Asahi runs it; then the selection is read back twice (`bless --getBoot` → `diskutil info` → APFSVolumeGroupID, and Asahi's `nvram boot-volume` parse) and both must name the stub's group; then Asahi's `prepare_for_step2` hides SystemVersion so the stub opens Recovery | startup-volume-group, bless-boot-device, nvram-boot-volume-group, recovery-app | policy-read-back |

Setup hints come from the owner's own preferences: `AppleLocale` to a glibc
locale, the current keyboard layout to an XKB layout from a fixed table
(layouts that need a variant, such as Dvorak, are left out),
`/etc/localtime`'s zone, `scutil --get ComputerName`, and the owner's full name
from `AdminUserRecoveryInfo.plist`. Any of them may be absent. They are read
before the first intent, so a read that fails changes nothing. The owner's home
folder and uid come from the directory service, and every preference file is
read with `O_NOFOLLOW | O_NONBLOCK`, must be a regular file owned by the owner
(root for `/Library/Preferences` and the admin records) and at most 1 MiB; a
link, FIFO, device, folder, foreign or oversized file is no hint.

### Mounts

Each run works through one view whose mounts it records by GPT or volume UUID.
Only Sekka's own volumes are mounted read-write: the planned stub's volumes once
stub-partition has created its container, and the planned ESP once partitions
has created it. Everything else Asahi's code or the stages mount (macOS's paired
Recovery volume, other systems' volumes) is mounted read-only. The esp stage
unmounts the ESP as soon as its read-back passes, and when the run ends,
successful or not, everything it mounted is unmounted; volumes macOS mounted
itself are never touched. A mount is recorded before `diskutil` is asked
for it, and a view that stops while it is being built unmounts what it
mounted, so a run stopped at any point, even as `diskutil` mounts, leaves
nothing of its own mounted.

Asahi's `step2.sh` flow is not installed. The Recovery app takes over what it
did: check that it runs in the stub's paired One True Recovery, `bputil -nc`,
`kmutil configure-boot -c Sekka/InstallData/boot.bin --raw --entry-point 2048
--lowest-virtual-address 0`, then restore `SystemVersion.plist` and rename
`.IAPhysicalMedia` to `IAPhysicalMedia-disabled.plist`.

### startup-disk: the bounded retry

When execute's startup-disk stage fails, the payload is in place and
verified, and only the startup selection is missing. The helper retries it
with this mode, which runs `stage_startup_disk` exactly as execute does
(Asahi's `prepare_for_bless`, `bless --stdinpass`, both read-backs, Asahi's
`prepare_for_step2`), so the stub still starts into Recovery and opens the
Recovery app. A retry of bless alone would leave SystemVersion as bless needs
it and the stub would start normally. No other stage runs in this mode.

The same retry follows a startup-disk stage that stopped part way, when the
helper or the Mac restarted during it and the journal ends with its intent
(in execute or in an earlier retry). It is the one interrupted stage the
contract lets run again, since everything below is checked first whatever
it left. The helper retries only while it holds the install lock, so no
engine is still in the stage, and records the interruption as that stage's
failure (`interrupted`, `disk_changed: unknown`) before it starts this mode.

Before its one intent, with nothing changed, it refuses (`stage: null`,
`disk_changed: no`) unless:

- the plan is canonical and passes `Plan::check`, the bundled profile is the
  plan's, and the installation UUID, owner and password are usable, as in
  execute;
- a fresh inspection as root agrees on the machine (binding, product type,
  device class, chip, system firmware), the GPT disk UUID and the running
  macOS's volume group, and the Mac runs its normal macOS. The layout digest
  isn't compared: the installation's own partitions changed it;
- the owner is in the running macOS's `AdminUserRecoveryInfo.plist`;
- exactly one stub's System volume has a `Sekka/InstallData/recovery.json`
  whose `installation` is `--installation` (`installation-not-found`,
  `installation-ambiguous`);
- that configuration still describes what is on the disk
  (`installation-changed` otherwise): its release, volume name and macOS
  volume group are the plan's; its stub container, volume group, System and
  Data volumes are the stub's own; its ESP and root partitions exist with the
  right types; `boot.bin` and every firmware-baseline file match their
  recorded size and SHA-256; the Recovery app's Info.plist is there; and
  `.IAPhysicalMedia` names `Sekka Recovery.app`;
- the ESP, mounted read-only for the check (and unmounted again, after an
  existing read-write mount is replaced), still holds exactly what execute
  installed and read back: its whole inventory, every file's path, size and
  SHA-256 in the esp stage's form, must give `--esp-inventory`
  (`esp-inventory-invalid` if that isn't a digest, `installation-changed` if
  it differs). So `m1n1/boot.bin`, which Recovery's `boot.bin` chainloads,
  the kernel, initrd, firmware and boot entries are as verified. Its
  `sekka/install.json` must name this installation, root and ESP, and its boot
  entries must be filled in and one must name the same partitions, by the
  rule execute filled it by.

The stub's volumes are only read until these checks pass; then they may be
mounted read-write for the stage, which renames the stub's SystemVersion. The
ESP is never mounted read-write in this mode.

Then it writes the intent, waits for the acknowledgement and runs the stage:
the checkpoint has the same facts and `policy-read-back` as in execute,
followed by `waiting-for-recovery`. A failure there is reported for the
stage with `disk_changed: yes`, since the installation changed the disk before
this run.

### Failures

The first failure ends the run with a `failure` for the running stage and a
failed `completion`; no stage is retried. Apple's own words are kept in
`apple_diagnostic`, verbatim apart from control characters, with the tool's
exit status, for example a resize failing with `-69606`.

`disk_changed` describes the disk against the plan's, not the stage alone:
`yes` once any stage that changes the disk has completed; before that the
engine inspects again and says `no` only if the layout digest is still the
plan's, `yes` if not, `unknown` if it can't look. A failed apple-inputs is
always `no`.

### Apple's restore image

Asahi's StubInstaller reads Apple's update (the zip on updates.cdn-apple.com
that `main.IPSW_VERSIONS` names for 14.8.3) through its URLCache while it
builds the stub. Here the apple-inputs stage fetches the same members first:
`needed_members` applies `install_files`' and `collect_firmware`'s selection
rules and path mapping for this board's build identity, and each member's local
header and data, with the zip's directory, are copied into a sparse spool file
at their own offsets. The stub install then reads the spool through an
ordinary ZipFile; reading any byte that wasn't fetched raises an error instead
of returning zeros. A network failure therefore stops the installation before
the disk changes.

The spool stays in the engine's working folder, on macOS's own volume, until
the run ends. Before fetching anything but the zip's directory, the stage
compares what the members need with the room the plan left: above Asahi's
minimum for the shrunk container (the helper's plan reserves the release's
`apple_inputs` estimate there), or the macOS container's free space less 1 GiB
for a free-space plan. That GiB is `plan.rs`'s `MACOS_MARGIN`, the engine's
`contract.MACOS_MARGIN`, both held to `tests/vectors/expected.json`: a plan
for free space needs the macOS container to have the downloads still to
come, the `apple_inputs` estimate and that margin free, or there is none
(`MacosNeedsRoom`). If they don't fit it stops (`apple-inputs-no-room`,
`disk_changed: no`), saying to check this Mac again for a new plan after a
shrink, and to free some space in macOS first beside free space, where a new
plan would be the same. The shrink stage's recheck of Asahi's minimum then leaves
out what the engine's own files occupy, since they are deleted when the run
ends.

What the bytes' authenticity rests on is what Asahi's rests on, no more. There
is no check against an Apple-signed list:

- an HTTPS connection to the host in the URL, verified against the CA bundle in
  the pinned runtime (`SSL_CERT_FILE`);
- the URL itself, pinned in Asahi's `main.py` inside the SHA-256-pinned
  tarball;
- each member's zip CRC-32, checked as it is read in full (corruption, not
  forgery).

Neither Asahi nor the adapter checks a cryptographic digest of Apple's bytes,
and `BuildManifest.plist` comes from the same zip. The Mac's boot chain
verifies the signed boot objects it later loads; it does not cover the
firmware copied out for Linux (Wi-Fi, Bluetooth, keyboard and trackpad). The checkpoint records the
BuildManifest's SHA-256 so that what was used can be identified afterwards.

## Tests

`python3 -I -B -m unittest discover -s tests -t tests` in
`installer/genkan/macos/engine`, with `SEKKA_ASAHI_ROOT` naming the extracted
pinned tarball (and, optionally, `SEKKA_TRANSCRIPT_VALIDATOR` naming the
contract's `validate_transcript` example). `default.nix`'s `tests` runs it
under Python 3.13 with the validator, after building the contract crate and
running its tests.

The fake Mac (`tests/support.py`) keeps one model of a Mac and answers every
command Asahi and the adapter run from it; Asahi's own modules do the work.
Partitions and volumes are directories and files in a temporary directory;
renumbering after a new partition is modeled, as diskutil does it, and so is
each mount's mode, the volumes macOS mounted itself (which the fake refuses to
let the engine unmount), and the engine's working folder taking space from
the macOS container, as `/private/tmp` does on a Mac. Linux
can't run libSystem's `fsctl`, LZFSE compression or the firmware collectors on
real Apple firmware, so the tests replace those, and serve a synthetic 14.8.3
update from memory through Asahi's URLCache class.

Scenarios: a J514S as it was before Sekka (sizes from its GPT), with macOS
already shrunk, with Sekka installed (the reference machine's layout today),
with Sekka's developer image installed (the reference machine itself), with
another system Asahi installed, with an interrupted stub, with unknown
systems, and Omarchy's recorded M1 Pro with Asahi installed (MIT fixtures, see
their NOTICE). The fake reports a FAT volume's `VolumeUUID` as msdosfs
derives it from its serial, which `newfs_msdos -I` sets. They cover:

- inspection of each, unprivileged and as root; issues for battery, FileVault
  converting, snapshots, missing resize limits, a second internal disk,
  untested firmware and another board; inspection never changes the fake, and
  as root too mounts only read-only (other systems' volumes and an Asahi
  ESP included) and leaves nothing mounted;
- recognition: an installation by its handoff, also on an ESP with the
  developer serial; the developer image alone (enrolled, waiting for
  Recovery, unprivileged, and without boot-bin's record), beside free space
  (described, not offered, and execute refusing a plan for it), without its
  root or boot files, stopped before its stub's system (also renamed), and
  as an ESP and root a removed stub left, naming those partitions by name
  and size; a container of the owner's in front of a developer ESP, not
  claimed; a developer stub and root without their ESP, and a root alone,
  not recognized and stopping nothing; Sekka's boot record without an
  identity, not guessed; another Asahi system on the J514S and the M1 Pro's,
  still `other-linux`; msdosfs's UUID for the M1 Pro's recorded ESP;
- the helper's inspection for an owner refusing exactly the owners and
  passwords execute and startup-disk refuse (`UNUSABLE_OWNERS`), with their
  codes, and otherwise reporting the same inspection;
- execute mounting read-write only the stub's volumes and the ESP, and leaving
  nothing mounted, the verified ESP included;
- setup hints: a FIFO, a link to `/dev/zero`, a folder, an oversized file or
  another account's file at the owner's preference paths gives no hint and
  never stalls the run;
- Apple's files at a plan's maximum size with nothing reserved:
  `apple-inputs-no-room` before anything is fetched or changed; with room
  reserved, the shrink succeeding although the engine's own files raised
  Asahi's minimum above the plan's size;
- execution of a shrink plan and a free-space plan to `waiting-for-recovery`,
  checking every partition, file, byte and selection left behind;
- the state at every acknowledgement equal to the state at the previous
  checkpoint, and apple-inputs changing nothing; refusing to acknowledge at
  any stage stops it with nothing changed after that;
- refusals with nothing changed: a stale plan (layout changed), another
  Mac, another disk, another profile, a changed or linked asset, an
  unsupported board, untested firmware, a non-canonical or inconsistent plan,
  a shrink below Asahi's minimum, a blocking issue, and unusable passwords,
  owners and installation IDs;
- a resize failing with `-69606`: Apple's message kept, `disk_changed: no`
  after reinspection, no retry; a resize failing part way: `yes`;
- a root read-back mismatch, bless failing, and bless exiting 0 without
  changing the selection;
- startup-disk on the Mac either bless failure leaves behind: only one
  intent and one `bless --setBoot`, nothing changed before the
  acknowledgement, the stub selected and SystemVersion hidden afterwards; bless
  failing again; another installation's UUID and a Mac without the
  installation refused; no acknowledgement, no bless; a changed `boot.bin`,
  launch marker, Recovery app, ESP handoff or missing root partition refused;
  a changed `m1n1/boot.bin`, a missing kernel or an extra file on the ESP, and
  a wrong or malformed `--esp-inventory`, refused; the ESP mounted only
  read-only and nothing left mounted; another Mac, an unreadable plan, no
  password, another owner and a malformed UUID refused;
- Recovery-zip attacks (traversal, absolute paths, links, devices, resource
  forks, case twins, backslashes, encryption, corruption, missing parts), and
  payload, template and manifest substitutions bound into a plan;
- canonical JSON's rules, and vectors: `tests/vectors` holds inspections,
  plans, a handoff, a Recovery configuration, a string-escaping document and
  ten transcripts. Python must write `expected.json`'s bytes and digests,
  which the Rust crate computed; the crate's `tests/engine_vectors.rs` checks
  the same documents with its types and feeds every transcript to the
  helper's `Transcript`, which accepts all of them.

## Packaging

`default.nix` takes `{ pkgs }` and returns `archive`, `tests` and `contract`
(the crate's examples). `archive` fetches the tarball by its SRI hash, adds
`sekka_engine/*.py`, `sekka-profiles/j514s.json` and `engine-lock.json`
(Asahi's version, tag, commit and tarball digest; the adapter's digest and
files; the profile's digest; the runtime and entry paths), and packs
everything deterministically: names sorted, owner 0:0, modes 0755 or 0644,
time 0, gzip without a name or time. Nothing of Asahi's is left out or
changed. `engine.json` beside it gives the archive's size and SHA-256.
`nix build --rebuild` reproduces it bit for bit.

```sh
nix build .#packages.aarch64-linux.mac-engine        # the archive and engine.json
nix build .#packages.aarch64-linux.mac-engine-tests  # the adapter's tests against a fake Mac
```

`source-lock.json` records the Asahi tag, commit and tarball, which of its
modules the adapter uses, and the Omarchy code and fixtures adapted, with their
license.

## Open points

- `RecoveryConfig::check` required the stub's volume group and Data volume to
  differ, but a group made with `diskutil apfs addVolume … -groupWith`, as
  Asahi makes the stub's, takes its Data volume's UUID (Omarchy's recorded M1
  Pro stub: `0BF26A61-…` for both). The check now allows that one pair; every
  other identity must still differ.
- The image (`nix/apple-silicon/image.nix`) must produce `manifest.json` as
  above and a boot entry with the two placeholders; today's entry names the
  root by label.
- The disk's GPT GUID comes from IOGUIDPartitionScheme's `UUID`, which
  IOStorageFamily's source publishes; it hasn't been read on the J514S.

## What only a real Mac can show

None of this has run on macOS. The fake reproduces the commands' formats
from recorded outputs and Asahi's own parsers, not Apple's behavior.
On the J514S, with a backup and an owner's explicit authorization:

- the bundled runtime running the adapter under `-I` with the helper's
  environment, and `ioreg`'s IOService plane carrying IOGUIDPartitionScheme's
  `UUID` and IOPlatformUUID where the adapter looks;
- `bputil`, `diskutil mount readOnly`, `apfs listSnapshots`, `fdesetup status`
  and the resize limits as an unprivileged user;
- the mount policy's inputs: `diskutil info`'s `DiskUUID`,
  `APFSContainerReference` and `PartitionMapPartition` for volumes and
  partitions, `diskutil unmount UUID`, and how much the spool under
  `/private/tmp` raises the container's used space;
- the developer installation's ESP in `diskutil info`: its `VolumeUUID`
  being msdosfs's for `5ECC-A5E1` and 262,144 blocks, and the inspection
  stopping on it;
- `diskutil apfs resizeContainer` to the plan's byte count, `addPartition`
  placing the stub exactly behind the shrunk container, `newfs_msdos` and
  diskutil's partition renumbering;
- the 14.8.3 update's members matching `needed_members`, the spool, and
  Asahi's stub install from it; `collect_firmware` on real firmware;
- raw writes and read-back on `/dev/rdisk`, and which flush call the device
  accepts;
- `bless --setBoot --stdinpass` with the owner's password, `bless --getBoot`'s
  output, the `nvram boot-volume` read-back, and the stub opening `Sekka
  Recovery.app` through `.IAPhysicalMedia`;
- everything after: the Recovery app, enrollment, the first boot from the
  handoff's partition UUIDs.
