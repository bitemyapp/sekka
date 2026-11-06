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
ROOT/Frameworks/Python.framework/Versions/3.13/bin/python3.13 -I \
    ROOT/sekka_engine/__main__.py inspect
ROOT/Frameworks/Python.framework/Versions/3.13/bin/python3.13 -I \
    ROOT/sekka_engine/__main__.py execute --plan FILE --assets DIR \
    --installation UUID --owner SHORTNAME     3< password
```

- **Environment.** `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, `LC_ALL=C`, and the
  runtime's own `DYLD_FRAMEWORK_PATH=ROOT/Frameworks` and
  `DYLD_LIBRARY_PATH=ROOT/Frameworks/Python.framework/Versions/Current/lib`,
  as Asahi's `install.sh` sets them. The engine then replaces its environment
  with `PATH`, `LC_ALL=C`, `LANG=C` and `SSL_CERT_FILE` naming the runtime's CA
  bundle; nothing else is read from it.
- **Why not `-m sekka_engine`.** Under `-I` the package would have to be on the
  runtime's own `sys.path`, which means writing into
  `Python.framework/Versions/3.13/lib/python3.13/site-packages`. That
  directory is sealed by the framework's code signature (its `CodeResources`
  lists `site-packages/README.txt`), so the archive leaves it alone. `python3
  -I -m sekka_engine …` runs the same code wherever the package is importable,
  as in the tests.
- **Arguments.** Exactly these, each once. `--installation` is the lower-case
  UUID the helper's journal uses for this installation; the engine writes it
  into the first-boot handoff and the Recovery configuration. `--owner` is the
  macOS short name whose password the helper verified. There is no option for
  simulation, another installer, profile, command or path; the tests inject
  their fakes into the `Engine` class, which no argument or variable reaches.
