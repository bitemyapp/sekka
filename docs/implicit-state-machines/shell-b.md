# SHB: Shell: lock, notifications, recording, systray and services

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/crates/aizome-shell`. Audited at `8e0acbe5`; line numbers refer to that commit.

61 files audited. 30 implicit machines: 7 High, 7 Medium, 16 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#shb-03)
- **Follow-up:** reified as typestate in [#298](https://github.com/bitemyapp/sekka/pull/298). Its suspected bug is fixed, and the mistake no longer compiles, apart from a person's actions called from an event handler, which the agent's backoff bounds (see `docs/typestate-tier2.md`).

<a id="shb-06"></a>

### SHB-06 — Session confirmation countdown (Restart, Shut Down, Log Out, power key)
- **Where:** `desktop/aizome/crates/aizome-shell/src/confirm.rs:36` (`Confirm.window`, `.verb`, `.left`, `.generation`, `.power`); `Shell.confirm` (`shell.rs:306`)
- **Encoding:** `Option<Confirm>`; seconds left; generation from `Shell.serial`; self-re-arming 1 s `Tick(generation)` timers
- **States:** `Idle` → `Counting(verb, left, power)` → `Done(verb)` | `Cancelled`; re-ask with the same `power` resets `left` in place
- **Transitions:** ask with same `power` changes verb and resets left (`confirm.rs:110`-`115`); otherwise close and open a new overlay (`confirm.rs:117`-`127`), first tick (`confirm.rs:128`); Cancel (`confirm.rs:135`); Now (`confirm.rs:137`-`138`); Do (`confirm.rs:141`-`142`); Tick decrements and re-arms (`confirm.rs:149`-`152`) or fires at 1 (`confirm.rs:145`-`147`); stale tick ignored (`confirm.rs:155`); verb carried out (`confirm.rs:173`-`191`), or recorded off-live (`confirm.rs:168`-`171`)
- **Hazards:** with no focused screen an ask closes the existing dialog and shows nothing (`confirm.rs:117`-`118`); the dialog's close by the compositor isn't tied to `confirm`
- **Reify:** High — user-visible power-off/logout consequences, suspected bug
- **Suspected bug:** the dialog is a layer on the focused output (`confirm.rs:118`-`122`); if that output goes (dock unplugged, monitor off) during the minute, `close_requested` removes only the `surfaces` entry (`shell.rs:1712`), `confirm` stays `Some`, ticks continue and the machine shuts down/restarts/logs out with no dialog to cancel; asking again only resets `left` (`confirm.rs:110`-`115`) and never reopens a window (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#shb-06)

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
- **Reproduction:** Confirmed (end-to-end): [summary](reproductions.md#shb-09)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#shb-12)
- **Follow-up:** reified as typestate in [#298](https://github.com/bitemyapp/sekka/pull/298). Its suspected bug is fixed, and the mistake no longer compiles.

<a id="shb-14"></a>

### SHB-14 — Critical notification cards surface
- **Where:** `desktop/aizome/crates/aizome-shell/src/notifications/cards.rs:105` (`Shell.cards: Option<(WindowKey, usize, f64)>`, `shell.rs:311`)
- **Encoding:** window key, card count shown and right margin, reconciled on every update against `notes.cards()` and the lock
- **States:** `None` → `Shown(n, right)` → resized/moved → `None` (no cards, or locked)
- **Transitions:** none while locked (`cards.rs:106`); close at 0 (`cards.rs:115`-`118`); open on the primary output (`cards.rs:121`-`131`); resize and re-margin (`cards.rs:134`-`141`); actions mark read or dismiss (`cards.rs:146`-`157`)
- **Hazards:** the surface stays on the output it opened on when the primary changes
- **Reify:** High — suspected bug, user-visible loss of critical notifications
- **Suspected bug:** when the cards' output goes, the compositor closes the layer and `close_requested` drops only the `surfaces` entry (`shell.rs:1712`); `cards` keeps the dead key, so `sync_cards` keeps resizing it (`cards.rs:135`) and never reopens, and critical cards stay invisible until every one is read elsewhere (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#shb-14)

<a id="shb-15"></a>

### SHB-15 — Bar notification line (banner)
- **Where:** `desktop/aizome/crates/aizome-shell/src/notifications/glue.rs:184` (`Shell.banner: Option<u32>`, `Shell.banner_queue` at `shell.rs:258`-`259`)
- **Encoding:** id of the notification in the line; `BannerDone(id)` timers keyed by notification id only; a `banner_queue` that is never filled
- **States:** `Menus` → `Banner(id)` → `Banner(id')` (newer takes the line, old expires) → `Menus`
- **Transitions:** post shows (`glue.rs:85`-`87`); show expires the replaced one and arms a timer (`glue.rs:185`-`193`); timer ends if still current (`glue.rs:198`-`205`); app close ends it (`glue.rs:96`-`99`)
- **Hazards:** `banner_queue` is dead state (only `retain`ed, `glue.rs:96`); `banner_done` closes transient ones as Expired (`glue.rs:203`-`205`)
- **Reify:** High — suspected bug, timer identity is implicit
- **Suspected bug:** a replacement of the notification on the line (same id through `replaces_id`) arms a second `BannerDone(id)` while the first is still pending; the first ends the line at the original deadline (`glue.rs:198`-`201`) and, for a transient one, closes the just-updated notification. A progress notification updated every 2 s gets its line cut short and flickering (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#shb-15)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#shb-20)
