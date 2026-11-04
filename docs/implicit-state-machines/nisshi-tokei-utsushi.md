# NTU: Nisshi, Tokei and Utsushi

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/apps/{nisshi,tokei,utsushi}`. Audited at `8e0acbe5`; line numbers refer to that commit.

56 files, 0 pending. 33 implicit machines: 6 High, 9 Medium, 18 Low.

## Machines

<a id="ntu-01"></a>

### NTU-01 — Nisshi system-log unlock (root reader through pkexec)
- **Where:** `desktop/aizome/apps/nisshi/src/app.rs:193` (`Nisshi.unlock`, `.root` :186, `.root_gen` :188, `.access` :190, `.user_access` :192, `.after_unlock` :195); `enum Unlock` `desktop/aizome/apps/nisshi/src/app.rs:43`; also `desktop/aizome/apps/nisshi/data/org.aizome.Nisshi.policy.in:17`
- **Encoding:** partially explicit: the `Unlock` enum (Locked/Asking/Unlocked/Refused) plus `root: Option<Client>`, `root_gen`, `access`/`user_access`, the deferred file `after_unlock`, and polkit's `auth_admin_keep` cached authorization outside the process.
- **States:** `Locked` → `Asking` → `Unlocked` | `Refused(why)` | `Locked` (cancelled); `Unlocked` → `Locked` (Lock, or the root reader went). Representable invalid: `Asking && root.is_none()` (sample mode, `desktop/aizome/apps/nisshi/src/app.rs:1185`); `after_unlock.is_some()` after the user moved to another scope; a root-read `Scope::File` still shown after the root reader died.
- **Transitions:** `start_root` Locked/Refused→Asking (`desktop/aizome/apps/nisshi/src/app.rs:405`); `Msg::Unlock` (`desktop/aizome/apps/nisshi/src/app.rs:1183`), auto-unlock from `OpenFile` (`desktop/aizome/apps/nisshi/src/app.rs:1207-1209`) and `Launched` (`desktop/aizome/apps/nisshi/src/app.rs:1339-1341`); root `Ready` Asking→Unlocked, access=System, cancel every read, replay `after_unlock`, reload (`desktop/aizome/apps/nisshi/src/app.rs:789-799`); root `Gone` Asking→Locked+notice when Cancelled (`desktop/aizome/apps/nisshi/src/app.rs:909`), Asking→Refused (`desktop/aizome/apps/nisshi/src/app.rs:910`), Unlocked→Locked+notice+reload as the user (`desktop/aizome/apps/nisshi/src/app.rs:912-920`); `Msg::Lock` → Locked, root requests dropped, File scope→All, reload (`desktop/aizome/apps/nisshi/src/app.rs:1189-1200`).
- **Hazards:** `after_unlock` is cleared only by `Gone` and `Launched` (`desktop/aizome/apps/nisshi/src/app.rs:907`, `desktop/aizome/apps/nisshi/src/app.rs:1332`), never by a later scope change; `Gone` from an unlocked root does not leave a root-read file as `Lock` does (`desktop/aizome/apps/nisshi/src/app.rs:1196-1198` vs `desktop/aizome/apps/nisshi/src/app.rs:915-920`); a late user `Ready` overwrites `access` with `user_access` even while Unlocked (`desktop/aizome/apps/nisshi/src/app.rs:801-802`); polkit keeps the authorization for minutes, so a second unlock can pass through `Asking` without a prompt.
- **Reify:** High — privilege boundary, six fields, eight mutation sites.
- **Suspected bug:** open an unreadable /var/log file: `read_file` sets `after_unlock` and the password dialog opens; while it is up the user picks All Messages; on authenticating, root `Ready` takes `after_unlock` and switches the window back to the file the user left (`desktop/aizome/apps/nisshi/src/app.rs:524`, `desktop/aizome/apps/nisshi/src/app.rs:795-797`). (confidence: medium)
- **Related:** helper `desktop/aizome/apps/nisshi/src/bin/helper.rs` (NTU-12); polkit agent dialog `desktop/aizome/crates/aizome-shell/src/auth.rs`.

<a id="ntu-02"></a>

### NTU-02 — Nisshi reader generations and pending-request table
- **Where:** `desktop/aizome/apps/nisshi/src/app.rs:189` (`Nisshi.pending: HashMap<(u64, u64), Want>`, `.user_gen` :187, `.root_gen` :188, `.user` :185); `enum Want` `desktop/aizome/apps/nisshi/src/app.rs:169`
- **Encoding:** a generation per reader start; map from (generation, request id) to purpose; `reader()` picks root only when `Unlocked` (`desktop/aizome/apps/nisshi/src/app.rs:382-390`).
- **States:** per request `Sent` → `Answering` → `Done` | `Failed` | `Cancelled` (removed by `stop_reads`) | `Orphaned` (reader gone); per reader `None` → `Started(gen)` → `Gone`.
- **Transitions:** `start_user` generation (`desktop/aizome/apps/nisshi/src/app.rs:393`); `start_root` (`desktop/aizome/apps/nisshi/src/app.rs:402`); inserts (`desktop/aizome/apps/nisshi/src/app.rs:430`, `desktop/aizome/apps/nisshi/src/app.rs:454`, `desktop/aizome/apps/nisshi/src/app.rs:481`, `desktop/aizome/apps/nisshi/src/app.rs:507-508`, `desktop/aizome/apps/nisshi/src/app.rs:529`, `desktop/aizome/apps/nisshi/src/app.rs:777`); `stop_reads` remove+cancel (`desktop/aizome/apps/nisshi/src/app.rs:489`, `desktop/aizome/apps/nisshi/src/app.rs:498`); stale-generation drop (`desktop/aizome/apps/nisshi/src/app.rs:782-785`); `Done` (`desktop/aizome/apps/nisshi/src/app.rs:863`); `Failed` (`desktop/aizome/apps/nisshi/src/app.rs:889`); `Gone` (`desktop/aizome/apps/nisshi/src/app.rs:903`); `Lock` (`desktop/aizome/apps/nisshi/src/app.rs:1193`).
- **Hazards:** the user reader starts only in `launched` (`desktop/aizome/apps/nisshi/src/app.rs:979`): after its `Gone` (`desktop/aizome/apps/nisshi/src/app.rs:922`) nothing restarts it and Reload or Lock read nothing; a `Want::Follow` that ends (`desktop/aizome/apps/nisshi/src/app.rs:886`) or fails (`desktop/aizome/apps/nisshi/src/app.rs:898`) is never re-armed while the toolbar still says Live; with `boots` still empty the dropdown's "This Boot" (index 0) maps to `Boot::All` (`desktop/aizome/apps/nisshi/src/app.rs:1103-1106`).
- **Reify:** Medium — the generation scheme is sound, but purpose and liveness live at about twelve sites.
- **Related:** ids issued by `desktop/aizome/apps/nisshi/src/backend.rs:145` (NTU-11).

<a id="ntu-03"></a>

### NTU-03 — Nisshi journal read pipeline: newest page, earlier pages, follow, whole-boot search
- **Where:** `desktop/aizome/apps/nisshi/src/app.rs:228` (`Nisshi.loading`, `.journal_search` :210, `.live_on` :214, `.store` :200); `Store.complete` `desktop/aizome/apps/nisshi/src/model.rs:21`; `Want::Load/Older/Follow` `desktop/aizome/apps/nisshi/src/app.rs:171-173`
- **Encoding:** one `loading` bool shared by every kind of read; `store.complete`; `journal_search: Option<String>` drives a self-continuing chain of `Older` pages; `live_on` and `boot` gate following.
- **States:** `Idle` → `Loading` → `Following` (Live, current or all boots) | `Paused`; `Paging` (Older); `SearchingBoot` → `Searched`. Representable invalid: `!loading` with a `Want::Load` pending; `journal_search.is_some()` with no `Older` in flight and `!store.complete`.
- **Transitions:** `load()` cancel, clear, ask (`desktop/aizome/apps/nisshi/src/app.rs:413-431`); `Done(Load)` complete, follow, continue search (`desktop/aizome/apps/nisshi/src/app.rs:864-872`); `load_older` (`desktop/aizome/apps/nisshi/src/app.rs:438-457`); `Done(Older)` (`desktop/aizome/apps/nisshi/src/app.rs:873-880`); `follow()` (`desktop/aizome/apps/nisshi/src/app.rs:461-483`); `Msg::Live` (`desktop/aizome/apps/nisshi/src/app.rs:1117-1128`); `SearchJournal` (`desktop/aizome/apps/nisshi/src/app.rs:1139-1140`); `StopSearchingJournal` (`desktop/aizome/apps/nisshi/src/app.rs:1144-1145`); `set_search` drops the boot search when the text differs (`desktop/aizome/apps/nisshi/src/app.rs:697-699`); empty journal → complete+follow (`desktop/aizome/apps/nisshi/src/app.rs:891-896`).
- **Hazards:** `Done(File)` (`desktop/aizome/apps/nisshi/src/app.rs:882`) and any `Failed`, including `Want::Fields` and `Boots` (`desktop/aizome/apps/nisshi/src/app.rs:890`), clear the shared `loading` mid-load; an `Older` failure leaves `journal_search` set with nothing in flight and the chip reading "Searching the whole boot…" (`desktop/aizome/apps/nisshi/src/ui/view.rs:231`); follow is not retried (NTU-02).
- **Reify:** High — four fields, about fifteen sites, user-visible progress.
- **Suspected bug:** select a row while the 50,000-entry newest page is still streaming; its fields read fails (cursor vacuumed, or "too many reads at once") and `Failed` sets `loading = false` with `Want::Load` still pending: "Reading…" vanishes, Read Earlier enables (`desktop/aizome/apps/nisshi/src/ui/menus.rs:95`), and a pending `--cursor` gives up early (NTU-06) (`desktop/aizome/apps/nisshi/src/app.rs:889-890`). (confidence: medium)

<a id="ntu-04"></a>

### NTU-04 — Nisshi log-file scope and its read
- **Where:** `desktop/aizome/apps/nisshi/src/app.rs:202` (`Nisshi.file_store`, `.files` :201); `Scope::File` `desktop/aizome/apps/nisshi/src/filter.rs:32`; `Want::File` `desktop/aizome/apps/nisshi/src/app.rs:177`
- **Encoding:** the scope variant chooses `file_store` over `store`; a pending `Want::File(path)`; `FileInfo.readable` routes to `after_unlock` (NTU-01).
- **States:** `Journal` → `FileReading(path)` → `FileShown` | `FileFailed` | `WaitingForUnlock(path)` → `Journal`.
- **Transitions:** `set_scope` (`desktop/aizome/apps/nisshi/src/app.rs:704-717`); `read_file` (`desktop/aizome/apps/nisshi/src/app.rs:512-532`); entries kept only for the current file (`desktop/aizome/apps/nisshi/src/app.rs:840-843`); `Done(File)` (`desktop/aizome/apps/nisshi/src/app.rs:881-885`); boot change forces All (`desktop/aizome/apps/nisshi/src/app.rs:1110-1112`); `Lock` forces All (`desktop/aizome/apps/nisshi/src/app.rs:1196-1198`); `Launched --file` (`desktop/aizome/apps/nisshi/src/app.rs:1337-1342`); `Reload` (`desktop/aizome/apps/nisshi/src/app.rs:1310-1313`).
- **Hazards:** leaving a file shows `store` as it was (`desktop/aizome/apps/nisshi/src/app.rs:714-716`), so a hand-off with both `--file` and `--boot` (`desktop/aizome/apps/nisshi/src/app.rs:1335-1338`) later shows the old boot's messages under the new boot's name; at startup `read_file` runs before the Files answer, so an unreadable file is read as the user and fails instead of offering to unlock (`desktop/aizome/apps/nisshi/src/app.rs:522`, `desktop/aizome/apps/nisshi/src/app.rs:806-809`).
- **Reify:** Medium — split across the scope, two stores and the unlock deferral.

<a id="ntu-05"></a>

### NTU-05 — Nisshi following the newest message, and the unseen count
- **Where:** `desktop/aizome/apps/nisshi/src/app.rs:216` (`Nisshi.following`, `.unseen` :218, `.selected` :212)
- **Encoding:** a bool plus selection emptiness plus a counter; `reveal()` derives the scroll target (`desktop/aizome/apps/nisshi/src/app.rs:601-607`).
- **States:** `AtNewest` → `Held` (a selection, context or cursor; `unseen` counts) → `AtNewest`.
- **Transitions:** `select` clears (`desktop/aizome/apps/nisshi/src/app.rs:634`); `context` (`desktop/aizome/apps/nisshi/src/app.rs:738`); `take_cursor` (`desktop/aizome/apps/nisshi/src/app.rs:749`); `load` sets (`desktop/aizome/apps/nisshi/src/app.rs:418`); Live on (`desktop/aizome/apps/nisshi/src/app.rs:1121`); JumpToNewest (`desktop/aizome/apps/nisshi/src/app.rs:1132`); follow entries count (`desktop/aizome/apps/nisshi/src/app.rs:834-836`); Refresh clears the count when following (`desktop/aizome/apps/nisshi/src/app.rs:1006-1008`).
- **Hazards:** emptying the selection never restores `following`; `Live(false)` leaves `following` set; `unseen` counts only follow entries.
- **Reify:** Medium — three fields and seven sites, but guarded and cosmetic.

<a id="ntu-06"></a>

### NTU-06 — Nisshi open-on-cursor pending selection
- **Where:** `desktop/aizome/apps/nisshi/src/app.rs:197` (`Nisshi.want_cursor`)
- **Encoding:** `Option<String>` checked on every Refresh by `take_cursor` (`desktop/aizome/apps/nisshi/src/app.rs:743-759`), abandoned once `loading` is false.
- **States:** `None` → `Wanted(cursor)` → `Selected` | `GaveUp(notice)`.
- **Transitions:** `take_request` (`desktop/aizome/apps/nisshi/src/app.rs:366-368`); `context` (`desktop/aizome/apps/nisshi/src/app.rs:732`); hand-off clears then sets (`desktop/aizome/apps/nisshi/src/app.rs:1331`, `desktop/aizome/apps/nisshi/src/app.rs:1336`); found (`desktop/aizome/apps/nisshi/src/app.rs:748-750`); give up (`desktop/aizome/apps/nisshi/src/app.rs:752-755`); checked at Refresh (`desktop/aizome/apps/nisshi/src/app.rs:1009`) and hand-off (`desktop/aizome/apps/nisshi/src/app.rs:1348`).
- **Hazards:** relies on NTU-03's `loading`, which other reads clear early; searches only what is read, never continues with earlier pages.
- **Reify:** Low — one field, but coupled to NTU-03.

<a id="ntu-07"></a>

### NTU-07 — Nisshi search typing debounce
- **Where:** `desktop/aizome/apps/nisshi/src/app.rs:206` (`Nisshi.search_due`, `.typed` :207)
- **Encoding:** a keystroke counter, an `Option<u64>` naming the keystroke due, and `cx.after(80 ms, ApplySearch(n))`, past `LARGE` messages only.
- **States:** `Applied` → `Due(n)` → `Applied`.
- **Transitions:** `Msg::Search` defers (`desktop/aizome/apps/nisshi/src/app.rs:1021-1024`) or applies (`desktop/aizome/apps/nisshi/src/app.rs:1026-1027`); `ApplySearch` (`desktop/aizome/apps/nisshi/src/app.rs:1031-1033`); `SubmitSearch` (`desktop/aizome/apps/nisshi/src/app.rs:1037-1038`); cleared by context (`desktop/aizome/apps/nisshi/src/app.rs:726`), Clear Filters (`desktop/aizome/apps/nisshi/src/app.rs:1075`), hand-off (`desktop/aizome/apps/nisshi/src/app.rs:1329`).
- **Hazards:** while due, `search_text` is new and `filter.search` old: completions (`desktop/aizome/apps/nisshi/src/app.rs:564`) and `SearchJournal` (`desktop/aizome/apps/nisshi/src/app.rs:1139`) mix the two.
- **Reify:** Low.

<a id="ntu-08"></a>

### NTU-08 — Nisshi view refresh coalescing and incremental extension
- **Where:** `desktop/aizome/apps/nisshi/src/app.rs:232` (`Nisshi.refresh_due`, `.full_refresh` :233); `View.seen` `desktop/aizome/apps/nisshi/src/model.rs:197`; `Store.lowest/highest` `desktop/aizome/apps/nisshi/src/model.rs:18-19`
- **Encoding:** due flag, full-or-partial flag and a 30/90 ms `cx.after`; the view's `seen` watermark is valid only while the store has been appended to, never prepended.
- **States:** `Clean` → `Due(partial)` → `Due(full)` → `Clean`.
- **Transitions:** `changed` (`desktop/aizome/apps/nisshi/src/app.rs:536-539`); `rebuild` clears full (`desktop/aizome/apps/nisshi/src/app.rs:545`); `Refresh` rebuilds when `full_refresh || view.seen > len` else extends (`desktop/aizome/apps/nisshi/src/app.rs:993-999`, `desktop/aizome/apps/nisshi/src/model.rs:230-240`); `load` sets full (`desktop/aizome/apps/nisshi/src/app.rs:434`).
- **Hazards:** `View::extend` is correct only because every prepend path passes `full = true` (`desktop/aizome/apps/nisshi/src/app.rs:819`, `desktop/aizome/apps/nisshi/src/app.rs:828`): an unenforced invariant between `Store::prepend` (`desktop/aizome/apps/nisshi/src/model.rs:33`) and the flag; the interval depends on NTU-03's `loading`.
- **Reify:** Medium — the watermark invariant crosses two files.

<a id="ntu-09"></a>

### NTU-09 — Nisshi selected message's fields fetch
- **Where:** `desktop/aizome/apps/nisshi/src/app.rs:226` (`Nisshi.fields: Option<(u64, Vec<…>)>`, `.selected` :212); `Want::Fields` `desktop/aizome/apps/nisshi/src/app.rs:174`
- **Encoding:** delayed `Fetch(seq)` (120 ms) and `Deepen(seq)` (30 ms) guarded by `selected == [seq]`; a pending fields read; the view checks the seq (`desktop/aizome/apps/nisshi/src/ui/detail.rs:87`).
- **States:** `Nothing` → `Selected(seq)` → `Fetching(seq)` → `Fields(seq)`.
- **Transitions:** `select` (`desktop/aizome/apps/nisshi/src/app.rs:636-645`); `fetch` (`desktop/aizome/apps/nisshi/src/app.rs:762-779`); `Event::Fields` (`desktop/aizome/apps/nisshi/src/app.rs:847-851`); `after_view` clears (`desktop/aizome/apps/nisshi/src/app.rs:556-558`).
- **Hazards:** an answer arriving after the selection moved is stored under its old seq (`desktop/aizome/apps/nisshi/src/app.rs:848-850`) and hidden only by the view's check; deselecting doesn't cancel the read.
- **Reify:** Low.

<a id="ntu-10"></a>

### NTU-10 — Nisshi timeline brush drag
- **Where:** `desktop/aizome/apps/nisshi/src/app.rs:231` (`Nisshi.brush: Option<(f64, f64)>`); `enum Brush` `desktop/aizome/apps/nisshi/src/app.rs:54`; `desktop/aizome/apps/nisshi/src/ui/timeline.rs:47-50`
- **Encoding:** anchor and current fraction; the canvas forwards Move and Up only while `brush.is_some()`.
- **States:** `Idle` → `Dragging(a, b)` → `Idle` (a press selects the nearest message, a drag adds a Range chip).
- **Transitions:** Down (`desktop/aizome/apps/nisshi/src/app.rs:1150`); Move (`desktop/aizome/apps/nisshi/src/app.rs:1151-1155`); Up takes (`desktop/aizome/apps/nisshi/src/app.rs:1157`), press (`desktop/aizome/apps/nisshi/src/app.rs:1158-1171`), range (`desktop/aizome/apps/nisshi/src/app.rs:1173-1175`).
- **Hazards:** no cancel; an Up the canvas never sees leaves `brush` set and later moves extend it; fractions map through the histogram as it is at Up, which live messages may have widened since Down.
- **Reify:** Low.

<a id="ntu-11"></a>

### NTU-11 — Nisshi reader client process (helper as the user, or through pkexec as root)
- **Where:** `desktop/aizome/apps/nisshi/src/backend.rs:91` (`Client.stdin: Mutex<Option<ChildStdin>>` :93, `.kinds` :94, `.next` :95); reader-thread locals `ready` :210, `pending` :213, `files` :214, `last_time` :215
- **Encoding:** stdin Some/None for open/closed input; a thread-local `ready`; an id→Kind map shared with the thread; exit code plus stderr text classify the end.
- **States:** `Spawning` → `SpawnFailed` (Gone at once) | `Running` → `Ready` → `InputClosed` (stop, drop, write error) → `Exited` → `Gone(Cancelled | Denied | Unavailable | Failed)`.
- **Transitions:** spawn (`desktop/aizome/apps/nisshi/src/backend.rs:103-105`), failure (`desktop/aizome/apps/nisshi/src/backend.rs:109`); thread (`desktop/aizome/apps/nisshi/src/backend.rs:118`); `Ready` frame (`desktop/aizome/apps/nisshi/src/backend.rs:232-235`); write error drops input (`desktop/aizome/apps/nisshi/src/backend.rs:158-160`); `cancel` (`desktop/aizome/apps/nisshi/src/backend.rs:185-188`); `stop` (`desktop/aizome/apps/nisshi/src/backend.rs:192-194`); `Drop` closes input and never kills (`desktop/aizome/apps/nisshi/src/backend.rs:197-204`); `Done`/`Failed` frames (`desktop/aizome/apps/nisshi/src/backend.rs:262-276`); end classified (`desktop/aizome/apps/nisshi/src/backend.rs:133-140`, `desktop/aizome/apps/nisshi/src/backend.rs:348-366`).
- **Hazards:** `Ready.version` is never compared with `VERSION` (`desktop/aizome/apps/nisshi/src/protocol.rs:24`), though "a session can outlive an update"; a cancelled `Files` request leaves its `files` entry behind (`desktop/aizome/apps/nisshi/src/backend.rs:257-266`); after a write error requests are taken and dropped until the exit is noticed; pkexec's outcome is matched on exact C-locale text and exit code (`desktop/aizome/apps/nisshi/src/backend.rs:355-364`, `desktop/aizome/apps/nisshi/src/backend.rs:399`).
- **Reify:** Medium — linear, but its phases sit in a mutex Option, a thread-local and exit-text parsing.
- **Related:** `desktop/aizome/apps/nisshi/src/bin/helper.rs` (NTU-12); frames `desktop/aizome/apps/nisshi/src/protocol.rs:87`.

<a id="ntu-12"></a>

### NTU-12 — Nisshi helper's running reads and process tree
- **Where:** `desktop/aizome/apps/nisshi/src/bin/helper.rs:59` (`running: HashMap<u64, Arc<Mutex<Option<Child>>>>`, type :27); cap `RUNNING` :23
- **Encoding:** presence in `running` means live and cancellable; absence at completion means cancelled (`desktop/aizome/apps/nisshi/src/bin/helper.rs:251`); `PR_SET_PDEATHSIG` on the helper (`desktop/aizome/apps/nisshi/src/bin/helper.rs:40`) and on each journalctl with a parent check (`desktop/aizome/apps/nisshi/src/bin/helper.rs:182-185`); stdin EOF ends everything.
- **States:** per request `Rejected` (cap, validation) | `Running(child)` → `Done` | `Failed` | `Cancelled` (killed, reported Done); helper `Ready sent` → `Serving` → `Draining` (stdin EOF) → `Exited`.
- **Transitions:** Ready (`desktop/aizome/apps/nisshi/src/bin/helper.rs:57`); Cancel remove+kill (`desktop/aizome/apps/nisshi/src/bin/helper.rs:73-79`); cap (`desktop/aizome/apps/nisshi/src/bin/helper.rs:81-84`); `run` registers (`desktop/aizome/apps/nisshi/src/bin/helper.rs:197`); reader thread waits, removes, reports (`desktop/aizome/apps/nisshi/src/bin/helper.rs:250-261`); Files and Read threads (`desktop/aizome/apps/nisshi/src/bin/helper.rs:96-112`); EOF drain-kill (`desktop/aizome/apps/nisshi/src/bin/helper.rs:118-122`); writer join (`desktop/aizome/apps/nisshi/src/bin/helper.rs:123-124`).
- **Hazards:** Files and Read threads aren't in `running`, so they can't be cancelled and don't count against `RUNNING`; the slot mutex is held across `Child::wait` (`desktop/aizome/apps/nisshi/src/bin/helper.rs:250`) while Cancel and the drain hold `running` and wait for the slot (`desktop/aizome/apps/nisshi/src/bin/helper.rs:74-75`, `desktop/aizome/apps/nisshi/src/bin/helper.rs:118-119`), stalling the request loop if a journalctl lingers after closing stdout; the `Option<Child>` is never set to None.
- **Reify:** Medium — privileged process, map plus two kinds of thread.
- **Related:** window side `desktop/aizome/apps/nisshi/src/backend.rs` (NTU-11).

<a id="ntu-13"></a>

### NTU-13 — (test harness) Nisshi stand-in journalctl
- **Where:** `desktop/aizome/apps/nisshi/src/fixture.rs:621` (`stage`), script :636-660
- **Encoding:** files in the stage directory: `asked` (each call's arguments, `desktop/aizome/apps/nisshi/src/fixture.rs:639`), `follow.pid` (written as a follow starts, `desktop/aizome/apps/nisshi/src/fixture.rs:652`), and the old/new JSON split.
- **States:** per call `Boots` | `Entry` | `EarlierPage` (always empty) | `NewestPage` | `Following` (`exec sleep 3600`) → killed.
- **Transitions:** `--list-boots` (`desktop/aizome/apps/nisshi/src/fixture.rs:641`); `--follow` (`desktop/aizome/apps/nisshi/src/fixture.rs:652`); one entry by cursor (`desktop/aizome/apps/nisshi/src/fixture.rs:654-656`); earlier page (`desktop/aizome/apps/nisshi/src/fixture.rs:658`); newest page (`desktop/aizome/apps/nisshi/src/fixture.rs:659`).
- **Hazards:** the follow answer ignores `--after-cursor`, and earlier pages are always empty, so Older paging is not exercised here.
- **Reify:** Low — fixture.

<a id="ntu-14"></a>

### NTU-14 — Single-instance socket claim and hand-off (Nisshi, Tokei, Utsushi)
- **Where:** `desktop/aizome/apps/nisshi/src/instance.rs:23` (`claim`), `desktop/aizome/apps/nisshi/src/instance.rs:54` (`serve`); `desktop/aizome/apps/tokei/src/instance.rs:30`, `desktop/aizome/apps/tokei/src/instance.rs:50`; `desktop/aizome/apps/utsushi/src/instance.rs:30`, `desktop/aizome/apps/utsushi/src/instance.rs:51`
- **Encoding:** the file `$XDG_RUNTIME_DIR/aizome-app.<name>.sock` and whether connecting to it works; a listener on a detached thread.
- **States:** `Handed` (connected and wrote) | `First(listener)` (stale file removed, bound) | `Alone` (bind failed); then `Serving` until the app's channel closes.
- **Transitions:** connect (`desktop/aizome/apps/nisshi/src/instance.rs:26`, `desktop/aizome/apps/tokei/src/instance.rs:32`, `desktop/aizome/apps/utsushi/src/instance.rs:32`); remove the stale file (`desktop/aizome/apps/nisshi/src/instance.rs:31`, `desktop/aizome/apps/tokei/src/instance.rs:39`, `desktop/aizome/apps/utsushi/src/instance.rs:40`); bind (`desktop/aizome/apps/nisshi/src/instance.rs:32`, `desktop/aizome/apps/tokei/src/instance.rs:40`, `desktop/aizome/apps/utsushi/src/instance.rs:41`); serve ends when sending fails (`desktop/aizome/apps/nisshi/src/instance.rs:62-63`, `desktop/aizome/apps/tokei/src/instance.rs:57-58`, `desktop/aizome/apps/utsushi/src/instance.rs:68-69`).
- **Hazards:** two launches at once both fail to connect, both remove the file, and the second unlinks the first's newly bound socket: two "first" instances, one unreachable; `Handed` is reported once bytes are written, whatever the receiver does; the socket file outlives the process (the next launch's refused connect cleans it).
- **Reify:** Low — tiny, but copied three times.
- **Related:** launcher `desktop/aizome/crates/aizome-shell/src/startup.rs:501` (`aizome-nisshi --launch`).

<a id="ntu-15"></a>

### NTU-15 — Nisshi `--print` one-shot read
- **Where:** `desktop/aizome/apps/nisshi/src/main.rs:25` (`query_id: Option<u64>`); loop `desktop/aizome/apps/nisshi/src/main.rs:27`
- **Encoding:** `query_id` None until Ready; a 120 s `recv_timeout` per event.
- **States:** `Starting` → `Queried(id)` → `Done` | `Empty` | `Failed` | `Gone` | `TimedOut`.
- **Transitions:** Ready → query (`desktop/aizome/apps/nisshi/src/main.rs:36`); entries (`desktop/aizome/apps/nisshi/src/main.rs:38`); done (`desktop/aizome/apps/nisshi/src/main.rs:39`); empty journal (`desktop/aizome/apps/nisshi/src/main.rs:40`); failure (`desktop/aizome/apps/nisshi/src/main.rs:41-44`); gone (`desktop/aizome/apps/nisshi/src/main.rs:45-48`); timeout leaves the loop (`desktop/aizome/apps/nisshi/src/main.rs:27`).
- **Hazards:** a timeout is treated as the end of the read.
- **Reify:** Low.
- **Suspected bug:** `aizome-nisshi --print --system` with the password dialog left unanswered for two minutes prints nothing and exits 0 (`desktop/aizome/apps/nisshi/src/main.rs:27`, `desktop/aizome/apps/nisshi/src/main.rs:64`), indistinguishable from an empty journal. (confidence: medium)

<a id="ntu-16"></a>

### NTU-16 — (test harness) Nisshi reader stage: stand-in pkexec, environment lock, polling waits
- **Where:** `desktop/aizome/apps/nisshi/tests/reader.rs:18` (`Stage`), pkexec script :36-49, `answer` file :53, `LOCK` :80
- **Encoding:** an `answer` file the stand-in pkexec reads on each unlock; process-wide environment variables serialised by `LOCK`; polls on `follow.pid` and `/proc/<pid>` (`desktop/aizome/apps/nisshi/tests/reader.rs:398-409`).
- **States:** pkexec `Cancel` (exit 126) | `Deny` (exit 127) | `Allow` (exec the helper); the test drives Locked→Asking→Locked→Refused→Unlocked→Locked (`desktop/aizome/apps/nisshi/tests/reader.rs:327-351`).
- **Transitions:** `stage.answer` (`desktop/aizome/apps/nisshi/tests/reader.rs:327`, `desktop/aizome/apps/nisshi/tests/reader.rs:333`, `desktop/aizome/apps/nisshi/tests/reader.rs:340`); environment set (`desktop/aizome/apps/nisshi/tests/reader.rs:67-72`, `desktop/aizome/apps/nisshi/tests/reader.rs:377-381`).
- **Hazards:** fixed 20 s and 5 s polls; pid reuse could make the `/proc` check pass falsely.
- **Reify:** Low — harness.

<a id="ntu-17"></a>

### NTU-17 — Tokei alarm schedule: alarms.json against systemd transient timers
- **Where:** `desktop/aizome/apps/tokei/src/alarms.rs:33` (`Alarm.enabled`); file `alarms_path` `desktop/aizome/apps/tokei/src/alarms.rs:148`; `sync` `desktop/aizome/apps/tokei/src/alarms.rs:194`; `Systemd` `desktop/aizome/apps/tokei/src/alarms.rs:211`; also `desktop/aizome/apps/tokei/src/main.rs:36-43`
- **Encoding:** the persisted `enabled` flag plus external user units `aizome-alarm-<id>.timer/.service` and `…-snooze` (transient, `--collect`), reconciled by `sync`; a once-alarm's calendar is a concrete date computed when scheduled (`desktop/aizome/apps/tokei/src/alarms.rs:122-125`).
- **States:** per alarm `Off` → `Armed` → `Ringing` (service runs `--ring`) → `Snoozed` | `Armed` (repeating) | `Off` (once, disabled by the ring); `Lapsed` at logout → `Armed` by `--sync`.
- **Transitions:** `sync` cancels the unwanted (`desktop/aizome/apps/tokei/src/alarms.rs:196-200`) and schedules the enabled (`desktop/aizome/apps/tokei/src/alarms.rs:202-206`); `Systemd::schedule` stops timer and service, then `systemd-run` (`desktop/aizome/apps/tokei/src/alarms.rs:265-269`); `cancel` (`desktop/aizome/apps/tokei/src/alarms.rs:271-274`); `snooze` (`desktop/aizome/apps/tokei/src/alarms.rs:276-291`); login `--sync` (`desktop/aizome/apps/tokei/src/main.rs:36-43`, unit `desktop/aizome/nix/module.nix:305`); Tokei's launch (`desktop/aizome/apps/tokei/src/app.rs:451`).
- **Hazards:** `stop` includes `<unit>.service` (`desktop/aizome/apps/tokei/src/alarms.rs:259`), so rescheduling a ringing alarm kills its ring; `sync`'s documented "unless it's a once-alarm whose time has gone" (`desktop/aizome/apps/tokei/src/alarms.rs:192-193`) isn't implemented: a once-alarm re-arms for its next occurrence until a ring disables it; `scheduled()` hides snooze timers (`desktop/aizome/apps/tokei/src/alarms.rs:311`).
- **Reify:** High — state split between a JSON file, systemd and two other writers (NTU-19, NTU-24).
- **Suspected bug:** a once-alarm rings; the user opens Tokei (the notification names Tokei's desktop entry) → `launched` → `sync_alarms` → `systemctl --user stop aizome-alarm-<id>.timer aizome-alarm-<id>.service` kills `--ring` before it turns the alarm off, then `schedule` arms it for the same time tomorrow; the resident notification's Snooze and Stop now reach nothing (`desktop/aizome/apps/tokei/src/app.rs:451`, `desktop/aizome/apps/tokei/src/alarms.rs:259`, `desktop/aizome/apps/tokei/src/alarms.rs:266-268`). (confidence: high)
- **Related:** systemd user manager; `desktop/aizome/nix/module.nix:305` (`aizome-tokei-alarms`); ring `desktop/aizome/apps/tokei/src/ring.rs` (NTU-24).

<a id="ntu-18"></a>

### NTU-18 — (test harness) Tokei recording scheduler and fake-clock rig
- **Where:** `desktop/aizome/apps/tokei/src/alarms.rs:320` (`Recorder.log: Rc<RefCell<Vec<String>>>`), `units` :335; `desktop/aizome/apps/tokei/tests/tokei.rs:24` (`Rig`), `wait` :53; `Scripted` `desktop/aizome/apps/tokei/src/ring.rs:214`
- **Encoding:** a call log replayed into a unit set (`systemd-run` adds, `stop` removes); a fake `(Timestamp, Duration)` cell stepped with the harness timers in 10 ms steps; a one-shot scripted answer.
- **States:** units scheduled by replay; the rig's clock.
- **Transitions:** `schedule` (`desktop/aizome/apps/tokei/src/alarms.rs:352-360`), `cancel` (`desktop/aizome/apps/tokei/src/alarms.rs:362-364`), `snooze` (`desktop/aizome/apps/tokei/src/alarms.rs:366-369`); `Rig::wait` (`desktop/aizome/apps/tokei/tests/tokei.rs:53-62`); `Scripted::answer` takes (`desktop/aizome/apps/tokei/src/ring.rs:226-228`).
- **Hazards:** `Recorder::cancel` stops only the main unit, whereas `Systemd::cancel` also stops `-snooze` (`desktop/aizome/apps/tokei/src/alarms.rs:271-274`), and services aren't modelled, so NTU-17's ring kill can't show in tests.
- **Reify:** Low — harness.

<a id="ntu-19"></a>

### NTU-19 — Tokei's in-memory alarm list, its saves and the undo slot
- **Where:** `desktop/aizome/apps/tokei/src/app.rs:180` (`Tokei.alarms`, `.selected_alarm` :184, `.deleted` :185, `.schedule_error` :187)
- **Encoding:** loaded once in `new` (`desktop/aizome/apps/tokei/src/app.rs:220`) and written whole by `save_alarms` (`desktop/aizome/apps/tokei/src/app.rs:324-330`) on every change; `deleted: Option<(index, Alarm)>`.
- **States:** `InSync` → `Stale` (a ring process changed the file) → `Overwritten` (the next save); `Deleted(i, a)` → `Undone`.
- **Transitions:** load (`desktop/aizome/apps/tokei/src/app.rs:220`); toggle (`desktop/aizome/apps/tokei/src/app.rs:529-535`); delete (`desktop/aizome/apps/tokei/src/app.rs:536-548`); undo (`desktop/aizome/apps/tokei/src/app.rs:549-557`); save from the sheet (`desktop/aizome/apps/tokei/src/app.rs:618-645`); `reschedule` (`desktop/aizome/apps/tokei/src/app.rs:333-343`).
- **Hazards:** never re-read, so a ring's `enabled = false` (`desktop/aizome/apps/tokei/src/ring.rs:94-95`) is undone by the next unrelated save; undo reinserts at a stale index without re-sorting; `selected_alarm` is an index.
- **Reify:** High — two processes write one file with no reload, merge or lock.
- **Suspected bug:** Tokei stays open while a once-alarm rings and is stopped (file now `enabled: false`); the window still shows it on with "Next: … tomorrow"; toggling any other alarm saves the stale list with it enabled, and the next launch or login `--sync` arms it again (`desktop/aizome/apps/tokei/src/app.rs:220`, `desktop/aizome/apps/tokei/src/app.rs:529-533`, `desktop/aizome/apps/tokei/src/app.rs:324-330`). (confidence: high)
- **Related:** `desktop/aizome/apps/tokei/src/ring.rs` (NTU-24); NTU-17.

<a id="ntu-20"></a>

### NTU-20 — Tokei sheets: Add City and the alarm editor
- **Where:** `desktop/aizome/apps/tokei/src/app.rs:195` (`Tokei.sheet: Option<Sheet>`); `enum Sheet` `desktop/aizome/apps/tokei/src/app.rs:157` (`Alarm { draft, index: Option<usize>, time, error }`)
- **Encoding:** partially explicit: the enum carries the draft; the kit's sheet window key; `index` into `alarms`; the shared `player` for previews.
- **States:** `None` → `AddCity{query, results, picked}` | `Alarm{draft, index, error}` → `None`.
- **Transitions:** open (`desktop/aizome/apps/tokei/src/app.rs:368-369`, `desktop/aizome/apps/tokei/src/app.rs:492-493`); query and pick (`desktop/aizome/apps/tokei/src/app.rs:497-510`); add (`desktop/aizome/apps/tokei/src/app.rs:511-525`); draft edits (`desktop/aizome/apps/tokei/src/app.rs:558-617`); save (`desktop/aizome/apps/tokei/src/app.rs:618-645`); cancel (`desktop/aizome/apps/tokei/src/app.rs:646-649`); delete closes it (`desktop/aizome/apps/tokei/src/app.rs:544-546`).
- **Hazards:** `SaveAlarm` writes `alarms[index]` (`desktop/aizome/apps/tokei/src/app.rs:626-628`), so anything that shifts indices under an open sheet makes it overwrite another alarm; the sound preview (`desktop/aizome/apps/tokei/src/app.rs:595`, `desktop/aizome/apps/tokei/src/app.rs:604`) and the timer's "complete" sound (`desktop/aizome/apps/tokei/src/app.rs:692`) share one player and stop each other; `error` and `time` must agree (re-checked at `desktop/aizome/apps/tokei/src/app.rs:620`).
- **Reify:** Medium — partially explicit; identity by index is the weak point.
- **Suspected bug:** delete alarm D, open Edit on X, choose Edit ▸ Undo Delete (still in the menu bar) → D is reinserted before X; Save then writes X's draft over the alarm now at X's old index, dropping it from the file while its timer stays armed (`desktop/aizome/apps/tokei/src/app.rs:549-552`, `desktop/aizome/apps/tokei/src/app.rs:625-628`). (confidence: low — needs the menu bar live while the sheet is up)

<a id="ntu-21"></a>

### NTU-21 — Tokei timer end check and tick phase
- **Where:** `desktop/aizome/apps/tokei/src/app.rs:190` (`Tokei.timer_end: Option<TimerHandle>`, `.ticker` :192, `.timer` :188); `TimerState` `desktop/aizome/apps/tokei/src/timing.rs:15`
- **Encoding:** explicit `TimerState`, plus app-held handles that must exist only while `Running`; ticks move onto the timer's seconds while the Timer page shows (`desktop/aizome/apps/tokei/src/app.rs:297-302`); a `rephase` flag (`desktop/aizome/apps/tokei/src/app.rs:459-460`).
- **States:** `Ready` → `Running(ends)` with end handle → `Paused(left)` without → `Running` → `Done` (sound, notification).
- **Transitions:** `timer_start` (`desktop/aizome/apps/tokei/src/app.rs:391-399`); `timer_stop_waiting` (`desktop/aizome/apps/tokei/src/app.rs:401-405`); `set_timer` (`desktop/aizome/apps/tokei/src/app.rs:407-412`); pause or resume (`desktop/aizome/apps/tokei/src/app.rs:661-676`); cancel (`desktop/aizome/apps/tokei/src/app.rs:677-681`); end check, done or re-armed (`desktop/aizome/apps/tokei/src/app.rs:682-698`); `tick` (`desktop/aizome/apps/tokei/src/app.rs:289-307`).
- **Hazards:** the monotonic clock (`desktop/aizome/apps/tokei/src/clock.rs:46`, `Instant`) stops during suspend, so a timer ends late by the suspend's length; a running timer is dropped silently when the window closes (only its length is kept, `desktop/aizome/apps/tokei/src/app.rs:849-853`).
- **Reify:** Low — the core is explicit; the handle coupling is local.

<a id="ntu-22"></a>

### NTU-22 — Tokei stopwatch run state and frame timer
- **Where:** `desktop/aizome/apps/tokei/src/timing.rs:97` (`Stopwatch.since: Option<Duration>`, `.banked` :95, `.laps`); `desktop/aizome/apps/tokei/src/app.rs:194` (`Tokei.frames`)
- **Encoding:** `since` Some means running, `banked` non-zero means started; a 40 ms `cx.every` handle kept while running on the Stopwatch page.
- **States:** `Zero` → `Running` → `Stopped(banked)` → `Running` | `Zero`.
- **Transitions:** `start` (`desktop/aizome/apps/tokei/src/timing.rs:111-115`), `stop` (`desktop/aizome/apps/tokei/src/timing.rs:117-121`), `lap` (`desktop/aizome/apps/tokei/src/timing.rs:123-127`), `reset` (`desktop/aizome/apps/tokei/src/timing.rs:129-131`); `frames()` (`desktop/aizome/apps/tokei/src/app.rs:310-320`) from Page (`desktop/aizome/apps/tokei/src/app.rs:468`) and WatchToggle (`desktop/aizome/apps/tokei/src/app.rs:707`).
- **Hazards:** the frame handle is recomputed only on those two messages; same suspend gap as NTU-21.
- **Reify:** Low.

<a id="ntu-23"></a>

### NTU-23 — Tokei state-file persist debounce
- **Where:** `desktop/aizome/apps/tokei/src/app.rs:198` (`Tokei.persist_seq`)
- **Encoding:** a counter and `cx.after(600 ms, Persist(seq))`.
- **States:** `Saved` → `Due(seq)` → `Saved`.
- **Transitions:** `persist_soon` (`desktop/aizome/apps/tokei/src/app.rs:273-278`); `Persist` (`desktop/aizome/apps/tokei/src/app.rs:720-724`); close saves at once (`desktop/aizome/apps/tokei/src/app.rs:850`).
- **Hazards:** none observed.
- **Reify:** Low.

<a id="ntu-24"></a>

### NTU-24 — Tokei alarm ring process (`aizome-tokei --ring`)
- **Where:** `desktop/aizome/apps/tokei/src/ring.rs:45` (`ring_with`: snapshot `alarms` :53, `posted` :63, loop :65); `Dbus` notifier `desktop/aizome/apps/tokei/src/ring.rs:105` (`.id`, `.rx`)
- **Encoding:** local variables of one process run; the alarms file snapshot taken at start; D-Bus signal threads feeding a channel filtered by notification id.
- **States:** `Loaded` → `Skipped` (gone or off) | `Posted`/`Unposted` → `Ringing` (sound restarted when it ends) → `Snoozed` | `Stopped` | `TimedOut` (10 min, or 1 min without a notification) → once-alarm turned off and saved.
- **Transitions:** skip (`desktop/aizome/apps/tokei/src/ring.rs:54-60`); post (`desktop/aizome/apps/tokei/src/ring.rs:63`); ring loop (`desktop/aizome/apps/tokei/src/ring.rs:65-77`); snooze (`desktop/aizome/apps/tokei/src/ring.rs:80-85`); off and save (`desktop/aizome/apps/tokei/src/ring.rs:91-98`); `Dbus::post` subscribes before Notify (`desktop/aizome/apps/tokei/src/ring.rs:139-158`), keeps the id (`desktop/aizome/apps/tokei/src/ring.rs:173`), forwards the first answer, dismissal as Stop (`desktop/aizome/apps/tokei/src/ring.rs:176-182`).
- **Hazards:** writes the start-time snapshot back whole (`desktop/aizome/apps/tokei/src/ring.rs:95`); on timeout the resident notification stays up after the process exits, its buttons dead; NTU-17's `stop` can kill it mid-ring with no cleanup.
- **Reify:** Medium — linear, but its file write races the window.
- **Suspected bug:** with Tokei open, a once-alarm starts ringing; the user adds alarm C (file now has C, C's timer armed), then presses Stop → `ring_with` saves its snapshot without C (`desktop/aizome/apps/tokei/src/ring.rs:53`, `desktop/aizome/apps/tokei/src/ring.rs:95`); at C's time `--ring C` logs "alarm C is gone" and stays silent. (confidence: medium)
- **Related:** notification server `desktop/aizome/crates/aizome-shell/src/notifications/server.rs`; NTU-17, NTU-19.

<a id="ntu-25"></a>

### NTU-25 — Tokei sound player child process
- **Where:** `desktop/aizome/apps/tokei/src/sound.rs:128` (`System.child: Option<Child>`); tone cache `tone_file` `desktop/aizome/apps/tokei/src/sound.rs:107`
- **Encoding:** an optional child; `try_wait` decides whether it plays.
- **States:** `Idle` → `Playing(child)` → `Finished` (kept until the next start or stop) → `Idle`.
- **Transitions:** `start` stops then spawns (`desktop/aizome/apps/tokei/src/sound.rs:132-149`); `playing` (`desktop/aizome/apps/tokei/src/sound.rs:151-156`); `stop` kills and waits (`desktop/aizome/apps/tokei/src/sound.rs:158-163`); `Drop` (`desktop/aizome/apps/tokei/src/sound.rs:166-170`).
- **Hazards:** the tone WAV is written once, not atomically (`desktop/aizome/apps/tokei/src/sound.rs:110-111`); a write cut short leaves a truncated file reused for good.
- **Reify:** Low.

<a id="ntu-26"></a>

### NTU-26 — Utsushi playback item lifecycle
- **Where:** `desktop/aizome/apps/utsushi/src/app.rs:157` (`Utsushi.status`, `.queue` :151, `.current` :152, `.item` :155, `.info` :156, `.error` :158, `.pos` :162, `.cues` :173, `.resumed` :196, `.resumed_timer` :197); `enum Status` `desktop/aizome/apps/utsushi/src/app.rs:131`
- **Encoding:** partially explicit `Status`; the item is named by `current` into `queue` (each replaceable on its own); an `item` counter guards covers only; "first Opened" compares `info.url`.
- **States:** `Empty` → `Loading` → `Paused` ⇄ `Playing` → `Ended` → next, repeat or stay | `Failed`; `Close` → `Empty`.
- **Transitions:** `load` (`desktop/aizome/apps/utsushi/src/app.rs:313-340`); `seek` Ended→Paused (`desktop/aizome/apps/utsushi/src/app.rs:361-363`); Opened (`desktop/aizome/apps/utsushi/src/app.rs:546-566`); Failed (`desktop/aizome/apps/utsushi/src/app.rs:567-572`); State, with Ended→Next/Repeat (`desktop/aizome/apps/utsushi/src/app.rs:573-608`); Seeked Loading→Paused (`desktop/aizome/apps/utsushi/src/app.rs:612-614`); `Msg::Open` (`desktop/aizome/apps/utsushi/src/app.rs:785-795`); Next, Previous, PlayItem (`desktop/aizome/apps/utsushi/src/app.rs:860-869`); `Close` (`desktop/aizome/apps/utsushi/src/app.rs:1104-1123`); `leave()` writes the resume record (`desktop/aizome/apps/utsushi/src/app.rs:343-350`).
- **Hazards:** player events carry no item id, so a `State(Ended)`, `Failed` or `Cues` queued for the previous item lands on the new one (an old Ended skips the new item, `desktop/aizome/apps/utsushi/src/app.rs:588`); `leave()` names the item by `current` into `queue`, which `Msg::Open` replaces first.
- **Reify:** High — ten fields, about fifteen sites, a data-loss bug.
- **Suspected bug:** watching a two-hour film at 45:00, open another file (Open, drop, Open Recent or a second launch): `Msg::Open` sets `queue = paths` (`desktop/aizome/apps/utsushi/src/app.rs:792`), then `load(0)` calls `leave()` (`desktop/aizome/apps/utsushi/src/app.rs:314`) with `current` still 0, recording the new file at 45:00 with the film's length; `resume_at` (`desktop/aizome/apps/utsushi/src/app.rs:327`) then starts the new file at 45:00 under "Resumed at 45:00". If the film had ended, it erases the new file's own resume point instead. (confidence: high)
- **Related:** `desktop/aizome/crates/aizome-media/src/player.rs:305` (`open`; events emitted untagged, its own `generation` at :637); store NTU-32.

<a id="ntu-27"></a>

### NTU-27 — Utsushi timeline scrubbing
- **Where:** `desktop/aizome/apps/utsushi/src/app.rs:163` (`Utsushi.scrub: Option<f64>`, `.scrub_playing` :164, `.last_scrub_seek` :165); producer `desktop/aizome/apps/utsushi/src/widgets.rs:158-190`
- **Encoding:** the Option means dragging; a bool remembers whether to resume; an `Instant` holds keyframe seeks to one per 60 ms.
- **States:** `Idle` → `Scrubbing(t, was_playing)` → `Idle` (precise seek, resume if it was playing).
- **Transitions:** Start pauses and seeks (`desktop/aizome/apps/utsushi/src/app.rs:824-836`); Move (`desktop/aizome/apps/utsushi/src/app.rs:837-846`, throttle :840); End (`desktop/aizome/apps/utsushi/src/app.rs:847-853`); `load` clears `scrub` (`desktop/aizome/apps/utsushi/src/app.rs:325`) but not `scrub_playing`.
- **Hazards:** the widget sends End on any left release while usable (`desktop/aizome/apps/utsushi/src/widgets.rs:179-182`) and ignores everything once unusable (`desktop/aizome/apps/utsushi/src/widgets.rs:159`), so End can come without Start (resuming on a stale `scrub_playing`) or not at all; a media-key Play during a drag isn't held off.
- **Reify:** Medium.

<a id="ntu-28"></a>

### NTU-28 — Utsushi full-screen controls auto-hide
- **Where:** `desktop/aizome/apps/utsushi/src/app.rs:183` (`Utsushi.controls`, `.over_controls` :185, `.controls_menus` :188, `.controls_timer` :189, `.controls_shown_at` :190, `.fullscreen` :178, `.hidden` :181)
- **Encoding:** five fields plus playing and has-video; a count of open menus; a 2.5 s timer re-armed at most four times a second.
- **States:** `Windowed` → `FullShown(timer)` → `FullHidden` (cursor hidden, picture scanned out) → `FullShown`; held while the pointer is over the controls or a menu is open; shown while paused.
- **Transitions:** `show_controls` (`desktop/aizome/apps/utsushi/src/app.rs:439-460`); `rest_controls` (`desktop/aizome/apps/utsushi/src/app.rs:468-473`); `controls_menu` counts up (`desktop/aizome/apps/utsushi/src/app.rs:481`); `ControlsMenuClosed` counts down (`desktop/aizome/apps/utsushi/src/app.rs:1089-1094`); `Pointer` (`desktop/aizome/apps/utsushi/src/app.rs:967-973`); `OverControls` (`desktop/aizome/apps/utsushi/src/app.rs:974-979`); `HideControls` (`desktop/aizome/apps/utsushi/src/app.rs:980-990`); paused shows (`desktop/aizome/apps/utsushi/src/app.rs:600-602`); `window_state` (`desktop/aizome/apps/utsushi/src/app.rs:1152-1164`); `pointer_presence` (`desktop/aizome/apps/utsushi/src/app.rs:1174-1177`); inputs `desktop/aizome/apps/utsushi/src/ui.rs:65-67`, `desktop/aizome/apps/utsushi/src/ui.rs:134`.
- **Hazards:** `controls_menus` is counted up before `cx.show_menu`, which does nothing if the window can't be found (`desktop/aizome/crates/aizome-kit/src/cx.rs:453`), and is never reset on leaving full screen or changing item, so a missed close keeps the controls and clock ticking over the picture for good; fourteen tests (`desktop/aizome/apps/utsushi/tests/utsushi.rs:117-284`) each pin one path, a sign of the spread.
- **Reify:** High — seven fields, ten handlers, battery and scan-out consequences.

<a id="ntu-29"></a>

### NTU-29 — Utsushi clock and subtitle wake timers
- **Where:** `desktop/aizome/apps/utsushi/src/app.rs:191` (`Utsushi.tick`, `.cue_timer` :192, `.cue_pos` :174)
- **Encoding:** handles recomputed by `schedule()` from status, speed, cues, subtitle delay and whether the clock shows.
- **States:** `Asleep` (not playing) | `CuesOnly` (hidden, or full-screen controls gone) | `Ticking`.
- **Transitions:** `schedule` cancels and re-arms (`desktop/aizome/apps/utsushi/src/app.rs:404-437`); Tick (`desktop/aizome/apps/utsushi/src/app.rs:992-999`); CueTick (`desktop/aizome/apps/utsushi/src/app.rs:1000-1007`); called from seek, player events, HideControls, window_state, Speed, SubtitleDelay and Close.
- **Hazards:** none observed (recomputed from scratch each time).
- **Reify:** Low.

<a id="ntu-30"></a>

### NTU-30 — Utsushi Media Info sheet
- **Where:** `desktop/aizome/apps/utsushi/src/app.rs:148` (`Utsushi.sheet: Option<WindowKey>`, `.info_timer` :193)
- **Encoding:** sheet key plus a one-second refresh timer.
- **States:** `Closed` → `Open(key)` → `Closed`.
- **Transitions:** `ShowInfo` (`desktop/aizome/apps/utsushi/src/app.rs:1008-1013`); `CloseSheet` (`desktop/aizome/apps/utsushi/src/app.rs:1020-1027`); close box redirected (`desktop/aizome/apps/utsushi/src/app.rs:1181-1184`).
- **Hazards:** stays open across item changes and Close, then shows an empty sheet (`desktop/aizome/apps/utsushi/src/ui.rs:607`).
- **Reify:** Low.

<a id="ntu-31"></a>

### NTU-31 — Utsushi MPRIS service thread
- **Where:** `desktop/aizome/apps/utsushi/src/mpris.rs:312` (`Mpris::start`); shared `State` `desktop/aizome/apps/utsushi/src/mpris.rs:37` (`item`, `seeks`); `Mpris.updates` `desktop/aizome/apps/utsushi/src/mpris.rs:305`
- **Encoding:** thread phase by control flow; `Arc<Mutex<State>>` read by property getters; old/new diff for PropertiesChanged; the `seeks` counter raises `Seeked`; `item` guards SetPosition.
- **States:** `WaitingForFirst` → `Connecting` (owning `org.mpris.MediaPlayer2.utsushi`) → `Serving` | `NoBus` (thread ends; later publishes are dropped).
- **Transitions:** first state (`desktop/aizome/apps/utsushi/src/mpris.rs:322`); connect (`desktop/aizome/apps/utsushi/src/mpris.rs:324`, `desktop/aizome/apps/utsushi/src/mpris.rs:393-399`); failure (`desktop/aizome/apps/utsushi/src/mpris.rs:326-329`); coalescing loop (`desktop/aizome/apps/utsushi/src/mpris.rs:331-337`); `publish_changes` (`desktop/aizome/apps/utsushi/src/mpris.rs:347-391`); SetPosition check (`desktop/aizome/apps/utsushi/src/mpris.rs:180`).
- **Hazards:** failing to own the name (a second, `Alone` instance) is only logged; no reconnect if the bus restarts.
- **Reify:** Low.
- **Related:** client `desktop/aizome/crates/aizome-sys/src/mpris/mod.rs` (the shell's media widget and keys).

<a id="ntu-32"></a>

### NTU-32 — Utsushi persisted resume positions and recents
- **Where:** `desktop/aizome/apps/utsushi/src/store.rs:53` (`Store.positions`, `.recent` :54), files `positions.json`, `recent.json`, `state.json`
- **Encoding:** per canonical path, absent means start or finished, `Left{position, duration, at}` means in progress; oldest dropped past 400.
- **States:** `Fresh` → `InProgress(position)` → `Forgotten` (barely begun, finished, short, evicted).
- **Transitions:** `leave` inserts, removes or evicts (`desktop/aizome/apps/utsushi/src/store.rs:115-131`); `resume_at` (`desktop/aizome/apps/utsushi/src/store.rs:106-111`); `played` (`desktop/aizome/apps/utsushi/src/store.rs:135-142`); `clear_recent` (`desktop/aizome/apps/utsushi/src/store.rs:144-149`).
- **Hazards:** trusts the caller to pair path with that file's position and length (broken by NTU-26); rewrites the whole file, atomically, on every pause (`desktop/aizome/apps/utsushi/src/store.rs:70-79`).
- **Reify:** Low.
- **Related:** writer `desktop/aizome/apps/utsushi/src/app.rs:343` (NTU-26).

<a id="ntu-33"></a>

### NTU-33 — (test harness) Utsushi real-player polling waits
- **Where:** `desktop/aizome/apps/utsushi/tests/utsushi.rs:33` (`until`)
- **Encoding:** a deadline five times the nominal seconds and a 10 ms sleep loop pumping the harness.
- **States:** `Waiting` → `Satisfied` | `TimedOut` (assert with status, error, position).
- **Transitions:** loop (`desktop/aizome/apps/utsushi/tests/utsushi.rs:35-43`).
- **Hazards:** real-time decoding makes timing machine-dependent (subtitles worked around, `desktop/aizome/apps/utsushi/tests/utsushi.rs:104-114`).
- **Reify:** Low.

## Already explicit

- `desktop/aizome/apps/tokei/src/timing.rs:15` `enum TimerState` — the countdown's Ready, Running{ends}, Paused{left}, Done; transitions centralised in `impl Timer` (`start` `desktop/aizome/apps/tokei/src/timing.rs:55`, `pause` :69, `reset` :75, `check` :80)
- `desktop/aizome/apps/tokei/src/app.rs:23` `enum Page` — the part shown, kept in state.json; changed only in the `Msg::Page` handler (`desktop/aizome/apps/tokei/src/app.rs:464`)

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/apps/nisshi/data/org.aizome.Nisshi.policy.in` | 21 | machines | [NTU-01](#ntu-01) | polkit action for the root reader; auth_admin_keep caches unlock |
| `desktop/aizome/apps/nisshi/examples/sample.rs` | 17 | none |  | one-shot CLI wrapper around fixture::stage |
| `desktop/aizome/apps/nisshi/src/ansi.rs` | 364 | none |  | pure SGR parser; per-call style accumulator |
| `desktop/aizome/apps/nisshi/src/app.rs` | 1402 | machines | [NTU-01](#ntu-01), [NTU-02](#ntu-02), [NTU-03](#ntu-03), [NTU-04](#ntu-04), [NTU-05](#ntu-05), [NTU-06](#ntu-06), [NTU-07](#ntu-07), [NTU-08](#ntu-08), [NTU-09](#ntu-09), [NTU-10](#ntu-10) | unlock, requests, reads, file scope, follow, cursor, debounces, brush |
| `desktop/aizome/apps/nisshi/src/backend.rs` | 439 | machines | [NTU-11](#ntu-11) | reader client process; Drop closes stdin, never kills |
| `desktop/aizome/apps/nisshi/src/bin/helper.rs` | 382 | machines | [NTU-12](#ntu-12) | privileged helper request table and journalctl children |
| `desktop/aizome/apps/nisshi/src/entry.rs` | 744 | none |  | entry parsing and data types; interner cache |
| `desktop/aizome/apps/nisshi/src/filter.rs` | 361 | none |  | pure filter types and matching |
| `desktop/aizome/apps/nisshi/src/fixture.rs` | 665 | machines | [NTU-13](#ntu-13) | stand-in journalctl with asked log and follow.pid |
| `desktop/aizome/apps/nisshi/src/instance.rs` | 67 | machines | [NTU-14](#ntu-14) | single-instance socket claim and hand-off |
| `desktop/aizome/apps/nisshi/src/known.rs` | 367 | none |  | data tables; LazyLock-compiled regexes |
| `desktop/aizome/apps/nisshi/src/launch.rs` | 199 | none |  | pure parsing; thread-local name cache |
| `desktop/aizome/apps/nisshi/src/lib.rs` | 80 | none |  | module list and number formatting |
| `desktop/aizome/apps/nisshi/src/main.rs` | 90 | machines | [NTU-15](#ntu-15), [NTU-14](#ntu-14) | --print one-shot read loop; instance claim |
| `desktop/aizome/apps/nisshi/src/model.rs` | 398 | machines | [NTU-08](#ntu-08) | Store seq and View.seen incremental watermark |
| `desktop/aizome/apps/nisshi/src/protocol.rs` | 140 | machines | [NTU-11](#ntu-11), [NTU-12](#ntu-12) | frame grammar of the helper protocol; VERSION never checked |
| `desktop/aizome/apps/nisshi/src/query.rs` | 259 | none |  | pure query validation and journalctl args |
| `desktop/aizome/apps/nisshi/src/search.rs` | 479 | none |  | pure tokenizer and matcher |
| `desktop/aizome/apps/nisshi/src/settings.rs` | 63 | none |  | config file, atomic save |
| `desktop/aizome/apps/nisshi/src/shape.rs` | 179 | none |  | pure message fingerprinting |
| `desktop/aizome/apps/nisshi/src/suggest.rs` | 498 | none |  | pure suggestion builders |
| `desktop/aizome/apps/nisshi/src/time.rs` | 310 | none |  | pure time formatting and parsing |
| `desktop/aizome/apps/nisshi/src/ui/detail.rs` | 143 | none |  | view; seq guard for NTU-09 fields |
| `desktop/aizome/apps/nisshi/src/ui/menus.rs` | 130 | none |  | view of NTU-01 and NTU-03 state |
| `desktop/aizome/apps/nisshi/src/ui/mod.rs` | 8 | none |  | module list |
| `desktop/aizome/apps/nisshi/src/ui/sidebar.rs` | 102 | none |  | view |
| `desktop/aizome/apps/nisshi/src/ui/table.rs` | 242 | none |  | view; reveal from NTU-05 |
| `desktop/aizome/apps/nisshi/src/ui/timeline.rs` | 121 | machines | [NTU-10](#ntu-10) | canvas gates pointer events on brush state |
| `desktop/aizome/apps/nisshi/src/ui/view.rs` | 400 | none |  | view; banner and boot-search chip of NTU-01/NTU-03 |
| `desktop/aizome/apps/nisshi/tests/look.rs` | 84 | none |  | tests (goldens) |
| `desktop/aizome/apps/nisshi/tests/perf.rs` | 162 | none |  | tests (frame benchmarks) |
| `desktop/aizome/apps/nisshi/tests/reader.rs` | 411 | machines | [NTU-16](#ntu-16) | stand-in pkexec answer file, env lock, polling |
| `desktop/aizome/apps/nisshi/tests/ui.rs` | 497 | none |  | tests |
| `desktop/aizome/apps/tokei/src/alarms.rs` | 481 | machines | [NTU-17](#ntu-17), [NTU-18](#ntu-18) | alarms.json vs systemd timers; Recorder double |
| `desktop/aizome/apps/tokei/src/app.rs` | 920 | machines | [NTU-19](#ntu-19), [NTU-20](#ntu-20), [NTU-21](#ntu-21), [NTU-22](#ntu-22), [NTU-23](#ntu-23), [NTU-17](#ntu-17) | alarm list copy, sheets, timer, stopwatch frames, persist; launch sync |
| `desktop/aizome/apps/tokei/src/cities.rs` | 145 | none |  | data table; OnceLock |
| `desktop/aizome/apps/tokei/src/clock.rs` | 245 | none |  | clock source and pure solar math |
| `desktop/aizome/apps/tokei/src/instance.rs` | 62 | machines | [NTU-14](#ntu-14) | single-instance socket claim |
| `desktop/aizome/apps/tokei/src/lib.rs` | 16 | none |  | module list |
| `desktop/aizome/apps/tokei/src/main.rs` | 59 | machines | [NTU-17](#ntu-17), [NTU-24](#ntu-24), [NTU-14](#ntu-14) | --ring and --sync process entry points |
| `desktop/aizome/apps/tokei/src/ring.rs` | 293 | machines | [NTU-24](#ntu-24), [NTU-18](#ntu-18) | ring process; Scripted test notifier |
| `desktop/aizome/apps/tokei/src/sound.rs` | 214 | machines | [NTU-25](#ntu-25) | player child process; tone file cache |
| `desktop/aizome/apps/tokei/src/timing.rs` | 284 | machines | [NTU-22](#ntu-22) | Stopwatch Option-encoded run; TimerState already explicit |
| `desktop/aizome/apps/tokei/src/views.rs` | 499 | none |  | view |
| `desktop/aizome/apps/tokei/src/widgets.rs` | 138 | none |  | stateless widgets |
| `desktop/aizome/apps/tokei/tests/tokei.rs` | 431 | machines | [NTU-18](#ntu-18) | fake-clock rig stepping |
| `desktop/aizome/apps/utsushi/src/app.rs` | 1212 | machines | [NTU-26](#ntu-26), [NTU-27](#ntu-27), [NTU-28](#ntu-28), [NTU-29](#ntu-29), [NTU-30](#ntu-30) | item lifecycle, scrub, full-screen controls, wake timers, info sheet |
| `desktop/aizome/apps/utsushi/src/instance.rs` | 73 | machines | [NTU-14](#ntu-14) | single-instance socket claim |
| `desktop/aizome/apps/utsushi/src/lib.rs` | 18 | none |  | module list |
| `desktop/aizome/apps/utsushi/src/main.rs` | 38 | machines | [NTU-14](#ntu-14) | argument parsing; instance claim |
| `desktop/aizome/apps/utsushi/src/mpris.rs` | 399 | machines | [NTU-31](#ntu-31) | MPRIS service thread |
| `desktop/aizome/apps/utsushi/src/picture.rs` | 68 | none |  | PNG encode and save; name-collision loop |
| `desktop/aizome/apps/utsushi/src/store.rs` | 200 | machines | [NTU-32](#ntu-32) | persisted resume positions and recents |
| `desktop/aizome/apps/utsushi/src/ui.rs` | 779 | machines | [NTU-28](#ntu-28) | view; full-screen controls hover/pointer inputs |
| `desktop/aizome/apps/utsushi/src/widgets.rs` | 314 | machines | [NTU-27](#ntu-27) | timeline widget emits scrub Start/Move/End |
| `desktop/aizome/apps/utsushi/tests/utsushi.rs` | 574 | machines | [NTU-33](#ntu-33) | real-player polling waits |
