# TSH: Tansu and Shiori

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/apps/{tansu,shiori}`. Audited at `8e0acbe5`; line numbers refer to that commit.

42 files audited. 32 implicit machines: 8 High, 8 Medium, 16 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

<a id="tsh-02"></a>

### TSH-02 — Shiori document open and worker-restart lifecycle
- **Where:** `desktop/aizome/apps/shiori/src/app.rs:128` (`enum Load`), `desktop/aizome/apps/shiori/src/app.rs:261` (`Doc.serial`), `:263` (`.password`), `:264` (`.engine`), `:265` (`.load`), `:281` (`.pending`), `:729` (`Shiori.serial`)
- **Encoding:** partially explicit: `Load` (Opening, Locked{tried}, Failed, Open) plus `engine: Option<Engine>` (None means failed, locked, or worker gone), `serial` to route and drop worker events, `password`, `trouble`/`trouble_until`, and the waiters `worker_gone` must clear (`sent`, `asked`, `copying`, `find.stopped`)
- **States:** `Opening(engine)` → `Open(engine)` | `Locked{tried}` | `Failed`; `Open` → `Open+Restarting` (crash, gave_up=false) → `Open` (Opened again) | `Open+WorkerGone` (engine None, lasting trouble); `Locked` → `Opening` (password given); representable: `Load::Failed` with `find`/`copying` still waiting (the `:1176-1183` path skips `worker_gone`)
- **Transitions:** `open_paths` Opening + engine (`desktop/aizome/apps/shiori/src/app.rs:896`, `:934`); first Opened → Open (`:1323`) or Failed "no pages" (`:1304-1305`); Opened again → resend find/copy/tiles (`:1284-1298`) or worker gone if it found no pages (`:1275-1282`); OpenFailed while Open (non-Worker) → `worker_gone` + `not_reopened` (`:1167-1174`); OpenFailed Password → Locked + password sheet (`:1178-1181`); other OpenFailed → Failed (`:1183`); Failed{gave_up:false} → passing notice (`:1243-1245`); Failed{gave_up:true} → `worker_gone` then Failed, `not_reopened` or lasting notice (`:1246-1260`); password OK → Opening + `restart` (`:1700-1703`, `:955-964`); Reload when not Open → Opening + `restart` (`:2085-2086`); window closed → engine dropped (`:1731`)
- **Hazards:** transitions spread across `worker`, `opened`, `reloaded`, `sheet_ok`, `Msg::Reload`; `load` and `engine` are written separately; the gave_up path leaves `trouble_until` at `ticks + 7` after clearing `trouble` (`desktop/aizome/apps/shiori/src/app.rs:1245-1254`), which is harmless; a document whose worker is gone recovers only through Reload or a file change (TSH-03)
- **Reify:** High — six-plus fields across five handlers, with representable inconsistent combinations
- **Suspected bug:** after a crash, the supervisor's respawn fails (fork EAGAIN/ENOMEM, `desktop/aizome/apps/shiori/src/engine.rs:140-145`) and sends `OpenFailed(Worker)`. Because `desktop/aizome/apps/shiori/src/app.rs:1167` excludes `Worker` errors, the document that was showing becomes `Load::Failed` (`:1183`): "can't open" covers its pages, find/copy stay pending, and Reload reopens it as a new document at the top, because `pending` was already taken (`:1326-1329`). (confidence: low)
- **Related:** supervisor `desktop/aizome/apps/shiori/src/engine.rs` (TSH-14), worker `desktop/aizome/apps/shiori/src/worker.rs` (TSH-21)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#tsh-02)

<a id="tsh-03"></a>

### TSH-03 — Changed-file watch and reload in place
- **Where:** `desktop/aizome/apps/shiori/src/app.rs:267` (`Doc.stamp`), `:270` (`.showing`), `:273` (`.reload: Option<(u64, Engine)>`), `:37` (`WATCH`)
- **Encoding:** (mtime, size) last looked at, the stamp of the version on screen, a second worker opening the newer file, polled each 1.5 s tick; `trouble` explains a kept old version
- **States:** `Showing` → `Changed` → `Reloading(second engine)` → `Swapped` (new engine, caches cleared) | `KeptOld` ("can't be read yet" or "couldn't open")
- **Transitions:** `watch` compares/updates stamp (`desktop/aizome/apps/shiori/src/app.rs:1741-1751`) → `reload` (`:1755`, `:970-977`); Msg::Reload refreshes stamp then reloads (`:2081-2083`); events routed by reload serial (`:1124-1127`); Opened with pages → take reload, swap engine/serial (`:987-993`), clear tiles/texts/selection/copying (`:1308-1318`), `showing = stamp` (`:1322`); Opened empty / OpenFailed → drop reload + `not_reopened` (`:998-1001`); Failed gave_up → same (`:1002-1005`); wording chosen by `stamp(path) != showing` (`:362-374`)
- **Hazards:** `watch` skips a document with a reload in flight (`desktop/aizome/apps/shiori/src/app.rs:1741`), but Msg::Reload doesn't, so a second Reload replaces the first engine; `stamp` isn't updated during a reload, so a change made meanwhile is seen only afterwards (one extra reload); on swap `expanded` (positional outline ids), `thumbs.asked` and `hover` survive although they describe the old document
- **Reify:** High — six fields across watch/reload/reloaded/opened/Msg::Reload, with two engines alive at once
- **Related:** supervisor `desktop/aizome/apps/shiori/src/engine.rs` (TSH-14)

<a id="tsh-04"></a>

### TSH-04 — Tile/text request coalescing and drawing caches
- **Where:** `desktop/aizome/apps/shiori/src/app.rs:287` (`Doc.sent`), `:293` (`.asked`), `:290` (`.broken`), `:282` (`.tiles`), `:285` (`.standins`); `struct Tiles` `desktop/aizome/apps/shiori/src/app.rs:195`
- **Encoding:** `sent` is the last Draw list not yet answered, which prevents resending; `asked` holds Text/Links jobs in flight; `broken` records failed sizes per page (two strikes); `Tiles` is an LRU with a clock and byte budget; `standins` holds partly made stand-ins
- **States:** per tile `Missing` → `Requested` → `Drawn` → `Evicted` | `Broken(size)`; per page's text and links `Missing` → `Asked` → `Have` → `Pruned`
- **Transitions:** `request` sends Draw only when the wanted set differs from `sent` (`desktop/aizome/apps/shiori/src/app.rs:1108-1112`) and asks Text/Links (`:1113-1120`); Drawn → `sent.retain` + insert with keep set (`:1133`, `:1154`); DrawFailed (`:1159-1160`); Failed clears `sent`, marks broken or empty (`:1222-1242`); Opened again / reload swap / restart clear `sent`+`asked` (`:1287-1288`, `:991-992`, `:962-963`); `worker_gone` (`:380-381`); scale change clears every document's `sent` (`:1835-1837`); text pruning (`:435-442`); LRU eviction (`:216-227`); stand-ins kept or grown (`:567-586`)
- **Hazards:** `sent` is cleared in six places and must mirror the worker's replace-on-Draw queue (`desktop/aizome/apps/shiori/src/worker.rs:83`); a Draw sent while no worker stdin exists (between crash and respawn) is recorded in `sent` but dropped (`desktop/aizome/apps/shiori/src/engine.rs:264`), recovered only because Opened-again clears it; any crash during a draw marks that page broken at that size (`desktop/aizome/apps/shiori/src/app.rs:1226`) whatever caused it; `idle()` (`:854-864`) waits on all of these
- **Reify:** High — five fields, about ten mutation sites, mirrors the worker's queue implicitly

<a id="tsh-08"></a>

### TSH-08 — Copy waiting for pages' text
- **Where:** `desktop/aizome/apps/shiori/src/app.rs:111` (`Copying.parts`, `.missing`), `:303` (`Doc.copying`)
- **Encoding:** `Option<Copying>` with the set of pages still missing; requests tracked in `Doc.asked`
- **States:** `None` → `Waiting(missing)` → `Copied` (clipboard set) | `Dropped` (worker gone / reload)
- **Transitions:** `copy` copies at once or waits and asks (`desktop/aizome/apps/shiori/src/app.rs:1588-1594`, `:423-432`); `text_came` takes parts and completes (`:1601-1611`); failed Text → empty text → `text_came` (`:1228-1230`, `:1262-1264`); Opened again re-asks (`:1296`); `worker_gone`/reload swap drop it (`:382`, `:1316`); toolbar "Reading N pages to copy…" (`desktop/aizome/apps/shiori/src/ui.rs:110-113`)
- **Hazards:** a new Copy silently replaces a waiting one; no timeout; `copy` never checks for an engine
- **Suspected bug:** the worker gives up and `engine` becomes None, but Load stays Open. The person then selects across pages whose text isn't loaded (pruned, or never read) and copies: `copy` stores the `Copying` (`desktop/aizome/apps/shiori/src/app.rs:1593`), but `ask_copy_text` returns without an engine (`:424`). Nothing is copied, nothing says why (the lasting notice occupies the status), and `idle()` stays false until a reload swap. (confidence: medium)
- **Reify:** High — suspected bug; a waiter with no exit when its producer is gone
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#tsh-08)

<a id="tsh-09"></a>

### TSH-09 — Find session
- **Where:** `desktop/aizome/apps/shiori/src/app.rs:140` (`Find.query` `:141`, `.serial` `:144`, `.results` `:146`, `.pages` `:148`, `.current` `:150`, `.jump` `:152`, `.typed` `:154`, `.stopped` `:156`), `Doc.find` `desktop/aizome/apps/shiori/src/app.rs:305`
- **Encoding:** `Option<Find>`: `typed` is the debounce generation, `serial` the search generation the worker echoes, `jump` means go to the first hit, `stopped` that the worker gave up; `done()` is derived
- **States:** `Closed` → `Open(empty)` → `Typing(typed)` → `Searching(serial)` → `Done` | `Stopped`, with a `current` hit cursor
- **Transitions:** created by `get_or_insert_with` (`desktop/aizome/apps/shiori/src/app.rs:1628`, `:1997`, `:2008`); FindQuery bumps typed, arms 200 ms FindRun (`:2001-2004`); `run_find` (`:1642-1661`); Found (`:1205-1212`); failed page (`:1235-1239`); `worker_gone` sets stopped (`:383-385`); Opened again resends remaining pages with the same serial (`:1289-1295`); reload reruns (`:1348-1353`); step (`:1667-1670`); `close_find` drops it and sends StopFind (`:1617-1620`); worker replaces or stops the search (`desktop/aizome/apps/shiori/src/worker.rs:94-97`)
- **Hazards:** `serial` and `typed` live inside `Find`, so they restart from 0 every time find reopens; `current` isn't rechecked when a page's results are overwritten; `done()` counts failed pages as searched
- **Suspected bug:** search "foo", press Escape (`find = None`, StopFind), then ⌘F with "bar" selected, or type it, while the worker is still on its current page. The new `Find` starts at serial 0, and `run_find` makes it 1 again (`desktop/aizome/apps/shiori/src/app.rs:1643`). The worker's late `Found{serial: 1}` for "foo" therefore passes the filter at `:1205`, is stored as a "bar" result and, with `jump` set, moves the view to a "foo" hit (`:1208-1211`). `current` can then point past that page's real hits once "bar" overwrites the entry. (confidence: medium)
- **Reify:** High — nine fields, eleven handlers, plus a suspected bug
- **Related:** worker search queue `desktop/aizome/apps/shiori/src/worker.rs:53` (TSH-21)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#tsh-09)

<a id="tsh-14"></a>

### TSH-14 — Document worker supervisor (spawn, watchdog, restart, give up, close)
- **Where:** `desktop/aizome/apps/shiori/src/engine.rs:55` (`Shared.stdin` `:56`, `.child` `:57`, `.busy` `:59`, `.stopped` `:61`, `.closed` `:63`); locals `failures` `desktop/aizome/apps/shiori/src/engine.rs:135`, `pages` `:214`
- **Encoding:** three threads (supervise, forward, watch) share mutexes and atomics; a 60 s window allows four failures; `Drop` sets `closed` and kills
- **States:** `Spawning` → `Opening` (Open frame written, child kept) → `Running` ⇄ `Busy(job, since)` → `Dead(Crashed|TooLong)` → `Spawning` | `GaveUp`; `SpawnFailed` (OpenFailed(Worker)); `Closed` from any
- **Transitions:** spawn (`desktop/aizome/apps/shiori/src/engine.rs:140-146`); Open frame + publish stdin (`:150-152`); keep-or-closed handshake (`:153`, `:189-200`); `stopped` reset (`:156`); Working → busy (`:221-223`); replies clear busy (`:230`, `:239`, `:245`, `:249`); implausible drawing stops reading (`:225-228`); exit cleanup (`:161-165`); classify and give up (`:169-181`); watchdog kill (`:276-286`); forward drops stdin on a write error (`:264-268`); Drop (`:118-127`)
- **Hazards:** requests sent while `stdin` is None are dropped silently (`desktop/aizome/apps/shiori/src/engine.rs:264`); the watchdog decides `late` under the busy lock (`:276-281`) but kills whatever `child` holds when it next locks it (`:284`), so if the late worker dies on its own and a new one is kept in between, the newcomer is killed while opening and `job == Open` gives up (`:176`); `stopped` resets only after the next child is kept (`:156`); after giving up, the forward and watch threads live until the Engine drops; `busy` assumes one job at a time
- **Reify:** High — a cross-thread lifecycle in five shared fields that already needs a lock-order handshake (`keep`)
- **Related:** app side `desktop/aizome/apps/shiori/src/app.rs` (TSH-02), worker side `desktop/aizome/apps/shiori/src/worker.rs` (TSH-21)

<a id="tsh-25"></a>

### TSH-25 — Tansu sheet lifecycle (forms holding segment indexes)
- **Where:** `desktop/aizome/apps/tansu/src/app.rs:184` (`Tansu.sheet: Option<Sheet>`), `struct Sheet` `desktop/aizome/apps/tansu/src/app.rs:96`, `enum SheetKind` `:82`; captured indexes `FormatForm.seg` `:33`, `CreateForm.seg/.offset/.max` `:44-46`, `ResizeForm.seg..max` `:63-68`, `Label/Flags/Unlock{seg}` `:86-89`, `Smart{task}` `:93`
- **Encoding:** partially explicit: one `Option<Sheet>` with a kind enum, but each form holds `seg`, an index into `self.layout.segs` taken when it opened; `error` resets on every sheet message
- **States:** `None` → `Format | Create | Resize | Label | Flags | Table | Unlock | Attach | Apply | Smart` → `None`; `Apply → Running` (TSH-26); any → another (replaced)
- **Transitions:** `open_sheet` closes any current sheet first (`desktop/aizome/apps/tansu/src/app.rs:479-488`); opened by actions (`:634`, `:654`, `:670`, `:687`, `:690`, `:694`), NewTable (`:1144`), AttachImage (`:1149`), ShowSmart (`:1186`), `open`/Launched (`:1104`, `:1232-1235`); error reset (`:704`); submits queue against the current layout by index (`:732`, `:789`, `:821`, `:828`, `:850`) or act on `layout.segs.get(seg)` (`:871`); close (`:490-494`, `:1191-1199`); SMART result (`:951-959`); views read the segment live (`desktop/aizome/apps/tansu/src/ui/sheets.rs:137`, `:265`, `:319`, `:344`, `:412`)
- **Hazards:** the kit blocks menus while a sheet is modal, but not `Msg::Changed` (TSH-23) or `Msg::Launched`/`Program::open`; a second launch replaces a half-filled form (passphrases included) without asking; Create re-checks its offset (`desktop/aizome/apps/tansu/src/plan.rs:441`), but the other forms never check that the segment is still the one they opened for
- **Suspected bug:** open Format… on a USB stick's partition, then unplug the stick (or let another tool repartition the disk). `refresh` falls back to the first drive (`desktop/aizome/apps/tansu/src/app.rs:308-311`), so the open sheet's `seg` now indexes that drive's layout. The sheet's title quietly changes to the other partition (`desktop/aizome/apps/tansu/src/ui/sheets.rs:137-140`), and Add to Pending queues a format of it (`desktop/aizome/apps/tansu/src/app.rs:732`); if the fallback is another removable disk, Apply asks for no typed name. Resize, Label and Flags behave the same, and Unlock tries the passphrase on another volume (`:871-873`). (confidence: medium)
- **Reify:** High — index-based identity across a mutable layout, with data-loss consequences
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#tsh-25)

<a id="tsh-26"></a>

### TSH-26 — Tansu apply session (Apply sheet → Run → Running sheet)
- **Where:** `desktop/aizome/apps/tansu/src/app.rs:185` (`Tansu.run: Option<Run>`), `:186` (`.run_timer`), `SheetKind::Running` `:92`, `:182` (`.history`)
- **Encoding:** `run` plus a 250 ms Tick timer plus the Running sheet; `run.is_some()` and `run.is_running()` differ: Some and finished means "waiting for the person to dismiss the sheet"
- **States:** `Idle` → `Confirming` (Apply sheet, typed names) → `Running` (run, timer, Running sheet) → `Finished` (run Some, outcome set, timer cancelled) → `Idle`
- **Transitions:** Msg::Apply (`desktop/aizome/apps/tansu/src/app.rs:1217-1222`); Apply Submit checks problems and names, agrees, switches to Running, starts the timer (`:913-928`); `advance_run` on every update while Some (`:1241-1243`, `:545-571`): keep undone ops, clear history, cancel timer, notice, force refresh; StopRun (`:1224-1227`); Done/Cancel clear `run` only when finished and the sheet is Running (`:930-935`, `:1192-1197`); window close refused while running (`:1295-1305`)
- **Hazards:** the completion bookkeeping (retain, `history.clear`, notice, `seen = u64::MAX` full re-read) repeats on every message for as long as a finished run stays Some; only the Running sheet clears `run`, yet `open_sheet` can replace that sheet; `can`, menus and `prune` all key on `run.is_some()`
- **Suspected bug:** while operations run, a second `aizome-tansu image.iso` (or an image opened with Tansu) hands its path over. `Msg::Launched` → `open_sheet(Attach)` closes the Running sheet (`desktop/aizome/apps/tansu/src/app.rs:1104`, `:480-482`). The run finishes in the background, but nothing can now clear `run`: every action answers "Wait for the operations to finish." (`:408-409`), Apply/Undo/Clear/Power Off stay disabled, pending ops are never pruned, and every message resets the status line to "Applied N operations." (`:559-562`) until Tansu restarts. Meanwhile the progress view and Stop After This One are gone. (confidence: high)
- **Reify:** High — four coupled holders of one phase; a session can get stuck for good
- **Related:** run engine `desktop/aizome/apps/tansu/src/run.rs` (TSH-31); instance hand-off (TSH-15)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#tsh-26)
