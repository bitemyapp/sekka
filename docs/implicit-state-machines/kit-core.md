# KIT: Toolkit runtime

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/crates/aizome-kit`. Audited at `8e0acbe5`; line numbers refer to that commit.

60 files, 0 pending. 33 implicit machines: 7 High, 9 Medium, 17 Low.

## Machines

<a id="kit-01"></a>

### KIT-01 — (test harness) Gallery demo and benchmark driver
- **Where:** `desktop/aizome/crates/aizome-kit/examples/gallery/views.rs:53` (`Msg::Demo`); `desktop/aizome/crates/aizome-kit/examples/gallery/views.rs:516` (`launched`), `:524` (`update`)
- **Encoding:** the `GALLERY_DEMO` value, re-read on every `Msg::Demo`, plus a `cx.after` one-shot timer that each bench step re-arms; `Gallery.page`, `.presses` and `.reveal` hold the step's progress
- **States:** `Off` (no env) → `Armed` (1200 ms timer) → `BenchLoop(mode)` (bench-toggle/select/scroll/scroll-editor/type, re-armed every 20 ms) | `OneShot` (popover, alert or the default menu)
- **Transitions:** launch arms 1200 ms (`views.rs:519-521`); bench modes re-arm 20 ms (`views.rs:532`, `:538`, `:546`, `:558`, `:572`); editor modes set the page and text on their first tick (`views.rs:549-553`, `:561-564`); popover/alert/menu end the loop (`views.rs:574-605`)
- **Hazards:** bench loops never stop; the mode is re-read from the environment on each tick; first-tick setup is keyed on `page != Editor`, so switching pages mid-run reloads the editor text
- **Reify:** Low — demo-only driver

<a id="kit-02"></a>

### KIT-02 — Draw-ahead of hidden windows (band drawing)
- **Where:** `desktop/aizome/crates/aizome-kit/src/ahead.rs:40` (`Drawing.scale`, `.bands`, `.armed` at `:44`); `desktop/aizome/crates/aizome-kit/src/engine.rs:175` (`Win.ahead`), `engine.rs:184` (`Win.refresh`)
- **Encoding:** `Win.ahead: Option<Drawing>` (off / drawing / complete when `bands.len()` reaches `ahead::bands(..).len()`), `Drawing.armed` (a zero-delay `TimerUse::Ahead` is in flight), `Win.refresh` (a hidden window may be viewed), with `Win.hidden`/`.built` and the platform's kept picture (KIT-03)
- **States:** `Off` → `Prepared` (ahead, refresh, dirty) → `Drawing(k of n bands, armed)` → `Complete` (picture handed over) → `Restarted` (bands cleared on rebuild or focus move) | `Off` on show; invalid but reachable: `armed` with no `TimerUse::Ahead` pending
- **Transitions:** `prepare_key` Off→Prepared, drops the old picture (`engine.rs:544-548`); `build` of a hidden prepared window → `restart_ahead` (`engine.rs:782-789`, `:578-583`); `arm_ahead` sets `armed` and a timer (`engine.rs:585-591`); `TimerUse::Ahead` → `draw_ahead` clears `armed`, draws a band, re-arms or completes (`engine.rs:2752-2756`, `:554-574`); focus moves while hidden restart it (`engine.rs:1881-1883`); `retheme` sets `refresh` (`engine.rs:2870`); `scale_changed` sets `refresh` (`engine.rs:1001`); `show_key` clears `ahead` and `refresh` (`engine.rs:488-489`)
- **Hazards:** `draw_ahead` returns before clearing `armed` when `!built` (`engine.rs:556-557`); `resized` sets `built=false` without `refresh` (`engine.rs:980-987`) and `dirty_windows` skips hidden windows without `refresh` (`engine.rs:656`), so a resize mid-drawing leaves `armed` stuck with no timer and the drawing stalls until the next `prepare_key`; `prepare_key` replacing a Drawing whose timer is in flight starts a second timer chain (harmless); completeness is implicit in band counts
- **Reify:** Medium — five fields over two structs, with a stuck-flag path
- **Related:** platform side KIT-03 (`desktop/aizome/crates/aizome-kit/src/app.rs:392`)

<a id="kit-03"></a>

### KIT-03 — Kept picture for a shown window's first frame
- **Where:** `desktop/aizome/crates/aizome-kit/src/app.rs:392` (`Maps.pictures`), `app.rs:396` (`Kept.shown` `:399`, `.changed` `:401`)
- **Encoding:** a `Kept` per window, its `shown` flag and the damage recorded since it showed
- **States:** `None` → `Kept(hidden)` → `Shown(first frame pending, collecting changes)` → `Consumed`
- **Transitions:** `keep_picture(Some)` → Kept, `keep_picture(None)` → None (`app.rs:648-657`); `show_window(true)` Kept→Shown, clears `changed` (`app.rs:635-641`); `show_window(false)` → None (`app.rs:643`); `invalidate`/`scroll` record into `changed` only while Shown (`app.rs:495-497`, `:698-700`); `invalidate_all` → None (`app.rs:505`); `close_window` → None (`app.rs:481`); `draw` consumes it and paints from it when it fits (`app.rs:840-846`)
- **Hazards:** damage while kept-but-hidden is not recorded (`app.rs:495`), so correctness relies on the engine dropping the picture on every rebuild (KIT-02); `draw` removes the entry even when it isn't shown (`app.rs:840`); when more than half the window changed (`ahead.rs:98-103`) the frame falls back to a full paint
- **Reify:** Low — small and local
- **Related:** engine side KIT-02; test model KIT-23

<a id="kit-04"></a>

### KIT-04 — Menu publication to the shell (dedup and debounce)
- **Where:** `desktop/aizome/crates/aizome-kit/src/app.rs:62` (`Runtime.menus_published`); `desktop/aizome/crates/aizome-kit/src/engine.rs:90` (`TimerUse::Menus`); `desktop/aizome/crates/aizome-kit/src/shell.rs:252` (`find`)
- **Encoding:** hash of the last menus queued for publication, and whether a `TimerUse::Menus` sits in `Core.timers` (debounce armed)
- **States:** `Current(hash)` → `Pending(new hash, 40 ms timer)` → `Sent`; the shell's copy can lag by up to 40 ms, or until its next `AboutToShow`
- **Transitions:** `refresh_menus` after every update and rebuild sets the hash and arms the timer if none is armed (`app.rs:255-268`, called at `app.rs:177-178`, `:188`); the timer → `publish_menus` (`app.rs:279-284`); a new link publishes at once and resets the hash (`app.rs:124-128`); `AboutToShow` refreshes and publishes (`shell.rs:288-291`); `Activate{id}` → `find` → `perform` (`shell.rs:278-287`)
- **Hazards:** the hash is recorded before the send (`app.rs:261`) and send failures only log (`shell.rs:84-86`), so a failed publish isn't retried until the menus change; `perform` (`engine.rs:2481-2511`) rechecks modal scope but not `enabled`, and `find` returns disabled actions
- **Reify:** High — cross-process staleness with a suspected bug
- **Suspected bug:** the shell's open menu shows Save enabled; the app disables it (republished 40 ms later); a click in the shell sends `Activate{id}`, `find` returns the now-disabled Save and `perform` queues its message anyway (confidence: low)
- **Related:** aizome-shell menu server `desktop/aizome/crates/aizome-shell/src/shell.rs:820` (AboutToShow), `:833` (Activate)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#kit-04)

<a id="kit-05"></a>

### KIT-05 — Sidestep handle maps and request ids
- **Where:** `desktop/aizome/crates/aizome-kit/src/app.rs:378` (`Maps.to_side`, `.to_kit`, `.timers`, `.tokens`, `.activations`, `.drags`); `app.rs:64` (`Runtime.window_states`)
- **Encoding:** id maps whose entries exist while a window, timer, activation request or native drag is live; `window_states` holds the last state told per window
- **States:** per handle `Live` → `Removed` (closed, one-shot fired, cancelled, answered or ended); repeating timers stay live until cancelled
- **Transitions:** `open_window`/`close_window` (`app.rs:475-477`, `:480-486`); `PopupDismissed` removes first (`app.rs:825-829`); `set_timer`/`cancel_timer` (`app.rs:593-607`); `Glue::timer` drops one-shots before dispatch (`app.rs:851-859`); `request_activation_token` → `activation_token` (`app.rs:730-735`, `:893`); `start_drag` → `drag_ended` (`app.rs:706-712`, `:910`); `window_state` dedup (`app.rs:305-315`)
- **Hazards:** each timer is tracked twice (`Core.timers` and `Maps.timers`/`.tokens`); engine paths that cancel through the platform without removing `Core.timers` leave orphans (KIT-17); `window_states` is never pruned; activation and drag entries leak if Sidestep never answers
- **Reify:** Low — plain bookkeeping

<a id="kit-06"></a>

### KIT-06 — Alert sheet lifecycle
- **Where:** `desktop/aizome/crates/aizome-kit/src/dialogs.rs:103` (`show_alert`); `desktop/aizome/crates/aizome-kit/src/engine.rs:274` (`AlertState`), `engine.rs:302` (`Core.alerts`), `engine.rs:155` (`Win.alerts`)
- **Encoding:** alert id present in both `Core.alerts` and its window's `Win.alerts`; sheet layer key `1000 + id`
- **States:** `Open` → `Chosen` (button message queued) | `Discarded` (window closed)
- **Transitions:** `show_alert`/`show_about` (`dialogs.rs:103-113`, `:116-129`); `install` adds a layer per alert (`engine.rs:706-710`); `KitMsg::AlertChoice` → `alert_chosen` (`engine.rs:2693`, `dialogs.rs:213-222`); `close_win` drops the window's alerts (`engine.rs:628-630`)
- **Hazards:** membership duplicated in two maps; alerts (`1000 + id`) and panels (`2000 + id`) draw ids from one counter (`engine.rs:342-345`, `:703`, `:708`), so an alert opened 1000 ids after a still-open panel gets that panel's layer key; closing the window skips the cancel button's message
- **Reify:** Low — two mutation sites

<a id="kit-07"></a>

### KIT-07 — File panel lifecycle and browsing state
- **Where:** `desktop/aizome/crates/aizome-kit/src/dialogs.rs:298` (`PanelState.dir`, `.entries`, `.selected`, `.name`, `.hidden`, `.error`, `.done`); `desktop/aizome/crates/aizome-kit/src/engine.rs:303` (`Core.panels`), `engine.rs:156` (`Win.panels`)
- **Encoding:** panel id in `Core.panels` and `Win.panels`; browsing state in `PanelState`; completion is the one-shot `done` callback
- **States:** `Open(dir, listed | error, selection, name)` → `Finished(Some(paths))` | `Cancelled(None)`; `chosen()` may navigate (Open→Open) instead of finishing
- **Transitions:** `show_panel` (`dialogs.rs:421-456`, pending focus `:455`); `Go`/`Up`/`Hidden` relist (`dialogs.rs:472-477`, `:497-500`); `Select` (`dialogs.rs:478-486`); `Activate`/`Confirm` → `chosen` (`dialogs.rs:487-495`, `:501`, navigation `:402-408`); `Cancel` (`dialogs.rs:502`); `finish` removes both and queues `done` (`dialogs.rs:458-465`)
- **Hazards:** `close_win` drops alerts but not panels (`engine.rs:628-630`); message indices index `entries` unchecked (`dialogs.rs:482`, `:403`, `:417`), so a stale index after a relist panics; `chosen()` mutates while deciding; listing runs synchronously on the UI thread (`dialogs.rs:340-379`)
- **Reify:** High — leak and lost completion
- **Suspected bug:** a program closes a window with `Cx::close_window` while its open or save panel is up: the `PanelState` stays in `Core.panels` for good and `done` never fires, so a program waiting for `Saved(None)` never leaves its saving state (confidence: high)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#kit-07)

<a id="kit-08"></a>

### KIT-08 — Editor undo history with typing bursts
- **Where:** `desktop/aizome/crates/aizome-kit/src/editor.rs:117` (`EditorContent.undo`, `.redo`, `.version`); `editor.rs:103` (`Step.kind`, `.at`, `.after`); `editor.rs:112` (`GROUP`)
- **Encoding:** undo and redo stacks; whether the top step is an open burst follows from its `kind`, `at` (last keystroke) and `after` (selection)
- **States:** `Clean` → `BurstOpen(Typing | Deleting, < 1.2 s, caret adjacent)` → `BurstClosed`; redo non-empty only after an undo with no edit since
- **Transitions:** `finish_step` merges or pushes, clears redo, caps at 500 (`editor.rs:368-389`); `undo` (`editor.rs:425-436`); `redo` (`editor.rs:438-449`); `set_text` resets both (`editor.rs:199-205`)
- **Hazards:** after an undo the step below is on top again, and typing within 1.2 s of its last keystroke merges into it; `version` bumps on undo and redo too
- **Reify:** Low — local and tested

<a id="kit-09"></a>

### KIT-09 — Window and sheet key registry
- **Where:** `desktop/aizome/crates/aizome-kit/src/engine.rs:288` (`Core.keys` of `KeyTarget`), `engine.rs:154` (`Win.sheets`), `engine.rs:306` (`Core.key_window`), `engine.rs:307` (`Core.quit`)
- **Encoding:** partially explicit: `KeyTarget::{Window, Sheet{parent}}` (`engine.rs:95`) plus the parent's ordered `sheets` and `Core.wins`
- **States:** key `Open(window)` | `Open(sheet of parent)` → `Closed`; `key_window` = last app window that had the keyboard
- **Transitions:** `open_app_window` (`engine.rs:349-398`; a popup over a closed parent returns `WindowKey::NONE`, `:370-373`); `open_sheet` (`engine.rs:427-436`); `close_key` window branch drops its sheets, closes, clears `key_window` (`engine.rs:440-456`); sheet branch (`engine.rs:457-462`); program popup dismissed → `close_key` + `CloseRequest` (`engine.rs:1122-1127`); `CloseRequest` asks the program (`app.rs:221-226`), ignored under a modal (`app.rs:245-247`); `close_lock_windows` (`engine.rs:635-647`); `window_focus` sets `key_window` (`engine.rs:2309-2311`)
- **Hazards:** windows of kind `Dialog{parent}` and program `Popup{parent}` aren't closed with their parent (only sheets are, `engine.rs:443-451`) despite the comment; sheet membership is duplicated in `keys` and `Win.sheets`; `Core.quit` is written (`app.rs:218`, `cx.rs:387`) but never read
- **Reify:** Medium — partially explicit, scattered cascade

<a id="kit-10"></a>

### KIT-10 — Window view, build and relayout lifecycle
- **Where:** `desktop/aizome/crates/aizome-kit/src/engine.rs:150` (`Win.dirty`), `engine.rs:165` (`.built`), `engine.rs:169` (`.relayout`), `engine.rs:184` (`.refresh`), `engine.rs:129` (`.snapshot`)
- **Encoding:** `dirty` (view again), `built` (laid out at the current size, scale and theme), `relayout` (node ids to lay out again without a view), `refresh` (hidden but viewable), `snapshot` (last build, for the diff)
- **States:** `Unbuilt` → `Clean` → `Dirty` → `Clean`; `Clean + RelayoutPending`; invalid but reachable: `!built` while the old tree still routes input
- **Transitions:** `Win::new` (`engine.rs:211`, `:223`); `resized`/`scale_changed` (`engine.rs:980-1003`); `mark_all_dirty` after every update (`engine.rs:659-665`, `app.rs:177`); `apply` rebuild (`engine.rs:1659-1673`); sheets, alerts and panels set `dirty` (`engine.rs:434`, `:460`; `dialogs.rs:111`, `:217`, `:453`, `:462`, `:509`); `build` → Clean (`engine.rs:751-754`); `relayout` skips dirty windows (`engine.rs:883-891`); popups (`engine.rs:964-968`, `:1043`); `retheme` (`engine.rs:2866-2871`); `Runtime::flush` drives both (`app.rs:180-192`)
- **Hazards:** hidden windows are viewed only with `refresh` (`engine.rs:656`), so a resize while hidden leaves them unbuilt (feeds KIT-02); `flush` gives up after 64 rounds (`app.rs:156`), leaving queued messages and dirty windows until the next event or wake
- **Reify:** Medium — four flags, many writers

<a id="kit-11"></a>

### KIT-11 — Window hide/show with held ticks, timers and focus
- **Where:** `desktop/aizome/crates/aizome-kit/src/engine.rs:172` (`Win.hidden`), `engine.rs:178` (`.held`), `engine.rs:179` (`.held_widgets`), `engine.rs:182` (`.refocus`), `engine.rs:167` (`.ticking`)
- **Encoding:** `hidden` plus queues of ticks and widget timers that came due or were cancelled while hidden, and the focus to give back
- **States:** `Shown` ⇄ `Hidden(held ticks, held widget timers, refocus)`; `Hidden + DrawnAhead` (KIT-02)
- **Transitions:** opened hidden (`engine.rs:394`); `show_key(false)` closes popups and tooltip, saves `refocus`, clears focus, moves widget timers to `held_widgets` (`engine.rs:477-510`); ticks and widget timers that come due while hidden are held (`engine.rs:2717-2720`, `:2730-2735`, `:1755-1758`); `show_key(true)` re-ticks, restarts widget timers whose type still matches, refocuses if nothing is focused (`engine.rs:512-530`); entry points `Cx::show_window`/`hide_window` (`cx.rs:114-126`)
- **Hazards:** a pending tooltip (`Win.tooltip`) isn't cancelled on hide (`engine.rs:477-485`) and `show_tooltip` doesn't check `hidden` (`engine.rs:2760-2772`), so without a pointer leave a tooltip popup opens over the hidden window; tick timers keep firing while hidden (only their delivery waits); `refocus` is dropped silently if the program focused something meanwhile; `relayout` ids pile up while hidden (`engine.rs:873`)
- **Reify:** Medium — six fields, three handlers

<a id="kit-12"></a>

### KIT-12 — Engine timer registry (ticks, widget timers, app timers)
- **Where:** `desktop/aizome/crates/aizome-kit/src/engine.rs:290` (`Core.timers`, `TimerUse` at `engine.rs:61`); `engine.rs:167` (`Win.ticking`); `desktop/aizome/crates/aizome-kit/src/tree.rs:88` (`Tree.ticks`, `.widget_timers`); `desktop/aizome/crates/aizome-kit/src/widget.rs:717` (`Out.ticks`, `.widget_timers`)
- **Encoding:** partially explicit: token → `TimerUse` names each timer's purpose, but dedup (`ticking`), hold queues (KIT-11) and type checks live elsewhere
- **States:** per timer `Armed` → `Fired` (removed unless a repeating app timer) | `Cancelled` | `Held` (window hidden) | `Stale` (window, node or widget type gone: ignored)
- **Transitions:** `Cx::after`/`every`/`cancel` (`cx.rs:225-242`); `tick_later` dedups by node (`engine.rs:1721-1728`); `widget_timer` replaces per (window, node, key) (`engine.rs:1730-1761`); `prune_widget_timers` on build, relayout, apply and close (`engine.rs:1763-1790`, `:595`); outboxes drained in `build`, `relayout`, `build_popup`, `apply` (`engine.rs:749-763`, `:914-931`, `:971-975`, `:1692-1698`); dispatch in `timer` (`engine.rs:2701-2758`)
- **Hazards:** tick, tooltip and ahead timers of a closed window stay in `Core.timers` until they fire (only widget timers are pruned on close, `engine.rs:595`); repeating app timers have no owner and run until cancelled; a tooltip timer cancelled on the platform is never removed (KIT-17)
- **Reify:** Medium — partially explicit, transitions in eight places

<a id="kit-13"></a>

### KIT-13 — Pointer hover, press and capture
- **Where:** `desktop/aizome/crates/aizome-kit/src/engine.rs:132` (`Win.hot`), `engine.rs:136` (`.hot_under`), `engine.rs:137` (`.active`), `engine.rs:142` (`.press_under`), `engine.rs:149` (`.pointer`), `engine.rs:151` (`.cursor`)
- **Encoding:** options naming the hovered node, the node under a drags-only node, the node that took the press (it gets the moves and the release), and a deferred click for a drags-only press
- **States:** `Outside` → `Hovering(hot[, hot_under])` → `Pressed(active)` → `Hovering`; `DragsOnlyPressed(active, press_under)` → release without a drag → synthesized click under it; `NativeDrag` (a widget drag clears `active`); invalid: `press_under` naming a node other than `active`
- **Transitions:** `pointer_move` routes to `active` when set (`engine.rs:2026-2053`); `update_hot` (`engine.rs:1932-2024`); `pointer_leave` clears hover, and `active`/`press_under` only for drags-only (`engine.rs:2055-2086`); a press resets `press_under` (`engine.rs:2125`), `press` sets `active` only if none (`engine.rs:2254-2256`), drags-only keeps `press_under` (`engine.rs:2135-2141`); a release takes both and may `click_under` (`engine.rs:2143-2175`, `:2181-2196`); capture requests (`engine.rs:1674-1678`); a widget drag clears `active` (`engine.rs:1633-1638`); a modal change filters `active` (`engine.rs:1898`)
- **Hazards:** `active` ends only by a release here, a capture release, a widget drag or a modal change; `Core::start_drag` (`engine.rs:2376-2386`, from `Cx::start_drag` `cx.rs:582-585`) leaves it set although the compositor takes the release; `press` keeps a stale `active` (`engine.rs:2254`), so the next click's release goes to the stale node
- **Reify:** High — six fields, nine handlers, a suspected bug
- **Suspected bug:** Hako calls `Cx::start_drag` while the button that began the drag is down (`desktop/aizome/apps/hako/src/ui/actions.rs:220`); the drop sends no release to the window, so `active` keeps naming the drag source: moves after re-entry go to it, and the next click's PointerUp reaches it instead of the clicked control, losing that click (confidence: medium)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#kit-13)

<a id="kit-14"></a>

### KIT-14 — Keyboard focus across modal layers, with pending focus and IME
- **Where:** `desktop/aizome/crates/aizome-kit/src/engine.rs:143` (`Win.focused`), `engine.rs:145` (`.modal_focus`), `engine.rs:146` (`.focus_modal`), `engine.rs:147` (`.focus_visible`), `engine.rs:148` (`.has_keyboard`), `engine.rs:161` (`.ime`), `engine.rs:298` (`Core.pending_focus`)
- **Encoding:** the focused node and whether it came from the keyboard, the modal layer it belongs to, saved focus per covered layer, focus requests by name waiting for a build, and a mirror of the platform's text-input state
- **States:** `Unfocused` → `Focused(node, visible)` in the top modal layer; `Suspended` (saved under the covered layer's id) → `Restored`; `Pending(name, window?)` → `Resolved`; IME `Off` ⇄ `On(caret)`
- **Transitions:** `set_focus` (`engine.rs:1848-1884`, IME off `:1869-1874`); Tab (`engine.rs:1905-1923`); click focus in `press` (`engine.rs:2212-2248`); `restore_modal_focus` on every build (`engine.rs:1888-1902`); focus lost with its node (`engine.rs:772-779`); `window_focus` (`engine.rs:2303-2317`); IME requests (`engine.rs:1685-1690`); `Cx::focus` (`cx.rs:346-363`) and panel focus (`dialogs.rs:455`) resolved in `install` (`engine.rs:715-720`); hide/show refocus (KIT-11)
- **Hazards:** pending names never expire: an entry whose element never appears waits until some later build grows an element of that name, which then takes the focus (`engine.rs:716-719`); `focus_name` counts the name of a killed node as found without focusing it (`engine.rs:1927-1929`; names aren't pruned, `tree.rs:84`); sheet layers are keyed by index (`engine.rs:697`), so a sheet replaced at the same index in one rebuild keeps `focus_modal` equal and skips save/restore
- **Reify:** High — seven fields, ten mutation sites

<a id="kit-15"></a>

### KIT-15 — Menu popup chain (menus, submenus, in-window bar menu)
- **Where:** `desktop/aizome/crates/aizome-kit/src/engine.rs:164` (`Win.popups`), `engine.rs:109` (`PopupKind::Menu{spec, look, bar}`), `engine.rs:153` (`Win.mapper`); the bar's `BarState.open` in the owner's `Win.states` (`engine.rs:1259`)
- **Encoding:** partially explicit: the popup kind enum, but the chain is owner links plus `Win.popups` order, one submenu per menu by convention, and a `BarState` while a bar title is open
- **States:** `None` → `Open(menu, grab)` → `Open + Submenu` (one at a time) → `Closed` (`look.closed` message queued); bar menus also mark the bar's title open
- **Transitions:** `show_menu` (`engine.rs:1191-1219`) with keyboard highlight (`engine.rs:1223-1236`); `open_bar_menu` (`engine.rs:1239-1262`); `menu_sub` (`engine.rs:1295-1328`); `menu_choose` (`engine.rs:1332-1384`); `close_menus` (`engine.rs:1142-1176`); `close_win` clears BarState and notifies (`engine.rs:613-627`); a press in the window (`engine.rs:2107-2122`); kit messages (`engine.rs:2666-2691`); filtering relayout (`engine.rs:1007-1044`); `retheme` (`engine.rs:2876-2883`); a modal change (`engine.rs:1901`); the shell coming or going (`app.rs:132-139`)
- **Hazards:** a chosen item is resolved by path against the spec settled when the menu opened, not the current state; BarState is dropped by type over the owner's whole state map; `closed` fires on every close path, theme changes included
- **Reify:** Medium — partially explicit, ten close paths

<a id="kit-16"></a>

### KIT-16 — Popover popup reconciliation
- **Where:** `desktop/aizome/crates/aizome-kit/src/engine.rs:116` (`PopupKind::Popover{node, on_dismiss, anchor}`); `desktop/aizome/crates/aizome-kit/src/widget.rs:410` (`PopoverContent.open`); `desktop/aizome/crates/aizome-kit/src/tree.rs:85` (`Tree.popovers`)
- **Encoding:** the program's `open` flag in the view, the existing popup keyed by the anchor's `NodeId`, and its last anchor rectangle
- **States:** per anchor `Closed` → `Open(popup, anchor)` → `Dismissed` (popup gone, `on_dismiss` queued) → `Closed` once the program clears `open`
- **Transitions:** `sync_popovers` opens, rebuilds or closes after each install (`engine.rs:1478-1528`); `follow_anchors` on relayout (`engine.rs:1048-1068`, `:935`); `popup_dismissed` (`engine.rs:1128-1138`); Escape dismisses the last popup (`engine.rs:2399-2402`); a rebuild inside a popover marks its owner dirty (`engine.rs:1663-1669`)
- **Hazards:** with `open` true and no `on_dismiss` (as in `tests/popups.rs:30`), a dismissed popover reopens at the next rebuild; a re-keyed anchor closes and reopens it, dropping its widgets' state
- **Reify:** Medium — split between program and engine

<a id="kit-17"></a>

### KIT-17 — Tooltip delay, show and dismissal
- **Where:** `desktop/aizome/crates/aizome-kit/src/engine.rs:157` (`Win.tooltip`: pending node and timer token), `engine.rs:158` (`.tooltip_shown`), `engine.rs:160` (`.tooltip_target`); `engine.rs:85` (`TimerUse::Tooltip`)
- **Encoding:** pending timer, shown popup, and the nearest tooltip source under the pointer (kept after a click so it isn't re-armed)
- **States:** `None` → `Pending(node, 650 ms)` → `Shown(popup)` → `ClickDismissed` (target kept) → `None` when the source changes or the pointer leaves
- **Transitions:** `update_hot` arms, keeps, cancels or refreshes (`engine.rs:1991-2018`); the timer → `show_tooltip` (`engine.rs:2744-2748`, `:2760-2772`); `pointer_leave` (`engine.rs:2073-2078`); a press closes the shown one (`engine.rs:2103-2105`); hide (`engine.rs:478-479`); `close_win` clears `tooltip_shown` (`engine.rs:609-611`); `retheme` (`engine.rs:2879`)
- **Hazards:** `pointer_leave` cancels the platform timer but never removes its `TimerUse::Tooltip` (`engine.rs:2073-2075`; contrast `:2001-2004`); a press closes only a shown tooltip, and a pending one keeps its deadline because `update_hot` returns early (`engine.rs:1960-1962`); hiding doesn't cancel the pending one (KIT-11)
- **Reify:** High — three fields, seven handlers, two suspected bugs
- **Suspected bug:** hover a button and click it within 650 ms: its tooltip still appears after the click, though a click dismisses tooltips (`tests/tooltip_refresh.rs:137-142` covers only a shown one) (confidence: medium); every pointer leave with a tooltip pending leaks one `Core.timers` entry (confidence: high)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#kit-17)

<a id="kit-18"></a>

### KIT-18 — Video placement and opaque-region mirror
- **Where:** `desktop/aizome/crates/aizome-kit/src/engine.rs:187` (`Win.videos`), `engine.rs:188` (`Win.opaque`); `widgets::video::Placed.below` in `Win.states` (`engine.rs:827-831`)
- **Encoding:** the last place sent per video, whether each is drawn clear because something covers it, and the last opaque region sent
- **States:** per video `Unplaced` → `Placed(rect, below)` → `Removed`; region `Unsent` → `Sent(rects)`
- **Transitions:** `sync_videos` diffs and sends after build and relayout (`engine.rs:798-858`, `:790`, `:936`)
- **Hazards:** diffed only against what was last sent; the covered state is recomputed from `over_video()` nodes each layout (`engine.rs:802-809`); nothing is sent on window close (platform teardown removes it)
- **Reify:** Low — one function owns it
- **Related:** Sidestep's video planes (`sidestep_ui::video::VideoPlace`, in `desktop/aizome/sukumo`)

<a id="kit-19"></a>

### KIT-19 — Session lock windows
- **Where:** `desktop/aizome/crates/aizome-kit/src/engine.rs:635` (`close_lock_windows` over `WindowKind::Lock` windows in `Core.wins`); `desktop/aizome/crates/aizome-kit/src/cx.rs:542` (`lock_session`, `unlock_session`); `desktop/aizome/crates/aizome-kit/src/app.rs:332` (hooks)
- **Encoding:** the lock state lives in Sidestep (`is_session_locked`, `cx.rs:552-554`); the kit holds only the lock windows and forwards the compositor's answers
- **States:** `Unlocked` → `Requested(lock windows open)` → `Locked` → `Unlocked` | `Finished` (refused or ended)
- **Transitions:** `Cx::lock_session` (`cx.rs:542-544`); lock windows open as `OpenKind::Lock` (`engine.rs:364`); the locked hook (`app.rs:332-336`, `:877-880`); `Cx::unlock_session` closes the lock windows (`cx.rs:547-550`); finished closes them, then tells the program (`app.rs:338-343`, `:882-885`)
- **Hazards:** the program's keys of closed lock windows go stale silently; lock windows can be opened without a lock request
- **Reify:** Low — thin pass-through
- **Related:** aizome-shell lock screen `desktop/aizome/crates/aizome-shell/src/lock.rs:322`, `:395`; Sidestep's ext-session-lock backend

<a id="kit-20"></a>

### KIT-20 — Inspection files per window
- **Where:** `desktop/aizome/crates/aizome-kit/src/inspect.rs:34` (`file`), `inspect.rs:116` (`painted`), `inspect.rs:136` (`closed`)
- **Encoding:** `<dir>/<app_id>.<pid>.<window>.json` exists while the window is open and has painted; written through a `.json.tmp` and a rename
- **States:** `Absent` → `Written` (rewritten on every paint and pointer move) → `Removed`
- **Transitions:** `painted` from `paint` and `pointer_move` (`engine.rs:2811-2813`, `:2030-2032`; `inspect.rs:125-129`); `closed` from `close_win` (`engine.rs:596-598`; `inspect.rs:136-140`)
- **Hazards:** files stay behind when the process exits without closing its windows (quit, crash), so readers must filter by pid
- **Reify:** Low — external test aid
- **Related:** VM tests that read `AIZOME_KIT_INSPECT` output (e.g. `nix/aizome-greeter-desktops-vm-test.nix`)

<a id="kit-21"></a>

### KIT-21 — Shell connection: link, watcher and menu placement
- **Where:** `desktop/aizome/crates/aizome-kit/src/shell.rs:28` (`ShellLink.alive` `:32`), `shell.rs:108` (`ShellWatch.found`, `.stop`); `desktop/aizome/crates/aizome-kit/src/app.rs:53` (`Runtime.shell`, `.shell_place`, `.shell_watch`); `desktop/aizome/crates/aizome-kit/src/engine.rs:294` (`Core.in_window_menus`)
- **Encoding:** an optional link whose reader thread clears `alive` on hangup, an optional watcher thread that polls the socket and parks a found link in a mutex slot, and `in_window_menus` putting the bars in the windows
- **States:** `OtherDesktop` (no socket path) | `IsShell` (nothing published) | `Watching` (in-window menus) → `Connected` → `Dead` → `Reconnected` | `Watching`
- **Transitions:** environment default (`engine.rs:327`); `launched` picks a mode (`app.rs:763-771`); `find_shell` (`app.rs:87-94`); `try_connect` spawns the reader, `alive=false` on hangup (`shell.rs:54-72`); the watcher loop (`shell.rs:117-142`), stopped on drop (`shell.rs:150-154`); `check_shell` on every wake: dead → reconnect, or watch and bars back (`app.rs:98-112`); found → link, bars out, publish (`app.rs:114-128`); `menus_go_in_windows` (`app.rs:132-139`); harness constructors (`testing.rs:475`, `:488`, `:502`)
- **Hazards:** `in_window_menus` is written in seven places and must agree with `shell.is_some()`; connect and reconnect handshake on the UI thread; window sizes include the bar only as computed at open (`engine.rs:376-384`), so a shell coming or going doesn't resize windows or their limits; the watcher sees `stop` only once per period and may connect after being dropped (the link is then dropped unused)
- **Reify:** High — two threads, seven writers of one flag, user-visible menus
- **Related:** aizome-shell menu server `desktop/aizome/crates/aizome-shell/src/shell.rs:820`; protocol client `desktop/aizome/crates/aizome-shell-proto/src/lib.rs` (`Client::hello`)

<a id="kit-22"></a>

### KIT-22 — (test harness) TestPlatform virtual clock and timer queue
- **Where:** `desktop/aizome/crates/aizome-kit/src/testing.rs:54` (`TestPlatform.timers`), `testing.rs:55` (`.next_token`), `testing.rs:57` (`.clock`)
- **Encoding:** token → (due, repeat period) on a clock the test moves
- **States:** per timer `Armed(due)` → `Fired` (removed, or re-armed at due + period) | `Cancelled`
- **Transitions:** `set_timer` (`testing.rs:286-291`); `cancel_timer` (`testing.rs:293-295`); `advance` fires due timers in (due, token) order, re-arming repeats before dispatch (`testing.rs:643-659`, `:165-167`); `deliver_timer` injects a stale token (`testing.rs:638-640`)
- **Hazards:** zero-delay timers set during dispatch fire within the same `advance`; repeat periods are clamped to 1 ms
- **Reify:** Low — test-only

<a id="kit-23"></a>

### KIT-23 — (test harness) TestPlatform visibility, picture and damage model
- **Where:** `desktop/aizome/crates/aizome-kit/src/testing.rs:93` (`TestPlatform.hidden`), `testing.rs:95` (`.pictures`), `testing.rs:98` (`.first`), `testing.rs:101` (`.ahead`), `testing.rs:53` (`.damage`), `testing.rs:81` (`.scrolls`)
- **Encoding:** the hidden set, a picture kept per hidden window, the picture taken for the first frame after showing, and damage and moves since the test last took them
- **States:** `Hidden(no picture | picture)` → `Shown(first pending | drawing damage)`; damage `Accumulating` → `Taken`
- **Transitions:** opened hidden (`testing.rs:186-191`); `show_window` (`testing.rs:401-422`); `keep_picture` (`testing.rs:424-434`); damage dropped while hidden (`testing.rs:209-216`); `scroll` moves pending damage (`testing.rs:324-361`); `close_window` (`testing.rs:193-201`); `render_frame` consumes `first` (`testing.rs:1293-1316`); `check_frame` takes `ahead` for hidden windows (`testing.rs:1193-1197`)
- **Hazards:** a second model of Sidestep's kept picture (KIT-03) whose rules can drift: here the picture moves to `first` at show, there `shown` is set and changes are recorded
- **Reify:** Low — test double
- **Related:** `desktop/aizome/crates/aizome-kit/src/app.rs:392` (KIT-03)

<a id="kit-24"></a>

### KIT-24 — (test harness) Fake session lock
- **Where:** `desktop/aizome/crates/aizome-kit/src/testing.rs:74` (`TestPlatform.locked`, `.lock_asked`); `desktop/aizome/crates/aizome-kit/tests/shell_api.rs:13` (fixture `Shell.locks`, `.locked`, `.finished`)
- **Encoding:** two booleans in the platform, three fields in the fixture program
- **States:** `Unlocked` → `Asked` → `Locked` → `Unlocked` | `Finished`
- **Transitions:** `lock_session`/`unlock_session` (`testing.rs:383-390`); `confirm_lock` only after an ask (`testing.rs:1046-1051`); `finish_lock` (`testing.rs:1054-1058`); fixture lock, unlock and hooks (`tests/shell_api.rs:60-69`, `:95-101`)
- **Hazards:** `confirm_lock` is silently a no-op unless asked; the fixture clears `locks` by hand to mirror the kit closing lock windows
- **Reify:** Low — test double
- **Related:** KIT-19

<a id="kit-25"></a>

### KIT-25 — (test harness) Retained frame oracles
- **Where:** `desktop/aizome/crates/aizome-kit/src/testing.rs:88` (`TestPlatform.frames`), `testing.rs:90` (`.kept`), `testing.rs:91` (`.exact_frames`)
- **Encoding:** the last composed frame per window, kept pixel buffers, and exact-oracle buffers with their size and scale
- **States:** `NoBaseline` → `Baseline(frame)` → compared and replaced on each check
- **Transitions:** `check_frame` records on its first call and composes after (`testing.rs:1186-1254`, baseline `:1220-1224`); `check_frame_exact` starts over on a size or scale change (`testing.rs:1107-1179`, `:1115-1122`); `render_damage`/`render_frame` (`testing.rs:1273-1316`)
- **Hazards:** the first check only records (it always passes); a size change silently discards the baseline
- **Reify:** Low — test-only

<a id="kit-26"></a>

### KIT-26 — (test harness) Wall-clock polling loops
- **Where:** `desktop/aizome/crates/aizome-kit/src/testing.rs:531` (`wait_for_shell`), `testing.rs:548` (`pump_shell`), `testing.rs:665` (`settle`)
- **Encoding:** a deadline `Instant` and a sleep per round
- **States:** `Polling` → `Done` | `TimedOut`
- **Transitions:** loops at `testing.rs:533-544`, `:550-564`, `:667-676`
- **Hazards:** wall-clock deadlines and sleeps (flaky under load); `wait_for_shell` calls `check_shell` directly, bypassing the wake path
- **Reify:** Low — test-only

<a id="kit-27"></a>

### KIT-27 — (test harness) Fake clipboard and primary selection
- **Where:** `desktop/aizome/crates/aizome-kit/src/testing.rs:58` (`TestPlatform.clipboard`, `.clipboard_data`), `testing.rs:61` (`.primary`, `.primary_data`)
- **Encoding:** text plus prepared multi-type data, valid only while the text still matches
- **States:** `Empty` → `Text` | `Prepared(data)`; prepared degrades to text only once a test overwrites the text
- **Transitions:** `set_clipboard_text` clears the data (`testing.rs:259-262`); prepared setters (`testing.rs:272-275`, `:281-284`); reads through `snapshot` (`testing.rs:1467-1472`)
- **Hazards:** none observed
- **Reify:** Low — test-only

<a id="kit-28"></a>

### KIT-28 — Appearance following and retheming
- **Where:** `desktop/aizome/crates/aizome-kit/src/theme.rs:341` (`Theme.generation`), `theme.rs:342` (`.fonts`), `theme.rs:58` (`ThemeChoice::Auto`); `desktop/aizome/crates/aizome-kit/src/app.rs:59` (`Runtime.settings_changed`), `app.rs:61` (`.watcher`); `desktop/aizome/crates/aizome-kit/src/engine.rs:282` (`Core.theme`, `.appearance`); `desktop/aizome/crates/aizome-kit/src/text.rs:203` (`TextCache`)
- **Encoding:** a settings-changed flag set by the watcher thread, the current appearance and theme with a generation, Auto's palette resolved once per retheme, and font and text-layout caches
- **States:** `Current(appearance, palette, generation)` → `SettingsChanged` (flag) → `Rethemed(generation + 1)`; Auto `Day` ⇄ `Night`, re-resolved only on a retheme
- **Transitions:** the watcher sets the flag and wakes (`app.rs:759-762`); `woken` takes it and reloads (`app.rs:291-294`); `appearance_changed` → `recheck_auto` (`app.rs:866-870`, `:367-373`); `Cx::set_appearance` (`cx.rs:377-379`); `rethemed` bumps the generation and keeps the fonts (`theme.rs:388-393`); `retheme` compares, clears the text cache, marks windows, closes menus and tooltips (`engine.rs:2854-2884`)
- **Hazards:** the kit keeps no clock for Auto (the "each hour" at `app.rs:365` has no timer): it flips only when Sidestep reports a desktop look change, which in Aizome comes from the shell's dconf relay, while the compositor re-resolves Auto every 60 s (`desktop/aizome/crates/aizome-comp-core/src/night.rs:100-110`); without that relay an Auto app keeps its launch-time palette; text-layout keys omit the theme and rely on `text.clear()` (`engine.rs:2863`, `text.rs:224-229`)
- **Reify:** Medium — four triggers and a cross-process dependency
- **Related:** aizome-shell `follow_gtk` (`desktop/aizome/crates/aizome-shell/src/shell.rs:1967`); settings watcher `desktop/aizome/crates/aizome-config/src/lib.rs:813`

<a id="kit-29"></a>

### KIT-29 — Node arena: liveness, layout cache and per-layout outboxes
- **Where:** `desktop/aizome/crates/aizome-kit/src/tree.rs:60` (`Node.dead`), `tree.rs:62` (`Node.cache`), `tree.rs:81` (`Tree.by_id`, `.names`, `.popovers`, `.ticks`, `.widget_timers`, `.local_damage`, `.scrolls`)
- **Encoding:** tombstoned arena nodes addressed by index, the `by_id` index map, cached constraints, and index-based outboxes filled during layout and drained by the engine
- **States:** node `Mounted` → `LaidOut(cached)` → `Uncached` → `Dead` → `Compacted away`; outboxes `Empty` → `Filled (layout)` → `Drained (settle, build)`; invalid but reachable: an outbox index into a compacted arena
- **Transitions:** `mount_under` (`tree.rs:145-186`); the layout cache (`tree.rs:249-268`); `uncache`/`uncache_up` (`tree.rs:189-206`, `widget.rs:505-514`); `add_child` (`widget.rs:496-501`); `kill` through `clear_children` (`tree.rs:237-246`, `widget.rs:540-545`); outbox pushes (`widget.rs:530-532`, `:570-591`); `compact` when dead > 256 and > half (`engine.rs:908-911`, `tree.rs:209-234`); drains (`engine.rs:2891-2909`, `tree.rs:285-311`, `:683-693`)
- **Hazards:** `compact` remaps parents, children, `by_id`, `root` and `popovers` but not `scrolls[].node` or `local_damage[].0`, and `Core::relayout` calls it between `layout_window` (which fills them) and `settle` (which reads them) (`engine.rs:907-912`); `names` keeps killed nodes' names (KIT-14)
- **Reify:** High — index validity is implicit, and a suspected bug
- **Suspected bug:** in a relayout that compacts, a widget mounted during layout (an editor, scroll view or list inside a virtual list's row, after earlier rows died) records `invalidate_rect` or `scrolled` under its old index: its damage or pixel move lands on another node, and `take_local_damage` indexes `self.nodes[*i]` past the end once the old index is at least the live count (`tree.rs:686`), which panics (confidence: medium)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#kit-29)

<a id="kit-30"></a>

### KIT-30 — Desktop opener worker (bounded xdg-open)
- **Where:** `desktop/aizome/crates/aizome-kit/src/urls.rs:76` (`Launcher.pending`, `.stopping`), `urls.rs:70` (`Active.failure`), `urls.rs:46` (`Permit`); worker locals `children` and `disconnected` (`urls.rs:108-109`); `desktop/aizome/crates/aizome-kit/src/app.rs:379` (`Maps.urls`, made on first use)
- **Encoding:** an atomic count of admitted requests (permits), a bounded channel, a worker-local list of live children with a poll-failure flag, and a stop flag
- **States:** request `Rejected` | `Admitted` → `Queued` → `Running(child)` → `Reaped(Ok | Exit)` | `SpawnFailed` | `PollFailed(killed, still polled)` | `Cancelled(Unavailable)`; worker `Idle` → `Polling` → `Saturated` → `Draining` → `Exited`
- **Transitions:** `open` admits and queues (`urls.rs:179-191`); the reap loop (`urls.rs:111-141`); stop drain (`urls.rs:142-147`); exit (`urls.rs:148-150`); saturated or draining sleeps (`urls.rs:151-154`); receive (`urls.rs:155-159`); spawn (`urls.rs:166-169`); `finish` releases the permit before the completion (`urls.rs:60-67`); `Drop` sets stop (`urls.rs:82-88`); lazy creation (`app.rs:424-425`)
- **Hazards:** a child whose `try_wait` keeps failing is killed but kept forever, holding a permit; a failed `Launcher::new` is retried on every `open_url`; completions run on the worker thread
- **Reify:** Medium — process lifecycle across atomics and loop locals (well tested)

<a id="kit-31"></a>

### KIT-31 — Per-node widget state across rebuilds
- **Where:** `desktop/aizome/crates/aizome-kit/src/widget.rs:417` (`StateMap`), `widget.rs:450` (`state_of`), `widget.rs:952` (`Vacant`); `desktop/aizome/crates/aizome-kit/src/engine.rs:128` (`Win.states`)
- **Encoding:** boxed state per (NodeId, TypeId), alive while the node id survives each build; the widget is swapped for `Vacant` while it runs
- **States:** `Absent` → `Created` (first `state()`) → `Retained` → `Dropped` (node gone after a build); widget `InTree` ⇄ `TakenOut(Vacant)`
- **Transitions:** creation (`widget.rs:450-453`, `:548-551`, `:803-806`); pruning in `build`, `relayout`, `build_popup` (`engine.rs:755-756`, `:916-917`, `:969-970`); BarState removed by type (`engine.rs:614`, `:1171`); take-out and put-back (`engine.rs:1567`, `:1595`, `:1814`, `:1841`, `:2447`, `:2475`, `:2579`, `:2607`; `tree.rs:256`, `:262`)
- **Hazards:** `relayout_popup` doesn't prune (`engine.rs:1007-1044`); duplicate keys get index-derived ids (`tree.rs:153-156`), so state moves between such siblings when they reorder
- **Reify:** Low — one map, central pruning

<a id="kit-32"></a>

### KIT-32 — (test harness) Menu-bar fixture's keyboard title walk
- **Where:** `desktop/aizome/crates/aizome-kit/tests/menu_hooks.rs:15` (`Bar.title`)
- **Encoding:** `Option<i32>` index of the bar title whose menu is open, moved by `Keyboard` and `Side(±1)` messages
- **States:** `None` → `Title(0)` → `Title((t + d) mod 3)`
- **Transitions:** `Keyboard`/`Side` set the title and reopen the menu under it (`tests/menu_hooks.rs:79-90`)
- **Hazards:** never reset to `None` when the menu closes (closed messages don't touch it)
- **Reify:** Low — fixture

<a id="kit-33"></a>

### KIT-33 — (test harness) Fake shell socket server
- **Where:** `desktop/aizome/crates/aizome-kit/tests/shell_link.rs:83` (listener), `tests/shell_link.rs:163` (late-shell listener); `desktop/aizome/crates/aizome-kit/tests/modal_commands.rs:200`
- **Encoding:** a `UnixListener` bound at a per-pid path and the accepted stream's line reader
- **States:** `NoSocket` → `Listening` → `Accepted` → `HelloRead` → `MenusRead` ⇄ `ActivateWritten` → `Dropped` → `Rebound`
- **Transitions:** bind, accept, read hello and menus, write Activate (`tests/shell_link.rs:83-115`); the app watches before the bind (`tests/shell_link.rs:157-168`); drop and remove the socket (`tests/shell_link.rs:179-182`); bind again (`tests/shell_link.rs:190-195`); the modal case (`tests/modal_commands.rs:200-232`)
- **Hazards:** a blocking `accept` with no timeout; read timeouts of 3–5 s; `read_menus` skips other messages until Menus (`tests/modal_commands.rs:183-190`)
- **Reify:** Low — test-only
- **Related:** KIT-21, KIT-04

## Already explicit

- None in this slice.

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/crates/aizome-kit/build.rs` | 76 | none |  | build script: icon codegen |
| `desktop/aizome/crates/aizome-kit/examples/gallery/main.rs` | 12 | none |  | entry point |
| `desktop/aizome/crates/aizome-kit/examples/gallery/views.rs` | 651 | machines | [KIT-01](#kit-01) | GALLERY_DEMO timer loop |
| `desktop/aizome/crates/aizome-kit/src/a11y.rs` | 72 | none |  | pure semantic-tree walk |
| `desktop/aizome/crates/aizome-kit/src/ahead.rs` | 132 | machines | [KIT-02](#kit-02) | draw-ahead Drawing; band math |
| `desktop/aizome/crates/aizome-kit/src/app.rs` | 945 | machines | [KIT-03](#kit-03), [KIT-04](#kit-04), [KIT-05](#kit-05), [KIT-10](#kit-10), [KIT-19](#kit-19), [KIT-21](#kit-21), [KIT-28](#kit-28), [KIT-30](#kit-30) | runtime glue: pictures, publication, handle maps |
| `desktop/aizome/crates/aizome-kit/src/cx.rs` | 586 | machines | [KIT-11](#kit-11), [KIT-12](#kit-12), [KIT-13](#kit-13), [KIT-14](#kit-14), [KIT-19](#kit-19), [KIT-28](#kit-28) | program API entry points; Pool is lazy init |
| `desktop/aizome/crates/aizome-kit/src/dialogs.rs` | 639 | machines | [KIT-06](#kit-06), [KIT-07](#kit-07), [KIT-10](#kit-10), [KIT-14](#kit-14) | alerts and file panels |
| `desktop/aizome/crates/aizome-kit/src/draw.rs` | 172 | none |  | drawing helpers; grain tile OnceLock |
| `desktop/aizome/crates/aizome-kit/src/editing.rs` | 100 | none |  | pure text-boundary functions |
| `desktop/aizome/crates/aizome-kit/src/editor.rs` | 577 | machines | [KIT-08](#kit-08) | undo history and bursts |
| `desktop/aizome/crates/aizome-kit/src/element.rs` | 240 | none |  | element builders and message closures |
| `desktop/aizome/crates/aizome-kit/src/engine.rs` | 2938 | machines | [KIT-09](#kit-09), [KIT-10](#kit-10), [KIT-11](#kit-11), [KIT-12](#kit-12), [KIT-13](#kit-13), [KIT-14](#kit-14), [KIT-15](#kit-15), [KIT-16](#kit-16), [KIT-17](#kit-17), [KIT-18](#kit-18), [KIT-19](#kit-19), [KIT-02](#kit-02), [KIT-04](#kit-04), [KIT-06](#kit-06), [KIT-07](#kit-07), [KIT-21](#kit-21), [KIT-28](#kit-28), [KIT-29](#kit-29), [KIT-31](#kit-31) | windows, input, popups, timers |
| `desktop/aizome/crates/aizome-kit/src/fonts.rs` | 108 | none |  | one-time font install (Once) |
| `desktop/aizome/crates/aizome-kit/src/hash.rs` | 114 | none |  | hasher |
| `desktop/aizome/crates/aizome-kit/src/icons.rs` | 276 | none |  | thread-local compute-once caches |
| `desktop/aizome/crates/aizome-kit/src/inspect.rs` | 140 | machines | [KIT-20](#kit-20) | per-window JSON files |
| `desktop/aizome/crates/aizome-kit/src/layout.rs` | 286 | none |  | layout math |
| `desktop/aizome/crates/aizome-kit/src/lib.rs` | 116 | none |  | module list and re-exports |
| `desktop/aizome/crates/aizome-kit/src/menu.rs` | 752 | none |  | menu data model and shortcut matching |
| `desktop/aizome/crates/aizome-kit/src/platform.rs` | 172 | none |  | platform trait and open specs |
| `desktop/aizome/crates/aizome-kit/src/program.rs` | 363 | none |  | program trait and specs; hooks used by KIT-19 |
| `desktop/aizome/crates/aizome-kit/src/shell.rs` | 395 | machines | [KIT-21](#kit-21), [KIT-04](#kit-04) | shell link, watcher, menu ids |
| `desktop/aizome/crates/aizome-kit/src/stats.rs` | 100 | none |  | bounded sample buffers |
| `desktop/aizome/crates/aizome-kit/src/testing.rs` | 1472 | machines | [KIT-22](#kit-22), [KIT-23](#kit-23), [KIT-24](#kit-24), [KIT-25](#kit-25), [KIT-26](#kit-26), [KIT-27](#kit-27), [KIT-21](#kit-21) | headless harness and test platform |
| `desktop/aizome/crates/aizome-kit/src/text.rs` | 826 | machines | [KIT-28](#kit-28) | LRU layout cache valid per theme |
| `desktop/aizome/crates/aizome-kit/src/theme.rs` | 610 | machines | [KIT-28](#kit-28) | appearance, Auto, generation |
| `desktop/aizome/crates/aizome-kit/src/tree.rs` | 694 | machines | [KIT-29](#kit-29), [KIT-12](#kit-12), [KIT-16](#kit-16), [KIT-31](#kit-31) | node arena, outboxes, diff |
| `desktop/aizome/crates/aizome-kit/src/urls.rs` | 362 | machines | [KIT-30](#kit-30) | opener worker |
| `desktop/aizome/crates/aizome-kit/src/widget.rs` | 958 | machines | [KIT-31](#kit-31), [KIT-29](#kit-29), [KIT-12](#kit-12), [KIT-16](#kit-16) | widget trait, state map, outboxes |
| `desktop/aizome/crates/aizome-kit/tests/alert_layout.rs` | 133 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/canvas.rs` | 92 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/collections.rs` | 218 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/core.rs` | 359 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/dialogs.rs` | 303 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/drag_out.rs` | 179 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/editor.rs` | 320 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/field_blur.rs` | 56 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/goldens.rs` | 117 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/hidden.rs` | 117 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/inspect.rs` | 124 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/lists.rs` | 242 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/menu_hooks.rs` | 273 | machines | [KIT-32](#kit-32) | bar fixture title walk |
| `desktop/aizome/crates/aizome-kit/tests/modal_commands.rs` | 235 | machines | [KIT-33](#kit-33) | fake shell socket |
| `desktop/aizome/crates/aizome-kit/tests/nested_modals.rs` | 165 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/panes.rs` | 263 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/perf.rs` | 314 | none |  | benchmark driver; fixed Mode config |
| `desktop/aizome/crates/aizome-kit/tests/popups.rs` | 45 | none |  | tests (popover open without on_dismiss, see KIT-16) |
| `desktop/aizome/crates/aizome-kit/tests/program_popups.rs` | 70 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/scroll_growth.rs` | 48 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/scrolling.rs` | 242 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/shell_api.rs` | 251 | machines | [KIT-24](#kit-24) | lock fixture |
| `desktop/aizome/crates/aizome-kit/tests/shell_link.rs` | 202 | machines | [KIT-33](#kit-33) | fake shell socket lifecycle |
| `desktop/aizome/crates/aizome-kit/tests/smoke.rs` | 74 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/text_drag.rs` | 139 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/text_field.rs` | 219 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/tooltip_refresh.rs` | 203 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/video_modals.rs` | 128 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/widget_timers.rs` | 187 | none |  | tests |
| `desktop/aizome/crates/aizome-kit/tests/window_state.rs` | 70 | none |  | tests |
