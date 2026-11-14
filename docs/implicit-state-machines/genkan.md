# GEN: Genkan installer

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `installer/genkan`. Audited at `8e0acbe5`; line numbers refer to that commit.

83 files audited. 37 implicit machines: 7 High, 8 Medium, 22 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

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
- **Reproduction:** Confirmed (unit): [summary](reproductions.md#gen-03)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#gen-07)

<a id="gen-28"></a>

### GEN-28 — Disk scan, selection and installed-memory read
- **Where:** `installer/genkan/ui/src/app.rs:496` (`Genkan.scanning`), `app.rs:492` (`.disks`), `app.rs:494` (`.selected_disk`), `app.rs:493` (`.firmware`), `app.rs:495` (`.memory`)
- **Encoding:** An in-flight bool for the scan; a selection index into the list; `memory: Option<MemInfo>` where `None` means both "reading" and "failed"; `firmware` defaults to `Bios` until the first scan (`app.rs:630`).
- **States:** `Scanning` → `Listed(selection none)` → `Selected(i)`; memory `Reading` → `Known` | `Failed(swap forced off)`.
- **Transitions:** `rescan` (`app.rs:877`–`889`); `Scanned` → `show_disks` resets the selection (`app.rs:1980`–`1986`, `app.rs:895`–`899`); `SelectDisk` (`app.rs:1994`–`1998`); `Memory` (`app.rs:1987`–`1993`); `Swap` (`app.rs:2002`).
- **Hazards:** "Failed" and "still reading" are the same representation (`None`), and the read is never retried.
- **Reify:** High — about five fields, and "Failed" can't be told apart from "Reading" (suspected bug).
- **Suspected bug:** If `/proc/meminfo` can't be read, `Msg::Memory(Err)` turns swap off and leaves `memory` as `None` (`app.rs:1989`–`1992`). If the person turns swap back on, the Disk page says "Reading installed memory…" and Next says "Still reading installed memory; try again in a moment." indefinitely (`app.rs:916`–`918`, `app.rs:951`–`953`) (confidence: high in code; the trigger is rare).
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#gen-28)

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
- **Reproduction:** Partly confirmed (unit): [summary](reproductions.md#gen-33)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#gen-34)

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
