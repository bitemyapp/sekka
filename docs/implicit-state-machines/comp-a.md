# CPA: Compositor core: backends, recovery, pacing and rendering

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/crates/aizome-comp-core`. Audited at `8e0acbe5`; line numbers refer to that commit.

28 files, 0 pending. 32 implicit machines: 10 High, 8 Medium, 14 Low.

## Machines

<a id="cpa-01"></a>

### CPA-01 — Headless output refresh pacing and buffer age
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs:43` (`HeadlessOutputState.rendered_once`, `.timer`); also `desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs:59` (renderer chosen once)
- **Encoding:** per output `rendered_once: bool` (damage age 0 or 1) and `timer: Option<RegistrationToken>` (an armed 60 Hz wake), read with `OutputState.last_frame`, `.refresh`, `.powered`, `.redraw_queued`; GLES falls back to pixman once in `Headless::new`
- **States:** `Fresh` (age 0) → `Rendered` (age 1); per frame `Due` | `TooSoon` (timer armed) | `Unpowered` (queued redraw dropped)
- **Transitions:** `add_target` resets to Fresh with no timer (`desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs:148`, `desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs:149`); `frame_due` drops the queue when unpowered (`desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs:213`), arms the timer when too soon (`desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs:222`); the timer fires and clears its token (`desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs:230`); `redraw` clears the queue (`desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs:253`) and sets `rendered_once` (`desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs:279`); `configure_output` removes the state on disable (`desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs:162`) and rebuilds it on every other change (`desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs:182`)
- **Hazards:** `configure_output` drops a state whose timer is still armed, so a second timer gets armed beside the orphan, whose callback then clears the new state's token (`desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs:230`): only extra wakeups; a failed render (`desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs:282`) has already cleared the queued redraw (`desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs:253`)
- **Reify:** Low — small, local and self-healing

<a id="cpa-02"></a>

### CPA-02 — Per-output redraw scheduling (queued, due, drawn) across backends
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/backend/mod.rs:81` (`Backend::frame_due`), `desktop/aizome/crates/aizome-comp-core/src/backend/mod.rs:292` (`post_frame`); the fields `OutputState.redraw_queued`, `.powered`, `.last_frame`, `.frame_seq`, `.damaged_frames` are declared outside this slice (`desktop/aizome/crates/aizome-comp-core/src/state.rs:137`)
- **Encoding:** `redraw_queued: bool`, set by any `queue_redraw*` caller, is gated by each backend's `frame_due` (headless timer, udev `Redraw` plus session and power, winit always) and cleared by each backend's `redraw`; `post_frame` re-queues mirrors and, while `View.animating`, the output itself; the loop is `State::redraw_due` (`desktop/aizome/crates/aizome-comp-core/src/state.rs:605`, other slice)
- **States:** `Idle` → `Queued` → `Due` → `Drawn` (udev: `WaitingForVBlank`, CPA-03) → `Idle` | `Queued` (animating or mirrored); `Gated` (unpowered or session paused: the queue is dropped). Representable invalid state: `redraw_queued && !powered` (cleared lazily by `frame_due`)
- **Transitions:** power on re-queues (`desktop/aizome/crates/aizome-comp-core/src/backend/mod.rs:194`); gates drop the queue (`desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs:215`, `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:423`); drawing clears it (`desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs:253`, `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:451`, `desktop/aizome/crates/aizome-comp-core/src/backend/winit.rs:121`); `post_frame` stamps `frame_seq`, `damaged_frames` and `last_frame` (`desktop/aizome/crates/aizome-comp-core/src/backend/mod.rs:299`), re-queues mirrors of a damaged source (`desktop/aizome/crates/aizome-comp-core/src/backend/mod.rs:309`) and animating outputs (`desktop/aizome/crates/aizome-comp-core/src/backend/mod.rs:330`); a udev vblank re-queues while animating (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:1120`); session resume queues every output (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:262`)
- **Hazards:** the queue is cleared before rendering, so a render, bind or queue failure (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:504`, `desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs:282`, `desktop/aizome/crates/aizome-comp-core/src/backend/winit.rs:144`) drops the request and skips `post_frame`, which means no frame callbacks and no FIFO release until something else queues a redraw; a persistent failure freezes that output's clients; an undamaged udev frame queues no flip, so the animating re-queue at `desktop/aizome/crates/aizome-comp-core/src/backend/mod.rs:331` waits for some other wakeup; `last_frame` is what CPA-13 tests for "drawing", so a gated output still looks like it is drawing for up to 1 s
- **Reify:** High — one phase is spread over a flag, three backend gates and about ten mutation sites
- **Related:** the scheduler in `desktop/aizome/crates/aizome-comp-core/src/state.rs:605` (comp-core, other slice); CPA-13 and CPA-14 consume its frames

<a id="cpa-03"></a>

### CPA-03 — Udev per-CRTC page-flip state
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:96` (`enum Redraw`), `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:108` (`Surface.redraw`)
- **Encoding:** partially explicit: `Redraw::{Idle, WaitingForVBlank}` per surface, while the in-flight frame and its presentation feedback live inside smithay's `DrmOutput`; resets are scattered across four handlers
- **States:** `Idle` → `WaitingForVBlank` → `Idle`
- **Transitions:** `redraw` queues a damaged frame, Idle→Waiting (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:499`), and is skipped while waiting (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:447`); `vblank` Waiting→Idle with `frame_submitted` (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:1091`); session resume →Idle with `reset_buffers` (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:249`); `set_power` →Idle (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:640`); `frame_due` gates on it (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:430`)
- **Hazards:** there is no timeout, so a page flip whose event never arrives (`DrmEvent::Error` only logs, `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:732`) leaves the CRTC waiting and the output frozen forever; `set_power(false)` forces Idle while a flip may still be in flight, so a late vblank calls `frame_submitted` on a cleared compositor (warning at `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:1117`); a failed `queue_frame` stays Idle, but the queued redraw is already gone (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:451`)
- **Reify:** Medium — the enum exists, but its resets and its invariant with smithay's pending frame are scattered

<a id="cpa-04"></a>

### CPA-04 — libseat session pause and resume (VT switching)
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:219` (session notifier handler); `Udev.session` (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:135`)
- **Encoding:** libseat's `session.is_active()` (read by `frame_due`, `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:421`), libinput suspended or resumed, each `DrmOutputManager` paused or activated, per-surface `Redraw` and buffers reset, `OutputState.powered` re-applied, Night Light forced
- **States:** `Active` → `Paused` (input suspended, DRM paused, frames gated) → `Resuming` (activate, reset, re-power-off, queue all, re-gamma) → `Active`
- **Transitions:** `PauseSession` (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:222`); `ActivateSession` (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:234`); `change_vt` requests a switch (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:411`), called from Text Console (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:967`)
- **Hazards:** hotplug, `enable_output`, mode changes and gamma all still run while paused and fail against the inactive device (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:1011`, `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:681`, `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:617`), and resume doesn't rescan connectors; a `li.resume()` failure only logs (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:236`), leaving no input and no retry; an activation failure only logs (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:245`), but surfaces are reset and redraws queued anyway; the idle poll keeps running while paused, so lock and monitors-off fire while the user is on another VT (re-applied at `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:256`)
- **Reify:** High — a cross-process phase (logind/seatd) that every other output machine depends on
- **Suspected bug:** a monitor plugged in while the user is on another VT stays dark after they return. The udev `Changed` event runs `device_changed` while paused and reserves the connector (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:834`); `initialize_output` fails on the inactive device (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:1037`); `ActivateSession` never calls `device_changed`; and later `Changed` events skip the connector as already known (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:823`) until it is unplugged (confidence: medium)
- **Related:** libseat with logind or seatd (seat management, outside the repo)

<a id="cpa-05"></a>

