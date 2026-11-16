# UIW: Sukumo UI and toolkit widgets

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/sukumo/crates/sidestep-ui, desktop/aizome/crates/aizome-kit/src/widgets`. Audited at `8e0acbe5`; line numbers refer to that commit.

34 files audited. 34 implicit machines: 8 High, 11 Medium, 15 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

<a id="uiw-01"></a>

### UIW-01 — Press-then-release click pairing in buttons, switches, checkboxes, radios and segmented controls
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/controls.rs:421` (`ButtonWidget::event`); also `controls.rs:593` (`ToggleWidget::event`), `controls.rs:856` (`Check::event`), `controls.rs:1575` (`SegWidget::event`); the press itself is the kit engine's `active` (`desktop/aizome/crates/aizome-kit/src/engine.rs:2255`, `:2144`)
- **Encoding:** no widget state; "pressed" is the engine's `w.active` (set to whatever handled the press, taken on release) and `hot`; widgets read it only to paint (`cx.is_pressed()`, `controls.rs:281`) and act on any left `PointerUp` inside their bounds
- **States:** `Idle` → `Pressed` (engine `active == self`) → `ReleasedInside` (fires) | `ReleasedOutside` (nothing); representable: a `PointerUp` delivered with no press (`active == None`)
- **Transitions:** handled PointerDown → engine sets `active` (`engine.rs:2255`); release → engine takes `active` and bubbles to it, or to whatever is under the pointer when none (`engine.rs:2144`, `:2167`); Button emits `on_press` if inside (`controls.rs:429-432`); Toggle flips (`controls.rs:598`); Check fires (`controls.rs:861`); Segmented chooses (`controls.rs:1591`)
- **Hazards:** release handlers never check that the press was theirs; a pressed node rebuilt away drops `active` (`engine.rs:1898`), so its release lands on whatever replaced it
- **Reify:** High — the click machine is split between the engine and four widgets, and misfires are user-visible
- **Suspected bug:** press on inert background (nothing handles it, `active` stays `None`), move onto a button and let go: the engine bubbles the `PointerUp` to the button, which emits `on_press` though it was never pressed; the same for switches, checkboxes, radios and segments (confidence: high)
- **Related:** kit engine pointer routing `desktop/aizome/crates/aizome-kit/src/engine.rs:2088`
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#uiw-01)

<a id="uiw-08"></a>

