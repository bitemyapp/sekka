# SKB: Sukumo engine: Wayland client backend

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/sukumo/crates/sidestep-engine`. Audited at `8e0acbe5`; line numbers refer to that commit.

30 files, 0 pending. 42 implicit machines: 5 High, 20 Medium, 17 Low.

## Machines

<a id="skb-01"></a>

### SKB-01 — Client-side decoration presence and redraw cache
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:195` (`Decor.header_dirty`, `.strips_dirty`, `.drawn`, `.passthrough`, `Look.title_mask`, `Surface.mapped`, `Surface.at`); also `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:533` (`Win.decor`, `Win.client_side`)
- **Encoding:** `Win.decor: Option<Decor>` (present iff toplevel && titled && (client_side || forced)); per part `Surface.mapped`/`at`; two dirty flags plus a `drawn` key `(width, height, scale bits, state, style, bar)`; `title_mask` cache keyed by scale; role entries in `State.roles`
- **States:** `NoDecor` → `Created(dirty, undrawn)` → `Drawn(clean)` ⇄ `Dirty(header|strips)`; per part `Hidden` (fullscreen header, non-floating strips) ⇄ `Shown`; invalid but representable: `decor.is_some()` with `client_side == false` until the next `update_decorations`, `drawn == Some(key)` with stale `title_mask`
- **Transitions:** `update_decorations` create/remove (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2031`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2046`) from configure (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2238`) and `SetStyle` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1034`); `set_passthrough` dirties both (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:306`); `title_changed` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:330`); `scale_changed` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:335`); `restyle` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:342`); `set_capabilities` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:347`); `draw` re-keys (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:358`), clears flags (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:365`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:378`), hides header in fullscreen (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:367`) or strips when tiled/maximized (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:384`); `Surface::hide` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:126`), `show_as` sets mapped (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:166`); `needs_draw` read by `refresh_decor` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2013`)
- **Hazards:** dirty flags and `drawn` must agree with what each subsurface shows; `with_decor` takes the decor out and puts it back (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:825`) so a panic in between loses it; pointer focus may still hold a `Role::Decor` of surfaces destroyed by removal (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/seat.rs:89`) until Leave
- **Reify:** Medium — two flags plus a key plus per-surface mapped state, mutated from six entry points
- **Related:** client-side decoration mode chosen by the compositor, `desktop/aizome/crates/aizome-comp-core/src/shell.rs`

<a id="skb-02"></a>

### SKB-02 — Header button hover and press tracking
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:211` (`Look.hover`, `Look.pressed`)
- **Encoding:** two `Option<Control>` fields; `header_dirty` set on change
- **States:** `Idle` → `Hover(c)` → `Pressed(c)` → `Released` (activate if still over `c`) | `Cancelled` (left or released elsewhere)
- **Transitions:** `pointer_moved` sets hover (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:837`); `pointer_left` clears both (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:849`); left press on a control sets `pressed` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:887`), else double-click maximize (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:889`) or move (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:894`); left release takes `pressed` and acts if it equals the control under the pointer (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:898`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:900`); right press opens the window menu (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:911`)
- **Hazards:** none observed (the implicit pointer grab keeps the header focused while pressed)
- **Reify:** Low — two local fields, one handler set

<a id="skb-03"></a>

### SKB-03 — Incoming drag-and-drop session
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:61` (`Dnd.current`, `.leaving`, `.named`, `.ticking`); `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:75` (`Drag.told`, `.outstanding`, `.pending`, `.status`, `.periodic`, `.dropped`, `.selected`, `.offer`, `.serial`)
- **Encoding:** two `Option<Drag>` slots (current, leaving this batch), a name counter, a periodic timer token; per drag two bools, an outstanding-answer counter, a coalesced position, the last answer
- **States:** `None` → `Entered(awaiting URL list, !told)` → `Told` ⇄ `AwaitingAnswer(outstanding>0, pending pos)` → `Leaving(batch)` → `Left` | `Re-entered(same window)`; `Dropped(!told: drop deferred to tell)` → `Dropped(told)` → `Finished`; invalid but representable: `current` and `leaving` both naming the same id, `dropped && leaving.is_some()`, `outstanding` drifting from the real number of unanswered messages
- **Transitions:** `enter` re-entry over the same window (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:144`), new drag (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:158`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:159`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:180`), URL read then idle `tell` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:192`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:205`); `tell` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:214`, drop deferred `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:218`); `moved` coalesces (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:259`) or sends (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:262`); `leave` keeps a dropped drag (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:269`) else parks it in `leaving` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:275`) with an idle flush (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:276`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:281`); `dropped` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:292`, deferred `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:294`); `source_actions` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:319`); `selected_action` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:329`); `status` decrements and forwards pending (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:342`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:350`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:353`); `finish` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:362`); tick timer sends (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:238`) and stops when both slots are empty (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:231`)
- **Hazards:** one `DndStatus` per message is assumed: a message the main thread never answers freezes motion forwarding and ticks for that drag; the tick timer keeps firing every 50 ms while a dropped drag awaits `DndFinish`; a dropped drag is discarded unannounced by the next enter (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:158`); stale idle `tell`s are filtered only by id and `told`
- **Reify:** High — eleven fields across two slots, a counter protocol with the main thread, transitions in nine handlers
- **Related:** main-thread side `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:1544` (DndEnter…DndDrop, always answers with DndFinish); drag pasteboard `clipboard.rs` (SKB-36)

<a id="skb-04"></a>

### SKB-04 — Outgoing drag source lifecycle
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/drag_out.rs:40` (`Out.source`, `.drag`, `.contents`, `.icon`, `.action`); `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/selection.rs:76` (`Selection.out`)
- **Encoding:** `Option<Out>`; the compositor's last chosen action; the icon surface owned by `Out` (destroyed on drop)
- **States:** `None` → `Dragging(action)` → `Ended(performed action | cancelled)`; no state for "dropped, awaiting finish" (`dnd_dropped` ignored)
- **Transitions:** `start` ends any previous drag (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/drag_out.rs:69`), fails at once without window or serial (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/drag_out.rs:71`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/drag_out.rs:81`), else starts (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/drag_out.rs:90`); `action` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/drag_out.rs:134`); `end` from `cancelled` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/selection.rs:502`), `dnd_finished` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/selection.rs:513`), or a new start; reports `DragEnded` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/drag_out.rs:148`)
- **Hazards:** a new drag reports the old one cancelled even if its destination would still finish it; late `send_request`s for an ended source fall through to the clipboard lookup and get a closed pipe (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/selection.rs:481`); promised data shares SKB-28's unbounded token map
- **Reify:** Medium — small, but ends are spread over three callbacks and one start path
- **Related:** `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:1590` (ProvideDragData, DragEnded)

<a id="skb-05"></a>

