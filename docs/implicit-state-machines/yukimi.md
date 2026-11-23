# YUK: Yukimi system manager (Aizome app and core crates)

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/apps/yukimi, apps/yukimi`. Audited at `8e0acbe5`; line numbers refer to that commit.

62 files audited. 30 implicit machines: 11 High, 9 Medium, 10 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

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
- **Reproduction:** Confirmed (unit): [summary](reproductions.md#yuk-02)
- **Follow-up:** reified as typestate in [#290](https://github.com/bitemyapp/sekka/pull/290). Its suspected bug is fixed, and the mistake no longer compiles.

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
- **Reproduction:** Confirmed (end-to-end): [summary](reproductions.md#yuk-09)
- **Follow-up:** reified as typestate in [#290](https://github.com/bitemyapp/sekka/pull/290). Its suspected bug is fixed, and the mistake no longer compiles.

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
- **Reproduction:** Confirmed (end-to-end): [summary](reproductions.md#yuk-10)
- **Follow-up:** reified as typestate in [#290](https://github.com/bitemyapp/sekka/pull/290). Its suspected bug is fixed, and the mistake no longer compiles.

<a id="yuk-11"></a>

### YUK-11 — Rollback and clean-up sequences
- **Where:** `apps/yukimi/yukimi-helper/src/main.rs:1011` (`fn rollback`), `:1068` (`fn clean`)
- **Encoding:** statement order around `commit()`, receipt writes and `switch-to-configuration`
- **States:** rollback `Validated` → `Planned(auto)` → `RunningSaved` → `Committed` → `ProfileSwitched` → `ReceiptActivating` → `ChoicesRestored` → `Switched` → `Verified` → `ReceiptActivated | ReceiptPrepared`; clean `GenerationsDeleted`? → `Pruned` → `Collecting` (stoppable) → `Committed` → `BootMenuUpdated`
- **Transitions:** rollback commits before changing anything (`apps/yukimi/yukimi-helper/src/main.rs:1030`), `--switch-generation` (`:1031-1033`), receipt (`:1034-1040`), restore (`:1043-1051`), switch (`:1052`), verify (`:1053-1055`), receipt (`:1056-1057`); clean deletes and prunes (`:1084-1091`), collects through `wait` (`:1092-1103`), commits (`:1106`), runs `switch-to-configuration boot` (`:1107`)
- **Hazards:** a rollback can't be stopped at all; a crash between the profile switch and the receipt leaves the previous receipt; restored choices are written before the switch succeeds; clean's collection stays cancellable after generations are already gone
- **Reify:** High — irreversible steps before an uncancellable commit point, suspected bug
- **Suspected bug:** Clean Up with "older than a week", then Stop during the long `nix-store --gc`: `wait()` returns "Stopped" (`apps/yukimi/yukimi-helper/src/main.rs:1095`) before `commit()` and `switch-to-configuration boot` (`:1106-1107`), so the generations are deleted but the boot menu still lists them; the next collection removes their closures and those entries no longer boot. The same happens if GC fails. (confidence: medium)
- **Reproduction:** Confirmed (end-to-end): [summary](reproductions.md#yuk-11)
- **Follow-up:** reified as typestate in [#290](https://github.com/bitemyapp/sekka/pull/290). Its suspected bug is fixed, and the mistake no longer compiles.

<a id="yuk-12"></a>

### YUK-12 — Privileged operation client and worker (pkexec client, transient unit, result file)
- **Where:** `apps/yukimi/yukimi-helper/src/transaction.rs:45` (`fn client`), `:84` (`cancelled`), `:8` (`OPERATIONS`); worker `apps/yukimi/yukimi-helper/src/main.rs:1131`, lock `:1193`; retention `apps/yukimi/nix/module.nix:144`
- **Encoding:** `/var/lib/yukimi/operations/<nanos>-<pid>/` holding `progress.log` and `result.new` → `result.json`; unit `yukimi-operation-<id>.service` (StandardInput=null, KillMode=control-group, TimeoutStopSec=infinity, `--collect`); `/run/lock/yukimi.lock` flock; the client loop's `cancelled` flag; the client's `Phase` reused as "cancel requested"
- **States:** `DirCreated` → `UnitStarted` → `Running` → `CancelForwarded` (once) → `ResultWritten` → `UnitGone`; `Lost` (unit not active, no result); worker `LockBusy` → error result
- **Transitions:** directory and log (`transaction.rs:46-57`); `systemd-run` (`:58-82`); stdin and signal watch (`:83`); forward the log (`:86`); read the result (`:87-90`, race `:100-104`); SIGINT forwarded once (`:91-95`); ActiveState poll (`:96-109`), 250 ms sleep (`:110`); worker directory check (`:10-21`), lock (`apps/yukimi/yukimi-helper/src/main.rs:1193-1202`), worker stop watch (`apps/yukimi/yukimi-helper/src/main.rs:1203`), atomic result (`transaction.rs:23-31`, `apps/yukimi/yukimi-helper/src/main.rs:1153`)
- **Hazards:** no client timeout (by design); the single SIGINT can land before the worker installs its handler (it takes the lock first, `apps/yukimi/yukimi-helper/src/main.rs:1193-1203`), killing it without a result, so the GUI reports "ended without a verified result"; a dead client leaves `result.json` unread (the GUI recovers only through YUK-02); tmpfiles deletes operation logs after 30 days; the client runs `systemctl show` four times a second for the whole build
- **Reify:** High — privilege boundary and cross-process protocol encoded in files, unit state and a flag
- **Related:** GUI `desktop/aizome/apps/yukimi/src/ops.rs:379` (pkexec) and `desktop/aizome/apps/yukimi/src/job.rs:136`; CLI `apps/yukimi/yukimi-helper/src/bin/sekka-update.rs:77-78` (keeps stdin open)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#yuk-15)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#yuk-19)

<a id="yuk-20"></a>

### YUK-20 — Sheet lifecycles: package details, branch chooser, comparison
- **Where:** `desktop/aizome/apps/yukimi/src/app.rs:367` (`.details`), `:380` (`.branch`; `BranchChooser.branches` `:161`), `:381` (`.comparison`; `ComparisonSheet.details` `:168`, `.path` `:169`)
- **Encoding:** an `Option<(WindowKey, …)>` per sheet, an async payload `Option` inside, a drill-down path
- **States:** `Closed` → `Open(Loading)` → `Open(Loaded)` → `Closed`; comparison path depth
- **Transitions:** `Open`/`close_details` (`app.rs:1431-1435`, `:778-782`); `choose_branch` spawns the branch list (`:906-920`); `Branches` (`:1397-1401`); `follow` (`:922-947`); `InspectChanges` (`:1342-1367`); `ComparisonLoaded` checks the sheet key (`:1368-1374`); drill-down (`:1375-1396`); `CloseSheet` (`:1564-1575`); `close_requested` (`:1619-1627`)
- **Hazards:** late comparison results are guarded by the sheet key; the branch list isn't
- **Reify:** High — suspected bug (otherwise Medium: three similar Options)
- **Suspected bug:** `Msg::Branches` carries neither input nor sheet key (`app.rs:915`, `:1397-1401`): open the branch chooser for one GitHub input, cancel, open it for another, and whichever GitHub answer arrives last fills the open sheet, so the user can pick another repository's branch and start a Follow that fails in the helper. (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#yuk-20)
