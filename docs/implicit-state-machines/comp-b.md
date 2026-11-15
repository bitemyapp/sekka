# CPB: Compositor core: state, input, focus and protocols

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/crates/aizome-comp-core`. Audited at `8e0acbe5`; line numbers refer to that commit.

31 files audited. 34 implicit machines: 12 High, 12 Medium, 10 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

<a id="cpb-02"></a>

### CPB-02 — xdg_popup grab (menu chain) lifecycle
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/focus.rs:164` (`PopupGrabbed.grab`, `.root`, `.dismissed`); also `desktop/aizome/crates/aizome-comp-core/src/state.rs:323` (`Aizome.popup_grab`), `desktop/aizome/crates/aizome-comp-core/src/shell.rs:442` (`XdgShellHandler::grab`)
- **Encoding:** `Option<PopupGrabbed>` plus `dismissed: bool`, plus smithay's `PopupGrab` (active and dismissed popup lists behind `has_ended()` / `current_grab()`), plus the seat's `PopupKeyboardGrab` and `PopupPointerGrab`, which are identified only by the grab's two serials (`owns_serial`), plus `layer_focus`, which is cleared when the root was a clicked layer surface.
- **States:** `None` → `Live` (not dismissed, topmost popup ≠ root) → `ClosedBySmithay` (not dismissed, no popup left, not ended) → `Dismissed` (popup_done sent, waiting for the client to destroy the popups) → `Ended` (taken). Keyboard sub-state: held by the menu grab, or left with an input method (`over_input_method`). Representable but invalid: `Dismissed` while the seat grab is still owned; `popup_grab` pointing at an older chain while a newer chain holds the seat grabs.
- **Transitions:** refusals (`shell.rs:447`, `:456`, `:470`, `:494`, `:499`); a new top-level chain dismisses the old one (`shell.rs:463-465`); installed: focus, keyboard grab, pointer grab, `Some(PopupGrabbed{dismissed:false})` (`shell.rs:503-511`); keys follow the topmost popup and the keyboard is taken back after an IME lets go (`focus.rs:227-247`); keyboard target elsewhere → dismiss (`focus.rs:203-205`); `dismiss_popup_grab` Live→Dismissed (`focus.rs:257-264`); smithay-closed→Dismissed (`focus.rs:281-284`); Dismissed→Ended: take, unset the keyboard and pointer grabs, clear `layer_focus` (`focus.rs:289-310`); press on another client (`focus.rs:323-327`); polled every iteration (`state.rs:574`)
- **Hazards:** the end is only noticed by polling once per loop iteration. A client that never destroys its dismissed popups keeps the grab in `Dismissed`, and smithay's pointer grab keeps hover on that client until a forwarded press elsewhere (`press_outside_menus` ignores non-live grabs, `focus.rs:318`). Seat-grab ownership is inferred from two serials (`focus.rs:186-188`). Replacing `popup_grab` at `shell.rs:511` drops the old record without checking its grabs.
- **Reify:** High — five phases spread across smithay state, two seat grabs and about six call sites; failures leave the keyboard or pointer stuck.
- **Related:** aizome-comp-logic `desktop/aizome/crates/aizome-comp-logic/src/input.rs:130` and `desktop/aizome/crates/aizome-comp-logic/src/actions.rs:116` (callers); recovery path `desktop/aizome/crates/aizome-comp-core/src/fallback.rs:226`

<a id="cpb-07"></a>

