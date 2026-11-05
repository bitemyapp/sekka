# TSH: Tansu and Shiori

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/apps/{tansu,shiori}`. Audited at `8e0acbe5`; line numbers refer to that commit.

42 files, 0 pending. 32 implicit machines: 8 High, 8 Medium, 16 Low.

## Machines

<a id="tsh-01"></a>

### TSH-01 — (test harness) Corpus example's per-document worker driver
- **Where:** `desktop/aizome/apps/shiori/examples/corpus.rs:24` (`waiting`), `:23` (`out`), `:25` (`until`); shared file iterator `desktop/aizome/apps/shiori/examples/corpus.rs:110`
- **Encoding:** local counter of outstanding replies, an output log whose contents also serve as phase markers ("pages", "unanswered"), a 120 s deadline
- **States:** `Opening` (waiting == 0) → `Asked` (waiting = tiles + 2·pages + 1) → `Done` | `OpenFailed` | `GaveUp` | `Timeout` ("unanswered(n)")
- **Transitions:** first Opened → Asked, sends Draw/Text/Links/Find (`desktop/aizome/apps/shiori/examples/corpus.rs:33-62`, `:54`); a later Opened while waiting > 0 (worker restarted) ignored (`:34-37`); Drawn/Text/Links decrement (`:68`); DrawFailed (`:69-72`); Found decrements only for page 9 or when it is the last outstanding (`:73-78`); Failed decrements, breaks if gave_up (`:79-85`); recv timeout (`:28-30`); exit test (`:88-94`)
- **Hazards:** requests queued in a worker that crashed without giving up are lost and never re-sent, so the count can only end in the 120 s "unanswered" timeout; the Found rule relies on the worker running find after all draws and asks (`desktop/aizome/apps/shiori/src/worker.rs:61-78`); `waiting -= 1` at `desktop/aizome/apps/shiori/examples/corpus.rs:68` and `:71` doesn't saturate
- **Reify:** Low — example program
- **Related:** engine `desktop/aizome/apps/shiori/src/engine.rs` (TSH-14), worker `desktop/aizome/apps/shiori/src/worker.rs` (TSH-21)

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
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#tsh-02)

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

<a id="tsh-05"></a>

### TSH-05 — Sidebar preview prefetch (ThumbRows)
- **Where:** `desktop/aizome/apps/shiori/src/app.rs:69` (`ThumbRows.recent` `:71`, `.asked` `:73`), `Doc.thumbs` `desktop/aizome/apps/shiori/src/app.rs:310`
- **Encoding:** a deque of recently built rows (at most 48) and a set of pages asked for and not yet drawn, mutated by the view while it builds rows
- **States:** per page `Unseen` → `Seen` → `Asked` → `Drawn` (removed from asked); `Asked` indefinitely if that key never arrives
- **Transitions:** row build calls `saw` and `asked.insert`, then sends `Msg::Thumb` (`desktop/aizome/apps/shiori/src/ui.rs:260-271`); `Msg::Thumb` → `request` (`desktop/aizome/apps/shiori/src/app.rs:2029`); `request` adds previews of recent rows (`:1088-1095`); Drawn of the exact preview key removes from asked (`:1142-1144`); keep set protects them (`:1145-1151`)
- **Hazards:** state is mutated during `view` through a RefCell. `asked` is cleared only when that exact preview arrives. A preview that fails, is dropped by the worker's replace-on-Draw queue or by `MOST_WANTED` truncation (`desktop/aizome/apps/shiori/src/app.rs:1096`), changes key with scale, or is cleared by a reload stays asked, so its row never sends `Msg::Thumb` again and waits for some other request. It isn't reset on reload or restart.
- **Reify:** Medium — view-time mutation, and an asked set that never expires

<a id="tsh-06"></a>

### TSH-06 — Pinch / ❖-scroll zoom settle debounce
- **Where:** `desktop/aizome/apps/shiori/src/app.rs:313` (`Doc.zooming`), `:314` (`.settling`)
- **Encoding:** generation counter + bool + 160 ms `cx.after` timer
- **States:** `Rested` → `Settling(seq)` → `Rested`
- **Transitions:** ZoomBy increments, sets settling, arms `ZoomRested(seq)` (`desktop/aizome/apps/shiori/src/app.rs:1852-1855`); a matching ZoomRested clears (`:1859-1861`); `request` skips tiles while settling (`:1051`, `:1062`); `idle()` false meanwhile (`:859`)
- **Hazards:** none observed (stale timers dropped by seq)
- **Reify:** Low — small and local

<a id="tsh-07"></a>

### TSH-07 — Pointer press/drag gesture on the pages
- **Where:** `desktop/aizome/apps/shiori/src/app.rs:295` (`Doc.drag: Option<Drag>`), `:298` (`.drag_at`), `:299` (`.autoscrolling`), `:302` (`.link_press`), `:294` (`.selection`), `:304` (`.hover`); `enum Drag` `desktop/aizome/apps/shiori/src/app.rs:96`
- **Encoding:** partially explicit: the `Drag` enum (Select, Bar, HBar, Pan) plus the drag point, an autoscroll flag with a 50 ms DragTick timer, the pressed link with its slop, and the selection; the widget routes moves and releases by `dragging` (`desktop/aizome/apps/shiori/src/pages.rs:493-502`)
- **States:** `Idle` → `Pressed(link?)` → `Selecting` [→ `Autoscrolling`] | `BarDrag` | `HBarDrag` | `Panning` → `Idle` (release; follows the link if `link_press` survived)
- **Transitions:** press (`desktop/aizome/apps/shiori/src/app.rs:1469-1494`); slop clears link (`:1505-1510`); autoscroll arm (`:1517-1520`); focus moves (`:1524-1526`); DragTick (`:1911-1917`); Release (`:1920-1928`); PanStart (`:1931-1934`); BarPress/HBarPress (`:1958`, `:1969`); Escape clears selection (`:1940-1941`)
- **Hazards:** `autoscrolling` is cleared only by the tick it armed, and Release doesn't touch it; a left press during a middle-button pan replaces `Pan` with `Select`, and either button's release ends it (`desktop/aizome/apps/shiori/src/pages.rs:500`); a press that misses text sets `Select` with `selection = None` (`desktop/aizome/apps/shiori/src/app.rs:1472-1475`), so dragging from a margin into text selects nothing; a lost PointerUp would leave `drag` set (relies on kit capture)
- **Reify:** Medium — six fields, eight handlers

<a id="tsh-08"></a>

### TSH-08 — Copy waiting for pages' text
- **Where:** `desktop/aizome/apps/shiori/src/app.rs:111` (`Copying.parts`, `.missing`), `:303` (`Doc.copying`)
- **Encoding:** `Option<Copying>` with the set of pages still missing; requests tracked in `Doc.asked`
- **States:** `None` → `Waiting(missing)` → `Copied` (clipboard set) | `Dropped` (worker gone / reload)
- **Transitions:** `copy` copies at once or waits and asks (`desktop/aizome/apps/shiori/src/app.rs:1588-1594`, `:423-432`); `text_came` takes parts and completes (`:1601-1611`); failed Text → empty text → `text_came` (`:1228-1230`, `:1262-1264`); Opened again re-asks (`:1296`); `worker_gone`/reload swap drop it (`:382`, `:1316`); toolbar "Reading N pages to copy…" (`desktop/aizome/apps/shiori/src/ui.rs:110-113`)
- **Hazards:** a new Copy silently replaces a waiting one; no timeout; `copy` never checks for an engine
- **Suspected bug:** the worker gives up and `engine` becomes None, but Load stays Open. The person then selects across pages whose text isn't loaded (pruned, or never read) and copies: `copy` stores the `Copying` (`desktop/aizome/apps/shiori/src/app.rs:1593`), but `ask_copy_text` returns without an engine (`:424`). Nothing is copied, nothing says why (the lasting notice occupies the status), and `idle()` stays false until a reload swap. (confidence: medium)
- **Reify:** High — suspected bug; a waiter with no exit when its producer is gone
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#tsh-08)

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
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#tsh-09)

<a id="tsh-10"></a>

### TSH-10 — Shiori sheets (password, go to page, info)
- **Where:** `desktop/aizome/apps/shiori/src/app.rs:718` (`Shiori.sheets`), `enum Sheet` `desktop/aizome/apps/shiori/src/app.rs:614`
- **Encoding:** a map from sheet window to `Sheet` (partially explicit); the password error comes from `Load::Locked{tried}`
- **States:** `None` → `Password{text}` | `GoTo{text, error}` | `Info` → closed
- **Transitions:** `ask_password` (one per parent) (`desktop/aizome/apps/shiori/src/app.rs:1356-1362`); GoToPanel (`:2063-2064`); Info (`:2069-2070`); SheetText (`:2047-2054`); `sheet_ok` (`:1693-1725`); SheetCancel (`:2057-2058`); closing the sheet window (`:2131-2133`); `close_doc` drops a document's sheets (`:1733`); `target()` maps sheet to parent (`:845`)
- **Hazards:** Cancel leaves Locked; the overlay's button is the only retry (`desktop/aizome/apps/shiori/src/pages.rs:283-291`); GoTo/Info aren't deduplicated, but the kit's modal menu guard makes a second one unreachable
- **Reify:** Low

<a id="tsh-11"></a>

### TSH-11 — Tick-counted notice expiry and scroll-bar visibility
- **Where:** `desktop/aizome/apps/shiori/src/app.rs:731` (`Shiori.ticks`), `:317` (`Doc.bar_until`), `:323` (`.trouble`), `:325` (`.trouble_until`)
- **Encoding:** the 1.5 s watch tick serves as the clock; deadlines are tick numbers
- **States:** bar `Hidden` → `Shown(until)`; notice `None` → `Passing(until)` | `Lasting`
- **Transitions:** Tick increments and expires (`desktop/aizome/apps/shiori/src/app.rs:2096-2102`); `bar_until` set by scroll/nav/drags (`:1432`, `:1538`, `:1548`, `:1553`, `:1844`, `:1959`, `:1970`); bar shown (`desktop/aizome/apps/shiori/src/pages.rs:250`); notices set (`desktop/aizome/apps/shiori/src/app.rs:1243-1245`, `:1256-1259`, `:363-373`) and cleared (`:1248`, `:1317`)
- **Hazards:** "a moment after" is anywhere from 0 to 1.5 s; the drag override at `desktop/aizome/apps/shiori/src/pages.rs:250` covers `Drag::Bar` but not `HBar`
- **Reify:** Low

<a id="tsh-12"></a>

### TSH-12 — Per-document view modes (sidebar, tab, outline expansion, full screen)
- **Where:** `desktop/aizome/apps/shiori/src/app.rs:306` (`Doc.sidebar`), `:308` (`.sidebar_tab`), `:311` (`.expanded`), `:315` (`.fullscreen`)
- **Encoding:** bool/usize/set; sidebar persisted in `Place.sidebar` (TSH-19); `fullscreen` mirrors the compositor
- **States:** sidebar `Hidden` | `Pages` | `Contents`; full screen `Windowed` → `Requested` → `FullScreen` (mirror)
- **Transitions:** first open from place, else outline-and-width default (`desktop/aizome/apps/shiori/src/app.rs:1339-1345`); ToggleSidebar (`:2019`); SidebarTab (`:2025`); OutlineToggle (`:2032-2036`); FullScreen requests the opposite of the mirror (`:2074-2075`); Escape leaves (`:1942-1943`); mirror updated (`:2126`); persisted by `place()` (`:405`)
- **Hazards:** tab 1 is kept when there's no outline (clamped in `desktop/aizome/apps/shiori/src/ui.rs:228`); positional `expanded` ids survive a reload swap; two FullScreen commands before the compositor answers both request the same state
- **Reify:** Low

<a id="tsh-13"></a>

### TSH-13 — Window bookkeeping: empty placeholder window and command target
- **Where:** `desktop/aizome/apps/shiori/src/app.rs:721` (`Shiori.empty`), `:727` (`.focused`)
- **Encoding:** `Option<WindowKey>` of the window with nothing open; the last key window
- **States:** `empty = Some(w)` (launched without files) → `None` (taken by the first open, or closed)
- **Transitions:** launched (`desktop/aizome/apps/shiori/src/app.rs:1775-1776`); `open_paths` takes it (`:880-886`); close (`:2134-2136`); focused per update (`:1788-1790`); `target()` (`:844-847`)
- **Hazards:** none observed
- **Reify:** Low

<a id="tsh-14"></a>

### TSH-14 — Document worker supervisor (spawn, watchdog, restart, give up, close)
- **Where:** `desktop/aizome/apps/shiori/src/engine.rs:55` (`Shared.stdin` `:56`, `.child` `:57`, `.busy` `:59`, `.stopped` `:61`, `.closed` `:63`); locals `failures` `desktop/aizome/apps/shiori/src/engine.rs:135`, `pages` `:214`
- **Encoding:** three threads (supervise, forward, watch) share mutexes and atomics; a 60 s window allows four failures; `Drop` sets `closed` and kills
- **States:** `Spawning` → `Opening` (Open frame written, child kept) → `Running` ⇄ `Busy(job, since)` → `Dead(Crashed|TooLong)` → `Spawning` | `GaveUp`; `SpawnFailed` (OpenFailed(Worker)); `Closed` from any
- **Transitions:** spawn (`desktop/aizome/apps/shiori/src/engine.rs:140-146`); Open frame + publish stdin (`:150-152`); keep-or-closed handshake (`:153`, `:189-200`); `stopped` reset (`:156`); Working → busy (`:221-223`); replies clear busy (`:230`, `:239`, `:245`, `:249`); implausible drawing stops reading (`:225-228`); exit cleanup (`:161-165`); classify and give up (`:169-181`); watchdog kill (`:276-286`); forward drops stdin on a write error (`:264-268`); Drop (`:118-127`)
- **Hazards:** requests sent while `stdin` is None are dropped silently (`desktop/aizome/apps/shiori/src/engine.rs:264`); the watchdog decides `late` under the busy lock (`:276-281`) but kills whatever `child` holds when it next locks it (`:284`), so if the late worker dies on its own and a new one is kept in between, the newcomer is killed while opening and `job == Open` gives up (`:176`); `stopped` resets only after the next child is kept (`:156`); after giving up, the forward and watch threads live until the Engine drops; `busy` assumes one job at a time
- **Reify:** High — a cross-thread lifecycle in five shared fields that already needs a lock-order handshake (`keep`)
- **Related:** app side `desktop/aizome/apps/shiori/src/app.rs` (TSH-02), worker side `desktop/aizome/apps/shiori/src/worker.rs` (TSH-21)

<a id="tsh-15"></a>

### TSH-15 — Single-instance socket hand-off (Shiori and Tansu)
- **Where:** `desktop/aizome/apps/shiori/src/instance.rs:30` (`claim`), `:51` (`serve`); also `desktop/aizome/apps/tansu/src/instance.rs:26`, `:45`; listener held in `desktop/aizome/apps/shiori/src/app.rs:724` (`Shiori.instance`) and `desktop/aizome/apps/tansu/src/app.rs:190` (`Tansu.listener`)
- **Encoding:** the socket file `$XDG_RUNTIME_DIR/aizome-app.{shiori,tansu}.sock`, a `Claim` result, and a listener parked until `launched` starts the serve thread
- **States:** socket `Absent` | `Stale` | `Served`; a launch ends `Handed` | `First` | `Alone`
- **Transitions:** connect + write → Handed (`desktop/aizome/apps/shiori/src/instance.rs:32-37`; `desktop/aizome/apps/tansu/src/instance.rs:28-32`); remove stale + bind (`desktop/aizome/apps/shiori/src/instance.rs:40-46`; `desktop/aizome/apps/tansu/src/instance.rs:34-40`); main stores the listener (`desktop/aizome/apps/shiori/src/main.rs:31-35`; `desktop/aizome/apps/tansu/src/main.rs:35-39`); serve thread started (`desktop/aizome/apps/shiori/src/app.rs:1770-1772`; `desktop/aizome/apps/tansu/src/app.rs:1093-1095`); each connection → Msg::Raise / Msg::Launched (`desktop/aizome/apps/shiori/src/instance.rs:68`; `desktop/aizome/apps/tansu/src/instance.rs:58`)
- **Hazards:** the socket file isn't removed on exit (the next launch removes it after a refused connect); a successful connect doesn't prove the peer is processing (a hung instance swallows the files); Tansu's client never shuts down its write side, so the server's 500 ms read relies on the client exiting; `--sample` skips the claim (`desktop/aizome/apps/tansu/src/main.rs:33`)
- **Suspected bug:** two launches at the same moment both fail to connect, then both `remove_file` and `bind` (`desktop/aizome/apps/shiori/src/instance.rs:40-41`). The second unlinks the first's socket, so the first instance keeps running but can't be reached, later launches go to the second, and the two instances save `places.json` over each other (two Tansu windows could each queue and apply plans). (confidence: medium)
- **Reify:** Medium — a time-of-check/time-of-use gap between probe, unlink and bind; ownership isn't explicit (no lock file or abstract socket)
- **Reproduction:** not attempted; only suspected bugs on High machines were reproduced

<a id="tsh-16"></a>

### TSH-16 — Pages widget incremental-damage snapshot
- **Where:** `desktop/aizome/apps/shiori/src/pages.rs:304` (`struct Laid`: size, whole, scroll, parts, bar), kept in `cx.state::<Laid>()` `desktop/aizome/apps/shiori/src/pages.rs:330`
- **Encoding:** what the previous layout produced, retained by the kit across frames
- **States:** `Unlaid` → `Laid(hash)`; each layout is `Whole` (size or hash changed) or `Incremental` (scroll blit + changed parts)
- **Transitions:** snapshot replaced (`desktop/aizome/apps/shiori/src/pages.rs:332-336`); full redraw (`:337-340`); scroll (`:349-352`); bar tracks (`:354-360`); part diff (`:362-369`)
- **Hazards:** the snapshot advances on every layout whether or not a paint follows (correct only if the kit accumulates invalidations); anything drawn outside `parts`/`whole` relies on other invalidation
- **Reify:** Low

<a id="tsh-17"></a>

### TSH-17 — Paint-to-program view-size feedback (Seen)
- **Where:** `desktop/aizome/apps/shiori/src/pages.rs:37` (`Reporter.last`), `desktop/aizome/apps/shiori/src/pages.rs:63` (`Pages.assumed`); `desktop/aizome/apps/shiori/src/app.rs:319` (`Doc.reported`), `:278-279` (`Doc.view`, `.scale`)
- **Encoding:** the program's assumed view and scale vs what paint saw; a hashed key ensures each change is reported once
- **States:** `InSync` → `Reported(key)` → `InSync` (Msg::Seen applied)
- **Transitions:** paint compares and sends (`desktop/aizome/apps/shiori/src/pages.rs:657-667`); Msg::Seen relays out, sets the global scale, clears `sent` on a scale change (`desktop/aizome/apps/shiori/src/app.rs:1818-1838`)
- **Hazards:** `Shiori.scale` is last-writer-wins across windows (`desktop/aizome/apps/shiori/src/app.rs:1834`) and seeds new documents (`:904`); a message is emitted from paint
- **Reify:** Low

<a id="tsh-18"></a>

### TSH-18 — ActualText marked-content stack
- **Where:** `desktop/aizome/apps/shiori/src/pdftext.rs:119` (`Collector.actual`)
- **Encoding:** a stack of `Option<(text, first glyph)>`, one per open marked-content sequence, across the interpreter's device callbacks
- **States:** `Outside` → `Open(depth, ActualText?)` → `Outside`
- **Transitions:** begin pushes (`desktop/aizome/apps/shiori/src/pdftext.rs:150-155`); end pops and replaces glyphs when outermost with text (`:157-178`)
- **Hazards:** an unmatched end pops nothing; a sequence never closed leaves its ActualText unapplied; inner ActualText is ignored under an outer one
- **Reify:** Low

<a id="tsh-19"></a>

### TSH-19 — Persisted reading place (places.json)
- **Where:** `desktop/aizome/apps/shiori/src/places.rs:53` (`Places.file` `:55`, `.places` `:57`), file at `desktop/aizome/apps/shiori/src/places.rs:70`; `desktop/aizome/apps/shiori/src/app.rs:281` (`Doc.pending`), `:321` (`Doc.kept`)
- **Encoding:** newest-last list of (path, Place) capped at 400, loaded once per process; per document, the place to apply and the place last saved; schema migration through `#[serde(default)] sidebar` (`desktop/aizome/apps/shiori/src/places.rs:48-49`)
- **States:** per path `Unknown` → `Remembered(Place)`; per open document `Pending(place)` → `Applied`, `Kept(place)`
- **Transitions:** load (`desktop/aizome/apps/shiori/src/app.rs:821`, `desktop/aizome/apps/shiori/src/places.rs:62-66`); open reads pending and kept (`desktop/aizome/apps/shiori/src/app.rs:889`, `:930`); first open takes and applies pending (`:1326-1345`); `keep_place` on each tick and on close sets and saves when moved (`:1011-1020`, `:2105-2108`, `:1728`); save via tmp + rename (`desktop/aizome/apps/shiori/src/places.rs:91-101`); cap (`:85-87`)
- **Hazards:** the in-memory list is loaded once, so the next save overwrites places another Shiori process saved; the fixed `places.json.tmp` name races between processes; a corrupt file loads as empty and is then overwritten
- **Reify:** Low
- **Related:** single instance (TSH-15)