### CPA-05 — DRM device and connector hotplug, and output enablement
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:130` (`Device.connectors`), `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:131` (`Device.surfaces`), `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:149` (`Udev.disabled`); also `Udev.devices` (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:142`), `Udev.renderers` (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:138`)
- **Encoding:** connector→CRTC reservations, CRTC→driven `Surface`, and name→(node, connector, CRTC) for outputs turned off, plus `Aizome.disabled_outputs`, `.outputs` and `.out` (CPA-12); the renderer kind switches from GLES to pixman when the first device has no hardware GL
- **States:** per connector `Unknown` → `Reserved` (CRTC taken) → `Driven` (surface, output in layout) | `OffBySettings` (disabled entry, no surface) | `TurnedOff` (surface dropped, disabled entry) → `Driven` (enable) → `Gone`. Reachable invalid states: `Reserved` with neither a surface nor a disabled entry; a `disabled_outputs` entry with no `Udev.disabled` entry; a disabled entry whose device is gone
- **Transitions:** `device_added` (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:719`; renderer switch `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:754`; insert `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:803`); `device_changed` reserves (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:834`) then `connector_connected` (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:848`), or releases (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:840`) then `connector_disconnected` (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:845`); `connector_connected_as` off by settings (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:998`) or driven (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:1044`, `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:1059`); `configure_output` turns off (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:651`); `enable_output` (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:926`); `connector_disconnected` (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:1070`); `device_removed` (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:852`)
- **Hazards:** the reservation happens before setup and only an unplug undoes it; the first device picks the renderer for the whole session, so a GL-less primary GPU with a GL-capable second GPU renders everything with pixman; the device count changes wl_shm release behaviour (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:375`)
- **Reify:** High — the per-connector phase is spread over four maps in two structs, with reachable invalid combinations
- **Suspected bug:** (1) Any setup failure leaves the connector reserved without a surface: no modes (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:962`), no renderer (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:1010`), or `initialize_output` (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:1037`). Later `Changed` events skip it as known (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:823`), so that monitor never lights until it is replugged (confidence: medium). (2) `enable_output` removes the `disabled` entry (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:928`) before it looks up the device (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:929`), reads the connector (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:934`) and sets the output up (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:936`). On failure the output either still shows as off but is refused by the next enable as "isn't off", or (past `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:935`) becomes unknown to IPC and Settings entirely (confidence: medium). (3) `device_removed` disconnects only the CRTCs that have surfaces (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:853`), so an output turned off on a removed GPU (for example a DisplayLink dock) stays in `Udev.disabled` and `disabled_outputs`. Enabling it fails with "its device is gone" after the entry has been dropped. Unless the settings also say off (which dedupes at `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:1002`), a replug adds a driven output with the same name beside the stale one (confidence: medium)
- **Related:** IPC, settings and wlr-output-management callers of `State::configure_output` (`desktop/aizome/crates/aizome-comp-core/src/outputs.rs:345`); the Settei displays pane

<a id="cpa-06"></a>

### CPA-06 — Winit full-redraw countdown
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/backend/winit.rs:29` (`WinitBackend.full_redraw`); also `.tracker`, `.output` (`desktop/aizome/crates/aizome-comp-core/src/backend/winit.rs:25`, `desktop/aizome/crates/aizome-comp-core/src/backend/winit.rs:28`)
- **Encoding:** a `u8` counting down the frames drawn at buffer age 0 after start or a resize; `Option`s filled in by `init`
- **States:** `Uninitialised` (no tracker) → `FullRedraw(n)` → `Incremental`
- **Transitions:** `new` sets 4 (`desktop/aizome/crates/aizome-comp-core/src/backend/winit.rs:58`); `init` fills tracker and output (`desktop/aizome/crates/aizome-comp-core/src/backend/winit.rs:93`); `Resized` sets 4 (`desktop/aizome/crates/aizome-comp-core/src/backend/winit.rs:46`); `redraw` decrements it and picks the age (`desktop/aizome/crates/aizome-comp-core/src/backend/winit.rs:124`)
- **Hazards:** none observed (a redraw before `init` returns early at `desktop/aizome/crates/aizome-comp-core/src/backend/winit.rs:123`)
- **Reify:** Low — development backend, local

<a id="cpa-07"></a>

### CPA-07 — Blur backdrop cache validity
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/blur.rs:274` (`Probe.under`, `.ids`), `desktop/aizome/crates/aizome-comp-core/src/blur.rs:53` (`Slot.commit`, `.geometry`), `desktop/aizome/crates/aizome-comp-core/src/blur.rs:391` (`Chain.region`, `.used`), `desktop/aizome/crates/aizome-comp-core/src/blur.rs:366` (`Gl.programs`, `.blurs`, `.frame`); `BlurElement.recompute` (`desktop/aizome/crates/aizome-comp-core/src/blur.rs:73`)
- **Encoding:** a per-output snapshot of what lay under each blur last frame; per surface and output, an element id and commit counter kept in the surface's data; per GL context, texture chains with the region each holds and the frame it was last used; the element's `recompute` flag links them
- **States:** per blur `NoChain` → `Fresh(region)` → `Stale` (something under it changed, or it moved) → `Fresh`; `Evicted` (600 frames unused, or its surface is dead)
- **Transitions:** `BlurElement::new` marks it moved and bumps the commit (`desktop/aizome/crates/aizome-comp-core/src/blur.rs:113`); `prepare` compares and renews (`desktop/aizome/crates/aizome-comp-core/src/blur.rs:325`, `desktop/aizome/crates/aizome-comp-core/src/blur.rs:351`) and clears the probe when there is no blur (`desktop/aizome/crates/aizome-comp-core/src/blur.rs:302`); `renew` bumps the commit (`desktop/aizome/crates/aizome-comp-core/src/blur.rs:134`); `draw_gles` compiles once (`desktop/aizome/crates/aizome-comp-core/src/blur.rs:673`), evicts (`desktop/aizome/crates/aizome-comp-core/src/blur.rs:689`), creates (`desktop/aizome/crates/aizome-comp-core/src/blur.rs:704`), and copies again on recompute or a region change (`desktop/aizome/crates/aizome-comp-core/src/blur.rs:715`)
- **Hazards:** `prepare` runs only for screen frames, so captures and mirrors draw whatever chain the screen last kept (correct only while the same context drew it); a failed shader compile is retried, with a warning, every frame (`desktop/aizome/crates/aizome-comp-core/src/blur.rs:679`); `probe.ids` survives a frame without blurs while `under` is cleared (`desktop/aizome/crates/aizome-comp-core/src/blur.rs:303`); eviction counts frames across all of a context's outputs
- **Reify:** Medium — four structures with three different lifetimes; a wrong result is only visual

<a id="cpa-08"></a>

