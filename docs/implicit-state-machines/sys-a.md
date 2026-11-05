# SYA: System clients: launch, audio, Bluetooth, disks, MPRIS

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/crates/aizome-sys`. Audited at `8e0acbe5`; line numbers refer to that commit.

20 files, 0 pending. 19 implicit machines: 5 High, 9 Medium, 5 Low.

## Machines

<a id="sya-01"></a>

### SYA-01 — App launch route cascade and observer event stream
- **Where:** `desktop/aizome/crates/aizome-sys/src/apps/launch.rs:181` (`launch_observed`); also `desktop/aizome/crates/aizome-sys/src/apps/launch.rs:349` (`launch_commands`), `:142` (`LaunchObserver`), `:131` (`enum LaunchEvent`), `:605` (`observe_child`)
- **Encoding:** control flow over `options.use_dbus_activation`/`use_systemd` and `Backend`; the attempt's lifecycle is only visible as the `LaunchEvent` sequence emitted to the observer from the caller's thread and from one reaper thread per child
- **States:** `DBusActivating` → `Activated` (terminal, never followed by `Exited`) | `ExecResolving` (`Message("Executable: …")` per argv) → `ScopeStarting` → `Started(Launched)` → `Exited{status}`; `ScopeStarting` → `PlainFallback` (`Message`) → `Started` → `Exited`; `Failed` (`Err` returned after zero or more `Message`s); multi-target launches run several `Started`/`Exited` pairs on one observer
- **Transitions:** D-Bus ok → `Activated` (`apps/launch.rs:190`); D-Bus error → `Message` and fall through to Exec (`apps/launch.rs:194-200`); backend chosen (`apps/launch.rs:203-204`); program missing → `Err(NotFound)` (`apps/launch.rs:358-359`); per-argv `Executable` message (`apps/launch.rs:360-366`); scope fallback → `Message` + `spawn_plain` (`apps/launch.rs:377-385`); `Started` emitted (`apps/launch.rs:609`); reaper → `Exited` (`apps/launch.rs:611-615`); first process fails → `Err` (`apps/launch.rs:392`), later failures only logged (`apps/launch.rs:393`)
- **Hazards:** D-Bus activation waits up to `Opts::ACTION` (120 s, `bus.rs:236`) and treats any error, including a timeout from an app that is slowly starting, as "run its Exec", so a slow `DBusActivatable` app can end up running twice; `Activated` has no terminal counterpart, so the consumer needs its own deadline; if the reaper thread can't be spawned (`apps/launch.rs:617-619`) the `Child` is dropped unreaped (zombie) and the observer never gets `Exited`; events arrive on arbitrary threads after `launch*` has returned
- **Reify:** Medium — the attempt lifecycle is a cross-thread event protocol that the shell rebuilds into its own state
- **Related:** shell side `desktop/aizome/crates/aizome-shell/src/startup.rs:277` (`Event::Process`: `pids`, `activated`, `exited`, outcome text); `desktop/aizome/apps/hako/src/file_manager.rs:78`

<a id="sya-02"></a>

### SYA-02 — systemd-run scope handshake (reader thread, timeout, 500 ms grace)
- **Where:** `desktop/aizome/crates/aizome-sys/src/apps/launch.rs:498` (`spawn_scope`; reader-local `started` `:537`, channel `:531`, `deadline` `:570`); also `:643` (`systemd_version` cache `KNOWN`)
- **Encoding:** the `systemd-run` child, a stderr reader thread with `started: bool` and a `said` buffer, an mpsc message (`None` = unit line seen, `Some(text)` = stderr closed without it), `recv_timeout(timeout)`, then a 500 ms `try_wait` poll
- **States:** `Spawned` → `UnitNamed` (running in scope) | `ClosedWithoutLine` → {`ExitedOk` | `ExitedFailed` → Fallback | `StillRunning` (taken as started)} | `Silent` (timeout → killed → Fallback) | `ReaderGone` (killed → Fallback)
- **Transitions:** spawn (`apps/launch.rs:521`); no stderr pipe → kill + Fallback (`apps/launch.rs:527-529`); reader spawn fails → kill (`apps/launch.rs:557-559`); line contains unit → `send(None)` (`apps/launch.rs:545-547`); EOF without it → `send(Some)` (`apps/launch.rs:553-555`); `Ok(None)` → `observe_child` (`apps/launch.rs:562-564`); exited 0 → emit `Started`+`Exited` (`apps/launch.rs:573-578`); exited non-zero → Fallback (`apps/launch.rs:580`); still running after deadline → `observe_child` (`apps/launch.rs:582-585`); `try_wait` error → kill (`apps/launch.rs:586-589`); timeout → kill + Fallback (`apps/launch.rs:593-596`); reader died → kill (`apps/launch.rs:597-600`)
- **Hazards:** "started" is `text.contains(&wanted)` on any stderr line (`apps/launch.rs:545`), so an error line that names the unit counts as success and suppresses the fallback; if the line is ever missing and the app itself exits non-zero within 500 ms, `apps/launch.rs:580` reads that as a systemd-run failure and `launch_commands` starts the app a second time with `spawn_plain` (`apps/launch.rs:384`); the timeout kills only systemd-run's pid; `systemd_version` caches per un-canonicalised path (`apps/launch.rs:647-662`, `find_program` doesn't resolve symlinks), so an in-place systemd upgrade under `/run/current-system` keeps the old answer to whether to pass `--expand-environment=no`
- **Reify:** Medium — six outcomes hidden in nested matches; one misclassification double-launches

<a id="sya-03"></a>

### SYA-03 — (test harness) launcher-lifetime marker files and probe wait loops
- **Where:** `desktop/aizome/crates/aizome-sys/src/apps/launch.rs:977` (`sandbox_survives_launcher_exit`); also `:850` (`Probe::report`), `:883` (`assert_reaped`), `:949` (fake and hanging `systemd-run`), `:1079` (shim)
- **Encoding:** the test binary re-executes itself with `AIZOME_TEST_LAUNCH_LIFETIME`; marker files `launching`, `ready`, `continue`, `survived`, `supervisor` (pid) and the `systemd-run` shim's presence select the role and phase; 10 s/2 s polling deadlines
- **States:** parent `Spawned` → waits `ready`+`supervisor` → kills launcher → writes `continue` → waits `survived` → `assert_reaped`; child-as-launcher (`launching` absent) writes `launching`, launches, writes `supervisor`, sleeps 20 s; grandchild (`launching` present) sets `PR_SET_PDEATHSIG`, writes `ready`, waits `continue`, writes `survived`
- **Transitions:** role split (`apps/launch.rs:980-982`); `launching` (`apps/launch.rs:995`); `ready` (`apps/launch.rs:986`); `supervisor` (`apps/launch.rs:1014`); parent kill (`apps/launch.rs:1035-1036`); `continue` (`apps/launch.rs:1038`); `survived` (`apps/launch.rs:992`); report poll (`apps/launch.rs:851-865`); reap poll (`apps/launch.rs:884-892`)
- **Hazards:** leftover markers in a reused temp dir change roles; fixed deadlines on a loaded CI host fail spuriously
- **Reify:** Low — test-only file protocol

<a id="sya-04"></a>

### SYA-04 — PulseAudio connection and reconnect loop
- **Where:** `desktop/aizome/crates/aizome-sys/src/audio.rs:922` (`run`), `:950` (`session`); also `:260` (`Audio.commands`, `.eventfd`, `.stop`), `:626` (`Live.dirty`)
- **Encoding:** which loop the audio thread is in (connect wait `:971`, main loop `:1018`, backoff `:941`), libpulse `Context::get_state()`, a per-session `Live` (`:990`) whose `dirty` flag gates publishing, `AudioState.available` in the model, the eventfd I/O event registered only once `Ready`, `Stop`
- **States:** `NoThread` (no eventfd) | `Connecting` → `Ready` (snapshot published on first dirty; `available` true) → `Lost` → `Backoff` (2 s, model reset, queued commands dropped) → `Connecting` …; any → `Stopped`
- **Transitions:** thread only with an eventfd (`audio.rs:291-300`); connect (`audio.rs:965-968`); `Ready` (`audio.rs:982`); `Failed`/`Terminated` (`audio.rs:983-985`); load + subscribe (`audio.rs:991-992`); eventfd watched (`audio.rs:999-1012`); early commands run (`audio.rs:1014-1016`); publish (`audio.rs:1032-1035`); server left (`audio.rs:1028-1030`); model reset (`audio.rs:935`); drain (`audio.rs:937-939`); backoff (`audio.rs:941-946`); `Drop` stop + poke (`audio.rs:437-441`)
- **Hazards:** while connecting the eventfd isn't watched and `iterate(true)` has no timeout, so `Drop`'s poke can't stop a thread stuck on a server that accepts but never answers; `available` turns true on the first callback (often server info), before the device lists, so a volume key right after (re)connect gets "no default output" (`audio.rs:1301-1302`); `send` refuses every command while `available` is false (`audio.rs:320-322`), so key presses during startup or the 2 s backoff fail instead of queueing; with no eventfd the service is unavailable for good, silently
- **Reify:** Medium — structured as loops, but readiness is inferred from `dirty` and the snapshot
- **Related:** consumers `desktop/aizome/crates/aizome-shell/src/sys.rs:45`, `desktop/aizome/apps/settei/src/panes/sound.rs:308`

<a id="sya-05"></a>

### SYA-05 — Optimistic audio levels awaiting server confirmation
- **Where:** `desktop/aizome/crates/aizome-sys/src/audio.rs:538` (`Expect.volumes`, `.muted`, `.since`); also `:625` (`Live.expects`), `:620` (`Live.sinks`… levels overwritten in place), `:618` (`Live.default_sink`, `.default_source`), `:1175` (`finish` once-guard)
- **Encoding:** per `(Kind, index)` an `Expect` with two `Option`s and an `Instant` (1 s window, `:544`); the stored level/mute and the defaults are written before the server replies; the reply only completes the `Done`
- **States:** per object `Confirmed` → `Expecting{volume?, mute?}` → `Confirmed` (matching report) | `Expired` (first report after 1 s) | `Removed`; on refusal the object stays at the unconfirmed level with its `Expect` live
- **Transitions:** volume set (`audio.rs:595-597`); mute set (`audio.rs:608-610`); balance set (`audio.rs:716-719`); report kept or cleared (`audio.rs:556-577`); object removed (`audio.rs:680`); default set optimistically (`audio.rs:1266-1270`); result (`audio.rs:1178-1181`); step + unmute (`audio.rs:1305-1315`); balance read-then-write (`audio.rs:1322-1345`)
- **Hazards:** a refused operation (`finish` with `Err`, `audio.rs:1180`) reverts neither the level/default nor the `Expect`, and PulseAudio sends no change event for a refused change, so the snapshot keeps a level the server never applied; expiry is lazy (checked only on the next report, `audio.rs:557`); `expect()` refreshes `since` for both fields when either is set (`audio.rs:582`), so a mute press extends a pending volume expectation; within the window a genuine change by another client is masked; defaults have no `Expect`, so a server-info reply requested earlier can undo an optimistic default (`audio.rs:1117-1121`); Balance drops its `done` (task `Cancelled`, not `NotFound`) when the device lookup returns no item or the context is gone (`audio.rs:1323-1345`)
- **Reify:** High — state spread over `Expect`, the entry maps, the defaults and per-operation callbacks, with a user-visible mismatch
- **Suspected bug:** Settings changes the volume of a stream the server refuses to change (for example a sink input with `volume_writable` false): `set_volume_in` has already stored the new level and marked the model dirty, the callback reports `Err`, no event follows, so the snapshot and slider show the unapplied level until another event for that stream (confidence: medium)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#sya-05)

<a id="sya-06"></a>

### SYA-06 — Bluetooth pairing-agent registration worker
- **Where:** `desktop/aizome/crates/aizome-sys/src/bluetooth/agent.rs:232` (`Worker.tx`); also `:277` (`serve` local `current: Option<Registration>`), `:302` (`Registration`, unregisters on `Drop` `:332`)
- **Encoding:** a lazily spawned thread behind `Mutex<Option<Sender<Op>>>`; `current` holds the live registration; BlueZ's AgentManager1 state on the far side
- **States:** `NoThread` → `Unregistered` → `Registering` → `Registered` | `Unregistered` (failed); `Registered` → `Unregistered` (Unregister, re-Register, channel closed)
- **Transitions:** lazy spawn (`bluetooth/agent.rs:259-268`); op sent (`bluetooth/agent.rs:271`); previous dropped first (`bluetooth/agent.rs:282`); register (`bluetooth/agent.rs:283-288`): stale object removed and served (`bluetooth/agent.rs:311-312`), ping until served (`bluetooth/agent.rs:314`), `RegisterAgent` tolerating `AlreadyExists` (`bluetooth/agent.rs:316-321`), `RequestDefaultAgent` (`bluetooth/agent.rs:322-324`), rollback (`bluetooth/agent.rs:325-328`); Unregister (`bluetooth/agent.rs:292`); channel end (`bluetooth/agent.rs:298`); `Drop` (`bluetooth/agent.rs:332-341`)
- **Hazards:** BlueZ forgets agents when bluetoothd restarts, but `current` stays `Some` and nothing re-registers (the follower's `Appeared`, `bluetooth/mod.rs:358`, never reaches the worker); a failed re-Register leaves no agent at all, since the old one was dropped first; thread spawn failure leaves `tx` `None` and cancels each op; `Registration::drop` blocks the worker on two D-Bus calls
- **Reify:** High — a suspected bug, and BlueZ's agent state can diverge from the local `Option`
- **Suspected bug:** bluetoothd restarts while Settei's Bluetooth pane is shown; Settei keeps `agent = true` (`desktop/aizome/apps/settei/src/panes/bluetooth.rs:453`, reset only on a failed registration at `:621` or on hide at `:490`) and the worker keeps its `Registration`, so BlueZ has no agent and pairing a device that needs a passkey or confirmation fails with no prompt until the pane is hidden and shown again (confidence: medium)
- **Related:** `desktop/aizome/apps/settei/src/panes/bluetooth.rs:442` (registers on show), `:489` (unregisters on hide); BlueZ `org.bluez.AgentManager1`
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#sya-06)

<a id="sya-07"></a>

### SYA-07 — Pairing request conversation (prompt, responder, BlueZ cancel)
- **Where:** `desktop/aizome/crates/aizome-sys/src/bluetooth/agent.rs:118` (`Agent.current`); also `:75` (`Responder.tx`), `:144` (`ask`)
- **Encoding:** per request a `crate::oneshot` channel (sender in `Responder.tx: Option`, receiver awaited by the method's own zbus task); `current: Mutex<Option<String>>`, the device last shown, which BlueZ's argument-less `Cancel` refers to
- **States:** per request `LookingUpName` → `Shown` → `Answered(Accept|Reject|PinCode|Passkey)` | `Dropped` (= Rejected); display-only `Shown` needs no answer; `Cancel` → `Cancelled` prompt for the taken `current`; `current`: `None` → `Some(device)` → `None` only on `Cancel`
- **Transitions:** alias lookup (`bluetooth/agent.rs:146`); `current` set (`bluetooth/agent.rs:147`, `:165`); prompt sent (`bluetooth/agent.rs:148-153`); await (`bluetooth/agent.rs:154`); display prompt (`bluetooth/agent.rs:158-161`); respond (`bluetooth/agent.rs:90-94`); dropped responder → Rejected (`bluetooth/agent.rs:172`, `:185`, `:196`); bad PIN/passkey → Rejected (`bluetooth/agent.rs:183`, `:195`); `Cancel` takes `current` (`bluetooth/agent.rs:218-224`)
- **Hazards:** `current` is set only after the alias lookup (up to 2 s): a `Cancel` in that window names the previous device or `""` (Settei then closes whatever prompt shows), and the prompt shown afterwards is never cancelled; BlueZ's `Cancel` doesn't end the pending handler, so the receiver stays alive, `wants_answer()` stays true after BlueZ gave up (contrary to its doc, `bluetooth/agent.rs:104-108`), and the task lives until the UI drops the `Responder`; `current` is never cleared after an answer
- **Reify:** Medium — small, but its correctness depends on the UI matching `Cancelled` by device
- **Related:** `desktop/aizome/apps/settei/src/panes/bluetooth.rs:587` (Prompted/Cancelled), `:320` (`close_prompt` by device); BlueZ `org.bluez.Agent1` caller

<a id="sya-08"></a>

### SYA-08 — ObjectManager follower cache (BlueZ, UDisks)
- **Where:** `desktop/aizome/crates/aizome-sys/src/bluetooth/mod.rs:347` (closure `objects`, `present` `:348`, `dirty` `:349`); also `desktop/aizome/crates/aizome-sys/src/disks/mod.rs:233` (`objects`, `present` `:234`, `dirty` `:235`)
- **Encoding:** closure-captured `Objects` cache plus `present`/`dirty` bools, driven by `bus::follow` events (SYA-10); `available` in the published model
- **States:** `Absent` (default snapshot) → `Loaded+Dirty` → `Published` ⇄ `Dirty` (signal applied) → `Absent` (Vanished); reachable invalid combination `present && objects empty` after a failed load
- **Transitions:** Appeared (`bluetooth/mod.rs:358-365`; `disks/mod.rs:237-244`); Vanished (`bluetooth/mod.rs:366-370`; `disks/mod.rs:245-249`); Signal (`bluetooth/mod.rs:371-377`; `disks/mod.rs:250-256`); Idle publish (`bluetooth/mod.rs:378-383`; `disks/mod.rs:257-262`)
- **Hazards:** a failed `GetManagedObjects` on Appeared (`bluetooth/mod.rs:360-363`, `disks/mod.rs:239-242`) still sets `present`, publishing `available: true` with no adapters or drives, and nothing retries until the daemon re-appears; refetching invalidated properties blocks the follower up to 2 s each (`bluetooth/mod.rs:374`, `disks/mod.rs:253`); property signals for objects not yet known are dropped by design (`bus.rs:514-516`)
- **Reify:** Medium — two copies of the same closure machine with a reachable invalid state
- **Suspected bug:** UDisks is started by activation (`disks/mod.rs:236`, `activate = true`) and is still enumerating when `GetManagedObjects` hits the 2 s query timeout; Tansu and the desktop then show UDisks as available with no drives until udisksd restarts (confidence: low)
- **Reproduction:** not attempted; only suspected bugs on High machines were reproduced

<a id="sya-09"></a>

### SYA-09 — Multi-step BlueZ actions (unblock then power, pair then trust then connect)
- **Where:** `desktop/aizome/crates/aizome-sys/src/bluetooth/mod.rs:424` (`set_powered`); also `:480` (`pair`), `:456` (`start_discovery`), `:466` (`stop_discovery`)
- **Encoding:** sequential calls in one task; the rfkill block state comes from the cached snapshot; tolerated BlueZ errors stand for "already in that phase"
- **States:** power: `Blocked` → `Unblocked` (rfkill) → (300 ms) → `Powered` | `UnblockedNotPowered`; pair: `Unpaired` → `Paired` → `Trusted` → `Connected`, with `AlreadyExists`/`AlreadyConnected`/`InProgress` read as success
- **Transitions:** blocked read from snapshot (`bluetooth/mod.rs:425`); unblock (`bluetooth/mod.rs:430`); fixed sleep (`bluetooth/mod.rs:432`); Powered (`bluetooth/mod.rs:434`); Pair (`bluetooth/mod.rs:483-487`); Trusted (`bluetooth/mod.rs:488`); Connect (`bluetooth/mod.rs:489`, `:561-566`); discovery errors tolerated (`bluetooth/mod.rs:460`, `:470`)
- **Hazards:** a fixed 300 ms instead of waiting for BlueZ to see the unblock; a failure after Pair leaves the device paired but untrusted or disconnected, reported as a failed pairing; the blocked check uses a possibly stale snapshot
- **Reify:** Low — linear sequences; the phases live in BlueZ and rfkill

<a id="sya-10"></a>

### SYA-10 — Daemon follower (subscription, owner tracking, Appeared/Vanished/Idle)
- **Where:** `desktop/aizome/crates/aizome-sys/src/bus.rs:573` (`follow`); also `:647` (`follow_loop` local `owner: Option<String>`), `:685` (`dispatch`), `:34` (`SYSTEM`/`SESSION` shared connections)
- **Encoding:** a drain thread and a handler thread; a `ready` channel for the subscription handshake; `owner` for the current owner; `Stop`; events handed to the service's closure
- **States:** `Subscribing` → `Failed` (Vanished+Idle, end) | `Resolving owner` (optionally activating) → `Present(owner)` | `Absent` ⇄ via `NameOwnerChanged` → `Stopped`/`Disconnected` (Vanished+Idle, end)
- **Transitions:** subscribe (`bus.rs:587-594`); handshake (`bus.rs:608-611`); owner query and activation (`bus.rs:647-653`); initial event (`bus.rs:654-659`); owner change → Vanished/Appeared (`bus.rs:694-701`); signals only from `owner` (`bus.rs:705-707`); batch then Idle (`bus.rs:671-676`); stop or disconnect (`bus.rs:664-666`); final Vanished+Idle (`bus.rs:613-614`)
- **Hazards:** the drain thread checks `stop` only when a message arrives (`bus.rs:597`), so after a handle is dropped it and its match rules stay on the shared connection until the next matching signal; activation is tried once at start (`bus.rs:648`); the cached shared connections (`bus.rs:59-70`) are never invalidated, so after the bus connection closes every follower ends with Vanished and nothing reconnects for the rest of the process; a slow Appeared load holds up every later event
- **Reify:** Medium — reused by every daemon client; owner, handshake and thread lifetimes are implicit
- **Related:** other `bus::follow` users outside this slice: `users.rs:284`, `power/logind.rs:192`, `power/upower.rs:319`, `power/profiles.rs:125`, `notifications/client.rs:163`, `network/mod.rs:195` (all under `desktop/aizome/crates/aizome-sys/src/`)

<a id="sya-11"></a>

### SYA-11 — Single-object property follower with keep-on-exit (timedated, localed, hostnamed)
- **Where:** `desktop/aizome/crates/aizome-sys/src/bus.rs:757` (`follow_props`; `props: Option<Props>` `:766`, `dirty` `:767`); also `desktop/aizome/crates/aizome-sys/src/datetime/mod.rs:140` (TimeDate), `:256` (Locale `env` cache), `:391` (Hostname `env` cache)
- **Encoding:** `props` (`None` = daemon gone or never seen), `dirty`, the `keep_on_exit` flag; in datetime, `env: Option<HashMap>` caches the unit's `Environment=` from which the `*_managed` flags come
- **States:** `Unknown` (None) → `Present(props)` → `Away` (props kept, `keep_on_exit`) → `Present` (reloaded) …; without keep-on-exit `Present` → `None`
- **Transitions:** Appeared → `get_all` (`bus.rs:769-772`); Vanished (`bus.rs:773-778`); PropertiesChanged and invalidated refetch (`bus.rs:779-800`); Idle publish (`bus.rs:801-806`); TimeDate re-reads env on every change (`datetime/mod.rs:141`); Locale/Hostname cache it once (`datetime/mod.rs:258`, `:393`)
- **Hazards:** a failed `GetAll` on Appeared yields `Some(empty)` (`bus.rs:770`), so the services publish `available: true` with empty values; Locale and Hostname never refresh their cached environment, so their `*_managed` flags don't follow a `nixos-rebuild switch` (and an empty map from a failed first lookup is cached), while TimeDate's do; the client-side guards act on that snapshot (`datetime/mod.rs:174`, `:285`, `:299`, `:406`, `:420`); while the idle-exiting daemon is away, changes made without it go unseen until it is next activated
- **Reify:** Medium — `keep_on_exit` and the cached policy make "available" and "managed" phase-dependent
- **Related:** systemd `systemd-timedated`/`localed`/`hostnamed` (D-Bus activated) and their unit `Environment=` from the NixOS module; consumer `desktop/aizome/crates/aizome-shell/src/shell.rs:1157`

<a id="sya-12"></a>

### SYA-12 — Wait-until-serving ping retry loop
- **Where:** `desktop/aizome/crates/aizome-sys/src/bus.rs:719` (`wait_until_serving`; `tries` `:722`)
- **Encoding:** attempt counter, 100 ms ping timeout, error kind
- **States:** `Pinging(n)` → `Served` | `PolicyForbidsSelfCall` (Ok) | `GaveUp` (Timeout after 50) | `Error`
- **Transitions:** timeout → retry (`bus.rs:725`); `NotAuthorized` → Ok (`bus.rs:729`); anything else returns (`bus.rs:730`)
- **Hazards:** up to about 5 s of blocking on the caller's thread (the agent worker); `NotAuthorized` is taken as served without checking
- **Reify:** Low — local retry loop
- **Related:** callers `bluetooth/agent.rs:314`, and outside this slice `polkit.rs:762`, `statusnotifier/mod.rs:214`, `notifications/server.rs:453`

<a id="sya-13"></a>

### SYA-13 — Destructive-operation confirmation token
- **Where:** `desktop/aizome/crates/aizome-sys/src/disks/confirm.rs:80` (`Confirm.fingerprint`, `.at`); also `:56` (`Fingerprint`), `:17` (`CONFIRMATION_LIFETIME`), `desktop/aizome/crates/aizome-sys/src/disks/mod.rs:595` (`destructive`)
- **Encoding:** partially explicit: typestate `Confirm<Op>` moved into the call (single use), with a fingerprint and an `Instant`; validity is decided at run time against a fresh `GetManagedObjects`
- **States:** `Fresh` → `Consumed` (checked and run) | `Expired` (> 10 min) | `Gone` | `Changed` | `InUse`
- **Transitions:** made (`disks/confirm.rs:94-96`); expiry (`disks/confirm.rs:105-111`); gone (`disks/confirm.rs:112-114`); changed (`disks/confirm.rs:115-121`); re-read state (`disks/mod.rs:602`); check (`disks/mod.rs:603`); in-use refusal (`disks/mod.rs:604-606`); run (`disks/mod.rs:607`)
- **Hazards:** check-then-act: the device can be mounted or changed between `disks/mod.rs:603-604` and the UDisks call; the fingerprint has no drive identity (serial/WWN) and no partition-table identity, and whole partitioned disks have empty `IdType`/`IdUUID`, so a different disk of the same size at the same kernel name matches; `Instant` is monotonic, so time asleep in suspend doesn't count towards the 10 minutes
- **Reify:** High — a data-loss guard; the gaps are in what the token records
- **Suspected bug:** a UI that holds a `Confirm<NewPartitionTable>` made for USB stick A (32 GB, GPT) while A is swapped for an identical-size partitioned stick B that comes up as `sdb` again: path, device, size and empty id fields all match, so B is wiped (confidence: low; no app makes these tokens yet)
- **Reproduction:** Confirmed (unit): [test and output](reproductions.md#sya-13)
- **Follow-up:** reified as typestate in [#290](https://github.com/bitemyapp/sekka/pull/290). Its suspected bug is fixed, and the mistake no longer compiles.

<a id="sya-14"></a>

### SYA-14 — Disk image mount: attach, probe, mount, autoclear, clean up
- **Where:** `desktop/aizome/crates/aizome-sys/src/disks/mod.rs:387` (`mount_image`; `mounted_by_us` `:391`, `deadline` `:394`); also `:741` (`cleanup_failed_image`), `:726` (`loop_setup`)
- **Encoding:** local `device` (loop attached), `mounted_by_us: Option<String>`, a 5 s probe deadline, UDisks' `Autoclear` flag that hands the loop's lifetime to the kernel
- **States:** `Attached` → `Probing` (poll every 100 ms) → `MountedByUs` | `AlreadyMounted` → `Autoclear` (done); failure from any phase → `CleaningUp` (unmount if ours, delete loop) → `Failed`
- **Transitions:** attach (`disks/mod.rs:390`); probe (`disks/mod.rs:396-398`); already mounted (`disks/mod.rs:399-401`); mount (`disks/mod.rs:403-405`); autoclear (`disks/mod.rs:410-417`); deadline (`disks/mod.rs:420-422`); cleanup (`disks/mod.rs:426`, `:743-752`)
- **Hazards:** a mount made concurrently by someone else (an automounter reacting to the new loop) leaves `mounted_by_us` `None`, and a later failure deletes the loop under that mount; each probe is a full `GetManagedObjects`; cleanup errors are folded into the message, and a half-cleaned loop stays attached
- **Reify:** Medium — what cleanup does depends on how far the attempt got

<a id="sya-15"></a>

### SYA-15 — Drive release before eject or power-off
- **Where:** `desktop/aizome/crates/aizome-sys/src/disks/mod.rs:704` (`release_drive`); also `:311` (`lock`), `:358` (`eject`), `:368` (`power_off`)
- **Encoding:** one refreshed snapshot, blocks ordered by number of dependents, two passes
- **States:** `Mounted/SwapOn/Unlocked` → `Unmounted` → `SwapOff` → `Locked` → `Ejected` | `PoweredOff`; partial on any error
- **Transitions:** refresh (`disks/mod.rs:705`); order (`disks/mod.rs:709`); unmount and swap off (`disks/mod.rs:710-717`); lock (`disks/mod.rs:718-722`); eject or power off (`disks/mod.rs:362`, `:372`); lock path unmounts cleartext then locks (`disks/mod.rs:314-322`)
- **Hazards:** no rollback, so an error leaves the drive half released; the order is by dependent count, not depth; a device mounted after the snapshot isn't seen
- **Reify:** Low — linear teardown; the phases live in UDisks

<a id="sya-16"></a>

### SYA-16 — GeoClue locate request with deadline and cancellation
- **Where:** `desktop/aizome/crates/aizome-sys/src/location.rs:160` (`locate_until`; `client: Option<Proxy>` `:166`); also `:75` (`Cancellation`: `cancelled` flag + `event` `:78-81`), `:139` (`until`)
- **Encoding:** one deadline shared by every step, a one-shot cancellation token, `client` set once `GetClient` answers (it decides whether cleanup runs), a 200 ms `Location` poll
- **States:** `Connecting` → `GettingClient` → `Configuring` → `Started` → `Polling` → `Located` | `TimedOut` | `Cancelled` | `Denied/Failed`; then `CleaningUp` (Stop + DeleteClient, ≤ 250 ms) when `client` is set
- **Transitions:** connect (`location.rs:111-114`); GetClient (`location.rs:169`); client recorded (`location.rs:170`); DesktopId/accuracy (`location.rs:172-173`); Start (`location.rs:174`); poll (`location.rs:175-181`); deadline/cancel race (`location.rs:144-157`); cancel (`location.rs:84-87`); cleanup (`location.rs:184-201`)
- **Hazards:** a `GetClient` reply that lands after the deadline or cancel never sets `client`, so the GeoClue client isn't stopped or deleted; that is harmless on `locate`'s private connection but leaks on a caller-owned one (`locate_on*`) until it closes; a token stays cancelled for good (reusing one fails at once)
- **Reify:** Low — contained, already deadline-bounded
- **Related:** `desktop/aizome/crates/aizome-shell/src/location.rs:58`, `desktop/aizome/apps/tenki/src/services.rs:130`; GeoClue2 Manager/Client

<a id="sya-17"></a>

### SYA-17 — Level meter thread lifecycle
- **Where:** `desktop/aizome/crates/aizome-sys/src/meter.rs:77` (`run`); also `:27` (`Meter.level`, `.stop`)
- **Encoding:** thread position (connect wait `:84`, poll loop `:139`), libpulse context state, the record stream, the `level` atomic read by the UI, `Stop`
- **States:** `Connecting` → `Ready` → `Recording` (level updates) → `Stopped` (level reset to 0) | `Errored` (thread gone, level frozen); `Recording` with a dead stream is representable and unhandled
- **Transitions:** connect (`meter.rs:83`); ready or failed (`meter.rs:91-94`); stream connected (`meter.rs:130-137`); level stored + wake (`meter.rs:110-112`); context lost (`meter.rs:150-152`); stop → disconnect + reset (`meter.rs:154-155`); `Drop` sets stop (`meter.rs:62-66`)
- **Hazards:** only the stop path zeroes `level`; every error return (`meter.rs:89`, `:93`, `:142-151`) leaves the last peak in place, with no reconnect and no way for the owner to tell the meter is dead; the loop checks the context, not the stream: with `DONT_MOVE` (`meter.rs:135`) the server kills the stream when its source goes away while the context stays `Ready`; the connect wait blocks in `iterate(true)` and doesn't see `stop`
- **Reify:** High — a suspected bug, and the dead state can't be seen from the handle
- **Suspected bug:** pipewire-pulse restarts, or the default microphone disappears and comes back under the same name, while Settei's Sound pane is open: the meter thread exits (or idles on a dead stream) with `level` at its last peak, and Settei, which polls `level()` on a timer and recreates the meter only when the default input's name changes (`desktop/aizome/apps/settei/src/panes/sound.rs:208-216`), shows a frozen input level (confidence: medium)
- **Related:** `desktop/aizome/apps/settei/src/panes/sound.rs:213`; pipewire-pulse peak-detect stream
- **Reproduction:** Partly confirmed (integration): [test and output](reproductions.md#sya-17)

<a id="sya-18"></a>

### SYA-18 — MPRIS player-set follower
- **Where:** `desktop/aizome/crates/aizome-sys/src/mpris/mod.rs:424` (`follow_players`); also `:473` (`follow`), `:509` (`appeared`), `:167` (`Players.map`)
- **Encoding:** a drain thread and a follower thread (as SYA-10); `Players.map` keyed by bus name, with `owner` per entry; NameOwnerChanged drives add and remove
- **States:** `Subscribing` → `Listing` → `Following` → `Ended` (players cleared unless stopped) | `Stopped`; per name `Absent` → `Tracked(owner)` → `Absent` / re-`Tracked(new owner)`
- **Transitions:** subscribe (`mpris/mod.rs:428-445`); ListNames + read each (`mpris/mod.rs:481-487`); owner gone or changed → remove (`mpris/mod.rs:545-547`); new owner → appeared (`mpris/mod.rs:548-550`, `:517-532`); batch publish (`mpris/mod.rs:499-504`); end clears (`mpris/mod.rs:460-464`)
- **Hazards:** `available()` is only "a session bus exists" (`mpris/mod.rs:225-227`), so a follower that failed (ListNames error) or ended leaves `available` true with no players and no retry; the drain thread outlives `stop` until the next signal (`mpris/mod.rs:440`); a hung player stalls everyone for 2 s per read (`mpris/mod.rs:511-515`); `player_task` refuses players missing from a possibly stale snapshot (`mpris/mod.rs:236-238`)
- **Reify:** Medium — a second copy of the follower machine with its own lifecycle rules
- **Related:** media players on the session bus; consumer `desktop/aizome/crates/aizome-shell/src/sys.rs:52`

<a id="sya-19"></a>

### SYA-19 — MPRIS position estimate and active-player counters
- **Where:** `desktop/aizome/crates/aizome-sys/src/mpris/mod.rs:99` (`Tracked.position_us`, `.position_at`, `.played`, `.changed`, `.stale_position`); also `:169` (`Players.seq`), `desktop/aizome/crates/aizome-sys/src/mpris/player.rs:297` (`estimate_us`), `:329` (`choose_active`)
- **Encoding:** a base position plus the `Instant` it was read, extrapolated while `PlaybackStatus` is Playing; a `stale_position` flag asking for a re-read at publish; a global `seq` stamped into `played` (status to or from Playing) and `changed` (any change) to pick the active player
- **States:** position `Fresh` → `Estimating` (Playing) → `Stale` (status, track or rate touched) → `Fresh` (read, announced, Seeked, refreshed); activity order by `(playing, played, changed)`
- **Transitions:** rebase before a change (`mpris/mod.rs:119-125`); announced Position (`mpris/mod.rs:134-139`); mark stale (`mpris/mod.rs:140-142`); played/changed (`mpris/mod.rs:144-147`); Seeked (`mpris/mod.rs:604-612`); refetch after invalidation (`mpris/mod.rs:584-596`); stale read at publish (`mpris/mod.rs:634-641`); `refresh_position` (`mpris/mod.rs:340-345`); counters reset when a name re-appears (`mpris/mod.rs:518-530`)
- **Hazards:** `publish` clears `stale_position` with `mem::take` before reading (`mpris/mod.rs:634`) and skips on error (`mpris/mod.rs:637`), so after a failed read (for example on a track change) the old track's extrapolated position stands until the next status, track or rate change; positions are written from two threads (the follower and `refresh_position` tasks), safe only because both write and build under the lock with an owner check; re-appearance resets `played`, demoting a player that was just active
- **Reify:** High — five fields mutated at five sites across two threads

## Already explicit

- `desktop/aizome/crates/aizome-sys/src/mpris/player.rs:49` `enum LoopStatus` — a player's repeat mode for a repeat button; next state centralised in `fn cycle` (`desktop/aizome/crates/aizome-sys/src/mpris/player.rs:80`)

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/crates/aizome-sys/src/apps.rs` | 17 | none |  | re-exports and module wiring only |
| `desktop/aizome/crates/aizome-sys/src/apps/launch.rs` | 1148 | machines | [SYA-01](#sya-01), [SYA-02](#sya-02), [SYA-03](#sya-03) | launch cascade, systemd-run handshake, test harness |
| `desktop/aizome/crates/aizome-sys/src/audio.rs` | 1450 | machines | [SYA-04](#sya-04), [SYA-05](#sya-05) | connection loop; optimistic levels |
| `desktop/aizome/crates/aizome-sys/src/bluetooth/agent.rs` | 341 | machines | [SYA-06](#sya-06), [SYA-07](#sya-07) | agent registration worker; pairing conversation |
| `desktop/aizome/crates/aizome-sys/src/bluetooth/mod.rs` | 639 | machines | [SYA-08](#sya-08), [SYA-09](#sya-09) | ObjectManager follower; multi-step actions |
| `desktop/aizome/crates/aizome-sys/src/bus.rs` | 852 | machines | [SYA-10](#sya-10), [SYA-11](#sya-11), [SYA-12](#sya-12) | follow, follow_props, wait_until_serving; shared connection cache |
| `desktop/aizome/crates/aizome-sys/src/datetime/mod.rs` | 500 | machines | [SYA-11](#sya-11) | timedated/localed/hostnamed via follow_props; env policy cache |
| `desktop/aizome/crates/aizome-sys/src/datetime/zones.rs` | 212 | none |  | pure tzdata parsing |
| `desktop/aizome/crates/aizome-sys/src/disks/confirm.rs` | 150 | machines | [SYA-13](#sya-13) | confirmation token |
| `desktop/aizome/crates/aizome-sys/src/disks/mod.rs` | 779 | machines | [SYA-08](#sya-08), [SYA-13](#sya-13), [SYA-14](#sya-14), [SYA-15](#sya-15) | UDisks follower; destructive check; image mount; drive release |
| `desktop/aizome/crates/aizome-sys/src/disks/model.rs` | 1247 | none |  | pure derivation from UDisks objects; tests |
| `desktop/aizome/crates/aizome-sys/src/disks/smart.rs` | 202 | none |  | pure SMART formatting; tests |
| `desktop/aizome/crates/aizome-sys/src/disks/types.rs` | 194 | none |  | data tables |
| `desktop/aizome/crates/aizome-sys/src/error.rs` | 222 | none |  | error classification; tests |
| `desktop/aizome/crates/aizome-sys/src/keyboard.rs` | 826 | none |  | pure XKB registry parsing; tests |
| `desktop/aizome/crates/aizome-sys/src/lib.rs` | 78 | none |  | module declarations |
| `desktop/aizome/crates/aizome-sys/src/location.rs` | 227 | machines | [SYA-16](#sya-16) | GeoClue request deadline and cleanup |
| `desktop/aizome/crates/aizome-sys/src/meter.rs` | 171 | machines | [SYA-17](#sya-17) | level meter thread |
| `desktop/aizome/crates/aizome-sys/src/mpris/mod.rs` | 681 | machines | [SYA-18](#sya-18), [SYA-19](#sya-19) | player-set follower; position and activity counters |
| `desktop/aizome/crates/aizome-sys/src/mpris/player.rs` | 498 | machines | [SYA-19](#sya-19) | estimate and active choice over SYA-19 state; LoopStatus explicit |