<a id="tsh-20"></a>

### TSH-20 — Stand-in pixel buffer pool
- **Where:** `desktop/aizome/apps/shiori/src/standin.rs:30` (`SPARES` thread-local)
- **Encoding:** buffers listed in the pool; availability inferred from `Arc::strong_count == 1`
- **States:** per buffer `New` → `InUse` (an image holds it; also listed) → `Spare` → `Reused` | `Evicted`
- **Transitions:** take a spare of exact length with count 1 (`desktop/aizome/apps/shiori/src/standin.rs:73-80`); paper fill or draw over (`:117-126`); return every buffer and trim to 64 MB (`:140-146`)
- **Hazards:** a reused buffer keeps old pixels where neither `rows.fill` nor a covering size wrote; that's correct only because same-size sources don't overlap (`desktop/aizome/apps/shiori/src/standin.rs:117-121`)
- **Reify:** Low

<a id="tsh-21"></a>

### TSH-21 — Worker process job queue and document lifecycle
- **Where:** `desktop/aizome/apps/shiori/src/worker.rs:45` (`Queue.open` `:46`, `.draws` `:47`, `.asks` `:48`, `.find` `:49`, `.closed` `:50`), `doc: Option<Doc>` `desktop/aizome/apps/shiori/src/worker.rs:153`
- **Encoding:** a mutex+condvar queue fed by a stdin reader thread; `Next` (`desktop/aizome/apps/shiori/src/worker.rs:102`) is derived by priority open > draws > asks > find
- **States:** `Unopened` → `Opening` → `Open(doc)` | `OpenFailed(no doc)`; `Draining` (stdin EOF) → exit; `AppGone` (write error) → exit; search `Idle` ⇄ `Searching(serial, pages)`
- **Transitions:** `take` (`desktop/aizome/apps/shiori/src/worker.rs:80-99`); `next` (`:61-78`, search ends `:74`); Open resets doc (`:169-178`); jobs without a doc skipped (`:181`, `:200`); EOF marks closed (`:142-145`), exit once empty (`:162-163`); write error exits (`:206-209`); death signal / orphan check (`:114`, `:122-124`)
- **Hazards:** jobs after a failed open are dropped with no reply (the app relies on dropping its engine); on EOF the queue drains before exit; Draw replaces the whole pending list, which the app's `sent` must mirror (TSH-04); a Found for a replaced or stopped search can still be sent for the page in progress (TSH-09)
- **Reify:** Medium — explicit priorities, but the open state and the queue are separate
- **Related:** app side `desktop/aizome/apps/shiori/src/engine.rs` (TSH-14); process role chosen in `desktop/aizome/apps/shiori/src/main.rs:13-15`

