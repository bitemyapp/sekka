# WM: Window manager logic and compositor integration tests

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/crates/aizome-comp-logic, desktop/aizome/crates/aizome-comp`. Audited at `8e0acbe5`; line numbers refer to that commit.

62 files, 0 pending. 38 implicit machines: 9 High, 8 Medium, 21 Low.

## Machines

<a id="wm-01"></a>

### WM-01 — Toplevel configure size negotiation (requested size until the client commits it)
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/lib.rs:105` (`WinData.requested_size`); also `desktop/aizome/crates/aizome-comp-logic/src/actions.rs:512` (`request_size`), `desktop/aizome/crates/aizome-comp-logic/src/actions.rs:520` (`configure`), `desktop/aizome/crates/aizome-comp-logic/src/actions.rs:991` (`window_committed`), `desktop/aizome/crates/aizome-comp-logic/src/lib.rs:123` (`Saved.requested_sizes`)
- **Encoding:** `requested_size: Option<(w, h)>` per window, smithay's per-surface configure bookkeeping (`pending_configures()`, `current_serial`, `configure_serial`) and the acked `current_state().size`; `Win.rect` holds what the model believes.
- **States:** `Settled` (None) → `Requested(w,h)` (re-sent with every configure while set) → `ClientBehind` (commits of an older configure ignored) → `Settled`; representable invalid: `Requested` that no configure ever carried.
- **Transitions:** `request_size` →Requested (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:514`); `configure` puts it in the pending state and sends (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:529-530`, `:555-557`, `:564-566`); commit of an older serial or a mismatched acked size stays (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1019-1033`); latest committed →Settled (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1035`) and the client's size is adopted and pushed to group members (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1050-1054`); unmap drops it (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1078`); reload carries it (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:84`, `desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:66-67`).
- **Hazards:** every caller must pair `request_size` with `configure`; `adopt_windows` → `place_new_window` can set it (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1241`, `desktop/aizome/crates/aizome-comp-logic/src/actions.rs:900`) with no configure in that path, so the client's commits are ignored (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1024-1033`) until some unrelated configure goes out; while set, every focus/park configure repeats the size; feeds WM-04's anchor.
- **Reify:** Medium — centralised in two functions, but correctness rests on every caller pairing the calls and on smithay serial internals.
- **Related:** the test client's ack/commit side in `desktop/aizome/crates/aizome-comp/tests/common/mod.rs:905`.

<a id="wm-02"></a>

### WM-02 — Motion timelines (Desk slides, tab slides, overlay entrances)
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/anim.rs:54` (`Animations.desks`, `.tabs`, `.overlays`); also `desktop/aizome/crates/aizome-comp-logic/src/anim.rs:12` (`DeskSlide`), `desktop/aizome/crates/aizome-comp-logic/src/anim.rs:35` (`Slide`)
- **Encoding:** presence in the vec/maps means running; each entry's `start: Instant` + `dur`; desk slides keyed by output name, tabs by window, overlays by name.
- **States:** `Idle` → `Running` → `Done` (still stored until the next tick) → removed.
- **Transitions:** `start_desk` replaces a display's slide (`desktop/aizome/crates/aizome-comp-logic/src/anim.rs:89`, `:94`) from `desks_moved` (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:654`); `start_tab` (`desktop/aizome/crates/aizome-comp-logic/src/anim.rs:104`, `:107`) from `desktop/aizome/crates/aizome-comp-logic/src/actions.rs:392`, `desktop/aizome/crates/aizome-comp-logic/src/actions.rs:449`, `desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:310`; `start_overlay` (`desktop/aizome/crates/aizome-comp-logic/src/anim.rs:110-117`) from `desktop/aizome/crates/aizome-comp-logic/src/overlays/overview.rs:98`, `desktop/aizome/crates/aizome-comp-logic/src/overlays/switcher.rs:95`; `tick` retires (`desktop/aizome/crates/aizome-comp-logic/src/anim.rs:69-75`) only from the scene hook (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:247`); unmap drops a tab slide (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1077`); quiesce drops all (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:96`).
- **Hazards:** `Done` is only noticed while frames are drawn, and `animating` is published from `active()` (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:115`), so the core keeps asking for frames until one renders; overlay entrances outlive a closed overlay; `is_hidden_by_animation` is a stub (`desktop/aizome/crates/aizome-comp-logic/src/anim.rs:82-84`).
- **Reify:** Low — small, time-derived and self-retiring.

<a id="wm-03"></a>

### WM-03 — Window move and tab-drag gesture
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:135` (`MoveGrab.tab`, `.pending_tear`, `.moved`, `.start_rect`); also `desktop/aizome/crates/aizome-comp-logic/src/input.rs:36` (`Interactive.drag`), `desktop/aizome/crates/aizome-comp-logic/src/input.rs:45` (`TabDrag.target`, `.at`, `.slot`)
- **Encoding:** grab-local bools and rect, plus the shared `Logic.interactive.drag` that the decoration layout and drop feedback read (`desktop/aizome/crates/aizome-comp-logic/src/render.rs:245-250`, `desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:223`); core `cursor_override`.
- **States:** `Pressed` (moved=false) → `PendingTear` (background or ⌥ tab, under 8pt) → `Torn` (unstacked) → `Moving` (+ `Hovering(target, at, slot)` for tabs) → `Dropped` → `Stacked` | `AdoptedByDisplay` | `MemberShown` (a click); representable invalid: `interactive.drag` set with no move grab installed.
- **Transitions:** `start_move` (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:161-173`, drag set `:170`); first motion past 2pt (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:189`); tear (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:195-206`); move (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:215`); hover target and slot (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:298`, `:302`); release or repeated press of its button ends it (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:223-241`: drag taken `:226`, show member `:230`, `stack_at` `:233`, `adopt_screen` `:236`); `unset` clears the drag (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:246`); a closed window clears it (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:767-769`); quiesce wipes `interactive` (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:94`) before the core ends the grab (`desktop/aizome/crates/aizome-comp-core/src/logic.rs:423-424`); a client's xdg move enters it (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:548-556`).
- **Hazards:** gesture state split between the grab object and `Logic.interactive`; a press of the starting button ends the gesture without a drop (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:227`); after `window_gone` the grab keeps running on a dead id (each step no-ops).
- **Reify:** Medium — several phases over two owners, but the end condition is centralised (`ends`, `desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:125`).
- **Related:** the core's guarded grab slot `desktop/aizome/crates/aizome-comp-core/src/focus.rs:333` and `end_logic_grabs` `desktop/aizome/crates/aizome-comp-core/src/logic.rs:456`.

<a id="wm-04"></a>

### WM-04 — Interactive resize and its edge anchor
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:322` (`ResizeGrab.edges`, `.start_rect`); also `desktop/aizome/crates/aizome-comp-logic/src/lib.rs:103` (`WinData.resize_anchor`), `desktop/aizome/crates/aizome-comp-logic/src/lib.rs:109` (`ResizeAnchor`)
- **Encoding:** the grab object, a per-member `resize_anchor: Option<{left, top, start}>`, the toplevel's xdg `Resizing` pending state, and `requested_size` (WM-01).
- **States:** `Idle` → `Resizing` (grab live, anchor Some, `Resizing` set) → `Ended` (grab gone, `Resizing` unset) — with the anchor still `Some` in `Ended`.
- **Transitions:** `start_resize` sets `Resizing`, the anchor and placement Free (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:343-360`); motion requests sizes (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:404-407`); a commit applies the anchor (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1040-1047`); release ends (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:412-415`); `unset` unsets `Resizing` and configures (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:419-424`); nothing clears `resize_anchor` except unmap (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1078`) or a reload (not in `Saved`).
- **Hazards:** the anchor outlives the gesture and its `start` rect goes stale after moves and display changes; `unset` acts on the group as it is at the end, so a window that left the group mid-resize keeps `Resizing`.
- **Reify:** High — suspected bug; the invariant "anchor only while a resize grab runs" is not enforced anywhere.
- **Suspected bug:** resize a window from its left (or top) edge, then drag it elsewhere by its tab; the next commit whose size differs from the model's rect (an unfill or display relayout the client clamps to its minimum size, a client resizing itself) runs `r.x = a.start.right() - size.0` (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1041-1043`) and the window jumps back to where the old resize's right edge was, possibly on another display. (confidence: medium)
- **Reproduction:** Confirmed (end-to-end): [test and output](reproductions.md#wm-04)

<a id="wm-05"></a>

### WM-05 — Tab slide and close/zoom box grabs
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:433` (`TabSlideGrab.start_offset`); also `desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:501` (`BoxGrab.hit`), `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:64` (`Win.tab_offset`)
- **Encoding:** grab objects; the slide writes `tab_offset` on every group member.
- **States:** slide `Sliding` → ended; box `Armed(hit)` → `Fired` (released over the same box) | `Cancelled`.
- **Transitions:** `start_tab_slide` (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:439-449`); motion stores the clamped offset (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:463-480`); end (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:486-488`, `:491-494`); `start_box_click` (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:506-509`); release over the box closes or fills (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:527-537`).
- **Hazards:** none observed.
- **Reify:** Low — local, one owner each.

<a id="wm-06"></a>

### WM-06 — Logic instance lifecycle and reload handoff
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:56` (`init`), `:80` (`save`), `:91` (`quiesce`), `:104` (`release`); also `desktop/aizome/crates/aizome-comp-logic/src/lib.rs:121` (`Saved.model`, `.requested_sizes`, `.slides`, `.audible`), `desktop/aizome/crates/aizome-comp-logic/src/lib.rs:203` (`install`), `desktop/aizome/crates/aizome-comp-logic/src/lib.rs:209` (`uninstall`), `desktop/aizome/crates/aizome-comp-logic/src/lib.rs:172` (`MARK`), `desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1198` (`adopt_windows`)
- **Encoding:** the core's raw state slot plus an owner mark (address of a static), the JSON `Saved` blob, and which fields `Saved` carries (model, requested sizes, slides, audible) versus drops (overlays, grabs, anchors, caches, `generation`, `relaid`).
- **States:** `Absent` → `Installed` (init) → `Running` → `Quiesced` (overlays closed, input state cleared) → `Saved` → `Released`; quiesce alone (Reset Input) returns to `Running`.
- **Transitions:** `init` parses `Saved`, installs, adopts the core's windows and slides (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:59-75`, `desktop/aizome/crates/aizome-comp-logic/src/lib.rs:205`); `adopt_windows` forgets gone windows and places new ones (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1202-1246`); `quiesce` (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:92-101`); `save` (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:82-88`); `release` (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:105`, `desktop/aizome/crates/aizome-comp-logic/src/lib.rs:214`); every accessor checks the owner (`desktop/aizome/crates/aizome-comp-logic/src/lib.rs:187`, `:196`).
- **Hazards:** `quiesce` doubles as Reset Input (`desktop/aizome/crates/aizome-comp-core/src/recovery/reset.rs:83`), so what it drops without telling anyone is lost in a live session too (an IPC area pick, WM-12); at a reload the core releases no keys, so applied sticky modifiers are lost (WM-10); `Saved` fields need `#[serde(default)]` by convention only.
- **Reify:** Medium — the phases are fixed by the vtable but split between this crate and the core, and the saved/dropped split is implicit in `Saved`'s fields.
- **Related:** `desktop/aizome/crates/aizome-comp-core/src/logic.rs:420` (`stop_logic`), `desktop/aizome/crates/aizome-comp-core/src/recovery/reset.rs:83`.

