# SHA: Shell: main loop, startup, menubar, deskbar, desktop and palette

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/crates/aizome-shell`. Audited at `8e0acbe5`; line numbers refer to that commit.

19 files audited. 26 implicit machines: 7 High, 9 Medium, 10 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#sha-02)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#sha-03)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#sha-12)

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
- **Reproduction:** Confirmed (unit): [summary](reproductions.md#sha-13)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#sha-25)