### UIW-08 — Menu interaction: hover, open submenu, pointer aim, type-to-filter, strip tear-off
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/menus.rs:268` (`MenuState.hover`, `.open_sub`, `.query`, `.last`, `.aiming`, `.strip_press`); rows cached in `MenuView.rows` (`menus.rs:285`)
- **Encoding:** six fields in node state; the submenu itself is an engine popup opened and closed through `KitMsg::MenuSub`, `MenuCloseSubs`, `MenuBack`
- **States:** `Browsing(hover)` | `SubmenuOpen(path)` | `Aiming(row, tick pending)` | `Filtering(query)` | `StripPressed(at)`; representable: `open_sub` set while the engine has no submenu, `hover ≥ rows.len()`
- **Transitions:** `set_hover` (`menus.rs:458`) opens a submenu (`menus.rs:470-471` → `menus.rs:485`, `MenuSub` `menus.rs:488`) or closes it (`menus.rs:474-477`); moving toward an open submenu sets `aiming` and a 220 ms tick (`menus.rs:847-849`), Tick follows the pointer (`menus.rs:858-859`), leave clears (`menus.rs:865-869`); typing, Backspace, Escape → `set_query` resets `hover`/`open_sub` (`menus.rs:532-540`, `:946-980`); strip press (`menus.rs:886`), drag over 4 pt tears off (`menus.rs:828-835`), release clears (`menus.rs:895`); choosing sends `MenuChoose` (`menus.rs:500`)
- **Hazards:** `hover` indexes `rows`, rebuilt every layout (`menus.rs:598`) and never clamped; ticks coalesce per node (`engine.rs:1723`), so the aim window runs from the first aiming move, not the last; nothing clears `open_sub` when the engine closes the submenu (`engine.rs:2676-2688`) or the compositor dismisses it
- **Reify:** High — six interacting fields, many handlers, a far side in the engine, and crashes below
- **Suspected bug:** (1) a torn-off palette (`menu_view`, rendered live by the shell) whose menu shrinks while a row past the new end is hovered panics on the next pointer move, leave or arrow key: `set_hover` calls `row_rect(old)` (`menus.rs:460` → `self.rows[i]` `menus.rs:423`), and Enter/→ index `self.rows[i]` (`menus.rs:493`, `:930`); (2) after ← closes a submenu (`MenuBack` closes only the child window), the parent's `open_sub` still names it, so hovering that row again doesn't reopen it (`menus.rs:470`) and rightward moves still count as aiming (`menus.rs:842`) (confidence: medium)
- **Related:** kit engine menu popups `desktop/aizome/crates/aizome-kit/src/engine.rs:1295` (`menu_sub`), `:2662` (`kit`); shell palettes `desktop/aizome/crates/aizome-shell/src/tearoff.rs:111`
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#uiw-08)

<a id="uiw-09"></a>

### UIW-09 — In-window menu bar's open title
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/menus.rs:1060` (`BarState.open`, `.hover`)
- **Encoding:** `BarState` in the bar node's state, written by the strip and by the engine, and deleted wholesale by the engine when a bar menu closes
- **States:** `Closed` → `Open(i)` ⇄ `Open(j)` (sliding along the bar) → `Closed`; `hover` alongside
- **Transitions:** press on a title opens (`menus.rs:1172` → `open=Some` `menus.rs:1102`, `OpenBarMenu` `menus.rs:1104`); press on the open title closes (`menus.rs:1167-1170`); moving onto another title while open switches (`menus.rs:1184-1188`); leave clears hover (`menus.rs:1193`); engine deletes the state when the bar's menu window closes (`engine.rs:613-616`, `:1171`) and sets `open` after opening (`engine.rs:1259`)
- **Hazards:** three writers and deletion-by-type instead of a transition; hover is wiped whenever a menu closes
- **Reify:** High — scattered writers across widget and engine, user-visible bug below
- **Suspected bug:** clicking the open title to close its menu reopens it: the engine closes a window's menus on any press before routing the press (`engine.rs:2107-2122`), which deletes `BarState`, so the strip sees `open == None` and takes the open arm (`menus.rs:1172`) instead of the close arm (`menus.rs:1167`) (confidence: medium — needs the compositor to deliver the press to the parent while the popup grabs, as wlroots does)
- **Related:** `desktop/aizome/crates/aizome-kit/src/engine.rs:1239` (`open_bar_menu`), `:593` (`close_win`)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#uiw-09)

<a id="uiw-11"></a>