<a id="tsh-22"></a>

### TSH-22 — (test harness) Shiori settle and wait loops
- **Where:** `desktop/aizome/apps/shiori/tests/budget.rs:20` (`settle`), `desktop/aizome/apps/shiori/tests/shiori.rs:55` (`settle`, `quiet`), `desktop/aizome/apps/shiori/tests/shiori.rs:430` (`kill_and_lose_the_worker`), `desktop/aizome/apps/shiori/tests/worker.rs:23` (`wait`)
- **Encoding:** render/send/advance rounds polling `Shiori::idle()`; consecutive-quiet counter; per-call deadlines
- **States:** `Busy` → `Quiet(n)` → `Settled` | panic after 3000 rounds; worker tests `Waiting(10 s)` → `Picked` | panic
- **Transitions:** budget settles on the first idle round (`desktop/aizome/apps/shiori/tests/budget.rs:21-29`); shiori.rs needs four in a row (`desktop/aizome/apps/shiori/tests/shiori.rs:63-70`); kill then `h.settle` until the engine is None (`:431-436`); recv loop (`desktop/aizome/apps/shiori/tests/worker.rs:24-31`)
- **Hazards:** `idle()` (`desktop/aizome/apps/shiori/src/app.rs:854-864`) is a seven-field conjunction, so any stuck waiter (e.g. TSH-08) shows up only as "Shiori didn't settle"
- **Reify:** Low — test harness

