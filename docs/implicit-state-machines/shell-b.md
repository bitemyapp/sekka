# SHB: Shell: lock, notifications, recording, systray and services

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/crates/aizome-shell`. Audited at `8e0acbe5`; line numbers refer to that commit.

61 files, 0 pending. 30 implicit machines: 7 High, 7 Medium, 16 Low.

## Machines

<a id="shb-01"></a>

### SHB-01 — App connection registry (hello, menus, goodbye)
- **Where:** `desktop/aizome/crates/aizome-shell/src/apps/registry.rs:27` (`Registry.conns`); `registry.rs:14` (`AppConn.menus`, `.generation`)
- **Encoding:** presence of a `ConnId` key in the map (no key = connected but not helloed, or gone); `menus` + `generation` counter per connection
- **States:** `Unknown` (socket open, no Hello) → `Helloed` (menus empty, generation 0) → `Published` (menus, generation n) → `Gone`; a second Hello on the same connection silently resets to `Helloed`
- **Transitions:** Hello inserts/resets (`registry.rs:45`); Menus before Hello dropped (`registry.rs:65`); Menus changed bumps generation (`registry.rs:59`-`62`); unchanged Menus → Nothing (`registry.rs:64`); connection closed removes (`registry.rs:76`); lookups prefer the newest connection of a pid (`registry.rs:84`)
- **Hazards:** a re-Hello resets `generation` to 0, so it is not monotonic per connection; the reader thread's `Gone` (SHB-02) is the only exit, so a half-open socket keeps a stale app's menus
- **Reify:** Low — small, map-keyed, one handler
- **Related:** app side `desktop/aizome/crates/aizome-shell-proto/src/lib.rs:280` (`Client::hello`), `desktop/aizome/crates/aizome-kit/src/shell.rs`

<a id="shb-02"></a>

### SHB-02 — Shell socket ownership and per-connection threads
- **Where:** `desktop/aizome/crates/aizome-shell/src/apps/server.rs:25` (`Server.writers`, `.path`, `.handles`, `.inode`)
- **Encoding:** socket file at `path` with its inode remembered; `Arc<()>` strong count as "last handle"; `writers` map of per-connection `Sender`; an accept thread with a `next` id counter; a reader and a writer thread per connection
- **States:** socket: `Absent` → `Probed` (live-shell check) → `Bound(inode)` → `Replaced` (another shell bound the path) | `Removed`; connection: `Accepted` → `Serving` (reader + writer) → `WriterDead` (write failed, still in map) → `Gone`
- **Transitions:** live check refuses (`server.rs:43`); `listen_replacing` unlinks and binds (`server.rs:57`, `61`), records inode (`server.rs:62`); accept assigns id (`server.rs:70`-`71`); writer registered (`server.rs:114`); writer exits on error (`server.rs:117`-`118`); reader ends → removed and `Gone` (`server.rs:134`-`135`); last handle unlinks only if inode matches (`server.rs:95`-`100`); restart chooses `listen_replacing` (`shell.rs:1067`)
- **Hazards:** check-then-bind race: two shells starting together both pass `listen` and the second unlinks the first's socket (`server.rs:41`-`57`); the accept thread never ends after `Drop`; after a writer error its `Sender` stays in `writers` until the reader notices, so sends are dropped silently; the `listen` probe itself appears as a connection to the live shell; inode reuse on the path would let an old shell's `Drop` unlink a new shell's socket
- **Reify:** Medium — ownership spread over path/inode/refcount plus thread lifetimes
- **Related:** clients in `desktop/aizome/crates/aizome-shell-proto/src/lib.rs:280`; restart handover in `shell.rs:1061`-`1067`

<a id="shb-03"></a>

### SHB-03 — Polkit authentication dialog
- **Where:** `desktop/aizome/crates/aizome-shell/src/auth.rs:58` (`AuthState.prompt`, `.password`, `.info`, `.error`, `.busy`, `.identity`, `.window`); `Shell.auth` (`shell.rs:297`), `Shell.polkit` (`shell.rs:1567`)
- **Encoding:** `Option<AuthState>` keyed by polkit cookie; `busy` bool; `prompt`/`error`/`info` options; window key in `surfaces`
- **States:** `Closed` → `Starting` (window open, conversation started, no prompt) → `Prompted` → `Checking` (`busy`) → `Failed` (error, restarted) → `Prompted` … → `Done`/`Cancelled` → `Closed`; agent: `Registering` → `Ready` | `Failed`
- **Transitions:** Begin cancels any old request, opens overlay, starts conversation (`auth.rs:241`-`273`); no focused screen → cancel (`auth.rs:246`-`249`); Prompt clears busy (`auth.rs:277`-`278`); Error (`auth.rs:283`-`284`); Info (`auth.rs:287`); Failed clears password and restarts the conversation (`auth.rs:291`-`295`); Done closes (`auth.rs:298`); Identity restarts (`auth.rs:307`-`310`); Authenticate sets busy and responds (`auth.rs:318`-`321`); Cancel (`auth.rs:324`-`325`); close (`auth.rs:331`-`333`); agent registration on a thread (`auth.rs:205`-`216`), result stored (`shell.rs:1567`-`1570`)
- **Hazards:** `Failed` always restarts the conversation with no attempt cap or delay (`auth.rs:294`-`295`); if the window's output goes, `close_requested` only drops the `surfaces` entry (`shell.rs:1712`), leaving `auth` pointing at a dead window while polkit waits; a response sent before the first Prompt is queued by aizome-sys, but the Prompt then clears `busy` while the helper is already checking, and a second submit is queued as the next prompt's answer; `info` is never cleared; agent registration failure is only logged, never retried
- **Reify:** High — six handlers, four phase fields, privilege boundary, suspected bug
- **Suspected bug:** when the helper can't be reached (`polkit-agent-helper` socket missing, spawn error), `Session::start` emits Error then Failed at once (`desktop/aizome/crates/aizome-sys/src/polkit.rs:247`-`252`); the shell's Failed handler calls `authenticate` again (`auth.rs:294`-`295`), which fails the same way, so the shell spins through connect attempts and warnings on its event loop until the user cancels (confidence: medium)
- **Related:** `desktop/aizome/crates/aizome-sys/src/polkit.rs:233` (`Session::start`, conversation generation, `respond` queue)

<a id="shb-04"></a>

### SHB-04 — Keep Awake status (request, watcher, indicator)
- **Where:** `desktop/aizome/crates/aizome-shell/src/awake.rs:80` (watcher's `bus`, `last`); `Shell.awake` (`shell.rs:362`), `Shell.tray.awake`
- **Encoding:** `aizome_awake::Status` held in two places (shell and the watcher's `last`), a lazily opened system-bus connection, the requested-setting file read by `aizome_awake::requested()`
- **States:** `Off` → `Requested` (aizome-awake running) → `On` | `Unprotected` (setting on, logind doesn't list locks) → `Off`
- **Transitions:** switch spawns `aizome-awake on|off` (`awake.rs:32`-`42`), or sets the status directly off the real session (`awake.rs:28`-`31`); result sets status optimistically (`awake.rs:48`) or notifies (`awake.rs:49`-`58`); watcher polls every 2 s and sends only changes (`awake.rs:85`-`92`); bus opened lazily (`awake.rs:107`-`109`), dropped on error → `Unprotected` (`awake.rs:124`-`126`); status change updates tray and closes the cup's panel when Off (`awake.rs:69`-`74`); watcher exits when the shell is gone (`awake.rs:88`-`89`)
- **Hazards:** two writers to `Shell.awake` (optimistic `AwakeSet` and the watcher) with no ordering; a transient D-Bus error reports `Unprotected` (red) for up to 2 s while the locks are held
- **Reify:** Low — explicit status enum, small
- **Related:** `desktop/aizome/crates/aizome-awake/src/lib.rs:54` (`requested`) and its user service that holds the inhibitors

<a id="shb-05"></a>

### SHB-05 — Timed overlay surfaces (bell flash, OSD)
- **Where:** `desktop/aizome/crates/aizome-shell/src/bell.rs:40` (`Shell.flash: Option<(WindowKey, u64)>`, `Shell.serial`); `desktop/aizome/crates/aizome-shell/src/osd.rs:64` (`OsdState.window`, `.generation`)
- **Encoding:** `Option` of window key plus a generation; a `cx.after` timer carrying that generation; for the flash, the shell-wide `serial` counter (`shell.rs:305`)
- **States:** `Hidden` → `Shown(gen)` → `Shown(gen+1)` (re-shown/extended) → `Hidden`
- **Transitions:** bell closes any flash and opens one (`bell.rs:28`-`40`), timer (`bell.rs:41`), matching timer closes (`bell.rs:45`-`54`); OSD opens or updates and bumps generation (`osd.rs:102`-`119`), timer (`osd.rs:122`), matching timer closes (`osd.rs:126`-`130`)
- **Hazards:** the OSD generation restarts at 1 each time it reopens (`osd.rs:102`), safe only because every timer has the same `SHOWS` and fires in order; both stay on the output they opened on; if the compositor closes the layer (output gone) `close_requested` keeps `flash`/`osd` pointing at the dead window until the timer fires (`shell.rs:1712`), so OSD updates in that window go nowhere
- **Reify:** Low — small, self-healing within 1.2 s

<a id="shb-06"></a>

### SHB-06 — Session confirmation countdown (Restart, Shut Down, Log Out, power key)
- **Where:** `desktop/aizome/crates/aizome-shell/src/confirm.rs:36` (`Confirm.window`, `.verb`, `.left`, `.generation`, `.power`); `Shell.confirm` (`shell.rs:306`)
- **Encoding:** `Option<Confirm>`; seconds left; generation from `Shell.serial`; self-re-arming 1 s `Tick(generation)` timers
- **States:** `Idle` → `Counting(verb, left, power)` → `Done(verb)` | `Cancelled`; re-ask with the same `power` resets `left` in place
- **Transitions:** ask with same `power` changes verb and resets left (`confirm.rs:110`-`115`); otherwise close and open a new overlay (`confirm.rs:117`-`127`), first tick (`confirm.rs:128`); Cancel (`confirm.rs:135`); Now (`confirm.rs:137`-`138`); Do (`confirm.rs:141`-`142`); Tick decrements and re-arms (`confirm.rs:149`-`152`) or fires at 1 (`confirm.rs:145`-`147`); stale tick ignored (`confirm.rs:155`); verb carried out (`confirm.rs:173`-`191`), or recorded off-live (`confirm.rs:168`-`171`)
- **Hazards:** with no focused screen an ask closes the existing dialog and shows nothing (`confirm.rs:117`-`118`); the dialog's close by the compositor isn't tied to `confirm`
- **Reify:** High — user-visible power-off/logout consequences, suspected bug
- **Suspected bug:** the dialog is a layer on the focused output (`confirm.rs:118`-`122`); if that output goes (dock unplugged, monitor off) during the minute, `close_requested` removes only the `surfaces` entry (`shell.rs:1712`), `confirm` stays `Some`, ticks continue and the machine shuts down/restarts/logs out with no dialog to cancel; asking again only resets `left` (`confirm.rs:110`-`115`) and never reopens a window (confidence: medium)

<a id="shb-07"></a>

### SHB-07 — Login display-scales publisher (coalescing worker)
- **Where:** `desktop/aizome/crates/aizome-shell/src/greeter.rs:7` (worker thread and channel); `Shell.greeter_scales` (`shell.rs:1120`)
- **Encoding:** an mpsc channel; worker blocked in `recv`, draining queued newer values
- **States:** `Idle` → `Publishing(latest)` → `Idle`; newer values queued while publishing are collapsed to the last
- **Transitions:** receive (`greeter.rs:12`), coalesce (`greeter.rs:13`-`15`), publish over the system bus (`greeter.rs:17`-`18`), failure logged (`greeter.rs:19`-`21`)
- **Hazards:** a failed publish is never retried until the next change; if the thread fails to spawn (`greeter.rs:24`) the returned sender's sends fail silently forever
- **Reify:** Low — already a clean single-owner worker
- **Related:** `desktop/aizome/crates/aizome-sys/src/users.rs` (`publish_display_scales`), the greeter reading them

<a id="shb-08"></a>

### SHB-08 — GTK look publication to dconf
- **Where:** `desktop/aizome/crates/aizome-shell/src/gtk.rs:51` (`tell`); `Shell.gtk_look` (`shell.rs:269`, compared at `shell.rs:1972`)
- **Encoding:** last-told `GtkLook`; one detached thread per change writing four keys
- **States:** `Told(look)` → `Writing(look')` (possibly several threads at once) → `Told(look')`
- **Transitions:** dedupe and record before writing (`shell.rs:1972`-`1974`); thread writes keys in order (`gtk.rs:53`-`63`), stops at first spawn error (`gtk.rs:59`-`61`)
- **Hazards:** writer threads are unordered: two changes in quick succession (Night → Day → Night in the Display panel) can interleave, leaving dconf with the older look or a mix of keys; `gtk_look` is recorded as told even when dconf fails, so it is never retried
- **Reify:** Low — fix is a single coalescing worker as in SHB-07

<a id="shb-09"></a>

### SHB-09 — Compositor event follower (connect, snapshot, subscribe, reconnect)
- **Where:** `desktop/aizome/crates/aizome-shell/src/ipc.rs:122` (`follow`: `wait`, `was_connected`); `ipc.rs:151` (`connect`)
- **Encoding:** loop-local backoff `Duration`, `was_connected` flag; `IpcEvent::Connected/Event/Disconnected` stream to the shell, which resets its model on `Connected` (`shell.rs:1171`-`1174`)
- **States:** `Connecting` → `Snapshotting` → `Subscribed` (streaming) → `Disconnected` → `Backoff(wait)` → `Connecting`
- **Transitions:** connect (`ipc.rs:126`); success resets backoff (`ipc.rs:128`), sends snapshot (`ipc.rs:130`); events relayed, bad lines skipped, other errors end the stream (`ipc.rs:131`-`137`); Disconnected (`ipc.rs:138`); sleep and double, capped at 3 s (`ipc.rs:146`-`147`); snapshot by four calls then Subscribe (`ipc.rs:154`-`167`)
- **Hazards:** the snapshot is four separate requests, so windows and focus may disagree; nothing ties the request thread (SHB-10) to this connection's state
- **Reify:** High — suspected bug, reconnect semantics matter to the whole shell
- **Suspected bug:** events between the snapshot calls and `Subscribe` are lost: the compositor's `Subscribe` only sets `subscribed` with no replay (`desktop/aizome/crates/aizome-comp-core/src/ipc.rs:207`-`211`), so a window mapped in that gap (autostart apps at login, or while the shell reconnects after a compositor restart) is missing from the Deskbar and menus until the next reconnect (confidence: medium)
- **Related:** `desktop/aizome/crates/aizome-comp-core/src/ipc.rs:207`; `desktop/aizome/crates/aizome-ipc/src/lib.rs:1094` (`Client::events`)

<a id="shb-10"></a>

### SHB-10 — Compositor request sender (lazy connection, one retry)
- **Where:** `desktop/aizome/crates/aizome-shell/src/ipc.rs:41` (`Ipc.tx`); request thread's `client: Option<Client>` (`ipc.rs:71`)
- **Encoding:** `Option<Client>` reopened lazily; two-attempt loop per request; optional reply tag
- **States:** `NoClient` → `Connected` → (request error) `NoClient` → retry once → `Answered` | `Dropped`
- **Transitions:** connect on demand (`ipc.rs:75`-`77`); success (`ipc.rs:80`-`82`); error drops client and retries (`ipc.rs:84`-`87`); replies routed by tag, tagless errors logged, tagged failures answered "no compositor" (`ipc.rs:90`-`95`)
- **Hazards:** a request whose write succeeded but whose reply read failed (compositor restarting) is resent on a new connection, so non-idempotent requests (Close, NewDesk, MoveToDesk, Quit) may run twice; tagless requests sent while the compositor is down are dropped with only a debug log (e.g. `ReloadConfig` after a settings save)
- **Reify:** Medium — retry semantics are implicit and per-request

<a id="shb-11"></a>

### SHB-11 — GeoClue location lookup
- **Where:** `desktop/aizome/crates/aizome-shell/src/location.rs:48` (`Shell.locating`, `Shell.located_at` at `shell.rs:364`-`365`)
- **Encoding:** `locating: bool`, `located_at: Option<Instant>`, a worker thread, the `[location]` section of `settings.toml`
- **States:** `Never` → `Locating` → `Located(at)` → (after 6 h) `Due` → `Locating`
- **Transitions:** guard (`location.rs:49`-`54`); start (`location.rs:56`-`62`), spawn failure resets (`location.rs:63`-`65`); answer clears and stamps (`location.rs:71`-`72`); error ends (`location.rs:75`-`78`); unchanged ends (`location.rs:81`-`83`); reload, save, apply (`location.rs:85`-`92`)
- **Hazards:** a failure stamps `located_at` too (`location.rs:72`), so a GeoClue/network not ready at login means no retry for six hours; no shell-side timeout, so a thread that never replies leaves `locating` true forever; read-modify-write of `settings.toml` (`location.rs:85`-`87`) races a concurrent Settei save
- **Reify:** Low — two fields, one handler pair

<a id="shb-12"></a>

### SHB-12 — Lock screen lifecycle (partially explicit)
- **Where:** `desktop/aizome/crates/aizome-shell/src/lock.rs:47` (`LockState.phase`, `.surfaces`, `.password`, `.busy`, `.error`, `.caps`, `.since`, `.sleep_pending`); `enum Phase` (`lock.rs:25`)
- **Encoding:** partially explicit: `Phase` enum plus phase-dependent fields (`busy` for an in-flight PAM thread, `sleep_pending` for logind's delay inhibitor, per-output surface list); the toolkit's own `Locking` state underneath
- **States:** `Unlocked` → `Locking` (surfaces open, awaiting compositor) → `Locked` → `Checking` (`busy`) → `Locked`+error | `Unlocked`; orthogonal `sleep_pending`; invalid but representable: `busy` with a different lock instance than the one that started the check
- **Transitions:** `lock()` Unlocked→Locking (`lock.rs:322`-`328`); surfaces follow outputs (`lock.rs:338`-`358`), closed surface replaced (`shell.rs:1713`-`1717`); `lock_confirmed` →Locked, releases sleep delay (`lock.rs:362`-`368`); `lock_ended` →Unlocked, clears busy, releases delay (`lock.rs:375`-`387`); `unlock` (`lock.rs:395`-`396`); Submit sets busy, spawns PAM (`lock.rs:416`-`420`); Result clears busy and unlocks or errors (`lock.rs:423`-`433`); typing ignored while busy (`lock.rs:402`-`405`); PrepareForSleep sets `sleep_pending` and locks (`shell.rs:2052`-`2054`); logind Lock/Unlock (`shell.rs:2049`-`2050`)
- **Hazards:** PAM results carry no lock generation: a result arriving after `lock_ended` and a re-lock acts on the new lock (Ok unlocks it, Err shows an error on a fresh prompt), and `lock_ended` clears `busy` (`lock.rs:380`) while the old PAM thread runs, so two checks can overlap; PrepareForSleep while `Locking` releases the delay at once (`shell.rs:2052`, `2055`-`2056`) before the lock is confirmed; `lock_confirmed` doesn't check the phase and relies on sidestep-ui ignoring a late `SessionLocked` (`desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:1605`-`1609`); `caps` survives unlock
- **Reify:** High — 8 fields, 8+ mutation sites across two files, security boundary
- **Suspected bug:** whatever PAM says is logged and replaced by "That password didn't work" (`lock.rs:427`-`428`), so an expired password (`pam_acct_mgmt` → new token required) or a faillock lockout reports the correct password as wrong on every try and the user can't learn why (confidence: medium)
- **Related:** `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:1057` (kit's `Locking`), compositor ext-session-lock; logind sleep delay in `desktop/aizome/crates/aizome-sys/src/power/logind.rs:437`

<a id="shb-13"></a>

### SHB-13 — Lock field shake animation
- **Where:** `desktop/aizome/crates/aizome-shell/src/lock.rs:56` (`LockState.shake`)
- **Encoding:** step counter 0–9 advanced by self-scheduled 30 ms timers carrying the next step
- **States:** `Still(0)` → `Step(1..8)` → `Still(0)`
- **Transitions:** failure starts at 1 if motion allowed (`lock.rs:429`-`431`); each step re-arms (`lock.rs:438`-`441`)
- **Hazards:** no generation: a second failure inside 270 ms would run two chains (prevented only by PAM's failure delay)
- **Reify:** Low — purely cosmetic

<a id="shb-14"></a>

### SHB-14 — Critical notification cards surface
- **Where:** `desktop/aizome/crates/aizome-shell/src/notifications/cards.rs:105` (`Shell.cards: Option<(WindowKey, usize, f64)>`, `shell.rs:311`)
- **Encoding:** window key, card count shown and right margin, reconciled on every update against `notes.cards()` and the lock
- **States:** `None` → `Shown(n, right)` → resized/moved → `None` (no cards, or locked)
- **Transitions:** none while locked (`cards.rs:106`); close at 0 (`cards.rs:115`-`118`); open on the primary output (`cards.rs:121`-`131`); resize and re-margin (`cards.rs:134`-`141`); actions mark read or dismiss (`cards.rs:146`-`157`)
- **Hazards:** the surface stays on the output it opened on when the primary changes
- **Reify:** High — suspected bug, user-visible loss of critical notifications
- **Suspected bug:** when the cards' output goes, the compositor closes the layer and `close_requested` drops only the `surfaces` entry (`shell.rs:1712`); `cards` keeps the dead key, so `sync_cards` keeps resizing it (`cards.rs:135`) and never reopens, and critical cards stay invisible until every one is read elsewhere (confidence: medium)

<a id="shb-15"></a>

### SHB-15 — Bar notification line (banner)
- **Where:** `desktop/aizome/crates/aizome-shell/src/notifications/glue.rs:184` (`Shell.banner: Option<u32>`, `Shell.banner_queue` at `shell.rs:258`-`259`)
- **Encoding:** id of the notification in the line; `BannerDone(id)` timers keyed by notification id only; a `banner_queue` that is never filled
- **States:** `Menus` → `Banner(id)` → `Banner(id')` (newer takes the line, old expires) → `Menus`
- **Transitions:** post shows (`glue.rs:85`-`87`); show expires the replaced one and arms a timer (`glue.rs:185`-`193`); timer ends if still current (`glue.rs:198`-`205`); app close ends it (`glue.rs:96`-`99`)
- **Hazards:** `banner_queue` is dead state (only `retain`ed, `glue.rs:96`); `banner_done` closes transient ones as Expired (`glue.rs:203`-`205`)
- **Reify:** High — suspected bug, timer identity is implicit
- **Suspected bug:** a replacement of the notification on the line (same id through `replaces_id`) arms a second `BannerDone(id)` while the first is still pending; the first ends the line at the original deadline (`glue.rs:198`-`201`) and, for a transient one, closes the just-updated notification. A progress notification updated every 2 s gets its line cut short and flickering (confidence: medium)

<a id="shb-16"></a>

### SHB-16 — Notifications server: bus name ownership and ids
- **Where:** `desktop/aizome/crates/aizome-shell/src/notifications/server.rs:161` (`NoteServer.tx`, `.next`); `watch_name` (`server.rs:229`)
- **Encoding:** zbus connection serving the object; well-known name requested with replace/allow-replacement/no-queue; `NameAcquired`/`NameLost` signal threads; shared `AtomicU32` id counter; signal-emitter thread fed by a channel
- **States:** `Serving(no name)` → `Owner` → `Lost` (another shell took it) | `Refused` (another server keeps it)
- **Transitions:** connection built (`server.rs:182`-`183`); subscribe before requesting (`server.rs:233`); request (`server.rs:236`-`240`); acquired (`server.rs:244`-`248`); lost (`server.rs:251`-`255`) → shell retires (`shell.rs:1584`-`1588`); ids from `replaces_id` or the counter (`server.rs:57`), shell's own (`server.rs:215`-`216`)
- **Hazards:** with `DoNotQueue` a refusal (`server.rs:238`) is final: if another daemon (mako, dunst) owns the name the shell never becomes the server, even after it exits, and nothing says so beyond a warning; ids must stay disjoint across the server counter, `Store.next_id` and the restart skip of 1000 (`shell.rs:1054`); `replaces_id` from any sender is trusted (`server.rs:57`)
- **Reify:** Medium — ownership phases implicit in signals and a retiring flag elsewhere
- **Related:** restart handover `shell.rs:1580`-`1589` and `desktop/aizome/crates/aizome-shell/src/carry.rs`

<a id="shb-17"></a>

### SHB-17 — Notification lifecycle in the store
- **Where:** `desktop/aizome/crates/aizome-shell/src/notifications/store.rs:182` (`Store.list`, `.next_id`); `Notification.read`, `.transient`, `.resident`, `.urgency` (`store.rs:132`-`139`)
- **Encoding:** presence in `list`, `read` bool, urgency (critical → card), transient flag; `next_id` high-water mark
- **States:** `Posted(unread)` → `Read` | `Card(critical, unread)` → `Closed`; transient: `Posted` → closed after its line; replaced: back to unread
- **Transitions:** post or replace (`store.rs:255`-`261`); evict past 200 (`store.rs:263`-`270`); id high-water (`store.rs:271`); close (`store.rs:280`-`284`); transient after line (`store.rs:287`-`292`); read by app (`store.rs:320`-`326`) or card action (`cards.rs:153`-`155`); restore and merge after restart (`store.rs:205`-`223`)
- **Hazards:** eviction falls back to index 0, the oldest unread (possibly a critical card), without a `NotificationClosed` and leaving its shell actions in `note_actions`; the `None => break` arm is unreachable; a replacement resets `read`
- **Reify:** Medium — several flags, handlers in three files

<a id="shb-18"></a>

### SHB-18 — PAM transaction
- **Where:** `desktop/aizome/crates/aizome-shell/src/pam.rs:146` (`Pam::authenticate`); `Conversation.said` (`pam.rs:81`)
- **Encoding:** a PAM handle for the call's duration; the conversation's collected messages
- **States:** `Started` → `Authenticated` → `AccountOk` → `CredsRefreshed` → `Ended(ok|err)`
- **Transitions:** start (`pam.rs:158`-`161`); authenticate (`pam.rs:162`); account (`pam.rs:163`-`165`); refresh creds, failure ignored (`pam.rs:166`-`170`); end (`pam.rs:181`); password wiped (`pam.rs:183`-`184`); conversation answers echo-off with the password, echo-on with the user name, collects error/info (`pam.rs:112`-`121`)
- **Hazards:** any echo-on prompt (an OTP module) is answered with the user name (`pam.rs:114`); a password-change requirement is just a failure; the caller's `String` copy of the password isn't wiped
- **Reify:** Low — linear within one call
- **Related:** NixOS PAM service `aizome`; outcome handled in SHB-12

<a id="shb-19"></a>

### SHB-19 — Power key: logind inhibitor and HandlePowerKey
- **Where:** `desktop/aizome/crates/aizome-shell/src/power.rs:37` (`Shell.power_inhibit: Option<Task<Inhibition>>`, `Shell.logind_power_key` at `shell.rs:356`-`358`)
- **Encoding:** presence of an async inhibit task; logind's `HandlePowerKey` read once
- **States:** `NotHeld` → `Held` (task pending or done) → `NotHeld`; `HandlePowerKey` `Unknown` → `Known(value)`
- **Transitions:** reconcile with the setting (`power.rs:38`-`49`); read on a thread (`power.rs:54`), stored (`shell.rs:1620`); key dispatch (`power.rs:26`-`32`)
- **Hazards:** a failed `Inhibit` call leaves `Some(failed task)`, counted as held and never retried, so logind and the shell both act on the key; `HandlePowerKey` is read once at start (`shell.rs:1166`), and until it arrives the key does nothing when the setting is unset
- **Reify:** Low — two fields, one reconcile

<a id="shb-20"></a>

### SHB-20 — Screen recording session (partially explicit)
- **Where:** `desktop/aizome/crates/aizome-shell/src/recording/mod.rs:332` (`Live.phase`, `.path`, `.recorder`, `.since`, `.handle`, `.serial`, `.tick`); `enum Phase` (`recording/mod.rs:320`); `Shell.recording`, `Shell.record_gen` (`shell.rs:367`-`368`); transitions in `desktop/aizome/crates/aizome-shell/src/recording_glue.rs`
- **Encoding:** partially explicit: `Phase` plus `since: Option<Instant>` (recorder running), `handle` (thread), serial filtering thread messages, tick generation for the indicator timer
- **States:** `Idle` → `Picking` → `Starting` → `Recording` ⇄ (`GaveUp`: `since` None) → `Stopping` → `Idle`; adopted recordings enter `Recording` directly
- **Transitions:** one at a time (`recording_glue.rs:22`-`25`); pick (`recording_glue.rs:28`-`39`); picked → start or nothing (`recording_glue.rs:64`-`67`); start (`recording_glue.rs:84`-`125`), test mode fakes Started (`recording_glue.rs:126`-`130`); stop (`recording_glue.rs:137`-`148`); thread events filtered by serial (`recording_glue.rs:164`): Started (`recording_glue.rs:168`-`173`), GaveUp (`recording_glue.rs:178`-`180`), Ended (`recording_glue.rs:183`-`184`); tick (`recording_glue.rs:193`-`201`); adopt (`recording_glue.rs:269`-`290`)
- **Hazards:** `Picking` has no timeout: a compositor restart or dropped IPC before `AreaPicked` leaves it stuck, `record` refuses, the Capture menu disables Record and hides Stop (`recording_glue.rs:311`, `332`); only ⌥Print or the palette's Stop Recording clear it; after `GaveUp` the badge reads 0:00 in `Recording`
- **Reify:** High — 7 fields, many handlers, suspected bug
- **Suspected bug:** `adopt_recording` never arms the indicator tick (only `Started` does, `recording_glue.rs:173`), so after a shell restart mid-recording the systray clock only advances when something else repaints the bar, about once a minute (confidence: high)
- **Related:** picker in `desktop/aizome/crates/aizome-comp-logic/src/overlays/area.rs:63`; thread side SHB-22/SHB-23

<a id="shb-21"></a>

### SHB-21 — Recorder capability probe
- **Where:** `desktop/aizome/crates/aizome-shell/src/recording/probe.rs:47` (`Inner.started`, `.found`)
- **Encoding:** `started` bool + `found: Option<Found>` behind a mutex; strikes mutate `found`
- **States:** `Unprobed` → `Probing` (`started`, no `found`) → `Found` → `Found` with strikes
- **Transitions:** start once (`probe.rs:57`-`60`); thread stores result (`probe.rs:67`); spawn failure stores empty result (`probe.rs:71`); strike (`probe.rs:85`-`88`); each probe command has a 30 s deadline with kill (`probe.rs:133`-`143`)
- **Hazards:** recordings wait on `found()` polled every 50 ms (`run.rs:136`-`143`); a panicking probe thread or poisoned lock (`unwrap`s at `probe.rs:56`, `67`, `77`, `83`) leaves `found` None forever and every recording waits in `Starting` until stopped; strikes are permanent for the session
- **Reify:** Low — small, single owner

<a id="shb-22"></a>

### SHB-22 — Recorder supervisor thread (fallback, settle, stop escalation)
- **Where:** `desktop/aizome/crates/aizome-shell/src/recording/run.rs:154` (`supervise`); `run.rs:123` (`start`); `Proc` (`run.rs:287`)
- **Encoding:** control flow over a `Cmd` channel (`Stop`, `Exited`), the attempt list, `last_error`, the output file's size, a pidfd or `waitid(WNOWAIT)` watcher thread per attempt
- **States:** `AwaitingProbe` → per attempt `Spawned` → `Settling` → `Recording` → `Stopping(SIGINT → SIGTERM → SIGKILL)` → `Ended(Saved|Incomplete|Failed|Crashed|Cancelled)`; attempt `GaveUp` → next attempt
- **Transitions:** wait for probe or cancel (`run.rs:136`-`142`); pending stop (`run.rs:167`-`168`); clear file and spawn (`run.rs:170`-`177`); watch and note (`run.rs:179`-`182`); Started (`run.rs:183`); settle exit → GaveUp (`run.rs:185`-`190`), settle stop (`run.rs:192`); recording stop (`run.rs:197`); exit without frames → GaveUp (`run.rs:204`-`208`), else Crashed (`run.rs:211`-`214`); escalation (`run.rs:230`-`239`); exit read (`run.rs:255`-`265`); watcher (`run.rs:300`-`316`)
- **Hazards:** each attempt deletes the file (`run.rs:170`), so a recorder that crashes after writing under 16 KiB is silently replaced by the next one; `expect("a thread")` (`run.rs:149`, `318`) panics the caller if a thread can't spawn; after SIGKILL a timed-out wait falls through to a blocking `child.wait()` (`run.rs:238`)
- **Reify:** Medium — explicit in control flow but phases live in nested loops and timeouts
- **Related:** gpu-screen-recorder / wf-recorder processes; shell side SHB-20

<a id="shb-23"></a>

### SHB-23 — Recording handover note and adoption across shell restarts
- **Where:** `desktop/aizome/crates/aizome-shell/src/recording/run.rs:99` (`Note`, file `$XDG_RUNTIME_DIR/aizome/recording.json`, `run.rs:111`)
- **Encoding:** a JSON marker file (pid, start ticks, recorder, path, since) written per attempt and removed when the thread ends; a pidfd taken by the adopting shell
- **States:** `NoNote` → `Noted(attempt pid)` → (shell restarts) `Adoptable` → `Adopted` → `Stopping` → `NoNote`; stale note → removed
- **Transitions:** write after spawn (`run.rs:180`-`182`, `373`-`387`); remove at thread end (`run.rs:144`-`146`, `441`); validate by start ticks, drop stale (`run.rs:395`-`398`); adopt opens pidfd, rechecks (`run.rs:412`-`416`), stop escalation (`run.rs:421`-`432`), unexpected exit (`run.rs:433`-`436`); new shell adopts at start (`shell.rs:1164`)
- **Hazards:** in a restart both shells hold the recorder until the old one exits: the old thread still owns the `Child`, so a stop from the new shell can make the old thread report Crashed or, under 16 KiB, start the next recorder (`run.rs:204`-`209`) and rewrite the note; the note is written non-atomically (`run.rs:386`), and an unparsable note is neither adopted nor removed (`run.rs:394`), so its recorder runs on in its own process group with no Stop; `since` is wall-clock (`run.rs:380`) mapped to an `Instant` (`recording_glue.rs:271`-`272`)
- **Reify:** Medium — persisted cross-process phase with two owners during restarts
- **Related:** shell restart handover `shell.rs:1048`-`1067`

<a id="shb-24"></a>

### SHB-24 — (test harness) Pretend recorders for the supervisor
- **Where:** `desktop/aizome/crates/aizome-shell/src/recording/run.rs:479` (`recorder`, `failing`, and inline `dies`, `slow`, `stubborn` scripts)
- **Encoding:** shell scripts trapping INT/TERM, writing to the output file, exiting with chosen codes; `FAST` timing (`run.rs:491`)
- **States:** `Running` → `Trapped` → `Exited(0)`; `failing` exits at once; `dies` writes 20 KB then exits 9; `slow` writes a header then exits; `stubborn` ignores signals
- **Transitions:** trap writes "done" and exits (`run.rs:481`); fixed sleeps to cross the settle window (`run.rs:520`, `582`, `605`, `640`, `664`)
- **Hazards:** timing-dependent: the sleeps assume the 300 ms settle has passed on a loaded machine
- **Reify:** Low — test fixture

<a id="shb-25"></a>

### SHB-25 — Services selection wait before the pointer menu
- **Where:** `desktop/aizome/crates/aizome-shell/src/services.rs:48` (`Shell.services_wait: Option<(serial, x, y)>`, `Shell.selection`, `shell.rs:271`, `304`)
- **Encoding:** pending serial from the shared `Shell.serial`; first of app reply, `wl-paste` thread or 150 ms timer wins
- **States:** `Idle` → `Waiting(serial)` → `MenuShown(selection)`
- **Transitions:** start wait, ask the app or read the primary selection, arm timeout (`services.rs:46`-`55`); first matching answer shows the menu (`services.rs:61`-`64`), later ones ignored (`services.rs:66`); app reply routed (`shell.rs:1297`-`1299`)
- **Hazards:** `wl-paste` has no timeout (`services.rs:16`-`23`), so a hung selection owner leaves one blocked thread per menu; `selection` outlives the menu and is read by later service actions (`services.rs:72`); `selection-N.txt` files in the runtime dir are never removed (`services.rs:104`)
- **Reify:** Low — clean first-wins wait
- **Related:** app side `desktop/aizome/crates/aizome-kit/src/shell.rs:292`-`295`

<a id="shb-26"></a>

### SHB-26 — Screen-share indicator settle (vestigial)
- **Where:** `desktop/aizome/crates/aizome-shell/src/sharing.rs:19` (`Share.since`), `SETTLE` (`sharing.rs:27`)
- **Encoding:** per-share first-seen `Instant` kept across updates; a settle delay now zero
- **States:** `Unknown` → `New(since)` → `Settled`; with `SETTLE = 0` every share is settled at once
- **Transitions:** update keeps `since` by id (`sharing.rs:31`-`36`); settled filter (`sharing.rs:41`); compositor report (`sharing.rs:96`)
- **Hazards:** none observed; the debounce now lives in the compositor
- **Reify:** Low — vestigial; could drop `since`
- **Related:** compositor's `shares-changed` debounce in aizome-comp

<a id="shb-27"></a>

### SHB-27 — Systray panel popup and Wi-Fi password ask
- **Where:** `desktop/aizome/crates/aizome-shell/src/systray/glue.rs:18` (`OpenPanel`); `glue.rs:28` (`PanelUi.asking`, `.error`); `Shell.panel`, `Shell.panel_ui` (`shell.rs:272`-`273`)
- **Encoding:** `Option<OpenPanel>` (popup window, widget, size), `PanelUi.asking: Option<(ssid, typed)>`, `error: Option<String>`
- **States:** `Closed` → `Open(widget)` → `Open(Network, Asking(ssid))` → `Joining` (asking taken) → `Open`; `Closed` on toggle, Escape, settings row, compositor close, Keep Awake off
- **Transitions:** toggle closes then reopens unless same widget (`glue.rs:193`-`212`), UI reset (`glue.rs:198`); clock opens agenda refresh (`glue.rs:199`-`201`, minute refresh `shell.rs:1469`-`1471`); close (`glue.rs:225`-`228`); size follows content (`glue.rs:235`-`239`); ask password (`glue.rs:286`-`289`); type (`glue.rs:298`-`300`); join (`glue.rs:303`-`308`); compositor close (`shell.rs:1707`-`1709`); Keep Awake off closes (`awake.rs:72`-`74`)
- **Hazards:** `PanelUi.error` is never set to `Some` anywhere, so a failed join is never shown; `asking` survives the network vanishing from the scan, and the panel height still counts its row (`panels.rs:831`-`833`); Join drops the typed password silently when the SSID is no longer in the snapshot (`glue.rs:305`-`308`); agenda results carry no token
- **Reify:** Medium — dead error state and several close paths across files
- **Related:** NetworkManager client `desktop/aizome/crates/aizome-sys/src/network`

<a id="shb-28"></a>

### SHB-28 — Status item click and menu fetch
- **Where:** `desktop/aizome/crates/aizome-shell/src/systray/items.rs:165` (`item_click`); `Shell.item_wheel` (`shell.rs:338`), `Shell.item_icons`
- **Encoding:** async `Task`s turned into `Msg::ItemMenuWanted` / `Msg::ItemMenu`; per-slot wheel remainder; icon cache keyed by signature
- **States:** `Idle` → `Activating` → (error and has menu) `FetchingMenu` → `MenuShown`; wheel `Remainder(v)`
- **Transitions:** activate, fall back to menu on error (`items.rs:176`-`188`); pressed-to-close (`items.rs:191`); context menu for items without one (`items.rs:204`-`207`); fetch (`items.rs:209`-`210`); show (`items.rs:222`-`231`); wheel steps (`items.rs:155`, `68`-`76`); cache refresh (`items.rs:86`-`94`)
- **Hazards:** menu fetches carry no request token: a slow dbusmenu reply pops its menu whenever it arrives, after the user moved on, opened another item or locked the screen; `item_wheel` entries for gone items are never pruned
- **Reify:** Low — short async hop, few fields
- **Related:** `desktop/aizome/crates/aizome-sys/src/statusnotifier` (watcher, dbusmenu)

<a id="shb-29"></a>

### SHB-29 — (test harness) Lock screen PAM settle loop
- **Where:** `desktop/aizome/crates/aizome-shell/tests/lock.rs:38` (`settle`); `tests/lock.rs:16` (`locked`)
- **Encoding:** poll `lock.busy` every 10 ms, up to 50 times, pumping `Msg::Nothing`
- **States:** `Checking` → `Settled` | `GaveUp(500 ms)`
- **Transitions:** lock and confirm (`tests/lock.rs:27`-`30`); poll (`tests/lock.rs:39`-`45`)
- **Hazards:** a timeout returns silently, so a slow PAM thread fails later assertions far from the cause
- **Reify:** Low — test fixture

<a id="shb-30"></a>

### SHB-30 — (test harness) Private D-Bus daemon and fake MPRIS player
- **Where:** `desktop/aizome/crates/aizome-shell/tests/media.rs:21` (`PrivateBus`); `tests/media.rs:89` (`Player.playing`, `.track`)
- **Encoding:** a spawned `dbus-daemon` whose address is read from stdout and which is killed on `Drop`; a fake player toggling `playing` and cycling `track`; `eventually` with a 5 s deadline
- **States:** bus `Starting` → `Listening(address)` → `Killed`; player `Playing`/`Paused` × track
- **Transitions:** spawn and read address (`tests/media.rs:52`-`67`); drop kills and cleans (`tests/media.rs:72`-`76`); play/pause, next, previous (`tests/media.rs:96`-`110`); wait (`tests/media.rs:135`-`141`)
- **Hazards:** `read_line` blocks forever if the daemon never prints an address; skipped silently without `dbus-daemon`
- **Reify:** Low — test fixture

## Already explicit

- None in this slice.

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/crates/aizome-shell/examples/deskbar_preview.rs` | 239 | none |  | example program; drag phases come from widgets/drawn, Moving fixed at No |
| `desktop/aizome/crates/aizome-shell/src/actions.rs` | 317 | none |  | action vocabulary enums and URL test |
| `desktop/aizome/crates/aizome-shell/src/agenda.rs` | 261 | none |  | Koyomi/Tokei readers and row formatting; refresh timing in SHB-27 |
| `desktop/aizome/crates/aizome-shell/src/apps/catalog.rs` | 203 | none |  | pure category and glyph tables |
| `desktop/aizome/crates/aizome-shell/src/apps/mod.rs` | 39 | none |  | id comparison and test index helper |
| `desktop/aizome/crates/aizome-shell/src/apps/registry.rs` | 155 | machines | [SHB-01](#shb-01) | connection registry |
| `desktop/aizome/crates/aizome-shell/src/apps/server.rs` | 180 | machines | [SHB-02](#shb-02) | socket ownership and connection threads |
| `desktop/aizome/crates/aizome-shell/src/auth.rs` | 336 | machines | [SHB-03](#shb-03) | polkit dialog; suspected retry loop |
| `desktop/aizome/crates/aizome-shell/src/awake.rs` | 129 | machines | [SHB-04](#shb-04) | Keep Awake status and watcher |
| `desktop/aizome/crates/aizome-shell/src/bell.rs` | 70 | machines | [SHB-05](#shb-05) | flash overlay (shared with OSD entry) |
| `desktop/aizome/crates/aizome-shell/src/clock.rs` | 192 | none |  | pure local-time and calendar math |
| `desktop/aizome/crates/aizome-shell/src/confirm.rs` | 207 | machines | [SHB-06](#shb-06) | countdown; suspected invisible shutdown |
| `desktop/aizome/crates/aizome-shell/src/greeter.rs` | 26 | machines | [SHB-07](#shb-07) | coalescing publish worker |
| `desktop/aizome/crates/aizome-shell/src/gtk.rs` | 96 | machines | [SHB-08](#shb-08) | unordered dconf writer threads |
| `desktop/aizome/crates/aizome-shell/src/icons.rs` | 69 | none |  | pure icon rasterising |
| `desktop/aizome/crates/aizome-shell/src/ipc.rs` | 169 | machines | [SHB-09](#shb-09), [SHB-10](#shb-10) | event follower and request sender |
| `desktop/aizome/crates/aizome-shell/src/launch.rs` | 235 | none |  | path and argv helpers; trash writes info then renames per spec |
| `desktop/aizome/crates/aizome-shell/src/lib.rs` | 112 | none |  | module list and msg CLI |
| `desktop/aizome/crates/aizome-shell/src/location.rs` | 114 | machines | [SHB-11](#shb-11) | GeoClue locate with 6 h re-ask |
| `desktop/aizome/crates/aizome-shell/src/lock.rs` | 494 | machines | [SHB-12](#shb-12), [SHB-13](#shb-13) | lock lifecycle and shake |
| `desktop/aizome/crates/aizome-shell/src/look.rs` | 3 | none |  | re-export |
| `desktop/aizome/crates/aizome-shell/src/main.rs` | 24 | none |  | argument dispatch |
| `desktop/aizome/crates/aizome-shell/src/notifications/cards.rs` | 160 | machines | [SHB-14](#shb-14) | cards surface; suspected lost after output change |
| `desktop/aizome/crates/aizome-shell/src/notifications/glue.rs` | 488 | machines | [SHB-15](#shb-15) | banner line; suspected early end on replace |
| `desktop/aizome/crates/aizome-shell/src/notifications/mod.rs` | 6 | none |  | module list |
| `desktop/aizome/crates/aizome-shell/src/notifications/server.rs` | 302 | machines | [SHB-16](#shb-16) | bus name and ids |
| `desktop/aizome/crates/aizome-shell/src/notifications/store.rs` | 474 | machines | [SHB-17](#shb-17) | notification lifecycle and eviction |
| `desktop/aizome/crates/aizome-shell/src/osd.rs` | 247 | machines | [SHB-05](#shb-05) | OSD overlay, entry under bell.rs |
| `desktop/aizome/crates/aizome-shell/src/pam.rs` | 240 | machines | [SHB-18](#shb-18) | PAM transaction |
| `desktop/aizome/crates/aizome-shell/src/power.rs` | 70 | machines | [SHB-19](#shb-19) | power-key inhibitor reconcile |
| `desktop/aizome/crates/aizome-shell/src/recording/mod.rs` | 495 | machines | [SHB-20](#shb-20) | recording Live and Phase; plans are pure |
| `desktop/aizome/crates/aizome-shell/src/recording/probe.rs` | 226 | machines | [SHB-21](#shb-21) | probe started/found/strike |
| `desktop/aizome/crates/aizome-shell/src/recording/run.rs` | 678 | machines | [SHB-22](#shb-22), [SHB-23](#shb-23), [SHB-24](#shb-24) | supervisor, handover note, test recorders |
| `desktop/aizome/crates/aizome-shell/src/recording_glue.rs` | 381 | machines | [SHB-20](#shb-20), [SHB-23](#shb-23) | transitions of SHB-20; adopt side of SHB-23 |
| `desktop/aizome/crates/aizome-shell/src/services.rs` | 125 | machines | [SHB-25](#shb-25) | selection wait before pointer menu |
| `desktop/aizome/crates/aizome-shell/src/sharing.rs` | 182 | machines | [SHB-26](#shb-26) | vestigial settle debounce |
| `desktop/aizome/crates/aizome-shell/src/sound.rs` | 183 | none |  | fire-and-forget player threads; feedback timers not coalesced |
| `desktop/aizome/crates/aizome-shell/src/sys.rs` | 170 | none |  | service handles and snapshot reads |
| `desktop/aizome/crates/aizome-shell/src/systray/glue.rs` | 423 | machines | [SHB-27](#shb-27) | panel popup and Wi-Fi ask |
| `desktop/aizome/crates/aizome-shell/src/systray/items.rs` | 353 | machines | [SHB-28](#shb-28) | status item menu fetch and wheel remainder |
| `desktop/aizome/crates/aizome-shell/src/systray/meters.rs` | 116 | none |  | sampler primed once; pure bar math |
| `desktop/aizome/crates/aizome-shell/src/systray/mod.rs` | 7 | none |  | module list |
| `desktop/aizome/crates/aizome-shell/src/systray/panels.rs` | 887 | none |  | pure panel views and sizes (asking row height noted in SHB-27) |
| `desktop/aizome/crates/aizome-shell/src/wallpaper.rs` | 3 | none |  | re-export |
| `desktop/aizome/crates/aizome-shell/src/widgets/mod.rs` | 4 | none |  | re-export |
| `desktop/aizome/crates/aizome-shell/tests/auth.rs` | 71 | none |  | tests |
| `desktop/aizome/crates/aizome-shell/tests/common/mod.rs` | 299 | none |  | tests; sequential scene fixture (Connected, Hello, Menus) |
| `desktop/aizome/crates/aizome-shell/tests/deskbar.rs` | 989 | none |  | tests |
| `desktop/aizome/crates/aizome-shell/tests/deskbar_designs.rs` | 199 | none |  | tests; ignored picture generators |
| `desktop/aizome/crates/aizome-shell/tests/desktop.rs` | 179 | none |  | tests |
| `desktop/aizome/crates/aizome-shell/tests/lock.rs` | 200 | machines | [SHB-29](#shb-29) | PAM settle poll loop |
| `desktop/aizome/crates/aizome-shell/tests/media.rs` | 170 | machines | [SHB-30](#shb-30) | private dbus-daemon and fake MPRIS |
| `desktop/aizome/crates/aizome-shell/tests/menubar.rs` | 109 | none |  | tests |
| `desktop/aizome/crates/aizome-shell/tests/menus.rs` | 330 | none |  | tests; waits out the SHB-25 timer |
| `desktop/aizome/crates/aizome-shell/tests/palette.rs` | 173 | none |  | tests |
| `desktop/aizome/crates/aizome-shell/tests/panels.rs` | 312 | none |  | tests |
| `desktop/aizome/crates/aizome-shell/tests/perf.rs` | 176 | none |  | tests; timing loops |
| `desktop/aizome/crates/aizome-shell/tests/recording.rs` | 164 | none |  | tests; drives SHB-20 test mode |
| `desktop/aizome/crates/aizome-shell/tests/session.rs` | 180 | none |  | tests |
| `desktop/aizome/crates/aizome-shell/tests/settings.rs` | 318 | none |  | tests |
| `desktop/aizome/crates/aizome-shell/tests/sharing.rs` | 79 | none |  | tests; settle sleep for zero SETTLE |