### SKB-05 — xdg-foreign parent import link
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:493` (`Win.foreign_parent`); `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/foreign.rs:48`
- **Encoding:** `Option<ZxdgImportedV2>`
- **States:** `None` → `Imported` → `Defunct` (compositor said destroyed; proxy destroyed but still `Some`) → `Replaced` | `Closed`
- **Transitions:** `SetForeignParent` destroys the old and imports anew (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1166`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1170`); `Destroyed` event destroys the proxy (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/foreign.rs:59`); `close_window` destroys again (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1422`)
- **Hazards:** the field is not cleared on `Destroyed`, so later paths destroy a dead proxy (harmless in wayland-client)
- **Reify:** Low — one field, three sites

<a id="skb-06"></a>

### SKB-06 — Compose (dead-key) sequence
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/keyboard.rs:72` (`Compose.state`, `Compose.pending`)
- **Encoding:** kbvm compose state plus a display string mirroring it
- **States:** `Idle` → `Pending(accent shown)` → `Composed` | `Aborted` → `Idle`
- **Transitions:** `feed` Pending (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/keyboard.rs:108`), Composed (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/keyboard.rs:112`), Aborted (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/keyboard.rs:122`), not in table (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/keyboard.rs:103`); `reset` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/keyboard.rs:128`) on keymap (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/seat.rs:941`), keyboard leave (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/seat.rs:958`), keys that type nothing (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/keyboard.rs:260`); shortcuts bypass it (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/keyboard.rs:268`)
- **Hazards:** a shortcut pressed mid-sequence leaves it pending without reporting `composing`, so the next plain key completes a sequence the user may think was dropped
- **Reify:** Low — kbvm holds the real state; the mirror string is local

<a id="skb-07"></a>

### SKB-07 — Layer surface configure and close
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/layer.rs:55`; `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:420` (`Shell::Layer`), `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:501` (`Win.configured`, `.width`, `.height`)
- **Encoding:** `Win.configured`; `width`/`height` double as the asked size before configure and the configured size after
- **States:** `Missing(no layer shell → CloseRequested)` | `Committed(awaiting configure)` → `Configured` ⇄ `Re-requested(commit, awaiting configure)` → `Closed(close requested, surface kept)`
- **Transitions:** create commits (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/layer.rs:77`), no shell (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1229`); `request` stores size before configure (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/layer.rs:94`) and commits (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/layer.rs:108`); `configure` fills 0 sides from the asked size (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/layer.rs:131`) then `resize`; `closed` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/layer.rs:115`)
- **Hazards:** after `closed` the surface lives on until the main thread closes it and later requests still commit it
- **Reify:** Low — follows SKB-11 with one extra exit
- **Related:** `desktop/aizome/crates/aizome-comp-core/src/shell.rs` (layer shell); panels in `desktop/aizome/crates/aizome-shell/src/deskbar.rs`

<a id="skb-08"></a>

### SKB-08 — Session lock lifecycle
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:358` (`State.session_lock`); `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/lock.rs:24`; `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:422` (`Shell::Lock`)
- **Encoding:** `Option<SessionLock>` plus smithay-client-toolkit's internal `locked` flag (`is_locked`); lock surfaces as windows with `Shell::Lock(_, output)`
- **States:** `None` → `Requested` → `Locked` → `Unlocked(None)` | `Finished(None)`; `Requested` → `Withdrawn` (dropped); invalid but representable: `Shell::Lock` windows alive with `session_lock == None`
- **Transitions:** `lock` ignored while one exists (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/lock.rs:25`), asked (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/lock.rs:29`), no protocol → finished (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/lock.rs:32`); `locked` → `SessionLocked` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/lock.rs:60`); `unlock` takes and unlocks or drops (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/lock.rs:40`); `finished` unlocks and reports (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/lock.rs:66`); surface create needs a lock and one per output (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/lock.rs:50`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/lock.rs:51`); render-thread exit forgets a held lock (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:337`)
- **Hazards:** `UnlockSession` before `locked` arrives drops the lock (destroy); if the compositor already sent `locked`, ext-session-lock calls that destroy a protocol error; lock surfaces outlive the lock until the main thread closes them; `locked`/`finished` don't check which lock they're for
- **Reify:** Medium — four states held in an `Option` plus a hidden flag; consequences are a stuck locked session
- **Suspected bug:** the lock screen asks to lock and unlocks almost at once (authentication done before the compositor's `locked` reaches this thread): `unlock` drops the unconfirmed lock, the compositor sees `destroy` after sending `locked`, kills the client with `invalid_destroy`, and the session stays locked with no lock client (confidence: low)
- **Related:** `desktop/aizome/crates/aizome-shell/src/lock.rs`; compositor `desktop/aizome/crates/aizome-comp-core/src/state.rs`; main thread `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs`
- **Reproduction:** not attempted; only suspected bugs on High machines were reproduced

<a id="skb-09"></a>

### SKB-09 — In-window menu bar presence and redraw
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/menubar.rs:28` (`Bar.height`, `.drawn`, `.surface.mapped`); `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:535` (`Win.menubar`)
- **Encoding:** `Option<Bar>`; `drawn: Option<(width, height, scale bits)>` (None = redraw); `Role::Bar` entry in `State.roles`
- **States:** `None` → `Set(undrawn)` → `Drawn` ⇄ `Hidden(fullscreen or 0 height)`; `Removed`
- **Transitions:** `set` removes (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/menubar.rs:95`), updates (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/menubar.rs:101`), creates (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/menubar.rs:109`), re-sends limits and Configure (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/menubar.rs:113`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/menubar.rs:119`); `draw` hides (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/menubar.rs:63`), skips (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/menubar.rs:69`), draws (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/menubar.rs:82`); fullscreen hides through `bar_height` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:612`)
- **Hazards:** see SKB-17 for the input region it never updates
- **Reify:** Low — one object, two sites

<a id="skb-10"></a>

### SKB-10 — Render thread lifecycle
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:174` (`start`), `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:196` (`ANY_THREAD`), `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:142` (`MainSender.tx`), `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:388` (`State.exit`)
- **Encoding:** a global latest sender, an `Option` sender taken on drop, the loop's `exit` flag, `conn.protocol_error()`
- **States:** `NotStarted` → `Running` → `Stopped(channel closed | dispatch error | protocol error)` → (`Running` again on a new start)
- **Transitions:** `start` spawns and publishes the sender (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:185`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:191`); channel close sets exit (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:262`); dispatch error (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:322`); protocol error (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:328`); a held lock is forgotten (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:338`); `MainSender` drop closes then wakes (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:164`)
- **Hazards:** `ANY_THREAD` keeps a stopped thread's sender, so `send_if_running` silently feeds a dead channel (clipboard reads each wait `READ_TIMEOUT`); `video::STARTED` and published caps outlive the thread (SKB-35)
- **Reify:** Medium — four places hold pieces of one lifecycle across threads
- **Related:** owner of `Backend`, `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs`

<a id="skb-11"></a>

