# Tier 1 state machines as typestate

The implicit state machine catalog (PR #282, `docs/implicit-state-machines.md`)
found the machines behind Sekka's worst confirmed bugs. The crate evaluation in
the same PR (`docs/state-machine-crates.md`) chose hand-written typestate to
make them explicit. This change does that for the tier 1 machines: those whose
bugs can wipe the wrong disk, lose files, or leave a system that doesn't boot.
Every tier 1 bug is fixed. For each one, the code that made the mistake no
longer compiles, and a `compile_fail` doctest shows it.

## The machines and their bugs

| Component | Machine | Bug | Fixed | Mistake rejected at compile time |
|---|---|---|---|---|
| aizome-sys | SYA-13 destructive-operation confirmation | A confirmation for one stick held for another at the same path | yes | yes |
| Tansu | TSH-25 sheets holding segment indexes | After an unplug or swap, an open sheet retargeted another partition | yes | yes |
| Kakehashi | KTL-04 chosen drive | OK re-read the drive by name and could erase a swapped stick | yes | yes |
| Kakehashi | KTL-03 write job | A helper killed mid-write was reported as "Not written" | yes | yes |
| Genkan | GEN-03 disk identity | Revalidation failed when an unrelated disk changed | yes | yes, apart from a runtime fact (below) |
| Genkan | GEN-06 helper session | A confirmation sent during preparation was used as preparation ended | yes (hazards) | yes |
| Genkan | GEN-07 install transaction | The deferred route skipped the root-space check, so the install failed after erasing | yes | yes |
| Genkan | GEN-34 review and confirmation (GUI) | A preparation failure after an early confirmation was reported as a failed install | yes | yes |
| Genkan | GEN-28 disk scan and memory read | A failed memory read showed "Reading installed memory…" forever | yes | yes, apart from collapsing two errors (below) |
| Yukimi | YUK-09 change transaction | A failed or stopped change left root's channels advanced | yes | yes |
| Yukimi | YUK-11 rollback and clean-up | Stop during garbage collection left boot entries for deleted generations | yes | yes |
| Yukimi | YUK-02 update receipt | A stale "activating" receipt outlived a repair and a reboot | yes | yes |
| Yukimi | YUK-10 saved configurations | A reused generation number kept another generation's configuration | yes | yes |
| Hako | HAKO-05 replace-aside | Put Back with Replace permanently deleted the newer item | yes | yes |
| Hako | HAKO-06 cross-filesystem move | Sources under a folder that couldn't be synced were deleted | yes | yes |
| Hako | HAKO-08 Trash item protocol | A partly restored folder lost its Trash info | yes | yes |
| Hako | HAKO-09 undo history | A failed undo dropped its step | yes | yes |

GEN-06 had no catalogued bug, only High-rated hazards, and its machine was
reified with the install transaction.

## How "caught at compile time" was shown

For each bug:

1. **A `compile_fail` doctest.** It writes the bug's own mistake against the
   new API, for example:
   - planning the drive found by name at OK;
   - erasing `request.disk().path`;
   - choosing deletion for a replaced item;
   - filing an undo's result without checking it.

   Each probe names the error it expects (` ```compile_fail,E0308 `), and
   the first error is the guarantee itself, not a typo or a missing import.
   Beside it, a passing twin makes the legal call. The branch has about 90
   such probes.
2. **Checked error codes.** Stable rustdoc doesn't check those codes, so
   they were checked with `RUSTC_BOOTSTRAP=1 cargo test --doc`, which does.
   `DEVELOPERS.md` says to run them that way, and the Nix checks now do
   (`nix/checked-doctests.sh`).
3. **A regression test.** The bug's reproduction from the catalog became its
   regression test. It fails on main and passes here.
4. **An independent review.** A second agent tried to write each mistake
   every other way a maintainer might:
   - another constructor, `Default`, `Clone` or `Deserialize`;
   - a kept proof, or a proof for another target;
   - the old code verbatim;
   - an external crate.

   Every route that compiled was closed, or is listed below, and was then
   re-verified.

## The shared crate

`desktop/aizome/crates/aizome-typestate` has no dependencies and no workspace
inheritance, so Genkan, Kakehashi and Yukimi take it by path from their own
workspaces. Their Nix sources add only that directory. Everything in it is
`#[inline]` or generic, so a program compiles only what it uses.

| Piece | What it gives | Used by |
|---|---|---|
| `machine!` | One struct per state over a borrowed context, with private fields. Also the stored state (one byte when states carry no data), the typed state and `with_typed`, whose `for<'s>` keeps a state from moving between machines. Opt-in parts: `branches`, `stored(Copy)`, `access(...)`, `new(...)`, `with_typed(...)` | Kakehashi job, Hako undo, Genkan review flow and memory read |
| `Generation`, `Ticket`, `Answer`, `Fresh` | An answer to a superseded request is dropped, and a transition can take only an answer proven fresh, with the proof bound to that answer | Genkan, Hako, Kakehashi |
| `Reviewed<T, B>`, `Confirmed<T, B>`, `SameTarget<F>` | A destructive step acts on the target the person or record reviewed, re-identified when it acts (`confirm`, `recheck`). The brand `B` lets only the reviewing code make a review | aizome-sys (and through it Tansu), Kakehashi, Genkan, Hako; Yukimi uses `SameTarget` alone, for generations |
| `Unsettled<T, F>` | A value spent through a named step, or by its fallback when dropped. It covers the paths types can't close, such as an early return or a panic | Yukimi, Hako |

The crate's API grew from the five components' requests. Two requests were
declined:
- `Reviewed::map`, because it would let any holder of a review change what
  is compared;
- `Fresh::map`, because it would let any holder of a proof attach it to
  another payload.

### Macro cost

`machine!` is `macro_rules!`, not a proc macro. It was measured on the
evaluation's Hotaru wrap machine, the hottest path in the evaluation. The
source files are in the evaluation workspace in PR #282, under
`docs/state-machine-crates/eval/experiments/typestate-wrap/`.

| Variant | Lines | Instructions per printed cell vs `enum` (default build / thin LTO) |
|---|---:|---|
| Hand-written borrowed layout | 404 | 1.00 / 1.00 |
| `machine!` | 347 (−14%) | 1.02 / 1.02 |
| `machine!` with `stored(Copy)` | 347 | 1.02 / **0.96** |

Crate-only builds of that workspace, 5 repetitions: dev 0.50 s with the
macro against 0.54 s without, release 0.64 s against 0.72 s. That's within
noise, so the macro has no measurable compile-time cost. The compile-time
impact on each component is in [Compile time](#compile-time).

Most tier 1 machines are linear transactions: prepare, confirm, execute; back
up, change, then commit or restore. Consuming types and proof values fit them
better than `machine!`, so the macro reduced lines only in the four
event-driven machines.

## Code size

These counts cover the source files the change touches, as non-blank,
non-comment lines; unit tests inside those files count, and doc comments and
doctests don't.

| Component | Before | After | Change |
|---|---:|---:|---:|
| Genkan | 7,361 | 8,843 | +1,482 |
| Kakehashi | 2,466 | 3,581 | +1,115 |
| Yukimi (helper and libraries) | 2,628 | 3,545 | +917 |
| Hako | 4,389 | 5,257 | +868 |
| `aizome-typestate` (new) | 0 | 613 | +613 |
| Tansu | 5,022 | 5,352 | +330 |
| aizome-sys | 1,746 | 1,976 | +230 |
| Yukimi window | 2,074 | 2,103 | +29 |
| **Total** | 25,686 | 31,270 | **+5,584 (+22%)** |

The evaluation predicted this: typestate cost 1.8× the enum's lines there.
Most of the added code is proof types, consuming stage types and the module
boundaries that keep their constructors private.

## Compile time

The touched packages were built on main (`e2a1e508`) and on this branch,
alternating which went first in each of 5 repetitions, under the benchmark
lock, on 12 CPU threads with 8 build jobs, in dev profile. Two kinds of build
were timed:
- **crate-only:** `cargo clean -p`, then build, with dependencies already
  built;
- **incremental:** touch the package's root file, then build with
  `CARGO_INCREMENTAL=1`.

Other sessions shared the host, so the load average ran from 4.8 to 39
(median 14). Repetitions spread by about ±15%, so read the table's smaller
differences as noise. The table gives medians in seconds.

| Package | Crate-only, main | Crate-only, branch | Change | Incremental, main | Incremental, branch | Change |
|---|---:|---:|---:|---:|---:|---:|
| `aizome-typestate` (new) | — | 0.11 | new | — | 0.10 | new |
| `aizome-sys` | 4.97 | 4.84 | −3% | 0.86 | 0.83 | −3% |
| `aizome-tansu` | 4.07 | 4.59 | +13% | 2.80 | 3.21 | +15% |
| `aizome-hako` | 5.06 | 5.82 | +15% | 3.30 | 3.51 | +6% |
| `aizome-yukimi` | 4.44 | 4.48 | +1% | 2.89 | 2.54 | −12% |
| `kakehashi-core` | 1.01 | 1.01 | 0% | 0.35 | 0.33 | −6% |
| `kakehashi` (window) | 5.72 | 5.58 | −2% | 6.93 | 6.58 | −5% |
| `kakehashi-helper` | 0.31 | 0.33 | +4% | 0.29 | 0.39 | +33% |
| `kakehashi-write` (new) | — | 0.21 | new | — | 0.18 | new |
| `calamares-nixos` (Genkan backend) | 1.51 | 1.57 | +4% | 0.75 | 0.89 | +18% |
| `genkan` (window) | 4.33 | 4.37 | +1% | 1.22 | 0.95 | −22% |
| `yukimi-system` | 0.84 | 0.86 | +2% | 0.22 | 0.21 | −5% |
| `yukimi-helper` | 0.58 | 0.70 | +21% | 0.42 | 0.43 | +2% |

The shared crate costs 0.1 s, once per workspace. Hako and Tansu build about
0.5 to 0.8 s slower from scratch, in step with their added code: the runs
overlap, but the medians and most repetitions agree. `yukimi-helper`'s
+0.12 s is small but consistent. Everything else is within noise. No package
uses a proc macro, and the macros add no dependency. The raw timings are in
[`typestate-tier1/compile-times.txt`](typestate-tier1/compile-times.txt), from
[`compile-times.sh`](typestate-tier1/compile-times.sh).


## Not caught at compile time

Every bug's mistake as it was written is rejected. These are what the types
still can't rule out, grouped by why. A follow-up pass closed some of them,
as each group notes; the rest are for discussion.

### Runtime facts no type can know

The comparison happens at run time, but the code can't skip it: the step
takes a proof only the comparison makes.

- **GEN-03:** that reading the target disk touches no other disk.
- **SYA-13 and TSH-25:** the identities UDisks reports, and the window
  between the check and UDisks acting (UDisks has no "only if unchanged"
  call).
- **KTL-03 and KTL-04:** the helper's progress reports and plan, which come
  from another process. A faked progress file can't be detected.
- **YUK-02 and YUK-10:** boot ids and generation link targets, from the
  kernel and Nix.
- **HAKO-06 and HAKO-08:** whether an `fsync` failed, and whether an item has
  left the Trash can. Which info file a proof found is its device, inode and
  change time: ext4 gives a freed inode to the next file, so the inode alone
  matched a later item's info. Forgetting also needs the item still gone, so
  a later item in the can keeps its info whatever inode it got. Left open:
  an item being trashed under the name, between claiming its info and
  moving in, with a reused inode and an equal change time.

### Rust has no linear types

- **GEN-34 and GEN-28:** a transition can hand back its state unchanged, for
  example a handler that ignores an event. `with_typed` accepts that.
  - **Closed in part:** the typestate modules in all five components deny
    `clippy::wildcard_enum_match_arm`, and every `_ =>` arm in them names
    its cases. A `_ =>` that hides a new state or event fails clippy.
  - **Still open:** an arm that names a state and hands it back unchanged.
- **GEN-28:** a caller could collapse the swap partition's size into
  "reading". **Closed:** it's an enum (`Off`, `Known`, `Reading`,
  `Unreadable`) with no shortcuts, so a caller must match it, and
  `.is_err()` doesn't compile.
- **YUK-09, YUK-11 and HAKO-05:** dropping a transaction mid-way is covered
  by an `Unsettled` fallback at run time, and a process killed with SIGKILL
  skips the fallback.
  - **Closed for YUK-11:** Clean Up records the deletion durably in
    `/var/lib/yukimi/clean-up.json` before it starts, and removes the record
    only after the boot menu is rewritten. The operation's systemd unit runs
    `yukimi-helper __recover` from `ExecStopPost=`, which systemd runs
    however the worker ended, SIGKILL included. Every operation also
    recovers at its start, which covers a power cut. Recovery takes the
    operation lock, so it never runs beside a live Clean Up.
  - **Still open:** YUK-09 and HAKO-05 have no record of this kind.

### Enforced by a lint, not a type

Yukimi and Genkan let only their typed steps name the programs that change
the system or a disk (a `Program` value with a private constructor). The
other ways to start a program are banned by clippy `disallowed-methods`,
which their Nix check phases run.

**Closed:**
- **The program ban:** the helper and window binaries now `forbid` it, so a
  local `#[allow]` there is an error (E0453). The libraries keep `deny`,
  because their one process module must start programs.
- **`unsafe`:** it's confined to one system-call module per crate (Genkan
  also keeps libnm's FFI), denied in the libraries and forbidden in the
  binaries.
- **A Nix check-phase guard:** it fails the build if either lint is allowed
  anywhere but those modules.

**Still open:**
- free-form `nix build` and `systemctl` arguments (Yukimi);
- writing the device node with ordinary file APIs (Genkan).

### A proof shows a check, not a person

- **Kakehashi:** a second, throwaway drive list can produce a confirmation.
- **aizome-sys:** an identity can be read from live state and confirmed with
  another brand. Tansu's own brand rejects that.
- **Kakehashi:** a stale dialog ticket is refused at run time, not compile
  time.

### Hardware that looks identical

**Closed in part:** a drive's identity now includes its USB port, compared
when both readings have one, so an identical serial-less stick in another
port, or a stick moved to another port, no longer matches.
- **Kakehashi:** the port comes from sysfs on Linux and from the IOKit
  location ID on macOS. Windows doesn't read it. The dialog shows the port.
- **aizome-sys:** the port comes from the disk's USB `/dev/disk/by-path`
  link.

**Still open:**
- **Kakehashi:** an identical stick put into the same port, or two cards
  behind one reader. The kernel's disk sequence number would separate them,
  but media-change events also bump it and would refuse legitimate
  card-reader writes, so it wasn't added.
- **aizome-sys:** it also compares UDisks' detection times, which change
  with each plug. Only a card swapped within one poll of the reader could
  slip through.

### Left as follow-ups

- **Yukimi:** `rollback N` still names its target by number. Passing the
  system the person saw needs a protocol change.
- **Genkan:** a per-preparation nonce and `PTUUID` would bind confirmation to
  preparation over the wire.
- **Hako:** files left by a crash (`.hako-aside-*`, `.hako-part-*`) stay
  hidden, and nothing sweeps them.
- **Tansu, TSH-26 (not tier 1):** a second launch still replaces the Running
  sheet.

## Changes a person will notice

**Genkan**
- An early confirmation waits for preparation.
- A preparation failure returns to the review page with Back.
- A failure before any write says the disk is unchanged.
- A helper that stops before the review opens returns to the pages with a
  note.
- A disk that vanishes during a scan is listed as unavailable.
- A failed memory read says so.
- The deferred route refuses a selection the root can't hold before
  erasing.

**Kakehashi**
- A drive swapped or re-plugged while the dialog is open isn't written, and
  the window says why.
- The dialog shows the drive's USB port. A stick moved to another port is
  treated as another drive.
- A helper killed after it reported shows the "partly written" warning.

**Tansu**
- Operations whose device is gone or replaced are dropped with a notice.
- A sheet whose target is gone keeps its title, says why, and can only be
  cancelled.

**Yukimi**
- A failed or stopped change restores root's channels.
- Clean Up rewrites the boot menu before collecting garbage, and Stop waits
  for that step.
- A copy saved for a generation number that Nix has since reused is set
  aside as `generation-<n>.unrecorded`, not used for the new generation.

**Hako**
- Put Back and Duplicate send what they replace to the Trash. An item the
  Trash refuses is kept beside its replacement as `name (replaced).ext`.
- A folder that couldn't be synced keeps its sources, and its unsafe copy is
  removed.
- A partly restored Trash folder keeps its info.
- Undo and Redo run one at a time. A failed undo keeps its step.
- A step whose undo does nothing twice is dropped, with a note.

## Wire and file formats

- **Genkan's helper protocol:** unchanged on the wire. The helper now
  refuses a confirmation sent before `Prepared` instead of keeping it, and
  the GUI sends one only after `Prepared`.
- **Kakehashi's `plan.json`:** it gains an optional `port` key, omitted when
  there's none, and old plans still read. The progress lines are unchanged.
  The helper reads the plan as its own type.
- **Yukimi's saved configurations:** they gain a `.system` record. Copies
  from before it are accepted once and then recorded.
- **Yukimi's Clean Up:** it writes `/var/lib/yukimi/clean-up.json` while it
  runs.
- **Aizome-sys's API:** `Destructive` is now `Change`, and making partitions,
  labels, partition type, name and flags, and passphrase changes now needs a
  confirmation. Only Tansu calls it.

## Checks

The check kinds are kept apart, as `AGENTS.md` asks.

- **Component tests**, on the integrated branch:

  | Workspace | Tests passed | Doctests passed, codes checked |
  |---|---:|---:|
  | Aizome (crate, aizome-sys, Tansu, Hako, Yukimi window) | 644 | 61 |
  | Kakehashi | 79 | 20 |
  | Yukimi | 164, plus 4 namespace end-to-end | 24 |
  | Genkan backend | 125 | 29 |
  | Genkan UI | 48 | 8 |

  Clippy is clean in every workspace.
- **Regression tests:** each fails on main and passes here.
- **Nix builds:** `.#kakehashi`, `.#yukimi.helper` (with its new clippy
  pass), `.#genkan`, `.#genkan.helper` and `.#vm-fixture`.
- **VM checks**, in disposable NixOS VMs:
  - `kakehashi-vm`, all five subtests, including one where the helper
    refuses the plan's drive in another port;
  - `storage`;
  - `encrypted-storage`, 12 BIOS and UEFI cases, re-run after the disk
    lock moved into Genkan's system-call module;
  - `genkan-preflight`;
  - `tansu-resize`.
- **Nix build of Uta** (`.#uta`), which builds against this branch's kit
  and Sukumo.
- **ISO installs**, from an ISO built from `abaa9c61` (SHA-256
  `cd9b5ba6…7508`), in disposable QEMU VMs driven by mouse and keyboard
  through QMP, each screenshot inspected:

  | Case | Result |
  |---|---|
  | UEFI GUI install, confirmed while preparation was still running | pass: the window waited ("Finishing preparation…"), the disk stayed untouched for 9 minutes until `Prepared`, then it installed; the installed system booted and its greeter took the password |
  | UEFI GUI install, confirmed after "Ready to install" | pass: installing began at once; the disk was untouched before the click |
  | BIOS GUI install, confirmed while preparing | pass, as the first, with GRUB |
  | Deferred route, a 26 GiB disk too small for Firefox and Uta, confirmed early | pass: refused before any write ("needs at least 23.0 GiB on the root filesystem; it would have 21.0 GiB"); the queued confirmation was dropped and the window returned to Review; the disk image's hash didn't change |
  | Deferred route, an 80 GiB disk | fail, from a defect older than this branch (below) |
  | The same, rebased onto main with #305, from an ISO built from `3d6ff2c7` (SHA-256 `01ea26da…ffa5`) | pass: Uta compiled on the target, then the system; the installed system booted to the greeter and passed the fixture's checks |
  | Scripted helper session | pass: confirmation sent only after `prepared`, then `complete`; the installed system passed the fixture's checks |

  The deferred route chose its route, passed the room check, erased the
  confirmed disk and started building on it, then failed while compiling
  Uta. Genkan builds there with `--option build-dir /nix/var/nix/builds`,
  which for a build into `/mnt` is on the live system's RAM-backed root
  (1.9 GB with 4 GiB of memory). The argument dates from Genkan's import,
  and main hides it because the cache serves its Uta; this branch's Uta
  isn't cached, so it compiled. The same Uta builds on the host. An earlier
  attempt also hit a transient download error, which aborts the install
  after erasing because the build doesn't fall back. #305 fixed both on
  main: the build runs in the target's own `/nix/var/nix/builds`, and a
  failed download is retried, the last time with `--fallback`. Rebased onto
  it, this branch calls #305's `build_on_target` from its
  `Build::OnTarget` arm, and the same case passed.
- **Hardware:** no physical hardware was tested.
