# UIW: Sukumo UI and toolkit widgets

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/sukumo/crates/sidestep-ui, desktop/aizome/crates/aizome-kit/src/widgets`. Audited at `8e0acbe5`; line numbers refer to that commit.

34 files, 0 pending. 34 implicit machines: 8 High, 11 Medium, 15 Low.

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
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#uiw-01)

<a id="uiw-02"></a>

### UIW-02 — Slider drag and commit
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/controls.rs:971` (`SliderWidget`, `.on_change`, `.on_release`)
- **Encoding:** engine `active` read through `cx.is_pressed()`; values streamed by `on_change`, the drag committed by `on_release`
- **States:** `Idle` → `Dragging` (values streamed) → `Committed` (`on_release`)
- **Transitions:** PointerDown sets the value (`controls.rs:1084`); PointerMove while pressed (`controls.rs:1088-1090`); PointerUp emits `on_release` (`controls.rs:1092-1095`); keys change the value with no release (`controls.rs:1099-1111`)
- **Hazards:** `usable()` is checked before anything (`controls.rs:1078`): a slider disabled, or whose range collapses, mid-drag ignores the release and never sends `on_release`; an unpressed slider still emits `on_release` for a stray release routed to it (`engine.rs:2167`); keyboard changes never commit; a release lost to a compositor grab leaves it pressed (engine clears `active` on leave only for drags-only widgets, `engine.rs:2069`)
- **Reify:** Medium — small, but the commit message depends on a press lifecycle the widget doesn't own

<a id="uiw-03"></a>

### UIW-03 — Hovered part of steppers and segmented controls
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/controls.rs:1224` (`StepperState.zone`); also `controls.rs:1456` (`SegState.hover`)
- **Encoding:** per-node state: −1/0/+1 half under the pointer; segment index under the pointer
- **States:** `zone ∈ {−1, 0, +1}`; `hover: None | Some(i)`
- **Transitions:** stepper PointerMove (`controls.rs:1336`), PointerDown (`controls.rs:1343`); segment PointerMove (`controls.rs:1583`); neither is cleared on PointerLeave
- **Hazards:** stale part after leaving is masked only by `cx.is_hovered()` in paint (`controls.rs:1290-1292`, `:1517`)
- **Reify:** Low — local and paint-only

<a id="uiw-04"></a>

### UIW-04 — Indeterminate progress sweep
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/controls.rs:1894` (`Sweep.started`); also `controls.rs:1890` (`ProgressWidget.moving`)
- **Encoding:** `started: Option<Instant>` in node state, `moving` recomputed each layout, a self-rearming tick every 50 ms
- **States:** `Still` (determinate or reduced motion) → `Sweeping(started)`
- **Transitions:** layout sets `moving`, `started.get_or_insert`, arms a tick (`controls.rs:1907-1911`); Tick re-arms while moving (`controls.rs:1956-1958`); becoming determinate stops re-arming
- **Hazards:** `started` is never cleared, so going determinate and back resumes the old phase (cosmetic)
- **Reify:** Low — one field and one timer

<a id="uiw-05"></a>

### UIW-05 — Image scaled on its second sighting
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/controls.rs:2243` (`Scaled.made`, `.seen`)
- **Encoding:** `seen: Cell<Option<ScaleKey>>`, `made: RefCell<Option<(ScaleKey, Image)>>`, keyed by pixel size, revision, device size and scale
- **States:** `Unseen` → `Seen(key)` (drawn scaled, slow) → `Made(key)` (copied 1:1)
- **Transitions:** first `get` for a key records it and returns none (`controls.rs:2259`); a second consecutive one renders and stores (`controls.rs:2262-2263`); another key restarts; state created in layout (`controls.rs:2275`), used in paint (`controls.rs:2316-2333`)
- **Hazards:** the copy is made only on a second paint at the same key, so a view painted once at its final size (end of a resize) stays on the slow bicubic path until something else repaints it; one key cached, so alternating sizes never settle; the offscreen render runs inside paint
- **Reify:** Low — a two-step debounce, local

<a id="uiw-06"></a>

### UIW-06 — Mouse area press, drag, drag-out and hover gesture
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/interact.rs:294` (`MouseState.start`, `.last`, `.inside`)
- **Encoding:** three fields in node state; button state implied by `start.is_some()`
- **States:** `Outside` | `Inside` × `Idle` | `Pressed(start, last)` | `Dragging`; with `drag_out`, `Pressed` → `DraggedOut` (start cleared)
- **Transitions:** PointerMove sets `inside`, `on_hover(true)` (`interact.rs:337-341`); 4 pt past the press with `drag_out` clears and starts the drag (`interact.rs:345-352`); drag `Move` with `last.replace` (`interact.rs:354-357`); PointerLeave `inside=false`, `on_hover(false)` (`interact.rs:366-370`); PointerDown: double click short-circuits (`interact.rs:374-379`), `Start` (`interact.rs:385-390`), drag-out arming (`interact.rs:392-397`); PointerUp takes `start`, `End` (`interact.rs:401-405`)
- **Hazards:** `Move` is sent whenever `start` is set, not only while a button is down: a release lost to a compositor grab (engine keeps `active` on leave, `engine.rs:2069`) keeps the area dragging on plain hover until the next press and release; with both `on_drag` and `drag_out`, a drag out ends the gesture with `Start` (and maybe `Move`s) but no `End` (documented)
- **Reify:** Medium — guarded, but the gesture phase is inferred from one `Option`

