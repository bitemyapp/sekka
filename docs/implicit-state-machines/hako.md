# HAKO: Hako file manager

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/apps/hako`. Audited at `8e0acbe5`; line numbers refer to that commit.

45 files, 0 pending. 31 implicit machines: 11 High, 6 Medium, 14 Low.

## Machines

<a id="hako-01"></a>

### HAKO-01 — FileManager1 show-in-folder service bus-name lifecycle
- **Where:** `desktop/aizome/apps/hako/src/file_manager.rs:113` (`serve`); also `desktop/aizome/apps/hako/src/file_manager.rs:109` (`connect`), `desktop/aizome/apps/hako/src/file_manager.rs:35` (`FileManager::show`)
- **Encoding:** the zbus `Connection` held by `serve` (object exported, then name requested without queueing or replacement), the blocking `receive_name_lost()` stream, the process exit status, and systemd `Restart=on-failure` in the session unit
- **States:** `Exporting` → `NameOwned/Serving` → `NameLost` | `BusClosed` (process exits with error); `NameTaken` at start exits immediately; per call: `HandOver` (running Hako) | `Launch` (new Hako)
- **Transitions:** `connect` exports then claims (`file_manager.rs:110`); `NameTaken` → error exit (`file_manager.rs:121`); NameLost → error exit (`file_manager.rs:127-128`); stream end → error exit (`file_manager.rs:130`); per call `instance::hand_over` ok → done (`file_manager.rs:63`), else `launch_command_observed` (`file_manager.rs:85`)
- **Hazards:** every exit is an error, so with another non-replaceable provider holding the name the unit restarts every 2 s until systemd's start limit trips; the hand-over/launch choice is per call with no memory, so two calls racing a cold start each launch a Hako (the instance lock in HAKO-27 then folds them)
- **Reify:** Low — linear lifecycle owned by systemd; little to gain
- **Related:** `desktop/aizome/session/aizome-file-manager.service` (Type=dbus, Restart=on-failure); `desktop/aizome/apps/hako/src/ui/instance.rs` (hand-over socket); callers are browsers / the OpenURI portal over D-Bus

<a id="hako-02"></a>

### HAKO-02 — bsdtar child process run (compress/extract) with pause, cancel and partial output
- **Where:** `desktop/aizome/apps/hako/src/fsops/archive.rs:275` (`run`: `stopped`, `child`, reader thread); also `desktop/aizome/apps/hako/src/fsops/archive.rs:130` (`compress`), `desktop/aizome/apps/hako/src/fsops/archive.rs:193` (`extract`)
- **Encoding:** `Child` + `pid`, `stopped: bool` mirroring `Control::is_paused`, an mpsc channel fed by a stderr reader thread (disconnect = child closed stderr), `errors: Vec<String>`, `env.progress.preparing`, and on disk the hidden `partial` file/folder (`names::partial_name`)
- **States:** `Listing` (extract: `bsdtar -t`) → `Preparing` → `Spawned/Running` ↔ `Stopped` (SIGSTOP) → `Exited(ok)` → `RenamedIntoPlace` | `Exited(fail)` | `Killed(cancel)`; on disk `Absent` → `Partial` → `InPlace` | `Removed`
- **Transitions:** spawn (`archive.rs:277`); pause/resume by SIGSTOP/SIGCONT when `paused != stopped` (`archive.rs:319-324`); cancel → SIGCONT+SIGKILL, wait, join (`archive.rs:309-317`); exit seen by `try_wait` with drain (`archive.rs:325-335`) or by disconnect + `wait` (`archive.rs:303-307`); compress commit `rename_noreplace(partial, archive)` (`archive.rs:169`), cleanup on fail/cancel (`archive.rs:175`, `:180`, `:184`); extract partial dir made (`archive.rs:222`), contents moved out (`archive.rs:243-252`), cleanup (`archive.rs:231`, `:236`, `:256`)
- **Hazards:** the extract listing (`archive.rs:202-218`, `Command::output`) ignores pause and cancel, so Stop on a large compressed archive waits for a full decompression pass; after disconnect `child.wait()` blocks with no cancel check, and a failing `wait` spins the loop (disconnected `recv_timeout` returns at once); a crash leaves `.name.hako-part-*` forever (no startup sweep); the archive name is chosen when the menu command runs (`program.rs:397`) and only checked at the final rename, so a name taken meanwhile fails after all the work
- **Reify:** Medium — process lifecycle with signal-mirrored pause state and on-disk partials spread over three functions

<a id="hako-03"></a>

### HAKO-03 — Operation pause/cancel control
- **Where:** `desktop/aizome/apps/hako/src/fsops/control.rs:22` (`Control.cancelled`, `.paused`, `.wake`)
- **Encoding:** `AtomicBool cancelled` + `Mutex<bool> paused` + `Condvar wake`
- **States:** `Running` ↔ `Paused` → `Cancelled`; `paused && cancelled` is representable (cancel while paused) and handled by `checkpoint`
- **Transitions:** `cancel` (`control.rs:34-38`); `set_paused` (`control.rs:44-47`) toggled by `Msg::PauseJob` (`program.rs:565-569`); `checkpoint` waits while paused, returns `Cancelled` (`control.rs:54-63`); mirrored to bsdtar by HAKO-02 (`archive.rs:319-324`)
- **Hazards:** pause/cancel only act at checkpoints, so blocking work between them (bsdtar listing, a conflict prompt, `delete::measure` between its 512-item checkpoints) ignores them; nothing marks a job finished, and the job's state shows "Paused" whatever its question state (`jobs.rs:148-153` orders the checks)
- **Reify:** Low — small, correct and local

<a id="hako-04"></a>

### HAKO-04 — In-kernel copy with buffered fallback
- **Where:** `desktop/aizome/apps/hako/src/fsops/sys.rs:63` (`copy_contents`: `kernel`, `total`, `buf`)
- **Encoding:** local `kernel: bool` mode flag and `total` bytes copied
- **States:** `Kernel` (copy_file_range) → `Buffered` (read/write) → `Done` | `Error` | `StoppedByHearer`
- **Transitions:** fallback on EXDEV/EINVAL/ENOSYS/EOPNOTSUPP/EBADF/EPERM only while `total == 0` (`sys.rs:86-91`); EINTR retry (`sys.rs:92`); each chunk may stop it through `each` (`sys.rs:113`)
- **Hazards:** a copy_file_range failure after some bytes is a hard error instead of a fallback, so the item goes to the error prompt and a Retry restarts the whole file
- **Reify:** Low — one function, one flag

<a id="hako-05"></a>

### HAKO-05 — Transfer placement with conflict prompt and replace-aside protocol
- **Where:** `desktop/aizome/apps/hako/src/fsops/transfer.rs:355` (`Worker::place_top`: `replace`, `aside`); also `desktop/aizome/apps/hako/src/fsops/transfer.rs:449` (`set_aside`), `:463` (`dispose`), `:584` (`merge`), `:276` (`Worker.trash`); `desktop/aizome/apps/hako/src/fsops/names.rs:175` (`aside_name`), `desktop/aizome/apps/hako/src/fsops/names.rs:186` (`is_temporary`)
- **Encoding:** local `replace: bool` and `aside: Option<PathBuf>`, the `Resolution` answer, `Worker.trash: Option<&Trash>`, and on disk the hidden `.name.hako-aside-*` sibling holding the replaced item
- **States:** per item `Stat` → `NoConflict` | `Conflict(asked)` → `Skipped` | `Cancelled` | `KeepBoth(new name)` | `Merging(recursive)` | `Replacing`; replace sub-states `TargetInPlace` → `SetAside` → `NewPlaced` → `Disposed(trashed | deleted)` or `PlaceFailed` → `AsideRestored` | `AsideStranded(problem)`
- **Transitions:** conflict asked (`transfer.rs:401`); KeepBoth renames target (`transfer.rs:404-407`); Merge recurses (`transfer.rs:408-411`, `:584-605`); Replace (or Merge on non-folders) sets `replace` (`transfer.rs:412-417`); `set_aside` rename (`transfer.rs:453`); success → `dispose` (`transfer.rs:424-426`) → `trash_as` (`transfer.rs:465`) else `remove_quietly` (`transfer.rs:469`); failure → rename aside back (`transfer.rs:430-441`)
- **Hazards:** a crash between `set_aside` and `dispose`/restore leaves the user's original only under a hidden temporary name; `is_temporary` (whose doc says lists hide these) is called only by tests, so nothing hides, recovers or reports them; whether a replaced item survives depends on which caller built the `Worker` (`trash: None` in `restore`, `transfer.rs:141`, and `duplicate`, `transfer.rs:52`), not on the prompt
- **Reify:** High — data-loss consequences, several on-disk phases, outcome decided by a far-away constructor argument
- **Suspected bug:** Put Back (and Undo of Move to Trash, `undo.rs:167`) runs `restore` with `trash: None`, so when an item of that name is now at the original location and the person picks Replace (the sheet says "Replacing moves the one there to the Trash", `sheets.rs:322-326`), the newer item is permanently deleted by `remove_quietly` (`transfer.rs:469`) (confidence: high)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#hako-05)
- **Follow-up:** reified as typestate in [#290](https://github.com/bitemyapp/sekka/pull/290). Its suspected bug is fixed, and the mistake no longer compiles.

<a id="hako-06"></a>

### HAKO-06 — Copy-to-partial commit and cross-filesystem move (copy, fsync, delete arrived sources)
- **Where:** `desktop/aizome/apps/hako/src/fsops/transfer.rs:481` (`Worker::place`: `partial`, `arrived`, `sync`); also `desktop/aizome/apps/hako/src/fsops/transfer.rs:271` (`Arrived.leaves`, `.dirs`), `:622` (`copy_any`), `:657` (`copy_file`), `:717` (`copy_dir`), `:563` (`rename_into`)
- **Encoding:** `Arrived` lists of sources safe to delete, `sync = mode == Move`, the hidden `partial` tree on disk, `complete` flags returned up the recursion, `problems`, and a temporary chmod of read-only source folders
- **States:** `TryRename` → `Renamed` | `CrossDevice` → `CopyingPartial` → `PartialComplete` → `RenamedIntoPlace` → `DestSynced` → `SourcesDeleted`; `CopyFailed` → `PartialRemoved`; `DestSyncFailed` → `SourcesKept`
- **Transitions:** rename attempt and EXDEV fall-through (`transfer.rs:483-497`); partial copied (`transfer.rs:509-518`); leaves recorded on each whole copy (`transfer.rs:651-653`, `:681-683`), folders after their sync (`transfer.rs:762-764`); rename into place (`transfer.rs:519-536`); `sync_dir(dest)` gate (`transfer.rs:537-541`); delete leaves then empty folders (`transfer.rs:542-556`); read-only folder chmod and restore around the rename (`transfer.rs:572-575`)
- **Hazards:** a crash mid-copy leaves `.name.hako-part-*` debris that nothing sweeps (`names.rs:164`); a crash between the chmod and its restore leaves a read-only folder writable; leaves are queued for deletion as soon as each file is fsync'd, before their enclosing folder is known to be durable
- **Reify:** High — the module's "a move deletes only what arrived" promise (`fsops/mod.rs:10-15`) rests on implicit bookkeeping across a recursion
- **Suspected bug:** in a cross-filesystem move, when a nested folder's `sync_dir` fails (`transfer.rs:758-760`, e.g. EIO on a flaky USB stick) the folder is reported as "couldn't be written safely", but the files under it were already pushed to `arrived.leaves`; if the top-level partial and its parent still sync, `place` deletes those source files (`transfer.rs:543-551`), emptying a source folder the report says was kept (confidence: medium)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#hako-06)
- **Follow-up:** reified as typestate in [#290](https://github.com/bitemyapp/sekka/pull/290). Its suspected bug is fixed, and the mistake no longer compiles.

<a id="hako-07"></a>

### HAKO-07 — Ask-and-retry loops of the file operations
- **Where:** `desktop/aizome/apps/hako/src/fsops/transfer.rs:342` (`source_meta`); also `transfer.rs:451` (`set_aside`), `:483` and `:519` (`place`), `:608` (`read_names`), `:644` (`copy_link`), `:665` (`copy_file`), `:725` (`copy_dir`), `:100` (`trash_items`); `desktop/aizome/apps/hako/src/fsops/mod.rs:240` (`Env::ask_error`); `transfer.rs:300` (`Worker::ask`)
- **Encoding:** `loop` per operation step; the `ErrorChoice` answer; `Failure.can_retry`; the Asker's remembered answers (HAKO-28)
- **States:** `Attempt` → `Ok` | `Failed` → asked → `Retry`(→ `Attempt`) | `Skip` | `Cancel`; Retry without `can_retry` becomes Skip
- **Transitions:** cancelled control short-circuits to Cancel (`fsops/mod.rs:241-243`); Retry demoted (`fsops/mod.rs:245`); `copy_file` removes the half-written file and subtracts its counted bytes before retrying (`transfer.rs:670-671`); `set_aside` regenerates its random name on AlreadyExists (`transfer.rs:455`)
- **Hazards:** conflict questions (`transfer.rs:401`) are not gated on `is_cancelled` the way error questions are, so a job stopped between its checkpoint and a conflict still puts up a prompt; retries are unbounded by design (the person decides)
- **Reify:** Low — structurally identical local loops

<a id="hako-08"></a>

### HAKO-08 — Trash can item protocol (trashinfo claim, rename, directorysizes, forget)
- **Where:** `desktop/aizome/apps/hako/src/fsops/trash.rs:298` (`Trash::trash_as`); also `trash.rs:383` (`forget`), `trash.rs:401` (`list_can`), `trash.rs:514` (`edit_sizes`); `desktop/aizome/apps/hako/src/fsops/transfer.rs:132` (`restore`), `:182` (`erase`), `:228` (`empty_trash`)
- **Encoding:** on disk per item: `info/<name>.trashinfo` (made with O_EXCL to claim the name), `files/<name>`, a line in `directorysizes`; the `tried` counter for name collisions
- **States:** `Absent` → `InfoClaimed` → `Trashed` (info + file, size line) → `Restored`/`Erased` → `Forgotten`; abnormal `GhostInfo` (info, no file) and `Orphan` (file, no info; listed with unknown origin)
- **Transitions:** claim info (`trash.rs:323`), write+sync (`trash.rs:328-331`), rename item in (`trash.rs:334`), add size line (`trash.rs:338-343`), roll back info on rename failure (`trash.rs:346-353`); restore forgets on Done (`transfer.rs:165`); erase forgets only when fully removed (`transfer.rs:210-212`); Empty Trash drops ghost infos (`transfer.rs:232-247`)
- **Hazards:** a crash after the info claim leaves a ghost info that only Empty Trash removes; `edit_sizes` is an unlocked read-modify-rename, so two jobs (Hako runs jobs concurrently) or another file manager can lose each other's lines; undo `Trash` records look items up by path (`undo.rs:162`), so after the item is gone a different item later trashed under the same name is the one put back
- **Reify:** High — persisted multi-file phases with crash windows; a suspected bug
- **Suspected bug:** Put Back of a folder onto an existing folder with Merge, skipping a conflicting child: `merge` returns Ok, `place_top` reports Done, and `restore` calls `trash.forget(entry)` (`transfer.rs:164-166`) though the skipped child is still in `files/<name>/`; its info is gone, so it lists as an orphan of unknown origin that Put Back refuses ("where it came from isn't known", `transfer.rs:138`) (confidence: high)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#hako-08)
- **Follow-up:** reified as typestate in [#290](https://github.com/bitemyapp/sekka/pull/290). Its suspected bug is fixed, and the mistake no longer compiles.

<a id="hako-09"></a>

### HAKO-09 — Undo/redo history with in-flight inversion jobs
- **Where:** `desktop/aizome/apps/hako/src/fsops/undo.rs:67` (`History.undo`, `.redo`); also `desktop/aizome/apps/hako/src/ui/program.rs:446` (Undo/Redo), `desktop/aizome/apps/hako/src/ui/app.rs:806` (`finished`), `desktop/aizome/apps/hako/src/ui/jobs.rs:282` (`Work::Invert`)
- **Encoding:** two `Vec<Step>` stacks (depth 64 on push), a step popped into a running `Work::Invert` job, and `Done.inverted: Option<bool>` saying how to file the result
- **States:** `Idle(undo, redo)` → `Undoing(step taken, job running)` → `Undone(inverse on redo)` | `Lost(empty inverse)`; same for `Redoing`; new actions clear redo
- **Transitions:** `push` clears redo (`undo.rs:79-88`) from jobs (`app.rs:808`), renames (`app.rs:1047-1050`), new folders (`app.rs:1073-1074`); `take_undo` + job start (`program.rs:447-450`); `take_redo` (`program.rs:453-456`); `undone`/`redone` on Done (`app.rs:809-810`, `undo.rs:107-118`); `invert` returns an empty record on failure (`undo.rs:129`, `:143`)
- **Hazards:** the step leaves the stack when the job starts, not when it succeeds; several undos can run at once on overlapping paths and file their inverses in completion order; an action finishing during an undo clears redo, then the undo's inverse lands on redo after it, breaking "new action forgets redo"; `redone` doesn't apply the 64-step cap
- **Reify:** High — history state split between stacks and running jobs, user-visible ordering
- **Suspected bug:** an undo that fails or is stopped (rename back onto a name taken since, or Stop at a move's conflict prompt) yields an empty inverse that `undone` discards (`undo.rs:108`), so the step is gone from both stacks and the next ⌘Z silently undoes the step before it (confidence: medium)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#hako-09)
- **Follow-up:** reified as typestate in [#290](https://github.com/bitemyapp/sekka/pull/290). Its suspected bug is fixed, and the mistake no longer compiles.

<a id="hako-10"></a>

### HAKO-10 — Thumbnail disk cache validity and failure marks
- **Where:** `desktop/aizome/apps/hako/src/model/thumbs.rs:261` (`make`); also `thumbs.rs:146` (`is_valid`), `thumbs.rs:133` (`save_failure`), `thumbs.rs:168` (`write_png`)
- **Encoding:** PNGs under `$XDG_CACHE_HOME/thumbnails/{normal,large}` and `fail/aizome-hako`, keyed by MD5 of the URI, valid while `Thumb::URI` and `Thumb::MTime` match; temporary `.<name>.<random>` beside them
- **States:** per file `Missing` → `Valid(mtime)` | `FailMarked(mtime)` → `Stale` (file mtime changed) → remade
- **Transitions:** lookup (`thumbs.rs:265`); skip if failed/too big/cached file (`thumbs.rs:268`); mark failure (`thumbs.rs:279`); save large then normal (`thumbs.rs:282-283`); write temp and rename (`thumbs.rs:171-194`)
- **Hazards:** validity is whole-second mtime only, so a rewrite within the same second keeps a stale thumbnail and a decode failure of a file still being written in that second sticks; a crash leaves temp PNGs in the cache folders
- **Reify:** Low — follows the freedesktop spec; cache, not control flow
- **Related:** other programs sharing `~/.cache/thumbnails` by the Thumbnail Managing Standard

<a id="hako-11"></a>

### HAKO-11 — inotify folder watcher and its watch-descriptor map
- **Where:** `desktop/aizome/apps/hako/src/model/watch.rs:27` (`Watcher.fd`, `.stop`, `.watches`, `.thread`); also `desktop/aizome/apps/hako/src/ui/app.rs:560` (`update_watch`), `desktop/aizome/apps/hako/src/ui/program.rs:27` (creation)
- **Encoding:** inotify fd, a stop pipe, `HashMap<wd, PathBuf>` behind a mutex, a polling thread, and the kernel's own watch list (which drops watches by itself on IN_IGNORED)
- **States:** `Starting` → `Watching(dirs)` → `Stopped`; per folder `Unwatched` → `Watched(wd)` → `KernelDropped(stale wd still mapped)` | `Removed`
- **Transitions:** thread spawned (`watch.rs:53-56`); `set` removes watches for dirs no longer wanted (`watch.rs:63-68`) and adds those whose path isn't in the map (`watch.rs:69-76`); events mapped to paths (`watch.rs:132-137`) and sent (`watch.rs:139-141`); stop byte + join on drop (`watch.rs:88-90`)
- **Hazards:** the map is never told when the kernel drops a watch (folder deleted or moved: IN_IGNORED/IN_MOVE_SELF) and `set` dedups by path, so a stale entry blocks re-adding; IN_Q_OVERFLOW (wd -1) is dropped with no rescan; two paths to one folder (a link and its target, both shown) share one wd and the map keeps only the last path, so only one view reloads
- **Reify:** High — the map and the kernel's state drift apart; suspected bug
- **Suspected bug:** a shown folder that's deleted and recreated under the same path (`rm -rf dist && mkdir dist`, a build tool or checkout) stays in the map under its dead wd; the parent's reload keeps the column (same path), `update_watch` passes the same dirs, `set` skips it (`watch.rs:70`), and changes inside the new folder never show until the person navigates away and back (confidence: medium)
- **Reproduction:** Confirmed (unit): [test and output](reproductions.md#hako-11)

<a id="hako-12"></a>

### HAKO-12 — Desks follower thread (compositor IPC)
- **Where:** `desktop/aizome/apps/hako/src/ui/actions.rs:642` (`follow_desks`); also `actions.rs:553` (spawn), `desktop/aizome/apps/hako/src/ui/program.rs:534` (`Msg::Desks`), `desktop/aizome/apps/hako/src/ui/app.rs:284` (`Hako.desk`)
- **Encoding:** a thread with a request `Client` and a second events connection; `Hako.desk: Option<DeskInfo>`
- **States:** `Connecting` → `Fetched` → `Following(events)` → `Ended` (connect failure, event error, or receiver gone)
- **Transitions:** connect and first fetch (`actions.rs:648-651`); subscribe (`actions.rs:652`); refetch on DeskChanged/DesksChanged (`actions.rs:655-660`); exit on error (`actions.rs:663`) or closed stream (`actions.rs:657-659`)
- **Hazards:** no reconnect: a compositor restart or a failed first connect ends the thread for good, leaving the sidebar's Desk place (and `root_for`'s Desk root) stale; failed refetches keep the old value silently
- **Reify:** Low — small background follower
- **Related:** `desktop/aizome/crates/aizome-ipc/src/lib.rs` (client, events); `desktop/aizome/crates/aizome-comp-logic/src/actions.rs` (emits DesksChanged)

<a id="hako-13"></a>

### HAKO-13 — Sheet registry over windows (Info, Quick Look, Ask, Confirm, Message, Unlock)
- **Where:** `desktop/aizome/apps/hako/src/ui/app.rs:276` (`Hako.sheets`); also `app.rs:119` (`enum Sheet`), `app.rs:412` (`open_sheet`)
- **Encoding:** `HashMap<sheet WindowKey, Sheet>`; each variant carries its owner window; the kit keeps the actual sheets inside their parent window (partially explicit: the enum is explicit, its lifecycle isn't)
- **States:** per sheet `Open` → `Closed`; Unlock adds `busy`; Ask is tied to a job (HAKO-28); Quick Look toggles
- **Transitions:** opened from GetInfo (`program.rs:478-483`), QuickLook (`program.rs:502`), AskErase/AskEmptyTrash (`program.rs:372`, `:378`), job questions (`app.rs:782`), messages (`app.rs:847-851`, `actions.rs:420-424`), Unlock (`actions.rs:469-473`); closed by CloseSheet (`program.rs:505-514`), Confirmed (`program.rs:382-383`), Quick Look toggle (`program.rs:488-493`), Escape (`actions.rs:116-118`), answers (`actions.rs:567-570`), job Done (`app.rs:791-794`), unlock success (`actions.rs:386-387`), owner closing (`actions.rs:692-700`, `:704-724`), the window's close button (`program.rs:61-66`)
- **Hazards:** closing a browser window removes only its Unlock and Ask sheets from the map; Info, Quick Look, Confirm and Message entries stay (the kit has destroyed them with the parent); `open_sheet` over a dead parent stores a sheet under `WindowKey::NONE` (`desktop/aizome/crates/aizome-kit/src/cx.rs:408` returns NONE); Confirm holds a snapshot of trash entries executed later against whatever is at those paths then
- **Reify:** Medium — explicit variants, lifecycle spread over about fifteen sites

<a id="hako-14"></a>

### HAKO-14 — File clipboard (cut/copy ownership and cut fading)
- **Where:** `desktop/aizome/apps/hako/src/ui/app.rs:281` (`Hako.clip`); also `app.rs:307` (`cut_paths`), `app.rs:137` (`FileClip.text`), `desktop/aizome/apps/hako/src/ui/clipboard.rs:25` (`offer_files`), `clipboard.rs:37` (`foreign_files`)
- **Encoding:** `clip: Option<FileClip>` (paths, cut, the text Hako set), `cut_paths` faded in views, and the system selection itself; ownership is inferred by comparing the clipboard's text to `clip.text`
- **States:** `Empty` → `Copied(paths)` | `Cut(paths, faded)` → `Pasted` (cut cleared) | `Superseded` (someone else's clipboard, detected only at paste)
- **Transitions:** copy/cut sets text, file offer, fading and `clip` (`app.rs:982-985`); paste matches ownership (`app.rs:992`) else reads foreign files or text (`app.rs:996-1003`); a cut is cleared when the paste job starts (`app.rs:1006-1009`)
- **Hazards:** the cut is forgotten when the move starts, not when it succeeds, so after a cancelled or failed cut-paste a second paste no longer sees Hako's own clip and (since its own selection likely isn't "foreign") falls back to the plain-text paths and copies instead of moving; another program taking the clipboard leaves rows faded until the next copy
- **Reify:** Medium — ownership by text comparison and three fields that can disagree
- **Related:** `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs` (selection offers, `foreign_mimes`)

<a id="hako-15"></a>

### HAKO-15 — Operations window presence
- **Where:** `desktop/aizome/apps/hako/src/ui/app.rs:279` (`Hako.ops_window`); also `desktop/aizome/apps/hako/src/ui/actions.rs:573` (`show_operations`)
- **Encoding:** `ops_window: Option<WindowKey>`, the 600 ms `JobSlow` timer per job, `jobs.is_empty()`
- **States:** `Hidden` → `Shown` → `Hidden` (last job done, or closed by the person)
- **Transitions:** shown on JobSlow if the job still runs (`program.rs:577-581`) or when the last browser window closes with jobs left (`program.rs:73-76`, `:110-112`); closed when the last job finishes (`app.rs:796-800`) or by its close button (`program.rs:67-71`)
- **Hazards:** closing it reparents only questions whose `job.window` is the operations window, which is never set (see HAKO-28); each new slow job reopens it after the person closed it; its size is fixed by the job count when opened
- **Reify:** Low — small, but coupled to question re-parenting

<a id="hako-16"></a>

### HAKO-16 — Volume mount, unlock and two-phase eject tasks
- **Where:** `desktop/aizome/apps/hako/src/ui/app.rs:287` (`Hako.mounting`, `.mount_tasks`, `.unlock_tasks`, `.eject_tasks`); also `desktop/aizome/apps/hako/src/ui/actions.rs:370` (`disks_changed`), `actions.rs:457` (`mount`), `actions.rs:489` (`unlock`), `actions.rs:503` (`eject`), `app.rs:125` (`Sheet::Unlock { pass, error, busy }`)
- **Encoding:** a `HashSet` of busy blocks, three task vectors polled on every `DisksChanged`, the Unlock sheet's `busy`/`error`/`pass`, and `eject_tasks`' `Option<drive>` meaning "unmount done, eject next"
- **States:** per block `Idle` → `Unlocking(sheet busy)` → `Unlocked` → `Mounting` → `Mounted(go there)` | `Failed(message)`; `Unmounting` → `EjectingDrive` → `Idle` | `Failed`
- **Transitions:** unlock prompt (`actions.rs:461-475`); unlock submit sets busy and starts the task (`actions.rs:495-500`); harvest unlock (`actions.rs:375-401`); mount start (`actions.rs:484-486`); harvest mount (`actions.rs:405-428`); eject start (`actions.rs:516-528`); unmount done → drive eject (`actions.rs:439-445`); finish (`actions.rs:446-452`); busy blocks closing the sheet (`program.rs:58`, `:506-508`); owner window closing drops the prompt (`actions.rs:692-700`)
- **Hazards:** `mounting` is the only guard and is cleared in five places; an unlock or mount task that never completes leaves the sheet unclosable and the row "…" forever; eject doesn't wait for jobs writing to the volume
- **Reify:** High — task lifecycles across four collections and a sheet; suspected bug
- **Suspected bug:** ejecting a volume a window is showing sends that window home with `Browser::go` directly (`actions.rs:511`), skipping `start_loads`, `update_watch` and the title; its new Home column stays `Load::Wanted` (blank, no message) because nothing else starts a `Wanted` load (`reload_dir` only refreshes Ready/Failed, `app.rs:645`) until the person navigates (confidence: high)
- **Related:** `desktop/aizome/crates/aizome-sys/src/disks/mod.rs` (UDisks tasks; completions wake `DisksChanged`)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#hako-16)

<a id="hako-17"></a>

### HAKO-17 — Thumbnail request pipeline (queue, asked set, running count)
- **Where:** `desktop/aizome/apps/hako/src/ui/app.rs:294` (`Hako.thumb_wanted`, `.thumb_asked`, `.thumb_running`); also `desktop/aizome/apps/hako/src/ui/fileview.rs:123` (`Thumbs.images`, `.generation`), `app.rs:293` (`Hako.thumbs`)
- **Encoding:** a FIFO of wanted paths, a set of paths ever asked for, an in-flight counter capped at 2, and an `Rc<Thumbs>` of images with a generation for repaint (copy-on-write in `thumbs_mut`)
- **States:** per path `Unasked` → `Queued` → `Running` → `Shown(image)` | `NoThumb`; `Queued` → `Dropped(off screen)`
- **Transitions:** queue unless already asked (`app.rs:676-680`); start up to two (`app.rs:686-694`); result decrements and stores (`program.rs:515-523`, `actions.rs:353-355`); queue pruned to what's on screen (`actions.rs:364`); restart (`actions.rs:365`)
- **Hazards:** `thumb_asked` and `Thumbs.images` only ever grow (memory for every picture seen); an image edited on disk keeps its old thumbnail all session; `thumbs_pending` (`app.rs:866-869`) is what tests wait on
- **Reify:** High — three coupled fields with a set that is never reconciled; suspected bug
- **Suspected bug:** opening a folder of many pictures and leaving before they're all made prunes the rest from `thumb_wanted` (`actions.rs:364`) but leaves them in `thumb_asked`; coming back, `want_thumbs` skips them (`app.rs:677`), so they show generic icons for the rest of the session (confidence: high)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#hako-17)

<a id="hako-18"></a>

### HAKO-18 — Background result caches (previews, folder sizes, free space, Trash count)
- **Where:** `desktop/aizome/apps/hako/src/ui/app.rs:297` (`Hako.previews`); also `app.rs:298` (`sizes`), `app.rs:320` (`space`), `app.rs:311` (`trash_count`)
- **Encoding:** `HashMap<PathBuf, Arc<Preview>>` with a `full` flag and wholesale clearing past 32; `HashMap` of measured sizes; free space with an `Instant` 10 s TTL; a cached Trash count
- **States:** per path `Absent` → `Requested` → `Cached(column | full)`; space `Fresh` → `Expired` → refreshed on read
- **Transitions:** preview request skipped if cached and not full (`app.rs:714`), cache cleared past 32 (`app.rs:717-719`); result kept unless it would replace full with partial (`program.rs:524-529`); sizes stored (`program.rs:530-532`) from `measure` (`actions.rs:341-347`); space refreshed past 10 s (`app.rs:882-889`); Trash count recounted (`app.rs:857-859`)
- **Hazards:** a preview of a file changed on disk is never refreshed until the next wholesale clear; Get Info shows an old size instead of "Counting…" while it counts again; `trash_changed` lists every can on the UI thread at each Trash reload
- **Reify:** Low — caches whose staleness is cosmetic

<a id="hako-19"></a>

### HAKO-19 — Column listing load, reload debounce and refresh reconciliation
- **Where:** `desktop/aizome/apps/hako/src/ui/browser.rs:37` (`enum Load`), `browser.rs:48` (`Column.load`, `.pending`, `.scroll`); also `desktop/aizome/apps/hako/src/ui/app.rs:300` (`Hako.reloads`), `app.rs:302` (`generation`), `browser.rs:175` (`Browser.focus_pending`)
- **Encoding:** partially explicit: `Load::{Wanted, Loading(gen), Ready, Failed}` per column, plus `pending` names to select, `scroll: Option<(row, gen)>`, `focus_pending`, a per-path debounce generation in `reloads`, and the global `generation` counter
- **States:** `Wanted` → `Loading(g)` → `Ready` | `Failed`; `Ready|Failed` → `Wanted` on reload; reload debounce `Idle` → `Armed(g)` (150 ms, trailing) → `Fired`; refresh `PathKept` | `PathClosed`
- **Transitions:** new columns `Wanted` (`browser.rs:70`) via `go`/`open_after` (`browser.rs:235-265`, `:313-316`); `start_loads` → Loading (`app.rs:519-530`); `loaded` → Ready (`app.rs:584-588`) or Failed (`app.rs:590-595`), stale generations dropped (`app.rs:579`), `refresh_after` (`app.rs:601`, `browser.rs:326-347`), focus once read (`app.rs:620-625`); `DirChanged` arms (`program.rs:148-152`), `Reload` fires if latest (`program.rs:153-158`); `reload_dir` re-wants Ready/Failed only (`app.rs:645-648`); `select_after` sets pending (`app.rs:659-668`)
- **Hazards:** no "dirty while loading" flag; `pending` set on a column whose read is already in flight applies to a listing taken before the change and is consumed without a match; the trailing debounce never fires while events keep coming (another program's long `cp -r` into a shown folder); a Failed read clears the selection that a later good read can't restore
- **Reify:** High — enum plus four side fields, transitions in three files; suspected bug
- **Suspected bug:** a change heard while its column is `Loading` is dropped: `reload_dir` skips `Loading` columns (`app.rs:645`) and nothing re-wants them when the stale read lands, so in a big or slow folder a file created after the read passed its slot stays missing until the next change (confidence: medium)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#hako-19)

<a id="hako-20"></a>

### HAKO-20 — Rename session (inline field, new-folder auto-rename)
- **Where:** `desktop/aizome/apps/hako/src/ui/browser.rs:165` (`Browser.rename: Option<Rename>`); also `browser.rs:125` (`Rename.row`, `.path`, `.text`, `.error`), `desktop/aizome/apps/hako/src/ui/app.rs:309` (`Hako.rename_when_loaded`)
- **Encoding:** `rename: Option<Rename>` holding a row index and the path, `error: Option<String>`, and a separate app-level `Option<(window, path)>` meaning "start renaming this new folder once it shows"
- **States:** `Idle` → `Editing(text)` → `Error(text, why)` ↔ `Editing` → `Committed` | `Cancelled` | `Dropped`; auto-rename `Armed(path)` → `Started` | stays armed
- **Transitions:** start (`app.rs:1024`); text edits clear the error (`program.rs:286-290`); commit success (`app.rs:1046-1054`) or error (`app.rs:1039`, `:1062`); cancel (`program.rs:293-298`); commit on select-away and click-away (`actions.rs:65-67`, `:98-101`); dropped by `go` (`browser.rs:237`), select in another column (`browser.rs:294-296`), refresh (`browser.rs:340-342`), search (`actions.rs:316`); new folder arms (`app.rs:1082`), checked on every `Loaded` (`program.rs:146`, `actions.rs:49-60`)
- **Hazards:** `rename.row` is an index that a reload of the column can point at another item, so the field is drawn over the wrong row while commit renames the original path (`view.rs:465-476`); a commit error raised by select-away is then dropped silently by `select`
- **Reify:** High — two cooperating optionals with about a dozen mutation sites; suspected bug
- **Suspected bug:** `rename_when_loaded` is cleared only when the focused column is Ready with exactly that folder selected; if New Folder runs while its column is mid-load (the read predates the mkdir, so `pending` finds nothing), it stays armed, and much later clicking that folder (whose next column's `Loaded` calls `maybe_rename_new`) unexpectedly opens the rename field (confidence: low)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#hako-20)

<a id="hako-21"></a>

### HAKO-21 — Search session (generation, running flag, index cache, worker threads)
- **Where:** `desktop/aizome/apps/hako/src/ui/browser.rs:135` (`Search.generation`, `.running`, `.root`); also `browser.rs:168` (`Browser.search`), `desktop/aizome/apps/hako/src/ui/app.rs:305` (`Hako.search_index`), `desktop/aizome/apps/hako/src/ui/actions.rs:284` (`search`)
- **Encoding:** `Option<Search>` with generation and `running`, an index cache keyed by root with an `Instant` (120 s TTL on use), one unjoined thread per query
- **States:** `Off` → `Searching(g)` → `Results(g)` → `Searching(g+1)` … → `Off`
- **Transitions:** query starts or updates (`actions.rs:300-314`), empty query ends it (`actions.rs:294-298`); results accepted only for the current generation (`program.rs:320-328`); index cached (`program.rs:329-331`); ended by Done/Escape (`program.rs:332-339`) or by `go` (`browser.rs:238`)
- **Hazards:** every keystroke spawns a thread that may walk up to 200,000 entries; the cache fills only when a walk ends, so fast typing runs several walks at once, and none is cancelled when the search ends; the cache is never evicted
- **Reify:** Medium — generation guard is sound; resource hazards only
- **Related:** `desktop/aizome/crates/aizome-search` (`FileIndex`, the palette's shared index cache)

<a id="hako-22"></a>

### HAKO-22 — Drag session (in-window drag, spring-loading, drag-out and its return)
- **Where:** `desktop/aizome/apps/hako/src/ui/browser.rs:147` (`Drag.sources`, `.target`, `.from`, `.spring`); also `browser.rs:169` (`Browser.drag`), `desktop/aizome/apps/hako/src/ui/app.rs:318` (`Hako.outbound`), `desktop/aizome/apps/hako/src/ui/actions.rs:152` (`on_drag`)
- **Encoding:** `Option<Drag>` per window with a spring counter used as a timer generation, `outbound: Option<(DragId, Vec<PathBuf>)>` for a drag handed to the desktop; the view's half lives in HAKO-26
- **States:** `Idle` → `Dragging(target)` → `Dropped` | `Cancelled` | `Outbound(desktop)` → `Ended`; spring `Waiting(n)` → `Opened` | `Stale`
- **Transitions:** start (`actions.rs:154-159`); over updates target and arms a 900 ms spring (`actions.rs:166-178`); spring opens only if counter and target match (`actions.rs:229-233`); drop takes the drag and starts a job (`actions.rs:182-193`); cancel (`actions.rs:195-198`), Escape (`actions.rs:121-123`), refresh closes it (`browser.rs:344`); leave → desktop drag, outbound set (`actions.rs:200-203`, `:220-222`); drops from outside compare against outbound (`actions.rs:248`); cleared on the kit's drag end (`program.rs:607-612`)
- **Hazards:** the app and the view hold separate drag states, so `refresh_after` can end the app's while the view keeps dragging and the drop is ignored silently; `outbound` matches by exact, ordered equality and is cleared only by `drag_ended`, so if that never comes, the same files later dropped from another program are moved rather than copied
- **Reify:** Medium — two-sided state, but each half is guarded
- **Related:** `desktop/aizome/crates/aizome-kit` (`start_drag`, `drag_ended`, Wayland data-device DnD)

<a id="hako-23"></a>

### HAKO-23 — Per-window navigation history (Back/Forward)
- **Where:** `desktop/aizome/apps/hako/src/ui/browser.rs:163` (`Browser.back`, `.forward`)
- **Encoding:** two stacks of `(Loc, selected name)`, capped at 64
- **States:** `(back, forward)` stacks; any new navigation clears forward
- **Transitions:** `push_history` (`browser.rs:268-279`) from `go(..., history=true)` (`app.rs:502-504`); `history_step` pops one stack and pushes the current place on the other (`actions.rs:40-45`)
- **Hazards:** entries refer to places that may be gone (a Failed column on return); a step taken with no current location loses the opposite entry
- **Reify:** Low — standard and local

<a id="hako-24"></a>

### HAKO-24 — Location bar edit mode
- **Where:** `desktop/aizome/apps/hako/src/ui/browser.rs:166` (`Browser.path_edit`, `.path_error`)
- **Encoding:** `path_edit: Option<String>` (Some = editing) and `path_error: Option<String>`
- **States:** `Crumbs` → `Editing(text)` → `Error(text, why)` → `Editing` | `Crumbs`
- **Transitions:** EditPath (`program.rs:239-250`); PathText (`program.rs:251-256`); PathSubmit goes or sets error (`program.rs:257-273`); PathCancel (`program.rs:274-281`)
- **Hazards:** other navigation (sidebar, Back, a drop) doesn't leave edit mode, so stale text stays in the field
- **Reify:** Low — two optionals, four handlers

<a id="hako-25"></a>

### HAKO-25 — Drop-target spot registry
- **Where:** `desktop/aizome/apps/hako/src/ui/dnd.rs:56` (`Spots.map`, `.window`); also `desktop/aizome/apps/hako/src/ui/view.rs:25` (retain), `desktop/aizome/apps/hako/src/ui/fileview.rs:670` (views register), `desktop/aizome/apps/hako/src/ui/sidebar.rs:76`, `sidebar.rs:251` (rows and crumbs register)
- **Encoding:** `Rc<RefCell<BTreeMap<id, Spot>>>` of rects and resolver closures that capture rows and geometry at paint time; ids are column indices, `SIDEBAR+n`, `PATHBAR+n`; pruned only by id range
- **States:** per id `Unregistered` → `Registered(rect, rows at last paint)` → `Stale` (not painted since) → `Pruned`
- **Transitions:** set on paint (`fileview.rs:666-670`, `sidebar.rs:76-83`, `:251-258`); pruned to `id < columns.len()` plus the two 64-id ranges (`view.rs:25-27`); looked up by priority, then by id order (`dnd.rs:82-87`)
- **Hazards:** spots persist after their widget stops painting; at equal priority the lowest id wins; list and icon layouts show only the focused column under spot id `focus` (`view.rs:407-408`, `:451`), and search results use spot 0 with no view destination
- **Reify:** High — validity depends on paint history; suspected bug
- **Suspected bug:** in List or Icons view, after opening a subfolder (columns `[Home, work]`, focus 1) or switching from Columns, spot 0 keeps the parent listing's full-size rect and rows; dragging an item in the current view resolves through stale spot 0 first, so a drop moves the files into whichever parent folder was at that row (or into the parent folder itself) with no highlight shown; in search, drops off a folder row fall through to stale column spots the same way (confidence: high)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#hako-25)

<a id="hako-26"></a>

### HAKO-26 — File view gesture and keyboard state
- **Where:** `desktop/aizome/apps/hako/src/ui/fileview.rs:289` (`ViewState.press`, `.dragging`, `.over`, `.band`, `.thumb`, `.anchor`, `.cursor`, `.typed`, `.typed_at`, `.scroll_gen`, `.bar_until`); also `fileview.rs:310` (`Press.deferred`)
- **Encoding:** widget state with several optionals and a bool for mutually exclusive gestures, pointer capture only for drags, a type-select buffer with a 1 s window, the last applied scroll generation
- **States:** `Idle` → `Pressed(row, deferred)` → `Dragging(over)` → `Dropped` | `Cancelled` | `LeftWindow`; `Idle` → `Banding` → `Idle`; `Idle` → `ThumbDrag` → `Idle`; type-select `Fresh` ↔ `Accumulating`; invalid combinations like `band.is_some() && dragging` are representable
- **Transitions:** press (`fileview.rs:889`); band start (`fileview.rs:850`); thumb grab (`fileview.rs:825`); slop exceeded → dragging, capture, Start (`fileview.rs:936-944`); leaving the window (`fileview.rs:947-950`); over (`fileview.rs:955-957`); release ends thumb/band/press, emits Drop or applies the deferred select (`fileview.rs:963-980`); Escape while dragging (`fileview.rs:477-482`); `end_drag` (`fileview.rs:561-568`); type-select (`fileview.rs:541-545`); scroll-to applied once per generation (`fileview.rs:606-609`)
- **Hazards:** no pointer-cancel or focus-loss handling, and band and thumb drags don't capture the pointer, so a release outside can leave them stuck; `DragEvent::Cancel` is documented as "Escape, or the pointer lost" (`dnd.rs:107`) but only Escape emits it; `anchor`/`cursor` row indices survive reloads that reorder rows, so ⇧-click extends from the wrong item
- **Reify:** Medium — several exclusive gestures as independent optionals, one widget

<a id="hako-27"></a>

### HAKO-27 — Single-instance claim and hand-over socket
- **Where:** `desktop/aizome/apps/hako/src/ui/instance.rs:102` (`claim_at`); also `instance.rs:123` (`serve`), `instance.rs:32` (`enum Claim`), `desktop/aizome/apps/hako/src/ui/mod.rs:38`, `desktop/aizome/apps/hako/src/ui/app.rs:304` (`Hako.instance`)
- **Encoding:** `$XDG_RUNTIME_DIR/aizome-app.hako[.display].sock` and a persistent `.lock` beside it (flock), the listener held until `launched`, a serving thread (partially explicit: `Claim` names the outcome)
- **States:** `Locking` → `HandedOver` (exit) | `StaleSocketRemoved` → `First(listening)` | `Alone`; `First` → `Serving` → `Stopped` (receiver gone)
- **Transitions:** take the lock (`instance.rs:106-107`); hand over (`instance.rs:108-109`); remove stale socket (`instance.rs:112`); bind (`instance.rs:113-119`); listener stored (`ui/mod.rs:40`) and served at launch (`program.rs:38-40`); serve loop ends when the program's stream closes (`instance.rs:138-140`)
- **Hazards:** the socket isn't removed on exit (cleaned by the next launch's failed connect); a hand-over to a wedged instance succeeds and the second launch exits; the serving thread never restarts
- **Reify:** Low — well-guarded and tested for races
- **Related:** `desktop/aizome/apps/hako/src/file_manager.rs` (FileManager1 hands requests over through `hand_over`)

<a id="hako-28"></a>

### HAKO-28 — Job lifecycle with blocking questions
- **Where:** `desktop/aizome/apps/hako/src/ui/jobs.rs:128` (`Job.question`, `.sheet`, `.window`, `.control`, `.answers`, `.progress`); also `jobs.rs:298` (`UiAsker.conflicts`, `.errors`), `jobs.rs:350` (`Throttle.last`), `desktop/aizome/apps/hako/src/ui/app.rs:277` (`Hako.jobs`, `next_job`), `desktop/aizome/apps/hako/src/fsops/mod.rs:155` (`Progress.preparing`)
- **Encoding:** a worker thread per job, an mpsc channel of answers it blocks on, `question: Option<Question>` + `sheet: Option<WindowKey>` + `window` on the UI side, "apply to all" answers memoized in the worker, progress throttled to 50 ms
- **States:** `Started` → `Preparing` → `Running` ↔ `Asking(question, sheet)` ↔ `Paused` → `Done`; `Asking` with `sheet == None` or a dead sheet is representable
- **Transitions:** start (`app.rs:745-762`, `jobs.rs:200-210`); preparing set/cleared (`transfer.rs:314`, `:327`; `archive.rs:146`, `:160`, `:200`, `:219`; `transfer.rs:184`, `:195`); progress (`app.rs:767-771`); ask opens a sheet over a fallback parent (`app.rs:772-786`); answer takes the question and sends (`actions.rs:559-571`, `program.rs:547-555`); "all" memo (`jobs.rs:316-318`, `:337-339`); cancel (`program.rs:570-575`); window close re-parents (`actions.rs:701-724`); Done removes job and sheet (`app.rs:788-801`)
- **Hazards:** the worker waits forever on `answers.recv()` with no timeout, so a question with no reachable sheet hangs the job until Stop; the final progress before Done may be throttled away
- **Reify:** High — question state split across thread, job record, sheet map and window; suspected bug
- **Suspected bug:** if a job's window closes before it asks, the question opens over the operations window or another window but `job.window` stays the dead key (`app.rs:774-786`); `reparent_questions` matches only `j.window == gone` (`actions.rs:702`), so closing that host window destroys the sheet without re-asking, and the job hangs "Waiting for an answer" until stopped from Operations (confidence: medium)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#hako-28)

<a id="hako-29"></a>

### HAKO-29 — Column strip follow-scroll and divider resize
- **Where:** `desktop/aizome/apps/hako/src/ui/strip.rs:40` (`StripState.offset`, `.reveal_gen`, `.follow`, `.resizing`, `.near`, `.bar_until`)
- **Encoding:** widget state; the follow mode is re-armed by a generation from `Browser.reveal`
- **States:** `Following` ↔ `Manual(scrolled)`; `Idle` ↔ `NearDivider` → `Resizing(left)` → `Idle`
- **Transitions:** new reveal generation → follow (`strip.rs:84-87`); following positions the offset (`strip.rs:90-100`); wheel → manual (`strip.rs:179`); divider press captures (`strip.rs:226-227`); moves emit widths (`strip.rs:192-199`); release ends (`strip.rs:233-236`); hover near/leave (`strip.rs:203-206`, `:210-217`)
- **Hazards:** resizing ends only on PointerUp (no cancel on capture loss); widths are emitted every move but saved only with other settings
- **Reify:** Low — one widget's gestures

<a id="hako-30"></a>

### HAKO-30 — (test harness) settle and wait loops for the headless window
- **Where:** `desktop/aizome/apps/hako/tests/common/mod.rs:123` (`settle`); also `tests/common/mod.rs:146` (`wait_for`), `desktop/aizome/apps/hako/tests/goldens.rs:53` (`settle_all`), `tests/goldens.rs:23` (`utc` Once)
- **Encoding:** polling loops with 5 s deadlines over `Load`, `jobs`, `search.running` and `thumbs_pending`, advancing the harness clock
- **States:** `Waiting` → `Settled` (plus one extra pass) | `TimedOut` (returns silently)
- **Transitions:** poll and advance (`tests/common/mod.rs:126-141`); condition met (`tests/common/mod.rs:131-135`); deadline (`tests/common/mod.rs:137-139`); thumbnails (`tests/goldens.rs:56-59`)
- **Hazards:** a timeout returns quietly, so later assertions fail far from the cause; a column stuck in `Wanted` (HAKO-16) costs every test 5 s
- **Reify:** Low — test scaffolding

<a id="hako-31"></a>

### HAKO-31 — (test harness) private D-Bus daemon and FileManager1 service driver
- **Where:** `desktop/aizome/apps/hako/tests/file_manager.rs:64` (`Bus::new`); also `tests/file_manager.rs:111` (`service`), `:154` (`launched`), `:171` (`received`), `:185` (`no_owner`), `:47` (`Process` drop)
- **Encoding:** child processes (dbus-daemon, the service), a record file written by a stand-in `aizome-hako`, a nonblocking stand-in instance socket, 10 s polling deadlines
- **States:** `DaemonStarting` → `AddressRead` → `ServiceSpawned` → `NameOwned` → calls → `Killed` on drop; record `Absent` → `Written` → consumed
- **Transitions:** spawn and read address (`tests/file_manager.rs:87-94`); poll name owner, asserting the service hasn't exited (`tests/file_manager.rs:144-148`); poll record and remove it (`tests/file_manager.rs:157-162`); accept loop (`tests/file_manager.rs:173-182`); wait for release (`tests/file_manager.rs:187-190`); kill and wait (`tests/file_manager.rs:49-52`)
- **Hazards:** none observed
- **Reify:** Low — test scaffolding

## Already explicit

- None in this slice.

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/apps/hako/src/file_manager.rs` | 131 | machines | [HAKO-01](#hako-01) | FileManager1 bus-name service and hand-over/launch |
| `desktop/aizome/apps/hako/src/fsops/archive.rs` | 438 | machines | [HAKO-02](#hako-02) | bsdtar child with SIGSTOP pause, kill, partials |
| `desktop/aizome/apps/hako/src/fsops/control.rs` | 96 | machines | [HAKO-03](#hako-03) | pause/cancel flags and condvar |
| `desktop/aizome/apps/hako/src/fsops/delete.rs` | 213 | none |  | recursive removal and measure walks; only Control checkpoints |
| `desktop/aizome/apps/hako/src/fsops/mod.rs` | 352 | machines | [HAKO-07](#hako-07), [HAKO-28](#hako-28) | ask_error cancel gate; Progress.preparing phase; test TempDir |
| `desktop/aizome/apps/hako/src/fsops/names.rs` | 309 | machines | [HAKO-05](#hako-05), [HAKO-06](#hako-06) | hako-part/hako-aside temporary names; rest pure |
| `desktop/aizome/apps/hako/src/fsops/sys.rs` | 188 | machines | [HAKO-04](#hako-04) | copy_file_range with buffered fallback |
| `desktop/aizome/apps/hako/src/fsops/transfer.rs` | 1161 | machines | [HAKO-05](#hako-05), [HAKO-06](#hako-06), [HAKO-07](#hako-07), [HAKO-08](#hako-08) | placement, replace-aside, cross-fs move, retry loops, restore/erase |
| `desktop/aizome/apps/hako/src/fsops/trash.rs` | 865 | machines | [HAKO-08](#hako-08) | trashinfo claim, rename, directorysizes, orphans |
| `desktop/aizome/apps/hako/src/fsops/undo.rs` | 261 | machines | [HAKO-09](#hako-09) | undo/redo stacks and inversion |
| `desktop/aizome/apps/hako/src/lib.rs` | 36 | none |  | module roots and XDG paths |
| `desktop/aizome/apps/hako/src/main.rs` | 15 | none |  | entry dispatch |
| `desktop/aizome/apps/hako/src/model/entry.rs` | 297 | none |  | pure listing and natural sort |
| `desktop/aizome/apps/hako/src/model/format.rs` | 230 | none |  | pure formatting |
| `desktop/aizome/apps/hako/src/model/mime.rs` | 577 | none |  | compute-once MIME database and memo caches |
| `desktop/aizome/apps/hako/src/model/mod.rs` | 11 | none |  | module list |
| `desktop/aizome/apps/hako/src/model/places.rs` | 182 | none |  | pure parsing of user dirs and mounts |
| `desktop/aizome/apps/hako/src/model/thumbs.rs` | 341 | machines | [HAKO-10](#hako-10) | thumbnail cache validity and fail marks |
| `desktop/aizome/apps/hako/src/model/uri.rs` | 110 | none |  | pure URI encoding and parsing |
| `desktop/aizome/apps/hako/src/model/watch.rs` | 175 | machines | [HAKO-11](#hako-11) | inotify watcher and wd map |
| `desktop/aizome/apps/hako/src/ui/actions.rs` | 787 | machines | [HAKO-12](#hako-12), [HAKO-16](#hako-16), [HAKO-19](#hako-19), [HAKO-20](#hako-20), [HAKO-21](#hako-21), [HAKO-22](#hako-22), [HAKO-23](#hako-23), [HAKO-28](#hako-28) | Desks follower; disks, drag, search, answer/reparent transitions |
| `desktop/aizome/apps/hako/src/ui/app.rs` | 1166 | machines | [HAKO-13](#hako-13), [HAKO-14](#hako-14), [HAKO-15](#hako-15), [HAKO-16](#hako-16), [HAKO-17](#hako-17), [HAKO-18](#hako-18), [HAKO-09](#hako-09), [HAKO-19](#hako-19), [HAKO-20](#hako-20), [HAKO-28](#hako-28) | program state: sheets, clipboard, ops, disks, thumbs, caches, loads, jobs |
| `desktop/aizome/apps/hako/src/ui/browser.rs` | 466 | machines | [HAKO-19](#hako-19), [HAKO-20](#hako-20), [HAKO-21](#hako-21), [HAKO-22](#hako-22), [HAKO-23](#hako-23), [HAKO-24](#hako-24) | column Load, rename, search, drag, history, path edit |
| `desktop/aizome/apps/hako/src/ui/clipboard.rs` | 57 | machines | [HAKO-14](#hako-14) | file offers and foreign reads |
| `desktop/aizome/apps/hako/src/ui/dnd.rs` | 145 | machines | [HAKO-25](#hako-25) | drop spot registry; DragEvent messages |
| `desktop/aizome/apps/hako/src/ui/dragout.rs` | 52 | none |  | pure drag chip drawing |
| `desktop/aizome/apps/hako/src/ui/fileview.rs` | 1353 | machines | [HAKO-26](#hako-26), [HAKO-25](#hako-25), [HAKO-17](#hako-17) | view gestures; registers spots; Thumbs struct |
| `desktop/aizome/apps/hako/src/ui/instance.rs` | 208 | machines | [HAKO-27](#hako-27) | single-instance socket and lock |
| `desktop/aizome/apps/hako/src/ui/jobs.rs` | 411 | machines | [HAKO-28](#hako-28), [HAKO-09](#hako-09) | job record, UiAsker, Throttle; Invert work |
| `desktop/aizome/apps/hako/src/ui/keys.rs` | 50 | none |  | stateless key catcher |
| `desktop/aizome/apps/hako/src/ui/look.rs` | 124 | none |  | icon and dye tables |
| `desktop/aizome/apps/hako/src/ui/menus.rs` | 298 | none |  | menus built from current state |
| `desktop/aizome/apps/hako/src/ui/mod.rs` | 44 | machines | [HAKO-27](#hako-27) | instance claim at startup |
| `desktop/aizome/apps/hako/src/ui/program.rs` | 623 | machines | [HAKO-03](#hako-03), [HAKO-09](#hako-09), [HAKO-11](#hako-11), [HAKO-13](#hako-13), [HAKO-15](#hako-15), [HAKO-16](#hako-16), [HAKO-17](#hako-17), [HAKO-18](#hako-18), [HAKO-19](#hako-19), [HAKO-20](#hako-20), [HAKO-21](#hako-21), [HAKO-22](#hako-22), [HAKO-24](#hako-24), [HAKO-27](#hako-27), [HAKO-28](#hako-28) | update handlers performing transitions |
| `desktop/aizome/apps/hako/src/ui/settings.rs` | 74 | none |  | config load and atomic save |
| `desktop/aizome/apps/hako/src/ui/sheets.rs` | 467 | machines | [HAKO-13](#hako-13), [HAKO-28](#hako-28), [HAKO-16](#hako-16), [HAKO-05](#hako-05) | sheet views; conflict text promises Trash |
| `desktop/aizome/apps/hako/src/ui/sidebar.rs` | 316 | machines | [HAKO-25](#hako-25) | rows and crumbs register drop spots on paint |
| `desktop/aizome/apps/hako/src/ui/strip.rs` | 243 | machines | [HAKO-29](#hako-29) | follow-scroll and divider resize |
| `desktop/aizome/apps/hako/src/ui/view.rs` | 566 | machines | [HAKO-25](#hako-25), [HAKO-19](#hako-19) | spot retain by id; renders Load state |
| `desktop/aizome/apps/hako/tests/common/mod.rs` | 165 | machines | [HAKO-30](#hako-30) | settle and wait_for harness loops |
| `desktop/aizome/apps/hako/tests/file_manager.rs` | 274 | machines | [HAKO-31](#hako-31) | private dbus-daemon and service driver |
| `desktop/aizome/apps/hako/tests/files.rs` | 767 | none |  | tests |
| `desktop/aizome/apps/hako/tests/goldens.rs` | 93 | machines | [HAKO-30](#hako-30) | settle_all thumbnail wait |
| `desktop/aizome/apps/hako/tests/perf.rs` | 132 | none |  | tests (benchmarks) |
| `desktop/aizome/apps/hako/tests/window.rs` | 114 | none |  | tests |
