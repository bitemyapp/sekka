# SHA: Shell: main loop, startup, menubar, deskbar, desktop and palette

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/crates/aizome-shell`. Audited at `8e0acbe5`; line numbers refer to that commit.

19 files, 0 pending. 26 implicit machines: 7 High, 9 Medium, 10 Low.

## Machines

<a id="sha-01"></a>

### SHA-01 — Bar menu open / slide / press-to-close, and the Toolchest-to-palette handover
- **Where:** `desktop/aizome/crates/aizome-shell/src/barmenus.rs:17` (`BarMenu.bar`, `.slot`, `.showing`); `desktop/aizome/crates/aizome-shell/src/shell.rs:345` (`Shell.bar_menu`), `shell.rs:346` (`.menu_gen`), `shell.rs:348` (`.just_closed`); also `desktop/aizome/crates/aizome-shell/src/menubar.rs:324`, `menubar.rs:356`, `menubar.rs:981`
- **Encoding:** `bar_menu: Option<BarMenu>` holds the lit title and a showing serial taken from `menu_gen`. `just_closed: Option<(WindowKey, Slot, Instant)>` holds a 50 ms "this press closed it" window. The bar's titles send `Hover` only while `menu_open` (`on_hover_if`). A 250 ms `Msg::ToolchestHandedOver` timer ends the Toolchest's key-holding.
- **States:** `Closed` → `Open(bar, slot, showing)` → `JustClosed(bar, slot, t)` → `Closed`; `Open(Toolchest)` → `HandingOver` (palette open, Toolchest still holding keys) → `Closed`. Representable but inconsistent: `bar_menu` and `just_closed` both `Some` for different slots; `bar_menu` pointing at a bar window that is gone.
- **Transitions:** `as_bar_menu` Closed/Open→Open, `menu_gen += 1` (`barmenus.rs:34`, `barmenus.rs:35`); `menu_closed` Open→JustClosed when the serial matches (`barmenus.rs:41`–`barmenus.rs:46`); `pressed_to_close` consumes JustClosed, less than 50 ms old (`barmenus.rs:53`–`barmenus.rs:54`); any message other than `Bar::Press` clears JustClosed (`shell.rs:1500`–`shell.rs:1501`); `BarMsg::Press` reopens unless just closed (`shell.rs:683`); `hover_title` Open(a)→Open(b) (`barmenus.rs:59`–`barmenus.rs:66`); `bar_side` ←/→ (`barmenus.rs:70`–`barmenus.rs:75`); `focus_menu_bar` (F10) ends the banner, then Open (`barmenus.rs:81`–`barmenus.rs:92`); `open_slot` Toolchest/Menu (`shell.rs:754`, `shell.rs:776`); `toolchest_typed` opens the palette and arms 250 ms (`barmenus.rs:114`–`barmenus.rs:122`); `ToolchestHandedOver` closes every bar's menus (`shell.rs:1623`–`shell.rs:1628`)
- **Hazards:**
  - The press-to-close test is a time and message-order heuristic. A frame slower than 50 ms, or any thread message between `MenuClosed` and the `Press` (`Meters`, `Sys`, `Tick`, `Startup`), clears `just_closed`. The press then reopens the menu it meant to close.
  - The title stays lit if the kit never delivers `on_closed`.
  - `ToolchestHandedOver` closes whatever bar menu is open 250 ms later, including one opened after the palette was dismissed.
  - The timer is armed only on the first keystroke (palette `None`).
- **Reify:** High — three fields, about eight mutation sites in three files, and a timing heuristic.
- **Related:** `desktop/aizome/crates/aizome-shell/src/systray/items.rs:230` (status-item menus also call `as_bar_menu`); the aizome-kit menu popup `on_closed` / `on_type_until_closed`

<a id="sha-02"></a>

### SHA-02 — Shell-replacement carry handoff (state file, final word, retiring)
- **Where:** `desktop/aizome/crates/aizome-shell/src/carry.rs:23` (`Carried.pid`, `.last`/`final`, `.notes`, `.next_id`, `.folded`, `.notes_open`); `desktop/aizome/crates/aizome-shell/src/shell.rs:249` (`Shell.carried`), `shell.rs:250` (`.carry_ready`), `shell.rs:253` (`.retiring`)
- **Encoding:**
  - The state file is `$XDG_RUNTIME_DIR/aizome-shell-state.$WAYLAND_DISPLAY.json`, with `final` (field `last`) marking the last word.
  - `carried: Option<u64>` is the content mark of the last save, used to dedupe.
  - `carry_ready` means saving is allowed. It is false while a replacement waits for the old shell's last word.
  - `retiring` is terminal: the bus name has been lost.
  - `AIZOME_SHELL_PREVIOUS_PID` (env) and the `/proc/<pid>` liveness check drive the wait.
- **States:** new shell `Restoring` → `WaitingForLast` (`carry_ready=false`) → `Saving` → `Retiring` (old shell: final written, saves stop); file `Live(last=false)` → `Final(last=true)`. Invalid: `retiring && carry_ready` is normal, but nothing ever leaves `retiring`.
- **Transitions:**
  - Load and restore, skipping 1000 ids in a restart (`shell.rs:1050`–`shell.rs:1058`).
  - `carry_ready = previous.is_none()` (`shell.rs:1061`).
  - Socket `listen_replacing` (`shell.rs:1067`).
  - `wait_for_last` thread (`shell.rs:1142`–`shell.rs:1150`) polls every 25 ms until final, the old pid is gone, or 8 s pass (`carry.rs:109`–`carry.rs:123`).
  - `Msg::Carried` merges notes and sets `carry_ready = true` (`shell.rs:1591`–`shell.rs:1597`).
  - `after_update` saves while `carry_ready && !retiring` (`shell.rs:1826`–`shell.rs:1827`), deduped by mark (`shell.rs:1860`–`shell.rs:1863`), through a tmp file and rename (`carry.rs:88`–`carry.rs:94`).
  - `NoteEvent::NameLost` calls `save_carried(true)` and sets `retiring = true` (`shell.rs:1584`–`shell.rs:1588`).
  - `for_restore` migration drops the old keyboard offers (`carry.rs:41`–`carry.rs:54`).
- **Hazards:**
  - `Msg::Carried` merges only `notes`. Fold and expansion changes the old shell made after the new one read the file are dropped.
  - After 8 s, or once the pid is gone, a non-final file is accepted. A final word written later is lost.
  - Both shells write the same file during the overlap. The old one saves until `NameLost`, or until it stops if there is no bus. Because the new shell dedupes by mark, the file can be left holding the old shell's content.
  - The `+1000` id skip assumes fewer than 1000 notifications arrive during the overlap.
- **Reify:** High — the protocol spans two processes and a file, with a terminal flag and representable stale states.
- **Suspected bug:**
  - The scenario: another notification daemon (dunst or mako started by hand, or any client asking for `org.freedesktop.Notifications` with replace) takes the name. That fires `NameLost` (`notifications/server.rs:251`–`notifications/server.rs:253`).
  - The shell then writes a final state, sets `retiring = true` for good and keeps running. Nobody stops it, and `NameAcquired` never clears the flag.
  - From then on it never saves its notifications, folds or expansions again, so a later crash or reload restores stale state. It never asks for the name again.
  - Confidence: medium.
- **Related:** `desktop/aizome/crates/aizome-comp-core/src/session.rs` (starts the replacement and sets `AIZOME_SHELL_PREVIOUS_PID`); `desktop/aizome/crates/aizome-shell/src/notifications/server.rs:229` (`watch_name`, `RequestName` with `AllowReplacement|ReplaceExisting|DoNotQueue`)

<a id="sha-03"></a>

### SHA-03 — Deskbar auto-hide visibility
- **Where:** `desktop/aizome/crates/aizome-shell/src/deskbar_glue.rs:20` (`Visibility.revealed`, `.inside`, `.control_inside`, `.menu`, `.dragging`, `.native_drag`, `.generation`); `desktop/aizome/crates/aizome-shell/src/shell.rs:318`
- **Encoding:** seven fields plus the `deskbar_cfg.auto_hide`/`collapsed` settings. A 350 ms `DeskbarMsg::AutoHide(generation)` timer is valid only if `generation` is unchanged. `self.serial` tags the Deskbar's menus.
- **States:** `Hidden` → `Revealed(held: pointer | control | menu | row drag | native drag)` → `Revealed(armed, gen)` → `Hidden`; inert while not auto-hide or collapsed. Representable invalid combinations: `inside=true` after its surface is gone; `menu=Some` with no menu open; `native_drag=None` during a live platform drag (after a settings reset).
- **Transitions:**
  - `deskbar_hover`: entering reveals (`deskbar_glue.rs:59`–`deskbar_glue.rs:60`), leaving arms the hide (`deskbar_glue.rs:62`).
  - `arm_deskbar_hide` bumps the generation and sets the timer (`deskbar_glue.rs:69`–`deskbar_glue.rs:71`).
  - `deskbar_menu_style` sets `menu=Some` (`deskbar_glue.rs:76`–`deskbar_glue.rs:77`).
  - The `AutoHide` handler hides only if nothing holds the Deskbar (`deskbar_glue.rs:465`–`deskbar_glue.rs:476`).
  - `MenuClosed` releases the menu and re-arms (`deskbar_glue.rs:478`–`deskbar_glue.rs:481`).
  - Row-drag holds are set and cleared at `deskbar_glue.rs:371`, `deskbar_glue.rs:376`, `deskbar_glue.rs:397`, `deskbar_glue.rs:400`.
  - A vanished drag source is dropped (`deskbar_glue.rs:237`–`deskbar_glue.rs:246`). Reopen and close resets: `deskbar_glue.rs:262`–`deskbar_glue.rs:265`, `deskbar_glue.rs:276`–`deskbar_glue.rs:279`. Re-arm after a reopen: `deskbar_glue.rs:311`–`deskbar_glue.rs:312`.
  - Surface-side handlers: `pointer_presence` (`shell.rs:1747`–`shell.rs:1755`), `drag_started` (`shell.rs:1760`–`shell.rs:1765`), `drag_ended` (`shell.rs:1768`–`shell.rs:1771`).
  - `read_settings` replaces the whole struct when `auto_hide` or `collapsed` changes (`shell.rs:505`–`shell.rs:510`).
  - `SetDeskbarCollapsed` sets `revealed = !on` (`shell.rs:907`–`shell.rs:910`).
  - The bar's `N apps` control sends `DeskbarHover` (`menubar.rs:814`–`menubar.rs:815`, `shell.rs:678`).
- **Hazards:**
  - `read_settings` wipes `menu`, `dragging` and `native_drag` while they are live. The Deskbar can then hide under its own open menu, or close mid-drag.
  - `close_requested` for `Surface::Deskbar` (`shell.rs:1727`–`shell.rs:1731`) does not reset `inside`, `control_inside` or `menu`.
  - `SetDeskbarCollapsed(false)` from `aizomectl` with auto-hide sets `revealed` without arming a hide.
  - `arm_deskbar_hide` ignores drags, while `AutoHide` checks them. Correctness relies on every drag end re-arming.
- **Reify:** High — seven fields, a generation timer, and about a dozen mutation sites in three files.
- **Suspected bug:**
  - The scenario: the compositor closes an auto-hide Deskbar while the pointer is over it, because its output went or the main display changed.
  - `close_requested` leaves `inside=true`, and `sync_deskbar` reopens the Deskbar through `(true, None)` with `reopened=false`, so no hide is armed.
  - `AutoHide` then rejects every timer, and the Deskbar stays revealed until the pointer enters and leaves the new surface.
  - Confidence: low; it depends on whether the kit delivers a leave event first.
- **Related:** `desktop/aizome/crates/aizome-comp-core/src/state.rs` (treats `aizome-deskbar-transient` as an overlay, not a layout participant)

<a id="sha-04"></a>

### SHA-04 — Deskbar surface lifecycle (open, resize, reopen on display/kind/edge change)
- **Where:** `desktop/aizome/crates/aizome-shell/src/shell.rs:238` (`Shell.deskbar`), `shell.rs:316` (`.deskbar_size`), `shell.rs:319` (`.deskbar_transient`), `shell.rs:322` (`.deskbar_output`), `shell.rs:324` (`.deskbar_side`); logic at `desktop/aizome/crates/aizome-shell/src/deskbar_glue.rs:234`
- **Encoding:** an `Option<WindowKey>` plus four "what it was opened with" fields, compared against the settings and the primary output on every `after_update`.
- **States:** `Closed` → `Open(output, side, transient, size)` → `Reopening` (closed and reopened in one pass) | `Closed`. Representable: `deskbar=None` with stale `deskbar_size`, `deskbar_output` and `deskbar_side` after `close_requested`.
- **Transitions:** reopen when moved, kind or edge changed (`deskbar_glue.rs:252`–`deskbar_glue.rs:266`); close when not wanted (`deskbar_glue.rs:270`–`deskbar_glue.rs:280`); open, recording output, side and kind (`deskbar_glue.rs:282`–`deskbar_glue.rs:308`); resize on content change (`deskbar_glue.rs:315`–`deskbar_glue.rs:322`); compositor close (`shell.rs:1727`–`shell.rs:1730`); `sync_screens` runs it first (`shell.rs:591`) and `after_update` every time (`shell.rs:1834`)
- **Hazards:** the visibility and drag resets are duplicated in two branches and missing from `close_requested` (see SHA-03). Every reopen makes the compositor refit the windows beside it.
- **Reify:** Medium — five fields, but the transitions are mostly in one function.
- **Related:** `desktop/aizome/crates/aizome-comp-core/src/state.rs` (layer namespace `aizome-deskbar` fits windows beside it)

<a id="sha-05"></a>

### SHA-05 — Deskbar row drags (reorder an app; window onto a Desk)
- **Where:** `desktop/aizome/crates/aizome-shell/src/shell.rs:317` (`Shell.deskbar_drag`); `desktop/aizome/crates/aizome-shell/src/deskbar_glue.rs:25` (`Visibility.dragging`), `deskbar_glue.rs:32` (`enum DragTarget`); emitted at `desktop/aizome/crates/aizome-shell/src/deskbar.rs:330`, `deskbar.rs:382`, `desktop/aizome/crates/aizome-shell/src/deskbar_compact.rs:188`, `deskbar_compact.rs:266`
- **Encoding:** `deskbar_drag: Option<(app_id, press point)>` (app drags only) plus `Visibility.dragging: Option<DragTarget>`
- **States:** `Idle` → `DraggingApp(id, from)` | `DraggingWindow(id)` → `Idle` (reorder, `MoveToDesk`, or dropped)
- **Transitions:** `AppDrag::Start` (`deskbar_glue.rs:370`–`deskbar_glue.rs:372`); `AppDrag::End` takes the drag, checks the source and that the pointer is inside the width, then reorders (`deskbar_glue.rs:375`–`deskbar_glue.rs:385`); `WindowDrag::Start`/`End`, with `MoveToDesk` if released over the desk strip (`deskbar_glue.rs:396`–`deskbar_glue.rs:411`); a vanished source cancels (`deskbar_glue.rs:237`–`deskbar_glue.rs:246`); surface reopen or close cancels (`deskbar_glue.rs:263`, `deskbar_glue.rs:266`, `deskbar_glue.rs:278`, `deskbar_glue.rs:280`)
- **Hazards:**
  - `WindowDrag::End` clears `dragging` without checking that the drag is the same one.
  - `deskbar_drag` is app-only, but the window-drag cancel path also clears it.
  - `desk_at_deskbar_point` uses `desk_strip`, which is recomputed each `after_update` (`shell.rs:1845`) and can be one update stale.
  - `lock()` does not cancel these drags (`lock.rs:316` ends only the edge and grip drags).
- **Reify:** Medium — two fields with a separate cancel path.

<a id="sha-06"></a>

### SHA-06 — Deskbar edge-resize drag and its preview surface
- **Where:** `desktop/aizome/crates/aizome-shell/src/shell.rs:327` (`Shell.deskbar_resize`), `shell.rs:329` (`.deskbar_preview`); handlers at `desktop/aizome/crates/aizome-shell/src/deskbar_glue.rs:510`; element at `desktop/aizome/crates/aizome-shell/src/deskbar.rs:538`
- **Encoding:** `deskbar_resize: Option<(width at press, x at press)>` plus `deskbar_preview: Option<(overlay WindowKey, width)>`
- **States:** `Idle` → `Pressed(from, x)` → `Previewing(window, w)` → `Idle` (commit `PlaceDeskbar`, or cancel). Representable: `preview=Some` with `resize=None` is avoided by `end_deskbar_resize`; `resize=Some` with `preview=None` happens after the compositor closes the preview.
- **Transitions:** Start (`deskbar_glue.rs:516`–`deskbar_glue.rs:518`); Move opens or updates the preview (`deskbar_glue.rs:536`, `deskbar_glue.rs:584`–`deskbar_glue.rs:607`); End commits if the width changed (`deskbar_glue.rs:530`–`deskbar_glue.rs:534`); `end_deskbar_resize` (`deskbar_glue.rs:611`–`deskbar_glue.rs:616`); cancel when the pointer leaves (`shell.rs:1752`–`shell.rs:1753`), on Deskbar close (`shell.rs:1730`), on reopen or close (`deskbar_glue.rs:261`, `deskbar_glue.rs:275`); preview closed by the compositor (`shell.rs:1732`–`shell.rs:1733`)
- **Hazards:** a Move or End after a cancel is dropped because `deskbar_resize` is `None`, which is intended. The preview's size is fixed at open and not refreshed when the display changes mid-drag.
- **Reify:** Medium — two coupled options and five cancel sites.

<a id="sha-07"></a>

### SHA-07 — Deskbar grip-move drag and its landing surface
- **Where:** `desktop/aizome/crates/aizome-shell/src/shell.rs:332` (`Shell.deskbar_landing`); projected as `desktop/aizome/crates/aizome-shell/src/deskbar.rs:190` `enum Moving` via `desktop/aizome/crates/aizome-shell/src/deskbar_glue.rs:224`–`deskbar_glue.rs:228`; handlers at `deskbar_glue.rs:484`
- **Encoding:** `deskbar_landing: Option<(overlay WindowKey, leaving)>`. The `Moving` enum (`No`/`Held`/`Leaving`) is derived from it for the view, so the machine is partially explicit.
- **States:** `No` → `Held` ⇄ `Leaving` → `No` (moves to the other edge if released while `Leaving`)
- **Transitions:** Start opens the landing (`deskbar_glue.rs:493`, `deskbar_glue.rs:635`–`deskbar_glue.rs:657`); Move flips `leaving` (`deskbar_glue.rs:496`–`deskbar_glue.rs:499`); End commits if live and leaving (`deskbar_glue.rs:501`–`deskbar_glue.rs:506`); `end_deskbar_move` (`deskbar_glue.rs:662`–`deskbar_glue.rs:666`); compositor close (`shell.rs:1735`–`shell.rs:1736`); cancel paths are shared with SHA-06 (`deskbar_glue.rs:672`–`deskbar_glue.rs:675`)
- **Hazards:**
  - If the compositor closes the landing mid-drag, End sees `live=false` and discards a real move.
  - With no primary output at Start, the drag runs but can never commit.
- **Reify:** Low — one option that already acts as the enum.

<a id="sha-08"></a>

### SHA-08 — Per-app Deskbar fold and notification-expansion toggles
- **Where:** `desktop/aizome/crates/aizome-shell/src/shell.rs:244` (`Shell.folded`), `shell.rs:255` (`.notes_open`); logic at `desktop/aizome/crates/aizome-shell/src/deskbar_glue.rs:126`, `deskbar_glue.rs:388`, `deskbar_glue.rs:416`
- **Encoding:** `folded: HashMap<app, bool>`, where a missing key means automatic (three or more windows and not focused). `notes_open: Vec<app>` means every unread card shows. Both are carried across restarts (SHA-02).
- **States:** fold per app `Auto` → `Folded` ⇄ `Unfolded`; notes per app `Collapsed` → `Expanded` → `Collapsed`, which also marks the notes read.
- **Transitions:** `Fold` writes the explicit opposite of the current state (`deskbar_glue.rs:389`–`deskbar_glue.rs:391`); `ShowNotes` toggles, and closing calls `mark_read` (`deskbar_glue.rs:417`–`deskbar_glue.rs:421`); the model shows 8 cards versus 1 (`deskbar_glue.rs:186`–`deskbar_glue.rs:189`); restore (`shell.rs:1057`–`shell.rs:1058`); save (`shell.rs:1854`–`shell.rs:1870`)
- **Hazards:**
  - Nothing returns an app to `Auto`.
  - `notes_open` entries are never pruned when an app's unread notes reach zero some other way, so its next notification arrives expanded.
  - Both collections grow without bound across restarts.
- **Reify:** Low — small, per-app toggles.

<a id="sha-09"></a>

### SHA-09 — Desktop icon selection, icon drag and rubber band
- **Where:** `desktop/aizome/crates/aizome-shell/src/desktop.rs:81` (`DesktopState.selected`, `.positions`, `.band`, `.dragging`); `desktop/aizome/crates/aizome-shell/src/shell.rs:288` (`Shell.drag_grab`); handlers at `desktop/aizome/crates/aizome-shell/src/desktop_glue.rs:170`
- **Encoding:** `band: Option<(from, to)>`, `dragging: Option<(key, at)>`, `drag_grab: Vec2` (the press offset, held separately on `Shell`), `selected: Vec<key>`. `positions` is persisted in `desktop.json`.
- **States:** `Idle` → `DraggingIcon(key, at)` → `Idle` (moved and snapped, or trashed); `Idle` → `Banding(a, b)` → `Idle`. Representable: `dragging` and `band` both `Some`; `drag_grab` stale from a previous drag.
- **Transitions:** Select/Clear (`desktop_glue.rs:170`–`desktop_glue.rs:181`); `IconDrag` Start (`desktop_glue.rs:195`–`desktop_glue.rs:197`), Move (`desktop_glue.rs:199`), End: trash or snap, then save (`desktop_glue.rs:200`–`desktop_glue.rs:218`); `Band` Start/Move/End (`desktop_glue.rs:225`–`desktop_glue.rs:233`); load (`desktop.rs:98`, `shell.rs:1025`); save (`desktop.rs:106`)
- **Hazards:**
  - One `DesktopState` serves every output, so a band or dragged icon is drawn at the same local position on every output's desktop (`desktop.rs:298`–`desktop.rs:305`).
  - A drag or band whose source surface is closed (output gone) or covered by the lock never gets End. It stays drawn until the next drag.
  - `selected` keeps keys of icons that have gone.
- **Reify:** Medium — four fields split across two structs, with no cancel path.

<a id="sha-10"></a>

### SHA-10 — Wallpaper scaling pipeline
- **Where:** `desktop/aizome/crates/aizome-shell/src/shell.rs:279` (`Shell.wallpapers`), `shell.rs:281` (`.wall_pending`), `shell.rs:282` (`.wall_failed`), `shell.rs:284` (`.wall_paths`); logic at `desktop/aizome/crates/aizome-shell/src/desktop_glue.rs:44`
- **Encoding:** per `WallKey` (path and pixel size): membership in `wall_pending`, `wallpapers` or `wall_failed` (keyed by path only)
- **States:** `Absent` → `Pending` → `Ready` | `Failed(path)`; `Ready` → `Absent` when no output wants it
- **Transitions:** `refresh_wall_paths` clears failures when the paths change (`desktop_glue.rs:46`–`desktop_glue.rs:48`); `prepare_wallpapers` prunes, marks pending and spawns (`desktop_glue.rs:92`–`desktop_glue.rs:105`), called on every `after_update` (`shell.rs:1830`); `wallpaper_ready` (`desktop_glue.rs:111`–`desktop_glue.rs:119`, `shell.rs:1605`)
- **Hazards:**
  - A transient read failure is sticky for that path until the setting changes.
  - In-flight scaling is never cancelled. A theme flip or scale change mid-scale spawns both.
  - `wallpapers_ready` (`desktop_glue.rs:80`) has no callers.
- **Reify:** Low — a per-key tri-state in one module.

<a id="sha-11"></a>

### SHA-11 — Change-polling watcher threads and the one-shot desktop refresh
- **Where:** `desktop/aizome/crates/aizome-shell/src/desktop_glue.rs:400` (`watch_desktop`, `last` modification stamps); `desktop/aizome/crates/aizome-shell/src/shell.rs:1088` (meters thread, `last` shown readings); also `desktop_glue.rs:391` (`refresh_desktop_items`)
- **Encoding:** each loop holds a thread-local `last` value and sends only when it changes. The loop exits when a send fails (the shell is gone).
- **States:** `Polling(last)` → `Sent(new last)` → `Polling` … → `Exited`
- **Transitions:** watcher compares and sends (`desktop_glue.rs:410`–`desktop_glue.rs:415`); meters (`shell.rs:1102`–`shell.rs:1106`); one-shot refresh after a trash action (`desktop_glue.rs:210`, `desktop_glue.rs:254`); results applied (`shell.rs:1577`–`shell.rs:1579`)
- **Hazards:** `DesktopItems` from the watcher and from `refresh_desktop_items` carry no sequence number, so a stale listing can land last. It heals within one 2 s poll once the stamp changes.
- **Reify:** Low — local loops.

<a id="sha-12"></a>

### SHA-12 — Palette open/close and its drawn-ahead spare window
- **Where:** `desktop/aizome/crates/aizome-shell/src/shell.rs:260` (`Shell.palette`), `shell.rs:262` (`.palette_ahead`); `desktop/aizome/crates/aizome-shell/src/palette/mod.rs:26` (`PaletteState.window`, `.selected`, `.scroll`, `.scope`, `.query`), `palette/mod.rs:57` (`Ahead.on`, `.spare`, `.output`, `.scale`, `.inputs`, `.asked`, `.bumped`)
- **Encoding:** `palette: Option<PaletteState>` is the open palette. `Ahead` holds a hidden spare window and its state:
  - `scale=None` means the spare is not drawn at the current scale.
  - `inputs` is a hash of what the palette opens with.
  - `asked` is a debounce counter for `PreparePalette(n)`.
  - `bumped` counts changes that the hash does not see.
- **States:** `Cold` (`on=false`: windows opened and closed) → `Warm-closed(spare, drawn | stale)` → `Open(palette)` → `Warm-closed`; spare re-created per output. Within Open: selection `Row(i)` → second click on the same row runs it. Invalid but representable: `palette` holding a dead window after the compositor closes it.
- **Transitions:**
  - `WarmPalette` at 2 s sets `on = true` (`shell.rs:1606`–`shell.rs:1613`).
  - `open_palette`: re-scope if already open (`palette/mod.rs:263`–`palette/mod.rs:267`), reuse the spare (`palette/mod.rs:273`–`palette/mod.rs:278`), or open a new window (`palette/mod.rs:280`–`palette/mod.rs:285`).
  - `close_palette` hides the window into the spare, or closes it (`palette/mod.rs:311`–`palette/mod.rs:321`). `toggle_palette` (`palette/mod.rs:250`–`palette/mod.rs:254`).
  - `spare_palette_on` drops the spare for another output (`palette/mod.rs:926`–`palette/mod.rs:932`).
  - `keep_palette_ahead` compares the inputs hash on every update (`palette/mod.rs:949`–`palette/mod.rs:952`, `shell.rs:1838`). `palette_ahead_soon` bumps `asked` (`palette/mod.rs:939`–`palette/mod.rs:940`). `PreparePalette(n == asked)` (`shell.rs:1615`–`shell.rs:1618`) runs `prepare_palette` (`palette/mod.rs:885`–`palette/mod.rs:916`).
  - `bumped++` at `palette/mod.rs:595`, `shell.rs:1303`, `shell.rs:1308`.
  - `palette_msg` Click selects, then runs (`palette/mod.rs:643`–`palette/mod.rs:650`). `palette_move` and `palette_cycle` (`palette/mod.rs:657`–`palette/mod.rs:676`). `run_selected` closes unless keep-open (`palette/mod.rs:692`–`palette/mod.rs:703`).
  - Dismiss (`shell.rs:1391`).
- **Hazards:**
  - `close_requested` (`shell.rs:1703`) handles neither `palette.window` nor `palette_ahead.spare`. A palette whose layer the compositor closes stays `Some` with a dead window.
  - `open_palette` while one is already open only re-ranks into the existing window.
  - The spare holds a 5.6 MB surface per output.
  - `inputs` hashes the `Arc` pointer of `apps` and the length of `settings_list`, so a settings list of the same length but different content is missed.
- **Reify:** High — two structs, nine phase fields, a debounce counter, and window lifecycles on several paths.
- **Suspected bug:**
  - The scenario: the palette is open when its output is unplugged.
  - The compositor closes the layer and `close_requested` forgets only the surface, so `self.palette` stays `Some(dead window)`.
  - Typing in the Toolchest then calls `toolchest_typed` → `open_palette`, which re-ranks into the dead window. Nothing shows, and the handover timer is not armed. An `aizomectl` palette open with a query is ignored the same way.
  - It recovers only after a toggle closes the ghost.
  - Confidence: medium.

<a id="sha-13"></a>

### SHA-13 — Palette file search, index lifetime and per-opening extras
- **Where:** `desktop/aizome/crates/aizome-shell/src/shell.rs:264` (`Shell.file_index`); `desktop/aizome/crates/aizome-shell/src/palette/mod.rs:35` (`PaletteState.files`), `palette/mod.rs:36` (`.files_for`), `palette/mod.rs:42` (`.search`), `palette/mod.rs:45` (`.recent`), `palette/mod.rs:46` (`.boosts`)
- **Encoding:**
  - `file_index` is held only while the palette is open.
  - `search` is a sequence number for dropping stale `Files(seq)` results.
  - `files_for` names the query the files are for; `"\u{0}"` is a "search again" sentinel.
  - The `recent` and `boosts` arrive from a thread after opening.
- **States:** `NoIndex` → `IndexLoading` → `Indexed`; per query `Fresh(files_for == q)` → `Searching(seq)` → `Fresh`; `Recent` (empty query)
- **Transitions:**
  - Opening resets `search=0` and `files_for` to the sentinel (`palette/mod.rs:344`, `palette/mod.rs:349`), spawns `Recent` (`palette/mod.rs:290`–`palette/mod.rs:300`) and `index_soon(true)` (`palette/mod.rs:302`–`palette/mod.rs:303`).
  - `rank` shows recent files for an empty query (`palette/mod.rs:472`–`palette/mod.rs:475`); otherwise it increments `search` and spawns a search (`palette/mod.rs:476`–`palette/mod.rs:486`).
  - `Index` is accepted only while open and resets the sentinel (`palette/mod.rs:582`–`palette/mod.rs:589`).
  - `Recent` (`palette/mod.rs:597`–`palette/mod.rs:603`).
  - `Files(seq)` is accepted if `search == seq` (`palette/mod.rs:612`–`palette/mod.rs:618`).
  - Close drops the index (`palette/mod.rs:310`).
- **Hazards:**
  - `search` restarts at 0 on every opening, so the sequence numbers are not unique across openings.
  - `Recent` from one opening can be applied to the next.
  - Every opening reloads the index from disk. Until it arrives, typed queries show no file rows.
- **Reify:** High — five fields, a sentinel string, and a stale-result counter that resets.
- **Suspected bug:**
  - The scenario: type a query, close the palette with Esc before its search returns, then reopen and type again. The new search also gets `seq=1`.
  - The old `Files(1, …)` is accepted for the new opening, with `files_for` set to the new query.
  - If it arrives after the new results, the old query's files stay shown until the next keystroke.
  - Confidence: low; the old search must outlast closing, reopening and an index load.

<a id="sha-14"></a>

### SHA-14 — File index cache: validity, rebuild and refresh timer
- **Where:** `desktop/aizome/crates/aizome-shell/src/palette/mod.rs:811` (`index_soon`); `palette/mod.rs:800` (`start_palette_threads`); on disk: the `FileIndex::default_cache_path()` file and the `.key` file beside it
- **Encoding:**
  - The cache file has an age; the `.key` file names the roots and skips it was built over.
  - `cx.every(reindex_minutes)` delivers `PaletteEvent::Refresh`.
  - `keep` decides whether the result goes to the palette.
  - There is no in-flight flag.
- **States:** disk `Missing` | `Fresh(key match, age < max)` | `Stale` → `Building` → `Fresh`; memory per SHA-13
- **Transitions:** a load is fresh if the key matches and the age is within limit (`palette/mod.rs:821`–`palette/mod.rs:823`), else build, save and write the key (`palette/mod.rs:824`–`palette/mod.rs:832`); startup and periodic refresh (`palette/mod.rs:803`–`palette/mod.rs:805`); `Refresh` only while closed (`palette/mod.rs:606`–`palette/mod.rs:609`); on open (`palette/mod.rs:302`–`palette/mod.rs:303`)
- **Hazards:** no guard against concurrent builds. `FileIndex::save` uses a fixed `<cache>.tmp` name (`aizome-search/src/files.rs:300`), and the key file is written separately from the index, which is not atomic.
- **Reify:** Medium — persisted validity rules plus unguarded concurrency.
- **Suspected bug:**
  - The scenario: at login the cache is older than `reindex_minutes`, so the startup `index_soon(false)` begins a rebuild. Opening the palette before it finishes runs `index_soon(true)`, which sees the same stale cache and builds again.
  - Both threads `File::create` the same `.tmp` and rename it. This wastes a full home-directory scan and can tear the cache.
  - The same stacking happens when a refresh tick fires while a slow build is still running.
  - Confidence: medium.

<a id="sha-15"></a>

### SHA-15 — Frecency store: refresh on staleness, record on a thread
- **Where:** `desktop/aizome/crates/aizome-shell/src/shell.rs:263` (`Shell.frecency`); `desktop/aizome/crates/aizome-shell/src/palette/mod.rs:774` (`record_frecency`), `palette/mod.rs:792` (`refresh_frecency`)
- **Encoding:** the in-memory `Frecency`, with its `seen` file stamp, and the history file
- **States:** `Current` → `Stale(file changed)` → `Reloaded`; per pick, memory `Visited` → `Recording(thread)` → `Replaced(store as written)`
- **Transitions:** load at start (`shell.rs:1023`); refresh when stale, on open and on prepare (`palette/mod.rs:262`, `palette/mod.rs:890`, `palette/mod.rs:793`–`palette/mod.rs:794`); `run_selected` visits in memory and spawns a record (`palette/mod.rs:687`–`palette/mod.rs:690`); the `Frecency` reply replaces the store and bumps `ahead` (`palette/mod.rs:592`–`palette/mod.rs:595`)
- **Hazards:**
  - Each pick runs its own read-modify-write thread (`aizome-search/src/frecency.rs:107`–`aizome-search/src/frecency.rs:114`), sharing the tmp name `json.tmp`.
  - Quick ❖↩ picks race: a pick can be lost, or the second rename fails.
  - Replies can arrive out of order and replace a newer store with an older one. The next `is_stale` check heals it.
- **Reify:** Low — low impact, and it self-heals.

<a id="sha-16"></a>

### SHA-16 — Shelf items and parked-image files
- **Where:** `desktop/aizome/crates/aizome-shell/src/shelf.rs:141` (`Shelf.items`, `.dir`); `desktop/aizome/crates/aizome-shell/src/shell.rs:242`
- **Encoding:** `shelf.json` (through `shelf.json.tmp`) plus `image-<ms>.<ext>` files, collected by `save`
- **States:** item `Parked(memory)` → `Saved`; image file `Written` → `Referenced` → `Orphaned` → `Deleted` (on the next save)
- **Transitions:** load at start (`shelf.rs:152`, `shell.rs:1024`); `park` (`shelf.rs:180`–`shelf.rs:182`); `park_image` writes the file before the item is saved (`shelf.rs:186`–`shelf.rs:192`); `save` and its GC (`shelf.rs:160`–`shelf.rs:175`); drop to park, then save (`deskbar_glue.rs:452`–`deskbar_glue.rs:463`); `ShelfRemove { index }` uses the index captured when the menu was built (`deskbar_glue.rs:446`, `shell.rs:958`–`shell.rs:960`)
- **Hazards:**
  - Two overlapping shells during a replacement each save their own items, so the last writer wins.
  - The GC deletes image files the other shell parked.
  - Removal goes by index, not identity.
  - The file is never reloaded.
  - `park_image` has no callers in `src`.
- **Reify:** Low — small, persisted lifecycle.

<a id="sha-17"></a>

### SHA-17 — Per-output bar/desktop surfaces and the surface registry
- **Where:** `desktop/aizome/crates/aizome-shell/src/shell.rs:237` (`Shell.surfaces`), `shell.rs:236` (`.screens`), `shell.rs:170` (`Screen.bar`, `.desktop`)
- **Encoding:** `surfaces: HashMap<WindowKey, Surface>` plus per-feature `Option<WindowKey>` fields: screens, `deskbar`, the palette, `tear_shown`, `startup.window`, `panel`, `lock`, `cards`, `flash`. They are reconciled in `close_requested`.
- **States:** per output `Absent` → `Present{bar: None|Open, desktop: None|Open}` → `Gone`; per surface `Open` → `ClosedByCompositor` (forgotten until the next sync)
- **Transitions:** `sync_screens` closes surfaces of gone outputs (`shell.rs:572`–`shell.rs:580`) and opens missing ones (`shell.rs:602`–`shell.rs:623`); `close_requested` forgets the surface and clears `bar`/`desktop` (`shell.rs:1712`–`shell.rs:1726`), the Deskbar, preview and landing (`shell.rs:1727`–`shell.rs:1737`), startup and panel (`shell.rs:1704`–`shell.rs:1709`); sync triggers: launch (`shell.rs:1494`), Connected (`shell.rs:1176`), OutputsChanged (`shell.rs:1207`), `outputs_changed` (`shell.rs:1742`–`shell.rs:1744`), Resumed (`shell.rs:2060`)
- **Hazards:**
  - A bar or desktop layer the compositor closes while its output stays is reopened only at the next outputs change, resume or reconnect.
  - `close_requested` does not reconcile the palette, the palette spare or `tear_shown` (see SHA-12 and SHA-25), nor the cards, flash, osd, confirm and auth owners.
- **Reify:** Medium — two sources of truth with a single reconcile point that is incomplete.
- **Related:** the aizome-kit `close_requested` dispatch (`desktop/aizome/crates/aizome-kit/src/app.rs:221`)

<a id="sha-18"></a>

### SHA-18 — Desk rename in place
- **Where:** `desktop/aizome/crates/aizome-shell/src/shell.rs:241` (`Shell.renaming`); view at `desktop/aizome/crates/aizome-shell/src/menubar.rs:407`
- **Encoding:** `renaming: Option<(desk index, text so far)>`
- **States:** `Idle` → `Editing(index, text)` → `Idle` (`RenameDesk` sent, or cancelled)
- **Transitions:** `Action::RenameDesk` (`shell.rs:890`–`shell.rs:893`), entered from a double-click (`menubar.rs:493`) or the desk menu (`menus/build.rs:376`); `RenameText` (`shell.rs:690`–`shell.rs:693`); `RenameDone` (`shell.rs:695`–`shell.rs:703`); `RenameCancel` on Esc (`shell.rs:705`, `menubar.rs:414`–`menubar.rs:417`)
- **Hazards:** there is no exit on focus loss, lock or desk changes. The field persists until Enter or Esc reaches it. The edit is keyed by index, not desk id.
- **Reify:** Medium — small, but keyed by an unstable index.
- **Suspected bug:**
  - The scenario: while Desk 3 is being renamed, a lower desk is removed (shortcut, `aizomectl`, another bar), so the desks renumber.
  - The field jumps to whichever desk now has index 3. If none does, it vanishes while `renaming` stays `Some` and reappears on the next desk created as 3.
  - Enter renames the wrong desk.
  - Confidence: medium.
- **Related:** the compositor's desk renumbering (`Request::RenameDesk` with `DeskSel::Index`)

<a id="sha-19"></a>

### SHA-19 — Launch waiting for an activation token
- **Where:** `desktop/aizome/crates/aizome-shell/src/shell.rs:374` (`Shell.launches`)
- **Encoding:** `HashMap<ActivationRequest, (Launch, cwd)>` plus a 5 s `Msg::ActivationExpired(req)` timer
- **States:** `Requested` → `Begun(token)` | `Begun(no token, expired)`
- **Transitions:** `launch` inserts and arms (`shell.rs:998`–`shell.rs:1001`); `activation_token` removes and begins (`shell.rs:1776`–`shell.rs:1779`); `ActivationExpired` removes and begins (`shell.rs:1554`–`shell.rs:1557`)
- **Hazards:** none observed. Whichever path removes the entry first wins, and the other is a no-op. The cwd is captured at request time.
- **Reify:** Low — map-based and clean.
- **Related:** aizome-kit's xdg-activation token request (`cx.request_activation_token`)

<a id="sha-20"></a>

### SHA-20 — App index load and reload
- **Where:** `desktop/aizome/crates/aizome-shell/src/shell.rs:233` (`Shell.apps`), `shell.rs:378` (`._apps_watch`)
- **Encoding:** `apps: Option<Arc<AppIndex>>`, with the watch installed on the first load. Each `AppsChanged` spawns a full reload, without a generation number.
- **States:** `Unloaded` → `Loaded(watching)` → `Reloading×N` → `Loaded`
- **Transitions:** initial load thread (`shell.rs:1078`–`shell.rs:1086`); `Msg::Apps` installs the watch once and replaces the index (`shell.rs:1506`–`shell.rs:1515`); `AppsChanged` spawns a reload (`shell.rs:1517`–`shell.rs:1522`)
- **Hazards:** there is no coalescing or ordering. A burst of directory changes (a profile switch) starts several concurrent loads, and the last to finish wins, even if it read a half-updated tree.
- **Reify:** Low — one field, but stale results are possible.
- **Suspected bug:**
  - The scenario: a `home-manager` or `nixos-rebuild` switch rewrites the application dirs in a burst.
  - An early reload that saw a partial tree finishes last. Apps then stay missing from the Toolchest, palette and Deskbar until the next change.
  - Confidence: low.

<a id="sha-21"></a>

### SHA-21 — Clock tick timer cadence
- **Where:** `desktop/aizome/crates/aizome-shell/src/shell.rs:1474` (`arm_clock`)
- **Encoding:** a single self-re-arming `Msg::Tick`, whose period (seconds or minutes) is decided when it is armed
- **States:** `ArmedMinute` | `ArmedSecond` → `Tick` → re-armed
- **Transitions:** armed at launch (`shell.rs:1495`); `Tick` refreshes and re-arms (`shell.rs:1539`–`shell.rs:1541`); `TimeChanged` refreshes without re-arming (`shell.rs:1545`); `reload_settings` neither re-arms nor cancels (`shell.rs:1270`–`shell.rs:1286`)
- **Hazards:** a cadence change in settings waits for the already-armed tick.
- **Reify:** Low — one timer.
- **Suspected bug:**
  - The scenario: turning on `[menubar] clock_seconds` makes the bar label show seconds immediately.
  - The pending tick is still due at the next minute (`clock.rs:111`–`clock.rs:114`), so the seconds freeze for up to 59 s.
  - Confidence: medium.

<a id="sha-22"></a>

### SHA-22 — logind sleep-delay handshake (session-events side)
- **Where:** `desktop/aizome/crates/aizome-shell/src/shell.rs:2041` (`session_events`), `shell.rs:2083`; state in `lock.rs:60` (`LockState.sleep_pending`) and `lock.rs:25` (`Phase`)
- **Encoding:** the logind delay inhibitor (held or released), `lock.sleep_pending`, `lock.phase` and `lock_cfg.before_sleep`
- **States:** `DelayHeld` → `PrepareForSleep` → (`Locking + sleep_pending` → `Locked` → released) | released at once → asleep → `Resumed` → `DelayHeld`
- **Transitions:**
  - Delay taken at start, only if `before_sleep` is set (`shell.rs:2083`–`shell.rs:2088`).
  - `PrepareForSleep` while Unlocked sets `sleep_pending` and locks (`shell.rs:2052`–`shell.rs:2054`). In any other state the delay is released immediately (`shell.rs:2055`–`shell.rs:2056`).
  - `Resumed` resyncs the screens and takes the delay again, unconditionally (`shell.rs:2059`–`shell.rs:2066`).
  - The delay is released in `lock_confirmed` or `lock_ended` (`lock.rs:365`–`lock.rs:367`, `lock.rs:384`–`lock.rs:386`).
- **Hazards:**
  - `Resumed` takes the delay even when `before_sleep=false`.
  - Turning `before_sleep` on at runtime takes no delay until after a resume, so the first sleep does not wait for the lock.
- **Reify:** Medium — the state is spread over shell.rs, lock.rs and logind.
- **Suspected bug:**
  - The scenario: a lock was requested moments before suspend, so `phase == Locking`. This happens with `loginctl lock-session && systemctl suspend`, or an idle lock racing a lid close.
  - The `else` branch calls `sleep_ready()` at once, so the machine sleeps before the compositor confirms the lock.
  - On resume, the unlocked desktop can show briefly.
  - Confidence: medium.
- **Related:** `desktop/aizome/crates/aizome-shell/src/lock.rs:361` (`lock_confirmed`); the aizome-sys logind power delay inhibitor (`take_sleep_delay`/`sleep_ready`)

<a id="sha-23"></a>

### SHA-23 — Launch attempt lifecycle (Application Launches)
- **Where:** `desktop/aizome/crates/aizome-shell/src/startup.rs:40` (`Attempt.outcome`, `.ready`, `.failed`, `.activated`, `.expects_window`, `.had_window`, `.windows_before`, `.pids`, `.units`, `.window`, `.exited`, `.notification`, `.started`); `startup.rs:84` (`Startup.attempts`, `.next`)
- **Encoding:** five booleans, `window: Option`, `notification: Option`, pid and unit lists, a start `Instant`, and a free-text `outcome` acting as the displayed state. There is a 15 s `Check(id)` timer, a 120 s window-matching cutoff and a 30 s crash cutoff. At most 64 attempts are kept (LIMIT).
- **States:** `Starting` → `ProcessStarted` → `WindowOpened(ready)` | `ActivationAccepted` | `CommandCompleted` | `Failed` | `WaitingNotified`; `WindowOpened` → `ClosedDuringStartup(failed)`. Representable: `ready && failed`; `exited` set by the first of several processes. The view recomputes a summary with its own precedence (`startup.rs:442`–`startup.rs:452`).
- **Transitions:**
  - Begin: insert, evict the oldest, arm `Check`, start the worker (`startup.rs:152`–`startup.rs:190`); a spawn error becomes `Failed` (`startup.rs:191`–`startup.rs:192`).
  - `Started` (`startup.rs:282`–`startup.rs:293`) re-scans the windows (`startup.rs:341`–`startup.rs:347`).
  - `Activated` (`startup.rs:295`–`startup.rs:300`). `Exited`: after ready only a crash fails it, otherwise it fails or completes (`startup.rs:303`–`startup.rs:335`). `Failed` (`startup.rs:350`–`startup.rs:356`). `Check` (`startup.rs:358`–`startup.rs:367`).
  - `startup_window` sets ready and dismisses the waiting note (`startup.rs:240`–`startup.rs:247`), called from `WindowOpened`/`WindowChanged` (`desktop/aizome/crates/aizome-shell/src/shell.rs:1189`, `shell.rs:1196`).
  - `notify_startup` replaces the attempt's note (`startup.rs:375`–`startup.rs:404`).
- **Hazards:**
  - Once `failed`, an attempt can no longer be matched by a window (`startup.rs:219`–`startup.rs:220`). A "could not open" critical note stays even if the app's window opens later, for example after a wrapper exits nonzero.
  - The note ids may already have been dismissed.
  - An evicted attempt's pending `Check` and launcher events are dropped quietly.
- **Reify:** High — thirteen fields, a string state, three timers and seven handlers.
- **Related:** the aizome-sys apps `LaunchObserver` / `launch_observed`; the compositor's `WindowOpened` events through `desktop/aizome/crates/aizome-shell/src/ipc.rs`

<a id="sha-24"></a>

### SHA-24 — Application Launches window and its journal reads
- **Where:** `desktop/aizome/crates/aizome-shell/src/startup.rs:87` (`Startup.window`), `startup.rs:88` (`.selected`); `startup.rs:541` (`read_logs`)
- **Encoding:** `window: Option<WindowKey>`, `selected: Option<id>`, and `Attempt.logs` with the placeholder `"Reading application output…"`. `read_logs` runs a journalctl child with two drain threads, a 5 s `try_wait` deadline and kill, and a 1 s `recv_timeout` per stream.
- **States:** window `Closed` → `Open(selected)` → `Closed`; logs `Prompt` → `Reading` → `Read(text)`; child `Spawned` → `Exited` | `TimedOut → Killed`
- **Transitions:** `show_startup` opens or activates (`startup.rs:411`–`startup.rs:420`); `Close` (`startup.rs:266`–`startup.rs:270`); compositor close (`shell.rs:1704`–`shell.rs:1706`); `read_startup_logs` (`startup.rs:427`–`startup.rs:431`); `Logs(id)` (`startup.rs:272`–`startup.rs:275`); deadline loop and kill (`startup.rs:584`–`startup.rs:596`)
- **Hazards:** `Logs` carries no request number, so with two quick Refreshes the older read can land last. If the thread dies, the placeholder stays.
- **Reify:** Low — small, and the deadlines are local.

<a id="sha-25"></a>

### SHA-25 — Torn-off palettes (persisted list, shown surfaces, strip drag)
- **Where:** `desktop/aizome/crates/aizome-shell/src/tearoff.rs:28` (`TearOff.app`, `.path`, `.output`, `.x`, `.y`, `.pinned`), `tearoff.rs:42` (`Shown.window`, `.size`, `.grab`); `desktop/aizome/crates/aizome-shell/src/shell.rs:299` (`Shell.tearoffs`), `shell.rs:300` (`.tear_shown`), `shell.rs:160` (`Surface::TearOff(usize)`)
- **Encoding:**
  - `palettes.json` is the remembered list.
  - `tear_shown: HashMap<index, Shown>` holds the open surfaces, keyed by Vec index, as does `Surface::TearOff(index)`.
  - `grab: Option<(press, origin)>` tracks the strip drag.
  - Visibility depends on focus, pin and lock phase.
- **States:** per palette `Remembered` → `Shown(window, size)` ⇄ `Hidden` (focus or lock) → `Removed`; strip `Idle` → `Grabbed(press, origin)` → `Idle` (saved). Invalid but representable: `tear_shown[i]` belonging to a different `tearoffs[i]` after an index shift; `tear_shown[i]` holding a window the compositor closed.
- **Transitions:** load at start (`shell.rs:1034`); `tear_off` removes the same app and path, then pushes and syncs (`tearoff.rs:187`–`tearoff.rs:197`); `sync_tearoffs` opens, resizes or closes by index (`tearoff.rs:214`–`tearoff.rs:245`), gated on lock (`tearoff.rs:212`), on every update (`shell.rs:1836`); `Close` drains every shown window, removes the palette and resyncs (`tearoff.rs:262`–`tearoff.rs:271`); `Pin` saves without syncing (`tearoff.rs:274`–`tearoff.rs:278`); `Drag` Start, Move and End (`tearoff.rs:289`–`tearoff.rs:311`)
- **Hazards:**
  - Keying by index is safe only on the `Close` path.
  - `close_requested` (`shell.rs:1703`) never touches `tear_shown`.
  - `Pin(false)` leaves an unfocused palette showing until the next sync changes something.
  - `TearMsg` indices from a view built before a shift address the wrong palette.
- **Reify:** High — index-keyed state is duplicated across three maps, with representable mismatches.
- **Suspected bug:**
  - Two scenarios.
  - (1) Re-tearing a menu that already has a palette goes through `tear_off`. Its `retain` shifts later palettes down while `tear_shown` and `Surface::TearOff(i)` keep the old indices.
    - `sync_tearoffs` then only resizes the existing windows. Palettes show each other's menus at each other's positions, and drags move the wrong entry.
    - With a single palette, the re-torn one stays where it was: the new x and y are saved, but no margins are set.
    - Confidence: high.
  - (2) If a palette's output is unplugged, the compositor closes the layer and `close_requested` forgets only the surface. `tear_shown[i]` keeps the dead window.
    - A pinned or Toolchest palette, which is always visible, therefore never reopens: not on the primary output, and not when the monitor returns, until the user closes it or the shell restarts.
    - Confidence: medium.

<a id="sha-26"></a>

### SHA-26 — Mirror of the compositor's state (windows, focus, Desks, lock)
- **Where:** `desktop/aizome/crates/aizome-shell/src/wm.rs:27` (`Wm.windows`, `.opened`, `.app_order`, `.recent`, `.desks`, `.current_desk`, `.focused`, `.outputs`, `.locked`, `.connected`)
- **Encoding:** a replica built from the IPC `Snapshot` on Connected, then patched by events. Focus is stored twice (`focused` and each window's `WindowInfo.focused`), as is the current Desk (`current_desk` and `desks[].current`).
- **States:** `NeverConnected` → `Connected(snapshot + events)` → `Disconnected(stale data kept)` → `Connected`
- **Transitions:** `reset` (`wm.rs:59`–`wm.rs:76`, from `shell.rs:1174`); `apply` handles WindowOpened, Closed, Changed, Focused, DeskChanged, DesksChanged, OutputsChanged and LockStateChanged (`wm.rs:98`–`wm.rs:157`); `Disconnected` (`shell.rs:1211`)
- **Hazards:**
  - `WindowOpened` with `focused` sets `self.focused` but leaves other windows' `w.focused` set (`wm.rs:100`–`wm.rs:103`).
  - `DesksChanged` with no current desk keeps a possibly stale `current_desk` (`wm.rs:148`–`wm.rs:150`).
  - `reset` keeps `locked`, `app_order` and `recent`. `locked` is not in the snapshot, so after a reconnect with a missed unlock event, `Shell::locked()` (`notifications/glue.rs:119`–`notifications/glue.rs:120`) stays true. Notifications stay quiet, cards stay hidden and the Launches window is refused.
  - `connected` is written but never read.
- **Reify:** Medium — duplicated truth with event-order invariants.
- **Related:** `desktop/aizome/crates/aizome-shell/src/ipc.rs:59` (connect and reconnect, `Disconnected` at `ipc.rs:138`); the aizome-comp-core IPC server that sends the snapshot and events

## Already explicit

- None in this slice.

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/crates/aizome-shell/src/barmenus.rs` | 126 | machines | [SHA-01](#sha-01) | bar menu open/slide/press-to-close, Toolchest handover |
| `desktop/aizome/crates/aizome-shell/src/carry.rs` | 227 | machines | [SHA-02](#sha-02) | restart state file, final word, wait_for_last |
| `desktop/aizome/crates/aizome-shell/src/deskbar.rs` | 767 | machines | [SHA-05](#sha-05), [SHA-06](#sha-06), [SHA-07](#sha-07) | view; Moving enum projects grip drag; drag elements |
| `desktop/aizome/crates/aizome-shell/src/deskbar_compact.rs` | 372 | none |  | pure view (emits SHA-05 window drags) |
| `desktop/aizome/crates/aizome-shell/src/deskbar_glue.rs` | 968 | machines | [SHA-03](#sha-03), [SHA-04](#sha-04), [SHA-05](#sha-05), [SHA-06](#sha-06), [SHA-07](#sha-07), [SHA-08](#sha-08), [SHA-16](#sha-16) | auto-hide, surface, row/edge/grip drags, toggles, park |
| `desktop/aizome/crates/aizome-shell/src/desktop.rs` | 405 | machines | [SHA-09](#sha-09) | DesktopState selection/drag/band, positions file |
| `desktop/aizome/crates/aizome-shell/src/desktop_glue.rs` | 422 | machines | [SHA-09](#sha-09), [SHA-10](#sha-10), [SHA-11](#sha-11) | desktop handlers, wallpaper pipeline, poll watcher |
| `desktop/aizome/crates/aizome-shell/src/menubar.rs` | 1190 | machines | [SHA-01](#sha-01), [SHA-03](#sha-03), [SHA-18](#sha-18) | view; hover gating, N apps hover, rename field |
| `desktop/aizome/crates/aizome-shell/src/menus/build.rs` | 596 | none |  | pure menu builders |
| `desktop/aizome/crates/aizome-shell/src/menus/mod.rs` | 5 | none |  | module declarations |
| `desktop/aizome/crates/aizome-shell/src/menus/model.rs` | 354 | none |  | menu data and type-ahead ranking |
| `desktop/aizome/crates/aizome-shell/src/palette/mod.rs` | 1035 | machines | [SHA-12](#sha-12), [SHA-13](#sha-13), [SHA-14](#sha-14), [SHA-15](#sha-15) | palette window/spare, file search, index cache, frecency |
| `desktop/aizome/crates/aizome-shell/src/palette/model.rs` | 506 | none |  | pure ranking, data tables, tests |
| `desktop/aizome/crates/aizome-shell/src/palette/view.rs` | 518 | none |  | pure view and scroll math |
| `desktop/aizome/crates/aizome-shell/src/shelf.rs` | 269 | machines | [SHA-16](#sha-16) | persisted items, image GC on save |
| `desktop/aizome/crates/aizome-shell/src/shell.rs` | 2514 | machines | [SHA-01](#sha-01), [SHA-02](#sha-02), [SHA-03](#sha-03), [SHA-04](#sha-04), [SHA-05](#sha-05), [SHA-06](#sha-06), [SHA-07](#sha-07), [SHA-08](#sha-08), [SHA-09](#sha-09), [SHA-10](#sha-10), [SHA-11](#sha-11), [SHA-12](#sha-12), [SHA-13](#sha-13), [SHA-15](#sha-15), [SHA-16](#sha-16), [SHA-17](#sha-17), [SHA-18](#sha-18), [SHA-19](#sha-19), [SHA-20](#sha-20), [SHA-21](#sha-21), [SHA-22](#sha-22), [SHA-23](#sha-23), [SHA-24](#sha-24), [SHA-25](#sha-25), [SHA-26](#sha-26) | primary SHA-17..22; lock/panel/osd/recording/services fields audited in their modules |
| `desktop/aizome/crates/aizome-shell/src/startup.rs` | 998 | machines | [SHA-23](#sha-23), [SHA-24](#sha-24) | launch attempts, launches window, journal read |
| `desktop/aizome/crates/aizome-shell/src/tearoff.rs` | 373 | machines | [SHA-25](#sha-25) | index-keyed torn-off palettes |
| `desktop/aizome/crates/aizome-shell/src/wm.rs` | 477 | machines | [SHA-26](#sha-26) | compositor state mirror |