<a id="uiw-07"></a>

### UIW-07 — Previous-layout snapshots for partial redraw
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/interact.rs:614` (`CanvasParts.parts`, `.size`); also `desktop/aizome/crates/aizome-kit/src/widgets/scroll.rs:1039` (`DrawnSelection`), `desktop/aizome/crates/aizome-kit/src/widgets/text_editor.rs:300` (`EditorState.drawn`, `.drawn_x`, `.caret`, `.width`)
- **Encoding:** what the last layout drew, kept in node state and diffed against the new layout to invalidate only what changed
- **States:** `NoSnapshot` (first layout, or size/width changed: full redraw) → `Snapshot(prev)`
- **Transitions:** canvas parts replaced (`interact.rs:551`), size remembered (`interact.rs:553`); selection replaced (`scroll.rs:1052`); editor lines taken and rewritten (`text_editor.rs:995-1008`), full on width change (`text_editor.rs:1009`)
- **Hazards:** correct only if every layout's damage reaches the screen; none observed in the widgets
- **Reify:** Low — memo-and-diff, each local

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
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#uiw-08)

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
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#uiw-09)

<a id="uiw-10"></a>

### UIW-10 — Split divider drag
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/panes.rs:130` (`SplitState.at`, `.dragging`, `.near`)
- **Encoding:** user's divider position, grab offset while dragging, hover flag; pointer captured while dragging
- **States:** `Idle` → `Near` (resize cursor) → `Dragging(grab)` → `Idle`; `at` overrides the program's start once set
- **Transitions:** hover toggles `near` (`panes.rs:255-261`); press near starts and captures (`panes.rs:264-268`); move while dragging sets `at`, relayouts, emits `on_resize` (`panes.rs:245-252`); release ends and releases capture (`panes.rs:270-275`); leave clears `near` (`panes.rs:276-283`); layout prefers `at` (`panes.rs:183`)
- **Hazards:** after one drag, `at` beats later `first_size`/`ratio` from the program for the element's life; `on_resize` reports the raw pointer position (negative or past the end) though layout clamps (`panes.rs:188-189`); a lost release keeps dragging on hover (leave doesn't end it); any button's release ends it
- **Reify:** Medium — three fields, guarded handlers

<a id="uiw-11"></a>

### UIW-11 — Scroll-bar visibility and thumb drag
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/scroll.rs:52` (`ScrollState.shown_until`, `.drag`, `.hover_bar`)
- **Encoding:** linger deadline, `drag: Option<(vertical, grab)>`, hover flag; `visible()` = dragging or hovered or before the deadline (`scroll.rs:109-111`); hidden by a Tick
- **States:** `Hidden` → `Shown(until)` → `Hidden`; `Hovered`; `Dragging(axis, grab)`
- **Transitions:** `scroll_by` pushes `shown_until` and asks for a tick at LINGER+20 ms (`scroll.rs:161-162`); bar press starts a drag (`scroll.rs:189`, `:196`); move drags (`scroll.rs:203-205`) or toggles hover (`scroll.rs:208-212`); release ends, re-lingers (`scroll.rs:217-219`); leave clears hover (`scroll.rs:227-230`); Tick redraws the bars only if no longer visible, and never re-arms (`scroll.rs:233-238`)
- **Hazards:** the engine keeps one pending tick per node, first request wins (`engine.rs:1721-1727`); a lost release leaves `drag` set and moves keep scrolling
- **Reify:** High — three fields plus a timer whose arming the widget can't see, bug below
- **Suspected bug:** scroll for more than ~20 ms (any touchpad gesture, several wheel notches): only the first `scroll_by` arms the tick, which fires ~920 ms later while `shown_until`, pushed on by later events, is still ahead; nothing is invalidated and nothing re-arms, so the bars stay drawn until something else repaints them — in every `scroll`, `list`, `table` and `tree` (confidence: high)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#uiw-11)

<a id="uiw-12"></a>

### UIW-12 — Scroll offset, reveal latch and laid-out offset
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/scroll.rs:53` (`ScrollState.offset`, `.content`, `.viewport`); also `scroll.rs:61` (`.revealed`), `scroll.rs:63` (`.laid`)
- **Encoding:** offset clamped to content minus viewport; `revealed: Option<u64>` latches the last program reveal; `laid` is the offset content was last placed at
- **States:** offset; `revealed: None | Some(what)`; `laid: None | Some(prev)`
- **Transitions:** `scroll_by` (`scroll.rs:156-157`); layout clamps (`scroll.rs:417-419`, `:804-806`); `reveal_once` latches and moves (`scroll.rs:84-94`) from list (`scroll.rs:807-809`), table (`table.rs:433-435`) and tree (`table.rs:1004-1008`) layouts; `moved_since_laid` (`scroll.rs:70-71`) → pixel scroll (`scroll.rs:427`)
- **Hazards:** a reveal fires only when its key changes, so revealing the same row after the user scrolled away does nothing (documented); list and table keys are row indices, so a different item at the same index isn't revealed
- **Reify:** Low — clamped, single writer per field

<a id="uiw-13"></a>

