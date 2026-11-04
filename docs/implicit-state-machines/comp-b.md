# CPB: Compositor core: state, input, focus and protocols

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/crates/aizome-comp-core`. Audited at `8e0acbe5`; line numbers refer to that commit.

31 files, 0 pending. 34 implicit machines: 12 High, 12 Medium, 10 Low.

## Machines

<a id="cpb-01"></a>

### CPB-01 — Live settings reconciliation against the last-applied snapshot
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/config.rs:27` (`State::apply_settings`); also `desktop/aizome/crates/aizome-comp-core/src/state.rs:294` (`Aizome.settings`), `desktop/aizome/crates/aizome-comp-core/src/lib.rs:198` (watcher channel)
- **Encoding:** `az.settings` serves as both the wanted settings and the record of what was applied; derived state (`cursor_theme`, `palette`, repeat info, `keyboards.active`, outputs, night light, idle timers) changes only where the old and new snapshots differ.
- **States:** `Applied(snapshot)`; for each subsystem `InSync` | `Diverged` (its apply failed, but the snapshot already counts it as applied)
- **Transitions:** watcher tick → `reload_config` (`lib.rs:201-207`, `config.rs:20-25`); snapshot swapped before applying (`config.rs:28`); cursor theme reload (`config.rs:41`); keymap (`config.rs:47`); repeat (`config.rs:52`); libinput (`config.rs:59`); outputs (`config.rs:64`); night light (`config.rs:67`); idle timers reset unconditionally (`config.rs:70`); logic `settings_changed` (`config.rs:72`); IPC `ReloadConfig` (`requests.rs:64-67`)
- **Hazards:** the new snapshot is stored first, so a failed output entry (only a warning: `config.rs:95-97`, `:106-108`) or a keymap that won't compile is not retried until the file changes again. An unhealthy logic never hears `settings_changed` and depends on its `init` to read the settings.
- **Reify:** Low — a local diff-and-apply.

<a id="cpb-02"></a>

### CPB-02 — xdg_popup grab (menu chain) lifecycle
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/focus.rs:164` (`PopupGrabbed.grab`, `.root`, `.dismissed`); also `desktop/aizome/crates/aizome-comp-core/src/state.rs:323` (`Aizome.popup_grab`), `desktop/aizome/crates/aizome-comp-core/src/shell.rs:442` (`XdgShellHandler::grab`)
- **Encoding:** `Option<PopupGrabbed>` plus `dismissed: bool`, plus smithay's `PopupGrab` (active and dismissed popup lists behind `has_ended()` / `current_grab()`), plus the seat's `PopupKeyboardGrab` and `PopupPointerGrab`, which are identified only by the grab's two serials (`owns_serial`), plus `layer_focus`, which is cleared when the root was a clicked layer surface.
- **States:** `None` → `Live` (not dismissed, topmost popup ≠ root) → `ClosedBySmithay` (not dismissed, no popup left, not ended) → `Dismissed` (popup_done sent, waiting for the client to destroy the popups) → `Ended` (taken). Keyboard sub-state: held by the menu grab, or left with an input method (`over_input_method`). Representable but invalid: `Dismissed` while the seat grab is still owned; `popup_grab` pointing at an older chain while a newer chain holds the seat grabs.
- **Transitions:** refusals (`shell.rs:447`, `:456`, `:470`, `:494`, `:499`); a new top-level chain dismisses the old one (`shell.rs:463-465`); installed: focus, keyboard grab, pointer grab, `Some(PopupGrabbed{dismissed:false})` (`shell.rs:503-511`); keys follow the topmost popup and the keyboard is taken back after an IME lets go (`focus.rs:227-247`); keyboard target elsewhere → dismiss (`focus.rs:203-205`); `dismiss_popup_grab` Live→Dismissed (`focus.rs:257-264`); smithay-closed→Dismissed (`focus.rs:281-284`); Dismissed→Ended: take, unset the keyboard and pointer grabs, clear `layer_focus` (`focus.rs:289-310`); press on another client (`focus.rs:323-327`); polled every iteration (`state.rs:574`)
- **Hazards:** the end is only noticed by polling once per loop iteration. A client that never destroys its dismissed popups keeps the grab in `Dismissed`, and smithay's pointer grab keeps hover on that client until a forwarded press elsewhere (`press_outside_menus` ignores non-live grabs, `focus.rs:318`). Seat-grab ownership is inferred from two serials (`focus.rs:186-188`). Replacing `popup_grab` at `shell.rs:511` drops the old record without checking its grabs.
- **Reify:** High — five phases spread across smithay state, two seat grabs and about six call sites; failures leave the keyboard or pointer stuck.
- **Related:** aizome-comp-logic `desktop/aizome/crates/aizome-comp-logic/src/input.rs:130` and `desktop/aizome/crates/aizome-comp-logic/src/actions.rs:116` (callers); recovery path `desktop/aizome/crates/aizome-comp-core/src/fallback.rs:226`

<a id="cpb-03"></a>

### CPB-03 — Keyboard focus arbitration (who should hold the keys)
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/focus.rs:388` (`Aizome::keyboard_target`); `desktop/aizome/crates/aizome-comp-core/src/state.rs:321` (`Aizome.layer_focus`); also `focus.rs:351` (`exclusive_keyboard_target`), `focus.rs:195` (`update_keyboard_focus`)
- **Encoding:** a priority chain computed on demand from persistent inputs: `recovery.is_open()`, `lock` (and per-output `lock_surface`), exclusive Overlay/Top layer surfaces, a live `popup_grab`, `view.grabs_keyboard`, `layer_focus` (on-demand layer clicked), and `view.focused` + `view.shown`; the result is applied to smithay's keyboard focus.
- **States:** `Recovery` (no client) | `Locked(lock surface or none)` | `ExclusiveLayer` | `MenuRoot` | `LayerOnDemand` | `OverlayKeys` (no client) | `Window(id)` | `Nothing`
- **Transitions:** recomputed at `update_keyboard_focus` (`focus.rs:195-219`), called from layer commits (`shell.rs:312`), layer destruction (`shell.rs:724`), lock/unlock/lock surface (`shell.rs:739`, `:750`, `:764`), popup grab end (`focus.rs:283`, `:310`), logic swaps (`logic.rs:470`), pointer motion while locked (`input.rs:301-303`), output changes while locked (`state.rs:584-589`); `layer_focus` cleared at `focus.rs:295`, `shell.rs:711-713`
- **Hazards:** nothing re-evaluates the chain on its own, so every input change needs a call at the right site (the logic calls it itself after `focus_window`). While locked, focus follows the pointer between per-output lock surfaces (`focus.rs:357-365`). `layer_focus` still wins while an overlay grabs the keyboard (checked before `overlay_keys`, `focus.rs:399-411`).
- **Reify:** Medium — derived each time, but its inputs are owned by five other machines.
- **Related:** aizome-comp-logic `desktop/aizome/crates/aizome-comp-logic/src/input.rs:157-174` (sets and clears `layer_focus`); `desktop/aizome/crates/aizome-comp-core/src/fallback.rs:98`, `:268`