### UIW-11 — Scroll-bar visibility and thumb drag
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/scroll.rs:52` (`ScrollState.shown_until`, `.drag`, `.hover_bar`)
- **Encoding:** linger deadline, `drag: Option<(vertical, grab)>`, hover flag; `visible()` = dragging or hovered or before the deadline (`scroll.rs:109-111`); hidden by a Tick
- **States:** `Hidden` → `Shown(until)` → `Hidden`; `Hovered`; `Dragging(axis, grab)`
- **Transitions:** `scroll_by` pushes `shown_until` and asks for a tick at LINGER+20 ms (`scroll.rs:161-162`); bar press starts a drag (`scroll.rs:189`, `:196`); move drags (`scroll.rs:203-205`) or toggles hover (`scroll.rs:208-212`); release ends, re-lingers (`scroll.rs:217-219`); leave clears hover (`scroll.rs:227-230`); Tick redraws the bars only if no longer visible, and never re-arms (`scroll.rs:233-238`)
- **Hazards:** the engine keeps one pending tick per node, first request wins (`engine.rs:1721-1727`); a lost release leaves `drag` set and moves keep scrolling
- **Reify:** High — three fields plus a timer whose arming the widget can't see, bug below
- **Suspected bug:** scroll for more than ~20 ms (any touchpad gesture, several wheel notches): only the first `scroll_by` arms the tick, which fires ~920 ms later while `shown_until`, pushed on by later events, is still ahead; nothing is invalidated and nothing re-arms, so the bars stay drawn until something else repaints them — in every `scroll`, `list`, `table` and `tree` (confidence: high)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#uiw-11)

<a id="uiw-15"></a>

### UIW-15 — Selection cursor and anchor in lists, tables and outlines
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/table.rs:37` (`PickState.anchor`, `.cursor`); also `desktop/aizome/crates/aizome-kit/src/widgets/scroll.rs:698` (`ListState.anchor`, `.cursor`)
- **Encoding:** kit-owned row indices kept beside the program-owned selection (indices for list and table, ids flattened to `selected_rows` for the outline, `table.rs:926-927`)
- **States:** `NoCursor` → `Cursor(i)` with `Anchor(a)`; representable and reachable: cursor pointing at another item, or past the end, after the program's rows change
- **Transitions:** clicks set anchor (`table.rs:57`, `:63`; `scroll.rs:942`, `:948`) and cursor (`table.rs:636`, `:1105`; `scroll.rs:741`); keys move (`table.rs:110`, `:115`; `scroll.rs:758`); drag-out release (`table.rs:563`; `scroll.rs:905`); right click (`table.rs:646`; `scroll.rs:960`); the outline's triangle toggles rows without touching either (`table.rs:1098-1101`); nothing clamps or remaps them when rows change
- **Hazards:** `cursor.or(selected.last())` prefers the stale cursor over the program's selection (`table.rs:80`, `:660`, `:1128`; `scroll.rs:977`); table/outline ↑ and Page Up don't clamp (`table.rs:84`, `:88`); Enter activates the cursor index
- **Reify:** High — two copies, many writers, invariants against program data that nothing enforces, crash and wrong-file bugs below
- **Suspected bug:** (1) outline: click a row deep in an open branch (cursor 25), close its parent by the triangle (rows shrink, cursor untouched), press Return, ← or → → `self.rows[c]` out of bounds panics (`table.rs:1130`, `:1135`, `:1150`) (confidence: high); (2) table: in the portal's file chooser, click file A, sort by a header (the chooser remaps its selection by path, `desktop/aizome/crates/aizome-portal/src/chooser/model.rs:231-254`), press Return → `on_activate` gets the old cursor index (`table.rs:660-662`), so another file is chosen and returned to the requesting app; ↓ likewise moves from the old row (confidence: high)
- **Related:** portal file chooser `desktop/aizome/crates/aizome-portal/src/chooser/model.rs:319` (`activate`), `desktop/aizome/crates/aizome-portal/src/chooser/view.rs:213`
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#uiw-15)

<a id="uiw-18"></a>