### SKB-11 — Window configure, size and geometry lifecycle
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:501` (`Win.configured`, `.width`, `.height`, `.px_width`, `.px_height`, `.state`, `.client_side`, `.geometry`, `.reported`, `.sent_limits`, `.held`); `Frame.placed` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:449`)
- **Encoding:** a bool, sizes doubling as requested-before-configure and configured-after, the geometry and frame size last committed, caches of what the main thread and compositor were last told
- **States:** `Created(unconfigured)` → `Configured(new size, held, geometry != expected)` → `Presented(geometry == expected)` ⇄ `Reconfigured`; invalid but representable: `configured && geometry != expected_geometry()` while other paths commit the frame
- **Transitions:** create (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1289`), sheets configured at once (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1327`); toplevel configure sets state and decoration mode (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2215`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2227`), keeps fixed sizes (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2250`), resizes (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2265`), refreshes decor at an unchanged size (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2272`); popup configure (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2314`); `Request::Resize` before/after configure (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1501`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1499`); `resize` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1584`, buffers dropped `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1591`, held `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1595`); `report` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1611`); `apply_limits` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1448`); `place_decorations` sets geometry (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1974`) and frame size (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1985`)
- **Hazards:** smithay-client-toolkit acks each configure before the handler runs, so the next commit of the frame answers it; `content_input` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2083`), `SetOpaqueRegion` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1155`), `request_frame` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1814`) and `refresh_decor` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2022`) all commit between a size-changing configure and the main thread's present; only `sheet::commit_parent` checks for that state (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/sheet.rs:73`); `refresh_decor` goes on whenever geometry differs, even with nothing to redraw (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2015`)
- **Reify:** High — eleven fields, seven mutation sites, an unnamed "configured but not presented" state that some paths respect and others don't
- **Suspected bug:** after the header's maximize button is clicked, the configure arrives; a pointer move over the header before the main thread presents changes the hover (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:837`), so `refresh_decor` commits the maximized geometry and frame around the old-size content buffer: one frame with an undrawn band (confidence: low)
- **Related:** main thread Configure handling `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:1395`
- **Reproduction:** Confirmed (end-to-end): [test and output](reproductions.md#skb-11)

<a id="skb-12"></a>

### SKB-12 — Window scale source
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:497` (`Win.scale`), `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:546` (`Win.scale_given`)
- **Encoding:** the scale plus a bool saying whether the compositor named it
- **States:** `Guessed(outputs agree | 1)` → `FromOutputs(unconfigured)` → `Given(preferred or integer)`
- **Transitions:** create from outputs, parent or layer output (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1201`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1205`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1232`); `outputs_changed` only while unconfigured and not given (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1640`); `set_scale` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1647`, tiles dropped `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1653`, decor `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1655`, resize `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1663`); integer path only without fractional scale (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2149`); fractional preferred scale (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2439`)
- **Hazards:** a window configured before its preferred scale draws a first frame at the guess, then redraws (documented); a sheet copies the parent's scale but has its own fractional object
- **Reify:** Low — two fields, guarded transitions

<a id="skb-13"></a>

### SKB-13 — Frame pacing and layer-tree animation gating
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:514` (`Win.frame_seq`, `.frame_signalled`, `.main_waiting`, `.ticks`, `.ca_pending`, `.held`); fallback callback `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/tiles.rs:696`
- **Encoding:** a present counter tagging callbacks, four bools; the first matching callback on the content or one tile counts
- **States:** per window `Signalled` → `InFlight(seq)` → `Signalled`; orthogonal `MainWaiting`, `Held(main owes a frame)`, `CaPending`, `Ticking`; invalid but representable: `main_waiting` with no callback requested (a Present to an unconfigured window returns early, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1833`)
- **Transitions:** `Present` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1089`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1090`); `present` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1838`, callback `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1852`, tile callbacks `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/tiles.rs:831`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/tiles.rs:705`); `request_frame` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1810`); callback (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2390`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2391`, Frame `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2394`, Tick `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2400`, animate `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2404`, tick-only `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2406`); `animate` defers (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1794`); `hold` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1735`) from content paints (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1063`) and commits (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1108`); resize holds (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1595`); `FrameTicks` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1115`); `ca_frame` clears pending (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1772`)
- **Hazards:** a window the compositor stops calling back (hidden) leaves the main thread waiting and animations parked, by design; stale callbacks are dropped by `seq` equality only
- **Reify:** High — six fields, eight mutation sites, a cross-thread handshake
- **Related:** `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:1417` and `desktop/aizome/sukumo/crates/sidestep-ui/src/window.rs:675` (`frame_pending`)

<a id="skb-14"></a>

### SKB-14 — Shared-memory buffer reuse and staleness (window and tile buffers)
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:504` (`Win.pending`, `.damage`, `.buffers`, `.front`), `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:636` (`RootBuffer.stale`); also `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/tiles.rs:213` (`Shown.buffers`), `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/tiles.rs:230` (`Upload.stale`)
- **Encoding:** per buffer a stale-rectangle list; busy/free lives in smithay-client-toolkit's slot (`canvas()` is `Some` once released); the last-presented index
- **States:** per buffer `Free` ⇄ `HeldByCompositor`; `Stale(rects)` → `CaughtUp`; window `NoFront(first frame at this size)` → `Front(i)`
- **Transitions:** paints/scrolls queue (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1676`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1693`); `draw_frame` picks front, free or new (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1875`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1883`), passes damage on (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1888`), catches up from the front (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1904`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1908`), fills background (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1924`), sets front (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1956`); resize drops all (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1591`); tile `upload` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/tiles.rs:859`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/tiles.rs:865`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/tiles.rs:870`)
- **Hazards:** no cap on buffers: a compositor that holds every buffer gets a new full-size one each present; correctness depends on every write going through `pass_on`
- **Reify:** Medium — invariant-heavy but already funnelled through `pass_on`

<a id="skb-15"></a>

### SKB-15 — xdg_popup lifecycle and grab serial
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1363` (`create_popup`), `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:554` (`Win.popup_parent`), `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1395` (`grab_serial`)
- **Encoding:** `Shell::Popup`, the parent id, the seats' latest serial, a static reposition token counter
- **States:** `Created(committed)` → `Configured` → `Repositioned` → `Done` → `Closed`; `Failed` (no parent, sheet or lock parent, no positioner)
- **Transitions:** create gives a layer parent the popup (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1377`), grabs (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1382`), commits (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1384`); failure destroys the surface and returns (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1218`); `reposition` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1529`, ignored below v3 `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1524`); configure (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2314`); done (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2319`); grab fallback to serial 0 under a layer surface (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1403`)
- **Hazards:** unlike layer and lock surfaces (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1229`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1241`), a failed popup sends no `CloseRequested`; `repositioned` events are ignored
- **Reify:** Medium — small, but its failure exit is missing
- **Suspected bug:** a menu opened from a sheet (or a lock surface) fails in `create_popup` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1373`); the main thread is never told (no `CloseRequested` or `PopupDone`), so it keeps a popup window that never configures, never shows and never closes on its own (confidence: medium)
- **Related:** `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:1528` (PopupDone)
- **Reproduction:** not attempted; only suspected bugs on High machines were reproduced

<a id="skb-16"></a>

### SKB-16 — Activation tokens
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:380` (`State.startup_token`), `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2326` (`Activation.request`, `.target`)
- **Encoding:** a one-shot startup token; per request user data naming the main thread's request id or a target surface
- **States:** `StartupToken(unused)` → `Used`; per request `Asked` → `Answered(token)` | `NoProtocol(None at once)`
- **Transitions:** first toplevel takes the startup token (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1257`); `request_token` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1559`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1564`); `activate` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1542`); `new_token` answers or activates (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2353`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2355`)
- **Hazards:** no timeout if the compositor never sends `done`
- **Reify:** Low — one-shot plus request ids

<a id="skb-17"></a>

### SKB-17 — Pointer passthrough and input-region application
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:544` (`Win.passthrough`), `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:552` (`Win.input_region`), `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:383` (`State.empty_region`)
- **Encoding:** applied `passthrough` vs wanted `style.passthrough`; the main thread's region; a lazily made empty region shared by every surface
- **States:** `Takes(all | region)` ⇄ `Passthrough(empty everywhere)`
- **Transitions:** `apply_input` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2062`) at create (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1324`) and SetStyle (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1035`), decor updated (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2064`); `content_input` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2081`) from SetInputRegion (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1146`); new decor copies it (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2043`); a new menu bar copies it (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/menubar.rs:106`)
- **Hazards:** each subsurface kind applies the flag on its own path; the menu bar only at creation
- **Reify:** Medium — the applied state fans out to subsurfaces inconsistently
- **Suspected bug:** `apply_input` updates the content and decorations but not an existing menu bar, so a toplevel with a menu bar that becomes passthrough still takes clicks on the bar, and a bar made while passthrough keeps an empty region after passthrough ends (confidence: medium)
- **Reproduction:** not attempted; only suspected bugs on High machines were reproduced

