# KOY: Koyomi calendar

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/apps/koyomi`. Audited at `8e0acbe5`; line numbers refer to that commit.

31 files, 0 pending. 19 implicit machines: 10 High, 5 Medium, 4 Low.

## Machines

<a id="koy-01"></a>

### KOY-01 — Koyomi agent service lifecycle (unit, bus connect retry, listener threads, wake loop)
- **Where:** `desktop/aizome/apps/koyomi/data/aizome-koyomi-agent.service:18` (`ExecCondition`, `PartOf=` :11, `Restart=on-failure` :20, `RestartSec=5` :21); `desktop/aizome/apps/koyomi/src/agent.rs:345` (`run`)
- **Encoding:** systemd unit state (ExecCondition on `graphical-session.target`, PartOf, Restart=on-failure); in `run()`: the `loop { Dbus::connect() }` retry (`desktop/aizome/apps/koyomi/src/agent.rs:352`), two `koyomi-{signal}` listener threads holding `Sender<Reply>` clones (`desktop/aizome/apps/koyomi/src/agent.rs:283`), whether `rx` is still connected (`RecvTimeoutError::Disconnected`, `desktop/aizome/apps/koyomi/src/agent.rs:387`), the wake deadline from `next_wake` (`desktop/aizome/apps/koyomi/src/agent.rs:372`); settings, clock and zone captured once (`desktop/aizome/apps/koyomi/src/agent.rs:346-348`)
- **States:** `Conditioned` (skipped if the session isn't active) → `ConnectingBus` (retry every 5 s) → `Listening` | `Deaf` (listen failed, logged only) → `Looping` (refresh → due → post → save → wait) ⇄ `HandlingReplies`; `ListenersGone` (rx disconnected: sleeps each turn, no reconnect); `Stopped` (PartOf session) | `Crashed` → restarted after 5 s
- **Transitions:** systemd start after graphical-session.target (`desktop/aizome/apps/koyomi/data/aizome-koyomi-agent.service:12`, `:18-19`); connect retry (`desktop/aizome/apps/koyomi/src/agent.rs:353-359`); listen spawns threads (`desktop/aizome/apps/koyomi/src/agent.rs:278-303`); a thread ends when its signal stream ends or send fails (`desktop/aizome/apps/koyomi/src/agent.rs:286-299`); wait until wake or reply (`desktop/aizome/apps/koyomi/src/agent.rs:372-374`); replies drained (`desktop/aizome/apps/koyomi/src/agent.rs:375-385`); Disconnected → sleep (`desktop/aizome/apps/koyomi/src/agent.rs:387`); Open spawns `aizome-koyomi --show` (`desktop/aizome/apps/koyomi/src/agent.rs:337-341`); crash → `Restart=on-failure` (`desktop/aizome/apps/koyomi/data/aizome-koyomi-agent.service:20-21`)
- **Hazards:** no reconnect: if the bus connection drops, the listener threads end, every `Notify` fails (`desktop/aizome/apps/koyomi/src/agent.rs:328-331`) and the process keeps running, so `Restart=on-failure` never fires; settings (snooze, clock) and the time zone are read once (`desktop/aizome/apps/koyomi/src/agent.rs:346-350`), so changes and travel need an agent restart; `recv_timeout` waits on the monotonic clock (≤60 s, `desktop/aizome/apps/koyomi/src/agent.rs:373`), which stops in suspend, so alerts can be up to a minute late after resume; `run` never returns, so its `i32` is unreachable
- **Reify:** High — lifecycle spread over the unit, a retry loop, two threads and the wake loop, with a leak below
- **Suspected bug:** every Open click spawns a child (`desktop/aizome/apps/koyomi/src/agent.rs:339`) whose `Child` is dropped without `wait()`. SIGCHLD isn't ignored, so each child stays a zombie under the long-running agent until the agent exits (confidence: high)
- **Related:** aizome-shell `desktop/aizome/crates/aizome-shell/src/notifications/server.rs` (serves org.freedesktop.Notifications, emits ActionInvoked/NotificationClosed); `desktop/aizome/nix/module.nix:316` (unit wantedBy)

<a id="koy-02"></a>

### KOY-02 — Alert delivery state: due → fired → shown → snoozed/closed
- **Where:** `desktop/aizome/apps/koyomi/src/agent.rs:52` (`AgentState.last_check` :54, `.fired` :56, `.snoozed` :58); `desktop/aizome/apps/koyomi/src/agent.rs:94` (`Agent.shown`, `.dirty` :95)
- **Encoding:** persisted `$XDG_STATE_HOME/aizome/koyomi/agent.json` (`last_check`, `fired: key→due second`, `snoozed: (at, Note)`); in memory `shown: notification id→Note` (not persisted) and `dirty`
- **States:** per alert key `Scheduled` → `Due` (in `[max(last_check, now−6 h), now+1 s)`) → `Fired` (key in `fired`) → `Shown(id)` → `Snoozed(at)` → `Due` → `Shown(id')` → `Done` (Open/Closed removes it from `shown`) → `Forgotten` (pruned after 3 days). Representable but invalid: `Fired` with no notification (Notify failed); `Shown(id)` for a notification the server already closed; `shown` empty while notifications from before a restart are still on screen
- **Transitions:** `due` skips alerts for events already over (`desktop/aizome/apps/koyomi/src/agent.rs:175-177`), marks fired (`desktop/aizome/apps/koyomi/src/agent.rs:182`), releases due snoozes (`desktop/aizome/apps/koyomi/src/agent.rs:185-188`), sets `last_check` (`desktop/aizome/apps/koyomi/src/agent.rs:189`), prunes (`desktop/aizome/apps/koyomi/src/agent.rs:191-192`), sets dirty (`desktop/aizome/apps/koyomi/src/agent.rs:193`); `post` records `shown` only when Notify succeeds (`desktop/aizome/apps/koyomi/src/agent.rs:226-229`); `handle` Snooze moves shown→snoozed (`desktop/aizome/apps/koyomi/src/agent.rs:236-241`), Open/Closed remove (`desktop/aizome/apps/koyomi/src/agent.rs:244-247`); `save_if_changed` (`desktop/aizome/apps/koyomi/src/agent.rs:252-255`); load at start (`desktop/aizome/apps/koyomi/src/agent.rs:66-68`, `:350`)
- **Hazards:** an alert is marked fired before it's delivered; `dirty` is set by every `due` call, so agent.json is rewritten at every wake (≤30 s); the `replaces` lookup (`desktop/aizome/apps/koyomi/src/agent.rs:225`) can never match a returning snooze because Snooze already removed the entry (`desktop/aizome/apps/koyomi/src/agent.rs:237`), so the comment's "comes back over its old notification" path is dead; `shown` is never reconciled with the server
- **Reify:** High — the persisted state and the in-memory state disagree across restarts and failures, and alerts can be lost
- **Suspected bug:** if `Notify` fails because the shell hasn't yet taken org.freedesktop.Notifications at login or is restarting, the alert stays in `fired`, is saved, and is never retried, so catch-up alerts at login can vanish (confidence: medium). After an agent restart (`Restart=on-failure`), Snooze and Open on its notifications still on screen (timeout 0, `desktop/aizome/apps/koyomi/src/agent.rs:324`) do nothing, because `handle` finds no `shown` entry (`desktop/aizome/apps/koyomi/src/agent.rs:237`, `:244`) (confidence: high)
- **Related:** aizome-shell `desktop/aizome/crates/aizome-shell/src/notifications/server.rs` (notification ids, ActionInvoked, NotificationClosed)

<a id="koy-03"></a>

### KOY-03 — Settings and window-state persistence (unsaved settings, debounced state, agent's stale copy)
- **Where:** `desktop/aizome/apps/koyomi/src/app.rs:233` (`Koyomi.settings`, `.state` :234, `.show_sidebar` :248, `.desktop_24h` :271); `desktop/aizome/apps/koyomi/src/settings.rs:61` (`Settings`), `desktop/aizome/apps/koyomi/src/settings.rs:140` (`State`, `.window` :145)
- **Encoding:** in-memory `settings` against `koyomi.toml`; `state` against `state.json`, saved through the `persist_seq` debounce (KOY-06); `state.window` read but never written; `show_sidebar` never stored; `desktop_24h` captured at start (`desktop/aizome/apps/koyomi/src/app.rs:296`); the agent's own copy (`desktop/aizome/apps/koyomi/src/agent.rs:346`)
- **States:** settings `Saved` ⇄ `Unsaved` (memory ≠ file); state `Saved` → `Pending(seq)` → `Saved`; across processes `AgentCurrent` | `AgentStale`
- **Transitions:** to `Unsaved`: FirstDay (`desktop/aizome/apps/koyomi/src/app.rs:1722`), WeekNumbers (`desktop/aizome/apps/koyomi/src/app.rs:1725`), ClockFormat (`desktop/aizome/apps/koyomi/src/app.rs:1726`), DefaultAlert/DefaultAllDayAlert (`desktop/aizome/apps/koyomi/src/app.rs:1727-1734`), Snooze (`desktop/aizome/apps/koyomi/src/app.rs:1735`), DayStarts (`desktop/aizome/apps/koyomi/src/app.rs:1737`), ImportInto `get_or_insert` (`desktop/aizome/apps/koyomi/src/app.rs:1608`); to `Saved` only at SettingsDone (`desktop/aizome/apps/koyomi/src/app.rs:1717-1719`) and MakeDefault (`desktop/aizome/apps/koyomi/src/app.rs:1496-1499`); state pending from set_view (`desktop/aizome/apps/koyomi/src/app.rs:773-775`), ToggleCalendar/Solo (`desktop/aizome/apps/koyomi/src/app.rs:1478`, `:1484`), SidebarWidth (`desktop/aizome/apps/koyomi/src/app.rs:1742-1743`); state saved at Persist (`desktop/aizome/apps/koyomi/src/app.rs:1781-1784`) and main-window close (`desktop/aizome/apps/koyomi/src/app.rs:1829-1831`)
- **Hazards:** `close_requested` saves `state` but not `settings`; ImportInto silently makes the import target the default calendar (`desktop/aizome/apps/koyomi/src/app.rs:1608`), and the next unrelated save persists it; `state.window` is never written (`desktop/aizome/apps/koyomi/src/app.rs:1366`), so the window size never persists; sidebar visibility isn't kept, and the menu bar's clock format (`desktop/aizome/apps/koyomi/src/app.rs:296`) isn't followed after start
- **Reify:** High — save points are scattered across handlers, and the loss is user-visible
- **Suspected bug:** View ▸ Week Numbers and View ▸ First Day of the Week (`desktop/aizome/apps/koyomi/src/ui/sheets.rs:197-205`, `:225-228`) change `settings` without saving it. The choice is lost at relaunch unless the Settings sheet is later closed with Done (confidence: high). "Snooze for" in Settings (`desktop/aizome/apps/koyomi/src/ui/sheets.rs:138-143`) is saved, but the running agent read `snooze_minutes` once (`desktop/aizome/apps/koyomi/src/agent.rs:105`, `:346`). A new snooze length only takes effect when the agent restarts, normally at the next login (confidence: high)
- **Related:** the agent in the same package (`desktop/aizome/apps/koyomi/src/agent.rs:346`); `--upcoming` reads `State.hidden` (`desktop/aizome/apps/koyomi/src/main.rs:36`)

<a id="koy-04"></a>

### KOY-04 — Navigation and selection: view, anchor, cursor, mini month, selected occurrence
- **Where:** `desktop/aizome/apps/koyomi/src/app.rs:238` (`Koyomi.view`, `.cursor` :240, `.anchor` :242, `.mini` :244, `.selected` :245, `.now` :246, `.today` :247)
- **Encoding:** partially explicit: the `View` enum, plus `anchor` (which days `range()` covers, `desktop/aizome/apps/koyomi/src/app.rs:358-376`), `cursor` (the highlighted day, also used by the agenda, the week number and New Event), `mini` (the browsed month), `selected: Option<OccRef>`, and `today`/`now` from Tick
- **States:** `{Day, Week, Month, Year}` × (`cursor == anchor` | `cursor ≠ anchor`, inside the range) × `selected` (`None` | `Visible` | `Stale`, meaning not in `occ` after navigation or hiding)
- **Transitions:** go sets cursor, anchor and mini (`desktop/aizome/apps/koyomi/src/app.rs:751-756`); step (`desktop/aizome/apps/koyomi/src/app.rs:758-768`); set_view keeps anchor and cursor (`desktop/aizome/apps/koyomi/src/app.rs:770-777`); a second GoTo on the same day in Week → Day (`desktop/aizome/apps/koyomi/src/app.rs:1406-1411`); GridAction::Day sets only cursor when the day is in range (`desktop/aizome/apps/koyomi/src/app.rs:1424-1434`); Mini Pick/Browse (`desktop/aizome/apps/koyomi/src/app.rs:1412-1417`); ShowDay/ShowWeek/ShowMonth (`desktop/aizome/apps/koyomi/src/app.rs:1449-1460`); Today (`desktop/aizome/apps/koyomi/src/app.rs:1401-1405`); `--show` (`desktop/aizome/apps/koyomi/src/app.rs:1309-1317`); Tick day rollover (`desktop/aizome/apps/koyomi/src/app.rs:1745-1752`); select_key (`desktop/aizome/apps/koyomi/src/app.rs:900-907`); Deselect (`desktop/aizome/apps/koyomi/src/app.rs:1468`); cleared by delete (`desktop/aizome/apps/koyomi/src/app.rs:983`), Undo (`desktop/aizome/apps/koyomi/src/app.rs:1696`), calendar deletion (`desktop/aizome/apps/koyomi/src/app.rs:1576`); filtered on reload (`desktop/aizome/apps/koyomi/src/app.rs:1759`)
- **Hazards:** `select_key` has an empty branch (`desktop/aizome/apps/koyomi/src/app.rs:902-906`); the selection survives navigation and hiding while `selected_event()` (`desktop/aizome/apps/koyomi/src/app.rs:420-425`) quietly returns None; Redo doesn't clear a selection the way Undo does; zone and locale are fixed at start
- **Reify:** High — six fields share an implicit `cursor ∈ range` invariant, and two bugs follow from it
- **Suspected bug:** single-click a day in Month or Week view (GridAction::Day sets only `cursor`, `desktop/aizome/apps/koyomi/src/app.rs:1431`), then press ⌘1 or Day. The Day view shows `anchor` (`desktop/aizome/apps/koyomi/src/app.rs:362`; title `desktop/aizome/apps/koyomi/src/app.rs:382`), but the agenda (`desktop/aizome/apps/koyomi/src/ui/mod.rs:411`), the status bar's week (`desktop/aizome/apps/koyomi/src/ui/mod.rs:338`) and New Event (`desktop/aizome/apps/koyomi/src/app.rs:1646`) use the clicked `cursor` day (confidence: high). Select an event, then press → or hide its calendar, then press ⌫. `Msg::Delete` acts on `selected` (`desktop/aizome/apps/koyomi/src/app.rs:1678-1681`; the menu item is enabled by `selected.is_some()`, `desktop/aizome/apps/koyomi/src/ui/sheets.rs:157`), so it deletes an event no longer on screen, while Edit does nothing (confidence: medium)

<a id="koy-05"></a>

### KOY-05 — Modal sheet slots: editor, import, name, settings
- **Where:** `desktop/aizome/apps/koyomi/src/app.rs:249` (`Koyomi.editor`, `.import` :250, `.name` :251, `.settings_sheet` :252)
- **Encoding:** four independent `Option`s, each holding a kit sheet `WindowKey`; `view()` dispatches by key and falls back to the main window's view for a key no slot holds (`desktop/aizome/apps/koyomi/src/app.rs:1796-1819`)
- **States:** per slot `Closed` → `Open(key)` → `Closed`. Representable: several slots open at once, and a slot replaced while its old sheet is still stacked, leaving an orphan key
- **Transitions:** open: new_event (`desktop/aizome/apps/koyomi/src/app.rs:1022-1023`), edit_selected (`desktop/aizome/apps/koyomi/src/app.rs:1038-1040`), open_import (`desktop/aizome/apps/koyomi/src/app.rs:1233-1234`), NewCalendar (`desktop/aizome/apps/koyomi/src/app.rs:1502-1503`), RenameCalendar (`desktop/aizome/apps/koyomi/src/app.rs:1508-1509`), ShowSettings, which checks first (`desktop/aizome/apps/koyomi/src/app.rs:1709-1711`). Close: save_edit (`desktop/aizome/apps/koyomi/src/app.rs:1065-1066`), EditCancel (`desktop/aizome/apps/koyomi/src/app.rs:1665-1666`), EditDelete (`desktop/aizome/apps/koyomi/src/app.rs:1671-1672`), NameDone (`desktop/aizome/apps/koyomi/src/app.rs:1518-1520`), NameCancel (`desktop/aizome/apps/koyomi/src/app.rs:1552-1553`), finish_import (`desktop/aizome/apps/koyomi/src/app.rs:1238-1239`), ImportCancel (`desktop/aizome/apps/koyomi/src/app.rs:1640-1641`), SettingsDone (`desktop/aizome/apps/koyomi/src/app.rs:1714-1715`), close_requested (`desktop/aizome/apps/koyomi/src/app.rs:1833-1844`)
- **Hazards:** only `settings_sheet` checks whether it's already open. The kit stacks sheets and blocks menus while one is up, but `Msg::Instance` (a second launch) isn't blocked
- **Reify:** High — four slots with nothing keeping them exclusive, and the orphan state is reachable
- **Suspected bug:** with the editor open, the desktop action New Event (`aizome-koyomi --new`, `desktop/aizome/apps/koyomi/data/org.aizome.Koyomi.desktop:19`) reaches take_args → NewEvent (`desktop/aizome/apps/koyomi/src/app.rs:1301-1305`). A new Editor replaces the slot (`desktop/aizome/apps/koyomi/src/app.rs:1023`) without closing the old sheet, whose key now renders `ui::view`, the whole main window, as a stacked sheet. Opening an .ics from Files while the import sheet is open does the same to `import` (`desktop/aizome/apps/koyomi/src/app.rs:1333`, `:1234`) (confidence: medium)
- **Related:** aizome-kit `desktop/aizome/crates/aizome-kit/src/engine.rs:427` (`open_sheet` pushes onto the window's sheet stack), `desktop/aizome/crates/aizome-kit/src/app.rs:200` (asks the program's view for every sheet key)

<a id="koy-06"></a>

### KOY-06 — Sequence-guarded one-shot timers: status note, state persist, grid scroll request
- **Where:** `desktop/aizome/apps/koyomi/src/app.rs:253` (`Koyomi.note`), `desktop/aizome/apps/koyomi/src/app.rs:268` (`.persist_seq`, `.note_seq` :269, `.scroll` :270); `desktop/aizome/apps/koyomi/src/ui/week.rs:184` (`GridState.token`)
- **Encoding:** a counter bumped per request plus a delayed message carrying it, with stale timers dropped by comparing; the scroll request is a `(token, minute)` pair the grid acts on once
- **States:** note `None` → `Shown(seq)` → `None` (6 s); persist `Idle` → `Pending(seq)` → `Saved` (600 ms); scroll `Requested(token)` → `Applied`
- **Transitions:** note (`desktop/aizome/apps/koyomi/src/app.rs:745-749`) → ClearNote (`desktop/aizome/apps/koyomi/src/app.rs:1786-1789`); persist_soon (`desktop/aizome/apps/koyomi/src/app.rs:779-785`) → Persist (`desktop/aizome/apps/koyomi/src/app.rs:1781-1784`); the scroll token is bumped by Today (`desktop/aizome/apps/koyomi/src/app.rs:1404`) and DayStarts (`desktop/aizome/apps/koyomi/src/app.rs:1738`) and applied in grid layout (`desktop/aizome/apps/koyomi/src/ui/week.rs:303-308`)
- **Hazards:** none observed
- **Reify:** Low — small, local and correctly guarded

<a id="koy-07"></a>

### KOY-07 — Derived occurrence caches: index, generation, occ, view-model memo
- **Where:** `desktop/aizome/apps/koyomi/src/app.rs:255` (`Koyomi.occ`, `.index` :257, `.generation` :259, `.models` :260); `desktop/aizome/apps/koyomi/src/occur.rs:81` (`Index`)
- **Encoding:** `Index` (per-calendar positional bounds, overrides and masters); `occ: Rc<Vec<Occurrence>>` for `range()`; the `generation` counter; `Models.key`, a hash of the inputs with `0` as "invalid" (`desktop/aizome/apps/koyomi/src/app.rs:453`) and `Rc::as_ptr(&occ)` in the key (`desktop/aizome/apps/koyomi/src/app.rs:478`)
- **States:** `Fresh` → `IndexStale` (events mutated, index not yet refreshed) → `OccStale` → `ModelsStale` → rebuilt lazily when viewed
- **Transitions:** changed() bumps generation and refreshes one calendar's index, then occ (`desktop/aizome/apps/koyomi/src/app.rs:789-796`); reindex rebuilds everything (`desktop/aizome/apps/koyomi/src/app.rs:805-809`); refresh rebuilds occ and zeroes the key (`desktop/aizome/apps/koyomi/src/app.rs:450-454`); models() rebuilds when the key changes (`desktop/aizome/apps/koyomi/src/app.rs:483-491`); ToggleCalendar/Solo (`desktop/aizome/apps/koyomi/src/app.rs:1476-1477`, `:1482-1483`); FirstDay (`desktop/aizome/apps/koyomi/src/app.rs:1723`); Tick day rollover (`desktop/aizome/apps/koyomi/src/app.rs:1747-1751`); Poll reload (`desktop/aizome/apps/koyomi/src/app.rs:1757-1758`); Index::refresh (`desktop/aizome/apps/koyomi/src/occur.rs:116-124`)
- **Hazards:** the index is positional (calendar, event), so it must be refreshed after every mutation of a calendar's `events` before `occurrences` runs, or events are silently skipped (`desktop/aizome/apps/koyomi/src/occur.rs:191`); `changed()` refreshes only the named calendar, so moving a series between calendars depends on calling it for both (`desktop/aizome/apps/koyomi/src/app.rs:1159-1161`); `week_models()` and `mini_model()` `expect` the cache to match the view (`desktop/aizome/apps/koyomi/src/app.rs:729-731`, `:740`); invalidation is spread over a dozen sites (KOY-14 shows one that's missed)
- **Reify:** Medium — correct on the usual paths, but invalidation is manual and scattered
- **Related:** the agent keeps its own index (`desktop/aizome/apps/koyomi/src/agent.rs:116`)

