# Review of the tier 1 typestate work

A review of PR #290 on 2026-11-06, for its author. Six reviewers each read one
component (Genkan's backend, Genkan's window, Kakehashi, aizome-sys with
Tansu, Yukimi, Hako), and the shared crate was read in full. The code was
read, not run, except where an entry says otherwise. The findings ranked
highest were checked again against the code before they were written down.
Line numbers are for the commit that adds this file, on main at `d866289c`.

## Fixed on this branch

- **Rebased onto #270.** #270 added `yukimi-helper delete`, which deleted
  generations, rewrote the boot menu and collected garbage outside this
  branch's typed steps, wrote no record a kill could be recovered from,
  started its programs by name, and called `statvfs` from the helper. It now
  goes through `clean::Generations::delete`, `Deleted::update_boot_menu` and
  `BootMenu::collect_garbage`, writes the same `clean-up.json` record as
  Clean Up, and measures free space through `yukimi_system::sys`.
- **Genkan's program ban checked nothing in Nix.** The helper's source set
  left out `rust/clippy.toml`, so Clippy banned no method and passed. The set
  now includes it, and the check refuses to run without it.
- **A Tansu resize could shrink a partition under its filesystem.** Each
  step's confirmation held the partition to its place and filesystem but not
  its size. So after another tool grew a partition that had a grow queued,
  the run took the grow path from the old size and moved the end back. A
  resize's confirmation is now made with `Confirm::resize`, which holds the
  partition to the size it was queued at or the size chosen. Tansu's plan
  checks the same confirmation without spending it, so the pending resize is
  dropped and Apply refuses it.