<a id="skb-18"></a>

### SKB-18 — (test harness) Null render thread as a fake compositor
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/null.rs:318` (`run`: `windows`, `focused`); statics `null.rs:146` (`REQUESTED`), `null.rs:149` (`RUNNING`), `null.rs:153` (`SENT`, `HANDLED`), `null.rs:169` (`TEXT`), `null.rs:178` (`PIXELS`)
- **Encoding:** a window map, a focused window, sent/handled counters tests compare, capture toggles as `Option`s
- **States:** `Idle(HANDLED >= SENT)` ⇄ `Busy`; per window `Created(configured)` → `Focused` ⇄ `Unfocused` → `Closed`
- **Transitions:** `start` sets RUNNING (`null.rs:257`); main thread counts sends (`null.rs:161`); create focuses (`null.rs:383`); close unfocuses (`null.rs:391`); activation focuses (`null.rs:444`, `null.rs:464`); layer keyboard (`null.rs:493`, `null.rs:494`); each handled message counts (`null.rs:570`)
- **Hazards:** `RUNNING` and `REQUESTED` never reset; counters are process-wide and monotonic
- **Reify:** Medium — test-only, but its idle handshake has a hole
- **Suspected bug:** a `Request` for a window the null thread doesn't have takes `continue` (`null.rs:449`), skipping `HANDLED` (`null.rs:570`); from then on `testing::render_idle()` (`desktop/aizome/sukumo/crates/sidestep-ui/src/testing.rs:75`) is false for the rest of the process and tests polling it spin to their timeout (confidence: high)
- **Related:** `desktop/aizome/sukumo/crates/sidestep-ui/src/testing.rs:75`, `desktop/aizome/sukumo/crates/sidestep-ui/src/window.rs:687`
- **Reproduction:** not attempted; only suspected bugs on High machines were reproduced

<a id="skb-19"></a>

### SKB-19 — Output discovery, settle and learned scales
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/outputs.rs:34` (`Tracker.expected`, `.settled`, `.published`, `.scales`, `.bounds`, `.windows`, `.last`)
- **Encoding:** thread-local tracker: startup output count, two bools, learned scale and work area keyed by the output's integer scale and size then, per-window output lists
- **States:** `Unsettled(expected > described)` → `Settled` → `Published(first)` → `Changed(news)`; learned value `Valid` ⇄ `Stale(filtered while the output differs)`
- **Transitions:** `started` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/outputs.rs:55`); `changed` settles (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/outputs.rs:65`), publishes (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/outputs.rs:70`), marks news (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/outputs.rs:68`); `requested` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/outputs.rs:84`); `destroyed` forgets (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/outputs.rs:160`) and re-snapshots on idle (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/outputs.rs:173`); `surface_moved` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/outputs.rs:186`); `window_closed` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/outputs.rs:197`); `learned` only for a window alone on an output (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/outputs.rs:204`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/outputs.rs:215`)
- **Hazards:** an output announced at startup that goes before describing itself keeps `settled` false (and `requested` publishes nothing) until the count is reached again
- **Reify:** Medium — two bools plus caches, five entry points
- **Related:** `desktop/aizome/sukumo/crates/sidestep-engine/src/outputs.rs` (SKB-40)

<a id="skb-20"></a>

### SKB-20 — Keyboard focus source, seat devices and first mouse
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/seat.rs:79` (`Seat.pointer`, `.keyboard`), `seat.rs:147` (`Keyboard.focus`), `seat.rs:72` (`Seats.activated`), `seat.rs:65` (`Seats.latest`); `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2228`
- **Encoding:** optional devices per seat; focus per keyboard; when no seat has a keyboard, `WindowState.activated` stands for focus; a list of windows that got focus since their last pointer event; the latest input serial
- **States:** per window `Unfocused` ⇄ `Focused(keyboard)` | `Focused(activation, no keyboard)`; `FirstMousePending` → `Consumed`; invalid but representable: Focus{true} sent from activation while a keyboard capability later appears and is never reconciled
- **Transitions:** capabilities create/drop devices (`seat.rs:324`, `seat.rs:349`, `seat.rs:344`, `seat.rs:363`); `drop_keyboard` unfocuses (`seat.rs:284`) and promotes activated windows (`seat.rs:293`); keyboard enter (`seat.rs:947`, `seat.rs:949`, `seat.rs:951`); leave (`seat.rs:959`); configure without keyboard (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2232`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2234`); first mouse consumed by motion and buttons (`seat.rs:435`, `seat.rs:582`); seat removal (`seat.rs:246`, `seat.rs:252`); window close (`seat.rs:831`, `seat.rs:844`)
- **Hazards:** two focus sources switch on `has_keyboard`, with no reconciliation when a keyboard capability appears; `latest` is never invalidated on release, so grabs and selection use an old serial
- **Reify:** Medium — focus is spread over keyboards, activation state and a list, across two files
- **Related:** `desktop/aizome/crates/aizome-comp-core/src/focus.rs`

<a id="skb-21"></a>

### SKB-21 — Pointer focus and content crossings
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/seat.rs:89` (`Pointer.focus`, `.enter_serial`, `.x`, `.y`, `.inside`)
- **Encoding:** surface focus with role, position, and the window the main thread was last told the pointer is in
- **States:** `Outside` → `OnSurface(role)` → settled `InsideContent(window)` | `OverDecor`; transient `focus == None && inside == Some` between Leave and Frame
- **Transitions:** Enter (`seat.rs:406`, `seat.rs:407`); Leave (`seat.rs:421`); Motion (`seat.rs:432`); `settle` on frame, button, motion (`seat.rs:489`, `seat.rs:440`, `seat.rs:433`) sends Leave/Enter (`seat.rs:528`, `seat.rs:531`) and records (`seat.rs:534`); pre-v5 settles per event (`seat.rs:411`); device dropped sends Leave (`seat.rs:264`); window closed clears (`seat.rs:835`, `seat.rs:838`)
- **Hazards:** a `Role::Decor` focus can outlive its decorations (SKB-01)
- **Reify:** Medium — settled at frame ends from several handlers

<a id="skb-22"></a>

### SKB-22 — Click counting
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/seat.rs:132` (`Clicks.time`, `.x`, `.y`, `.button`, `.target`, `.count`)
- **Encoding:** last press and running count
- **States:** `None` → `Series(n)` → `Series(n+1)` | `Series(1)`
- **Transitions:** `press` (`seat.rs:596`, `seat.rs:597`); release reports the count (`seat.rs:576`)
- **Hazards:** none observed
- **Reify:** Low — local, tested

<a id="skb-23"></a>