<a id="tsh-23"></a>

### TSH-23 — Tansu UDisks snapshot refresh and drive fallback
- **Where:** `desktop/aizome/apps/tansu/src/app.rs:174` (`Tansu.seen`), `:175` (`.state`), `:176` (`.drives`), `:177` (`.drive`), `:208` (`.seen_used`), `:195` (`.start_show`), `:192` (`.start_paths`), `:197` (`.live`)
- **Encoding:** a generation compare (`u64::MAX` forces a re-read), the cached snapshot, deferred launch intents, and a flag that swaps a placeholder mock for the live UDisks client at launch
- **States:** backend `Placeholder` → `Live`; snapshot `Fresh` | `Stale(gen)`; drive `Chosen` → `Gone → FirstDrive`; `--show` `Pending` → `Consumed`
- **Transitions:** `live()` starts on an empty mock (`desktop/aizome/apps/tansu/src/app.rs:247-250`); launched swaps the backend and forces a refresh (`:1084-1091`); `refresh` (`:284-314`) records usage (`:289-295`), prunes the plan when no run exists (`:296-301`), falls back to the first drive (`:308-311`); `refresh_show` (`:267-278`); start paths opened at launch (`:1096-1099`); forced after a run (`:569`); every update refreshes (`:1239`)
- **Hazards:** `refresh` runs after every message, so a UDisks change can rewrite `layout` under an open sheet (TSH-25); the drive fallback is silent; `prune` is skipped while `run` is Some, and `run` can stay Some (TSH-26)
- **Reify:** Medium — eight fields, several triggers
- **Related:** UDisks client `desktop/aizome/crates/aizome-sys/src/disks/mod.rs`