### UIW-13 — Row press that may become a drag out
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/scroll.rs:515` (`DragPress.at`, `.collapse`)
- **Encoding:** press point and row, and a deferred "select this row alone" when the press fell on a multi-selection
- **States:** `Idle` → `Pressed(at, row[, collapse])` → `DraggedOut` | `Released` (collapse selects the row)
- **Transitions:** `drag_pressed` (`scroll.rs:528-529`) from list (`scroll.rs:929-933`) and table (`table.rs:628-634`); `drag_moved` past 4 pt clears and starts the drag (`scroll.rs:539-545`) from PointerMove (`scroll.rs:898-901`, `table.rs:553-556`); `drag_released` (`scroll.rs:551-553`) on release (`scroll.rs:903-908`, `table.rs:558-566`)
- **Hazards:** `drag_moved` runs on every move, pressed or not: after a lost release, hovering 4 pt away starts a drag with no button down; the table skips `drag_released` while a column resizes (`table.rs:559`)
- **Reify:** Medium — shared by two widgets, phase inferred from an `Option`

<a id="uiw-14"></a>

### UIW-14 — Table column resize
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/table.rs:333` (`TableState.widths`, `.resizing`, `.near_edge`)
- **Encoding:** user widths by column index, `resizing: Option<(column, from_x, start_width)>`, hovered edge
- **States:** `Idle` → `NearEdge(c)` → `Resizing(c, from, w0)` → `Idle`
- **Transitions:** header hover (`table.rs:582-586`); press on an edge (`table.rs:597-599`); move resizes (`table.rs:569-576`); release ends (`table.rs:613-616`); leave clears the edge (`table.rs:590-593`); layout applies overrides (`table.rs:413-419`)
- **Hazards:** overrides are by index, so a program that reorders or inserts columns applies them to the wrong ones; `resizing.0` indexes `self.columns` unguarded (`table.rs:570`) if columns shrink mid-drag; a lost release keeps resizing on hover
- **Reify:** Medium — three fields, guarded

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
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#uiw-15)

<a id="uiw-16"></a>

### UIW-16 — Type-select prefix buffer
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/scroll.rs:701` (`ListState.typed`, `.typed_at`); also `desktop/aizome/crates/aizome-kit/src/widgets/table.rs:40` (`PickState.typed`, `.typed_at`)
- **Encoding:** typed prefix and the time of its last key
- **States:** `Empty` → `Typing(prefix, at)` → reset after 1 s idle
- **Transitions:** cleared if more than 1000 ms passed (`scroll.rs:995-997`; `table.rs:94-96`); appended and stamped (`scroll.rs:998-999`; `table.rs:97-98`)
- **Hazards:** reset is lazy (on the next key); none observed
- **Reify:** Low — tiny debounce

<a id="uiw-17"></a>

### UIW-17 — Outline's own open set
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/table.rs:847` (`OpenState.open`)
- **Encoding:** set of open ids in node state, used only when the program gives no `expanded`
- **States:** per id `Closed` | `Open`
- **Transitions:** `toggle` inserts or removes (`table.rs:944-948`) and still emits `on_toggle` (`table.rs:952-954`); `flatten` reads it (`table.rs:912`)
- **Hazards:** ids are never pruned when items go; switching between kit-kept and program-kept modes leaves the other set stale
- **Reify:** Low — one set, one writer

<a id="uiw-18"></a>

### UIW-18 — Editor viewport, position-bar linger and caret blink on one tick
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/text_editor.rs:281` (`EditorState.top_line`, `.top_offset`, `.scroll_x`, `.bar_until`, `.next_blink`, `.blink_off`, `.focused`)
- **Encoding:** viewport as top line plus offset, bar deadline, blink deadline and phase, focus flag; both timers share the node's one coalesced Tick
- **States:** viewport; bar `Hidden` | `Shown(until)`; caret `Unfocused` | `Blinking(next, on/off)`
- **Transitions:** `scroll_by` moves, shows the bar, asks for a tick (`text_editor.rs:765-770`); layout reveal moves the viewport (`text_editor.rs:910-956`, written `:998-1000`); Focus(true) sets `next_blink` and asks for a tick (`text_editor.rs:1313-1316`); Focus(false) clears the preedit (`text_editor.rs:1310-1312`); Tick hides a due bar (`text_editor.rs:1334-1337`), blinks and re-arms only if the blink is due (`text_editor.rs:1340-1351`)
- **Hazards:** a Tick that arrives early (armed by the other use) neither blinks nor re-arms; the bar only re-checks through the blink chain
- **Reify:** High — seven fields, two timers multiplexed on one tick, bugs below
- **Suspected bug:** (1) focus the editor while an earlier tick is pending (scroll it, click into it 0.4–0.9 s later; or Tab away and back within half a second): the focus tick request is dropped (`engine.rs:1723`), the pending tick fires before `next_blink`, isn't due (`text_editor.rs:1340`), so nothing re-arms and the caret stops blinking until the next focus change; (2) an unfocused editor scrolled for more than ~20 ms keeps its position bar drawn, as in UIW-11 (confidence: medium)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#uiw-18)

<a id="uiw-19"></a>

### UIW-19 — Editor pointer selection and dragging selected text out
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/text_editor.rs:292` (`EditorState.drag`); also `text_editor.rs:296` (`.drag_text`)
- **Encoding:** `drag: Option<(granularity, from)>`, `drag_text: Option<(press point, character)>`; moves gated on `cx.is_pressed()`
- **States:** `Idle` → `Selecting(char|word|line, from)` | `PendingDragOut(at, char)` → `DraggedOut` | `ClickedInSelection`
- **Transitions:** press on the selection arms the drag out (`text_editor.rs:1230-1234`); other presses start selecting (`text_editor.rs:1236-1242`); moves while pressed: past 4 pt start the drag out (`text_editor.rs:1246-1251`), else extend by granularity (`text_editor.rs:1254-1276`) and autoscroll past the edges (`text_editor.rs:1278-1281`); release clears and collapses to the press point (`text_editor.rs:1285-1290`)
- **Hazards:** autoscroll happens only on pointer moves (holding still past the edge stops it); `on_click` swallows single clicks the program claims (`text_editor.rs:1205-1211`)
- **Reify:** Medium — two fields, mutually exclusive by convention