### SKB-23 — Touchpad scroll gesture and axis frame
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/seat.rs:104` (`Pointer.gesture`), `seat.rs:115` (`AxisFrame`), `seat.rs:111` (`Pointer.inverted`)
- **Encoding:** per-frame accumulators reset at each frame; `gesture: Option<Vec<(time, dx, dy)>>` meaning a finger scroll under way with its recent moves
- **States:** `NoGesture` → `Began` → `Changed`* → `Ended(velocity)`; wheel frames `None`
- **Transitions:** axis events accumulate (`seat.rs:450`, `seat.rs:458`, `seat.rs:463`, `seat.rs:468`, `seat.rs:473`); frame → `scrolled` (`seat.rs:490`); `scrolled` takes the frame (`seat.rs:605`), Began/Changed (`seat.rs:634`, `seat.rs:637`), records (`seat.rs:640`), Ended on stop (`seat.rs:647`)
- **Hazards:** `gesture` isn't cleared on Leave or window close; `finger` is inferred from `gesture.is_some()` for sourceless frames (`seat.rs:613`); Ended goes to whatever content is under the pointer at stop time, or nowhere
- **Reify:** Medium — phase hidden in an `Option<Vec>`, ended only by an event that may not come
- **Suspected bug:** a two-finger scroll whose window closes (or that a popup grab interrupts) never gets `axis_stop` on our surface: the old window never hears Ended, and the next touchpad scroll in any window starts with Changed and no Began (confidence: low)
- **Reproduction:** not attempted; only suspected bugs on High machines were reproduced

<a id="skb-24"></a>

### SKB-24 — Pinch gesture
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/seat.rs:107` (`Pointer.pinch_scale`)
- **Encoding:** `Option<f64>` (Some while a pinch is under way)
- **States:** `Idle` → `Pinching(scale)` → `Idle`
- **Transitions:** Begin (`seat.rs:687`); Update ignored without Begin (`seat.rs:691`); End (`seat.rs:695`)
- **Hazards:** Began/Ended go only to content under the pointer; over decorations they're dropped
- **Reify:** Low — one field

<a id="skb-25"></a>

### SKB-25 — Cursor visibility and shown cache
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/seat.rs:68` (`Seats.hidden`), `seat.rs:98` (`Pointer.shown`, `Pointer.hidden`)
- **Encoding:** `Option<bool>` tri-state (shown / hidden / hidden until moved) plus per-pointer applied state
- **States:** `Shown` ⇄ `Hidden` | `HiddenUntilMoved`
- **Transitions:** `hide_cursor` (`seat.rs:757`); motion unhides (`seat.rs:554`); enter resets the cache (`seat.rs:409`); `update_cursor` hides (`seat.rs:740`) or shows (`seat.rs:744`); `set_cursor` caches (`seat.rs:770`)
- **Hazards:** none observed
- **Reify:** Low — small, local

<a id="skb-26"></a>

### SKB-26 — Key repeat timer
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/seat.rs:153` (`Keyboard.repeating`, `.rate`, `.delay`)
- **Encoding:** `Option<(code, calloop token)>`; rate and delay from the compositor
- **States:** `Idle` → `Delay(code)` → `Repeating(code)` → `Idle`
- **Transitions:** repeating press starts (`seat.rs:1028`), other press stops (`seat.rs:1030`), release of that key stops (`seat.rs:1033`); `start_repeat` (`seat.rs:1045`, `seat.rs:1055`); `stop_repeat` (`seat.rs:1061`); `repeat` stops itself without focus or keymap (`seat.rs:1070`); leave (`seat.rs:956`); window closed (`seat.rs:846`); keyboard dropped (`seat.rs:281`); RepeatInfo (`seat.rs:993`)
- **Hazards:** a rate or delay change takes effect only at the next press
- **Reify:** Medium — seven mutation sites for one timer

<a id="skb-27"></a>

### SKB-27 — Selection and primary-selection ownership
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/selection.rs:64` (`Selection.source`, `.pending`, `.cleared_by_us`); `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/primary.rs:36` (`Primary.source`, `.pending`, `.cleared_by_us`)
- **Encoding:** our source with contents, `Option<Option<Contents>>` waiting for an input serial, a bool swallowing the echo of our own clear; one global flag per selection kind across all data devices
- **States:** `NotOwner` → `Owner(source)` → `Cancelled(other client took it)`; `PendingOffer(no serial)` → `Owner`; `Cleared(awaiting echo)` → `NotOwner`
- **Transitions:** `set` pending (`selection.rs:126`), offer (`selection.rs:134`), clear (`selection.rs:137`, `selection.rs:139`); `focus_changed` retries (`selection.rs:114`); `cancelled` (`selection.rs:506`); a cleared event consumes the flag (`selection.rs:536`); primary `set` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/primary.rs:78`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/primary.rs:86`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/primary.rs:91`), `focus_changed` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/primary.rs:67`), clear echo (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/primary.rs:115`), `cancelled` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/primary.rs:174`)
- **Hazards:** `cleared_by_us` is consumed by the next cleared event from any device; a clear the compositor doesn't echo (nothing to clear) leaves it set; pending contents wait for a keyboard enter indefinitely while the main thread believes its write is current; the main thread isn't told when our source is cancelled
- **Reify:** High — three fields in two structurally identical copies, five handlers each, feeding the shared clipboard state
- **Suspected bug:** we clear an already-empty selection on a compositor that doesn't echo a no-op clear; later a client copies, then quits: its clear is swallowed as ours (`selection.rs:536`), so the shared state keeps the dead client's types and read-ahead text and our paste returns it (confidence: low)
- **Related:** `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs` (SKB-36); compositor `desktop/aizome/crates/aizome-comp-core/src/handlers.rs`
- **Reproduction:** Not confirmed: [test and output](reproductions.md#skb-27)

<a id="skb-28"></a>

### SKB-28 — Promised clipboard and drag data tokens
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/selection.rs:71` (`Selection.promised`, `.next_token`)
- **Encoding:** token → client write pipe
- **States:** `Requested(pipe waits)` → `Provided(written)` | `Refused(closed)`
- **Transitions:** `send_request` stores and asks the main thread (`selection.rs:492`, `selection.rs:494`, `selection.rs:495`); `provided` (`selection.rs:185`) from SelectionData (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1136`)
- **Hazards:** no timeout or cleanup: an unanswered promise keeps the client's pipe open forever, and entries outlive their source
- **Reify:** Medium — cross-thread request ids with no expiry
- **Related:** `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:1541` (ProvideSelection), `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:1590` (ProvideDragData)

<a id="skb-29"></a>

### SKB-29 — Bounded pipe transfers
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/selection.rs:295` (`read_pipe_bounded`), `selection.rs:194` (`write_pipe`), `selection.rs:288` (`ReadPolicy`)
- **Encoding:** a pipe source and a timer that each remove the other via shared `Cell<Option<RegistrationToken>>`; `Rc<RefCell<Option<done>>>` for exactly-once completion; last-data instant
- **States:** `Transferring` → `Done` | `TooLong` | `Failed(idle | deadline | cancelled | error)`
- **Transitions:** read ends and removes the timer (`selection.rs:340`, `selection.rs:342`); timer cancels the read (`selection.rs:356`, `selection.rs:357`); install failures (`selection.rs:346`, `selection.rs:366`); write ends (`selection.rs:209`, `selection.rs:211`); write timer (`selection.rs:217`)
- **Hazards:** none observed
- **Reify:** Low — already self-contained and exactly-once

<a id="skb-30"></a>

### SKB-30 — Sheet placement and parent commit gating
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/sheet.rs:30` (`Sheet.parent`, subsurface)
- **Encoding:** position as the parent's pending state; the parent's "presented its size" derived from `geometry == expected_geometry()`
- **States:** `Placed(pending parent commit)` → `Shown`
- **Transitions:** create (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1204`, configured at once `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1327`); `place` (`sheet.rs:65`); `commit_parent` gated (`sheet.rs:73`); `parent_resized` (`sheet.rs:88`) from resize (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1599`) and present (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:2000`); sheet resize (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1477`)
- **Hazards:** relies on SKB-11's implicit state
- **Reify:** Low — derived gate, few sites

<a id="skb-31"></a>

### SKB-31 — text-input-v3 focus, enable and event batching
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/textinput.rs:27` (`Input.focus`, `.surface`, `.enabled`, `.preedit`, `.commit`); `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:548` (`Win.text_input`, `Win.caret`)
- **Encoding:** per seat focus, the surface role carets are given in, enabled flag, events buffered until `done`; per window wanted flag and caret
- **States:** `NoFocus` → `Focused(disabled)` ⇄ `Enabled` → (`Composing`) → `done` → `Enabled`; `Leave` → `NoFocus`
- **Transitions:** Enter (`textinput.rs:170`, enable `textinput.rs:173`); Leave (`textinput.rs:178`, `textinput.rs:185`, empty preedit `textinput.rs:189`); `set_wanted` (`textinput.rs:75`, `textinput.rs:84`); `reset` (`textinput.rs:109`); `apply` (`textinput.rs:127`); Preedit/Commit (`textinput.rs:193`, `textinput.rs:196`); Done (`textinput.rs:203`, `textinput.rs:207`)
- **Hazards:** `done`'s serial is ignored (commits aren't counted), so events for an older client state are applied; focus can name a closed window until Leave
- **Reify:** Medium — five fields plus two in `Win`, six handlers
- **Related:** compositor `desktop/aizome/crates/aizome-comp-core/src/handlers.rs`; main thread IME `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:1534`