### CPB-07 — Idle-inhibitor registry
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/state.rs:335` (`Aizome.idle_inhibitors`); also `desktop/aizome/crates/aizome-comp-core/src/handlers.rs:231`
- **Encoding:** a `HashSet<WlSurface>` (keyed by surface, not by inhibitor object), plus `idle_notifier.set_is_inhibited`.
- **States:** `NotInhibited` ⇄ `Inhibited(set non-empty)`
- **Transitions:** `inhibit` inserts and inhibits (`handlers.rs:232-233`); `uninhibit` (`handlers.rs:236-238`) and surface destruction (`shell.rs:100-103`) → `drop_idle_inhibitor` (`idle.rs:77-82`)
- **Hazards:** smithay calls `inhibit`/`uninhibit` once per inhibitor object, but the set has one entry per surface. Inhibitors count whether or not the surface is visible.
- **Reify:** High — the keying loses inhibitors (see bug).
- **Suspected bug:** a client with two inhibitors on one surface (say video playback plus a call in one browser window) destroys one; the surface leaves the set, inhibition ends and the display idles or locks while the other inhibitor is still alive (confidence: low)
- **Related:** idle timers `desktop/aizome/crates/aizome-comp-core/src/idle.rs:84` (comp-a)
- **Reproduction:** Confirmed (end-to-end): [summary](reproductions.md#cpb-07)

<a id="cpb-08"></a>

### CPB-08 — Pointer constraint activation (lock and confine)
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/handlers.rs:254` (`PointerConstraintsHandler`); also `desktop/aizome/crates/aizome-comp-core/src/input.rs:264` (locked handling), `input.rs:305` (activation)
- **Encoding:** smithay's per-surface `PointerConstraint` active flag, plus pointer focus, plus `recovery.unconstrained` (held off after Reset Input), plus `az.pointer_pos`.
- **States:** `None` → `Requested` → `Active(Locked | Confined)` → `Inactive` (focus left; smithay deactivates) → `Active` again | `HeldOff` (recovery)
- **Transitions:** a new constraint clears the hold-off and activates if focused (`handlers.rs:256-262`); motion activates any inactive constraint on the focused surface (`input.rs:305-316`); a locked pointer sends relative motion only and returns (`input.rs:264-282`); a cursor hint moves the pointer while active (`handlers.rs:272-279`); a press re-picks the target unless constrained (`input.rs:354-366`)
- **Hazards:** only `Locked` is enforced; `Confined` constraints are activated but motion is never clamped to the region (only the recovery status display names `Confined`, `recovery/mod.rs:1166`). Locked motion skips `pointer_motion_absolute` and with it `note_activity`.
- **Reify:** High — the enforcement is missing, and the activity coupling is wrong.
- **Suspected bug:** (1) a game or VM (or Xwayland's confine-to) confines the pointer and gets `confined`, but the pointer leaves the window freely (confidence: high). (2) In a pointer-locked app driven only by the mouse (an FPS, a VM viewer), `note_activity` never runs, so Aizome's idle timers and ext-idle-notify dim or lock the screen mid-use unless the app also inhibits idle (confidence: medium).
- **Related:** `desktop/aizome/crates/aizome-comp-core/src/idle.rs:57` (comp-a), `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:772-782` (comp-a)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#cpb-08)

<a id="cpb-11"></a>

### CPB-11 — Key press/release pairing (suppressed and translated keys)
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/state.rs:329` (`Aizome.suppressed_keys`), `state.rs:331` (`.translated_keys`); also `desktop/aizome/crates/aizome-comp-core/src/input.rs:451` (`decide_key`)
- **Encoding:** two `HashSet<Keycode>` that record how each held key's press was decided, so its release is decided the same way; they live in the core (they survive reloads) and are written by both the core and the logic.
- **States:** for each keycode `Up` | `Forwarded` | `Suppressed` | `Translated(mods)`; representable but invalid: a keycode in both sets, or a stale entry for a key that is up
- **Transitions:** swallowed release (`input.rs:470-472`); catch-key press (`input.rs:481`); core binding press (`input.rs:487`); unhealthy logic drops the translation at release (`input.rs:491-493`); logic overlay/binding presses insert into `suppressed` (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:284`, `:322`); logic translation press/release (`aizome-comp-logic/src/input.rs:328`, `:289`); Reset Input clears both (`recovery/reset.rs:140-141`)
- **Hazards:** everything depends on each press seeing its release. A lost release (device unplugged mid-press, VT switch, a key delayed by the logic's slow keys and then dropped) leaves an entry that swallows the next forwarded press's release, so the client sees a stuck, repeating key until Reset Input.
- **Reify:** High — six mutation sites in two crates, with user-visible stuck keys.

<a id="cpb-17"></a>

### CPB-17 — Compositor logic library lifecycle (load, reload, poison, rollback, set aside)
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/logic.rs:229` (`LogicHost.state`, `.owner`, `.vt`, `.current`, `.previous`, `.last_saved`, `.generation`, `.poisoned`, `.panics`, `.pending_reload`, `.depth`, `.reloads`, `.rollbacks`, `.last_error`); also `desktop/aizome/crates/aizome-comp-core/src/requests.rs:276` (`reload`)
- **Encoding:** about a dozen fields with no phase enum. `healthy()` = `vt.is_some() && poisoned.is_none()`; `depth` counts logic frames on the stack; `generation` invalidates timers; the per-process copy directory `~/.cache/aizome/logic/<pid>/<gen>-*.so` is on-disk state.
- **States:** `None` (recovery path) → `Running(current, previous?)` → `Poisoned` (rest of the iteration) → `RolledBack` | `Restarted` | `SetAside` (no logic); `Reloading` with `PendingReload`. Representable but invalid: `vt` Some while `current` None (throughout the new logic's `init`, `logic.rs:373-377`), `state` non-null with `vt` None, `poisoned` set while `depth>0`.
- **Transitions:** `lg_call` depth++/-- and poisoning (`logic.rs:341-349`, `:359`); `load_logic`: open (`:368`), stop old (`:372`), swap (`:373-376`), init (`:377`), on failure restore old from its saved state or give up (`:386-403`), on success record previous, last_saved, current, clear panics (`:405-411`); `stop_logic` quiesce, end grabs, save, fall back to `last_saved` only if poisoned, discard (`:423-440`); `recover_logic`: pending reload (`:479-483`), panic window (`:486-487`), stop (`:491`), choose previous, restart (<3 panics in 60 s) or set aside (`:498-502`), init from saved then from nothing (`:504-518`); key reload deferred (`input.rs:518`); IPC reload (`requests.rs:296-327`); private copy and stale pid dirs (`logic.rs:646-663`)
- **Hazards:** a healthy logic whose `save` fails or panics during a user reload hands `None` to the new logic (no `last_saved` fallback, `logic.rs:438`), and the arrangement is lost. A poisoned logic falls back to `last_saved` from the last *reload*, which can be hours old. Coming back from `SetAside` always starts from nothing (`stop_logic` returns early at `logic.rs:421`). `init`/`save`/`quiesce` called outside `lg_call` don't raise `depth`. Copies accumulate per generation, and dead-pid directories survive pid reuse.
- **Reify:** High — 12+ fields, many invalid combinations, user-visible state loss.
- **Related:** `desktop/aizome/crates/aizome-comp-core/src/reload.rs:12-15` (in_call deferral, comp-a); `desktop/aizome/crates/aizome-comp-logic/src/input.rs:489` (`reload_soon`)

<a id="cpb-18"></a>

### CPB-18 — Popup tracking and popup-layer stacking (xdg and input-method popups)
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/state.rs:325` (`Aizome.popup_order`), `state.rs:265` (`Aizome.popups`); also `desktop/aizome/crates/aizome-comp-core/src/popups.rs:48` (`placed_popups`), `desktop/aizome/crates/aizome-comp-core/src/shell.rs:407`
- **Encoding:** smithay `PopupManager` tracking, plus Aizome's own `popup_order` Vec (stacking, rendering, hit-testing and capture all read it), plus smithay's per-popup initial-configure flag and positioner geometry.
- **States:** `Created` → `Tracked` (+ in `popup_order`) → `Configured` → `Placed` (root shows and popup mapped) → `Dismissed` → `Dead` (pruned)
- **Transitions:** xdg `new_popup` unconstrains, pushes and tracks (`shell.rs:408-412`); layer popups are unconstrained again once parented (`shell.rs:690`); IME popups are only tracked (`handlers.rs:319`); initial configure on first commit (`shell.rs:134-138`); reposition (`shell.rs:416-421`); cleanup and prune each iteration (`state.rs:573`, `:575`, `popups.rs:84-86`); IME dismiss (`handlers.rs:325-327`); recovery dismiss (`recovery/reset.rs:70`)
- **Hazards:** two registries must agree. A popup appears only if it is in `popup_order` and smithay maps it under a shown root (`popups.rs:50-61`). Unconstraining is skipped while the root doesn't show (`shell.rs:364`).
- **Reify:** High — the registries diverge (see bug).
- **Suspected bug:** input-method popup surfaces (zwp_input_popup_surface_v2, as fcitx5 uses for its candidate list) are tracked but never pushed to `popup_order`, so `placed_popups` skips them: they are never drawn (`render.rs:155`) or hit-tested (`popups.rs:74-80`) (confidence: medium)
- **Reproduction:** Confirmed (end-to-end): [summary](reproductions.md#cpb-18)

<a id="cpb-20"></a>

### CPB-20 — Foreign-toplevel handle synchronisation (wlr management and ext list)
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/protocols/foreign_toplevel.rs:25` (`ForeignToplevelManagerState.managers`, `.handles`, `.sent`, `.ext`)
- **Encoding:** three maps keyed by `WinId` (wlr handles per client, the last-sent `Snapshot`, ext handles) plus the manager list; diffs are sent once per loop iteration.
- **States:** for each window `Announced` → `Synced(snapshot)` → `Dirty` → `Synced` … → `Closed`; managers `Bound` → `Stopped`
- **Transitions:** bind announces all windows (`foreign_toplevel.rs:229-235`); `window_opened` (`foreign_toplevel.rs:124-135`, from `shell.rs:197`); `refresh` diffs and sends (`foreign_toplevel.rs:168-218`, from `state.rs:595`); `window_closed` (`foreign_toplevel.rs:137-148`, from `shell.rs:243`); `Stop` (`foreign_toplevel.rs:253-256`); handle destroyed (`foreign_toplevel.rs:290-294`)
- **Hazards:** a handle created at bind already carries the current state, while `sent` may be older, so the next refresh can repeat `output_enter` on it. A wl_output bound after the announce never gets `output_enter`. Managers are pruned only at refresh (`foreign_toplevel.rs:217`).
- **Reify:** High — three maps must stay in lockstep; suspected bug.
- **Suspected bug:** `Snapshot.parent` is compared in `refresh` (`foreign_toplevel.rs:173`) but no `parent` event is ever sent after the announce (`foreign_toplevel.rs:179-200`), so a dialog whose parent is set or changed after mapping is never re-parented in taskbars (confidence: high)
- **Reproduction:** Confirmed (end-to-end): [summary](reproductions.md#cpb-20)

<a id="cpb-21"></a>

### CPB-21 — Gamma-control ownership per output
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/state.rs:153` (`OutputState.gamma_client`); `desktop/aizome/crates/aizome-comp-core/src/protocols/gamma_control.rs:27` (`ControlData.output`)
- **Encoding:** a per-output `bool` that isn't tied to a particular control, plus an immutable `Option<Output>` in each control's user data.
- **States:** output `NightLight` → `HeldByClient` → `NightLight`; control `Failed` | `Holding` | `Released` (still `Some(output)`)
- **Transitions:** get control: refused if the size is 0 or the output is held (`gamma_control.rs:64-69`), otherwise held (`gamma_control.rs:72-76`); `SetGamma` failure → `failed` + release (`gamma_control.rs:98-100`); destroy → release (`gamma_control.rs:115-117`); release restores Night Light (`gamma_control.rs:123-126`); output disable drops the whole `OutputState` (`outputs.rs:113`)
- **Hazards:** a released control keeps `Some(output)` and can still `SetGamma`. Disabling or unplugging the output forgets the hold without sending `failed`, so after re-enabling, a second client and Night Light both drive the ramps.
- **Reify:** High — an unowned flag with a suspected bug.
- **Suspected bug:** client A's `SetGamma` fails (for example while the VT is switched away), so A's control is failed and released; client B (or A again) takes the output; A then destroys its failed control, `release` clears B's hold and Night Light overwrites B's ramps (confidence: medium)
- **Reproduction:** Confirmed (unit): [summary](reproductions.md#cpb-21)

<a id="cpb-22"></a>

### CPB-22 — ext-image-copy-capture session and frame lifecycle
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/protocols/image_copy_capture.rs:73` (`SessionState.source`, `.size`, `.scale`, `.dmabuf`, `.stream`, `.shown`, `.frame`); `image_copy_capture.rs:95` (`Pending.since`); `image_copy_capture.rs:117` (`FrameInner.buffer`, `.captured`); `image_copy_capture.rs:123` (`CursorData.taken`)
- **Encoding:** sessions and pending captures in Vecs; `source: None` means stopped; `shown` means the next frame waits for damage; `frame` is the one current frame; frames carry `captured`; `since` holds the source serial last found unchanged.
- **States:** session `Started(constraints sent)` → `Streaming(shown)` → `Resized(shown=false)` → … → `Stopped` → destroyed; frame `Created` → `BufferAttached` → `Captured` → `Pending(since)` → `Copied` → `Ready` | `Failed(reason)` | `Replaced` (never answered)
- **Transitions:** `start` (`image_copy_capture.rs:367-394`); cursor sessions start stopped (`:255-260`); `CreateFrame` replaces the current frame and drops its pending entry (`:279-285`); `AttachBuffer`/`Capture` (`:320`, `:327-334`); `capture` checks stopped and constraints (`:415-423`); `flush` waits on `since`, takes the stream, copies, sets `shown` (`:452-491`); `follow_sources` stops or resizes and fails mismatched pending frames (`:509-541`); `ready` ignores replaced frames (`:551-555`); IPC `stop` (`:608-620`); session and frame destroyed (`:290-301`, `:340-349`)
- **Hazards:** `capture` doesn't check that the frame is still the session's current one, so a replaced frame can still be queued and copied. The dmabuf target is captured at `start` from a cache that is never refreshed (`screencopy.rs:240-248`).
- **Reify:** High — two interleaved machines, many fields, scattered transitions.
- **Suspected bug:** a frame copied with a fence is replaced (`CreateFrame`) before the fence signals; its copy already advanced the stream's damage, `ready` drops it, and the new frame then finds `Unchanged` and waits for the next change, so an idle screen stalls the resumed portal stream (confidence: low)
- **Related:** `desktop/aizome/crates/aizome-shell/src/sharing.rs` (share indicator via IPC); `desktop/aizome/crates/aizome-comp/tests/image_copy_capture.rs`
- **Reproduction:** Partly confirmed (end-to-end): [summary](reproductions.md#cpb-22)

<a id="cpb-24"></a>

### CPB-24 — wlr-output-management configuration lifecycle
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/protocols/output_management.rs:27` (`OutputManagementState.instances`, `.serial`); `output_management.rs:56` (`ConfigInner.used`, `.heads`); `output_management.rs:62` (`HeadConfig`)
- **Encoding:** a global u32 serial, bumped at every `outputs_changed`; per-manager heads keyed by output name, with their mode objects fixed at creation; per-configuration `used` flag and its head list.
- **States:** manager `Bound` → `Stopped`; configuration `Building` → `Used` → `Cancelled` (stale serial) | `Succeeded` | `Failed`; head `Announced` → `Updated` → `Finished`
- **Transitions:** bind (`output_management.rs:205-211`); `outputs_changed` bumps the serial, finishes gone heads, updates or creates others, sends `done` (`:169-191`); create configuration (`:230-232`); enable/disable head (`:280-297`); apply/test: used check, serial check, validation, sequential `configure_output` (`:300-346`); `Stop` (`:233-236`)
- **Hazards:** `Test` checks only the scale range and names, not modes. Each `configure_output` fires `outputs_changed` in the middle of an apply. Mode lists never refresh for a head that keeps its name. `EnableHead` after `used` isn't rejected.
- **Reify:** High — the serial, used flag and partial apply interact; suspected bug.
- **Suspected bug:** kanshi applies a profile of two heads and the second fails (for example "has no mode WxH" from `backend/udev.rs`); the first head's change stays applied while the client is told `failed`, so the layout is half-changed against the protocol's rule to revert on failure (`output_management.rs:339-346`) (confidence: high)
- **Reproduction:** Not reproduced (environment) (end-to-end): [summary](reproductions.md#cpb-24)

<a id="cpb-25"></a>

### CPB-25 — wlr-screencopy frame lifecycle and stream pool
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/protocols/screencopy.rs:46` (`ScreencopyManagerState.pending`, `.streams`, `.dmabuf`); `screencopy.rs:64` (`Pending.since`); `screencopy.rs:79` (`FrameInner.output`, `.manager`, `.used`)
- **Encoding:** pending copies in a Vec; a stream pool keyed by (manager id, output, region, cursor), at most 4 per manager; a frame's `used` flag set to true up front for frames already failed; `since` holds the output's `damaged_frames`; the dmabuf target is asked once and cached.
- **States:** frame `Created` → `Failed` | `Offered(buffer events)` → `Copying(pending, since)` → `Copied` → `AwaitingFence` → `Ready` | `Failed`; stream `New` → `Pooled` → `TakenForCopy` → `Pooled` → `Evicted` | `Dropped`
- **Transitions:** capture request (`screencopy.rs:147-176`); copy or copy-with-damage marks used and queues (`:206-216`); destroy unqueues (`:199-201`); `flush_pending` checks alive, output gone, unchanged-since (`:259-275`); `take_stream` reuses or evicts (`:313-326`); a copy puts the stream back and re-queues or completes (`:282-297`); manager destroyed drops streams (`:179-182`); `ready` (`:330-343`)
- **Hazards:** a stream's key leaves out the output's scale, while its tracker scale is fixed at creation (`capture.rs:82`). Pending copies for a destroyed manager recreate a stream under the dead key, which is never removed (`:277`, `:282`). The cached dmabuf formats (`:240-248`) never follow a renderer change. Damage-only frames wait indefinitely on an idle output (by design).
- **Reify:** High — two coupled machines; suspected bugs.
- **Suspected bug:** (1) wf-recorder records a whole output and the user changes only its scale (same mode); the region is unchanged, so the old stream is reused with its old tracker scale and surfaces are drawn at the wrong size; the ext path resets on a scale change (`image_copy_capture.rs:515-521`), this one doesn't (confidence: medium). (2) A client that destroys its manager right after requesting a frame leaves one stream, with a full-output offscreen buffer, per manager for as long as the output exists (confidence: low).
- **Reproduction:** Confirmed (end-to-end): [summary](reproductions.md#cpb-25)

<a id="cpb-30"></a>

### CPB-30 — Toplevel map/unmap lifecycle and window registry
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/state.rs:278` (`Aizome.windows`), `state.rs:279` (`.by_surface`), `state.rs:280` (`.unmapped`), `state.rs:281` (`.next_win`); also `desktop/aizome/crates/aizome-comp-core/src/shell.rs:153`
- **Encoding:** membership in `unmapped` vs `windows`/`by_surface`, smithay's `initial_configure_sent` (reset by smithay on unmap), buffer presence, and fullscreen agreed before map in the pending state.
- **States:** `New` → `InitialConfigured` (pre-map fullscreen or unfullscreen may reconfigure) → `Mapped(WinId)` → `Unmapped(null buffer)` → `Mapped(new WinId)` … → `Destroyed`
- **Transitions:** `new_toplevel` (`shell.rs:403-404`); first commit → logic `first_configure` + configure (`shell.rs:156-161`); first buffer → `map_window`: id, registry, logic or fallback, foreign toplevel (`shell.rs:167-169`, `:183-197`); null buffer → `unmap_window` + back to unmapped (`shell.rs:214-218`); pre-map fullscreen (`shell.rs:542-554`), unfullscreen (`shell.rs:563-571`), maximize (`shell.rs:518-520`); destroyed (`shell.rs:594-598`); dead clients swept (`state.rs:636-640`)
- **Hazards:** a remap gets a new `WinId`, so foreign-toplevel handles close and window capture sessions stop (`Source::Window` gone). Fullscreen is promised before map only if the logic is healthy at that moment (`shell.rs:542`). A toplevel that never draws stays in `unmapped` for good. `by_surface` and `windows` are updated in separate statements (`shell.rs:184-185`, `:236-237`).
- **Reify:** High — the phases are spread over Vec membership, maps and smithay flags, across about ten handlers.
- **Related:** aizome-comp-logic `desktop/aizome/crates/aizome-comp-logic/src/actions.rs:991` (`window_committed`), `desktop/aizome/crates/aizome-comp-core/src/fallback.rs` (comp-a)