<a id="cpb-04"></a>

### CPB-04 — Guarded logic pointer grab
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/focus.rs:430` (`Guarded.failed`); `desktop/aizome/crates/aizome-comp-core/src/logic.rs:251` (`LogicHost.pointer_grab`)
- **Encoding:** `failed: bool` inside the installed grab, plus the serial of the logic's grab in `lg.pointer_grab`, plus `az.cursor_override`.
- **States:** `None` → `Held(serial)` → `Failed` (panicked; installed but inert) → `Unset`
- **Transitions:** `set_logic_pointer_grab` records the serial only if the seat took the grab (`focus.rs:340-343`); a panic sets `failed=true` and poisons the logic (`focus.rs:442`, `:448`); motion/button/axis panics unset at once (`focus.rs:455-462`), but frame and gesture panics keep the grab (`focus.rs:464`); `Guarded::unset` clears the serial (`focus.rs:562`); `end_logic_grabs` at a logic stop (`logic.rs:424`, `:456-465`)
- **Hazards:** after a frame or gesture panic the grab stays installed and silently swallows every pointer event until `recover_logic` ends it at the end of the iteration. A grab the seat didn't record (`has_grab` false at `focus.rs:341`) is never ended at a reload, so code from the old library keeps running (safe only because libraries are never unloaded). Logic code running inside a grab doesn't raise `depth`, so `in_call()` is false there.
- **Reify:** Medium — a small machine, but it guards the logic-swap invariants.
- **Related:** aizome-comp-logic `desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:173` (grab makers)

<a id="cpb-05"></a>

### CPB-05 — Client drag-and-drop icon
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/state.rs:317` (`Aizome.dnd_icon`, `.dnd_offset`); also `desktop/aizome/crates/aizome-comp-core/src/handlers.rs:129`
- **Encoding:** `Option<WlSurface>` icon plus an accumulated `Point` offset, alongside smithay's DnD grab.
- **States:** `NoDrag` → `Dragging(icon?, offset)` → `NoDrag`
- **Transitions:** `started` sets the icon and resets the offset (`handlers.rs:130-131`); icon commits add buffer deltas (`shell.rs:334-345`); `dropped` (also reached from the grab's `unset`) clears it (`handlers.rs:134-135`); recovery reset (`recovery/reset.rs:84`)
- **Hazards:** the offset only resets at `started`, and an icon surface destroyed mid-drag stays referenced until the drop.
- **Reify:** Low — two fields with one owner.

<a id="cpb-06"></a>

### CPB-06 — xdg-activation token validity
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/state.rs:337` (`Aizome.valid_tokens`); also `desktop/aizome/crates/aizome-comp-core/src/handlers.rs:186`
- **Encoding:** a `HashSet<String>` of tokens allowed to move focus, beside smithay's own token store (every token is kept: `token_created` returns true), plus the token's creation timestamp.
- **States:** `Created(invalid)` | `Created(valid)` → `Used(valid if <30 s)` → removed
- **Transitions:** creation is marked valid from recent keyboard enter or keyboard/pointer focus (`handlers.rs:191-203`); a request on a mapped window consumes the token from both stores and forwards `Activate{valid}` (`handlers.rs:209-214`); a request on an unmapped surface returns early (`handlers.rs:208`)
- **Hazards:** tokens that are never used (launches whose app never activates) stay in both stores for the whole session. An activation that arrives before the window maps is dropped and its token leaks. Validity depends on keyboard and pointer state at creation time, not at use.
- **Reify:** Medium — two stores have to agree, and the leaks are unbounded.

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

<a id="cpb-09"></a>

### CPB-09 — Input-method commit serial (bypassed)
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/handlers.rs:352` (`Dispatch<ZwpInputMethodV2>`)
- **Encoding:** smithay's count of `done` events against the IME's commit serial is no longer consulted; every `commit` calls `text_input().done(false)`.
- **States:** (replaced) `InSync` | `Stale` → always `Applied`
- **Transitions:** `Commit{serial}` → apply (`handlers.rs:362-365`); other requests go to smithay (`handlers.rs:367-369`)
- **Hazards:** a commit made for a field that has since lost focus is applied to whatever text input is current; this is deliberate, to work around IBus resetting its serial (`handlers.rs:342-351`).
- **Reify:** Low — a single override that is documented.

<a id="cpb-10"></a>

### CPB-10 — Rasterized asset caches (app icons, text labels)
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/icons.rs:38` (`Icons.files`, `.desktop`, `.textures`); `desktop/aizome/crates/aizome-comp-core/src/text.rs:71` (`Labels.cache`, `.widths`, `.last_gc`; `Label.last_used` at `text.rs:68`)
- **Encoding:** negative-caching maps (`Option` values), a lazily built desktop-file index, time-stamped labels with a throttled GC, and a size-capped widths map.
- **States:** for each key `Unknown` → `Found` | `Missing` (cached); labels `Fresh` → `Stale(>30 s)` → evicted
- **Transitions:** `Icons::file` inserts hits and misses (`icons.rs:83`); the index is built on first use (`icons.rs:66-68`); `Icons::clear` (`icons.rs:53-57`, called from `desktop/aizome/crates/aizome-comp-logic/src/config.rs:12`); `Labels::get` touches `last_used` (`text.rs:203`); `gc` runs at most every 10 s (`text.rs:236-242`); widths are wiped at 4096 entries (`text.rs:145-147`); `warm_up` clears the cache (`text.rs:98`)
- **Hazards:** an app installed after its first lookup shows the generic glyph until the settings change triggers `clear`; nothing invalidates the desktop index otherwise.
- **Reify:** Low — caches with an explicit invalidation.

<a id="cpb-11"></a>

### CPB-11 — Key press/release pairing (suppressed and translated keys)
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/state.rs:329` (`Aizome.suppressed_keys`), `state.rs:331` (`.translated_keys`); also `desktop/aizome/crates/aizome-comp-core/src/input.rs:451` (`decide_key`)
- **Encoding:** two `HashSet<Keycode>` that record how each held key's press was decided, so its release is decided the same way; they live in the core (they survive reloads) and are written by both the core and the logic.
- **States:** for each keycode `Up` | `Forwarded` | `Suppressed` | `Translated(mods)`; representable but invalid: a keycode in both sets, or a stale entry for a key that is up
- **Transitions:** swallowed release (`input.rs:470-472`); catch-key press (`input.rs:481`); core binding press (`input.rs:487`); unhealthy logic drops the translation at release (`input.rs:491-493`); logic overlay/binding presses insert into `suppressed` (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:284`, `:322`); logic translation press/release (`aizome-comp-logic/src/input.rs:328`, `:289`); Reset Input clears both (`recovery/reset.rs:140-141`)
- **Hazards:** everything depends on each press seeing its release. A lost release (device unplugged mid-press, VT switch, a key delayed by the logic's slow keys and then dropped) leaves an entry that swallows the next forwarded press's release, so the client sees a stuck, repeating key until Reset Input.
- **Reify:** High — six mutation sites in two crates, with user-visible stuck keys.

<a id="cpb-12"></a>

### CPB-12 — Forwarded-button accounting
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/state.rs:347` (`Aizome.forwarded_buttons`); also `desktop/aizome/crates/aizome-comp-core/src/input.rs:524`
- **Encoding:** a `HashSet<u32>` of buttons the seat saw pressed and not yet released.
- **States:** for each button `Up` → `HeldBySeat` → `Up`
- **Transitions:** `forward_button` inserts or removes (`input.rs:527`, `:529`); the logic routes a release to the seat when its press was forwarded (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:182`); Reset Input drains the set and releases (`recovery/reset.rs:112`)
- **Hazards:** smithay ends implicit grabs and DnD only once no buttons are held, so a release that never reaches `forward_button` (handled by an overlay or the recovery path) leaves the seat grabbed.
- **Reify:** Medium — invariants cross crates.

<a id="cpb-13"></a>

### CPB-13 — IPC connection lifecycle
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/ipc.rs:20` (`Conn.inbuf`, `.outbuf`, `.subscribed`); `ipc.rs:28` (`IpcServer.path`, `.conns`, `.next`)
- **Encoding:** a map of connections by a monotonic id, each with buffers and a subscribed flag; the reader is a separate calloop source; the socket file lives from `listen` until `Drop`.
- **States:** server `Unbound` → `Listening(path)` → `Dropped` (file removed); connection `Accepted` → `Subscribed` → `Closed` (EOF | read error | inbuf > 16 MiB | write error | outbuf > 16 MiB)
- **Transitions:** stale socket removed and bound (`ipc.rs:49-50`, `:70`); accept (`ipc.rs:157-159`, failure `:170`); subscribe (`ipc.rs:207-211`); EOF/error/backlog on read (`ipc.rs:178-180`, `:185-187`, `:193`); write error or backlog on flush (`ipc.rs:129-137`); reader removed once the conn is gone (`ipc.rs:244`); socket file removed on drop (`ipc.rs:143-148`)
- **Hazards:** no writable interest is registered, so a full socket buffer drains only when `flush` next runs (emits, requests, `state.rs:594` per loop iteration) and an idle compositor holds a response back. A connection dropped in `flush` keeps its reader source until it reads EOF. A subscription can't be revoked.
- **Reify:** Medium — a moderate spread; the idle-flush stall is easy to miss.
- **Related:** `desktop/aizome/crates/aizome-shell/src/ipc.rs:151` (shell client), `desktop/aizome/crates/aizome-ipc/src/lib.rs` (client library, aizomectl)

<a id="cpb-14"></a>

### CPB-14 — Typing-keyboard keymap switching
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/keyboards.rs:92` (`Keyboards.devices`, `.met`, `.last`, `.sending`, `.active`)
- **Encoding:** a device map, the identities already announced, the device that typed last, the device whose key is in flight, and the spec the seat keymap was compiled from.
- **States:** `Keymap(spec for last device)`; for each key `Sending(device | none)`; `active` may record a spec that failed to compile
- **Transitions:** `keyboard_added` (clears the old entry, announces once per identity: `keyboards.rs:155-162`); `keyboard_removed` clears `last`/`sending` (`keyboards.rs:168-174`); `key_from` switches `last` and recompiles (`keyboards.rs:181-186`); `apply_keymap` restores locks and layout and records `active` even on failure (`keyboards.rs:200-227`); settings change (`config.rs:47`); synthetic plug/unplug and keys (`requests.rs:222-248`)
- **Hazards:** keys that the logic delays (slow or sticky keys) are decided after another keyboard may have switched the keymap, and `KeyCaught` reports `sending` at decision time (`input.rs:482`). A failed compile isn't retried until the spec changes. `met` grows for the whole session.
- **Reify:** Medium — five fields with an ordering invariant against key flow.
- **Related:** device hot-plug in `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs` (comp-a)

<a id="cpb-15"></a>

### CPB-15 — IPC key-catch request
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/keyboards.rs:106` (`Keyboards.catch`)
- **Encoding:** `Option<(tag, connection id)>` plus the IPC subscription state of that connection.
- **States:** `Idle` → `Waiting(tag, conn)` → `Caught` (event sent, press suppressed) | `Cancelled` | `Abandoned` (conn gone, dropped at the next press)
- **Transitions:** set only for subscribed connections (`ipc.rs:216-223`); cancelled by the same connection (`ipc.rs:224-229`); consumed by the next unlocked press (`input.rs:476-485`); refused through `handle_request` (`requests.rs:34`)
- **Hazards:** a second asker silently replaces the first; while locked the wait lasts indefinitely; there is no timeout.
- **Reify:** Low — one field and three sites.
- **Related:** Settei keyboard setup via `desktop/aizome/crates/aizome-ipc/src/setup.rs`

<a id="cpb-16"></a>

### CPB-16 — Compositor process startup and shutdown sequence
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/lib.rs:108` (`run`)
- **Encoding:** the order of control flow in `run`, `IpcServer` drop, the config watcher channel, and the result of the logic load.
- **States:** `Parse` → `Settings(loaded | defaults)` → `Backend` → `State` → `IPC listening` → `Outputs applied` → `Services started` → `Logic loaded | Recovery path` → `Session started` → `Running` → `Stopping`
- **Transitions:** settings fall back to defaults (`lib.rs:134-137`); backend failure exits (`lib.rs:163-181`); IPC bind failure exits (`lib.rs:191-194`); watcher (`lib.rs:198-207`); outputs, pointer, night light, idle, watchdog, recovery (`lib.rs:209-214`); logic load or notification (`lib.rs:219-227`); session start (`lib.rs:228`); loop (`lib.rs:230-235`); children stopped (`lib.rs:236`)
- **Hazards:** the shell starts after the logic attempt, so it may find the recovery path. An IPC bind failure exits after the backend already took the seat.
- **Reify:** Low — linear and local.

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

<a id="cpb-19"></a>

### CPB-19 — Capture stream damage and scratch-buffer state
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/protocols/capture.rs:65` (`Stream.tracker`, `.scratch`, `.region`, `.scale`, `.cursor`); also `capture.rs:216` (`after_fence`)
- **Encoding:** a per-stream `OutputDamageTracker` (its `last_state` decides the next damage), a type-erased offscreen buffer reused while the size matches, and a one-shot fence watch whose completion runs through `Rc<RefCell<Option<closure>>>`.
- **States:** `Fresh` (no history: full damage) → `Tracked` → copy `Unchanged` | `Copied` → `AwaitingFence` → `Ready`
- **Transitions:** `Stream::new` (`capture.rs:81-84`); damage computed and recorded before rendering (`capture.rs:301`); `Unchanged` when a damage-only copy has none (`capture.rs:303-305`); full render (`capture.rs:342`, `:369`, `:418`); scratch reused or reallocated (`capture.rs:355-364`, `:387`, `:407-413`, `:427`); fence watch or synchronous fallback (`capture.rs:223-240`)
- **Hazards:** damage is committed to the tracker before the copy can fail (dmabuf size mismatch, bind or render error at `capture.rs:335-343`), so the next damage-only copy may say `Unchanged` and wait for a change the client never received. The tracker's scale is fixed at creation. The fence fallback blocks the main loop for up to 1 s (`capture.rs:237`).
- **Reify:** Medium — local, but correctness depends on callers resetting streams.
- **Related:** `desktop/aizome/crates/aizome-comp-core/src/backend/mod.rs` (`screencopy`, comp-a)

<a id="cpb-20"></a>

### CPB-20 — Foreign-toplevel handle synchronisation (wlr management and ext list)
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/protocols/foreign_toplevel.rs:25` (`ForeignToplevelManagerState.managers`, `.handles`, `.sent`, `.ext`)
- **Encoding:** three maps keyed by `WinId` (wlr handles per client, the last-sent `Snapshot`, ext handles) plus the manager list; diffs are sent once per loop iteration.
- **States:** for each window `Announced` → `Synced(snapshot)` → `Dirty` → `Synced` … → `Closed`; managers `Bound` → `Stopped`
- **Transitions:** bind announces all windows (`foreign_toplevel.rs:229-235`); `window_opened` (`foreign_toplevel.rs:124-135`, from `shell.rs:197`); `refresh` diffs and sends (`foreign_toplevel.rs:168-218`, from `state.rs:595`); `window_closed` (`foreign_toplevel.rs:137-148`, from `shell.rs:243`); `Stop` (`foreign_toplevel.rs:253-256`); handle destroyed (`foreign_toplevel.rs:290-294`)
- **Hazards:** a handle created at bind already carries the current state, while `sent` may be older, so the next refresh can repeat `output_enter` on it. A wl_output bound after the announce never gets `output_enter`. Managers are pruned only at refresh (`foreign_toplevel.rs:217`).
- **Reify:** High — three maps must stay in lockstep; suspected bug.
- **Suspected bug:** `Snapshot.parent` is compared in `refresh` (`foreign_toplevel.rs:173`) but no `parent` event is ever sent after the announce (`foreign_toplevel.rs:179-200`), so a dialog whose parent is set or changed after mapping is never re-parented in taskbars (confidence: high)

<a id="cpb-21"></a>

### CPB-21 — Gamma-control ownership per output
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/state.rs:153` (`OutputState.gamma_client`); `desktop/aizome/crates/aizome-comp-core/src/protocols/gamma_control.rs:27` (`ControlData.output`)
- **Encoding:** a per-output `bool` that isn't tied to a particular control, plus an immutable `Option<Output>` in each control's user data.
- **States:** output `NightLight` → `HeldByClient` → `NightLight`; control `Failed` | `Holding` | `Released` (still `Some(output)`)
- **Transitions:** get control: refused if the size is 0 or the output is held (`gamma_control.rs:64-69`), otherwise held (`gamma_control.rs:72-76`); `SetGamma` failure → `failed` + release (`gamma_control.rs:98-100`); destroy → release (`gamma_control.rs:115-117`); release restores Night Light (`gamma_control.rs:123-126`); output disable drops the whole `OutputState` (`outputs.rs:113`)
- **Hazards:** a released control keeps `Some(output)` and can still `SetGamma`. Disabling or unplugging the output forgets the hold without sending `failed`, so after re-enabling, a second client and Night Light both drive the ramps.
- **Reify:** High — an unowned flag with a suspected bug.
- **Suspected bug:** client A's `SetGamma` fails (for example while the VT is switched away), so A's control is failed and released; client B (or A again) takes the output; A then destroys its failed control, `release` clears B's hold and Night Light overwrites B's ramps (confidence: medium)

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

<a id="cpb-23"></a>

### CPB-23 — Screen-share reporting (settle delay and last report)
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/protocols/image_copy_capture.rs:62` (`ImageCopyCaptureState.reported`, `.serial`); `image_copy_capture.rs:90` (`SessionState.shown_at`); `image_copy_capture.rs:571` (`SHARE_SETTLE`)
- **Encoding:** `shown_at: Option<Instant>` per session, a 1 s settle constant, the last reported `Vec<ShareInfo>`, and a one-shot calloop timer.
- **States:** session `Unseen` → `Settling(<1 s)` → `Shared` → `Gone`; report `InSync` | `Changed`
- **Transitions:** first copied frame sets `shown_at` and arms the timer (`image_copy_capture.rs:477-481`, `:627-637`); `report_shares` diffs and emits at every flush and at stop (`:434`, `:438`, `:593-599`, `:621`); stop or destroy removes a share (`:609-612`, `:293`)
- **Hazards:** the shares are derived from elapsed time, so a report depends on a timer or a later flush coming round.
- **Reify:** Medium — time-derived, with a cross-process consumer.
- **Related:** `desktop/aizome/crates/aizome-shell/src/sharing.rs`

<a id="cpb-24"></a>

### CPB-24 — wlr-output-management configuration lifecycle
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/protocols/output_management.rs:27` (`OutputManagementState.instances`, `.serial`); `output_management.rs:56` (`ConfigInner.used`, `.heads`); `output_management.rs:62` (`HeadConfig`)
- **Encoding:** a global u32 serial, bumped at every `outputs_changed`; per-manager heads keyed by output name, with their mode objects fixed at creation; per-configuration `used` flag and its head list.
- **States:** manager `Bound` → `Stopped`; configuration `Building` → `Used` → `Cancelled` (stale serial) | `Succeeded` | `Failed`; head `Announced` → `Updated` → `Finished`
- **Transitions:** bind (`output_management.rs:205-211`); `outputs_changed` bumps the serial, finishes gone heads, updates or creates others, sends `done` (`:169-191`); create configuration (`:230-232`); enable/disable head (`:280-297`); apply/test: used check, serial check, validation, sequential `configure_output` (`:300-346`); `Stop` (`:233-236`)
- **Hazards:** `Test` checks only the scale range and names, not modes. Each `configure_output` fires `outputs_changed` in the middle of an apply. Mode lists never refresh for a head that keeps its name. `EnableHead` after `used` isn't rejected.
- **Reify:** High — the serial, used flag and partial apply interact; suspected bug.
- **Suspected bug:** kanshi applies a profile of two heads and the second fails (for example "has no mode WxH" from `backend/udev.rs`); the first head's change stays applied while the client is told `failed`, so the layout is half-changed against the protocol's rule to revert on failure (`output_management.rs:339-346`) (confidence: high)

<a id="cpb-25"></a>

### CPB-25 — wlr-screencopy frame lifecycle and stream pool
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/protocols/screencopy.rs:46` (`ScreencopyManagerState.pending`, `.streams`, `.dmabuf`); `screencopy.rs:64` (`Pending.since`); `screencopy.rs:79` (`FrameInner.output`, `.manager`, `.used`)
- **Encoding:** pending copies in a Vec; a stream pool keyed by (manager id, output, region, cursor), at most 4 per manager; a frame's `used` flag set to true up front for frames already failed; `since` holds the output's `damaged_frames`; the dmabuf target is asked once and cached.
- **States:** frame `Created` → `Failed` | `Offered(buffer events)` → `Copying(pending, since)` → `Copied` → `AwaitingFence` → `Ready` | `Failed`; stream `New` → `Pooled` → `TakenForCopy` → `Pooled` → `Evicted` | `Dropped`
- **Transitions:** capture request (`screencopy.rs:147-176`); copy or copy-with-damage marks used and queues (`:206-216`); destroy unqueues (`:199-201`); `flush_pending` checks alive, output gone, unchanged-since (`:259-275`); `take_stream` reuses or evicts (`:313-326`); a copy puts the stream back and re-queues or completes (`:282-297`); manager destroyed drops streams (`:179-182`); `ready` (`:330-343`)
- **Hazards:** a stream's key leaves out the output's scale, while its tracker scale is fixed at creation (`capture.rs:82`). Pending copies for a destroyed manager recreate a stream under the dead key, which is never removed (`:277`, `:282`). The cached dmabuf formats (`:240-248`) never follow a renderer change. Damage-only frames wait indefinitely on an idle output (by design).
- **Reify:** High — two coupled machines; suspected bugs.
- **Suspected bug:** (1) wf-recorder records a whole output and the user changes only its scale (same mode); the region is unchanged, so the old stream is reused with its old tracker scale and surfaces are drawn at the wrong size; the ext path resets on a scale change (`image_copy_capture.rs:515-521`), this one doesn't (confidence: medium). (2) A client that destroys its manager right after requesting a frame leaves one stream, with a full-output offscreen buffer, per manager for as long as the output exists (confidence: low).

<a id="cpb-26"></a>

### CPB-26 — Toplevel icon build and freeze
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/protocols/toplevel_icon.rs:35` (`IconState.buffers`, `.immutable`)
- **Encoding:** a Vec of buffers by (size, scale) plus an `immutable` flag behind a Mutex.
- **States:** `Building` → `Set(immutable)`
- **Transitions:** add buffer (replaces same size/scale) (`toplevel_icon.rs:117-118`); `set_icon` checks buffers alive, freezes and forgets them (`toplevel_icon.rs:81-87`); `set_name`/`add_buffer` once frozen are errors (`toplevel_icon.rs:106-111`)
- **Hazards:** none observed (replaces smithay's buggy tracker).
- **Reify:** Low — tiny and explicit enough.

<a id="cpb-27"></a>

### CPB-27 — Layer-surface lifecycle and usable-area tracking
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/shell.rs:663` (`new_layer_surface`); `desktop/aizome/crates/aizome-comp-core/src/state.rs:157` (`OutputState.deskbar_geometry`); also `state.rs:285` (`Aizome.ghosts`), `state.rs:321` (`layer_focus`)
- **Encoding:** smithay's per-output layer map (with `LayerSurfaceData.initial_configure_sent`), plus Aizome's last-known Deskbar rectangle, plus ghosts of unmapped or destroyed surfaces, plus the shell-handover flag (`children.handover`).
- **States:** `Created` → `Mapped in layer map` → `Configured` → `Drawn` → `Unmapping(ghost)` → `Destroyed(ghost)`; zone `Stable` | `Changed`
- **Transitions:** no output → `send_close` (`shell.rs:671-673`); destruction hook keeps a ghost (`shell.rs:680-682`); `map_layer` (`shell.rs:684`); commit: ghost if unmapping (`shell.rs:109`), handover (`shell.rs:285-289`), arrange (`shell.rs:293`), initial configure (`shell.rs:295-297`), Deskbar rect updated (`shell.rs:300-302`), zone change → `outputs_changed` and logic `usable_area_changed` unless the shell restart holds the layout (`shell.rs:303-311`); `layer_destroyed`: ghost, unmap, clear `layer_focus`, zone diff (`shell.rs:702-725`)
- **Hazards:** while a shell restart holds the layout, `usable_area_changed` is suppressed; if the handover never ends, the logic never hears the change. Every layer commit recomputes keyboard focus.
- **Reify:** Medium — smithay owns most of the state, Aizome adds three coupled pieces.
- **Related:** `desktop/aizome/crates/aizome-comp-core/src/session.rs` (handover, comp-a), `desktop/aizome/crates/aizome-comp-core/src/ghosts.rs` (comp-a)

<a id="cpb-28"></a>

### CPB-28 — Decoration-mode negotiation
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/shell.rs:631` (`XdgDecorationHandler`); `desktop/aizome/crates/aizome-comp-core/src/state.rs:114` (`Unmapped.wants_csd`)
- **Encoding:** smithay's pending and committed `decoration_mode`, plus `wants_csd` (written, never read), plus the logic's own `ssd` per window.
- **States:** `Undecided(CSD by default)` → `ServerSide` ⇄ `ClientSide` (pending → acked → committed)
- **Transitions:** `new_decoration` → SSD pending, `wants_csd=false` (`shell.rs:633-639`); `request_mode` (`shell.rs:644-647`); `unset_mode` → SSD (`shell.rs:651-654`); the mode is read at map (`shell.rs:180`); the logic notices committed changes (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1001-1009`)
- **Hazards:** `wants_csd` is dead state, and a remap resets it to false (`shell.rs:217`). `ToplevelRequest::Decorations` is never produced by the core. The logic's configure rewrites `decoration_mode` from the committed mode (`aizome-comp-logic/src/actions.rs:562`), which can overwrite a client's `request_mode` that hasn't been acked yet.
- **Reify:** Low — mostly smithay's; clean up the dead field.

<a id="cpb-29"></a>

### CPB-29 — dmabuf-fence commit blockers
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/shell.rs:77` (pre-commit hook in `new_surface`)
- **Encoding:** a smithay blocker on the surface plus a calloop source that clears it.
- **States:** `Commit pending` → `Blocked(fence)` → `Cleared` → `Applied`
- **Transitions:** blocker generated and source inserted (`shell.rs:85-95`); fence readable → `blocker_cleared` (`shell.rs:88-92`)
- **Hazards:** if `insert_source` fails, no blocker is added and the commit applies before the GPU has finished (`shell.rs:93-95`).
- **Reify:** Low — smithay's pattern, local.

<a id="cpb-30"></a>

### CPB-30 — Toplevel map/unmap lifecycle and window registry
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/state.rs:278` (`Aizome.windows`), `state.rs:279` (`.by_surface`), `state.rs:280` (`.unmapped`), `state.rs:281` (`.next_win`); also `desktop/aizome/crates/aizome-comp-core/src/shell.rs:153`
- **Encoding:** membership in `unmapped` vs `windows`/`by_surface`, smithay's `initial_configure_sent` (reset by smithay on unmap), buffer presence, and fullscreen agreed before map in the pending state.
- **States:** `New` → `InitialConfigured` (pre-map fullscreen or unfullscreen may reconfigure) → `Mapped(WinId)` → `Unmapped(null buffer)` → `Mapped(new WinId)` … → `Destroyed`
- **Transitions:** `new_toplevel` (`shell.rs:403-404`); first commit → logic `first_configure` + configure (`shell.rs:156-161`); first buffer → `map_window`: id, registry, logic or fallback, foreign toplevel (`shell.rs:167-169`, `:183-197`); null buffer → `unmap_window` + back to unmapped (`shell.rs:214-218`); pre-map fullscreen (`shell.rs:542-554`), unfullscreen (`shell.rs:563-571`), maximize (`shell.rs:518-520`); destroyed (`shell.rs:594-598`); dead clients swept (`state.rs:636-640`)
- **Hazards:** a remap gets a new `WinId`, so foreign-toplevel handles close and window capture sessions stop (`Source::Window` gone). Fullscreen is promised before map only if the logic is healthy at that moment (`shell.rs:542`). A toplevel that never draws stays in `unmapped` for good. `by_surface` and `windows` are updated in separate statements (`shell.rs:184-185`, `:236-237`).
- **Reify:** High — the phases are spread over Vec membership, maps and smithay flags, across about ten handlers.
- **Related:** aizome-comp-logic `desktop/aizome/crates/aizome-comp-logic/src/actions.rs:991` (`window_committed`), `desktop/aizome/crates/aizome-comp-core/src/fallback.rs` (comp-a)

<a id="cpb-31"></a>

### CPB-31 — Session lock (partially explicit)
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/state.rs:186` (`enum LockState`); `state.rs:139` (`OutputState.lock_surface`), `state.rs:140` (`.powered`), `state.rs:145` (`.last_frame`); also `desktop/aizome/crates/aizome-comp-core/src/shell.rs:735`
- **Encoding:** partially explicit: `LockState::{Unlocked, Locking(SessionLocker, Instant), Locked}`, while the per-output lock surfaces and frame times that decide confirmation live in `OutputState`.
- **States:** `Unlocked` → `Locking(since)` → `Locked` → `Unlocked`; for each output `NoLockSurface` | `LockSurface`
- **Transitions:** `lock` (`shell.rs:737`); lock surface configured and stored (`shell.rs:757-763`); `check_lock` confirms once every powered output has drawn since `since` (`shell.rs:772-780`, polled at `state.rs:597`); `unlock` clears all lock surfaces (`shell.rs:745-748`)
- **Hazards:** `Locking` has no timeout. While the session is inactive or an output's frames stall, `locked` is never sent. `last_frame` counts rendered frames, not presented ones.
- **Reify:** Medium — the enum exists, but confirmation depends on fields outside it.
- **Related:** `desktop/aizome/crates/aizome-shell/src/lock.rs` (lock client)

<a id="cpb-32"></a>

### CPB-32 — Per-output frame and presentation state
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/state.rs:136` (`OutputState.redraw_queued`, `.powered`, `.frame_seq`, `.damaged_frames`, `.last_frame`, `.presented`, `.refresh`, `.gamma`)
- **Encoding:** flags and counters per output, recreated whenever the output is enabled again.
- **States:** `Idle` → `RedrawQueued` → `Drawing` → `WaitingForVBlank` (backend) → `Idle`; `Powered` ⇄ `Off`
- **Transitions:** created queued (`state.rs:168`); queued (`state.rs:740-760`); drawn when due (`state.rs:610-625`); the backend clears the flag when drawing or when it can't draw (`backend/udev.rs:421-424`, `:450-451`); counters (`backend/mod.rs:299-303`); wake for released commits (`state.rs:627-631`)
- **Hazards:** a redraw requested while the session is inactive or the display is off is dropped, not deferred, and relies on a later full queue. Counters reset when the output is re-enabled, so screencopy's `since` and the lock's `last_frame` comparisons restart.
- **Reify:** Medium — the state is here, the transitions are in the backends.
- **Related:** `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs`, `desktop/aizome/crates/aizome-comp-core/src/pacing.rs` (comp-a)

<a id="cpb-33"></a>

### CPB-33 — Loop-iteration pipeline and dirty-flag coalescing
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/state.rs:572` (`on_loop_iteration`); `state.rs:289` (`Aizome.window_outputs_dirty`), `state.rs:292` (`.outputs_dirty`)
- **Encoding:** a fixed sequence of phases run after every batch of events, plus two dirty flags set anywhere and consumed at fixed points.
- **States:** `Tidy` → `RecoverLogic` → `OutputsChanged?` → `LogicIteration` → `Publish` → `WindowOutputs` → `IPC flush` → `ForeignToplevels` → `Redraw` → `LockCheck` → `Capture flush` → `Client flush`
- **Transitions:** popups and windows tidied (`state.rs:573-576`); logic recovered (`state.rs:578`); `outputs_dirty` taken (`state.rs:579-590`); logic hooks (`state.rs:591-592`); `window_outputs_dirty` consumed (`state.rs:826-830`, set at `shell.rs:196`, `:250`, `:306`, `logic.rs:469`, `state.rs:656`); then `state.rs:594-601`
- **Hazards:** the correctness of the order is implicit (the lock check after redraw, capture after redraw so `damaged_frames` is current, rollback before hooks). `outputs_dirty` is cleared before the logic hears it, so a logic that is poisoned at that moment never does (it relies on `init` re-reading).
- **Reify:** Medium — central and order-sensitive, but in one place.

<a id="cpb-34"></a>

### CPB-34 — Default-terminal index and one-time Hotaru migration marker
- **Where:** `desktop/aizome/crates/aizome-comp-core/src/terminal.rs:27` (`INDEX`); `terminal.rs:30` (`MOVED` marker in `$XDG_STATE_HOME/aizome`)
- **Encoding:** a process-global `Mutex<Option<AppIndex>>`, refreshed on use, plus a marker file on disk that records the migration.
- **States:** index `Unloaded` → `Loaded` (refreshed on change); migration `Pending` → `Done(marker)` | `Failed` (retried next session)
- **Transitions:** marker check (`terminal.rs:72-74`); migration thread (`terminal.rs:75-98`); marker written even for `Unmatched` (`terminal.rs:93-94`); index load or refresh (`terminal.rs:36-45`); ❖Return spawns on a thread (`terminal.rs:53-61`)
- **Hazards:** a ❖Return that takes the index lock before the migration thread starts the pre-migration terminal once. A failed marker write repeats the migration next session.
- **Reify:** Low — one-shot and local.
- **Related:** called from `desktop/aizome/crates/aizome-comp-core/src/session.rs:176` (comp-a)

## Already explicit

- None in this slice.

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/crates/aizome-comp-core/build.rs` | 278 | none |  | build-time ABI fingerprint hashing, consumed by CPB-17 |
| `desktop/aizome/crates/aizome-comp-core/src/cli.rs` | 204 | none |  | option parsing, pure |
| `desktop/aizome/crates/aizome-comp-core/src/config.rs` | 113 | machines | [CPB-01](#cpb-01) | settings diff-and-apply |
| `desktop/aizome/crates/aizome-comp-core/src/cursor.rs` | 152 | none |  | compute-once xcursor image cache with arrow fallback |
| `desktop/aizome/crates/aizome-comp-core/src/focus.rs` | 568 | machines | [CPB-02](#cpb-02), [CPB-03](#cpb-03), [CPB-04](#cpb-04) | popup grab, keyboard focus arbitration, guarded logic grabs |
| `desktop/aizome/crates/aizome-comp-core/src/geom.rs` | 105 | none |  | pure rectangle math |
| `desktop/aizome/crates/aizome-comp-core/src/handlers.rs` | 476 | machines | [CPB-05](#cpb-05), [CPB-06](#cpb-06), [CPB-07](#cpb-07), [CPB-08](#cpb-08), [CPB-09](#cpb-09), [CPB-18](#cpb-18) | DnD icon, activation tokens, idle inhibit, constraints, IME commit; IME popups in CPB-18 |
| `desktop/aizome/crates/aizome-comp-core/src/icons.rs` | 416 | machines | [CPB-10](#cpb-10) | icon caches with negative entries and clear |
| `desktop/aizome/crates/aizome-comp-core/src/input.rs` | 630 | machines | [CPB-11](#cpb-11), [CPB-12](#cpb-12), [CPB-08](#cpb-08), [CPB-15](#cpb-15), [CPB-17](#cpb-17) | key and button pairing; constraint activation, catch consumption, key reload |
| `desktop/aizome/crates/aizome-comp-core/src/ipc.rs` | 246 | machines | [CPB-13](#cpb-13), [CPB-15](#cpb-15) | IPC connection lifecycle; catch-key set and clear |
| `desktop/aizome/crates/aizome-comp-core/src/keyboards.rs` | 282 | machines | [CPB-14](#cpb-14), [CPB-15](#cpb-15) | per-device keymap switching; catch field |
| `desktop/aizome/crates/aizome-comp-core/src/lib.rs` | 239 | machines | [CPB-16](#cpb-16) | process startup and shutdown order |
| `desktop/aizome/crates/aizome-comp-core/src/logic.rs` | 764 | machines | [CPB-17](#cpb-17), [CPB-04](#cpb-04) | logic host lifecycle; grab serial |
| `desktop/aizome/crates/aizome-comp-core/src/pieces.rs` | 135 | none |  | DecorCache damage-identity slots, a cache |
| `desktop/aizome/crates/aizome-comp-core/src/popups.rs` | 87 | machines | [CPB-18](#cpb-18) | popup-layer placement and pruning |
| `desktop/aizome/crates/aizome-comp-core/src/protocols/capture.rs` | 509 | machines | [CPB-19](#cpb-19) | stream damage tracker, scratch buffer, fence wait |
| `desktop/aizome/crates/aizome-comp-core/src/protocols/foreign_toplevel.rs` | 295 | machines | [CPB-20](#cpb-20) | wlr and ext handle sync |
| `desktop/aizome/crates/aizome-comp-core/src/protocols/gamma_control.rs` | 140 | machines | [CPB-21](#cpb-21) | gamma ownership per output |
| `desktop/aizome/crates/aizome-comp-core/src/protocols/image_copy_capture.rs` | 637 | machines | [CPB-22](#cpb-22), [CPB-23](#cpb-23) | sessions and frames; share reporting |
| `desktop/aizome/crates/aizome-comp-core/src/protocols/mod.rs` | 12 | none |  | module declarations |
| `desktop/aizome/crates/aizome-comp-core/src/protocols/output_management.rs` | 391 | machines | [CPB-24](#cpb-24) | heads, serial, configurations |
| `desktop/aizome/crates/aizome-comp-core/src/protocols/screencopy.rs` | 343 | machines | [CPB-25](#cpb-25) | frames, pending copies, stream pool |
| `desktop/aizome/crates/aizome-comp-core/src/protocols/toplevel_icon.rs` | 127 | machines | [CPB-26](#cpb-26) | icon build then immutable |
| `desktop/aizome/crates/aizome-comp-core/src/requests.rs` | 517 | machines | [CPB-17](#cpb-17), [CPB-14](#cpb-14), [CPB-15](#cpb-15), [CPB-22](#cpb-22) | IPC dispatch; reload orchestration, synthetic keyboards, stop share |
| `desktop/aizome/crates/aizome-comp-core/src/screenshot.rs` | 191 | none |  | one-shot capture, encode and save |
| `desktop/aizome/crates/aizome-comp-core/src/shell.rs` | 788 | machines | [CPB-27](#cpb-27), [CPB-28](#cpb-28), [CPB-29](#cpb-29), [CPB-02](#cpb-02), [CPB-18](#cpb-18), [CPB-30](#cpb-30), [CPB-31](#cpb-31) | layer surfaces, decorations, fence blockers; handlers for grab, popups, toplevels, lock |
| `desktop/aizome/crates/aizome-comp-core/src/state.rs` | 876 | machines | [CPB-30](#cpb-30), [CPB-31](#cpb-31), [CPB-32](#cpb-32), [CPB-33](#cpb-33), [CPB-11](#cpb-11), [CPB-12](#cpb-12) | window registry, lock, per-output state, loop pipeline |
| `desktop/aizome/crates/aizome-comp-core/src/svg.rs` | 31 | none |  | pure SVG rasterization |
| `desktop/aizome/crates/aizome-comp-core/src/terminal.rs` | 102 | machines | [CPB-34](#cpb-34) | terminal index and migration marker file |
| `desktop/aizome/crates/aizome-comp-core/src/text.rs` | 305 | machines | [CPB-10](#cpb-10) | label cache with timed GC, in CPB-10 |
| `desktop/aizome/crates/aizome-comp-core/tests/keymap_options.rs` | 64 | none |  | tests (subprocess re-exec, assertions only) |
