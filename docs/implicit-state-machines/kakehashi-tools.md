# KTL: Kakehashi, respin tools and scripts

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `apps/kakehashi, rust, scripts`. Audited at `8e0acbe5`; line numbers refer to that commit.

47 files, 0 pending. 31 implicit machines: 8 High, 10 Medium, 13 Low.

## Machines

<a id="ktl-01"></a>

### KTL-01 — Kakehashi image selection and checksum check
- **Where:** `apps/kakehashi/app/src/view.rs:75` (`Image.path`, `.sha`, `.holds`, `.check`, `.cancel`); `Kakehashi.image` `apps/kakehashi/app/src/view.rs:115`, `.error` `apps/kakehashi/app/src/view.rs:124`
- **Encoding:** partially explicit: `Option<Image>` plus `enum Check` (`apps/kakehashi/app/src/view.rs:55`); `sha: Option<String>` set only on some `Check` transitions; `holds: Option<String>` filled by a separate thread; per-image `cancel: Arc<AtomicBool>`; hash and holds threads report through `Msg::Hashed`/`HashDone`/`Holds`, keyed by `path`
- **States:** `NoImage` → `Hashing{done}` → `Verified` | `Unknown` | `Mismatch` | `Failed`; or `NoImage` → `Verified` directly for a finished download (known SHA); `holds` None → Some on its own. Representable invalid: `Mismatch` with `sha: Some(..)` left from an earlier transition (the Mismatch arm never clears `sha`); `holds: None` while the drive holding the image is offered
- **Transitions:** `choose_image` cancels the old image and sets Hashing or Verified (`apps/kakehashi/app/src/view.rs:336`, `apps/kakehashi/app/src/view.rs:345`), spawns the holds thread (`apps/kakehashi/app/src/view.rs:355`) and the hash thread (`apps/kakehashi/app/src/view.rs:362`); `Msg::Hashed` (`apps/kakehashi/app/src/view.rs:241`); `Msg::HashDone` → Verified/Mismatch/Unknown/Failed and `sha` (`apps/kakehashi/app/src/view.rs:246-261`); `Msg::Holds` (`apps/kakehashi/app/src/view.rs:266`); `start_download` cancels and clears (`apps/kakehashi/app/src/view.rs:406-408`); `Msg::Fetched` error clears (`apps/kakehashi/app/src/view.rs:287`); pick/drop call `choose_image` (`apps/kakehashi/app/src/view.rs:396`, `apps/kakehashi/app/src/view.rs:639`); a cancelled hash thread exits without a message (`apps/kakehashi/app/src/view.rs:377`)
- **Hazards:** stale results are filtered by path equality only (`apps/kakehashi/app/src/view.rs:240`, `apps/kakehashi/app/src/view.rs:245`, `apps/kakehashi/app/src/view.rs:265`), not a per-choice generation, so re-choosing the same path (or a download landing on a path still being hashed) can accept an older thread's `HashDone` that finished just before its cancel flag was set, overwriting `check`/`sha`; until `Msg::Holds` arrives, or when `image_drive` returns None, the drive holding the image is offered, and the helper's re-check uses `excluded(None, true)` (`apps/kakehashi/core/src/write.rs:62`), so HoldsImage is never re-checked
- **Reify:** Medium — five fields fed by two threads, but writing is blocked outside Verified/Unknown (`apps/kakehashi/app/src/view.rs:458-476`)

<a id="ktl-02"></a>

### KTL-02 — Kakehashi nightly download session
- **Where:** `apps/kakehashi/app/src/view.rs:85` (`Download.build`, `.fetch`, `.from`, `.cancel`); `Kakehashi.download` `apps/kakehashi/app/src/view.rs:116`
- **Encoding:** partially explicit: `Option<Download>`; `fetch: Option<Fetch>` (None = starting; `Fetch::Checking`/`Downloading`, `apps/kakehashi/core/src/nightly.rs:82`); `from: Option<(u64, Instant)>` rate origin; `cancel` flag shared with the worker thread; worker messages keyed by `build.id`
- **States:** `Idle` → `Starting` → `Checking` ↔ `Downloading` → `Idle` + image chosen | `Idle` + `error`; `Stopping` exists only as `cancel == true` while `download` is still Some
- **Transitions:** `start_download` (`apps/kakehashi/app/src/view.rs:404-411`, worker `apps/kakehashi/app/src/view.rs:413`); `Msg::Fetch` sets `from` once and `fetch` (`apps/kakehashi/app/src/view.rs:271-276`); Stop sets `cancel` (`apps/kakehashi/app/src/view.rs:789`); `Msg::Fetched` takes the session by id and chooses the image or sets `error` (`apps/kakehashi/app/src/view.rs:280-289`); a window drop clears it (`apps/kakehashi/app/src/view.rs:638`)
- **Hazards:** no generation, messages are matched by build id only (`apps/kakehashi/app/src/view.rs:270`, `apps/kakehashi/app/src/view.rs:280`); no Stopping state is shown: after Stop the session stays until workers drain (up to the 30 s range timeout, `apps/kakehashi/core/src/nightly/parallel.rs:299`); Download buttons are disabled only while `download.is_some()` (`apps/kakehashi/app/src/view.rs:711`, `apps/kakehashi/app/src/view.rs:750`)
- **Reify:** High — suspected bug; an orphaned worker can share a `.part` with a new session
- **Suspected bug:** dropping a file onto the window during a download sets `this.download = None` (`apps/kakehashi/app/src/view.rs:638`) without storing `cancel`, so the worker keeps downloading into Downloads in the background; the Download buttons re-enable, and restarting the same build runs a second worker on the same `.part` (no lock, `apps/kakehashi/core/src/nightly.rs:138`), the orphan's `Msg::Fetched` with the same id ends the new session (`apps/kakehashi/app/src/view.rs:280`), and an orphan in fallback mode truncates the shared file (`apps/kakehashi/core/src/nightly/parallel.rs:210`) (confidence: high for the orphaned download; medium for the double-writer outcome)

<a id="ktl-03"></a>

