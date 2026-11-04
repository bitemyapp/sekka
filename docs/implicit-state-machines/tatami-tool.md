# TTL: Tatami command, display watcher and configuration

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/tatami/tool, desktop/tatami`. Audited at `8e0acbe5`; line numbers refer to that commit.

39 files, 0 pending. 29 implicit machines: 8 High, 9 Medium, 12 Low.

## Machines

<a id="ttl-01"></a>

### TTL-01 — Mako Do Not Disturb mode
- **Where:** `desktop/tatami/config/mako/config:29` (`[mode=do-not-disturb]`; notify-send exception `:32`); driven by `desktop/tatami/tool/src/toggle.rs:45` (`notifications`), read by `desktop/tatami/tool/src/toggle.rs:41` (`silenced`)
- **Encoding:** a mode held only in the running mako process (`makoctl mode`); Waybar `custom/silence` indicator refreshed only by signal 10 (`desktop/tatami/tool/src/toggle.rs:10`)
- **States:** `Normal` ↔ `DoNotDisturb`; representable invalid: indicator shows silenced while a restarted mako is back in `Normal`
- **Transitions:** toggle `makoctl mode -t` (`desktop/tatami/tool/src/toggle.rs:46`), read back (`toggle.rs:47`), notify and signal Waybar (`toggle.rs:49-53`); mako restart by `Restart=on-failure` (`desktop/tatami/module.nix:221`) or `tatami reload` try-restart (`desktop/tatami/tool/src/main.rs:192-196`) → `Normal`, unsignalled
- **Hazards:** not persisted; any mako restart clears it silently (see TTL-17); `interval: once` indicator goes stale after a mako crash
- **Reify:** Low — one bit in an external daemon; persistence, not an enum, would fix it
- **Related:** Waybar `custom/silence` (`desktop/tatami/config/waybar/config.jsonc:77-83`)

<a id="ttl-02"></a>

### TTL-02 — Tatami session unit lifecycle and restart policies
- **Where:** `desktop/tatami/module.nix:206` (`unit`), `desktop/tatami/module.nix:228` (`sessionServices`), `desktop/tatami/module.nix:382` (`units.json`)
- **Encoding:** systemd user units wanted by `wayland-session@tatami.target`, `partOf` `graphical-session.target`, ordered after `wayland-wm@tatami.service` and `wayland-session-waitenv.service`; per-unit `Restart`, `StartLimitIntervalSec`, `ExecStartPre`, `Type=oneshot`; `units.json` as reload's list of own units
- **States:** per unit `Inactive` → `Starting` (ExecStartPre, e.g. `background ensure`) → `Running` → `Failed` → restart after 1 s | `Stopped` with the graphical session; `tatami-restore` `Ran` once; `tatami-hyprsunset` `OnDemand`
- **Transitions:** start with the session target (`desktop/tatami/module.nix:208`); `Restart=on-failure`, `RestartSec=1` (`module.nix:221-222`); display recovery never exhausts its start limit, 5 s stop timeout (`module.nix:235-236`); swaybg `ExecStartPre` (`module.nix:280`); restore oneshot, `Restart=no` (`module.nix:314-321`); hyprsunset not wanted at login (`module.nix:323-325`); power-key inhibitor held by `sleep infinity` (`module.nix:261-271`); reload try-restart of running own units (TTL-17)
- **Hazards:** `tatami-restore` runs once with no retry and no check that Hyprland answers, so its evals can fail silently; try-restart of `tatami-power-key` briefly returns the power key to logind; in-memory state of restarted daemons (mako mode, hyprsunset tint) is lost on any restart
- **Reify:** Low — declarative systemd ordering; recorded for its restart side effects
- **Related:** uwsm `wayland-wm@tatami.service` started from the session entry (`desktop/tatami/module.nix:54-66`)

<a id="ttl-03"></a>

### TTL-03 — Keep Awake: intent marker, inhibitor holder unit and verification
- **Where:** `desktop/tatami/tool/src/awake.rs:13` (`requested`: `$XDG_STATE_HOME/tatami/keep-awake`), `:57` (`edit_lock`), `:129` (`hold`), `:146` (`verify`); unit `desktop/tatami/keep-awake.nix:21`
- **Encoding:** persisted marker file (intent), `keep-awake.lock` flock, state of `tatami-keep-awake.service`, logind `ListInhibitors` row matched by who/mode/uid/pid (`awake.rs:35-50`), `MAINPID`; receipt fingerprint `keep_awake` (TTL-24)
- **States:** `Off` → `Starting` (marker set, start job, `verify` polling) → `Protected` (block inhibitor owned by MAINPID) → `Lost` (holder exits) → `Starting`…; `Stopping` → `Off`; representable invalid: marker without verified inhibitor (indicator `warning`), inhibitor without marker after a failed stop (indicator `active`, `awake.rs:209-211`)
- **Transitions:** `on|off|toggle` under flock (`awake.rs:185-198`); `set` writes/removes marker first (`awake.rs:89`, `:72-86`), `systemctl start|stop` (`awake.rs:90-93`), checks logind (`awake.rs:99`), records receipt on off (`awake.rs:103`), restarts hypridle (`awake.rs:107`); gate `ExecCondition=awake requested` (`desktop/tatami/keep-awake.nix:22`); `systemd-inhibit … awake hold` (`keep-awake.nix:23-32`); `hold` checks parent then every 1 s, Err → exit (`awake.rs:132-140`); `ExecStartPost=awake verify` polls 100 ms until owned or process gone (`awake.rs:154-164`), bounded by `TimeoutStartSec=15` (`keep-awake.nix:37`); `Restart=always`, `RestartSec=1`, `StartLimitIntervalSec=0` (`keep-awake.nix:38-39`, `:15`); `restartIfChanged=false` (`keep-awake.nix:18`); excluded from reload (`desktop/tatami/tool/src/main.rs:231`)
- **Hazards:** marker is written before the unit is verified, so a failed `on` keeps the intent and restarts the unit every second without backoff; `set(false)` verifies with `protected(None)`, which matches any `Tatami Keep Awake` row of the uid (another session's holder makes `off` fail); `hold` cannot tell a busctl error from a lost inhibitor
- **Reify:** High — intent file, unit, logind and receipt are four sources of truth with sleep consequences
- **Suspected bug:** one slow logind reply (`busctl --timeout=3`, `awake.rs:21-22`) makes `protected(...)?` return Err in `hold` (`awake.rs:138`); the holder exits, `systemd-inhibit` releases the block, and for `RestartSec=1` plus re-verification nothing blocks lid-close or idle sleep (confidence: low)
- **Related:** logind `org.freedesktop.login1.Manager.ListInhibitors`; Waybar `custom/stay-awake` (`desktop/tatami/config/waybar/config.jsonc:84-90`); Yukimi `apps/yukimi/crates/yukimi-system/src/completion.rs:543`

<a id="ttl-04"></a>

### TTL-04 — Wallpaper choice and per-output overrides
- **Where:** `desktop/tatami/tool/src/background.rs:160` (`chosen`: symlink `$XDG_STATE_HOME/tatami/background`), `:30` (`background.json`), `:178` (`ensure`)
- **Encoding:** chosen symlink (global image), JSON settings (mode, color, per-output overrides with images), `background.lock` flock, shipped default `/etc/xdg/tatami/background`, swaybg unit restarted to apply
- **States:** global `Default` (missing or dangling symlink) | `Chosen(image)`; per output `Inherit` | `Override`; per-output image `Missing` → drawn with the global image but kept; settings `Malformed` (only a full reset recovers)
- **Transitions:** `change` under flock (`background.rs:208-214`); global `set` relinks and restarts (`background.rs:236-239`); other keys save atomically and restart (`background.rs:256-257`, `:91-97`); full reset ignores a malformed file (`background.rs:217-221`); `ensure` relinks the default when the target is gone (`background.rs:179-181`), from `ExecStartPre` (`desktop/tatami/module.nix:280`) and `run` (`background.rs:287`); missing per-output image falls back non-destructively (`background.rs:124-126`); `restart --no-block` (`background.rs:146-153`)
- **Hazards:** two fallback policies: a missing per-output image is temporary, a missing global image is permanent; `--no-block` restart reports nothing after the decode check
- **Reify:** High — persisted choice with a destructive fallback (data loss)
- **Suspected bug:** a global wallpaper on a removable or late-mounted drive (udiskie automounts concurrently, `desktop/tatami/module.nix:311`) is a dangling symlink when `tatami-swaybg`'s `ExecStartPre=background ensure` runs at login; `ensure` replaces it with the default for good, whereas the same image as a per-output choice would survive (confidence: medium)

<a id="ttl-05"></a>

### TTL-05 — Style › Background settings menu flow
- **Where:** `desktop/tatami/tool/src/background.rs:355` (`configure`)
- **Encoding:** local `output` from the first panel and an unbounded `loop` that reloads settings every round
- **States:** `ChoosingDisplay` → `ChoosingAction(output)` → `ChoosingMode` | `PromptingColor` | `ChoosingImage` (→ `PromptingPath`) → apply → `ChoosingAction`; any cancel → exit
- **Transitions:** display panel (`background.rs:389-392`); action panel (`background.rs:393-412`); sub-pickers (`background.rs:413-426`); `change` then loop (`background.rs:427-431`); unreadable settings exit (`background.rs:394-396`)
- **Hazards:** none observed
- **Reify:** Low — linear Walker dialogue

<a id="ttl-06"></a>

### TTL-06 — Screenshot selection and screen freeze
- **Where:** `desktop/tatami/tool/src/capture.rs:99` (`screenshot`), `:212` (`Freeze.child`, `.dir`), `:311` (`Grab`)
- **Encoding:** presence of any `slurp` process as "selecting"; `Freeze` with optional hyprpicker child and private runtime dir `tatami-freeze-PID`; `Grab` temporary PNG; leaked-frame sweep by name and PID liveness
- **States:** `Idle` → `Frozen+Selecting` → `Capturing` (grim) → `Announced` | `Cancelled`; `Freeze`: `Started(child?)` → `Terminating` (SIGTERM ≤500 ms) → `Killed` → dir removed; leftovers `Leaked` until swept
- **Transitions:** second Print terminates slurp (`capture.rs:101-105`); freeze start sweeps, mkdir 0700, spawns, sleeps 100 ms (`capture.rs:218-247`); selection (`capture.rs:126-140`); capture then `drop(freeze)` (`capture.rs:141-142`); Drop SIGTERM, wait, SIGKILL, remove dir (`capture.rs:250-270`); sweep only while no hyprpicker runs (`capture.rs:275-289`); OCR/QR via `frozen_region` and `Grab` (`capture.rs:303-326`, `:330-393`); color picker toggles any hyprpicker (`capture.rs:645-652`)
- **Hazards:** the 100 ms sleep is not a readiness check; state inferred from process names, so Print cancels a screenrecord/OCR slurp and Super+Print during a selection kills the freeze instead of opening the picker; no PDEATHSIG, so a killed helper leaves hyprpicker overlaying the screen; `tatami-grab-PID.png` (possibly a QR secret) is never swept (`capture.rs:293-300`)
- **Reify:** Medium — process-presence state and a Drop lifecycle shared by three commands

<a id="ttl-07"></a>

### TTL-07 — Screenshot Edit notification action
- **Where:** `desktop/tatami/tool/src/capture.rs:172` (`notify_edit`), spawned at `capture.rs:167`; `desktop/tatami/config/mako/config:35-39`
- **Encoding:** detached `tatami screenshot-notify PATH` process blocked in `notify-send -A default=edit`; mako `history=0` for the card
- **States:** `Waiting` (card shown, owner alive, ≤10 s) → `Editing` (satty) | `Expired/Closed` (owner exits)
- **Transitions:** spawn owner (`capture.rs:164-168`); notify-send returns the action (`capture.rs:173-186`); `default` runs satty (`capture.rs:187-202`); mako keeps the card out of history so restore cannot show a dead action (`desktop/tatami/config/mako/config:35-39`)
- **Hazards:** `makoctl invoke` (Super+Alt+,) acts on the newest card, which may not be this one
- **Reify:** Low — one-shot owner, covered by a VM check

<a id="ttl-08"></a>

### TTL-08 — Screen recording start, fallback and stop
- **Where:** `desktop/tatami/tool/src/capture.rs:534` (`screenrecord`), `:398` (`RECORDERS`), `:401` (`recording`), `:510` (`started`), `:614` (`stop_recording`)
- **Encoding:** no state of its own: presence of `gpu-screen-reco`/`wf-recorder` processes, detached recorders, the mix helper's loop (TTL-22), Waybar signal 8 with `interval: once`
- **States:** `Idle` → `Menu` → `Selecting` → `StartingGsr` (≤3 s probe) → `RecordingGsr` | `StartingFallback` → `RecordingWf` | `RecordingMix` | `FailedToStart`; any recording → `Stopping` (SIGINT ≤5 s) → `Saved` | `Forced` (SIGTERM)
- **Transitions:** any recorder running → stop (`capture.rs:535-539`); `--stop` when idle (`capture.rs:540-542`); `--menu` (`capture.rs:544-547`); spawn gsr (`capture.rs:579-582`); probe `started(15)` (`capture.rs:584`, `:510-518`); mix fallback (`capture.rs:585-592`); wf fallback and `started(5)` (`capture.rs:593-605`); signal and failure notice (`capture.rs:607-610`); SIGINT, wait, terminate (`capture.rs:615-634`)
- **Hazards:** `started` treats any moment without a recorder as a start failure; any recorder by name counts, including other programs'; nobody waits on detached gsr/wf-recorder, so a crash leaves the indicator showing recording
- **Reify:** High — process-name state with a user-visible race
- **Suspected bug:** stopping within ~3 s of starting (Alt+Print, the bar indicator or the menu) SIGINTs gsr while the starting invocation is still in `started(15)`; it sees no recorder, takes that for "gsr cannot run" and starts the wf-recorder or mixed fallback on the same file (`capture.rs:584-605`), so recording resumes after the user stopped it (or contends with the just-saved file) (confidence: medium)
- **Related:** Waybar `custom/screenrecording` (`desktop/tatami/config/waybar/config.jsonc:63-69`)

<a id="ttl-09"></a>

### TTL-09 — Display session record: closed-display mode, lid edges, remembered off choices
- **Where:** `desktop/tatami/tool/src/display_policy.rs:95` (`Session.displays`, `.lid_off`, `.lid_closed`, `.off`), file `display_policy.rs:110` (`$XDG_RUNTIME_DIR/tatami/displays-session.json`)
- **Encoding:** four fields of one persisted struct; the next state comes from pure `plan` (`display_policy.rs:229`) and is committed by `reconcile` (TTL-10); duplicated by the connector list `monitors-off` (TTL-26)
- **States:** lid `Unknown(None)` | `Open` | `Closed`; `Docked` | `Undocked`; `ClosedDisplay` (`lid_off` set, lid closed, external connected); `UndockedClosedLid` (panel blanked once); per arrangement `ChosenOff(set)`; `Fallback` (a screen forced on); representable invalid: `lid_off` naming a panel the user also chose off, `lid_closed=None` while closed (read as a close edge), record missing while monitors-off exists
- **Transitions:** cleared at session start (`desktop/tatami/tool/src/window.rs:375`); seeded at login behind a closed lid (`desktop/tatami/tool/src/display_recovery.rs:430-437`); rebuilt from monitors-off when absent or unreadable (`display_policy.rs:123-143`); `choose` (`display_policy.rs:166-182`) from Displays apply (`desktop/tatami/tool/src/displays.rs:495-496`) and Super+Ctrl+Delete (`window.rs:440-441`, `:462-463`); `plan`: arrangement change re-applies choices (`display_policy.rs:257-264`), closed-display adds `lid_off` (`:271-280`), open/undock drains it (`:284-288`), lid edges against `lid_closed` (`:290-291`), fallback (`:300-317`), blank/wake (`:325-346`), unmirror (`:360-371`), next record (`:380-385`)
- **Hazards:** an unreadable file falls to the legacy rebuild, losing `lid_off`, `lid_closed` and other arrangements' choices; `lid_off` records only panels that are on when closed-display mode is decided, so it depends on the commit protocol (TTL-10); two records of "off" (this and monitors-off) are written at different sites
- **Reify:** High — four fields, five mutation sites in four files, implicit lid-edge detection
- **Related:** Displays window `desktop/tatami/displays/src/system.rs:82-84` (runs `tatami displays apply`)

<a id="ttl-10"></a>

### TTL-10 — Display reconciliation commit (plan execution)
- **Where:** `desktop/tatami/tool/src/display_recovery.rs:386` (`reconcile`), `:240` (`carry_out`), `:21` (`lock`)
- **Encoding:** `displays.lock` flock with 5 s bounded wait; locals `errors`, `turned_on`, `turned_off`, `on`; polling waits per step; record saved only on full success
- **States:** `Locked` → `Enabling` (kept rule → wait 1 s → preferred → wait 1 s) → `LastResort` (nothing lit) → `Unmirroring` (wait ≤2 s) → `Disabling` (only with another screen on, wait ≤2 s) → `Dpms` → `PersistMonitorsOff` → `Committed` | `PartiallyApplied` (Err, record unsaved, retried next tick)
- **Transitions:** empty plan saves the record only (`display_recovery.rs:392-397`); `turn_on` ladder (`:185-213`), fallback keeps recovered settings (`:262-264`, `:359-375`); last resort per candidate (`:217-237`, `:275-279`); unmirror (`:283-302`); disable guarded by `remaining` (`:304-327`); DPMS (`:328-338`); `window::switch_off` (`:339-343`); error aggregation (`:350-354`); commit (`:399-403`)
- **Hazards:** steps succeed independently but the commit is all-or-nothing; waits time out though Hyprland may finish later; the lock is held across waits, so Displays and shortcuts fail with "Another display change is still in progress" (`:40-42`)
- **Reify:** High — staged transaction with partial-failure semantics
- **Suspected bug:** closing the lid while docked, if a step errs after the panel was disabled (e.g. `wait_disabled`'s 2 s, `:153-165`, expires on a slow modeset that then completes, or an unmirror wait fails), returns Err before `display_policy::save` (`:399-400`); next tick the panel is already off, so `plan` does not add it to `lid_off` (`desktop/tatami/tool/src/display_policy.rs:274-279`) and saves a record without it; opening the lid then never turns the built-in display back on (confidence: medium)
- **Related:** Hyprland command socket via `desktop/tatami/tool/src/hypr.rs:18` (`request`)

<a id="ttl-11"></a>

### TTL-11 — Display watcher event loop
- **Where:** `desktop/tatami/tool/src/display_recovery.rs:673` (`watch`: `changed`, `last_check`, `last_error`, `screens`, `rescue_until`, `network_fit_until`, `lid`, `scale`)
- **Encoding:** locals of one loop over Hyprland's `.socket2.sock` with a 500 ms read timeout; `changed` coalesces events; 1 s periodic tick; error de-duplication by last message
- **States:** `Connecting` → `Reconciling` (event or ≥1 s) → `Reading` → … → `Exited` (EOF or socket error) → systemd restart
- **Transitions:** connect (`:677-681`); tick condition (`:695`); lid poll, reconcile, error log (`:696-703`); layout, scale, rescue, fit (`:707-726`); `last_check` (`:727`); Network open (`:735-737`), lid (`:738-740`), monitor/config events (`:742-751`); timeouts (`:754-761`); exit (`:733`, `:762`); restarted forever (`desktop/tatami/module.nix:229-239`)
- **Hazards:** reconcile blocks the loop for seconds (lock and step waits), so events collapse into one `changed`; a restart loses `screens`, so a layout change in the gap gets no rescue window
- **Reify:** Medium — small loop multiplexing timers and the sub-machines TTL-12 to TTL-14
- **Related:** Hyprland event socket; lid events from `desktop/tatami/config/hypr/hyprland.lua:632-634`

<a id="ttl-12"></a>

### TTL-12 — Lid state tracker (bindings plus logind poll)
- **Where:** `desktop/tatami/tool/src/display_recovery.rs:546` (`Lid.closed`, `.polled`, `.last_poll`); probe `desktop/tatami/tool/src/lid.rs:9` (`closed`)
- **Encoding:** believed lid `closed`, last logind/ACPI answer `polled`, 2 s poll throttle `last_poll`; socket events overwrite `closed`
- **States:** `Open` | `Closed`, each with `polled` ∈ {`None`, `Some(open)`, `Some(closed)`}; representable: `closed` disagreeing with `polled` (bindings-only hardware, logind lag)
- **Transitions:** init from logind/ACPI (`display_recovery.rs:553-560`); socket event (`:738-740`); poll flips `closed` only when logind's answer changes (`:563-580`), forced after events (`:696`)
- **Hazards:** a quick close/open where the "close" poll still saw open and the "open" poll sees logind's late "closed" sets `closed=true` with the lid open for up to 2 s (panel off and on again); `desktop/tatami/tool/src/displays.rs:437`, `desktop/tatami/tool/src/window.rs:427`, `display_recovery.rs:408` and `:415` ask logind/ACPI directly and can disagree with the watcher on bindings-only hardware
- **Reify:** Medium — two sources merged by an edge rule

<a id="ttl-13"></a>

### TTL-13 — XWayland Xft.dpi retry
- **Where:** `desktop/tatami/tool/src/display_recovery.rs:586` (`XScale.display`, `.set`, `.last_error`, `.next_try`); `desktop/tatami/tool/src/xwayland.rs:44` (`display`), `:64` (`set_dpi`)
- **Encoding:** cached X display, last applied dpi, last error (log de-duplication), retry deadline
- **States:** `NoDisplay` → `Unset` → `Set(dpi)`; `Failed` (retry after 5 s); `Set(dpi)` → `Unset` when the wanted dpi changes
- **Transitions:** `keep` each tick (`display_recovery.rs:715`); skip when equal or before `next_try` (`:607-609`); resolve display only while None (`:610-612`); xrdb merge (`:613-616`); record result and backoff (`:617-625`)
- **Hazards:** `display` is cached on first discovery and never re-resolved after errors (a stale `DISPLAY` retries forever); an XWayland restart keeps `set`, so the value returns only when the scale or main display changes (documented)
- **Reify:** Medium — four fields for one retry/backoff machine

<a id="ttl-14"></a>

### TTL-14 — Window rescue and settings-window fit grace periods
- **Where:** `desktop/tatami/tool/src/display_recovery.rs:687` (`screens`), `:688` (`rescue_until`), `:692` (`network_fit_until`)
- **Encoding:** last lit-screen layout string and two `Instant` deadlines
- **States:** `Settled` ↔ `Rescuing` (≤3 s after a layout change); `FittingNetwork` (≤3 s after start or a Network open) ↔ `Idle`
- **Transitions:** first layout recorded without rescue (`:709-714`); change opens the window (`:711`); rescue per tick (`:717-721`, `:488-522`); Network open extends fit (`:735-737`); fit per tick (`:722-726`, `:647-671`)
- **Hazards:** rescue and fit run only on ticks, so a 3 s window gives a few passes; `screens` resets when the watcher restarts
- **Reify:** Low — two deadlines in one loop

<a id="ttl-15"></a>

### TTL-15 — Displays apply transaction
- **Where:** `desktop/tatami/tool/src/displays.rs:417` (`apply_command`); `desktop/tatami/tool/src/display_recovery.rs:80` (`verify`)
- **Encoding:** display lock guard, pre-change monitor snapshot `all`, session record, store files, monitors-off; order of writes before evaluation
- **States:** `Parsed` → `Locked` → `Refused` (closed lid) | `Persisted` (store, monitors-off, session choice) → `Evaluated` → `Verified` | `Unverified` (Err after 2 s)
- **Transitions:** lock (`displays.rs:424`); refusal behind a closed lid (`displays.rs:436-452`); `save_store` (`:466-468`); `window::switch_off` (`:481-483`); `choose` and save (`:484-499`); evaluate (`:500-502`); verify every 100 ms up to 2 s (`:503`, `display_recovery.rs:84-118`)
- **Hazards:** everything is persisted before Hyprland accepts the change and nothing is rolled back on failure (a later reload applies the unverified state); rollback depends on the caller's revert countdown
- **Reify:** Medium — ordered steps without rollback, mitigated by the GUI
- **Related:** Displays window `desktop/tatami/displays/src/system.rs:82-84` and its revert dialog `desktop/tatami/displays/src/window.rs:389-401`

<a id="ttl-16"></a>

### TTL-16 — Display settings store and legacy scale migration
- **Where:** `desktop/tatami/tool/src/displays.rs:262` (`load_store`), `:313` (`save_store`), `:568` (`migrate`)
- **Encoding:** `displays.json` and generated `displays.lua`, each renamed atomically but written in sequence; migration gated on `displays.json` absent and `toggles/monitor-scale` present
- **States:** `Legacy` → `Migrated`; store `Consistent` | `Diverged` (json written, lua failed)
- **Transitions:** migrate from restore and reload (`desktop/tatami/tool/src/window.rs:123`, `:355`) → save (`displays.rs:598`); saves from apply (`displays.rs:466`), Super+/ (`:561-562`), recovery (`desktop/tatami/tool/src/display_recovery.rs:372`)
- **Hazards:** migration is one-shot for the monitors connected at that login; a failed lua write after the json write leaves stale rules until the next save
- **Reify:** Low — persisted settings with a one-time migration

<a id="ttl-17"></a>

### TTL-17 — Live reload pipeline (`tatami reload`, `update-session`)
- **Where:** `desktop/tatami/tool/src/main.rs:165` (`reload_parts`), `desktop/tatami/tool/src/main.rs:100` (`session` config choice), `desktop/tatami/tool/src/window.rs:348` (`reload_hyprland`, `Reload`), `desktop/tatami/tool/src/hypr.rs:105` (`config_pinned`)
- **Encoding:** Hyprland's command-line config path (chosen once by `session`), `Reload` result, own-unit list `/etc/xdg/tatami/units.json` (`desktop/tatami/module.nix:382-384`), `systemctl list-units` output, failure list
- **States:** session config `User` | `SessionFile` (reloadable) | `Pinned` (store path); reload `DaemonReloaded` → `HyprlandReloaded` | `Pinned` | `Failed` → `UnitsRestarted` → `WaitingForNotifications` (≤2 s) → `Done` | `PartialFailure`
- **Transitions:** config choice (`desktop/tatami/tool/src/main.rs:107-120`); daemon-reload (`main.rs:167`); Hyprland (`main.rs:170-181`): lock (`window.rs:349`), migrate (`window.rs:355`), pinned check (`window.rs:356-358`), snapshot monitors then `hyprctl reload` (`window.rs:362`), restore (`window.rs:365`); try-restart running own units except Keep Awake (`main.rs:184-196`, `:227-233`); wait for a Notifications owner (`main.rs:205-210`); outcome into the receipt (TTL-24)
- **Hazards:** anything unknown counts as pinned (`hypr.rs:115-117`), so a failed `hyprctl instances` reports "applies from the next login" and keeps the old fingerprint; try-restart restarts daemons whose state is only in memory
- **Reify:** High — procedure across four files whose outcome is persisted
- **Suspected bug:** `tatami reload` try-restarts every running own unit except Keep Awake (`main.rs:231`), including an active `tatami-hyprsunset` and `tatami-mako`; hyprsunset returns at its identity profile (`desktop/tatami/config/hypr/hyprsunset.conf:2-5`) and mako without `do-not-disturb`, so an appearance reload or a Yukimi `update-session` silently turns off Night Light and Do Not Disturb (confidence: medium)
- **Related:** Yukimi `apps/yukimi/crates/yukimi-system/src/completion.rs:686` runs `tatami update-session`

<a id="ttl-18"></a>

### TTL-18 — Walker menu open/close handoff
- **Where:** `desktop/tatami/tool/src/menu.rs:113` (`after_menu`), `:107` (`walker_open`)
- **Encoding:** the Walker service's own open state (a request while open closes it) and sticky size, probed through Hyprland layers
- **States:** `Closed` ↔ `Open`; `Closing` (menu entry running while the layer is still mapped)
- **Transitions:** open (`menu.rs:89-91`); wait ≤1 s for the layer to go (`menu.rs:114-119`) before another view or a capture (`menu.rs:141-143`); every view restates its size (`menu.rs:77-86`)
- **Hazards:** after 1 s it proceeds anyway and Walker takes the request as "close"; size is hidden Walker state
- **Reify:** Low — external toggle, polled

<a id="ttl-19"></a>

### TTL-19 — Wi-Fi join flow
- **Where:** `desktop/tatami/tool/src/network.rs:621` (`join`), `:560` (`join_with_password`), `:498` (`Secret`); classification `:28` (`Join`)
- **Encoding:** `Join` enum, saved-connection UUID, `created` flag for rollback, `Secret` temporary file removed on Drop
- **States:** `Active` | `SavedUp` → `Connected` | `PasswordRetry`; new secured: `Prompt` → `SecretWritten` → `ConnectionAdded` → `Up` → `Connected` | `RolledBack`; enterprise/WEP → `HandedOff`
- **Transitions:** already active (`network.rs:622-624`); saved lookup (`:625`); hand-off (`:628-630`); password path (`:631-633`); saved/open up (`:636-649`); password retry after a saved failure (`:653-655`); add with own UUID (`:572-597`); up with passwd-file (`:601-610`); delete created on failure (`:612-616`); Secret Drop (`:525-529`)
- **Hazards:** a killed helper leaves `tatami-wifi-PID` with the PSK (0600) in the runtime dir and/or a key-less created connection; `saved` takes the first profile with that SSID; scan results arrive only for the next opening (`:711`)
- **Reify:** Low — linear flow with one retry branch
- **Related:** Network window `tatami-network --join` (`desktop/tatami/network/`)

<a id="ttl-20"></a>

### TTL-20 — Lock, idle and DPMS
- **Where:** `desktop/tatami/tool/src/power.rs:6` (`locked`), `:12` (`lock`), `:30` (`lock_wait`); `desktop/tatami/config/hypr/hypridle.conf:5-17`; `desktop/tatami/config/hypr/hyprlock.conf:9-11`
- **Encoding:** hyprlock process presence by comm name; detached `tatami lock-wait`; Hyprland DPMS; hypridle timer and sleep hooks
- **States:** `Unlocked` → `Locking` → `Locked` → `LockedBlanked` (DPMS off after 3 s) → `Unlocked` (hyprlock exits, DPMS on); sleep `PreSleep` (`--lock-only`) → `Resumed` (`wake 1`)
- **Transitions:** idle 300 s (`hypridle.conf:14-15`), `lock_cmd` (`:6`), before/after sleep (`:7-9`), resume (`:16`); Super+Ctrl+L (`desktop/tatami/config/hypr/hyprland.lua:658`); spawn unless locked (`power.rs:13-19`); layout reset (`power.rs:21`); delayed DPMS off (`power.rs:22-27`); hyprlock returns → `wake(0)` (`power.rs:30-33`); input wakes DPMS (`hyprland.lua:224-225`)
- **Hazards:** check-then-spawn race between two `tatami lock` calls (idle timeout and a key) can start two hyprlocks, and the loser's `wake(0)` turns DPMS on; `.hyprlock-wrapped` is longer than the 15-character comm and can never match (`power.rs:7`); a hyprlock crash runs `wake` while Hyprland keeps the session locked (`hyprland.lua:183`)
- **Reify:** Low — the real state lives in external daemons

<a id="ttl-21"></a>

### TTL-21 — Power meter socket service and reading throttle
- **Where:** `desktop/tatami/tool/src/power_meter.rs:79` (`last: Option<(Instant, String)>`), `:61` (`serve`); units `desktop/tatami/module.nix:410-450`
- **Encoding:** socket activation (`LISTEN_PID`/`LISTEN_FDS`), 60 s poll timeout, timestamped cached reading
- **States:** `SocketOnly` → `Serving` (fresh read at most once a second) → `IdleExit` (60 s) → `SocketOnly`
- **Transitions:** activation check (`power_meter.rs:64-76`); poll (`:87`); idle exit (`:88-90`); cache refresh (`:100-107`)
- **Hazards:** none observed
- **Reify:** Low — small and local
- **Related:** Activity window `desktop/tatami/activity/src/power.rs:25` (`SOCKET`)

<a id="ttl-22"></a>

### TTL-22 — Mixed-audio recorder children (SessionChild, AudioMix)
- **Where:** `desktop/tatami/tool/src/recording_audio.rs:14` (`SessionChild`), `:137` (`AudioMix.client`, `.sink_id`, `.source_id`); loop `desktop/tatami/tool/src/capture.rs:472` (`record_with_mix`)
- **Encoding:** children with PDEATHSIG and a per-child stop signal; pw-cli kept alive by its open stdin; combine node and two links in the graph; 1 s health timer `last_check`
- **States:** `MixStarting` (≤3 s for links) → `MixConnected` → `RecorderStarting` (≤1 s) → `Recording` (poll 100 ms, health 1 s) → `RecorderExited` | `MixDied` | `SourceLost` → Drop: recorder SIGINT ≤5 s then SIGKILL, then mixer SIGKILL
- **Transitions:** spawn with PDEATHSIG and parent check (`recording_audio.rs:17-36`); load-module (`:158-163`); wait for links (`:170-179`); recorder spawn (`capture.rs:474-481`); `started(5)` (`capture.rs:482`); poll loop (`capture.rs:487-506`); Drop (`recording_audio.rs:39-60`)
- **Hazards:** recorder-before-mixer teardown relies on local declaration order; the mix follows exact device serials, so a default-device switch keeps the old device and an unplug aborts the recording; `started(5)` checks any recorder, not this child
- **Reify:** Medium — two coupled process lifecycles with a Drop-order invariant

<a id="ttl-23"></a>

### TTL-23 — Night Light (hyprsunset)
- **Where:** `desktop/tatami/tool/src/toggle.rs:8` (`SUNSET_UNIT`), `:79` (`night_file`), `:99` (`set_temperature`), `:124` (`tinted`); `desktop/tatami/config/hypr/hyprsunset.conf:2-5`; unit `desktop/tatami/module.nix:323-325`
- **Encoding:** on-demand unit state, hyprsunset's in-memory temperature (via hyprctl), persisted night temperature, Waybar signal 11 with `interval: once`
- **States:** `Off` (unit inactive) → `Starting` (≤2 s for the socket) → `Day` (≥6000 K) ↔ `Night` (<6000 K); `Unresponsive` (active, no temperature); the unit is never stopped
- **Transitions:** toggle (`toggle.rs:131-141`); `on|off|KELVIN` (`toggle.rs:147-169`), night temperature kept (`toggle.rs:162-164`); start unit (`toggle.rs:100-102`), wait (`:104-109`), set and verify (`:110-119`); 07:00 identity profile (`hyprsunset.conf:2-5`); unit restart → identity (TTL-17)
- **Hazards:** the 07:00 profile and any restart change the tint without signalling Waybar, so the indicator stays active until the next toggle; a failed `set_temperature` still notifies success (`toggle.rs:134-139`)
- **Reify:** Medium — unit, daemon value and indicator are separate sources
- **Related:** Waybar `custom/nightlight` (`desktop/tatami/config/waybar/config.jsonc:70-76`); Displays Night Shift `desktop/tatami/displays/src/system.rs:172`

<a id="ttl-24"></a>

### TTL-24 — Session update receipt
- **Where:** `desktop/tatami/tool/src/update.rs:9` (`Manifest`), file `update.rs:24-28` (`$XDG_RUNTIME_DIR/tatami/update-session.json`); desired manifest `desktop/tatami/module.nix:358-380`
- **Encoding:** version, boot and session identity, `session`/`reload`/`keep_awake` fingerprints, `error`
- **States:** `Recorded` → `Pending` (switch changed the desired manifest) → `Applied` | `PinnedRetained` (old hyprland fingerprint kept) | `Failed(error)` | `Unverifiable` (other boot or session); `keep_awake` advances only while the holder is inactive; `session` only at next login
- **Transitions:** `remember` at launch (`update.rs:53-57`, `desktop/tatami/tool/src/main.rs:101`); `apply` identity check (`update.rs:77-79`), reload and error record (`:80-89`), advance or retain (`:90-100`), clear error (`:102`), Keep Awake (`:103-113`), save (`:116`); `awake_stopped` (`update.rs:59-70`, from `desktop/tatami/tool/src/awake.rs:103`)
- **Hazards:** `apply` and `awake_stopped` read-modify-write without a lock (turning Keep Awake off during `update-session` can lose one update); `session_invocation` is recorded but checked only by Yukimi
- **Reify:** Medium — versioned JSON protocol with per-field advancement rules
- **Related:** Yukimi `apps/yukimi/crates/yukimi-system/src/completion.rs:11` (`SESSION_RECEIPT`), `:540-543`, `:686`

<a id="ttl-25"></a>

### TTL-25 — Weather report cache and fetch coordination
- **Where:** `desktop/tatami/tool/src/weather.rs:50` (`cache_file`), `:58` (`place_file`), `:71` (`refreshing_file`), `:84` (`Lock`); thresholds `weather.rs:26-35`
- **Encoding:** cached report with `fetched` and `located.at`, saved place file, timestamped refreshing marker, `weather.lock` flock, retry pauses
- **States:** report `None` | `Fresh` (<15 min) | `Due` | `Stale` (≥6 h, hidden); located place `Valid` (<1 h) | `Expired`; fetch `Idle` | `Fetching` | `Retrying` (3 s, 10 s); `Refreshing` (marker <60 s)
- **Transitions:** Waybar poll spawns a fetch when due (`weather.rs:514-524`); background fetch lock, retries, write, clear marker, signal (`:528-545`); click marks, signals, locks or returns, fetches with re-location, clears (`:550-567`); place set or cleared then refresh (`:571-638`)
- **Hazards:** with the lock held, a click's re-location is dropped and only the running fetch clears the marker; cache writes are not atomic (`:104`) while Waybar reads every minute; a backwards clock keeps a report fresh
- **Reify:** High — five artefacts with TTLs coordinated across processes
- **Suspected bug:** choosing a new place while a background fetch attempt is in flight (it read `weather-place.json` at `weather.rs:319`) makes `refresh` return at the lock (`:554-556`); the running fetch writes the old place's report with a fresh `fetched`, so `due` stays false and the bar shows the old place for up to 15 minutes (confidence: low)
- **Related:** Waybar `custom/weather` (`desktop/tatami/config/waybar/config.jsonc:119-126`)

<a id="ttl-26"></a>

### TTL-26 — Session monitor state files (monitors-off, monitors.lua)
- **Where:** `desktop/tatami/tool/src/window.rs:291` (`MONITORS_LUA`), `:292` (`MONITORS_OFF`), `:320` (`remember_monitors`), `:330` (`switch_off`); loaded by `desktop/tatami/config/hypr/hyprland.lua:25-33`
- **Encoding:** connector list and generated Lua in `$XDG_RUNTIME_DIR/tatami/`, loaded after `displays.lua` so the last rule wins
- **States:** `Empty` (new session, all on) → `Off(set)`; must match Hyprland's disabled set at each reload and the session record (TTL-09)
- **Transitions:** cleared at session start (`window.rs:370-376`, `desktop/tatami/tool/src/main.rs:105`); seeded behind a closed lid (`desktop/tatami/tool/src/display_recovery.rs:435`); recovery (`display_recovery.rs:339-343`); Displays apply (`desktop/tatami/tool/src/displays.rs:481`); Super+Ctrl+Delete (`window.rs:439`, `:460`); snapshot before reload (`window.rs:362`); rewritten by `restore` (`window.rs:124`)
- **Hazards:** plain `fs::write` (`window.rs:325-326`) although Hyprland may read it during a reload (contrast `displays.rs:304-311`), and two files that can diverge; `restore` read-modify-writes without the display lock when run by `tatami-restore`, so it can revert a concurrent watcher change at login
- **Reify:** High — eight mutation sites in four files, redundant with TTL-09

<a id="ttl-27"></a>

### TTL-27 — Remembered Hyprland toggles
- **Where:** `desktop/tatami/tool/src/window.rs:16` (`toggles/`), `:13-14` (`GAPS`, `ASPECT`), `:731` (`TOUCHPAD`), `:67` (`workspace-layouts/`)
- **Encoding:** presence of flag files and per-workspace layout files, reapplied by `restore`
- **States:** per toggle `Off` ↔ `On`; `OffPendingLogin` (flag removed, reload pinned or failed, setting still active); per workspace `Default` | `Layout(name)`
- **Transitions:** `flip` on writes flag and evals (`window.rs:40-45`), off removes flag and reloads (`window.rs:46-51`); layout write and eval (`window.rs:87-89`); touchpad flag (`window.rs:762-772`); `restore` at login (`desktop/tatami/module.nix:314-321`) and after reloads (`window.rs:98-120`, `:365`)
- **Hazards:** turning a toggle off when the reload is `Failed` (e.g. display lock busy) removes the flag silently while the setting stays on; the touchpad toggle reads the flag, not the device
- **Reify:** Low — independent flags

<a id="ttl-28"></a>

### TTL-28 — Displays window presentation and replacement
- **Where:** `desktop/tatami/tool/src/window.rs:560` (`present_displays`), `:602` (`bring_here`)
- **Encoding:** Hyprland client list, D-Bus name owner and responsiveness, process age, attempt counter, saved `cursor:no_warps`
- **States:** `Present+Responsive` → `Brought`; `Present+Hung` → `Killed` → `Launching`; `OwnerWithoutWindow` (>5 s) → `Killed`; `Launching` → poll 8 s → `Brought` | `Retry` → poll 8 s → `Failed`
- **Transitions:** present or replace (`window.rs:561-577`); poll and retry (`:578-594`); notify (`:595`); `stop_process` SIGTERM 2 s then SIGKILL (`:520-531`); warps off, move, center, focus, restore saved value (`:603-640`)
- **Hazards:** `no_warps` is saved and restored by whichever invocation runs, so two concurrent `tatami display` calls polling for a new window (`:578-587`) can interleave and leave warps off until a reload; an owner younger than 5 s is not replaced
- **Reify:** Medium — retry ladder over another process's lifecycle
- **Related:** Displays app `org.tatami.Displays` (`desktop/tatami/displays/`)

<a id="ttl-29"></a>

### TTL-29 — Pop-out window toggle
- **Where:** `desktop/tatami/tool/src/window.rs:196` (`pop`), `:166` (`pop_steps`)
- **Encoding:** the window's `pinned` property as discriminator, plus floating and the `pop` tag
- **States:** `Tiled|Floating` → `Popped` (floating, fitted, pinned, top, tagged) → `Tiled`
- **Transitions:** read `pinned` (`window.rs:200`); pop steps (`:179-192`); unpop steps (`:169-174`)
- **Hazards:** any pinned window (picture-in-picture rules pin, `desktop/tatami/config/hypr/hyprland.lua:294`, `:306`) counts as popped and is unpinned and tiled; the `pop` tag is set but never read
- **Reify:** Low — one property

## Already explicit

- None in this slice.

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/tatami/config/foot/foot.ini` | 54 | none |  | terminal configuration values |
| `desktop/tatami/config/hypr/hypridle.conf` | 17 | machines | [TTL-20](#ttl-20) | idle lock and sleep hooks |
| `desktop/tatami/config/hypr/hyprland.lua` | 658 | machines | [TTL-11](#ttl-11), [TTL-12](#ttl-12), [TTL-20](#ttl-20), [TTL-26](#ttl-26), [TTL-29](#ttl-29) | lid events, state-file loading, window rules; no submaps |
| `desktop/tatami/config/hypr/hyprlock.conf` | 61 | machines | [TTL-20](#ttl-20) | lock screen configuration |
| `desktop/tatami/config/hypr/hyprsunset.conf` | 5 | machines | [TTL-23](#ttl-23), [TTL-17](#ttl-17) | 07:00 identity profile |
| `desktop/tatami/config/mako/config` | 39 | machines | [TTL-01](#ttl-01), [TTL-07](#ttl-07) | do-not-disturb mode; screenshot card history |
| `desktop/tatami/config/polkit/hypr/application-style.conf` | 4 | none |  | style values |
| `desktop/tatami/config/polkit/kdeglobals` | 30 | none |  | color scheme values |
| `desktop/tatami/elephant/package.nix` | 23 | none |  | package override |
| `desktop/tatami/elephant/search.patch` | 75 | none |  | stateless query-matching change |
| `desktop/tatami/keep-awake.nix` | 41 | machines | [TTL-03](#ttl-03) | inhibitor holder unit |
| `desktop/tatami/module.nix` | 546 | machines | [TTL-02](#ttl-02), [TTL-03](#ttl-03), [TTL-04](#ttl-04), [TTL-17](#ttl-17), [TTL-21](#ttl-21), [TTL-24](#ttl-24) | session units, units.json, update.json, power-meter socket |
| `desktop/tatami/mpv.conf` | 6 | none |  | player defaults |
| `desktop/tatami/tool/package.nix` | 58 | none |  | build recipe |
| `desktop/tatami/tool/src/awake.rs` | 265 | machines | [TTL-03](#ttl-03) | Keep Awake |
| `desktop/tatami/tool/src/background.rs` | 518 | machines | [TTL-04](#ttl-04), [TTL-05](#ttl-05) | wallpaper choice; settings menu |
| `desktop/tatami/tool/src/browser.rs` | 139 | none |  | desktop entry parsing and launch |
| `desktop/tatami/tool/src/capture.rs` | 802 | machines | [TTL-06](#ttl-06), [TTL-07](#ttl-07), [TTL-08](#ttl-08), [TTL-22](#ttl-22) | screenshots, freeze, recording |
| `desktop/tatami/tool/src/clip.rs` | 58 | none |  | stateless chord mapping |
| `desktop/tatami/tool/src/display_policy.rs` | 612 | machines | [TTL-09](#ttl-09) | session record and pure plan |
| `desktop/tatami/tool/src/display_recovery.rs` | 822 | machines | [TTL-10](#ttl-10), [TTL-11](#ttl-11), [TTL-12](#ttl-12), [TTL-13](#ttl-13), [TTL-14](#ttl-14), [TTL-09](#ttl-09) | reconcile, watcher, lid, Xft.dpi, rescue; login seeding |
| `desktop/tatami/tool/src/displays.rs` | 739 | machines | [TTL-15](#ttl-15), [TTL-16](#ttl-16), [TTL-09](#ttl-09), [TTL-26](#ttl-26) | apply transaction; store and migration |
| `desktop/tatami/tool/src/hypr.rs` | 356 | machines | [TTL-17](#ttl-17) | config_pinned; bounded one-shot IPC helpers |
| `desktop/tatami/tool/src/info.rs` | 156 | none |  | time and battery formatting |
| `desktop/tatami/tool/src/lid.rs` | 127 | machines | [TTL-12](#ttl-12), [TTL-09](#ttl-09) | lid probes; DRM connectors for login seeding |
| `desktop/tatami/tool/src/main.rs` | 692 | machines | [TTL-17](#ttl-17), [TTL-24](#ttl-24), [TTL-26](#ttl-26) | dispatch; session launch and reload |
| `desktop/tatami/tool/src/media.rs` | 201 | none |  | stateless volume and brightness commands |
| `desktop/tatami/tool/src/menu.rs` | 647 | machines | [TTL-18](#ttl-18) | Walker handoff; rest is layout math |
| `desktop/tatami/tool/src/network.rs` | 948 | machines | [TTL-19](#ttl-19) | Wi-Fi join; DNS and panel are one-shot |
| `desktop/tatami/tool/src/power.rs` | 124 | machines | [TTL-20](#ttl-20) | lock and DPMS |
| `desktop/tatami/tool/src/power_meter.rs` | 140 | machines | [TTL-21](#ttl-21) | socket-activated throttle |
| `desktop/tatami/tool/src/recording_audio.rs` | 234 | machines | [TTL-22](#ttl-22) | recorder children and mixer |
| `desktop/tatami/tool/src/terminal.rs` | 186 | none |  | launch helpers; stateless focus-or-launch |
| `desktop/tatami/tool/src/toggle.rs` | 270 | machines | [TTL-23](#ttl-23), [TTL-01](#ttl-01) | Night Light; DND toggle; indicators |
| `desktop/tatami/tool/src/update.rs` | 117 | machines | [TTL-24](#ttl-24) | session receipt |
| `desktop/tatami/tool/src/util.rs` | 287 | none |  | process helpers; pids_named probes back TTL-06 TTL-08 TTL-20 |
| `desktop/tatami/tool/src/weather.rs` | 762 | machines | [TTL-25](#ttl-25) | report cache and fetches |
| `desktop/tatami/tool/src/window.rs` | 929 | machines | [TTL-26](#ttl-26), [TTL-27](#ttl-27), [TTL-28](#ttl-28), [TTL-29](#ttl-29), [TTL-17](#ttl-17), [TTL-09](#ttl-09) | monitor state, toggles, Displays presentation, pop |
| `desktop/tatami/tool/src/xwayland.rs` | 174 | machines | [TTL-13](#ttl-13) | Xft.dpi helpers |