<a id="uiw-20"></a>

### UIW-20 — Editor caret reveal and goal column
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/text_editor.rs:298` (`EditorState.reveal`); also `text_editor.rs:306` (`.shown_selection`), `text_editor.rs:285` (`.goal_x`)
- **Encoding:** `reveal` flags the editor's own moves; a selection different from `shown_selection` means the program moved it; `goal_x` keeps the column through vertical moves
- **States:** `Settled` | `OwnMove` (reveal) | `ProgramMove` (far jump recentres); `goal_x: None | Some(x)`
- **Transitions:** emitting an action sets `reveal` (`text_editor.rs:608`); layout takes it (`text_editor.rs:904`), detects a program move (`text_editor.rs:905`), records the selection (`text_editor.rs:906`), recentres a far program jump (`text_editor.rs:923-925`); vertical motion sets the goal (`text_editor.rs:661`), horizontal moves and edits clear it (`text_editor.rs:644`, `:616`)
- **Hazards:** `reveal` is consumed by the next layout whether or not the program has applied the action; if a layout runs in between, the editor's own move is then classed as a program jump and recentred; a program that ignores the action leaves the flag spent for nothing
- **Reify:** Medium — ordering-dependent inference across the program boundary

<a id="uiw-21"></a>

### UIW-21 — Input-method composition in editors and text fields
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/text_editor.rs:286` (`EditorState.preedit`, `.preedit_caret`); also `desktop/aizome/crates/aizome-kit/src/widgets/text_field.rs:187` (`FieldState.preedit`, `.preedit_caret`)
- **Encoding:** text being composed and its caret, shown inserted at the caret
- **States:** `NotComposing` (empty) → `Composing(text, caret)` → `Committed` | `Dropped` (blur)
- **Transitions:** Preedit sets (`text_editor.rs:1301-1302`; `text_field.rs:716-717`); Commit clears and inserts (`text_editor.rs:1295-1296`; `text_field.rs:710-711`); blur clears (`text_editor.rs:1311`; `text_field.rs:739`); layout inserts it at the caret (`text_editor.rs:967`; `text_field.rs:497-500`); caret rectangle to the input method (`text_editor.rs:1318`, `:1349`; `text_field.rs:363`)
- **Hazards:** clicks, arrows or a program change that move the caret mid-composition carry the preedit along and never reset the input method; `preedit_caret` isn't cleared on commit (harmless while the preedit is empty); key events arriving while composing take the normal key path
- **Reify:** Medium — duplicated in two widgets, protocol on the far side
- **Related:** text-input protocol `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/textinput.rs`; kit engine IME on/off `desktop/aizome/crates/aizome-kit/src/engine.rs:1872`

<a id="uiw-22"></a>

### UIW-22 — Text field focus, caret blink and first-focus selection
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/text_field.rs:195` (`FieldState.focused`); also `text_field.rs:189` (`.blink_off`), `text_field.rs:200` (`.primed`)
- **Encoding:** focus flag, blink phase with a self-rearming tick, one-shot `primed` latch for `select_on_focus`
- **States:** `Unfocused` → `Focused(on|off)`; `primed: false → true` once
- **Transitions:** Focus(true) sets focus, primes and applies `select_on_focus` once (`text_field.rs:724-734`), asks for a tick (`text_field.rs:736`); Focus(false) clears the preedit, emits `on_blur` (`text_field.rs:739-742`); Tick toggles and re-arms while focused (`text_field.rs:750-757`); edits and moves show the caret (`text_field.rs:321`, `:338`)
- **Hazards:** `primed` lasts for the key's life, so a field reused under one key never selects again (documented)
- **Reify:** Low — local, one writer per field

<a id="uiw-23"></a>

### UIW-23 — Text field pointer selection and dragging selected text out
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/text_field.rs:190` (`FieldState.selecting`); also `text_field.rs:194` (`.drag_text`)
- **Encoding:** selecting flag and a pending drag-out press; moves gated on `cx.is_pressed()`
- **States:** `Idle` → `Selecting` | `PendingDragOut(at, offset)` → `DraggedOut` | `ClickedInSelection`
- **Transitions:** clear button empties a search field (`text_field.rs:635-639`); press on the selection arms the drag out (`text_field.rs:644-648`); other presses select word, all or caret (`text_field.rs:650-669`); moves while pressed: past 4 pt start the drag (`text_field.rs:676-683`), else extend (`text_field.rs:686-690`); release clears and collapses (`text_field.rs:695-705`)
- **Hazards:** none observed
- **Reify:** Low — two fields, gated on the engine's press

