# HAKO: Hako file manager

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/apps/hako`. Audited at `8e0acbe5`; line numbers refer to that commit.

45 files audited. 31 implicit machines: 11 High, 6 Medium, 14 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

<a id="hako-05"></a>

### HAKO-05 — Transfer placement with conflict prompt and replace-aside protocol
- **Where:** `desktop/aizome/apps/hako/src/fsops/transfer.rs:355` (`Worker::place_top`: `replace`, `aside`); also `desktop/aizome/apps/hako/src/fsops/transfer.rs:449` (`set_aside`), `:463` (`dispose`), `:584` (`merge`), `:276` (`Worker.trash`); `desktop/aizome/apps/hako/src/fsops/names.rs:175` (`aside_name`), `desktop/aizome/apps/hako/src/fsops/names.rs:186` (`is_temporary`)
- **Encoding:** local `replace: bool` and `aside: Option<PathBuf>`, the `Resolution` answer, `Worker.trash: Option<&Trash>`, and on disk the hidden `.name.hako-aside-*` sibling holding the replaced item
- **States:** per item `Stat` → `NoConflict` | `Conflict(asked)` → `Skipped` | `Cancelled` | `KeepBoth(new name)` | `Merging(recursive)` | `Replacing`; replace sub-states `TargetInPlace` → `SetAside` → `NewPlaced` → `Disposed(trashed | deleted)` or `PlaceFailed` → `AsideRestored` | `AsideStranded(problem)`
- **Transitions:** conflict asked (`transfer.rs:401`); KeepBoth renames target (`transfer.rs:404-407`); Merge recurses (`transfer.rs:408-411`, `:584-605`); Replace (or Merge on non-folders) sets `replace` (`transfer.rs:412-417`); `set_aside` rename (`transfer.rs:453`); success → `dispose` (`transfer.rs:424-426`) → `trash_as` (`transfer.rs:465`) else `remove_quietly` (`transfer.rs:469`); failure → rename aside back (`transfer.rs:430-441`)
- **Hazards:** a crash between `set_aside` and `dispose`/restore leaves the user's original only under a hidden temporary name; `is_temporary` (whose doc says lists hide these) is called only by tests, so nothing hides, recovers or reports them; whether a replaced item survives depends on which caller built the `Worker` (`trash: None` in `restore`, `transfer.rs:141`, and `duplicate`, `transfer.rs:52`), not on the prompt
- **Reify:** High — data-loss consequences, several on-disk phases, outcome decided by a far-away constructor argument
- **Suspected bug:** Put Back (and Undo of Move to Trash, `undo.rs:167`) runs `restore` with `trash: None`, so when an item of that name is now at the original location and the person picks Replace (the sheet says "Replacing moves the one there to the Trash", `sheets.rs:322-326`), the newer item is permanently deleted by `remove_quietly` (`transfer.rs:469`) (confidence: high)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#hako-05)
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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#hako-06)
- **Follow-up:** reified as typestate in [#290](https://github.com/bitemyapp/sekka/pull/290). Its suspected bug is fixed, and the mistake no longer compiles.

<a id="hako-08"></a>

### HAKO-08 — Trash can item protocol (trashinfo claim, rename, directorysizes, forget)
- **Where:** `desktop/aizome/apps/hako/src/fsops/trash.rs:298` (`Trash::trash_as`); also `trash.rs:383` (`forget`), `trash.rs:401` (`list_can`), `trash.rs:514` (`edit_sizes`); `desktop/aizome/apps/hako/src/fsops/transfer.rs:132` (`restore`), `:182` (`erase`), `:228` (`empty_trash`)
- **Encoding:** on disk per item: `info/<name>.trashinfo` (made with O_EXCL to claim the name), `files/<name>`, a line in `directorysizes`; the `tried` counter for name collisions
- **States:** `Absent` → `InfoClaimed` → `Trashed` (info + file, size line) → `Restored`/`Erased` → `Forgotten`; abnormal `GhostInfo` (info, no file) and `Orphan` (file, no info; listed with unknown origin)
- **Transitions:** claim info (`trash.rs:323`), write+sync (`trash.rs:328-331`), rename item in (`trash.rs:334`), add size line (`trash.rs:338-343`), roll back info on rename failure (`trash.rs:346-353`); restore forgets on Done (`transfer.rs:165`); erase forgets only when fully removed (`transfer.rs:210-212`); Empty Trash drops ghost infos (`transfer.rs:232-247`)
- **Hazards:** a crash after the info claim leaves a ghost info that only Empty Trash removes; `edit_sizes` is an unlocked read-modify-rename, so two jobs (Hako runs jobs concurrently) or another file manager can lose each other's lines; undo `Trash` records look items up by path (`undo.rs:162`), so after the item is gone a different item later trashed under the same name is the one put back
- **Reify:** High — persisted multi-file phases with crash windows; a suspected bug
- **Suspected bug:** Put Back of a folder onto an existing folder with Merge, skipping a conflicting child: `merge` returns Ok, `place_top` reports Done, and `restore` calls `trash.forget(entry)` (`transfer.rs:164-166`) though the skipped child is still in `files/<name>/`; its info is gone, so it lists as an orphan of unknown origin that Put Back refuses ("where it came from isn't known", `transfer.rs:138`) (confidence: high)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#hako-08)
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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#hako-09)
- **Follow-up:** reified as typestate in [#290](https://github.com/bitemyapp/sekka/pull/290). Its suspected bug is fixed, and the mistake no longer compiles.

<a id="hako-11"></a>

### HAKO-11 — inotify folder watcher and its watch-descriptor map
- **Where:** `desktop/aizome/apps/hako/src/model/watch.rs:27` (`Watcher.fd`, `.stop`, `.watches`, `.thread`); also `desktop/aizome/apps/hako/src/ui/app.rs:560` (`update_watch`), `desktop/aizome/apps/hako/src/ui/program.rs:27` (creation)
- **Encoding:** inotify fd, a stop pipe, `HashMap<wd, PathBuf>` behind a mutex, a polling thread, and the kernel's own watch list (which drops watches by itself on IN_IGNORED)
- **States:** `Starting` → `Watching(dirs)` → `Stopped`; per folder `Unwatched` → `Watched(wd)` → `KernelDropped(stale wd still mapped)` | `Removed`
- **Transitions:** thread spawned (`watch.rs:53-56`); `set` removes watches for dirs no longer wanted (`watch.rs:63-68`) and adds those whose path isn't in the map (`watch.rs:69-76`); events mapped to paths (`watch.rs:132-137`) and sent (`watch.rs:139-141`); stop byte + join on drop (`watch.rs:88-90`)
- **Hazards:** the map is never told when the kernel drops a watch (folder deleted or moved: IN_IGNORED/IN_MOVE_SELF) and `set` dedups by path, so a stale entry blocks re-adding; IN_Q_OVERFLOW (wd -1) is dropped with no rescan; two paths to one folder (a link and its target, both shown) share one wd and the map keeps only the last path, so only one view reloads
- **Reify:** High — the map and the kernel's state drift apart; suspected bug
- **Suspected bug:** a shown folder that's deleted and recreated under the same path (`rm -rf dist && mkdir dist`, a build tool or checkout) stays in the map under its dead wd; the parent's reload keeps the column (same path), `update_watch` passes the same dirs, `set` skips it (`watch.rs:70`), and changes inside the new folder never show until the person navigates away and back (confidence: medium)
- **Reproduction:** Confirmed (unit): [summary](reproductions.md#hako-11)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#hako-16)

<a id="hako-17"></a>

### HAKO-17 — Thumbnail request pipeline (queue, asked set, running count)
- **Where:** `desktop/aizome/apps/hako/src/ui/app.rs:294` (`Hako.thumb_wanted`, `.thumb_asked`, `.thumb_running`); also `desktop/aizome/apps/hako/src/ui/fileview.rs:123` (`Thumbs.images`, `.generation`), `app.rs:293` (`Hako.thumbs`)
- **Encoding:** a FIFO of wanted paths, a set of paths ever asked for, an in-flight counter capped at 2, and an `Rc<Thumbs>` of images with a generation for repaint (copy-on-write in `thumbs_mut`)
- **States:** per path `Unasked` → `Queued` → `Running` → `Shown(image)` | `NoThumb`; `Queued` → `Dropped(off screen)`
- **Transitions:** queue unless already asked (`app.rs:676-680`); start up to two (`app.rs:686-694`); result decrements and stores (`program.rs:515-523`, `actions.rs:353-355`); queue pruned to what's on screen (`actions.rs:364`); restart (`actions.rs:365`)
- **Hazards:** `thumb_asked` and `Thumbs.images` only ever grow (memory for every picture seen); an image edited on disk keeps its old thumbnail all session; `thumbs_pending` (`app.rs:866-869`) is what tests wait on
- **Reify:** High — three coupled fields with a set that is never reconciled; suspected bug
- **Suspected bug:** opening a folder of many pictures and leaving before they're all made prunes the rest from `thumb_wanted` (`actions.rs:364`) but leaves them in `thumb_asked`; coming back, `want_thumbs` skips them (`app.rs:677`), so they show generic icons for the rest of the session (confidence: high)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#hako-17)

<a id="hako-19"></a>

### HAKO-19 — Column listing load, reload debounce and refresh reconciliation
- **Where:** `desktop/aizome/apps/hako/src/ui/browser.rs:37` (`enum Load`), `browser.rs:48` (`Column.load`, `.pending`, `.scroll`); also `desktop/aizome/apps/hako/src/ui/app.rs:300` (`Hako.reloads`), `app.rs:302` (`generation`), `browser.rs:175` (`Browser.focus_pending`)
- **Encoding:** partially explicit: `Load::{Wanted, Loading(gen), Ready, Failed}` per column, plus `pending` names to select, `scroll: Option<(row, gen)>`, `focus_pending`, a per-path debounce generation in `reloads`, and the global `generation` counter
- **States:** `Wanted` → `Loading(g)` → `Ready` | `Failed`; `Ready|Failed` → `Wanted` on reload; reload debounce `Idle` → `Armed(g)` (150 ms, trailing) → `Fired`; refresh `PathKept` | `PathClosed`
- **Transitions:** new columns `Wanted` (`browser.rs:70`) via `go`/`open_after` (`browser.rs:235-265`, `:313-316`); `start_loads` → Loading (`app.rs:519-530`); `loaded` → Ready (`app.rs:584-588`) or Failed (`app.rs:590-595`), stale generations dropped (`app.rs:579`), `refresh_after` (`app.rs:601`, `browser.rs:326-347`), focus once read (`app.rs:620-625`); `DirChanged` arms (`program.rs:148-152`), `Reload` fires if latest (`program.rs:153-158`); `reload_dir` re-wants Ready/Failed only (`app.rs:645-648`); `select_after` sets pending (`app.rs:659-668`)
- **Hazards:** no "dirty while loading" flag; `pending` set on a column whose read is already in flight applies to a listing taken before the change and is consumed without a match; the trailing debounce never fires while events keep coming (another program's long `cp -r` into a shown folder); a Failed read clears the selection that a later good read can't restore
- **Reify:** High — enum plus four side fields, transitions in three files; suspected bug
- **Suspected bug:** a change heard while its column is `Loading` is dropped: `reload_dir` skips `Loading` columns (`app.rs:645`) and nothing re-wants them when the stale read lands, so in a big or slow folder a file created after the read passed its slot stays missing until the next change (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#hako-19)

<a id="hako-20"></a>

### HAKO-20 — Rename session (inline field, new-folder auto-rename)
- **Where:** `desktop/aizome/apps/hako/src/ui/browser.rs:165` (`Browser.rename: Option<Rename>`); also `browser.rs:125` (`Rename.row`, `.path`, `.text`, `.error`), `desktop/aizome/apps/hako/src/ui/app.rs:309` (`Hako.rename_when_loaded`)
- **Encoding:** `rename: Option<Rename>` holding a row index and the path, `error: Option<String>`, and a separate app-level `Option<(window, path)>` meaning "start renaming this new folder once it shows"
- **States:** `Idle` → `Editing(text)` → `Error(text, why)` ↔ `Editing` → `Committed` | `Cancelled` | `Dropped`; auto-rename `Armed(path)` → `Started` | stays armed
- **Transitions:** start (`app.rs:1024`); text edits clear the error (`program.rs:286-290`); commit success (`app.rs:1046-1054`) or error (`app.rs:1039`, `:1062`); cancel (`program.rs:293-298`); commit on select-away and click-away (`actions.rs:65-67`, `:98-101`); dropped by `go` (`browser.rs:237`), select in another column (`browser.rs:294-296`), refresh (`browser.rs:340-342`), search (`actions.rs:316`); new folder arms (`app.rs:1082`), checked on every `Loaded` (`program.rs:146`, `actions.rs:49-60`)
- **Hazards:** `rename.row` is an index that a reload of the column can point at another item, so the field is drawn over the wrong row while commit renames the original path (`view.rs:465-476`); a commit error raised by select-away is then dropped silently by `select`
- **Reify:** High — two cooperating optionals with about a dozen mutation sites; suspected bug
- **Suspected bug:** `rename_when_loaded` is cleared only when the focused column is Ready with exactly that folder selected; if New Folder runs while its column is mid-load (the read predates the mkdir, so `pending` finds nothing), it stays armed, and much later clicking that folder (whose next column's `Loaded` calls `maybe_rename_new`) unexpectedly opens the rename field (confidence: low)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#hako-20)

<a id="hako-25"></a>

### HAKO-25 — Drop-target spot registry
- **Where:** `desktop/aizome/apps/hako/src/ui/dnd.rs:56` (`Spots.map`, `.window`); also `desktop/aizome/apps/hako/src/ui/view.rs:25` (retain), `desktop/aizome/apps/hako/src/ui/fileview.rs:670` (views register), `desktop/aizome/apps/hako/src/ui/sidebar.rs:76`, `sidebar.rs:251` (rows and crumbs register)
- **Encoding:** `Rc<RefCell<BTreeMap<id, Spot>>>` of rects and resolver closures that capture rows and geometry at paint time; ids are column indices, `SIDEBAR+n`, `PATHBAR+n`; pruned only by id range
- **States:** per id `Unregistered` → `Registered(rect, rows at last paint)` → `Stale` (not painted since) → `Pruned`
- **Transitions:** set on paint (`fileview.rs:666-670`, `sidebar.rs:76-83`, `:251-258`); pruned to `id < columns.len()` plus the two 64-id ranges (`view.rs:25-27`); looked up by priority, then by id order (`dnd.rs:82-87`)
- **Hazards:** spots persist after their widget stops painting; at equal priority the lowest id wins; list and icon layouts show only the focused column under spot id `focus` (`view.rs:407-408`, `:451`), and search results use spot 0 with no view destination
- **Reify:** High — validity depends on paint history; suspected bug
- **Suspected bug:** in List or Icons view, after opening a subfolder (columns `[Home, work]`, focus 1) or switching from Columns, spot 0 keeps the parent listing's full-size rect and rows; dragging an item in the current view resolves through stale spot 0 first, so a drop moves the files into whichever parent folder was at that row (or into the parent folder itself) with no highlight shown; in search, drops off a folder row fall through to stale column spots the same way (confidence: high)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#hako-25)

<a id="hako-28"></a>

### HAKO-28 — Job lifecycle with blocking questions
- **Where:** `desktop/aizome/apps/hako/src/ui/jobs.rs:128` (`Job.question`, `.sheet`, `.window`, `.control`, `.answers`, `.progress`); also `jobs.rs:298` (`UiAsker.conflicts`, `.errors`), `jobs.rs:350` (`Throttle.last`), `desktop/aizome/apps/hako/src/ui/app.rs:277` (`Hako.jobs`, `next_job`), `desktop/aizome/apps/hako/src/fsops/mod.rs:155` (`Progress.preparing`)
- **Encoding:** a worker thread per job, an mpsc channel of answers it blocks on, `question: Option<Question>` + `sheet: Option<WindowKey>` + `window` on the UI side, "apply to all" answers memoized in the worker, progress throttled to 50 ms
- **States:** `Started` → `Preparing` → `Running` ↔ `Asking(question, sheet)` ↔ `Paused` → `Done`; `Asking` with `sheet == None` or a dead sheet is representable
- **Transitions:** start (`app.rs:745-762`, `jobs.rs:200-210`); preparing set/cleared (`transfer.rs:314`, `:327`; `archive.rs:146`, `:160`, `:200`, `:219`; `transfer.rs:184`, `:195`); progress (`app.rs:767-771`); ask opens a sheet over a fallback parent (`app.rs:772-786`); answer takes the question and sends (`actions.rs:559-571`, `program.rs:547-555`); "all" memo (`jobs.rs:316-318`, `:337-339`); cancel (`program.rs:570-575`); window close re-parents (`actions.rs:701-724`); Done removes job and sheet (`app.rs:788-801`)
- **Hazards:** the worker waits forever on `answers.recv()` with no timeout, so a question with no reachable sheet hangs the job until Stop; the final progress before Done may be throttled away
- **Reify:** High — question state split across thread, job record, sheet map and window; suspected bug
- **Suspected bug:** if a job's window closes before it asks, the question opens over the operations window or another window but `job.window` stays the dead key (`app.rs:774-786`); `reparent_questions` matches only `j.window == gone` (`actions.rs:702`), so closing that host window destroys the sheet without re-asking, and the job hangs "Waiting for an answer" until stopped from Operations (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#hako-28)
