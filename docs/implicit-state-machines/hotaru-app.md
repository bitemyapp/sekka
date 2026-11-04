# HOT: Hotaru's Aizome frontend and link matcher

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/apps/hotaru, desktop/aizome/crates/hotaru-link-regex`. Audited at `8e0acbe5`; line numbers refer to that commit.

51 files, 0 pending. 40 implicit machines: 10 High, 9 Medium, 21 Low.

## Machines

<a id="hot-01"></a>

### HOT-01 — (test harness) OSC 7 latency benchmark driver
- **Where:** `desktop/aizome/apps/hotaru/examples/cwd_latency.rs:22` (`until` poll loop); also `desktop/aizome/apps/hotaru/tests/cwd_latency.py:44` (`run`), `:57`, `:60`
- **Encoding:** loop index and harness predicates (`ready`, `displayed`) over the live `Hotaru` program; ABBA/BAAB `order` and `rotation` per block in the Python runner
- **States:** `Starting` → `PromptReady` → `Displayed` → `WarmUp(0..10)` → `Measured(10..70)` → `PaletteProbes(10)` → `Exiting` → `Done`; runner: `WarmUp` → `Block(n, ABBA|BAAB)` → `Summary`
- **Transitions:** first prompt `cwd_latency.rs:88`; shaped frame `:90`; cd/ACK round trips `:98-113`; palette open/facts/close `:118-129`; exit until panes empty `:136-137`; runner warm-up `cwd_latency.py:57-59`, blocks `:60-68`
- **Hazards:** 10 s hard deadline per wait (`cwd_latency.rs:23`, asserts); every invocation spawns a fresh app with 30 s timeout (`cwd_latency.py:47-50`)
- **Reify:** Low — sequential benchmark script

<a id="hot-02"></a>

### HOT-02 — Pane lifecycle: live, ended, failed, removed, window teardown
- **Where:** `desktop/aizome/apps/hotaru/src/app.rs:136` (`Pane.session`), `:143` (`.ended`), `:138` (`.status`); `Hotaru.tabs`/`.panes` `:182-183`
- **Encoding:** `session: Option<Session>`, `ended: bool`, `status: String`, pane membership in a tab's `PaneTree`; `settings.wait_after_command` chooses kept vs removed
- **States:** `Live` (session Some, !ended) → `EndedKept` (session Some, ended; PTY worker still holds history) | `Failed` (session None, ended) | `Removed`; also `SpawnFailed`/`Preview` (session None, !ended); last tab removed → `WindowClosing` (or deferred, HOT-12)
- **Transitions:** spawn `new_pane` `app.rs:391-399` → Live/SpawnFailed; `Exited` `:2069-2081` → remove (`:2073`) or EndedKept (`:2080`); `Failed` `:2083-2096` (`:2094-2095`); key on ended pane `:2121-2127`; `close_pane` `:509-513`, `close_tab` `:481-485`; `remove_pane` `:517-537`; `remove_tab` `:488-507` (window close `:492-502`); `close_requested` clears all `:2655-2659`
- **Hazards:** the ended-pane key path returns from `update` early (`app.rs:2127`), skipping the post-match reconciliation (`:2408-2420`: automatic-link cancel, `recent_tabs`, `sync_search`) until the next message; `SpawnFailed` panes never take the "press a key to close" path and search treats them as Preview (`:718-720`); handlers re-check `!ended` independently (`:861`, `:921`, `:1008`, `:2269`); 64-pane cap checked in two places (`:427`, `:466`)
- **Reify:** High — three fields, seven mutation sites, reachable `session None && !ended` combination
- **Related:** `session.rs` PTY worker (HOT-21) on the other side of `Msg::Exited`/`Msg::Failed`

<a id="hot-03"></a>

### HOT-03 — Tab, pane focus, zoom and recency model
- **Where:** `desktop/aizome/apps/hotaru/src/app.rs:176` (`Tab.active`), `:178` (`Tab.zoomed`), `:184` (`Hotaru.active`), `:242` (`recent_tabs`), `:144` (`Pane.bell`)
- **Encoding:** active tab id, per-tab active pane, per-tab zoom flag (meaningful only for `PaneTree::Split`), MRU list of tab ids, per-pane bell flag
- **States:** per tab `Unzoomed` ⇄ `Zoomed`; active tab/pane; MRU order; pane `Quiet` ⇄ `Rang`; invalid but representable: `zoomed` on a `Leaf` tree
- **Transitions:** `toggle_zoom` `app.rs:457-462`; `split_pane` unzooms `:473-477`; `remove_pane` `:527-531`; `focus_pane` unzooms on pane change and clears bell `:598-612`; `SelectTab` `:1806-1811`; `new_tab` `:434-435`; `remove_tab` picks neighbour `:503-505`; bell set `:2103-2107`; MRU reorder only at end of `update` `:2416-2419`
- **Hazards:** zoom validity relies on `&=`/guards at `:460`, `:531`; MRU update skipped by early-returning arms; `Bell` is set even for the focused, visible pane and is cleared only by `focus_pane`, so the tab-strip "•" persists until a click or refocus
- **Reify:** Medium — several handlers, guarded invalid combination

<a id="hot-04"></a>

### HOT-04 — Focus-restore timer generation
- **Where:** `desktop/aizome/apps/hotaru/src/app.rs:209` (`focus_timer`), `:210` (`focus_generation`)
- **Encoding:** pending zero-delay timer handle plus generation counter carried in `Msg::RestoreFocus(pane, generation)`
- **States:** `Idle` → `Pending(gen)` → `Idle`
- **Transitions:** `focus_after_rebuild` cancels/bumps/arms `app.rs:347-359`; `RestoreFocus` accepts matching generation `:1786-1793`
- **Hazards:** none observed (stale generations ignored; timer cancelled before rearm)
- **Reify:** Low — small, local, already token-guarded

<a id="hot-05"></a>

### HOT-05 — Pane directory provenance and title derivation
- **Where:** `desktop/aizome/apps/hotaru/src/app.rs:132` (`Pane.working_directory`), `:134` (`.link_directory`), `:130` (`.title`); also `desktop/aizome/apps/hotaru/src/session.rs:485` (`seen_title`)
- **Encoding:** a startup/launch guess and a validated OSC 7 report in two `Option<PathBuf>` fields; a PTY-worker latch that stops cwd-derived titles once a program sets one
- **States:** `Unknown` → `Guessed` (working_directory only) → `Reported` (both set) → `Cleared` (empty report: both None); title `CwdDerived` → `Explicit` (latched)
- **Transitions:** guess at spawn `desktop/aizome/apps/hotaru/src/app.rs:390`, `:403-404`; `StartupDirectory` `:2047-2051` (sent `desktop/aizome/apps/hotaru/src/session.rs:129-131`); `WorkingDirectory` `desktop/aizome/apps/hotaru/src/app.rs:2052-2056` (from `desktop/aizome/apps/hotaru/src/session.rs:423-435`, `:460-462`); `Title` `desktop/aizome/apps/hotaru/src/app.rs:2058-2068`; latch set `desktop/aizome/apps/hotaru/src/session.rs:419-422`, consulted `:430-432`
- **Hazards:** consumers must distinguish guess from report: inheritance asks the PTY owner only when `link_directory` is None (`app.rs:388`), the resolver uses only reports (`:1983`, re-checked `:2015`), Go to Tab ranks report > /proc > guess (`:1493-1494`); `seen_title` never resets (after RIS or a program exit, cwd titles stay off for the pane)
- **Reify:** Medium — precedence spread across five consumers and two files
- **Related:** `session.rs` `publish_events` (PTY worker side)

<a id="hot-06"></a>

### HOT-06 — Pane font-shaping epoch and scale acceptance
- **Where:** `desktop/aizome/apps/hotaru/src/app.rs:146` (`Pane.rendered`), `:147` (`.font_worker`), `:148` (`.font_epoch`), `:150` (`.font_scale`); also `desktop/aizome/apps/hotaru/src/font.rs:60` (`Pending.active` epoch)
- **Encoding:** epoch counter bumped on every visual invalidation; output scale; last accepted `Frame` (epoch, metrics.scale, snapshot)
- **States:** `Unshaped` (rendered None) → `Shaping(epoch)` → `Shown(epoch)`; `Invalidated` (epoch bumped); `Failed` (status message); invalid: `rendered.epoch != font_epoch` is normal lag, `font_worker None` is permanent `Unshaped`
- **Transitions:** `shape_pane` bump `app.rs:632-651` (`:635-637`); `shape_all` `:653-658`; grid size change bumps `:1833-1836`; `ShapeScale` `:1880-1888`; `Shaped` accept on epoch+scale `:1889-1911` (drops stale `hovered_link` `:1906-1908`); `FontFailed` `:1912-1918`; worker cancels active only for a different epoch `font.rs:141-152`
- **Hazards:** same-epoch frames for older snapshots are still accepted (displayed snapshot lags `pane.snapshot`; link/selection code must use `displayed_snapshot`); a failed worker spawn only logs (`app.rs:415-417`) leaving the pane unshaped with no status; shaping cancellation discards the whole row cache (`font.rs:279`)
- **Reify:** Medium — epoch invariants spread across six handlers
- **Related:** font worker thread (HOT-15), `terminal_view.rs` retained frame (HOT-33)

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

<a id="hot-08"></a>

### HOT-08 — User copy/paste request ordering and paste notice
- **Where:** `desktop/aizome/apps/hotaru/src/app.rs:195` (`clipboard`), `:196` (`clipboard_token`), `:197` (`clipboard_cancel`), `:198` (`clipboard_format`), `:199` (`clipboard_read`); `Pane.paste_notice` `:141`
- **Encoding:** one global token plus a swapped cancel flag and the pending `ClipboardRead`; per-pane `(token, message)` notice compared with `status`
- **States:** `Idle` → `Copying(t)` | `CopyAllCapturing(t)` → `CopyFormatting(t)` → `Published` | `CopyFailed`; `Pasting(t)` → `Pasted` | `PasteFailed(notice t)` → `NoticeCleared`; any → `Superseded` by the next request
- **Transitions:** `clipboard_request` `app.rs:790-799`; `copy` `:801-831`; `CopyAll` `:2222-2226` → `CopySnapshot` `:2227-2240`; `CopyReady` `:2241-2245`; `CopyFailed` `:2246-2250`; `Paste` `:2251-2265`; `PastePrimary` `:2149-2163`; `PasteReady` `:2266-2286` (notice `:2276`, 5 s timer `:2277`); `ClearPasteNotice` `:2287-2293`; cleared by input `:620-623`; close paths `:483`, `:511`, `:2656-2657`; `Drop` `:2664-2672`
- **Hazards:** the token is global, so closing any tab or pane (`:483`, `:511`) cancels an in-flight copy or paste of a different pane; a paste-event paste (`:2156-2158`, `:2258-2260`) bumps the token without recording a read; `CopyAll` for a pane without a session never answers and shows nothing; the notice restore depends on status text equality (`:154-160`)
- **Reify:** Medium — tokens centralised in `clipboard_request`, but four request kinds and a two-phase CopyAll across eight arms
- **Related:** clipboard worker (HOT-15), `session.rs:639-641` (CopyAll capture), aizome-kit `ClipboardRead`

<a id="hot-09"></a>

### HOT-09 — Copy-on-select primary-selection token
- **Where:** `desktop/aizome/apps/hotaru/src/app.rs:229` (`primary_token`), `:230` (`primary_cancel`)
- **Encoding:** token and swapped cancel flag for the latest finished pointer selection
- **States:** `Idle` → `Preparing(t)` → `Offered` | `Superseded` | `Dropped`
- **Transitions:** `copy_to_primary` `app.rs:835-855` (from `Selected`, `:2143`; emitted `terminal_view.rs:1177`, `:1219`); worker `selections.rs:376-381`; `PrimaryReady` `app.rs:2144-2148`
- **Hazards:** silently dropped when the selections queue is full (`app.rs:851-854`), where it can sit behind up to 16 reads that each wait up to 2 s
- **Reify:** Low — small token machine

<a id="hot-10"></a>

### HOT-10 — Program clipboard request flow (OSC 52 / OSC 5522) and the single question
- **Where:** `desktop/aizome/apps/hotaru/src/app.rs:232` (`asking`), `:233` (`ask_token`), `:228` (`selections`); also `desktop/aizome/apps/hotaru/src/selections.rs:317` (`Question`)
- **Encoding:** one global `Option<Question>` (token, request, captured reply) shown as an alert; settings `clipboard_read`/`clipboard_write`; session grants held in the PTY worker's terminal
- **States:** `Requested` → `Refused` | `Allowed` | `Capturing` (selections worker) → `Captured` → `Asking(token)` → `Answered(Allow|Remember|Refuse)` → `Replied`; concurrent requests while `Asking` → `Busy`
- **Transitions:** `program_clipboard` `app.rs:859-916`; `ask` `:932-994` (busy `:941-949`, set `:993`); `ClipboardCaptured` re-checks settings `:2165-2187`; `ClipboardAnswer` token match `:2188-2192`; `answer` `:999-1042` (grant `:1016`, `:1026`)
- **Hazards:** one question for all panes; it is not cleared when its pane closes, so every other pane's prompted requests get the "another waits"/EBUSY refusal until someone answers; answers for ended panes are dropped silently (`:1008-1010`); a refused OSC 52 read under `Deny` gets no reply at all (`:901-904`, `:2182`)
- **Reify:** High — privacy-sensitive, five handlers, settings re-read at two points
- **Related:** `session.rs:439-453` (requests from the PTY worker), `selections.rs` worker and tickets (HOT-20), hotaru-core `kitty_clipboard` grants, aizome-kit alerts

<a id="hot-11"></a>

### HOT-11 — Desktop notification gating
- **Where:** `desktop/aizome/apps/hotaru/src/app.rs:235` (`focused_pane`), `:237` (`notified`)
- **Encoding:** pane holding keyboard focus; `Option<Instant>` 1 s throttle
- **States:** `Ready` → `Throttled(<1 s)` → `Ready`; focus `None` ⇄ `Some(pane)`
- **Transitions:** `TerminalFocus` `app.rs:2135-2142`; `notify` `:1046-1064` (stamp `:1062`)
- **Hazards:** `focused_pane` can name a removed pane until the next focus event; notifications inside the 1 s window are dropped, not coalesced
- **Reify:** Low — two small fields

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

<a id="hot-13"></a>

### HOT-13 — Settings window and shortcut recording mode
- **Where:** `desktop/aizome/apps/hotaru/src/app.rs:217` (`settings_window`), `:218` (`settings_page`), `:219` (`settings_notice`), `:225` (`recording`); also `desktop/aizome/apps/hotaru/src/preferences.rs:626` (`Recorder`)
- **Encoding:** optional window key, page enum, optional recording action, optional status notice
- **States:** `Closed` → `Open(page)` → `Recording(action)` (stays recording after each `Recorded`) → `Open` → `Closed`; invalid: `recording` Some with no window (cleared on every close path)
- **Transitions:** `show_settings` `app.rs:1130-1145`; `close_settings` `:1147-1152`; window close `:2643-2647`; last tab `:494-497`; `SettingsPage` `:1660-1663`; `RecordShortcut` `:1671-1675`; `StopRecording` `:1676-1680` (from `preferences.rs:680-681`, `:692-694`); `Recorded` `desktop/aizome/apps/hotaru/src/app.rs:1681-1685` → `record` `:1161-1198`; `RestoredShortcuts` `:1703-1707`; key-window routing `:1272-1282`; menus drop shortcuts while recording `:1393`, `:1434`
- **Hazards:** `StopRecording` is keyed by action so the old recorder's focus loss can't stop a newly opened row; `settings_notice` persists across pages until open, record or a later successful save
- **Reify:** Medium — four fields, nine handlers, guarded

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

<a id="hot-15"></a>

### HOT-15 — Latest-request single-slot worker threads (six copies)
- **Where:** `desktop/aizome/apps/hotaru/src/automatic_links/mod.rs:46` (`Pending {job, active, stop, ready}`); also `desktop/aizome/apps/hotaru/src/link_resolver.rs:40` (same), `desktop/aizome/apps/hotaru/src/clipboard.rs:68` (`Pending {job, active: Option<Cancellation>, stop}`), `desktop/aizome/apps/hotaru/src/search.rs:99`, `desktop/aizome/apps/hotaru/src/font.rs:58` (`active: Option<(epoch, cancel)>`), `desktop/aizome/apps/hotaru/src/settings.rs:768` (`Queue {job, stop}`)
- **Encoding:** mutex+condvar slot holding one pending job, the active job's cancel flag, a stop flag and (links/resolver) a one-response mailbox drained by `take_response`
- **States:** `Waiting` → `Pending(job)` → `Active(cancel)` → `Ready(response)` | message sent | dropped when cancelled → `Waiting`; any → `Stopped`; `Ready` and `Active` may coexist
- **Transitions:** worker loops `automatic_links/mod.rs:82-113`, `link_resolver.rs:60-90`, `clipboard.rs:148-179`, `search.rs:113-159`, `font.rs:99-137`, `settings.rs:780-801`; submit replaces and cancels `automatic_links/mod.rs:120-135` (admission cancel `:121-124`), `link_resolver.rs:93-103`, `clipboard.rs:183-196`, `search.rs:163-175`, `font.rs:141-152` (cancels only another epoch `:144`), `settings.rs:804-808`; `cancel` `automatic_links/mod.rs:139-148`, `link_resolver.rs:107-116`; `Drop` sets stop, never joins `automatic_links/mod.rs:150-165`, `link_resolver.rs:118-131`, `clipboard.rs:199-213`, `search.rs:178-191`, `font.rs:155-167`, `settings.rs:817-822` (drains the pending job first `:787-789`)
- **Hazards:** when `deliver()`/`tx.send` fails the thread exits but the handle stays, so later submits are accepted and never run (only at shutdown); detached threads outlive the UI while a foreign call or helper child is stuck; the settings slot lets a Load and a Save replace each other with no reply to the replaced caller; one response can be announced twice (second `take_response` returns None)
- **Reify:** Medium — six hand-rolled copies of one pattern; a generic latest-wins worker would centralise it

<a id="hot-16"></a>

### HOT-16 — Go to Tab palette lifecycle
- **Where:** `desktop/aizome/apps/hotaru/src/go_to_tab.rs:311` (`Palette {query, selected, scroll, token, facts, rows, tabs}`); also `desktop/aizome/apps/hotaru/src/app.rs:239` (`go_to_tab`), `:240` (`go_to_tab_token`)
- **Encoding:** `Option<Palette>` open flag, gathering token, facts `Option` (gathering vs gathered), selection/scroll
- **States:** `Closed` → `Open(gathering)` → `Open(gathered)` → `Closed` (Go/Click/Close/toggle)
- **Transitions:** `toggle_go_to_tab` `app.rs:1475-1502` (close `:1476-1478`, open `:1480-1484`, gather thread `:1499`, focus `:1501`); `GoToTabFacts` token check `:1814-1820`; `go_to_tab_event` `:1504-1535`; `GoToTabFocus` `:1821-1825`; `Palette::refresh` `go_to_tab.rs:329-339`, `step` `:341-348`, `scroll_by` `:350-352`
- **Hazards:** rows, facts and the tab count are a snapshot at open; tabs or panes that close while it is open stay listed (Go then targets a missing pane and just closes); gather threads aren't cancellable, only ignored by token; `GoToTabFocus` is untokened
- **Reify:** Low — mostly an `Option` with a token

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

<a id="hot-18"></a>

### HOT-18 — Link-path resolver helper child supervision
- **Where:** `desktop/aizome/apps/hotaru/src/link_resolver.rs:159` (`supervise`: `failure`, `eof`, `status`, `deadline` locals)
- **Encoding:** loop locals plus the child process and a non-blocking socket pair
- **States:** `Spawned` → `Reading` → `Failed(Cancelled|Timeout|Output)` → `Killed` → `Reaped` → `Err`; `Reading` → `EOF` → `Reaped(success)` → `Ok(target)` | `Err(Exit|Output)`
- **Transitions:** spawn `link_resolver.rs:148-156`; cancel/timeout `:173-176`; output cap `:184-187`; kill `:200-202`; reap `:204-219`; result `:220-230`; 5 ms poll `:231`
- **Hazards:** after a kill the loop waits for `try_wait` with no deadline (documented): a child stuck in kernel I/O holds the worker's single slot, so later activations queue behind it
- **Reify:** Low — local to one call, documented bounds

<a id="hot-19"></a>

### HOT-19 — Find: query, capture, whole-history search and navigation
- **Where:** `desktop/aizome/apps/hotaru/src/search.rs:17` (`State {worker, pane, token, cancel, in_flight, dirty, timer, caches, navigation, pending_navigation, retry_navigation, copy, error}`); also `desktop/aizome/apps/hotaru/src/app.rs:187-189` (`find`, `search`, `find_state`)
- **Encoding:** thirteen fields plus the PTY worker's search mailbox and the search worker slot; tokens for capture and navigation
- **States:** `Closed` → `Idle` → `Debouncing(timer)` → `Capturing(in_flight)` → `Searching(in_flight)` → `Ready(caches)` → `Navigating(pending_navigation)` → `Selected(copy)` | `RetryNavigation` | `Error`; invalid but representable: `in_flight && timer`, `copy && pending_navigation`, `retry_navigation && !dirty`
- **Transitions:** `Find` `app.rs:2294-2297`; `Search` `:2298-2303`; `sync_search` `:660-671`; `schedule_search` `:673-684`; `SearchRefresh` `:2304-2309`; `request_search` `:686-722`; `SearchSnapshot` `:2310-2329`; `SearchReady` `:2330-2372`; `SearchFailed` `:2373-2378`; `navigate_search` `:724-773`; `SearchNavigated` `:2380-2394`; frame handling `:1843-1877`; `Select` `:2205-2214`; `Failed` `:2083-2091`; `CloseFind` `:2395-2400`; `State::invalidate` `search.rs:54-67`; PTY side `session.rs:190-201`, `:532-549`
- **Hazards:** `in_flight` is set at `app.rs:701` before the pane lookup at `:704` (an early return leaves it set until the next `sync_search`); the PTY worker drops a cancelled Capture without reply (`session.rs:547`), so every cancel must go with `invalidate`'s reset; correctness relies on the timer always carrying the current token (each `invalidate` is preceded by taking the timer); early `return`s in `update` skip `sync_search`; `SearchFailed` doesn't reschedule even when dirty
- **Reify:** High — thirteen fields, about fifteen handlers, several invalid combinations
- **Related:** `session.rs` controls mailbox (HOT-23), search worker (HOT-15), font highlights `font.rs:286-290`, hotaru-core `search::Results`

<a id="hot-20"></a>

### HOT-20 — Per-pane clipboard request admission tickets and the selections queue
- **Where:** `desktop/aizome/apps/hotaru/src/selections.rs:75` (`Bounds {count, bytes}`), `:81` (`Ticket`), `:170` (`Request._ticket`), `:343` (`Worker.jobs`)
- **Encoding:** atomic per-pane counters charged optimistically and refunded on `Drop`; `Arc<Ticket>` shared by every clone of a request; 16-slot sync channel
- **States:** per request `Admitted` → `Queued` → `Captured` → `Asking` → `Answered` → `Released`; per pane `Open` (<16 requests, ≤32 MiB) | `Full` (new requests refused or EBUSY)
- **Transitions:** `Ticket::take` `selections.rs:87-94`; refund `:97-101`; OSC 52 `request` `:176-188`; Kitty `kitty` `:223-263`; refused in PTY worker `session.rs:439-453`; queue `selections.rs:354-356`; serial capture `:359-387`
- **Hazards:** a slot is released only when the last clone (including a pending `Question`) drops; reads run serially with a 2 s timeout each, so a stalled owner causes head-of-line blocking for later reads and primary-selection jobs
- **Reify:** Low — RAII counters, small

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

<a id="hot-22"></a>

### HOT-22 — PTY worker frame cadence, acknowledgement handshake and idle compression
- **Where:** `desktop/aizome/apps/hotaru/src/session.rs:69` (`Shared.frame_pending`), `:500` (`changed`), `:501` (`last_frame`), `:505` (`last_activity`), `:506` (`compress_more`)
- **Encoding:** one-frame-in-flight flag cleared by the UI, dirty flag, 8 ms cadence, idle timestamp and more-history-to-compress flag driving the poll timeout
- **States:** `Clean` → `Dirty` → `FrameInFlight` → (UI ack) → `Clean`|`Dirty`; history `Active` → `IdleWait(100 ms)` → `Compressing` → `Compact`
- **Transitions:** dirty `session.rs:522`, `:526`, `:530`, `:543`, `:721`; send `:779-786`; ack `received_frame` `:237-240` (UI `app.rs:1830-1832`); activity `:520-521`, `:734-737`; compression step `:800-802`; timeout choice `:803-813`
- **Hazards:** exit reporting also waits for `!changed` (`:788`), so a missing UI ack would delay `Exited`; the ack is sent before shaping, so font-worker load is not back-pressured (it coalesces instead)
- **Reify:** Medium — four interacting fields steer the poll timeout

<a id="hot-23"></a>

### HOT-23 — PTY worker latest-state controls mailbox
- **Where:** `desktop/aizome/apps/hotaru/src/session.rs:78` (`Controls {resize, colors, cursor, search_capture, search_navigation}`)
- **Encoding:** mutex-held latest values that bypass the bounded command queue
- **States:** per control `Empty` → `Posted(latest)` → `Applied`
- **Transitions:** post `session.rs:167-201`; take-all `:513`; apply `:514-549`
- **Hazards:** a cancelled search control is dropped without a reply (`:547`); a coalesced Navigate leaves the replaced token unanswered (the UI's newer `pending_navigation` covers it)
- **Reify:** Low — explicit per-field mailbox, one consumer

<a id="hot-24"></a>

### HOT-24 — PTY input and reply back-pressure accounting
- **Where:** `desktop/aizome/apps/hotaru/src/session.rs:70` (`Shared.input_bytes`), `:492` (`output`), `:493` (`output_at`, `reply_bytes`), `:495-496` (`chunk`, `chunk_at`), `:498` (`readable`)
- **Encoding:** byte reservation per command kind released as written or discarded; reply budget gating PTY reads; ordered write queue with offset; current read chunk with offset
- **States:** `Flowing` → `ReplyThrottled` (reply_bytes ≥ 64 KiB: reader stalls) → `Flowing`; `InputFull` (≥32 MiB or 64 queued commands: input refused); `HungUp` (queue drained, budgets reset); `Eof`
- **Transitions:** reserve `session.rs:204-226`, refund on send failure `:227-233`; per-kind release `:561-562`, `:578`, `:587`, `:609`, `:618`, `:620-628`, `:645`, `:655-659`, `:670` (`encoded_input` `:383-396`); feed gate `:692`, chunk swap `:697-716`; replies queued `:724-727`; writes `:739-764`; hang-up drain `:765-778`; `readable` re-armed `:831-842`
- **Hazards:** reservations in `command()` and releases in the run loop must match exactly per kind, or the 32 MiB budget leaks until input is refused permanently; encoded input larger than its reservation fails the whole session (`:390`); input queued at hang-up is dropped silently
- **Reify:** Medium — accounting spread over a dozen sites but guarded

<a id="hot-25"></a>

### HOT-25 — PTY reader (gather) thread
- **Where:** `desktop/aizome/apps/hotaru/src/session.rs:265` (`Gather {output, spare, stop}`); `gather` `:311`
- **Encoding:** bounded chunk channel (4), spare-buffer channel, stop eventfd, local `ended`/`full`
- **States:** `Reading` → `Sending` (blocks while full) → `Polling` → `Reading`; → `Ended(Ok|Err)`; → `Stopped`
- **Transitions:** spawn `session.rs:272-289`; read loop `:323-341`; send `:343-349`; end `:350-354`; poll and stop `:358-374`; `Drop` signals stop `:299-306`
- **Hazards:** none observed (a blocked send fails once the session drops its receiver)
- **Reify:** Low — self-contained loop

<a id="hot-26"></a>

### HOT-26 — Tab strip scroll-follow mode
- **Where:** `desktop/aizome/apps/hotaru/src/tab_strip.rs:131` (`Scrolled {offset, content, width, revealed, following}`)
- **Encoding:** widget state persisted between views; `revealed` remembers (shown, count)
- **States:** `Following` (keeps the shown tab in sight) → `Manual` (scrolled by hand) → `Following` when the shown tab, tab count or width changes
- **Transitions:** re-arm `tab_strip.rs:217-220`; follow `:221-233`; manual scroll `:144-155`; count click `:302-309`
- **Hazards:** none observed
- **Reify:** Low — small and local

<a id="hot-27"></a>

### HOT-27 — Pointer drag selection and held-button tracking
- **Where:** `desktop/aizome/apps/hotaru/src/terminal_view.rs:245` (`ViewState.drag`), `:253` (`.buttons`), `:247` (`.scroll_fraction`)
- **Encoding:** drag anchor, list of held mouse buttons (sets `any_button` in mouse reports), fractional wheel accumulator; kit pointer capture
- **States:** `Idle` → `Pressed` → `Dragging(anchor)` → `Released` (→ `Selected` emitted); multi-click `Word`/`Line` selects at once; buttons `None` ⇄ `Held(list)`
- **Transitions:** press `terminal_view.rs:1150-1171` (buttons `:1153-1157`, anchor `:1166-1168`, capture `:1170`); extend `:1182-1196`; release `:1198-1222` (buttons `:1212`, `drag.take` `:1213`, `Selected` `:1218-1220`, `release_pointer` `:1221`); link press clears drag `:1143`; wheel accumulator `:1233-1236`
- **Hazards:** neither `PointerLeave` (`:1223-1228`) nor focus loss (`:1246-1265`) clears `drag` or `buttons`; the kit ends a press on leave only for `drags_only` widgets (`desktop/aizome/crates/aizome-kit/src/engine.rs:2065-2072`), which this view is not; a right/middle press "ours" never records a button but its release still sends a mouse release (`:1214-1217`)
- **Suspected bug:** begin a drag-select, then let the compositor take the pointer (one of its own grabs; the kit comment at `engine.rs:2065-2068` says the release then goes elsewhere): `drag` and `buttons` stay set, so later plain hover over the pane keeps extending the selection and reports button-held motion to mouse-tracking programs, and copy-on-select never fires, until the next click (confidence: medium)
- **Reify:** High — suspected bug; state spans view, kit capture and app selection

<a id="hot-28"></a>

### HOT-28 — Modifier-click link press gesture
- **Where:** `desktop/aizome/apps/hotaru/src/terminal_view.rs:258` (`ViewState.link_press`); also `desktop/aizome/apps/hotaru/src/links.rs:120` (`Press {link, position, cancelled}`)
- **Encoding:** `Option<Press>` with a sticky `cancelled` flag; pointer capture
- **States:** `None` → `Pressed` → `Cancelled` (motion, ❖ released, new frame, leave, scroll, focus loss) → release: `Open(link)` | `Discarded` → `None`
- **Transitions:** start `terminal_view.rs:1138-1149`; motion `:1185-1187` (`links.rs:126-128`); modifier `terminal_view.rs:476-478`; new snapshot `:632-634`; leave `:1224-1226`; scroll `:1230-1232`; focus loss `:1255-1257`; release `:1200-1209` (`links.rs:129-132`)
- **Hazards:** same lost-release root cause as HOT-27: a stale `link_press` swallows all motion (`terminal_view.rs:1185-1187`), so the next plain drag doesn't extend and its release consumes the stale press (`terminal_view.rs:1200-1209`) without clearing `drag`
- **Reify:** Medium — explicit struct, cancel sites scattered over seven handlers

<a id="hot-29"></a>

### HOT-29 — Hover and automatic-link request lifecycle in the terminal view
- **Where:** `desktop/aizome/apps/hotaru/src/terminal_view.rs:254` (`hovered`), `:255` (`auto_request`), `:256` (`auto_revision`), `:257` (`auto_refresh_pending`), `:259` (`pointer`), `:260` (`link_modifier`), `:261` (`hover_clear_pending`)
- **Encoding:** last hit link, the outstanding request identity, the app-provided result's request identity, deferred refresh and clear flags, last pointer position and modifier
- **States:** `NoHover` → `Requested(request)` → `Hit(link)` | `Miss` → `Stale` (new displayed frame: clear pending, refresh pending) → `Requested`; `ModifierHeld` toggles cursor and underline
- **Transitions:** `hover` `terminal_view.rs:432-489` (request `:438-456`, emit `MatchLink` `:483-485`, `HoverLink` `:486-488`); revision change `:617-624`; new snapshot `:625-635`; deferred tick `:680-688` → `:1046-1063`; leave/focus loss `hover(None)` `:1227`, `:1258`; key presses refresh `:1093-1094`
- **Hazards:** while the pointer rests over text during continuous output, every displayed frame re-requests a match (`:630`), bounded only by the latest-wins worker; `HoverLink(None)` is deferred to the next tick, so the app's `hovered_link` can briefly reference an older frame (filtered at `app.rs:1906-1908`)
- **Reify:** High — seven fields, protocol with the app's identity checks
- **Related:** `app.rs` pipeline (HOT-07)

<a id="hot-30"></a>

### HOT-30 — Layout-to-tick deferred notifications
- **Where:** `desktop/aizome/apps/hotaru/src/terminal_view.rs:248` (`resize_pending`), `:242` (`scale_pending`), `:250` (`theme_pending`), `:251` (`window_rect`), `:243-244` (`font_epoch`, `font_metrics`)
- **Encoding:** dirty flags set during layout, drained on the next tick which emits app messages
- **States:** per notification `Clean` → `Pending` → `Emitted` (`ResizePixels`, `ShapeScale`, `Theme`, `Geometry`)
- **Transitions:** set `terminal_view.rs:608`, `:610`, `:652`, `:653`; tick requested `:680-688`; drained `:1052-1084` (geometry `:1064-1068`)
- **Hazards:** geometry is compared only on ticks requested for other reasons, so a pane that moves without resizing could keep a stale `Pane.rect` for directional focus (rare with binary splits)
- **Reify:** Low — flags with one drain site

<a id="hot-31"></a>

### HOT-31 — Cursor blink phase
- **Where:** `desktop/aizome/apps/hotaru/src/terminal_view.rs:29` (`Blink {hidden, armed}`), `:236` (`last_output_reset`), `:235` (`input_activity`), `:246` (`preedit`); also `desktop/aizome/apps/hotaru/src/app.rs:149` (`Pane.input_activity`)
- **Encoding:** partially explicit: `Blink::sync` centralises arm/cancel, but eligibility derives from focus, frame, cursor modes and preedit; output resets throttled to 500 ms
- **States:** `Ineligible` → `ArmedVisible` ⇄ `ArmedHidden` (600 ms timer); reset to `ArmedVisible` on input, output or cursor move
- **Transitions:** `Blink::sync` `terminal_view.rs:35-46`; layout `:639-648`, timer arm `:689-695`; timer `:1027-1043`; `reset_blink` `:539-560` (keys `:1090-1092`, IME `:1114-1122`, focus `:1246-1247`); app input counter `app.rs:622`
- **Hazards:** none observed (tests check timer counts)
- **Reify:** Low — already centralised in `sync`

<a id="hot-32"></a>

### HOT-32 — Held-key tracking for synthetic releases
- **Where:** `desktop/aizome/apps/hotaru/src/terminal_view.rs:252` (`ViewState.keys`)
- **Encoding:** list of (evdev code, normalized event) pressed in this pane
- **States:** per key `Up` → `Down` → `Up` (real release, or synthetic on focus loss)
- **Transitions:** press `terminal_view.rs:1103-1106`; release `:1107-1108`, unmatched release swallowed `:1109-1111`; focus loss releases all `:1259-1262`
- **Hazards:** none observed (binding-consumed presses are never recorded, so their releases are swallowed by design)
- **Reify:** Low — small and local

<a id="hot-33"></a>

### HOT-33 — Retained previous frame for scroll reuse and overhang repair
- **Where:** `desktop/aizome/apps/hotaru/src/terminal_view.rs:231` (`ViewState.snapshot`), `:232` (`.rendered`), `:262` (`.sprite_overhang`)
- **Encoding:** previous snapshot and admitted frame (≤8 MiB) kept to prove row identity; overhang flag kept separately because retention can reject a frame
- **States:** `NoPrevious` → `Retained(frame)` | `Rejected(oversize)`; repair `Pending` → `Done`
- **Transitions:** replace `terminal_view.rs:601`, `:625`; overhang `:603-604`, `:696-698`; reuse proof `:660-678` (`scroll_reuse` `:279-360`)
- **Hazards:** none observed (all reuse requires identity proofs)
- **Reify:** Low — validity cache with one update site

<a id="hot-34"></a>

### HOT-34 — (test harness) Native harness waits and scripted PTY child programs
- **Where:** `desktop/aizome/apps/hotaru/tests/native.rs:19` (`settle_frame`), `:171` (`settle_grid`), `:187` (`advancing`), `:199` (`settle_search`), `:1626` (`pump`)
- **Encoding:** deadline loops with steady counters; child test binaries driven by stdin bytes and env vars
- **States:** waits `Polling` → `Settled` | `TimedOut`; children e.g. `search_history_child` `Initial` → `append` | `prune` | `alternate` | `primary` → exit; `kitty_clipboard_child` `paste` mode vs request list, each `go` → `request` → `answer` → `show`
- **Transitions:** steady grid `native.rs:176-182`; search status `:201-210`; program-in-front retry `:1054-1068`; children `:137-168`, `:536-566`, `:1519-1543`, `:1923-1948`, `:2147-2207`
- **Hazards:** fixed 5 s deadlines on loaded machines (comments at `:462-464`)
- **Reify:** Low — test fixtures

<a id="hot-35"></a>

### HOT-35 — (test harness) Virtual pointer and keyboard probes with a stdin command protocol
- **Where:** `desktop/aizome/apps/hotaru/tests/wayland/auto-link-pointer.c:43` (`super`), `:51` (sequence loop); also `desktop/aizome/apps/hotaru/tests/wayland/link-pointer.c:43`
- **Encoding:** mode argument (`plain|super|hover|hold|drag|stationary|sequence`), ❖ held from start to cleanup, `ready`/`ack` lines on stdout
- **States:** `Connected` → `SuperHeld` → `Positioned` → `Ready` → per command `Pressed`/`Released`/`Moved`/`KeyF` → `Ack` → `q` → `Cleanup`
- **Transitions:** ❖ press `auto-link-pointer.c:44-47`; commands `:54-74`; mode flow `:77-94`; cleanup `:95-100`; `link-pointer.c:51-73`
- **Hazards:** a driver killed mid-sequence leaves ❖ and the left button logically held on the private seat until the virtual devices are destroyed
- **Reify:** Low — test probe

<a id="hot-36"></a>

### HOT-36 — (test harness) Stage-file fixtures acknowledged by window title
- **Where:** `desktop/aizome/apps/hotaru/tests/wayland/auto_links.py:33` (fixture), `:60` (`stage`); also `desktop/aizome/apps/hotaru/tests/wayland/cursor.py:25`, `:48`; `desktop/aizome/apps/hotaru/tests/wayland/links.py:58`, `:92`; `desktop/aizome/apps/hotaru/tests/wayland/pixel_scroll.py:14`, `:82`
- **Encoding:** driver writes `stage` (atomic rename in auto_links), the PTY fixture polls it, redraws and sets the title; the driver waits for the inspected title; then stability predicates
- **States:** `Stage(n)` → `TitleAcked(n)` → `Settled` (geometry stable 0.2 s, or two identical screenshots) → `Checked` → `Stage(n+1)`
- **Transitions:** fixture loops `auto_links.py:35-43`, `cursor.py:27-33`, `links.py:60-66`, `pixel_scroll.py:24-52`; acks `auto_links.py:62`, `cursor.py:50`, `links.py:94`, `pixel_scroll.py:84`; settle `auto_links.py:69-74`, `pixel_scroll.py:87-96`; probe protocol `auto_links.py:84-100`
- **Hazards:** title acks can precede shaped text (comment `pixel_scroll.py:85-86`), hence the extra screenshot stability loop
- **Reify:** Low — test fixtures

<a id="hot-37"></a>

### HOT-37 — (test harness) Per-pane FIFO fixture for program clipboard requests
- **Where:** `desktop/aizome/apps/hotaru/tests/wayland/selections.py:86` (fixture: `commands` thread, `pending`/`replies` parser)
- **Encoding:** per-pid directory with a FIFO of verbs, reply-N/kitty/typed files, `flooded` marker; exit on Control-D
- **States:** pane fixture `Ready` → `Writing`/`Reading`/`Flooding` → `ReplyRecorded(n)` → `Exited(^D)`; driver phases writes → reads → flood → Kitty → paste events → stalled owner → write flood → primary → Control-D → quit while pending
- **Transitions:** verbs `selections.py:96-110`; reply parse `:118-138`; phases `:227-437`
- **Hazards:** none observed
- **Reify:** Low — test fixture

<a id="hot-38"></a>

### HOT-38 — (test harness) Private headless-Sway driver lifecycle and polling waits
- **Where:** `desktop/aizome/apps/hotaru/tests/wayland/smoke.py:20` (`wait`); also every driver's `run`: `desktop/aizome/apps/hotaru/tests/wayland/smoke.py:83`, `desktop/aizome/apps/hotaru/tests/wayland/fonts.py:81`, `desktop/aizome/apps/hotaru/tests/wayland/sprites.py:139`, `desktop/aizome/apps/hotaru/tests/wayland/settings.py:139`, `desktop/aizome/apps/hotaru/tests/wayland/shell.py:89`, and the drivers listed under HOT-36, HOT-37, HOT-39
- **Encoding:** temp runtime dir, process list, log files, result dict written in `finally`
- **States:** `RuntimeCreated` → `SwayStarted` → `SocketsReady` → `AppStarted` → `Inspected` → scripted checks → `Teardown` (terminate, 3 s, kill) → `ResultWritten` → `RuntimeRemoved`
- **Transitions:** sway spawn `smoke.py:106-111`; app spawn `:185-188`; teardown `:309-318`; settle loops `fonts.py:81-87`, `sprites.py:139-149`
- **Hazards:** 15 s default wait timeout (`smoke.py:20`); evidence written even on failure
- **Reify:** Low — test drivers

<a id="hot-39"></a>

### HOT-39 — (test harness) Marker-file PTY fixture handshake
- **Where:** `desktop/aizome/apps/hotaru/tests/wayland/smoke.py:152` (fixture); also `desktop/aizome/apps/hotaru/tests/wayland/search.py:43`
- **Encoding:** files `produce`, `ready`, `received-paste.txt`, `request-input`, `input-ready`, `received-input.bin` in the evidence dir
- **States:** `WaitProduce` → `Output` → `Ready` → `RawPasteRead` → `PasteReceived` → `WaitRequestInput` → `InputReady` → `InputReceived`
- **Transitions:** fixture `smoke.py:154-181`; marker reset `:183-184`; driver `:200-201`, `:270`, `:288-289`, `:298`; `search.py:45-47`, `:78-79`
- **Hazards:** none observed
- **Reify:** Low — test fixture

<a id="hot-40"></a>

### HOT-40 — (test harness) Stalled data-control clipboard owner
- **Where:** `desktop/aizome/apps/hotaru/tests/wayland/stall.c:14` (`held`, `holding`)
- **Encoding:** offered source, array of up to 256 unwritten pipe fds
- **States:** `Offering` → `Holding(n fds)` → `Exited` (source cancelled or device finished)
- **Transitions:** offer `stall.c:62-68`; hold `:29-32`; exit `:33-36`, `:45-48`; dispatch `:69`
- **Hazards:** none observed (killed by the driver)
- **Reify:** Low — test fixture

## Already explicit

- `desktop/aizome/apps/hotaru/src/preferences.rs:25` `enum Page` — Settings sidebar page; set only in `Msg::SettingsPage` (`desktop/aizome/apps/hotaru/src/app.rs:1660`)

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/apps/hotaru/examples/auto_link_probe.rs` | 45 | none |  | stateless stdin probe of mapping and matcher |
| `desktop/aizome/apps/hotaru/examples/cwd_latency.rs` | 141 | machines | [HOT-01](#hot-01) | harness latency driver |
| `desktop/aizome/apps/hotaru/src/app.rs` | 2743 | machines | [HOT-02](#hot-02), [HOT-03](#hot-03), [HOT-04](#hot-04), [HOT-05](#hot-05), [HOT-06](#hot-06), [HOT-07](#hot-07), [HOT-08](#hot-08), [HOT-09](#hot-09), [HOT-10](#hot-10), [HOT-11](#hot-11), [HOT-12](#hot-12), [HOT-13](#hot-13), [HOT-14](#hot-14), [HOT-16](#hot-16), [HOT-17](#hot-17), [HOT-19](#hot-19), [HOT-22](#hot-22), [HOT-31](#hot-31) | app model and message handlers |
| `desktop/aizome/apps/hotaru/src/automatic_links/mapping.rs` | 266 | none |  | per-call bounded extraction with budget |
| `desktop/aizome/apps/hotaru/src/automatic_links/mod.rs` | 243 | machines | [HOT-15](#hot-15), [HOT-07](#hot-07) | latest-request matcher worker |
| `desktop/aizome/apps/hotaru/src/clipboard.rs` | 227 | machines | [HOT-15](#hot-15), [HOT-08](#hot-08) | copy and paste worker slot |
| `desktop/aizome/apps/hotaru/src/font.rs` | 966 | machines | [HOT-15](#hot-15), [HOT-06](#hot-06) | font worker slot; row cache is a bounded cache |
| `desktop/aizome/apps/hotaru/src/font_benchmark.rs` | 149 | none |  | ignored benchmark; thread-local probe counter |
| `desktop/aizome/apps/hotaru/src/go_to_tab.rs` | 791 | machines | [HOT-16](#hot-16) | palette state; ranking is pure |
| `desktop/aizome/apps/hotaru/src/input.rs` | 351 | none |  | pure input normalization and encoding |
| `desktop/aizome/apps/hotaru/src/instance.rs` | 156 | machines | [HOT-17](#hot-17) | lock and socket handoff |
| `desktop/aizome/apps/hotaru/src/lib.rs` | 27 | none |  | module declarations |
| `desktop/aizome/apps/hotaru/src/link_resolver.rs` | 350 | machines | [HOT-18](#hot-18), [HOT-15](#hot-15), [HOT-07](#hot-07) | helper child supervision and worker slot |
| `desktop/aizome/apps/hotaru/src/links.rs` | 204 | machines | [HOT-28](#hot-28), [HOT-07](#hot-07) | Press gesture struct and link identity |
| `desktop/aizome/apps/hotaru/src/main.rs` | 89 | machines | [HOT-17](#hot-17), [HOT-12](#hot-12) | initial instance claim and managed flag |
| `desktop/aizome/apps/hotaru/src/panes.rs` | 97 | none |  | pure split-tree operations |
| `desktop/aizome/apps/hotaru/src/preferences.rs` | 721 | machines | [HOT-13](#hot-13), [HOT-14](#hot-14) | Recorder widget; Page enum explicit |
| `desktop/aizome/apps/hotaru/src/search.rs` | 292 | machines | [HOT-19](#hot-19), [HOT-15](#hot-15) | find state and search worker slot |
| `desktop/aizome/apps/hotaru/src/selections.rs` | 577 | machines | [HOT-20](#hot-20), [HOT-10](#hot-10), [HOT-09](#hot-09) | tickets, question type, selections worker |
| `desktop/aizome/apps/hotaru/src/session.rs` | 1038 | machines | [HOT-21](#hot-21), [HOT-22](#hot-22), [HOT-23](#hot-23), [HOT-24](#hot-24), [HOT-25](#hot-25), [HOT-05](#hot-05), [HOT-19](#hot-19) | PTY worker and reader threads |
| `desktop/aizome/apps/hotaru/src/settings.rs` | 995 | machines | [HOT-12](#hot-12), [HOT-15](#hot-15) | settings worker slot and atomic save |
| `desktop/aizome/apps/hotaru/src/sprites.rs` | 132 | none |  | bounded LRU glyph cache |
| `desktop/aizome/apps/hotaru/src/tab_strip.rs` | 349 | machines | [HOT-26](#hot-26) | scroll-follow widget state |
| `desktop/aizome/apps/hotaru/src/terminal_view.rs` | 1908 | machines | [HOT-27](#hot-27), [HOT-28](#hot-28), [HOT-29](#hot-29), [HOT-30](#hot-30), [HOT-31](#hot-31), [HOT-32](#hot-32), [HOT-33](#hot-33), [HOT-14](#hot-14) | terminal widget interaction state |
| `desktop/aizome/apps/hotaru/tests/cursor.rs` | 135 | none |  | tests |
| `desktop/aizome/apps/hotaru/tests/cwd_latency.py` | 117 | machines | [HOT-01](#hot-01) | benchmark block runner |
| `desktop/aizome/apps/hotaru/tests/native.rs` | 2259 | machines | [HOT-34](#hot-34) | harness waits and scripted PTY children |
| `desktop/aizome/apps/hotaru/tests/pixel_scroll.rs` | 367 | none |  | tests |
| `desktop/aizome/apps/hotaru/tests/wayland/auto-link-pointer.c` | 106 | machines | [HOT-35](#hot-35) | virtual input probe with command protocol |
| `desktop/aizome/apps/hotaru/tests/wayland/auto_links.py` | 130 | machines | [HOT-36](#hot-36), [HOT-35](#hot-35), [HOT-38](#hot-38) | stage-file driver |
| `desktop/aizome/apps/hotaru/tests/wayland/cursor.py` | 108 | machines | [HOT-36](#hot-36), [HOT-38](#hot-38) | stage-file driver |
| `desktop/aizome/apps/hotaru/tests/wayland/fonts.py` | 113 | machines | [HOT-38](#hot-38) | private Sway driver with settle loop |
| `desktop/aizome/apps/hotaru/tests/wayland/key.c` | 77 | none |  | linear virtual-key injector |
| `desktop/aizome/apps/hotaru/tests/wayland/link-pointer.c` | 79 | machines | [HOT-35](#hot-35) | virtual input probe modes |
| `desktop/aizome/apps/hotaru/tests/wayland/links.py` | 172 | machines | [HOT-36](#hot-36), [HOT-35](#hot-35), [HOT-38](#hot-38) | stage-file driver |
| `desktop/aizome/apps/hotaru/tests/wayland/pixel_scroll.py` | 117 | machines | [HOT-36](#hot-36), [HOT-38](#hot-38) | stage-file driver with stable screenshots |
| `desktop/aizome/apps/hotaru/tests/wayland/pointer.c` | 66 | none |  | linear virtual-pointer injector |
| `desktop/aizome/apps/hotaru/tests/wayland/search.py` | 117 | machines | [HOT-39](#hot-39), [HOT-38](#hot-38) | marker-file driver |
| `desktop/aizome/apps/hotaru/tests/wayland/selections.py` | 460 | machines | [HOT-37](#hot-37), [HOT-38](#hot-38), [HOT-40](#hot-40) | per-pane FIFO fixture driver |
| `desktop/aizome/apps/hotaru/tests/wayland/settings.py` | 248 | machines | [HOT-38](#hot-38) | private Sway driver |
| `desktop/aizome/apps/hotaru/tests/wayland/shell.py` | 140 | machines | [HOT-38](#hot-38) | private Sway driver |
| `desktop/aizome/apps/hotaru/tests/wayland/smoke.py` | 323 | machines | [HOT-38](#hot-38), [HOT-39](#hot-39) | shared wait and marker-file driver |
| `desktop/aizome/apps/hotaru/tests/wayland/sprites.py` | 169 | machines | [HOT-38](#hot-38) | private Sway driver with compare loop |
| `desktop/aizome/apps/hotaru/tests/wayland/stall.c` | 71 | machines | [HOT-40](#hot-40) | stalled clipboard owner fixture |
| `desktop/aizome/crates/hotaru-link-regex/build.rs` | 44 | none |  | build script |
| `desktop/aizome/crates/hotaru-link-regex/examples/bounds.rs` | 37 | none |  | one-shot probe |
| `desktop/aizome/crates/hotaru-link-regex/examples/probe.rs` | 29 | none |  | line-by-line probe |
| `desktop/aizome/crates/hotaru-link-regex/src/abi.c` | 11 | none |  | static layout asserts |
| `desktop/aizome/crates/hotaru-link-regex/src/engine.rs` | 205 | none |  | RAII FFI handles; OnceLock global init |
| `desktop/aizome/crates/hotaru-link-regex/src/lib.rs` | 270 | none |  | per-call bounded search loop |
| `desktop/aizome/crates/hotaru-link-regex/tests/matching.rs` | 390 | none |  | tests |