### CPA-08 — Fallback (no-logic) window focus and stacking
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/fallback.rs:84` (`focus`); state in `Aizome.view.focused`, `.stacking`, `Aizome.layer_focus`, `WinView.rect`, `.shown`
- **Encoding:** while no logic is healthy, the core writes the view the logic normally publishes, plus toplevel activation, `layer_focus` for OnDemand layer surfaces, and `cursor_override` over empty space
- **States:** `LogicOwnsView` ↔ `FallbackOwnsView` (decided by `lg.healthy()`, other slice); in fallback, `Focused(window)` | `LayerFocused` | `None`
- **Transitions:** `window_mapped` places, shows, stacks and focuses a window (`desktop/aizome/crates/aizome-comp-core/src/fallback.rs:45`) and drops a pending fullscreen (`desktop/aizome/crates/aizome-comp-core/src/fallback.rs:40`); `window_unmapped` focuses the top window (`desktop/aizome/crates/aizome-comp-core/src/fallback.rs:62`); `focus` raises, sets `focused`, flips activation and clears layer focus (`desktop/aizome/crates/aizome-comp-core/src/fallback.rs:87`); `focus_under` sets layer focus (`desktop/aizome/crates/aizome-comp-core/src/fallback.rs:267`); `pointer_motion` sets or clears the cursor override (`desktop/aizome/crates/aizome-comp-core/src/fallback.rs:212`); maximize and fullscreen requests are acknowledged, not granted (`desktop/aizome/crates/aizome-comp-core/src/fallback.rs:73`)
- **Hazards:** the handover is implicit: the core writes `az.view` only while no logic runs (`desktop/aizome/crates/aizome-comp-core/src/view.rs:10`), and a reloaded logic has to adopt the fallback's `rect` and `stacking`; `cursor_override` is also written by the recovery overlay (CPA-16) and by Reset Input
- **Reify:** Low — small, and only used while the logic is down
- **Related:** the logic host's `healthy` (`desktop/aizome/crates/aizome-comp-core/src/logic.rs:316`); the aizome-comp-logic window model

<a id="cpa-09"></a>

### CPA-09 — Layer-surface ghosts (last frames kept after unmap)
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/ghosts.rs:33` (`Ghost.born`, `.id`); `Aizome.ghosts`, `Aizome.next_ghost`
- **Encoding:** a `Vec<Ghost>` with a `born: Instant` and session-unique ids, gated by the logic's `View.keep_on_unmap` and by a shell handover
- **States:** `Live` → `Ghost(born)` → `DroppedByLogic` | `Expired` (older than 1 s)
- **Transitions:** an unmapping commit or a destroy calls `keep_ghost` (`desktop/aizome/crates/aizome-comp-core/src/ghosts.rs:119`, `desktop/aizome/crates/aizome-comp-core/src/ghosts.rs:140`), which pushes a ghost (`desktop/aizome/crates/aizome-comp-core/src/ghosts.rs:103`); `expire_ghosts` runs for every scene (`desktop/aizome/crates/aizome-comp-core/src/ghosts.rs:154`, called from `desktop/aizome/crates/aizome-comp-core/src/render.rs:49`)
- **Hazards:** expiry runs only when a scene is built, so ghosts outlive 1 s while no output draws (displays off); ghosts are skipped during a shell handover (`desktop/aizome/crates/aizome-comp-core/src/ghosts.rs:120`, `desktop/aizome/crates/aizome-comp-core/src/ghosts.rs:141`), so notification cards vanish rather than slide out then
- **Reify:** Low — small, bounded by `MAX_AGE`
- **Related:** aizome-comp-logic (drops the ghosts it animates); aizome-shell notification cards

<a id="cpa-10"></a>

### CPA-10 — Idle progression to lock and monitors-off
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/idle.rs:17` (`IdleState.last_activity`, `.lock_after`, `.off_after`, `.lock_sent`, `.off_sent`); `Aizome.idle_inhibitors`
- **Encoding:** a last-input timestamp, two thresholds from the settings, two sent flags and the inhibitor set, polled every 5 s
- **States:** `Active` → `LockSent` → `OffSent` (or `OffSent` first when its threshold is shorter) → `Active` on input; `Inhibited` keeps it `Active`
- **Transitions:** `start` arms the 5 s poll (`desktop/aizome/crates/aizome-comp-core/src/idle.rs:46`); `check_idle` →LockSent (`desktop/aizome/crates/aizome-comp-core/src/idle.rs:94`), →OffSent and turns the displays off (`desktop/aizome/crates/aizome-comp-core/src/idle.rs:101`), and inhibitors refresh the activity time (`desktop/aizome/crates/aizome-comp-core/src/idle.rs:85`); `note_activity` →Active, emits `Resumed`, and turns the displays on only from OffSent (`desktop/aizome/crates/aizome-comp-core/src/idle.rs:61`); `drop_idle_inhibitor` (`desktop/aizome/crates/aizome-comp-core/src/idle.rs:78`); `reset_timers` (`desktop/aizome/crates/aizome-comp-core/src/idle.rs:38`)
- **Hazards:** displays turned off over IPC (`PowerOffMonitors`) never set `off_sent`, so input doesn't turn them back on (CPA-15); thresholds change without clearing the sent flags; the poll keeps running while the session is paused (CPA-04)
- **Reify:** Medium — two flags, plus a power flag owned elsewhere
- **Related:** aizome-shell `Event::Idle` handling (`desktop/aizome/crates/aizome-shell/src/shell.rs:1201`)

<a id="cpa-11"></a>

### CPA-11 — Night Light application
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/night.rs:18` (`Night.enabled`, `.temperature`, `.current`, `.started`); per output `OutputState.gamma`, `.gamma_client`
- **Encoding:** IPC overrides, the last applied kelvin (0 means never applied), a minute timer armed once, and a per-output "takes gamma" flag set by the last attempt
- **States:** `NotStarted` → `NeverApplied` (current 0) → `Applied(k)`; a forced re-apply when an output is added or the session resumes
- **Transitions:** the first call arms the timer (`desktop/aizome/crates/aizome-comp-core/src/night.rs:98`); unchanged and not forced returns early (`desktop/aizome/crates/aizome-comp-core/src/night.rs:114`); `current` is updated (`desktop/aizome/crates/aizome-comp-core/src/night.rs:118`) and then applied (`desktop/aizome/crates/aizome-comp-core/src/night.rs:123`), except a neutral first value (`desktop/aizome/crates/aizome-comp-core/src/night.rs:120`); the per-output result is recorded (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:625`); forced from connect and resume (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:1064`, `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:264`) and from enable (`desktop/aizome/crates/aizome-comp-core/src/outputs.rs:357`)
- **Hazards:** `current` is recorded before, and regardless of, gamma success, so after a failed apply the unforced ticks don't retry; an output held by a gamma-control client is skipped (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:611`) and needs a forced update when the client lets go
- **Reify:** Low — small; the forced paths cover the known resets
- **Related:** wlr-gamma-control (comp-core protocols, other slice); Settei and the shell through `NightLightChanged`

<a id="cpa-12"></a>

