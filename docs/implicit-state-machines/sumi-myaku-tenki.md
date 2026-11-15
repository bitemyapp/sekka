# SMT: Sumi, Myaku and Tenki

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/apps/{sumi,myaku,tenki}`. Audited at `8e0acbe5`; line numbers refer to that commit.

58 files audited. 33 implicit machines: 10 High, 6 Medium, 17 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

<a id="smt-02"></a>

### SMT-02 — Table selection, expansion and launch request across readings
- **Where:** `desktop/aizome/apps/myaku/src/app.rs:124` (`Myaku.sel`, `.expanded` :122, `.rows` :123, `.inspector` :125, `.request` :131, `.frames` :134); also `desktop/aizome/apps/myaku/src/ui/view.rs:191`, `desktop/aizome/apps/myaku/src/ui/inspector.rs:41`
- **Encoding:** selection by identity (`Sel::Group(first member's (pid, start))`, `Sel::Process((pid, start))`); set of expanded group keys; `rows` (row index → frame index) rebuilt every reading; `request.pid` pending until the next reading; `frames == 1` triggers the focused-app auto-select
- **States:** NoSelection → Selected(key) → Orphaned (key absent: inspector "Nothing selected", `can_quit` "It has ended."); PendingPid(pid) → Selected | NotFound (notice); representable: inspector open on an orphaned key, `expanded` holding keys of ended groups
- **Transitions:** first reading auto-selects the focused group (`desktop/aizome/apps/myaku/src/app.rs:248-252`); pending pid resolved or failed in `take()` (`desktop/aizome/apps/myaku/src/app.rs:240-247`); `Msg::Launched --pid` re-arms it (`desktop/aizome/apps/myaku/src/app.rs:609-612`); `SelectRows` (`desktop/aizome/apps/myaku/src/app.rs:532-536`); `Activate` (`desktop/aizome/apps/myaku/src/app.rs:537-542`); `Select` (and expand its group) (`desktop/aizome/apps/myaku/src/app.rs:543-551`); `Toggle` (`desktop/aizome/apps/myaku/src/app.rs:526-531`); `Inspector` (`desktop/aizome/apps/myaku/src/app.rs:552`); rows rebuilt and re-sorted every reading (`desktop/aizome/apps/myaku/src/app.rs:254`, `305-367`)
- **Hazards:** `SelectRows`/`Activate` carry row indices resolved against `rows` at handling time, while `rows` is re-sorted every reading; a group's key is its first-started member's, so when that process exits the group gets a new key and the selection orphans; `expanded` is never pruned
- **Reify:** High — identity-based selection mixed with index-based messages over a list that reorders every second; suspected bug.
- **Suspected bug:** Right-click a row, wait a second, choose Inspect: the item carries the row index captured at right-click (`desktop/aizome/apps/myaku/src/ui/view.rs:205`), a reading re-sorts `rows` (by % CPU) before the click, and `Msg::Activate(i)` (`desktop/aizome/apps/myaku/src/app.rs:538-540`) then selects and inspects whatever row now sits at `i`, overriding the correct selection the right-click made (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#smt-02)

<a id="smt-03"></a>

### SMT-03 — Quit/pause confirmation and the status notice
- **Where:** `desktop/aizome/apps/myaku/src/app.rs:127` (`Myaku.notice`); alert payload `Msg::Signal(Sel, Signal)` `desktop/aizome/apps/myaku/src/app.rs:69`; also `desktop/aizome/apps/myaku/src/ui/view.rs:457`, `desktop/aizome/apps/myaku/src/ui/inspector.rs:308`
- **Encoding:** a kit alert whose buttons carry `Msg::Signal(sel, signal)` captured at ask time; `notice: Option<(String, error: bool)>`
- **States:** Idle → Confirming(sel, force) → Signalled | Cancelled; Pause/Resume signals at once; notice None → Shown(text, error), never back to None
- **Transitions:** `AskQuit` checks `can_quit`, opens the alert or sets an error notice (`desktop/aizome/apps/myaku/src/app.rs:564-588`); confirm → `signal()` re-resolves targets and sets the notice (`desktop/aizome/apps/myaku/src/app.rs:589`, `440-461`); `Pause` (`desktop/aizome/apps/myaku/src/app.rs:590-593`); unknown launch pid (`desktop/aizome/apps/myaku/src/app.rs:246`); Cancel just closes the alert (kit)
- **Hazards:** `can_quit` (kernel, other user, Myaku itself; `desktop/aizome/apps/myaku/src/app.rs:402-419`) is checked only at ask time; targets are re-resolved by key at confirm time, so the processes signalled may differ from the count the alert named; if the target ended meanwhile `targets()` is empty and `signal()` reports success "Asked to quit: ." with no name; nothing clears `notice`, and the status line prefers it to the summary (`desktop/aizome/apps/myaku/src/ui/view.rs:457-459`)
- **Reify:** High — a confirmation lifecycle split between alert payloads and a sticky field; suspected bug.
- **Suspected bug:** After any quit, pause or refused attempt, the status line keeps that message (in red for "It belongs to another user…" and the like) for the rest of the session, hiding the process/thread/uptime summary and the GPU lens's client count, because `notice` is never reset to None (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#smt-03)

<a id="smt-13"></a>

### SMT-13 — Close-after-save for dirty documents
- **Where:** `desktop/aizome/apps/sumi/src/app.rs:176` (`Sumi.closing: HashSet<WindowKey>`)
- **Encoding:** membership in `closing`, plus kit alerts whose buttons carry `Msg::Close(w, Save|Discard)`, `Msg::Overwrite`, `Msg::SaveUtf8`, `Msg::Reload`
- **States:** Open → Asking ("Save the changes?") → Discarded (closed) | SavePending (in `closing`) → Closed | still SavePending (save stopped by another alert or the panel)
- **Transitions:** `request_close` alert (`desktop/aizome/apps/sumi/src/app.rs:583-589`); `Close(Save)` inserts and saves (`desktop/aizome/apps/sumi/src/app.rs:1146-1148`); a successful `save` removes and closes (`desktop/aizome/apps/sumi/src/app.rs:542-544`); `SaveTo(None)` (panel cancelled) removes (`desktop/aizome/apps/sumi/src/app.rs:1098-1100`); `forget` removes (`desktop/aizome/apps/sumi/src/app.rs:598`); `Close(Discard)` (`desktop/aizome/apps/sumi/src/app.rs:1150`)
- **Hazards:** `save` returns false and leaves the window in `closing` on the changed-on-disk alert (`desktop/aizome/apps/sumi/src/app.rs:484-496`), the unencodable alert (`:498-512`) and a write failure (`:514-517`); none of their Cancel/OK paths remove it
- **Reify:** High — an intent flag outlives the alert chain that justified it; suspected bug.
- **Suspected bug:** Close a dirty document, choose Save, then Cancel the "changed by another program" or "can't be saved in Latin-1" alert (or hit a write error): the window stays open but stays in `closing`, so the next ordinary ⌘S, perhaps much later, saves and then closes the window unasked (`desktop/aizome/apps/sumi/src/app.rs:542-544`) (confidence: high)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#smt-13)

<a id="smt-14"></a>

### SMT-14 — Link and Go to Line sheets
- **Where:** `desktop/aizome/apps/sumi/src/app.rs:155` (`enum Sheet`), `desktop/aizome/apps/sumi/src/app.rs:167` (`Sumi.sheets`); also `desktop/aizome/apps/sumi/src/view.rs:193`
- **Encoding:** partially explicit: `HashMap<sheet WindowKey, Sheet>`, each `Sheet` carrying its parent, field text and (Link) the selection range taken at open; the field messages `LinkText`, `LinkUrl`, `LinkDone`, `GotoText`, `GotoDone` carry no window key
- **States:** None → Link{parent, text, url, range} | Goto{parent, line, max} → Done(ok) | Cancelled | closed by request
- **Transitions:** open Link (`desktop/aizome/apps/sumi/src/app.rs:856-859`); open Goto (`desktop/aizome/apps/sumi/src/app.rs:959-961`); field edits (`desktop/aizome/apps/sumi/src/app.rs:1196-1209`, `1221-1227`); finish (`desktop/aizome/apps/sumi/src/app.rs:1210-1220`, `1228-1245`); `close_requested` removes (`desktop/aizome/apps/sumi/src/app.rs:1000-1002`)
- **Hazards:** every field message picks the first sheet of its kind from a `HashMap` (`sheet_of`, `desktop/aizome/apps/sumi/src/app.rs:968-970`), so with two sheets of one kind open over two documents the wrong one is edited, applied or closed; the Link range is fixed at open while menu commands aimed at the sheet go to its parent (`desktop/aizome/apps/sumi/src/app.rs:233`) and can change the text under it
- **Reify:** High — the enum carries data but messages lose which sheet they belong to; suspected bug.
- **Suspected bug:** Open Link… (⌘K) in document A, switch to document B and open Link… there: typing in B's sheet updates whichever Link sheet the map yields first, so B's field may not take the text, and Insert Link can insert A's link into A and close A's sheet while B's stays open (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#smt-14)

<a id="smt-17"></a>

### SMT-17 — Document file, dirty and autosave lifecycle
- **Where:** `desktop/aizome/apps/sumi/src/doc.rs:178` (`Doc.path`, `.untitled` :180, `.saved` :185, `.text_dirty` :186, `.format_dirty` :188, `.stamp` :190, `.autosave_seq` :196, `.autosaved` :198, `.notice` :202); also `desktop/aizome/apps/sumi/src/app.rs:477`, `desktop/aizome/apps/sumi/src/files.rs:272`
- **Encoding:** nine fields; "pristine" derived (`desktop/aizome/apps/sumi/src/doc.rs:235-237`); conflict derived from `stamp` against the disk (`desktop/aizome/apps/sumi/src/doc.rs:373-378`); autosave debounce by `autosave_seq` echoed in `Msg::Autosave(w, seq)` 800 ms later
- **States:** Pristine Untitled → Untitled-dirty → Saved-clean ↔ Saved-dirty (text and/or format) → Conflict (changed on disk); orthogonal Autosaved ↔ AutosavePending; representable: `untitled > 0` with a path, `text_dirty` true with text equal to `saved` (only undo/redo recompute it)
- **Transitions:** `Doc::new` (`desktop/aizome/apps/sumi/src/doc.rs:206-228`); `install` (`desktop/aizome/apps/sumi/src/doc.rs:268-289`); `after_edit` marks dirty and bumps the seq (`desktop/aizome/apps/sumi/src/doc.rs:345-347`); undo/redo recompute (`desktop/aizome/apps/sumi/src/doc.rs:311-313`); `new_untitled_with` (`desktop/aizome/apps/sumi/src/app.rs:304-306`); save success (`desktop/aizome/apps/sumi/src/app.rs:522-539`); encoding and line-ending changes (`desktop/aizome/apps/sumi/src/app.rs:824`, `835`); `restore` (`desktop/aizome/apps/sumi/src/app.rs:433-461`); debounce timer (`desktop/aizome/apps/sumi/src/app.rs:747`) → `Autosave` (`:1152-1156`) → `autosave_now` (`:404-413`); `Quit` forces a fresh autosave (`:1280`)
- **Hazards:** `format_dirty` isn't kept in the session, and `restore` for a document without unsaved text reloads it with the detected encoding (`desktop/aizome/apps/sumi/src/app.rs:454`), ignoring the session's `encoding` and `line_ending`; the background reload path doesn't remove the autosave text (`desktop/aizome/apps/sumi/src/app.rs:1134`) as the small-file path does (`:1122`)
- **Reify:** High — nine fields mutated from a dozen sites with representable invalid combinations; suspected bug.
- **Suspected bug:** Change a document's Encoding or Line Endings (the title says "— Edited"), then Quit, which promises everything comes back: on relaunch the document is reloaded in its detected encoding and clean, so the unsaved format change is gone; a Reopen with Encoding choice is lost the same way (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#smt-17)

<a id="smt-18"></a>

### SMT-18 — Background file read generation
- **Where:** `desktop/aizome/apps/sumi/src/doc.rs:200` (`Doc.loading: Option<u64>`); also `desktop/aizome/apps/sumi/src/app.rs:177` (`Sumi.seq`), `desktop/aizome/apps/sumi/src/view.rs:62`
- **Encoding:** a request id in `loading`, echoed by `Msg::Loaded(w, seq, result)`; `path` and title set before the read starts
- **States:** Idle → Loading(seq) → Installed | Failed
- **Transitions:** `read_in_background` sets `loading`, `path`, title (`desktop/aizome/apps/sumi/src/app.rs:349-357`), from `open_path` (`:335-338`) and a big `Reload` (`:1113-1114`); stale answers dropped (`:1130-1132`); `install` clears (`desktop/aizome/apps/sumi/src/doc.rs:277`); failure clears the path and sets `untitled = 1` (`desktop/aizome/apps/sumi/src/app.rs:1135-1142`); edits refused while loading (`:1034-1036`)
- **Hazards:** only `Msg::Edit` is gated: Format commands, Replace All, Shift Right and Link edit the old text during a load and are then overwritten by `install`; the read can't be cancelled (closing the window just drops its answer); the failure path neither resets the title nor checks whether "Untitled" is taken
- **Reify:** High — a request id plus side fields with an inconsistent failure path; suspected bug.
- **Suspected bug:** Open a >2 MiB file that can be stat'ed but not read (a root-only log): after the alert, a window titled with the file's name remains holding an empty "Untitled" document; and a Revert or Reopen with Encoding of a >2 MiB document whose file became unreadable detaches the document from its file (path cleared), whereas the same failure for a small file keeps the path (`desktop/aizome/apps/sumi/src/app.rs:1125`) (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#smt-18)

<a id="smt-19"></a>

### SMT-19 — Find bar and search generation
- **Where:** `desktop/aizome/apps/sumi/src/doc.rs:19` (`Find.shown`, `.replace`, `.query`, `.matches`, `.current`, `.error`, `.version`, `.seq`); also `desktop/aizome/apps/sumi/src/app.rs:615`, `desktop/aizome/apps/sumi/src/view.rs:161`
- **Encoding:** visibility flags; matches valid for content `version`; `seq` drops stale background answers (`Msg::Searched`)
- **States:** Hidden ↔ Shown ↔ Shown+Replace; search Idle → Searching(seq) (on a thread above 1 MiB) → Found(version) | Error; Found turns Stale when `version` ≠ the content's
- **Transitions:** show (`desktop/aizome/apps/sumi/src/app.rs:891-909`); hide (`:910-916`); `search` (`:615-647`); `found` (`:649-676`); `fresh_matches` (`:679-687`); `find_step` (`:690-710`); re-search after an edit only when shown and small (`:748-750`); `install` invalidates (`desktop/aizome/apps/sumi/src/doc.rs:288`)
- **Hazards:** marks go to the editor whether or not the bar is shown (`desktop/aizome/apps/sumi/src/view.rs:161`) and aren't refreshed after edits while it's hidden; above 1 MiB `fresh_matches` only spawns a search, so `find_step` and `replace_one` go on with stale ranges, and `FindQuery` selects the previous query's current match (`desktop/aizome/apps/sumi/src/app.rs:934`); background searches are never cancelled (a full scan per keystroke)
- **Reify:** High — eight fields, nine mutation sites, a sync/async split; suspected bug.
- **Suspected bug:** Find a word, press Done (or Escape), then type above a match: the match highlights stay painted at their old character offsets and drift onto other text; and in a >1 MiB document, ⌘G right after an edit selects a range from the pre-edit match list (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#smt-19)

<a id="smt-20"></a>

### SMT-20 — Incremental highlight states and colouring-pass scheduling
- **Where:** `desktop/aizome/apps/sumi/src/syntax/cache.rs:41` (`Highlight.states`, `.valid`, `.pending`, `.guess`, `.incomplete` :55, `.scheduled` :57, `.pump` :60, `.notify` :67); also `desktop/aizome/apps/sumi/src/app.rs:181` (`Sumi.highlight_soon`), `desktop/aizome/apps/sumi/src/view.rs:22`
- **Encoding:** `Arc<Mutex<Highlight>>` per document: a valid prefix, pending edit regions and guesses; three pass flags in the cache and a coalescing flag in the app; `Msg::HighlightSoon` → 4 ms timer → `Msg::Highlight`
- **States:** Complete → Incomplete (a line guessed this pass) → Scheduled (notify sent) → Pumping (next `state_at` spends the budget) → Complete | Incomplete; an edit moves `valid` back
- **Transitions:** `edited` (`desktop/aizome/apps/sumi/src/syntax/cache.rs:107-140`); `advance` (`:144-193`); `state_at` takes `pump`, sets `incomplete`, starts guesses (`:226-248`); `reset` on a language change keeps notify, budget and rate (`:93-99`); the highlighter schedules (`desktop/aizome/apps/sumi/src/view.rs:28-33`); notify installed per window (`desktop/aizome/apps/sumi/src/app.rs:262-267`); `HighlightSoon` coalesces (`:1157-1165`); `Highlight` clears flags, sets `pump` and bumps `styles` for every document (`:1166-1176`)
- **Hazards:** the "one budget a pass" gate (`incomplete`) is cleared only by `Msg::Highlight`, so a document without `notify` would guess forever; every pass bumps `styles` of all open documents, re-styling every window to finish one; `pump` lingers on documents not redrawn
- **Reify:** High — five flags across three files and six mutation sites, crossing into the kit's editor.
- **Related:** the kit's text editor calling the highlighter per visible line (`desktop/aizome/crates/aizome-kit/src/widgets/text_editor.rs`)

<a id="smt-23"></a>

### SMT-23 — Place search debounce, generation and early Return
- **Where:** `desktop/aizome/apps/tenki/src/app.rs:43` (`Search.text`, `.generation`, `.busy`, `.results`, `.answered`, `.error`, `.add_first`; `Tenki.search` :128); also `desktop/aizome/apps/tenki/src/ui/mod.rs:117`
- **Encoding:** a generation counter echoed by `Msg::RunSearch(g)` and `Msg::Searched(g, …)`; a 250 ms debounce timer; `add_first` for Return typed before the answer
- **States:** Empty/Short → Debouncing(g) → Busy(g) → Answered(text, results) | Failed(text, error); with AddFirst armed, the answer adds its first place
- **Transitions:** a text change resets everything and arms the debounce (`desktop/aizome/apps/tenki/src/app.rs:539-557`); `RunSearch` (`:558-562`) → `run_search` (`:442-454`); `Searched` (`:563-582`); `AddFirst` (`:588-600`); `Add` → `add` resets under a new generation (`:583-587`, `437`); `ClearSearch` (`:601-603`)
- **Hazards:** seven fields must be reset together on every change (the comment at `desktop/aizome/apps/tenki/src/app.rs:546-549` records the busy-release trap); if an early-Return search answers before the 250 ms debounce fires, `RunSearch` finds its generation current and `busy` false and sends the same request again; superseded requests run to completion; `Msg::Add(i)` indexes `results` as rendered
- **Reify:** High — seven fields and eight handlers encode one request lifecycle.
- **Related:** Open-Meteo's geocoding API (`desktop/aizome/apps/tenki/src/places.rs:7`)

<a id="smt-24"></a>

### SMT-24 — Location consent and GeoClue lookup
- **Where:** `desktop/aizome/apps/tenki/src/app.rs:61` (`enum Locating`; `Tenki.current` :120, `.locating` :121, `.location_generation` :122, `.location_cancel` :123); also `desktop/aizome/apps/tenki/src/store.rs:186` (`enum Consent`), `desktop/aizome/apps/tenki/src/store.rs:180` (`State.located`)
- **Encoding:** partially explicit: persisted `Consent {Ask, Allowed, Declined}` in `tenki.toml`, `Locating {Idle, Busy, Failed}`, a generation counter and a per-request `Cancellation`, the last position in memory and in `state.json`
- **States:** Ask → Allowed{Idle | Busy | Failed} | Declined; position None | Some; representable: Declined with a position kept, Failed with an old position shown
- **Transitions:** position read at start (`desktop/aizome/apps/tenki/src/app.rs:148`); `launched` locates when allowed (`:499-501`); `locate` (`:367-378`); `cancel_location` (`:380-384`); `decline_location` (`:386-392`); removing the current row (`:412-415`); `AllowLocation` (`:647-657`); `ToggleLocation`, `SearchInstead` (`:659-670`); `Refresh` locates only without a position (`:609-611`); `Located` with the generation check (`:671-692`); `Drop` cancels (`:476-480`)
- **Hazards:** declining never clears `current`, and `save_state` (`desktop/aizome/apps/tenki/src/app.rs:254-258`) rewrites it to `state.json` on every selection; the position is refreshed only at launch, Allow and Try Again (the minute tick never re-locates), and a failed re-lookup with an old position shows the old place's weather with no failure note (`desktop/aizome/apps/tenki/src/ui/mod.rs:87-93`); Busy relies on the shared client's 20 s deadline (`desktop/aizome/apps/tenki/src/services.rs:131`)
- **Reify:** High — consent, lookup and position over six fields in two files with representable invalid combinations; suspected bug.
- **Suspected bug:** Allow location, let it resolve, then choose Not Now or turn Show Current Location off: the last GeoClue position stays in `state.json` and is written again whenever a place is selected, although the tests take care that answers arriving after a decline are not saved (`desktop/aizome/apps/tenki/tests/tenki.rs:596`) (confidence: medium)
- **Related:** the GeoClue client and `Cancellation` in `desktop/aizome/crates/aizome-sys/src/location.rs`; the GeoClue 2 D-Bus service
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#smt-24)