<a id="uiw-24"></a>

### UIW-24 — Text field undo history, typing groups and optimistic caret
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/text_field.rs:196` (`FieldState.history`, `.future`, `.last_edit`); also `text_field.rs:184` (`.caret`, `.anchor`)
- **Encoding:** undo and redo stacks of (value, caret, anchor), the last edit's time and kind for grouping, byte offsets into a value the program owns
- **States:** history/future stacks; `Grouping(kind, at)` | `Ungrouped`
- **Transitions:** `replace` pushes unless grouped within 1 s (`text_field.rs:306-312`), clears the future (`text_field.rs:314`), stamps (`text_field.rs:315`), moves the caret before the program answers (`text_field.rs:319-320`), emits (`text_field.rs:322`); undo/redo swap (`text_field.rs:378-389`); caret moves end a group (`text_field.rs:339`); layout floors caret and anchor to the value shown (`text_field.rs:485-488`)
- **Hazards:** a program that rejects or rewrites the text (digits only, length limit, formatting) leaves the caret moved without the text; history stores values the program may since have replaced, so undo restores text the program had rewritten
- **Reify:** Medium — kit-side history over program-owned text

<a id="uiw-25"></a>

### UIW-25 — Video above or below the window's drawing
- **Where:** `desktop/aizome/crates/aizome-kit/src/widgets/video.rs:38` (`Placed.below`)
- **Encoding:** per-node flag the engine sets after each layout; paint punches a clear hole while below
- **States:** `OnItsPlane` ⇄ `UnderDrawing`
- **Transitions:** engine flips it when an `over_video` element overlaps and invalidates (`engine.rs:827-830`, `:834-835`); paint clears the picture's rectangle (`video.rs:158-163`)
- **Hazards:** `Placed` isn't part of the widget's paint hash; correct only through the engine's explicit invalidation
- **Reify:** Low — one flag, one writer
- **Related:** `desktop/aizome/sukumo/crates/sidestep-ui/src/window.rs:783` (`place_video`); `desktop/aizome/sukumo/crates/sidestep-engine/src/video.rs`

<a id="uiw-26"></a>

### UIW-26 — Application run lifecycle and loop wake-up
- **Where:** `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:650` (`RUNNING`); also `app.rs:784` (`Cx.quit`), `app.rs:788` (`.opened_any`), `app.rs:582` (`Wake.signalled`, `.woken`), `app.rs:781` (`.timers`, `.timer_due`)
- **Encoding:** process-wide running latch with a drop guard, quit flag, "a window was ever opened" latch, wake flags, timer map keyed by due time
- **States:** `NotRunning` → `Running(no window yet)` → `Running(windows)` → `Stopping` (quit, or last window closed) → `NotRunning`
- **Transitions:** `run` asserts and sets the latch (`app.rs:700`), guard clears it (`app.rs:655-658`); `open_window` latches `opened_any` (`app.rs:811`); `done()` (`app.rs:1296-1297`) checked after each message and timer (`app.rs:1257`, `:1279`, `:1347`), then `close_all` (`app.rs:736`); `wake` sets `signalled` (`app.rs:591`), `wait` clears it (`app.rs:610`); `Proxy::wake` sets `woken` (`app.rs:624`), the loop swaps it (`app.rs:1261`); repeating timers keep their phase (`app.rs:1336-1345`); sleep until the first-frame, timer or coast deadline (`app.rs:1288-1292`)
- **Hazards:** hidden windows count as open, so an app whose windows are all hidden keeps running (by design); none observed otherwise
- **Reify:** Low — well contained, single loop

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
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#uiw-27)
- **Follow-up:** reified as typestate in [#298](https://github.com/bitemyapp/sekka/pull/298). Its suspected bug is fixed, and the mistake no longer compiles.

<a id="uiw-28"></a>

### UIW-28 — Touchpad coasting
- **Where:** `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:787` (`Cx.coast: Option<Coasting>`); also `app.rs:756` (`Coasting.physics`)
- **Encoding:** the coasting window, position, modifiers and physics (with `began` and a deadline)
- **States:** `Idle` → `Coasting(not begun)` → `Coasting(begun)` → `Idle` (Ended sent if begun)
- **Transitions:** a touchpad scroll ending fast starts it (`app.rs:1497-1508`); each Frame of its window steps it (`app.rs:1422-1424`), or the deadline does (`app.rs:1276-1278`); steps report Began/Changed (`app.rs:1375-1377`), slowing stops (`app.rs:1378-1380`); a press (`app.rs:1466-1468`) or another scroll (`app.rs:1482`) stops it with Ended (`app.rs:1363-1368`); hiding or closing its window drops it silently (`app.rs:895-897`)
- **Hazards:** steps are paced by that window's frames; a window that doesn't redraw coasts at the 50 ms fallback
- **Reify:** Medium — one `Option`, but driven from five places
- **Related:** physics `desktop/aizome/sukumo/crates/sidestep-engine/src/momentum.rs`

<a id="uiw-29"></a>

### UIW-29 — Drag-and-drop offers over windows
- **Where:** `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:794` (`Cx.drags: HashMap<u64, DragState>`); also `app.rs:749` (`DragState.response`)
- **Encoding:** per offer: the window, what it carries and the handler's last answer
- **States:** per drag `Entered(window)` → `Moving(response)` (periodic re-asks) → `Left` | `Dropped(performed)`
- **Transitions:** DndEnter inserts and answers (`app.rs:1545-1556`); DndMotion, DndActions, DndTick re-ask and answer (`app.rs:1558-1568`, `drag_update` `app.rs:1648-1683`, stored `app.rs:1681`); DndLeave removes, `drag_left` (`app.rs:1569-1572`); DndDrop removes, `dropped` or `drag_left`, then DndFinish (`app.rs:1574-1585`); hiding or closing the window removes silently (`app.rs:894`)
- **Hazards:** a window hidden mid-drag loses its offers without `drag_left` (the kit's drop target can keep its highlight); the drop uses the last cached answer
- **Reify:** Medium — several handlers, render thread waits on each answer
- **Related:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs`

