# SYA: System clients: launch, audio, Bluetooth, disks, MPRIS

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/crates/aizome-sys`. Audited at `8e0acbe5`; line numbers refer to that commit.

20 files audited. 19 implicit machines: 5 High, 9 Medium, 5 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

<a id="sya-05"></a>

### SYA-05 — Optimistic audio levels awaiting server confirmation
- **Where:** `desktop/aizome/crates/aizome-sys/src/audio.rs:538` (`Expect.volumes`, `.muted`, `.since`); also `:625` (`Live.expects`), `:620` (`Live.sinks`… levels overwritten in place), `:618` (`Live.default_sink`, `.default_source`), `:1175` (`finish` once-guard)
- **Encoding:** per `(Kind, index)` an `Expect` with two `Option`s and an `Instant` (1 s window, `:544`); the stored level/mute and the defaults are written before the server replies; the reply only completes the `Done`
- **States:** per object `Confirmed` → `Expecting{volume?, mute?}` → `Confirmed` (matching report) | `Expired` (first report after 1 s) | `Removed`; on refusal the object stays at the unconfirmed level with its `Expect` live
- **Transitions:** volume set (`audio.rs:595-597`); mute set (`audio.rs:608-610`); balance set (`audio.rs:716-719`); report kept or cleared (`audio.rs:556-577`); object removed (`audio.rs:680`); default set optimistically (`audio.rs:1266-1270`); result (`audio.rs:1178-1181`); step + unmute (`audio.rs:1305-1315`); balance read-then-write (`audio.rs:1322-1345`)
- **Hazards:** a refused operation (`finish` with `Err`, `audio.rs:1180`) reverts neither the level/default nor the `Expect`, and PulseAudio sends no change event for a refused change, so the snapshot keeps a level the server never applied; expiry is lazy (checked only on the next report, `audio.rs:557`); `expect()` refreshes `since` for both fields when either is set (`audio.rs:582`), so a mute press extends a pending volume expectation; within the window a genuine change by another client is masked; defaults have no `Expect`, so a server-info reply requested earlier can undo an optimistic default (`audio.rs:1117-1121`); Balance drops its `done` (task `Cancelled`, not `NotFound`) when the device lookup returns no item or the context is gone (`audio.rs:1323-1345`)
- **Reify:** High — state spread over `Expect`, the entry maps, the defaults and per-operation callbacks, with a user-visible mismatch
- **Suspected bug:** Settings changes the volume of a stream the server refuses to change (for example a sink input with `volume_writable` false): `set_volume_in` has already stored the new level and marked the model dirty, the callback reports `Err`, no event follows, so the snapshot and slider show the unapplied level until another event for that stream (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#sya-05)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#sya-06)

<a id="sya-13"></a>

### SYA-13 — Destructive-operation confirmation token
- **Where:** `desktop/aizome/crates/aizome-sys/src/disks/confirm.rs:80` (`Confirm.fingerprint`, `.at`); also `:56` (`Fingerprint`), `:17` (`CONFIRMATION_LIFETIME`), `desktop/aizome/crates/aizome-sys/src/disks/mod.rs:595` (`destructive`)
- **Encoding:** partially explicit: typestate `Confirm<Op>` moved into the call (single use), with a fingerprint and an `Instant`; validity is decided at run time against a fresh `GetManagedObjects`
- **States:** `Fresh` → `Consumed` (checked and run) | `Expired` (> 10 min) | `Gone` | `Changed` | `InUse`
- **Transitions:** made (`disks/confirm.rs:94-96`); expiry (`disks/confirm.rs:105-111`); gone (`disks/confirm.rs:112-114`); changed (`disks/confirm.rs:115-121`); re-read state (`disks/mod.rs:602`); check (`disks/mod.rs:603`); in-use refusal (`disks/mod.rs:604-606`); run (`disks/mod.rs:607`)
- **Hazards:** check-then-act: the device can be mounted or changed between `disks/mod.rs:603-604` and the UDisks call; the fingerprint has no drive identity (serial/WWN) and no partition-table identity, and whole partitioned disks have empty `IdType`/`IdUUID`, so a different disk of the same size at the same kernel name matches; `Instant` is monotonic, so time asleep in suspend doesn't count towards the 10 minutes
- **Reify:** High — a data-loss guard; the gaps are in what the token records
- **Suspected bug:** a UI that holds a `Confirm<NewPartitionTable>` made for USB stick A (32 GB, GPT) while A is swapped for an identical-size partitioned stick B that comes up as `sdb` again: path, device, size and empty id fields all match, so B is wiped (confidence: low; no app makes these tokens yet)
- **Reproduction:** Confirmed (unit): [summary](reproductions.md#sya-13)

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
- **Reproduction:** Partly confirmed (integration): [summary](reproductions.md#sya-17)

<a id="sya-19"></a>

### SYA-19 — MPRIS position estimate and active-player counters
- **Where:** `desktop/aizome/crates/aizome-sys/src/mpris/mod.rs:99` (`Tracked.position_us`, `.position_at`, `.played`, `.changed`, `.stale_position`); also `:169` (`Players.seq`), `desktop/aizome/crates/aizome-sys/src/mpris/player.rs:297` (`estimate_us`), `:329` (`choose_active`)
- **Encoding:** a base position plus the `Instant` it was read, extrapolated while `PlaybackStatus` is Playing; a `stale_position` flag asking for a re-read at publish; a global `seq` stamped into `played` (status to or from Playing) and `changed` (any change) to pick the active player
- **States:** position `Fresh` → `Estimating` (Playing) → `Stale` (status, track or rate touched) → `Fresh` (read, announced, Seeked, refreshed); activity order by `(playing, played, changed)`
- **Transitions:** rebase before a change (`mpris/mod.rs:119-125`); announced Position (`mpris/mod.rs:134-139`); mark stale (`mpris/mod.rs:140-142`); played/changed (`mpris/mod.rs:144-147`); Seeked (`mpris/mod.rs:604-612`); refetch after invalidation (`mpris/mod.rs:584-596`); stale read at publish (`mpris/mod.rs:634-641`); `refresh_position` (`mpris/mod.rs:340-345`); counters reset when a name re-appears (`mpris/mod.rs:518-530`)
- **Hazards:** `publish` clears `stale_position` with `mem::take` before reading (`mpris/mod.rs:634`) and skips on error (`mpris/mod.rs:637`), so after a failed read (for example on a track change) the old track's extrapolated position stands until the next status, track or rate change; positions are written from two threads (the follower and `refresh_position` tasks), safe only because both write and build under the lock with an owner check; re-appearance resets `played`, demoting a player that was just active
- **Reify:** High — five fields mutated at five sites across two threads