<a id="koy-08"></a>

### KOY-08 — Series-scope confirmation (`pending` + alert)
- **Where:** `desktop/aizome/apps/koyomi/src/app.rs:262` (`Koyomi.pending: Option<Pending>`; enum at `desktop/aizome/apps/koyomi/src/app.rs:176`)
- **Encoding:** partially explicit: `Pending::{Move, Delete, Save}` held while a kit alert asks This / Future / All; `Save` depends on `editor` (KOY-05) still holding the draft
- **States:** `Idle` → `Asking(Move | Delete | Save)` → `Idle` (Scope or ScopeCancel)
- **Transitions:** reschedule sets Move (`desktop/aizome/apps/koyomi/src/app.rs:918-930`); ask_delete sets Delete (`desktop/aizome/apps/koyomi/src/app.rs:991-1003`); save_edit sets Save (`desktop/aizome/apps/koyomi/src/app.rs:1054-1063`); Scope takes and applies, turning Future into All for Save (`desktop/aizome/apps/koyomi/src/app.rs:1683-1688`); ScopeCancel (`desktop/aizome/apps/koyomi/src/app.rs:1689`)
- **Hazards:** Poll isn't gated on `pending`, so a reload can replace the data the `OccRef` came from before the answer arrives (it's applied by uid, so usually still right); EditDelete closes the editor before asking (`desktop/aizome/apps/koyomi/src/app.rs:1670-1677`), so Cancel on a series discards the draft; if the editor is gone when Save is answered, `save_edit` returns silently (`desktop/aizome/apps/koyomi/src/app.rs:1045`)
- **Reify:** Medium — the enum exists, but whether it's valid depends on other slots