### KTL-03 — Kakehashi write job, GUI side of the helper protocol
- **Where:** `apps/kakehashi/app/src/view.rs:100` (`Job.stage`, `.phase`, `.cancel`, `.cancelling`, `.ended`); `enum Stage` `apps/kakehashi/app/src/view.rs:93`; worker in `start_write` `apps/kakehashi/app/src/view.rs:510`; also `apps/kakehashi/app/src/elevate.rs:29`, `apps/kakehashi/app/src/staged_image.rs:14`
- **Encoding:** partially explicit: `Stage` (`PreparingImage`/`Authorizing`/`Running(Progress)`/`Declined`) plus separate `ended: bool`, `cancelling: bool`, `phase: Option<(bool, u64, Instant)>`; on disk a private temp dir holding `plan.json`, an append-only `progress` file of JSON lines, a `progress.cancel` marker and, on macOS, the staged `image.iso`; the helper's exit status mapped to `Ran`
- **States:** `NoJob` → `PreparingImage` → `Authorizing` → `Running(Preparing)` → `Running(Writing)` → `Running(Verifying)` → `Running(Finished)` | `Running(Failed{touched})` | `Declined`, terminal ones with `ended = true`; `cancelling` orthogonal. Representable invalid: `ended == false` with a terminal stage, `ended == true` with `PreparingImage`, `phase` left from the other pass
- **Transitions:** `start_write` → PreparingImage (`apps/kakehashi/app/src/view.rs:529-537`); macOS staging and cancel check (`apps/kakehashi/app/src/view.rs:546`, `apps/kakehashi/app/src/view.rs:549`; staging removes its copy on failure `apps/kakehashi/app/src/staged_image.rs:31-33`); plan written (`apps/kakehashi/app/src/view.rs:552`); early failures → `JobEnded(Failed)` (`apps/kakehashi/app/src/view.rs:561`, `apps/kakehashi/app/src/view.rs:567`); `Msg::Authorizing` (`apps/kakehashi/app/src/view.rs:571`, applied `apps/kakehashi/app/src/view.rs:294`); runner thread (`apps/kakehashi/app/src/view.rs:573`) polled every 100 ms with a last read after exit (`apps/kakehashi/app/src/view.rs:576-583`); `read_progress` tails whole lines (`apps/kakehashi/app/src/view.rs:599-615`) → `Msg::Job` sets `phase` and `stage` (`apps/kakehashi/app/src/view.rs:302-308`); `cancel_write` sets `cancelling` and creates the marker (`apps/kakehashi/app/src/view.rs:592-593`); runner exit drops the temp dir (`apps/kakehashi/app/src/view.rs:585`) → `JobEnded` sets `ended` and synthesises a terminal stage if none was reported (`apps/kakehashi/app/src/view.rs:313-324`); "Write another drive" clears the job (`apps/kakehashi/app/src/view.rs:1080`); pkexec 126 → Declined, 127 or signal → Failed (`apps/kakehashi/app/src/elevate.rs:47-53`)
- **Hazards:** Stop is hidden during `Authorizing` (`apps/kakehashi/app/src/view.rs:1065`) and the helper only looks at the marker at its own checkpoints (KTL-12); creating the marker after the temp dir is gone fails silently (`apps/kakehashi/app/src/view.rs:593`); drive listing pauses while `!ended` (`apps/kakehashi/app/src/view.rs:208`); closing the window quits the app (`apps/kakehashi/app/src/main.rs:32-36`) with no guard, leaving the elevated helper writing unobserved and the temp dir, with macOS's staged image copy, unremoved; for an exit without a report, `touched` is guessed from the stage (`apps/kakehashi/app/src/view.rs:322`)
- **Reify:** High — four fields, three files and an exit code across three threads, feeding the data-loss-relevant `touched` message
- **Suspected bug:** when the helper dies by a signal mid-write on Linux (OOM killer, `kill`), `elevate::run` returns `Ran::Failed("the helper was killed")` (`apps/kakehashi/app/src/elevate.rs:51`) and `JobEnded` maps every `Ran::Failed` to `Failed { touched: false }` (`apps/kakehashi/app/src/view.rs:319`) whatever the last `Running(Writing|Verifying)` stage was, so the window says "Not written" without the "partly written" warning (confidence: high)
- **Related:** helper side `apps/kakehashi/helper/src/main.rs:111` and `apps/kakehashi/core/src/write.rs:171` (KTL-12)

<a id="ktl-04"></a>

### KTL-04 — Kakehashi drive list polling and chosen drive
- **Where:** `apps/kakehashi/app/src/view.rs:117` (`Kakehashi.drives`, `.listing`, `.listed`, `.show_all`, `.chosen`)
- **Encoding:** partially explicit: `Loading<Vec<Drive>>` + `listing: bool` in-flight guard + `listed: Instant` 2 s refresh timer + `chosen: Option<String>` drive name; the `_poll` task ticks every 100 ms (`apps/kakehashi/app/src/view.rs:153-160`)
- **States:** `Pending` → `Listing` ↔ `Ready(drives)` | `Failed`; paused while a job runs; `chosen` None ↔ Some(name). Representable invalid: `chosen` naming a device that has been replaced by another with the same name
- **Transitions:** tick refresh when no job is running (`apps/kakehashi/app/src/view.rs:208-211`); `list_drives` sets `listing` (`apps/kakehashi/app/src/view.rs:192-195`); `Msg::Drives` clears `listing`, replaces the list and clears `chosen` if its name vanished (`apps/kakehashi/app/src/view.rs:227-237`); click sets `chosen` (`apps/kakehashi/app/src/view.rs:906`); job end forces a refresh (`apps/kakehashi/app/src/view.rs:327`); "Write another drive" clears `chosen` and forces a refresh (`apps/kakehashi/app/src/view.rs:1081-1082`)
- **Hazards:** identity is the name only; a hung `lsblk`/`diskutil` keeps `listing` true forever, no timeout (`apps/kakehashi/app/src/view.rs:192`); if the chosen drive vanishes while the confirm dialog is open, OK silently does nothing (`apps/kakehashi/app/src/view.rs:511`)
- **Reify:** High — suspected bug; the selection feeds an erase and is keyed by a reusable name
- **Suspected bug:** `confirm` describes the drive captured when the dialog opens (`apps/kakehashi/app/src/view.rs:479-490`), but OK calls `start_write` (`apps/kakehashi/app/src/view.rs:502`), which re-reads `chosen_drive()` (`apps/kakehashi/app/src/view.rs:511`) from a list that keeps refreshing under the modal; if the stick is swapped for another that gets the same name (`sdb`, `disk4`) between two refreshes, the plan and the helper's `still_there` both use the new drive, and a drive other than the one named in "Erase …?" is erased (confidence: medium)

<a id="ktl-05"></a>

