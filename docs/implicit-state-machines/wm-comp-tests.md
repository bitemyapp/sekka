# WM: Window manager logic and compositor integration tests

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/crates/aizome-comp-logic, desktop/aizome/crates/aizome-comp`. Audited at `8e0acbe5`; line numbers refer to that commit.

62 files audited. 38 implicit machines: 9 High, 8 Medium, 21 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

<a id="wm-04"></a>

### WM-04 — Interactive resize and its edge anchor
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:322` (`ResizeGrab.edges`, `.start_rect`); also `desktop/aizome/crates/aizome-comp-logic/src/lib.rs:103` (`WinData.resize_anchor`), `desktop/aizome/crates/aizome-comp-logic/src/lib.rs:109` (`ResizeAnchor`)
- **Encoding:** the grab object, a per-member `resize_anchor: Option<{left, top, start}>`, the toplevel's xdg `Resizing` pending state, and `requested_size` (WM-01).
- **States:** `Idle` → `Resizing` (grab live, anchor Some, `Resizing` set) → `Ended` (grab gone, `Resizing` unset) — with the anchor still `Some` in `Ended`.
- **Transitions:** `start_resize` sets `Resizing`, the anchor and placement Free (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:343-360`); motion requests sizes (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:404-407`); a commit applies the anchor (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1040-1047`); release ends (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:412-415`); `unset` unsets `Resizing` and configures (`desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:419-424`); nothing clears `resize_anchor` except unmap (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1078`) or a reload (not in `Saved`).
- **Hazards:** the anchor outlives the gesture and its `start` rect goes stale after moves and display changes; `unset` acts on the group as it is at the end, so a window that left the group mid-resize keeps `Resizing`.
- **Reify:** High — suspected bug; the invariant "anchor only while a resize grab runs" is not enforced anywhere.
- **Suspected bug:** resize a window from its left (or top) edge, then drag it elsewhere by its tab; the next commit whose size differs from the model's rect (an unfill or display relayout the client clamps to its minimum size, a client resizing itself) runs `r.x = a.start.right() - size.0` (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1041-1043`) and the window jumps back to where the old resize's right edge was, possibly on another display. (confidence: medium)
- **Reproduction:** Confirmed (end-to-end): [summary](reproductions.md#wm-04)

<a id="wm-10"></a>

### WM-10 — Sticky keys (latched modifiers)
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/input.rs:775` (`Access.alone`, `.latched`, `.applied`)
- **Encoding:** the modifier pressed with nothing else since, modifiers latched for the next key, and keys currently down with the synthetic modifier presses sent for them.
- **States:** per modifier `Idle` → `Alone` → `Latched` (tap) → `Idle` (tap again) ; per key `Up` → `AppliedDown(mods)` → `Up`.
- **Transitions:** modifier press →Alone (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:669`); release toggles latch (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:670-677`); other key clears Alone (`:680`); press with latches sends synthetic modifier presses →AppliedDown (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:681-688`); its release sends the key's then the modifiers' releases (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:690-697`); setting off clears `latched` and `alone` only (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:660-665`); quiesce drops all (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:97`).
- **Hazards:** `applied` holds synthetic presses the seat still believes are down; it is not drained when the setting turns off or when the logic is replaced.
- **Reify:** High — suspected bug; three fields with an unwritten "every applied press gets its release" invariant.
- **Suspected bug:** with sticky keys on, tap ⌃ and ❖ (both latch), then press R: the synthetic ⌃ and ❖ presses go out with R (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:681-688`), the core takes ⌃❖R and reloads at the end of that loop iteration while R is still down (`desktop/aizome/crates/aizome-comp-core/src/input.rs:518`); the reload drops `applied` (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:97`; unlike Reset Input it releases no keys), so R's release reaches the new logic as a plain release (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:698`) and ⌃ and ❖ stay pressed in the seat: the next letter typed arrives as a ⌃❖ chord (Q would lock the session). Turning sticky keys off while a latched chord is held does the same. (confidence: medium)
- **Reproduction:** Confirmed (end-to-end): [summary](reproductions.md#wm-10)

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
- **Reproduction:** Confirmed (end-to-end): [summary](reproductions.md#wm-12)

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
- **Reproduction:** Confirmed (end-to-end): [summary](reproductions.md#wm-15)

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

<a id="wm-21"></a>

### WM-21 — Displays and their Desks
- **Where:** `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:185` (`Model.screens`); also `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:188` (`Model.homes`), `:180` (`Model.current`), `:96` (`Screen.desk`)
- **Encoding:** each display's Desk, each output name's last chosen Desk, and the current Desk (the focused display's).
- **States:** `NoDisplays` (current is the only Desk shown) → `Displays(assignment)`; invariants: a Desk on at most one display, `current` shown when any display exists.
- **Transitions:** `show_desk` swap or take (`desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:1013-1029`); `sync_screens` (`:1130-1197`, new Desk when short `:1163-1165`); `remove_desk` (`:1403-1413`); `adopt_into_screen` (`:1273`); `focus` (`:481-483`); `sync_screens` wrapper with the shell-restart hold (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:710-748`); `desks_moved` side effects (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:642-673`).
- **Hazards:** `homes` is never pruned of output names; `sync_screens` creates Desks that the settings sync later trims.
- **Reify:** High — three fields, many mutators, and display hot-plug ordering.