<a id="wm-07"></a>

### WM-07 — View publication and change coalescing
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/lib.rs:89` (`Logic.published`); also `desktop/aizome/crates/aizome-comp-logic/src/lib.rs:75` (`Logic.dirty_windows`), `desktop/aizome/crates/aizome-comp-logic/src/lib.rs:91` (`Logic.scales_seen`), `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:192` (`Model.generation`), `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:196` (`Model.relaid`)
- **Encoding:** a model generation bumped by nearly every mutator (including `get_mut`, `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:335-338`), a memo of (generation, grabs, animating, keep), a dirty set of windows for IPC events, a queue of windows the model refitted, and a hash signature of outputs' scales.
- **States:** per channel `Clean` ⇄ `Dirty`; published view `Current` ⇄ `Stale`.
- **Transitions:** mutators bump `generation`; `publish` compares and stores the memo (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:122-126`) after draining `relaid` (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:112`, `desktop/aizome/crates/aizome-comp-logic/src/actions.rs:753`); `relayout`/`unpark_visible` queue (`desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:1061`, `:1241-1243`); `window_changed` marks (`desktop/aizome/crates/aizome-comp-logic/src/lib.rs:162`), `flush_ipc_events` drains each loop iteration (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:810`, `desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:296`); unmap unmarks (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1076`); `scales::refresh` memo (`desktop/aizome/crates/aizome-comp-logic/src/scales.rs:48`, `:57`).
- **Hazards:** a view change that bumps no generation and none of the three flags is never published; `generation` and `relaid` are `serde(skip)`, so refits queued but undrained at a reload are lost; the scale signature counts popups but not subsurfaces (`desktop/aizome/crates/aizome-comp-logic/src/scales.rs:79-81`), so a subsurface added later is not told until something else changes.
- **Reify:** Medium — four coalescing channels with different drain points.
- **Related:** `desktop/aizome/crates/aizome-comp-core/src/view.rs` (the core's copy of the view).

<a id="wm-08"></a>

### WM-08 — Key decision pipeline (bound action, translation, suppression)
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/lib.rs:87` (`Logic.pending_action`); also `desktop/aizome/crates/aizome-comp-logic/src/input.rs:276` (`decide_key`), core `suppressed_keys` and `translated_keys`
- **Encoding:** the action stashed between `decide_key` and the core calling `key_action`; core sets of keycodes whose press was swallowed or translated, consulted on release.
- **States:** per key `Up` → `Forwarded` | `Translated` | `Suppressed` (overlay or binding) → `Up`; per action `None` → `Pending(action)` → run.
- **Transitions:** overlay takes it (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:282-286`); binding →Pending + suppressed (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:322-324`); `key_action` takes and runs (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:218-219`); translation on press (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:327-329`) and on release (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:289-293`); quiesce clears the action (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:95`).
- **Hazards:** the release's translation is recomputed from the current modifiers and focus (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:723-743`), so a key pressed as a translated Ctrl chord is released untranslated, with the real modifiers, if ❖ came up or focus moved first; `pending_action` survives if the core never calls `key_action`.
- **Reify:** Low — two-phase by the core's design and short-lived.
- **Related:** `desktop/aizome/crates/aizome-comp-core/src/input.rs:446`, `desktop/aizome/crates/aizome-comp-core/src/input.rs:508`.

<a id="wm-09"></a>

### WM-09 — Slow keys
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/input.rs:777` (`Access.slow: HashMap<u32, Option<TimerId>>`); also `desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:278` (`TIMER_SLOW_KEY`)
- **Encoding:** a key absent (up), present with `Some(timer)` (waiting to count) or present with `None` (counted, held).
- **States:** `Up` → `Waiting` → `Counted` → `Up`; `Waiting` → `Up` (let go too soon, swallowed).
- **Transitions:** press schedules (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:249-251`); repeat ignored (`:245-247`); timer →Counted and delivers the press (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:269-272`, dispatched `desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:283`); early release cancels (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:254-256`); counted release falls through; setting off drains (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:258-262`); quiesce cancels (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:97-100`).
- **Hazards:** `None` means both "counted" and "the core refused the timer" (`schedule` returns `Option`), so a failed schedule silently counts nothing and later delivers a release without a press; a key waiting when the setting goes off or at a reload is dropped and its release passes through unpaired.
- **Reify:** Low — one map, one owner.

<a id="wm-10"></a>