### CPA-12 — Output layout membership and wl_output global retirement
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/outputs.rs:62` (`attach`), `desktop/aizome/crates/aizome-comp-core/src/outputs.rs:82` (`detach`), `desktop/aizome/crates/aizome-comp-core/src/outputs.rs:58` (`OutputGlobal`); state in `Aizome.outputs`, `.mirrors`, `.disabled_outputs`, `.out`
- **Encoding:** an output's phase is which of three lists hold it, whether `out` has its `OutputState`, and whether its `OutputGlobal` holds a live `GlobalId`; the backends keep parallel maps (udev `disabled`, headless `outputs`)
- **States:** `InLayout` (outputs, out, global) → `Mirroring` (mirrors, out, detached) | `TurnedOff` (disabled_outputs, no out) | `Removed`; the global goes `Advertised` → `Disabled` (5 s) → `Removed`
- **Transitions:** `add_output` (`desktop/aizome/crates/aizome-comp-core/src/outputs.rs:31`); `attach` (`desktop/aizome/crates/aizome-comp-core/src/outputs.rs:62`); `detach` (`desktop/aizome/crates/aizome-comp-core/src/outputs.rs:82`; global disabled `desktop/aizome/crates/aizome-comp-core/src/outputs.rs:95`, removed 5 s later `desktop/aizome/crates/aizome-comp-core/src/outputs.rs:97`); `disable` (`desktop/aizome/crates/aizome-comp-core/src/outputs.rs:111`); `remove_output` (`desktop/aizome/crates/aizome-comp-core/src/outputs.rs:38`); `forget_mirrors` re-attaches the mirrors of a source that leaves (`desktop/aizome/crates/aizome-comp-core/src/outputs.rs:47`); `set_mirror` (`desktop/aizome/crates/aizome-comp-core/src/outputs.rs:129`); `State::configure_output` (`desktop/aizome/crates/aizome-comp-core/src/outputs.rs:345`); `mirror_output` refuses an output that is off (`desktop/aizome/crates/aizome-comp-core/src/outputs.rs:366`)
- **Hazards:** every backend has to pair these calls with its own map (`desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs:161`, `desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs:176`, `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:665`, `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:1059`, `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:1081`), and drift between them produces CPA-05's limbo outputs; `known_outputs` concatenates the lists without dedupe (`desktop/aizome/crates/aizome-comp-core/src/outputs.rs:300`), so a stale off entry and a new driven output with the same name both show; `detach` closes every layer surface on the output (`desktop/aizome/crates/aizome-comp-core/src/outputs.rs:84`), relying on the shell to recreate them
- **Reify:** High — four collections plus user data encode one phase, and backend drift makes invalid combinations reachable
- **Related:** wlr-output-management (`desktop/aizome/crates/aizome-comp-core/src/protocols/output_management.rs`, comp-core); IPC `OutputsChanged` consumers (shell, Settei)

<a id="cpa-13"></a>

### CPA-13 — FIFO barrier release and backstop
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/pacing.rs:45` (`Pacing.backstop`, `.fifo_waiters`, `.released`)
- **Encoding:** smithay fifo barriers in surface state, a list of waiters, a one-shot backstop timer token, and the clients the last frame cleared, waiting for `blocker_cleared`
- **States:** per commit `Waiting(barrier)` → `ClearedByFrame` (client in `released`) → `Unblocked`; or `ClearedByBackstop` (its output hasn't drawn for 1 s)
- **Transitions:** `commit_requested` records the waiter, queues a redraw and arms the backstop (`desktop/aizome/crates/aizome-comp-core/src/pacing.rs:98`); `frame_drawn` signals the barriers on that output (`desktop/aizome/crates/aizome-comp-core/src/pacing.rs:120`); `release_cleared` unblocks (`desktop/aizome/crates/aizome-comp-core/src/pacing.rs:133`, called from `desktop/aizome/crates/aizome-comp-core/src/state.rs:625`); the backstop fires and clears its token (`desktop/aizome/crates/aizome-comp-core/src/pacing.rs:315`), clears surfaces nothing draws, prunes, and re-arms while any waiter remains (`desktop/aizome/crates/aizome-comp-core/src/pacing.rs:166`)
- **Hazards:** `frame_drawn` also runs after undamaged frames (`desktop/aizome/crates/aizome-comp-core/src/backend/mod.rs:317`) and, on udev, before the vblank, so a barrier clears when a frame is queued rather than scanned out; waiters are pruned only by the backstop (`desktop/aizome/crates/aizome-comp-core/src/pacing.rs:178`), so it fires once more after frames have cleared them; "drawing" means a `last_frame` within 1 s (`desktop/aizome/crates/aizome-comp-core/src/pacing.rs:297`), so a surface on an output that just stopped can wait about 2 s; a commit that no path clears stays blocked, along with every later commit (module docs)
- **Reify:** Medium — the timer, the list and smithay's state agree only by convention

<a id="cpa-14"></a>

### CPA-14 — Commit-timing (timestamped commit) release timer
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/pacing.rs:51` (`Pacing.timed`); `OutputState.presented` (set at `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:1112`)
- **Encoding:** one `Option<(RegistrationToken, Duration)>` for the earliest wake, smithay's per-surface commit timers, and vblanks predicted from the last presentation time while it is under 1 s old
- **States:** `Unarmed` → `Armed(at)` → `Fired` → `Armed(next)` | `Unarmed`
- **Transitions:** `commit_requested` arms (`desktop/aizome/crates/aizome-comp-core/src/pacing.rs:107`); `arm_timed` keeps the earliest wake and replaces a later one (`desktop/aizome/crates/aizome-comp-core/src/pacing.rs:325`); the timer fires, clears the slot and runs `timed_due` (`desktop/aizome/crates/aizome-comp-core/src/pacing.rs:333`); `timed_due` queues frames for surfaces that are drawn, signals the ones that aren't, and re-arms (`desktop/aizome/crates/aizome-comp-core/src/pacing.rs:186`); `release_timed` signals due timers before a frame and re-arms (`desktop/aizome/crates/aizome-comp-core/src/pacing.rs:141`)
- **Hazards:** only udev reports `presented`, so headless and winit release at the timestamp itself (`desktop/aizome/crates/aizome-comp-core/src/pacing.rs:365`); a drawn surface whose frame never comes depends on the `now + BACKSTOP` re-arm (`desktop/aizome/crates/aizome-comp-core/src/pacing.rs:206`)
- **Reify:** Low — one timer slot, local logic

<a id="cpa-15"></a>

### CPA-15 — Display power and lid docking layout
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/power.rs:16` (`Power.monitors_off`, `.lid_closed`, `.lid_disabled`); `OutputState.powered`
- **Encoding:** a global off flag (idle or IPC), the lid switch, snapshots of panels taken out of the layout while docked, and each output's `powered` flag and (on udev) cleared CRTC
- **States:** per panel `On` → `OffForIdle` | `OffBehindLid` (in the layout, unpowered) | `LidDisabled` (out of the layout, snapshot kept) → `Restored`; external outputs `On` ↔ `OffForIdle`
- **Transitions:** `set_monitors_power` (`desktop/aizome/crates/aizome-comp-core/src/power.rs:45`); `lid_switched` (`desktop/aizome/crates/aizome-comp-core/src/power.rs:52`; opening the lid counts as activity, `desktop/aizome/crates/aizome-comp-core/src/power.rs:58`); `apply_power` runs `apply_lid_layout`, which disables and snapshots (`desktop/aizome/crates/aizome-comp-core/src/power.rs:84`) or restores everything (`desktop/aizome/crates/aizome-comp-core/src/power.rs:73`), then `Backend::apply_power` flips `powered` and re-queues (`desktop/aizome/crates/aizome-comp-core/src/backend/mod.rs:190`), then udev `set_power` (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:634`); an explicit disable drops the snapshot (`desktop/aizome/crates/aizome-comp-core/src/outputs.rs:355`); it all runs on every `outputs_dirty` (`desktop/aizome/crates/aizome-comp-core/src/state.rs:579`)
- **Hazards:** IPC `PowerOffMonitors` sets `monitors_off` without setting `IdleState.off_sent`, so neither input (`desktop/aizome/crates/aizome-comp-core/src/idle.rs:62`) nor opening the lid turns the displays back on; only `PowerOnMonitors` does; changes made to a lid-disabled panel are silently dropped (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:648`), and the stale snapshot is what gets restored; a failed restore discards its snapshot (`desktop/aizome/crates/aizome-comp-core/src/power.rs:74`, `desktop/aizome/crates/aizome-comp-core/src/power.rs:79`); a mirror source behind the lid stays lit (`desktop/aizome/crates/aizome-comp-core/src/power.rs:35`, `desktop/aizome/crates/aizome-comp-core/src/power.rs:87`)
- **Reify:** High — the panel's phase spans three fields, the layout lists and the backend, with user-visible results
- **Related:** the libinput lid switch (`desktop/aizome/crates/aizome-comp-core/src/input.rs:227`); `aizomectl power-off-monitors`

<a id="cpa-16"></a>

### CPA-16 — Recovery overlay session (open and closed)
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:176` (`Recovery.open`), with `.output` (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:178`), `.sel`, `.last_app`, `.hover`, `.scroll`, `.message`, `.notice`, `.opened_at` (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:203`), `.gpu_warm` (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:213`); `Aizome.cursor_override`
- **Encoding:** an `open: bool` with about a dozen fields that only mean something while open, the output picked at open, the cursor override, a "first frame drawn" timestamp, and the prewarm key
- **States:** `Closed` → `Open(output, sel)` → `Closed`; `PrewarmFakeOpen` (open set true around an offscreen capture); `NoticePending` (a stall notice waiting for the next open)
- **Transitions:** `open` (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:505`; message from the notice `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:512`; selection `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:526`; `take_input` `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:531`); `close` (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:543`); `toggle` (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:559`) on Ctrl+Alt+Del (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:628`) and over IPC (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:1062`); closed by Escape (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:659`), a click outside (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:825`), Reset Input (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:911`), Lock (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:930`) and Text Console (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:966`); `prewarm_gpu` turns `open` on and off (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:369`, `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:376`); the stall notice opens it at start (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:274`); the first frame consumes `opened_at` (`desktop/aizome/crates/aizome-comp-core/src/recovery/view.rs:257`); `refresh_rows` repairs the selection (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:424`)
- **Hazards:** `prewarm_gpu` makes `is_open()` true during a synchronous capture and runs `refresh_rows` while closed (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:368`); `output` is never reset when its output leaves, so the panel is drawn on no output (`desktop/aizome/crates/aizome-comp-core/src/recovery/view.rs:270`) while hit testing still uses the departed output's geometry (`desktop/aizome/crates/aizome-comp-core/src/recovery/view.rs:159`); `cursor_override` has three writers (here, `desktop/aizome/crates/aizome-comp-core/src/fallback.rs:213`, `desktop/aizome/crates/aizome-comp-core/src/recovery/reset.rs:145`)
- **Reify:** High — many dependent fields, many entry points, user-visible
- **Suspected bug:** open the overlay with the pointer on a laptop panel while an external display is connected, then close the lid (or unplug the display the overlay opened on). That output leaves the layout (CPA-15, CPA-05) but `recovery.output` still names it, so the overlay shows only the scrim with no panel. Keys still act unseen, and a click closes it (confidence: medium)