<a id="uiw-30"></a>

### UIW-30 — Clipboard and primary-selection ownership
- **Where:** `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:791` (`Cx.clipboard`); also `app.rs:793` (`.primary`), `app.rs:164` (`ClipboardRead.cancelled`)
- **Encoding:** (identity, data) of the program's last offer, compared with the shared selection's current identity at each read; worker snapshots carry a cancel flag and the foreign read's currency
- **States:** `NotOwner` | `Owner(identity)` (until another client's offer changes the identity); snapshot `Current` → `Cancelled` | `Stale`
- **Transitions:** offering bumps the identity and records (`app.rs:1107-1109`; primary `app.rs:1123-1125`); reads compare identities (`app.rs:1136`, `:1153`, `:1164`, `:1186`); `cancel` (`app.rs:201-206`); `is_current` (`app.rs:197-199`)
- **Hazards:** the program's last selection stays in memory after another client takes ownership, until it offers another (possibly MiBs)
- **Reify:** Low — compare-on-read, one writer
- **Related:** `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs`

<a id="uiw-31"></a>

### UIW-31 — Window showing and frame-pacing lifecycle
- **Where:** `desktop/aizome/sukumo/crates/sidestep-ui/src/window.rs:354` (`WindowData.showing`, `.shown_before`, `.configured`, `.frame_pending`, `.damage`, `.scrolls`, `.title_dirty`, `.focused`, `.pointer_inside`, `.text_input`, `.popup_size`, `.shared_showing`); also `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:776` (`Cx.showings`), `app.rs:778` (`.next_showing`), `desktop/aizome/sukumo/crates/sidestep-ui/src/video.rs:43` (`To::Window.showing`)
- **Encoding:** a showing id per show (the first is the window's own number, later ones from `RESHOWN`, `app.rs:746`) maps render-thread messages back, so messages for an old showing find nothing; pacing flags gate each pass; an atomic copy of the showing lets video sinks on other threads drop frames while hidden
- **States:** `Created(hidden)` → `Shown(unconfigured)` → `Configured(idle)` ⇄ `FramePending` → `Hidden` → `Shown(new showing)` … → `Closed`
- **Transitions:** `show_window` allocates and maps a showing (`app.rs:845-855`), `WindowData::show` resets pacing and re-sends regions, cursor, videos (`window.rs:571-592`); Configure sets `configured`, a resize clears `frame_pending` and redraws all (`app.rs:1391`, `:1402-1406`); `display` takes damage and scrolls, presents, sets `frame_pending` (`app.rs:1711-1712`, `:1752-1753`); Frame clears it (`app.rs:1420`); `hide_window` closes popups, forgets the showing, synthesizes `Focused(false)`/`PointerLeft` (`app.rs:875-887`), `WindowData::hide` (`window.rs:596-607`); `close_window` (`app.rs:903-911`); damage and pending pixel moves gather (`window.rs:614-669`); `wants_pass` gate (`window.rs:672-677`); sinks drop frames while the showing is 0 (`desktop/aizome/sukumo/crates/sidestep-ui/src/video.rs:98-101`)
- **Hazards:** a popup opened while its parent is hidden stays open but never shows (`app.rs:838-842`); drawing waits for the render thread's Frame, so a Present that never gets one stalls the window until a resize
- **Reify:** High — a dozen fields with mutation sites across two files and the render thread
- **Related:** render thread `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs` (CreateWindow, CloseWindow, Present); null backend `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/null.rs`

<a id="uiw-32"></a>

### UIW-32 — (test harness) Step-polled scenario drivers on the null render thread
- **Where:** `desktop/aizome/sukumo/crates/sidestep-ui/tests/desktop.rs:66` (`Test.step`); also `desktop/aizome/sukumo/crates/sidestep-ui/tests/drag_out.rs:29` (`Test.step`), `desktop/aizome/sukumo/crates/sidestep-ui/tests/headless.rs:33` (`Test.step`, `.mark`), `desktop/aizome/sukumo/crates/sidestep-ui/tests/scrolling.rs:63` (`List.step`), `desktop/aizome/sukumo/crates/sidestep-ui/tests/offscreen.rs:32` (`Shown.drawn`)
- **Encoding:** an integer step advanced by a 2 ms repeating timer that polls conditions (events seen, render log, `testing::render_idle()`), with a 30 s deadline
- **States:** desktop 0–14 (20 between 2 and 3); headless 0–12; drag_out 0–3; scrolling 0–12; then quit
- **Transitions:** desktop `advance` (`tests/desktop.rs:128-355`, e.g. `:155`, `:182`, `:204`, `:351`) and deadline (`tests/desktop.rs:397-401`); headless `next` (`tests/headless.rs:73-76`), step 8 re-injects a press every poll until coasting ends (`tests/headless.rs:319-321`); drag_out (`tests/drag_out.rs:70-110`); scrolling `step += 1` (`tests/scrolling.rs:158`); offscreen quits once drawn and idle (`desktop/aizome/sukumo/crates/sidestep-ui/tests/offscreen.rs:50-51`)
- **Hazards:** a condition that never holds fails only at the deadline; headless step 8 may inject several presses
- **Reify:** Low — test scaffolding
- **Related:** null render thread `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/null.rs`

<a id="uiw-33"></a>

### UIW-33 — (test harness) Virtual pointer driver
- **Where:** `desktop/aizome/sukumo/crates/sidestep-ui/tests/support/pointer.rs:16` (`enum Do`); also `desktop/aizome/sukumo/crates/sidestep-ui/tests/support/pointer.rs:31` (`run`)
- **Encoding:** a thread consuming commands on a channel; the button's state is implied by the order of `Press` and `Release`
- **States:** `Up` ⇄ `Held` (after `Press`) → `Done`
- **Transitions:** `MoveTo` (`desktop/aizome/sukumo/crates/sidestep-ui/tests/support/pointer.rs:46-49`); `Click` (`:50-55`); `Press`/`Release` (`:56-60`); `Wheel` (`:61-65`); `Done` ends and destroys the pointer (`:66`, `:72`); 40 ms between commands (`:70`)
- **Hazards:** `Done` or a dropped sender after `Press` leaves the button down until the pointer is destroyed; fixed pacing, no acknowledgement
- **Reify:** Low — test scaffolding

<a id="uiw-34"></a>

### UIW-34 — (test harness) Headless-sway scenario drivers
- **Where:** `desktop/aizome/sukumo/crates/sidestep-ui/tests/wayland_desktop.rs:555` (`enum Step`, `Layers.step`, `.asked`, `.marks`); also `tests/wayland_desktop.rs:400` (`enum Drop`), `:846` (`enum Grab`), `:1172` (`enum Plug`), `:1273` (`enum Lock`), `:938` (`Sharp` counters), `:169` (`Activated` flags), and `desktop/aizome/sukumo/crates/sidestep-ui/tests/wayland_system.rs:51` (`enum Step`, `Test.asked`)
- **Encoding:** partially explicit: step enums advanced by 20 ms (10 ms) timers, plus one-shot `asked` flags, per-window event marks, counters (`Sharp.pressed`, `.checked`, `.released`, `.ended`), shell commands on threads (`wtype`, `wl-paste`, `swaymsg`, `grim`) and the virtual pointer thread
- **States:** e.g. `Layers`: Showing → Pressed → Menu → Moved → Closed → Keyboard → (Typed) → Grown → Measured → Regions → Done
- **Transitions:** `Layers::next` (`tests/wayland_desktop.rs:590-594`), `advance` (`:611-784`), Regions retried when sway hadn't the input region yet (`:760-765`); `Sharp` counter steps (`:996-1026`); `Grabbed` 5 s limit (`:926-927`); `wayland_system` `next` (`tests/wayland_system.rs:92-96`) and `asked` guards (`:250`, `:273`); `run` sequences scenarios, spawning and joining pointer threads (`tests/wayland_desktop.rs:1070-1167`; `tests/wayland_system.rs:335`, `:355`)
- **Hazards:** steps race other clients and screenshots; failures surface only as deadline asserts
- **Reify:** Low — test scaffolding

## Already explicit

- `desktop/aizome/sukumo/crates/sidestep-ui/tests/frame_budget.rs:38` `enum Phase` — the measured drawing phases of the frame-budget run; advanced only in `timer` (`desktop/aizome/sukumo/crates/sidestep-ui/tests/frame_budget.rs:129`)

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/crates/aizome-kit/src/widgets/chrome.rs` | 187 | none |  | stateless frames; sheet modality is the engine's |
| `desktop/aizome/crates/aizome-kit/src/widgets/controls.rs` | 2354 | machines | [UIW-01](#uiw-01), [UIW-02](#uiw-02), [UIW-03](#uiw-03), [UIW-04](#uiw-04), [UIW-05](#uiw-05) | click pairing, slider, hover parts, sweep, image scale cache |
| `desktop/aizome/crates/aizome-kit/src/widgets/interact.rs` | 617 | machines | [UIW-06](#uiw-06), [UIW-07](#uiw-07) | mouse area gesture; canvas parts snapshot; popover content handed off once |
| `desktop/aizome/crates/aizome-kit/src/widgets/layout.rs` | 759 | none |  | pure layout |
| `desktop/aizome/crates/aizome-kit/src/widgets/menus.rs` | 1278 | machines | [UIW-08](#uiw-08), [UIW-09](#uiw-09) | menu hover/submenu/aim/filter; in-window bar title |
| `desktop/aizome/crates/aizome-kit/src/widgets/mod.rs` | 13 | none |  | module list |
| `desktop/aizome/crates/aizome-kit/src/widgets/panes.rs` | 789 | machines | [UIW-10](#uiw-10) | split divider drag; rest pure layout |
| `desktop/aizome/crates/aizome-kit/src/widgets/scroll.rs` | 1059 | machines | [UIW-11](#uiw-11), [UIW-12](#uiw-12), [UIW-13](#uiw-13), [UIW-15](#uiw-15), [UIW-16](#uiw-16), [UIW-07](#uiw-07) | scroll bars, offset/reveal, row drag press, list cursor and type-select |
| `desktop/aizome/crates/aizome-kit/src/widgets/table.rs` | 1184 | machines | [UIW-14](#uiw-14), [UIW-15](#uiw-15), [UIW-16](#uiw-16), [UIW-17](#uiw-17), [UIW-13](#uiw-13), [UIW-12](#uiw-12) | column resize, pick cursor, type-select, outline open set |
| `desktop/aizome/crates/aizome-kit/src/widgets/text_editor.rs` | 1444 | machines | [UIW-18](#uiw-18), [UIW-19](#uiw-19), [UIW-20](#uiw-20), [UIW-21](#uiw-21), [UIW-07](#uiw-07) | viewport/bar/blink tick, selection drag, reveal, IME |
| `desktop/aizome/crates/aizome-kit/src/widgets/text_field.rs` | 846 | machines | [UIW-22](#uiw-22), [UIW-23](#uiw-23), [UIW-24](#uiw-24), [UIW-21](#uiw-21) | focus/blink, selection drag, undo and caret, IME |
| `desktop/aizome/crates/aizome-kit/src/widgets/video.rs` | 223 | machines | [UIW-25](#uiw-25) | video over or under drawing, set by engine |
| `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs` | 2074 | machines | [UIW-26](#uiw-26), [UIW-27](#uiw-27), [UIW-28](#uiw-28), [UIW-29](#uiw-29), [UIW-30](#uiw-30), [UIW-31](#uiw-31) | run loop, session lock, coasting, DnD offers, clipboard, showings |
| `desktop/aizome/sukumo/crates/sidestep-ui/src/canvas.rs` | 997 | none |  | per-pass op recorder; save/restore stack |
| `desktop/aizome/sukumo/crates/sidestep-ui/src/color.rs` | 247 | none |  | colour values and appearance lookups |
| `desktop/aizome/sukumo/crates/sidestep-ui/src/event.rs` | 519 | none |  | event data types and protocol conversion |
| `desktop/aizome/sukumo/crates/sidestep-ui/src/image.rs` | 286 | none |  | image handles; Drop releases render cache; lazy fallback |
| `desktop/aizome/sukumo/crates/sidestep-ui/src/layer_shell.rs` | 235 | none |  | layer placement option types |
| `desktop/aizome/sukumo/crates/sidestep-ui/src/lib.rs` | 110 | none |  | re-exports and docs |
| `desktop/aizome/sukumo/crates/sidestep-ui/src/offscreen.rs` | 502 | none |  | synchronous offscreen raster; thread-local glyph cache |
| `desktop/aizome/sukumo/crates/sidestep-ui/src/testing.rs` | 321 | none |  | test injection helpers; null backend switches live in sidestep-engine |
| `desktop/aizome/sukumo/crates/sidestep-ui/src/text.rs` | 886 | none |  | font and text layout values |
| `desktop/aizome/sukumo/crates/sidestep-ui/src/video.rs` | 143 | machines | [UIW-31](#uiw-31) | sink gated by the window's showing id |
| `desktop/aizome/sukumo/crates/sidestep-ui/src/window.rs` | 1068 | machines | [UIW-31](#uiw-31), [UIW-27](#uiw-27) | window showing and frame pacing; lock window kind |
| `desktop/aizome/sukumo/crates/sidestep-ui/tests/desktop.rs` | 416 | machines | [UIW-32](#uiw-32) | step-polled null-backend driver |
| `desktop/aizome/sukumo/crates/sidestep-ui/tests/drag_out.rs` | 125 | machines | [UIW-32](#uiw-32) | step-polled null-backend driver |
| `desktop/aizome/sukumo/crates/sidestep-ui/tests/frame_budget.rs` | 177 | explicit |  | Phase index advanced only in timer |
| `desktop/aizome/sukumo/crates/sidestep-ui/tests/headless.rs` | 552 | machines | [UIW-32](#uiw-32) | step-polled null-backend driver |
| `desktop/aizome/sukumo/crates/sidestep-ui/tests/image_budget.rs` | 123 | none |  | tests; timing loop |
| `desktop/aizome/sukumo/crates/sidestep-ui/tests/offscreen.rs` | 95 | machines | [UIW-32](#uiw-32) | drawn-then-quit driver |
| `desktop/aizome/sukumo/crates/sidestep-ui/tests/scrolling.rs` | 178 | machines | [UIW-32](#uiw-32) | step-polled null-backend driver |
| `desktop/aizome/sukumo/crates/sidestep-ui/tests/support/pointer.rs` | 114 | machines | [UIW-33](#uiw-33) | virtual pointer command thread |
| `desktop/aizome/sukumo/crates/sidestep-ui/tests/wayland_desktop.rs` | 1413 | machines | [UIW-34](#uiw-34) | headless-sway scenario drivers |
| `desktop/aizome/sukumo/crates/sidestep-ui/tests/wayland_system.rs` | 364 | machines | [UIW-34](#uiw-34) | headless-sway scenario driver |
