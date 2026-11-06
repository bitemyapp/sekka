# Genkan's installation record and bootloader retry

Phase 7 of the [Apple Silicon installer plan](../apple-silicon-installer-plan.md#7-bring-durable-evidence-and-safe-retry-to-genkan-on-pcs):
durable, secret-free evidence of what Genkan did to a PC's disk, and one
bounded retry. The code is in Genkan's backend (`installer/genkan/rust`):
`record.rs` (format, chain, durable writes, states, reports), `retry.rs`
(evidence read from the target, the recheck, the retry loop), and the stages
in `install.rs`; the protocol in `session.rs` and `helper.rs`; the window in
`ui/src/app.rs` and `ui/src/view/install.rs`. It follows the Mac contract's
principles (`installer/genkan/macos/crates/contract/src/record.rs`,
`state.rs`) without depending on that workspace.

This is component work. Nothing here has been through a VM installation or
an ISO yet; see [What is tested](#what-is-tested) and
[What still needs a VM or hardware](#what-still-needs-a-vm-or-hardware).

## Where records live

| Copy | Path | Lifetime |
| --- | --- | --- |
| Live | `/run/sekka-install/<installation>.jsonl` | Written from the start of preparation. Outlives the window and the helper; `/run` is memory, so lost on restart. |
| Installed | `/var/log/sekka-install/<installation>.jsonl` on the new root | Written from the moment storage completed until the flush. The durable evidence. |

`<installation>` is a random UUID per helper session. Both directories are
root's (0700, checked when they already exist; a link or a directory others
can write is refused), the files 0600. Each review that reaches the helper
opens a record; a review withdrawn before confirming ends `cancelled`.

The record is opened in `install::prepare` right after the installer lock
(so two helpers can't interleave), before anything else is prepared. It is
**mirrored to the target only after the storage stage completed**: the root
filesystem was created with the UUID chosen before erasure, that UUID read
back from its superblock, the filesystem mounted at Genkan's private mount
point, and the mount itself checked (`findmnt`). Before that point the target
is not known to be this installation's filesystem, and writing to it would
be writing to a disk whose state is uncertain.

From then on each entry is written to the live copy first, then to the
mirror. The mirror's last entry is the flush stage's intent: it is written,
then `sync --file-system` flushes it with everything else, then the target
is unmounted. The flush's completion is in the live copy only; an unmounted
filesystem can't be written to. A complete, chain-valid mirror ending in the
flush intent is itself the evidence that the flush reached the disk.

**Reconciliation**: the two copies must hold byte-identical entries as far
as the shorter goes, and the mirror may lag the live copy but never lead it
(`Record::reconcile`). Anything else is a conflict and neither copy is
trusted; the copy that says the most progress is never chosen for that
reason. After a restart only the mirror exists, and it is evidence only.

The existing timing record, `/var/log/calamares-nixos/install.json`, is
unchanged.

## Format

One JSON object per line, schema `sekka.genkan.record/1`:

```json
{"schema":"sekka.genkan.record/1","installation":"<uuid>","sequence":7,"previous":"<sha256 of line 6>","at":1793750400,"kind":"intent","stage":"files"}
```

| Field | Meaning |
| --- | --- |
| `sequence` | 1, 2, 3… with no gaps |
| `previous` | SHA-256 of the previous line's exact bytes (`null` for the first) |
| `at` | seconds since 1970 by the live clock; informational |
| `kind` | `opened`, `intent`, `completed`, `failed`, `confirmed`, `cancelled`, `refused` |
| `stage` | `preparation`, `storage`, `files`, `copy`, `bootloader`, `verification`, `flush` |
| `facts` | `[["name","value"], …]` from a closed registry (`record::FACTS`) |
| `verification` | `inspected`, `read-back`, `registered` (`none` is never accepted on completion) |
| `disk_changed`, `message` | on `failed`; `message` alone on `refused` |

Limits: 64 entries, 4 KiB per line, 64 KiB per record, failure messages cut
to 1 KiB (the window gets all of the message).

Parsing is explicit at every boundary. A line is deserialized into a wire
type of strings (`deny_unknown_fields`), every field is then parsed and
checked (UUIDs, digests, store paths, device paths, spans, numbers in their
canonical form, known fact names), the entry is re-serialized and must equal
the line byte for byte, and `Record::parse` checks the chain, the sequence,
that every entry names the same installation, and that the entries follow
Genkan's stage order (`observe`). `Record` and `Journal` are not
`Deserialize` (a compile-fail doctest says so), and `InstallPlan` stays
immutable and non-`Deserialize`. A hash chain has no secret key: it detects
torn, reordered, edited and spliced records and accidental damage, not a
forger with root. What stops a forged record from causing harm is that a
retry also compares the record with the plan held in memory and with the
real disk.

### Durable writes

`record::write_atomic` replaces a copy so that after a crash at any point it
holds either the previous contents or all of the new ones: a temporary
`.<name>.tmp` beside it (exclusive create, `O_NOFOLLOW`, 0600), `write_all`,
`fsync`, `rename` over the copy, `fsync` of the directory. A stale temporary
file from a crash is replaced. A failure before the rename leaves the copy as
it was and removes the temporary file.

After any failed write the journal writes nothing more: whether the rename
or the directory sync reached the disk isn't known. Because an intent is
written before the change it announces, a record that can't be written stops
the installation before that change. The file operations are a trait
(`record::Io`), so the tests make each one fail.

## Stages and their evidence

Intent is written before each stage; completion only after its result was
read back.

| Stage | Intent | Completion evidence | Claim |
| --- | --- | --- | --- |
| Preparation | — | system and derivation paths, deferred build, the live closure's digest/paths/bytes | inspected |
| (confirmation) | `confirmed`: digests of `configuration.nix`, `flake.nix`, `flake.lock`, `hardware-configuration.nix` | — | — |
| Storage | partition spans in MiB, the identities chosen before erasure | the same disk identity, the kernel's partition geometry, every superblock's identity (LUKS headers and the filesystems in their mappings too), what is mounted at the root and `/boot` | inspected |
| Files | — | the four files read back and their digests compared; account written; Wi-Fi profile count | read-back |
| Copy | system, deferred | the target store's closure (digest of each path's NAR hash and size); equal to the live closure when it was prepared in memory | registered |
| Bootloader | attempt number | the system profile followed inside the target, the media's lock, systemd-boot's `loader/loader.conf` or GRUB's `grub/grub.cfg` | inspected |
| Verification | — | both copies read back equal to what was written; the full recheck below; the bootloader's evidence again | read-back |
| Flush | (last entry of the mirror) | `sync`, unmount, mappings closed, nothing mounted at the target any more | inspected |

Two refusals keep the record truthful about an untouched disk. A
confirmation that is malformed or isn't this plan's phrase is recorded as
`refused` after `prepared`. And before the storage intent, the helper claims
the disk: it opens it, takes its exclusive lock and checks again that it is
the reviewed disk, present, unchanged and not mounted (`claim_disk`). A disk
that can't be claimed (mounted while the phrase was typed, gone, changed,
locked) is recorded as `refused` after `confirmed`, with nothing written, so
the window says the disk is as it was. The intent is then written while the
lock is held, and the erase runs on the same locked device: the gap between
the check and the first write is one small append to `/run`.

The claims are distinct: *read-back* goes through the page cache before the
final flush; *registered* means the target's Nix database registers the
closure with the prepared NAR hashes, not that the bytes were hashed again;
nothing claims the system boots.

The configuration is recorded by digest only: the files name the person and
the computer. Facts are a closed registry; free text is limited to the
disk's model, serial number and WWN and to failure messages. Passwords,
password hashes, the disk passphrase, Wi-Fi secrets and SSH keys have nowhere
to go, and the tests check that a plan with all of them leaves none in its
record.

## What the window is told

After any failure once the installation was confirmed, the helper sends
`Event::Recovery { report }` before `Failed`:

| Field | Content |
| --- | --- |
| `message` | the failure in full |
| `changed` | what the record shows was done to the disk |
| `next` | the next safe action |
| `retry` | an offer, when the bootloader may be tried again |
| `support` | the record redacted for support |

| Record shows | Next action |
| --- | --- |
| Opened, preparing, prepared, confirmed, cancelled; refused or failed before any disk change | Review again: the disk is as it was |
| A stage started with no result (interrupted) | Start over: install from the start, erasing again |
| Storage, files, copy, verification or flush failed | Start over |
| Bootloader failed after storage (inspected), files (read-back) and copy (registered) completed, fewer than three attempts | **Try the bootloader again**, or start over |
| Bootloader failed three times, or a retry was refused | Start over |
| Verification completed, or the flush started or completed | Shut down and start the installed disk |

The Install page shows `changed` under "Installation did not complete", then
a "Next" group with `next`, the button **Try the Bootloader Again** (Genkan's
buttons are title case and it writes "bootloader" as one word; the plan's
"Try the boot loader again" in Genkan's voice) with "Attempt 2 of 3", and a
note that Copy Details includes the installation record. Copy Details copies
the failure, what changed, the next action and the redacted record. Without
a report (the helper died) the page keeps its earlier wording.

`CALAMARES_UI_PREVIEW=bootfail` shows the retry offer; `failed` now shows a
failed copy with the record's account. Add `bootfail` to the preview list in
DEVELOPERS.md when merging.

## Retry rules

Only the bootloader is repeated, and only within the same helper session:

1. The bootloader fails. The helper records `failed` (disk changed: yes),
   keeps the target mounted, its encryption mappings open and the installer
   lock held, discards anything the window sent before the offer, and sends
   a `Recovery` whose `retry` is `{stage: "bootloader", record: <digest of
   the last entry>, attempt, attempts}`. It then waits on stdin.
2. The window answers with one line, `{"retry":"bootloader","record":"<digest>"}`
   (`session::RetryRequest`). Closing stdin declines: the helper unmounts,
   closes the mappings, sends a final report and `Failed`, and exits.
3. The helper accepts the answer only if both fields equal the offer's. It
   reads both copies of the record back and requires them to be exactly what
   it wrote; a copy changed by anything else is kept as evidence (never
   written over) and nothing more is written. The record must still offer
   this retry.
4. `retry::recheck` compares the record with the plan in memory (disk
   identity digest including serial and WWN, firmware, filesystem,
   encryption, the confirmed digests, the partition layout, the system) and
   then with the real target: the disk's current identity, the kernel's
   partition geometry, each superblock's identity, the mounts, the four
   files' digests and the target store's closure digest.
5. Any failure is recorded as `refused`, the report says to start over, and
   nothing is offered again. Otherwise the helper records the next attempt's
   intent and runs `nixos-install` again; then verification and the flush
   follow as in a normal installation.

At most three attempts per installation (`record::BOOTLOADER_ATTEMPTS`).
`nixos-install --system` with the closure already present sets the system
profile and runs `switch-to-configuration boot` with the bootloader
installation forced; it doesn't partition, format or copy, which is why it
is the stage proven safe to repeat. Nix reuses the profile's last generation
when it already points at the same system, so a repeat shouldn't add a boot
entry; a VM run should confirm that.

Never repeated: preparation (nothing changed: review again), storage, files,
copy, verification, flush, and any stage whose intent has no result. A retry
isn't offered once the session has ended (the helper exited, the window
closed, the computer restarted): no new helper attaches to an old record.
`install` and `preflight` (no window) never offer one.

The helper's `session` reads stdin after the confirmation only to answer an
offer; the end of input there never cancels, as before. The VM fixture's
session closes stdin right after confirming, so an unexpected offer ends its
run instead of waiting.

## Redaction

The record is root's and stays on the person's own disk, so it keeps the
disk's serial number and WWN (to say "the same disk") and failure messages.
The support record (`Record::support`, what Copy Details copies) replaces
serial numbers and WWNs with the first 12 hex digits of their SHA-256, and
replaces the hostname, username, full name, Git name and email and GitHub
username (three characters or more) in messages with `[redacted]`. Its
lines no longer match their digests; its header names where the full record
is.

## What is tested

Run with the locked Cargo tests (this branch: aarch64-linux, Rust 1.99.1,
a `nix-shell` with glib/Wayland/fontconfig and Aizome's fonts):

- `record::tests` (18): a complete chain; every entry on disk before the
  journal returns; torn and truncated records; edited, reordered, dropped,
  spliced-in-from-another-installation, renamed, non-canonical, unknown-field,
  unknown-fact and future-version entries; a well-chained forgery that skips
  a stage, completes unchecked or cancels a changed disk; retry eligibility,
  attempt limits and checked-stage requirements; ENOSPC, a short write that
  writes nothing, a short write that completes, failed fsync, failed rename,
  failed directory fsync, a stale temporary file; the mirror lagging, leading
  and disagreeing; a copy changed on disk; redaction and the absence of
  secrets; real files' modes, a shared directory and links refused.
- `retry::tests` (12, one more ignored): retry on the same checked target;
  attempts running out; declining by closing; no offer without a window;
  refusals for another disk (serial), repartitioned, reformatted, remounted,
  a changed store, an edited configuration; requests naming another record or
  stage, malformed and confirmation-shaped answers; a record replaced on disk
  between offer and answer (refused, kept); records for another plan, layout,
  configuration or system; an incomplete record; encrypted BIOS evidence;
  both `nix path-info` JSON forms; a plan's opening record holds no secrets.
- `install::tests`: the `Recovery` event's shape; the profile followed inside
  the target, never out of it; missing bootloader configuration, wrong lock
  or system refused; a disk that can't be claimed is recorded as refused with
  nothing written ("Review again", no retry, no storage intent after it); a
  missing path and a regular file locked by another descriptor can't be
  claimed (no block device is touched); a refused confirmation is recorded.
- `session::tests`: one confirmation, then only retry lines; the two line
  kinds can't be confused. The helper's own test: lines sent before an offer
  don't answer it, and the end of input declines.
- `ui/tests/pages.rs`: every page including `bootfail` draws; a failed copy
  says what changed, offers no retry, can be closed and copies the redacted
  record; a failed bootloader offers to try again, can be closed while
  waiting, and once retried can't be closed, shows the check, and finishes;
  an offer is withdrawn when the helper ends; a disk that couldn't be
  claimed after confirming (a real record's report) is said to be as it
  was, never lost.
- The VM fixture (`installer/genkan/tests/vm-fixture`) builds, and its
  `verify` now requires exactly one record under `/var/log/sekka-install`,
  parsed by the same code, ending at the flush intent, whose storage
  `root-uuid` is the mounted root's UUID.
- Read-only on this host: `retry::tests::real_closures_parse` with
  `GENKAN_TEST_CLOSURE=<store path>` parses Determinate Nix 3.24's
  `nix path-info --json --recursive` the way the copy is checked.

## What still needs a VM or hardware

Not run for this change: the storage VM tests (`.#storage`,
`.#encrypted-storage`) and the installer VM matrix are x86_64-only, and this
work was done on an aarch64 Mac. Those runs must pass before merging,
because every installation now runs the target probes:

- `findmnt --json` sources for ext4, XFS, Btrfs (`/dev/vda2[/]`) and
  dm-crypt mappings (`/dev/mapper/genkan-…` or `/dev/dm-N`, compared after
  canonicalizing);
- `blkid --probe` on mounted filesystems and on open LUKS containers;
- `disk::discover` while the target is mounted (its identity, not its
  blocked state, is compared);
- `nix path-info --json --recursive --store local?root=…` on the target
  after `nix copy` and after an on-target build, matching the live store's
  closure exactly;
- the profile links and `loader/loader.conf` / `grub/grub.cfg` that
  `nixos-install` leaves, UEFI and BIOS, encrypted and not;
- the mirror under `/var/log/sekka-install` surviving activation and the
  first boot.

The PC qualification scope applies to any ISO with this change: at least one
normal UI installation per firmware, a disk-only boot, Aizome login and a
Yukimi update/rollback on the exact ISO.

### The interrupted-bootloader VM case

Not added to the fixtures (it needs a VM run to develop). Precisely:

1. In the disposable UEFI installer VM (`RESPIN_TEST_ONLY` disk), drive the
   packaged helper's `session` as `invoke_session` does, but keep stdin open
   after confirming.
2. On `Progress { step: 5 }`, make the ESP refuse writes:
   `mount -o remount,ro /run/calamares-target-*/boot`. Expect `Recovery`
   with `retry.attempt == 2` and the helper still running; the live record
   ends `intent bootloader` (attempt 1), `failed bootloader`.
3. Negative pass (fresh VM): before answering, change something —
   `mkswap -U <another uuid> /dev/vda3`, or append a byte to
   `/run/sekka-install/<id>.jsonl`, or answer with another digest. Expect
   `Recovery` without `retry`, `Failed`, a non-zero exit, nothing mounted
   under `/run/calamares-target-*`, the record ending `refused` (or, for the
   edited record, left exactly as edited).
4. Positive pass: `mount -o remount,rw` the ESP, write
   `{"retry":"bootloader","record":"<offered digest>"}` and a newline. Expect
   `Progress` "Checking the disk and the copied system…", then
   `Complete`. Boot the disk alone: Aizome's greeter appears, and
   `/var/log/sekka-install/<id>.jsonl` parses with 2 bootloader attempts and
   ends at the flush intent.
5. BIOS: the remount trick doesn't apply (GRUB writes `/boot/grub` on the
   root filesystem); use `chattr +i` on the target's `boot/grub` directory
   for the failing attempt, or a root-owned, `test_diagnostics`-gated
   setting that fails the first `nixos-install` once. That setting doesn't
   exist yet and would be test-only code in the helper.

The remount races `nixos-install`'s activation only in principle (it writes
the ESP seconds after the step starts); a VM run should confirm it fails at
the bootloader rather than earlier.