<a id="tsh-24"></a>

### TSH-24 — Tansu segment selection tracking
- **Where:** `desktop/aizome/apps/tansu/src/app.rs:180` (`Tansu.sel: Option<Sel>`), `:205` (`.sel_at`); `enum Sel` `desktop/aizome/apps/tansu/src/app.rs:23`
- **Encoding:** partially explicit `Sel` (Target | Free{offset, logical}) plus the last resolved position, used to re-find a selection after the layout changes
- **States:** `None` → `Target(block | new id)` | `Free(offset)`; on layout change `Target gone → Free(old offset)`, `Free now covered → Target`, `None → first partition`
- **Transitions:** relayout repairs (`desktop/aizome/apps/tansu/src/app.rs:338-360`); resolution (`:377-403`); SelectDrive (`:1112-1115`); SelectSeg / SelectRows / Context (`:1119`, `:1126`, `:1131`); a created partition is selected (`:791`); a new table clears (`:858`); drive fallback / `--show` clear (`:310`, `:275`)
- **Hazards:** Free resolves to the stretch holding the old offset, else a partition back there, else the nearest free (`desktop/aizome/apps/tansu/src/app.rs:383-401`), so after Undo it can land on a different partition; `Target::New(id)` ids can be reused after Undo restores an older `Plan.next` (`desktop/aizome/apps/tansu/src/plan.rs:114`)
- **Reify:** Medium

<a id="tsh-25"></a>