<a id="koy-09"></a>

### KOY-09 — Undo/redo history
- **Where:** `desktop/aizome/apps/koyomi/src/app.rs:263` (`Koyomi.undo`, `.redo` :264); `Change`/`Step` at `desktop/aizome/apps/koyomi/src/app.rs:184-199`
- **Encoding:** two stacks of inverse snapshots (a UID's events, a removed calendar, a made id, name and colour) captured before each change; redo is cleared on record; both are cleared on reload
- **States:** `Empty` → `CanUndo` ⇄ `CanUndo+CanRedo` → `Invalidated` (reload)
- **Transitions:** record (`desktop/aizome/apps/koyomi/src/app.rs:841-847`) from do_reschedule (`desktop/aizome/apps/koyomi/src/app.rs:955`), delete (`desktop/aizome/apps/koyomi/src/app.rs:981`), save_edit (`desktop/aizome/apps/koyomi/src/app.rs:1088`, `:1158`), duplicate (`desktop/aizome/apps/koyomi/src/app.rs:1187`), finish_import (`desktop/aizome/apps/koyomi/src/app.rs:1245`, `:1267`), SetColor (`desktop/aizome/apps/koyomi/src/app.rs:1491`), NameDone (`desktop/aizome/apps/koyomi/src/app.rs:1532`, `:1544`), DeleteCalendarConfirmed (`desktop/aizome/apps/koyomi/src/app.rs:1575`); Undo (`desktop/aizome/apps/koyomi/src/app.rs:1691-1699`); Redo (`desktop/aizome/apps/koyomi/src/app.rs:1700-1707`); apply builds the inverse (`desktop/aizome/apps/koyomi/src/app.rs:851-898`); a Poll reload clears both (`desktop/aizome/apps/koyomi/src/app.rs:1761-1762`)
- **Hazards:** any external change wipes the history, and KOY-14's perpetual reload wipes it every 3 s; a failed restore or remove in `apply` (`desktop/aizome/apps/koyomi/src/app.rs:873`, `:881`) pushes no inverse, so a level is lost silently; Undo clears the selection but Redo doesn't (`desktop/aizome/apps/koyomi/src/app.rs:1696` vs `:1700-1706`); the 100-step cap uses `Vec::remove(0)` (`desktop/aizome/apps/koyomi/src/app.rs:843-845`); the shortcuts still work during a pointer drag (KOY-17, KOY-18)
- **Reify:** Medium — records come from nine handlers, and invalidation is tied to the store

<a id="koy-10"></a>

### KOY-10 — Calendar save pipeline: dirty → debounce → background write → stamp
- **Where:** `desktop/aizome/apps/koyomi/src/app.rs:265` (`Koyomi.dirty`, `.writing` :266, `.flush_seq` :267); `desktop/aizome/apps/koyomi/src/store.rs:29` (`Store.stamps`)
- **Encoding:** `dirty` calendar ids; the `flush_seq` debounce token; an in-flight `writing` count per id; the background job's `Msg::Saved(id, stamp, err)`; `Store.stamps`; the Poll gate `dirty.is_empty() && writing == 0`
- **States:** per calendar `Clean` → `Dirty` (300 ms debounce) → `Writing(n)` → `Clean(stamp)` | `FailedClean` (the bookkeeping says clean, but the file is older than memory); external reads are suppressed while `Dirty` or `Writing`
- **Transitions:** changed() adds to dirty and arms Flush (`desktop/aizome/apps/koyomi/src/app.rs:795-799`), or flushes at once in tests (`desktop/aizome/apps/koyomi/src/app.rs:801`); Flush(seq) guard (`desktop/aizome/apps/koyomi/src/app.rs:1765-1768`); flush takes dirty, bumps writing and spawns the write (`desktop/aizome/apps/koyomi/src/app.rs:813-825`), or saves inline (`desktop/aizome/apps/koyomi/src/app.rs:826-827`); Saved decrements, and records the stamp once the count reaches 0 (`desktop/aizome/apps/koyomi/src/app.rs:1770-1779`); Poll reloads when idle (`desktop/aizome/apps/koyomi/src/app.rs:1754-1763`); main-window close flushes (`desktop/aizome/apps/koyomi/src/app.rs:1828`); write_atomic (`desktop/aizome/apps/koyomi/src/settings.rs:167-174`)
- **Hazards:** writes spawned at close (`desktop/aizome/apps/koyomi/src/app.rs:1828`) aren't awaited: the kit quits when the last window closes, and the process exits with pool threads mid-write (the old file survives, the edit is lost); while dirty or writing, external changes aren't read, and the next write overwrites them; two in-flight writes of one calendar share the temp name `.{name}.{pid}.tmp` (`desktop/aizome/apps/koyomi/src/settings.rs:169`) and can finish out of order, leaving the older snapshot on disk with its stamp recorded; a write still in flight when its calendar is removed recreates the file, and the next Poll brings the calendar back
- **Reify:** High — four fields across app and store, background threads, and data-loss consequences
- **Suspected bug:** when a background write fails (disk full, read-only folder), `dirty` has already been taken, and Saved records the unchanged file's stamp (`desktop/aizome/apps/koyomi/src/app.rs:1774-1775`). Nothing retries, so the edit lives only in memory. It's lost at quit, because close flushes only `dirty` (`desktop/aizome/apps/koyomi/src/app.rs:1828`), and the next external change's reload discards it (confidence: medium)
- **Related:** the agent reads the same files (`desktop/aizome/apps/koyomi/src/agent.rs:113`); CLI `--import` writes them (`desktop/aizome/apps/koyomi/src/main.rs:127-129`)

<a id="koy-11"></a>

### KOY-11 — Launch phase and argument hand-off
- **Where:** `desktop/aizome/apps/koyomi/src/app.rs:272` (`Koyomi.instance`, `.waiting` :274, `.new_waiting` :276, `.args` :278, `.window` :261)
- **Encoding:** `window == WindowKey::NONE` means not yet launched; `args: Option<Vec<String>>` is set once and never cleared; `waiting` files and `new_waiting` queue requests made before launch; `instance` is taken at launch
- **States:** `Constructed` → `Launched` (window open, args taken, queue drained); `open()` is ignored while `args` is Some
- **Transitions:** main sets args and the listener (`desktop/aizome/apps/koyomi/src/app.rs:1853-1857`); launched opens the window, starts Tick and Poll, serves the instance, takes args and drains the queue (`desktop/aizome/apps/koyomi/src/app.rs:1365-1382`); open() (`desktop/aizome/apps/koyomi/src/app.rs:1385-1394`); take_args queues before launch (`desktop/aizome/apps/koyomi/src/app.rs:1302-1303`, `:1330-1331`) or acts after (`desktop/aizome/apps/koyomi/src/app.rs:1305`, `:1333`)
- **Hazards:** with both files and `--new` queued, `--new` is dropped (else-if, `desktop/aizome/apps/koyomi/src/app.rs:1377-1382`); `--show` from Month or Year also switches to Day and persists that (`desktop/aizome/apps/koyomi/src/app.rs:1313-1315`)
- **Reify:** Low — one-way and local

<a id="koy-12"></a>

### KOY-12 — Recurring series split state: master rule, EXDATE/RDATE, RECURRENCE-ID overrides
- **Where:** `desktop/aizome/apps/koyomi/src/model.rs:261` (`Event.rrule`, `.rdates` :262, `.exdates` :263, `.recurrence_id` :266, `.sequence` :269); transitions in `desktop/aizome/apps/koyomi/src/edit.rs:167` (`reschedule`), `desktop/aizome/apps/koyomi/src/edit.rs:213` (`shift_series`), `desktop/aizome/apps/koyomi/src/edit.rs:290` (`delete`)
- **Encoding:** persisted in the .ics: one master per UID (`Calendar::master`, `desktop/aizome/apps/koyomi/src/model.rs:813-815`), override events carrying `RECURRENCE-ID`, EXDATEs and RDATEs on the master, COUNT/UNTIL in the rule; `SEQUENCE`/`LAST-MODIFIED` bumped by `touch` (`desktop/aizome/apps/koyomi/src/model.rs:369-374`) and compared by import merge (`desktop/aizome/apps/koyomi/src/store.rs:230`)
- **States:** `Single` → `Series` → `Series+Exceptions` (EXDATEs, overrides) → `Truncated` (UNTIL set by a Future delete) → `Deleted`. Representable but invalid: an override whose RECURRENCE-ID no longer names an instance (rule changed or removed); an override with no master, which is shown on its own (`desktop/aizome/apps/koyomi/src/occur.rs:199-206`); override rids moved but override times not
- **Transitions:** move This without an override pushes one (`desktop/aizome/apps/koyomi/src/edit.rs:188-195`), with one sets its times (`desktop/aizome/apps/koyomi/src/edit.rs:197-199`); move All/Future shifts the master, its EXDATEs and RDATEs, the rule, and every override's rid (`desktop/aizome/apps/koyomi/src/edit.rs:202-205`, `:246-268`); delete This adds an EXDATE and drops the override (`desktop/aizome/apps/koyomi/src/edit.rs:297-306`); delete Future sets UNTIL, clears COUNT and drops later RDATEs and overrides (`desktop/aizome/apps/koyomi/src/edit.rs:308-334`), or removes everything if cut at the start (`desktop/aizome/apps/koyomi/src/edit.rs:311-312`); delete All (`desktop/aizome/apps/koyomi/src/edit.rs:294-296`); editor save makes an unmoved override for This (`desktop/aizome/apps/koyomi/src/app.rs:1114-1117`) and replaces the master's rule (`desktop/aizome/apps/koyomi/src/app.rs:1134-1143`); undo restores whole-UID snapshots (`desktop/aizome/apps/koyomi/src/app.rs:855-867`)
- **Hazards:** `Scope::Future` moves the whole series, past instances included (`desktop/aizome/apps/koyomi/src/edit.rs:202`); this is unreachable from the UI only because the alerts offer This/All for moves and Save maps Future to All (`desktop/aizome/apps/koyomi/src/app.rs:1686`). `shift_series` moves each override's RECURRENCE-ID but not its DTSTART, so an override made only to retitle one instance (unmoved, `desktop/aizome/apps/koyomi/src/app.rs:1116`) stays at the old time when the series moves. SEQUENCE goes up twice per editor save (`desktop/aizome/apps/koyomi/src/edit.rs:184` or `:199`, then `desktop/aizome/apps/koyomi/src/app.rs:1144`)
- **Reify:** High — the invariants live in the file format and are maintained by hand in several functions
- **Suspected bug:** take a weekly series with one moved instance, open another instance, set Repeat to Never, and Save → All Events. The master's rule is cleared (`desktop/aizome/apps/koyomi/src/app.rs:1141`) but the override stays, so the moved instance still shows as a separate event (`desktop/aizome/apps/koyomi/src/occur.rs:199-206`). Changing the frequency leaves overrides and EXDATEs for instances that no longer exist in the same way (confidence: medium)

<a id="koy-13"></a>

### KOY-13 — Single-instance socket: claim, hand off, serve
- **Where:** `desktop/aizome/apps/koyomi/src/instance.rs:26` (`claim`), `desktop/aizome/apps/koyomi/src/instance.rs:45` (`serve`); socket `$XDG_RUNTIME_DIR/aizome-app.koyomi.sock` (`desktop/aizome/apps/koyomi/src/instance.rs:11-17`)
- **Encoding:** the socket file, and whether anything accepts on it; `Claim::{First, Handed, Alone}`; the serve thread lives until the program's channel closes
- **States:** `Probe` → `Handed` (connect and write succeed) | `StaleRemoved` → `Listening` (First) | `Alone` (bind failed); `Listening` → `Serving(args)` → `Gone` (send fails)
- **Transitions:** connect and write (`desktop/aizome/apps/koyomi/src/instance.rs:28-32`); remove the path (`desktop/aizome/apps/koyomi/src/instance.rs:34`); bind (`desktop/aizome/apps/koyomi/src/instance.rs:35-40`); serve reads to EOF with a 500 ms timeout per client (`desktop/aizome/apps/koyomi/src/instance.rs:51-57`), sends `Msg::Instance`, stops when that fails (`desktop/aizome/apps/koyomi/src/instance.rs:58-60`); used at launch (`desktop/aizome/apps/koyomi/src/app.rs:1854-1857`, `:1371-1373`)
- **Hazards:** two launches at once can both fail to connect; the second's `remove_file` deletes the first's socket, and both run as first instances with separate in-memory copies of the same calendars; the socket is never removed at exit (a later launch removes it after a refused connect); `Koyomi::new` loads every calendar, and may create Home/Work, before `claim` (`desktop/aizome/apps/koyomi/src/app.rs:1852-1854`), so every hand-off launch (agent Open, desktop actions) parses everything first; a client that connects and stalls delays later ones by up to 500 ms
- **Reify:** Medium — small, but the gap between remove and bind is a real race
- **Related:** the agent's Open launches through it (`desktop/aizome/apps/koyomi/src/agent.rs:337`)

<a id="koy-14"></a>

### KOY-14 — Store's view of the calendar folder: stamps, reload, trash
- **Where:** `desktop/aizome/apps/koyomi/src/store.rs:29` (`Store.stamps`, `.calendars` :27, `.problems` :31)
- **Encoding:** `calendars` and `stamps` as parallel vectors, with (mtime, length) per file; files moved into `.trash/` for removed calendars; `problems` for unreadable files
- **States:** per file `Unknown` → `Loaded(stamp)` → `ChangedOnDisk` → `Loaded` (reload); `Created(stamp None)` → `Saved(stamp)`; `Removed` (in .trash) → `Restored`
- **Transitions:** load (`desktop/aizome/apps/koyomi/src/store.rs:54-76`); changed_on_disk (`desktop/aizome/apps/koyomi/src/store.rs:97-109`); reload (`desktop/aizome/apps/koyomi/src/store.rs:112-114`); save records the stamp (`desktop/aizome/apps/koyomi/src/store.rs:117-122`); saved from background writes (`desktop/aizome/apps/koyomi/src/store.rs:133-137`); create pushes, then saves (`desktop/aizome/apps/koyomi/src/store.rs:143-147`); remove renames into .trash (`desktop/aizome/apps/koyomi/src/store.rs:174-184`); restore (`desktop/aizome/apps/koyomi/src/store.rs:188-195`); ensure_some (`desktop/aizome/apps/koyomi/src/store.rs:199-204`)
- **Hazards:** the stamp is (mtime, length), so a same-length rewrite within the mtime granularity is missed; two files with one stem (`a.ics`, `a.ICS`) get the same id; `calendars` is sorted only at load, create and restore, so a rename isn't re-sorted until a reload; `create` and `restore` leave the calendar in memory when the write fails
- **Reify:** High — the stamps' meaning is shared with KOY-10, and there are two failure modes below
- **Suspected bug:** an unreadable .ics in the folder (permission denied) is counted by `files()` but not loaded, so `changed_on_disk` stays true forever (`desktop/aizome/apps/koyomi/src/store.rs:99-101`). The app then reloads every 3 s and clears undo/redo each time (`desktop/aizome/apps/koyomi/src/app.rs:1755-1762`), and the agent rebuilds its index at every wake (`desktop/aizome/apps/koyomi/src/agent.rs:114-116`) (confidence: medium). If `create` fails to write, the new calendar stays sorted into `calendars` (`desktop/aizome/apps/koyomi/src/store.rs:143-147`), but NameDone and finish_import skip `reindex` (`desktop/aizome/apps/koyomi/src/app.rs:1536`, `:1249-1252`), so `occ` and the index point at shifted calendars. In Day view the agenda's `calendars[o.cal].events[o.event]` (`desktop/aizome/apps/koyomi/src/ui/mod.rs:423-424`) can then index past the end and panic (confidence: medium)
- **Related:** the agent's refresh (`desktop/aizome/apps/koyomi/src/agent.rs:113`)

<a id="koy-15"></a>

### KOY-15 — Event editor draft
- **Where:** `desktop/aizome/apps/koyomi/src/ui/editor.rs:48` (`Editor.target` :51, `.series` :53, `.all_day` :58, `.repeat` :64, `.alert` :67, `.length` :69, `.error` :70, `.changed` :72)
- **Encoding:** `target` None (new) or the occurrence; `repeat: Option<Repeat>`, where None means a rule the editor can't show, kept as is; `alert: AlertChoice`, including `Kept`; `length`, kept so the end follows the start; text fields parsed on demand; `error` until the next edit
- **States:** `New` | `Edit` | `EditSeries`; repeat `Custom(kept)` → `Never` | `Freq{…}`; alert `None` | `Before(m)` | `Kept`; `Valid` | `Error(msg)`; `AwaitingScope` (KOY-08) → `Closed`
- **Transitions:** opened (`desktop/aizome/apps/koyomi/src/app.rs:1023`, `:1039-1040`); `update` per field (`desktop/aizome/apps/koyomi/src/ui/editor.rs:230-367`): AllDay resets the length and the alert (`desktop/aizome/apps/koyomi/src/ui/editor.rs:239-259`), start fields move the end (`desktop/aizome/apps/koyomi/src/ui/editor.rs:260-267`, `:204-218`), end fields recompute the length (`desktop/aizome/apps/koyomi/src/ui/editor.rs:268-275`, `:220-228`), Freq leaves Custom (`desktop/aizome/apps/koyomi/src/ui/editor.rs:276-292`); save validates (`desktop/aizome/apps/koyomi/src/app.rs:1046-1051`), then asks the scope (`desktop/aizome/apps/koyomi/src/app.rs:1054-1063`) or closes (`desktop/aizome/apps/koyomi/src/app.rs:1065-1066`); cancel, delete and close (`desktop/aizome/apps/koyomi/src/app.rs:1664-1677`, `:1833-1835`)
- **Hazards:** `changed` is set (`desktop/aizome/apps/koyomi/src/ui/editor.rs:231`) but never read, so Cancel, Escape or closing the sheet discards edits without asking; turning All-day on and off loses the original time and length (`desktop/aizome/apps/koyomi/src/ui/editor.rs:243`, `:250-251`); `length` goes stale while a start or end field doesn't parse; Poll isn't gated on the editor, so a reload while it's open leaves `target` pointing at a re-read event (applied by uid); for Scope::This the Repeat control shows the master's rule, but changes to it are silently ignored (`desktop/aizome/apps/koyomi/src/app.rs:1134`)
- **Reify:** Medium — several coupled modes in one struct, but local to the sheet

<a id="koy-16"></a>

### KOY-16 — Month chip drag
- **Where:** `desktop/aizome/apps/koyomi/src/ui/month.rs:239` (`DragState.press`, `.target`, `.at`)
- **Encoding:** widget-persistent state: `press` (origin, a copy of the chip, home cell), `target` cell, `at` pointer; `at.is_none()` means still under the 4 pt threshold
- **States:** `Idle` → `Pressed` → `Dragging` → `Idle` (drop: Move if target ≠ home)
- **Transitions:** press on a chip captures the pointer (`desktop/aizome/apps/koyomi/src/ui/month.rs:426-434`); moving past 4 pt sets target and at (`desktop/aizome/apps/koyomi/src/ui/month.rs:445-467`); release takes the state and emits Move (`desktop/aizome/apps/koyomi/src/ui/month.rs:469-495`)
- **Hazards:** no cancel: Escape and focus loss don't end it, and the kit has no pointer-cancel event; home and target are cells re-read from whatever month is current at the drop (`desktop/aizome/apps/koyomi/src/ui/month.rs:480-481`), so pressing ← or → mid-drag drops by cell offset in the new month
- **Reify:** Low — small and local; the chip is held by value, so data changes are safe

<a id="koy-17"></a>

### KOY-17 — Time-grid pointer drag: move, resize, create; edge hover and scroll
- **Where:** `desktop/aizome/apps/koyomi/src/ui/week.rs:181` (`GridState.scroll` :182, `.token` :184, `.drag` :185, `.edge` :187); `Drag` at `desktop/aizome/apps/koyomi/src/ui/week.rs:168` (`.kind`, `.block` :171, `.active` :175, `.ghost` :177)
- **Encoding:** partially explicit: `DragKind` (`desktop/aizome/apps/koyomi/src/ui/week.rs:161`) inside an `Option<Drag>` whose `block: Option<usize>` is an index into the current `GridData.blocks`, `active` (past the slop) and `ghost` (column, minutes); `edge` for the resize cursor; widget state keyed by tree position, so it survives navigation and reloads within a view
- **States:** `Idle` → `Pressed(kind)` → `Active(kind, ghost)` → `Idle` (emits Move, Resize, Create or nothing); `Hover(edge)` while idle
- **Transitions:** press on a block or on empty time (`desktop/aizome/apps/koyomi/src/ui/week.rs:479-517`); slop → active (`desktop/aizome/apps/koyomi/src/ui/week.rs:531-538`); ghost updates and auto-scroll (`desktop/aizome/apps/koyomi/src/ui/week.rs:539-573`); release clears and emits (`desktop/aizome/apps/koyomi/src/ui/week.rs:576-609`); edge hover (`desktop/aizome/apps/koyomi/src/ui/week.rs:523-528`); PointerLeave (`desktop/aizome/apps/koyomi/src/ui/week.rs:611-616`); scroll request (`desktop/aizome/apps/koyomi/src/ui/week.rs:303-308`, KOY-06)
- **Hazards:** no cancel path (Escape, window focus loss); `block` and `ghost.0` are indices re-resolved against whatever data the widget holds at paint (`desktop/aizome/apps/koyomi/src/ui/week.rs:396`) and at the drop (`desktop/aizome/apps/koyomi/src/ui/week.rs:588-589`)
- **Reify:** High — an enum plus index-valued fields that go stale, with a wrong-event move below
- **Suspected bug:** drag an event in Week view and press → (or ⌘→) before releasing, to carry it into the next week. The `Drag.block` index (`desktop/aizome/apps/koyomi/src/ui/week.rs:484`) is re-resolved against the next week's blocks (`desktop/aizome/apps/koyomi/src/ui/week.rs:549`, `:589`), so the drop moves whichever event has that index there, or nothing, instead of the event that was picked up. An external change read by Poll mid-drag does the same (confidence: medium)

<a id="koy-18"></a>

### KOY-18 — All-day strip drag
- **Where:** `desktop/aizome/apps/koyomi/src/ui/week.rs:946` (`StripState.press`, `.ghost`)
- **Encoding:** `press: Option<(origin, Option<bar index>)>` into the current `StripData.bars`, and `ghost` columns
- **States:** `Idle` → `Pressed(i)` → `Dragging(ghost)` → `Idle` (Move if the column shift ≠ 0)
- **Transitions:** press on a bar captures the pointer (`desktop/aizome/apps/koyomi/src/ui/week.rs:1081-1086`); move past the slop sets the ghost (`desktop/aizome/apps/koyomi/src/ui/week.rs:1104-1127`); release takes the state and emits Move (`desktop/aizome/apps/koyomi/src/ui/week.rs:1129-1154`)
- **Hazards:** no cancel path; the bar is held by index, not by key
- **Reify:** High — small, but a crash below
- **Suspected bug:** drag an all-day bar and press → (Keys has the focus since the press) into a week with fewer all-day bars: PointerMove indexes `self.data.bars[i]` directly (`desktop/aizome/apps/koyomi/src/ui/week.rs:1109`, and the drop at `:1136`) and panics. With enough bars, it moves a different event (`desktop/aizome/apps/koyomi/src/ui/week.rs:1142-1150`) (confidence: medium)

<a id="koy-19"></a>

### KOY-19 — (test harness) private D-Bus daemon and fake notification server
- **Where:** `desktop/aizome/apps/koyomi/tests/agent.rs:21` (`Bus.child`, `.address`, `.dir`); `desktop/aizome/apps/koyomi/tests/agent.rs:72` (`Daemon.posted`, `.next`)
- **Encoding:** a spawned `dbus-daemon --nofork --print-address`, its first stdout line taken as the address, killed and reaped in Drop; a zbus-served fake Notifications with an id counter that echoes `replaces`
- **States:** `NotStarted` → `Spawned` → `AddressRead` → `Serving` → `Killed` (Drop); `Skipped` (no daemon, or no address)
- **Transitions:** spawn (`desktop/aizome/apps/koyomi/tests/agent.rs:41-53`); read the address (`desktop/aizome/apps/koyomi/tests/agent.rs:55-57`); serve (`desktop/aizome/apps/koyomi/tests/agent.rs:107-114`); emitted signals drive Snooze, Open and Closed (`desktop/aizome/apps/koyomi/tests/agent.rs:140-179`); Drop kills, waits and removes the directory (`desktop/aizome/apps/koyomi/tests/agent.rs:61-66`)
- **Hazards:** the test returns early and passes when `dbus-daemon` is missing or prints no address (`desktop/aizome/apps/koyomi/tests/agent.rs:49-51`, `:57`, `:105`), so a daemon that can't listen, for example on a socket path under a long `CARGO_TARGET_TMPDIR`, skips silently; `read_line` has no timeout
- **Reify:** Low — test fixture

## Already explicit

- `desktop/aizome/apps/koyomi/src/app.rs:34` `enum View` — the main view mode, persisted as `state.view`; every change goes through `fn set_view` (`desktop/aizome/apps/koyomi/src/app.rs:770`). Its coupling to anchor and cursor is KOY-04

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/apps/koyomi/data/aizome-koyomi-agent.service` | 30 | machines | [KOY-01](#koy-01) | agent unit: ExecCondition, PartOf, Restart=on-failure |
| `desktop/aizome/apps/koyomi/src/agent.rs` | 462 | machines | [KOY-01](#koy-01), [KOY-02](#koy-02), [KOY-03](#koy-03), [KOY-14](#koy-14) | agent run loop and persisted alert state |
| `desktop/aizome/apps/koyomi/src/app.rs` | 1860 | machines | [KOY-03](#koy-03), [KOY-04](#koy-04), [KOY-05](#koy-05), [KOY-06](#koy-06), [KOY-07](#koy-07), [KOY-08](#koy-08), [KOY-09](#koy-09), [KOY-10](#koy-10), [KOY-11](#koy-11), [KOY-12](#koy-12), [KOY-13](#koy-13), [KOY-14](#koy-14), [KOY-15](#koy-15) | program state: nav, sheets, saves, undo, scope prompt, launch |
| `desktop/aizome/apps/koyomi/src/edit.rs` | 661 | machines | [KOY-12](#koy-12) | recurrence edit scopes over series split state |
| `desktop/aizome/apps/koyomi/src/format.rs` | 262 | none |  | pure date/time formatting and parsing |
| `desktop/aizome/apps/koyomi/src/ics/mod.rs` | 335 | none |  | pure parser; BEGIN/END stack local to one call |
| `desktop/aizome/apps/koyomi/src/ics/value.rs` | 306 | none |  | pure value codecs |
| `desktop/aizome/apps/koyomi/src/instance.rs` | 63 | machines | [KOY-13](#koy-13) | single-instance socket claim and serve thread |
| `desktop/aizome/apps/koyomi/src/lib.rs` | 25 | none |  | module list |
| `desktop/aizome/apps/koyomi/src/locale.rs` | 149 | none |  | pure locale week and clock tables |
| `desktop/aizome/apps/koyomi/src/main.rs` | 162 | machines | [KOY-10](#koy-10) | CLI dispatch; --import is an external calendar writer |
| `desktop/aizome/apps/koyomi/src/model.rs` | 876 | machines | [KOY-12](#koy-12) | series fields; SEQUENCE bumped by touch |
| `desktop/aizome/apps/koyomi/src/occur.rs` | 485 | machines | [KOY-07](#koy-07), [KOY-12](#koy-12) | positional Index cache; overrides shown on their own |
| `desktop/aizome/apps/koyomi/src/rrule.rs` | 647 | none |  | pure RRULE expansion; loop guards local to one call |
| `desktop/aizome/apps/koyomi/src/settings.rs` | 193 | machines | [KOY-03](#koy-03), [KOY-10](#koy-10) | settings/state files; write_atomic pid temp name |
| `desktop/aizome/apps/koyomi/src/store.rs` | 314 | machines | [KOY-10](#koy-10), [KOY-14](#koy-14) | file stamps, reload, trash |
| `desktop/aizome/apps/koyomi/src/tz.rs` | 468 | none |  | zone resolution; Zones.resolved is a memo cache |
| `desktop/aizome/apps/koyomi/src/ui/colors.rs` | 80 | none |  | pure colour math |
| `desktop/aizome/apps/koyomi/src/ui/editor.rs` | 627 | machines | [KOY-15](#koy-15) | event editor draft modes |
| `desktop/aizome/apps/koyomi/src/ui/mini.rs` | 346 | none |  | stateless widgets; browse state lives in KOY-04 |
| `desktop/aizome/apps/koyomi/src/ui/mod.rs` | 503 | machines | [KOY-04](#koy-04), [KOY-14](#koy-14) | view composition; agenda reads cursor and indexes occ |
| `desktop/aizome/apps/koyomi/src/ui/month.rs` | 658 | machines | [KOY-16](#koy-16) | month chip drag state |
| `desktop/aizome/apps/koyomi/src/ui/sheets.rs` | 232 | machines | [KOY-03](#koy-03), [KOY-04](#koy-04), [KOY-05](#koy-05) | menus mutate settings unsaved; sheet views |
| `desktop/aizome/apps/koyomi/src/ui/week.rs` | 1163 | machines | [KOY-06](#koy-06), [KOY-17](#koy-17), [KOY-18](#koy-18) | time-grid and all-day strip drags; scroll token |
| `desktop/aizome/apps/koyomi/src/upcoming.rs` | 168 | none |  | pure report for the shell |
| `desktop/aizome/apps/koyomi/src/vtimezone.rs` | 92 | none |  | pure VTIMEZONE generation |
| `desktop/aizome/apps/koyomi/tests/agent.rs` | 180 | machines | [KOY-19](#koy-19) | private dbus-daemon and fake notification server |
| `desktop/aizome/apps/koyomi/tests/app.rs` | 465 | none |  | tests |
| `desktop/aizome/apps/koyomi/tests/common/mod.rs` | 191 | none |  | test fixtures and harness constructors |
| `desktop/aizome/apps/koyomi/tests/ics.rs` | 211 | none |  | tests |
| `desktop/aizome/apps/koyomi/tests/perf.rs` | 108 | none |  | tests (CPU budget measurement) |
