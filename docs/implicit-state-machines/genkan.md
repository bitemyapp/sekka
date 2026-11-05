# GEN: Genkan installer

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `installer/genkan`. Audited at `8e0acbe5`; line numbers refer to that commit.

83 files, 0 pending. 37 implicit machines: 7 High, 8 Medium, 22 Low.

## Machines

<a id="gen-01"></a>

### GEN-01 — (test harness) Encrypted-storage and boot-splash VM drivers
- **Where:** `installer/genkan/nix/encrypted-splash-test.nix:162` (per-firmware subtest sequence), `encrypted-splash-test.nix:72` (`splash-hold` unit gated on `/run/splash-release`), `encrypted-splash-test.nix:85` (`slow-stop`); also `installer/genkan/nix/encryption-test.nix:68`
- **Encoding:** Python test-script control flow per VM; the marker file `/run/splash-release` polled by the `splash-hold` oneshot (`until [ -e /run/splash-release ]`, `encrypted-splash-test.nix:82`) holds Plymouth up; OCR polling loops with deadlines (`wait_for_passphrase_view`, `encrypted-splash-test.nix:132`–`141`; `gone`, `encrypted-splash-test.nix:150`–`155`).
- **States:** `Live` → `StorageWritten` (runs the GEN-10 matrix) → `BootEntrySwitched` → `Crashed` → `Asking` → `Waited100s` → `Typed` → `Unlocked(splash held)` → `Hinted` → `AskedAfterTakeover` → `QuestionAbandoned` → `Released(login)` → `Restarting` → `AskedAgain(wrong)` → `CapsLock` → `TextView` ↔ `GraphicalView` → `Unlocked again` → `Shutdown`; encryption-test.nix runs the first five states then `Unlocked` → `Shutdown`.
- **Transitions:** storage matrix (`encrypted-splash-test.nix:169`); `switch-to-configuration boot`, `crash`, `start` (`encrypted-splash-test.nix:176`–`179`); `send_chars` passphrase (`encrypted-splash-test.nix:195`); `touch /run/splash-release` (`encrypted-splash-test.nix:269`, `encrypted-splash-test.nix:323`); `reboot` (`encrypted-splash-test.nix:276`); Esc toggles (`encrypted-splash-test.nix:298`, `encrypted-splash-test.nix:303`); `encryption-test.nix:82`–`91`.
- **Hazards:** The stage order is implicit in script order, and each step depends on the VM state the previous one left. The second boot's "restart left root clean" check depends on the first boot's `splash-hold` release. OCR text matching stands in for state queries (the variants logic at `encrypted-splash-test.nix:119`–`130`).
- **Reify:** Low — a test script; its order is its specification.
- **Related:** `installer/genkan/rust/src/install.rs:994` (GEN-10, the in-VM storage driver); boot-splash machines GEN-16–GEN-19; initrd ordering GEN-02

<a id="gen-02"></a>

### GEN-02 — Generated initrd unlock sequence (root first, swap with root's passphrase, no device timeout)
- **Where:** `installer/genkan/rust/src/config.rs:263` (`storage_configuration` encrypted branch); also `installer/genkan/rust/src/test-data/bios-ext4.nix:1`, `installer/genkan/rust/src/test-data/uefi-ext4.nix:1`
- **Encoding:** Generated NixOS options that become systemd units in the initrd: `x-systemd.device-timeout=0` on `/` (`config.rs:267`); `boot.initrd.luks.devices.cryptroot`/`cryptswap` (`config.rs:299`–`310`); a drop-in that orders `systemd-cryptsetup@cryptswap` `after` `systemd-cryptsetup@cryptroot` (`config.rs:315`); `boot.resumeDevice` set to the swap filesystem's UUID inside the swap mapping (`config.rs:321`–`323`).
- **States:** `AskingRoot` → `RootOpen` (passphrase cached) → `SwapOpen` (reuses cache) → `RootMounted`; a wrong root passphrase goes `AskingRoot` → `AskingRoot`; `AskingRoot` waits forever (no device timeout).
- **Transitions:** systemd-cryptsetup for root after the prompt is answered; cryptswap starts only after cryptroot finishes (`config.rs:315`); sysroot.mount when the root mapping's filesystem UUID appears (`config.rs:270`–`275`).
- **Hazards:**
  - `after=` orders the units but does not require root to succeed. If cryptroot gives up, cryptswap runs and asks for its own passphrase.
  - Root has no device timeout, so a missing or failed disk leaves boot at the prompt indefinitely. This is deliberate.
  - The resume device appears only once root is unlocked. This module sets no timeout on the resume wait, so whether a slow passphrase skips resume from hibernation depends on systemd defaults not examined here.
- **Reify:** Low — declarative generated config; the encrypted-boot VM test (GEN-01) pins the behaviour.
- **Related:** systemd initrd (systemd-cryptsetup, the ask-password agent); the Plymouth prompt machines GEN-16, GEN-19

<a id="gen-03"></a>

### GEN-03 — Selected-disk identity: discovered → reviewed → revalidated before and after the first write
- **Where:** `installer/genkan/rust/src/disk.rs:19` (`Identity`), `disk.rs:31` (`Disk.blocked`), `disk.rs:182` (`revalidate_with_mappings`); also `installer/genkan/rust/src/install.rs:342`, `install.rs:547`, `install.rs:573`, `install.rs:628`, `installer/genkan/ui/src/app.rs:1613`
- **Encoding:** An `Identity` snapshot (path, maj:min, bytes, serial, wwn, model) taken by the GUI's discovery and carried in the request. It is checked again by full rediscovery (`discover_with_mappings`, `disk.rs:93`) at each phase, with an allow-list of this installation's own dm mappings after formatting.
- **States:** `Discovered(GUI)` → `Reviewed` (GUI worker revalidation) → `Prepared` (helper revalidation) → `PreWrite` (under the disk flock) → `Repartitioned` → `Formatted(own holders allowed)` → `Mounted`; any check can fail → `Refused`.
- **Transitions:** GUI review worker (`app.rs:1613`); `prepare` (`install.rs:342`); before `wipefs` (`install.rs:547`); after `parted` and the kernel layout check (`install.rs:573`); after formatting, with `mappings.paths()` (`install.rs:628`); `blocked` is set by read-only, mounted or held (`disk.rs:161`), sector size or virtual device checks in discovery.
- **Hazards:**
  - The same full-system rediscovery runs before and after erasure. A failure means "refuse safely" before `install.rs:555` and "abort a half-written disk" after it. Nothing distinguishes the two cases.
  - `discover_with_mappings` returns an error, rather than skipping, when any listed disk's node misbehaves: `fs::metadata(p)?` (`disk.rs:127`), "Disk changed during discovery" (`disk.rs:136`), `read_dir` of holders (`disk.rs:158`).
  - Identity equality relies on serial, wwn and model strings. Two disks with empty identifiers at the same path and size compare equal.