<a id="skb-32"></a>

### SKB-32 — Scroll-layer tile lifecycle and raster queue
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/tiles.rs:84` (`Layers.pool`, `.stack`, `.spare`, `.queue`, `.top`); `tiles.rs:118` (`Tile.canvas`, `.dirty`, `.shown`, `.pending`); `tiles.rs:184` (`Overlay.shown`)
- **Encoding:** tiles exist while the main thread keeps them; `shown: Option<Shown>` while on screen; pending paints as a queue; a FIFO of tiles to rasterize that may list missing tiles; pooled surface sets; last stack order
- **States:** per tile `Kept(off screen)` → `PendingRaster` → `Rasterized` → `Shown` → `OffScreen(pooled surfaces)` → `Dropped(spare canvas)`
- **Transitions:** `place` drops tiles on grid/opacity change (`tiles.rs:328`) and remakes the overlay (`tiles.rs:336`); `drop_layer` (`tiles.rs:343`); `drop_tiles` (`tiles.rs:355`, `tiles.rs:360`); `rescaled` (`tiles.rs:376`, `tiles.rs:382`); `paint_tiles` creates covered tiles (`tiles.rs:474`) and queues (`tiles.rs:478`, `tiles.rs:394`); `rasterize_pending` (`tiles.rs:425`); present finishes shown tiles (`tiles.rs:667`), pools off-screen ones (`tiles.rs:657`), shows (`tiles.rs:804`), restacks (`tiles.rs:718`); `content_moved` (`tiles.rs:558`)
- **Hazards:** queue entries outlive their tiles and a remade tile is queued twice (both skipped harmlessly); while the queue is non-empty the render loop polls with a zero timeout (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:322`); `rescaled` forgets the overlay rect until the next PlaceLayer
- **Reify:** Medium — many collections jointly encode each tile's phase, with tests covering the core
- **Related:** main-thread layer manager in `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs`

<a id="skb-33"></a>

### SKB-33 — Video subsurface placement sync
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:101` (`VideoSurface.place`, `.applied`, `.crop`, `.synced`, `.inhibitor`)
- **Encoding:** asked vs applied placement, a synchronized flag, an optional idle inhibitor
- **States:** `New(synced)` → `Placing(synced, at the window's present)` → `Live(desync)` ⇄ `Moved(pending)`; `Removed`
- **Transitions:** `place` creates (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:228`), toggles inhibitor (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:248`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:251`), removes (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:210`); `before_commit` syncs (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:277`) and applies (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:294`); `after_commit` desyncs (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:306`); `show` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:326`); `clear` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:356`)
- **Hazards:** placement applies only at the window's next present (skipped while unconfigured, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1833`), so a new video stays synchronized and its frames don't show until then
- **Reify:** Low — one object, guarded by present

<a id="skb-34"></a>

### SKB-34 — Video compositor buffer cache and frame release
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:85` (`Cached.buffer`, `.in_flight`), `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:71` (`Videos.buffers`, `.since_sweep`), `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:140` (`Feedback`); also `desktop/aizome/sukumo/crates/sidestep-engine/src/video.rs:135` (`VideoFrame.shown`)
- **Encoding:** cache keyed by the producer's `Arc` address with a `Weak` to tell reuse; frames kept until release; one-shot report callbacks
- **States:** per frame `Queued` → `Attached(in flight)` → `Released` (report once: Presented | Discarded, or Discarded on drop); per cache entry `Live` → `Orphaned(producer gone, frames in flight)` → `Destroyed`
- **Transitions:** `cached` reuse/evict (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:366`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:371`) and create (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:407`); `show` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:339`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:346`); sweep (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:416`); `released` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:426`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:434`); feedback (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:561`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:563`); frame report/drop (`desktop/aizome/sukumo/crates/sidestep-engine/src/video.rs:149`, `desktop/aizome/sukumo/crates/sidestep-engine/src/video.rs:158`)
- **Hazards:** dmabuf `failed` is ignored (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:477`), leaving a dead buffer cached and attached; a buffer whose release never comes keeps its frames and their `keep` payloads (decoder surfaces) forever, since sweep only drops entries without strong references; one buffer on two videos is released for both at once
- **Reify:** Medium — lifetime spread across a cache, frames and compositor events

<a id="skb-35"></a>

### SKB-35 — Video capability discovery handshake
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:77` (`Videos.caps.known`); `desktop/aizome/sukumo/crates/sidestep-engine/src/video.rs:202` (`STARTED`), `desktop/aizome/sukumo/crates/sidestep-engine/src/video.rs:209` (`CAPS`)
- **Encoding:** a `known` bool set after a display sync; a global started flag; a published `Option<VideoCaps>` with a condvar
- **States:** `NotStarted` → `Started(waiting)` → `Known` → `Refined(dmabuf feedback after known)`
- **Transitions:** `render_started` (`desktop/aizome/sukumo/crates/sidestep-engine/src/video.rs:206`); sync callback (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:510`, `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:511`); feedback republishes if known (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:464`); null backend publishes at once (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/null.rs:260`); `caps` waits (`desktop/aizome/sukumo/crates/sidestep-engine/src/video.rs:230`)
- **Hazards:** nothing resets `STARTED` or `CAPS` when a render thread stops or another starts, so `caps()` can answer for a dead connection
- **Reify:** Low — two flags and a slot

<a id="skb-36"></a>