### WM-10 — Sticky keys (latched modifiers)
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/input.rs:775` (`Access.alone`, `.latched`, `.applied`)
- **Encoding:** the modifier pressed with nothing else since, modifiers latched for the next key, and keys currently down with the synthetic modifier presses sent for them.
- **States:** per modifier `Idle` → `Alone` → `Latched` (tap) → `Idle` (tap again) ; per key `Up` → `AppliedDown(mods)` → `Up`.
- **Transitions:** modifier press →Alone (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:669`); release toggles latch (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:670-677`); other key clears Alone (`:680`); press with latches sends synthetic modifier presses →AppliedDown (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:681-688`); its release sends the key's then the modifiers' releases (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:690-697`); setting off clears `latched` and `alone` only (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:660-665`); quiesce drops all (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:97`).
- **Hazards:** `applied` holds synthetic presses the seat still believes are down; it is not drained when the setting turns off or when the logic is replaced.
- **Reify:** High — suspected bug; three fields with an unwritten "every applied press gets its release" invariant.
- **Suspected bug:** with sticky keys on, tap ⌃ and ❖ (both latch), then press R: the synthetic ⌃ and ❖ presses go out with R (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:681-688`), the core takes ⌃❖R and reloads at the end of that loop iteration while R is still down (`desktop/aizome/crates/aizome-comp-core/src/input.rs:518`); the reload drops `applied` (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:97`; unlike Reset Input it releases no keys), so R's release reaches the new logic as a plain release (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:698`) and ⌃ and ❖ stay pressed in the seat: the next letter typed arrives as a ⌃❖ chord (Q would lock the session). Turning sticky keys off while a latched chord is held does the same. (confidence: medium)
- **Reproduction:** Confirmed (end-to-end): [test and output](reproductions.md#wm-10)

<a id="wm-11"></a>

### WM-11 — Pointer interaction bookkeeping (hover, double-click, overlay-owned presses)
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/input.rs:31` (`Interactive.hover`, `.last_tab_click`, `.overlay_presses`)
- **Encoding:** the decoration under an ungrabbed pointer, the last tab press `(Instant, win, pos)`, and the buttons whose presses an overlay took.
- **States:** hover `None` ⇄ `Hit`; tab click `None` → `FirstClick(t)` → `Double` (within 400 ms and 4pt) → `None`; button `Normal` → `OverlayOwned` → released (swallowed).
- **Transitions:** hover set when ungrabbed (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:94`); first and double click (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:613-628`); overlay takes a press (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:137-139`), any event for that button clears it (`:134`), its release is swallowed (`:181-189`); window gone (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:764-766`); quiesce resets (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:94`).
- **Hazards:** a quiesce between an overlay's press and its release forwards that release to a client that never saw the press.
- **Reify:** Low — independent small flags.

<a id="wm-12"></a>

### WM-12 — Area picker (screenshots and IPC `pick-area`)
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/overlays/area.rs:20` (`AreaPick.start`, `.pos`, `.window_mode`, `.report`); also `desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:28` (`Overlays.area`)
- **Encoding:** `Option<AreaPick>` (up or not), `start: Option<(x,y)>` (banding), `window_mode`, `report: Option<Report>` (IPC requester's tag and allowed targets), the cursor override, and the IPC events `PickStarted`/`AreaPicked`.
- **States:** `Closed` → `Picking(area|window mode)` → `Banding` → `Picked` (screenshot or `AreaPicked{area}`) | `Cancelled` (`AreaPicked{area: None}`) | `Superseded`.
- **Transitions:** `start` (supersedes and reports the old pick, `desktop/aizome/crates/aizome-comp-logic/src/overlays/area.rs:65-69`; `PickStarted` `:71`; set `:75-76`); Space toggles mode (`desktop/aizome/crates/aizome-comp-logic/src/overlays/area.rs:214-219`); press starts a band (`:168-169`); release finishes (`desktop/aizome/crates/aizome-comp-logic/src/overlays/area.rs:170-195`) or keeps picking (`:177-179`); window-mode press picks (`desktop/aizome/crates/aizome-comp-logic/src/overlays/area.rs:141-164`); Esc and lock cancel with an event (`desktop/aizome/crates/aizome-comp-logic/src/overlays/area.rs:94-100`, `desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:97`); `close_all` drops it with no event (`desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:81`); IPC entry (`desktop/aizome/crates/aizome-comp-logic/src/requests.rs:259-262`).
- **Hazards:** no timeout and no owner liveness check for a reporting pick; three exits, only two of which tell the requester.
- **Reify:** High — suspected bug; a cross-process protocol whose terminal event depends on which exit runs.
- **Suspected bug:** while the share picker (or the shell's recording "pick an area") waits, a logic reload or Ctrl+Alt+Del → Reset Input runs `quiesce` → `close_all` (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:92`, `desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:81`), which drops the pick without `AreaPicked{area: None}`; `aizome-share-picker` loops on events forever and the shell's recording stays in its picking phase. (confidence: high)
- **Related:** `desktop/aizome/crates/aizome-ipc/src/bin/aizome-share-picker.rs:105-129`, `desktop/aizome/crates/aizome-shell/src/recording_glue.rs:39`.
- **Reproduction:** Confirmed (end-to-end): [test and output](reproductions.md#wm-12)

<a id="wm-13"></a>

### WM-13 — Marking menu
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/overlays/marking.rs:119` (`Marking.serial`, `.shown`, `.sticky`, `.keys`, `.sub`, `.hover`, `.desk_hover`, `.output`, `.center`, `.pos`); also `desktop/aizome/crates/aizome-comp-logic/src/overlays/marking.rs:336` (`MarkingGrab.moved`), `desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:31` (`Overlays.marking_opened`), `desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:277` (`TIMER_MARKING`)
- **Encoding:** five bools, two hover options, a per-opening serial checked by the pause timer, and a pointer grab that exists only for press-opened menus.
- **States:** `Closed` → `Pausing` (held, not drawn) → `Flicked` (verb runs undrawn) | `Shown` (held) → `Sticky` (quick click; or opened by a client request or keys) → `DeskRing` (`sub`) → `Picked` | `Closed`; `KeysMode` = shown+sticky+keys with no grab; representable invalid: `keys` with a grab, `sub` without hover on ToDesk.
- **Transitions:** `open` (`desktop/aizome/crates/aizome-comp-logic/src/overlays/marking.rs:148-181`, serial `:175-179`, timer `:181`); `open_from_keys` (`desktop/aizome/crates/aizome-comp-logic/src/overlays/marking.rs:197-218`); `pause_over` →Shown (`desktop/aizome/crates/aizome-comp-logic/src/overlays/marking.rs:224-233`); flick (`desktop/aizome/crates/aizome-comp-logic/src/overlays/marking.rs:307-320`); hover and ring (`:323-329`); grab release: quick click →Sticky (`desktop/aizome/crates/aizome-comp-logic/src/overlays/marking.rs:368-379`), ring wait (`:384-390`), pick (`:391-392`), sticky press elsewhere closes (`:393-400`); `unset` closes unless sticky and shown (`desktop/aizome/crates/aizome-comp-logic/src/overlays/marking.rs:486-490`); keys (`desktop/aizome/crates/aizome-comp-logic/src/overlays/marking.rs:506-533`); sticky clicks without a grab (`desktop/aizome/crates/aizome-comp-logic/src/overlays/marking.rs:638-659`, routed `desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:165-170`); window gone (`desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:209-211`); lock (`desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:100`).
- **Hazards:** mode logic spread over the grab, the overlay router and the key path; a closed menu leaves its grab running until the next event unsets it (`desktop/aizome/crates/aizome-comp-logic/src/overlays/marking.rs:355-357`, `:363-365`); keys reach it only once `shown` (`desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:119`).
- **Reify:** High — many flags, three entry paths and transitions in six handlers.

<a id="wm-14"></a>

### WM-14 — Overlay modality (which overlay owns input)
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:24` (`Overlays.marking`, `.switcher`, `.overview`, `.area`)
- **Encoding:** four independent `Option`s; precedence re-encoded in each router.
- **States:** `None` | `Area` | `Switcher` | `Overview` | `Marking`, and representable combinations (area with any other; marking with switcher or area).
- **Transitions:** opens: `desktop/aizome/crates/aizome-comp-logic/src/overlays/area.rs:75`, `desktop/aizome/crates/aizome-comp-logic/src/overlays/switcher.rs:83`, `desktop/aizome/crates/aizome-comp-logic/src/overlays/overview.rs:86` (closes switcher and marking, `:82-83`), `desktop/aizome/crates/aizome-comp-logic/src/overlays/marking.rs:148`; switcher refused under the overview (`desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:178-183`); `close_all` (`desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:76-84`); `close_for_lock` each loop iteration (`desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:92-102`, `desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:295`); `window_gone` (`desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:196-217`).
- **Hazards:** keys route area > switcher > overview > marking (`desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:110-121`) but buttons route area > overview > switcher > marking (`:153-170`) and scroll overview > switcher (`:132`); the marking menu is deliberately left out of `grabs_keyboard` (`:47`), so its keys pass a different test than the others'; `close_all` and `close_for_lock` close differently (WM-12).
- **Reify:** High — four Options with precedence duplicated in four places and reachable combinations.

<a id="wm-15"></a>

### WM-15 — Desks overview
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/overlays/overview.rs:26` (`Overview.output`, `.filter`, `.drag`, `.zoom`, `.wheel`, `.cards`, `.thumbs`, `.pointer`)
- **Encoding:** output it is bound to, filter text (empty = browse mode, non-empty = filter mode), thumbnail drag, zoom and wheel accumulator, and hit boxes from the last drawing.
- **States:** `Closed` → `Browsing` ⇄ `Filtering` → `Dragging(thumb)` → `Dropped` | `Activated` → `Closed`.
- **Transitions:** `toggle` (`desktop/aizome/crates/aizome-comp-logic/src/overlays/overview.rs:75-100`); Esc clears filter or closes (`:124-130`); typing (`:161-162`); Return activates the first match (`:133-141`); digits go to a Desk (`:142-153`); `p` parks (`desktop/aizome/crates/aizome-comp-logic/src/overlays/overview.rs:171-181`); wheel (`:187-191`); press on a thumb →Dragging (`:203`); card click shows or adds a Desk (`:209-220`); release (`desktop/aizome/crates/aizome-comp-logic/src/overlays/overview.rs:226-243`); drawing refreshes hit boxes and clamps zoom (`:314`, `:341-342`); window gone clears drag (`desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:212-216`); IPC (`desktop/aizome/crates/aizome-comp-logic/src/requests.rs:237-242`).
- **Hazards:** hit boxes are only as fresh as the last frame on its output; nothing closes or moves the overview when its output goes.
- **Reify:** High — suspected bug; modes keyed on filter emptiness plus an unchecked output binding.
- **Suspected bug:** unplug (or lid-close) the display showing the overview: `items` never matches the gone output (`desktop/aizome/crates/aizome-comp-logic/src/overlays/overview.rs:262-265`), so nothing is drawn, yet `grabs_keyboard` stays true, every key is swallowed (`:119-121`, `:166`) and every press is taken (`:223`) — the desktop looks normal but ignores input until a blind Esc. (confidence: medium)
- **Reproduction:** Confirmed (end-to-end): [test and output](reproductions.md#wm-15)

<a id="wm-16"></a>

### WM-16 — App switcher
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/overlays/switcher.rs:28` (`Switcher.apps`, `.app`, `.windows_tier`, `.win`, `.held`, `.output`, `.app_boxes`, `.win_boxes`)
- **Encoding:** a snapshot of apps and windows taken at open, two cursors, a tier flag, and `held` (release of ❖ picks).
- **States:** `Closed` → `AppsTier` ⇄ `WindowsTier` → `Activated` | `Cancelled` | `AppActed` (q/w/h).
- **Transitions:** `open` or step (`desktop/aizome/crates/aizome-comp-logic/src/overlays/switcher.rs:54-59`, `:83-93`); `step_app` (`:109-114`), `step_win` (`:116-129`); ❖ release when held (`desktop/aizome/crates/aizome-comp-logic/src/overlays/switcher.rs:152-155`); Return/Esc/q/w/h (`:168-194`); click (`:210-227`); window gone trims or closes (`desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:197-207`); lock closes without switching (`desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:98`).
- **Hazards:** a lost ❖ release leaves a held switcher grabbing the keyboard; bound to an output that may go (as WM-15); new windows are not added to the snapshot.
- **Reify:** Medium — indices and tier guarded by `window_gone`, but `held` depends on a release event arriving.

<a id="wm-17"></a>

### WM-17 — Notification card slides
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/slides.rs:77` (`Slides.coming`, `.going`, `.seen`); also `desktop/aizome/crates/aizome-comp-logic/src/slides.rs:95` (`SavedSlide`)
- **Encoding:** cards seen up (so each comes in once), slides in by surface id, slides out by the core's ghost id, carried over a reload as elapsed/duration; the core's `ghosts` list and `children.handover` flag.
- **States:** card `Unseen` → `ComingIn` → `InPlace` → (unmapped) `Ghost` → `GoingOut` → dropped.
- **Transitions:** `track` per drawn output (`desktop/aizome/crates/aizome-comp-logic/src/slides.rs:209-239`: seen and start `:210-214`, forget `:217`, ghost start/drop `:221-236`); `save`/`restore` (`desktop/aizome/crates/aizome-comp-logic/src/slides.rs:112-138`, `:142-168`); `keep_on_unmap` published (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:139`).
- **Hazards:** progress only advances when the card's output is drawn, so ghosts for an output that went are never dropped by the logic; a card that briefly loses its buffer slides in again.
- **Reify:** Medium — three maps across a process-independent ghost list and a reload.
- **Related:** `desktop/aizome/crates/aizome-shell/src/notifications/cards.rs`, `desktop/aizome/crates/aizome-comp-core/src/ghosts.rs`.