- **Descriptors.** Standard output is the transcript: one canonical JSON
  message per line, numbered from 1. Standard input carries the helper's
  acknowledgements. Standard error is a diagnostic log. In `execute`, file
  descriptor 3 carries the owner's password, read once to end of file and
  closed; it is kept in a `bytearray` only until `bless` has it, then
  overwritten. (Python may hold other copies, such as the bytes handed to
  `bless`'s standard input; no complete erasure is claimed.) Before anything
  else runs, the engine takes private copies of descriptors 0 and 1 and points
  them at `/dev/null` and standard error, so neither Asahi's progress printing
  nor any child process can write into the transcript or read an
  acknowledgement.
- **Acknowledgements.** After each `intent` with sequence number N, the engine
  reads exactly `{"ack":N}` and a newline. Anything else, or end of input,
  stops it: it writes a `failure` for that stage (`not-acknowledged`) and a
  failed `completion`, and the stage never starts.
- **Exit status.** 0 after `inspected` or `waiting-for-recovery`, 1 after
  `failed`, 2 for a usage error (nothing is written to standard output).
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
`get_partitions`) and `osenum.OSEnum` (each container's systems). It runs
unprivileged, before the app asks for a password:

- volumes are mounted read-only (`diskutil mount readOnly`) and unmounted
  again; nothing is written;
- what only root can read is reported in a `log` line rather than guessed:
  without `bputil` the boot mode is taken from the booted volume group, and a
  stub's state is `unknown`.

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
- **other-linux**: a stub with Asahi's marks (m1n1 version, `Finish
  Installation.app`, an ESP with `asahi/`).
- **unknown**: any other stub, an APFS container with no system (blocking
  `partial-installation` if it is named Sekka, otherwise a non-blocking
  `unrecognized-layout`), and EFI or Linux partitions no stub claims.

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
  `admin_users`);
- Asahi's `choose_ipsw` rule, without prompts, gives an OS firmware the
  profile lists (14.8.3).

The assets directory holds exactly:

| File | Checked |
| --- | --- |
| `payload.zip` | only `esp/…` and `root.img`; no links, devices, encryption or case-twins; every ESP file's SHA-256 is the manifest's; `root.img`'s size is the manifest's; the boot entry has each placeholder once |
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
| partitions | Asahi's `OSInstaller.partition_disk` from the template: the FAT ESP (`newfs_msdos -I 0x5ecca5e1`) and the root, sizes and places checked | esp-partition, esp-offset, esp-size, esp-volume-id, root-partition, root-offset, root-size | inspected |
| firmware | Asahi's `collect_firmware` into a `FWPackage`, copied to the ESP's `vendorfw/`; unmounted, mounted, read back and hashed | firmware-files, firmware-cpio, firmware-cpio-size, firmware-inventory | read-back |
| esp | the payload's `esp/` tree (Asahi's `extract_tree`, CRC-checked); the boot entry's `@SEKKA_ROOT_PARTUUID@`/`@SEKKA_ESP_PARTUUID@` filled with the real GPT UUIDs; `sekka/install.json`; Asahi's `asahi/` installer data; AppleDouble files removed; unmounted, mounted, every file read back and compared with what was written, nothing extra | esp-inventory, esp-files, esp-bytes, boot-entry, kernel-arguments, handoff, installation, setup-hints | read-back |
| root | `root.img` streamed from the package to `/dev/rDISKsN`, hashed as read, flushed (`F_FULLFSYNC`, else `DKIOCSYNCHRONIZECACHE`, else `fsync`), then read back from the raw device and hashed; both must be the manifest's digest | root-partition, root-image-size, root-image, root-read-back, flush | read-back |
| recovery | `Sekka Recovery.app` from the checked zip, staged and renamed into place on the stub's System volume; `Sekka/InstallData/` with `recovery.json`, `boot.bin` (Asahi's `m1n1.build` of `boot/m1n1.bin` with `chosen.asahi,efi-system-partition` and `chainload=ESP;m1n1/boot.bin`, as `osinstall.install` builds it) and the firmware baseline under `firmware/`; `.IAPhysicalMedia` from Asahi's plist with `AppName` `Sekka Recovery.app`; unmounted, mounted, read back | recovery-app, recovery-app-files, recovery-config, boot-object, boot-object-size, firmware-baseline, iaphysicalmedia | read-back |
| startup-disk | `bless --setBoot --device /dev/SYSTEM --user OWNER --stdinpass`, as Asahi runs it; then the selection is read back twice (`bless --getBoot` → `diskutil info` → APFSVolumeGroupID, and Asahi's `nvram boot-volume` parse) and both must name the stub's group; then Asahi's `prepare_for_step2` hides SystemVersion so the stub opens Recovery | startup-volume-group, bless-boot-device, nvram-boot-volume-group, recovery-app | policy-read-back |

Setup hints come from the owner's own preferences: `AppleLocale` to a glibc
locale, the current keyboard layout to an XKB layout from a fixed table
(layouts that need a variant, such as Dvorak, are left out),
`/etc/localtime`'s zone, `scutil --get ComputerName`, and the owner's full name
from `AdminUserRecoveryInfo.plist`. Any of them may be absent.

Asahi's `step2.sh` flow is not installed. The Recovery app takes over what it
did: check that it runs in the stub's paired One True Recovery, `bputil -nc`,
`kmutil configure-boot -c Sekka/InstallData/boot.bin --raw --entry-point 2048
--lowest-virtual-address 0`, then restore `SystemVersion.plist` and rename
`.IAPhysicalMedia` to `IAPhysicalMedia-disabled.plist`.

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

What the bytes' authenticity rests on is what Asahi's rests on, no more:

- an HTTPS connection to the host in the URL, verified against the CA bundle in
  the pinned runtime (`SSL_CERT_FILE`);
- the URL itself, pinned in Asahi's `main.py` inside the SHA-256-pinned
  tarball;
- each member's zip CRC-32, checked as it is read in full (corruption, not
  forgery).

Neither Asahi nor the adapter checks a cryptographic digest of Apple's bytes.
The Mac's boot chain verifies the signed boot objects it later loads; it does
not cover the firmware copied out for Linux. The checkpoint records the
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
renumbering after a new partition is modeled, as diskutil does it. Linux
can't run libSystem's `fsctl`, LZFSE compression or the firmware collectors on
real Apple firmware, so the tests replace those, and serve a synthetic 14.8.3
update from memory through Asahi's URLCache class.

Scenarios: a J514S as it was before Sekka (sizes from its GPT), with macOS
already shrunk, with Sekka installed (the reference machine's layout today),
with an interrupted stub, with unknown systems, and Omarchy's recorded M1 Pro
with Asahi installed (MIT fixtures, see their NOTICE). They cover:

- inspection of each, unprivileged and as root; issues for battery, FileVault
  converting, snapshots, missing resize limits, a second internal disk,
  untested firmware and another board; inspection never changes the fake;
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
- Recovery-zip attacks (traversal, absolute paths, links, devices, resource
  forks, case twins, backslashes, encryption, corruption, missing parts), and
  payload, template and manifest substitutions bound into a plan;
- canonical JSON's rules, and vectors: `tests/vectors` holds inspections,
  plans, a handoff, a Recovery configuration, a string-escaping document and
  six transcripts. Python must write `expected.json`'s bytes and digests,
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
nix build --impure --expr 'let pkgs = (builtins.getFlake (toString ./.)).inputs.nixpkgs.legacyPackages.aarch64-linux; in (import ./installer/genkan/macos/engine/default.nix { inherit pkgs; }).archive'
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
- The helper's `RetryStartupDisk` has no engine mode yet.
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