### SKB-36 — Foreign selection text, change count and identity
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:291` (`Shared.generation`, `.used`); `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:219` (`Foreign.offer`, `.local_write`, `.drag`, `.mimes`, `.text`, `.cache`, `.cached_for`); `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:199` (`enum Text`)
- **Encoding:** partially explicit: `Text` is an enum (Unread / Reading / Read) but news is decided across `generation`, `offer`, `before`, `mimes`, the cache keyed by generation, and the sticky `used` flag
- **States:** `Read(None)` → `Reading(offer, deadline)` → `Read(text)` | `Unread(too long)`; generation bumps only on news; identity `(offer, local_write)` moves at once
- **Transitions:** `foreign_offered` (`desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:387`, `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:399`, `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:402`, `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:404`); `drag_offered` (`desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:419`, `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:423`); `text_arrived` (`desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:441`, `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:453`, `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:455`); `foreign_text` resolves Unread (`desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:533`); `bump_identity` (`desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:351`, `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:352`); `cache_for` (`desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:284`); `remember` (`desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:705`)
- **Hazards:** a `Reading` whose `text_arrived` never comes (render thread died mid-read) makes every byte read wait out its own deadline (`desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:630`) and freezes the generation; read-ahead starts only after the first use, so the first paste after a foreign copy reads synchronously
- **Reify:** High — eight fields over two threads, seven transition sites
- **Related:** render side `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/selection.rs:424` and `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/primary.rs:108` (SKB-27); main-thread pasteboard in `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs`

<a id="skb-37"></a>

### SKB-37 — Clipboard read requests by token
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:230` (`Foreign.reads`), `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:238` (`Reading.active`, `.answer`, `.bounds`), `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:247` (`ReadBounds`)
- **Encoding:** token → reading with last-progress instant, `Option<Option<Vec<u8>>>` answer, identity, deadline and cancellation flag
- **States:** `Asked` → `Progressing` → `Answered(data | none)` | `TimedOut` | `Invalid(identity changed, deadline, cancelled)` → removed
- **Transitions:** `read_offer_bounded` cache hit (`desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:654`), insert (`desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:656`), ask (`desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:660`), answered (`desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:669`, `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:679`), idle timeout (`desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:687`, `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:689`), invalid (`desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:664`); `read_progressed` (`desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:464`); `read_arrived` (`desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:473`); `read_options` (`desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:485`); `cancel_read` (`desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:376`); render side `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/selection.rs:148` and `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/selection.rs:176`
- **Hazards:** a read while no render thread runs waits `READ_TIMEOUT` and then caches None for the generation, so later reads in that generation answer None at once
- **Reify:** Medium — explicit enough per token, but validity is re-derived in four places on two threads
- **Related:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/selection.rs:146` (SKB-29)

<a id="skb-38"></a>

### SKB-38 — Settings portal D-Bus session
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/desktop.rs:155` (`Bus.stream`, `.reader`, `.serial`); `desktop/aizome/sukumo/crates/sidestep-engine/src/desktop.rs:105` (`run`)
- **Encoding:** straight-line phases in `run`, a serial counter, socket timeouts switched off for the watch loop
- **States:** `Connecting` → `Authenticating` → `Hello` → `Subscribed` → `InitialReads` (appearance done after the third key) → `Watching` → `Ended` (any error; no reconnect)
- **Transitions:** `connect` AUTH/BEGIN (`desktop/aizome/sukumo/crates/sidestep-engine/src/desktop.rs:182`, `desktop/aizome/sukumo/crates/sidestep-engine/src/desktop.rs:188`); Hello (`desktop/aizome/sukumo/crates/sidestep-engine/src/desktop.rs:107`); AddMatch (`desktop/aizome/sukumo/crates/sidestep-engine/src/desktop.rs:112`); ReadOne/Read per key (`desktop/aizome/sukumo/crates/sidestep-engine/src/desktop.rs:123`, `desktop/aizome/sukumo/crates/sidestep-engine/src/desktop.rs:125`); `done` (`desktop/aizome/sukumo/crates/sidestep-engine/src/desktop.rs:134`); timeout off (`desktop/aizome/sukumo/crates/sidestep-engine/src/desktop.rs:137`); signal loop (`desktop/aizome/sukumo/crates/sidestep-engine/src/desktop.rs:139`); `call` skips non-replies (`desktop/aizome/sukumo/crates/sidestep-engine/src/desktop.rs:208`)
- **Hazards:** signals arriving during the initial calls are discarded by `call`; a timed-out call's late reply is discarded and a partial read can desynchronize the stream; any read error ends the watcher for good
- **Reify:** Medium — linear today, but with no retry and silent loss
- **Suspected bug:** at login, when D-Bus activation of xdg-desktop-portal takes over the 2 s timeout, ReadOne and Read for `color-scheme` both time out; the scheme stays the light default until the user changes it, so apps started then show light on a dark desktop (confidence: low)
- **Related:** xdg-desktop-portal Settings (external); `desktop/aizome/sukumo/crates/sidestep-engine/src/settings.rs` (SKB-41)
- **Reproduction:** not attempted; only suspected bugs on High machines were reproduced

<a id="skb-39"></a>

### SKB-39 — Momentum coast phases
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/momentum.rs:25` (`Coast.velocity`, `.last`, `.began`)
- **Encoding:** a struct the toolkit holds as `Option<Coast>`; `began` marks the first step; `slow` ends it
- **States:** `None` → `Starting(!began)` → `Coasting` → `Slow` (owner drops it)
- **Transitions:** `start` (`momentum.rs:46`); `step` (`momentum.rs:58`, `momentum.rs:64`); the owner stops it at a new scroll, click or window close
- **Hazards:** none observed
- **Reify:** Low — explicit physics; the phase is one bool
- **Related:** owner `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:1502`

<a id="skb-40"></a>

### SKB-40 — Published outputs first-snapshot handshake
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/outputs.rs:48` (`Published.settled`, `.generation`, `.changed`, `.asked`)
- **Encoding:** a settled bool, a snapshot counter, a sticky news flag, the instant of the first ask; condvar
- **States:** `Unsettled` → `Asked(waiting ≤ FIRST_SNAPSHOT)` → `Settled(generation n)`
- **Transitions:** `publish` (`desktop/aizome/sukumo/crates/sidestep-engine/src/outputs.rs:73`, `desktop/aizome/sukumo/crates/sidestep-engine/src/outputs.rs:74`, `desktop/aizome/sukumo/crates/sidestep-engine/src/outputs.rs:75`); `snapshot` asks once (`desktop/aizome/sukumo/crates/sidestep-engine/src/outputs.rs:92`), starts the render thread (`desktop/aizome/sukumo/crates/sidestep-engine/src/outputs.rs:94`), waits (`desktop/aizome/sukumo/crates/sidestep-engine/src/outputs.rs:101`); `latest` (`desktop/aizome/sukumo/crates/sidestep-engine/src/outputs.rs:112`)
- **Hazards:** after the first ask's second passes, readers get none without waiting until settled; nothing resets on render-thread restart
- **Reify:** Low — small, one lock
- **Related:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/outputs.rs` (SKB-19)

<a id="skb-41"></a>

### SKB-41 — Desktop appearance knowledge and change notification
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/settings.rs:32` (`STARTED`), `desktop/aizome/sukumo/crates/sidestep-engine/src/settings.rs:34` (`CHANGED`), `desktop/aizome/sukumo/crates/sidestep-engine/src/settings.rs:36` (`WAKE`), `desktop/aizome/sukumo/crates/sidestep-engine/src/settings.rs:43` (`SYSTEM` with `KNOWN` bit)
- **Encoding:** a start instant, a dirty flag consumed by the main thread, the render thread's sender once connected, appearance bits with a known bit
- **States:** `NotStarted` → `Waiting(first answer, ≤ 150 ms for first frames)` → `Known`; `Changed` → `Taken`; `Unconnected` → `Connected(render sender)`
- **Transitions:** `start` (`desktop/aizome/sukumo/crates/sidestep-engine/src/settings.rs:140`, overrides `desktop/aizome/sukumo/crates/sidestep-engine/src/settings.rs:148`); watcher updates (`desktop/aizome/sukumo/crates/sidestep-engine/src/settings.rs:157`, `desktop/aizome/sukumo/crates/sidestep-engine/src/settings.rs:161`); `done` defaults to light (`desktop/aizome/sukumo/crates/sidestep-engine/src/settings.rs:180`); `notify` (`desktop/aizome/sukumo/crates/sidestep-engine/src/settings.rs:190`, `desktop/aizome/sukumo/crates/sidestep-engine/src/settings.rs:194`); `connect` (`desktop/aizome/sukumo/crates/sidestep-engine/src/settings.rs:208`, `desktop/aizome/sukumo/crates/sidestep-engine/src/settings.rs:210`); `ready` (`desktop/aizome/sukumo/crates/sidestep-engine/src/settings.rs:216`); `take_changed` (`desktop/aizome/sukumo/crates/sidestep-engine/src/settings.rs:234`)
- **Hazards:** a change notified between `connect`'s send and storing `WAKE` doesn't reach that render thread's decorations; `WAKE` keeps a stopped render thread's sender
- **Reify:** Medium — globals on three threads with an ordering window
- **Related:** `desktop/aizome/sukumo/crates/sidestep-engine/src/desktop.rs` (SKB-38); decorations `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:274`