### TSH-25 — Tansu sheet lifecycle (forms holding segment indexes)
- **Where:** `desktop/aizome/apps/tansu/src/app.rs:184` (`Tansu.sheet: Option<Sheet>`), `struct Sheet` `desktop/aizome/apps/tansu/src/app.rs:96`, `enum SheetKind` `:82`; captured indexes `FormatForm.seg` `:33`, `CreateForm.seg/.offset/.max` `:44-46`, `ResizeForm.seg..max` `:63-68`, `Label/Flags/Unlock{seg}` `:86-89`, `Smart{task}` `:93`
- **Encoding:** partially explicit: one `Option<Sheet>` with a kind enum, but each form holds `seg`, an index into `self.layout.segs` taken when it opened; `error` resets on every sheet message
- **States:** `None` → `Format | Create | Resize | Label | Flags | Table | Unlock | Attach | Apply | Smart` → `None`; `Apply → Running` (TSH-26); any → another (replaced)
- **Transitions:** `open_sheet` closes any current sheet first (`desktop/aizome/apps/tansu/src/app.rs:479-488`); opened by actions (`:634`, `:654`, `:670`, `:687`, `:690`, `:694`), NewTable (`:1144`), AttachImage (`:1149`), ShowSmart (`:1186`), `open`/Launched (`:1104`, `:1232-1235`); error reset (`:704`); submits queue against the current layout by index (`:732`, `:789`, `:821`, `:828`, `:850`) or act on `layout.segs.get(seg)` (`:871`); close (`:490-494`, `:1191-1199`); SMART result (`:951-959`); views read the segment live (`desktop/aizome/apps/tansu/src/ui/sheets.rs:137`, `:265`, `:319`, `:344`, `:412`)
- **Hazards:** the kit blocks menus while a sheet is modal, but not `Msg::Changed` (TSH-23) or `Msg::Launched`/`Program::open`; a second launch replaces a half-filled form (passphrases included) without asking; Create re-checks its offset (`desktop/aizome/apps/tansu/src/plan.rs:441`), but the other forms never check that the segment is still the one they opened for
- **Suspected bug:** open Format… on a USB stick's partition, then unplug the stick (or let another tool repartition the disk). `refresh` falls back to the first drive (`desktop/aizome/apps/tansu/src/app.rs:308-311`), so the open sheet's `seg` now indexes that drive's layout. The sheet's title quietly changes to the other partition (`desktop/aizome/apps/tansu/src/ui/sheets.rs:137-140`), and Add to Pending queues a format of it (`desktop/aizome/apps/tansu/src/app.rs:732`); if the fallback is another removable disk, Apply asks for no typed name. Resize, Label and Flags behave the same, and Unlock tries the passphrase on another volume (`:871-873`). (confidence: medium)
- **Reify:** High — index-based identity across a mutable layout, with data-loss consequences
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#tsh-25)
- **Follow-up:** reified as typestate in [#290](https://github.com/bitemyapp/sekka/pull/290). Its suspected bug is fixed, and the mistake no longer compiles.

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
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#tsh-26)

<a id="tsh-27"></a>

### TSH-27 — Tansu immediate operations in flight (mount, unlock, attach…)
- **Where:** `desktop/aizome/apps/tansu/src/app.rs:187` (`Tansu.busy: Vec<Busy>`), `struct Busy` `desktop/aizome/apps/tansu/src/app.rs:103`, `enum BusyTask` `:109`
- **Encoding:** a list of tasks polled after every message (a task's completion wakes the app)
- **States:** per op `Started(task)` → `Succeeded(notice)` | `Failed(notice + alert)`
- **Transitions:** `start` pushes and polls once without `cx` (`desktop/aizome/apps/tansu/src/app.rs:496-500`); `poll_busy` after each update (`:1237`, `:503-543`); the alert only when `cx` is given (`:534-539`)
- **Hazards:** a task already finished when `start` polls (always with the mock; a fast failure in live use) reports its error in the status line but never raises the alert (`desktop/aizome/apps/tansu/src/app.rs:499`); busy ops don't block plan edits, Apply, Detach, or a second identical op before UDisks reports the first
- **Reify:** Low

<a id="tsh-28"></a>

### TSH-28 — Tansu filesystem-tool probing
- **Where:** `desktop/aizome/apps/tansu/src/app.rs:200` (`.missing`), `:201` (`.tools`), `:202` (`.asked_tools`)
- **Encoding:** a once flag, a list of pending probe tasks, and the result map
- **States:** `NotAsked` → `Probing(tasks)` → `Known(missing)`
- **Transitions:** first refresh with UDisks up and filesystems listed (`desktop/aizome/apps/tansu/src/app.rs:302-306`); collected (`:1322-1334`); used by `cannot_make` (`:1337-1346`) and the format choices (`desktop/aizome/apps/tansu/src/ui/sheets.rs:90-103`)
- **Hazards:** probed once per process, so a UDisks restart or newly installed tool isn't noticed; failed probes count as available; until the probes finish `missing` is empty, so an early format passes `cannot_make`
- **Reify:** Low

<a id="tsh-29"></a>

### TSH-29 — (test harness) In-memory UDisks mock
- **Where:** `desktop/aizome/apps/tansu/src/mock.rs:25` (`Mock.state` `:26`, `.generation` `:27`, `.fail` `:32`, `.serial` `:33`); also `desktop/aizome/apps/tansu/tests/resize.rs:34` (`finish`)
- **Encoding:** mutable sample state with a generation counter, a one-shot armed failure, a UUID serial, a call log
- **States:** failure `Disarmed` → `Armed(call, message)` → consumed by the next matching call; generation counts changes
- **Transitions:** `change` bumps generation (`desktop/aizome/apps/tansu/src/mock.rs:79-82`); `fail_next` arms (`:54-56`); `call` consumes (`:85-93`); UUID serial (`:99-102`); every task is ready immediately (`:144-146`); resize tests run `advance` up to 20 times (`desktop/aizome/apps/tansu/tests/resize.rs:35-41`)
- **Hazards:** tasks never pend, so UI tests never exercise TSH-27's polling or TSH-31's in-flight and settle waits; `confirmed` checks only presence and in-use, not the confirmation snapshot (`desktop/aizome/apps/tansu/src/mock.rs:106-113`); the mock also serves `--sample` (`desktop/aizome/apps/tansu/src/main.rs:31`) and the live placeholder (`desktop/aizome/apps/tansu/src/app.rs:248`)
- **Reify:** Low — test harness

<a id="tsh-30"></a>

### TSH-30 — Tansu pending plan and undo history
- **Where:** `desktop/aizome/apps/tansu/src/plan.rs:112` (`Plan.ops` `:113`, `.next` `:114`); `desktop/aizome/apps/tansu/src/app.rs:182` (`Tansu.history: Vec<Plan>`)
- **Encoding:** ordered ops under per-device rules (one rewrite per device; label/flags join a format; delete drops the others; a new table drops the disk's ops), shown as `Mark`s in the preview (`desktop/aizome/apps/tansu/src/layout.rs:36`); the undo stack holds whole Plan clones
- **States:** per device `Untouched` → `Pending{Format[+Label] | Resize | Repair | Delete | Label | Flags}`; new partition `New(id)` edited in place; disk `NewTable`; ops end `Applied` | `Dropped` (pruned) | `Undone`
- **Transitions:** `queue` pushes history (`desktop/aizome/apps/tansu/src/app.rs:448-466`); create / delete / format / resize / label / flags / repair / new table (`desktop/aizome/apps/tansu/src/plan.rs:438-454`, `:457-473`, `:477-499`, `:525-551`, `:555-584`, `:587-616`, `:619-629`, `:429-435`); gates (`:187-383`, `:217-226`); prune (`:633-643`); problems at Apply (`:647-669`); Undo (`desktop/aizome/apps/tansu/src/app.rs:1201-1207`); Clear (`:1209-1214`); run completion keeps undone ops and clears history (`:554-555`)
- **Hazards:** the rules are enforced one function at a time, not by a per-device state; history snapshots are never pruned, so Undo can restore ops for a disk that's gone (caught only by `problems`) and an older `next`, reusing ids; a partly done resize stays pending with a stale `from` (`desktop/aizome/apps/tansu/tests/resize.rs:93-113`), so its words say "Grow … from 140 GB" for a partition already at 160
- **Reify:** Medium

<a id="tsh-31"></a>

### TSH-31 — Tansu run step engine (steps, effect waits, confirmation tokens)
- **Where:** `desktop/aizome/apps/tansu/src/run.rs:112` (`Run.items` `:113`, `.current` `:114`, `.step` `:115`, `.inflight` `:116`, `.after` `:118`, `.expect` `:119`, `.outcome` `:120`, `.stop_requested` `:122`, `.held` `:124`); `Item.status/.token/.agreed/.made` `desktop/aizome/apps/tansu/src/run.rs:51-60`
- **Encoding:** partially explicit: `Status` per item and `Outcome`, but the step within an operation is an integer indexing per-kind step lists rebuilt on each call (`desktop/aizome/apps/tansu/src/run.rs:380-386`, `:412-416`), with the in-flight task, the awaited effect and its deadline in separate Options; tokens are consumed by first steps
- **States:** `Agreed` → per op `Step(n)`: `Starting` → `InFlight` → `AwaitingEffect(≤15 s)` → … → `Finished` → next op → `Done` | `Failed` | `Stopped`; `Held` (pictures)
- **Transitions:** `agree` makes tokens (`desktop/aizome/apps/tansu/src/run.rs:138-177`); `advance` (`:197-272`): collect task (`:215-231`), record made path (`:224-225`), arm effect and step + 1 (`:238-239`), wait or time out (`:241-250`), start step (`:252-256`), finish op / outcome (`:257-266`); `fail` (`:274-280`); `next_step` (`:283-497`) with token takes (`:292`, `:336`, `:343`, `:389`, `:419`, `:491`) and re-confirmation (`:366-376`); `picture` holds (`:507-517`); stop flag from the app (`desktop/aizome/apps/tansu/src/app.rs:1226`)
- **Hazards:** an effect not seen within SETTLE is logged and the run goes on (`desktop/aizome/apps/tansu/src/run.rs:244-248`); Flags re-derives its step from live state (`:461-487`), so a type change not reported within SETTLE is repeated at step 1 and step 2 finishes without setting the bits, yet the op is marked Done; in-flight tasks have no timeout (cancellation goes through UDisks jobs); stop is honoured only between operations
- **Reify:** Medium — transitions are already centralised in `advance`/`next_step`; an enum of steps per op kind would replace the integer index
- **Related:** confirmations `desktop/aizome/crates/aizome-sys/src/disks/confirm.rs`; app session (TSH-26)

<a id="tsh-32"></a>

### TSH-32 — (test harness) Private dbus-daemon with a fake UDisks2
- **Where:** `desktop/aizome/apps/tansu/tests/udisks.rs:30` (`TestBus`), `desktop/aizome/apps/tansu/tests/udisks.rs:132` (`Block`), `:200` (`Part`), `:244` (`Fs`)
- **Encoding:** a spawned daemon process with its temp dir; fake objects whose properties change on method calls; polling helpers with deadlines
- **States:** bus `Spawned` → `AddressRead` → `Serving` → `Killed` (Drop); devices change on Format/Delete/Mount; tests poll `eventually` (5 s) and `finish` (10 s)
- **Transitions:** start (`desktop/aizome/apps/tansu/tests/udisks.rs:37-69`); Drop kills, waits and removes the dir (`:72-78`); Format (`:172-187`); Delete removes objects (`:229-241`); Mount (`:254-262`); swapped-stick simulation (`:370-375`); `eventually` (`:307-316`); `finish` (`:325-332`)
- **Hazards:** a daemon that prints no address yields an empty address and a panic in `serve` instead of a skip
- **Reify:** Low — test harness
- **Related:** UDisks client `desktop/aizome/crates/aizome-sys/src/disks/mod.rs`

## Already explicit

- `desktop/aizome/apps/shiori/src/layout.rs:32` `enum Zoom` — zoom mode (fit width, fit page, fixed); after construction (`desktop/aizome/apps/shiori/src/app.rs:900`) set only in `fn zoom_to` (`desktop/aizome/apps/shiori/src/app.rs:1365`)
- `desktop/aizome/apps/shiori/src/app.rs:53` `enum Hover` — what the pointer is over (link, text, nothing); set only by `Msg::Hover` in `fn update` (`desktop/aizome/apps/shiori/src/app.rs:1946-1950`), computed by `fn hover_at` (`desktop/aizome/apps/shiori/src/pages.rs:472`)
- `desktop/aizome/apps/tansu/src/run.rs:44` `enum Status` — an operation's Waiting/Running/Done/Failed; set in `fn advance` (`desktop/aizome/apps/tansu/src/run.rs:253`, `:258`) and `fn fail` (`desktop/aizome/apps/tansu/src/run.rs:276`), plus `fn picture` for pictures (part of TSH-31)

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/apps/shiori/examples/corpus.rs` | 126 | machines | [TSH-01](#tsh-01) | example worker driver |
| `desktop/aizome/apps/shiori/src/app.rs` | 2161 | machines | [TSH-02](#tsh-02), [TSH-03](#tsh-03), [TSH-04](#tsh-04), [TSH-05](#tsh-05), [TSH-06](#tsh-06), [TSH-07](#tsh-07), [TSH-08](#tsh-08), [TSH-09](#tsh-09), [TSH-10](#tsh-10), [TSH-11](#tsh-11), [TSH-12](#tsh-12), [TSH-13](#tsh-13) | document state; also holds listener (TSH-15) and places use (TSH-19) |
| `desktop/aizome/apps/shiori/src/document.rs` | 299 | none |  | PDF structure parsing; lazy named-destination cache |
| `desktop/aizome/apps/shiori/src/engine.rs` | 372 | machines | [TSH-14](#tsh-14) | worker supervisor threads |
| `desktop/aizome/apps/shiori/src/fonts.rs` | 179 | none |  | font resolver memo cache; FFI lookup |
| `desktop/aizome/apps/shiori/src/instance.rs` | 73 | machines | [TSH-15](#tsh-15) | single-instance socket |
| `desktop/aizome/apps/shiori/src/layout.rs` | 343 | explicit |  | pure layout math; enum Zoom mode |
| `desktop/aizome/apps/shiori/src/lib.rs` | 26 | none |  | module list |
| `desktop/aizome/apps/shiori/src/main.rs` | 38 | machines | [TSH-15](#tsh-15), [TSH-21](#tsh-21) | instance claim and --worker role |
| `desktop/aizome/apps/shiori/src/pages.rs` | 669 | machines | [TSH-16](#tsh-16), [TSH-17](#tsh-17), [TSH-07](#tsh-07), [TSH-11](#tsh-11) | pages widget; gesture routing; bar visibility |
| `desktop/aizome/apps/shiori/src/pdftext.rs` | 424 | machines | [TSH-18](#tsh-18) | text extraction; ActualText stack |
| `desktop/aizome/apps/shiori/src/places.rs` | 132 | machines | [TSH-19](#tsh-19) | persisted reading places |
| `desktop/aizome/apps/shiori/src/protocol.rs` | 380 | none |  | wire types; lifecycle in TSH-14 and TSH-21 |
| `desktop/aizome/apps/shiori/src/standin.rs` | 386 | machines | [TSH-20](#tsh-20) | stand-in pixel pool |
| `desktop/aizome/apps/shiori/src/text.rs` | 242 | none |  | pure selection geometry |
| `desktop/aizome/apps/shiori/src/ui.rs` | 597 | machines | [TSH-05](#tsh-05) | view; mutates ThumbRows while building rows |
| `desktop/aizome/apps/shiori/src/worker.rs` | 366 | machines | [TSH-21](#tsh-21) | worker process queue |
| `desktop/aizome/apps/shiori/tests/budget.rs` | 112 | machines | [TSH-22](#tsh-22) | settle loop |
| `desktop/aizome/apps/shiori/tests/common/mod.rs` | 235 | none |  | PDF fixture generator |
| `desktop/aizome/apps/shiori/tests/shiori.rs` | 889 | machines | [TSH-22](#tsh-22) | settle loop; kill-and-wait |
| `desktop/aizome/apps/shiori/tests/worker.rs` | 267 | machines | [TSH-22](#tsh-22) | recv wait loop |
| `desktop/aizome/apps/tansu/src/app.rs` | 1362 | machines | [TSH-23](#tsh-23), [TSH-24](#tsh-24), [TSH-25](#tsh-25), [TSH-26](#tsh-26), [TSH-27](#tsh-27), [TSH-28](#tsh-28) | app state; also undo history (TSH-30) and listener (TSH-15) |
| `desktop/aizome/apps/tansu/src/backend.rs` | 146 | none |  | backend trait and delegation |
| `desktop/aizome/apps/tansu/src/drives.rs` | 162 | none |  | pure sidebar entries |
| `desktop/aizome/apps/tansu/src/fs.rs` | 167 | none |  | filesystem capability tables |
| `desktop/aizome/apps/tansu/src/instance.rs` | 63 | machines | [TSH-15](#tsh-15) | single-instance socket |
| `desktop/aizome/apps/tansu/src/layout.rs` | 519 | none |  | pure layout and flags math; Mark data |
| `desktop/aizome/apps/tansu/src/lib.rs` | 24 | none |  | module list |
| `desktop/aizome/apps/tansu/src/main.rs` | 47 | machines | [TSH-15](#tsh-15) | instance claim; --sample skips it |
| `desktop/aizome/apps/tansu/src/mock.rs` | 1058 | machines | [TSH-29](#tsh-29) | in-memory UDisks mock |
| `desktop/aizome/apps/tansu/src/plan.rs` | 1230 | machines | [TSH-30](#tsh-30) | pending plan rules |
| `desktop/aizome/apps/tansu/src/run.rs` | 518 | machines | [TSH-31](#tsh-31) | run step engine |
| `desktop/aizome/apps/tansu/src/ui/glyphs.rs` | 292 | none |  | drawing; glyph path cache |
| `desktop/aizome/apps/tansu/src/ui/map.rs` | 392 | none |  | map widget; hatch LRU cache |
| `desktop/aizome/apps/tansu/src/ui/mod.rs` | 6 | none |  | module list |
| `desktop/aizome/apps/tansu/src/ui/sheets.rs` | 748 | machines | [TSH-25](#tsh-25), [TSH-26](#tsh-26) | sheet views read segment and run live |
| `desktop/aizome/apps/tansu/src/ui/view.rs` | 731 | none |  | window view; reads TSH-26 and TSH-30 for enablement |
| `desktop/aizome/apps/tansu/tests/look.rs` | 188 | none |  | tests (goldens; holds a Run via picture) |
| `desktop/aizome/apps/tansu/tests/perf.rs` | 117 | none |  | tests |
| `desktop/aizome/apps/tansu/tests/resize.rs` | 113 | machines | [TSH-29](#tsh-29) | mock-driven run loop |
| `desktop/aizome/apps/tansu/tests/udisks.rs` | 401 | machines | [TSH-32](#tsh-32) | fake UDisks on private bus |
| `desktop/aizome/apps/tansu/tests/ui.rs` | 440 | none |  | tests |
