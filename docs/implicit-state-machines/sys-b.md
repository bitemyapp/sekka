# SYB: System clients: network, notifications, polkit, power, tray, users

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/crates/aizome-sys`. Audited at `8e0acbe5`; line numbers refer to that commit.

20 files, 0 pending. 16 implicit machines: 5 High, 8 Medium, 3 Low.

## Machines

<a id="syb-01"></a>

### SYB-01 — Airplane mode across rfkill, NetworkManager, BlueZ and a marker file
- **Where:** `desktop/aizome/crates/aizome-sys/src/network/mod.rs:72` (`RadioPaths.fallback`, the marker `$XDG_RUNTIME_DIR/aizome-airplane-mode`, `mod.rs:81`); also `desktop/aizome/crates/aizome-sys/src/network/model.rs:881`, `desktop/aizome/crates/aizome-sys/src/network/rfkill.rs:79`
- **Encoding:** kernel rfkill soft blocks (every radio), NM `WirelessEnabled`/`WwanEnabled`, BlueZ `Adapter1.Powered`, the marker file listing the adapters the fallback switched off; derived `NetworkState.airplane_mode`, computed by four different rules: `model.rs:881-885` (`build`), `mod.rs:131-134` (`airplane()`, rfkill watcher at `mod.rs:179`), `mod.rs:578` (Follower `|=` marker), `mod.rs:251` (set right away after the fallback)
- **States:** `Off` → `OnViaRfkill` (all radios blocked) | `OnViaFallback` (marker present, NM Wi-Fi/WWAN off, BlueZ adapters off) → `Off`. Invalid combinations that can be represented: the marker present while Wi-Fi is on (a stale marker); the marker present while rfkill blocks everything; no rfkill radios with Wi-Fi and WWAN off counted as airplane by `build` without the marker
- **Transitions:** `set_airplane_mode(on)`: rfkill write succeeds → OnViaRfkill (`mod.rs:234`); permission/NotFound error (`rfkill.rs:180-182`) → `airplane_without_rfkill(on)` → OnViaFallback (`mod.rs:246-252`; marker written `mod.rs:116`); `set_airplane_mode(false)`: rfkill unblock plus, if the marker exists, adapters restored and the marker removed (`mod.rs:238-243`, `mod.rs:118-124`); fallback off (`mod.rs:248`); external: the Wi-Fi switch (`mod.rs:211-213`) clears the derived state but leaves the marker; rfkill events (`mod.rs:175-183`); powering Bluetooth on unblocks it (`desktop/aizome/crates/aizome-sys/src/bluetooth/mod.rs:430`)
- **Hazards:** only `set_airplane_mode(false)` clears the marker. The fallback remembers BlueZ adapters but not the earlier `WirelessEnabled`/`WwanEnabled`, so turning airplane mode off always switches Wi-Fi and WWAN back on (`mod.rs:104-106`). On the rfkill path, off unblocks every radio and forgets radios that were blocked before airplane mode. The derivation rules disagree. On and off are separate `Task::spawn` threads (`desktop/aizome/crates/aizome-sys/src/network/mod.rs:234`), so a quick toggle can interleave the marker's write and read and leave the marker behind. The marker lives in the runtime dir, so a reboot forgets which adapters to restore, while NM and BlueZ keep them off
- **Reify:** High — five external switches plus a persisted marker, four derivation sites, a user-visible bug
- **Suspected bug:** Airplane mode turns on through the fallback (the marker lists hci0). The person then turns Wi-Fi back on with the Wi-Fi switch: `airplane_mode` reads false, but the marker stays. When they turn airplane mode on again, `bluetooth_on` returns `[]` because hci0 is still off, and `std::fs::write` (`desktop/aizome/crates/aizome-sys/src/network/mod.rs:116`) replaces the list with "". Turning airplane mode off then never powers Bluetooth back on. Separately, the stale marker makes a later plain Wi-Fi-off read as airplane on (`mod.rs:578`) (confidence: medium)
- **Related:** `desktop/aizome/crates/aizome-shell/src/systray/glue.rs:267-271`, `desktop/aizome/apps/settei/src/panes/network/mod.rs:75` (switches); BlueZ via `desktop/aizome/crates/aizome-sys/src/bluetooth/mod.rs:430`
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#syb-01)

<a id="syb-02"></a>

### SYB-02 — Wi-Fi join / activation transaction with in-memory edit and rollback
- **Where:** `desktop/aizome/crates/aizome-sys/src/network/mod.rs:694` (`rejoin_with`), `mod.rs:744` (`add_and_wait`), `mod.rs:774` (`wait_active`), `mod.rs:60` (`JOIN_TIMEOUT`)
- **Encoding:** one blocking Task thread per join. On the NM side the profile is either edited in memory (`Update2` `UPDATE_IN_MEMORY`, `mod.rs:705-712`), saved (`Save`, `mod.rs:714`) or reloaded (`LoadConnections`, `mod.rs:716-725`); a new profile is either added (`AddAndActivateConnection2`, `mod.rs:746-753`) or deleted on failure (`mod.rs:767-769`). The poll loop uses a local `Instant` start against the 45 s timeout (`mod.rs:782-806`)
- **States:** `Requested` → `EditedInMemory` | `Added` → `Activating` (polled every 150 ms) → `Activated` → `Saved`; or `Failed(reason)` / `Rejected` / `Timeout` → `RolledBack` (reloaded from disk, or deleted)
- **Transitions:** dispatch in `connect` (`mod.rs:287-310`), `connect_enterprise` (`mod.rs:316-332`), hidden networks (`mod.rs:335-361`), `activate` (`mod.rs:365-371`); `Update2` in memory (`mod.rs:705`); activate (`mod.rs:732-740`); poll: Activated (`mod.rs:797`), Deactivated/NotFound → failed with the device's reason (`mod.rs:798`, `mod.rs:783-794`), timeout (`mod.rs:802-804`); Save (`mod.rs:714`); rollback (`mod.rs:717-724`); pre-1.16 fallback API (`mod.rs:757-764`); forget on failure (`mod.rs:767-769`); hotspot forgets old AP profiles, then adds (`mod.rs:468-469`)
- **Hazards:** no cancellation: `Stop` isn't checked, so a dropped `Network` leaves the thread polling for up to 45 s. On Timeout, `activate_and_wait`/`rejoin_with` leave NM's activation running (no DeactivateConnection), while `add_and_wait` deletes a profile whose activation may only be slow (NM's own DHCP timeout is of the same order). An in-memory-only profile (empty `filename`, `mod.rs:716`) keeps the untested edit on failure. `update_settings` (`mod.rs:634-649`) is a read-modify-write saved with `UPDATE_TO_DISK`, so another task editing the same profile during a rejoin's in-memory window can save the untested change to disk. When two joins run at once, the one that was replaced reports Failed
- **Reify:** High — the phases are spread across NM calls, timeout and rollback, with user-visible wrong outcomes
- **Suspected bug:** Joining a saved network with a new password that takes longer than 45 s to come up: `wait_active` returns Timeout and `rejoin_with` reloads the old profile from disk (`mod.rs:717-724`). NM keeps activating with the in-memory settings it already applied and may connect. The UI reported failure and the working password was never saved, so the next reconnect fails (confidence: low)
- **Related:** NetworkManager (Settings.Connection, Connection.Active); `desktop/aizome/apps/settei/src/panes/network/mod.rs` (callers); the `aizome-network-agent` secret agent
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#syb-02)

<a id="syb-03"></a>

### SYB-03 — Daemon follower present/dirty coalescing (NetworkManager, logind, UPower, power profiles, AccountsService)
- **Where:** `desktop/aizome/crates/aizome-sys/src/network/mod.rs:501` (`Follower.present`, `.dirty`, `.objects`, `.settings`); also `desktop/aizome/crates/aizome-sys/src/power/logind.rs:187-191` (closure locals `manager`, `session_props`, `state`, `present`, `dirty`), `desktop/aizome/crates/aizome-sys/src/power/upower.rs:314-317` (`objects`, `display`, `present`, `dirty`), `desktop/aizome/crates/aizome-sys/src/power/profiles.rs:123-124` (`props`, `present`; no dirty flag), `desktop/aizome/crates/aizome-sys/src/users.rs:276-278` (`objects`, `present`, `dirty`)
- **Encoding:** a `present` bool, a `dirty` bool and cached property maps, kept in each service's `bus::follow` handler and driven by `Event::{Appeared, Vanished, Signal, Idle}`
- **States:** `Absent` → `Present(dirty)` → `Present(clean)` ⇄ `Present(dirty)` → `Absent`; the final Vanished+Idle arrives when the follow thread ends
- **Transitions:** Appeared (load everything, dirty): `network/mod.rs:515-525`, `power/logind.rs:194-228`, `power/upower.rs:326-330`, `power/profiles.rs:126-129`, `users.rs:286-295`; Signal → dirty: `network/mod.rs:540,564,569`, `power/logind.rs:249,252,280,288`, `power/upower.rs:346,352,363,371`, `users.rs:306,312,317,321`; Idle publishes and clears: `network/mod.rs:574-581`, `power/logind.rs:293-298`, `power/upower.rs:375-379`, `power/profiles.rs:146-150` (every Idle), `users.rs:325-329`; Vanished publishes the default immediately: `network/mod.rs:526-532`, `power/logind.rs:229-233`, `power/upower.rs:331-335`, `power/profiles.rs:130-133`, `users.rs:296-299` (all paths under `desktop/aizome/crates/aizome-sys/src/`)
- **Hazards:** Vanished bypasses Idle and leaves `dirty` as it was. The Idle `set_if_changed` races other writers of the same model (SYB-14). Appeared makes serial blocking calls (NM GetSettings per profile `network/mod.rs:521-523`; six logind `Can…` calls `power/logind.rs:204-219`; UPower `load`), so a slow daemon holds everything up. The power-profiles daemon name is chosen once (`power/profiles.rs:116-120`): when neither daemon was owned or activatable at start, the legacy name is never followed. Keyboard backlights are enumerated only at Appeared (`power/upower.rs:472-490`). NM's Vanished keeps the radios but resets `airplane_mode` to false. logind's Lock/Unlock are filtered by the session path captured at Appeared (`power/logind.rs:256,260`), so an empty path silently drops lock requests
- **Reify:** Medium — the same 4-state shape is hand-copied in five handlers; a shared helper (as `bus::follow_props` does) could own present/dirty
- **Related:** `desktop/aizome/crates/aizome-sys/src/bus.rs:573-708` (part A: emits Appeared/Vanished/Idle)

<a id="syb-04"></a>

### SYB-04 — rfkill watcher thread and radio table
- **Where:** `desktop/aizome/crates/aizome-sys/src/network/rfkill.rs:132` (`watch`: local `radios: BTreeMap`, `stop`); also `desktop/aizome/crates/aizome-sys/src/network/mod.rs:167` (shared radios seeded from sysfs)
- **Encoding:** a thread's lifetime plus the boolean `watch` returns; radios rebuilt from ADD/CHANGE/DEL events (`rfkill.rs:58-76`)
- **States:** `NotStarted` (open failed → false) → `Running` (poll 500 ms) → `Exited` (stop, EOF, poll or read error)
- **Transitions:** open (`rfkill.rs:134-136`); spawn (`rfkill.rs:137`); poll (`rfkill.rs:143`); exits (`rfkill.rs:148`, `rfkill.rs:164`, `rfkill.rs:168`); stop checked (`rfkill.rs:140`); burst published (`rfkill.rs:171-173`)
- **Hazards:** the return value is ignored (`network/mod.rs:175`). Without a readable `/dev/rfkill`, the radios stay at the sysfs snapshot read once (`mod.rs:167`). The thread exits silently on errors, without logging or restarting, which freezes the radios and airplane-mode derivation (SYB-01)
- **Reify:** Low — a small, local thread lifecycle

<a id="syb-05"></a>

### SYB-05 — Notification client: id matching, early-signal buffer, server loss
- **Where:** `desktop/aizome/crates/aizome-sys/src/notifications/client.rs:215` (`Dispatcher.sent`, `.tokens`, `.early`); also `client.rs:159-180` (subscription handshake)
- **Encoding:** `sent: HashSet<u32>`, `tokens: HashMap<u32, String>`, `early: VecDeque<(Instant, Signal)>` bounded by `EARLY_FOR` 10 s / `EARLY_MAX` 256 (`client.rs:24-25`); `ready_tx: Option<Sender>` one-shot; a `Msg` channel with `Stop`
- **States:** per id: `Unknown/Buffered` → `Sent` → `TokenPending` → `ActionInvoked` → `Closed`; client: `Subscribing` → `Ready` (or a 5 s timeout) → `Running` → `Stopped`; `ServerGone` closes every id
- **Transitions:** ready at the first Idle (`client.rs:172-176`), awaited (`client.rs:180`); `notify` → `Sent` (`client.rs:187`) → replay of early signals (`client.rs:230-239`); a known id's signal delivered (`client.rs:240`, `client.rs:262-277`: Token `client.rs:264-266`, Action takes the token `client.rs:267-270`, Closed `client.rs:271-275`); an unknown id buffered (`client.rs:241-248`); ServerGone (`client.rs:249-257`); Stop (`client.rs:229`, from Drop `client.rs:207-212`)
- **Hazards:** `Sent(id)` can race `ServerGone`: if the server leaves between Notify's reply and the `Sent` message, the id stays in `sent` forever, is never reported closed, and once a new server reuses the id, another app's ActionInvoked/Closed reaches this callback. `early` is pruned only when another unknown signal arrives. After the 5 s handshake timeout the client proceeds unsubscribed. Tokens are dropped only on Closed
- **Reify:** Medium — three collections and a buffer with expiry encode each id's phase
- **Related:** `desktop/aizome/crates/aizome-sys/src/notifications/server.rs` (this slice) or any other `org.freedesktop.Notifications` server

<a id="syb-06"></a>

### SYB-06 — Open notification and its pausable expiry timer
- **Where:** `desktop/aizome/crates/aizome-sys/src/notifications/server.rs:120` (`Entry.lifetime`, `.since`, `.shown`, `.paused`); `server.rs:148` (`State.entries`, `.last_id`, `.jobs`)
- **Encoding:** per id, an `Entry` in a `BTreeMap`; `deadline()` derived from the fields (`server.rs:131-137`); the worker waits on a `Condvar` until the earliest deadline; jobs queued under the lock
- **States:** `Open(Running)` ⇄ `Open(Paused)` → `Closed(Expired | Dismissed | Closed | Undefined)`; `lifetime: None` never expires (critical/resident by policy, `server.rs:177-190`); a replacement in place restarts the timer and keeps a pause; a resident notification stays open after an action
- **Transitions:** new/replace (`server.rs:205-231`, pause carried over `server.rs:217`); pause (`server.rs:266-267`), resume (`server.rs:268-269`), worker woken (`server.rs:272`); expiry (`server.rs:283-291`); close (`server.rs:233-241`, via CloseNotification `server.rs:381-387` and the shell's `dismiss` `server.rs:480-482`); action (`server.rs:243-260`: a non-resident one closes as Dismissed `server.rs:255-257`); Drop closes all as Undefined (`server.rs:529-537`)
- **Hazards:** only `set_paused` keeps the rule that a paused entry's `since` is ignored and `shown` frozen. Replacement resets `shown`, so a notification updated more often than its lifetime (progress) never expires. Ids wrap past `u32::MAX` (`server.rs:158-166`)
- **Reify:** Medium — four fields encode running/paused/remaining, with five mutation sites
- **Related:** `desktop/aizome/crates/aizome-shell/src/notifications/glue.rs` (banners; pause/resume on hover and do-not-disturb)

<a id="syb-07"></a>

### SYB-07 — Notification server lifecycle and bus-name ownership
- **Where:** `desktop/aizome/crates/aizome-sys/src/notifications/server.rs:406` (`NotificationServer.worker`, `.serving`, `.named`); `server.rs:152` (`State.stopped`)
- **Encoding:** `worker: Option<JoinHandle>`, the `serving`/`named` bools, `stopped` in the shared state, and the bus name's actual owner (queried only by `owns_name`)
- **States:** `Starting` (worker spawned) → `Serving` (interface at PATH) → `Named` → `Running` → [`Replaced`: name lost, still serving] → `Stopping` (`stopped`, draining jobs) → `Stopped` (joined, name released, interface removed)
- **Transitions:** worker spawned (`server.rs:435-442`); served (`server.rs:449-452`); ping until it answers (`server.rs:453`); name requested (`server.rs:454-470`, `named` set `server.rs:461`, `server.rs:465`); Drop: entries cleared and stopped (`server.rs:529-537`), join (`server.rs:539-543`), release (`server.rs:545-547`), remove (`server.rs:548-550`); the worker returns once stopped and drained (`server.rs:301-303`)
- **Hazards:** losing the name to a ReplaceExisting server (allowed when `config.replace`, `server.rs:454-456`) goes unseen unless someone polls `owns_name()` (`server.rs:518-521`); the old server keeps its timers and the shell keeps showing notifications whose signals clients no longer hear. Drop joins a worker that runs the shell's callback, so a callback that blocks on the dropping thread deadlocks (skipped only when dropped on the worker itself, `server.rs:540`)
- **Reify:** Medium — three flags plus an unobserved external owner
- **Related:** `desktop/aizome/crates/aizome-shell/src/notifications/glue.rs`; apps through `notifications/client.rs`

<a id="syb-08"></a>

### SYB-08 — One-value async channel
- **Where:** `desktop/aizome/crates/aizome-sys/src/oneshot.rs:11` (`State.value`, `.waker`, `.closed`)
- **Encoding:** `Option<T>` value, `Option<Waker>`, `closed` bool; `Arc` strong count for `is_closed`
- **States:** `Empty` → `Waiting(waker)` → `Sent(value)` | `Closed`; a sent value is followed by `closed` once the Sender drops
- **Transitions:** `send` (`oneshot.rs:33-40`); Sender Drop → closed (`oneshot.rs:48-56`); `poll` takes the value / reports closed / stores the waker (`oneshot.rs:61-71`); `is_closed` (`oneshot.rs:43-45`)
- **Hazards:** none observed
- **Reify:** Low — small and self-contained
- **Related:** `desktop/aizome/crates/aizome-sys/src/polkit.rs:588` (BeginAuthentication reply), `polkit.rs:728` (agent stop signal)

<a id="syb-09"></a>

### SYB-09 — polkit authentication session and helper conversation
- **Where:** `desktop/aizome/crates/aizome-sys/src/polkit.rs:195` (`SessionInner.reply`, `.conversation`, `.generation`); `polkit.rs:537` (`AgentState.sessions`)
- **Encoding:** `reply: Option<oneshot::Sender<bool>>` (Some = still open); `conversation: Option<Conversation>` (the helper's `responses` channel and `Ender`, `polkit.rs:204-230`); a `generation` counter that discards earlier conversations; the converse thread's local `outcome`; the BeginAuthentication future awaiting the oneshot (`polkit.rs:612`)
- **States:** `Pending` (no conversation) → `Conversing(gen n)` (`Prompt` → `AwaitingResponse` → …) → `Failed` (reply open, no conversation) → `Conversing(gen n+1)` … → `Done(authorized | cancelled)` (reply None). Representable: a stale converse thread for gen n−1 still running beside gen n
- **Transitions:** BeginAuthentication creates, inserts, emits Begin (`polkit.rs:589-610`; duplicate cookie refused `polkit.rs:598-600`); `authenticate` → `Session::start` (`polkit.rs:169-171`, `polkit.rs:233-264`: ends the old conversation `polkit.rs:239-241`, generation++ `polkit.rs:242`, connect `polkit.rs:244`, connect error → Error+Failed `polkit.rs:247-252`, conversation set `polkit.rs:255`, thread `polkit.rs:258-263`); prompt and response (`polkit.rs:289-298`, `polkit.rs:328-333`); SUCCESS → finish(true) (`polkit.rs:301-303`, `polkit.rs:323`); FAILURE, unknown line or EOF → conversation None + Failed (`polkit.rs:305-314`, `polkit.rs:321`, `polkit.rs:324`); `cancel` (`polkit.rs:181-182`), CancelAuthentication (`polkit.rs:623-640`), authority change (`polkit.rs:551-561`) → `finish(false)` (`polkit.rs:336-345`); the future resolves and removes the session (`polkit.rs:612-616`)
- **Hazards:** `start` holds `inner` across `connect()` (spawn or socket connect plus write), which blocks `respond`/`finish`/`current`. Between converse's `current()` check (`polkit.rs:318`) and `conversation = None` (`polkit.rs:321`), a new `start()` can install a newer conversation, which is then wiped (its responses channel drops and its first prompt fails). Converse blocks on `responses.recv()` (`polkit.rs:291`) with no timeout. Begin is emitted while the agent's state lock is held (`polkit.rs:595-611`)
- **Reify:** High — the privilege boundary, three fields plus a generation counter, and a suspected bug
- **Suspected bug:** Retry loop without backoff. When the helper can't be reached (connect error, `polkit.rs:245-252`) or exits without printing SUCCESS/FAILURE (EOF → Failed, `polkit.rs:324`), for example a broken setuid wrapper, a socket unit refusing, or PAM refusing before any prompt, Failed is emitted at once. The shell's Failed handler immediately calls `authenticate` again (`desktop/aizome/crates/aizome-shell/src/auth.rs:289-296`), which fails again, so the dialog spins and spawns helpers or logs warnings as fast as the UI loop runs (confidence: medium)
- **Related:** `desktop/aizome/crates/aizome-shell/src/auth.rs:237-328` (dialog); polkitd and `polkit-agent-helper-1` (`HELPER_SOCKET`, `HELPER_PATHS`, `polkit.rs:66-74`)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#syb-09)
- **Follow-up:** reified as typestate in [#298](https://github.com/bitemyapp/sekka/pull/298). Its suspected bug is fixed, and the mistake no longer compiles.

<a id="syb-10"></a>

### SYB-10 — polkit agent registration and reconnect loop
- **Where:** `desktop/aizome/crates/aizome-sys/src/polkit.rs:781-785` (`maintain` locals `owner`, `registered`, `cleanup_pending`, `warned`); also `polkit.rs:645` (`PolkitAgent.stop`, `.worker`), `polkit.rs:536` (`AgentState.owner`)
- **Encoding:** an outer reconnect loop (`polkit.rs:684-701`) around the `maintain` loop's locals; `AgentState.owner` gates which caller may start authentications (`polkit.rs:541-547`); a oneshot stop plus a `JoinHandle`
- **States:** `SetupDeferred` (retry every 2 s) → `Serving(no owner)` → `Registering` [→ `CleanupPending`] → `Registered` → (owner change → `Registering`) | (bus lost → `SetupDeferred`) | `Stopping` → `Unregistered`
- **Transitions:** connect+serve (`polkit.rs:685-687`); setup error → wait (`polkit.rs:694`, `polkit.rs:698`); first owner (`polkit.rs:781-782`); activation (`polkit.rs:787-791`); cleanup (`polkit.rs:793-795`); registered (`polkit.rs:806-810`); error, with cleanup after Timeout (`polkit.rs:811-822`); bounded wait while unregistered (`polkit.rs:843`); Stop (`polkit.rs:847`), Disconnected (`polkit.rs:848`), owner change resets (`polkit.rs:852-859`); exit purges sessions, unregisters, removes the object (`polkit.rs:863-869`); Drop (`polkit.rs:888-897`); `register_with` is single-shot (`polkit.rs:708-725`)
- **Hazards:** Drop joins the worker on the caller's thread, while the worker may be in a blocking bus call (`polkit.rs:668`, `polkit.rs:788`, `polkit.rs:798-805`, `polkit.rs:794`, `polkit.rs:816`, `polkit.rs:867`). The stop oneshot is only awaited between calls, so dropping can block for several bus timeouts. Once registered, nothing re-checks the registration unless the owner changes. `change_owner` briefly leaves the owner None (`polkit.rs:554-560`). `warned` silences a registration that stays refused (another agent owns the session) after the first log
- **Reify:** High — four loop locals, an outer loop and the owner gate guard the privilege boundary
- **Related:** polkitd Authority (`RegisterAuthenticationAgent`); `desktop/aizome/crates/aizome-shell/src/auth.rs:200-217` (owns the agent)

<a id="syb-11"></a>

### SYB-11 — Backlight brightness shown before logind sets it, and the sysfs watcher
- **Where:** `desktop/aizome/crates/aizome-sys/src/power/backlight.rs:123` (`Backlight.model`), `backlight.rs:121` (`session` cache), watcher `backlight.rs:238`
- **Encoding:** `apply` writes the requested level into the model (`backlight.rs:211-215`) before logind's SetBrightness thread runs (`backlight.rs:217-227`); the watcher thread re-reads sysfs on POLLPRI or every second and calls `set_if_changed` (`backlight.rs:264`)
- **States:** `Confirmed(level)` → `Requested(level)` (call in flight, not tracked) → `Confirmed(new)` | `Reverted` (the watcher read the old level, or the call failed)
- **Transitions:** `step`/`set_brightness`/`set_fraction` → `apply` (`backlight.rs:174-200`) → model updated (`backlight.rs:211`); the call's result (`backlight.rs:225-226`); the watcher overwrites (`backlight.rs:240-265`); Drop stops it (`backlight.rs:231-235`)
- **Hazards:** a requested level isn't marked pending. A watcher pass between `backlight.rs:211` and logind applying the level writes the old level back (OSD flicker), and a quick next `step()` (`backlight.rs:195`) steps from the reverted level, losing a press. A failed call leaves the requested level shown until the next pass. The session path is cached forever (`backlight.rs:162-170`)
- **Reify:** Medium — an unmarked request state races a polling writer
- **Related:** `desktop/aizome/crates/aizome-shell/src/osd.rs:189` (brightness keys); logind `Session.SetBrightness`

<a id="syb-12"></a>

### SYB-12 — logind sleep-delay inhibitor and lock-before-sleep handshake
- **Where:** `desktop/aizome/crates/aizome-sys/src/power/logind.rs:143` (`Shared.events`, `.delay`, `.want_delay`); `logind.rs:103` (`LogindState.sleep_delay_held`), `logind.rs:93` (`preparing_for_sleep`)
- **Encoding:** `delay: Option<OwnedFd>` (the held inhibitor); `want_delay` bool; an `events` Vec drained by `take_events`; `sleep_delay_held`, written both by `model.update` and at Idle; the shell's `lock.sleep_pending`
- **States:** `NotWanted` → `Held` → `SleepPending` (PrepareForSleep(true) queued, fd held until `sleep_ready`) → `Released` (asleep) → `Held` (retaken on resume or Appeared); `Lost` (logind vanished: fd gone, want kept)
- **Transitions:** `take_sleep_delay` (`logind.rs:381-390`); PrepareForSleep(true): queue the event, drop the fd unless wanted (`logind.rs:268-273`); `sleep_ready` (`logind.rs:393-396`); PrepareForSleep(false): queue Resumed, retake (`logind.rs:274-278`); `release_sleep_delay` (`logind.rs:399-405`); Vanished (`logind.rs:231`); Appeared retakes (`logind.rs:222-226`); Idle mirrors `held` (`logind.rs:295-296`)
- **Hazards:** the delay is retaken twice per resume, by the follower (`logind.rs:276-278`) and by the shell's Resumed handler (`desktop/aizome/crates/aizome-shell/src/shell.rs:2062-2066`). Inhibit failures are swallowed (`.ok()`, `logind.rs:225`, `logind.rs:277`). The follower holds the `shared` lock across the blocking Inhibit call (`logind.rs:223-226`, `logind.rs:267-278`), which stalls `take_events`/`sleep_ready` on the UI thread. `take_sleep_delay` is itself a blocking call made on the UI thread. `sleep_ready` doesn't say which sleep it answers. `events` is unbounded
- **Reify:** High — state across three fields, the model and the shell, seven mutation sites, and a security-relevant outcome
- **Suspected bug:** The lock isn't confirmed before logind's delay limit expires, so the machine sleeps with the shell's `sleep_pending` still set. On resume the delay is retaken (`logind.rs:276-278`, `shell.rs:2063`). Then `lock_confirmed` (`desktop/aizome/crates/aizome-shell/src/lock.rs:365-367`) calls `sleep_ready()` and drops the fresh inhibitor. Nothing retakes it until the next resume, so the next suspend isn't delayed and can sleep before the screen locks (confidence: medium-low)
- **Related:** `desktop/aizome/crates/aizome-shell/src/shell.rs:2041-2090`, `desktop/aizome/crates/aizome-shell/src/lock.rs:361-389`; logind `Inhibit("sleep", "delay")`
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#syb-12)
- **Follow-up:** reified as typestate in [#298](https://github.com/bitemyapp/sekka/pull/298). Its suspected bug is fixed, and the mistake no longer compiles.

<a id="syb-13"></a>

### SYB-13 — Background Task result slot
- **Where:** `desktop/aizome/crates/aizome-sys/src/service.rs:45` (`Slot.result`, `.finished`); `service.rs:147` (`Done.slot`)
- **Encoding:** `Mutex<Option<Result<T>>>`, an `AtomicBool` finished, a `Condvar`; `Done.slot: Option<Arc<Slot>>` is taken once
- **States:** `Pending` → `Finished(result)` → `Taken` (result None, finished true); `Cancelled` when the Done drops without a result
- **Transitions:** `pending` (`service.rs:80-83`); `spawn` (`service.rs:66-75`); `ready` (`service.rs:87-95`); `complete` (`service.rs:157-166`); Done Drop → Cancelled (`service.rs:168-172`); `take` (`service.rs:104-106`); `wait` (`service.rs:109-120`)
- **Hazards:** `try_result`/`take` return None for both Pending and Taken. `wait_timeout` (`service.rs:123-134`) on a taken task waits out the whole timeout. Dropping a Task never cancels its thread (joins poll for up to 45 s, SYB-02). A failed spawn drops the Done (a wake and Cancelled on an orphan slot) and returns a different ready Task (`service.rs:69-73`)
- **Reify:** Low — local, and `is_done` disambiguates

<a id="syb-14"></a>

### SYB-14 — Service model generation and the Stop latch
- **Where:** `desktop/aizome/crates/aizome-sys/src/service.rs:176` (`Model.current`, `.generation`, `.wakes`); `service.rs:257` (`Stop`)
- **Encoding:** an `RwLock<Arc<M>>` snapshot; an `AtomicU64` change counter the UI compares; a one-way `AtomicBool` stop flag polled by service threads
- **States:** generation n → n+1 on each change; Stop: `Running` → `Stopped`
- **Transitions:** `set` (`service.rs:193-196`); `set_if_changed` (`service.rs:199-206`); `update` (`service.rs:209-219`); `changed` (`service.rs:223-229`); `Stop::stop` from every handle's Drop (`service.rs:260-262`)
- **Hazards:** `set_if_changed` compares through `get()` and then calls `set()` without holding the lock in between, and its callers build the model from inputs read earlier, so a concurrent `update()` can be lost. This happens with the rfkill watcher (`network/mod.rs:178`) against the Follower (`network/mod.rs:576-579`), `take_sleep_delay`/`sleep_ready` (`power/logind.rs:388`, `power/logind.rs:395`) against logind's Idle (`power/logind.rs:295-296`), and `apply` (`power/backlight.rs:211`) against the watcher (`power/backlight.rs:264`); paths under `desktop/aizome/crates/aizome-sys/src/`. Stop is noticed only at a thread's next poll; a thread blocked in a call keeps running
- **Reify:** Medium — the lost update affects user-visible fields in three services

<a id="syb-15"></a>

### SYB-15 — StatusNotifierWatcher service lifecycle and name ownership
- **Where:** `desktop/aizome/crates/aizome-sys/src/statusnotifier/mod.rs:162` (`StatusNotifier.serving`, `.named`, `.stop`); threads `mod.rs:328-372`
- **Encoding:** the `serving`/`named` bools, the `Stop` latch, two threads (drain and follow), the bus name's actual owner (unobserved)
- **States:** `None` (no bus) → `Following` (subscribed, `mod.rs:359`) → `Serving` (`mod.rs:210`) → `Named` (`mod.rs:219`) → `Running` → [`Replaced`] | [`Dead`: follow thread ended, items cleared] → `Stopped`
- **Transitions:** `start` (`mod.rs:200-227`): follow (`mod.rs:207`), serve (`mod.rs:210-213`), ping (`mod.rs:214`), name (`mod.rs:215-226`); the follow thread ends and clears items when not stopped (`mod.rs:363-368`); Drop: stop, release, remove (`mod.rs:299-311`)
- **Hazards:** with AllowReplacement|ReplaceExisting (`mod.rs:215-216`), any later watcher can take the name. `named`/`available()` (`mod.rs:230-232`) never learn of it, so items go stale while apps register elsewhere. If the follow thread ends without a stop, the watcher still answers but RegisterStatusNotifierItem fails "stopping" while `available()` stays true. Drop doesn't join the threads, and the drain thread notices stop only on its next message (`mod.rs:352`)
- **Reify:** Medium — flags and threads whose liveness isn't reflected
- **Related:** `desktop/aizome/crates/aizome-shell/src/sys.rs:53`, `desktop/aizome/crates/aizome-shell/src/systray/glue.rs`; apps' libappindicator/Qt items

<a id="syb-16"></a>

### SYB-16 — Status item registry: register, re-read, gone
- **Where:** `desktop/aizome/crates/aizome-sys/src/statusnotifier/mod.rs:93` (`Items.map`, `.seq`), `mod.rs:87` (`Tracked.owner`)
- **Encoding:** key → `Tracked { owner unique name, item }`; a `seq` counter fixes order; a per-burst `stale: BTreeSet` (`mod.rs:394`) coalesces re-reads
- **States:** per item: `Unregistered` → `Registered(order n)` → `Stale` → `Registered(re-read)` → `Gone`
- **Transitions:** RegisterStatusNotifierItem → inbox (`mod.rs:116-127`) → `register` (`mod.rs:433-462`: owner `mod.rs:434`, order kept or allocated `mod.rs:449-455`, insert `mod.rs:457`, signal `mod.rs:459-461`); an item signal marks it stale (`mod.rs:421-427`) → `reread` (`mod.rs:486-497`, keeps the old item on failure `mod.rs:492`); NameOwnerChanged with an old owner → `gone` (`mod.rs:413-420`, `mod.rs:465-482`); publish after each burst (`mod.rs:402-403`)
- **Hazards:** Register and NameOwnerChanged reach the inbox from different threads, so their order isn't guaranteed; `get_all` failing on a vanished peer covers it. `gone` for a well-known name drops items even when the name moved straight to a new owner, relying on the app to re-register. One hung item stalls every update for the query timeout (one follow thread). RegisteredStatusNotifierItems never emits PropertiesChanged
- **Reify:** Medium — per-item phase lives in the map, the stale set and the signals' order
- **Related:** apps' `org.kde.StatusNotifierItem` objects; `desktop/aizome/crates/aizome-shell/src/systray/glue.rs`

## Already explicit

- `desktop/aizome/crates/aizome-sys/src/network/model.rs:172` `enum NmState` — NetworkManager's overall state; transitions belong to NetworkManager, decoded in `fn from_u32` (`desktop/aizome/crates/aizome-sys/src/network/model.rs:185`)
- `desktop/aizome/crates/aizome-sys/src/network/model.rs:350` `enum DeviceState` — a device's activation state; transitions belong to NetworkManager, decoded in `fn from_u32` (`desktop/aizome/crates/aizome-sys/src/network/model.rs:369`)
- `desktop/aizome/crates/aizome-sys/src/network/model.rs:748` `enum ActiveState` — an active connection's state; transitions belong to NetworkManager, decoded in `fn from_u32` (`desktop/aizome/crates/aizome-sys/src/network/model.rs:758`), polled by `wait_active` (`desktop/aizome/crates/aizome-sys/src/network/mod.rs:796`)
- `desktop/aizome/crates/aizome-sys/src/power/upower.rs:145` `enum BatteryState` — a battery's charge state; transitions belong to UPower, decoded in `fn from_u32` (`desktop/aizome/crates/aizome-sys/src/power/upower.rs:158`)
- `desktop/aizome/crates/aizome-sys/src/statusnotifier/item.rs:36` `enum ItemStatus` — an item's Passive/Active/NeedsAttention; set by the app, parsed in `fn parse` (`desktop/aizome/crates/aizome-sys/src/statusnotifier/item.rs:46`), replaced whole on each re-read (`desktop/aizome/crates/aizome-sys/src/statusnotifier/mod.rs:495`)

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/crates/aizome-sys/src/network/mod.rs` | 861 | machines | [SYB-01](#syb-01), [SYB-02](#syb-02), [SYB-03](#syb-03) | airplane fallback marker, join transaction, NM follower |
| `desktop/aizome/crates/aizome-sys/src/network/model.rs` | 1655 | machines | [SYB-01](#syb-01) | pure build(); airplane derivation in SYB-01; NM state enums explicit |
| `desktop/aizome/crates/aizome-sys/src/network/profile.rs` | 352 | none |  | settings mapping and one-shot update tasks; RMW hazard noted in SYB-02 |
| `desktop/aizome/crates/aizome-sys/src/network/rfkill.rs` | 232 | machines | [SYB-04](#syb-04), [SYB-01](#syb-01) | rfkill watcher thread; block_all used by airplane mode |
| `desktop/aizome/crates/aizome-sys/src/notifications/client.rs` | 322 | machines | [SYB-05](#syb-05) | dispatcher sent/early/tokens and subscribe handshake |
| `desktop/aizome/crates/aizome-sys/src/notifications/hints.rs` | 407 | none |  | pure hint parsing and image data |
| `desktop/aizome/crates/aizome-sys/src/notifications/mod.rs` | 529 | none |  | data types, markup stripping, Timeout config |
| `desktop/aizome/crates/aizome-sys/src/notifications/server.rs` | 654 | machines | [SYB-06](#syb-06), [SYB-07](#syb-07) | expiry timers; server and name lifecycle |
| `desktop/aizome/crates/aizome-sys/src/oneshot.rs` | 87 | machines | [SYB-08](#syb-08) | one-value async channel |
| `desktop/aizome/crates/aizome-sys/src/polkit.rs` | 1022 | machines | [SYB-09](#syb-09), [SYB-10](#syb-10) | auth session conversation; registration loop |
| `desktop/aizome/crates/aizome-sys/src/power/backlight.rs` | 320 | machines | [SYB-11](#syb-11) | optimistic level vs sysfs watcher |
| `desktop/aizome/crates/aizome-sys/src/power/logind.rs` | 525 | machines | [SYB-12](#syb-12), [SYB-03](#syb-03) | sleep delay inhibitor; logind follower |
| `desktop/aizome/crates/aizome-sys/src/power/mod.rs` | 29 | none |  | module re-exports |
| `desktop/aizome/crates/aizome-sys/src/power/profiles.rs` | 195 | machines | [SYB-03](#syb-03) | PPD follower; daemon name fixed at start |
| `desktop/aizome/crates/aizome-sys/src/power/upower.rs` | 570 | machines | [SYB-03](#syb-03) | UPower follower; BatteryState mirror explicit |
| `desktop/aizome/crates/aizome-sys/src/service.rs` | 330 | machines | [SYB-13](#syb-13), [SYB-14](#syb-14) | Task slot; model generation and Stop latch |
| `desktop/aizome/crates/aizome-sys/src/statusnotifier/item.rs` | 274 | explicit |  | property parsing; ItemStatus mirror enum |
| `desktop/aizome/crates/aizome-sys/src/statusnotifier/menu.rs` | 174 | none |  | pure dbusmenu layout parsing |
| `desktop/aizome/crates/aizome-sys/src/statusnotifier/mod.rs` | 531 | machines | [SYB-15](#syb-15), [SYB-16](#syb-16) | watcher lifecycle; item registry |
| `desktop/aizome/crates/aizome-sys/src/users.rs` | 482 | machines | [SYB-03](#syb-03) | AccountsService follower; DeleteUser token is a value |