- **Reify:** High — the check's meaning depends on the phase (pre-write vs post-write), with data-loss consequences, and there is a suspected bug.
- **Suspected bug:** If another USB disk is unplugged or re-enumerated between `lsblk` and the per-disk `fs::metadata` during the post-partition or post-format revalidation (`install.rs:573`, `install.rs:628`), the installation aborts with an error about an unrelated disk. The target disk has already been wiped and partitioned (confidence: medium-low).
- **Related:** GUI side `installer/genkan/ui/src/app.rs:877` (GEN-28 scan)
- **Reproduction:** Confirmed (unit): [test and output](reproductions.md#gen-03)
- **Follow-up:** reified as typestate in [#290](https://github.com/bitemyapp/sekka/pull/290). Its suspected bug is fixed, and the mistake no longer compiles.

<a id="gen-04"></a>

### GEN-04 — LUKS mapping registry and its cleanup order
- **Where:** `installer/genkan/rust/src/encryption.rs:38` (`Mappings(Vec<String>)`); also `installer/genkan/rust/src/install.rs:666`–`668`
- **Encoding:** A list of `genkan-<uuid>` names, registered before `cryptsetup open` runs. `close()` pops while closing; `Drop` closes the rest, ignoring errors. Cleanup order with the mount guard (GEN-08) depends on local declaration order in `execute` ("Drop order matters", `install.rs:666`).
- **States:** `Empty` → `Registered(name)` (mapping may or may not exist) → `Open` → `Closed`; `Drop` with entries left → best-effort close.
- **Transitions:** `format_open`: refuse if the mapping exists (`encryption.rs:50`–`53`), `wipefs` (`encryption.rs:54`), `luksFormat` (`encryption.rs:55`–`74`), UUID probe (`encryption.rs:75`–`78`), register (`encryption.rs:80`), open (`encryption.rs:81`–`95`); `close` (`encryption.rs:104`–`110`); `Drop` (`encryption.rs:112`–`119`); `execute` closes after unmount (`install.rs:830`).
- **Hazards:**
  - The invariant "unmount before close" holds only because `mappings` is declared before `target` (`install.rs:667`–`668`). The matrix test repeats the same order by hand (`install.rs:1032`–`1033`).
  - A name is registered before open succeeds, so `close` on a mapping that never opened fails. `close()` then stops early with `?` and leaves the remaining entries to `Drop`.
  - `Drop` never forces removal, so a mapping still mounted after a failed unmount stays open.
- **Reify:** Medium — small, but the ordering invariant with GEN-08 is implicit.
- **Related:** GEN-08 (`Target`), GEN-07

<a id="gen-05"></a>

### GEN-05 — (test harness) Loop-device image guard in the reformat matrix
- **Where:** `installer/genkan/rust/src/filesystem.rs:624` (`Image { device, dir }`)
- **Encoding:** A test-local struct with a `Drop` that unmounts, detaches the loop device, then deletes the backing file only if the detach succeeded.
- **States:** `Created(dir)` → `Attached(loop)` → `Mounted` ↔ `Unmounted` (per format pair) → `Dropped(detached, files removed)`.
- **Transitions:** `losetup --find` (`filesystem.rs:650`–`662`); guard built (`filesystem.rs:663`); mount/umount per pair (`filesystem.rs:692`–`715`); `Drop` (`filesystem.rs:629`–`640`).
- **Hazards:** A mount left by a failure is unmounted by `Drop`, but `remove_dir` silently leaves the directory if that unmount fails. This is intended: there is no recursive delete.
- **Reify:** Low — a test fixture.

<a id="gen-06"></a>

### GEN-06 — Privileged helper session: request → preparation → confirmation or cancel → execution
- **Where:** `installer/genkan/rust/src/helper.rs:24` (`session`); also `installer/genkan/rust/src/process.rs:15` (`CANCEL`), `helper.rs:68` (one-shot `install`/`preflight` path)
- **Encoding:** Control flow in `session()`, plus a stdin-reader thread and an `mpsc` channel carrying at most one confirmation line. The process-global `AtomicBool CANCEL` is set by that thread on EOF, error or an oversized line. `install::prepare`/`execute` read it through `process::run`, `installer_lock` (`install.rs:300`), `precache::warm` and the post-cache check (`install.rs:501`).
- **States:** `ReadingRequest` → `Parsed` → `Preparing` (reader armed) → `Prepared` (awaiting) → `Confirmed` → `Executing` → `Exit(0)`; `Preparing`/`Prepared` → `Cancelled` (EOF) → `Exit(0)`; any error → `Failed` → `Exit(1)`. Representable: a confirmation already queued while still `Preparing`; `CANCEL` latched while `Prepared`.
- **Transitions:** request read and parsed (`helper.rs:26`–`33`); the reader thread sets `CANCEL` (`helper.rs:35`–`41`); prepare error while cancelled → `Cancelled` (`helper.rs:44`–`46`); `Prepared` event (`helper.rs:50`–`52`); `recv` gives None → `Cancelled` (`helper.rs:53`–`55`); phrase check and `execute` (`helper.rs:57`–`59`); `main` reports `Failed` (`helper.rs:91`–`95`).
- **Hazards:**
  - A confirmation that arrives during preparation is consumed the moment preparation ends. Execution begins without any further GUI round trip.
  - Once the confirmation line has been read, stdin is no longer watched. Nothing can cancel the rest of preparation (a build of up to 7200 s), although no disk has been written yet.
  - Any preparation error after `CANCEL` is reported as `Cancelled`, whatever its cause.
  - `CANCEL` never resets. After it is set, every later subprocess fails at `process::run`'s entry check.
  - The one-shot `install`/`preflight` path (`helper.rs:68`–`82`) is a second entry into the same transaction, with its own confirmation (`install.rs:852`–`853`).
- **Reify:** High — privilege boundary; phases split across a thread, a channel and a global latch; the window between confirmation and the first write can't be cancelled.
- **Related:** GUI side `installer/genkan/rust/src/session.rs:56` (GEN-15) and `installer/genkan/ui/src/app.rs:1681` (GEN-34); Polkit policy `installer/genkan/nix/org.calamares.nixos.install.policy.in:7` (`auth_admin_keep`)
- **Follow-up:** reified as typestate in [#290](https://github.com/bitemyapp/sekka/pull/290).

<a id="gen-07"></a>

### GEN-07 — Install transaction: non-destructive preparation stages, then destructive steps 1–6
- **Where:** `installer/genkan/rust/src/install.rs:267` (`Prepared`), `install.rs:325` (`prepare`), `install.rs:647` (`execute`), `install.rs:91` (`Summary.deferred`); also `installer/genkan/rust/src/filesystem.rs:73` (`Identities`), `installer/genkan/ui/src/app.rs:128` (`INSTALL_STEPS`)
- **Encoding:** Sequential code with `Stage` timers and `Event::Preparing` text. The `Prepared` token carries the lock, staging dir, pre-chosen filesystem identities, layout, `configuration.nix`, derivation and system paths, and `Summary`. The `deferred` bool picks the route. Destructive progress is reported as bare integers `progress(1..6)`. Typestate gate: `InstallPlan::confirm` → `ConfirmedInstall`.
- **States:** `Checks` → `Configuration` → `Evaluation` → (`Build` → `ClosureCheck` → `Cache`) | `Deferred` → `Prepared` → `Confirmed` → `Step1 Erase+GPT` → `Step2 Format/Mount` → `Step3 Files` → `Step4 Copy` | `Step4 Build-on-target` → `Step5 Bootloader` → `Record` → `Step6 Flush/Unmount` → `Complete`.
- **Irreversibility:** No write before `install.rs:555`. From `wipefs` on the whole disk (`install.rs:555`), the old partition table is gone. `parted` (`install.rs:559`) writes the new GPT. Filesystems are made from `install.rs:579`. The disk is bootable only after `nixos-install` (`install.rs:788`–`799`) and cleanly unmounted only at `install.rs:829`–`830`. Nothing is rolled back.
- **Transitions:** live guard, firmware, lock, revalidation and preflight (`install.rs:330`–`356`); layout and identities chosen before erasure (`install.rs:357`–`363`); hardware config and `configuration.nix` (`install.rs:377`–`384`); dry-run (`install.rs:396`–`419`); route choice (`install.rs:429`–`432`); build, closure-size check and cache (`install.rs:452`–`504`); `Prepared` (`install.rs:507`); `execute` checks that the configuration is unchanged (`install.rs:653`–`656`); `prepare_storage` (`install.rs:671`); files (`install.rs:683`–`706`); copy or deferred build (`install.rs:731`–`781`); bootloader (`install.rs:785`–`803`); record (`install.rs:808`–`824`); flush (`install.rs:827`–`831`); `Complete` (`install.rs:837`).
- **Hazards:**
  - In the deferred route, `closure_bytes` is never set (it is assigned only at `install.rs:477`). Copy progress then reports `total: 0` (`install.rs:712`, `install.rs:722`–`725`), which the GUI ignores, so step 4 shows no progress.
  - Step numbers are a convention shared with the GUI's `INSTALL_STEPS`/`STEP_WEIGHTS`. `progress(6)` is emitted twice (`install.rs:827`, `install.rs:833`).
  - A failure writing `install.json` (`install.rs:820`–`824`) after the bootloader step reports `Failed` for a system that is installed.
  - The sleep inhibitor (GEN-12) covers only `execute`.
- **Reify:** High — the most important transaction; destructive phases are implicit in code order and integer events, and there is a suspected bug.
- **Suspected bug:** The deferred route skips the "selected system needs at least N GiB on the root filesystem" guard (`install.rs:479`–`488`), which runs only after an in-memory build. A low-RAM machine with a large selection on a small disk therefore passes preparation. It then fails with "No space left" while building on the target, after the disk was erased. `missing.fetch_bytes` is known by then and could be checked (confidence: medium).
- **Related:** helper session GEN-06; GUI progress GEN-35 (`installer/genkan/ui/src/app.rs:1855`); typestate `installer/genkan/rust/src/plan.rs:354`
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#gen-07)
- **Follow-up:** reified as typestate in [#290](https://github.com/bitemyapp/sekka/pull/290). Its suspected bug is fixed, and the mistake no longer compiles.

<a id="gen-08"></a>

### GEN-08 — Install target mount guard
- **Where:** `installer/genkan/rust/src/install.rs:234` (`Target(PathBuf, Cell<bool>)`)
- **Encoding:** A directory kept under `/run`, plus a `Cell<bool>` that means "something is mounted here". `Drop` unmounts recursively if the flag is set, then `remove_dir` (never recursive).
- **States:** `Created` → `Mounted` → `Unmounted` → `Dropped`; `Mounted` → `Dropped` (best-effort `umount --recursive`).
- **Transitions:** `new` (`install.rs:236`–`244`); flag set after the root mount (`install.rs:630`; test `install.rs:1088`); `unmount` clears it (`install.rs:245`–`249`, called at `install.rs:829`); `Drop` (`install.rs:251`–`258`).
- **Hazards:** The flag is set only after root mounts, so a failure inside `mount` itself leaves it false. `Drop`'s cleanup order relative to `Mappings` is implicit (GEN-04).
- **Reify:** Low — small RAII guard.
- **Related:** GEN-04

<a id="gen-09"></a>

### GEN-09 — Installer exclusivity lock and the partition-table disk lock
- **Where:** `installer/genkan/rust/src/install.rs:286` (`installer_lock`), `install.rs:269` (`Prepared._lock`), `install.rs:532` (`device: Option<File>`)
- **Encoding:** An `flock` on `/run/calamares-nixos.lock`, taken with a 200 ms retry loop and a 30 s deadline that also polls `CANCEL`, and held by `Prepared`. A second `flock` on the target block device is held in an `Option` while partitioning and formatting, then taken out so udev events flow.
- **States:** `Waiting` → `Held` → `Released` (Prepared dropped); disk: `Locked` → `Released(before udev trigger)`.
- **Transitions:** retry loop (`install.rs:294`–`302`); acquired in `prepare` (`install.rs:341`); disk flock (`install.rs:538`–`546`); `device.take()` + `sync_all` (`install.rs:614`–`616`); `drop(device)` (`install.rs:641`).
- **Hazards:** The lock is held for as long as the user leaves the Review page open. The 30 s wait assumes a withdrawn helper notices EOF promptly. The disk lock is advisory only.
- **Reify:** Low — local and well commented.
- **Related:** GEN-06

<a id="gen-10"></a>

### GEN-10 — (test harness) Encrypted storage matrix in the disposable VM
- **Where:** `installer/genkan/rust/src/install.rs:994` (`encrypted_storage_matrix`)
- **Encoding:** Nested loops over filesystem × swap, each driving `prepare_storage` → assertions → explicit or unwinding-style cleanup → reopen with the right key. Ordering matters across iterations ("Ext4 last", `install.rs:1014`).
- **States:** per case `Partitioned+Mounted` → `Sentinel written` → `Cleaned(explicit | Drop)` → `Reopened` → `Trimmed` → `Closed`; the final case leaves the disk for the VM's reboot (GEN-01).
- **Transitions:** `prepare_storage` (`install.rs:1034`); explicit unmount/close for swap cases (`install.rs:1051`–`1054`); `drop(target)`/`drop(mappings)` (`install.rs:1056`–`1057`); reopen (`install.rs:1074`–`1086`); close of `genkan-reopen` (`install.rs:1098`).
- **Hazards:** The `genkan-reopen` mapping has no guard. A failed assertion between `install.rs:1086` and `install.rs:1098` leaves it open, and later revalidation (or the next run) fails.
- **Reify:** Low — test driver.
- **Related:** GEN-01

<a id="gen-11"></a>

### GEN-11 — Nix internal-json activity tracker and copy-progress throttle
- **Where:** `installer/genkan/rust/src/nixlog.rs:105` (`Tracker.kinds`, `.copied`, `.copied_bytes`, `.paths`, `.builds`, `.errors`, `.tail`); also `installer/genkan/rust/src/install.rs:713` (`last: Mutex<Instant>`)
- **Encoding:** A per-activity-id map from `start` to `stop`; per-copy-activity byte counters summed into `copied_bytes`; an error list and a 40-line ring buffer for failure reports; a 250 ms throttle in `execute`'s line callback.
- **States:** per activity `Started(kind)` → `Progress*` → `Stopped`; tracker `Clean` → `HasErrors` (failure report available).
- **Transitions:** `start` inserts the kind (`nixlog.rs:145`–`151`); `stop` removes it (`nixlog.rs:152`–`155`); level-0 `msg` (`nixlog.rs:156`–`162`); build-log line (`nixlog.rs:163`–`171`); progress result (`nixlog.rs:172`–`186`); throttled `Copying` (`install.rs:718`–`727`).
- **Hazards:** `copied` is never pruned on `stop`. Progress for an id that was already stopped is dropped. `paths` and `builds` are tracked but not reported. The throttle drops the final partial update, which the copy route makes up for (`install.rs:777`–`780`) and the deferred route does not.
- **Reify:** Low — a parser with small, local state.
- **Related:** GEN-07, GUI GEN-35

<a id="gen-12"></a>

### GEN-12 — Sleep/idle inhibitor during installation
- **Where:** `installer/genkan/rust/src/power.rs:13` (`Awake.lock: Option<OwnedFd>`); also `installer/genkan/rust/src/install.rs:650`
- **Encoding:** A logind `Inhibit("sleep:idle", block)` file descriptor held in an `Option`. `None` means logind refused or the build has no D-Bus support.
- **States:** `Unprotected` | `Held` → `Released` (drop at the end of `execute`).
- **Transitions:** `stay_awake` (`power.rs:26`–`31`) at `install.rs:650`; recorded via `held()` (`install.rs:817`); released when `execute` returns.
- **Hazards:** Not held during preparation or while the plan waits for confirmation. A lid-close then suspends a long in-memory build. This is harmless to the disk.
- **Reify:** Low — RAII guard.
- **Related:** logind D-Bus

<a id="gen-13"></a>

### GEN-13 — Page-cache warming worker pool
- **Where:** `installer/genkan/rust/src/precache.rs:147` (`warm`: `next`, `read`, `done`, `limited`, `stop`, `active`, `first_error`, external `cancel`)
- **Encoding:** Scoped worker threads that pull file indices from an atomic counter. A supervisor loop polls every 50 ms for budget and memory pressure, emits progress every 250 ms, and drops cached compressed pages.
- **States:** `Collecting` → `Reading` → `Stopping(limited)` | `Cancelled` → `Done` → `Outcome`.
- **Transitions:** collect, which checks cancel per entry (`precache.rs:121`–`124`, `precache.rs:152`–`156`); worker loop (`precache.rs:179`–`199`); stop on over-budget or squeezed (`precache.rs:206`–`210`); progress and drop (`precache.rs:211`–`215`); outcome (`precache.rs:219`–`231`).
- **Hazards:** A failed `/proc/meminfo` read counts as "squeezed" and stops with `limited` (`precache.rs:207`). Errors surface only when no file was read. Two different cancel flags drive it: `process::CANCEL` in the helper and a per-warm `AtomicBool` in the GUI.
- **Reify:** Medium — several atomics, but joined in one scope.
- **Related:** callers `installer/genkan/rust/src/install.rs:498`, `installer/genkan/ui/src/app.rs:1505` (GEN-36)

<a id="gen-14"></a>

### GEN-14 — Subprocess run loop: spawn, drain, poll for deadline or cancel, kill the group
- **Where:** `installer/genkan/rust/src/process.rs:115` (`run`); also `process.rs:15` (`CANCEL`)
- **Encoding:** Local variables: a child in its own process group, two drain threads, and a deadline `Instant` polled every 50 ms together with `CANCEL`.
- **States:** `Refused(cancelled)` | `Running` → `Exited(status)` | `TimedOut` | `Cancelled` → `Killed` → `Reaped`.
- **Transitions:** entry check (`process.rs:123`); spawn with `process_group(0)` (`process.rs:124`–`130`); poll loop (`process.rs:175`–`196`); deadline (`process.rs:178`–`180`); cancel (`process.rs:181`); `SIGKILL` to the group (`process.rs:191`–`193`); status check (`process.rs:206`–`209`).
- **Hazards:** Killing the group does not stop work done by `nix-daemon` on the client's behalf, or descendants that call `setsid`. Once `CANCEL` is set, every later call fails at entry.
- **Reify:** Low — one function, transitions centralised.
- **Related:** GEN-06

<a id="gen-15"></a>

### GEN-15 — GUI-side helper Session handle (pkexec child, writer and reader threads)
- **Where:** `installer/genkan/rust/src/session.rs:56` (`Session.writer: Sender<Option<String>>`)
- **Encoding:** `pkexec helper session` with piped stdio. A writer thread writes the plan line, then waits for exactly one `Option<String>` (`Some` = confirm, `None` = cancel) and drops stdin. A reader thread owns the `Child`, forwards parsed `Event`s, then sends `Finished(exit status)`. `Drop` sends `None`.
- **States:** `Spawned(auth pending)` → `RequestWritten` → `Confirmed` | `Withdrawn(stdin closed)`; reader: `Streaming` → `Finished(Ok|Err)`. Representable: confirm after cancel (ignored), cancel after confirm (ignored).
- **Transitions:** spawn (`session.rs:70`–`76`); request write (`session.rs:85`–`88`); one `recv` (`session.rs:89`–`96`); stdin dropped at thread exit (`session.rs:97`–`99`); events (`session.rs:101`–`106`); `Finished` (`session.rs:107`–`113`); `confirm` (`session.rs:119`–`121`); `cancel` (`session.rs:123`–`125`); `Drop` (`session.rs:127`–`130`).
- **Hazards:**
  - A cancel sent while the request write is blocked on Polkit authorization takes effect only after authorization. The withdrawn session's Polkit prompt stays up, and authorizing it starts a helper that prepares until it reads EOF.
  - No handle can terminate the child, because the reader thread owns it.
  - The helper's stderr is discarded, and unparsable or unknown event lines are dropped silently.
  - `auth_admin_keep` lets a re-review within the Polkit window skip the prompt.
- **Reify:** Medium — small, with transitions in four methods, but it sits on the privilege boundary.
- **Related:** helper side `installer/genkan/rust/src/helper.rs:24` (GEN-06); owner `installer/genkan/ui/src/app.rs:571` (GEN-34); policy `installer/genkan/nix/org.calamares.nixos.install.policy.in:7`

<a id="gen-16"></a>

### GEN-16 — Plymouth: unanswered prompts per client connection, dropped when the client hangs up (patch)
- **Where:** `installer/genkan/rust/system/boot-splash/plymouth/abandoned-prompts.patch:77` (`ply_boot_connection_t.pending_answers`); also `abandoned-prompts.patch:41` (`on_answer_abandoned`)
- **Encoding:** A per-connection list of answer triggers, added by the patch, alongside upstream's `disconnected` bit and reference count, and plymouthd's `state->entry_triggers` queue and `entry_buffer`.
- **States:** per request `Queued` (`Displayed` when first) → `Answered` | `Abandoned`.
- **Transitions:** append on password or question request (`abandoned-prompts.patch:153`, `abandoned-prompts.patch:161`); remove on answer (`abandoned-prompts.patch:130`, `abandoned-prompts.patch:145`); hangup loop calls the handler (`abandoned-prompts.patch:172`–`183`); handler clears the buffer if first, removes the entry, pulls NULL, and redraws (`abandoned-prompts.patch:56`–`60`).
- **Hazards:** The handler runs inside `on_hangup` before upstream marks the connection `disconnected`. Pulling the trigger therefore still sends a NO_ANSWER reply to the departed client (`abandoned-prompts.patch:132`–`133`), which relies on plymouthd surviving the write error. The VM test checks that plymouthd keeps answering pings (`installer/genkan/nix/encrypted-splash-test.nix:265`–`266`).
- **Reify:** Low — upstream C; keep the carried patch minimal.
- **Related:** systemd ask-password agent; theme GEN-19; config `installer/genkan/rust/system/boot-splash/default.nix:48`

<a id="gen-17"></a>

### GEN-17 — Plymouth: ownership of the local console terminal across renderer takeover (patch)
- **Where:** `installer/genkan/rust/system/boot-splash/plymouth/console-owner.patch:19` (`manager->local_console_managed`, `local_console_terminal`, `renderer->terminal`)
- **Encoding:** A bool saying "some renderer owns the console terminal", set upstream when a renderer takes it. The patch clears it when the renderer holding that terminal is freed.
- **States:** `Free` → `Managed(by renderer R)` → `Free` (R removed, patch).
- **Transitions:** clear on renderer removal (`console-owner.patch:19`–`23`), after keyboards and displays are freed (`console-owner.patch:17`).
- **Hazards:** Clearing the flag helps only renderers created afterwards. A DRM renderer that was created while simpledrm still owned the console does not get the terminal retroactively. The fix relies on device-arrival order.
- **Reify:** Low — upstream C.
- **Related:** `UseSimpledrm=2` config at `installer/genkan/rust/system/boot-splash/default.nix:112`; GEN-18

<a id="gen-18"></a>

### GEN-18 — Plymouth: which keyboard the splash reads, across keyboard removal (patch)
- **Where:** `installer/genkan/rust/system/boot-splash/plymouth/keyboard-removal.patch:85` (`plugin->keyboard`); also `keyboard-removal.patch:24` (`splash->keyboard`), `keyboard-removal.patch:132` (`script_lib_plymouth_data_t.keyboard`)
- **Encoding:** One keyboard pointer copied into the boot splash, the script plugin and the script library, plus the device manager's keyboard list and plymouthd's per-keyboard handlers. The patch keeps the copies in step through set/unset handlers.
- **States:** `Attached(K)` → `Unattached` → `Attached(K')` (a remaining keyboard) | `None` (Caps Lock reads as off).
- **Transitions:** the removal handler runs before the list removal and free (`keyboard-removal.patch:36`–`40`); `on_keyboard_removed` unsets, then attaches each remaining keyboard (`keyboard-removal.patch:56`–`65`); the plugin moves its input handler and updates the library (`keyboard-removal.patch:77`–`108`); NULL guard (`keyboard-removal.patch:121`).
- **Hazards:**
  - The handler runs while the removed keyboard is still listed (hence the `remaining != keyboard` check).
  - The re-attach loop calls `set_keyboard` once per remaining keyboard, and the last one wins. It may pick another keyboard of the same renderer that is about to be removed.
  - Any free path that skips the handler brings back the use-after-free.
- **Reify:** Low — upstream C; four copies of one pointer can't be reified from a patch.
- **Related:** GEN-17; theme GEN-19

<a id="gen-19"></a>

### GEN-19 — Sekka splash theme: rail, prompt and hint phases
- **Where:** `installer/genkan/rust/system/boot-splash/sekka.script:39` (`global.prompting`), `sekka.script:41` (`.bullets`, −1 = question), `sekka.script:44` (`.hinted`), `sekka.script:47`–`48` (`.waited`, `.clock`), `sekka.script:51`–`53` (`.estimate`, `.resumed_at`, `.resumed_from`), `sekka.script:40` (`.caps_on`), `sekka.script:36` (`.scale`)
- **Encoding:** Script globals updated by Plymouth callbacks. The mode is fixed at start (`sekka.script:14`–`29`).
- **States:** `Normal(rail at progress)` ↔ `Prompting(Password | Question)`; `Hinted` latch; `Normal(resumed)` rescales the remaining estimate over the remaining rail. Representable: `prompting == 0` with stale `answer` and `caps_on`.
- **Transitions:** `on_password` (`sekka.script:277`–`283`); `on_question` (`sekka.script:285`–`292`); `on_normal` records the resume point (`sekka.script:294`–`303`); `on_progress` accumulates waiting time, advances the rail and latches the hint (`sekka.script:245`–`269`); `on_refresh` polls Caps Lock while prompting (`sekka.script:326`–`335`); hotplug relayout and reload on scale change (`sekka.script:122`–`130`); `on_quit` (`sekka.script:337`–`341`).
- **Hazards:**
  - `waited` counts the whole interval since the previous progress tick once a prompt has opened, and misses the tail after it closes.
  - The hint latch never resets.
  - `answer` and `caps_on` survive `on_normal`. The next prompt can show stale Caps Lock until the next refresh.
- **Reify:** Medium — about ten globals across five callbacks; the script language limits how far it can be reified.
- **Related:** Plymouth patches GEN-16–GEN-18; tested by `installer/genkan/nix/encrypted-splash-test.nix:181` (GEN-01)

<a id="gen-20"></a>

### GEN-20 — Aizome IBus frontend service: exit classification at logout and restart
- **Where:** `installer/genkan/rust/system/input-method.nix:29` (`startFrontend`), `input-method.nix:88` (`aizome-ibus` unit)
- **Encoding:** A wrapper that maps exit status 1 to success when `aizome.service` or `aizome-session.target` is inactive, deactivating or failed. `Restart=on-failure`, `partOf` the session target, `ConditionEnvironment`. The legacy autostart exits at run time inside Aizome.
- **States:** `Running` → `Exited(clean)` | `Exited(1 during session stop → clean)` | `Failed → Restarting`.
- **Transitions:** status checks (`input-method.nix:35`–`41`); `exit "$result"` (`input-method.nix:42`); `Restart = "on-failure"` (`input-method.nix:102`); legacy guard (`input-method.nix:49`).
- **Hazards:** The classification samples other units' state after the fact. If `aizome.service` crashed (`failed`), an IBus exit 1 caused by the crash counts as clean, and a genuine IBus failure during a session stop is masked. The run-time guard exists because the user manager carries another desktop's environment (GEN-21).
- **Reify:** Low — a small shell wrapper.
- **Related:** `aizome.service` / `aizome-session.target` (Aizome module); GEN-21

<a id="gen-21"></a>

### GEN-21 — User-manager environment record and per-login reset
- **Where:** `installer/genkan/rust/system/session-env/src/main.rs:25` (`RECORD` in `$XDG_RUNTIME_DIR`); also `installer/genkan/rust/system/desktops.nix:491` (save unit), `desktops.nix:87` (PAM rule), `desktops.nix:77` (Plasma Login X11 cleanup trap)
- **Encoding:** A JSON file written atomically (`.partial` + rename) by a user oneshot before `default.target` (`RemainAfterExit`, `restartIfChanged = false`). A `pam_exec` `open_session` rule, as root, becomes the user, compares the record with the current environment and applies `UnsetAndSetEnvironment`. It skips when another graphical session is open.
- **States:** `NoRecord` → `Recorded(manager start)` → per login `Reset` | `Skipped(no session id | no record | other graphical session)`; X11 session exit → `Cleaned` (targets stopped, display variables unset).
- **Transitions:** `save` (`main.rs:51`–`56`); `reset` guards (`main.rs:62`–`79`); apply (`main.rs:96`); unit (`desktops.nix:491`–`502`); PAM wiring (`desktops.nix:504`–`512`); trap (`desktops.nix:79`–`82`).
- **Hazards:**
  - The record is never refreshed during the manager's lifetime. A lingering manager (Docker enables linger, `installer/genkan/rust/src/applications.nix:202`) keeps resetting each login to the boot-time environment, even after `nixos-rebuild switch`.
  - A second concurrent graphical session keeps the first session's exports.
  - If the oneshot has not finished when PAM runs, `reset` silently does nothing.
- **Reify:** Low — cross-process, but each piece is small.
- **Related:** the desktops' environment exporters; GEN-20

<a id="gen-22"></a>

### GEN-22 — USB keyboard and mouse wake path enablement
- **Where:** `installer/genkan/rust/system/usb-wake.nix:22` (udev rules), `usb-wake.nix:27` (`sekka-usb-wake` oneshot); also `installer/genkan/rust/system/usb-wake.py:11` (`enable`)
- **Encoding:** Each ancestor's sysfs `power/wakeup` attribute. udev `RUN+=` on add or change for keyboard and mouse event nodes. A oneshot `wantedBy` multi-user and sleep targets, `before = sleep.target`, that re-triggers change events and settles.
- **States:** per device or hub `disabled` → `enabled` (one way).
- **Transitions:** udev add or change (`usb-wake.nix:22`–`23`); write `enabled` (`usb-wake.py:28`–`29`); pre-sleep re-trigger (`usb-wake.nix:37`–`38`).
- **Hazards:** Never reverts. A wake source someone disabled on purpose is re-enabled on every change event and before every sleep. The pre-sleep trigger re-runs every input change rule, and settling can delay suspend by up to 10 s.
- **Reify:** Low — idempotent latch.

<a id="gen-23"></a>

### GEN-23 — wpa_supplicant pending scan-result handler left behind by a disabled interface (patch)
- **Where:** `installer/genkan/rust/system/wpa-supplicant/clear-scan-res-handler.patch:38` (`wpa_s->scan_res_handler`); also `installer/genkan/rust/system/wpa-supplicant/default.nix:14`
- **Encoding:** A function-pointer field that decides how the *next* scan results are handled (`scan_only_handler` for D-Bus `Scan(AllowRoam=false)`). It is set when the scan's radio work is queued, not when the scan runs.
- **States:** `None` → `Pending(scan_only)` → consumed by results | (patch) cleared on `EVENT_INTERFACE_DISABLED`.
- **Transitions:** cleared together with `radio_remove_works` (`clear-scan-res-handler.patch:32`–`38`).
- **Hazards:** Other paths that remove queued scan work without results are not covered by the patch. The field outlives the scan work it belongs to.
- **Reify:** Low — upstream C; carried until hostap merges a fix.
- **Related:** NetworkManager's address-randomising rescans

<a id="gen-24"></a>

### GEN-24 — (test harness) VM fixture's helper-session driver
- **Where:** `installer/genkan/tests/vm-fixture/src/main.rs:52` (`invoke_session`: `confirmed: Option<Instant>`, `complete: bool`)
- **Encoding:** Reads the helper's event lines and sends the confirmation once `Prepared` arrives.
- **States:** `RequestSent` → `Prepared → Confirmed` → `Complete` → `Exited`.
- **Transitions:** request write (`main.rs:61`–`63`); confirm on `Prepared` (`main.rs:70`–`78`); `Complete` (`main.rs:79`); final check (`main.rs:83`–`94`).
- **Hazards:** It always confirms after `Prepared`. The GUI's early-confirm path (GEN-34) is never exercised in the VM.
- **Reify:** Low — test driver.
- **Related:** `installer/genkan/rust/src/helper.rs:24` (GEN-06)

<a id="gen-25"></a>

### GEN-25 — (test harness) VM fixture stages and the PAM pty conversation
- **Where:** `installer/genkan/tests/vm-fixture/src/main.rs:502` (`prepare-integrated` | `prepare` | `install` | `verify`); also `main.rs:112` (`pam_authenticate`)
- **Encoding:** Separate invocations coordinated through files: `/run/calamares-package-path` (`main.rs:517`–`520`, `main.rs:559`), a settings swap `vm-settings.json` → `settings.json` (`main.rs:522`–`532`), a seeded Wi-Fi keyfile (`main.rs:533`–`542`), and the disk's blank state as a precondition (`main.rs:593`–`596`). The PAM driver uses a `sent` flag, a 30 s deadline and 50 ms polling.
- **States:** `Prepared(settings, package path)` → `Install(negative requests leave the disk blank → preflight → seed previous fs → real install)` → external reboot → `Verify`; PAM: `WaitingPrompt` → `Sent` → `Exited` | `TimedOut`.
- **Transitions:** negatives (`main.rs:599`–`645`); preflight (`main.rs:646`–`650`); seed (`main.rs:651`); install (`main.rs:652`–`656`); PAM loop (`main.rs:144`–`169`).
- **Hazards:** Stage order lives in the outer test. `prepare-integrated` uses `create_new`, so a rerun in the same VM fails. The PAM driver keys on the literal `Password:` prompt.
- **Reify:** Low — test driver.

<a id="gen-26"></a>

### GEN-26 — Wizard navigation and page locking
- **Where:** `installer/genkan/ui/src/app.rs:484` (`Genkan.step`), `app.rs:485` (`.reached`); gating flags `app.rs:569` (`.reviewing`), `app.rs:580` (`.installing`), `app.rs:588` (`.completed`)
- **Encoding:** Partially explicit. `Step` is an enum, but whether Next, Back and the sidebar are allowed depends on `reviewing`, `installing`, `completed` and `failed`. `go()` runs side effects keyed on the (old, new) step pair.
- **States:** `Editing(step ≤ Keyboard, reached)` → `Reviewing(step = Keyboard)` → `Review` → `Install` → `Done`; `Review` → `Editing(Keyboard)` via Back.
- **Transitions:** `go` (`app.rs:743`–`769`, which re-detects the zone at `app.rs:745`–`752` and starts or stops the key catch at `app.rs:756`–`761`); `jump` (`app.rs:781`–`785`); `forward` (`app.rs:811`–`849`); `backward` (`app.rs:851`–`866`); review redirects (`app.rs:1544`, `app.rs:1551`, `app.rs:1556`); `Reviewed` → Review (`app.rs:2242`); confirm → Install (`app.rs:1720`); GitHub auto-advance (`app.rs:2072`–`2074`).
- **Hazards:**
  - While `reviewing`, `step` stays at `Keyboard`, so step-keyed side effects (the key catch, GEN-33) stay active.
  - Back from Review leaves `review`, `prepared`, `preparation`, `erase` and `consent` stale until the next `show_review`.
  - `reached` never goes down.
- **Reify:** Medium — the enum exists, but the gating flags and side effects are scattered.
- **Related:** GEN-33, GEN-34

<a id="gen-27"></a>

### GEN-27 — Bottom-bar activity slot and timed notes
- **Where:** `installer/genkan/ui/src/app.rs:486` (`Genkan.activity`), `app.rs:487` (`.note` generation)
- **Encoding:** One `Option<Activity { busy, warning }>` shared by three producers (notes, cache warming, the review check), with a `note` counter that drops stale `ClearNote` timers.
- **States:** `Empty` | `Note(warning, n)` | `Warming(busy)` | `Warmed` | `Checking(busy)`.
- **Transitions:** `note` (`app.rs:728`–`736`); `ClearNote` (`app.rs:1973`–`1977`); `stop_warm` clears non-warnings (`app.rs:1472`–`1474`); `Warm` sets it unless a warning shows (`app.rs:2208`–`2225`); review sets "Checking…" (`app.rs:1605`–`1609`); `Reviewed` clears it (`app.rs:2237`); confirm clears it (`app.rs:1719`).
- **Hazards:** Priority rules are implicit. A note shown during "Checking…" replaces it, and after six seconds the bar is empty while the review is still running.
- **Reify:** Low — display only.
- **Related:** footer `installer/genkan/ui/src/view/mod.rs:188`

<a id="gen-28"></a>

### GEN-28 — Disk scan, selection and installed-memory read
- **Where:** `installer/genkan/ui/src/app.rs:496` (`Genkan.scanning`), `app.rs:492` (`.disks`), `app.rs:494` (`.selected_disk`), `app.rs:493` (`.firmware`), `app.rs:495` (`.memory`)
- **Encoding:** An in-flight bool for the scan; a selection index into the list; `memory: Option<MemInfo>` where `None` means both "reading" and "failed"; `firmware` defaults to `Bios` until the first scan (`app.rs:630`).
- **States:** `Scanning` → `Listed(selection none)` → `Selected(i)`; memory `Reading` → `Known` | `Failed(swap forced off)`.
- **Transitions:** `rescan` (`app.rs:877`–`889`); `Scanned` → `show_disks` resets the selection (`app.rs:1980`–`1986`, `app.rs:895`–`899`); `SelectDisk` (`app.rs:1994`–`1998`); `Memory` (`app.rs:1987`–`1993`); `Swap` (`app.rs:2002`).
- **Hazards:** "Failed" and "still reading" are the same representation (`None`), and the read is never retried.
- **Reify:** High — about five fields, and "Failed" can't be told apart from "Reading" (suspected bug).
- **Suspected bug:** If `/proc/meminfo` can't be read, `Msg::Memory(Err)` turns swap off and leaves `memory` as `None` (`app.rs:1989`–`1992`). If the person turns swap back on, the Disk page says "Reading installed memory…" and Next says "Still reading installed memory; try again in a moment." indefinitely (`app.rs:916`–`918`, `app.rs:951`–`953`) (confidence: high in code; the trigger is rare).
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#gen-28)
- **Follow-up:** reified as typestate in [#290](https://github.com/bitemyapp/sekka/pull/290). Its suspected bug is fixed, and the mistake no longer compiles.

<a id="gen-29"></a>

### GEN-29 — Derived fields that follow until edited (username, Git name, Git email)
- **Where:** `installer/genkan/ui/src/app.rs:507` (`Genkan.username_edited`), `app.rs:526` (`.git_name_edited`), `app.rs:527` (`.git_email_edited`)
- **Encoding:** Per-field bools set from "non-empty at edit time". The suggestion sources are the full name and the GitHub profile.
- **States:** `Following` ↔ `Edited` (clearing a field returns it to `Following`).
- **Transitions:** `Msg::Username` (`app.rs:2030`–`2036`); `full_name_changed` (`app.rs:1036`–`1046`); `fill_git` (`app.rs:1170`–`1177`); `Msg::GitName`/`GitEmail` (`app.rs:2086`–`2099`); GitHub result fill (`app.rs:2070`–`2071`); `git_fields` treats an unedited name without an email as no identity (`app.rs:1181`–`1189`).
- **Hazards:** none observed.
- **Reify:** Low — three independent flags, each small.

<a id="gen-30"></a>

### GEN-30 — GitHub account lookup
- **Where:** `installer/genkan/ui/src/app.rs:515` (`Genkan.github_profile`), `app.rs:516` (`.github_epoch`), `app.rs:517` (`.github_pending`), `app.rs:519` (`.github_advance`), `app.rs:520` (`.github_status`)
- **Encoding:** An epoch counter plus `Option` pending epoch to drop stale results; an "advance after lookup" bool; a profile whose validity is recomputed from the field text (`github_profile_current`, `app.rs:1056`–`1063`).
- **States:** `Empty` | `NeedsLookup` → `Looking(epoch, advance?)` → `Current(profile)` | `Failed(status)`; `Current` → `Stale` on edit.
- **Transitions:** `lookup_github` (`app.rs:1094`–`1113`); edit cancels pending and advance (`app.rs:1116`–`1135`); Next starts a lookup with advance (`app.rs:816`–`824`); `Msg::Github` epoch check, then profile and auto-forward (`app.rs:2058`–`2083`); the preview pokes `github_pending` directly (`installer/genkan/ui/src/preview.rs:107`).
- **Hazards:** A superseded `curl` still runs to completion; only its result is dropped. `advance` calls `forward()` again from inside the result handler.
- **Reify:** Medium — guarded epoch pattern, but five fields.

<a id="gen-31"></a>

### GEN-31 — Time zone detection versus manual choice
- **Where:** `installer/genkan/ui/src/app.rs:544` (`Genkan.zone`), `app.rs:547` (`.zone_manual`), `app.rs:549` (`.zone_fallback`), `app.rs:550` (`.zone_epoch`), `app.rs:551` (`.zone_pending`); also `app.rs:554` (`.internet`)
- **Encoding:** Epoch + pending detection; `manual` blocks detections from replacing a picked zone; `fallback` re-detects on entering Location once the network may be up.
- **States:** `Detecting(epoch)` → `Detected(source)` | `Fallback` | `NotOnMap` | `Error`; `Manual(zone)`.
- **Transitions:** `detect_zone` (`app.rs:1343`–`1358`); `choose_zone` (`app.rs:1361`–`1367`); `Msg::Detect` (`app.rs:2133`–`2136`); `Msg::Zone` (`app.rs:2137`–`2166`); `Msg::Internet` (`app.rs:2167`–`2172`); re-detect in `go` (`app.rs:745`–`752`); launched (`app.rs:1953`).
- **Hazards:** Toggling Internet while a detection is pending starts a second concurrent detection; the first's result is dropped but its network lookups still run. A detection whose zone isn't on the map keeps the previous zone and only changes the status line.
- **Reify:** Medium — guarded epoch pattern over six fields.

<a id="gen-32"></a>

### GEN-32 — World-map hover and click-to-pick
- **Where:** `installer/genkan/ui/src/app.rs:545` (`Genkan.zone_hover`); also `installer/genkan/ui/src/view/map.rs:301`
- **Encoding:** `Option<usize>` hover set from pointer moves, cleared on leave; a pick on `PointerUp` with no press tracking.
- **States:** `NoHover` ↔ `Hover(i)`; pick on release.
- **Transitions:** `PointerMove` (`view/map.rs:301`–`304`); `PointerLeave` (`view/map.rs:305`); `PointerUp` (`view/map.rs:306`–`312`); `Msg::HoverZone` (`app.rs:2131`).
- **Hazards:** A drag that starts elsewhere and is released over the map picks a zone. The hover index survives leaving the page.
- **Reify:** Low — tiny.

<a id="gen-33"></a>

### GEN-33 — Keyboard step: catching the key beside the space bar through the compositor
- **Where:** `installer/genkan/ui/src/app.rs:560` (`Genkan.key_check: KeyCheck`), `app.rs:564` (`.key_waits: Arc<AtomicU64>`), `app.rs:562` (`.key_error`), `app.rs:566` (`.super_key`)
- **Encoding:** Partially explicit. The `KeyCheck` enum (`app.rs:207`–`219`) holds the UI state. A shared counter serves as both the wait generation and the IPC thread's give-up signal. While a wait is open, the compositor holds back the next key from every app.
- **States:** `Asking` → `Waiting(n)` → `Caught{keyboard, beside}` | `Unavailable(why)` | `Asking` (Esc or Stop); a wrong key (space, right of space) → `Waiting(n+1)` with an error.
- **Transitions:** `listen_for_key` (`app.rs:1395`–`1407`); `stop_listening` (`app.rs:1410`–`1416`); `key_caught` (`app.rs:1419`–`1432`); `caught` (`app.rs:1438`–`1454`); `go` auto-listens and stops (`app.rs:756`–`761`); `Msg::ListenForKey` (`app.rs:2175`–`2180`); `Msg::SwapAltSuper` doesn't touch the wait (`app.rs:2183`–`2189`); `close_requested` (`app.rs:2297`).
- **Hazards:**
  - The wait is given up only by polling: the IPC thread checks every 500 ms.
  - `start_review` does not stop the wait, and `step` stays `Keyboard` until `Reviewed`.
  - Any key other than space or right-of-space counts as "the key beside the space bar" and sets `super_key = AsIs`, overriding a manual toggle.
- **Reify:** High — the catch spans components and swallows keystrokes; suspected bug.
- **Suspected bug:** Suppose the person clicks Review while the key wait is still open. The review worker's Wi-Fi snapshot (`app.rs:1614`) can raise a Secret Agent or wallet prompt, and the Polkit prompt can appear within the 500 ms poll window after `Reviewed`. The compositor holds back the first key typed into that prompt and delivers it to Genkan as the caught key. The password loses its first character, and the GUI's `super_key` silently becomes `AsIs` (confidence: low).
- **Related:** `desktop/aizome/crates/aizome-ipc/src/setup.rs:66` (`catch_key`, 500 ms poll at `setup.rs:77`–`82`); compositor `CatchKey` handling
- **Reproduction:** Partly confirmed (unit): [test and output](reproductions.md#gen-33)

<a id="gen-34"></a>

### GEN-34 — Review, background preparation and confirmation (GUI side of the helper session)
- **Where:** `installer/genkan/ui/src/app.rs:569` (`Genkan.reviewing`), `app.rs:570` (`.generation`), `app.rs:571` (`.session: Option<Session>`), `app.rs:572` (`.review`), `app.rs:574`–`575` (`.erase`, `.consent`), `app.rs:576` (`.prepared`), `app.rs:577` (`.preparation: Preparation { state: PrepState, … }`); also `app.rs:256` (`Handoff<T>`), `app.rs:482` (`.preview` bypass)
- **Encoding:** A generation counter filters `Reviewed` and `Session` messages. The `Session` is carried once through a `Handoff`; dropping it cancels. `PrepState` is explicit, but `prepared`, `session`, `reviewing`, `erase` and `consent` are separate fields. Preview mode stands in for a session.
- **States:** `Idle` → `Reviewing(gen)` → `Review{AwaitingAuth}` → `Preparing` → `Prepared` | `PrepFailed` | `Cancelled` | `HelperGone`; `Review` → `Confirmed` (allowed before `Prepared`) → GEN-35; `Review` → `Idle` via Back. Representable: `prepared && preparation.state == Failed`; `session == None` with `preparation.state == Working` (in preview).
- **Transitions:** `start_review` (`app.rs:1530`–`1625`: field checks `app.rs:1536`–`1548`; generation `app.rs:1602`–`1603`; worker parse, revalidate, Wi-Fi snapshot, `Session::start` `app.rs:1612`–`1619`); `Reviewed` (`app.rs:2228`–`2249`); `show_review` resets (`app.rs:1627`–`1641`); `session_update` Preparing, Log, Caching, Prepared, Cancelled, Failed, Finished (`app.rs:1810`–`1901`); `confirmation_ready` (`app.rs:1643`–`1649`); `confirm_install` sends the phrase and wipes the passwords (`app.rs:1689`–`1703`); Back withdraws (`app.rs:857`–`863`); `close_requested` (`app.rs:2299`).
- **Hazards:**
  - Confirmation doesn't wait for `Prepared`, or even for Polkit authorization, by design ("Finishing preparation", `app.rs:1709`–`1714`). After it, passwords are cleared and neither Back nor Close is possible, so a failure during the rest of preparation can only be handled by restarting the installer.
  - `Event::Cancelled` is ignored once `installing` is set.
  - Stale `review`, `prepared` and `preparation` values persist after Back.
- **Reify:** High — seven or more fields, about ten handlers, the privilege boundary and a suspected bug.
- **Suspected bug:** Suppose the person confirms before preparation finishes and preparation then fails (for example, a download error during the in-memory build, "the target disk has not been erased"). `show_failure` takes the `installing` branch (`app.rs:1768`–`1776`). It marks step 1 "Partition the disk" as failed, heads the page "Installation Failed", and shows "The disk may have been modified" (`installer/genkan/ui/src/view/install.rs:73`). No disk write happened, the person can't go back, and they must re-enter every password (confidence: medium).
- **Related:** `installer/genkan/rust/src/session.rs:56` (GEN-15), `installer/genkan/rust/src/helper.rs:24` (GEN-06), `installer/genkan/rust/src/install.rs:37` (`Event`)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#gen-34)
- **Follow-up:** reified as typestate in [#290](https://github.com/bitemyapp/sekka/pull/290). Its suspected bug is fixed, and the mistake no longer compiles.

<a id="gen-35"></a>

### GEN-35 — Installation progress and outcome on the Install page
- **Where:** `installer/genkan/ui/src/app.rs:580` (`Genkan.installing`), `app.rs:581` (`.started`), `app.rs:595` (`.tick`), `app.rs:583` (`.install_step`), `app.rs:584` (`.steps: [StepState; 6]`), `app.rs:585` (`.copy_fraction`), `app.rs:588`–`590` (`.completed`, `.failed`, `.failure`), `app.rs:594` (`.powering`)
- **Encoding:** Bools and counters; `steps` is derived from `install_step` except on failure. `set_install_step(7)` is the "all done" sentinel. Integer steps match the helper's `progress(n)` by convention (`INSTALL_STEPS`, `app.rs:128`–`135`; `STEP_WEIGHTS`, `app.rs:138`).
- **States:** `Confirmed(step 0)` → `Step(1..6)` → `Completed` | `Failed(installing)` → `Failed(finished)`; `Completed` → `Powering` → `PowerFailed`. Representable: `completed && failed`; `failed` with every step still pending.
- **Transitions:** `confirm_install` (`app.rs:1704`–`1718`); `Progress` → `set_install_step` (`app.rs:1855`–`1859`, `app.rs:1723`–`1735`); `Copying` (`app.rs:1860`–`1865`); `show_failure` (`app.rs:1766`–`1786`); `show_done` (`app.rs:1788`–`1793`); `Finished` (`app.rs:1876`–`1901`); `Tick` (`app.rs:2257`–`2263`); `power`/`PowerRequested` (`app.rs:1905`–`1927`, `app.rs:2271`–`2276`); close refused while `installing` (`app.rs:2291`–`2295`).
- **Hazards:**
  - `Copying` with `total == 0`, which the deferred route always sends (GEN-07), is ignored. The bar then sits at the start of step 4, the 72 % segment, for the whole download and build.
  - Close is refused for the entire time `installing` is true, including the pre-write "Finishing preparation" stretch.
  - A failure before the first `Progress` marks step 1 as failed (GEN-34).
- **Reify:** High — about ten fields, user-visible outcome, and step numbers coupled to the helper only by convention.
- **Related:** `installer/genkan/rust/src/install.rs:105` (`progress`), `install.rs:712` (copy total)

<a id="gen-36"></a>

### GEN-36 — Speculative page-cache warming of the current selection
- **Where:** `installer/genkan/ui/src/app.rs:598` (`Genkan.warm_generation`), `app.rs:599` (`.warm_cancel`), `app.rs:600` (`.warm_timer`)
- **Encoding:** A 2 s debounce timer that carries a generation, a per-run cancel flag, an unjoined thread that sends throttled `Warm` messages, and the shared activity slot (GEN-27).
- **States:** `Idle` → `Debouncing(gen)` → `Warming(gen, cancel)` → `Done(gen)`; `Stopped` (gen+1, cancel set).
- **Transitions:** `schedule_warm` (`app.rs:1479`–`1486`); `stop_warm` (`app.rs:1464`–`1475`); `WarmStart` (`app.rs:2196`–`2201`); `start_warm` (`app.rs:1488`–`1525`); `Warm` (`app.rs:2202`–`2226`); scheduled by `ToggleDesktop`, `Unfree`, `ToggleApp`, launch, Back from Review and a failed review (`app.rs:2103`, `app.rs:2114`, `app.rs:2127`, `app.rs:1954`, `app.rs:862`, `app.rs:2246`); stopped by review start and close (`app.rs:1601`, `app.rs:2296`).
- **Hazards:** Both the cancel flag and the generation encode staleness. A cancelled thread checks the flag only between files, so it can overlap the next warm and the helper's own warming (GEN-13).
- **Reify:** Medium — guarded, but triggered from many places.
- **Related:** GEN-13

<a id="gen-37"></a>

### GEN-37 — (test harness) Preview and sample-data driver
- **Where:** `installer/genkan/ui/src/preview.rs:61` (`show`); mode flag `installer/genkan/ui/src/app.rs:482` (`Genkan.preview`)
- **Encoding:** Scripted message and event sequences that push the app into a named page state. It writes fields directly (`github_pending`, `reached`, `activity`). The `preview: Option<String>` mode replaces the helper session (`app.rs:1646`, `app.rs:1691`), the timer (`app.rs:1716`), warming (`app.rs:1480`) and the key catch (`app.rs:758`).
- **States:** `Sample loaded` → page in {`welcome` … `keyboard`} | `review` → `ready` | `prepfail` → `install` → `done` | `failed`.
- **Transitions:** sample messages (`preview.rs:63`–`146`); direct writes (`preview.rs:107`, `preview.rs:147`, `preview.rs:164`); synthetic helper events (`preview.rs:205`–`301`).
- **Hazards:** Direct field writes bypass the transitions that the real flow relies on, for example the epoch match at `preview.rs:107`/`preview.rs:122`.
- **Reify:** Low — fixture.
- **Related:** `installer/genkan/ui/tests/pages.rs:15`

## Already explicit

- `installer/genkan/rust/src/plan.rs:151` `struct InstallPlan` → `plan.rs:372` `struct ConfirmedInstall` — typestate for reviewed vs confirmed intent. Built only by `RawRequest::parse` (`plan.rs:179`) and `InstallPlan::confirm` (`plan.rs:354`), and downgraded to wire data by `into_request` (`plan.rs:346`, `plan.rs:380`). The secret-taking `take_password`/`take_encryption_password` (`plan.rs:331`, `plan.rs:283`) leave a plan whose passwords are empty; that is used only inside `execute`.
- `installer/genkan/rust/src/wifi.rs:33` `enum WifiTransfer` — opt-out vs copy (possibly empty), built only in `WifiTransfer::parse` (`wifi.rs:38`); the review-time snapshot replaces it in `InstallPlan::snapshot_wifi` (`installer/genkan/rust/src/plan.rs:337`).

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `installer/genkan/nix/encrypted-splash-test.nix` | 327 | machines | [GEN-01](#gen-01) | test harness: encrypted boot and splash VM phases, splash-release marker |
| `installer/genkan/nix/encryption-test.nix` | 101 | machines | [GEN-01](#gen-01) | test harness: encrypted storage VM boot (in GEN-01) |
| `installer/genkan/nix/live-guard-probe.rs` | 17 | none |  | one-shot diagnostic probe |
| `installer/genkan/nix/org.calamares.nixos.install.policy.in` | 10 | machines | [GEN-15](#gen-15), [GEN-06](#gen-06) | polkit auth_admin_keep caches authorization for re-review |
| `installer/genkan/nix/package.nix` | 151 | none |  | build recipe |
| `installer/genkan/nix/storage-test.nix` | 41 | none |  | tests |
| `installer/genkan/nix/storage-tests.nix` | 42 | none |  | build recipe for test binary |
| `installer/genkan/rust/examples/detect-graphics.rs` | 23 | none |  | one-shot read-only example |
| `installer/genkan/rust/examples/zonemap-data.rs` | 315 | none |  | data generator |
| `installer/genkan/rust/reference.nix` | 85 | none |  | config |
| `installer/genkan/rust/src/applications.nix` | 204 | none |  | config; retired-id compat; docker linger noted in GEN-21 |
| `installer/genkan/rust/src/applications.rs` | 166 | none |  | catalog parsing and dependency closure |
| `installer/genkan/rust/src/config.rs` | 559 | machines | [GEN-02](#gen-02) | generated initrd unlock ordering; otherwise pure generation |
| `installer/genkan/rust/src/desktop.rs` | 132 | none |  | data and validation; omarchy alias |
| `installer/genkan/rust/src/disk.rs` | 582 | machines | [GEN-03](#gen-03) | identity snapshot and revalidation; layout math pure |
| `installer/genkan/rust/src/encryption.rs` | 119 | machines | [GEN-04](#gen-04) | LUKS mapping registry and drop order |
| `installer/genkan/rust/src/filesystem.rs` | 753 | machines | [GEN-05](#gen-05), [GEN-07](#gen-07) | test loop-image guard; pre-chosen identities in GEN-07 |
| `installer/genkan/rust/src/github.rs` | 481 | none |  | parsing and one-shot fetch |
| `installer/genkan/rust/src/graphics.rs` | 369 | none |  | pure sysfs detection |
| `installer/genkan/rust/src/helper.rs` | 97 | machines | [GEN-06](#gen-06) | helper session protocol and one-shot path |
| `installer/genkan/rust/src/install.rs` | 1122 | machines | [GEN-07](#gen-07), [GEN-08](#gen-08), [GEN-09](#gen-09), [GEN-10](#gen-10), [GEN-03](#gen-03), [GEN-04](#gen-04), [GEN-11](#gen-11), [GEN-12](#gen-12) | install transaction, target guard, locks, storage test matrix |
| `installer/genkan/rust/src/keyring.nix` | 72 | none |  | package wrapper |
| `installer/genkan/rust/src/lib.rs` | 204 | none |  | settings trust checks; enums |
| `installer/genkan/rust/src/memory.rs` | 68 | none |  | pure sizing |
| `installer/genkan/rust/src/nixlog.rs` | 284 | machines | [GEN-11](#gen-11) | Nix activity tracker |
| `installer/genkan/rust/src/plan.rs` | 416 | explicit |  | InstallPlan/ConfirmedInstall typestate (Already explicit) |
| `installer/genkan/rust/src/plan_tests.rs` | 629 | none |  | tests |
| `installer/genkan/rust/src/power.rs` | 64 | machines | [GEN-12](#gen-12) | logind sleep inhibitor guard |
| `installer/genkan/rust/src/precache.rs` | 276 | machines | [GEN-13](#gen-13) | page-cache warm worker pool |
| `installer/genkan/rust/src/process.rs` | 253 | machines | [GEN-14](#gen-14), [GEN-06](#gen-06) | subprocess run loop; CANCEL latch |
| `installer/genkan/rust/src/session.rs` | 158 | machines | [GEN-15](#gen-15) | GUI-side pkexec helper session handle |
| `installer/genkan/rust/src/test-data/bios-ext4.nix` | 19 | machines | [GEN-02](#gen-02) | generated fixture of GEN-02 ordering |
| `installer/genkan/rust/src/test-data/uefi-ext4.nix` | 22 | machines | [GEN-02](#gen-02) | generated fixture of GEN-02 ordering |
| `installer/genkan/rust/src/timezone.rs` | 351 | none |  | one-shot detection fallback chain |
| `installer/genkan/rust/src/wifi.rs` | 120 | explicit |  | WifiTransfer enum (Already explicit) |
| `installer/genkan/rust/src/wifi/nm.rs` | 341 | none |  | bounded one-shot D-Bus snapshot with deadline |
| `installer/genkan/rust/src/zonemap.rs` | 633 | none |  | geometry and data |
| `installer/genkan/rust/system/binary-cache.nix` | 46 | none |  | config |
| `installer/genkan/rust/system/boot-splash/default.nix` | 141 | machines | [GEN-16](#gen-16), [GEN-17](#gen-17), [GEN-18](#gen-18), [GEN-19](#gen-19) | applies Plymouth patches; UseSimpledrm and log-level config |
| `installer/genkan/rust/system/boot-splash/generate.py` | 119 | none |  | asset generator |
| `installer/genkan/rust/system/boot-splash/package.nix` | 65 | none |  | build recipe |
| `installer/genkan/rust/system/boot-splash/plymouth/abandoned-prompts.patch` | 211 | machines | [GEN-16](#gen-16) | per-connection pending answers |
| `installer/genkan/rust/system/boot-splash/plymouth/console-owner.patch` | 56 | machines | [GEN-17](#gen-17) | local console ownership flag |
| `installer/genkan/rust/system/boot-splash/plymouth/keyboard-removal.patch` | 150 | machines | [GEN-18](#gen-18) | splash keyboard attachment copies |
| `installer/genkan/rust/system/boot-splash/sekka.script` | 354 | machines | [GEN-19](#gen-19) | splash rail, prompt and hint phases |
| `installer/genkan/rust/system/default.nix` | 23 | none |  | module imports |
| `installer/genkan/rust/system/desktops.nix` | 546 | machines | [GEN-21](#gen-21) | session-env unit and PAM wiring, X11 cleanup trap; rest config |
| `installer/genkan/rust/system/fish/default.nix` | 115 | none |  | config; per-shell exec predicate |
| `installer/genkan/rust/system/fish/package.nix` | 57 | none |  | build recipe |
| `installer/genkan/rust/system/hyprland.nix` | 43 | none |  | config |
| `installer/genkan/rust/system/input-method.nix` | 107 | machines | [GEN-20](#gen-20) | IBus frontend exit classification and restart |
| `installer/genkan/rust/system/nvidia.nix` | 53 | none |  | config |
| `installer/genkan/rust/system/prebuilt-binaries.nix` | 19 | none |  | config |
| `installer/genkan/rust/system/session-env/package.nix` | 27 | none |  | build recipe |
| `installer/genkan/rust/system/session-env/src/main.rs` | 318 | machines | [GEN-21](#gen-21) | user-manager environment record and reset |
| `installer/genkan/rust/system/store-space.nix` | 13 | none |  | config thresholds |
| `installer/genkan/rust/system/tuning.nix` | 249 | none |  | config; restart policy for out-of-slice agent-workloads.py |
| `installer/genkan/rust/system/usb-wake.nix` | 42 | machines | [GEN-22](#gen-22) | udev rule and pre-sleep oneshot |
| `installer/genkan/rust/system/usb-wake.py` | 42 | machines | [GEN-22](#gen-22) | wakeup attribute latch |
| `installer/genkan/rust/system/wpa-supplicant/clear-scan-res-handler.patch` | 41 | machines | [GEN-23](#gen-23) | pending scan_res_handler |
| `installer/genkan/rust/system/wpa-supplicant/default.nix` | 18 | machines | [GEN-23](#gen-23) | applies GEN-23 patch |
| `installer/genkan/tests/vm-fixture/src/main.rs` | 1057 | machines | [GEN-24](#gen-24), [GEN-25](#gen-25) | test harness: helper session driver, fixture stages, PAM pty |
| `installer/genkan/ui/src/app.rs` | 2360 | machines | [GEN-26](#gen-26), [GEN-27](#gen-27), [GEN-28](#gen-28), [GEN-29](#gen-29), [GEN-30](#gen-30), [GEN-31](#gen-31), [GEN-32](#gen-32), [GEN-33](#gen-33), [GEN-34](#gen-34), [GEN-35](#gen-35), [GEN-36](#gen-36) | wizard, workers, review/session, install outcome, warming |
| `installer/genkan/ui/src/format.rs` | 73 | none |  | pure formatting |
| `installer/genkan/ui/src/lib.rs` | 20 | none |  | module root |
| `installer/genkan/ui/src/main.rs` | 5 | none |  | entry point |
| `installer/genkan/ui/src/media.rs` | 57 | none |  | one-shot check |
| `installer/genkan/ui/src/preview.rs` | 302 | machines | [GEN-37](#gen-37) | test harness: preview sample driver |
| `installer/genkan/ui/src/view/account.rs` | 80 | none |  | pure view |
| `installer/genkan/ui/src/view/applications.rs` | 102 | none |  | pure view |
| `installer/genkan/ui/src/view/desktop.rs` | 123 | none |  | pure view |
| `installer/genkan/ui/src/view/disk.rs` | 178 | none |  | pure view (reads GEN-28) |
| `installer/genkan/ui/src/view/github.rs` | 107 | none |  | pure view (reads GEN-30) |
| `installer/genkan/ui/src/view/install.rs` | 168 | none |  | pure view (reads GEN-35) |
| `installer/genkan/ui/src/view/keyboard.rs` | 199 | none |  | pure view (reads GEN-33) |
| `installer/genkan/ui/src/view/location.rs` | 124 | none |  | pure view (reads GEN-31) |
| `installer/genkan/ui/src/view/map.rs` | 324 | machines | [GEN-32](#gen-32) | pointer hover and pick messages |
| `installer/genkan/ui/src/view/mod.rs` | 302 | none |  | pure view (reads GEN-26, GEN-27) |
| `installer/genkan/ui/src/view/partitions.rs` | 166 | none |  | pure view and layout math |
| `installer/genkan/ui/src/view/review.rs` | 313 | none |  | pure view (reads GEN-34) |
| `installer/genkan/ui/src/view/welcome.rs` | 79 | none |  | static view |
| `installer/genkan/ui/src/zone.rs` | 120 | none |  | pure time-zone formatting |
| `installer/genkan/ui/tests/pages.rs` | 408 | none |  | tests (drive GEN-37) |
