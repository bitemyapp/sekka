# HOT: Hotaru's Aizome frontend and link matcher

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/apps/hotaru, desktop/aizome/crates/hotaru-link-regex`. Audited at `8e0acbe5`; line numbers refer to that commit.

51 files audited. 40 implicit machines: 10 High, 9 Medium, 21 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

<a id="hot-02"></a>

### HOT-02 — Pane lifecycle: live, ended, failed, removed, window teardown
- **Where:** `desktop/aizome/apps/hotaru/src/app.rs:136` (`Pane.session`), `:143` (`.ended`), `:138` (`.status`); `Hotaru.tabs`/`.panes` `:182-183`
- **Encoding:** `session: Option<Session>`, `ended: bool`, `status: String`, pane membership in a tab's `PaneTree`; `settings.wait_after_command` chooses kept vs removed
- **States:** `Live` (session Some, !ended) → `EndedKept` (session Some, ended; PTY worker still holds history) | `Failed` (session None, ended) | `Removed`; also `SpawnFailed`/`Preview` (session None, !ended); last tab removed → `WindowClosing` (or deferred, HOT-12)
- **Transitions:** spawn `new_pane` `app.rs:391-399` → Live/SpawnFailed; `Exited` `:2069-2081` → remove (`:2073`) or EndedKept (`:2080`); `Failed` `:2083-2096` (`:2094-2095`); key on ended pane `:2121-2127`; `close_pane` `:509-513`, `close_tab` `:481-485`; `remove_pane` `:517-537`; `remove_tab` `:488-507` (window close `:492-502`); `close_requested` clears all `:2655-2659`
- **Hazards:** the ended-pane key path returns from `update` early (`app.rs:2127`), skipping the post-match reconciliation (`:2408-2420`: automatic-link cancel, `recent_tabs`, `sync_search`) until the next message; `SpawnFailed` panes never take the "press a key to close" path and search treats them as Preview (`:718-720`); handlers re-check `!ended` independently (`:861`, `:921`, `:1008`, `:2269`); 64-pane cap checked in two places (`:427`, `:466`)
- **Reify:** High — three fields, seven mutation sites, reachable `session None && !ended` combination
- **Related:** `session.rs` PTY worker (HOT-21) on the other side of `Msg::Exited`/`Msg::Failed`

<a id="hot-07"></a>

### HOT-07 — Automatic-link activation pipeline (match, hover, resolve, open)
- **Where:** `desktop/aizome/apps/hotaru/src/app.rs:211` (`automatic_worker`), `:212` (`automatic_request`), `:213` (`automatic_hit`), `:214` (`resolver_worker`), `:215` (`resolver_request`); `Pane.hovered_link` `:142`
- **Encoding:** `Arc` identity of `automatic_links::Request` checked with `Arc::ptr_eq` against the pane's displayed snapshot; resolver request identity; two lazily created latest-wins workers
- **States:** `Idle` → `Matching(request)` → `Hit(link)` | `NoHit` → `Resolving(resolver_request)` → `Opening` → `Opened` | `Failed(status)`; any → `Cancelled`; invalid: `automatic_hit`/`resolver_request` without `automatic_request` (prevented by `cancel_automatic`)
- **Transitions:** valid `MatchLink(Some)` cancels then sets and submits `app.rs:1919-1943`; `MatchLink(None)` for the same pane cancels `:1944-1946`; `MatchedLink` sets hit `:1948-1967`; `HoverLink` `:1968-1975`; `OpenLink` automatic → `resolver_request` + submit `:1977-2000`, explicit → opener `:2001-2005`; `ResolvedLink` `:2008-2036` (clears `:2017`); `LinkOpened` failure `:2037-2046`; end-of-update cancel when the displayed frame changed `:2408-2415`; `cancel_automatic` `:539-549`; workers dropped on close `:2648-2650`
- **Hazards:** `cancel_automatic` always cancels the resolver too, so any new hover request (another cell, another pane, pointer leave, focus loss) or any newly displayed frame aborts an activation in flight with no notice; the end-of-update check is skipped by early-returning arms
- **Suspected bug:** ❖-click a relative path (resolved by a spawned helper, up to 250 ms) and move the pointer one cell or out of the pane before the helper answers: `terminal_view.rs:441-455`/`:484` emits `MatchLink`, `app.rs:1929`/`:1945` cancels `resolver_request`, `ResolvedLink` finds nothing and the link silently never opens (confidence: medium)
- **Reify:** High — five fields, identity invariants, user-visible silent failure
- **Related:** `terminal_view.rs` hover requests (HOT-29); workers in `automatic_links/mod.rs`, `link_resolver.rs` (HOT-15, HOT-18); aizome-kit `urls` opener
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#hot-07)

<a id="hot-10"></a>

### HOT-10 — Program clipboard request flow (OSC 52 / OSC 5522) and the single question
- **Where:** `desktop/aizome/apps/hotaru/src/app.rs:232` (`asking`), `:233` (`ask_token`), `:228` (`selections`); also `desktop/aizome/apps/hotaru/src/selections.rs:317` (`Question`)
- **Encoding:** one global `Option<Question>` (token, request, captured reply) shown as an alert; settings `clipboard_read`/`clipboard_write`; session grants held in the PTY worker's terminal
- **States:** `Requested` → `Refused` | `Allowed` | `Capturing` (selections worker) → `Captured` → `Asking(token)` → `Answered(Allow|Remember|Refuse)` → `Replied`; concurrent requests while `Asking` → `Busy`
- **Transitions:** `program_clipboard` `app.rs:859-916`; `ask` `:932-994` (busy `:941-949`, set `:993`); `ClipboardCaptured` re-checks settings `:2165-2187`; `ClipboardAnswer` token match `:2188-2192`; `answer` `:999-1042` (grant `:1016`, `:1026`)
- **Hazards:** one question for all panes; it is not cleared when its pane closes, so every other pane's prompted requests get the "another waits"/EBUSY refusal until someone answers; answers for ended panes are dropped silently (`:1008-1010`); a refused OSC 52 read under `Deny` gets no reply at all (`:901-904`, `:2182`)
- **Reify:** High — privacy-sensitive, five handlers, settings re-read at two points
- **Related:** `session.rs:439-453` (requests from the PTY worker), `selections.rs` worker and tickets (HOT-20), hotaru-core `kitty_clipboard` grants, aizome-kit alerts

<a id="hot-12"></a>

### HOT-12 — Settings persistence, reload and deferred window close
- **Where:** `desktop/aizome/apps/hotaru/src/app.rs:204` (`settings_worker`), `:205` (`settings_token`), `:206` (`settings_pending`), `:207` (`closing`), `:221` (`settings_managed`), `:226` (`open_after_save`), `:219` (`settings_notice`); also `desktop/aizome/apps/hotaru/src/settings.rs:768` (`Queue`), `desktop/aizome/apps/hotaru/src/main.rs:84`
- **Encoding:** token per save/load, pending flag, close latch, open-editor latch, managed (symlink) flag, `hotaru.toml` written by temp file + rename (`settings.rs:443-475`)
- **States:** `Clean` → `Saving(t)` → `Saved` | `SaveFailed` (→ `Managed` if now a symlink); `Reloading(t)` → `Loaded` | `LoadFailed`; `Closing` (close or last tab while saving) → `Closed` on matching `ConfigSaved`; `OpenAfterSave`; invalid: `closing && !settings_pending`, `open_after_save` with no save outstanding
- **Transitions:** `save_settings` `app.rs:1260-1269`; `ReloadConfig` `:1369-1376`; `ConfigLoaded` `:1708-1725`; `ConfigSaved` `:1726-1755` (managed `:1738`, open `:1746`, close `:1751-1754`); last tab `:498-502`; `close_requested` `:2651-2654`; `open_config_file` `:1211-1223`; initial load/managed `main.rs:75-86`
- **Hazards:** `ConfigLoaded` ignores `closing`; `ReloadConfig` clears `settings_pending` and drops the result of an in-flight save; the worker's single slot lets a Load replace a queued Save (`settings.rs:804-808`); no timeout, so a worker that never answers keeps a requested close from completing
- **Suspected bug:** close the window (or end its last tab) while a save is in flight, then press ⇧❖, before it lands: the token bump (`app.rs:1370-1371`) makes `ConfigSaved` stale (`:1727`), `ConfigLoaded` never checks `closing`, so the window stays open with `closing` latched and a later unrelated settings change closes the window and Settings unexpectedly (`:1751-1754`); `open_after_save` latches the same way and opens hotaru.toml later (confidence: low)
- **Reify:** High — seven fields, eight handlers, reachable invalid combinations
- **Related:** settings worker (HOT-15); Nix/Home Manager–managed hotaru.toml symlinks
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#hot-12)

<a id="hot-14"></a>

### HOT-14 — Appearance ownership between the desktop watcher and Hotaru's theme
- **Where:** `desktop/aizome/apps/hotaru/src/app.rs:208` (`desktop_appearance`), `:200` (`colors`), `settings.theme`; also `desktop/aizome/apps/hotaru/src/terminal_view.rs:249` (`ViewState.theme`), `:250` (`theme_pending`)
- **Encoding:** a cached copy of the desktop's appearance, the configured theme override, the last colours pushed to PTY workers, and the view's last seen theme tuple
- **States:** `FollowDesktop` (desktop_appearance refreshed) | `Override(theme)` (desktop_appearance frozen); colours `Synced` ⇄ `Pending`
- **Transitions:** launch snapshot `app.rs:1545`; `SetTheme` `:1576-1580` → `apply_appearance` `:1246-1258`; `Msg::Theme` `:1756-1781` (refresh only under Desktop `:1757-1758`, override reapplied `:1760`); view emits on theme change `terminal_view.rs:653`, `:1069-1072`
- **Hazards:** the kit's settings watcher rethemes to the desktop's appearance on its own (`desktop/aizome/crates/aizome-kit/src/app.rs:289-294`), and Hotaru only refreshes `desktop_appearance` while following the desktop; under an override, desktop changes to other appearance fields are reverted with stale values
- **Suspected bug:** with Hotaru set to Night and the desktop on Day, switch the desktop to Night (no visible change in Hotaru, so no `Msg::Theme`), then choose Follow Desktop: `apply_appearance` uses the stale Day `desktop_appearance` and Hotaru shows Day while the desktop is Night, until the desktop changes again; the Appearance page previews use the same stale base (confidence: medium)
- **Reify:** High — ownership split across kit and app with a suspected bug
- **Related:** aizome-kit `desktop/aizome/crates/aizome-kit/src/app.rs:290` (`woken` settings watcher) and `desktop/aizome/crates/aizome-kit/src/cx.rs:377` (`set_appearance`)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#hot-14)

<a id="hot-17"></a>

### HOT-17 — Single-instance claim and launch handoff
- **Where:** `desktop/aizome/apps/hotaru/src/instance.rs:25` (`Instance {listener, _lock}`); also `desktop/aizome/apps/hotaru/src/main.rs:66`, `desktop/aizome/apps/hotaru/src/app.rs:194`
- **Encoding:** `flock` on `aizome-app.hotaru.lock`, the Unix socket file, a length-prefixed JSON request and a one-byte acknowledgement
- **States:** first: `LockHeld` → `Bound` (backlog only) → `Serving` → `Ended`; others: `LockBusy` → `Connecting` (≤2 s retry) → `Sent` → `Acked(Forwarded)` | `Error` → standalone app
- **Transitions:** lock and stale-socket removal `instance.rs:76-92`; connect retry `:101-113`; send/ack `:114-123`; `serve` thread `:127-155` (Launch `:148`, ack `:151`); claim in `main.rs:66-73`; served in `launched` `app.rs:1567-1571`; `Launch` opens a tab `:1800-1805`
- **Hazards:** the listener is bound before the event loop runs, but connections are only read once `launched()` calls `serve`; one connection is handled at a time with 500 ms timeouts; an error from `claim` makes the caller run standalone
- **Suspected bug:** if the first instance takes over 2 s from `claim` to `serve` (cold start), a second `aizome-hotaru` times out on the ack (`instance.rs:115`, `:119`) and opens its own window, while the first later reads the buffered request and opens a tab too: one launch, two terminals (confidence: low)
- **Reify:** High — cross-process handshake with a suspected bug
- **Related:** other `aizome-hotaru` processes (same file, client side)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#hot-17)

<a id="hot-19"></a>

### HOT-19 — Find: query, capture, whole-history search and navigation
- **Where:** `desktop/aizome/apps/hotaru/src/search.rs:17` (`State {worker, pane, token, cancel, in_flight, dirty, timer, caches, navigation, pending_navigation, retry_navigation, copy, error}`); also `desktop/aizome/apps/hotaru/src/app.rs:187-189` (`find`, `search`, `find_state`)
- **Encoding:** thirteen fields plus the PTY worker's search mailbox and the search worker slot; tokens for capture and navigation
- **States:** `Closed` → `Idle` → `Debouncing(timer)` → `Capturing(in_flight)` → `Searching(in_flight)` → `Ready(caches)` → `Navigating(pending_navigation)` → `Selected(copy)` | `RetryNavigation` | `Error`; invalid but representable: `in_flight && timer`, `copy && pending_navigation`, `retry_navigation && !dirty`
- **Transitions:** `Find` `app.rs:2294-2297`; `Search` `:2298-2303`; `sync_search` `:660-671`; `schedule_search` `:673-684`; `SearchRefresh` `:2304-2309`; `request_search` `:686-722`; `SearchSnapshot` `:2310-2329`; `SearchReady` `:2330-2372`; `SearchFailed` `:2373-2378`; `navigate_search` `:724-773`; `SearchNavigated` `:2380-2394`; frame handling `:1843-1877`; `Select` `:2205-2214`; `Failed` `:2083-2091`; `CloseFind` `:2395-2400`; `State::invalidate` `search.rs:54-67`; PTY side `session.rs:190-201`, `:532-549`
- **Hazards:** `in_flight` is set at `app.rs:701` before the pane lookup at `:704` (an early return leaves it set until the next `sync_search`); the PTY worker drops a cancelled Capture without reply (`session.rs:547`), so every cancel must go with `invalidate`'s reset; correctness relies on the timer always carrying the current token (each `invalidate` is preceded by taking the timer); early `return`s in `update` skip `sync_search`; `SearchFailed` doesn't reschedule even when dirty
- **Reify:** High — thirteen fields, about fifteen handlers, several invalid combinations
- **Related:** `session.rs` controls mailbox (HOT-23), search worker (HOT-15), font highlights `font.rs:286-290`, hotaru-core `search::Results`

<a id="hot-21"></a>

### HOT-21 — PTY worker lifecycle (spawn, run, EOF, hang-up, exit report, stop)
- **Where:** `desktop/aizome/apps/hotaru/src/session.rs:71` (`Shared.stopping`), `:74` (`Shared.child`), `:499` (`eof`, `hung_up`, `notified_exit`)
- **Encoding:** atomic stop flag and child pid shared with the UI; run-loop locals for reader EOF, master hang-up and exit-reported
- **States:** `Spawning` (≤1 s inherited-cwd wait) → `Running` → `HungUp` → `Eof` → `ExitReported` (worker keeps history until dropped) → `Stopped`; `Failed` (run error); invalid: `hung_up` without `eof` is a normal transient
- **Transitions:** thread start `session.rs:120-136` (cwd wait `:121-125`, stop check `:126-128`); `Pty::spawn` `:479`, pid `:480`; reader `End`/disconnect → eof `:702-705`, `:711-713`; hang-up on EIO `:758-760` or POLLHUP `:843-845`; exit report after EOF, no pending frame and reap `:787-797` (pid cleared `:793`); `Failed` on run error `:132-134`; stopping `:135`, `Drop` `:243-247`; commands refused once stopping `:163-165`
- **Hazards:** the child is reaped only after reader EOF; with EOF but no exit the loop polls every 100 ms indefinitely (`:807-808`); ended-but-kept panes keep both threads and the whole terminal alive
- **Suspected bug:** in bash, `sleep 600 &` then `exit`: the background job still holds the PTY slave, so EOF never arrives, `try_wait` is never polled (`:787-790`) and the pane neither closes nor shows "Session ended" until the job exits; terminals that watch the child report the exit at once (may be an accepted trade-off for lossless output) (confidence: medium)
- **Reify:** High — three loop flags plus two shared atomics, with a suspected bug
- **Related:** `app.rs` pane lifecycle (HOT-02); hotaru-io `Pty`
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#hot-21)

<a id="hot-27"></a>

### HOT-27 — Pointer drag selection and held-button tracking
- **Where:** `desktop/aizome/apps/hotaru/src/terminal_view.rs:245` (`ViewState.drag`), `:253` (`.buttons`), `:247` (`.scroll_fraction`)
- **Encoding:** drag anchor, list of held mouse buttons (sets `any_button` in mouse reports), fractional wheel accumulator; kit pointer capture
- **States:** `Idle` → `Pressed` → `Dragging(anchor)` → `Released` (→ `Selected` emitted); multi-click `Word`/`Line` selects at once; buttons `None` ⇄ `Held(list)`
- **Transitions:** press `terminal_view.rs:1150-1171` (buttons `:1153-1157`, anchor `:1166-1168`, capture `:1170`); extend `:1182-1196`; release `:1198-1222` (buttons `:1212`, `drag.take` `:1213`, `Selected` `:1218-1220`, `release_pointer` `:1221`); link press clears drag `:1143`; wheel accumulator `:1233-1236`
- **Hazards:** neither `PointerLeave` (`:1223-1228`) nor focus loss (`:1246-1265`) clears `drag` or `buttons`; the kit ends a press on leave only for `drags_only` widgets (`desktop/aizome/crates/aizome-kit/src/engine.rs:2065-2072`), which this view is not; a right/middle press "ours" never records a button but its release still sends a mouse release (`:1214-1217`)
- **Suspected bug:** begin a drag-select, then let the compositor take the pointer (one of its own grabs; the kit comment at `engine.rs:2065-2068` says the release then goes elsewhere): `drag` and `buttons` stay set, so later plain hover over the pane keeps extending the selection and reports button-held motion to mouse-tracking programs, and copy-on-select never fires, until the next click (confidence: medium)
- **Reify:** High — suspected bug; state spans view, kit capture and app selection
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#hot-27)

<a id="hot-29"></a>

### HOT-29 — Hover and automatic-link request lifecycle in the terminal view
- **Where:** `desktop/aizome/apps/hotaru/src/terminal_view.rs:254` (`hovered`), `:255` (`auto_request`), `:256` (`auto_revision`), `:257` (`auto_refresh_pending`), `:259` (`pointer`), `:260` (`link_modifier`), `:261` (`hover_clear_pending`)
- **Encoding:** last hit link, the outstanding request identity, the app-provided result's request identity, deferred refresh and clear flags, last pointer position and modifier
- **States:** `NoHover` → `Requested(request)` → `Hit(link)` | `Miss` → `Stale` (new displayed frame: clear pending, refresh pending) → `Requested`; `ModifierHeld` toggles cursor and underline
- **Transitions:** `hover` `terminal_view.rs:432-489` (request `:438-456`, emit `MatchLink` `:483-485`, `HoverLink` `:486-488`); revision change `:617-624`; new snapshot `:625-635`; deferred tick `:680-688` → `:1046-1063`; leave/focus loss `hover(None)` `:1227`, `:1258`; key presses refresh `:1093-1094`
- **Hazards:** while the pointer rests over text during continuous output, every displayed frame re-requests a match (`:630`), bounded only by the latest-wins worker; `HoverLink(None)` is deferred to the next tick, so the app's `hovered_link` can briefly reference an older frame (filtered at `app.rs:1906-1908`)
- **Reify:** High — seven fields, protocol with the app's identity checks
- **Related:** `app.rs` pipeline (HOT-07)
