# AZN: Small crates, session launcher, Nix modules and VM tests

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome (remaining)`. Audited at `8e0acbe5`; line numbers refer to that commit.

114 files audited. 29 implicit machines: 5 High, 2 Medium, 22 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

<a id="azn-01"></a>

### AZN-01 — Keep Awake requested/protected status (setting file against logind's locks)
- **Where:** `desktop/aizome/crates/aizome-awake/src/lib.rs:49` (`setting()`: marker file `$XDG_STATE_HOME/aizome/keep-awake`), `:54` (`requested`), `:60` (`protected_by`), `:72` (`enum Status`); also `desktop/aizome/crates/aizome-awake/src/main.rs:117` (`set`), `:87` (`edit_lock`), `desktop/aizome/session/aizome-keep-awake.service:23`, `desktop/aizome/nix/module.nix:336`
- **Encoding:** partially explicit: `Status` (`lib.rs:84`) is computed from two separately held facts: whether the `keep-awake` file exists (requested; persisted across logins) and whether logind's `ListInhibitors` shows a `block` inhibitor from `WHO` covering sleep, idle and lid (protected). The state also includes the user unit's run state and the `keep-awake.lock` flock that serialises edits.
- **States:** `Off` → `Requested+Starting` → `On` | `Unprotected` (requested, but the holder failed or is restarting, or logind restarted) → (off) `Stopping` → `Off`. `!requested && protected` is reported as `On` (the off transition is still running).
- **Transitions:** `set(true)` takes the lock (`main.rs:118`), writes the file via `.new`+rename (`main.rs:107`), runs `systemctl --user start` (`main.rs:122`), then polls `protected()==on` every 100 ms for 3 s (`main.rs:132`, `:134`); on timeout it probes `is-active` (`main.rs:144`). `set(false)` removes the file (`main.rs:109`) and stops the unit. At login, `aizome-session.target` wants the unit (`module.nix:337`) and `ExecCondition` skips it when the file is absent (`aizome-keep-awake.service:23`; `requested` branch `main.rs:32`). `restartIfChanged = false` keeps it running across switches (`module.nix:338`).
- **Hazards:** `toggle` reads `requested()` before taking the edit lock (`main.rs:30` vs `main.rs:118`). The file is written before the start succeeds, so a failed start leaves "on" persisted for later logins, with no rollback. `set` polls with `pid: None`, so the momentary lock of a crash-looping holder (AZN-02) can satisfy the poll and report "on". The design relies on systemd not auto-restarting a unit that `ExecCondition` skipped while `Restart=always` is set.
- **Reify:** High — suspected bug; the start/poll/timeout phases of `set` are open-coded across two processes and logind.
- **Suspected bug:** two concurrent `aizome-awake toggle` runs (the case named by the lock's doc comment at `main.rs:85`) both evaluate `!requested()` = true before either takes the lock, so both turn Keep Awake on and the second toggle is lost (confidence: medium; the menu bar sends `on`/`off`, so only CLI and script callers of `toggle` are affected).
- **Related:** aizome-shell `desktop/aizome/crates/aizome-shell/src/awake.rs:33` (runs `aizome-awake on|off` and shows the cup from logind's state).
- **Reproduction:** Confirmed (end-to-end): [summary](reproductions.md#azn-01)

<a id="azn-04"></a>

### AZN-04 — Agent registration with NetworkManager (retry, re-register on restart)
- **Where:** `desktop/aizome/crates/aizome-network-agent/src/lib.rs:370` (`Registration{done, asked}`), `:336` (`owner: Option<String>`), `:320` (`run`), `:302` (`REGISTER_RETRY`)
- **Encoding:** the current NetworkManager unique name (`owner`), `Registration.done`, `Registration.asked`, a channel fed by a NameOwnerChanged watcher thread (`:327`–`:335`) and a 2 s `recv_timeout` that drives retries.
- **States:** `NoNM` (owner None; blocks on `owners.recv()` `:352`) → `Unasked` → `Deferred(asked, !done)` (retries every 2 s, `:347`) → `Registered(done)` (blocks on owner changes). Any change of owner resets the machine (`:361`–`:363`). When the bus closes, `run` exits with an error (`:355`) so that systemd restarts the agent.
- **Transitions:** `attempt` (`:344` → `:375`): `Ok` → done (`:388`); PermissionDenied "already registered" → `done = asked` (`:392`–`:397`); any other error → not done (`:398`); `asked = true` in every case (`:405`). Owner left → `cancel_all` (`:359`); new owner → fresh `Registration` (`:362`).
- **Hazards:** `asked` cannot tell "asked, and the reply was lost" from "asked, and was refused". Once `done`, registration is never verified again. Process restarts are limited by the unit's default start limit (`desktop/aizome/session/aizome-network-agent.service:12`–`:13`).
- **Reify:** High — suspected bug; an enum with Unasked, Refused, ReplyLost and Registered would remove it.
- **Suspected bug:** suppose another agent with the same identifier is still registered for the user for longer than `REGISTER_RETRY` (the case the comment at `desktop/aizome/crates/aizome-network-agent/src/lib.rs:389`–`:391` anticipates: "another still is, until its connection closes"). The first refusal sets `asked` with `done` false; the second identical refusal sets `done = asked = true`. The loop then blocks on `owners.recv()` and never registers once the other agent disconnects. NetworkManager is left with no agent and credential prompts never appear until NetworkManager restarts (confidence: medium). `tests/credentials.rs:350`–`:356` covers only "timed out, then refused".
- **Related:** NetworkManager AgentManager (`RegisterWithCapabilities`); test harness AZN-06.
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#azn-04)

<a id="azn-13"></a>

### AZN-13 — Time-zone ownership across activations (saved zone file and `/etc/localtime` link)
- **Where:** `desktop/aizome/nix/module.nix:261` (`time.timeZone` forced to null at priority 90), `:264` (`AIZOME_TIMEZONE_MANAGED`), `:267` (`aizomeTimeZoneSave`), `:281` (`aizomeTimeZoneRestore`)
- **Encoding:** the persisted `/var/lib/aizome/timezone` (the last zone seen); the shape of the `/etc/localtime` link (absolute `/etc/zoneinfo/…` means NixOS's, relative `../etc/zoneinfo/…` means timedated's or Aizome's, or no link at all); the `mutableTimeZone` option; the policy published to timedated.
- **States:** `NixOS-pinned` (absolute link) → saved at activation → `Aizome-managed` (relative link, `:293`) ⇄ changed through timedated. A `Link missing` state is restored from the saved file (`:289`–`:290`), or stays UTC when nothing was saved.
- **Transitions:** save runs before `etc` (`:277`) when the link resolves into a zoneinfo directory (`:268`–`:273`). Restore runs after `etc` (`:282`): it rewrites absolute links as relative ones, or recreates the link from the file (`:293`).
- **Hazards:** the saved file is never cleared, so if `/etc/localtime` is later removed (UTC by absence) the next activation brings back the old zone. Only absolute `/etc/zoneinfo/` links are rewritten. A declared `time.timeZone` is overridden at priority 90 and never used, not even as the starting zone.
- **Reify:** High — suspected bug; the zone is persisted across boots in two representations, plus an activation ordering.
- **Suspected bug:** on a fresh system installed without Genkan (plain `nixos-install` with `programs.aizome.enable` and `time.timeZone = "Europe/Paris"`), there is no link and no saved zone. The declared zone is therefore discarded and the machine runs in UTC until someone sets the zone in Settings. Genkan works around this by seeding the link (`installer/genkan/rust/src/install.rs:699`) (confidence: medium).
- **Related:** Settei `desktop/aizome/apps/settei/src/panes/general/datetime.rs`; `desktop/aizome/crates/aizome-sys/src/datetime/mod.rs`; tests `desktop/aizome/nix/timezone-policy-test.nix`, `desktop/aizome/nix/vm-test.nix:190`.
- **Reproduction:** Confirmed (unit): [summary](reproductions.md#azn-13)

<a id="azn-25"></a>

### AZN-25 — xdg-desktop-portal-wlr screencast capture lifecycle (carried patch)
- **Where:** `desktop/aizome/nix/xdg-desktop-portal-wlr-captures.patch:76` (adds `capture_timer`), `:181` (`xdpw_wlr_frame_capture_cancel`), `:160` (`wlr_frame_capture_timer`), `:256`–`:281` (stream state handler, final form), `:121`–`:125` (`remove_buffer`), `:27`/`:149` (destroy clears the timer pointers), `:208` (records the wlr frame); applied by `desktop/aizome/nix/module.nix:54`
- **Encoding:** per cast: the `pwr_stream_state` bool; `current_frame.pw_buffer`/`xdpw_buffer` (a buffer kept dequeued); `capture_timer` (a delayed capture is scheduled); `process_retry` (retry while starved of buffers); `ext_session.frame` or `wlr_session.wlr_frame` (a frame in flight).
- **States:** `Paused(no buffer)` | `Paused(kept buffer, capture cancelled)` → `Streaming` → `CaptureScheduled(capture_timer)` → `CaptureInFlight(frame)` → ready → buffer queued → next capture; `Starved(process_retry)`. When the current buffer is removed, the capture is cancelled and the buffer forgotten. `Destroyed`: timers freed and their pointers cleared.
- **Transitions:** entering STREAMING with a kept buffer → capture (`desktop/aizome/nix/xdg-desktop-portal-wlr-captures.patch:260`–`:262`); without one → the process handler (`:263`–`:264`). Leaving STREAMING → retry disarmed and capture cancelled, buffer kept (`:268`–`:275`). `remove_buffer` of the current buffer → cancel and clear (`:121`–`:125`). The timer fires → `capture_timer` cleared, then the capture starts (`:163`–`:164`). Cancel destroys the timer and the frame (`:182`–`:194`). Destroy clears `process_retry` and `capture_timer` (`desktop/aizome/nix/xdg-desktop-portal-wlr-captures.patch:27`, `:148`–`:149`).
- **Hazards:** the rule of at most one live timer or frame per cast assumes `xdpw_wlr_frame_capture` is never entered while a capture is pending. If it were, the armed timer's pointer would be overwritten (`:171`), and that timer would later fire on whatever buffer was current. A kept buffer stays dequeued through UNCONNECTED and ERROR until PipeWire removes it. The patch is carried until upstream takes equivalent fixes.
- **Reify:** High — five fields spread over three source files, with invalid combinations (a kept buffer while the stream flag is false, a timer and a frame alive together) that the patch exists to rule out.
- **Related:** Aizome's compositor capture protocols `desktop/aizome/crates/aizome-comp-core/src/protocols/image_copy_capture.rs`, `desktop/aizome/crates/aizome-comp-core/src/protocols/screencopy.rs`; AZN-15.

<a id="azn-27"></a>

### AZN-27 — Session launcher phases (`aizome-session`)
- **Where:** `desktop/aizome/session/aizome-session:17` (login-shell re-exec, sentinel `--no-login-shell` `:24`), `:63` (manager state), `:70`–`:71` (session lock), `:77` (service state), `:90` (`cleanup`), `:104`–`:105` (`wait_pid`, `pending_signal`), `:118` (`restarts`), `:119` (restart loop)
- **Encoding:** the sentinel argument; `state` (from `is-system-running`, then `ActiveState`); an flock on fd 9 for `$XDG_RUNTIME_DIR/aizome-session.lock` (kept, never unlinked); `wait_pid`; `pending_signal`; which traps are installed (deferring or exiting); `restarts`, `started` (wall clock) and `result`; the exported `AIZOME_ENVIRONMENT_KEYS="1 …"` record, versioned and read back from `/proc/PID/environ` (`:59`).
- **States:** `LoginShell?` → `EnvLoaded` → `NoManager` (exec `dbus-run-session aizome`, `:148`–`:154`) | `Locked` → `Refused` (lock held `:71`–`:74`, or the service is neither inactive nor failed `:78`–`:81`) | `Prepared` (traps, reset-failed, environment imported, `:106`–`:114`) → `Starting` (signals deferred, `:126`–`:129`) → `Waiting` (`:131`–`:135`) → `Exited` → `WatchdogRestart` (Result=watchdog, with fewer than 3 restarts or a run of at least 300 s, `:138`–`:144`) → `Starting` | `Cleanup` (EXIT trap: kill and reap the start client `:94`–`:99`, start the shutdown target `:100`, unset the environment `:102`) → exit with the status.
- **Transitions:** `exec 9>` and `flock -n` (`:70`–`:71`); `trap 'cleanup' EXIT` (`:106`); background start (`:129`) with `wait_pid` captured (`:130`); pending signal re-raised (`:134`); `result` read (`:138`); count reset (`:140`); limit (`:141`); `reset-failed aizome.service` (`:144`); exit (`:146`).
- **Hazards:** the restart window is measured with `date +%s` (`:120`, `:140`), so a clock step (NTP at boot, or Settings' Date & Time) can reset or stretch the five minutes wrongly. The first `reset-failed` (`:110`) clears the failed state of every user unit, not only Aizome's. Any manager state other than running, degraded or starting (initializing, stopping, unreachable) runs the compositor outside systemd, with no session target, credential agent or Keep Awake (`:148`–`:154`). If the launcher is SIGKILLed mid-cleanup (for example by the scope's stop timeout), the environment is never unset, and a stale `XDG_SESSION_DESKTOP=aizome` keeps nm-applet out of the next desktop (`desktop/aizome/nix/module.nix:89`–`:93`). A launcher killed while its service survives turns later logins away ("already running") until that service stops.
- **Reify:** High — eight phases encoded in shell variables, traps and a lock file, with signal-safety invariants that the tests can only check by forcing instruction-sized windows.
- **Related:** AZN-28 (units), AZN-16, AZN-17 (tests); Yukimi `apps/yukimi/crates/yukimi-system/src/completion/aizome.rs` (reads `AIZOME_ENVIRONMENT_KEYS`).
