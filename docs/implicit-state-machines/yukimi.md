# YUK: Yukimi system manager (Aizome app and core crates)

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/apps/yukimi, apps/yukimi`. Audited at `8e0acbe5`; line numbers refer to that commit.

62 files, 0 pending. 30 implicit machines: 11 High, 9 Medium, 10 Low.

## Machines

<a id="yuk-01"></a>

### YUK-01 — Sekka update channel and legacy migration in flake.nix
- **Where:** `apps/yukimi/crates/yukimi-config/src/sekka.rs:28` (`NIGHTLY`), `:35` (`EARLY`), `:46` (`fn prepare`); also `apps/yukimi/yukimi-helper/src/main.rs:918`, `apps/yukimi/crates/yukimi-system/src/updates.rs:259`, `desktop/aizome/apps/yukimi/src/model.rs:251`, `desktop/aizome/apps/yukimi/src/checks.rs:204`
- **Encoding:** the literal `inputs.sekka.url` string in `flake.nix` (absent, `github:bitemyapp/sekka` or `/main`, `/nightly`, `?ref=main`, another branch or pin); presence of `inputs.calamares|tatami|yukimi`; per external input either `inputs.<name>.url` at Sekka's default address or `inputs.<name>.follows = "sekka/<name>"`; `flake.lock` `original.ref`
- **States:** `Legacy` (calamares/tatami/yukimi inputs) → `SekkaDefaultMain` → `Nightly` | `Early` (`?ref=main`) | `OtherBranchOrPin` | `Unsupported`; per input `OwnAddressAtSekkaDefault` → `FollowsSekka` (one-way) | `OwnAddressCustom`; `flake.lock` cannot tell `Early` from a legacy `main` (both lock `ref = main`)
- **Transitions:** `migrate` Legacy→Nightly (`sekka.rs:226`, header rewrite `:239-243`); `prepare` default main→Nightly (`sekka.rs:51-52`); `follow_sekka` input→FollowsSekka, dropping `inputs.sekka.inputs.<n>.follows` (`sekka.rs:98`, `:107`); helper `--follow sekka=main` →Early (`apps/yukimi/yukimi-helper/src/main.rs:918-919`), other branches via `branch::with` (`apps/yukimi/yukimi-helper/src/main.rs:921`), written by `write_file` (`apps/yukimi/yukimi-helper/src/main.rs:864`, `:924`); readers: `sekka_targets` keeps main's answer for Early, else targets nightly (`apps/yukimi/crates/yukimi-system/src/updates.rs:261-275`), `Model::early` (`model.rs:251-253`), `following` keeps FollowsSekka inputs out of `nix flake update` (`apps/yukimi/yukimi-helper/src/main.rs:941-945`), `update_operation` treats a flake without `sekka` as migrating and drops the legacy inputs (`checks.rs:205-213`); GUI `Operation::Early` (`desktop/aizome/apps/yukimi/src/ops.rs:226-229`), CLI `--early`/`--nightly` (`apps/yukimi/yukimi-helper/src/bin/sekka-update.rs:38-43`)
- **Hazards:** the mode is only the exact URL spelling, so a hand-written `github:bitemyapp/sekka/main` meant as early updates is silently moved to nightly by the next `prepare`; two sources of truth (flake.nix for `early()`, the lock's `original` for the branch chip in `desktop/aizome/apps/yukimi/src/ui/updates.rs:129`, `:144-145`) disagree while flake.nix is edited but unbuilt; FollowsSekka has no way back in Yukimi; `supported()` reruns the whole migration parse on every check (`checks.rs:96-100`)
- **Reify:** High — four mutation sites over one persisted string, user-visible update channel, read by helper, CLI and GUI
- **Related:** privileged helper `apps/yukimi/yukimi-helper/src/main.rs:804` (`fn change`) applies it; the nightly branch itself is moved by CI outside this slice

<a id="yuk-02"></a>

### YUK-02 — Update receipt and system completion advice
- **Where:** `apps/yukimi/crates/yukimi-system/src/completion.rs:59` (`Receipt.state`, `.system`, `.boot_id`), `:9` (`RECEIPT`); writers `apps/yukimi/yukimi-helper/src/main.rs:978`, `:1034`, `:998` (`fn save_update`)
- **Encoding:** `/var/lib/yukimi/update.json` `{system, boot_id, state: String, reasons}` together with `/run/booted-system`, `/run/current-system`, `/nix/var/nix/profiles/system` and `/proc/sys/kernel/random/boot_id`
- **States:** receipt `Absent` | `"activating"` | `"prepared"` | `"activated"` (any other string behaves like activated); derived `Applied` (booted = active = selected) | `PendingReboot` (selected ≠ active, prepared for selected) | `ActivationFailed` (activating, not superseded) | `Unverified` | `LiveButBootArtefactsChanged`; representable but invalid: `prepared` for a system no longer selected, `activating` for a system never selected
- **Transitions:** `change` writes activating before activation (`apps/yukimi/yukimi-helper/src/main.rs:978-980`), then prepared/activated (`apps/yukimi/yukimi-helper/src/main.rs:988-989`); `rollback` the same (`apps/yukimi/yukimi-helper/src/main.rs:1034-1040`, `:1056-1057`); atomic write with fsync (`apps/yukimi/yukimi-helper/src/main.rs:998-1009`); `system_state` supersedes activating only when `r.system` is booted, active and selected in a later boot (`apps/yukimi/crates/yukimi-system/src/completion.rs:484-495`), prepared→Reboot only for the selected system (`:499-509`), otherwise boot-artefact comparison (`:515-526`); unreadable receipt → Unknown (`:621-626`); system advice outranks session advice (`with_session`, `:639-648`); `finish_update` recomputes after a live switch (`:692-706`); `clean` writes no receipt
- **Hazards:** stringly typed state; only Yukimi's own change/rollback rewrite the receipt, so out-of-band rebuilds never clear it; reasons recorded at plan time are shown verbatim later; the whole read runs on every GUI reload (`desktop/aizome/apps/yukimi/src/model.rs:163`)
- **Reify:** High — persisted cross-boot phase with string states, three writers, user-visible advice, suspected bug
- **Suspected bug:** after a Yukimi activation fails past the commit point (`switch-to-configuration` or `nix-env --set` failing at `apps/yukimi/yukimi-helper/src/main.rs:984`), the receipt stays `activating` for system S1; if the user repairs things with `nixos-rebuild switch` to a different system S2 and even reboots into it, `system_state` still returns Unknown "Activation did not finish successfully" on every read (`apps/yukimi/crates/yukimi-system/src/completion.rs:484-495`) until another Yukimi operation rewrites the receipt. (confidence: medium)
- **Related:** privileged helper `apps/yukimi/yukimi-helper/src/main.rs` (sole writer); GUI `desktop/aizome/apps/yukimi/src/model.rs:163`; CLI `apps/yukimi/yukimi-helper/src/bin/sekka-update.rs:17`
- **Reproduction:** Confirmed (unit): [test and output](reproductions.md#yuk-02)

<a id="yuk-03"></a>

### YUK-03 — Tatami session update record and Keep Awake check
- **Where:** `apps/yukimi/crates/yukimi-system/src/completion.rs:63` (`SessionManifest.version`, `.boot_id`, `.session_id`, `.session_invocation`, `.error`), `:11` (`SESSION_RECEIPT`), `:10` (`SESSION_MANIFEST`)
- **Encoding:** `$XDG_RUNTIME_DIR/tatami/update-session.json` (what the session applied) against `<active>/etc/xdg/tatami/update.json` (what is installed); `wayland-wm@tatami.service` ActiveState/InvocationID/EnvironmentFiles; uwsm `env_session.conf`; `tatami-keep-awake.service` ActiveState and MainPID cmdline
- **States:** `NoRecord` → `Recorded(boot, session, invocation)` → `Current` | `PendingLogout` (session/reload/keep_awake differ) | `Errored` (`error` set) | `Stale` (other boot, session or invocation); Keep Awake `Inactive` | `ActiveCurrent` | `ActivePrevious` | `Changing`
- **Transitions:** Tatami records it at login (outside slice); `apply_desktop` runs `tatami update-session` after a verified live switch (`apps/yukimi/crates/yukimi-system/src/completion.rs:678-690`, gated in `finish_update` `:696-704`); `tatami_session_id` validates boot, session, invocation and uwsm context (`:417-469`); `awake_state` reads the running inhibitor (`:569-597`), with an in-memory override when it is current (`:664-666`); `session_state` compares (`:529-547`)
- **Hazards:** `update-session`'s exit status is ignored (`apps/yukimi/crates/yukimi-system/src/completion.rs:684-687`); check-then-act between `read_system`/current-system and the apply (`:696-703`); `version == 1` is hard-coded, so a record format bump turns every session Unknown; several `systemctl` calls per read
- **Reify:** Medium — cross-process persisted record whose invalid states are guarded
- **Related:** Tatami `desktop/tatami/tool/src/update.rs:53` (`remember`) and `:72` (`apply`)

<a id="yuk-04"></a>

### YUK-04 — Aizome live-session completion probe
- **Where:** `apps/yukimi/crates/yukimi-system/src/completion/aizome.rs:27` (`Running` snapshot), `:236` (`fn assess`, `result.action`)
- **Encoding:** one IPC conversation (Version → Stats → Reload{check}) over a non-blocking connect with 2 s timeouts; `/proc/<pid>/{exe,environ,cwd,maps}`; the launcher's `AIZOME_ENVIRONMENT_KEYS="1 …"` and `AIZOME_ENVIRONMENT_FILE` record; reload report action strings; a mutable `Completion.action` escalated by about eleven assignments
- **States:** probe `Connecting` → `Versioned` → `StatsChecked` → `ReloadChecked` → `Done` | `Failed` (Unknown); advice lattice `None` < `Reload` < `Logout` < `Unknown`, with special cases (an environment error demotes None/Reload to Unknown but keeps Logout)
- **Transitions:** `snapshot` sequence (`aizome.rs:112-147`), old-session refusal (`:116-118`); `assess` sets Logout (`:253`, `:264`, `:275`, `:281`, `:295`), Unknown (`:268`, `:287`, `:315`, `:319`, `:343`), returns early (`:324-326`), Reload (`:337-339`); `assess_session` demotion (`:92-94`); `inspect` rechecks `/run/current-system` last (`:72-74`)
- **Hazards:** precedence is implicit in assignment order (Unknown overwrites Logout, Logout never overwrites Unknown, Reload only from None); reload action strings (`"same"`, `"unchanged"`, `"would-reload"`) and the `"1 "` record prefix are unnegotiated protocol versions (fail closed to Unknown); synchronous IPC inside `Model::quick` can add seconds to each reload
- **Reify:** Medium — an ordered advice enum combined with `max()` would replace the scattered precedence rules
- **Related:** compositor IPC `desktop/aizome/crates/aizome-ipc` (`Request::Version`, `Stats`, `Reload`); launcher `desktop/aizome/session/aizome-session:59`

<a id="yuk-05"></a>

### YUK-05 — (test harness) fake Aizome IPC server and stubbed session launcher
- **Where:** `apps/yukimi/crates/yukimi-system/src/completion/aizome.rs:1044` (scripted three-request server), `:1090` (silent server), `:1108` (full accept queue), `:1134` (invalid responses), `:935` (launcher run with a stub `systemctl`)
- **Encoding:** server threads expecting an ordered request list; `TEST_MANAGER` running/offline switching the stub manager's answers; mpsc channels releasing held servers
- **States:** `Listening` → `Accepted` → request i answered … → `Done`; silent server `Accepted` → `HeldUntilReleased`
- **Transitions:** ordered request loop (`aizome.rs:1072-1081`); release through channels (`:1096-1105`, `:1118-1131`)
- **Hazards:** a regression that blocks holds a server thread up to 5 s (bounded by `recv_timeout`); the launcher test reads the host's `/etc/xdg/aizome/environment` (`aizome.rs:1015`)
- **Reify:** Low — test fixture

<a id="yuk-06"></a>

### YUK-06 — Nix activity tracker and the helper's stage protocol
- **Where:** `apps/yukimi/crates/yukimi-system/src/log.rs:91` (`Progress.activities`, `.order`, `.expected_bytes`, `.bytes`, `.logged`, `.log`); also `apps/yukimi/yukimi-helper/src/main.rs:184` (`fn stage`), `:191` (`fn stage_done`)
- **Encoding:** per-activity map, start order, byte counters, a 400-line ring with an ever-growing `logged` cursor; the helper's type-0 activities, ids 1–5 reused, name its stages
- **States:** per activity `Started` → `Progressing` (done/expected, phase) → `Stopped`; reader cursor `seen ≤ logged`
- **Transitions:** Start inserts and pushes order (`log.rs:160-165`); Stop removes and settles bytes (`:166-174`); PROGRESS (`:176-199`); SET_EXPECTED (`:200-205`); SET_PHASE (`:206-210`); `push_log` (`:229-235`); `since` (`:239-242`); helper stages `stage(1)` (`apps/yukimi/yukimi-helper/src/main.rs:863`, `:868`), `stage(5)` (`:909`), `stage(2)` (`:928`), `stage(3)` (`:958`), `stage(4)` (`:981`)
- **Hazards:** helper failure paths leave through `?` without `stage_done`, so the failed stage stays "current"; `expected_bytes` is never pruned; a reader more than 400 lines behind loses lines silently; stage ids are reused within one run
- **Reify:** Low — a local reducer; the stage protocol could carry explicit stage ids and outcomes
- **Related:** GUI `desktop/aizome/apps/yukimi/src/job.rs:127` (snapshot cursor); CLI `apps/yukimi/yukimi-helper/src/bin/sekka-update.rs:88-100`

<a id="yuk-07"></a>

### YUK-07 — Stop handle and streamed child lifecycle
- **Where:** `apps/yukimi/crates/yukimi-system/src/nix.rs:68` (`StopState.requested`, `.signal`, `.stdin`, `.pid`)
- **Encoding:** one `Arc<Mutex<StopState>>` shared by every step of a job; captured `stderr: Option<String>`
- **States:** `Idle` → `Running{stdin, pid}` → `Stopping` (requested, stdin dropped, maybe SIGTERM) → `Reaped`; `requested` is sticky; stderr capture `Complete` → `Discarded` (over 8 KiB, non-UTF-8 or read error)
- **Transitions:** `request` (`nix.rs:81-89`); `signal` per step (`:93-95`, set at `desktop/aizome/apps/yukimi/src/job.rs:151`); refuse to start once requested (`nix.rs:119-121`); spawn stores stdin and pid (`:128-129`); clear after wait (`:170-174`); stderr discard (`:151`, `:155-162`)
- **Hazards:** between `child.wait()` returning (`nix.rs:169`) and `pid` being cleared (`:172`), a concurrent `request()` can SIGTERM a recycled pid (user-run Nix only); closing stdin is the only stop for the root helper, so it relies on pkexec passing the pipe through; discarded stderr disables the auth-retry classification by design
- **Reify:** Medium — cancellation, signalling and process lifetime share one mutex-guarded record across threads
- **Related:** privileged helper client `apps/yukimi/yukimi-helper/src/transaction.rs:83` (stdin EOF → cancel)

<a id="yuk-08"></a>

### YUK-08 — Helper cancellation phase (Working / Stopping / Committed)
- **Where:** `apps/yukimi/yukimi-helper/src/main.rs:83` (`STATE: (Phase, Option<u32>)`), `:74` (`enum Phase`), `:84` (`INTERRUPTED`)
- **Encoding:** process-global mutex of phase and current child pid (partially explicit); an async-signal flag polled every 50 ms; a stdin-EOF thread; the low-space watcher's `done`/`low` flags
- **States:** `Working` → `Stopping` | `Committed`; Committed absorbs stops; pid is Some only while `wait()` runs a child
- **Transitions:** `stop()` Working→Stopping and SIGTERM to the child (`apps/yukimi/yukimi-helper/src/main.rs:90-100`), called from the signal poll (`:116-123`), stdin EOF (`:132-136`) and low space (`:454-457`); `commit()` Working→Committed or Err when Stopping (`:145-152`), called at `:969`, `:1030`, `:1106`; `wait()` refuses when Stopping, records the pid, checks again after (`:155-169`); the client process reuses the same static to remember a cancel (`apps/yukimi/yukimi-helper/src/transaction.rs:91`)
- **Hazards:** a stop is only observed at `wait()` boundaries, so in-process steps (file writes, `copy_configuration`) carry on after it; the pid is cleared after `wait_with_output` (`apps/yukimi/yukimi-helper/src/main.rs:165-166`), leaving a recycled-pid window; one static means "phase" in the worker and "cancel requested" in the client; the low-space stop is reported as "Nothing was changed" although `Backup::restore` ignores write errors
- **Reify:** High — privileged process, three threads and a signal handler mutate it, and it decides whether the configuration is restored
- **Related:** GUI stop `desktop/aizome/apps/yukimi/src/job.rs:349`; client forwarding `apps/yukimi/yukimi-helper/src/transaction.rs:91-95`

<a id="yuk-09"></a>

### YUK-09 — Privileged change transaction (back up, edit, update, build, commit, activate)
- **Where:** `apps/yukimi/yukimi-helper/src/main.rs:804` (`fn change`), `:234` (`Backup`), `:860` (`backup`), `:861` (`attempt` closure); also `apps/yukimi/nix/module.nix:147`
- **Encoding:** an in-memory `Backup` of flake.lock, flake.nix, the main file and yukimi.nix; the order of steps in the `attempt` closure; `Phase` (YUK-08); the receipt (YUK-02); the `activation_mode` result; NixOS preSwitchChecks hook
- **States:** `Validated` → `RunningSnapshotSaved` → `BackedUp` → `SekkaWritten`? → `ConfigWritten`? → `BranchFollowed`? → `InputsUpdated` | `ChannelsUpdated`? → `Built` → `Planned(Live | Boot | Refused)` → `Committed` → `ReceiptActivating` → `ProfileSet` → `NewSnapshotSaved` → `Switched` → `Verified` → `ReceiptActivated | ReceiptPrepared` → `LaunchersRefreshed`; failures `FailedBeforeCommit` (restored) | `FailedAfterCommit` (not restored)
- **Transitions:** validation (`apps/yukimi/yukimi-helper/src/main.rs:805-845`); save running configuration (`:849-854`); back up (`:855-860`); Sekka flake (`:862-866`); main file and yukimi.nix (`:867-907`); follow (`:908-926`); prepared-lock check or `nix flake update` (`:927-950`); `nix-channel --update` (`:951-957`); build under `with_room` (`:958-960`, `:435-473`); plan, mode, commit (`:963-971`); restore on error (`:972-976`); receipt and activation (`:978-989`, `activate` `:559-568`); verification (`:985-987`); launchers (`:990-992`); post-commit error text (`:1142-1148`); target-side gate `check-activation` refuses live switches for any frontend (`module.nix:147-149`, `apps/yukimi/yukimi-helper/src/main.rs:1175-1187`)
- **Hazards:** restore is best effort (non-atomic `fs::write`, errors ignored, `apps/yukimi/yukimi-helper/src/main.rs:241-252`) with no on-disk journal, so SIGKILL, OOM or power loss mid-attempt leaves edited files and no marker; `Backup::take` records unreadable files as absent and restore deletes them (`:238`, `:248`); new files get `git add --intent-to-add` (`:269-282`), which restore never undoes; `save_configuration` failures are ignored unless `--sekka` (`:850-853`); post-commit failures keep the edits and an `activating` receipt
- **Reify:** High — multi-step privileged transaction with an implicit step order, partial rollback and a commit point
- **Suspected bug:** `--channels` runs `nix-channel --update` (`apps/yukimi/yukimi-helper/src/main.rs:951-956`), but root's channels profile isn't in the backup, so after a failed or stopped build only files are put back: the channels stay advanced, the Updates page calls them current, and the next unrelated change (installing a package) silently rebuilds the whole system on the new channel. (confidence: medium)
- **Related:** GUI `desktop/aizome/apps/yukimi/src/ops.rs:186` (`helper_args`) and `:289` (prepared lock); CLI `apps/yukimi/yukimi-helper/src/bin/sekka-update.rs:74`
- **Reproduction:** Confirmed (end-to-end): [test and output](reproductions.md#yuk-09)

<a id="yuk-10"></a>

### YUK-10 — Saved per-generation configuration snapshots
- **Where:** `apps/yukimi/yukimi-helper/src/main.rs:62` (`SAVED`), `:327` (`fn save_configuration`), `:344` (`fn restore_choices`), `:371` (`fn prune_saved`); also `apps/yukimi/crates/yukimi-system/src/lib.rs:38`
- **Encoding:** `/var/lib/yukimi/configurations/generation-<n>` directories (plus `.new` partials), keyed by profile generation number
- **States:** per n `Unsaved` → `Saving(.new)` → `Saved` (kept unless `replace`) → `Pruned` (only when Yukimi's clean runs)
- **Transitions:** running generation saved without replace (`apps/yukimi/yukimi-helper/src/main.rs:849-854`, `:1026-1028`); new generation saved with replace after `nix-env --set` (`:563-566`); partial→final (`:332-336`); three-way restore on rollback (`:1043-1051`, `:344-368`); prune after deleting generations (`:1089`); readers `desktop/aizome/apps/yukimi/src/app.rs:891`, `desktop/aizome/apps/yukimi/src/update_history.rs:17-26`, `desktop/aizome/apps/yukimi/src/comparison.rs:95`, `:110`
- **Hazards:** Nix reuses generation numbers once the newest generations are deleted, and only Yukimi's clean prunes; `remove_dir_all` then `rename` (`apps/yukimi/yukimi-helper/src/main.rs:335-336`) leaves no snapshot if interrupted between them; hidden files and files over 1 MiB are never kept (`:311`, `:318`)
- **Reify:** High — persisted state keyed by a reusable number, read by helper and GUI, decides which files a rollback rewrites
- **Suspected bug:** return to generation N, run `nix-collect-garbage -d` (deletes N+1…, leaves `generation-<N+1>`), then `nixos-rebuild switch` makes a new N+1; the next Yukimi change calls `save_configuration(N+1, false)` and keeps the stale copy (`apps/yukimi/yukimi-helper/src/main.rs:329-331`), so History and Updates show the old build's revisions and a later return to N+1 restores another build's flake.lock and configuration. (confidence: low)
- **Related:** GUI `desktop/aizome/apps/yukimi/src/update_history.rs:17`
- **Reproduction:** Confirmed (end-to-end): [test and output](reproductions.md#yuk-10)

<a id="yuk-11"></a>

### YUK-11 — Rollback and clean-up sequences
- **Where:** `apps/yukimi/yukimi-helper/src/main.rs:1011` (`fn rollback`), `:1068` (`fn clean`)
- **Encoding:** statement order around `commit()`, receipt writes and `switch-to-configuration`
- **States:** rollback `Validated` → `Planned(auto)` → `RunningSaved` → `Committed` → `ProfileSwitched` → `ReceiptActivating` → `ChoicesRestored` → `Switched` → `Verified` → `ReceiptActivated | ReceiptPrepared`; clean `GenerationsDeleted`? → `Pruned` → `Collecting` (stoppable) → `Committed` → `BootMenuUpdated`
- **Transitions:** rollback commits before changing anything (`apps/yukimi/yukimi-helper/src/main.rs:1030`), `--switch-generation` (`:1031-1033`), receipt (`:1034-1040`), restore (`:1043-1051`), switch (`:1052`), verify (`:1053-1055`), receipt (`:1056-1057`); clean deletes and prunes (`:1084-1091`), collects through `wait` (`:1092-1103`), commits (`:1106`), runs `switch-to-configuration boot` (`:1107`)
- **Hazards:** a rollback can't be stopped at all; a crash between the profile switch and the receipt leaves the previous receipt; restored choices are written before the switch succeeds; clean's collection stays cancellable after generations are already gone
- **Reify:** High — irreversible steps before an uncancellable commit point, suspected bug
- **Suspected bug:** Clean Up with "older than a week", then Stop during the long `nix-store --gc`: `wait()` returns "Stopped" (`apps/yukimi/yukimi-helper/src/main.rs:1095`) before `commit()` and `switch-to-configuration boot` (`:1106-1107`), so the generations are deleted but the boot menu still lists them; the next collection removes their closures and those entries no longer boot. The same happens if GC fails. (confidence: medium)
- **Reproduction:** Confirmed (end-to-end): [test and output](reproductions.md#yuk-11)

<a id="yuk-12"></a>

### YUK-12 — Privileged operation client and worker (pkexec client, transient unit, result file)
- **Where:** `apps/yukimi/yukimi-helper/src/transaction.rs:45` (`fn client`), `:84` (`cancelled`), `:8` (`OPERATIONS`); worker `apps/yukimi/yukimi-helper/src/main.rs:1131`, lock `:1193`; retention `apps/yukimi/nix/module.nix:144`
- **Encoding:** `/var/lib/yukimi/operations/<nanos>-<pid>/` holding `progress.log` and `result.new` → `result.json`; unit `yukimi-operation-<id>.service` (StandardInput=null, KillMode=control-group, TimeoutStopSec=infinity, `--collect`); `/run/lock/yukimi.lock` flock; the client loop's `cancelled` flag; the client's `Phase` reused as "cancel requested"
- **States:** `DirCreated` → `UnitStarted` → `Running` → `CancelForwarded` (once) → `ResultWritten` → `UnitGone`; `Lost` (unit not active, no result); worker `LockBusy` → error result
- **Transitions:** directory and log (`transaction.rs:46-57`); `systemd-run` (`:58-82`); stdin and signal watch (`:83`); forward the log (`:86`); read the result (`:87-90`, race `:100-104`); SIGINT forwarded once (`:91-95`); ActiveState poll (`:96-109`), 250 ms sleep (`:110`); worker directory check (`:10-21`), lock (`apps/yukimi/yukimi-helper/src/main.rs:1193-1202`), worker stop watch (`apps/yukimi/yukimi-helper/src/main.rs:1203`), atomic result (`transaction.rs:23-31`, `apps/yukimi/yukimi-helper/src/main.rs:1153`)
- **Hazards:** no client timeout (by design); the single SIGINT can land before the worker installs its handler (it takes the lock first, `apps/yukimi/yukimi-helper/src/main.rs:1193-1203`), killing it without a result, so the GUI reports "ended without a verified result"; a dead client leaves `result.json` unread (the GUI recovers only through YUK-02); tmpfiles deletes operation logs after 30 days; the client runs `systemctl show` four times a second for the whole build
- **Reify:** High — privilege boundary and cross-process protocol encoded in files, unit state and a flag
- **Related:** GUI `desktop/aizome/apps/yukimi/src/ops.rs:379` (pkexec) and `desktop/aizome/apps/yukimi/src/job.rs:136`; CLI `apps/yukimi/yukimi-helper/src/bin/sekka-update.rs:77-78` (keeps stdin open)

<a id="yuk-13"></a>

### YUK-13 — X11 fallback nested Weston lifecycle
- **Where:** `desktop/aizome/apps/yukimi/data/yukimi-launch:12` (`weston_pid`), `:11` (`run`)
- **Encoding:** shell variables `run`, `weston_pid`, `status`; EXIT/INT/TERM traps
- **States:** `Wayland` (exec) | `Help` (exec) | X11 `TmpDirMade` → `WestonRunning` → `Exited` → `CleanedUp`
- **Transitions:** exec paths (`yukimi-launch:5-10`); mktemp (`:11`); traps (`:21-23`); start (`:25-28`); wait and clear the pid (`:30-31`); cleanup kill, wait, rm, rmdir (`:13-20`)
- **Hazards:** the exit status is Weston's, not the app's; an `rmdir` failure in the EXIT trap under `set -e` changes the exit status
- **Reify:** Low — small and local

<a id="yuk-14"></a>

### YUK-14 — GUI reading epochs (configuration, store, Nixpkgs path, deferred launch)
- **Where:** `desktop/aizome/apps/yukimi/src/app.rs:345` (`Yukimi.loading`), `:347` (`.reading_store`), `:348` (`.epoch`), `:390` (`.launch`)
- **Encoding:** an epoch counter tagging `Msg::Read`, `Msg::StoreRead` and `Msg::FlakeNixpkgs`; two bools; an `Option<Launch>` consumed by the first read
- **States:** `Loading(e)` → `ReadingStore(e)` → `Ready(e)`; launch `Pending` → `Proposed`
- **Transitions:** `reload` (`app.rs:515-523`); `read` drops stale epochs, clears loading, reconciles drafts, consumes the launch, spawns the store and Nixpkgs reads (`:525-571`); `StoreRead` (`:1289-1301`); `FlakeNixpkgs` (`:1302-1307`); `Launched` deferred while loading (`:1583-1590`)
- **Hazards:** between `read` and `StoreRead` the model pairs new generations with the previous store (`app.rs:532`), so a generation opened in that window is diffed against a graph that lacks it (every package "Removed") until the store read reopens it; a second launch during loading overwrites a pending one (`:1585`)
- **Reify:** Medium — three fields and four handlers, guarded by the epoch

<a id="yuk-15"></a>

### YUK-15 — Package index lifecycle
- **Where:** `desktop/aizome/apps/yukimi/src/app.rs:350` (`.index`), `:353` (`.index_stale`), `:354` (`.indexing`); cache `apps/yukimi/crates/yukimi-system/src/index.rs:113` (`fn load_cached`), `:127` (`fn make`)
- **Encoding:** `Option<Arc<PackageIndex>>` and two bools; `IndexMade(Err(""))` as a sentinel for "the cached copy is current"; files `~/.cache/aizome/yukimi/packages-<nixpkgs>.json` (plus `.new`), newest two kept, the GTK Yukimi's `~/.cache/yukimi` read too
- **States:** `None` → `Indexing` → `Current` | `StaleWhileIndexing` → `Current` | `Failed(None or Stale)`; representable but unhandled: `index_stale && !indexing` after a failure
- **Transitions:** `ensure_index` guard and start (`app.rs:578-613`), from `read` (`:551`), `show` (`:786`), `Search` (`:1423`), `FlakeNixpkgs` (`:1305`), `MakeIndex` (`:1325`); `IndexCached` (`:1308-1312`); `IndexMade` (`:1313-1324`); cache write, rename and retention (`index.rs:148-155`)
- **Hazards:** nothing records which Nixpkgs the loaded index is for, and `IndexCached`/`IndexMade` carry no epoch; failures have no backoff
- **Reify:** High — scattered triggers, a sentinel error, two suspected bugs
- **Suspected bug:** after a live update moves the system's Nixpkgs, `read` calls `ensure_index`, which returns early because an index exists and isn't marked stale (`app.rs:583`); Discover keeps searching the old Nixpkgs' list (versions, "on your system" badges) for the rest of the session, and an in-flight build for the old path that lands later is marked current too (`:1316-1318`). (confidence: high)
- **Suspected bug:** when `make` fails (`index.rs:142-145`), `indexing` drops to false with the index still None or stale, so every Search keystroke, Discover visit or reload starts another minute-long, multi-GB `nix eval` and posts another note. (confidence: medium)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#yuk-15)

<a id="yuk-16"></a>

### YUK-16 — Update check (in-flight flag and persisted answer)
- **Where:** `desktop/aizome/apps/yukimi/src/app.rs:356` (`.check`), `:357` (`.checking`), `:402` (`.auto_check`); `desktop/aizome/apps/yukimi/src/checks.rs:26` (`UpdateCheck.when`, `.of`, `.sources`)
- **Encoding:** `Option<Arc<UpdateCheck>>` and a bool; `~/.cache/aizome/yukimi/updates.json` (or the GTK Yukimi's), chosen by `of` and the newest `when`; ten-minute staleness; scratch directory `checks/`
- **States:** `NoCheck` → `Checking` → `Fresh` → `Stale` → `Checking`; a failed check keeps the previous answer
- **Transitions:** load at the first read (`app.rs:534-536`); `check_if_stale` (`:619-628`) from `read` (`:552-554`) and `show(Updates)` (`:788`); `check_updates` (`:630-641`); `Checked` (`:1329-1335`); `run` resets scratch and saves (`checks.rs:109-124`); an all-failed check keeps the last (`:146-148`); per-source guard in `movements` (`:156`, `:174`)
- **Hazards:** `Checked` carries no epoch, and `job_event` shows Updates before reloading (`app.rs:747`, `:755`), so a check can run against the pre-update model (mitigated by recomputing movements against the current model); non-atomic save (`checks.rs:91`)
- **Reify:** Medium — cache validity rules split between app.rs and checks.rs

<a id="yuk-17"></a>

### YUK-17 — Search query coalescing
- **Where:** `desktop/aizome/apps/yukimi/src/app.rs:363` (`.asked`), `:364` (`.found`), `:365` (`.searcher`)
- **Encoding:** a query counter, the last result tagged by query text and index pointer, a lazily spawned thread draining to the newest query
- **States:** `Idle` → `Asked(n)` → `Answered(n)`; answers for m < n dropped
- **Transitions:** `ask` (`app.rs:646-675`), drain (`:661-663`), thread exit when the stream closes (`:667-669`); `Found` (`:1426-1430`)
- **Hazards:** `index_id` is an `Arc` address (`app.rs:335-337`), so a freed index's address reused by a new one makes an old `found` look current (`desktop/aizome/apps/yukimi/src/ui/discover.rs:230`)
- **Reify:** Low — small and correct

<a id="yuk-18"></a>

### YUK-18 — GUI job record and progress sheet
- **Where:** `desktop/aizome/apps/yukimi/src/app.rs:392` (`.job`), `:393` (`.job_sheet`), `:394` (`.jobs`), `:395` (`.quiet_timer`); `desktop/aizome/apps/yukimi/src/job.rs:245` (`Job.outcome`), `:246` (`.retry_authentication`), `:236` (`.status`)
- **Encoding:** `Option<Job>` where running means `outcome.is_none()` (partially explicit), a sheet key, an id counter, a repeating timer, a status string, a linger message
- **States:** `NoJob` → `Running(Starting | WaitingForPermission | Working | Recovering | Stopping)` → `Done` (lingers 900 ms, then closes) | `Failed(retryable?)` → `Closed`; representable but invalid: outcome set and later events still applied
- **Transitions:** `run` (`app.rs:699-735`, guard `:680-685`); `job_event` (`:737-756`); `Job::take` (`desktop/aizome/apps/yukimi/src/job.rs:285-345`); `request_stop` (`desktop/aizome/apps/yukimi/src/job.rs:349-354`, from `app.rs:1521-1525`); `close_job` (`app.rs:758-766`); `JobLinger` (`:1548-1552`); `RetryAuthentication` (`:1527-1535`); `close_requested` (`:1606-1636`)
- **Hazards:** `take` has no terminal guard (a test injects an event after Done, `desktop/aizome/apps/yukimi/tests/headless.rs:719-722`); if the worker thread dies without `Done`, the job is "running" forever: Close is refused, Stop only changes the status, and Yukimi can't be closed normally; the quiet timer is cancelled only by an outcome or a new run
- **Reify:** High — state across two structs and nine handlers, user-visible close and stop rules
- **Related:** privileged helper through `desktop/aizome/apps/yukimi/src/ops.rs:278` (`root_step`) and `apps/yukimi/yukimi-helper/src/transaction.rs:45`

<a id="yuk-19"></a>

### YUK-19 — Desktops page choice drafts
- **Where:** `desktop/aizome/apps/yukimi/src/app.rs:385` (`.draft`), `:388` (`.moved`)
- **Encoding:** full-value drafts per setting plus the set of drafts Yukimi moved itself; three-way display against configured and running values (`desktop/aizome/apps/yukimi/src/ui/desktops.rs:130-138`)
- **States:** per setting `Configured` | `Drafted` (by the user) | `Moved` (a dependent forced into its `within` list) → `Applied` (draft equals configured after a read); shown also as `BuiltNotRunning` ("after restart")
- **Transitions:** `set_choice` with recursive dependent move and restore (`app.rs:1054-1087`); `pick` (`:1089-1112`); `propose` (`:1172-1183`); `RevertChoices` (`:1411-1414`); `UndoChoice` (`:1415-1418`); reconciliation on read (`:544-547`); `apply_choices` (`:1115-1163`)
- **Hazards:** the move-and-restore logic lives only in `set_choice`; drafts survive unrelated operations
- **Reify:** High — invariants between two collections, recursion, suspected bug
- **Suspected bug:** untick Aizome under Desktops (Yukimi moves the default desktop to Tatami, `app.rs:1077-1085`), then press that group's Undo: `UndoChoice` removes only the desktops draft (`:1415-1418`), so Default desktop stays drafted as "Tatami (chosen)" and Apply writes `calamares.defaultDesktop = "tatami"`, a change the user never made. (confidence: high)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#yuk-19)

<a id="yuk-20"></a>

### YUK-20 — Sheet lifecycles: package details, branch chooser, comparison
- **Where:** `desktop/aizome/apps/yukimi/src/app.rs:367` (`.details`), `:380` (`.branch`; `BranchChooser.branches` `:161`), `:381` (`.comparison`; `ComparisonSheet.details` `:168`, `.path` `:169`)
- **Encoding:** an `Option<(WindowKey, …)>` per sheet, an async payload `Option` inside, a drill-down path
- **States:** `Closed` → `Open(Loading)` → `Open(Loaded)` → `Closed`; comparison path depth
- **Transitions:** `Open`/`close_details` (`app.rs:1431-1435`, `:778-782`); `choose_branch` spawns the branch list (`:906-920`); `Branches` (`:1397-1401`); `follow` (`:922-947`); `InspectChanges` (`:1342-1367`); `ComparisonLoaded` checks the sheet key (`:1368-1374`); drill-down (`:1375-1396`); `CloseSheet` (`:1564-1575`); `close_requested` (`:1619-1627`)
- **Hazards:** late comparison results are guarded by the sheet key; the branch list isn't
- **Reify:** High — suspected bug (otherwise Medium: three similar Options)
- **Suspected bug:** `Msg::Branches` carries neither input nor sheet key (`app.rs:915`, `:1397-1401`): open the branch chooser for one GitHub input, cancel, open it for another, and whichever GitHub answer arrives last fills the open sheet, so the user can pick another repository's branch and start a Follow that fails in the helper. (confidence: medium)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#yuk-20)

<a id="yuk-21"></a>

### YUK-21 — History generation diffs
- **Where:** `desktop/aizome/apps/yukimi/src/app.rs:372` (`.opened`), `:373` (`.changes`), `:140` (`enum Changes`)
- **Encoding:** the set of open rows plus per generation `Working | Ready{kept, diff, store_read, has_previous}` (partially explicit)
- **States:** `Closed` → `Open(Working)` → `Open(Ready)` → `Closed(cached Ready)`
- **Transitions:** `toggle_generation` (`app.rs:874-904`); `Changes` accepted only while open (`:1472-1476`); a store read clears and reopens (`:1294-1299`); `ReturnTo` reads `kept` (`:1477-1495`)
- **Hazards:** diff results carry no store epoch; two in-flight diffs for one generation race and the last writer wins; see YUK-14 for the stale-store window
- **Reify:** Medium — invariants with `opened` and the store are implicit

<a id="yuk-22"></a>

### YUK-22 — Status line notes
- **Where:** `desktop/aizome/apps/yukimi/src/app.rs:398` (`.notes`), `:399` (`.notes_made`)
- **Encoding:** a deque of (id, text) capped at three; `NoteGone(id)` timers
- **States:** `Shown(id)` → `Expired`
- **Transitions:** `note` (`app.rs:501-508`); `NoteGone` (`:1582`)
- **Hazards:** none observed
- **Reify:** Low — small and local

<a id="yuk-23"></a>

### YUK-23 — Restart or sign out to finish an update
- **Where:** `desktop/aizome/apps/yukimi/src/app.rs:1195` (`fn finish`)
- **Encoding:** the expected `Completion.action` captured from the model; the worker reads completion again before acting
- **States:** `Offered` → `Rechecking` → `Requested` | `Refused(status changed)` → `Reloaded`
- **Transitions:** `Restart`/`SignOut` (`app.rs:1555-1556`); recheck (`:1204-1206`); `systemctl --check-inhibitors=yes reboot` or `loginctl terminate-session` (`:1207-1213`, `desktop/aizome/apps/yukimi/src/ops.rs:22-26`); `Finished` → reload (`app.rs:1557-1562`)
- **Hazards:** no in-flight flag, so repeated presses run the command again; only the action is compared, not the reasons
- **Reify:** Low — a small check-then-act
- **Related:** logind (`loginctl`, `systemctl reboot`)

<a id="yuk-24"></a>

### YUK-24 — Single-instance socket and hand-off
- **Where:** `desktop/aizome/apps/yukimi/src/instance.rs:20` (`fn claim`), `desktop/aizome/apps/yukimi/src/app.rs:405` (`.listener`); also `desktop/aizome/apps/yukimi/src/main.rs:55`
- **Encoding:** the `$XDG_RUNTIME_DIR/aizome-app.yukimi.sock` file; the listener `Option` moved into a thread
- **States:** `Unclaimed` → `Handed` (exit) | `First(listener)` → `Listening` → `Released`; `First(None)` when bind fails
- **Transitions:** connect and write → Handed (`instance.rs:22-27`); unlink stale and bind (`:29-30`); listening thread started at launch (`app.rs:1277-1280`), ends when the app's stream closes (`instance.rs:40-42`); `release` unlinks (`:49-53`, `desktop/aizome/apps/yukimi/src/main.rs:67-69`)
- **Hazards:** two simultaneous launches both unlink and bind, leaving two instances with only the last reachable; `release` unlinks whatever socket is at the path, including another live instance's; a failed bind silently runs without hand-off
- **Reify:** Medium — small, but its races leak extra instances

<a id="yuk-25"></a>

### YUK-25 — Job step runner with pkexec authentication recovery
- **Where:** `desktop/aizome/apps/yukimi/src/job.rs:136` (`fn work`), `:153` (`recovery_deadline`), `:155-156` (`last`, `seen`, `pending`)
- **Encoding:** a loop over boxed steps; throttle state per attempt; a 5 s recovery deadline; exact-match classification of pkexec failures
- **States:** `NextStep` → `Building` → `Running(as_root)` → `Recovering` (up to 5 s, retries the same command) → `StepDone` … → `Done(Ok | Err)` | `AuthorizationFailed(Unavailable | Cancelled | Denied)`
- **Transitions:** stop check (`desktop/aizome/apps/yukimi/src/job.rs:139-142`); build the step (`:143-150`); signal mode and Started (`:151-152`); stream with throttled snapshots (`:157-172`); success (`:174-178`); classification (`:70-98`, `:180-207`); recovery sleeps (`:191-201`); finish (`:212`, `:217-221`)
- **Hazards:** the throttle has no trailing flush: a snapshot arriving within 150 ms of the last send waits in `pending` until the next stderr line (`desktop/aizome/apps/yukimi/src/job.rs:163-168`), so during a silent activation or collection the sheet can show the previous stage and `heard` isn't refreshed
- **Reify:** Medium — nested loops and retry rules are hard to follow but local
- **Related:** privileged helper through pkexec `desktop/aizome/apps/yukimi/src/ops.rs:379-387`

<a id="yuk-26"></a>

### YUK-26 — (test harness) fake pkexec and stop timers
- **Where:** `desktop/aizome/apps/yukimi/src/job.rs:481` (attempt-counting script), `:458`, `:559`, `:581`; also `apps/yukimi/crates/yukimi-system/src/nix.rs:231`
- **Encoding:** a shell script counting attempts in a temporary file and failing twice the way pkexec does; timer threads calling `request()`
- **States:** `Attempt1Fails` → `Attempt2Fails` → `Executed`; stop timer `Sleeping` → `Requested`
- **Transitions:** attempt count (`desktop/aizome/apps/yukimi/src/job.rs:489-495`); timers (`desktop/aizome/apps/yukimi/src/job.rs:464-467`, `nix.rs:241-244`, `:255-258`)
- **Hazards:** none observed (asserts are time-bounded)
- **Reify:** Low — test fixture

<a id="yuk-27"></a>

### YUK-27 — Unprivileged update preparation pipeline
- **Where:** `desktop/aizome/apps/yukimi/src/ops.rs:289` (`fn update_steps`); `apps/yukimi/crates/yukimi-system/src/updates.rs:312` (`fn lock_command`), `:324` (`fn new_sources`), `:344` (`fn fetch_command`)
- **Encoding:** fixed files `~/.cache/aizome/yukimi/update.lock` and `update-sources.json`; step order lock → fetch → helper `--lock`
- **States:** `Locking` → `Fetching` | `NothingToFetch` → `Authorizing` → `HelperChecks` (YUK-09) → …; `NoCacheDir` → the helper fetches everything itself
- **Transitions:** no cache directory (`ops.rs:292-294`); the lock step removes the old lock and locks as the user (`:296-303`); fetch step (`:304-317`, skip `:310-312`); helper with `--lock` (`:318`, `:210-215`); helper acceptance check (`apps/yukimi/yukimi-helper/src/main.rs:931-936`, `apps/yukimi/crates/yukimi-config/src/lock.rs:335-361`)
- **Hazards:** the proposed lock is made before the password prompt, so a flake.lock change in between (CLI, another frontend) makes the helper refuse the whole update rather than lock again; fixed shared file names; a leftover `update.lock` is removed only by the next lock step
- **Reify:** Medium — three-step pipeline with an on-disk hand-off across the privilege boundary
- **Related:** privileged helper `apps/yukimi/yukimi-helper/src/main.rs:927`

<a id="yuk-28"></a>

### YUK-28 — Falling snow animation ticks
- **Where:** `desktop/aizome/apps/yukimi/src/ui/widgets.rs:483` (`SnowState.ticking`, `.last`, `.time`, `.active`)
- **Encoding:** kit widget state with an `Option<Instant>`, a bool, and a `Cell<bool>` written while painting
- **States:** `Still` → `Ticking` (33 ms) ⇄ `Background` (1 s poll) → `Still` when motion is off
- **Transitions:** start in layout (`widgets.rs:517-522`); stop when motion is off (`:570-573`); background poll (`:574-581`); advance (`:582-587`); `active` set while painting (`:528`)
- **Hazards:** activity is learned only through a 1×1 repaint, so an unpainted window keeps polling once a second
- **Reify:** Low — local animation

<a id="yuk-29"></a>

### YUK-29 — Per-input revision phase from saved generations
- **Where:** `desktop/aizome/apps/yukimi/src/update_history.rs:10` (`History.snapshots`, `.active`, `.current`)
- **Encoding:** saved flake.lock inputs per generation; active and selected generation numbers; the lock's pin; the last check's target
- **States:** per input `RunningIsPinned` | `PinnedNotBuilt` | `Prepared` (selected ≠ active) | `TargetNewer`; links prefer Pending → Prepared → Configured → Last update → Running or Pinned commit
- **Transitions:** derived in `load` (`update_history.rs:17-26`); `prepared` (`:39-43`); `revisions` (`:45-72`); `link` priority (`:74-145`)
- **Hazards:** correctness depends on YUK-10's snapshots; unreadable snapshots are skipped silently
- **Reify:** Low — pure derivation over persisted state
- **Related:** helper snapshots `apps/yukimi/yukimi-helper/src/main.rs:327`

<a id="yuk-30"></a>

### YUK-30 — (test harness) settle loops and scripted Nix/helper stand-ins
- **Where:** `desktop/aizome/apps/yukimi/tests/headless.rs:55` (`fn settle`), `:17` (`WORKS`), `:23` (`FAILS`), `:38` (recording mock runner); also `desktop/aizome/apps/yukimi/tests/golden.rs:17`, `desktop/aizome/apps/yukimi/tests/look.rs:10`, `:83`
- **Encoding:** 5 ms polling for up to 5 s with a `NoteGone(0)` message as a pump; shell scripts emitting `@nix` events and a JSON report
- **States:** `Waiting` → `Settled` | `TimedOut` (asserted only in headless.rs)
- **Transitions:** poll loops (`headless.rs:57-60`, `golden.rs:19-22`, `look.rs:12-15`); an event injected after completion (`headless.rs:719-722`)
- **Hazards:** golden.rs and look.rs don't assert after timing out; look.rs's stand-in sleeps 30 s and relies on Stop
- **Reify:** Low — test harness

## Already explicit

- `desktop/aizome/apps/yukimi/src/app.rs:36` `enum Page` — the sidebar place; transitions centralised in `fn show` (`desktop/aizome/apps/yukimi/src/app.rs:784`), with the initial value from launch arguments (`desktop/aizome/apps/yukimi/src/main.rs:61-63`)

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `apps/yukimi/crates/yukimi-config/src/branch.rs` | 113 | none |  | pure URL branch functions |
| `apps/yukimi/crates/yukimi-config/src/edit.rs` | 694 | none |  | pure Nix text editing |
| `apps/yukimi/crates/yukimi-config/src/lib.rs` | 42 | none |  | error enum |
| `apps/yukimi/crates/yukimi-config/src/lock.rs` | 521 | none |  | pure lock parsing; check_update guard used by YUK-09 |
| `apps/yukimi/crates/yukimi-config/src/packages.rs` | 138 | none |  | pure render/read of yukimi.nix |
| `apps/yukimi/crates/yukimi-config/src/sekka.rs` | 361 | machines | [YUK-01](#yuk-01) | flake update channel and migration |
| `apps/yukimi/crates/yukimi-config/tests/legacy-flake.nix` | 45 | none |  | test fixture data |
| `apps/yukimi/crates/yukimi-store/src/db.rs` | 341 | none |  | read-only queries; one-shot open fallback |
| `apps/yukimi/crates/yukimi-store/src/lib.rs` | 34 | none |  | module root and errors |
| `apps/yukimi/crates/yukimi-store/src/path.rs` | 214 | none |  | pure store path parsing |
| `apps/yukimi/crates/yukimi-store/src/roots.rs` | 194 | none |  | stateless root scan |
| `apps/yukimi/crates/yukimi-system/examples/completion.rs` | 6 | none |  | one-shot diagnostic print |
| `apps/yukimi/crates/yukimi-system/src/catalog.rs` | 258 | none |  | catalog data merge |
| `apps/yukimi/crates/yukimi-system/src/channels.rs` | 109 | none |  | stateless channel listing |
| `apps/yukimi/crates/yukimi-system/src/completion.rs` | 1082 | machines | [YUK-02](#yuk-02), [YUK-03](#yuk-03) | update receipt; Tatami session record |
| `apps/yukimi/crates/yukimi-system/src/completion/aizome.rs` | 1155 | machines | [YUK-04](#yuk-04), [YUK-05](#yuk-05) | live Aizome probe; fake IPC server |
| `apps/yukimi/crates/yukimi-system/src/diff.rs` | 160 | none |  | pure closure diff |
| `apps/yukimi/crates/yukimi-system/src/generations.rs` | 137 | none |  | reads profile links; flags feed YUK-02 and YUK-29 |
| `apps/yukimi/crates/yukimi-system/src/index.rs` | 301 | machines | [YUK-15](#yuk-15) | package index cache files |
| `apps/yukimi/crates/yukimi-system/src/info.rs` | 67 | none |  | system facts read |
| `apps/yukimi/crates/yukimi-system/src/lib.rs` | 99 | none |  | constants and formatting; saved path for YUK-10 |
| `apps/yukimi/crates/yukimi-system/src/log.rs` | 349 | machines | [YUK-06](#yuk-06) | Nix activity tracker |
| `apps/yukimi/crates/yukimi-system/src/nix.rs` | 265 | machines | [YUK-07](#yuk-07), [YUK-26](#yuk-26) | stop handle; test timers |
| `apps/yukimi/crates/yukimi-system/src/profile.rs` | 148 | none |  | manifest v2/v3 parse |
| `apps/yukimi/crates/yukimi-system/src/setup.rs` | 398 | none |  | configuration detection |
| `apps/yukimi/crates/yukimi-system/src/updates.rs` | 457 | machines | [YUK-01](#yuk-01), [YUK-27](#yuk-27) | Sekka targets; preparation commands |
| `apps/yukimi/nix/helper.nix` | 52 | none |  | package build |
| `apps/yukimi/nix/module.nix` | 202 | machines | [YUK-09](#yuk-09), [YUK-12](#yuk-12) | preSwitchChecks gate; operations dir retention |
| `apps/yukimi/nix/nixos.nix` | 8 | none |  | module wrapper |
| `apps/yukimi/nix/package.nix` | 32 | none |  | package join |
| `apps/yukimi/yukimi-helper/data/io.github.bitemyapp.Yukimi.policy.in` | 19 | none |  | polkit action; auth_admin_keep cache is polkit's |
| `apps/yukimi/yukimi-helper/src/bin/sekka-update.rs` | 124 | machines | [YUK-12](#yuk-12), [YUK-01](#yuk-01), [YUK-03](#yuk-03) | CLI client keeps stdin open; finish_update |
| `apps/yukimi/yukimi-helper/src/main.rs` | 1395 | machines | [YUK-08](#yuk-08), [YUK-09](#yuk-09), [YUK-10](#yuk-10), [YUK-11](#yuk-11), [YUK-02](#yuk-02), [YUK-06](#yuk-06), [YUK-12](#yuk-12) | phase; change; snapshots; rollback and clean |
| `apps/yukimi/yukimi-helper/src/transaction.rs` | 112 | machines | [YUK-12](#yuk-12) | operation client and worker |
| `desktop/aizome/apps/yukimi/data/yukimi-launch` | 32 | machines | [YUK-13](#yuk-13) | nested Weston lifecycle |
| `desktop/aizome/apps/yukimi/src/app.rs` | 1714 | machines | [YUK-14](#yuk-14), [YUK-15](#yuk-15), [YUK-16](#yuk-16), [YUK-17](#yuk-17), [YUK-18](#yuk-18), [YUK-19](#yuk-19), [YUK-20](#yuk-20), [YUK-21](#yuk-21), [YUK-22](#yuk-22), [YUK-23](#yuk-23) | GUI state and handlers |
| `desktop/aizome/apps/yukimi/src/checks.rs` | 333 | machines | [YUK-16](#yuk-16) | update check cache and staleness |
| `desktop/aizome/apps/yukimi/src/comparison.rs` | 321 | none |  | pure loader for YUK-20 sheet |
| `desktop/aizome/apps/yukimi/src/fixture.rs` | 596 | none |  | sample data tables |
| `desktop/aizome/apps/yukimi/src/instance.rs` | 53 | machines | [YUK-24](#yuk-24) | single-instance socket |
| `desktop/aizome/apps/yukimi/src/job.rs` | 590 | machines | [YUK-25](#yuk-25), [YUK-26](#yuk-26), [YUK-18](#yuk-18) | step runner; Job record; fake pkexec |
| `desktop/aizome/apps/yukimi/src/lib.rs` | 21 | none |  | module list |
| `desktop/aizome/apps/yukimi/src/main.rs` | 70 | machines | [YUK-24](#yuk-24) | claim and release around the run |
| `desktop/aizome/apps/yukimi/src/model.rs` | 654 | none |  | snapshot read on a worker; consumed by YUK-14 |
| `desktop/aizome/apps/yukimi/src/ops.rs` | 569 | machines | [YUK-27](#yuk-27), [YUK-23](#yuk-23) | update preparation steps; logout command |
| `desktop/aizome/apps/yukimi/src/paths.rs` | 99 | none |  | path configuration |
| `desktop/aizome/apps/yukimi/src/ui/comparison.rs` | 196 | none |  | view of YUK-20 |
| `desktop/aizome/apps/yukimi/src/ui/desktops.rs` | 162 | none |  | view of YUK-19 tags |
| `desktop/aizome/apps/yukimi/src/ui/discover.rs` | 384 | none |  | view of YUK-15 and YUK-17 |
| `desktop/aizome/apps/yukimi/src/ui/history.rs` | 173 | none |  | view of YUK-21 |
| `desktop/aizome/apps/yukimi/src/ui/installed.rs` | 295 | none |  | view |
| `desktop/aizome/apps/yukimi/src/ui/job.rs` | 106 | none |  | view of YUK-18 |
| `desktop/aizome/apps/yukimi/src/ui/mod.rs` | 481 | none |  | view; status line priority |
| `desktop/aizome/apps/yukimi/src/ui/overview.rs` | 451 | none |  | view |
| `desktop/aizome/apps/yukimi/src/ui/storage.rs` | 212 | none |  | view |
| `desktop/aizome/apps/yukimi/src/ui/updates.rs` | 476 | none |  | view of YUK-16 and YUK-20 |
| `desktop/aizome/apps/yukimi/src/ui/widgets.rs` | 812 | machines | [YUK-28](#yuk-28) | snow ticks; tap uses kit press state |
| `desktop/aizome/apps/yukimi/src/update_history.rs` | 363 | machines | [YUK-29](#yuk-29) | revision phase from saved locks |
| `desktop/aizome/apps/yukimi/tests/budget.rs` | 179 | none |  | tests; benchmark loops |
| `desktop/aizome/apps/yukimi/tests/golden.rs` | 108 | machines | [YUK-30](#yuk-30) | settle loop |
| `desktop/aizome/apps/yukimi/tests/headless.rs` | 732 | machines | [YUK-30](#yuk-30) | settle loop and scripted stand-ins |
| `desktop/aizome/apps/yukimi/tests/look.rs` | 197 | machines | [YUK-30](#yuk-30) | settle loop and scripted stand-ins |
