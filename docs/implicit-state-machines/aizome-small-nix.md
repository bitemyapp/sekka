# AZN: Small crates, session launcher, Nix modules and VM tests

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome (remaining)`. Audited at `8e0acbe5`; line numbers refer to that commit.

114 files, 0 pending. 29 implicit machines: 5 High, 2 Medium, 22 Low.

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

<a id="azn-02"></a>

### AZN-02 — Keep Awake holder service lifecycle (`aizome-awake hold`)
- **Where:** `desktop/aizome/crates/aizome-awake/src/main.rs:171` (`hold`), `:206` (`IdleInhibitor`), `:232` (`wait`); also `desktop/aizome/session/aizome-keep-awake.service:13`, `:19`, `:25`–`:27`
- **Encoding:** the lock lives exactly as long as the process: the logind inhibitor fd `_lock` plus the Wayland connection holding `zwp_idle_inhibitor_v1` on an unmapped surface. Readiness is signalled with `sd_notify` (`Type=notify`). The loop is `Restart=always`, `RestartSec=1`, `StartLimitIntervalSec=0`, `TimeoutStartSec=15`.
- **States:** `Starting` (inhibitor taken `desktop/aizome/crates/aizome-awake/src/main.rs:173` → verified listed `desktop/aizome/crates/aizome-awake/src/main.rs:175` → idle inhibitor `desktop/aizome/crates/aizome-awake/src/main.rs:178`) → `Ready` (`READY=1` `desktop/aizome/crates/aizome-awake/src/main.rs:179`) → `Holding` (1 s loop `desktop/aizome/crates/aizome-awake/src/main.rs:180`–`:183`) → `Failed` (logind dropped the lock `desktop/aizome/crates/aizome-awake/src/main.rs:183`, compositor gone `desktop/aizome/crates/aizome-awake/src/main.rs:233`/`desktop/aizome/crates/aizome-awake/src/main.rs:244`, D-Bus error) → systemd auto-restart after 1 s → `Starting`. It is `Stopped` by `stop` or through `PartOf=aizome-session.target`.
- **Transitions:** `inhibit` (`desktop/aizome/crates/aizome-awake/src/main.rs:173`); self-check (`desktop/aizome/crates/aizome-awake/src/main.rs:175`); `IdleInhibitor::new` (`desktop/aizome/crates/aizome-awake/src/main.rs:178`); notify (`desktop/aizome/crates/aizome-awake/src/main.rs:179`); `idle.wait(1s)` (`desktop/aizome/crates/aizome-awake/src/main.rs:181`); re-check against logind (`desktop/aizome/crates/aizome-awake/src/main.rs:182`); every error returns from `main` with a failure exit, and the unit's restart policy takes over.
- **Hazards:** while no compositor is reachable (between watchdog restarts, or with a stale `WAYLAND_DISPLAY`), each restart takes and drops logind's block lock within milliseconds, once a second without limit. `protected` flickers as a result, and AZN-01's 3 s poll can catch one of those moments. A logind restart goes unnoticed for up to a second, during which no lock is held.
- **Reify:** Low — linear and short; the unit's restart policy does the looping.
- **Related:** compositor idle-inhibit (aizome-comp-core); logind `Inhibit`/`ListInhibitors`.

<a id="azn-03"></a>

### AZN-03 — GetSecrets request registry and cancellation (network agent)
- **Where:** `desktop/aizome/crates/aizome-network-agent/src/lib.rs:147` (`Request{key, number, cancel, authorized}`), `:154` (`Requests{arrived, list}`); also `desktop/aizome/crates/aizome-network-agent/src/process.rs:22` (`Cancel{cancelled, deadline}`)
- **Encoding:** each request is described by its membership in `list`, its `authorized` bool, a shared `Cancel` (an `AtomicBool` plus an `Instant` deadline) and its arrival `number`. Each call runs on its own worker thread (`work`, `lib.rs:200`). `Engine::get` (`lib.rs:40`) runs the steps (supplied values → keyring → dialog → store) with cancellation checks between them.
- **States:** `Listed(unauthorized)` → `Authorized` → `Working` (lookup `:55` → dialog `:89` → store `:113`) → `Done`/removed; `Cancelled` (flag set and entry removed) from Listed or Authorized; `TimedOut` (120 s deadline); `Rejected` (NotAuthorized, or more than 16 outstanding).
- **Transitions:** arrive and push (`:231`–`:232`); `authorize` (`:236`); cap check (`:240`); `cancel_before` displaces earlier requests for the same key (`:243`); marked authorized (`:245`); engine on a thread (`:250`); removed on return (`:253`). `CancelGetSecrets` takes a number (`:264`), authorizes (`:265`) and calls `cancel_before` (`:266`). When NM's owner leaves, `cancel_all` runs (`:359` → `:195`). Checks happen at `:41`, `:58`, `:107` and `:115`.
- **Hazards:** a request is listed before it is authorized, so a cancel that arrives during authorization finds it. A displaced entry is never marked `authorized` (`:244` finds nothing) and keeps running until its next `cancel.check()`. The invariant "cancelled ⇒ removed from `list`" holds only because `cancel_before` and `cancel_all` each do both. A single deadline covers every step, so a slow dialog leaves little time for the keyring store.
- **Reify:** Medium — per-request phases (pending, authorized, cancelled) would make the cap and displacement rules explicit; today they are guarded but scattered over three handlers.
- **Related:** NetworkManager SecretAgent caller; `desktop/aizome/nix/vm-network-auth-test.py:214` (cancellation by NetworkManager).

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

<a id="azn-05"></a>

### AZN-05 — Credential worker subprocess lifecycle (keyring, dialog, VPN helper)
- **Where:** `desktop/aizome/crates/aizome-network-agent/src/process.rs:84` (`run`), `:73` (`abort`); also `desktop/aizome/crates/aizome-network-agent/src/keyring.rs:38` (`Keyring::call`), `desktop/aizome/crates/aizome-network-agent/src/prompt.rs:15` (`Native::ask`), `:39` (`Dialog.answer`), `desktop/aizome/crates/aizome-network-agent/src/vpn.rs:120`, `desktop/aizome/crates/aizome-network-agent/src/main.rs:19`, `:26` (worker modes), `desktop/aizome/crates/aizome-network-agent/src/keyring.c:37` (`SECRET_SEARCH_UNLOCK`)
- **Encoding:** locals of `run`: `child`; `stdin: Option` (None once fully written or closed); `stdout: Option` (None at EOF); `written`; `output`. The child runs in its own session (`setsid`, `desktop/aizome/crates/aizome-network-agent/src/process.rs:89`–`:94`). Cancellation uses the `Cancel` flag and deadline. The dialog worker answers with `Option<HashMap>`, where `null` means the person cancelled (`prompt.rs:26`–`:28`).
- **States:** `Spawned` (`desktop/aizome/crates/aizome-network-agent/src/process.rs:96`) → `Feeding` (`desktop/aizome/crates/aizome-network-agent/src/process.rs:108`–`:118`) → `Draining` (`desktop/aizome/crates/aizome-network-agent/src/process.rs:119`–`:123`) → `Exited` (`desktop/aizome/crates/aizome-network-agent/src/process.rs:129`) → final drain (`desktop/aizome/crates/aizome-network-agent/src/process.rs:150`) → `Ok` | `Failed` | `UserCanceled`. From any running state, `Aborted` kills the process group: on cancel or deadline (`desktop/aizome/crates/aizome-network-agent/src/process.rs:104`–`:106`), on oversized output (`desktop/aizome/crates/aizome-network-agent/src/process.rs:124`–`:126`) or on a wait error (`desktop/aizome/crates/aizome-network-agent/src/process.rs:141`–`:143`).
- **Transitions:** 25 ms poll between steps (`desktop/aizome/crates/aizome-network-agent/src/process.rs:138`); post-exit cancel check (`desktop/aizome/crates/aizome-network-agent/src/process.rs:153`); status mapping (`desktop/aizome/crates/aizome-network-agent/src/process.rs:157`); in the dialog, `Submit` sets `answer` and quits (`prompt.rs:61`–`:63`), `Cancel` quits with none (`prompt.rs:65`).
- **Hazards:** after a normal exit, whatever the helper started keeps running by design (a browser for single sign-on) and is not killed by a later cancel. A non-zero exit means `Failed` for Aizome's own workers but `UserCanceled` for VPN helpers (`vpn.rs:124`). Cancelling a keyring unlock prompt assumes gnome-keyring drops the prompt when the killed worker's D-Bus connection closes (`keyring.c:1`–`:4`). If the deadline passes after a successful exit, the output is discarded anyway (`desktop/aizome/crates/aizome-network-agent/src/process.rs:153`).
- **Reify:** Low — confined to one well-structured function.
- **Related:** libsecret/gnome-keyring (Secret Service); NetworkManager VPN plugin `auth-dialog` protocol; the dialog is an aizome-kit `Program`.

<a id="azn-06"></a>

### AZN-06 — (test harness) private bus with a scripted AgentManager and a blocking dialog stand-in
- **Where:** `desktop/aizome/crates/aizome-network-agent/tests/credentials.rs:301` (`AgentManager{calls, answers}`), `:67` (`Ui{asked, answer, wait}`), `:205`/`:339` (private `dbus-daemon`), `:329` (`eventually`)
- **Encoding:** a scripted reply queue consumed by `answers.remove(0)` (`:314`); a call counter; a `wait` loop in the dialog stand-in that runs until cancelled (`:75`–`:80`); the NM name requested and released (`:326`, `:389`).
- **States:** daemon up → NM owns its name → agent registered after the scripted failures → request pending in the dialog → NM releases its name → request cancelled → new NM → agent registers again → daemon killed → agent ends.
- **Hazards:** fixed sleeps are used to assert that nothing happens (`:275`, `:384`). There is no case with two consecutive "already registered" refusals (AZN-04).
- **Reify:** Low — test fixture.

<a id="azn-07"></a>

### AZN-07 — Power meter socket activation, idle exit and reading throttle
- **Where:** `desktop/aizome/crates/aizome-power-meter/src/main.rs:80` (`Cache(Option<(u64, String)>)`), `:29` (`EVERY_US`), `:31` (`IDLE_MS`), `:107` (serve loop); also `desktop/aizome/nix/module.nix:352`, `:361` (socket and sandboxed root service), `desktop/aizome/nix/vm-power-meter-test.nix:15` (stand-in with the same throttle, `read` `:37`)
- **Encoding:** a systemd socket (persistent) plus the process lifetime; `Cache` holds the last reading and its CLOCK_BOOTTIME time.
- **States:** `Listening` (socket only) → `Activated` (`LISTEN_PID`/`LISTEN_FDS` check, `desktop/aizome/crates/aizome-power-meter/src/main.rs:100`) → `Serving` (per accept, a fresh reading if at least 1 s has passed, otherwise the cached one, `:87`) → `IdleExit` after 60 s without a question (`desktop/aizome/crates/aizome-power-meter/src/main.rs:111`) → `Listening`.
- **Transitions:** activation on connect; refresh (`:89`–`:90`); idle exit (`desktop/aizome/crates/aizome-power-meter/src/main.rs:111`); continue on EINTR (`desktop/aizome/crates/aizome-power-meter/src/main.rs:112`); accept and answer (`desktop/aizome/crates/aizome-power-meter/src/main.rs:116`–`:119`).
- **Hazards:** the cache lives only in the process, so the first question after an idle exit gets a fresh reading (readings still come at least 60 s apart). One slow client blocks the single-threaded loop for up to the 1 s write timeout (`desktop/aizome/crates/aizome-power-meter/src/main.rs:118`). Each question resets the idle timer, so a periodic asker keeps the root service up indefinitely.
- **Reify:** Low — small, and the timing is centralised.
- **Related:** Myaku's client `desktop/aizome/crates/aizome-sys/src/sysinfo/power.rs:97` (`meter_counters`).

<a id="azn-08"></a>

### AZN-08 — File index: query-narrowing cache and persisted index freshness
- **Where:** `desktop/aizome/crates/aizome-search/src/files.rs:91` (`last: Mutex<Option<(String, Vec<u32>)>>`), `:89` (`built: Option<SystemTime>`), `:194` (`age`), `:295` (`save`), `:316` (`load`)
- **Encoding:** the last query and its matched ids (for narrowing); the build time, taken from the walk or from the file's mtime on load (`:318`); the on-disk `files.idx` with magic `AZFI1` (`:302`, `:320`), written to a temp file and renamed (`:300`, `:312`).
- **States:** `NoLast` → `Narrowable(q)` (`:266`–`:267`); the next search reuses the stored matches when it extends `q` (`:228`), and scans everything otherwise. On disk: `Absent` → `Saved(built = mtime)` → stale by age, a decision left to the caller.
- **Transitions:** `build` (`:180`); `search` reads and replaces `last` (`:226`–`:229`, `:266`); `save` (`:295`–`:313`); `load` (`:316`–`:345`).
- **Hazards:** narrowing is valid only while the matcher is monotone under prefix extension (it is, for smart case and multi-atom queries). Concurrent searches race on `last` but always store a consistent pair. Concurrent savers share the fixed `files.tmp` name. The staleness policy (age plus key file) lives in the caller.
- **Reify:** Low — a single cache, correctly guarded.
- **Related:** `desktop/aizome/crates/aizome-shell/src/palette/mod.rs:811` (`index_soon` rebuilds when the index is stale or its key differs).

<a id="azn-09"></a>

### AZN-09 — Frecency history file: staleness stamp and read-modify-write recording
- **Where:** `desktop/aizome/crates/aizome-search/src/frecency.rs:37` (`Frecency{entries, path, seen}`), `:44` (`seen: Option<Stamp>`), `:99` (`is_stale`), `:107` (`record`)
- **Encoding:** `$XDG_STATE_HOME/aizome/frecency.json` on disk is the authority. A process holds an in-memory copy together with the (mtime, len) stamp of the file as it last read or wrote it. A missing file means the history was forgotten.
- **States:** `Held(seen)` → `Stale` (another writer changed the file, or Settings' Forget removed it) → reloaded by the holder. `record` runs load → visit → save (temp file and rename) → new stamp (`:93`).
- **Transitions:** `load` (`:74`–`:82`); `save` (`:85`–`:95`); `record` (`:108`–`:113`); `visit_at` and the capacity eviction (`:127`–`:139`).
- **Hazards:** `record` is an unlocked read-modify-write, and the shell starts one on a new thread for every pick (`desktop/aizome/crates/aizome-shell/src/palette/mod.rs:780`). Two picks close together can therefore lose one, with both writers using the same `frecency.json.tmp` (`desktop/aizome/crates/aizome-search/src/frecency.rs:90`). A `record` in flight while Settings deletes the file writes the old history back. The (mtime, len) stamp misses a same-length rewrite within one timestamp tick of the filesystem.
- **Reify:** Low — small; an flock around `record` would close the races.
- **Related:** Settei's search pane (Forget deletes the file); `desktop/aizome/nix/vm-settings-test.py:1010`.

<a id="azn-10"></a>

### AZN-10 — Shell socket connection protocol (hello, menus, services-text serial, request/reply)
- **Where:** `desktop/aizome/crates/aizome-shell-proto/src/lib.rs:27` (`AppMessage`), `:121` (`ShellMessage`), `:141` (`request`), `:267` (`Client`)
- **Encoding:** an ordering convention on one line-delimited stream. `Hello` comes first and once (`:30`). Complete `Menus` are sent again on every change (`:32`). `ServicesText{serial}` pairs a request with its response (`:35`, `:129`). `Request` needs no hello and is answered by `Reply` (`:43`, `:132`).
- **States:** each connection is either `Connected` → `Hello'd(pid)` → `Publishing` ⇄ `AboutToShow` or `ServicesText(serial)` pending → closed; or `Connected` → `Request sent` → `Reply`, with the client loop skipping other messages (`:144`–`:149`).
- **Transitions:** `Client::hello` (`:280`); `send` (`:286`); `recv` skips blank and unknown lines (`:293`–`:307`); `request` loops until `Reply` or EOF (`:144`–`:150`).
- **Hazards:** `request` and `recv` block with no timeout, so a shell that never replies hangs `aizomectl`. Undecodable lines are dropped silently (`:303`). The pid in `Hello` is self-reported, and the shell keys menus by it without checking peer credentials (`desktop/aizome/crates/aizome-shell/src/apps/registry.rs:44`). A second `Hello` on the same connection replaces the app entry.
- **Reify:** Low — the message types are already explicit; the connection phase lives on the server side.
- **Related:** aizome-shell `desktop/aizome/crates/aizome-shell/src/apps/registry.rs:42`, `desktop/aizome/crates/aizome-shell/src/apps/server.rs:106`; aizome-kit `desktop/aizome/crates/aizome-kit/src/shell.rs`.

