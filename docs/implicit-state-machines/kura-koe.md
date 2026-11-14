# KKO: Kura and Koe

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/apps/{kura,koe}`. Audited at `8e0acbe5`; line numbers refer to that commit.

52 files audited. 37 implicit machines: 9 High, 8 Medium, 20 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

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
- **Reproduction:** Partly confirmed (integration): [summary](reproductions.md#kko-01)

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

<a id="kko-18"></a>

### KKO-18 — Vocabulary page: request queue, stale-read guard, teaching client, closing
- **Where:** `desktop/aizome/apps/koe/src/vocab.rs:75` (`Vocabulary`), fields `.status` (`:88`), `.token` (`:89`), `.preview` (`:90`), `.examples_for` (`:94`), `.loaded` (`:96`), `.busy` (`:97`), `.queued` (`:98`), `.sent` (`:101`), `.reading` (`:102`), `.ticks` (`:103`), `.closing` (`:105`)
- **Encoding:** busy flag + FIFO of mutating requests; `sent` counter as an epoch for reads; `reading` count; teaching token and preview mirroring KKO-08; closing flag.
- **States:** requests `Idle` → `Busy(+queued)` → `Idle`; reads `InFlight(sent)` → `Applied` | `Dropped`; teaching `NoToken` → `Starting` → `Recording` → `Recognising` → `Review(preview)` → `Saved` | `Discarded` | `Lost`; window `Open` → `Closing` (wait for busy, then TeachCancel) → `Closed`. Representable: `preview.is_some() && token.is_none()` (cleared together by hand at four sites).
- **Transitions:** `request` (`desktop/aizome/apps/koe/src/vocab.rs:319-331`); `read` (`:333-341`); Tick chooses TeachPoll, Library or Examples (`:394-405`); `Read` dropped if `sent` moved or closing, TeachPoll errors clear the token (`:406-426`); `Reply` clears busy, clears the token on teaching errors or after Cancel/Save, then pops the queue or continues closing (`:427-475`); Record sets the token and sends TeachStart (`:561-574`); `set_status(None)` discards the example (`:151-163`); `close`/`finish_close` (`:351-370`).
- **Hazards:** queued `SaveLibrary` commands carry library snapshots whose revision is stale once the first save lands; the token's lease depends on a poll that pauses while busy; a read spawned inside a `Reply` that then pops a queued request is dropped.
- **Reify:** High — twelve fields, four interleaved machines, many handlers.
- **Related:** daemon lease `desktop/aizome/apps/koe/src/daemon.rs:31` (KKO-08); store `desktop/aizome/apps/koe/src/vocabulary.rs:289` (KKO-19)

<a id="kko-30"></a>

### KKO-30 — Kura scan lifecycle and page
- **Where:** `desktop/aizome/apps/kura/src/ui/mod.rs:52` (`enum Page`), `:59` (`Scanning.progress`, `.timer`), `:189` (`Kura.page`), `:193` (`.scanning`), `:194` (`.scan`), `:229` (`.last_root`)
- **Encoding:** partially explicit — `Page` enum, but the running scan is `scanning: Option<Scanning>` (progress, poll timer) and the result `scan`; the worker's `Msg::Scanned` carries no identity.
- **States:** `Disks` → `Scanning(root)` → `Results`, or back to `Disks`/`Results` with a notice on error or stop; ShowDisks during a scan gives `page==Disks && scanning.is_some()`. Representable invalid: a scan worker running while `scanning == None` (orphaned).
- **Transitions:** `start_scan` cancels the previous progress, drops its timer, spawns, sets the page (`desktop/aizome/apps/kura/src/ui/mod.rs:420-441`); Poll (`:578-582`); Stop (`:583-587`); `Scanned` → `finish_scan`, then Results or the error page (`:588-613`); Rescan (`:615-620`); ShowDisks (`:553-556`); Launched and `open` (`:802-807`, `:540-544`).
- **Hazards:** `Scanned` tears down whichever scan is current (`:589`) and applies its result without checking that it belongs to it; Stop and Rescan enablement read `scanning` (`:832-834`).
- **Reify:** High — scan identity is implicit and results are applied unconditionally.
- **Suspected bug:** while a scan runs, press ⌘D and Scan another disk (or ⌘N/⌘O, or launch `aizome-kura PATH` again): `start_scan` cancels the first scan, whose `Scanned(Err(Cancelled))` then arrives and `finish_scan` cancels the second scan's poll timer and clears `scanning` (`desktop/aizome/apps/kura/src/ui/mod.rs:589`), so the window shows "The scan didn't finish: the scan was stopped" while the second scan runs unseen and can no longer be stopped; its result later replaces the page unannounced, and a Rescan meanwhile starts a third scan without cancelling it (confidence: high)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#kko-30)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#kko-31)

<a id="kko-34"></a>

### KKO-34 — Destructive actions: confirm, move or empty, rescan, graft
- **Where:** `desktop/aizome/apps/kura/src/ui/mod.rs:102` (`enum Pending`), `:248` (`Kura.pending`), `:256` (`Kura.busy`), `:215` (`.collector`), `:213` (`.selected`), `:197` (`.center`); logic `desktop/aizome/apps/kura/src/ui/suggest.rs:256-428`
- **Encoding:** partially explicit `Pending` for hint actions (the Collector's alert bypasses it); `busy: u32` counts moves, empties and rescans in flight; NodeIds in collector, selection and centre remapped by path after grafts; `generation` bumped per change.
- **States:** `Idle` → `Asking(Pending)` → `Moving` | `Emptying` (busy+1) → `Rescanning` (busy+1) → `Grafted` → re-analysis (KKO-31) → `Idle`; Collector `Asking(alert)` → `Moving`.
- **Transitions:** `ask_act`/`ask_group` set `pending` and raise the alert (`desktop/aizome/apps/kura/src/ui/suggest.rs:204`, `:219`, `:251`); Confirm/CancelPending (`desktop/aizome/apps/kura/src/ui/mod.rs:766-771`); AskTrash → `Msg::Trash` reads the collector at confirm time (`desktop/aizome/apps/kura/src/ui/mod.rs:711-736`); `carry_out` (`desktop/aizome/apps/kura/src/ui/suggest.rs:256-265`); `move_to_trash` busy+1 and spawn (`:269-280`); `moved` busy−1, `tree.remove`, rescan parents and the Trash (`:285-316`); `emptied` (`:319-330`); `rescan` busy+1 (`:334-364`); `rescanned` busy−1, graft, remap by path, re-index (`:368-406`); `after_change` drops gone NodeIds (`:410-428`).
- **Hazards:** action buttons don't check `busy`, so moves and rescans overlap; `Rescanned` has no generation and grafts by path into whatever tree is current, including a folder trashed after its rescan started (resurrected until its parent's rescan lands, KKO-28); a worker that panics leaves `busy` above 0 ("Scanning what changed…") forever; hints stay stale between a move and the next analysis.
- **Reify:** High — counter-based in-flight tracking across five messages and two files, with NodeId remapping.
- **Related:** the freedesktop Trash (KKO-27)