<a id="cpa-17"></a>

### CPA-17 — Two-press confirmation (Log Out, Text Console)
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:192` (`Recovery.confirm: Option<(Act, Instant)>`)
- **Encoding:** the action waiting for its second press and when it was asked; valid for 4 s (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:170`)
- **States:** `None` → `Awaiting(act, t)` → `Confirmed` (acts and clears) | `Expired` (implicit, by time) | `Cancelled`
- **Transitions:** the first press sets it (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:940`, `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:960`); a second press within 4 s clears it and acts (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:943`, `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:965`); a selection change cancels it (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:747`); open and close clear it (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:509`, `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:546`); with the overlay closed, or over IPC, the action confirms at once (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:938`, `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:958`); it is drawn as the button label (`desktop/aizome/crates/aizome-comp-core/src/recovery/view.rs:372`)
- **Hazards:** expiry isn't an event, so a stale prompt stays drawn until something redraws
- **Reify:** Medium — small, but the view assumes only Log Out uses it
- **Suspected bug:** after the first Text Console press, the Text Console button reads "Really Log Out?" (the label is hard-coded for any pending confirmation, `desktop/aizome/crates/aizome-comp-core/src/recovery/view.rs:377`), on the screen meant to rescue a stuck session (confidence: high)

<a id="cpa-18"></a>

### CPA-18 — Overlay ownership of key and button releases
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:195` (`Recovery.swallow_keys`, `.swallow_buttons`)
- **Encoding:** sets of keycodes and buttons pressed into the overlay, whose releases it owns even after it closes
- **States:** per key `Free` → `Owned` (pressed into the overlay) → `Free` (release swallowed)
- **Transitions:** a Ctrl+Alt+Del press (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:629`), any key press while open (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:636`), a button press while open (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:805`); a release removes it and is swallowed (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:615`, `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:800`); while open, the release of a key pressed before opening is swallowed and dropped from the suppressed sets (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:618`)
- **Hazards:** a release that never reaches this hook (a VT switch from Text Console with the key still down, `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:967`; a device removed) leaves the code owned, so after a later normal press its release is swallowed and the client sees a stuck key or button; Reset Input doesn't clear these sets
- **Reify:** Low — two sets with one owner

<a id="cpa-19"></a>

### CPA-19 — Pointer-constraint hold across the overlay and Reset Input
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:208` (`Recovery.held_constraint`), `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:211` (`Recovery.unconstrained`)
- **Encoding:** the surface whose lock or confine was active when the overlay opened, and the surface whose constraints stay off after Reset Input until it is clicked or asks again
- **States:** `Free` → `Held(surface)` (overlay open) → `Unconstrained(surface)` (after a reset) → `Free`
- **Transitions:** `take_input` records and deactivates (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:573`); `reset_input` takes it and sets `unconstrained` (`desktop/aizome/crates/aizome-comp-core/src/recovery/reset.rs:89`); a click on that surface, or the surface dying, frees it (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:789`); a new constraint request frees it (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:772`); `may_constrain` gates reactivation (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:780`)
- **Hazards:** `close` doesn't clear `held_constraint`, so a later Reset Input over IPC reports and holds a stale surface (`desktop/aizome/crates/aizome-comp-core/src/recovery/reset.rs:90`); a dead `unconstrained` surface is noticed only at the next button press
- **Reify:** Low — two options, few sites
- **Related:** pointer-constraint activation in `desktop/aizome/crates/aizome-comp-core/src/input.rs` (comp-core, other slice)

<a id="cpa-20"></a>

### CPA-20 — Recovery ping and sample cadence, and the sample cache
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:197` (`Recovery.samples`), `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:200` (`.last_ping`), `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:201` (`.last_sample`)
- **Encoding:** the last ping and sample times throttle a repeating tick whose period depends on `open`; a map of each app's latest sample, never pruned, from which the rows are derived
- **States:** `ClosedCadence` (ping every 2 s) ↔ `OpenCadence` (ping every 0.5 s, sample every 1 s); per app sample `Absent` → `Fresh` → `Stale` (kept after the app is gone)
- **Transitions:** the tick (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:285`, `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:293`); `ping_all` (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:329`); `request_sample` (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:453`) from open (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:533`) and from the tick (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:305`); a report stores samples (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:470`); rows read `crashed` and `stopped` (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:401`, `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:415`)
- **Hazards:** samples are never removed, so a relaunched app inherits its previous instance's `crashed` and `stopped` until its first new sample
- **Reify:** Medium — the cache decides the default action
- **Suspected bug:** after an app with a CRASHED sample is relaunched with the same app ID, IPC `RecoveryState` with the overlay closed (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:1071`) keeps reporting it as crashed, since nothing is sampled while closed. At the next Ctrl+Alt+Del it is preselected as the first app in trouble (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:518`), and Enter force-quits the healthy app (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:758`) until the first new sample lands (confidence: medium)

<a id="cpa-21"></a>

### CPA-21 — Force Quit escalation (SIGKILL, then cut the connection)
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:976` (`force_quit`); the linger timer at `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:994`
- **Encoding:** a Kill request to the worker thread, a one-shot 1.5 s timer holding the app ID and PIDs, and the `Report::Killed` message
- **States:** `Requested` → `Killed` (report) | `NoReport` → `Lingering` (1.5 s) → `Disconnected` | `Gone`
- **Transitions:** send Kill (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:988`); arm the linger timer (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:994`); the report sets the message (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:477`); `disconnect_lingering` kills the clients still owning the app's windows (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:1004`)
- **Hazards:** the linger step runs whether or not the worker killed anything (CPA-24), and it skips xwayland-satellite (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:991`), so an X11 app depends entirely on the worker; a report that arrives while the overlay is closed sets a `message` only IPC sees
- **Reify:** Medium — the outcome is split across a thread, a timer and a message
- **Related:** the process worker `desktop/aizome/crates/aizome-comp-core/src/recovery/procinfo.rs` (CPA-24)

<a id="cpa-22"></a>

### CPA-22 — Re-locking after a shell restart while locked
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:1029` (`relock_when_shell_returns`)
- **Encoding:** a repeating timer that captures a 20 s deadline and the IPC subscriber count at restart, and reads the lock state and lock surfaces on each tick
- **States:** `Waiting` → `Asked` (lock event emitted, repeated every second) → `Done` (a lock surface exists, or unlocked) | `GaveUp` (20 s)
- **Transitions:** armed from Restart Shell while locked (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:919`); a tick drops the timer when unlocked, when a lock surface exists, or past the deadline (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:1034`); it emits the lock event once subscribers reach the baseline (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:1038`)
- **Hazards:** the baseline is taken right after SIGKILL (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:1031`), while the old shell's IPC connection may still be counted, and any subscriber (aizomectl, Settei) satisfies it, so the event can go out before the new shell listens (it repeats each second, so this is usually harmless); after 20 s the session stays locked with no lock surface and nothing retries
- **Reify:** Medium — a wait encoded in a timer, with cross-process conditions
- **Related:** the aizome-shell lock screen (`desktop/aizome/crates/aizome-shell/src/shell.rs`); ext-session-lock in comp-core

<a id="cpa-23"></a>

### CPA-23 — Per-client ping and pong responsiveness
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/recovery/ping.rs:20` (`Pending(Mutex<Option<Instant>>)`, kept in the shell client's data)
- **Encoding:** when the outstanding ping went out, per xdg_wm_base client
- **States:** `Answered` (None) → `Waiting(t)` → `NotResponding` (2.5 s or more) → `Answered` on pong
- **Transitions:** `ping` sends and stamps (`desktop/aizome/crates/aizome-comp-core/src/recovery/ping.rs:45`); `pong` clears (`desktop/aizome/crates/aizome-comp-core/src/recovery/ping.rs:56`); `not_responding` reads it (`desktop/aizome/crates/aizome-comp-core/src/recovery/ping.rs:68`)
- **Hazards:** when smithay refuses because another ping is already pending, the stamp is still set from now (`desktop/aizome/crates/aizome-comp-core/src/recovery/ping.rs:48`), so the wait is timed from this attempt and only a pong clears it
- **Reify:** Low — one option per client, gone with the client
- **Related:** the xdg-shell pong handler in comp-core (other slice)

<a id="cpa-24"></a>

### CPA-24 — Process-info worker: request coalescing, CPU baselines and kills
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/recovery/procinfo.rs:140` (`run`), `desktop/aizome/crates/aizome-comp-core/src/recovery/procinfo.rs:173` (`Meter.last`), `desktop/aizome/crates/aizome-comp-core/src/recovery/procinfo.rs:113` (`ProcInfo.tx`); `Recovery.procinfo` (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:199`)
- **Encoding:** a worker thread draining an mpsc queue (keeping only the newest Sample and doing Kills during the drain), per-app CPU baselines for percentages, and a calloop channel back; an `Option<ProcInfo>` on the main side
- **States:** thread `NotStarted` → `Idle` (blocked in recv) → `Busy(sample | kill)` → `Exited` (report channel closed); per app baseline `Fresh` (two reads 250 ms apart) → `Known`
- **Transitions:** `start` (`desktop/aizome/crates/aizome-comp-core/src/recovery/procinfo.rs:118`, called from `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:266`); drain (`desktop/aizome/crates/aizome-comp-core/src/recovery/procinfo.rs:145`); act on `req` (`desktop/aizome/crates/aizome-comp-core/src/recovery/procinfo.rs:155`); exit (`desktop/aizome/crates/aizome-comp-core/src/recovery/procinfo.rs:163`); baselines read and pruned (`desktop/aizome/crates/aizome-comp-core/src/recovery/procinfo.rs:191`, `desktop/aizome/crates/aizome-comp-core/src/recovery/procinfo.rs:240`)
- **Hazards:** everything runs serially on one thread, and X11 lookups wait up to 500 ms each (`desktop/aizome/crates/aizome-comp-core/src/recovery/x11.rs:28`), twice for a fresh app, so Kills queue behind slow Samples; nothing restarts the thread if it dies (sends just fail, `desktop/aizome/crates/aizome-comp-core/src/recovery/procinfo.rs:136`)
- **Reify:** High — the coalescing can drop a privileged action in the recovery path
- **Suspected bug:** when the first request the drain sees is a Kill, a Sample queued behind it overwrites it (`s => req = s`, `desktop/aizome/crates/aizome-comp-core/src/recovery/procinfo.rs:152`). With the worker busy (the fresh-app 250 ms sleep, slow X11 lookups for a hung X app), pressing Force Quit followed by the open overlay's 1 s sample (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:305`) silently loses the kill: no SIGKILL and no `Killed` report, and for X11 apps the linger step skips satellite (confidence: high)
- **Related:** systemd cgroups (`cgroup.kill`); CPA-21

<a id="cpa-25"></a>

### CPA-25 — X11 PID lookup thread with a deadline
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/recovery/x11.rs:19` (`pids_for`)
- **Encoding:** one thread per lookup and an mpsc receiver with a 500 ms timeout; an abandoned thread keeps running
- **States:** `Spawned` → `Answered` | `TimedOut` (thread abandoned, possibly blocked forever)
- **Transitions:** spawn (`desktop/aizome/crates/aizome-comp-core/src/recovery/x11.rs:22`); wait (`desktop/aizome/crates/aizome-comp-core/src/recovery/x11.rs:28`)
- **Hazards:** x11rb's connect and replies have no timeout (`desktop/aizome/crates/aizome-comp-core/src/recovery/x11.rs:32`), so with a hung X server each abandoned thread and its X connection stay blocked indefinitely
- **Reify:** Low — local, but it leaks under exactly the failure it exists for
- **Suspected bug:** with xwayland-satellite's X server hung, every sample while the overlay is open spawns, for each X11 app (twice for a fresh app), a thread that never ends; keeping the overlay open for a minute leaks on the order of a hundred threads and sockets (confidence: medium)
- **Related:** xwayland-satellite, supervised in `desktop/aizome/crates/aizome-comp-core/src/session.rs` (CPA-27)

<a id="cpa-26"></a>

### CPA-26 — Deferred logic reload request
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/reload.rs:14` (`LogicHost.pending_reload`, declared at `desktop/aizome/crates/aizome-comp-core/src/logic.rs:258`); also `desktop/aizome/crates/aizome-comp-core/src/reload.rs:30`
- **Encoding:** an `Option<ReloadArgs>`, set when a reload is asked for from inside a logic call and consumed at the loop's safe point
- **States:** `None` → `Pending(args)` → `Reloading` → `None`
- **Transitions:** `reload_compositor` defers while `in_call` (`desktop/aizome/crates/aizome-comp-core/src/reload.rs:12`) or reloads now (`desktop/aizome/crates/aizome-comp-core/src/reload.rs:17`); `reload_soon` (`desktop/aizome/crates/aizome-comp-core/src/reload.rs:30`); consumed outside this slice (`desktop/aizome/crates/aizome-comp-core/src/logic.rs:479`)
- **Hazards:** a second request overwrites the first one's arguments; the deferred path reports "Reloading Aizome" before the outcome is known
- **Reify:** Low — one option with a single consumer

<a id="cpa-27"></a>

### CPA-27 — Supervised helper process lifecycle (shell, xwayland-satellite)
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/session.rs:24` (`Supervised.child`, `.started`, `.backoff`, `.restart_at`, `.retiring`, `.restart_now`)
- **Encoding:** an `Option<Child>`, a start time, exponential backoff, an `Option<Instant>` restart deadline, a retiring predecessor and a restart-at-once flag, polled by a timer every 1 s (0.1 s during a handover, 50 ms after a forced restart)
- **States:** `NotRunning` → `Running(child)` → `Exited` → `BackingOff(restart_at)` → `Running`; `ForcedRestart` (restart_now: SIGKILL, then start on exit without backoff); `Retiring` (the old child beside a new one, CPA-28). Reachable invalid state: `child None && restart_at None` (never restarted)
- **Transitions:** created (`desktop/aizome/crates/aizome-comp-core/src/session.rs:279`) and started (`desktop/aizome/crates/aizome-comp-core/src/session.rs:206`); `start_child` succeeds (`desktop/aizome/crates/aizome-comp-core/src/session.rs:453`) or fails (`desktop/aizome/crates/aizome-comp-core/src/session.rs:457`); `supervise` reaps and backs off (`desktop/aizome/crates/aizome-comp-core/src/session.rs:467`) or restarts at once (`desktop/aizome/crates/aizome-comp-core/src/session.rs:469`), and starts a due restart (`desktop/aizome/crates/aizome-comp-core/src/session.rs:483`); `restart_supervised` (`desktop/aizome/crates/aizome-comp-core/src/session.rs:505`) with a fast poll (`desktop/aizome/crates/aizome-comp-core/src/session.rs:521`); `restart_shell` moves the child into `retiring` (`desktop/aizome/crates/aizome-comp-core/src/session.rs:337`); `stop_children` (`desktop/aizome/crates/aizome-comp-core/src/session.rs:534`)
- **Hazards:** `try_wait` errors are ignored (`desktop/aizome/crates/aizome-comp-core/src/session.rs:467`), so such a child counts as running; the backoff resets only after 60 s of uptime (`desktop/aizome/crates/aizome-comp-core/src/session.rs:477`)
- **Reify:** High — six fields, several pollers, and a reachable never-restarted state
- **Suspected bug:** (1) A helper whose first spawn fails (`desktop/aizome/crates/aizome-comp-core/src/session.rs:457`) keeps `restart_at` at None and is never retried (`desktop/aizome/crates/aizome-comp-core/src/session.rs:483`), so a transient spawn failure at login leaves the session with no shell, or no X11, until Restart Shell (confidence: high). (2) If the new binary given to `restart_shell` fails to exec, the old shell has already been moved to `retiring` (`desktop/aizome/crates/aizome-comp-core/src/session.rs:339`), and the call returns before any handover is set (`desktop/aizome/crates/aizome-comp-core/src/session.rs:346`). The running shell is no longer supervised, and a second `restart_shell` overwrites `retiring` with None, dropping its handle: two shells then run, and the old one is never stopped (confidence: medium)
- **Related:** aizome-shell (`AIZOME_SHELL_PREVIOUS_PID` in `desktop/aizome/crates/aizome-shell/src/carry.rs:103`); xwayland-satellite

<a id="cpa-28"></a>

### CPA-28 — Shell restart handover
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/session.rs:59` (`Handover.old`, `.new`, `.started`); `Children.handover` (`desktop/aizome/crates/aizome-comp-core/src/session.rs:50`)
- **Encoding:** the old and new shell PIDs and the start time; the old `Child` in `Supervised.retiring`; layer surfaces matched to a shell by client PID; layout and ghosts held while a handover is set
- **States:** `None` → `HandingOver(old, new)` (each new-shell surface replaces the old shell's surface with the same namespace) → `Finished` (old shell sent SIGTERM, then SIGKILL after 5 s; windows refit)
- **Transitions:** `restart_shell` refuses while one is under way or while locked (`desktop/aizome/crates/aizome-comp-core/src/session.rs:319`), and sets it (`desktop/aizome/crates/aizome-comp-core/src/session.rs:348`); `shell_handover_layer` unmaps the old shell's surface with the same namespace (`desktop/aizome/crates/aizome-comp-core/src/session.rs:363`); `check_handover` finishes when the old shell has no surfaces, after 8 s, or when the new one has died (`desktop/aizome/crates/aizome-comp-core/src/session.rs:410`), then clears it (`desktop/aizome/crates/aizome-comp-core/src/session.rs:415`), signals and reaps (`desktop/aizome/crates/aizome-comp-core/src/session.rs:416`), and refits windows (`desktop/aizome/crates/aizome-comp-core/src/session.rs:441`); ghosts and layout read the hold (`desktop/aizome/crates/aizome-comp-core/src/session.rs:356`, `desktop/aizome/crates/aizome-comp-core/src/ghosts.rs:120`)
- **Hazards:** if the new shell dies mid-handover, the working old shell is still terminated (`desktop/aizome/crates/aizome-comp-core/src/session.rs:410`, `desktop/aizome/crates/aizome-comp-core/src/session.rs:422`) and the broken binary keeps restarting with backoff (its path was written into `argv[0]` at `desktop/aizome/crates/aizome-comp-core/src/session.rs:336`); surfaces are tied to shells by client PID, so surfaces from a shell's child process never count; Recovery's Restart Shell during a handover SIGKILLs the new shell, which ends the handover by killing the old one too
- **Reify:** High — a cross-process handover spread over two structs and three handlers
- **Related:** aizome-shell state carry-over (`desktop/aizome/crates/aizome-shell/src/carry.rs`); D-Bus name takeover

<a id="cpa-29"></a>

### CPA-29 — Session start-up and systemd notification sequence
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/session.rs:168` (`start`), `desktop/aizome/crates/aizome-comp-core/src/session.rs:254` (`notify_ready`), `desktop/aizome/crates/aizome-comp-core/src/session.rs:258` (`notify_stopping`)
- **Encoding:** the order of steps in `start`: helpers spawned; environment imported into systemd and D-Bus activation; `READY=1`; `aizome-session.target` started. `AIZOME_SESSION` tells children whether this is a real session
- **States:** `Starting` → `HelpersSpawned` → `EnvExported` → `Ready` → `TargetStarted` → … → `Stopping`
- **Transitions:** environment and helpers (`desktop/aizome/crates/aizome-comp-core/src/session.rs:172`); import-environment (`desktop/aizome/crates/aizome-comp-core/src/session.rs:221`); D-Bus activation environment (`desktop/aizome/crates/aizome-comp-core/src/session.rs:226`); READY (`desktop/aizome/crates/aizome-comp-core/src/session.rs:231`); target started (`desktop/aizome/crates/aizome-comp-core/src/session.rs:233`); STOPPING (`desktop/aizome/crates/aizome-comp-core/src/session.rs:258`, called from `desktop/aizome/crates/aizome-comp-core/src/input.rs:549`)
- **Hazards:** helpers start before the environment is exported, so anything they activate over D-Bus straight away may miss it; `run_quietly` blocks the main loop on each command (`desktop/aizome/crates/aizome-comp-core/src/session.rs:245`); READY is sent even when the imports failed
- **Reify:** Low — a linear one-shot sequence
- **Related:** `desktop/aizome/session/aizome.service` (`Type=notify`, `WatchdogSec=30s`); `desktop/aizome/nix/module.nix`

<a id="cpa-30"></a>

### CPA-30 — Pixman thumbnail cache
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/thumbs.rs:42` (`Entry.k`, `.scale`, `.tracker`, `.used`), `desktop/aizome/crates/aizome-comp-core/src/thumbs.rs:56` (`ThumbCache.entries`)
- **Encoding:** pictures keyed by (surface id, size), valid while the scale factor and output scale match, each with a damage tracker that remembers the last picture, dropped 10 s after last use
- **States:** `Absent` → `Baked` → `Invalidated` (k or scale changed) → `Baked` | `Expired`
- **Transitions:** invalidate (`desktop/aizome/crates/aizome-comp-core/src/thumbs.rs:96`); create (`desktop/aizome/crates/aizome-comp-core/src/thumbs.rs:99`); use (`desktop/aizome/crates/aizome-comp-core/src/thumbs.rs:110`); redraw the damage and copy it (`desktop/aizome/crates/aizome-comp-core/src/thumbs.rs:127`); expire (`desktop/aizome/crates/aizome-comp-core/src/thumbs.rs:80`)
- **Hazards:** expiry runs only inside `bake` (pixman frames and captures), so entries outlive 10 s while nothing draws
- **Reify:** Low — a self-contained cache

<a id="cpa-31"></a>

### CPA-31 — Logic timer generation guard
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/timers.rs:22` (`schedule`)
- **Encoding:** each timer captures the logic host's generation when it is scheduled, and a mismatch at fire time drops the call
- **States:** `Scheduled(gen)` → `Fired` (same logic) | `Dropped` (reloaded) | `Cancelled`
- **Transitions:** schedule (`desktop/aizome/crates/aizome-comp-core/src/timers.rs:23`); the check when it fires (`desktop/aizome/crates/aizome-comp-core/src/timers.rs:27`); cancel (`desktop/aizome/crates/aizome-comp-core/src/timers.rs:37`)
- **Hazards:** a reload of the same logic with carried-over state silently drops its pending timers, so the logic has to re-arm anything its saved state still waits for
- **Reify:** Low — an explicit epoch check at a single site
- **Related:** the logic host's generation (`desktop/aizome/crates/aizome-comp-core/src/logic.rs:306`)

<a id="cpa-32"></a>

### CPA-32 — Main-loop stall watcher, systemd watchdog and the stall marker
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:40` (`BEAT`), `desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:42` (`CAPTURED`), `desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:43` (`BACKTRACE`), `desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:88` (`watch`'s `stalled_since`, `captured`); the marker file `$XDG_STATE_HOME/aizome/stalled` (`desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:196`)
- **Encoding:** a beat timestamp written by the loop, the watcher thread's stall phase, a signal-handler handshake for the backtrace, an appended log, and a marker file that outlives the process
- **States:** `Running` → `Stalled` (logged) → `Captured` (backtrace, log, marker) → `Recovered` (marker removed) | `Killed` (marker kept) → at the next start `NoticePending` → shown in the overlay
- **Transitions:** `beat` (`desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:51`); the keepalive timer (`desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:69`); stall (`desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:96`); recover (`desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:101`, marker removed at `desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:106`); capture (`desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:110`, `mark` at `desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:115`); signal handshake (`desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:132`, `desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:164`); `previous_stall` consumes the marker (`desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:216`) when recovery starts (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:274`)
- **Hazards:** the marker lives in the user's state directory, shared by every Aizome instance that user runs, so a nested or headless run can consume or report another instance's stall within 10 min (`desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:222`)
- **Reify:** High — a phase persisted across restarts, plus a thread, atomics and a signal handshake
- **Suspected bug:** the signal handler allocates (`Backtrace::force_capture`, `desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:169`). If the stall is the main thread slowly allocating (malloc in mmap or brk under memory pressure, holding an arena lock), the handler's own allocation deadlocks it for good, turning a recoverable stall into a permanent one; without systemd's watchdog (`desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:35`) nothing kills it (confidence: low)
- **Related:** `desktop/aizome/session/aizome.service` (`WatchdogSec=30s`, restarted by aizome-session)

## Already explicit

- None in this slice.

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs` | 389 | machines | [CPA-01](#cpa-01), [CPA-02](#cpa-02) | refresh timer and buffer age; frame gate |
| `desktop/aizome/crates/aizome-comp-core/src/backend/mod.rs` | 606 | machines | [CPA-02](#cpa-02), [CPA-15](#cpa-15) | redraw scheduling, post_frame, apply_power |
| `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs` | 1222 | machines | [CPA-03](#cpa-03), [CPA-04](#cpa-04), [CPA-05](#cpa-05), [CPA-02](#cpa-02), [CPA-11](#cpa-11), [CPA-15](#cpa-15) | page flips, libseat pause/resume, hotplug and enablement |
| `desktop/aizome/crates/aizome-comp-core/src/backend/winit.rs` | 181 | machines | [CPA-06](#cpa-06), [CPA-02](#cpa-02) | full-redraw countdown |
| `desktop/aizome/crates/aizome-comp-core/src/blur.rs` | 780 | machines | [CPA-07](#cpa-07) | blur backdrop cache validity |
| `desktop/aizome/crates/aizome-comp-core/src/fallback.rs` | 274 | machines | [CPA-08](#cpa-08) | no-logic focus and stacking |
| `desktop/aizome/crates/aizome-comp-core/src/ghosts.rs` | 157 | machines | [CPA-09](#cpa-09) | layer ghosts with expiry |
| `desktop/aizome/crates/aizome-comp-core/src/idle.rs` | 106 | machines | [CPA-10](#cpa-10) | idle lock and monitors-off flags |
| `desktop/aizome/crates/aizome-comp-core/src/night.rs` | 144 | machines | [CPA-11](#cpa-11) | night light application |
| `desktop/aizome/crates/aizome-comp-core/src/outputs.rs` | 388 | machines | [CPA-12](#cpa-12), [CPA-15](#cpa-15) | layout membership and global retirement |
| `desktop/aizome/crates/aizome-comp-core/src/pacing.rs` | 419 | machines | [CPA-13](#cpa-13), [CPA-14](#cpa-14) | fifo backstop and commit-timing timer |
| `desktop/aizome/crates/aizome-comp-core/src/painter.rs` | 376 | none |  | per-frame overlay scene builder; no persistent state |
| `desktop/aizome/crates/aizome-comp-core/src/power.rs` | 120 | machines | [CPA-15](#cpa-15) | display power and lid docking layout |
| `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs` | 1187 | machines | [CPA-16](#cpa-16), [CPA-17](#cpa-17), [CPA-18](#cpa-18), [CPA-19](#cpa-19), [CPA-20](#cpa-20), [CPA-21](#cpa-21), [CPA-22](#cpa-22) | recovery overlay session and actions |
| `desktop/aizome/crates/aizome-comp-core/src/recovery/ping.rs` | 70 | machines | [CPA-23](#cpa-23) | per-client ping/pong |
| `desktop/aizome/crates/aizome-comp-core/src/recovery/procinfo.rs` | 493 | machines | [CPA-24](#cpa-24) | worker coalescing drops kills |
| `desktop/aizome/crates/aizome-comp-core/src/recovery/reset.rs` | 153 | machines | [CPA-19](#cpa-19), [CPA-16](#cpa-16) | one-shot reset; writes constraint hold and cursor |
| `desktop/aizome/crates/aizome-comp-core/src/recovery/view.rs` | 528 | machines | [CPA-16](#cpa-16), [CPA-17](#cpa-17) | draw and hit test; first-frame stamp, confirm label |
| `desktop/aizome/crates/aizome-comp-core/src/recovery/x11.rs` | 85 | machines | [CPA-25](#cpa-25) | abandoned lookup threads |
| `desktop/aizome/crates/aizome-comp-core/src/reload.rs` | 32 | machines | [CPA-26](#cpa-26) | deferred reload request |
| `desktop/aizome/crates/aizome-comp-core/src/render.rs` | 272 | none |  | scene assembly; dispatches on lock, overlay and logic state owned elsewhere |
| `desktop/aizome/crates/aizome-comp-core/src/scene.rs` | 60 | none |  | data types |
| `desktop/aizome/crates/aizome-comp-core/src/session.rs` | 555 | machines | [CPA-27](#cpa-27), [CPA-28](#cpa-28), [CPA-29](#cpa-29) | helper supervision, shell handover, startup |
| `desktop/aizome/crates/aizome-comp-core/src/stats.rs` | 106 | none |  | rolling frame-time counters |
| `desktop/aizome/crates/aizome-comp-core/src/thumbs.rs` | 167 | machines | [CPA-30](#cpa-30) | pixman thumbnail cache |
| `desktop/aizome/crates/aizome-comp-core/src/timers.rs` | 39 | machines | [CPA-31](#cpa-31) | logic timer generation guard |
| `desktop/aizome/crates/aizome-comp-core/src/view.rs` | 70 | machines | [CPA-02](#cpa-02), [CPA-08](#cpa-08), [CPA-09](#cpa-09) | logic-published snapshot; fields feed other entries |
| `desktop/aizome/crates/aizome-comp-core/src/watchdog.rs` | 278 | machines | [CPA-32](#cpa-32) | stall watcher and marker file |
