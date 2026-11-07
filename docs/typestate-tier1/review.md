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

## Open findings

Most severe first. "Confirmed" means the path was traced in the code;
"plausible" needs a build or a run to be sure.

### 1. The catch-all match ban is enforced only in Genkan's backend

Confirmed. The typestate modules of all five components carry
`#![deny(clippy::wildcard_enum_match_arm)]`, and `docs/typestate-tier1.md`
says that a `_ =>` hiding a new state fails Clippy. But only the Genkan and
Yukimi helpers' Nix checks run Clippy, and no workflow does. So the deny is
never enforced for Genkan's window, Kakehashi, Hako, Tansu or aizome-sys.

The matches that matter most in Genkan's window aren't under the deny either.
`installing()` (`installer/genkan/ui/src/app.rs:823`) ends with `_ => false`,
and it decides whether the window may close. A state added later that writes
the disk would count as not installing. `matches!` and `let … else` in
`flow.rs` aren't linted at all.

Run the lint, alone like the program ban (`-A clippy::all -D
clippy::wildcard_enum_match_arm`), in each component's Nix check. Then put the
window's matches on the flow under it.

### 2. Genkan can make a disk review without the erase phrase

Confirmed. `disk::Reviewed::of` is public (`installer/genkan/rust/src/disk.rs:252`),
and so are `RawRequest`'s fields and `RawRequest::parse`
(`installer/genkan/rust/src/plan.rs:17`, `:179`). Together they lead to
`lock()` and `Layout::write` with no `ConfirmedInstall`, which contradicts
the comment above `Reviewed`. `Filesystem::format` went from `pub(crate)` to
`pub` (`installer/genkan/rust/src/filesystem.rs:70`), and it takes a `Device`
from an unlocked `revalidate()`. The code the installer runs doesn't misuse
either.

Make `lock()` (or a write-only `of`) take `&ConfirmedInstall`, and return
`format` to `pub(crate)`.

### 3. Yukimi's typed programs leave a way to run anything as root

Confirmed. `Program::SYSTEMD_RUN` is public to the helper's binary
(`apps/yukimi/yukimi-helper/src/process.rs:91`), and `command()` returns a
plain `Command`. So `command(Program::SYSTEMD_RUN).args(["--wait", "--pipe",
"nix-store", "--gc"])` collects garbage outside `BootMenu`, and the same goes
for `nix-channel --update` outside a restore point. `nix(Nix::Eval)` with
`allow-unsafe-native-code-during-evaluation` is another way. The doc lists
only free-form `nix build` and `systemctl` arguments.

The ban also stops at the helper's call sites. `yukimi_system` functions that
start programs themselves get past it: `nix::setting`, the completion checks,
and #270's `reclaim::estimate`, which `delete` calls as root. That one runs
`nix-store --gc --print-roots` (`apps/yukimi/crates/yukimi-system/src/nix.rs:58`)
without the helper's environment clearing or stop tracking. It only reads.

Give `systemd-run` a typed step that names the unit's program, and ban, or
route through the helper, the `yukimi_system` functions that start programs.

### 4. Kakehashi's KTL-03 fix can be bypassed

Confirmed. With `with_typed(pub)` (`apps/kakehashi/core/src/job.rs:398`) and
a public `Authorizing::exited(Ran)` (`:429`), a job can end as "not touched"
without reading the helper's reports, which is KTL-03. The guarantee claimed
at `job.rs:107` holds only for `Job::handle`. The window doesn't do this.
Make `exited` take an `Exit`, or make these methods and `with_typed`
crate-private.

### 5. On Windows, Kakehashi can report "Not written" for a stick it wiped

Confirmed, and older than this branch, but the window now trusts the flag.
`open()` sends `IOCTL_DISK_DELETE_DRIVE_LAYOUT`
(`apps/kakehashi/write/src/platform/windows.rs:63`) before the size check,
the second `still_there`, the fit check and the Stop check. `touched` is set
only after them (`apps/kakehashi/write/src/lib.rs:187`). Pressing Stop while
`open` waits on volume locks leaves a stick with no partition table and no
warning. The deletion also comes before the check after opening, so a stick
swapped just before `open` loses its layout. Set `touched` before `open` on
Windows, or delete the layout after the second check.

### 6. A Hako undo can keep a step that points at another item

Confirmed. After restoring, undoing Move to Trash finds what's left in the
Trash by path (`desktop/aizome/apps/hako/src/fsops/undo.rs:572`). One Move to
Trash takes `/d1/a` (as `files/a`) and `/d2/a` (as `files/a 2`), and a new
`/d2/a` appears. ⌘Z restores the first and forgets its info. The second's
conflict is answered Replace, so the new `/d2/a` goes to the Trash, under the
name just freed. The step is then kept as still holding the first item,
which is now the displaced one. The next ⌘Z puts back an item the person
never trashed, and Skip or Cancel leaves the step at the top of Undo. Find
the remaining items by the entries `restore` reported, not by path.

### 7. The shared crate lets a cloneable brand make reviews

Confirmed in the crate, not reachable today. `Confirmed::brand()`
(`desktop/aizome/crates/aizome-typestate/src/confirm.rs:156`) returns `&B`.
With a brand that derives `Clone`, as Genkan's `DiskReview` does,
`Reviewed::new(any_target, confirmed.brand().clone())` makes a review of
another target. Genkan keeps its raw `Confirmed` wrapped, so nothing reaches
it. Nothing calls `brand()`, so remove it.

## Lower severity

**Kakehashi**
- `platform::sys::ioctl` is a public safe function that writes raw bytes into
  any `O` (`apps/kakehashi/core/src/platform/windows.rs:68`, re-exported at
  `platform/mod.rs:44`). It was private before. Make it `unsafe`, or bound
  `O` to plain data.
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
- Clean Up without an age now refuses to collect garbage when
  `switch-to-configuration boot` fails (`yukimi-helper/src/clean.rs:114`),
  for example with an unmounted ESP. Nothing was deleted, so YUK-11 doesn't
  need that. Builds that run out of room tell the person to free space in
  Storage, which then refuses too.
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
- **aizome-sys and Tansu:** the `Queued` brand isn't `Clone`, and nothing
  reaches `brand()`; renumbered MBR partitions and remade sibling partitions
  behave correctly.
- **Yukimi:** the clean-up record is synced before deleting; the helper
  unwinds on panic, so its fallbacks run, and they don't panic themselves.
- **Hako:** nothing traced deletes a displaced item; symlinks aren't
  followed.
- **Doctests:** each `compile_fail` traced fails for its stated code, and
  `nix/checked-doctests.sh` checks that rustdoc is checking codes.
