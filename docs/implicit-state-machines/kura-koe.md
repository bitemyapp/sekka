# KKO: Kura and Koe

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/apps/{kura,koe}`. Audited at `8e0acbe5`; line numbers refer to that commit.

52 files, 0 pending. 37 implicit machines: 9 High, 8 Medium, 20 Low.

## Machines

<a id="kko-01"></a>

### KKO-01 — Dictation on/off: koe.toml `enabled` gating the user unit and the daemon it runs
- **Where:** `desktop/aizome/apps/koe/data/aizome-koe.service:15` (`ExecCondition=… enabled`), `:17` (`Restart=on-failure`), `:18` (`RestartSec=2`); `desktop/aizome/apps/koe/src/config.rs:13` (`Config.enabled`); `desktop/aizome/apps/koe/src/service.rs:32` (`Session.unit`), `:91` (`start`), `:125` (`stop`); also `desktop/aizome/apps/koe/src/main.rs:315`, `:332`, `:499`; `desktop/aizome/apps/koe/src/tray.rs:229`; `desktop/aizome/apps/koe/src/app.rs:374`
- **Encoding:** persisted `enabled` in koe.toml (tmp+rename save, `desktop/aizome/apps/koe/src/config.rs:196-206`); systemd unit state (inactive/active/auto-restart/failed); whether a daemon answers on control.sock (`service::running`, polled by `wait`); `Session.unit` choosing systemd or a self-spawned daemon.
- **States:** `Off` (enabled=false, nothing answers) → `Starting` (systemctl start or spawned child, polled ≤10 s) → `On` (enabled=true, daemon answers) → `Stopping` → `Off`; `CrashLooping`/`Failed` (unit restarted every 2 s). Representable mismatches: `enabled && !running` (start failed, crash), `!enabled && running` (off saved but stop failed, koe.toml edited, CLI disable with unreadable settings), an unmanaged daemon running while the unit is inactive.
- **Transitions:** login: `ExecCondition` runs `aizome-koe enabled` (`desktop/aizome/apps/koe/src/main.rs:257`) then `daemon`; window switch saves then starts/stops (`desktop/aizome/apps/koe/src/app.rs:382`, `:404-408`); CLI enable saves+starts (`desktop/aizome/apps/koe/src/main.rs:321`, `:329`), disable saves+stops (`desktop/aizome/apps/koe/src/main.rs:335-336`); tray Turn Off saves false then `Shutdown` (`desktop/aizome/apps/koe/src/tray.rs:233`, `:239`); shortcut `record` starts the service when enabled but not answering (`desktop/aizome/apps/koe/src/main.rs:500-508`); `Session::start`: `systemctl start` + `wait(up)` (`desktop/aizome/apps/koe/src/service.rs:96-100`) else spawn own daemon, kill on timeout, detach a reaper (`desktop/aizome/apps/koe/src/service.rs:103-121`); `Session::stop`: `systemctl stop` then IPC `Shutdown` + `wait(down)` (`desktop/aizome/apps/koe/src/service.rs:127-131`); daemon exits 0 on Shutdown (`desktop/aizome/apps/koe/src/daemon.rs:555-557`) so `Restart=on-failure` leaves it down.
- **Hazards:** three writers of `enabled` (window, tray inside the daemon, CLI) doing read-modify-write without a lock; `start` treats any `systemctl start` failure as "no unit"; `why()` (journal reason) is consulted only on the systemctl-success path; `wait` is a 50 ms poll with a fixed 10 s deadline; the daemon only reconfigures on `enabled=false`, it never stops itself.
- **Reify:** High — persisted flag, unit state and socket liveness form one cross-process machine with several writers and representable mismatches.
- **Suspected bug:** a crash loop (e.g. a startup error) under `Restart=on-failure`/`RestartSec=2` can trip systemd's default start limit; turning dictation on or pressing the shortcut then makes `systemctl start` fail, so `Session::start` spawns an unmanaged `aizome-koe daemon` outside the unit with stderr to /dev/null (`desktop/aizome/apps/koe/src/service.rs:103-115`), and the window shows only "Koe's service didn't start" instead of the journal reason (confidence: low)
- **Related:** NixOS module `desktop/aizome/nix/module.nix:322` (`systemd.user.services.aizome-koe`, `wantedBy = aizome-session.target`)
- **Reproduction:** Partly confirmed (integration): [test and output](reproductions.md#kko-01)

<a id="kko-02"></a>

### KKO-02 — Koe window's view of the service switch
- **Where:** `desktop/aizome/apps/koe/src/app.rs:206` (`Koe.switching`), `:209` (`.started`), `:198` (`.status`), `:195` (`.config_error`), `:216` (`.message`); combined in `desktop/aizome/apps/koe/src/dictation.rs:81`
- **Encoding:** `switching: Option<bool>` (worker in flight and its direction), `started: Option<u64>` (tick of a successful start until status arrives, ≤10 ticks), `status: Option<Status>` from the subscription, `config.enabled`, `config_error`.
- **States:** `Idle` | `TurningOn` | `TurningOff` → `AwaitingStatus` (`started`) → `Running` | `NotRunning`, crossed with `enabled`; header also renders `Off but still running`, `On but not running`, `Settings unreadable`. Representable: `started.is_some() && status.is_some()` until the next message; `switching == Some(false)` followed by a stale `Status(Some)` from the stream.
- **Transitions:** `switch()` refuses while switching (`desktop/aizome/apps/koe/src/app.rs:375`), saves, sets `switching` (`desktop/aizome/apps/koe/src/app.rs:387`) and spawns the worker (`desktop/aizome/apps/koe/src/app.rs:389-411`); `Msg::Switched` clears `switching`, re-reads config, sets `started` or clears `status` (`desktop/aizome/apps/koe/src/app.rs:583-601`); `Msg::Status` clears `started`, sets `status` (`desktop/aizome/apps/koe/src/app.rs:569-575`); `Tick` expires `started` after 10 ticks (`desktop/aizome/apps/koe/src/app.rs:559-561`); `Msg::Reset` renames koe.toml aside and `switch(false)` (`desktop/aizome/apps/koe/src/app.rs:756-771`); `Enable(true)` diverts to a set-up download when no model (`desktop/aizome/apps/koe/src/app.rs:576-582`).
- **Hazards:** a mirror of KKO-01 refreshed by two channels (worker result, subscription) that can arrive in either order; requests during `switching` are silently dropped; `started` is a tick count rather than a deadline.
- **Reify:** High — five fields combine into one header state computed in another file (`desktop/aizome/apps/koe/src/dictation.rs:68-213`).
- **Related:** daemon status stream `desktop/aizome/apps/koe/src/daemon.rs:421-431`

<a id="kko-03"></a>

### KKO-03 — Model download, set-up (download then enable), and closing while downloading
- **Where:** `desktop/aizome/apps/koe/src/app.rs:92` (`Download.model`, `.done`, `.cancel`, `.then_enable`), `:204` (`Koe.download`), `:223` (`Koe.closing`)
- **Encoding:** `download: Option<Download>` whose `cancel: Arc<AtomicBool>` is shared with the `koe-download` thread; `then_enable`; `closing`; the partial file belongs to the thread (KKO-16).
- **States:** `NoDownload` → `Downloading{then_enable}` → `Cancelling` (cancel set) → `Finished(Ok|Err)`; orthogonal `Closing` (window waits for the thread). Representable: cancel set while a successful result is already on its way.
- **Transitions:** `start_download` refuses if one runs (`desktop/aizome/apps/koe/src/app.rs:416`), removes a damaged file (`desktop/aizome/apps/koe/src/app.rs:420-426`), sets `download` (`desktop/aizome/apps/koe/src/app.rs:428`), spawns the thread (`desktop/aizome/apps/koe/src/app.rs:433`) which sets `cancel` if the window's stream is gone (`desktop/aizome/apps/koe/src/app.rs:447-449`) and always sends `Downloaded` (`desktop/aizome/apps/koe/src/app.rs:455`); `Progress` (`desktop/aizome/apps/koe/src/app.rs:622-626`); `CancelDownload` (`desktop/aizome/apps/koe/src/app.rs:656-660`); `close_requested` asks first (`desktop/aizome/apps/koe/src/app.rs:790-800`); `Msg::Close` sets cancel and `closing` (`desktop/aizome/apps/koe/src/app.rs:773-778`); `Downloaded` takes `download`, closes when `closing`, else applies the model and `switch(true)` when `then_enable` (`desktop/aizome/apps/koe/src/app.rs:627-654`).
- **Hazards:** `then_enable` is dropped when the result lands while closing; window close depends on the thread always reporting (it relies on `catch_unwind`); a failed `change()` before `switch(true)` is ignored (`desktop/aizome/apps/koe/src/app.rs:642-645`).
- **Reify:** High — four fields across seven handlers, gating the switch (KKO-02) and window close.

<a id="kko-04"></a>

### KKO-04 — Window's status subscription reconnect loop
- **Where:** `desktop/aizome/apps/koe/src/app.rs:495` (`follow`, local `was_up`)
- **Encoding:** thread-local `was_up: Option<bool>` and the open stream.
- **States:** `Unknown` → `Connected` (streaming `Status(Some)`) → `Down` (one `Status(None)`) → reconnect every 500 ms.
- **Transitions:** connect + Subscribe (`desktop/aizome/apps/koe/src/app.rs:500-503`); each line sets `was_up = Some(true)` and sends status (`desktop/aizome/apps/koe/src/app.rs:510-511`); EOF, bad line or `!ok` breaks (`desktop/aizome/apps/koe/src/app.rs:505-509`); edge-triggered `Status(None)` (`desktop/aizome/apps/koe/src/app.rs:517-520`); sleep (`desktop/aizome/apps/koe/src/app.rs:523`); ends when the window's stream closes.
- **Hazards:** a daemon replying `ok=false` (protocol version mismatch) is reconnected to every 500 ms indefinitely.
- **Reify:** Low — small, local, edge-triggered.
- **Related:** `desktop/aizome/apps/koe/src/daemon.rs:415-432` (`serve` subscription)

<a id="kko-05"></a>

### KKO-05 — Window's koe.toml and settings.toml change detection
- **Where:** `desktop/aizome/apps/koe/src/app.rs:196` (`config_stamp`), `:214` (`settings_stamp`), `:183` (`stamp`)
- **Encoding:** (mtime, len, inode) of the last read; `config_error`.
- **States:** `Fresh` | `Changed` (stamp differs) | `Unreadable` (`config_error` Some).
- **Transitions:** `read_config(false)` compares and reloads (`desktop/aizome/apps/koe/src/app.rs:268-286`) every other tick (`desktop/aizome/apps/koe/src/app.rs:562-566`) and before each edit (`desktop/aizome/apps/koe/src/app.rs:350`); own save refreshes the stamp (`desktop/aizome/apps/koe/src/app.rs:359-361`); `read_shortcuts` (`desktop/aizome/apps/koe/src/app.rs:290-300`); forced reloads after `Switched` and `ShortcutsAdded` (`desktop/aizome/apps/koe/src/app.rs:585-586`, `:733`).
- **Hazards:** read-modify-write of koe.toml against the tray and CLI writers with only a stamp check just before (lost update if both write together); an unreadable settings.toml silently keeps the old bindings (`desktop/aizome/apps/koe/src/app.rs:296`).
- **Reify:** Low — change-detection cache.

<a id="kko-06"></a>

### KKO-06 — pw-record capture task
- **Where:** `desktop/aizome/apps/koe/src/audio.rs:55` (`Capture.stop`, `.task`); loop locals `quiet_samples`, `heard_speech`, `last_audio`, `current_level` (`desktop/aizome/apps/koe/src/audio.rs:79-83`); `PcmDecoder.pending` (`desktop/aizome/apps/koe/src/audio.rs:20`)
- **Encoding:** child process + reader task + `stop` oneshot; loop-local silence and no-audio watchdog counters; the decoder's carried odd byte.
- **States:** `Spawned` → `Listening` (`heard_speech=false`) → `HeardSpeech` → `Ended(Ok: stopped | limit | silence)` | `Failed(no audio 10 s | EOF | read error)`; child killed and reaped on every exit.
- **Transitions:** spawn pw-record (`desktop/aizome/apps/koe/src/audio.rs:72`) and task (`desktop/aizome/apps/koe/src/audio.rs:75`); stop → Ok (`desktop/aizome/apps/koe/src/audio.rs:87`); watchdog → Err (`desktop/aizome/apps/koe/src/audio.rs:92-94`); EOF → Err (`desktop/aizome/apps/koe/src/audio.rs:97`); speech/quiet accounting (`desktop/aizome/apps/koe/src/audio.rs:102-103`); limit or silence → Ok (`desktop/aizome/apps/koe/src/audio.rs:104-106`); kill + wait (`desktop/aizome/apps/koe/src/audio.rs:111-112`); `finish` (`desktop/aizome/apps/koe/src/audio.rs:117-120`).
- **Hazards:** meters dropped by `try_send` when the daemon lags (intended); split-sample carry across reads (`desktop/aizome/apps/koe/src/audio.rs:26-37`).
- **Reify:** Low — loop-local and contained.
- **Related:** owner `desktop/aizome/apps/koe/src/daemon.rs:47` (`State.capture`, KKO-07)

<a id="kko-07"></a>

### KKO-07 — Daemon dictation pipeline (record → transcribe → deliver)
- **Where:** `desktop/aizome/apps/koe/src/daemon.rs:41` (`State`), fields `.status.phase` (`desktop/aizome/apps/koe/src/protocol.rs:107`), `.capture` (`desktop/aizome/apps/koe/src/daemon.rs:47`), `.meter_rx` (`:48`), `.samples` (`:49`), `.inference` (`:50`), `.delivery` (`:51`), `.cancelled` (`:52`), `.last` (`:53`), `.target` (`:54`), `.reported_phase` (`:55`), `.transcription_started` (`:56`)
- **Encoding:** partially explicit — `Phase` enum, but each phase's data lives in separate Options (capture, inference and delivery JoinHandles, samples, target, timer) and transitions are spread across `command`, `fail`, `cancel`, `recognize`, `begin_capture` and five `select!` arms in `run`.
- **States:** `Idle` → `Recording` ⇄ `Paused` → `Finishing` → `Transcribing` → `Delivering` → `Idle`; any → `Error`; `Error` → `Recording` | `Idle` | `Delivering` (PasteLast). Representable invalid: `phase==Recording && capture.is_none()`, `phase==Idle && inference.is_some()`, `phase==Transcribing && delivery.is_some()`.
- **Transitions:** Toggle maps phase to Start/Stop (`desktop/aizome/apps/koe/src/daemon.rs:283-297`); Start → `begin_capture` → Recording (`desktop/aizome/apps/koe/src/daemon.rs:302-326`, `:100`); Stop → Finishing, `stop_capture`, `recognize` → Transcribing (`desktop/aizome/apps/koe/src/daemon.rs:327-336`, `:128`); Pause/Resume (`desktop/aizome/apps/koe/src/daemon.rs:337-350`); capture ends by itself → `recognize` or `fail` (`desktop/aizome/apps/koe/src/daemon.rs:565-577`); inference done → `last`, Delivering, spawn delivery (`desktop/aizome/apps/koe/src/daemon.rs:578-607`); delivery done → Idle (`desktop/aizome/apps/koe/src/daemon.rs:611-617`); PasteLast → Delivering (`desktop/aizome/apps/koe/src/daemon.rs:355-366`); Cancel/Shutdown → `cancel` → Idle (`desktop/aizome/apps/koe/src/daemon.rs:351`, `:134-152`); `fail` → Error (`desktop/aizome/apps/koe/src/daemon.rs:77-88`); meter arm (`desktop/aizome/apps/koe/src/daemon.rs:560-564`).
- **Hazards:** Start awaits the compositor's focused-window inside the single command loop (`desktop/aizome/apps/koe/src/daemon.rs:310`), stalling every client; `cancel` aborts JoinHandles but a local Whisper `spawn_blocking` holds the model mutex until it polls the abort flag; `publish()` recomputes `has_last`/`teaching`/`model_loaded` only when called; the invalid combinations are prevented only by handler ordering.
- **Reify:** High — explicit Phase plus ten phase-dependent fields mutated in about fifteen places.
- **Related:** compositor IPC `desktop/aizome/crates/aizome-comp-core/src/requests.rs`; shell notifications `desktop/aizome/crates/aizome-shell-proto/src/lib.rs`; clients `desktop/aizome/apps/koe/src/tray.rs:244`, `desktop/aizome/apps/koe/src/main.rs:778`, `desktop/aizome/apps/koe/src/app.rs:484`

<a id="kko-08"></a>

### KKO-08 — Teaching session lease
- **Where:** `desktop/aizome/apps/koe/src/daemon.rs:31` (`Teaching.token`, `.revision`, `.samples`, `.preview`, `.heartbeat`), `:59` (`State.teaching`), `:60` (`State.playback`)
- **Encoding:** `teaching: Option<Teaching>` overlaid on the KKO-07 phase; `heartbeat: Instant` lease; `preview: Option<TeachingPreview>`; `playback: Option<JoinHandle>`; capture limits changed while teaching (`desktop/aizome/apps/koe/src/daemon.rs:92-95`).
- **States:** `None` → `Recording(token)` → `Recognising` → `Review(preview)` (phase Idle) → `Saved` | `Discarded` | `Expired`; `Failed` (phase Error, teaching Some, preview None). Representable: `teaching.is_some() && phase==Delivering` (prevented by the TeachStart guard and the inference arm's early `continue`).
- **Transitions:** TeachStart guard and set (`desktop/aizome/apps/koe/src/daemon.rs:191-210`), unset if capture fails (`:211-214`); every token command refreshes the lease (`:229`); TeachStop → `command(Stop)` (`:231-233`); inference result → preview + Idle (`:584-597`) or `fail` above 4096 bytes (`:586-589`); TeachSave persists then `cancel()` (`:236-267`); TeachPlay spawns playback (`:269-277`), reaped by its arm (`:519-522`); `cancel` clears (`:144`); watchdog expires after 8 s without a request (`:513-518`).
- **Hazards:** any `fail()` while teaching leaves `teaching` Some in phase Error until Discard or expiry, blocking Start, PasteLast and settings reload (`idle()`, `desktop/aizome/apps/koe/src/daemon.rs:154-160`); the lease relies on the window polling every 0.5 s while it isn't busy; a TeachStart reply lost to the 5 s client timeout leaves the microphone on until the lease expires.
- **Reify:** High — overlay on another machine with its own lease timer, scattered across `vocabulary()`, `command()`, `cancel()` and three `select!` arms.
- **Related:** window side `desktop/aizome/apps/koe/src/vocab.rs:89` (KKO-18)

<a id="kko-09"></a>

### KKO-09 — Deferred settings reload and model warmup
- **Where:** `desktop/aizome/apps/koe/src/daemon.rs:486` (`seen`), `:487` (`saved: Option<Config>`), `:478` (`warmup`), `:452` (`shared_config`)
- **Encoding:** settings file stamp, a pending config, the warmup task handle, an Arc snapshot for store commands.
- **States:** `Current` → `Pending(config)` (file changed while busy) → `Applied` → `WarmingUp` → `Ready`.
- **Transitions:** 1 s tick compares the stamp and sets or clears `saved` (`desktop/aizome/apps/koe/src/daemon.rs:491-501`); applies when `state.idle()` (`:502-511`); `reconfigure` swaps processor and, on engine change, recognizer and a new warmup (`:164-174`); warmup arm (`:523-530`).
- **Hazards:** `idle()` ignores a warmup in flight, so `reconfigure` replaces `warmup` and detaches the old task, which keeps loading the old model (two models resident for a while); a load error keeps an earlier pending `saved` that is applied later although the log says "keeping the settings in use"; Unload during a not-yet-running warmup is undone when it loads.
- **Reify:** Medium — three fields in one loop arm, guarded by `idle()`.

<a id="kko-10"></a>

### KKO-10 — Daemon IPC connection: request, subscription and shutdown flush
- **Where:** `desktop/aizome/apps/koe/src/daemon.rs:382` (`serve`), `:30` (`Command` carrying the `flushed` oneshot), `:449` (`permits`)
- **Encoding:** a task per connection, a 64-permit semaphore, reply and flush oneshots, a watch receiver.
- **States:** `Accepted` → `ReadRequest` (3 s) → `VersionRejected` | `StoreCommand` | `Queued` → `Replied` → `Subscribed` (until the client sends anything or closes) → `Closed`; for Shutdown `Replied` → `Flushed` (≤1 s) → daemon exits.
- **Transitions:** accept + permit (`desktop/aizome/apps/koe/src/daemon.rs:533-538`); read with timeout (`:391-392`); version check (`:394-399`); store commands run on the blocking pool, off the loop (`:401-414`); queue to the loop and await the reply (`:416-419`); flush signal (`:420`); subscription loop (`:421-432`); Shutdown waits for flush then breaks (`:555-558`).
- **Hazards:** connections past 64 are dropped silently (`:535`); each subscriber holds a permit for its lifetime; the first `changed()` re-sends the status just replied.
- **Reify:** Low — linear per-connection flow.
- **Related:** clients `desktop/aizome/apps/koe/src/ipc.rs:42-85`, `desktop/aizome/apps/koe/src/app.rs:495`, `desktop/aizome/apps/koe/src/main.rs:788`

<a id="kko-11"></a>

### KKO-11 — Paste delivery guard
- **Where:** `desktop/aizome/apps/koe/src/desktop.rs:118` (`deliver`), `:81` (`wait_for_release`), `:69` (`check_destination`); run as `State.delivery` (`desktop/aizome/apps/koe/src/daemon.rs:51`)
- **Encoding:** sequential async steps; `target: Option<u64>` captured at Start; 5 s timeout per wait; compositor polled every 25 ms.
- **States:** `WaitRelease` (lock and focus checked each poll) → `Copied` → `Delay` → `WaitRelease` → `FocusCheck` → `Pasted`; `Failed(locked | focus changed | keys held 5 s)`; Clipboard and None modes stop early.
- **Transitions:** first wait (`desktop/aizome/apps/koe/src/desktop.rs:124`); copy (`:126`); clipboard-only return (`:127-129`); delay (`:130`); second wait (`:132`); focus re-check (`:133-137`); synthesize (`:139-141`) or wtype (`:143-158`); cancelled by aborting the task (`desktop/aizome/apps/koe/src/daemon.rs:139-141`).
- **Hazards:** if focus changes or the desktop locks during `paste_delay_ms`, the error says "your words are kept" but the clipboard was already replaced; up to about 10 s of polling per delivery.
- **Reify:** Medium — linear, but safety-relevant ordering a step enum would make auditable.
- **Related:** compositor `desktop/aizome/crates/aizome-comp-core/src/requests.rs` (`focused-window`, `input-state`, `recovery-state`, `synthesize`)

<a id="kko-12"></a>

### KKO-12 — Resident Whisper model
- **Where:** `desktop/aizome/apps/koe/src/engine.rs:17` (`Recognizer.context`), `:18` (`.loaded`), `:20` (`.gpu`)
- **Encoding:** `Arc<Mutex<Option<Resident>>>` mirrored by two atomics; a per-call cancel `AtomicBool` passed to Whisper's abort callback.
- **States:** `Unloaded` → `Loading` (under the lock) → `Loaded{gpu}` ⇄ `Busy` (inference holds the lock) → `Unloaded`. `loaded` and `context` are updated in separate steps, so a reader can see them disagree.
- **Transitions:** first use loads (`desktop/aizome/apps/koe/src/engine.rs:131-148`, `loaded` `:147`); `unload` via `try_lock` then `loaded=false` (`:98-104`); cancel checks (`:107`, `:130`, `:149`, `:182`); abort callback (`:178-179`); `with_prompt` clones share the slot (`:80-84`); the daemon replaces the whole Recognizer on engine change (`desktop/aizome/apps/koe/src/daemon.rs:167`).
- **Hazards:** Unload while a warmup is queued but not yet holding the lock succeeds, then the warmup reloads; `on_gpu` reads two atomics without the lock.
- **Reify:** Medium — mutex-guarded, with duplicated truth in atomics.

<a id="kko-13"></a>

### KKO-13 — Single-instance socket hand-off (Koe and Kura windows)
- **Where:** `desktop/aizome/apps/koe/src/instance.rs:30` (`claim`), `:50` (`serve`); `desktop/aizome/apps/kura/src/ui/instance.rs:25` (`claim`), `:46` (`serve`)
- **Encoding:** `$XDG_RUNTIME_DIR/aizome-app.{koe,kura}.sock`; the `Claim` result; a listener thread.
- **States:** `Probe` → `Handed` (connect and write succeed) | `Stale` (socket removed) → `First` (bound) | `Alone` (bind failed).
- **Transitions:** connect + write (`desktop/aizome/apps/koe/src/instance.rs:32-35`, `desktop/aizome/apps/kura/src/ui/instance.rs:27-31`); remove a stale socket (`desktop/aizome/apps/koe/src/instance.rs:39`, `desktop/aizome/apps/kura/src/ui/instance.rs:35`); bind (`desktop/aizome/apps/koe/src/instance.rs:40`, `desktop/aizome/apps/kura/src/ui/instance.rs:36`); serve threads forward args until the app's stream closes (`desktop/aizome/apps/koe/src/instance.rs:55-67`, `desktop/aizome/apps/kura/src/ui/instance.rs:51-62`).
- **Hazards:** two simultaneous launches can both find no listener and the second's `remove_file` unlinks the first's socket, giving two windows; a hung first instance still accepts (backlog), so a relaunch exits silently; the socket is never removed on exit.
- **Reify:** Low — small, duplicated in two apps.

<a id="kko-14"></a>

### KKO-14 — Daemon single-instance lock and control socket file
- **Where:** `desktop/aizome/apps/koe/src/ipc.rs:90` (`SocketGuard.path`, `._lock`)
- **Encoding:** flock on `daemon.lock` held for the process lifetime; control.sock; 0700 runtime-dir check.
- **States:** `NoDaemon` → `Locked` → `Bound` → `Removed` (Drop); a crash leaves a stale socket that the next bind removes.
- **Transitions:** create the dir and check owner and mode (`desktop/aizome/apps/koe/src/ipc.rs:97-108`); `try_lock` or "already running" (`:117`); remove stale socket (`:118-120`); bind + chmod (`:121-122`); Drop unlinks (`:127-130`).
- **Hazards:** none observed beyond the stale socket after a crash (cleared on the next start).
- **Reify:** Low — correct and local.

<a id="kko-15"></a>

### KKO-15 — CLI vocabulary edit retry
- **Where:** `desktop/aizome/apps/koe/src/main.rs:530` (`Vocab::edit`, local `attempts`)
- **Encoding:** loop-local attempt counter; library revision as an optimistic token.
- **States:** `Load` → `Edit` → `Save` → `Done` | `Retry` (revision moved, ≤3) | `Fail`.
- **Transitions:** load (`desktop/aizome/apps/koe/src/main.rs:533`); save via `SaveLibrary` (`:536`); retry when the stored revision changed (`:538`); give up (`:539`).
- **Hazards:** the edit closure re-runs on each attempt; after three conflicts the error says "changed in another window" even from a script.
- **Reify:** Low — local retry loop.
- **Related:** `desktop/aizome/apps/koe/src/vocabulary.rs:289` (KKO-19)

<a id="kko-16"></a>

### KKO-16 — Model partial-download files
- **Where:** `desktop/aizome/apps/koe/src/models.rs:191` (`fetch`), `:149` (`ABANDONED`), `:153` (`remove_abandoned`)
- **Encoding:** `.ggml-<name>.bin.*.part` tempfile in the models folder; its mtime age as the liveness signal; a cancel `AtomicBool` polled every 50 ms.
- **States:** `Missing` → `Partial` (writing) → `Verified` → `Installed`; `Partial` → removed (cancel or error drops the tempfile) | `Abandoned` (older than 10 min after a crash or logout) → swept by the next download.
- **Transitions:** existing file verified or rejected (`desktop/aizome/apps/koe/src/models.rs:200-203`); sweep (`:204`); tempfile (`:232-233`); cancel mid-stream (`:238-240`) or while waiting via select (`:185-188`); size and hash check (`:247-250`); `persist_noclobber` (`:252`).
- **Hazards:** liveness by mtime assumes a write at least every 10 min; a window and a CLI download of the same model both write partials and the loser fails at persist.
- **Reify:** Low — filesystem-encoded and self-cleaning.
- **Related:** window side KKO-03 (`desktop/aizome/apps/koe/src/app.rs:433`)

<a id="kko-17"></a>

### KKO-17 — Tray mirror of daemon status and its own error
- **Where:** `desktop/aizome/apps/koe/src/tray.rs:21` (`Tray.status`, `.error`), `:190` (`previous`)
- **Encoding:** last status shown on SNI, a tray-local `error`, a de-dup snapshot, a sequential command loop.
- **States:** labels per `Phase` × `teaching` (`desktop/aizome/apps/koe/src/tray.rs:28-46`), `NeedsAttention` while `error` is Some; command loop `Idle` → `Acting` (200 ms delay, then IPC).
- **Transitions:** de-duplicated status applied, clearing `error` on a phase change (`desktop/aizome/apps/koe/src/tray.rs:206-215`); menu item → command (`:217-245`); `error` from the result (`:250`); stream closed → unregister (`:203`, `:254`).
- **Hazards:** status updates wait while a command runs (up to the 5 s IPC timeout); `error` lingers until the phase changes; activations are dropped when the 8-slot channel is full (`:52`).
- **Reify:** Low — a projection of KKO-07.
- **Related:** SNI host `desktop/aizome/crates/aizome-sys/src/statusnotifier/mod.rs`

<a id="kko-18"></a>

### KKO-18 — Vocabulary page: request queue, stale-read guard, teaching client, closing
- **Where:** `desktop/aizome/apps/koe/src/vocab.rs:75` (`Vocabulary`), fields `.status` (`:88`), `.token` (`:89`), `.preview` (`:90`), `.examples_for` (`:94`), `.loaded` (`:96`), `.busy` (`:97`), `.queued` (`:98`), `.sent` (`:101`), `.reading` (`:102`), `.ticks` (`:103`), `.closing` (`:105`)
- **Encoding:** busy flag + FIFO of mutating requests; `sent` counter as an epoch for reads; `reading` count; teaching token and preview mirroring KKO-08; closing flag.
- **States:** requests `Idle` → `Busy(+queued)` → `Idle`; reads `InFlight(sent)` → `Applied` | `Dropped`; teaching `NoToken` → `Starting` → `Recording` → `Recognising` → `Review(preview)` → `Saved` | `Discarded` | `Lost`; window `Open` → `Closing` (wait for busy, then TeachCancel) → `Closed`. Representable: `preview.is_some() && token.is_none()` (cleared together by hand at four sites).
- **Transitions:** `request` (`desktop/aizome/apps/koe/src/vocab.rs:319-331`); `read` (`:333-341`); Tick chooses TeachPoll, Library or Examples (`:394-405`); `Read` dropped if `sent` moved or closing, TeachPoll errors clear the token (`:406-426`); `Reply` clears busy, clears the token on teaching errors or after Cancel/Save, then pops the queue or continues closing (`:427-475`); Record sets the token and sends TeachStart (`:561-574`); `set_status(None)` discards the example (`:151-163`); `close`/`finish_close` (`:351-370`).
- **Hazards:** queued `SaveLibrary` commands carry library snapshots whose revision is stale once the first save lands; the token's lease depends on a poll that pauses while busy; a read spawned inside a `Reply` that then pops a queued request is dropped.
- **Reify:** High — twelve fields, four interleaved machines, many handlers.
- **Related:** daemon lease `desktop/aizome/apps/koe/src/daemon.rs:31` (KKO-08); store `desktop/aizome/apps/koe/src/vocabulary.rs:289` (KKO-19)

<a id="kko-19"></a>

### KKO-19 — Vocabulary store: revision token, lock and staged saves
- **Where:** `desktop/aizome/apps/koe/src/vocabulary.rs:41` (`Library.revision`), `:447` (`Store::lock`), `:338` (`save_example`), `:420` (`export_examples`)
- **Encoding:** increasing `revision` in library.json (optimistic concurrency); flock on library.lock; `.saving-*` and `.koe-export-*` temp folders renamed into place; one folder per saved example.
- **States:** library `Rev(n)` → `Rev(n+1)`; example `Staging` (hidden temp folder) → `Saved` → `Deleted`; export `Staging` → `Exported`.
- **Transitions:** `save` locks, checks and bumps the revision, writes atomically (`desktop/aizome/apps/koe/src/vocabulary.rs:290`, `:292`, `:304`, `:307`); `import` loads unlocked then `save` (`:314`, `:330`); `save_example` lock, word check, stage, rename (`:340-367`); `delete_example` (`:412-418`); `export_examples` (`:421-444`).
- **Hazards:** a crash between staging and rename leaves a hidden `.saving-*` folder with audio that nothing sweeps; one malformed visible entry fails the whole `examples()` listing; `Example.vocabulary_revision` is recorded but never checked.
- **Reify:** Medium — sound token and lock; staging leftovers are implicit.

<a id="kko-20"></a>

### KKO-20 — (test harness) Real-Aizome dictation regression driver
- **Where:** `desktop/aizome/apps/koe/tests/aizome.rs:18` (`Process`), `:28` (`wait_file`)
- **Encoding:** spawned headless compositor, daemon and foot; marker files; status and focus polling loops.
- **States:** `CompositorUp` → `DaemonUp` → `TerminalReady` → `Focused` → `ModelWarm` → per attempt `Recording` → (`Delivering` with keys held) → `Idle && has_last`.
- **Transitions:** `wait_file` (`desktop/aizome/apps/koe/tests/aizome.rs:125`, `:139`, `:154`); focus poll (`:155-165`); warmup poll (`:167-177`); attempt loop (`:178-251`).
- **Hazards:** fixed 5–60 s timeouts; ignored by default.
- **Reify:** Low — test driver.

<a id="kko-21"></a>

### KKO-21 — (test harness) Daemon process harness with fake microphone, ASR server, compositor and shell
- **Where:** `desktop/aizome/apps/koe/tests/daemon.rs:32` (`Harness`), `:604` (`DesktopState.held`, `.locked`, `.changed_focus`, `.input_reads`, `.pastes`), `:833` (`Quits`)
- **Encoding:** daemon child killed on drop; script fakes for pw-record, wl-copy and pw-play; a one-request HTTP server; a fake compositor with mutable desktop state; status polling.
- **States:** daemon `Spawned` → `SocketReady` → driven through phases; fake desktop `Held` → `Released` | `Locked` | `FocusChanged`.
- **Transitions:** socket wait (`desktop/aizome/apps/koe/tests/daemon.rs:115-121`); `wait_for` (`:166-178`); `record_audio` (`:179-188`); fake compositor replies (`:617-651`); outcome switch (`:665-684`).
- **Hazards:** 5 s polling caps; the HTTP fake serves exactly one request.
- **Reify:** Low — test harness.

<a id="kko-22"></a>

### KKO-22 — (test harness) Window set-up rig with a gated service and a stalling model server
- **Where:** `desktop/aizome/apps/koe/tests/setup.rs:29` (`Recorder.calls`, `.held`), `:63` (`model_server`), `:131` (`Rig::settle`)
- **Encoding:** `held` gate on start/stop; per-chunk pause; settle loop with a 15 s deadline.
- **States:** service `Released` ⇄ `Held`; server `Serving` → `Stalled`.
- **Transitions:** `hold`/`release` (`desktop/aizome/apps/koe/tests/setup.rs:38-43`); `call` spins while held (`:44-50`); chunk pause (`:82-87`); settle (`:131-141`).
- **Hazards:** 5 ms spin-wait.
- **Reify:** Low — test harness.

<a id="kko-23"></a>

### KKO-23 — (test harness) Private D-Bus tray roundtrip with host restart
- **Where:** `desktop/aizome/apps/koe/tests/tray.rs:29` (`Watcher`), `:40` (`wait_phase`), `:90` (`exercise_tray`)
- **Encoding:** private dbus-daemon, a fake StatusNotifierWatcher, a daemon child, phase polling.
- **States:** `Registered` → `Recording` → `Paused` → `Recording` → `Idle` → host restart → `Re-registered` → `TurnedOff` (daemon exits 0).
- **Transitions:** registration wait (`desktop/aizome/apps/koe/tests/tray.rs:139-140`); clicks + `wait_phase` (`:154-168`); host replaced (`:170-179`); Turn Off and exit wait (`:188-198`).
- **Hazards:** none observed; ignored by default.
- **Reify:** Low — test harness.

<a id="kko-24"></a>

### KKO-24 — CLI scan progress ticker
- **Where:** `desktop/aizome/apps/kura/src/cli.rs:190` (`spawn_ticker`, `stop`)
- **Encoding:** `Arc<AtomicBool>` stop flag + JoinHandle.
- **States:** `Ticking` → `Stopped` (joined).
- **Transitions:** spawned when stderr is a terminal (`desktop/aizome/apps/kura/src/cli.rs:164`); loop every 100 ms (`:194-206`); stop + join after the scan (`:168-172`).
- **Hazards:** none observed.
- **Reify:** Low — trivial.

<a id="kko-25"></a>

### KKO-25 — Kura's on-disk caches: index.json/index.md and nix-store.json
- **Where:** `desktop/aizome/apps/kura/src/index.rs:418` (`write`), `:429` (`read`); `desktop/aizome/apps/kura/src/nix.rs:90` (`store_size`), `:47` (`StoreSize.db_mtime`); `desktop/aizome/apps/kura/src/lib.rs:64` (`write_atomic`)
- **Encoding:** index.json and index.md replaced atomically one after the other; nix-store.json keyed by the Nix database's mtime; temp file `.<name>.<pid>.tmp`.
- **States:** index `Absent` → `Current(scan)` → `Stale` (never detected); store cache `Valid` (db_mtime matches) | `Invalid` → remeasured.
- **Transitions:** `index::write` (`desktop/aizome/apps/kura/src/index.rs:423-424`) after each CLI scan (`desktop/aizome/apps/kura/src/cli.rs:186`) and GUI analysis (`desktop/aizome/apps/kura/src/ui/mod.rs:469`); `load_or_scan` reuses any readable index when no PATH is given (`desktop/aizome/apps/kura/src/cli.rs:246-250`); `read` refuses newer versions (`desktop/aizome/apps/kura/src/index.rs:432`); `store_size` cache hit (`desktop/aizome/apps/kura/src/nix.rs:95-102`) else `nix path-info` and rewrite (`:103-122`).
- **Hazards:** `report`/`hints` without PATH serve the last index whatever its age or root; the two renames are separate, so a crash between leaves json and md from different scans; concurrent GUI analyses race to write last (KKO-31); db mtime has one-second resolution.
- **Reify:** Low — persisted caches with simple validity rules.
- **Related:** index format for agents `desktop/aizome/docs/kura-index.md`

<a id="kko-26"></a>

### KKO-26 — Parallel scanner work queue
- **Where:** `desktop/aizome/apps/kura/src/scan.rs:171` (`Queue.jobs`, `.busy`, `.done`), `:58` (`Progress.cancel`), `:156` (`Shared.queue`, `.wake`)
- **Encoding:** mutex + condvar job stack, a busy-worker count, a done flag, a shared cancel atomic.
- **States:** `Running` (jobs or busy>0) → `Done` (jobs empty and busy==0) | `Cancelled`; per worker `Waiting` ⇄ `Reading`.
- **Transitions:** pop + `busy+=1` (`desktop/aizome/apps/kura/src/scan.rs:277-279`); push subfolders, `busy-=1`, notify (`:296-308`); last idle worker sets done (`:281-285`); cancel observed under the lock clears jobs (`:271-276`) or mid-folder (`:329-331`); `scan` returns `Cancelled` after join (`:238-240`).
- **Hazards:** `Progress::cancel` doesn't notify the condvar, so waiting workers notice only when a busy one finishes; a worker that panics never decrements `busy`, the rest wait forever and `scan` never returns.
- **Reify:** Medium — implicit termination detection; correct today but fragile.

<a id="kko-27"></a>

### KKO-27 — Freedesktop Trash moves
- **Where:** `desktop/aizome/apps/kura/src/trash.rs:95` (`trash_one`), `:154` (`add_directory_size`), `:206` (`empty`)
- **Encoding:** an exclusively created `info/<name>.trashinfo` claims a name; `files/<name>` by rename; the `directorysizes` cache.
- **States:** per item `Refused` | `Claimed` (info written) → `Trashed` | rolled back (info removed); trash `NonEmpty` → `Emptied`.
- **Transitions:** refusal checks (`desktop/aizome/apps/kura/src/trash.rs:32-68`); name search (`:103-119`); claim (`:121-125`); write+sync or roll back (`:126-129`); rename or roll back (`:131-134`); directorysizes append (`:135-138`, `:154-162`); `empty` (`:212-231`).
- **Hazards:** a crash between claim and rename leaves an orphan .trashinfo; `directorysizes` is read-modify-write without a lock; refusal and rename are separate steps; `empty` stops at the first error, leaving a partly emptied Trash.
- **Reify:** Low — protocol-defined and small.
- **Related:** other freedesktop file managers sharing `~/.local/share/Trash`

<a id="kko-28"></a>

### KKO-28 — Tree arena tombstones and grafts
- **Where:** `desktop/aizome/apps/kura/src/tree.rs:48` (`flags::REMOVED`), `:57` (`Node.flags`), `:330` (`remove`), `:354` (`graft`)
- **Encoding:** a build phase (`push_children`, then `finish` sums and sorts); afterwards nodes are flagged `REMOVED` in place and rescanned subtrees appended; liveness derived by `gone()`.
- **States:** node `Live` → `Removed` (trashed, sizes zeroed) | `Replaced` (old children of a graft, unreachable); tree `Building` → `Finished(sorted)` → `Edited` (sibling order may be stale).
- **Transitions:** `push_children` (`desktop/aizome/apps/kura/src/tree.rs:127-151`); `finish` (`:160-178`); `remove` subtracts up the ancestors and flags (`:330-347`); `graft` flags old children, appends, adjusts ancestors and overwrites `at.flags` (`:357-392`).
- **Hazards:** `find` and `children` still return `REMOVED` nodes, so every caller must filter with `gone()`; `graft` onto a node removed meanwhile clears its `REMOVED` flag (`:392`) and adds its size back to the ancestors; NodeIds under a grafted folder change, so holders must remap by path (KKO-34).
- **Reify:** Medium — explicit flags, but the invariants live in the callers.

<a id="kko-29"></a>

### KKO-29 — Dragging into the Collector (sunburst and list rows)
- **Where:** `desktop/aizome/apps/kura/src/ui/drag.rs:39` (`Gesture.start`, `.dragging`, `.over`), `:20` (`ZoneCell`); `desktop/aizome/apps/kura/src/ui/mod.rs:204` (`Kura.hover`), `:205` (`.press`), `:216` (`.dragging`), `:217` (`.drag_over`), `:218` (`.zone`)
- **Encoding:** two implementations — rows keep a `Gesture` in widget state and capture the pointer; the sunburst keeps `press`/`dragging`/`drag_over` in program state; the drop zone writes its rectangle into a shared cell when painted.
- **States:** `Idle` → `Pressed` → (past the slop) `Dragging` ⇄ `OverCollector` → `Dropped` (collected) | `Cancelled`; a release before the slop is a click.
- **Transitions:** row press/double-click (`desktop/aizome/apps/kura/src/ui/drag.rs:174-183`), move → Start/Over (`:49-68`), release → Drop/Cancel (`:72-80`); program Drag events (`desktop/aizome/apps/kura/src/ui/mod.rs:657-672`); sunburst press (`desktop/aizome/apps/kura/src/ui/sunburst.rs:311-313`), drag start/over (`:298-305`), release (`:323-325`) → `Msg::Release` (`desktop/aizome/apps/kura/src/ui/mod.rs:675-686`); `refresh_rows` clears hover and press but not `dragging` (`desktop/aizome/apps/kura/src/ui/mod.rs:395-396`).
- **Hazards:** a tree change mid-drag (Moved/Rescanned) leaves `dragging` holding a NodeId that may have been replaced; the zone rectangle is whatever was last painted; two code paths must agree on Start/Over/Drop.
- **Reify:** Medium — duplicated gesture logic across widget and program state.

<a id="kko-30"></a>

### KKO-30 — Kura scan lifecycle and page
- **Where:** `desktop/aizome/apps/kura/src/ui/mod.rs:52` (`enum Page`), `:59` (`Scanning.progress`, `.timer`), `:189` (`Kura.page`), `:193` (`.scanning`), `:194` (`.scan`), `:229` (`.last_root`)
- **Encoding:** partially explicit — `Page` enum, but the running scan is `scanning: Option<Scanning>` (progress, poll timer) and the result `scan`; the worker's `Msg::Scanned` carries no identity.
- **States:** `Disks` → `Scanning(root)` → `Results`, or back to `Disks`/`Results` with a notice on error or stop; ShowDisks during a scan gives `page==Disks && scanning.is_some()`. Representable invalid: a scan worker running while `scanning == None` (orphaned).
- **Transitions:** `start_scan` cancels the previous progress, drops its timer, spawns, sets the page (`desktop/aizome/apps/kura/src/ui/mod.rs:420-441`); Poll (`:578-582`); Stop (`:583-587`); `Scanned` → `finish_scan`, then Results or the error page (`:588-613`); Rescan (`:615-620`); ShowDisks (`:553-556`); Launched and `open` (`:802-807`, `:540-544`).
- **Hazards:** `Scanned` tears down whichever scan is current (`:589`) and applies its result without checking that it belongs to it; Stop and Rescan enablement read `scanning` (`:832-834`).
- **Reify:** High — scan identity is implicit and results are applied unconditionally.
- **Suspected bug:** while a scan runs, press ⌘D and Scan another disk (or ⌘N/⌘O, or launch `aizome-kura PATH` again): `start_scan` cancels the first scan, whose `Scanned(Err(Cancelled))` then arrives and `finish_scan` cancels the second scan's poll timer and clears `scanning` (`desktop/aizome/apps/kura/src/ui/mod.rs:589`), so the window shows "The scan didn't finish: the scan was stopped" while the second scan runs unseen and can no longer be stopped; its result later replaces the page unannounced, and a Rescan meanwhile starts a third scan without cancelling it (confidence: high)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#kko-30)

<a id="kko-31"></a>

### KKO-31 — Analysis and index writing after a scan or a change
- **Where:** `desktop/aizome/apps/kura/src/ui/mod.rs:69` (`enum IndexState`), `:221` (`Kura.index`), `:239` (`Kura.analysis`), `:453` (`write_index`)
- **Encoding:** partially explicit — `IndexState` enum, but `analysis` duplicates its data, several analyses can be in flight, and `Msg::Indexed` carries no generation.
- **States:** `None` → `Writing` → `Written{json, reclaimable}` | `Failed`; per worker `Analysing` → `WroteIndex` → `Delivered`.
- **Transitions:** `write_index` sets Writing and spawns (`desktop/aizome/apps/kura/src/ui/mod.rs:460-481`), called from `Scanned` (`:607`), from `rescan` with nothing to rescan (`desktop/aizome/apps/kura/src/ui/suggest.rs:349`) and from `rescanned` (`desktop/aizome/apps/kura/src/ui/suggest.rs:405`); `Scanned` resets `analysis` (`desktop/aizome/apps/kura/src/ui/mod.rs:601`); `Indexed` sets state and `analysis`, re-highlights (`:738-745`).
- **Hazards:** the last analysis to arrive wins, not the newest; each worker writes index.json itself, so the file reflects whichever finished last; a panicking worker leaves `Writing` forever; `Arc::make_mut` in `moved`/`rescanned` deep-copies the tree while an analysis still holds it.
- **Reify:** High — concurrent results without an epoch, visible to people and agents.
- **Suspected bug:** scan the home folder (duplicate hashing keeps its analysis busy for a while), then scan a small folder, or trash something so a second analysis starts and finishes first: the first analysis's `Indexed` lands last, so the Suggestions panel shows hints and totals for the earlier tree (another root, or paths already in the Trash) with live Move to Trash buttons, and index.json/index.md describe the older scan (confidence: medium)
- **Related:** index consumers (KKO-25, `desktop/aizome/docs/kura-index.md`)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#kko-31)

<a id="kko-32"></a>

### KKO-32 — Launch-time background loads (disks, Nix store size)
- **Where:** `desktop/aizome/apps/kura/src/ui/mod.rs:190` (`mounts`), `:191` (`store`), `:192` (`measuring_store`)
- **Encoding:** empty `mounts` means loading; `measuring_store` + `store`.
- **States:** store `Measuring` → `Measured(size)` | `Unknown`; mounts `Loading` → `Listed` (relisted on ShowDisks).
- **Transitions:** `launched` spawns both (`desktop/aizome/apps/kura/src/ui/mod.rs:532-537`); ShowDisks relists (`:555`); `Disks` (`:557`); `StoreMeasured` (`:558-561`).
- **Hazards:** overlapping listings apply in arrival order; a genuinely empty mount list reads "Looking for disks…" forever (`desktop/aizome/apps/kura/src/ui/view.rs:96-98`).
- **Reify:** Low.

<a id="kko-33"></a>

### KKO-33 — Sunburst centre transition animation
- **Where:** `desktop/aizome/apps/kura/src/ui/mod.rs:214` (`transition`), `:203` (`outer_segs`); `desktop/aizome/apps/kura/src/ui/sunburst.rs:40` (`Transition.started`, `.duration`)
- **Encoding:** `Option<Transition>` with a start Instant, a separate `outer_segs` cache, a self-rearming `Msg::Frame` timer.
- **States:** `AtRest` → `ZoomingIn` | `ZoomingOut(progress)` → `AtRest`.
- **Transitions:** `set_center` starts one for a one-level move with motion on (`desktop/aizome/apps/kura/src/ui/mod.rs:365-379`); Frame re-arms or clears (`:695-702`); `Scanned` and `after_change` clear it (`:596`, `desktop/aizome/apps/kura/src/ui/suggest.rs:424-425`).
- **Hazards:** each `set_center` during a running transition starts another Frame chain, so several timers run until it ends; `outer_segs` must be cleared alongside `transition` by hand.
- **Reify:** Low — short-lived and local.

<a id="kko-34"></a>

### KKO-34 — Destructive actions: confirm, move or empty, rescan, graft
- **Where:** `desktop/aizome/apps/kura/src/ui/mod.rs:102` (`enum Pending`), `:248` (`Kura.pending`), `:256` (`Kura.busy`), `:215` (`.collector`), `:213` (`.selected`), `:197` (`.center`); logic `desktop/aizome/apps/kura/src/ui/suggest.rs:256-428`
- **Encoding:** partially explicit `Pending` for hint actions (the Collector's alert bypasses it); `busy: u32` counts moves, empties and rescans in flight; NodeIds in collector, selection and centre remapped by path after grafts; `generation` bumped per change.
- **States:** `Idle` → `Asking(Pending)` → `Moving` | `Emptying` (busy+1) → `Rescanning` (busy+1) → `Grafted` → re-analysis (KKO-31) → `Idle`; Collector `Asking(alert)` → `Moving`.
- **Transitions:** `ask_act`/`ask_group` set `pending` and raise the alert (`desktop/aizome/apps/kura/src/ui/suggest.rs:204`, `:219`, `:251`); Confirm/CancelPending (`desktop/aizome/apps/kura/src/ui/mod.rs:766-771`); AskTrash → `Msg::Trash` reads the collector at confirm time (`desktop/aizome/apps/kura/src/ui/mod.rs:711-736`); `carry_out` (`desktop/aizome/apps/kura/src/ui/suggest.rs:256-265`); `move_to_trash` busy+1 and spawn (`:269-280`); `moved` busy−1, `tree.remove`, rescan parents and the Trash (`:285-316`); `emptied` (`:319-330`); `rescan` busy+1 (`:334-364`); `rescanned` busy−1, graft, remap by path, re-index (`:368-406`); `after_change` drops gone NodeIds (`:410-428`).
- **Hazards:** action buttons don't check `busy`, so moves and rescans overlap; `Rescanned` has no generation and grafts by path into whatever tree is current, including a folder trashed after its rescan started (resurrected until its parent's rescan lands, KKO-28); a worker that panics leaves `busy` above 0 ("Scanning what changed…") forever; hints stay stale between a move and the next analysis.
- **Reify:** High — counter-based in-flight tracking across five messages and two files, with NodeId remapping.
- **Related:** the freedesktop Trash (KKO-27)

<a id="kko-35"></a>

### KKO-35 — Suggestions selection: focused hint, expanded, ticks and kept copies
- **Where:** `desktop/aizome/apps/kura/src/ui/mod.rs:241` (`focus_hint`), `:243` (`expanded`), `:245` (`choice`), `:247` (`keep`), `:250` (`highlight`), `:252` (`marks`), `:254` (`highlight_gen`)
- **Encoding:** hint identity by `hint_key` (id + title); per-path ticks by path string; kept copy as a positional index per duplicate hash; highlight NodeIds and per-segment marks recomputed from them.
- **States:** `NoFocus` | `Focused(key)` (rings centred on the common folder); per path `Default(by risk)` | `Chosen(bool)`; per set `Keep(i)`.
- **Transitions:** FocusHint (`desktop/aizome/apps/kura/src/ui/mod.rs:747`, `desktop/aizome/apps/kura/src/ui/suggest.rs:110-132`); Expand, Choose, Keep (`desktop/aizome/apps/kura/src/ui/mod.rs:748-763`); `refresh_highlight` on Indexed and after changes (`desktop/aizome/apps/kura/src/ui/mod.rs:742`, `desktop/aizome/apps/kura/src/ui/suggest.rs:135-157`, `:427`); `refresh_marks` on every row refresh (`desktop/aizome/apps/kura/src/ui/suggest.rs:161-180`, `desktop/aizome/apps/kura/src/ui/mod.rs:397`); `Scanned` clears focus and highlight but not `choice`, `keep` or `expanded` (`desktop/aizome/apps/kura/src/ui/mod.rs:599-600`).
- **Hazards:** `keep` indexes `group.paths`, so after a re-analysis reorders or shrinks a set the same index names another copy (the alert names the kept file, which mitigates it); `choice` and `keep` survive across scans and roots.
- **Reify:** Medium — several maps keyed differently; the positional `keep` is the risky part.

<a id="kko-36"></a>

### KKO-36 — Sunburst paint cache and invalidation counters
- **Where:** `desktop/aizome/apps/kura/src/ui/sunburst.rs:151` (`RingCache.drawn`, `.asked`), `:224` (`impl Hash for Burst`); `desktop/aizome/apps/kura/src/ui/mod.rs:196` (`generation`), `:211` (`rings_gen`), `:254` (`highlight_gen`)
- **Encoding:** LRU of three antialiased drawings by content hash; one outstanding request key; counters folded into the canvas hash to force repaints.
- **States:** per key `Missing` → `Asked` → `Drawn`; `settled` when nothing is asked.
- **Transitions:** paint asks for a missing key (`desktop/aizome/apps/kura/src/ui/sunburst.rs:451-467`); DrawRings spawns (`desktop/aizome/apps/kura/src/ui/mod.rs:687-690`); RingsDrawn → `arrived` clears `asked` if it matches and evicts the oldest (`desktop/aizome/apps/kura/src/ui/sunburst.rs:167-176`, `desktop/aizome/apps/kura/src/ui/mod.rs:691-694`); counters bumped in Scanned, `moved`, `rescanned` and `refresh_highlight` (`desktop/aizome/apps/kura/src/ui/mod.rs:594`, `desktop/aizome/apps/kura/src/ui/suggest.rs:304`, `:403`, `:155`).
- **Hazards:** if a requested drawing never arrives, `asked` stays on that key and it is never asked for again (drawn plainly until another key is asked); paint mutates the cache through a RefCell.
- **Reify:** Low — a cache with one in-flight slot.

<a id="kko-37"></a>

### KKO-37 — (test harness) Kura window settle loops
- **Where:** `desktop/aizome/apps/kura/tests/ui.rs:219` (`settle`), `:36` (`golden_settled`), `:1126` (`settled`)
- **Encoding:** bounded polling of program state (page, busy, IndexState, RingCache).
- **States:** waits for `Results && busy==0 && Written`, or `rings.settled()`.
- **Transitions:** advance + Poll each iteration (`desktop/aizome/apps/kura/tests/ui.rs:219-229`); ring wait (`:36-46`).
- **Hazards:** `settled()` can't tell a stale analysis from a current one (KKO-31).
- **Reify:** Low — test harness.

## Already explicit

- `desktop/aizome/apps/koe/src/app.rs:29` `enum Page` — the Koe window's page; changed only in `Msg::Page`/`Msg::Raise` (`desktop/aizome/apps/koe/src/app.rs:550-556`)
- `desktop/aizome/apps/koe/src/models.rs:84` `enum Installed` — what the models folder holds for a catalog model; computed in `fn installed` (`desktop/aizome/apps/koe/src/models.rs:91`), refreshed by `read_models` (`desktop/aizome/apps/koe/src/app.rs:303`)
- `desktop/aizome/apps/kura/src/ui/mod.rs:92` `enum Side` — Kura's side panel mode; changed only by `Msg::ShowSide` (`desktop/aizome/apps/kura/src/ui/mod.rs:746`)

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/apps/koe/data/aizome-koe.service` | 24 | machines | [KKO-01](#kko-01) | user unit gated by ExecCondition enabled |
| `desktop/aizome/apps/koe/src/app.rs` | 877 | machines | [KKO-02](#kko-02), [KKO-03](#kko-03), [KKO-04](#kko-04), [KKO-05](#kko-05) | switch, download, status follow, file stamps; Page explicit |
| `desktop/aizome/apps/koe/src/audio.rs` | 185 | machines | [KKO-06](#kko-06) | pw-record capture task; play is one-shot |
| `desktop/aizome/apps/koe/src/config.rs` | 262 | machines | [KKO-01](#kko-01) | enabled flag and tmp+rename save; rest validation |
| `desktop/aizome/apps/koe/src/daemon.rs` | 632 | machines | [KKO-07](#kko-07), [KKO-08](#kko-08), [KKO-09](#kko-09), [KKO-10](#kko-10) | dictation pipeline, teaching lease, reload, IPC |
| `desktop/aizome/apps/koe/src/desktop.rs` | 210 | machines | [KKO-11](#kko-11) | paste delivery guard |
| `desktop/aizome/apps/koe/src/dictation.rs` | 492 | none |  | view; renders KKO-02/KKO-03 state |
| `desktop/aizome/apps/koe/src/engine.rs` | 248 | machines | [KKO-12](#kko-12) | resident model and cancel flag |
| `desktop/aizome/apps/koe/src/instance.rs` | 68 | machines | [KKO-13](#kko-13) | single-instance socket |
| `desktop/aizome/apps/koe/src/ipc.rs` | 131 | machines | [KKO-14](#kko-14) | daemon lock and socket file; framing stateless |
| `desktop/aizome/apps/koe/src/lib.rs` | 19 | none |  | module list |
| `desktop/aizome/apps/koe/src/main.rs` | 806 | machines | [KKO-15](#kko-15), [KKO-01](#kko-01) | CLI edit retry; enable/disable/record start |
| `desktop/aizome/apps/koe/src/models.rs` | 279 | machines | [KKO-16](#kko-16), [KKO-03](#kko-03) | partial download files; Installed explicit |
| `desktop/aizome/apps/koe/src/protocol.rs` | 156 | machines | [KKO-07](#kko-07), [KKO-08](#kko-08) | Phase/Status/teaching wire types |
| `desktop/aizome/apps/koe/src/service.rs` | 306 | machines | [KKO-01](#kko-01) | Session start/stop polling; shortcut helpers pure |
| `desktop/aizome/apps/koe/src/text.rs` | 30 | none |  | pure text replacement |
| `desktop/aizome/apps/koe/src/tray.rs` | 311 | machines | [KKO-17](#kko-17), [KKO-01](#kko-01) | SNI mirror; Turn Off writes enabled |
| `desktop/aizome/apps/koe/src/vocab.rs` | 995 | machines | [KKO-18](#kko-18) | vocabulary page queue, reads, teaching, close |
| `desktop/aizome/apps/koe/src/vocabulary.rs` | 493 | machines | [KKO-19](#kko-19) | store revision, lock, staged saves |
| `desktop/aizome/apps/koe/tests/aizome.rs` | 266 | machines | [KKO-20](#kko-20) | test harness: real compositor driver |
| `desktop/aizome/apps/koe/tests/core.rs` | 193 | none |  | tests |
| `desktop/aizome/apps/koe/tests/daemon.rs` | 945 | machines | [KKO-21](#kko-21) | test harness: daemon and fakes |
| `desktop/aizome/apps/koe/tests/look.rs` | 130 | none |  | tests; golden images |
| `desktop/aizome/apps/koe/tests/setup.rs` | 405 | machines | [KKO-22](#kko-22) | test harness: gated service rig |
| `desktop/aizome/apps/koe/tests/tray.rs` | 208 | machines | [KKO-23](#kko-23) | test harness: private D-Bus |
| `desktop/aizome/apps/koe/tests/vocabulary.rs` | 246 | none |  | tests |
| `desktop/aizome/apps/kura/src/cli.rs` | 359 | machines | [KKO-24](#kko-24), [KKO-25](#kko-25) | progress ticker; reuses last index |
| `desktop/aizome/apps/kura/src/dupes.rs` | 218 | none |  | pure duplicate search with local budget |
| `desktop/aizome/apps/kura/src/format.rs` | 98 | none |  | pure formatting |
| `desktop/aizome/apps/kura/src/hints.rs` | 1219 | none |  | single-call detector; claimed bitmap local |
| `desktop/aizome/apps/kura/src/index.rs` | 441 | machines | [KKO-25](#kko-25) | index.json/index.md cache |
| `desktop/aizome/apps/kura/src/kinds.rs` | 150 | none |  | data tables |
| `desktop/aizome/apps/kura/src/lib.rs` | 111 | machines | [KKO-25](#kko-25) | write_atomic helper |
| `desktop/aizome/apps/kura/src/main.rs` | 23 | none |  | entry point dispatch |
| `desktop/aizome/apps/kura/src/mounts.rs` | 226 | none |  | pure mountinfo parsing |
| `desktop/aizome/apps/kura/src/nix.rs` | 224 | machines | [KKO-25](#kko-25) | nix-store.json cache keyed by db mtime |
| `desktop/aizome/apps/kura/src/report.rs` | 242 | none |  | pure markdown rendering |
| `desktop/aizome/apps/kura/src/scan.rs` | 522 | machines | [KKO-26](#kko-26) | worker queue busy/done/cancel |
| `desktop/aizome/apps/kura/src/time.rs` | 69 | none |  | pure time formatting |
| `desktop/aizome/apps/kura/src/trash.rs` | 328 | machines | [KKO-27](#kko-27) | freedesktop trash claim and rename |
| `desktop/aizome/apps/kura/src/tree.rs` | 496 | machines | [KKO-28](#kko-28) | arena tombstones and grafts |
| `desktop/aizome/apps/kura/src/ui/colors.rs` | 59 | none |  | pure colour math |
| `desktop/aizome/apps/kura/src/ui/drag.rs` | 206 | machines | [KKO-29](#kko-29) | drag gesture and drop zone |
| `desktop/aizome/apps/kura/src/ui/instance.rs` | 64 | machines | [KKO-13](#kko-13) | single-instance socket |
| `desktop/aizome/apps/kura/src/ui/mod.rs` | 868 | machines | [KKO-30](#kko-30), [KKO-31](#kko-31), [KKO-32](#kko-32), [KKO-33](#kko-33), [KKO-29](#kko-29), [KKO-34](#kko-34), [KKO-35](#kko-35), [KKO-36](#kko-36) | scan page, analysis, loads, animation; Side explicit |
| `desktop/aizome/apps/kura/src/ui/rings.rs` | 287 | none |  | pure ring geometry |
| `desktop/aizome/apps/kura/src/ui/suggest.rs` | 429 | machines | [KKO-34](#kko-34), [KKO-35](#kko-35), [KKO-31](#kko-31) | action pipeline and selection |
| `desktop/aizome/apps/kura/src/ui/suggest_view.rs` | 331 | none |  | view of KKO-34/KKO-35 state |
| `desktop/aizome/apps/kura/src/ui/sunburst.rs` | 701 | machines | [KKO-36](#kko-36), [KKO-29](#kko-29), [KKO-33](#kko-33) | ring cache; pointer events; Transition |
| `desktop/aizome/apps/kura/src/ui/view.rs` | 592 | none |  | view; renders page state |
| `desktop/aizome/apps/kura/tests/index.rs` | 173 | none |  | tests |
| `desktop/aizome/apps/kura/tests/ui.rs` | 1250 | machines | [KKO-37](#kko-37) | test harness: settle loops |