- **The shared crate let a cloneable brand make reviews.**
  `Confirmed::brand()` returned `&B`. With a brand that derives `Clone`, as
  Genkan's `DiskReview` does, `Reviewed::new(any_target,
  confirmed.brand().clone())` made a review of another target. Genkan keeps
  its raw `Confirmed` wrapped, so nothing reached it, and nothing called it.
  It is removed, and a doctest on `Confirmed` holds that the call doesn't
  compile.
- **Clean Up without an age refused to collect garbage when the boot menu
  couldn't be written.** With `switch-to-configuration boot` failing, for
  example on a full or unmounted ESP, it freed nothing, though it had
  deleted nothing and YUK-11 didn't need that. Builds that run out of room
  tell the person to free space in Storage, which then refused too. It now
  collects and says the menu wasn't updated, as long as no clean-up left
  its record: every deletion so far was then followed by a written menu.
  With a record left it still refuses, and so do Clean Up with an age and
  deleting generations.

### Follow-up fixes, 2026-11-15

- **Hako Undo could retain another item's Trash name.** `Trashed` now owns
  a private receipt identity captured through the claimed info file's open
  handle before the item moves. Its paths are read-only. Undo resolves both
  its initial targets and its remaining items through that receipt, so a
  replacement reusing the name does not become the old undo target. Tests
  cover reuse during restore, reuse before Undo, redo, and retrying a partial
  folder restore. Identity still has to be checked against the filesystem at
  runtime; the private fields prevent forging or retargeting the receipt.
- **Windows could erase the layout while still reporting untouched.** Opening
  now only locks, dismounts and queries the device. `Opened::check` binds a fresh
  reading to that opened drive through `Reviewed`/`Confirmed`, checks capacity
  and cancellation, and returns `Ready`. Only `Ready::begin` exposes `Writing`;
  it records possible damage before the layout-deletion IOCTL, including when
  that call fails. Compile-fail tests reject bypassing the checks or omitting
  the damage flag. Temporary-file tests exercise rejection and write failures.
- **The exported generic IOCTL wrapper accepted invalid Rust output types.**
  It has been removed. Geometry and length queries own their fixed Windows
  output types, and bufferless destructive controls accept a private enum in
  the helper's write crate. The remaining raw buffer helper is private and
  unsafe, with a safety argument at each call. Windows compile-fail tests
  reject the old generic/control imports and arbitrary output buffers.

The follow-up checks ran on `wx-workstation`: Hako's 136 unit/integration
tests and 28 doctests; Kakehashi's package, elevation and disposable-VM checks;
Windows temporary-file regressions and 28 doctests under Wine; and Clippy
for the Linux, Windows and macOS Kakehashi core/write/helper targets (macOS
was cross-checked, not executed). Compile-fail error codes were checked.
Hako's Clippy pass excludes `deprecated`: the pinned nightly warns about
the existing `AtomicUsize::fetch_update` in `aizome-kit`. These are component
checks, not new ISO or physical-disk qualification.

### Remaining API and lint findings resolved, 2026-11-15

The four remaining findings were reproduced again against `e9174ebe` before
this follow-up. The first three compiled through public APIs; the Kakehashi
probe also reproduced the incorrect untouched result with a temporary file.

- **The catch-all match ban now runs in the package checks.** Aizome checks
  Hako, Tansu and aizome-sys; Kakehashi checks core; Genkan checks both the
  backend and the window. `checked-state-matches.sh` proves the compiler
  accepts an exhaustive match and rejects a wildcard with the intended lint.
  Genkan's `installing()` names every flow state under the same deny.
  `matches!` and deliberately ignored, explicitly named states remain outside
  what this lint can prove.
- **Genkan write permission requires erase confirmation.** Read-only
  `Reviewed`/`Confirmed` values cannot lock a disk or yield a formatting
  device. `Authorized::of(&ConfirmedInstall)` binds permission to the confirmed
  plan's disk; consuming `lock()` performs the existing live identity and busy
  checks. Only `Locked` yields partitions. Regression tests check the matching
  phrase and identity; compile-fail examples reject the old review-to-write
  paths. The production storage VM fixture now obtains this same authority.
- **Yukimi's general service launcher is private.** The helper's client and
  worker share the closed `transaction::Action` enum. The launcher fixes the
  helper, worker entry point, operation and recovery hook; it executes the
  command internally. No public `SYSTEMD_RUN` constant or mutable launcher
  command escapes. Tests check all four operations and arguments resembling
  launcher options; compile-fail tests reject the old escape and raw client
  arguments. The free-form Nix and systemctl interfaces remain the limitations
  listed below, not a claim of a general process sandbox.
- **Kakehashi completion cannot bypass the final report drain.** The state
  visitor and phase transitions are private. Public completion takes the
  `Exit` made by `ProgressFile::finish`. Compile-fail tests independently check
  visitor and transition privacy, and temporary-file tests cover unread first
  and final reports. A late `NotStarted` event also reports possible damage
  instead of claiming the drive was untouched.

These fixes passed their checks on `wx-workstation`: the full Aizome,
Genkan UI/helper, Yukimi helper and Kakehashi packages; checked-code doctests
and the three original external bypass probes (now rejected); Genkan and
Yukimi namespace tests; Kakehashi elevation tests; and all 12 encrypted-storage
cases with BIOS/UEFI boots. Windows ran 60 core/write tests and 30 doctests
under Wine; Windows and macOS core/write/helper Clippy checks passed (macOS
was cross-compiled only). Aizome's first attempt exposed integration-test
metadata in its dependency-cache guard. Linting production libraries and
binaries fixed that without weakening the guard or removing integration tests.
These are component checks, with no new ISO or physical-hardware qualification.

## Lower severity

**Kakehashi**
- On Linux and Windows, the image file isn't checked again until after the
  drive is erased: `confirm` (`core/src/choice.rs:342`) compares the path and
  stored checksum, not the file. `Plan::with_image` (`core/src/write.rs:94`)
  is public on every platform, though only macOS needs it. macOS re-hashes
  its staged copy, so it is covered.
- The helper's re-check (`write/src/recheck.rs:24`) applies less than the
  window's exclusions (holds the image, not USB or removable, over 2 TB). The
  pkexec prompt is the real gate. This predates the branch.
- Windows reads no USB port (`core/src/platform/windows.rs:242`), and
  `PhysicalDriveN` numbers are reused. Identical sticks without serials are
  told apart in no port there, not only the same one.
- If the helper can't write its final `Finished` line (`helper/src/main.rs:136`),
  a written and verified drive shows as failed and partly written.
- The `compile_fail` doctest at `core/src/write.rs:15` belongs to
  `kakehashi-core`, so it says nothing about the window's dependencies. The
  real guard (`app/src/elevate.rs:112`) searches the window's manifest for
  the text `kakehashi-write`, which `writer = { path = "../write" }` gets
  past.

**Yukimi**
- Free-form `nix build`, `nix eval` (including unsafe native evaluation
  options), and `systemctl` arguments are not a complete process sandbox.
  Shared `yukimi_system` read-only helpers also start programs outside the
  privileged helper's command wrapper, including `reclaim::estimate`'s
  `nix-store --gc --print-roots`. The service-launcher fix does not change
  those interfaces.

- After a power cut, recovery waits for the next Yukimi operation
  (`yukimi-helper/src/main.rs:1041`). A persistent `nix.gc` timer, Nix's
  `min-free` or a manual collection can run first, while the menu still lists
  deleted generations. A boot-time unit running `__recover` would close it.
  Plausible.
- YUK-02 takes the same boot id as the same boot
  (`crates/yukimi-system/src/completion.rs:143`). `systemctl soft-reboot` and
  restarting an LXC or Incus container keep it, so a failed activation keeps
  reporting "did not finish" after a soft reboot into a repaired system.
  This errs on the safe side. Plausible.
- The four repro scripts are `#[ignore]` (`yukimi-helper/tests/namespace.rs:25`),
  so the Nix check runs none of them. Scenario D of `repro_yuk_11.sh` runs
  `__recover` by hand (`:175`), so the `ExecStopPost=` wiring
  (`yukimi-helper/src/transaction.rs:78`) is untested.
- On runnymede, `a_later_boot_supersedes_a_failed_activation` fails before and
  after the rebase: "The active system's Aizome compositor could not be
  verified." It reads the host's running system, so its result depends on
  the host. The PR records the four namespace tests as passing.
- A stop during `delete` is deferred with Clean Up's words, "Deleting the old
  generations and updating the boot menu" (`yukimi-helper/src/clean.rs:51`).