<a id="wm-18"></a>

### WM-18 — Window geometry mode (placement, restore, display home)
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:52` (`Win.placement`); also `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:54` (`Win.restore`), `:72` (`Win.frame`), `:74` (`Win.home`), `:25` (`enum Placement`), `:82` (`Home`)
- **Encoding:** partially explicit: the `Placement` enum, with `restore` (free rect to return to), `frame` (usable area it was laid out in) and `home` (place before its Desk moved to another display) held beside it.
- **States:** `Free` ⇄ `Fill` | `SplitLeft` | `SplitRight` | `Fullscreen` (restore Some), each `AtHome` or `Away(home)`; invalid but representable: `Free` with `restore` Some, a non-Free placement with `restore` None.
- **Transitions:** `set_placement` (`desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:728-749`) via `set_fill`/`set_split`/`set_fullscreen` (`:692-724`); moves and resizes force Free (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:246-247`, `:267-268`, `desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:355-357`); `stack_at` copies a's mode to the joiners (`desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:841-853`); `relayout` scales from `home` (`desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:1220-1239`); `set_rect`/`set_frame`/`adopt_into_screen`/`set_sticky` forget `home` (`:660`, `:686`, `:1269`, `:1367`); refit on usable-area changes (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1152-1191`); client fullscreen and maximize requests (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:909-923`, `:1097-1098`).
- **Hazards:** the restore invariant is kept by convention across six writers, five of them outside `set_placement` writing the fields directly; group members must stay identical by hand.
- **Reify:** High — enum plus three dependent fields mutated from three files.

<a id="wm-19"></a>