### UIW-18 — Editor viewport, position-bar linger and caret blink on one tick
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/text_editor.rs:281` (`EditorState.top_line`, `.top_offset`, `.scroll_x`, `.bar_until`, `.next_blink`, `.blink_off`, `.focused`)
- **Encoding:** viewport as top line plus offset, bar deadline, blink deadline and phase, focus flag; both timers share the node's one coalesced Tick
- **States:** viewport; bar `Hidden` | `Shown(until)`; caret `Unfocused` | `Blinking(next, on/off)`
- **Transitions:** `scroll_by` moves, shows the bar, asks for a tick (`text_editor.rs:765-770`); layout reveal moves the viewport (`text_editor.rs:910-956`, written `:998-1000`); Focus(true) sets `next_blink` and asks for a tick (`text_editor.rs:1313-1316`); Focus(false) clears the preedit (`text_editor.rs:1310-1312`); Tick hides a due bar (`text_editor.rs:1334-1337`), blinks and re-arms only if the blink is due (`text_editor.rs:1340-1351`)
- **Hazards:** a Tick that arrives early (armed by the other use) neither blinks nor re-arms; the bar only re-checks through the blink chain
- **Reify:** High — seven fields, two timers multiplexed on one tick, bugs below
- **Suspected bug:** (1) focus the editor while an earlier tick is pending (scroll it, click into it 0.4–0.9 s later; or Tab away and back within half a second): the focus tick request is dropped (`engine.rs:1723`), the pending tick fires before `next_blink`, isn't due (`text_editor.rs:1340`), so nothing re-arms and the caret stops blinking until the next focus change; (2) an unfocused editor scrolled for more than ~20 ms keeps its position bar drawn, as in UIW-11 (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#uiw-18)

<a id="uiw-27"></a>

### UIW-27 — Session lock
- **Where:** `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:548` (`enum Locking`), `app.rs:797` (`Cx.locking`); lock surfaces are windows of `Kind::Lock` (`desktop/aizome/sukumo/crates/sidestep-ui/src/window.rs:143`)
- **Encoding:** partially explicit: the `Locking` enum, plus lock windows the program opens and the toolkit closes
- **States:** `Unlocked` → `Asked` → `Locked` → `Unlocked`; lock windows meant to exist only while `Asked`/`Locked`
- **Transitions:** `lock_session` Unlocked→Asked (`app.rs:1049-1051`); `SessionLocked` Asked→Locked (`app.rs:1606-1608`); `unlock_session` →Unlocked and closes lock windows (`app.rs:1058-1063`); `SessionLockFinished` →Unlocked, closes them, tells the handler (`app.rs:1612-1615`)
- **Hazards:** render-thread messages carry no lock generation, so a `SessionLocked` or `SessionLockFinished` for a withdrawn lock applies to the next one; opening a lock window while `Unlocked` is refused only by the backend (`window.rs:226-227`)
- **Reify:** High — four transition sites across two threads, a lock screen's failure is a hang
- **Suspected bug:** a lock screen that unlocks and at once locks again while the old lock's `finished` is in flight: the stale `SessionLockFinished` turns the new `Asked` into `Unlocked` and closes the new lock surfaces (`app.rs:1612-1614`); the backend then creates the new lock, the compositor locks, and the `SessionLocked` is ignored as not `Asked` (`app.rs:1606`) — the session is locked with no lock surface and the program believes it unlocked (confidence: low)
- **Related:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/lock.rs:24` (`lock`), `:39` (`unlock`), `:63` (`finished`)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#uiw-27)
- **Follow-up:** reified as typestate in [#298](https://github.com/bitemyapp/sekka/pull/298). Its suspected bug is fixed, and the mistake no longer compiles.

<a id="uiw-31"></a>

### UIW-31 — Window showing and frame-pacing lifecycle
- **Where:** `desktop/aizome/sukumo/crates/sidestep-ui/src/window.rs:354` (`WindowData.showing`, `.shown_before`, `.configured`, `.frame_pending`, `.damage`, `.scrolls`, `.title_dirty`, `.focused`, `.pointer_inside`, `.text_input`, `.popup_size`, `.shared_showing`); also `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:776` (`Cx.showings`), `app.rs:778` (`.next_showing`), `desktop/aizome/sukumo/crates/sidestep-ui/src/video.rs:43` (`To::Window.showing`)
- **Encoding:** a showing id per show (the first is the window's own number, later ones from `RESHOWN`, `app.rs:746`) maps render-thread messages back, so messages for an old showing find nothing; pacing flags gate each pass; an atomic copy of the showing lets video sinks on other threads drop frames while hidden
- **States:** `Created(hidden)` → `Shown(unconfigured)` → `Configured(idle)` ⇄ `FramePending` → `Hidden` → `Shown(new showing)` … → `Closed`
- **Transitions:** `show_window` allocates and maps a showing (`app.rs:845-855`), `WindowData::show` resets pacing and re-sends regions, cursor, videos (`window.rs:571-592`); Configure sets `configured`, a resize clears `frame_pending` and redraws all (`app.rs:1391`, `:1402-1406`); `display` takes damage and scrolls, presents, sets `frame_pending` (`app.rs:1711-1712`, `:1752-1753`); Frame clears it (`app.rs:1420`); `hide_window` closes popups, forgets the showing, synthesizes `Focused(false)`/`PointerLeft` (`app.rs:875-887`), `WindowData::hide` (`window.rs:596-607`); `close_window` (`app.rs:903-911`); damage and pending pixel moves gather (`window.rs:614-669`); `wants_pass` gate (`window.rs:672-677`); sinks drop frames while the showing is 0 (`desktop/aizome/sukumo/crates/sidestep-ui/src/video.rs:98-101`)
- **Hazards:** a popup opened while its parent is hidden stays open but never shows (`app.rs:838-842`); drawing waits for the render thread's Frame, so a Present that never gets one stalls the window until a resize
- **Reify:** High — a dozen fields with mutation sites across two files and the render thread
- **Related:** render thread `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs` (CreateWindow, CloseWindow, Present); null backend `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/null.rs`