- Yukimi's Nix check has no guard like Genkan's against its `clippy.toml`
  leaving the source set. Today the whole `yukimi-helper` directory is in it.

**Genkan**
- A failure after the confirmation is sent but before step 1 is a dead end.
  `lock()` can refuse when the disk was mounted during the long preparation.
  The window shows "Installation Failed" with no Back, the passwords already
  forgotten (`ui/src/flow.rs:519`). Only failures while the confirmation is
  queued return to the review.
- A queued confirmation can't be withdrawn: the Install page offers no Back,
  and closing is refused with "cannot be closed while it is writing to the
  disk", which isn't true while preparation runs. Main behaves the same.
- GEN-06's helper-side race is narrowed, not closed: `watch`
  (`rust/src/session.rs:184`) classifies a line by the phase when it is
  read. Only a client that doesn't use `Ready` can reach it.
- The disk is written by path (`rust/src/disk.rs:472-479`); the locked file
  holds only the lock. The open descriptor likely keeps the name from passing
  to another disk, but that's the kernel's doing, not the types'.
- `CURL`, `NIX` and `NIXOS_INSTALL` are public (`rust/src/storage.rs:58`), and
  `process::command` returns a plain `Command` (`rust/src/process.rs:107`),
  so a general-purpose program can write a disk without a proof. The doc
  lists only "writing the device node with ordinary file APIs".
- Two serial-less virtio disks of one size, swapped in one slot, look the
  same. Plausible; the doc's "Hardware that looks identical" doesn't name
  Genkan.
- The fake `lsblk` in `rust/tests/revalidation.rs:65` ignores its device
  argument. `ui/tests/early_confirmation.rs:122` sends `Prepared` through the
  sample link, so the real queued send and its error path aren't tested.
- `docs/typestate-tier1.md` says the window binaries forbid the program ban.
  Genkan's window doesn't, and calls `Command::new("systemctl")`. It is
  unprivileged, so only the doc is wrong.

**aizome-sys and Tansu**
- `Op::agreement()` (`apps/tansu/src/plan.rs:258`) is public, and the backend
  takes its parameters (format spec, sizes, offsets, flags) beside the
  token. Any Tansu module can confirm a queued operation's device and pass
  other parameters. Each step makes its token as it starts, so
  `CONFIRMATION_LIFETIME` (`crates/aizome-sys/src/disks/confirm.rs:60`)
  never applies to Tansu.
- Restarting udisksd may drop every pending operation, since a drive's
  identity includes UDisks's detection times. Plausible.
- A format whose mkfs fails after UDisks wiped the old signatures may be
  dropped from the plan as "gone or has changed", losing the error.
  Plausible.
- The SYA-13 repro (`crates/aizome-sys/src/disks/repro_sya_13.rs:53`) gives
  the two sticks different table kinds as well as serials, so it would pass
  if the drive were ignored. `apps/tansu/tests/repro_tsh_25.rs` swaps
  sticks by UUIDs, not detection times.

**Hako**
- The inode-reuse test (`fsops/trash.rs:986`) never reaches the change-time
  comparison: the later item is still in the can, which decides first.
- Replace compares device and inode (`fsops/replace.rs:50`). On filesystems
  without stable inode numbers (vfat, exfat, CIFS with `noserverino`, FUSE
  without `use_ino`), a sheet left open can refuse spuriously. It refuses
  rather than loses anything. Plausible.
- If `symlink_metadata` of the set-aside item fails after the rename,
  `Changed(None)` (`fsops/replace.rs:127`) leaves it hidden as
  `.name.hako-aside-*` with no report.
- The note that a step left Undo is shown only if the window that started
  the undo still exists (`ui/app.rs:832`). An undo whose items are all gone
  is filed done without the note.
- Undoing Move to Trash now looks each item up twice, and each lookup reads
  mountinfo and lists the whole can. `forget` reads the cans per entry.
- Older than this branch: `erase` deletes whatever is at a stale entry's
  path, and undoing a merged Copy or Put Back trashes the whole merge target.

## Checked and found sound

- **Genkan:** GEN-07's room check comes before any write; the lock checks
  the locked descriptor's device against the fresh reading; #305's
  `build_on_target` arm is intact; the swap size enum is matched everywhere.
  The window's single `Generation`s are never replaced, so no ticket is
  reused.
- **Kakehashi:** KTL-04 on Linux and macOS; `Plan` has private fields and no
  `Deserialize`, `Default` or `Clone`; the job directory outlives the
  reports' last read.
- **aizome-sys and Tansu:** the `Queued` brand isn't `Clone`; renumbered MBR
  partitions and remade sibling partitions behave correctly.
- **Yukimi:** the clean-up record is synced before deleting; the helper
  unwinds on panic, so its fallbacks run, and they don't panic themselves.
- **Hako:** nothing traced deletes a displaced item; symlinks aren't
  followed.
- **Doctests:** each `compile_fail` traced fails for its stated code, and
  `nix/checked-doctests.sh` checks that rustdoc is checking codes.
