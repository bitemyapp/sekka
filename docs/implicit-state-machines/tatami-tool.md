# TTL: Tatami command, display watcher and configuration

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/tatami/tool, desktop/tatami`. Audited at `8e0acbe5`; line numbers refer to that commit.

39 files audited. 29 implicit machines: 8 High, 9 Medium, 12 Low. This page has the entries of the High machines and of TTL-19, which has a follow-up; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#ttl-03)

<a id="ttl-04"></a>

### TTL-04 — Wallpaper choice and per-output overrides
- **Where:** `desktop/tatami/tool/src/background.rs:160` (`chosen`: symlink `$XDG_STATE_HOME/tatami/background`), `:30` (`background.json`), `:178` (`ensure`)
- **Encoding:** chosen symlink (global image), JSON settings (mode, color, per-output overrides with images), `background.lock` flock, shipped default `/etc/xdg/tatami/background`, swaybg unit restarted to apply
- **States:** global `Default` (missing or dangling symlink) | `Chosen(image)`; per output `Inherit` | `Override`; per-output image `Missing` → drawn with the global image but kept; settings `Malformed` (only a full reset recovers)
- **Transitions:** `change` under flock (`background.rs:208-214`); global `set` relinks and restarts (`background.rs:236-239`); other keys save atomically and restart (`background.rs:256-257`, `:91-97`); full reset ignores a malformed file (`background.rs:217-221`); `ensure` relinks the default when the target is gone (`background.rs:179-181`), from `ExecStartPre` (`desktop/tatami/module.nix:280`) and `run` (`background.rs:287`); missing per-output image falls back non-destructively (`background.rs:124-126`); `restart --no-block` (`background.rs:146-153`)
- **Hazards:** two fallback policies: a missing per-output image is temporary, a missing global image is permanent; `--no-block` restart reports nothing after the decode check
- **Reify:** High — persisted choice with a destructive fallback (data loss)
- **Suspected bug:** a global wallpaper on a removable or late-mounted drive (udiskie automounts concurrently, `desktop/tatami/module.nix:311`) is a dangling symlink when `tatami-swaybg`'s `ExecStartPre=background ensure` runs at login; `ensure` replaces it with the default for good, whereas the same image as a per-output choice would survive (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#ttl-04)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#ttl-08)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#ttl-10)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#ttl-17)

<a id="ttl-19"></a>

### TTL-19 — Wi-Fi join flow
- **Where:** `desktop/tatami/tool/src/network.rs:621` (`join`), `:560` (`join_with_password`), `:498` (`Secret`); classification `:28` (`Join`)
- **Encoding:** `Join` enum, saved-connection UUID, `created` flag for rollback, `Secret` temporary file removed on Drop
- **States:** `Active` | `SavedUp` → `Connected` | `PasswordRetry`; new secured: `Prompt` → `SecretWritten` → `ConnectionAdded` → `Up` → `Connected` | `RolledBack`; enterprise/WEP → `HandedOff`
- **Transitions:** already active (`network.rs:622-624`); saved lookup (`:625`); hand-off (`:628-630`); password path (`:631-633`); saved/open up (`:636-649`); password retry after a saved failure (`:653-655`); add with own UUID (`:572-597`); up with passwd-file (`:601-610`); delete created on failure (`:612-616`); Secret Drop (`:525-529`)
- **Hazards:** a killed helper leaves `tatami-wifi-PID` with the PSK (0600) in the runtime dir and/or a key-less created connection; `saved` takes the first profile with that SSID; scan results arrive only for the next opening (`:711`)
- **Reify:** Low — linear flow with one retry branch
- **Related:** Network window `tatami-network --join` (`desktop/tatami/network/`)
- **Follow-up:** the key-file hazard is fixed in [#304](https://github.com/bitemyapp/sekka/pull/304): the key reaches nmcli through a pipe (`passwd-file /dev/stdin`), so a killed helper leaves no file. A key-less created connection can still stay behind. The machine stays implicit.

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#ttl-25)

<a id="ttl-26"></a>

### TTL-26 — Session monitor state files (monitors-off, monitors.lua)
- **Where:** `desktop/tatami/tool/src/window.rs:291` (`MONITORS_LUA`), `:292` (`MONITORS_OFF`), `:320` (`remember_monitors`), `:330` (`switch_off`); loaded by `desktop/tatami/config/hypr/hyprland.lua:25-33`
- **Encoding:** connector list and generated Lua in `$XDG_RUNTIME_DIR/tatami/`, loaded after `displays.lua` so the last rule wins
- **States:** `Empty` (new session, all on) → `Off(set)`; must match Hyprland's disabled set at each reload and the session record (TTL-09)
- **Transitions:** cleared at session start (`window.rs:370-376`, `desktop/tatami/tool/src/main.rs:105`); seeded behind a closed lid (`desktop/tatami/tool/src/display_recovery.rs:435`); recovery (`display_recovery.rs:339-343`); Displays apply (`desktop/tatami/tool/src/displays.rs:481`); Super+Ctrl+Delete (`window.rs:439`, `:460`); snapshot before reload (`window.rs:362`); rewritten by `restore` (`window.rs:124`)
- **Hazards:** plain `fs::write` (`window.rs:325-326`) although Hyprland may read it during a reload (contrast `displays.rs:304-311`), and two files that can diverge; `restore` read-modify-writes without the display lock when run by `tatami-restore`, so it can revert a concurrent watcher change at login
- **Reify:** High — eight mutation sites in four files, redundant with TTL-09
