# SVC: Applications index, media, IPC, portal and greeter

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/crates/aizome-{apps,media,ipc,portal,greeter}`. Audited at `8e0acbe5`; line numbers refer to that commit.

52 files audited. 41 implicit machines: 4 High, 10 Medium, 27 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

<a id="svc-05"></a>

### SVC-05 — Greeter screen stage (people, password, typed name)
- **Where:** `desktop/aizome/crates/aizome-greeter/src/app.rs:108` (`Greeter.stage`, `.selected`, `.name`, `.password`, `.session_of`); also `desktop/aizome/crates/aizome-greeter/src/view.rs:40-45`, `desktop/aizome/crates/aizome-greeter/src/config.rs:91` (`State.last_user`, `.sessions`)
- **Encoding:** the `Stage` enum (partially explicit) together with `login.user`/`login.step` (SVC-12), which split `Name` into "typing" and "typed" sub-states in the view; the persisted `state.toml` picks the initial stage
- **States:** `People` | `Password` | `Name{typing}` | `Name{typed}` → `Done` (quit); invalid but reachable: `Name` with `login.step == Failed`, from which nothing can begin again
- **Transitions:** initial stage from `last_user` and the user count (`app.rs:157-164`); `pick` →Password and begin (`app.rs:292-301`); `back_to_people` →People and cancel, only with two or more listed users (`app.rs:303-314`); `SubmitName` Name{typing}→Name{typed} (`app.rs:537-544`); `Msg::Other` →Name (`app.rs:559-565`, which the view never emits); `submit` on Failed → `begin` (`app.rs:259`), which needs `user()` and so does nothing in Name (`app.rs:194-199`, `app.rs:242-248`); Done → `state.logged_in`, save, quit (`app.rs:276-289`, `config.rs:104-115`); the view picks the sub-state (`view.rs:43-44`)
- **Hazards:** the typed-name password view offers no Escape and no "Log in as someone else" (`view.rs:279`, `view.rs:297`), and nothing resets `login.user` in the Name stage; `state.toml` is written without fsync (`config.rs:106-108`)
- **Reify:** High — a user-visible dead end; the stage's meaning is split between `Stage`, `Login` and the view.
- **Suspected bug:** with `user_list = false` (or nobody listed) a mistyped user name can't be corrected. PAM's auth error begins the same name again every time (`desktop/aizome/crates/aizome-greeter/src/login.rs:182-183`). If `CreateSession` itself fails, the step becomes `Failed` and Return does nothing (`app.rs:259` → `begin` sees `user() == None`). Only restarting the greeter helps. (confidence: high)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#svc-05)

<a id="svc-12"></a>

### SVC-12 — greetd login conversation
- **Where:** `desktop/aizome/crates/aizome-greeter/src/login.rs:48` (`Login.user`, `.step`, `.message`, `.failed`, `.asked`, `.generation`, `.open`)
- **Encoding:** the `Step` enum (partially explicit), plus a FIFO of `(Asked, generation)` for requests awaiting an answer, a generation counter that drops answers meant for an earlier login, and `open` (greetd holds a session)
- **States:** `Nobody` → `Waiting` → `Ask{secret, prompt}` → `Waiting` → `Starting` → `Done`; `Failed` (create failed); `open`, `generation` and queue depth vary independently; reachable odd combinations: `open && step == Failed`, `step == Waiting` after `lost` with nothing queued
- **Transitions:** `begin` (cancels first if open, bumps the generation, sends Create) (`login.rs:86-97`); `cancel` (`login.rs:100-111`); `answer` Ask→Waiting (`login.rs:114-121`); `heard`: drops Cancel and stale answers (`login.rs:126-134`), prompt → Ask (`login.rs:139-143`), info or error → acknowledge (`login.rs:145-153`), Success after Start → Done (`login.rs:155-158`), Success → Starting and StartSession (`login.rs:160-165`), Error → message, `failed += 1` on an auth error, begin again unless Create failed → Failed (`login.rs:166-189`); `lost` (`login.rs:194-199`)
- **Hazards:** correctness depends on greetd answering each request exactly once and in order (the queue pops blindly, `login.rs:126`); `lost` clears the queue without bumping `generation`; no timeout in `Waiting` or `Starting`; "someone else" stays available during `Starting`, which cancels a session greetd has already scheduled; secrets also linger in copies of `Msg::Password` outside this machine
- **Reify:** High — the authentication conversation is spread over seven fields; it is well tested but implicit.
- **Related:** the greetd daemon (external, greetd-ipc(7)); wire types at `desktop/aizome/crates/aizome-greeter/src/greetd.rs:12-54`; the UI side is SVC-05

<a id="svc-23"></a>

### SVC-23 — Playback session lifecycle
- **Where:** `desktop/aizome/crates/aizome-media/src/player.rs:623` (`Session.eof`, `.started`, `.from`, `.generation`, `.ended`, `.stepped`, `.seeked_said`); `player.rs:644` (`Ctl.session`, `.want_play`, `.speed`, `.visible`, `.started_at`)
- **Encoding:** `Option<Session>`, six bools and counters, and `want_play`; `PlayState` exists only as an event
- **States:** `Closed` → `Opening` → `Prerolling` (`!seeked_said`) → `Ready/Paused` → `Playing` (`started && want_play`) → `Ended`; side states `Stepped` and `Seeking` (`!started` after `seek_to`); invalid but representable: `ended && started && want_play`, `stepped` while playing
- **Transitions:** Open (`player.rs:756-763`, `player.rs:828-925`); `play` (`player.rs:1070-1086`); `resume` (`player.rs:1089-1101`); `pause` (`player.rs:1103-1120`); `seek_to` resets all of it (`player.rs:1131-1187`); `step` (`player.rs:1189-1211`); start once ready (`player.rs:1344-1357`); end detection (`player.rs:1369-1391`); `close` (`player.rs:1053-1068`); Speed (`player.rs:773-785`)
- **Hazards:** `Ctl.visible` is written (`player.rs:793`) and never read; a non-seekable item that ended can't be replayed (`play` → `seek` does nothing, `player.rs:1073-1075`, `player.rs:1123-1125`); FFmpeg reads have no interrupt callback, so Close, Open and Quit wait behind a stalled network read, and `Player::drop` (`player.rs:413-420`) blocks the UI thread with them; a seek while the video is suspended never becomes ready (SVC-29)
- **Reify:** High — eleven fields, nine transition sites, user-visible bugs.
- **Suspected bug:** stepping forward and then pressing Play jumps back to where stepping started. Forward steps only show frames (`player.rs:1196-1200`, `desktop/aizome/crates/aizome-media/src/video.rs:489-494`) and never move the clock, and `play` "catches up" by seeking to `clock.now()` (`player.rs:1077-1082`) rather than to the frame on screen (`video.last_pts`). (confidence: medium)
- **Suspected bug:** changing speed on non-seekable media with audio drops the tempo filter (`player.rs:777-779`), but the seek that would rebase the ring is skipped (`player.rs:1123-1125`). The ring's segment keeps the old speed, so the clock runs at the old rate while the audio plays at the new tempo: the video drifts and the position is wrong. (confidence: low)
- **Related:** the UI in `desktop/aizome/apps/utsushi/src/app.rs` (MPRIS seeks at `desktop/aizome/apps/utsushi/src/app.rs:649`, visibility at `desktop/aizome/apps/utsushi/src/app.rs:1149`)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#svc-23)

<a id="svc-29"></a>

### SVC-29 — Video suspension out of sight, and catching up
- **Where:** `desktop/aizome/crates/aizome-media/src/video.rs:430` (`State.suspended`, `.by_window`); `video.rs:94` (`Shared.suspended`, `.window_hidden`, `.resync`); `video.rs:194` (`Sight.presented`, `.discarded_run`, `.last_presented`)
- **Encoding:** two copies of "suspended" (one local to the thread, one shared), the window's hint, discard-run counters, a resync request flag
- **States:** `Visible` → `Suspended{by window}` | `Suspended{by discards, probing every second}` → `Resyncing` → `Visible`; invalid but reachable: local `suspended == true` while the shared `suspended == false`
- **Transitions:** detect and `suspend` (`video.rs:768-776`, `video.rs:873-885`); wait while hidden (`video.rs:778-782`); probe (`video.rs:783-804`); back in sight → resync (`video.rs:805-814`); the demuxer honours the shared flag (`desktop/aizome/crates/aizome-media/src/player.rs:1268`, `player.rs:1274`, `player.rs:1310`); `resync_video` (`player.rs:1539-1562`); `set_visible` (`player.rs:361-365`); `seek_to` clears only the shared flag (`player.rs:1176`); `Cmd::Start` clears both (`video.rs:540-541`)
- **Hazards:** the two flags diverge on every seek; after that the demuxer queues video packets the thread won't consume; probing shows the last frame every second while out of sight
- **Reify:** High — duplicated state with a reachable invalid combination that stalls playback.
- **Suspected bug:** a seek while the window is hidden stalls playback. Utsushi reports xdg suspension through `set_visible(false)` (`desktop/aizome/apps/utsushi/src/app.rs:1149`), and MPRIS seeks arrive at `app.rs:649`. `seek_to` stops the audio and waits for the first frame of the flushed generation (`player.rs:1146`, `player.rs:1344`). The video thread is still suspended locally, so `check_sight` returns before presenting (`video.rs:778-782`), and the audio stays silent until the window is shown again. Speed and track changes seek too, so they stall the same way. (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#svc-29)