### KTL-05 — Resumable nightly download on disk
- **Where:** `apps/kakehashi/core/src/nightly.rs:90` (`download`: `dest`, `dest.part`, `dest.sha256`)
- **Encoding:** files: a complete `dest` of `build.bytes`, a partial `.part` whose length is the resume offset, a `.sha256` sidecar; the running SHA-256 rebuilt from the `.part` prefix
- **States:** `Absent` → `Partial(have)` → `Complete+Sidecar`; also `Complete` without sidecar, `Complete` with a wrong hash (deleted), `Partial` longer than the build (deleted)
- **Transitions:** complete file re-hashed, then sidecar written or file removed (`apps/kakehashi/core/src/nightly.rs:104-111`); oversize `.part` removed (`apps/kakehashi/core/src/nightly.rs:115-118`); prefix re-hashed (`apps/kakehashi/core/src/nightly.rs:120-135`); append handle (`apps/kakehashi/core/src/nightly.rs:138-139`); probe → fallback or parallel transfer (`apps/kakehashi/core/src/nightly.rs:144-148`, KTL-07, KTL-08); mismatch deletes `.part` (`apps/kakehashi/core/src/nightly.rs:153-156`); rename and sidecar (`apps/kakehashi/core/src/nightly.rs:157-158`)
- **Hazards:** no lock on `.part`, so two concurrent downloads of one build (reachable through KTL-02) share it; a corrupt prefix is found only after the whole transfer, then everything is deleted; a `dest` of another length (a user's own file of that name) is overwritten by the rename; a build pruned upstream (KTL-29) turns every resume into a "try again to resume" error while the `.part` stays
- **Reify:** Medium — phase encoded by file presence and length, guarded by the final hash
- **Related:** `scripts/publish-nightly.py:362` prunes builds a window may be resuming

<a id="ktl-06"></a>

### KTL-06 — Adaptive range-connection controller
- **Where:** `apps/kakehashi/core/src/nightly/parallel.rs:23` (`Adaptive.limit`, `.best_limit`, `.best_rate`, `.flat`, `.stopped`, `.since`, `.bytes`, `.settling`)
- **Encoding:** `settling` and `stopped` bools, `flat` counter, sample window `since`/`bytes`
- **States:** `Settling(limit)` → `Sampling(limit, flat)` → `Settling(2·limit)` | `Stopped(best_limit)` | `Stopped(MAX)`; `stopped && settling` representable but unreachable
- **Transitions:** all in `observe`: settle end (`apps/kakehashi/core/src/nightly/parallel.rs:44-50`); improvement (`apps/kakehashi/core/src/nightly/parallel.rs:58-61`); flat sample (`apps/kakehashi/core/src/nightly/parallel.rs:63-66`); back off to best and stop (`apps/kakehashi/core/src/nightly/parallel.rs:67-71`); ceiling stop (`apps/kakehashi/core/src/nightly/parallel.rs:73-74`); double and resettle (`apps/kakehashi/core/src/nightly/parallel.rs:76-78`); driven from the transfer loop (`apps/kakehashi/core/src/nightly/parallel.rs:357-359`)
- **Hazards:** lowering `limit` below `pending.len()` only stops refilling; `Stopped` is final even if throughput later changes (by design)
- **Reify:** Low — small, transitions centralised in `observe`, unit-tested

<a id="ktl-07"></a>

### KTL-07 — Range-support mode: probe, sequential fallback, recheck
- **Where:** `apps/kakehashi/core/src/nightly/parallel.rs:149` (`probe` attempt loop); `fallback_with_interval` `apps/kakehashi/core/src/nightly/parallel.rs:195` (`checked`, `done`)
- **Encoding:** control flow plus `attempt` counter (0..3), `checked: Instant` recheck timer (30 s) and the open full-body response
- **States:** `Probing(attempt)` → `Parallel` (KTL-08) | `Sequential` → `Parallel` (recheck succeeded) | `Done` | `Failed`
- **Transitions:** probe attempts with backoff (`apps/kakehashi/core/src/nightly/parallel.rs:151-161`); 200 three times → full GET (`apps/kakehashi/core/src/nightly/parallel.rs:169`); fallback truncates the `.part` and resets the hasher (`apps/kakehashi/core/src/nightly/parallel.rs:210-212`); periodic `probe_once` (`apps/kakehashi/core/src/nightly/parallel.rs:233-250`); recovery hands over to `transfer` at `done` (`apps/kakehashi/core/src/nightly/parallel.rs:241-245`); short body fails (`apps/kakehashi/core/src/nightly/parallel.rs:252`)
- **Hazards:** falling back discards the resumable prefix; the full GET sets no per-call timeout (`apps/kakehashi/core/src/nightly/parallel.rs:168-169`), so a stalled server blocks `body.read` (`apps/kakehashi/core/src/nightly/parallel.rs:224`) and Stop, checked only between reads, cannot end the download; an error on the third probe fails the download (`apps/kakehashi/core/src/nightly/parallel.rs:158`)
- **Reify:** Medium — mode switch hidden in control flow, recovering into another function

<a id="ktl-08"></a>

### KTL-08 — Ordered range-commit pipeline with per-range retry
- **Where:** `apps/kakehashi/core/src/nightly/parallel.rs:256` (`Pending.received`, `.data`); `transfer_with_chunk` locals `stop`, `pending`, `committed`, `next` (`apps/kakehashi/core/src/nightly/parallel.rs:342-349`); retry in `range` (`apps/kakehashi/core/src/nightly/parallel.rs:261`)
- **Encoding:** `BTreeMap<u64, Pending>` of in-flight and received ranges, `committed`/`next` cursors, shared `stop: AtomicBool`, scoped workers reporting over mpsc, attempt counter in `range`
- **States:** per range `Scheduled` → `Fetching(attempt)` → `Received` → `Committed` | `Failed`; transfer `Running` → `Draining` (`next == bytes`) → `Done` | `Stopping`
- **Transitions:** spawn while under the limit (`apps/kakehashi/core/src/nightly/parallel.rs:369-384`); result recorded (`apps/kakehashi/core/src/nightly/parallel.rs:387`); contiguous commit to file and hasher (`apps/kakehashi/core/src/nightly/parallel.rs:388-393`); retry with backoff (`apps/kakehashi/core/src/nightly/parallel.rs:270-283`); cancel or error → `stop`, scope joins (`apps/kakehashi/core/src/nightly/parallel.rs:404`)
- **Hazards:** a range failing three times aborts the transfer and drops completed but uncommitted ranges (prefix-only by design); Stop waits for the scope join, bounded by the 30 s request timeout (`apps/kakehashi/core/src/nightly/parallel.rs:299`); up to `limit` × 16 MiB held in memory
- **Reify:** Medium — window state across a map, cursors and threads, but local and well tested

<a id="ktl-09"></a>

### KTL-09 — (test harness) Fault-injecting range HTTP server
- **Where:** `apps/kakehashi/core/src/nightly/parallel_tests.rs:30` (`Server.requests`, `.peak`, `.stop`, `.thread`); `enum Mode` `apps/kakehashi/core/src/nightly/parallel_tests.rs:11`
- **Encoding:** the shared request log decides each response: per-range `count` and global `probes` (`apps/kakehashi/core/src/nightly/parallel_tests.rs:112-116`); `stop` flag for the accept loop; `active` connection counter
- **States:** per-mode sequences keyed by request history, e.g. `FailOnce` 502 then ranges, `IgnoreProbes(n)` 200 for the first n probes, `FailedRecheck` 200 three times then 502 on the fourth probe; recovery fixtures hold the full body until the client's Nth probe arrives (`apps/kakehashi/core/src/nightly/parallel_tests.rs:152-164`)
- **Transitions:** accept loop until `stop` (`apps/kakehashi/core/src/nightly/parallel_tests.rs:52-68`); fault selection (`apps/kakehashi/core/src/nightly/parallel_tests.rs:118-129`); Drop stops and joins (`apps/kakehashi/core/src/nightly/parallel_tests.rs:88-93`)
- **Hazards:** behaviour depends on request order across concurrent handlers; the 2 s hold loop (`apps/kakehashi/core/src/nightly/parallel_tests.rs:155-161`) makes recovery fixtures timing-sensitive
- **Reify:** Low — test fixture

<a id="ktl-10"></a>

### KTL-10 — macOS authopen descriptor broker
- **Where:** `apps/kakehashi/core/src/platform/macos/authopen.rs:28` (`authorized_file`: `child`, `socket`, `received`)
- **Encoding:** child process plus socketpair with a 100 ms read timeout; loop result; kill on error
- **States:** `Spawned` → `Waiting` → `Received(fd)` | `Cancelled` | `Failed` → `Reaped` → exit status checked
- **Transitions:** cancel precheck (`apps/kakehashi/core/src/platform/macos/authopen.rs:29`); spawn and drop our sender copy (`apps/kakehashi/core/src/platform/macos/authopen.rs:35-38`); poll loop (`apps/kakehashi/core/src/platform/macos/authopen.rs:39-53`); kill on error (`apps/kakehashi/core/src/platform/macos/authopen.rs:54-56`); wait (`apps/kakehashi/core/src/platform/macos/authopen.rs:57`); cancelled returns first (`apps/kakehashi/core/src/platform/macos/authopen.rs:58-60`); status gate (`apps/kakehashi/core/src/platform/macos/authopen.rs:61-66`)
- **Hazards:** once a descriptor has arrived, `wait_with_output` (`apps/kakehashi/core/src/platform/macos/authopen.rs:57`) has no timeout and Stop is no longer polled, so an authopen that never exits hangs the helper
- **Reify:** Low — one function
- **Related:** called from `apps/kakehashi/core/src/platform/macos.rs:161` inside KTL-12

<a id="ktl-11"></a>

### KTL-11 — Windows volume lock, dismount and hold
- **Where:** `apps/kakehashi/core/src/platform/windows.rs:36` (`Held = Vec<OwnedHandle>`), `open` `apps/kakehashi/core/src/platform/windows.rs:284`; `Device.held` `apps/kakehashi/core/src/platform/mod.rs:38`
- **Encoding:** vector of locked volume handles owned by `Device`; lock retry loop; layout deletion whose error is ignored
- **States:** per volume `Mounted` → `Locked` (≤ 20 tries × 250 ms) → `Dismounted` → `Held` → released when `Device` drops; drive `LayoutIntact` → `LayoutDeleted` at open
- **Transitions:** enumerate volumes (`apps/kakehashi/core/src/platform/windows.rs:287`); lock loop (`apps/kakehashi/core/src/platform/windows.rs:291-299`); dismount (`apps/kakehashi/core/src/platform/windows.rs:300`); hold (`apps/kakehashi/core/src/platform/windows.rs:301`); open the physical drive (`apps/kakehashi/core/src/platform/windows.rs:303-308`); delete layout (`apps/kakehashi/core/src/platform/windows.rs:309`); release on device drop (`apps/kakehashi/core/src/write.rs:231`); eject reopens and updates properties (`apps/kakehashi/core/src/platform/windows.rs:323-331`)
- **Hazards:** a later volume failing to lock drops `held`, releasing volumes already dismounted; the partition table is deleted inside "open", before the helper marks the drive touched (KTL-12)
- **Reify:** Medium — lock lifetime implicit in `Device`'s lifetime, a destructive step hidden in `open`

<a id="ktl-12"></a>

### KTL-12 — Helper write/verify pipeline and `touched`
- **Where:** `apps/kakehashi/core/src/write.rs:171` (`execute`, `touched` parameter); `enum Progress` `apps/kakehashi/core/src/write.rs:33`; helper loop `apps/kakehashi/helper/src/main.rs:111-130` (`cancel`, `last`, `touched`); also `apps/kakehashi/core/src/platform/linux.rs:95`, `apps/kakehashi/core/src/platform/macos.rs:148`, `apps/kakehashi/core/src/platform/mod.rs:64`
- **Encoding:** partially explicit: `Progress` is the wire stage, but phases are control flow in `execute`, with `touched: &mut bool`, a throttle `last: Instant`, the `.cancel` marker polled by `cancel.exists()` and exit codes 0/1/2
- **States:** `Preparing` (re-check, unmount, open, re-check, fit) → `Writing` → `TailClearedSynced` → `Verifying` → `TailChecked` → `Ejecting` → `Finished{ejected}` | `Failed{touched}`
- **Transitions:** Preparing emitted (`apps/kakehashi/core/src/write.rs:177`); `still_there` (`apps/kakehashi/core/src/write.rs:178`); unmount (`apps/kakehashi/core/src/write.rs:182`); open (`apps/kakehashi/core/src/write.rs:183`); size and second `still_there` (`apps/kakehashi/core/src/write.rs:184-186`); cancel check (`apps/kakehashi/core/src/write.rs:196`); `*touched = true` (`apps/kakehashi/core/src/write.rs:199`); copy (`apps/kakehashi/core/src/write.rs:200-207`); SHA check (`apps/kakehashi/core/src/write.rs:208`); tail, sync, drop cache (`apps/kakehashi/core/src/write.rs:209-214`); verify (`apps/kakehashi/core/src/write.rs:218-226`); tail check (`apps/kakehashi/core/src/write.rs:227-230`); release device (`apps/kakehashi/core/src/write.rs:231`); eject outcome (`apps/kakehashi/core/src/write.rs:233-236`); Finished or Failed emitted, then exit (`apps/kakehashi/helper/src/main.rs:131-145`); bad plan → Failed untouched (`apps/kakehashi/helper/src/main.rs:103-108`); bad arguments or progress file → exit 2 with no report (`apps/kakehashi/helper/src/main.rs:90`, `apps/kakehashi/helper/src/main.rs:97`)
- **Hazards:** cancellation is first checked after unmount and open (`apps/kakehashi/core/src/write.rs:196`), so Stop during Preparing still unmounts the user's filesystems (`apps/kakehashi/core/src/platform/linux.rs:95-103`) and, on Windows, deletes the partition table; Linux unmount stops at the first failure, leaving earlier partitions unmounted; the throttle always lets stage changes through (`apps/kakehashi/helper/src/main.rs:118-126`)
- **Reify:** High — destructive phases with a hand-maintained `touched` flag that the GUI's warning depends on
- **Suspected bug:** on Windows `platform::open` deletes the drive layout (`apps/kakehashi/core/src/platform/windows.rs:309`) before `*touched = true` (`apps/kakehashi/core/src/write.rs:199`); a failed size check, second `still_there`, fit check, or a Stop seen at `apps/kakehashi/core/src/write.rs:196` then reports `Failed { touched: false }`, with no "partly written" warning, although the partition table is already gone (confidence: medium)
- **Related:** GUI side `apps/kakehashi/app/src/view.rs:599` (KTL-03)

<a id="ktl-13"></a>

### KTL-13 — Rootless builder VM host driver
- **Where:** `rust/src/builder.rs:61` (`main`: `builder.lock`, `serial.sock`, `qemu`, `sent`, `deadline`, `stop`); `RemoveOnDrop` `rust/src/builder.rs:22`
- **Encoding:** lock file created with `create_new`, removed on drop; serial socket existence; persistent `ssh-key`, `builder.raw` and `known-hosts` in `.work/rootless`; `sent: bool` console handshake flag; one 600 s `deadline` for boot and setup; drain thread `stop` flag; QEMU as a `Process` (KTL-17)
- **States:** `Locked` → `QemuStarting` → `AwaitPrompt` → `PrepareSent` → `Ready` → `Building` → `PoweringOff` → `Reaped` → `Unlocked`
- **Transitions:** lock (`rust/src/builder.rs:89-95`); socket check (`rust/src/builder.rs:97-101`); key and disk created once (`rust/src/builder.rs:116-133`); port reserved then released (`rust/src/builder.rs:152-154`); QEMU spawned (`rust/src/builder.rs:156-208`); wait for socket (`rust/src/builder.rs:211-217`); prompt → send prepare, `sent = true` (`rust/src/builder.rs:245-251`); READY → leave loop (`rust/src/builder.rs:252-253`); drain thread (`rust/src/builder.rs:259-273`); build over ssh (`rust/src/builder.rs:306-308`); poweroff and QEMU wait (`rust/src/builder.rs:311-316`); stop drain (`rust/src/builder.rs:320-321`)
- **Hazards:** SIGKILL leaves `builder.lock` and `serial.sock`, blocking later runs until removed by hand; the reserved port can be taken before QEMU binds it; a failing `prepare-builder` is noticed only at the 600 s deadline; the persisted `known-hosts` with `accept-new` meets fresh live-ISO host keys, so a reused random port fails ssh in BatchMode; if poweroff fails, QEMU is SIGTERMed with the persistent store mounted
- **Reify:** High — suspected bug; persisted markers plus a console handshake
- **Suspected bug:** the host waits for the guest's whole `build-iso` through `run()` (`rust/src/builder.rs:306`), capped at 7200 s (`rust/src/support.rs:67`), while the guest runs `nix flake check` and `nix build .#iso` under separate 7200 s caps (`rust/src/builder.rs:396`, `rust/src/builder.rs:429`) with `--max-jobs 1`; a cold build whose two steps together pass two hours is killed from the host ("Child process timed out") although each guest step is within its own limit (confidence: medium)
- **Related:** guest side `rust/src/builder.rs:327` (KTL-14)

<a id="ktl-14"></a>

### KTL-14 — Builder guest disk and store preparation markers
- **Where:** `rust/src/builder.rs:327` (`prepare`: `/dev/vda` filesystem, `/build/store-ready`)
- **Encoding:** on the persistent builder disk: ext4 label `respin-build` (presence tested with `blkid`), marker `/build/store-ready`, copied `/build/nix`; at runtime a bind mount over `/nix` and stopped/restarted daemon sockets; `RESPIN_BUILDER_READY` printed for KTL-13
- **States:** `Blank` → `Formatted` → `StoreCopying` → `StoreReady` → `BoundAndServing`
- **Transitions:** format when `blkid` fails (`rust/src/builder.rs:345-347`); mount (`rust/src/builder.rs:354`); stop daemons (`rust/src/builder.rs:355-360`); copy and mark (`rust/src/builder.rs:361-365`); bind and restart (`rust/src/builder.rs:366-367`); ready line (`rust/src/builder.rs:386`)
- **Hazards:** any `blkid` failure, not only "no filesystem", reformats the persistent disk; the marker is not keyed to the bootstrap ISO (`--sha256` selects another, `rust/src/builder.rs:68`), so a different live image gets an older store and database bound over its own `/nix`; a failure between stopping and restarting the daemons leaves Nix down for the session
- **Reify:** Medium — persisted marker phases with no version

<a id="ktl-15"></a>

### KTL-15 — Wi-Fi test tools NAR cache
- **Where:** `rust/src/builder.rs:459` (`export_wifi_tools`: `artifacts/wifi-tools/manifest.json`, `closure.nar`)
- **Encoding:** manifest `{store_path, sha256, lock_sha256}` vouches for the NAR; the consumer re-checks it
- **States:** `Absent` → `Exported` (valid) → `Stale` (path, lock or hash differ) → re-exported; `Truncated` NAR beside an old manifest after an interrupted export
- **Transitions:** reuse check (`rust/src/builder.rs:482-491`); truncate and export (`rust/src/builder.rs:492-497`); manifest written last (`rust/src/builder.rs:498-504`); consumer validation (`rust/src/wifi_test.rs:30-46`)
- **Hazards:** an interrupted export leaves a mismatched pair that fails closed in the Wi-Fi test until the next build; each reuse check hashes the whole NAR
- **Reify:** Low — explicit, fail-closed validity check

<a id="ktl-16"></a>

### KTL-16 — (test harness) Live boot and input-delivery test phases
- **Where:** `rust/src/live_test.rs:85` (`main`: `qmp`, `result`, per-phase deadlines); xev log parser `rust/src/live_test.rs:44` (`event`, `keys`, `clicked`); `probe_focused` `rust/src/live_test.rs:68`
- **Encoding:** control flow with `Instant` deadlines; one long-lived QMP connection (the monitor admits one client); `result` JSON with `passed`; the guest's `/tmp/live-input.log`; the parser carries the current `event` across lines
- **States:** `FirmwareMenu` → `ProfileSelected` → `AgentUp` → `DesktopVerified` → `ProbeOpen` → `ProbeFocused` → `InputSent` → `Verified` → `PoweredOff` → `Recorded`
- **Transitions:** serial marker wait (`rust/src/live_test.rs:134-141`); menu keys (`rust/src/live_test.rs:144-152`); agent (`rust/src/live_test.rs:153`); desktop retry (`rust/src/live_test.rs:159-164`); start xev (`rust/src/live_test.rs:202`); window wait (`rust/src/live_test.rs:204-211`); focus loop pressing ❖Tab (`rust/src/live_test.rs:214-227`); pointer selection (`rust/src/live_test.rs:228-244`); click and typing (`rust/src/live_test.rs:255-264`); check (`rust/src/live_test.rs:269-272`); poweroff and pass (`rust/src/live_test.rs:278-279`); failure diagnostics, stop, result (`rust/src/live_test.rs:282-298`)
- **Hazards:** the focus loop can move focus away again between checks (bounded by 30 s); any second monitor client while `qmp` is held would block (avoided by reusing it)
- **Reify:** Low — linear fixture with deadlines

<a id="ktl-17"></a>

### KTL-17 — Child supervision and interrupt latch
- **Where:** `rust/src/support.rs:17` (`INTERRUPTED`), `rust/src/support.rs:83` (`Process`)
- **Encoding:** global `OnceLock<Arc<AtomicBool>>` set by SIGINT/SIGTERM and never cleared; `Process(Child)` with a `Drop` escalation; timeouts in `wait`, `wait_cleanup` and `run`
- **States:** latch `Unarmed` → `Armed` → `Interrupted`; child `Running` → `Exited` | (drop) `Terminating` (SIGTERM, ≤ 15 s) → `Killed` → `Reaped`
- **Transitions:** arm (`rust/src/support.rs:19-27`); consumers fail once set (`rust/src/support.rs:30-36`; `pause` `rust/src/support.rs:41`; `output` `rust/src/support.rs:73`; `wait` `rust/src/support.rs:100`); `run` cap 7200 s (`rust/src/support.rs:67`); `wait_cleanup` ignores the latch (`rust/src/support.rs:87-96`); Drop SIGTERM → 150 × 100 ms → SIGKILL (`rust/src/support.rs:111-125`)
- **Hazards:** once the latch is set every later `pause`/`wait`/`Rpc::call` fails at once, so graceful guest poweroff and diagnostics are skipped and QEMU is SIGTERMed from Drop; `output()` (`rust/src/support.rs:72-81`) has no timeout and cannot be interrupted; each dropped `Process` can block 15 s
- **Reify:** Medium — shared by every driver; the latch's effect on cleanup is implicit

<a id="ktl-18"></a>

### KTL-18 — (test harness) QMP/QGA request correlation and guest-exec polling
- **Where:** `rust/src/vm.rs:16` (`IDS`), `Rpc` `rust/src/vm.rs:17`; `Vm::execute` `rust/src/vm.rs:415`; `guest_exec` `rust/src/vm.rs:977`
- **Encoding:** process-global id counter; 30 s deadline per call; responses with other ids skipped; guest-exec `pid` polled with `guest-exec-status`
- **States:** per call `Sent(id)` → `Awaiting` → `Answered` | `Error` | `TimedOut`; guest command `Started(pid)` → `Polling` → `Exited(code)` | `TimedOut`
- **Transitions:** QMP greeting and capabilities (`rust/src/vm.rs:28-31`); send (`rust/src/vm.rs:42-50`); skip stale (`rust/src/vm.rs:69-71`); shutdown without reply (`rust/src/vm.rs:51-57`); execute poll and timeout (`rust/src/vm.rs:417-436`); `guest_exec` loop (`rust/src/vm.rs:996-1010`)
- **Hazards:** the deadline is checked only between reads, so a call can take 30 s plus the socket timeout; ids restart at 1 in every process, so a separate `guest-exec` or `qmp-input` process can take a stale reply left by an earlier process with the same id; a timed-out guest command keeps running; `guest_exec` has no timeout
- **Reify:** Low — small protocol helper

<a id="ktl-19"></a>

### KTL-19 — (test harness) QEMU VM handle lifecycle
- **Where:** `rust/src/vm.rs:84` (`Vm.sockets`, `.process`, `.agent`); `work/sockets` link name `rust/src/vm.rs:93`
- **Encoding:** three `Option`s plus a symlink in the run directory through which other processes find the VM (`run_socket` `rust/src/vm.rs:117-121`); per-run `OVMF_VARS.fd`
- **States:** `Started` (process, sockets, link) → `AgentConnected` → `PoweringOff` → `Stopped`; representable: `agent` Some with `process` None
- **Transitions:** refuse when the link exists (`rust/src/vm.rs:205-209`); spawn and link (`rust/src/vm.rs:355-369`); `wait_agent` retries until the deadline or QEMU exits (`rust/src/vm.rs:379-414`, agent set `rust/src/vm.rs:403`); `poweroff` request, wait, stop (`rust/src/vm.rs:441-453`); `stop` clears the agent, drops the process and removes the link only if it is still ours (`rust/src/vm.rs:455-465`); Drop (`rust/src/vm.rs:476-480`)
- **Hazards:** a crashed run leaves the `sockets` link, which blocks reusing that work directory; `stop` after a failed poweroff SIGTERMs QEMU without a guest shutdown
- **Reify:** Medium — three options plus an external marker

<a id="ktl-20"></a>

### KTL-20 — (test harness) qemu-test install/reboot run and resumable evidence
- **Where:** `rust/src/vm.rs:759` (`main`: `result`, `vm`); `boot_installed` `rust/src/vm.rs:1013`; `unlock_disk` `rust/src/vm.rs:546`; `verify_installed` `rust/src/vm.rs:668`; `record` `rust/src/vm.rs:737`
- **Encoding:** persisted evidence: `artifacts/RUN/result.json` (`passed`, `backend_completed`, `install`, `gui`, `encrypted`, `media_lock_sha256`, `error`/`prior_error`, `resumed_from`), `install.log` markers (`{"kind":"complete"}`, `GUI_INSTALL_HELPER_STARTED`/`EXITED`), `.work/RUN/target.raw`, `OVMF_VARS.fd`; CALAMARES_TEST_* variables read again at verify time
- **States:** `LiveBoot` → `LiveVerified` → [`FixturePrepared` → `Installing(GUI | backend)` → `Installed`] → `LiveWifiChecked` → `BackendCompleted` → `PoweredOff` → `DiskBoot` → (`Unlocked`) → `InstalledVerified` → `Recorded(passed)`; `boot_installed`: evidence gate → `DiskBoot` → … → `Recorded`
- **Transitions:** initial result (`rust/src/vm.rs:845-848`); live retry loop (`rust/src/vm.rs:862-866`); GUI helper start/exit waits and markers (`rust/src/vm.rs:898-912`); backend install (`rust/src/vm.rs:915-922`); `backend_completed` (`rust/src/vm.rs:934`); poweroff and disk boot (`rust/src/vm.rs:935-945`); unlock (`rust/src/vm.rs:947-949`); verify (`rust/src/vm.rs:950-958`); record (`rust/src/vm.rs:744-754`); resume gate (`rust/src/vm.rs:1025-1033`); OVMF variables carried over (`rust/src/vm.rs:1061-1066`); `passed = false`, `prior_error`, `resumed_from` (`rust/src/vm.rs:1070-1078`)
- **Hazards:** in GUI mode `GUI_INSTALL_HELPER_EXITED` and `backend_completed` are written whatever the helper's outcome (`rust/src/vm.rs:909-912`, `rust/src/vm.rs:934`), so the resume gate admits failed GUI installs; `result.json` is written only by `record`, so a killed run cannot be resumed; waits of up to 15000 s
- **Reify:** High — suspected bug; phase evidence spread across files and the environment
- **Suspected bug:** `boot_installed` rebuilds the fixture's `verify` command from the current CALAMARES_TEST_* environment (`fixture_options()` at `rust/src/vm.rs:687`), but `result.json` records only `encrypted` (`rust/src/vm.rs:848`); a run installed with `CALAMARES_TEST_GITHUB` set and resumed in a fresh shell silently skips the fixture's SSH-key, sshd and Git assertions (`installer/genkan/tests/vm-fixture/src/main.rs:831`), and the reverse fails a good install (confidence: medium)
- **Related:** fixture `installer/genkan/tests/vm-fixture/src/main.rs:662` (`verify`)

<a id="ktl-21"></a>

### KTL-21 — (test harness) Wi-Fi test: host boot and guest simulated-AP lifecycle
- **Where:** `rust/src/wifi_test.rs:163` (`guest`: `/run/respin-wifi-test`, netns `respin-ap`, hostapd `process`); host `main` `rust/src/wifi_test.rs:17`
- **Encoding:** control flow with deadlines; marker directory created with `create_dir` (fails if present); kernel and NetworkManager state (`mac80211_hwsim` loaded, netns, connection `respin-wifi`); hostapd as a `Process`; `report.log` markers (`SCAN_FOUND`, `CONNECTION_ATTEMPT`, `WIFI_SCAN_CONNECT_RECONNECT_PASS`)
- **States:** guest `Guarded` → `RadiosCreated` → `NetnsCreated` → `ApEnabled` → `Scanned` → `Connected(1)` → `Down` → `Connected(2)` → `Down` → `TornDown` → `Pass`; host `Boot` → `AgentUp` → `ToolsImported` → `GuestTestRun` → `PoweredOff` → `Recorded`
- **Transitions:** guards (`rust/src/wifi_test.rs:164-183`); marker directory (`rust/src/wifi_test.rs:185`); supplicant activation (`rust/src/wifi_test.rs:189-206`); hwsim (`rust/src/wifi_test.rs:208`); netns (`rust/src/wifi_test.rs:228`); hostapd and AP-ENABLED wait (`rust/src/wifi_test.rs:252-266`); scan loop (`rust/src/wifi_test.rs:268-286`); connect/reconnect attempts (`rust/src/wifi_test.rs:316-341`); teardown (`rust/src/wifi_test.rs:351`, `rust/src/wifi_test.rs:358-360`); host boot, import, run, log (`rust/src/wifi_test.rs:70-114`)
- **Hazards:** one shot per boot: the marker directory, the loaded hwsim radios and the leftover `respin-wifi` connection make a rerun in the same guest refuse; a host-side timeout (300 s) leaves the guest test running
- **Reify:** Low — disposable-VM fixture

<a id="ktl-22"></a>

### KTL-22 — PR trust gate merge-commit polling
- **Where:** `scripts/check-pr-trust.py:45` (`select_merge`, `attempt`)
- **Encoding:** retry counter over GitHub's `mergeable` (None/True/False) and `merge_commit_sha` and the live `main` ref; result written as `allowed=true|false` plus SHAs to `GITHUB_OUTPUT`
- **States:** `Checking(attempt)` → `Admitted(head, base, merge)` | `Rejected(reason)`; `mergeable == null` or stale parents ⇒ retry
- **Transitions:** actor checks (`scripts/check-pr-trust.py:48-49`); PR re-read with author and head checks each attempt (`scripts/check-pr-trust.py:51-61`); conflict rejects (`scripts/check-pr-trust.py:63-64`); matching parents admit (`scripts/check-pr-trust.py:65-74`); sleep (`scripts/check-pr-trust.py:75-76`); give up (`scripts/check-pr-trust.py:77`); outputs (`scripts/check-pr-trust.py:87-100`)
- **Hazards:** actor permissions are checked once, before the loop; 6 × 2 s is the whole wait for GitHub's mergeability computation
- **Reify:** Low — bounded and local
- **Related:** `.github/workflows/pr.yml:49` and `.github/workflows/pr.yml:87` (run before and after checkout)

<a id="ktl-23"></a>

### KTL-23 — CI GC-root generation retention
- **Where:** `scripts/ci-retain.py:56` (`building`), `prune` `scripts/ci-retain.py:71`, `main` `scripts/ci-retain.py:137`
- **Encoding:** per-group directories named `TIMESTAMP-random`; a `.building` file whose flock shows liveness; a `success` marker; a tree-wide `.lock`; `result*` and `dependencies/` symlinks as roots; a free-space threshold that changes the keep counts
- **States:** `Building` (flock held) → `Succeeded` | `Failed` → `Expired` (removed); in low-space mode only the newest successful main generation per group survives
- **Transitions:** create and lock under the tree lock (`scripts/ci-retain.py:163-173`); nix run (`scripts/ci-retain.py:175`); root inputs (`scripts/ci-retain.py:179`, symlinks `scripts/ci-retain.py:128`); success marker (`scripts/ci-retain.py:182-183`); release (`scripts/ci-retain.py:185`); prune before and after (`scripts/ci-retain.py:164`, `scripts/ci-retain.py:186-187`); keep rules (`scripts/ci-retain.py:86`, `scripts/ci-retain.py:99-103`); expire (`scripts/ci-retain.py:106`); live builds skipped (`scripts/ci-retain.py:94-95`)
- **Hazards:** age comes from the creation time in the name, so a failed build that ran longer than a day is expired by its own post-build prune; between nix exiting and `retain_dependencies` the build inputs are unrooted while the daemon's min-free collection may run; in low-space mode a just-finished PR generation is removed at once
- **Reify:** Medium — persisted markers and lock liveness; rules spread over count, age and free space
- **Related:** `nix/runner.nix:82` (prune at admission), `.github/workflows/pr.yml:121`, `.github/workflows/nightly.yml:100`

<a id="ktl-24"></a>

### KTL-24 — flash_usb privileged write and verify step
- **Where:** `scripts/flash_usb.rs:619` (`write`); `Plan` `scripts/flash_usb.rs:568`; `still_there` `scripts/flash_usb.rs:603`
- **Encoding:** control flow in the sudo re-exec; plan JSON on argv; report `<image>.flash.json` written only on success
- **States:** `Rechecked` → `Unmounted` → `Opened` → `Rechecked` → `Writing` → `TailClearedSynced` → `Verifying` → `TailChecked` → `Reported` → `Ejected`
- **Transitions:** re-check (`scripts/flash_usb.rs:620`); unmount (`scripts/flash_usb.rs:621`); open and size (`scripts/flash_usb.rs:622-623`); re-check (`scripts/flash_usb.rs:625`); sector and fit checks (`scripts/flash_usb.rs:628-629`); copy and SHA (`scripts/flash_usb.rs:632-634`); tail and sync (`scripts/flash_usb.rs:635-638`); verify (`scripts/flash_usb.rs:643-645`); tail check (`scripts/flash_usb.rs:646-649`); report and chown (`scripts/flash_usb.rs:664-673`); eject (`scripts/flash_usb.rs:674-677`)
- **Hazards:** no partial-write state: any error after the copy starts leaves a partly written drive with only the error text; a previous run's `<image>.flash.json` (`passed: true`, maybe for another drive) is never removed when a later run fails; no cancellation short of killing the process mid-write
- **Reify:** High — suspected bug; destructive phases
- **Suspected bug:** on macOS the raw device opened at `scripts/flash_usb.rs:290-298` is flushed with `device.sync_all()` (`scripts/flash_usb.rs:637`), i.e. F_FULLFSYNC, which Kakehashi documents and tests as failing with ENOTTY on `/dev/rdiskN` (`apps/kakehashi/core/src/platform/macos.rs:177-194`, test `apps/kakehashi/core/src/platform/macos.rs:229`); every macOS flash therefore fails after writing the whole image, before verification and ejection (confidence: medium)
- **Related:** same design as KTL-12 (`apps/kakehashi/core/src/write.rs:171`)

<a id="ktl-25"></a>

### KTL-25 — flash_usb interactive selection and sudo hand-off
- **Where:** `scripts/flash_usb.rs:770` (`main`: `hashing`, `hashed`, `drives`, `plan`)
- **Encoding:** background hash thread with `AtomicU64` progress, joined after the drive is picked; one drive-list snapshot; typed device-name confirmation; re-exec through `sudo` with `--write PLAN`
- **States:** `ImageChosen` → `Hashing ∥ PickingDrive` → `Hashed` → `ChecksumChecked` → `Confirmed` → `Elevated` (KTL-24) | `Quit` (`q`, exit 0) | `Refused`
- **Transitions:** image (`scripts/flash_usb.rs:800-806`); hash thread (`scripts/flash_usb.rs:810-814`); drive snapshot (`scripts/flash_usb.rs:816-818`); pick (`scripts/flash_usb.rs:829-830`, quit `scripts/flash_usb.rs:696`); wait for the hash (`scripts/flash_usb.rs:839-844`); checksum gate (`scripts/flash_usb.rs:846-850`); confirmation (`scripts/flash_usb.rs:855-856`); write directly as root or via sudo (`scripts/flash_usb.rs:860-865`)
- **Hazards:** the snapshot can be arbitrarily old at confirmation; safety rests on KTL-24's `still_there` identity re-check
- **Reify:** Low — linear prompt flow

<a id="ktl-26"></a>

### KTL-26 — Live diagnostics bounded command capture
- **Where:** `scripts/live_diagnostics.rs:11` (`command`: `child`, drain threads, `deadline`)
- **Encoding:** child process, two pipe-drain threads, 15 s deadline
- **States:** `Spawned` → `Running` → `Exited` | `TimedOut` (killed) → `Drained`
- **Transitions:** spawn (`scripts/live_diagnostics.rs:13-22`); drain threads (`scripts/live_diagnostics.rs:32-33`); poll or kill (`scripts/live_diagnostics.rs:35-46`); join (`scripts/live_diagnostics.rs:47-53`)
- **Hazards:** the deadline does not bound the drain: a grandchild holding the inherited pipes keeps `read_to_end`, and so the join, blocked after the kill
- **Reify:** Low — one function

<a id="ktl-27"></a>

### KTL-27 — Release asset staging directory
- **Where:** `scripts/prepare-release.py:19` (`prepare`: `destination`, `assets/`)
- **Encoding:** the output directory's existence as a one-shot marker; files written in order: ISO or parts, `SHA256SUMS`, `ISO-SHA256SUMS`, `manifest.json`, `flake.lock`, `release-notes.md`
- **States:** `Absent` → `Staging` (partial) → `Complete`
- **Transitions:** refuse an existing output (`scripts/prepare-release.py:27-28`); create (`scripts/prepare-release.py:30`); copy or split (`scripts/prepare-release.py:35-49`); checksums (`scripts/prepare-release.py:51-52`); manifest (`scripts/prepare-release.py:54-57`); lock and notes (`scripts/prepare-release.py:58-94`)
- **Hazards:** an interrupted run leaves a partial directory told apart from a complete one only by missing files; a rerun refuses until it is removed by hand
- **Reify:** Low — one linear run
- **Related:** `.github/workflows/release.yml:75`

<a id="ktl-28"></a>

### KTL-28 — Qualification report review and audit gate
- **Where:** `scripts/publish-nightly.py:137` (`validate_report`); `audit_digest` `scripts/publish-nightly.py:62`
- **Encoding:** report JSON: `schema_version` 1 (exhaustive) or 2 (focused profile), per-check `status` and `candidate_sha256`, `audit` `{status, reviewer, mode, reviewed_report_sha256, reviewed_checks, reproduced_checks, completed_at}`; the digest of everything except `audit` is the staleness stamp
- **States:** `Drafted` → `ChecksPassed` → `Audited(digest)` → `Valid` | `Stale` (report changed after the audit) | `Rejected`; the schema version selects focused or exhaustive rules
- **Transitions:** schema and profile (`scripts/publish-nightly.py:139-142`); candidate binding (`scripts/publish-nightly.py:143`); stale or unpassed checks (`scripts/publish-nightly.py:174-175`); focused vs exhaustive (`scripts/publish-nightly.py:180-195`); audit identity (`scripts/publish-nightly.py:219-224`); audit digest (`scripts/publish-nightly.py:226`); reproduced set (`scripts/publish-nightly.py:229-231`); validated again on the staged snapshot (`scripts/publish-nightly.py:326`)
- **Hazards:** the workflow phases live in a hand-written JSON document; only the digest ties the audit to the final report
- **Reify:** Medium — schema-versioned persisted workflow, validated in one function
- **Related:** `.agents/skills/sekka-iso-qualification/references/release/report.md`

<a id="ktl-29"></a>

### KTL-29 — Nightly publication to R2
- **Where:** `scripts/publish-nightly.py:300` (`publish`); `R2` `scripts/publish-nightly.py:237`
- **Encoding:** remote objects: `nightly/builds/ID/publication.json` (reservation with If-None-Match), payload objects carrying `sha256` metadata, `nightly/builds.json` swapped with an ETag condition; local staging snapshot; retention by list position (7)
- **States:** `Validated` → `Reserved(prefix)` → `Uploading` → `Uploaded` → `Indexed` → `OldPruned`; failures stay in `ReservedUnindexed` or `PartiallyPruned`
- **Transitions:** candidate and report validation (`scripts/publish-nightly.py:301-302`); index and ETag read (`scripts/publish-nightly.py:306`); occupancy check (`scripts/publish-nightly.py:314-315`); snapshot and re-validation (`scripts/publish-nightly.py:320-326`); reservation (`scripts/publish-nightly.py:351-352`); ISO re-hash (`scripts/publish-nightly.py:353`); uploads with head verification (`scripts/publish-nightly.py:354-355`, `scripts/publish-nightly.py:267-275`); conditional index write (`scripts/publish-nightly.py:356-361`); pruned objects deleted (`scripts/publish-nightly.py:362-364`)
- **Hazards:** the ETag is read before a long upload, so any concurrent index change fails the swap and leaves a reserved, fully uploaded, unindexed prefix for manual review; the reservation precedes the final ISO re-hash, so a changed ISO also leaves an empty reserved prefix; deletions after the swap are never retried or rediscovered (`indexed_objects` never discovers deletions, `scripts/publish-nightly.py:282`), so a failed prune leaks objects; a client resuming a just-pruned build fails (KTL-05); `ident` and `uploaded` take separate clock readings and can straddle midnight
- **Reify:** High — remote publication phases with representable partial states and no reconciliation step
- **Related:** reader `apps/kakehashi/core/src/nightly.rs:71`; wrapper `nix/publish-nightly.nix:12`

<a id="ktl-30"></a>

### KTL-30 — CI Git object cache refresh and seed
- **Where:** `scripts/runner-git-cache.sh:6` (`$cache`); modes `scripts/runner-git-cache.sh:9` (refresh) and `scripts/runner-git-cache.sh:20` (seed)
- **Encoding:** presence of the bare repository; an empty `$GITHUB_WORKSPACE`; a shared clone using alternates, sparse to the trust script
- **States:** cache `Absent` → `Cloned` → `Fetched` (gc done); workspace `Empty` → `Seeded(sparse)` → `Expanded` (by actions/checkout) | `Unseeded` (no cache or clone failed)
- **Transitions:** clone if absent (`scripts/runner-git-cache.sh:11-13`); fetch (`scripts/runner-git-cache.sh:14-15`); synchronous gc (`scripts/runner-git-cache.sh:18`); seed guards (`scripts/runner-git-cache.sh:22-24`); shared clone or warning (`scripts/runner-git-cache.sh:25-28`); origin and sparse checkout (`scripts/runner-git-cache.sh:29-33`)
- **Hazards:** a clone killed hard leaves a directory that later refreshes never re-clone; jobs read the cache's objects through alternates, which is safe only because refresh runs between jobs
- **Reify:** Low — two small modes; fails open to a normal checkout
- **Related:** `nix/runner.nix:83` (seed at admission), `nix/runner.nix:162` (refresh between jobs under `timeout 180`)

<a id="ktl-31"></a>

### KTL-31 — Runner host setup convergence and KVM access migration
- **Where:** `scripts/runner-host-setup.sh:34` (`/etc/nix/nix.custom.conf`); group migration `scripts/runner-host-setup.sh:47`
- **Encoding:** grep-guarded appends to the Nix configuration, a udev rule file, an ACL on `/dev/kvm`, account existence; removing `nixbld*` from the `kvm` group migrates hosts set up by the script's first version
- **States:** `Unconfigured` → `v1` (build users in the kvm group) → `Current` (ACL, rule, caches, min-free)
- **Transitions:** account (`scripts/runner-host-setup.sh:17-23`); directories (`scripts/runner-host-setup.sh:27-28`); kvm feature (`scripts/runner-host-setup.sh:35-38`); udev rule and ACL (`scripts/runner-host-setup.sh:39-44`); v1 cleanup (`scripts/runner-host-setup.sh:47-51`); min-free (`scripts/runner-host-setup.sh:56-58`); caches (`scripts/runner-host-setup.sh:62-77`); daemon restart (`scripts/runner-host-setup.sh:79`)
- **Hazards:** guards test presence only (any `^min-free` line, any mention of a cache), so changed values never converge
- **Reify:** Low — idempotent one-shot script

## Already explicit

- `apps/kakehashi/app/src/view.rs:49` `enum Loading` — the nightly builds list (`Pending`/`Ready`/`Failed`); transitions only in `load_builds` (`apps/kakehashi/app/src/view.rs:183`) and `apply` for `Msg::Builds` (`apps/kakehashi/app/src/view.rs:220`)

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `apps/kakehashi/app/src/elevate.rs` | 114 | machines | [KTL-03](#ktl-03) | helper launch and exit-code mapping |
| `apps/kakehashi/app/src/main.rs` | 40 | none |  | app bootstrap; quits on last window close (KTL-03 hazard) |
| `apps/kakehashi/app/src/staged_image.rs` | 152 | machines | [KTL-03](#ktl-03) | macOS image staging step of the job |
| `apps/kakehashi/app/src/theme.rs` | 102 | none |  | palette data; follows system appearance |
| `apps/kakehashi/app/src/view.rs` | 1089 | machines | [KTL-01](#ktl-01), [KTL-02](#ktl-02), [KTL-03](#ktl-03), [KTL-04](#ktl-04) | window model: image, download, job, drives |
| `apps/kakehashi/app/src/when.rs` | 89 | none |  | pure time formatting |
| `apps/kakehashi/core/src/drive.rs` | 187 | none |  | pure drive classification and identity |
| `apps/kakehashi/core/src/image.rs` | 108 | none |  | pure hashing with cancel callback |
| `apps/kakehashi/core/src/lib.rs` | 16 | none |  | module list |
| `apps/kakehashi/core/src/nightly.rs` | 215 | machines | [KTL-05](#ktl-05) | resumable on-disk download |
| `apps/kakehashi/core/src/nightly/parallel.rs` | 411 | machines | [KTL-06](#ktl-06), [KTL-07](#ktl-07), [KTL-08](#ktl-08) | adaptive ranges, fallback mode, commit pipeline |
| `apps/kakehashi/core/src/nightly/parallel_tests.rs` | 484 | machines | [KTL-09](#ktl-09) | fault-injecting HTTP fixture |
| `apps/kakehashi/core/src/platform/linux.rs` | 177 | machines | [KTL-12](#ktl-12) | unmount/open/eject steps of the write |
| `apps/kakehashi/core/src/platform/macos.rs` | 251 | machines | [KTL-10](#ktl-10), [KTL-12](#ktl-12) | authopen open and device sync steps |
| `apps/kakehashi/core/src/platform/macos/authopen.rs` | 207 | machines | [KTL-10](#ktl-10) | authopen child and fd handoff |
| `apps/kakehashi/core/src/platform/mod.rs` | 116 | machines | [KTL-11](#ktl-11), [KTL-12](#ktl-12) | Device.held and sync dispatch |
| `apps/kakehashi/core/src/platform/unsupported.rs` | 30 | none |  | stubs |
| `apps/kakehashi/core/src/platform/windows.rs` | 332 | machines | [KTL-11](#ktl-11), [KTL-12](#ktl-12) | volume locks and layout deletion |
| `apps/kakehashi/core/src/write.rs` | 289 | machines | [KTL-12](#ktl-12) | helper write/verify pipeline |
| `apps/kakehashi/helper/src/main.rs` | 146 | machines | [KTL-12](#ktl-12), [KTL-03](#ktl-03) | helper side of progress/cancel protocol |
| `rust/src/builder.rs` | 597 | machines | [KTL-13](#ktl-13), [KTL-14](#ktl-14), [KTL-15](#ktl-15) | builder VM driver, guest markers, NAR cache |
| `rust/src/config.rs` | 141 | none |  | pure config validation |
| `rust/src/gui_input.rs` | 141 | none |  | stateless QMP input; click is a down/up pair |
| `rust/src/lib.rs` | 55 | none |  | command dispatch; arms KTL-17 latch |
| `rust/src/live_test.rs` | 341 | machines | [KTL-16](#ktl-16) | live boot input test phases |
| `rust/src/main.rs` | 7 | none |  | entry point |
| `rust/src/support.rs` | 220 | machines | [KTL-17](#ktl-17) | child supervision and interrupt latch |
| `rust/src/vm.rs` | 1292 | machines | [KTL-18](#ktl-18), [KTL-19](#ktl-19), [KTL-20](#ktl-20) | RPC, VM handle, run orchestration |
| `rust/src/wifi_test.rs` | 383 | machines | [KTL-21](#ktl-21), [KTL-15](#ktl-15) | Wi-Fi test phases; consumes NAR cache |
| `scripts/boot_installed.rs` | 11 | none |  | rust-script entry point (KTL-20) |
| `scripts/build_rootless.rs` | 11 | none |  | rust-script entry point (KTL-13) |
| `scripts/check-pr-trust.py` | 100 | machines | [KTL-22](#ktl-22) | mergeability polling gate |
| `scripts/check_application_matrix.py` | 49 | none |  | sequential evaluation; one result file |
| `scripts/check_cache_coverage.py` | 342 | none |  | single-run graph worklist with memo caches |
| `scripts/ci-retain.py` | 192 | machines | [KTL-23](#ktl-23) | GC-root generation retention |
| `scripts/flash_usb.rs` | 996 | machines | [KTL-24](#ktl-24), [KTL-25](#ktl-25) | privileged write and interactive flow |
| `scripts/live_diagnostics.rs` | 114 | machines | [KTL-26](#ktl-26) | bounded command capture |
| `scripts/prepare-release.py` | 100 | machines | [KTL-27](#ktl-27) | release asset staging |
| `scripts/prepare_builder.rs` | 11 | none |  | rust-script entry point (KTL-14) |
| `scripts/publish-nightly.py` | 394 | machines | [KTL-28](#ktl-28), [KTL-29](#ktl-29) | qualification gate and R2 publication |
| `scripts/qemu_storage_matrix.rs` | 51 | none |  | sequential qemu-test matrix; stops at first failure |
| `scripts/qemu_test.rs` | 11 | none |  | rust-script entry point (KTL-20) |
| `scripts/runner-admission.sh` | 13 | none |  | pure admission predicate |
| `scripts/runner-git-cache.sh` | 36 | machines | [KTL-30](#ktl-30) | git object cache refresh/seed |
| `scripts/runner-host-setup.sh` | 80 | machines | [KTL-31](#ktl-31) | host setup convergence and kvm migration |
| `scripts/test_live.rs` | 9 | none |  | rust-script entry point (KTL-16) |
| `scripts/test_wifi.rs` | 9 | none |  | rust-script entry point (KTL-21) |
