# STB: Settei: app shell, appearance, Bluetooth and displays

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/apps/settei`. Audited at `8e0acbe5`; line numbers refer to that commit.

42 files, 0 pending. 20 implicit machines: 4 High, 8 Medium, 8 Low.

## Machines

<a id="stb-01"></a>

### STB-01 — Pane navigation, history and the shown/hidden pane lifecycle
- **Where:** `desktop/aizome/apps/settei/src/app.rs:100` (`Settei.current`, `.back`, `.forward`, `.started` at `:116`); also `desktop/aizome/apps/settei/src/pane.rs:323` (`Pane::shown`/`hidden`/`crumb`/`back` contract), `desktop/aizome/apps/settei/src/shared.rs:44` (`Shared.keyboard`), `desktop/aizome/apps/settei/src/main.rs:50`
- **Encoding:** `current` index, `back`/`forward` stacks, `started` bool (the first `go` only shows), each pane's own sub-page state behind `crumb()`/`back()`, and `Shared.keyboard`, set only while a launch's reveal runs
- **States:** `NotStarted` → `Showing(pane, sub-page)`; history orthogonal; each pane `Hidden` ⇄ `Shown`, driven only by `go`. Representable but unused: history holding `current`
- **Transitions:** `launched` NotStarted→Showing(first) (`app.rs:506`, `started = true` `app.rs:205`); `go` hides the old pane (`app.rs:199`), records history (`app.rs:201-202`), shows the new (`app.rs:207`); `Show(current)` unwinds sub-pages with up to 8 `back_any` (`app.rs:524-528`); `Back` tries the pane's `back_any`, then pops history (`app.rs:542-547`); `Forward` (`app.rs:551-553`); `launched`/`Launched` set `shared.keyboard` around `reveal`/`open` and clear it (`app.rs:513-515`, `app.rs:596-598`); `SystemChanged`/`SettingsOnDisk` reach only the current pane once started (`app.rs:564-571`)
- **Hazards:** quitting or closing the window never calls `hidden` (the kit quits when the last window closes; `main.rs:50-52` only flushes), so pane teardown never runs (Bluetooth discoverable STB-09, Displays countdown STB-13); hidden panes miss `system_changed`/`settings_changed`, so whatever they derive stays stale until shown again; choosing the current pane from the Panes menu while its sheet is up runs `back_any`, which rejects a Bluetooth pairing (`bluetooth.rs:753-755`) or reverts a display change (`displays/mod.rs:626-628`); `shared.keyboard` only works as a side channel if a pane reads it synchronously in `focus_entry`
- **Reify:** Medium — history plus lifecycle across six handlers; the shown/hidden pairing and the missing teardown at quit are invariants that an explicit lifecycle would enforce
- **Suspected bug:** View ▸ Back (⌘[) is enabled only when `!back.is_empty()` (`app.rs:621`), but the title strip's Back button also counts a sub-page crumb (`app.rs:372`). Opened with `--pane general.about` and no history, ⌘[ does nothing while the on-screen Back works (confidence: medium)
- **Reproduction:** not attempted; only suspected bugs on High machines were reproduced

<a id="stb-02"></a>

### STB-02 — Sheet window ownership, split between the window and the panes
- **Where:** `desktop/aizome/apps/settei/src/app.rs:108` (`Settei.sheet: Option<(&'static str, WindowKey)>`); also `desktop/aizome/apps/settei/src/pane.rs:738` (`open_sheet`/`close_sheet`), pane-side state at `desktop/aizome/apps/settei/src/panes/bluetooth.rs:255` (`prompt`) and `desktop/aizome/apps/settei/src/panes/displays/mod.rs:112` (`keep`)
- **Encoding:** the window's `sheet` (owner pane id + sheet window), plus each pane's own "a sheet is open" field, which `Pane::sheet()` reads; confirm alerts (`pane.rs:682-696`) are kit dialogs that `sheet` doesn't track
- **States:** `None` → `Open(pane, window)`; pane side `Closed`/`Open`. Representable invalid: `sheet == None` while the pane's state says open (after the sheet window is closed, `app.rs:649-651`); `sheet == Some(p)` while `p.sheet()` is `None` (renders an empty sheet, `app.rs:613`); `sheet == Some(A)` while pane B's state is open
- **Transitions:** `OpenSheet` closes any current sheet window and opens a new one (`app.rs:581-587`); `CloseSheet` closes whatever is open, whoever owns it (`app.rs:588-592`); `close_requested` forgets it (`app.rs:648-653`)
- **Hazards:** `CloseSheet` carries no owner, so a late Bluetooth `Done(PAIRING)` → `close_prompt` (`bluetooth.rs:629-631`, `:328`) can close another pane's sheet, for example Displays' keep sheet, whose countdown then reverts out of sight; closing a sheet window leaves the pane's question and its responder alive until BlueZ times out
- **Reify:** Medium — one modal with two sources of truth; a sheet state tagged with its owner would remove the cross-pane close
- **Suspected bug:** each `DisplayPasskey` update (one per key typed on the remote device) arrives as `Prompted` and calls `open_sheet` again (`bluetooth.rs:607`), and `OpenSheet` closes and reopens the sheet window every time (`app.rs:582-586`), so the sheet flickers and loses focus (confidence: medium)
- **Reproduction:** not attempted; only suspected bugs on High machines were reproduced

<a id="stb-03"></a>

### STB-03 — Generation-tagged transient highlights: the revealed option and the status line
- **Where:** `desktop/aizome/apps/settei/src/app.rs:106` (`Settei.revealed`, `.reveal_generation`); also `desktop/aizome/apps/settei/src/shared.rs:35` (`Shared.status`, `.status_generation`)
- **Encoding:** an `Option` payload, a monotonic generation, and a one-shot `cx.after` timer that carries the generation
- **States:** `Idle` → `Shown(gen)` → `Idle` (only the newest timer clears)
- **Transitions:** `reveal` sets the highlight and arms the timer (`app.rs:233-235`); `Unreveal(g)` clears it if `g` is current (`app.rs:576-580`); `say` sets the line and arms 5 s, or 10 s for errors (`shared.rs:165-173`); `clear_status` (`shared.rs:175-179`)
- **Hazards:** none observed (stale timers are dropped by generation; timers are never cancelled, which is harmless)
- **Reify:** Low — small, local and correct

<a id="stb-04"></a>

### STB-04 — Single-instance socket: claim, hand-off, listen, release
- **Where:** `desktop/aizome/apps/settei/src/instance.rs:20` (`claim`), `:34` (`listen`), `:49` (`release`); also `desktop/aizome/apps/settei/src/main.rs:43`, `desktop/aizome/apps/settei/src/app.rs:112` (`Settei.listener`)
- **Encoding:** the socket file `$XDG_RUNTIME_DIR/aizome-app.settei.sock` and whether a live process listens on it; `Instance::{Handed, First(Option<UnixListener>)}`; a detached listener thread
- **States:** `Probe` → `Handed` (this launch exits) | `StaleRemoved` → `Listening` | `Unlisted` (no runtime dir or bind failed) → `Released` at exit
- **Transitions:** connect and write succeed → Handed (`instance.rs:22-25`); otherwise unlink (`instance.rs:29`) and bind (`instance.rs:30`); `launched` moves the listener into its thread (`app.rs:498-501`); the thread ends when `take` returns false (`instance.rs:40-41`); `release` after the event loop (`main.rs:59`), skipped by `exit(1)` on a kit error (`main.rs:55`)
- **Hazards:** TOCTOU between the failed connect, the unlink and the bind; `release` unlinks whatever socket is at the path, even another instance's; a successful `writeln` only means the kernel buffered the line, so a launch that arrives while the old instance is flushing at exit is dropped and nothing opens; `read_line` has no timeout, so one silent client blocks every later hand-off; `--sample` skips single-instance entirely
- **Reify:** Medium — persisted file state with racy transitions between processes
- **Suspected bug:** two launches at once (a double-click on the launcher): both fail to connect, A binds, B's `remove_file` (`instance.rs:29`) unlinks A's socket and B binds. Two windows run, A is unreachable, and whichever quits first unlinks the other's socket, so the next launch starts a third (confidence: medium)
- **Reproduction:** not attempted; only suspected bugs on High machines were reproduced

<a id="stb-05"></a>

### STB-05 — Remembered theme and size before High Contrast and Larger text (persisted)
- **Where:** `desktop/aizome/apps/settei/src/panes/accessibility.rs:193` (`set_high_contrast`, key `[appearance] theme_before_contrast`), `desktop/aizome/apps/settei/src/panes/accessibility.rs:176` (`set_larger`, key `size_before_larger`); also `desktop/aizome/apps/settei/src/panes/appearance.rs:334`
- **Encoding:** `appearance.theme`/`appearance.size` in `settings.toml`, plus extra keys that remember the value before the mode came on
- **States:** `Off` → `On(remembered)`; `On(no memory)` when chosen by another path; `Off(stale memory)` when left by another path
- **Transitions:** High Contrast on records the theme and sets it (`accessibility.rs:201-203`), off restores it and removes the key (`accessibility.rs:205`); Larger text on (`accessibility.rs:180-182`), off (`accessibility.rs:184`); Appearance's theme cards route High Contrast through `set_high_contrast` and remove the key for any other theme (`appearance.rs:337-341`); Appearance's Size sets `size` directly (`appearance.rs:347`)
- **Hazards:** Size has no path that clears `size_before_larger`, so the memory outlives Large; hand edits or other writers leave either key stale
- **Reify:** Low — two small persisted toggles; clearing the key in `Msg::Size` fixes it
- **Suspected bug:** start at Small, turn Larger text on (remembers "small"), choose Medium in Appearance, then Large, then turn Larger text off: it restores Small, not Medium, because `Msg::Size` never removes `size_before_larger` (confidence: medium)
- **Reproduction:** not attempted; only suspected bugs on High machines were reproduced

<a id="stb-06"></a>

### STB-06 — Lazy pane data loads keyed on an empty Option (wallpapers, apps to pin)
- **Where:** `desktop/aizome/apps/settei/src/panes/appearance.rs:26` (`Appearance.wallpapers: Option<Vec<(PathBuf, Option<Image>)>>`); also `desktop/aizome/apps/settei/src/panes/desktop.rs:29` (`Desktop.apps`)
- **Encoding:** `None` means both "never asked" and "asked, in flight"; once `Some`, each picture's `Option<Image>` thumbnail fills in as it is made
- **States:** `NotLoaded` → (`Scanning`, indistinguishable from `NotLoaded`) → `Listed(thumbnails pending)` → `Listed(done)`
- **Transitions:** `shown` starts the scan when `None` (`appearance.rs:326-330`, thread `appearance.rs:248-263`); `Found` (`appearance.rs:387`); `Thumb` fills a slot (`appearance.rs:388-394`); `Sample` (`appearance.rs:395`); a chosen file joins the list with its own thumbnail job (`appearance.rs:352-362`); Desktop's `shown` spawns `AppIndex::load` when `None` (`desktop.rs:146-161`); `Apps` (`desktop.rs:172`)
- **Hazards:** with no in-flight marker, showing the pane again before the reply starts a second scan or load, and a second `Found` resets thumbnails already made; a wallpaper chosen while the list is `None` isn't added, and the scan took its `chosen` list before the choice, so that tile never gets a thumbnail; the list is never rescanned (new pictures need a restart); the scan thread outlives `hidden` and stops only when the window is gone (`appearance.rs:259`)
- **Reify:** Low — cosmetic consequences only

<a id="stb-07"></a>

### STB-07 — Battery pane services, charge history and logind read
- **Where:** `desktop/aizome/apps/settei/src/panes/battery.rs:49` (`Battery.batteries`, `.modes`, `.history`, `.logind`)
- **Encoding:** `Option`s filled from the first `shown` on; `history` uses `Some(empty)` both for "request in flight" and for "fetched, nothing"
- **States:** `Idle` → `ServicesUp`; history `None` | `Requested` (`Some([])`) | `Loaded`; logind `None` | `Read`
- **Transitions:** `shown` creates the UPower and power-profiles clients once (`battery.rs:310-321`), fetches history (`battery.rs:332`; `fetch_history` returns early while UPower lists no battery, `battery.rs:252-257`) and reads logind.conf again every time (`battery.rs:333`); `system_changed` marks `Some([])` and fetches once a battery appears (`battery.rs:342-345`); `History`/`Logind` (`battery.rs:350-351`)
- **Hazards:** a failed fetch turns into `Some([])` (`battery.rs:256`), which looks the same as in flight and isn't retried until the pane is shown again; `shown` and `system_changed` can both fetch at once; the clients are never dropped on `hidden`
- **Reify:** Low — read-only display state
- **Related:** `desktop/aizome/crates/aizome-sys/src/power/upower.rs`, `desktop/aizome/crates/aizome-sys/src/power/profiles.rs`

<a id="stb-08"></a>

### STB-08 — Keep Awake switch: optimistic value, helper process, marker file
- **Where:** `desktop/aizome/apps/settei/src/panes/battery.rs:54` (`Battery.awake: Option<bool>`)
- **Encoding:** `awake` (`None` until read, then the value shown) over the real state: the marker file `$XDG_STATE_HOME/aizome/keep-awake` read by `aizome_awake::requested()`, which one `aizome-awake on|off` child per toggle changes
- **States:** `Unread` → `Shown(v)` → `Pending(v)` (optimistic) → `Confirmed(v)` | `Reverted(file)`
- **Transitions:** `shown` reads the file (`battery.rs:337`); `KeepAwake` sets the value optimistically and spawns the helper (`battery.rs:375-392`); `Awake(on)` (`battery.rs:394`); `Done("Keep Awake", Err)` reads the file again and reports the error (`battery.rs:397-400`)
- **Hazards:** no in-flight guard: a quick on/off runs two helpers concurrently, and the switch shows whichever finishes last, not what was asked for last; `shown` overwrites a pending optimistic value with the file's; nothing reads the file on `system_changed`
- **Reify:** Medium — user-visible state whose source of truth is in another process, with racing writers
- **Suspected bug:** if Keep Awake is turned off from the menu bar's cup while Battery & Power shows, the pane's switch stays on until the pane is shown again, because `awake` is read only in `shown` (`battery.rs:337`) (confidence: medium)
- **Related:** `desktop/aizome/crates/aizome-awake/src/lib.rs` (`setting`, `requested`), `desktop/aizome/crates/aizome-awake/src/main.rs` (`on`/`off`), the shell's menu-bar Keep Awake
- **Reproduction:** not attempted; only suspected bugs on High machines were reproduced

<a id="stb-09"></a>

### STB-09 — Bluetooth pane activity: pairing agent, discovery and discoverability while shown
- **Where:** `desktop/aizome/apps/settei/src/panes/bluetooth.rs:259` (`Bluetooth.made_discoverable`, `.agent` `:260`, `.showing` `:262`, `.looking` `:263`, `.pinned` `:267`)
- **Encoding:** four bools and the pinned adapter path, set against the adapter's `Discoverable`/`Discovering` in BlueZ and the agent's registration
- **States:** `Hidden` → `Shown(adapter unknown or off)` → `Looking` (+ `MadeDiscoverable`) → `Hidden`; agent `Unregistered` → `Registering` (`agent = true`) → `Registered` | `Failed`. Representable: `looking` true after the adapter was powered off
- **Transitions:** `shown` sets `showing`, clears `looking` (`bluetooth.rs:416-417`), registers the agent if `!agent` (`bluetooth.rs:434-455`) and looks (`bluetooth.rs:456`); `look` makes the adapter discoverable if it isn't and starts discovery (`bluetooth.rs:308-315`); `system_changed` retries `look` until it takes (`bluetooth.rs:461-463`); `Power` pins the adapter (`bluetooth.rs:498`); `Powered(true)` looks (`bluetooth.rs:504`); `Agent(Err)` clears `agent` (`bluetooth.rs:618-622`); `hidden` stops discovery, undoes discoverability, unregisters the agent (`bluetooth.rs:475-491`)
- **Hazards:** `made_discoverable` is set only if the adapter wasn't discoverable when looked at, so a run that finds it already discoverable never turns it off; powering off leaves `looking = true`, so powering on from elsewhere doesn't restart discovery while the pane shows; a late `Agent(Err)` from an earlier showing clears `agent` while a newer registration is live (registering replaces the old one, so this only costs a redundant re-register)
- **Reify:** High — five fields across six handlers, with an effect on privacy (discoverable)
- **Suspected bug:** quitting Settei while Bluetooth shows never calls `hidden` (STB-01), so the adapter stays discoverable after Settei is gone until BlueZ's `DiscoverableTimeout` expires (never, where it is 0), and the next run, finding it already discoverable, doesn't take it over (`bluetooth.rs:308`) (confidence: medium)
- **Related:** `desktop/aizome/crates/aizome-sys/src/bluetooth/mod.rs` (`set_discoverable`, `start_discovery`, `register_agent`), `desktop/aizome/crates/aizome-sys/src/bluetooth/agent.rs` (`Registration` unregisters on drop)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#stb-09)

<a id="stb-10"></a>

### STB-10 — Pairing questions: prompt, responders and per-device busy
- **Where:** `desktop/aizome/apps/settei/src/panes/bluetooth.rs:254` (`Bluetooth.busy`, `.prompt` `:255`, `.responders` `:256`, `.next_prompt` `:257`)
- **Encoding:** `prompt: Option<Prompt>` (the question shown and the code typed), `responders: Arc<Mutex<HashMap<u64, Responder>>>` filled on the agent's thread, `busy` (the device paths with a task of ours running), the sample's id counter, and the window's sheet (STB-02)
- **States:** per device `Idle` → `Busy` → `Idle`; question `None` → `Asking(id, request, typed)` → `Answered` | `Superseded` | `Cancelled` | `Moot` (pairing ended, or the device paired by itself)
- **Transitions:** `Connect`/`Disconnect`/`Pair` mark the device busy (`bluetooth.rs:521`, `:526`, `:531`); the sample's `Pair` makes up a prompt (`bluetooth.rs:540-548`); the agent thread stores the responder, then sends `Prompted` (`bluetooth.rs:443-451`); `Prompted(Cancelled)` closes that device's question (`bluetooth.rs:588-594`); any other `Prompted` drops the old responder (which rejects) and opens the sheet (`bluetooth.rs:601-607`); `PromptTyped` (`bluetooth.rs:612-616`); `Answer`, or `back`, → `answer` responds, cancels a display-only pairing, clears busy on reject (`bluetooth.rs:332-370`, `:753-755`); `Done` clears busy and closes our pairing's question (`bluetooth.rs:625-631`); `system_changed` closes a shown code once the device has paired (`bluetooth.rs:466-472`)
- **Hazards:** a `Cancelled` prompt's `Responder::none` is stored under a fresh id that nothing removes, so `responders` leaks one entry per cancel; a question from device B silently rejects A's; `busy` clears only when the task returns, so a task that never completes spins forever; `close_prompt` closes the window's sheet whoever owns it (STB-02)
- **Reify:** High — five handlers and an agent thread share the map, and "dropped means rejected" is an implicit transition
- **Related:** `desktop/aizome/crates/aizome-sys/src/bluetooth/agent.rs` (BlueZ `Agent1`; a dropped `Responder` rejects)

<a id="stb-11"></a>

### STB-11 — Typed drafts committed on Return or blur
- **Where:** `desktop/aizome/apps/settei/src/panes/bluetooth.rs:249` (`Bluetooth.name`, `.expanded` `:251`, `.device_name` `:252`); also `desktop/aizome/apps/settei/src/panes/desktop.rs:27` (`Desktop.drafts: HashMap<usize, String>`)
- **Encoding:** an `Option` or map of the text being typed, shown in place of the live value until committed; `expanded` names the device whose options (and name field) are open
- **States:** `Viewing` → `Editing(text)` → `Committed` | `Discarded` (empty or unchanged)
- **Transitions:** Bluetooth `NameText` (`bluetooth.rs:510`), `Rename` takes the draft (`bluetooth.rs:511-519`); `Expand` loads `device_name` and toggles (`bluetooth.rs:553-557`); `RenameDevice` (`bluetooth.rs:563-569`); `ForgetNow`/`back` close the options (`bluetooth.rs:583`, `:757`); Desktop `DeskName` (`desktop.rs:173-175`), `SaveDesk` (`desktop.rs:176-190`), `RemoveConfirmed` clears every draft (`desktop.rs:237`)
- **Hazards:** Desk drafts and `RemoveDesk`'s confirmation (`desktop.rs:224-235`) are keyed by index, so if the Desk list changes on disk while someone types or confirms, the name or the removal lands on another Desk; `device_name` isn't refreshed when the device is renamed elsewhere; a failed adapter rename has already dropped its draft
- **Reify:** Low — local edit buffers

<a id="stb-12"></a>

### STB-12 — Arrangement drag gesture
- **Where:** `desktop/aizome/apps/settei/src/panes/displays/arrangement.rs:143` (`State.drag: Option<Drag>`; `Drag.moved` `:134`, `.fit` `:136`, `.shown` `:139`)
- **Encoding:** widget state the kit keeps across rebuilds: `drag` present means the pointer is down on a tile; `moved` means it has passed a 4 pt slop; `fit` is frozen at the press; `shown` holds damage rects
- **States:** `Idle` → `Pressed` → `Dragging` → `Dropped` | `Clicked`; no `Cancelled`
- **Transitions:** `PointerDown` on a tile captures the pointer and selects (`arrangement.rs:349-369`); `PointerMove` past the slop sets `moved` (`arrangement.rs:374-393`); `PointerUp` releases and emits the move if it moved (`arrangement.rs:401-411`); arrow keys emit moves directly (`arrangement.rs:413-428`)
- **Hazards:** every event is ignored while `enabled` is false (`arrangement.rs:344-346`), `PointerUp` included, so a drag under way when the pane starts applying keeps its capture; there is no Escape cancel; `fit`, frozen at the press, ignores a resize mid-drag; a tile removed by an outputs event mid-drag is dropped silently
- **Reify:** Medium — a gesture with an unhandled exit; an explicit enum with `Cancel` would fix the stuck capture
- **Suspected bug:** pressing an arrow key while holding a tile down sends `Moved` (`arrangement.rs:426-427`); `commit` sets `applying` (`displays/mod.rs:263`) and the view disables the widget (`displays/view.rs:118`); the `PointerUp` that follows is ignored, leaving the pointer captured and the tile drawn as a ghost until the next press (confidence: low)
- **Reproduction:** not attempted; only suspected bugs on High machines were reproduced

<a id="stb-13"></a>

### STB-13 — Display change: apply, "Keep these display settings?" countdown, keep or revert
- **Where:** `desktop/aizome/apps/settei/src/panes/displays/mod.rs:110` (`Displays.applying`, `.revision` `:111`, `.keep: Option<Keep>` `:112`); `Keep` at `desktop/aizome/apps/settei/src/panes/displays/mod.rs:49` (`before`, `before_main`, `names`, `left`, `timer`); also `desktop/aizome/apps/settei/src/panes/displays/system.rs:168` (`Compositor::apply`), `desktop/aizome/apps/settei/src/panes/displays/view.rs:395` (`keep_sheet`)
- **Encoding:** the `applying` bool, the `revision` counter and the `keep` option, which carries the old settings and a repeating 1 s timer; the sheet (STB-02); the settings file is written only on Keep
- **States:** `Idle` → `Applying` → `Idle` (persisted) | `Keeping(left)` → `Kept` (persisted) | `Reverting` → `Applying(revert)` → `Idle`; `Applying` → `Failed` → `Reading`. Representable: `keep` without a timer; a revert's `Applying` with nothing left to fall back to
- **Transitions:** `change` refuses while applying or keeping (`displays/mod.rs:214-217`); `commit` sets `applying`, bumps `revision`, assumes the new settings, spawns `apply` (`displays/mod.rs:263-269`); `Applied Ok` clears `applying` (`displays/mod.rs:555`), then either arms `every(1 s, Tick)`, `keep` and the sheet (`displays/mod.rs:578-581`) or persists (`displays/mod.rs:583`); `Applied Err` bumps `revision` and reads again (`displays/mod.rs:587-591`); `Tick` counts down and reverts at 0 (`displays/mod.rs:594-605`); `Keep` → `end_keep` + persist (`displays/mod.rs:607-611`); `Revert`/`back` → `revert` → `commit(confirm = false, persist = false)` (`displays/mod.rs:330-344`, `:612`, `:626-628`)
- **Hazards:** the countdown lives only in Settei: no `hidden`/quit hook ends it, and the compositor has no timeout of its own; `Compositor::apply` sends the requests one by one and returns early on error (`system.rs:180-182`), so an I/O error mid-sequence, or the 10 s read timeout (`system.rs:179`), returns `Err` after some requests already applied, and the error path starts no countdown; a revert whose apply fails isn't retried, leaving the risky settings live (the file still has the old ones); `Loaded` is guarded by `applying`/`revision` but not by `keep` (`displays/mod.rs:496`), so a read that finishes during the countdown (the pane shown again) takes `main` from the file's old `primary`, and Keep then persists the old main display
- **Reify:** High — three or more fields, eight handlers, a timer, and a property that protects against a blank screen
- **Suspected bug:** closing the window or quitting (⌘Q) during the 15 s countdown exits without `revert`, leaving a resolution, rotation, mirror or off change applied in the compositor though it was never kept (and isn't in the file) (confidence: medium). Also: a modeset slower than the 10 s read timeout returns `Err`, so no countdown starts and the change stays live, unconfirmed and unpersisted (confidence: low)
- **Related:** `output-config`/`mirror-output` handling in `desktop/aizome/crates/aizome-comp-core/src/requests.rs` and `desktop/aizome/crates/aizome-comp-core/src/outputs.rs` (`configure_output`)
- **Reproduction:** Partly confirmed (integration): [test and output](reproductions.md#stb-13)

<a id="stb-14"></a>

### STB-14 — Displays read and watch, and Night Light state
- **Where:** `desktop/aizome/apps/settei/src/panes/displays/mod.rs:115` (`Displays.watching: Option<Arc<AtomicBool>>`, `.loaded` `:108`, `.error` `:109`, `.outputs` `:118`, `.night` `:121`, `.fractional` `:114`); also `desktop/aizome/apps/settei/src/panes/displays/system.rs:193` (`Compositor::watch`)
- **Encoding:** a stop flag per watcher thread; `loaded`/`error` for the first read; the last outputs list, kept to re-derive on settings changes; `revision` (STB-13), which drops stale reads; the watcher's connect → stream → reconnect loop
- **States:** pane `Unloaded` → `Loaded` | `LoadError`; watcher `Off` → `Connecting` → `Streaming` → (`Error` → sleep 2 s → `Connecting`) → `Stopped`
- **Transitions:** `shown` picks the backend once, reads, asks for Night Light, and starts a watcher if there's none (`displays/mod.rs:447-459`); `hidden` sets stop (`displays/mod.rs:463-465`); the watcher checks stop on each event and each 1 s timeout, and reconnects after errors (`system.rs:198-225`); `Loaded` (`displays/mod.rs:494-509`); `Outputs` is ignored while applying or keeping (`displays/mod.rs:510-514`); `settings_changed` re-takes the last list (`displays/mod.rs:468-476`); `take` keeps displays that are off, re-picks `main`/`selected`, and forces `fractional` on (`displays/mod.rs:172-205`); `Night` (`displays/mod.rs:486`); `NightLight` (`displays/mod.rs:487-493`)
- **Hazards:** a stopped watcher lingers for up to its 1 s timeout or 2 s sleep, and showing the pane again meanwhile starts a second thread (separate flags, so no cross-talk); `take` forces `fractional = true` whenever any display has a fractional scale (`displays/mod.rs:200-202`), so turning the Fractional scaling switch off springs it back on at the next outputs event; while hidden, no events are followed
- **Reify:** Medium — a watcher thread's lifecycle plus read gating spread over six handlers
- **Related:** `outputs-changed` from `desktop/aizome/crates/aizome-comp-core/src/outputs.rs` (`outputs_changed`, `all_info`)

<a id="stb-15"></a>

### STB-15 — Backlight brightness request coalescing
- **Where:** `desktop/aizome/apps/settei/src/panes/displays/mod.rs:60` (`Brightness.pending` `:65`, `.in_flight` `:66`, `.shown` `:68`)
- **Encoding:** the `in_flight` bool, a latest-wins `pending` level, and `shown`, which overrides the level the backlight reports
- **States:** `Idle` → `InFlight` → `InFlight + Pending` → `InFlight` (next level sent) → `Idle`
- **Transitions:** `set_brightness` shows the level, then sends or queues it (`displays/mod.rs:348-360`); `BrightnessDone` clears `in_flight`, reports errors, then sends the pending level or drops `shown` (`displays/mod.rs:543-552`)
- **Hazards:** `shown` is dropped when the task returns, not when the backlight reports the level as documented (`displays/mod.rs:67`), so the slider can jump back to the old level for a moment; a task that never returns wedges `in_flight`, and the slider stops changing the backlight
- **Reify:** Medium — a classic busy + queued pair, small but easy to get wrong
- **Related:** `desktop/aizome/crates/aizome-sys/src/power/backlight.rs`

<a id="stb-16"></a>

### STB-16 — What a display is used as (main, extended, mirror, off): fields and `[[outputs]]` keys
- **Where:** `desktop/aizome/apps/settei/src/panes/displays/model.rs:147` (`Settings.enabled`, `.mirror`) with `Displays.main` (`desktop/aizome/apps/settei/src/panes/displays/mod.rs:106`); persisted by `write_entries` (`desktop/aizome/apps/settei/src/panes/displays/model.rs:733`); also `desktop/aizome/apps/settei/src/panes/displays/view.rs:181`
- **Encoding:** partially explicit: `enum Use` (`model.rs:571`) is only the request; the state is `enabled` × `mirror` × `main`, written to `settings.toml` as `enabled`, `mirror = "…"` and `primary = true`, and rebuilt by `from_output` (`model.rs:197`), `known_off` (`model.rs:784`), `infer_mirrors` (`model.rs:822`) and the view (`view.rs:181-189`)
- **States:** `Main` | `Extended` | `Mirror(source)` | `Off`. Representable invalid: `!enabled && mirror.is_some()`; a mirror of a mirror or of a display that's off; `main` naming a display outside the arrangement; two entries marked `primary`
- **Transitions:** `use_as` (`model.rs:582-643`), via `Msg::UseAs` (`displays/mod.rs:520-522`); `main_display` (`model.rs:656-665`), from `take`; `write_entries` sets or removes `mirror`/`primary` and strips other entries' `primary` (`model.rs:754-778`); `infer_mirrors` turns coincident displays into mirrors and drops mirrors whose source is off (`model.rs:827-844`)
- **Hazards:** invariants are restored by repair passes rather than prevented; `infer_mirrors` treats two displays placed at the same origin by a hand edit as a mirror nobody asked for; `known_off` revives only entries keyed by connector (`model.rs:789`), so a display turned off and kept by make and model shows only while the compositor still lists it
- **Reify:** Medium — the four-state enum already exists, and storing it would remove the repairs
- **Related:** `[[outputs]]` `mirror`/`primary` read in `desktop/aizome/crates/aizome-config/src/compositor.rs`; mirrors in `desktop/aizome/crates/aizome-comp-core/src/outputs.rs`

<a id="stb-17"></a>

### STB-17 — settings.toml write pipeline: ordered writer, pending count, reload debounce
- **Where:** `desktop/aizome/apps/settei/src/shared.rs:37` (`Shared.reload_pending`, `.writer: Option<Writer>` `:38`); `Writer` at `desktop/aizome/apps/settei/src/shared.rs:204` (`sent`, the `done` counter and its condvar); also `desktop/aizome/apps/settei/src/main.rs:52`
- **Encoding:** a writer thread, started lazily and fed by a channel; `sent` (main thread) against `done` (writer) gives `pending()`; `reload_pending` debounces `reload-config` to one per 120 ms; the file's mtime and length, polled each second by `aizome-config`'s watcher
- **States:** `NoWriter` → `Idle` (`pending == 0`) ⇄ `Writing(n)`; reload `Idle` → `Scheduled` → `Requested`; file `Fresh` | `OursPending` | `ExternalUnseen`
- **Transitions:** `change` reads the file fresh only when nothing is pending (`shared.rs:93-107`), applies the edit, starts the writer once (`shared.rs:119`), bumps `sent` and queues the text (`shared.rs:120-121`); the writer writes atomically, bumps `done`, reports `Written` or `Said` (`shared.rs:217-227`); `written` arms the reload once (`shared.rs:127-130`); `reload` clears the flag and asks the compositor (`shared.rs:142-161`); `reread` refuses while anything is pending (`shared.rs:74-76`); `flush` waits for `done == sent` at exit (`shared.rs:134-137`, `main.rs:52`)
- **Hazards:** while writes are pending, `change` builds on the cached settings and `reread` drops the watcher's one notice for that stamp, so an external edit landing mid-burst is neither shown nor merged, and the next queued write overwrites it; the temporary name `settings.toml.tmp` (`shared.rs:256`) is the one `aizome-config`'s own save uses (`desktop/aizome/crates/aizome-config/src/lib.rs:691`), so a concurrent save by the shell or another app can rename the other's half-written file, or fail; if the writer thread fails to spawn (ignored, `shared.rs:216`), `pending()` never drains: rereads stop for good and `flush` hangs at quit
- **Reify:** High — risk of data loss, counters shared across threads, and ordering across processes
- **Suspected bug:** while a slider is dragged (writes pending throughout), the shell writes `settings.toml` (e.g. the Deskbar's edge dragged to a new width). `reread` drops the watcher's notice (`shared.rs:74`), the next `change` uses the cached settings (`shared.rs:95`), and the shell's change is overwritten (confidence: medium)
- **Related:** `aizome_config::Watcher` (`desktop/aizome/crates/aizome-config/src/lib.rs:823`); `reload-config` in `desktop/aizome/crates/aizome-comp-core/src/requests.rs`
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#stb-17)

<a id="stb-18"></a>

### STB-18 — Picture cache validated by file mtime
- **Where:** `desktop/aizome/apps/settei/src/widgets.rs:854` (`Pictures(RefCell<HashMap<String, (Option<SystemTime>, Option<Image>)>>)`); also `desktop/aizome/apps/settei/src/app.rs:114`
- **Encoding:** for each path, the mtime when read and the image (`None` when unreadable)
- **States:** `Absent` → `Cached(stamp, image | none)` → read again once the stamp differs
- **Transitions:** `get` stats the file, returns the cached image if the stamp matches, else reads it and replaces the entry (`widgets.rs:858-869`); called from `refresh_picture` on every `SystemChanged` (`app.rs:255-258`, `:563`)
- **Hazards:** a replacement within the filesystem's mtime granularity, or one that preserves the mtime, keeps showing the old picture; a failed read is cached as `None` until the stamp changes; entries are never evicted
- **Reify:** Low — a cache with a validity key; fine to leave implicit
- **Related:** AccountsService user icons (`/var/lib/AccountsService/icons/<user>`) via `desktop/aizome/crates/aizome-sys/src/users.rs`

<a id="stb-19"></a>

### STB-19 — (test harness) settle loops that pump the window until background work lands
- **Where:** `desktop/aizome/apps/settei/tests/budget_hw.rs:14` (`settle`); also `tests/devices.rs:101`, `tests/displays.rs:51`, `tests/displays_live.rs:34`, `tests/network.rs:120`, `tests/goldens_hw.rs:40`, `tests/live_snapshot.rs:24`, `src/panes/bluetooth.rs:1083`, `src/panes/displays/mod.rs:698` (all under `desktop/aizome/apps/settei/`)
- **Encoding:** a bounded loop (400 × 5 ms; 600 in `displays_live`, 40 in `live_snapshot`) that sleeps and sends `AppMsg::Nothing` until a predicate on the window's texts holds
- **States:** `Waiting` → `Ready` | `GaveUp`
- **Transitions:** predicate true → return (`budget_hw.rs:17-18`); otherwise sleep and pump (`budget_hw.rs:20-21`); when the bound runs out, most copies return silently, `displays_live.rs:42` returns `false`, `displays/mod.rs:707` panics, and `goldens_hw.rs:47` asserts after the loop
- **Hazards:** most copies give up silently after about 2 s, so on a slow machine the failure surfaces later at an unrelated assertion; the wall-clock bound is duplicated in nine places
- **Reify:** Low — a test helper; one shared `settle` that returns a `Result` would do

<a id="stb-20"></a>

### STB-20 — (test harness) live headless-compositor scenario for Displays
- **Where:** `desktop/aizome/apps/settei/tests/displays_live.rs:59` (`arranges_and_applies_on_a_headless_compositor`)
- **Encoding:** a scripted sequence of phases against a compositor started outside the test (`AIZOME_SOCKET`, `SETTEI_COMPOSITOR_SETTINGS`), each phase gated by polling `outputs()` or the settings file
- **States:** `Skipped` | `Loaded` → `Dragged` → `Persisted` → `Rotated (keeping)` → `RevertedByTimeout` → `Mirrored (keeping)` → `Reverted` → `MainChangedElsewhere`
- **Transitions:** drag (`displays_live.rs:85`), then poll the compositor (`displays_live.rs:96-104`); wait for `[[outputs]]` in the file (`displays_live.rs:108`); rotation menu (`displays_live.rs:116-122`); `advance(16 s)` through the countdown (`displays_live.rs:128-132`); mirror through Use as (`displays_live.rs:155`); Revert (`displays_live.rs:158-160`); external edit plus `ReloadConfig` (`displays_live.rs:165-172`)
- **Hazards:** a failure mid-script leaves the shared compositor rotated or mirrored for the next run; phases depend on menu row order
- **Reify:** Low — a manual test, ignored by default
- **Related:** the headless compositor's IPC in `desktop/aizome/crates/aizome-comp-core/src/requests.rs`

## Already explicit

- `desktop/aizome/apps/settei/src/system/accounts.rs:12` `enum Accounts` — AccountsService backend, None/Live/Sample; transitions centralised in `fn start` (`desktop/aizome/apps/settei/src/system/mod.rs:33`), called once from `desktop/aizome/apps/settei/src/app.rs:484`
- `desktop/aizome/apps/settei/src/panes/bluetooth.rs:34` `enum Bt` — BlueZ backend, None/Live/Sample; transitions centralised in `fn shown` (`desktop/aizome/apps/settei/src/panes/bluetooth.rs:418-433`), once
- `desktop/aizome/apps/settei/src/panes/battery.rs:26` `enum Batteries` and `:31` `enum Modes` — UPower and power-profiles backends, Live/Sample; transitions centralised in `fn shown` (`desktop/aizome/apps/settei/src/panes/battery.rs:310-321`), once
- `desktop/aizome/apps/settei/src/panes/displays/system.rs:14` `enum Compositor` — compositor backend, Live/Sample; transitions centralised in `fn start` (`desktop/aizome/apps/settei/src/panes/displays/mod.rs:151-162`), once

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/apps/settei/src/app.rs` | 664 | machines | [STB-01](#stb-01), [STB-02](#stb-02), [STB-03](#stb-03), [STB-18](#stb-18) | navigation, sheet ownership, reveal timer; picture cache user |
| `desktop/aizome/apps/settei/src/extra.rs` | 80 | none |  | toml key helpers |
| `desktop/aizome/apps/settei/src/form.rs` | 278 | none |  | form view builders |
| `desktop/aizome/apps/settei/src/instance.rs` | 53 | machines | [STB-04](#stb-04) | single-instance socket |
| `desktop/aizome/apps/settei/src/lib.rs` | 31 | none |  | module list |
| `desktop/aizome/apps/settei/src/listing.rs` | 89 | none |  | pure listing of panes and options |
| `desktop/aizome/apps/settei/src/main.rs` | 61 | machines | [STB-04](#stb-04), [STB-17](#stb-17), [STB-01](#stb-01) | claim, run, flush, release; no pane teardown |
| `desktop/aizome/apps/settei/src/pane.rs` | 817 | machines | [STB-01](#stb-01), [STB-02](#stb-02) | pane lifecycle contract and sheet calls |
| `desktop/aizome/apps/settei/src/panes/accessibility.rs` | 239 | machines | [STB-05](#stb-05) | remembered theme and size keys |
| `desktop/aizome/apps/settei/src/panes/appearance.rs` | 547 | machines | [STB-06](#stb-06), [STB-05](#stb-05) | wallpaper scan; theme_before_contrast |
| `desktop/aizome/apps/settei/src/panes/battery.rs` | 757 | machines | [STB-07](#stb-07), [STB-08](#stb-08) | services and history; Keep Awake |
| `desktop/aizome/apps/settei/src/panes/bluetooth.rs` | 1156 | machines | [STB-09](#stb-09), [STB-10](#stb-10), [STB-11](#stb-11), [STB-19](#stb-19) | discovery, pairing prompts, drafts; Bt explicit |
| `desktop/aizome/apps/settei/src/panes/desktop.rs` | 472 | machines | [STB-06](#stb-06), [STB-11](#stb-11) | apps lazy load; desk drafts by index |
| `desktop/aizome/apps/settei/src/panes/displays/arrangement.rs` | 490 | machines | [STB-12](#stb-12) | drag gesture |
| `desktop/aizome/apps/settei/src/panes/displays/mod.rs` | 775 | machines | [STB-13](#stb-13), [STB-14](#stb-14), [STB-15](#stb-15), [STB-19](#stb-19) | apply/keep/revert, watch, brightness |
| `desktop/aizome/apps/settei/src/panes/displays/model.rs` | 1065 | machines | [STB-16](#stb-16) | use mode as fields and [[outputs]] |
| `desktop/aizome/apps/settei/src/panes/displays/system.rs` | 240 | machines | [STB-13](#stb-13), [STB-14](#stb-14) | apply sequence, watch reconnect; Compositor explicit |
| `desktop/aizome/apps/settei/src/panes/displays/view.rs` | 418 | machines | [STB-13](#stb-13), [STB-16](#stb-16) | keep sheet; use mode derived |
| `desktop/aizome/apps/settei/src/panes/mod.rs` | 97 | none |  | pane registry and aliases |
| `desktop/aizome/apps/settei/src/preview.rs` | 254 | none |  | pure painting |
| `desktop/aizome/apps/settei/src/search.rs` | 247 | none |  | pure matching |
| `desktop/aizome/apps/settei/src/shared.rs` | 259 | machines | [STB-17](#stb-17), [STB-03](#stb-03) | settings writer and reload; status line |
| `desktop/aizome/apps/settei/src/system/accounts.rs` | 257 | explicit |  | Accounts backend enum; sample fixtures |
| `desktop/aizome/apps/settei/src/system/mod.rs` | 69 | explicit |  | System::start picks backend; sample Cell |
| `desktop/aizome/apps/settei/src/system/sample.rs` | 7 | none |  | constants |
| `desktop/aizome/apps/settei/src/tz.rs` | 346 | none |  | pure TZif and POSIX rule parsing |
| `desktop/aizome/apps/settei/src/wallpaper.rs` | 135 | none |  | pure file scan and thumbnails |
| `desktop/aizome/apps/settei/src/widgets.rs` | 896 | machines | [STB-18](#stb-18) | picture cache; press widget state is the kit's |
| `desktop/aizome/apps/settei/tests/budget.rs` | 95 | none |  | tests |
| `desktop/aizome/apps/settei/tests/budget_hw.rs` | 142 | machines | [STB-19](#stb-19) | settle loop; drag via press/move/release |
| `desktop/aizome/apps/settei/tests/common/mod.rs` | 43 | none |  | fixture temp dirs |
| `desktop/aizome/apps/settei/tests/desktop.rs` | 36 | none |  | tests |
| `desktop/aizome/apps/settei/tests/devices.rs` | 125 | machines | [STB-19](#stb-19) | settle loop |
| `desktop/aizome/apps/settei/tests/displays.rs` | 94 | machines | [STB-19](#stb-19) | settle loop |
| `desktop/aizome/apps/settei/tests/displays_live.rs` | 173 | machines | [STB-20](#stb-20), [STB-19](#stb-19) | live compositor scenario |
| `desktop/aizome/apps/settei/tests/goldens.rs` | 106 | none |  | tests |
| `desktop/aizome/apps/settei/tests/goldens_hw.rs` | 93 | machines | [STB-19](#stb-19) | settle loop in open |
| `desktop/aizome/apps/settei/tests/keyboard.rs` | 336 | none |  | tests |
| `desktop/aizome/apps/settei/tests/live_snapshot.rs` | 31 | machines | [STB-19](#stb-19) | fixed pump loop |
| `desktop/aizome/apps/settei/tests/network.rs` | 165 | machines | [STB-19](#stb-19) | settle loop |
| `desktop/aizome/apps/settei/tests/tour.rs` | 62 | none |  | tests |
| `desktop/aizome/apps/settei/tests/window.rs` | 293 | none |  | tests |
