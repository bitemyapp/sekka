# KTL: Kakehashi, respin tools and scripts

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `apps/kakehashi, rust, scripts`. Audited at `8e0acbe5`; line numbers refer to that commit.

47 files audited. 31 implicit machines: 8 High, 10 Medium, 13 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

<a id="ktl-02"></a>

### KTL-02 — Kakehashi nightly download session
- **Where:** `apps/kakehashi/app/src/view.rs:85` (`Download.build`, `.fetch`, `.from`, `.cancel`); `Kakehashi.download` `apps/kakehashi/app/src/view.rs:116`
- **Encoding:** partially explicit: `Option<Download>`; `fetch: Option<Fetch>` (None = starting; `Fetch::Checking`/`Downloading`, `apps/kakehashi/core/src/nightly.rs:82`); `from: Option<(u64, Instant)>` rate origin; `cancel` flag shared with the worker thread; worker messages keyed by `build.id`
- **States:** `Idle` → `Starting` → `Checking` ↔ `Downloading` → `Idle` + image chosen | `Idle` + `error`; `Stopping` exists only as `cancel == true` while `download` is still Some
- **Transitions:** `start_download` (`apps/kakehashi/app/src/view.rs:404-411`, worker `apps/kakehashi/app/src/view.rs:413`); `Msg::Fetch` sets `from` once and `fetch` (`apps/kakehashi/app/src/view.rs:271-276`); Stop sets `cancel` (`apps/kakehashi/app/src/view.rs:789`); `Msg::Fetched` takes the session by id and chooses the image or sets `error` (`apps/kakehashi/app/src/view.rs:280-289`); a window drop clears it (`apps/kakehashi/app/src/view.rs:638`)
- **Hazards:** no generation, messages are matched by build id only (`apps/kakehashi/app/src/view.rs:270`, `apps/kakehashi/app/src/view.rs:280`); no Stopping state is shown: after Stop the session stays until workers drain (up to the 30 s range timeout, `apps/kakehashi/core/src/nightly/parallel.rs:299`); Download buttons are disabled only while `download.is_some()` (`apps/kakehashi/app/src/view.rs:711`, `apps/kakehashi/app/src/view.rs:750`)
- **Reify:** High — suspected bug; an orphaned worker can share a `.part` with a new session
- **Suspected bug:** dropping a file onto the window during a download sets `this.download = None` (`apps/kakehashi/app/src/view.rs:638`) without storing `cancel`, so the worker keeps downloading into Downloads in the background; the Download buttons re-enable, and restarting the same build runs a second worker on the same `.part` (no lock, `apps/kakehashi/core/src/nightly.rs:138`), the orphan's `Msg::Fetched` with the same id ends the new session (`apps/kakehashi/app/src/view.rs:280`), and an orphan in fallback mode truncates the shared file (`apps/kakehashi/core/src/nightly/parallel.rs:210`) (confidence: high for the orphaned download; medium for the double-writer outcome)
- **Reproduction:** Confirmed (unit): [summary](reproductions.md#ktl-02)

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
- **Reproduction:** Confirmed (unit): [summary](reproductions.md#ktl-03)
- **Follow-up:** reified as typestate in [#290](https://github.com/bitemyapp/sekka/pull/290). Its suspected bug is fixed, and the mistake no longer compiles.

<a id="ktl-04"></a>

### KTL-04 — Kakehashi drive list polling and chosen drive
- **Where:** `apps/kakehashi/app/src/view.rs:117` (`Kakehashi.drives`, `.listing`, `.listed`, `.show_all`, `.chosen`)
- **Encoding:** partially explicit: `Loading<Vec<Drive>>` + `listing: bool` in-flight guard + `listed: Instant` 2 s refresh timer + `chosen: Option<String>` drive name; the `_poll` task ticks every 100 ms (`apps/kakehashi/app/src/view.rs:153-160`)
- **States:** `Pending` → `Listing` ↔ `Ready(drives)` | `Failed`; paused while a job runs; `chosen` None ↔ Some(name). Representable invalid: `chosen` naming a device that has been replaced by another with the same name
- **Transitions:** tick refresh when no job is running (`apps/kakehashi/app/src/view.rs:208-211`); `list_drives` sets `listing` (`apps/kakehashi/app/src/view.rs:192-195`); `Msg::Drives` clears `listing`, replaces the list and clears `chosen` if its name vanished (`apps/kakehashi/app/src/view.rs:227-237`); click sets `chosen` (`apps/kakehashi/app/src/view.rs:906`); job end forces a refresh (`apps/kakehashi/app/src/view.rs:327`); "Write another drive" clears `chosen` and forces a refresh (`apps/kakehashi/app/src/view.rs:1081-1082`)
- **Hazards:** identity is the name only; a hung `lsblk`/`diskutil` keeps `listing` true forever, no timeout (`apps/kakehashi/app/src/view.rs:192`); if the chosen drive vanishes while the confirm dialog is open, OK silently does nothing (`apps/kakehashi/app/src/view.rs:511`)
- **Reify:** High — suspected bug; the selection feeds an erase and is keyed by a reusable name
- **Suspected bug:** `confirm` describes the drive captured when the dialog opens (`apps/kakehashi/app/src/view.rs:479-490`), but OK calls `start_write` (`apps/kakehashi/app/src/view.rs:502`), which re-reads `chosen_drive()` (`apps/kakehashi/app/src/view.rs:511`) from a list that keeps refreshing under the modal; if the stick is swapped for another that gets the same name (`sdb`, `disk4`) between two refreshes, the plan and the helper's `still_there` both use the new drive, and a drive other than the one named in "Erase …?" is erased (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#ktl-04)
- **Follow-up:** reified as typestate in [#290](https://github.com/bitemyapp/sekka/pull/290). Its suspected bug is fixed, and the mistake no longer compiles.

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
- **Reproduction:** Not reproduced (environment) (unit): [summary](reproductions.md#ktl-12)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#ktl-13)

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
- **Reproduction:** Partly confirmed (integration): [summary](reproductions.md#ktl-20)

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
- **Reproduction:** Not reproduced (environment) (unit): [summary](reproductions.md#ktl-24)

<a id="ktl-29"></a>

### KTL-29 — Nightly publication to R2
- **Where:** `scripts/publish-nightly.py:300` (`publish`); `R2` `scripts/publish-nightly.py:237`
- **Encoding:** remote objects: `nightly/builds/ID/publication.json` (reservation with If-None-Match), payload objects carrying `sha256` metadata, `nightly/builds.json` swapped with an ETag condition; local staging snapshot; retention by list position (7)
- **States:** `Validated` → `Reserved(prefix)` → `Uploading` → `Uploaded` → `Indexed` → `OldPruned`; failures stay in `ReservedUnindexed` or `PartiallyPruned`
- **Transitions:** candidate and report validation (`scripts/publish-nightly.py:301-302`); index and ETag read (`scripts/publish-nightly.py:306`); occupancy check (`scripts/publish-nightly.py:314-315`); snapshot and re-validation (`scripts/publish-nightly.py:320-326`); reservation (`scripts/publish-nightly.py:351-352`); ISO re-hash (`scripts/publish-nightly.py:353`); uploads with head verification (`scripts/publish-nightly.py:354-355`, `scripts/publish-nightly.py:267-275`); conditional index write (`scripts/publish-nightly.py:356-361`); pruned objects deleted (`scripts/publish-nightly.py:362-364`)
- **Hazards:** the ETag is read before a long upload, so any concurrent index change fails the swap and leaves a reserved, fully uploaded, unindexed prefix for manual review; the reservation precedes the final ISO re-hash, so a changed ISO also leaves an empty reserved prefix; deletions after the swap are never retried or rediscovered (`indexed_objects` never discovers deletions, `scripts/publish-nightly.py:282`), so a failed prune leaks objects; a client resuming a just-pruned build fails (KTL-05); `ident` and `uploaded` take separate clock readings and can straddle midnight
- **Reify:** High — remote publication phases with representable partial states and no reconciliation step
- **Related:** reader `apps/kakehashi/core/src/nightly.rs:71`; wrapper `nix/publish-nightly.nix:12`