### WM-19 — Focus and visibility
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:181` (`Model.focused`); also `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:174` (`Model.mru`), `:180` (`Model.current`), `:55` (`Win.minimised`), `:57` (`Win.hidden`), `:59` (`Win.urgent`), `:49` (`Win.sticky`)
- **Encoding:** focused window, focus history, current Desk, per-window visibility flags, and the `Refocus` value mutators return for the caller to apply; the core's `layer_focus` and popup grab are side state.
- **States:** window `Shown` ⇄ `Minimised` | `Hidden` | `InactiveTab` | `OffDesk`; focus `None` | `Focused(w)`; `Urgent` until focused; invariant "focused is shown on the focused display" (representable broken: `Model::focus` accepts any window, `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:461-485`).
- **Transitions:** `focus` (`desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:464`, `:475-484`), `reveal` (`:496-509`), `minimise`/`hide_app`/`hide_others` with `refocus_if_hidden` (`:565-631`), `remove` (`:319-325`), `lower` (`:539-544`); `focus_window` applies side effects (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:87-130`); `apply_refocus` (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:133-139`); urgent on an invalid activation (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1139`); direct model focus bypassing side effects (`desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:868`, `desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1244`); settings re-check (`desktop/aizome/crates/aizome-comp-logic/src/config.rs:28-32`).
- **Hazards:** each mutator returns a `Refocus` the caller must remember to apply; adopted focus after a reload (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1243-1245`) sends no activated configure.
- **Reify:** High — invariant spread over a dozen mutators and two layers.

<a id="wm-20"></a>

### WM-20 — Tab groups
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:136` (`Group.members`, `.active`); also `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:62` (`Win.group`), `:175` (`Model.groups`)
- **Encoding:** a group map plus a back-pointer per window; a group of one is dissolved.
- **States:** `Ungrouped` → `Member(active | background)` → `Ungrouped`; invariants: two or more members, `active` ∈ members, back-pointers match.
- **Transitions:** `stack_at` creates or joins (`desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:827-857`); `leave_group` picks a neighbour and dissolves at one (`:884-905`); `set_group_active`/`focus`/`reveal` set `active` (`:908-915`, `:473`, `:504`); `reorder_tab` (`:941-948`); `remove` (`:305-308`).
- **Hazards:** `stack_at` copies geometry and Desk to joiners but the group's shared state is otherwise duplicated per member.
- **Reify:** Medium — centralised in the model but with back-pointer invariants.

<a id="wm-21"></a>

### WM-21 — Displays and their Desks
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:185` (`Model.screens`); also `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:188` (`Model.homes`), `:180` (`Model.current`), `:96` (`Screen.desk`)
- **Encoding:** each display's Desk, each output name's last chosen Desk, and the current Desk (the focused display's).
- **States:** `NoDisplays` (current is the only Desk shown) → `Displays(assignment)`; invariants: a Desk on at most one display, `current` shown when any display exists.
- **Transitions:** `show_desk` swap or take (`desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:1013-1029`); `sync_screens` (`:1130-1197`, new Desk when short `:1163-1165`); `remove_desk` (`:1403-1413`); `adopt_into_screen` (`:1273`); `focus` (`:481-483`); `sync_screens` wrapper with the shell-restart hold (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:710-748`); `desks_moved` side effects (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:642-673`).
- **Hazards:** `homes` is never pruned of output names; `sync_screens` creates Desks that the settings sync later trims.
- **Reify:** High — three fields, many mutators, and display hot-plug ordering.

<a id="wm-22"></a>

### WM-22 — Desk parking (suspended Desks)
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:152` (`Desk.parked`); also `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:154` (`Desk.left`)
- **Encoding:** `parked: Option<u64>` (when), `left: Option<u64>`, and the xdg `Suspended` state each window was last configured with.
- **States:** `Awake` ⇄ `Parked`; invariant: a visible Desk is awake, and a window's `Suspended` matches its Desk.
- **Transitions:** `set_parked` (`desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:1289-1299`) from `set_desk_parked`, which configures the Desk's windows (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:349-357`), from the overview's `p` (`desktop/aizome/crates/aizome-comp-logic/src/overlays/overview.rs:180`); `unpark_visible` (`desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:1052-1064`); `configure` writes `Suspended` (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:525`, `:546`); `move_to_desk` reconfigures (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:311-313`); `mark_left` (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:648-650`).
- **Hazards:** `remove_desk` (via IPC `desktop/aizome/crates/aizome-comp-logic/src/requests.rs:171-192` or settings `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:1474-1478`) moves windows onto a parked heir with no configure, so they keep drawing while their Desk reads parked.
- **Reify:** Medium — one field, but its client-facing mirror is updated by hand.

<a id="wm-23"></a>

### WM-23 — (test harness) frame-time benchmark driver
- **Where:** `desktop/aizome/crates/aizome-comp/examples/frame_bench.rs:19` (`Kill`); also `desktop/aizome/crates/aizome-comp/examples/frame_bench.rs:83` (`kill`)
- **Encoding:** a kill-on-drop list of the compositor and its clients, scenario gating by name, and sleeps.
- **States:** `Starting` (poll IPC, 20 s) → `Clients(n)` (poll window count) → scenario phases → `Torn down`.
- **Transitions:** spawn (`desktop/aizome/crates/aizome-comp/examples/frame_bench.rs:57-83`); wait for IPC (`:85-89`); wait for windows (`:102-113`); per-scenario stats resets (`:146-337`); early stop when no later scenario is wanted (`:230-234`); settings swaps (`:286-287`, `:314-315`); drop (`:339`).
- **Hazards:** scenario boundaries are fixed sleeps; later scenarios assume earlier ones ran (`windows(11)` after popping clients, `:301`, `:322`).
- **Reify:** Low — a benchmark script.

<a id="wm-24"></a>

### WM-24 — (test harness) sidestep test client modes
- **Where:** `desktop/aizome/crates/aizome-comp/examples/sidestep_client.rs:31` (`Client.mode`, `.flipped`, `.flip`, `.unlock`, `.quit`, `.close`, `.tick`, `.frame`)
- **Encoding:** a mode string (rewritten after unlocking) and one `Option<TimerId>` per phase.
- **States:** `toplevel`: `Green` → `Magenta`; `lock`: `Locking` → `Locked` → `Unlocked(new window)` → `Quit`; `note`: `Shown` → `Closed`.
- **Transitions:** `launched` (`desktop/aizome/crates/aizome-comp/examples/sidestep_client.rs:44-93`); `session_locked` (`:95-101`); timers (`:103-141`, mode rewrite `:127`).
- **Hazards:** none observed.
- **Reify:** Low — a fixture.

<a id="wm-25"></a>

### WM-25 — (test harness) raw Wayland probe clients recording protocol events
- **Where:** `desktop/aizome/crates/aizome-comp/tests/chromium.rs:27` (`Heard.configure`, `.decoration`, `.token`, `.icon_manager_done`); also `desktop/aizome/crates/aizome-comp/tests/idle_inhibit.rs:22` (`Heard.idled`, `.resumed`), `desktop/aizome/crates/aizome-comp/tests/pacing.rs:29` (`Probe.configured`, `.released`), `desktop/aizome/crates/aizome-comp/tests/protocols.rs:30` (`Probe` drag-and-drop fields), `desktop/aizome/crates/aizome-comp/tests/protocols.rs:457` (`Gamma`), `desktop/aizome/crates/aizome-comp/tests/scales.rs:24` (`Heard.fractional`, `.buffer_scale`)
- **Encoding:** `Option`/bool/counter fields set by `Dispatch` handlers, polled by deadline loops that roundtrip.
- **States:** per protocol: `Unconfigured` → `Configured`; buffer `Sent` → `Released(at)`; DnD `Offered` → `Entered` → `Dropped` → `Sent` → `Performed+Finished`; idle `Active` → `Idled` → `Resumed`.
- **Transitions:** `desktop/aizome/crates/aizome-comp/tests/chromium.rs:278`, `:293`, `:308`, `:323`; `desktop/aizome/crates/aizome-comp/tests/idle_inhibit.rs:154-155`; `desktop/aizome/crates/aizome-comp/tests/pacing.rs:89`, `:319`, `:350`; `desktop/aizome/crates/aizome-comp/tests/protocols.rs:379`, `:394-404`, `:424-425`, `:441-445`, `:508-509`; `desktop/aizome/crates/aizome-comp/tests/scales.rs:165`, `:180`, `:210`; waits `desktop/aizome/crates/aizome-comp/tests/chromium.rs:66-75`, `desktop/aizome/crates/aizome-comp/tests/idle_inhibit.rs:76-80`, `desktop/aizome/crates/aizome-comp/tests/pacing.rs:115-130`, `desktop/aizome/crates/aizome-comp/tests/protocols.rs:97-111`, `desktop/aizome/crates/aizome-comp/tests/scales.rs:47-59`.
- **Hazards:** `expect_error` loops on `roundtrip` with only an after-the-fact deadline check (`desktop/aizome/crates/aizome-comp/tests/chromium.rs:93-95`); fixed `run_for` windows stand in for "nothing happened" (`desktop/aizome/crates/aizome-comp/tests/idle_inhibit.rs:71`, `:87`).
- **Reify:** Low — test probes.

<a id="wm-26"></a>

### WM-26 — (test harness) external client processes and poll-until loops
- **Where:** `desktop/aizome/crates/aizome-comp/tests/common/desktop.rs:9` (`Program`); also `desktop/aizome/crates/aizome-comp/tests/sidestep.rs:21` (`Program`), `desktop/aizome/crates/aizome-comp/tests/reload.rs:119` (`Proc`), `desktop/aizome/crates/aizome-comp/tests/recovery.rs:434` (`ChildApp`), `desktop/aizome/crates/aizome-comp/tests/portal.rs:18` (`chooser`)
- **Encoding:** child handles killed on drop; `wait`/`wait_px`/`wait_for` loops with deadlines.
- **States:** `Spawned` → `Ready` (polled) → `Killed` on drop | `Exited`.
- **Transitions:** spawn (`desktop/aizome/crates/aizome-comp/tests/common/desktop.rs:37-47`, `desktop/aizome/crates/aizome-comp/tests/sidestep.rs:36-49`, `desktop/aizome/crates/aizome-comp/tests/reload.rs:135-155`, `desktop/aizome/crates/aizome-comp/tests/recovery.rs:443-456`); waits (`desktop/aizome/crates/aizome-comp/tests/common/desktop.rs:49-55`, `desktop/aizome/crates/aizome-comp/tests/sidestep.rs:51-65`, `desktop/aizome/crates/aizome-comp/tests/reload.rs:197-203`); kill on drop (`desktop/aizome/crates/aizome-comp/tests/common/desktop.rs:11-16`, `desktop/aizome/crates/aizome-comp/tests/recovery.rs:436-441`).
- **Hazards:** the re-executed child test loops forever until killed (`desktop/aizome/crates/aizome-comp/tests/recovery.rs:466-468`), so a parent killed outright leaves it running; the portal chooser child has no kill guard (`desktop/aizome/crates/aizome-comp/tests/portal.rs:19-27`).
- **Reify:** Low — test process plumbing.

<a id="wm-27"></a>

### WM-27 — (test harness) shell-like layer and popup client
- **Where:** `desktop/aizome/crates/aizome-comp/tests/common/layer.rs:182` (`Shell.surfaces`, `.serial`); also `desktop/aizome/crates/aizome-comp/tests/common/layer.rs:98` (`ShellSeen.focus`, `.shown`, `.frames`, `.dismissed`, `.pointer`, `.buttons`)
- **Encoding:** named surfaces, latest input serial for popup grabs, and observations shared with the test thread.
- **States:** surface `Created` → `Configured+Drawn` → `Dismissed`/`Closed` → destroyed; focus and pointer `None` ⇄ `On(name)`.
- **Transitions:** commands (`desktop/aizome/crates/aizome-comp/tests/common/layer.rs:243-307`, grab with serial `:290-292`); configure draws (`:456-467`, `:483-486`); `Closed`/`PopupDone` destroy (`:468`, `:508-511`); enter/leave (`:401-405`, `:436-440`); serials (`:407`, `:429-431`); destroy clears focus (`:204-225`).
- **Hazards:** a popup asked for before any input uses serial 0 (`desktop/aizome/crates/aizome-comp/tests/common/layer.rs:291`), deliberately.
- **Reify:** Low — fixture.

<a id="wm-28"></a>

### WM-28 — (test harness) headless compositor process
- **Where:** `desktop/aizome/crates/aizome-comp/tests/common/mod.rs:36` (`Compositor.child`, `.dir`, `.socket`, `.ipc`, `.log`)
- **Encoding:** a child process, a private runtime directory, and the IPC socket path.
- **States:** `Spawned` → `Answering` (Version over IPC within 20 s) → `Killed`, with the directory removed or kept on panic.
- **Transitions:** `start` (`desktop/aizome/crates/aizome-comp/tests/common/mod.rs:73-138`, poll `:128-136`); `wait_windows` (`:169-181`); drop (`:213-223`).
- **Hazards:** `settle` is a fixed 120 ms sleep standing in for "frames done" (`desktop/aizome/crates/aizome-comp/tests/common/mod.rs:204-206`).
- **Reify:** Low — test harness.

<a id="wm-29"></a>

### WM-29 — (test harness) solid-colour toplevel test client
- **Where:** `desktop/aizome/crates/aizome-comp/tests/common/mod.rs:540` (`App.pending_size`, `.size`, `.needs_draw`, `.configured`, `.quit`, `.ti_serial`, `.ti_commit`); also `desktop/aizome/crates/aizome-comp/tests/common/mod.rs:438` (`Seen`), `desktop/aizome/crates/aizome-comp/tests/common/mod.rs:355` (`Spec.defer_mapping`, `.configure_delay`)
- **Encoding:** a client thread driven by a command channel, with configure, draw and text-input bookkeeping.
- **States:** `Created` → `Configured` → `Mapped` (or `Deferred` until `Cmd::Map`) → `Drawing` (frame-paced if animated) → `Quit`; text input `Disabled` ⇄ `Enabled(serial)`.
- **Transitions:** commands (`desktop/aizome/crates/aizome-comp/tests/common/mod.rs:623-657`); toplevel configure stores size (`:982`); surface configure acks and draws (`:905-930`); `draw` (`:684-694`); frame callback (`:807-808`); close (`:991-994`); text input serial (`:768-791`).
- **Hazards:** `configure_delay` sleeps in the dispatch thread (`desktop/aizome/crates/aizome-comp/tests/common/mod.rs:906`).
- **Reify:** Low — fixture.

<a id="wm-30"></a>

### WM-30 — (test harness) session-lock client
- **Where:** `desktop/aizome/crates/aizome-comp/tests/common/mod.rs:1024` (`LockClient.locked: Option<bool>`)
- **Encoding:** `None` (asked), `Some(true)` (locked), `Some(false)` (finished).
- **States:** `Requested` → `Locked` | `Finished` → `Unlocked`/`Destroyed`.
- **Transitions:** lock and surfaces (`desktop/aizome/crates/aizome-comp/tests/common/mod.rs:1119-1128`); events (`:1224-1225`); surface configure draws (`:1240-1247`); exit unlocks only if locked (`:1138-1142`).
- **Hazards:** none observed.
- **Reify:** Low — fixture.
- **Related:** the compositor's session-lock handler in `desktop/aizome/crates/aizome-comp-core/src/shell.rs`.

<a id="wm-31"></a>

### WM-31 — (test harness) text-field app with a grabbing context menu
- **Where:** `desktop/aizome/crates/aizome-comp/tests/common/text.rs:125` (`App.menu`, `.pointer`, `.serial`); also `desktop/aizome/crates/aizome-comp/tests/common/text.rs:49` (`FieldSeen`)
- **Encoding:** an optional popup, the latest input serial, and text-input enable state.
- **States:** menu `Closed` → `Requested(grab serial)` → `Shown` → `Dismissed`/`Closed`.
- **Transitions:** right press opens (`desktop/aizome/crates/aizome-comp/tests/common/text.rs:399-407`, `:161-177`); configure shows (`:331-334`); `PopupDone` (`:347-350`); close (`:179-191`); serials (`:364-371`, `:391-394`); text input enable/disable (`:427-445`).
- **Hazards:** none observed.
- **Reify:** Low — fixture.

<a id="wm-32"></a>

### WM-32 — (test harness) ibus-like input method with keyboard grab
- **Where:** `desktop/aizome/crates/aizome-comp/tests/common/text.rs:514` (`Ime.grab`, `.holds`, `.active`, `.pending`); also `desktop/aizome/crates/aizome-comp/tests/common/text.rs:454` (`ImeSeen.active`, `.activations`, `.grabbed`)
- **Encoding:** a pending activation applied on `done`, an optional keyboard grab, and a `grabbed` flag that flips only once a sync confirms the grab or release.
- **States:** `Inactive` → `Active+Grabbed` → `Inactive` (released, or still grabbed when `holds`).
- **Transitions:** activate/deactivate pending (`desktop/aizome/crates/aizome-comp/tests/common/text.rs:567-568`); `done` applies (`:571-601`); sync confirms (`:616-618`); ready handshake (`:492`, `:533-534`); release on exit (`:536-538`).
- **Hazards:** none observed.
- **Reify:** Low — fixture.

<a id="wm-33"></a>

### WM-33 — (test harness) screen-capture clients (image-copy-capture and wlr-screencopy)
- **Where:** `desktop/aizome/crates/aizome-comp/tests/image_copy_capture.rs:41` (`Heard.constraints`, `.stopped`, `.ready`, `.failed`, `.answered`); also `desktop/aizome/crates/aizome-comp/tests/image_copy_capture.rs:82` (`Capturer.target`), `desktop/aizome/crates/aizome-comp/tests/screencopy.rs:26` (`Heard.buffer_done`, `.ready`, `.failed`), `desktop/aizome/crates/aizome-comp/tests/screencopy.rs:45` (`Copier.target`)
- **Encoding:** a constraints counter used as a generation, per-frame flags reset before each capture, and the in-flight shm target.
- **States:** session `Negotiating` → `Constrained(n)` → `Stopped`; frame `Idle` → `Capturing(target)` → `Ready` | `Failed(reason)`.
- **Transitions:** `desktop/aizome/crates/aizome-comp/tests/image_copy_capture.rs:120-128`, `:172-188`, `:193-215`, `:218-223`, `:506-514`, `:530-540`; `desktop/aizome/crates/aizome-comp/tests/screencopy.rs:82-91`, `:118-134`, `:138-161`, `:306-315`.
- **Hazards:** one `Heard` serves every frame, so starting a second frame resets the first's flags (`desktop/aizome/crates/aizome-comp/tests/image_copy_capture.rs:182`); `answered` compensates per frame id.
- **Reify:** Low — fixture.

<a id="wm-34"></a>

### WM-34 — (test harness) input method counting its serial as IBus does
- **Where:** `desktop/aizome/crates/aizome-comp/tests/input_method.rs:26` (`ImSeen.bound`, `.active`, `.serial`, `.globals`); also `desktop/aizome/crates/aizome-comp/tests/input_method.rs:88` (`Im.pending_active`)
- **Encoding:** a `done` counter reset by every registry global, used as the commit serial.
- **States:** `Binding` → `Bound` → `Active` ⇄ `Inactive`; serial `n` → `0` on each new global.
- **Transitions:** bound (`desktop/aizome/crates/aizome-comp/tests/input_method.rs:103`); global resets (`:147-149`); activate/deactivate/done (`:165-172`); commit uses the serial (`:106-109`).
- **Hazards:** none observed (the reset is the regression under test).
- **Reify:** Low — fixture.

<a id="wm-35"></a>

### WM-35 — (test harness) recovery probe client (freeze, pointer lock, late move)
- **Where:** `desktop/aizome/crates/aizome-comp/tests/recovery.rs:567` (`PApp.configured`, `.move_after_release`, `.pressed_serial`, `.quit`); also `desktop/aizome/crates/aizome-comp/tests/recovery.rs:497` (`PSeen.locked`, `.inhibited`, `.armed`, `.moved_with`), `desktop/aizome/crates/aizome-comp/tests/recovery.rs:627` (`frozen`)
- **Encoding:** a frozen flag that stops reading the connection, an armed late-move with the last press serial, and observed lock/inhibit states.
- **States:** `Running` ⇄ `Frozen`; late move `Idle` → `Armed` → `Pressed(serial)` → `MovedLate`; pointer `Free` ⇄ `Locked`.
- **Transitions:** commands (`desktop/aizome/crates/aizome-comp/tests/recovery.rs:629-656`); late move on release (`:832-845`); lock and inhibitor events (`:859-860`, `:873-874`); configure (`:770-773`).
- **Hazards:** none observed.
- **Reify:** Low — fixture.

<a id="wm-36"></a>

### WM-36 — (test harness) logic variant builds for reload tests
- **Where:** `desktop/aizome/crates/aizome-comp/tests/reload.rs:25` (`Build.aizome`, `.base`, `.red`); also `desktop/aizome/crates/aizome-comp/tests/reload.rs:425` (`ShellBuild`)
- **Encoding:** a process-wide `OnceLock`, build outputs kept in `target/reload-test` between runs, and an ABI-mark comparison.
- **States:** `Unbuilt` → `Built(plain)` → `Built(red)` → `Rebuilt(plain)` → `Consistent` | retry (up to 3).
- **Transitions:** `build` (`desktop/aizome/crates/aizome-comp/tests/reload.rs:33-79`: retry loop `:65-76`); `shell_build` (`:430-447`).
- **Hazards:** after three mismatches the loop exits without failing (`desktop/aizome/crates/aizome-comp/tests/reload.rs:65-77`), so tests proceed with libraries from different cores; the plain library is overwritten in place by each variant build.
- **Reify:** Low — test build cache.

<a id="wm-37"></a>

### WM-37 — (test harness) shell restart scenario with a private D-Bus
- **Where:** `desktop/aizome/crates/aizome-comp/tests/reload.rs:452` (`Bus._daemon`, `.dir`, `.address`); also `desktop/aizome/crates/aizome-comp/tests/reload.rs:562` (`restarting_the_shell_keeps_apps_and_notifications`)
- **Encoding:** a dbus-daemon child, the shell's reported state, the notifications name owner's pid, and a compositor log line marking the end of the restart.
- **States:** `ShellUp` → `AppsUp` → `Restarting` (two shells overlap) → `NewShellOwnsName` → `NotesCarried`.
- **Transitions:** waits (`desktop/aizome/crates/aizome-comp/tests/reload.rs:580-583`, `:591-595`, `:623-625`); restart request (`:642-651`); screenshot loop until the log says done (`:659-670`); takeover (`:674-678`); notes (`:680-689`); bus cleanup (`:458-462`).
- **Hazards:** the restart phase ends on a log string (`desktop/aizome/crates/aizome-comp/tests/reload.rs:660`); the loop bound is a screenshot count, not a time (`:669`).
- **Reify:** Low — scenario test.
- **Related:** the core's shell handover, `desktop/aizome/crates/aizome-comp-core/src/session.rs:50` (`children.handover`).

<a id="wm-38"></a>

### WM-38 — (test harness) udev backend VM scenario
- **Where:** `desktop/aizome/crates/aizome-comp/vm/udev.nix:52` (`testScript`)
- **Encoding:** NixOS test-driver steps.
- **States:** `Booted` → `LoggedIn` → `CompositorUp` (IPC socket) → `TerminalOpen` → `Typed`.
- **Transitions:** `desktop/aizome/crates/aizome-comp/vm/udev.nix:53-54`, `:57-58`, `:63-64`, `:70-71`.
- **Hazards:** a fixed 2 s sleep after login (`desktop/aizome/crates/aizome-comp/vm/udev.nix:55`) and after the window opens (`:65`).
- **Reify:** Low — VM script.

## Already explicit

- None in this slice.

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/crates/aizome-comp-logic/build.rs` | 44 | none |  | build-time source hash for the build id |
| `desktop/aizome/crates/aizome-comp-logic/src/actions.rs` | 1249 | machines | [WM-01](#wm-01), [WM-02](#wm-02), [WM-03](#wm-03), [WM-04](#wm-04), [WM-06](#wm-06), [WM-07](#wm-07), [WM-18](#wm-18), [WM-19](#wm-19), [WM-21](#wm-21), [WM-22](#wm-22) | WM actions; home of WM-01 |
| `desktop/aizome/crates/aizome-comp-logic/src/anim.rs` | 154 | machines | [WM-02](#wm-02) | motion timelines |
| `desktop/aizome/crates/aizome-comp-logic/src/bindings.rs` | 439 | none |  | chord parsing and default binding table |
| `desktop/aizome/crates/aizome-comp-logic/src/cmdkey.rs` | 40 | none |  | pure predicates |
| `desktop/aizome/crates/aizome-comp-logic/src/config.rs` | 35 | machines | [WM-19](#wm-19) | settings reaction; refocus after Desk sync |
| `desktop/aizome/crates/aizome-comp-logic/src/decor.rs` | 528 | none |  | pure decoration layout and hit testing |
| `desktop/aizome/crates/aizome-comp-logic/src/frost.rs` | 61 | none |  | stateless scene filter |
| `desktop/aizome/crates/aizome-comp-logic/src/grabs.rs` | 580 | machines | [WM-03](#wm-03), [WM-04](#wm-04), [WM-05](#wm-05), [WM-02](#wm-02), [WM-18](#wm-18) | pointer grabs |
| `desktop/aizome/crates/aizome-comp-logic/src/hooks.rs` | 299 | machines | [WM-06](#wm-06), [WM-07](#wm-07), [WM-01](#wm-01), [WM-02](#wm-02), [WM-03](#wm-03), [WM-08](#wm-08), [WM-09](#wm-09), [WM-10](#wm-10), [WM-11](#wm-11), [WM-12](#wm-12), [WM-13](#wm-13), [WM-14](#wm-14), [WM-17](#wm-17) | vtable hooks; logic lifecycle and publish memo |
| `desktop/aizome/crates/aizome-comp-logic/src/input.rs` | 784 | machines | [WM-08](#wm-08), [WM-09](#wm-09), [WM-10](#wm-10), [WM-11](#wm-11), [WM-03](#wm-03) | input policy, slow and sticky keys |
| `desktop/aizome/crates/aizome-comp-logic/src/lib.rs` | 248 | machines | [WM-01](#wm-01), [WM-04](#wm-04), [WM-06](#wm-06), [WM-07](#wm-07), [WM-08](#wm-08) | Logic state fields and Saved; entries live with their handlers |
| `desktop/aizome/crates/aizome-comp-logic/src/memory.rs` | 47 | none |  | persisted per-app geometry cache (windows.json), atomic write; no phase |
| `desktop/aizome/crates/aizome-comp-logic/src/overlays/area.rs` | 372 | machines | [WM-12](#wm-12), [WM-14](#wm-14) | area picker |
| `desktop/aizome/crates/aizome-comp-logic/src/overlays/marking.rs` | 731 | machines | [WM-13](#wm-13), [WM-14](#wm-14) | marking menu |
| `desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs` | 261 | machines | [WM-14](#wm-14), [WM-03](#wm-03), [WM-12](#wm-12), [WM-13](#wm-13), [WM-15](#wm-15), [WM-16](#wm-16) | overlay routing and closing |
| `desktop/aizome/crates/aizome-comp-logic/src/overlays/overview.rs` | 539 | machines | [WM-15](#wm-15), [WM-02](#wm-02), [WM-14](#wm-14), [WM-22](#wm-22) | Desks overview |
| `desktop/aizome/crates/aizome-comp-logic/src/overlays/switcher.rs` | 391 | machines | [WM-16](#wm-16), [WM-02](#wm-02), [WM-14](#wm-14) | app switcher |
| `desktop/aizome/crates/aizome-comp-logic/src/render.rs` | 253 | machines | [WM-03](#wm-03) | frame composition; reads drag and slide state |
| `desktop/aizome/crates/aizome-comp-logic/src/requests.rs` | 304 | machines | [WM-12](#wm-12), [WM-15](#wm-15), [WM-22](#wm-22) | IPC dispatch into WM machines |
| `desktop/aizome/crates/aizome-comp-logic/src/scales.rs` | 128 | machines | [WM-07](#wm-07) | scale signature memo |
| `desktop/aizome/crates/aizome-comp-logic/src/slides.rs` | 327 | machines | [WM-17](#wm-17) | notification card slides |
| `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs` | 1536 | machines | [WM-18](#wm-18), [WM-19](#wm-19), [WM-20](#wm-20), [WM-21](#wm-21), [WM-22](#wm-22), [WM-05](#wm-05), [WM-07](#wm-07) | window-management model |
| `desktop/aizome/crates/aizome-comp-logic/src/wm/tests.rs` | 936 | none |  | tests |
| `desktop/aizome/crates/aizome-comp/build.rs` | 6 | none |  | rpath link argument |
| `desktop/aizome/crates/aizome-comp/examples/frame_bench.rs` | 341 | machines | [WM-23](#wm-23) | benchmark driver |
| `desktop/aizome/crates/aizome-comp/examples/sidestep_client.rs` | 184 | machines | [WM-24](#wm-24) | test client modes |
| `desktop/aizome/crates/aizome-comp/src/main.rs` | 9 | none |  | entry point |
| `desktop/aizome/crates/aizome-comp/tests/chromium.rs` | 326 | machines | [WM-25](#wm-25) | probe client |
| `desktop/aizome/crates/aizome-comp/tests/churn.rs` | 114 | none |  | seeded random stress test; no phase state |
| `desktop/aizome/crates/aizome-comp/tests/common/desktop.rs` | 78 | machines | [WM-26](#wm-26) | process and wait helpers |
| `desktop/aizome/crates/aizome-comp/tests/common/layer.rs` | 515 | machines | [WM-27](#wm-27) | shell-like client |
| `desktop/aizome/crates/aizome-comp/tests/common/mod.rs` | 1258 | machines | [WM-28](#wm-28), [WM-29](#wm-29), [WM-30](#wm-30) | compositor, toplevel and lock harness |
| `desktop/aizome/crates/aizome-comp/tests/common/text.rs` | 640 | machines | [WM-31](#wm-31), [WM-32](#wm-32) | text app and input method |
| `desktop/aizome/crates/aizome-comp/tests/context_menus.rs` | 223 | none |  | tests |
| `desktop/aizome/crates/aizome-comp/tests/displays.rs` | 365 | none |  | tests |
| `desktop/aizome/crates/aizome-comp/tests/focus.rs` | 334 | none |  | tests |
| `desktop/aizome/crates/aizome-comp/tests/fullscreen.rs` | 53 | none |  | tests |
| `desktop/aizome/crates/aizome-comp/tests/fullscreen_frames.rs` | 39 | none |  | tests |
| `desktop/aizome/crates/aizome-comp/tests/headless.rs` | 748 | none |  | tests |
| `desktop/aizome/crates/aizome-comp/tests/idle_inhibit.rs` | 175 | machines | [WM-25](#wm-25) | probe client |
| `desktop/aizome/crates/aizome-comp/tests/image_copy_capture.rs` | 606 | machines | [WM-33](#wm-33) | capture client |
| `desktop/aizome/crates/aizome-comp/tests/input_method.rs` | 244 | machines | [WM-34](#wm-34) | IBus-like input method |
| `desktop/aizome/crates/aizome-comp/tests/keys.rs` | 234 | none |  | tests |
| `desktop/aizome/crates/aizome-comp/tests/layout.rs` | 225 | none |  | tests |
| `desktop/aizome/crates/aizome-comp/tests/layout_desktop.rs` | 343 | none |  | tests (ignored e2e on WM-26 helpers) |
| `desktop/aizome/crates/aizome-comp/tests/lock_displays.rs` | 208 | none |  | tests |
| `desktop/aizome/crates/aizome-comp/tests/marking_edges.rs` | 34 | none |  | tests |
| `desktop/aizome/crates/aizome-comp/tests/notebook_desktop.rs` | 96 | none |  | tests (ignored e2e on WM-26 helpers) |
| `desktop/aizome/crates/aizome-comp/tests/overlays.rs` | 610 | none |  | tests |
| `desktop/aizome/crates/aizome-comp/tests/pacing.rs` | 353 | machines | [WM-25](#wm-25) | probe client |
| `desktop/aizome/crates/aizome-comp/tests/picker_cursor.rs` | 48 | none |  | tests |
| `desktop/aizome/crates/aizome-comp/tests/portal.rs` | 82 | machines | [WM-26](#wm-26) | unguarded chooser child |
| `desktop/aizome/crates/aizome-comp/tests/protocols.rs` | 617 | machines | [WM-25](#wm-25) | probe clients |
| `desktop/aizome/crates/aizome-comp/tests/recovery.rs` | 876 | machines | [WM-35](#wm-35), [WM-26](#wm-26) | probe client and child process |
| `desktop/aizome/crates/aizome-comp/tests/reload.rs` | 744 | machines | [WM-36](#wm-36), [WM-37](#wm-37), [WM-26](#wm-26) | variant builds and shell restart |
| `desktop/aizome/crates/aizome-comp/tests/scales.rs` | 213 | machines | [WM-25](#wm-25) | probe client |
| `desktop/aizome/crates/aizome-comp/tests/screencopy.rs` | 347 | machines | [WM-33](#wm-33) | capture client |
| `desktop/aizome/crates/aizome-comp/tests/screens.rs` | 252 | none |  | tests |
| `desktop/aizome/crates/aizome-comp/tests/settings.rs` | 478 | none |  | tests |
| `desktop/aizome/crates/aizome-comp/tests/sidestep.rs` | 234 | machines | [WM-26](#wm-26) | process guard and pixel poll |
| `desktop/aizome/crates/aizome-comp/vm/udev.nix` | 75 | machines | [WM-38](#wm-38) | VM test script |