<a id="skb-42"></a>

### SKB-42 — Frame statistics collection and record/present pairing
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/stats.rs:30` (`STATE`), `desktop/aizome/sukumo/crates/sidestep-engine/src/stats.rs:185` (`Store.period`, `.since`, `.recorded`, `.frames`, `.latest`)
- **Encoding:** tri-state atomic (unknown, off, on); per-window FIFO of recordings capped at four; a period start instant
- **States:** `Unknown` → `Off` | `On`; per period `Collecting` → `Summarized`
- **Transitions:** `on` (`desktop/aizome/sukumo/crates/sidestep-engine/src/stats.rs:40`); `set_period` (`desktop/aizome/sukumo/crates/sidestep-engine/src/stats.rs:57`, `desktop/aizome/sukumo/crates/sidestep-engine/src/stats.rs:65`); `recorded` (`desktop/aizome/sukumo/crates/sidestep-engine/src/stats.rs:110`, `desktop/aizome/sukumo/crates/sidestep-engine/src/stats.rs:112`); `frame` pairs oldest (`desktop/aizome/sukumo/crates/sidestep-engine/src/stats.rs:123`); `add` rolls the period (`desktop/aizome/sukumo/crates/sidestep-engine/src/stats.rs:148`, `desktop/aizome/sukumo/crates/sidestep-engine/src/stats.rs:161`); `closed` (`desktop/aizome/sukumo/crates/sidestep-engine/src/stats.rs:171`)
- **Hazards:** pairing is FIFO by count, so a recording without a matching present shifts later pairings until the cap drops it
- **Reify:** Low — diagnostics only

## Already explicit

- None in this slice.

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs` | 1102 | machines | [SKB-01](#skb-01), [SKB-02](#skb-02) | CSD redraw cache and header buttons |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs` | 379 | machines | [SKB-03](#skb-03) | incoming drag session |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/drag_out.rs` | 150 | machines | [SKB-04](#skb-04), [SKB-28](#skb-28) | drag source; promised data via selection |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/foreign.rs` | 62 | machines | [SKB-05](#skb-05) | xdg-foreign import link |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/keyboard.rs` | 540 | machines | [SKB-06](#skb-06) | compose state; keymap translation is pure |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/layer.rs` | 135 | machines | [SKB-07](#skb-07), [SKB-11](#skb-11) | layer configure and close |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/lock.rs` | 82 | machines | [SKB-08](#skb-08) | ext-session-lock lifecycle |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/menubar.rs` | 121 | machines | [SKB-09](#skb-09), [SKB-17](#skb-17) | bar redraw; passthrough gap |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs` | 2456 | machines | [SKB-10](#skb-10), [SKB-11](#skb-11), [SKB-12](#skb-12), [SKB-13](#skb-13), [SKB-14](#skb-14), [SKB-15](#skb-15), [SKB-16](#skb-16), [SKB-17](#skb-17) | render thread, configure, pacing, buffers, popups |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/null.rs` | 576 | machines | [SKB-18](#skb-18) | test harness fake compositor |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/outputs.rs` | 242 | machines | [SKB-19](#skb-19) | output settle and learned values |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/primary.rs` | 179 | machines | [SKB-27](#skb-27), [SKB-29](#skb-29) | same ownership machine as clipboard |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/seat.rs` | 1143 | machines | [SKB-20](#skb-20), [SKB-21](#skb-21), [SKB-22](#skb-22), [SKB-23](#skb-23), [SKB-24](#skb-24), [SKB-25](#skb-25), [SKB-26](#skb-26) | focus, crossings, gestures, cursor, repeat |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/selection.rs` | 621 | machines | [SKB-27](#skb-27), [SKB-28](#skb-28), [SKB-29](#skb-29), [SKB-04](#skb-04) | ownership, promises, pipe transfers |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/sheet.rs` | 101 | machines | [SKB-30](#skb-30) | placement gated on parent geometry |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/textinput.rs` | 213 | machines | [SKB-31](#skb-31) | text-input-v3 enable and batching |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/tiles.rs` | 1203 | machines | [SKB-32](#skb-32), [SKB-14](#skb-14), [SKB-13](#skb-13) | tile lifecycle, uploads, frame fallback |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs` | 639 | machines | [SKB-33](#skb-33), [SKB-34](#skb-34), [SKB-35](#skb-35) | placement sync, buffer cache, caps |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs` | 1134 | machines | [SKB-36](#skb-36), [SKB-37](#skb-37) | foreign selection state and token reads |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/desktop.rs` | 602 | machines | [SKB-38](#skb-38) | portal D-Bus session phases |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/keys.rs` | 73 | none |  | constants |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/lib.rs` | 44 | none |  | module list |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/momentum.rs` | 108 | machines | [SKB-39](#skb-39) | coast phase flag |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/outputs.rs` | 118 | machines | [SKB-40](#skb-40) | first-snapshot wait handshake |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/protocol.rs` | 1226 | machines | [SKB-03](#skb-03), [SKB-08](#skb-08), [SKB-13](#skb-13), [SKB-28](#skb-28), [SKB-37](#skb-37) | message types; handshakes live elsewhere |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/settings.rs` | 268 | machines | [SKB-41](#skb-41) | appearance known and change flags |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/stats.rs` | 277 | machines | [SKB-42](#skb-42) | stats on/off and pairing |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/system.rs` | 265 | none |  | compute-once library and data lookups |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/trace.rs` | 32 | none |  | OnceLock env reads |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/video.rs` | 283 | machines | [SKB-34](#skb-34), [SKB-35](#skb-35) | frame report one-shot and caps slot |