<a id="azn-11"></a>

### AZN-11 — Foot palette mode switched by shell signals
- **Where:** `desktop/aizome/etc/foot/foot.ini:4` (contract comment), `:12` (`initial-color-theme=dark`); also `desktop/aizome/scripts/check-foot-theme:166`, `:195`–`:198`
- **Encoding:** each foot process holds its own palette (dark or light), which is not persisted. The shell sets it with SIGUSR1 (dark) or SIGUSR2 (light) when a terminal opens, when the shell (re)connects and when the theme changes.
- **States:** `Dark` (at start) ⇄ `Light`. Terminals can be out of step with the theme when they predate the shell or survive a shell restart.
- **Transitions:** foot starts dark (`:12`); the shell signals on each of the three events; `check-foot-theme` flips one terminal by hand (`:195`) and expects a shell reload to repair it (`:197`–`:198`).
- **Hazards:** every terminal starts dark and stays wrong until the shell signals it. Any process that sends SIGUSR1 or SIGUSR2 flips it. Clients of a foot server follow the server process.
- **Reify:** Low.
- **Related:** `desktop/aizome/crates/aizome-shell/src/shell.rs:1945`.

<a id="azn-12"></a>

### AZN-12 — Login screen session handoff and X11 session teardown
- **Where:** `desktop/aizome/nix/greeter.nix:26` (`x11Session`, `trap cleanup EXIT` `:33`), `:118` (`default_session`), `:124` (`initial_session`), `:133` (`/var/lib/aizome-greeter`)
- **Encoding:** greetd's initial session (automatic login) and default session (the greeter); the greeter's state directory (last user and each person's session, in `state.toml`); the X11 wrapper's EXIT trap, which stops `nixos-fake-graphical-session.target graphical-session.target` and unsets the session variables (`:30`–`:31`); the greeter's own compositor, started with `--no-session` (`:48`–`:55`). The command is resolved through `/run/current-system` (`:119`), so an update applies at the next greeter start.
- **States:** boot → `InitialSession` (`:124`) | `Greeter` → `UserSession(wayland|x11)` → session ends → `Greeter`. For X11: `Running` → the wrapper exits → `TornDown` (targets stopped, variables unset).
- **Transitions:** greetd starts the greeter or the initial session; the greeter starts the chosen session (X11 through `x11_prefix`, `:42`–`:45`); the wrapper's EXIT trap tears down (`:27`–`:33`).
- **Hazards:** the X11 cleanup stops `graphical-session.target` for the whole user manager, which also stops any other graphical session of the same user, including Aizome's units bound to it. Cleanup only happens if the wrapper reaches its EXIT trap; a SIGKILL of the scope leaves `DISPLAY` and `XDG_*` in the manager.
- **Reify:** Low — declarative; the state lives in greetd and aizome-greeter.
- **Related:** aizome-greeter `desktop/aizome/crates/aizome-greeter/src/app.rs:280` (state save); harness AZN-20.

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

<a id="azn-14"></a>

### AZN-14 — Per-login re-arming of session user units from persisted opt-ins
- **Where:** `desktop/aizome/nix/module.nix:305` (`aizome-tokei-alarms` oneshot), `:316` (Koyomi agent), `:322` (Koe), `:332` (update path), `:336` (Keep Awake), `:89` (nm-applet `ExecCondition` on `XDG_SESSION_DESKTOP`)
- **Encoding:** units wanted by `graphical-session.target` or `aizome-session.target`, gated by conditions that read persisted user state (Koe's `enabled` in `koe.toml`, the Keep Awake file, Tokei's saved alarms) or the user manager's environment (`XDG_SESSION_DESKTOP`, set and unset by the launcher).
- **States:** at each login: `Wanted` → condition yes → `Running`, or condition no → `Skipped`. Logout stops them through `PartOf`. Tokei's transient timers end with the user manager and are created again at the next graphical login by `--sync`.
- **Transitions:** target start pulls the units; `ExecCondition` decides (Koe `desktop/aizome/apps/koe/data/aizome-koe.service:15`, Koyomi `desktop/aizome/apps/koyomi/data/aizome-koyomi-agent.service:18`, nm-applet drop-ins `module.nix:95`–`:103`).
- **Hazards:** no Tokei alarm fires between the user manager ending and the next graphical login. Keeping nm-applet out depends on the launcher having unset `XDG_SESSION_DESKTOP` after an Aizome session (AZN-27 cleanup); if that cleanup is skipped, nm-applet is refused in the next desktop.
- **Reify:** Low — declarative wiring; the state machines live in the apps.
- **Related:** AZN-01; `desktop/aizome/nix/vm-koe-test.py:112`; `desktop/aizome/nix/vm-network-auth-test.py:188`.

<a id="azn-15"></a>

### AZN-15 — (test harness) ScreenCast portal client: request/Response chain and frame count
- **Where:** `desktop/aizome/nix/screencast-check.py:32` (`request`), `:53`–`:59`, `:73`–`:85`
- **Encoding:** each request gets a handle token (`tokens`, `:29`) and its own GLib main loop, which ends on the `Response` signal or a 30 s timeout (`:46`); the session handle is passed from one request to the next.
- **States:** `CreateSession` → `SelectSources` → `Start` (picker shown; the person picks, or refuses and the script exits with response 1, `:48`–`:49`) → `OpenPipeWireRemote` → GStreamer runs for N s (`timeout -s INT`, then KILL, `:75`) → frame count compared with the minimum (`:85`).
- **Hazards:** the 30 s GLib timeouts are never removed, so an earlier request's timer still fires later (harmlessly quitting a loop that already finished). A timeout and a missing Response both report `response None`. The deliberately non-live `pipewiresrc` hides the pts-0 stall that the carried patch fixes (comment `:60`–`:67`).
- **Reify:** Low.
- **Related:** xdg-desktop-portal-wlr (AZN-25); aizome-share-picker; `desktop/aizome/nix/vm-test.nix:320`.

<a id="azn-16"></a>

### AZN-16 — (test harness) real user manager with gated stand-in compositor (launcher systemd test)
- **Where:** `desktop/aizome/nix/session-launcher-systemd-test.py:41` (`start`), `:50` (`terminated`), `:59` (`relogin`); also `desktop/aizome/nix/session-launcher-systemd-test.nix:26` (wrapping `systemctl`, gate `hold-start` `:30`), `:67` (stand-in compositor, gate `hold-ready` `:72`, `READY=1` `:76`), `:56` (FileManager1 stand-in)
- **Encoding:** gate files and marker files under `/home/alice/launcher-test` (`hold-start`, `hold-ready`, `entered-<login>`, `client-<login>`); unit `ActiveState` polled through `systemctl show`.
- **States:** for each case: gate set → launcher started → start client entered, or service activating → signal or duplicate login → service inactive and environment cleared → gate removed → login again → active → terminated.
- **Transitions:** pending-start cases (`desktop/aizome/nix/session-launcher-systemd-test.py:91`–`:103`); submitted-start cases (`desktop/aizome/nix/session-launcher-systemd-test.py:107`–`:114`); duplicate logins (`desktop/aizome/nix/session-launcher-systemd-test.py:119`–`:137`); unowned services (`desktop/aizome/nix/session-launcher-systemd-test.py:140`–`:151`).
- **Hazards:** 10 s polling deadlines (`:35`). A failed case can leave gates set; the `finally` block kills the launchers and starts the shutdown target (`desktop/aizome/nix/session-launcher-systemd-test.py:152`–`:160`).
- **Reify:** Low.
- **Related:** AZN-27, AZN-28.

<a id="azn-17"></a>

### AZN-17 — (test harness) fake systemctl driven by marker files (launcher unit test)
- **Where:** `desktop/aizome/nix/session-launcher-test.py:15` (`SYSTEMCTL`), `:159` (`check_pending_start`), `:185` (signal injected into the fork/PID-capture window), `:202` (`check_duplicate_gate`), `:233` (watchdog restart limit)
- **Encoding:** files in a temporary directory stand for unit state (`active`, `activating`, `ready`, `start-entered`, `client-pid`, `cleanup-entered`) and gates (`hold-start`, `hold-ready`, `hold-cleanup`); a `calls` log; `TEST_RESULT` selects the reported Result.
- **States:** `inactive` → `start-entered` (gate `:27`) → `activating` (`:29`, gate `:30`) → `active`+`ready` (`:32`) → exit 7 once `active` is removed (`:37`–`:38`), or an immediate exit for the watchdog case (`:33`–`:36`). Shutdown clears `active` and `activating` (`:44`).
- **Hazards:** 3 s deadlines. The launcher's source is patched by string replacement to force the fork window (`:189`–`:196`), which would test nothing if that line moved; `assertIn` (`:191`) guards against this.
- **Reify:** Low.
- **Related:** AZN-27.

<a id="azn-18"></a>

### AZN-18 — (test harness) disposable-device fixtures behind VM marker files and case modes
- **Where:** `desktop/aizome/nix/vm-encrypted-volumes-test.py:98` (`seed`), `:73` (`authenticate_if_needed`), `:130`, `:142`–`:143`; also `desktop/aizome/nix/vm-encrypted-volumes-test.nix:39`–`:40`, `desktop/aizome/nix/encrypted-volumes-fixture.nix:7`, `desktop/aizome/nix/vm-loop-images-test.nix:11`, `desktop/aizome/nix/loop-images-fixture.nix:7`, `desktop/aizome/nix/vm-tansu-resize-test.nix:2`, `:17`, `:28`, `desktop/aizome/nix/vm-tansu-resize-test.py:62`, `:118`, `:141`, `:170`
- **Encoding:** `/etc/aizome-*-test*` marker files that destructive fixtures check before touching disks; a case selector `/etc/aizome-encryption-case` (`all` | `locked`); an `expect_failure` flag (the negative control) prepended to the script.
- **States:** encrypted volumes: `seeded` (three LUKS disks; the visible one unlocked and mounted) → [`all`: the unlocked case] → `locked` → prompt → `cancelled` → `wrong passphrase` → `unlocked+mounted` → file operations → relaunch. Tansu: `seeded` → mounted with data → unmounted → grow → (negative control: failure shown, then cleared) → mounted check → shrink.
- **Hazards:** `authenticate_if_needed` polls for a fixed window (30×0.1 s at `:73`; 20×0.2 s at `vm-tansu-resize-test.py:62`) and carries on without authenticating if the polkit dialog is slower. Device identity is checked by serial and size before formatting (`:99`–`:104`); the marker and these checks are the only safety interlock.
- **Reify:** Low.
- **Related:** aizome-sys `desktop/aizome/crates/aizome-sys/tests/disks_real.rs:11`, `desktop/aizome/crates/aizome-sys/tests/disks_encrypted_real.rs:8`.

<a id="azn-19"></a>

### AZN-19 — (test harness) kit-inspection UI driver shared by the component VM tests
- **Where:** `desktop/aizome/nix/vm-file-panel-multiple-test.py:60` (`until`), `:35` (`elements` filtered by the live pid, `:40`); the same pattern at `desktop/aizome/nix/vm-fusen-save-test.py:37`, `desktop/aizome/nix/vm-koe-test.py:27`, `desktop/aizome/nix/vm-koyomi-modal-test.py:31`, `desktop/aizome/nix/vm-myaku-grouping-test.py:25`, `desktop/aizome/nix/vm-shashin-modal-test.py:31`, `desktop/aizome/nix/vm-shashin-wallpaper-test.py:32`, `desktop/aizome/nix/vm-shiori-test.py:31`, `desktop/aizome/nix/vm-standard-menu-test.py:25`, `desktop/aizome/nix/vm-sumi-search-test.py:29`, `desktop/aizome/nix/vm-tansu-size-test.py:31`, `desktop/aizome/nix/vm-tansu-resize-test.py:46`, `desktop/aizome/nix/vm-tenki-live-search-test.py:59`, `desktop/aizome/nix/vm-terminal-shortcuts-test.py:32`, `desktop/aizome/nix/vm-tokei-reorder-test.py:27`, `desktop/aizome/nix/vm-utsushi-test.py:33`, `desktop/aizome/nix/vm-wallpaper-choice-test.py:27`, each with its `.nix` (greetd `initial_session` automatic login, an `AIZOME_KIT_INSPECT` directory)
- **Encoding:** poll-until-deadline loops; inspection JSON (`{APP}.{pid}.*.json`) filtered to the live window's pid, so documents left by a process that quit are ignored; dismissal of the first-run tour; `inputs` and `observations` logs; fixed sleeps after each input.
- **States:** VM boot → automatic login → shell up → tour closed → app spawned → window → painted document → act/observe → quit → no core dumps.
- **Hazards:** results depend on the fixed sleeps after input (0.15–0.3 s) and on the app having painted a fresh document. The wallpaper test waits out a six-second status message (`vm-shashin-wallpaper-test.py:84`). The Koe test reads the unit's `ConditionResult` (`vm-koe-test.py:112`, `:163`). The terminal test depends on a one-time migration marker (`vm-terminal-shortcuts-test.py:70`).
- **Reify:** Low — a shared helper module would replace 17 copies.
- **Related:** the kit's inspection writer (aizome-kit); harnesses AZN-18, AZN-21, AZN-22, AZN-23 use the same loop.

<a id="azn-20"></a>

### AZN-20 — (test harness) login/logout cycles with a lingering user manager
- **Where:** `desktop/aizome/nix/vm-greeter-test.nix:131` (`greeter_up`), `:138` (`session_of`), `:142` (`log_out`, through logind `:153` or from the user manager `:151`), `:228` (`state.toml`); also `desktop/aizome/nix/vm-greeter-x11-test.nix:45`, `:78`, `:82`, `desktop/aizome/nix/vm-koyomi-session-test.py:33` (`no_session`), `:48` (`login`, restarting the display manager with an AccountsService session), `desktop/aizome/nix/vm-tatami-compat-test.nix:13`–`:14` (`/run/compatibility-session` picks the next session), `desktop/aizome/nix/vm-tatami-compat-test.py:177`–`:178`, `:349`–`:350`
- **Encoding:** users with `linger = true`, so the user manager outlives each logout; greeter documents deleted between logins (`vm-greeter-test.nix:165`); a marker file choosing Aizome or Tatami for the next greetd login.
- **States:** `greeter` → `session` (Aizome, sway, X11, Xfce, GNOME or Tatami) → logout (logind terminate, the desktop's own Logout, `aizomectl quit`, `systemctl --user stop aizome.service`, `uwsm stop`) → manager alive, `graphical-session.target` inactive, environment cleared → next login.
- **Hazards:** "greeter up" is detected by the reappearance of deleted documents. The greeter's compositor socket is picked by the greeter's current pid, because a killed compositor leaves stale sockets behind (`vm-greeter-test.nix:72`–`:76`).
- **Reify:** Low.
- **Related:** AZN-12, AZN-27, AZN-28.

<a id="azn-21"></a>

### AZN-21 — (test harness) virtual radios, VPN server and NetworkManager activation driver
- **Where:** `desktop/aizome/nix/vm-network-auth-test.py:100` (`activate`: background `nmcli` that writes `/tmp/auth-result`), `:114` (`finished`), `:75` (`answer`), `:93` (`cancel_prompt`), `:296` (NetworkManager restart and re-registration), `:25` (`skips`, counted in the journal); also `desktop/aizome/nix/vm-network-auth-test.nix:127` (certificates oneshot), `:135` (hostapd), `:144` (dnsmasq), `:163` (OpenVPN server)
- **Encoding:** a result file marks completion of the background activation; counts of journal lines serve as event counters (registrations, saves, `ExecCondition` skips); `Requires`/`After` chains order the fixture units.
- **States:** for each scenario: `activate` → dialog shown → answered, cancelled, or cancelled by NetworkManager (device disconnect, `desktop/aizome/nix/vm-network-auth-test.py:216`) → result file → `down`. Restarting NetworkManager increases the registration count.
- **Hazards:** journal counts include lines from earlier scenarios. The activation wait is 110 s (`:118`). `network-test-ap` waits for `wlan0`'s device unit but not for `wlan2`/`wlan3` (`desktop/aizome/nix/vm-network-auth-test.nix:139`).
- **Reify:** Low.
- **Related:** AZN-03, AZN-04, AZN-05; `desktop/aizome/nix/vm-tatami-compat-test.nix:12` builds on this test.

<a id="azn-22"></a>

### AZN-22 — (test harness) polkit outage and recovery across system switches
- **Where:** `desktop/aizome/nix/vm-polkit-recovery-test.py:84` (`propose`), `:77` (`authenticate`), `:119` (polkit restarted while its dialog is open), `:134`/`:144` (shell stopped and continued), `:110`/`:126`/`:142`/`:150` (checks on the helper-start log); also `desktop/aizome/nix/vm-polkit-recovery-test.nix:35` (fixture helper appending to `/var/lib/qa-helper-starts` `:44` and switching to a specialisation `:45`–`:46`), `:93` (specialisations)
- **Encoding:** an append-only log counting privileged helper runs; specialisations as target system states; the shell process stopped with SIGSTOP to stand for an absent polkit agent.
- **States:** `none` → `first` (authenticated) → `second` → shell reload → propose `first` → polkit restart mid-dialog → dialog replaced or closed → agent paused → "Try Again" → agent resumed → authenticated → `first`.
- **Hazards:** the fixture helper accepts only exact argument strings (`desktop/aizome/nix/vm-polkit-recovery-test.nix:39`–`:42`). The sequence assumes polkit's restart closes the old dialog within the 40 s deadline (`desktop/aizome/nix/vm-polkit-recovery-test.py:23`).
- **Reify:** Low.
- **Related:** aizome-shell polkit agent `desktop/aizome/crates/aizome-shell/src/auth.rs`; Yukimi's helper.

<a id="azn-23"></a>

### AZN-23 — (test harness) Settings driver and ordered parts sharing system state
- **Where:** `desktop/aizome/nix/vm-settings-test.py:100` (`class Settings`, `pid` `:118`), `:123` (`open`, which asserts Settings isn't running `:125`), `:136` (`quit`), `:386` (`TESTS`), `:1753` (`ONLY`), `:1762` (run loop); Keep Awake part `:713` (holder killed `:741`, its replacement awaited `:742`)
- **Encoding:** `S.pid` (None = Settings closed, int = the open process); parts registered in order, each putting back what it changed (theme back to Night, keyboard back to US, lock back to Never, and so on); the `AIZOME_TEST_ONLY` and `AIZOME_TEST_PROBE` filters.
- **States:** `closed` → `open(pane)` → interacting → `reopen` (quit and open again to check persistence) → `closed`. Across parts, system state goes `default` → changed → restored.
- **Hazards:** parts depend on earlier parts having restored shared state (the rfkill ACL `:1501`/`:1518`, the time-zone link `:602`–`:606`). A failure in the middle of a part, or an `ONLY` subset, changes the starting state of later parts. `S.pid` goes stale if Settings crashes.
- **Reify:** Low.
- **Related:** AZN-01, AZN-02 (Keep Awake part), AZN-09 (search part).

<a id="azn-24"></a>

### AZN-24 — (test harness) session smoke test: reload, recording, sharing and watchdog restart
- **Where:** `desktop/aizome/nix/vm-test.nix:82` (`until`), `:157` (reload), `:233` (recording: phase poll `:257`, stop `:282`–`:283`), `:320` (`share`: events file and background portal client `:326`–`:338`), `:393` (stall: `debug-stall 60` `:402`, MainPID change `:404`, `stall.log` `:409`)
- **Encoding:** fields of `aizome-shell msg state` (`recording.phase`, `sharing`); an events log searched for `pick-started`; the compositor's MainPID before and after.
- **States:** session up → reload swaps the logic and shell (same compositor) → locked → unlocked → recording `phase: recording` → stopped (`recording: null`) → share picked → streaming → share ended → compositor stalled → stopped by the watchdog → restarted by the launcher → recovery status "stopped responding".
- **Hazards:** the watchdog case takes at least 30 s and checks only one restart. The recording check waits up to 30×1 s for 5 s of recording.
- **Reify:** Low.
- **Related:** AZN-27 (watchdog restart loop), AZN-25, AZN-15.

<a id="azn-25"></a>

### AZN-25 — xdg-desktop-portal-wlr screencast capture lifecycle (carried patch)
- **Where:** `desktop/aizome/nix/xdg-desktop-portal-wlr-captures.patch:76` (adds `capture_timer`), `:181` (`xdpw_wlr_frame_capture_cancel`), `:160` (`wlr_frame_capture_timer`), `:256`–`:281` (stream state handler, final form), `:121`–`:125` (`remove_buffer`), `:27`/`:149` (destroy clears the timer pointers), `:208` (records the wlr frame); applied by `desktop/aizome/nix/module.nix:54`
- **Encoding:** per cast: the `pwr_stream_state` bool; `current_frame.pw_buffer`/`xdpw_buffer` (a buffer kept dequeued); `capture_timer` (a delayed capture is scheduled); `process_retry` (retry while starved of buffers); `ext_session.frame` or `wlr_session.wlr_frame` (a frame in flight).
- **States:** `Paused(no buffer)` | `Paused(kept buffer, capture cancelled)` → `Streaming` → `CaptureScheduled(capture_timer)` → `CaptureInFlight(frame)` → ready → buffer queued → next capture; `Starved(process_retry)`. When the current buffer is removed, the capture is cancelled and the buffer forgotten. `Destroyed`: timers freed and their pointers cleared.
- **Transitions:** entering STREAMING with a kept buffer → capture (`desktop/aizome/nix/xdg-desktop-portal-wlr-captures.patch:260`–`:262`); without one → the process handler (`:263`–`:264`). Leaving STREAMING → retry disarmed and capture cancelled, buffer kept (`:268`–`:275`). `remove_buffer` of the current buffer → cancel and clear (`:121`–`:125`). The timer fires → `capture_timer` cleared, then the capture starts (`:163`–`:164`). Cancel destroys the timer and the frame (`:182`–`:194`). Destroy clears `process_retry` and `capture_timer` (`desktop/aizome/nix/xdg-desktop-portal-wlr-captures.patch:27`, `:148`–`:149`).
- **Hazards:** the rule of at most one live timer or frame per cast assumes `xdpw_wlr_frame_capture` is never entered while a capture is pending. If it were, the armed timer's pointer would be overwritten (`:171`), and that timer would later fire on whatever buffer was current. A kept buffer stays dequeued through UNCONNECTED and ERROR until PipeWire removes it. The patch is carried until upstream takes equivalent fixes.
- **Reify:** High — five fields spread over three source files, with invalid combinations (a kept buffer while the stream flag is false, a timer and a frame alive together) that the patch exists to rule out.
- **Related:** Aizome's compositor capture protocols `desktop/aizome/crates/aizome-comp-core/src/protocols/image_copy_capture.rs`, `desktop/aizome/crates/aizome-comp-core/src/protocols/screencopy.rs`; AZN-15.

<a id="azn-26"></a>

### AZN-26 — (test harness) private headless session drivers (development scripts)
- **Where:** `desktop/aizome/scripts/check-foot-theme:53` (`Session`), `:97` (`start`), `:127` (`close`), `:26` (`probe`, an OSC 11 loop); also `desktop/aizome/scripts/headless-session:39`–`:46`, `:55`, `desktop/aizome/scripts/test-startup.py:105`–`:111`, `:143`–`:152`, `desktop/aizome/scripts/validate-installed:74`–`:78`, `:91`
- **Encoding:** lists of child processes; the appearance of a socket file as the readiness signal; `trap … EXIT` kills; colour probe files.
- **States:** spawn the compositor → its socket exists → (spawn the shell → shell socket exists) → drive → terminate (SIGTERM to the process group, 5 s, then SIGKILL).
- **Hazards:** the socket loops in `validate-installed` (`:78`, `:91`) and `headless-session` (`:42`–`:46`) end after a fixed number of tries without failing, so the scripts carry on if the socket never appears. The `check-foot-theme` probe re-queries until it is killed.
- **Reify:** Low.

<a id="azn-27"></a>

### AZN-27 — Session launcher phases (`aizome-session`)
- **Where:** `desktop/aizome/session/aizome-session:17` (login-shell re-exec, sentinel `--no-login-shell` `:24`), `:63` (manager state), `:70`–`:71` (session lock), `:77` (service state), `:90` (`cleanup`), `:104`–`:105` (`wait_pid`, `pending_signal`), `:118` (`restarts`), `:119` (restart loop)
- **Encoding:** the sentinel argument; `state` (from `is-system-running`, then `ActiveState`); an flock on fd 9 for `$XDG_RUNTIME_DIR/aizome-session.lock` (kept, never unlinked); `wait_pid`; `pending_signal`; which traps are installed (deferring or exiting); `restarts`, `started` (wall clock) and `result`; the exported `AIZOME_ENVIRONMENT_KEYS="1 …"` record, versioned and read back from `/proc/PID/environ` (`:59`).
- **States:** `LoginShell?` → `EnvLoaded` → `NoManager` (exec `dbus-run-session aizome`, `:148`–`:154`) | `Locked` → `Refused` (lock held `:71`–`:74`, or the service is neither inactive nor failed `:78`–`:81`) | `Prepared` (traps, reset-failed, environment imported, `:106`–`:114`) → `Starting` (signals deferred, `:126`–`:129`) → `Waiting` (`:131`–`:135`) → `Exited` → `WatchdogRestart` (Result=watchdog, with fewer than 3 restarts or a run of at least 300 s, `:138`–`:144`) → `Starting` | `Cleanup` (EXIT trap: kill and reap the start client `:94`–`:99`, start the shutdown target `:100`, unset the environment `:102`) → exit with the status.
- **Transitions:** `exec 9>` and `flock -n` (`:70`–`:71`); `trap 'cleanup' EXIT` (`:106`); background start (`:129`) with `wait_pid` captured (`:130`); pending signal re-raised (`:134`); `result` read (`:138`); count reset (`:140`); limit (`:141`); `reset-failed aizome.service` (`:144`); exit (`:146`).
- **Hazards:** the restart window is measured with `date +%s` (`:120`, `:140`), so a clock step (NTP at boot, or Settings' Date & Time) can reset or stretch the five minutes wrongly. The first `reset-failed` (`:110`) clears the failed state of every user unit, not only Aizome's. Any manager state other than running, degraded or starting (initializing, stopping, unreachable) runs the compositor outside systemd, with no session target, credential agent or Keep Awake (`:148`–`:154`). If the launcher is SIGKILLed mid-cleanup (for example by the scope's stop timeout), the environment is never unset, and a stale `XDG_SESSION_DESKTOP=aizome` keeps nm-applet out of the next desktop (`desktop/aizome/nix/module.nix:89`–`:93`). A launcher killed while its service survives turns later logins away ("already running") until that service stops.
- **Reify:** High — eight phases encoded in shell variables, traps and a lock file, with signal-safety invariants that the tests can only check by forcing instruction-sized windows.
- **Related:** AZN-28 (units), AZN-16, AZN-17 (tests); Yukimi `apps/yukimi/crates/yukimi-system/src/completion/aizome.rs` (reads `AIZOME_ENVIRONMENT_KEYS`).

<a id="azn-28"></a>

### AZN-28 — Session unit graph: compositor binding, session and shutdown targets, service restart policies
- **Where:** `desktop/aizome/session/aizome-session.target:7` (`BindsTo=graphical-session.target`), `:10`–`:13`; also `desktop/aizome/session/aizome.service:14`–`:19`, `:23`, `:29`, `:34`, `:36`, `:40`, `desktop/aizome/session/aizome-shutdown.target:5`–`:8`, `desktop/aizome/session/aizome-file-manager.service:3`–`:6`, `:10`–`:14`, `desktop/aizome/session/aizome-network-agent.service:3`–`:6`, `:12`–`:13`, `desktop/aizome/session/aizome-portal.service:3`, `:6`, `:10`–`:13`, `desktop/aizome/session/dbus/org.freedesktop.impl.portal.desktop.aizome.service:4`, `desktop/aizome/session/aizome-keep-awake.service:10`–`:11`, `:25`–`:26`
- **Encoding:** systemd unit states and dependencies; compositor readiness via `sd_notify`; the watchdog; `Result=watchdog`, which the launcher reads; `PartOf` and `BindsTo` chains; D-Bus activation for the portal backend.
- **States:** `pre` → `aizome.service` activating (notify, 60 s timeout) → active (then `graphical-session.target` and `aizome-session.target` with what they want: credential agent, file manager, autostart, and through the module Keep Awake, Koe and the update path) → the compositor stops: if by the watchdog, the session units stay up while the launcher restarts it; otherwise the launcher starts `aizome-shutdown.target`, whose `Conflicts` stop the targets and with them the `PartOf` units, and `StopWhenUnneeded` then makes the shutdown target inactive again.
- **Transitions:** `Wants`/`Before` from `aizome.service` (`desktop/aizome/session/aizome.service:18`–`:19`); `WatchdogSec=30s` and `TimeoutAbortSec=5s` (`desktop/aizome/session/aizome.service:34`, `desktop/aizome/session/aizome.service:36`); `Restart=no` (`desktop/aizome/session/aizome.service:29`); `Conflicts` in the shutdown target (`desktop/aizome/session/aizome-shutdown.target:7`); `Restart=on-failure` with `RestartSec=2` for the file manager, credential agent and portal.
- **Hazards:** the restart policy is split: `aizome.service` has `Restart=no` and the launcher does the restarting, so a compositor started without the launcher gets neither restarts nor teardown. `aizome-session.target` is bound only to `graphical-session.target`, so session units outlive a compositor stall and must cope with no Wayland server (Keep Awake loops; the agent's dialogs fail). The file manager and credential agent use the default start limit with `RestartSec=2`, so five failures within about 10 s leave them failed for the rest of the session; FileManager1 has no D-Bus activation file to bring it back. The portal backend is `PartOf=graphical-session.target`, not `aizome-session.target`.
- **Reify:** Medium — declarative; the implicit part is the split restart policy and cross-unit invariants that are documented only in comments.
- **Related:** AZN-27; `desktop/aizome/nix/session-launcher-systemd-test.nix`.

<a id="azn-29"></a>

### AZN-29 — Post-update reload offer (path unit and a waiting oneshot)
- **Where:** `desktop/aizome/session/aizome-update.path:13`–`:14`; also `desktop/aizome/session/aizome-update.service:8`, `:10`–`:11`, `desktop/aizome/nix/module.nix:332`
- **Encoding:** a path unit watching `/run/current-system`, and a oneshot that stays activating for as long as the offer notification is shown (`TimeoutStartSec=infinity`).
- **States:** `Watching` → switch → `Offering` (service activating, notification shown) → reload chosen or dismissed → `Watching`. Logout stops both (`PartOf`).
- **Transitions:** a change of `/run/current-system` starts the oneshot (`PathChanged=`, `desktop/aizome/session/aizome-update.path:13`–`:14`), Watching→Offering; `aizomectl reload --offer` returns when the person reloads or dismisses the notification (`desktop/aizome/session/aizome-update.service:10`–`:11`), Offering→Watching; ending the session stops both units (`PartOf=`, `desktop/aizome/session/aizome-update.path:9`, `desktop/aizome/session/aizome-update.service:5`)
- **Hazards:** a switch made while an offer is still showing starts no second run, because the unit is already activating; the open offer then describes the earlier generation unless `aizomectl` reads the system again at click time. An offer left open blocks later offers indefinitely.
- **Reify:** Low.
- **Related:** aizomectl `desktop/aizome/crates/aizome-ipc/src/bin/aizomectl.rs:74` (`reload --offer`).

## Already explicit

- None in this slice.

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/crates/aizome-awake/src/lib.rs` | 128 | machines | [AZN-01](#azn-01) | setting file vs logind status |
| `desktop/aizome/crates/aizome-awake/src/main.rs` | 273 | machines | [AZN-01](#azn-01), [AZN-02](#azn-02) | set poll loop and hold service |
| `desktop/aizome/crates/aizome-network-agent/build.rs` | 10 | none |  | build script |
| `desktop/aizome/crates/aizome-network-agent/src/keyring.c` | 72 | machines | [AZN-05](#azn-05) | one-shot libsecret call in killable worker |
| `desktop/aizome/crates/aizome-network-agent/src/keyring.rs` | 132 | machines | [AZN-05](#azn-05) | keyring worker invocation |
| `desktop/aizome/crates/aizome-network-agent/src/lib.rs` | 407 | machines | [AZN-03](#azn-03), [AZN-04](#azn-04) | request registry and NM registration |
| `desktop/aizome/crates/aizome-network-agent/src/main.rs` | 39 | machines | [AZN-05](#azn-05) | worker mode dispatch |
| `desktop/aizome/crates/aizome-network-agent/src/model.rs` | 231 | none |  | pure profile and flag logic |
| `desktop/aizome/crates/aizome-network-agent/src/process.rs` | 161 | machines | [AZN-05](#azn-05), [AZN-03](#azn-03) | worker run loop and Cancel token |
| `desktop/aizome/crates/aizome-network-agent/src/prompt.rs` | 99 | machines | [AZN-05](#azn-05) | dialog worker answer |
| `desktop/aizome/crates/aizome-network-agent/src/vpn.rs` | 201 | machines | [AZN-05](#azn-05) | VPN helper exchange via worker |
| `desktop/aizome/crates/aizome-network-agent/tests/credentials.rs` | 750 | machines | [AZN-06](#azn-06) | fake AgentManager harness |
| `desktop/aizome/crates/aizome-power-meter/src/main.rs` | 208 | machines | [AZN-07](#azn-07) | socket activation and throttle |
| `desktop/aizome/crates/aizome-search/examples/index_home.rs` | 30 | none |  | benchmark example |
| `desktop/aizome/crates/aizome-search/src/calc.rs` | 598 | none |  | pure parser |
| `desktop/aizome/crates/aizome-search/src/calc/int.rs` | 532 | none |  | pure parser |
| `desktop/aizome/crates/aizome-search/src/files.rs` | 444 | machines | [AZN-08](#azn-08) | narrowing cache and saved index |
| `desktop/aizome/crates/aizome-search/src/frecency.rs` | 225 | machines | [AZN-09](#azn-09) | history file staleness and RMW |
| `desktop/aizome/crates/aizome-search/src/lib.rs` | 281 | none |  | matcher scratch and pure ranking |
| `desktop/aizome/crates/aizome-search/src/recent.rs` | 137 | none |  | pure xbel parsing |
| `desktop/aizome/crates/aizome-shell-proto/src/lib.rs` | 422 | machines | [AZN-10](#azn-10) | shell socket protocol |
| `desktop/aizome/etc/foot/foot.ini` | 92 | machines | [AZN-11](#azn-11) | palette set by shell signals |
| `desktop/aizome/icons/genicons.py` | 270 | none |  | icon generator |
| `desktop/aizome/justfile` | 10 | none |  | dev recipe |
| `desktop/aizome/nix/aizome-fonts.nix` | 33 | none |  | font bundle |
| `desktop/aizome/nix/cargo-output-hashes.nix` | 9 | none |  | data table |
| `desktop/aizome/nix/default.nix` | 224 | none |  | flake outputs wiring |
| `desktop/aizome/nix/encrypted-volumes-fixture.nix` | 17 | machines | [AZN-18](#azn-18) | fixture binary build |
| `desktop/aizome/nix/fonts.nix` | 45 | none |  | font package |
| `desktop/aizome/nix/greeter.nix` | 135 | machines | [AZN-12](#azn-12) | greetd handoff and X11 teardown |
| `desktop/aizome/nix/hotaru-fonts.nix` | 40 | none |  | font bundle |
| `desktop/aizome/nix/hotaru.nix` | 144 | none |  | build recipe |
| `desktop/aizome/nix/loop-images-fixture.nix` | 18 | machines | [AZN-18](#azn-18) | fixture binary build |
| `desktop/aizome/nix/module.nix` | 635 | machines | [AZN-13](#azn-13), [AZN-14](#azn-14), [AZN-01](#azn-01), [AZN-07](#azn-07), [AZN-25](#azn-25) | time zone, per-login units, meter socket, portal patch |
| `desktop/aizome/nix/package.nix` | 363 | none |  | build recipe; build-env.json static metadata |
| `desktop/aizome/nix/screencast-check.py` | 87 | machines | [AZN-15](#azn-15) | portal request chain harness |
| `desktop/aizome/nix/session-launcher-systemd-test.nix` | 88 | machines | [AZN-16](#azn-16) | stand-ins and gates |
| `desktop/aizome/nix/session-launcher-systemd-test.py` | 160 | machines | [AZN-16](#azn-16) | user-manager launcher driver |
| `desktop/aizome/nix/session-launcher-test.nix` | 15 | machines | [AZN-17](#azn-17) | runs shellcheck and harness |
| `desktop/aizome/nix/session-launcher-test.py` | 241 | machines | [AZN-17](#azn-17) | fake systemctl marker files |
| `desktop/aizome/nix/timezone-policy-test.nix` | 34 | none |  | tests (eval assertions) |
| `desktop/aizome/nix/vm-encrypted-volumes-test.nix` | 77 | machines | [AZN-18](#azn-18) | marker and case selector |
| `desktop/aizome/nix/vm-encrypted-volumes-test.py` | 216 | machines | [AZN-18](#azn-18), [AZN-19](#azn-19) | LUKS fixture phases |
| `desktop/aizome/nix/vm-file-panel-multiple-test.nix` | 59 | machines | [AZN-19](#azn-19) | generic VM fixture |
| `desktop/aizome/nix/vm-file-panel-multiple-test.py` | 194 | machines | [AZN-19](#azn-19) | generic inspection driver |
| `desktop/aizome/nix/vm-fusen-save-test.nix` | 44 | machines | [AZN-19](#azn-19) | generic VM fixture |
| `desktop/aizome/nix/vm-fusen-save-test.py` | 293 | machines | [AZN-19](#azn-19) | generic inspection driver |
| `desktop/aizome/nix/vm-greeter-test.nix` | 310 | machines | [AZN-20](#azn-20) | login/logout cycles |
| `desktop/aizome/nix/vm-greeter-x11-test.nix` | 102 | machines | [AZN-20](#azn-20) | X11 logout with lingering manager |
| `desktop/aizome/nix/vm-koe-test.nix` | 44 | machines | [AZN-19](#azn-19) | generic VM fixture |
| `desktop/aizome/nix/vm-koe-test.py` | 166 | machines | [AZN-19](#azn-19) | generic driver plus unit condition |
| `desktop/aizome/nix/vm-koyomi-modal-test.nix` | 52 | machines | [AZN-19](#azn-19) | generic VM fixture |
| `desktop/aizome/nix/vm-koyomi-modal-test.py` | 195 | machines | [AZN-19](#azn-19) | generic inspection driver |
| `desktop/aizome/nix/vm-koyomi-session-test.nix` | 47 | machines | [AZN-20](#azn-20) | lingering manager fixture |
| `desktop/aizome/nix/vm-koyomi-session-test.py` | 132 | machines | [AZN-20](#azn-20) | cross-desktop login cycles |
| `desktop/aizome/nix/vm-loop-images-test.nix` | 31 | machines | [AZN-18](#azn-18) | marker-gated loop fixture |
| `desktop/aizome/nix/vm-myaku-grouping-test.nix` | 48 | machines | [AZN-19](#azn-19) | generic VM fixture |
| `desktop/aizome/nix/vm-myaku-grouping-test.py` | 219 | machines | [AZN-19](#azn-19) | generic inspection driver |
| `desktop/aizome/nix/vm-network-auth-test.nix` | 189 | machines | [AZN-21](#azn-21) | radio and VPN fixture units |
| `desktop/aizome/nix/vm-network-auth-test.py` | 413 | machines | [AZN-21](#azn-21) | NM activation driver |
| `desktop/aizome/nix/vm-nisshi-test.nix` | 37 | none |  | tests |
| `desktop/aizome/nix/vm-nisshi-test.py` | 59 | none |  | tests (assertion sequence) |
| `desktop/aizome/nix/vm-polkit-recovery-test.nix` | 120 | machines | [AZN-22](#azn-22) | helper log and specialisations |
| `desktop/aizome/nix/vm-polkit-recovery-test.py` | 151 | machines | [AZN-22](#azn-22) | polkit outage driver |
| `desktop/aizome/nix/vm-power-meter-test.nix` | 95 | machines | [AZN-07](#azn-07) | stand-in meter with throttle |
| `desktop/aizome/nix/vm-settings-test.nix` | 257 | machines | [AZN-23](#azn-23) | Settings VM fixture |
| `desktop/aizome/nix/vm-settings-test.py` | 1780 | machines | [AZN-23](#azn-23) | Settings driver and ordered parts |
| `desktop/aizome/nix/vm-shashin-modal-test.nix` | 55 | machines | [AZN-19](#azn-19) | generic VM fixture |
| `desktop/aizome/nix/vm-shashin-modal-test.py` | 176 | machines | [AZN-19](#azn-19) | generic inspection driver |
| `desktop/aizome/nix/vm-shashin-wallpaper-test.nix` | 78 | machines | [AZN-19](#azn-19) | generic VM fixture |
| `desktop/aizome/nix/vm-shashin-wallpaper-test.py` | 230 | machines | [AZN-19](#azn-19) | generic inspection driver |
| `desktop/aizome/nix/vm-shiori-test.nix` | 75 | machines | [AZN-19](#azn-19) | generic VM fixture |
| `desktop/aizome/nix/vm-shiori-test.py` | 145 | machines | [AZN-19](#azn-19) | generic inspection driver |
| `desktop/aizome/nix/vm-standard-menu-test.nix` | 37 | machines | [AZN-19](#azn-19) | generic VM fixture |
| `desktop/aizome/nix/vm-standard-menu-test.py` | 158 | machines | [AZN-19](#azn-19) | generic inspection driver |
| `desktop/aizome/nix/vm-sumi-search-test.nix` | 46 | machines | [AZN-19](#azn-19) | generic VM fixture |
| `desktop/aizome/nix/vm-sumi-search-test.py` | 169 | machines | [AZN-19](#azn-19) | generic inspection driver |
| `desktop/aizome/nix/vm-tansu-resize-test.nix` | 29 | machines | [AZN-18](#azn-18) | marker and negative-control mode |
| `desktop/aizome/nix/vm-tansu-resize-test.py` | 204 | machines | [AZN-18](#azn-18), [AZN-19](#azn-19) | resize fixture phases |
| `desktop/aizome/nix/vm-tansu-size-test.nix` | 44 | machines | [AZN-19](#azn-19) | generic VM fixture |
| `desktop/aizome/nix/vm-tansu-size-test.py` | 158 | machines | [AZN-19](#azn-19) | generic inspection driver |
| `desktop/aizome/nix/vm-tatami-compat-test.nix` | 37 | machines | [AZN-20](#azn-20), [AZN-21](#azn-21) | session marker over network fixture |
| `desktop/aizome/nix/vm-tatami-compat-test.py` | 368 | machines | [AZN-20](#azn-20) | Aizome/Tatami login cycles |
| `desktop/aizome/nix/vm-tenki-live-search-test.nix` | 52 | machines | [AZN-19](#azn-19) | generic VM fixture |
| `desktop/aizome/nix/vm-tenki-live-search-test.py` | 162 | machines | [AZN-19](#azn-19) | generic inspection driver |
| `desktop/aizome/nix/vm-terminal-shortcuts-test.nix` | 105 | machines | [AZN-19](#azn-19) | generic VM fixture |
| `desktop/aizome/nix/vm-terminal-shortcuts-test.py` | 184 | machines | [AZN-19](#azn-19) | generic inspection driver |
| `desktop/aizome/nix/vm-test.nix` | 421 | machines | [AZN-24](#azn-24) | session smoke driver |
| `desktop/aizome/nix/vm-tokei-reorder-test.nix` | 44 | machines | [AZN-19](#azn-19) | generic VM fixture |
| `desktop/aizome/nix/vm-tokei-reorder-test.py` | 189 | machines | [AZN-19](#azn-19) | generic inspection driver |
| `desktop/aizome/nix/vm-utsushi-test.nix` | 84 | machines | [AZN-19](#azn-19) | generic VM fixture |
| `desktop/aizome/nix/vm-utsushi-test.py` | 211 | machines | [AZN-19](#azn-19) | generic inspection driver |
| `desktop/aizome/nix/vm-wallpaper-choice-test.nix` | 67 | machines | [AZN-19](#azn-19) | generic VM fixture |
| `desktop/aizome/nix/vm-wallpaper-choice-test.py` | 206 | machines | [AZN-19](#azn-19) | generic inspection driver |
| `desktop/aizome/nix/xdg-desktop-portal-wlr-captures.patch` | 285 | machines | [AZN-25](#azn-25) | capture lifecycle patch |
| `desktop/aizome/scripts/check-foot-theme` | 209 | machines | [AZN-26](#azn-26), [AZN-11](#azn-11) | headless session driver |
| `desktop/aizome/scripts/headless-session` | 56 | machines | [AZN-26](#azn-26) | headless session driver |
| `desktop/aizome/scripts/just-shell` | 10 | none |  | dev-shell re-exec guard |
| `desktop/aizome/scripts/rustc-wrapper` | 16 | none |  | per-crate flag injection |
| `desktop/aizome/scripts/test-startup.py` | 156 | machines | [AZN-26](#azn-26) | headless session driver |
| `desktop/aizome/scripts/validate-installed` | 154 | machines | [AZN-26](#azn-26) | headless session driver |
| `desktop/aizome/session/aizome-file-manager.service` | 17 | machines | [AZN-28](#azn-28) | session unit |
| `desktop/aizome/session/aizome-keep-awake.service` | 27 | machines | [AZN-02](#azn-02), [AZN-01](#azn-01) | restart loop and condition |
| `desktop/aizome/session/aizome-network-agent.service` | 18 | machines | [AZN-28](#azn-28), [AZN-04](#azn-04) | session unit restart policy |
| `desktop/aizome/session/aizome-portal.service` | 14 | machines | [AZN-28](#azn-28) | D-Bus activated unit |
| `desktop/aizome/session/aizome-portals.conf` | 13 | none |  | portal routing config |
| `desktop/aizome/session/aizome-session` | 155 | machines | [AZN-27](#azn-27) | launcher phases |
| `desktop/aizome/session/aizome-session.target` | 13 | machines | [AZN-28](#azn-28) | session target hub |
| `desktop/aizome/session/aizome-shutdown.target` | 8 | machines | [AZN-28](#azn-28) | teardown target |
| `desktop/aizome/session/aizome-update.path` | 17 | machines | [AZN-29](#azn-29) | update watch |
| `desktop/aizome/session/aizome-update.service` | 11 | machines | [AZN-29](#azn-29) | offer oneshot |
| `desktop/aizome/session/aizome.portal` | 6 | none |  | backend declaration |
| `desktop/aizome/session/aizome.service` | 47 | machines | [AZN-28](#azn-28), [AZN-27](#azn-27) | compositor unit and watchdog |
| `desktop/aizome/session/dbus/org.freedesktop.impl.portal.desktop.aizome.service` | 4 | machines | [AZN-28](#azn-28) | D-Bus activation |
