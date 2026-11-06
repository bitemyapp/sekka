# SVC: Applications index, media, IPC, portal and greeter

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/crates/aizome-{apps,media,ipc,portal,greeter}`. Audited at `8e0acbe5`; line numbers refer to that commit.

52 files, 0 pending. 41 implicit machines: 4 High, 10 Medium, 27 Low.

## Machines

<a id="svc-01"></a>

### SVC-01 — Icon lookup cache layers
- **Where:** `desktop/aizome/crates/aizome-apps/src/icons.rs:262` (`Inner.themes`, `.chain`, `.loose`, `.results`, `.theme`)
- **Encoding:** four lazily filled caches under one `Mutex<Inner>`: per-theme loads (`HashMap<String, Option<Arc<Theme>>>`), the resolved inheritance chain (`Option<Vec<_>>`), loose base-directory listings (`Option<Vec<_>>`), memoised answers keyed by (names, size, scale)
- **States:** `Cold` → `Warm` (each layer filled on first use) → `ThemeChanged` (chain and results dropped, themes and loose kept) | `Invalidated` (all dropped) → `Warm`
- **Transitions:** `Inner::theme` fills `themes` (`icons.rs:279-280`); `Inner::chain` fills `chain` (`icons.rs:293`); `Inner::loose` fills `loose` (`icons.rs:312`); `lookup_best` memoises (`icons.rs:465`); `set_theme` Warm→ThemeChanged (`icons.rs:421-425`); `invalidate` →Invalidated (`icons.rs:432-435`)
- **Hazards:** answers stay stale until a caller invokes `invalidate` (nothing here watches the icon dirs; the test at `icons.rs:654-669` relies on it); `results` grows without bound with distinct name/size/scale keys; negative answers are cached too
- **Reify:** Low — one lock, two documented invalidation entry points.

<a id="svc-02"></a>

### SVC-02 — Application index freshness (signature versus scan)
- **Where:** `desktop/aizome/crates/aizome-apps/src/index.rs:91` (`AppIndex.signature`); also `index.rs:598` (`Signature.apps`, `.lists`), `desktop/aizome/crates/aizome-apps/src/mime.rs:453`, `mime.rs:492`
- **Encoding:** `signature: Signature` (stamps of the app directories and the MIME/terminal lists) beside the scanned `entries`/`info`/`by_id`/`by_mime`/`visible` and `mime: MimeData`; the watcher keeps its own copy as baseline (SVC-03)
- **States:** `Fresh` (signature matches disk) → `Stale` (disk changed) → `Rescanned` (app dirs changed: full scan and MIME) | `ListsReloaded` (only lists changed) → `Fresh`; `SelfWritten` after this index writes a list
- **Transitions:** `load_with` computes the signature, then scans (`index.rs:104`, `index.rs:117-118`); `refresh_if_changed` Stale→Rescanned (`index.rs:268-270`) or →ListsReloaded (`index.rs:272`, `index.rs:274`); `reload_lists` after its own writes (`index.rs:280-283`, called at `mime.rs:453` and `mime.rs:492`); `watch` hands out a clone as the watcher's baseline (`index.rs:291`)
- **Hazards:** the signature is computed before the scan, so a change landing in between is read now and reported again next refresh (double work only); `reload_lists` updates only this index's copy, so the watcher's `last` still differs and wakes the UI for the index's own write (spurious wake; `refresh_if_changed` then returns false); writes to `mimeapps.list` and `xdg-terminals.list` are unlocked read-modify-writes (`mime.rs:740-750`, `mime.rs:754-772`), so Settei and the compositor's index can lose each other's edits
- **Reify:** Medium — freshness is split between two signature copies and three update sites.
- **Related:** `desktop/aizome/crates/aizome-comp-core/src/terminal.rs:34-47` (refreshes before each ❖Return), `desktop/aizome/crates/aizome-shell/src/shell.rs:1511` (watch consumer)

<a id="svc-03"></a>

### SVC-03 — Index watcher thread lifecycle
- **Where:** `desktop/aizome/crates/aizome-apps/src/index.rs:635` (`Watcher.stop`, `.thread`); loop state `index.rs:682-683` (`last`, `next_poll`)
- **Encoding:** `Option<OwnedFd>` (stop-pipe write end) and `Option<JoinHandle>`; loop locals `last: Signature`, `next_poll: Instant`; an optional inotify fd
- **States:** `Inert` (no pipe or thread) | `Polling` → `Settling` (after an inotify event, 150 ms) → `Comparing` → `Polling`; → `Stopped` when the pipe closes
- **Transitions:** `Watcher::start` →Inert on pipe or spawn failure (`index.rs:643-646`, `index.rs:657-660`) or →Polling (`index.rs:656`); `wait` Event → drain and settle (`index.rs:688-699`); Timeout re-arms `next_poll` (`index.rs:700`); compare and `wake` (`index.rs:702-706`); poll error → Timeout after 10 ms (`index.rs:725-729`); `Drop` closes the pipe and joins (`index.rs:667-669`)
- **Hazards:** inotify watches only top-level writable directories (`index.rs:650`) and is never re-armed after `IN_DELETE_SELF`/`IN_MOVE_SELF`, leaving 2 s polling; `Inert` is silent apart from a log line, so the UI never refreshes; `Drop` blocks its caller for a signature walk in progress; a `Wake` the UI misses is not repeated, because `last` has already moved on
- **Reify:** Low — small, local, and Drop is correct.
- **Related:** `desktop/aizome/crates/aizome-shell/src/shell.rs:1511`

<a id="svc-04"></a>

### SVC-04 — Default terminal moved from foot to Hotaru (one-shot migration)
- **Where:** `desktop/aizome/crates/aizome-apps/src/mime.rs:590` (`AppIndex::move_terminal_to_hotaru`); persisted `$XDG_CONFIG_HOME/xdg-terminals.list` through `mime.rs:754`
- **Encoding:** the order of the person's `xdg-terminals.list` (persisted), the compositor's old `[keys] terminal` argv, the comp-core marker `$XDG_STATE_HOME/aizome/terminal-moved-to-hotaru`; outcome `TerminalMove` (`mime.rs:66-76`)
- **States:** `NotMoved` → `Unchanged(current)` | `Hotaru` | `Listed(id)` | `Unmatched(argv)` → `Moved` (the caller writes the marker); `Failed` → retried next session
- **Transitions:** chosen is none or `["foot"]`, foot is the default and Hotaru is installed → Hotaru, else Unchanged (`mime.rs:597-603`); chosen equals the current exec → Unchanged (`mime.rs:604-606`); a matching exec or program → Listed (`mime.rs:615-619`); otherwise Unmatched (`mime.rs:620`); writes go through `set_role_default` → `edit_terminal_list` → `write_atomic` (`mime.rs:490-492`, `mime.rs:754-772`, `mime.rs:794-813`)
- **Hazards:** idempotence exists only in the caller's marker; an `Err` (for example `ManagedBySystem` for a Home Manager link, `mime.rs:782-785`) writes no marker, so the move is retried and warns at every login; the list edit is an unlocked read-modify-write that races Settei
- **Reify:** Low — a one-shot decision whose outcome is already an enum.
- **Related:** `desktop/aizome/crates/aizome-comp-core/src/terminal.rs:70-102` (marker check and write)

<a id="svc-05"></a>

### SVC-05 — Greeter screen stage (people, password, typed name)
- **Where:** `desktop/aizome/crates/aizome-greeter/src/app.rs:108` (`Greeter.stage`, `.selected`, `.name`, `.password`, `.session_of`); also `desktop/aizome/crates/aizome-greeter/src/view.rs:40-45`, `desktop/aizome/crates/aizome-greeter/src/config.rs:91` (`State.last_user`, `.sessions`)
- **Encoding:** the `Stage` enum (partially explicit) together with `login.user`/`login.step` (SVC-12), which split `Name` into "typing" and "typed" sub-states in the view; the persisted `state.toml` picks the initial stage
- **States:** `People` | `Password` | `Name{typing}` | `Name{typed}` → `Done` (quit); invalid but reachable: `Name` with `login.step == Failed`, from which nothing can begin again
- **Transitions:** initial stage from `last_user` and the user count (`app.rs:157-164`); `pick` →Password and begin (`app.rs:292-301`); `back_to_people` →People and cancel, only with two or more listed users (`app.rs:303-314`); `SubmitName` Name{typing}→Name{typed} (`app.rs:537-544`); `Msg::Other` →Name (`app.rs:559-565`, which the view never emits); `submit` on Failed → `begin` (`app.rs:259`), which needs `user()` and so does nothing in Name (`app.rs:194-199`, `app.rs:242-248`); Done → `state.logged_in`, save, quit (`app.rs:276-289`, `config.rs:104-115`); the view picks the sub-state (`view.rs:43-44`)
- **Hazards:** the typed-name password view offers no Escape and no "Log in as someone else" (`view.rs:279`, `view.rs:297`), and nothing resets `login.user` in the Name stage; `state.toml` is written without fsync (`config.rs:106-108`)
- **Reify:** High — a user-visible dead end; the stage's meaning is split between `Stage`, `Login` and the view.
- **Suspected bug:** with `user_list = false` (or nobody listed) a mistyped user name can't be corrected. PAM's auth error begins the same name again every time (`desktop/aizome/crates/aizome-greeter/src/login.rs:182-183`). If `CreateSession` itself fails, the step becomes `Failed` and Return does nothing (`app.rs:259` → `begin` sees `user() == None`). Only restarting the greeter helps. (confidence: high)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#svc-05)
- **Follow-up:** reified as typestate in [#298](https://github.com/bitemyapp/sekka/pull/298). Its suspected bug is fixed, and the mistake no longer compiles in the model; a guard added in the view or the app's handlers still compiles, and tests catch it (see `docs/typestate-tier2.md`).

<a id="svc-06"></a>

### SVC-06 — greetd link thread
- **Where:** `desktop/aizome/crates/aizome-greeter/src/app.rs:130` (`Greeter.to_greetd`); thread body `app.rs:447-479`
- **Encoding:** `Option<mpsc::Sender<Request>>`; a named thread that owns the `Greetd` socket (or the Demo); `Msg::GreetdLost` as its exit signal
- **States:** `NotStarted` (tests: `None`, requests dropped) → `Connecting` → `Serving` → `Lost` (connect error, I/O error, or the UI gone)
- **Transitions:** `start_greetd` (`app.rs:440-481`); connect failure → GreetdLost (`app.rs:453-458`); each request asked, zeroed and forwarded (`app.rs:461-471`); I/O error → GreetdLost (`app.rs:472-475`); on the UI side a send failure → `login.lost` (`app.rs:231-233`); GreetdLost → `login.lost`, and quit when live and not demo (`app.rs:522-529`)
- **Hazards:** a failed thread spawn is discarded with `.ok()` (`app.rs:479`) but `to_greetd` is still set, and a panicking link never posts GreetdLost. Either way sends fail, the screen says "Lost the login service", and the greeter never quits so greetd never restarts it. `Greetd::ask` has no timeout (`desktop/aizome/crates/aizome-greeter/src/greetd.rs:119-122`)
- **Reify:** Low — small; making a send failure quit as well would close the gap.
- **Related:** the greetd daemon (greetd-ipc(7)), through `desktop/aizome/crates/aizome-greeter/src/greetd.rs`

<a id="svc-07"></a>

### SVC-07 — Greeter surfaces per display and the main display
- **Where:** `desktop/aizome/crates/aizome-greeter/src/app.rs:123` (`Greeter.windows`, `.frames`, `.main_output`)
- **Encoding:** `Vec<(WindowKey, u32)>`, `HashMap<u32, Size>`, `Option<u32>`
- **States:** per output `Absent` → `Open(main | other)`; overall `NoMain` | `Main(id)`; when the main output changes, every surface is reopened
- **Transitions:** `sync_windows` closes outputs that are gone (`app.rs:366-372`), picks the main again (`app.rs:373-375`), reopens everything when it changed (`app.rs:376-381`), opens missing surfaces with exclusive keyboard on the main one (`app.rs:382-401`); `outputs_changed` (`app.rs:507-512`)
- **Hazards:** reopening every surface when the main display changes drops focus mid-typing; wallpaper entries per output are never pruned (SVC-08)
- **Reify:** Low — one function owns it.

<a id="svc-08"></a>

### SVC-08 — Greeter wallpaper loads and the day/night tick
- **Where:** `desktop/aizome/crates/aizome-greeter/src/app.rs:128` (`Greeter.wallpapers`), `app.rs:120` (`.now`)
- **Encoding:** per output `(path, pixel size, Option<Image>)`, where `None` means loading; `Now` plus a `Msg::Tick` that schedules itself again
- **States:** per output `Unloaded` → `Loading(path, size)` → `Loaded` | `Failed` (stays `None`); clock `Day` | `Night`
- **Transitions:** `load_wallpaper` skips an identical request, otherwise →Loading and spawns (`app.rs:414-425`); `Msg::Wallpaper` is accepted only if path and size still match (`app.rs:598-606`); `Tick` refreshes `now`, reloads every display on a day/night flip (`app.rs:588-595`) and reschedules (`app.rs:596`); first tick (`app.rs:504`)
- **Hazards:** a reload replaces the image on screen with `None` at once (`app.rs:420`), so the plain scrim flashes until decoding ends; a failed decode can't be told from one still loading; entries for unplugged outputs stay
- **Reify:** Low — stale results are already dropped by the key match.

<a id="svc-09"></a>

### SVC-09 — Wrong-password shake animation
- **Where:** `desktop/aizome/crates/aizome-greeter/src/app.rs:118` (`Greeter.shake`, `.shaken`)
- **Encoding:** a step counter, the last step scheduled, timers through `cx.after`
- **States:** `Still(0)` → `Shaking(1..8)` → `Still`
- **Transitions:** starts when `login.failed` changes (`app.rs:270-275`); steps and stops (`app.rs:579-587`)
- **Hazards:** none observed (a new failure mid-shake restarts at 1, and the `step == shaken` check drops the old timer chain)
- **Reify:** Low — a local animation.

<a id="svc-10"></a>

### SVC-10 — Greeter power action and its error
- **Where:** `desktop/aizome/crates/aizome-greeter/src/app.rs:135` (`Greeter.power_error`)
- **Encoding:** `Option<String>` set from the result of a spawned `systemctl`
- **States:** `Idle` → `Running` (not recorded) → `Idle` | `Failed(message)` (sticky)
- **Transitions:** `power` spawns (`app.rs:345-355`); `PowerFailed` sets the error (`app.rs:578`); it shows below login messages (`desktop/aizome/crates/aizome-greeter/src/view.rs:344-345`)
- **Hazards:** never cleared, so an old failure shows for the rest of the greeter's life; no in-flight guard (picking twice runs two `systemctl`s)
- **Reify:** Low.

<a id="svc-11"></a>

### SVC-11 — Greeter's private compositor and process teardown
- **Where:** `desktop/aizome/crates/aizome-greeter/src/compositor.rs:15` (`Compositor.child`, `.socket`); also `desktop/aizome/crates/aizome-greeter/src/main.rs:44-81`, `desktop/aizome/crates/aizome-greeter/src/displays.rs:12` (`SettingsFile`)
- **Encoding:** a `Child` and its socket name; a temporary settings file that `Drop` deletes; the drop order in `main`
- **States:** `Preparing` (environment, display-scales file) → `Starting` (waiting for the socket, up to 20 s) → `Ready` → `Greeting` (kit loop) → `Stopping` (SIGTERM, up to 3 s, then SIGKILL) → `Reaped`; `Failed` (exited early or timed out) → exit 1
- **Transitions:** environment and `displays::prepare` (`main.rs:44-57`, `displays.rs:35-64`); spawn with PDEATHSIG (`compositor.rs:39-52`); readiness loop (`compositor.rs:55-65`, early exit at `compositor.rs:57-59`, timeout → stop at `compositor.rs:60-63`); `try_run` (`main.rs:71-77`); drop the compositor, then the settings (`main.rs:79-80`); `stop` and `Drop` (`compositor.rs:70-92`)
- **Hazards:** "ready" only means "the socket path exists" (`compositor.rs:56`), not that it accepts connections, and a stale `aizome-greeter-<pid>` socket left by an earlier greeter with a reused pid passes at once; the fork-race check assumes init reaps orphans (`compositor.rs:46`); in an abort-on-panic build `Drop` never runs, leaving the settings file in `$XDG_RUNTIME_DIR`
- **Reify:** Low — linear and guarded by Drop.
- **Related:** greetd (restarts the greeter); the `aizome --socket` compositor in `desktop/aizome/crates/aizome-comp`

<a id="svc-12"></a>

### SVC-12 — greetd login conversation
- **Where:** `desktop/aizome/crates/aizome-greeter/src/login.rs:48` (`Login.user`, `.step`, `.message`, `.failed`, `.asked`, `.generation`, `.open`)
- **Encoding:** the `Step` enum (partially explicit), plus a FIFO of `(Asked, generation)` for requests awaiting an answer, a generation counter that drops answers meant for an earlier login, and `open` (greetd holds a session)
- **States:** `Nobody` → `Waiting` → `Ask{secret, prompt}` → `Waiting` → `Starting` → `Done`; `Failed` (create failed); `open`, `generation` and queue depth vary independently; reachable odd combinations: `open && step == Failed`, `step == Waiting` after `lost` with nothing queued
- **Transitions:** `begin` (cancels first if open, bumps the generation, sends Create) (`login.rs:86-97`); `cancel` (`login.rs:100-111`); `answer` Ask→Waiting (`login.rs:114-121`); `heard`: drops Cancel and stale answers (`login.rs:126-134`), prompt → Ask (`login.rs:139-143`), info or error → acknowledge (`login.rs:145-153`), Success after Start → Done (`login.rs:155-158`), Success → Starting and StartSession (`login.rs:160-165`), Error → message, `failed += 1` on an auth error, begin again unless Create failed → Failed (`login.rs:166-189`); `lost` (`login.rs:194-199`)
- **Hazards:** correctness depends on greetd answering each request exactly once and in order (the queue pops blindly, `login.rs:126`); `lost` clears the queue without bumping `generation`; no timeout in `Waiting` or `Starting`; "someone else" stays available during `Starting`, which cancels a session greetd has already scheduled; secrets also linger in copies of `Msg::Password` outside this machine
- **Reify:** High — the authentication conversation is spread over seven fields; it is well tested but implicit.
- **Related:** the greetd daemon (external, greetd-ipc(7)); wire types at `desktop/aizome/crates/aizome-greeter/src/greetd.rs:12-54`; the UI side is SVC-05

<a id="svc-13"></a>

### SVC-13 — aizome-dev watch-and-reload loop
- **Where:** `desktop/aizome/crates/aizome-ipc/src/bin/aizome-dev.rs:555` (`seen`); caches `aizome-dev.rs:182-183` (`Dev.cargo_env`, `.target_dir`)
- **Encoding:** loop locals `seen`/`settled` (mtime snapshots); the output file's mtime before and after a build counts as "changed"
- **States:** `Priming` → `Idle` (polls every 400 ms) → `Settling` (250 ms until stable) → `Planning` → `Building/Reloading` → `Idle`
- **Transitions:** initial builds (`aizome-dev.rs:550-554`); poll (`aizome-dev.rs:558-562`); settle loop (`aizome-dev.rs:564-572`); `seen = settled` before acting (`aizome-dev.rs:574`); build and reload each part (`aizome-dev.rs:582-597`); the reload is skipped when the output's mtime didn't change (`aizome-dev.rs:471`, `aizome-dev.rs:483-485`)
- **Hazards:** a failed build, or a failed reload after a good build, isn't retried until the next save, because the output's mtime has already moved
- **Reify:** Low — a linear developer tool.
- **Related:** compositor reload handling at `desktop/aizome/crates/aizome-comp-core/src/requests.rs:176`

<a id="svc-14"></a>

### SVC-14 — aizome-dev app restart
- **Where:** `desktop/aizome/crates/aizome-ipc/src/bin/aizome-dev.rs:686` (`restart_app`: `pids`, `deadline`)
- **Encoding:** a set of pids found through `/proc/<pid>/exe`, a 5 s deadline
- **States:** `Finding` → `Closing` → `WaitingExit` → `Spawned` | `TimedOut` | `NotRunning`
- **Transitions:** windows → pids and Close (`aizome-dev.rs:689-703`); not running and not `always` → return (`aizome-dev.rs:704-706`); wait loop (`aizome-dev.rs:707-719`); Spawn (`aizome-dev.rs:720-729`)
- **Hazards:** "exited" means "/proc/&lt;pid> is gone", so a reused pid within 5 s hides the old process; apps with no windows are never found
- **Reify:** Low.

<a id="svc-15"></a>

### SVC-15 — Share picker's pick-area wait
- **Where:** `desktop/aizome/crates/aizome-ipc/src/bin/aizome-share-picker.rs:105` (`pick`: `tag`, `events`)
- **Encoding:** a per-process tag `share-<pid>`, a subscribed `Events` stream opened before the request
- **States:** `Subscribed` → `Asked` → `Picked(line)` | `Cancelled` | `NotOnOffer` | `CompositorGone`
- **Transitions:** subscribe (`aizome-share-picker.rs:108`); PickArea on a second connection (`aizome-share-picker.rs:109-113`); a matching AreaPicked → result (`aizome-share-picker.rs:114-127`); end of stream → error (`aizome-share-picker.rs:129`)
- **Hazards:** no timeout, so the portal's share request waits for as long as the compositor never answers (for example, if its logic is reloaded or rolled back mid-pick); `e?` aborts on any event this binary can't decode (`aizome-share-picker.rs:115` with `desktop/aizome/crates/aizome-ipc/src/lib.rs:1158`), which refuses the share
- **Reify:** Low — a short-lived helper.
- **Related:** picker overlay `desktop/aizome/crates/aizome-comp-logic/src/overlays/area.rs:63-80` (sends `AreaPicked{area: None}` to a superseded tag); xdg-desktop-portal-wlr `chooser_cmd` in `desktop/aizome/nix/module.nix:46`

<a id="svc-16"></a>

### SVC-16 — `aizomectl reload --offer` flow
- **Where:** `desktop/aizome/crates/aizome-ipc/src/bin/aizomectl.rs:789` (`offer_reload`)
- **Encoding:** the check report, locals `would`/`core`, and the stdout of `notify-send --wait` as the answer
- **States:** `Checking` → `NothingToDo` | `Offered` (blocked on the notification) → `Dismissed` | `Reloading` → `Reported`
- **Transitions:** check (`aizomectl.rs:791-799`); decide (`aizomectl.rs:800-810`); notify and wait (`aizomectl.rs:822-834`); "reload" → reconnect and reload (`aizomectl.rs:835-852`)
- **Hazards:** the notification may be answered hours later, when the check is stale (the real reload decides again, so only the wording goes stale); the caller blocks for as long as the notification lives
- **Reify:** Low.
- **Related:** the post-update unit that runs it (`aizomectl.rs:135-137`); compositor side at `desktop/aizome/crates/aizome-comp-core/src/requests.rs:176`

<a id="svc-17"></a>

### SVC-17 — IPC client connection modes
- **Where:** `desktop/aizome/crates/aizome-ipc/src/lib.rs:1046` (`Client`); `lib.rs:1109` (`Events.early`)
- **Encoding:** typestate `Client` → `Events` (partially explicit), plus a `VecDeque<Event>` of events that arrived while a request on the subscribed connection waited for its answer
- **States:** `RequestResponse` → `Subscribing` → `EventStream{early empty | early queued}` → `Closed`
- **Transitions:** `request` (`lib.rs:1072-1082`); `events` Subscribe→Ok (`lib.rs:1094-1100`); `Events::request` buffers events into `early` until a line that isn't an event (`lib.rs:1131-1144`); `next` drains `early` first (`lib.rs:1151-1153`), then reads (`lib.rs:1154-1161`)
- **Hazards:** unknown events are skipped inside `Events::request` (`lib.rs:1141`) but come back as `Err(InvalidData)` from `next` (`lib.rs:1158`), so consumers that stop on errors (`aizomectl events`, the share picker) end at the first newer event; with a read timeout, a line cut mid-way is lost (the partial read goes with the local buffer)
- **Reify:** Low — already a typestate; the unknown-event handling should be made consistent.
- **Related:** server side `desktop/aizome/crates/aizome-comp-core/src/ipc.rs:206-244` (`subscribed`, CatchKey)

<a id="svc-18"></a>

### SVC-18 — Catch-key wait for keyboard setup
- **Where:** `desktop/aizome/crates/aizome-ipc/src/setup.rs:66` (`catch_key`)
- **Encoding:** a subscribed connection, the caller's `given_up` closure, a 500 ms read timeout; the server keeps `keyboards.catch = Some((tag, conn))`
- **States:** `Connecting` → `Asked` → `Waiting` → `Caught(code, keyboard)` | `GivenUp` (sends `CatchKey{tag: None}`) | `Error`
- **Transitions:** subscribe (`setup.rs:67`); early give-up (`setup.rs:69-71`); ask (`setup.rs:72-76`); timeout (`setup.rs:77`); give-up in the loop (`setup.rs:79-83`); caught (`setup.rs:85`); timeouts and cut lines skipped (`setup.rs:87-89`); end of stream (`setup.rs:91`)
- **Hazards:** a `KeyCaught` line split across a timeout is thrown away as `InvalidData` (`setup.rs:89`), so that key is missed and the wait goes on; another connection's CatchKey silently replaces this one (server, `desktop/aizome/crates/aizome-comp-core/src/ipc.rs:218`), and this wait then ends only through `given_up`; a give-up is noticed at most every 500 ms
- **Reify:** Low — small, though "who holds the catch" lives only in the compositor.
- **Related:** `desktop/aizome/crates/aizome-comp-core/src/ipc.rs:216-228`, `desktop/aizome/crates/aizome-comp-core/src/keyboards.rs:103`; callers `desktop/aizome/apps/settei/src/panes/keyboard/setup.rs`, `installer/genkan/ui/src/app.rs`

<a id="svc-19"></a>

### SVC-19 — Audio ring flush, halt and segment protocol
- **Where:** `desktop/aizome/crates/aizome-media/src/audio.rs:33` (`Ring.write`, `.read`, `.skip_to`, `.segments`, `.busy`, `.halted`, `.ended`)
- **Encoding:** atomics plus a small mutex-guarded segment list, shared by the player thread (producer), the output callback (consumer, real-time) and the output's control thread
- **States:** `Flowing` | `FlushPending` (`skip_to > read` while the consumer runs) | `Halted` (paused; the consumer takes nothing) | `Rewound` (read stepped back into the history); separate `ended` flag
- **Transitions:** `push` (`audio.rs:112-125`); `pop` applies `skip_to` (`audio.rs:129-147`); `flush` sets `skip_to` (and `read` when idle), clears `ended`, starts a segment (`audio.rs:161-173`); `rewind_to` (`audio.rs:196-206`); `fill` raises `busy` and honours `halted` (`audio.rs:223-246`); halt and unhalt in the outputs (`audio.rs:365`, `audio.rs:379`, `desktop/aizome/crates/aizome-media/src/pipewire.rs:375`, `pipewire.rs:382`); `ended = true` (`desktop/aizome/crates/aizome-media/src/player.rs:1378`)
- **Hazards:** `ended` is written but never read (dead state); after a flush that isn't idle, the producer's `space()` ignores `skip_to` until the next `pop` (`audio.rs:105-109`); only four segments are kept (`audio.rs:171-172`), so `pts_of` for an older index silently fails; pausing correctly depends on two outputs repeating the same SeqCst busy/halted handshake
- **Reify:** Medium — lock-free real-time state; a packed state word would help but must stay wait-free.

<a id="svc-20"></a>

### SVC-20 — Null audio output thread
- **Where:** `desktop/aizome/crates/aizome-media/src/audio.rs:286` (`NullOutput.run`, `.thread`); `audio.rs:291` (`NullRun.playing`, `.stop`)
- **Encoding:** `Option<Arc<NullRun>>`, `Option<JoinHandle>`, two atomics, a condvar
- **States:** `Closed` → `Open(paused)` ⇄ `Open(playing)` → `Closed`
- **Transitions:** `open` closes first, then spawns (`audio.rs:316-357`); `set_playing(false)` halts the ring, waits for the callback, stops the clock (`audio.rs:361-374`); `set_playing(true)` (`audio.rs:376-382`); `close` and `Drop` (`audio.rs:389-408`)
- **Hazards:** `set_playing` before `open` is silently ignored; while paused the thread wakes every 200 ms
- **Reify:** Low.

<a id="svc-21"></a>

### SVC-21 — Playback clock anchor
- **Where:** `desktop/aizome/crates/aizome-media/src/clock.rs:17` (`Clock.seq`, `.pos`, `.at`, `.rate`, `.on_start`)
- **Encoding:** a seqlock over (pos, at, rate); `rate == 0` means stopped; one callback on the Stopped→Running edge
- **States:** `Stopped(pos)` ⇄ `Running(pos, at, rate)`
- **Transitions:** `set` (`clock.rs:63-87`), with the edge at `clock.rs:75-79`; writers that won't wait give up (`clock.rs:82-84`); `stop_at`/`run_from` (`clock.rs:114-121`); the writers are the output callback (`desktop/aizome/crates/aizome-media/src/audio.rs:242`), the outputs' pause (`audio.rs:372`, `desktop/aizome/crates/aizome-media/src/pipewire.rs:390`) and the player (`desktop/aizome/crates/aizome-media/src/player.rs:1067`, `player.rs:1097`, `player.rs:1114`, `player.rs:1135`, `player.rs:1388`); the callback is installed at `player.rs:288`
- **Hazards:** `on_start` runs on whichever thread starts the clock, PipeWire's real-time thread included, and it pokes the video queue under a `Mutex` (`desktop/aizome/crates/aizome-media/src/video.rs:170-173`); the callback's update is dropped when the player is mid-write (a harmless one-cycle loss)
- **Reify:** Low — already one struct with one writer function.

<a id="svc-22"></a>

### SVC-22 — PipeWire output thread and stream
- **Where:** `desktop/aizome/crates/aizome-media/src/pipewire.rs:47` (`PipeWireOutput.tx`, `.thread`, `.kind`, `.describe`); `pipewire.rs:130` (`Playing.active`, `.volume`); `pipewire.rs:156` (`current`)
- **Encoding:** a command channel (`Cmd`), `Option<Playing>` on the PipeWire loop, an `active` bool, the ring's `halted`
- **States:** `Connecting` → `Ready(no stream)` → `Stream(inactive)` ⇄ `Stream(active)` → `Ready` (close) → `Quit`; `ConnectFailed`
- **Transitions:** `connect` and its ready handshake (`pipewire.rs:57-74`, `pipewire.rs:148-155`); `Open` closes, then opens INACTIVE (`pipewire.rs:160-180`, `pipewire.rs:345-355`); `Playing` → `set_playing` (`pipewire.rs:181-186`, `pipewire.rs:369-391`); `Flush` (`pipewire.rs:187-191`); `Close` (`pipewire.rs:198-201`); `Quit` and Drop (`pipewire.rs:120-127`, `pipewire.rs:202-205`)
- **Hazards:** there is no listener for stream state or errors, so a node that vanishes (daemon restart, sink removed) is only noticed by the player's 1.5 s stall heuristic (SVC-26); `set_stream` applies only to the next `Open`; `set_playing` blocks the player thread until the PipeWire loop replies
- **Reify:** Low — a small command-driven actor.

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
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#svc-23)

<a id="svc-24"></a>

### SVC-24 — Seek bookkeeping across threads
- **Where:** `desktop/aizome/crates/aizome-media/src/player.rs:264` (`Seeks.asked`, `.done`, `.to`)
- **Encoding:** a request counter, a completion counter, and the latest target as bits
- **States:** `Settled` (done == asked) ⇄ `Pending` (done < asked; the position is `to`)
- **Transitions:** `Player::seek` (`player.rs:319-323`); `Command::Seek` completes it (`player.rs:766-771`); read by `position` (`player.rs:374-380`)
- **Hazards:** `to` is clamped to ≥ 0 but not to the duration (the player's thread clamps at `player.rs:1133`), so `position` briefly reports a target past the end; seeks that do nothing still complete
- **Reify:** Low — tiny and correct.

<a id="svc-25"></a>

### SVC-25 — Audio decode part (skip, rebase, drain)
- **Where:** `desktop/aizome/crates/aizome-media/src/player.rs:591` (`AudioPart.packets`, `.pending`, `.next_pts`, `.skip_until`, `.rebase`, `.drained`, `.tempo`)
- **Encoding:** fields that coordinate samples decoded but not yet pushed, a trim threshold, a "the next sample starts a segment" flag, and the drain status
- **States:** `Fresh(rebase)` → `Filling` → `Full(HIGH)` | `Backlogged(pending)` → `Draining` → `Drained`; `Skipping(until)` after a precise seek or a resync
- **Transitions:** `start_audio` (`player.rs:928-950`); reset in `seek_to` (`player.rs:1151-1161`); `resync_video` keeps the decoded audio and skips packets read again (`player.rs:1548-1552`); `decode_audio` pushes pending samples, stops at HIGH, drains once, trims, and rebases through `ring.flush` (`player.rs:1427-1500`, rebase at `player.rs:1470-1473`); Speed drops `tempo` (`player.rs:777-779`)
- **Hazards:** frames without timestamps fall back to `next_pts` (`player.rs:1459`), so after a resync they may be skipped wrongly; the tempo filter is rebuilt lazily from the first frame's format
- **Reify:** Medium — six coupled fields, with transitions in four functions.

<a id="svc-26"></a>

### SVC-26 — Audio output fallback to silence
- **Where:** `desktop/aizome/crates/aizome-media/src/player.rs:655` (`Ctl.output`, `.silent`, `.output_name`, `.started_at`)
- **Encoding:** the real output plus an `Option<NullOutput>` that overrides it for the current item; an `Option<Instant>` stall timer
- **States:** `Real` → `Silent(open failed)` | `Silent(stalled)` → `Real` on the next item or track
- **Transitions:** PipeWire connect fallback at construction (`player.rs:686-695`); open failure → Silent (`player.rs:960-966`); `out()` picks one (`player.rs:726-731`); stall check → `silent()` (`player.rs:1360-1366`, `player.rs:1215-1230`); cleared by `select_audio` and `close` (`player.rs:1005-1006`, `player.rs:1058`)
- **Hazards:** the 1.5 s heuristic also fires for a sink that is slow to wake (a Bluetooth headset resuming), silencing the rest of the item; `started_at` is never cleared
- **Reify:** Medium — an override plus a timer heuristic, spread over five sites.

<a id="svc-27"></a>

### SVC-27 — Wake coalescing in the player and video threads
- **Where:** `desktop/aizome/crates/aizome-media/src/player.rs:39` (`Wake.flag`); `desktop/aizome/crates/aizome-media/src/video.rs:124` (`Queue.woken`)
- **Encoding:** a sticky "woken" bool beside each condvar, so a poke that arrives while the thread is busy isn't slept through
- **States:** `Clear` ⇄ `Woken`
- **Transitions:** `Wake::wake` (`player.rs:51-54`); `Wake::wait` consumes it (`player.rs:56-65`); `Shared::poke` (`video.rs:170-173`); the demuxer sets it on packets (`player.rs:1314`); `Shared::wait` consumes it (`video.rs:179-189`)
- **Hazards:** none observed
- **Reify:** Low — a textbook pattern.

<a id="svc-28"></a>

### SVC-28 — Video presentation loop (poster, steps, generations)
- **Where:** `desktop/aizome/crates/aizome-media/src/video.rs:405` (`State.decoder`, `.frames`, `.from`, `.poster`, `.generation`, `.steps`, `.draining`, `.last`); `video.rs:90` (`Shared.drained`, `.shown_generation`, `.last_pts`)
- **Encoding:** an optional decoder, a queue of decoded frames, a threshold to show from, poster and step flags, and the flush generation, published on its first presentation
- **States:** `NoStream` → `Started(poster)` → `Waiting(clock stopped)` | `Pacing(due later)` | `Presenting` | `Dropping(late)` → `Draining` → `Drained`; `Flushed(gen)` after each seek
- **Transitions:** `Cmd::Start` (`video.rs:529-545`); `Cmd::Flush` (`video.rs:546-558`); `Cmd::Step` (`video.rs:559`); `Cmd::Stop` (`video.rs:561`); the loop (`video.rs:469-519`: drained at `video.rs:484`, poster or step at `video.rs:489-494`, stopped wait at `video.rs:497-500`, drop at `video.rs:504-509`, pacing at `video.rs:510-513`, present at `video.rs:514-518`); `present` publishes the generation and wakes the demuxer (`video.rs:691-693`); readiness on the demuxer side (`desktop/aizome/crates/aizome-media/src/player.rs:1344`)
- **Hazards:** `Start` resets neither `generation` nor `steps`, and `Flush` doesn't reset `skipping`; steps pile up while frames are missing; readiness depends on a frame being presented, which suspension prevents (SVC-29)
- **Reify:** Medium — many flags, but one loop owns them.

<a id="svc-29"></a>

### SVC-29 — Video suspension out of sight, and catching up
- **Where:** `desktop/aizome/crates/aizome-media/src/video.rs:430` (`State.suspended`, `.by_window`); `video.rs:94` (`Shared.suspended`, `.window_hidden`, `.resync`); `video.rs:194` (`Sight.presented`, `.discarded_run`, `.last_presented`)
- **Encoding:** two copies of "suspended" (one local to the thread, one shared), the window's hint, discard-run counters, a resync request flag
- **States:** `Visible` → `Suspended{by window}` | `Suspended{by discards, probing every second}` → `Resyncing` → `Visible`; invalid but reachable: local `suspended == true` while the shared `suspended == false`
- **Transitions:** detect and `suspend` (`video.rs:768-776`, `video.rs:873-885`); wait while hidden (`video.rs:778-782`); probe (`video.rs:783-804`); back in sight → resync (`video.rs:805-814`); the demuxer honours the shared flag (`desktop/aizome/crates/aizome-media/src/player.rs:1268`, `player.rs:1274`, `player.rs:1310`); `resync_video` (`player.rs:1539-1562`); `set_visible` (`player.rs:361-365`); `seek_to` clears only the shared flag (`player.rs:1176`); `Cmd::Start` clears both (`video.rs:540-541`)
- **Hazards:** the two flags diverge on every seek; after that the demuxer queues video packets the thread won't consume; probing shows the last frame every second while out of sight
- **Reify:** High — duplicated state with a reachable invalid combination that stalls playback.
- **Suspected bug:** a seek while the window is hidden stalls playback. Utsushi reports xdg suspension through `set_visible(false)` (`desktop/aizome/apps/utsushi/src/app.rs:1149`), and MPRIS seeks arrive at `app.rs:649`. `seek_to` stops the audio and waits for the first frame of the flushed generation (`player.rs:1146`, `player.rs:1344`). The video thread is still suspended locally, so `check_sight` returns before presenting (`video.rs:778-782`), and the audio stays silent until the window is shown again. Speed and track changes seek too, so they stall the same way. (confidence: medium)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#svc-29)

<a id="svc-30"></a>

### SVC-30 — Late-frame skipping hysteresis
- **Where:** `desktop/aizome/crates/aizome-media/src/video.rs:433` (`State.late_run`, `.skipping`)
- **Encoding:** a counter that rises on late frames and falls on timely ones, and a latched decoder `skip_frame` mode
- **States:** `OnTime` ⇄ `Skipping(NONREF)` at `late_run >= 4`
- **Transitions:** `late` (`video.rs:644-657`), called on a drop (`video.rs:507`) and on a presentation (`video.rs:514`); `Flush` resets only `late_run` (`video.rs:556`)
- **Hazards:** `skipping` isn't reset on `Start`, so a new decoder may run DEFAULT while the flag says it skips (this corrects itself as `late_run` falls)
- **Reify:** Low.

<a id="svc-31"></a>

### SVC-31 — Frame path and the cache of exported surfaces
- **Where:** `desktop/aizome/crates/aizome-media/src/video.rs:420` (`State.path`, `.surfaces`, `.frames_ctx`, `.pool`); `video.rs:399` (`enum Path`); `video.rs:139` (`Stats.path`)
- **Encoding:** the partially explicit `Path` enum, plus a surface cache keyed by (frames-context pointer, surface id), an SHM pool sized to the last frame, and a separate integer path in the stats
- **States:** `Unknown` → `Dmabuf` (cache warm) | `Shm` (latched for the stream); pool `None` | `Sized(w, h)`
- **Transitions:** `prepare` (`video.rs:698-762`: a context change clears the cache at `video.rs:711-714`, a hit at `video.rs:715-717`, export → Dmabuf at `video.rs:718-725`, refusal → Shm at `video.rs:726-730`, pool resize at `video.rs:749-751`); `Cmd::Start` resets them (`video.rs:534-536`)
- **Hazards:** the cache is keyed by a raw pointer, so a frames context recreated at the same address (a resolution change mid-stream) would reuse old exports; a hardware frame copied back from the GPU and a software frame share `Shm`; `stats.path` is written per frame, separately from `path`
- **Reify:** Medium — the enum exists, but its data lives beside it.

<a id="svc-32"></a>

### SVC-32 — GPU decoding falling back to software
- **Where:** `desktop/aizome/crates/aizome-media/src/video.rs:436` (`State.failures`, `.any_frame`); `video.rs:104` (`Shared.hw_failed`); `desktop/aizome/crates/aizome-media/src/player.rs:653` (`Ctl.hardware`, `.hw_device`)
- **Encoding:** a failure counter up to the first frame, a flag between threads, a sticky process-wide "no VA-API" bool, a device created on first use
- **States:** `GpuDecoding` → `GpuFailing(n)` → `FallbackRequested` → `SoftwareDecoding`; device `Unprobed` → `Ready` | `Unavailable` (sticky)
- **Transitions:** errors counted (`video.rs:590-595`, `video.rs:620-623`); trips at 4 with no frame yet (`video.rs:629-635`); the player picks it up (`player.rs:1259-1261`) → `software_video` (`player.rs:1234-1254`); device created or failed (`player.rs:802-826`); environment override (`player.rs:696`)
- **Hazards:** a decoder that produces one frame and then keeps failing never falls back; `software_video` seeks, so it inherits SVC-29's stall while hidden
- **Reify:** Medium — spans two threads and two files.

<a id="svc-33"></a>

### SVC-33 — Ownership of shared-memory frame slots
- **Where:** `desktop/aizome/crates/aizome-media/src/video.rs:207` (`ShmPool.slots`); `video.rs:272` (`SlotGuard`); `video.rs:890` (`enum Keep`)
- **Encoding:** an `AtomicBool` "busy" per slot, released when the compositor drops the frame's `keep`
- **States:** per slot `Free` → `Busy(on screen)` → `Free`
- **Transitions:** `take` compare-and-swap (`video.rs:263-269`); the guard is stored in `keep` (`video.rs:686`, `video.rs:761`); `SlotGuard::drop` (`video.rs:274-278`); the pool is replaced on a size change (`video.rs:749-751`)
- **Hazards:** with all four slots busy, the frame is dropped (`video.rs:753`) and counted as a drop
- **Reify:** Low.

<a id="svc-34"></a>

### SVC-34 — (test harness) Media engine event-wait driver
- **Where:** `desktop/aizome/crates/aizome-media/tests/engine.rs:20` (`Run`)
- **Encoding:** an event channel and loops with deadlines
- **States:** `Waiting` → `Matched` | `TimedOut` (panic)
- **Transitions:** `until` (`engine.rs:41-57`); `state` (`engine.rs:59-61`); stats polled with deadlines (`engine.rs:154-158`, `engine.rs:167-171`)
- **Hazards:** wall-clock deadlines (5 s) can flake on loaded builders
- **Reify:** Low.

<a id="svc-35"></a>

### SVC-35 — File chooser's one-shot answer lifecycle
- **Where:** `desktop/aizome/crates/aizome-portal/src/chooser/mod.rs:102` (`Chooser.answer`, `.memory`, `.memory_path`); also `desktop/aizome/crates/aizome-portal/src/main.rs:10-21`, `desktop/aizome/crates/aizome-portal/src/memory.rs:19`
- **Encoding:** `Option<Answer>` (None means cancelled, or not yet answered), the kit's quit flag, and the persisted `file-chooser.json`, loaded at start and saved at the end
- **States:** `Browsing` → `ConfirmingReplace` (alert) → `Answered` | `Cancelled` → `Quit` → answer on stdout
- **Transitions:** memory loaded (`chooser/mod.rs:122-124`); `step` (`chooser/mod.rs:149-171`, alert at `chooser/mod.rs:162-167`); `Msg::Replace` (`chooser/mod.rs:394-397`); `Msg::Cancel` (`chooser/mod.rs:393`); `close_requested` → cancel (`chooser/mod.rs:472-474`); `finish` saves the memory and quits (`chooser/mod.rs:173-186`); the process writes the answer (`main.rs:17-19`)
- **Hazards:** up to 16 choosers run at once (`desktop/aizome/crates/aizome-portal/src/service.rs:21`), and each saves its whole memory when it ends, so the last to finish overwrites the folders the others remembered, all through the same `file-chooser.json.tmp` (`memory.rs:42-44`); nothing stops `finish` from running twice (Quit from the app menu calls `close_requested` for every window, `desktop/aizome/crates/aizome-kit/src/app.rs:211-219`)
- **Reify:** Medium — the outcome is clear, but the persisted memory is an unlocked read-modify-write across processes.
- **Related:** `desktop/aizome/crates/aizome-portal/src/worker.rs` reads the answer (SVC-39)

<a id="svc-36"></a>

### SVC-36 — Chooser sheet (Go to Folder, New Folder)
- **Where:** `desktop/aizome/crates/aizome-portal/src/chooser/mod.rs:105` (`Chooser.sheet`); `chooser/mod.rs:53` (`Sheet.key`, `.kind`, `.text`, `.problem`)
- **Encoding:** `Option<Sheet>` with an embedded problem string
- **States:** `None` → `Open(kind, text, problem?)` → `None`
- **Transitions:** `open_sheet` (`chooser/mod.rs:195-206`); `SheetText` clears the problem (`chooser/mod.rs:404-409`); `submit_sheet` closes on success and sets a problem on failure (`chooser/mod.rs:214-265`); `close_sheet` (`chooser/mod.rs:208-212`); close box (`chooser/mod.rs:468-471`)
- **Hazards:** none observed
- **Reify:** Low — explicit enough.

<a id="svc-37"></a>

### SVC-37 — Chooser location, history and listing
- **Where:** `desktop/aizome/crates/aizome-portal/src/chooser/model.rs:49` (`Browser.loc`, `.back`, `.forward`, `.entries`, `.shown`, `.shown_loc`, `.selected`, `.loading`, `.error`, `.problem`, `.select_after`, `.generation`)
- **Encoding:** the `Loc` enum, plus a generation counter that drops late reads, a `loading` bool, `error` versus `entries`, and `shown_loc`, which records the location `shown` came from
- **States:** `Loading(gen)` → `Listed(loc)` | `Unreadable(loc)`; history stacks; invalid but reachable: `loading` with `shown_loc != loc` while `selected` indexes the old listing
- **Transitions:** `go`/`go_back`/`go_forward` (`model.rs:158-180`); `start_reading` (`model.rs:188-193`); `listed` drops stale reads, keeps the selection on a re-read, and applies `select_after` (`model.rs:197-227`); `refresh` (`model.rs:231-255`); spawned reads (`desktop/aizome/crates/aizome-portal/src/chooser/mod.rs:139-142`); a late Desk reply sets `loc` directly (`chooser/mod.rs:342-356`); New Folder reads again (`chooser/mod.rs:250-253`)
- **Hazards:** while a read is in progress, Accept acts on the new `loc` with the old listing on screen (directory mode chooses the new folder, `model.rs:344-345`; Save joins the name with it, `model.rs:366`); the Desk reply can move someone who has already started selecting or typing, as long as they haven't navigated (`chooser/mod.rs:346`), and leaves no Back entry
- **Reify:** Medium — the generation guard is sound, but the loading window isn't modelled.

<a id="svc-38"></a>

### SVC-38 — Portal FileChooser request lifecycle
- **Where:** `desktop/aizome/crates/aizome-portal/src/service.rs:38` (`FileChooser.open`); per call `service.rs:46-87`
- **Encoding:** a shared count of open choosers, a `RequestHandle` object exported at the call's handle, a `Cancel` flag, a worker thread and a bounded channel
- **States:** `Refused(16 open)` | `Registered` → `Running` → `Answered(SUCCESS)` | `Cancelled(CANCELLED)` | `Closed/Failed(ENDED)` → `Unregistered`
- **Transitions:** count or refuse (`service.rs:48-55`); export the handle (`service.rs:59-61`); spawn the worker (`service.rs:65-68`); await (`service.rs:69`); unexport and decrement (`service.rs:70-71`); map the outcome (`service.rs:72-86`); `Request.Close` → cancel (`service.rs:139-141`); the service ends when the name moves (`service.rs:155-161`)
- **Hazards:** a handle that fails to export leaves the chooser impossible to cancel (`service.rs:59-61`); if the method future is dropped (the connection closing), the count and the handle leak and the chooser keeps running; `open.lock().unwrap()` panics once the mutex is poisoned
- **Reify:** Medium — request object, counter and thread, with cleanup on one path only.
- **Related:** the xdg-desktop-portal frontend (external, `org.freedesktop.impl.portal.Request`); the worker is SVC-39

<a id="svc-39"></a>

### SVC-39 — Chooser subprocess supervision
- **Where:** `desktop/aizome/crates/aizome-portal/src/worker.rs:57` (`run`: `child`, `stdin`, `stdout`, `written`, `output`)
- **Encoding:** optional pipe ends (None means done), byte counters, a cancel flag
- **States:** `Spawning` → `Feeding+Reading` → `Reading` → `Exited(Answered | Failed)` | `Aborted(Closed | TooMuch | Lost)`
- **Transitions:** early cancel (`worker.rs:58-60`); setsid and spawn (`worker.rs:63-74`); cancel → kill the process group (`worker.rs:82-85`, `worker.rs:48-53`); feed the request (`worker.rs:86-96`); read up to the cap (`worker.rs:97-113`); exit → drain → outcome (`worker.rs:114-129`); poll every 25 ms (`worker.rs:130-140`); wait error → abort (`worker.rs:141-144`)
- **Hazards:** wakes every 25 ms for as long as a chooser is open; on a normal exit, processes left in the chooser's session aren't killed; output such a process still holds after the chooser exits is lost
- **Reify:** Low — one function with an explicit `Outcome`.
- **Related:** the chooser process, `desktop/aizome/crates/aizome-portal/src/main.rs:10-21`

<a id="svc-40"></a>

### SVC-40 — (test harness) Chooser home fixture and settle
- **Where:** `desktop/aizome/crates/aizome-portal/tests/common/mod.rs:17` (`Home`)
- **Encoding:** a fixture tree with fixed mtimes, removed by `Drop`, and a wait until `browser.loading` is false
- **States:** `Built` → `Settled` | `TimedOut` → `Removed`
- **Transitions:** `Home::new` (`common/mod.rs:20-48`); `chooser` turns off the Desk lookup (`common/mod.rs:70-76`); `settle` (`common/mod.rs:79-81`); `Drop` (`common/mod.rs:55-59`)
- **Hazards:** none observed
- **Reify:** Low.

<a id="svc-41"></a>

### SVC-41 — (test harness) Private session bus with stand-in choosers
- **Where:** `desktop/aizome/crates/aizome-portal/tests/service.rs:19` (`Bus.daemon`, `.address`)
- **Encoding:** a `dbus-daemon` child, shell-script choosers, and a `started` marker file polled until a deadline
- **States:** `DaemonUp` → `Serving` → `ChooserStarted` → `Closed` → `DaemonKilled`
- **Transitions:** `Bus::new` (`service.rs:25-35`); `serve` (`service.rs:42-57`); wait for the marker (`service.rs:165-169`); Close (`service.rs:175`); `Drop` (`service.rs:60-65`)
- **Hazards:** none observed
- **Reify:** Low.

## Already explicit

- `desktop/aizome/crates/aizome-apps/src/entry.rs:243` `enum Group` — the parser's current group (none, `[Desktop Entry]`, an action); transitions centralised in the `Line::Group` arm of `fn DesktopEntry::parse` (`desktop/aizome/crates/aizome-apps/src/entry.rs:254-269`)
- `desktop/aizome/crates/aizome-apps/src/exec.rs:96` `enum State` — the `Exec` quoting lexer (plain, double-quoted, single-quoted); transitions centralised in the `match (&state, c)` loop of `fn Exec::parse` (`desktop/aizome/crates/aizome-apps/src/exec.rs:105-156`)

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/crates/aizome-apps/src/entry.rs` | 652 | explicit |  | parse-local group cursor enum; pure parsing otherwise |
| `desktop/aizome/crates/aizome-apps/src/error.rs` | 47 | none |  | error enum and io conversions |
| `desktop/aizome/crates/aizome-apps/src/exec.rs` | 540 | explicit |  | Exec quoting lexer enum; pure expansion |
| `desktop/aizome/crates/aizome-apps/src/icons.rs` | 754 | machines | [SVC-01](#svc-01) | icon lookup caches and invalidation |
| `desktop/aizome/crates/aizome-apps/src/index.rs` | 1185 | machines | [SVC-02](#svc-02), [SVC-03](#svc-03) | index freshness signature; watcher thread |
| `desktop/aizome/crates/aizome-apps/src/keyfile.rs` | 296 | none |  | pure key-file parsing |
| `desktop/aizome/crates/aizome-apps/src/lib.rs` | 172 | none |  | Wake callback, random hex, test TempDir RAII |
| `desktop/aizome/crates/aizome-apps/src/mime.rs` | 1238 | machines | [SVC-04](#svc-04), [SVC-02](#svc-02) | foot to Hotaru migration; list writes reload signature |
| `desktop/aizome/crates/aizome-apps/src/xdg.rs` | 256 | none |  | XDG path resolution, pure |
| `desktop/aizome/crates/aizome-greeter/src/app.rs` | 633 | machines | [SVC-05](#svc-05), [SVC-06](#svc-06), [SVC-07](#svc-07), [SVC-08](#svc-08), [SVC-09](#svc-09), [SVC-10](#svc-10) | greeter stage, greetd thread, surfaces, wallpaper, shake, power |
| `desktop/aizome/crates/aizome-greeter/src/clock.rs` | 100 | none |  | local time formatting |
| `desktop/aizome/crates/aizome-greeter/src/compositor.rs` | 92 | machines | [SVC-11](#svc-11) | compositor child lifecycle |
| `desktop/aizome/crates/aizome-greeter/src/config.rs` | 156 | machines | [SVC-05](#svc-05) | persisted last user picks the initial stage |
| `desktop/aizome/crates/aizome-greeter/src/displays.rs` | 83 | machines | [SVC-11](#svc-11) | temp settings file lives with the compositor |
| `desktop/aizome/crates/aizome-greeter/src/greetd.rs` | 213 | machines | [SVC-12](#svc-12) | greetd wire codec and stateless Demo; conversation in SVC-12 |
| `desktop/aizome/crates/aizome-greeter/src/lib.rs` | 27 | none |  | module list and usage text |
| `desktop/aizome/crates/aizome-greeter/src/login.rs` | 362 | machines | [SVC-12](#svc-12) | greetd login conversation |
| `desktop/aizome/crates/aizome-greeter/src/main.rs` | 109 | machines | [SVC-11](#svc-11) | process start and teardown order |
| `desktop/aizome/crates/aizome-greeter/src/sessions.rs` | 320 | none |  | session entry parsing, pure |
| `desktop/aizome/crates/aizome-greeter/src/users.rs` | 118 | none |  | passwd parsing, pure |
| `desktop/aizome/crates/aizome-greeter/src/view.rs` | 361 | machines | [SVC-05](#svc-05) | view derives the Name sub-state |
| `desktop/aizome/crates/aizome-greeter/tests/budget.rs` | 177 | none |  | tests (frame budget) |
| `desktop/aizome/crates/aizome-greeter/tests/screens.rs` | 234 | none |  | tests |
| `desktop/aizome/crates/aizome-ipc/src/bin/aizome-dev.rs` | 885 | machines | [SVC-13](#svc-13), [SVC-14](#svc-14) | watch loop; app restart |
| `desktop/aizome/crates/aizome-ipc/src/bin/aizome-share-picker.rs` | 169 | machines | [SVC-15](#svc-15) | pick-area wait |
| `desktop/aizome/crates/aizome-ipc/src/bin/aizomectl.rs` | 1139 | machines | [SVC-16](#svc-16) | reload --offer flow; rest is arg parsing and printing |
| `desktop/aizome/crates/aizome-ipc/src/lib.rs` | 1254 | machines | [SVC-17](#svc-17) | client typestate and early-event queue |
| `desktop/aizome/crates/aizome-ipc/src/setup.rs` | 109 | machines | [SVC-18](#svc-18) | catch-key wait |
| `desktop/aizome/crates/aizome-media/src/audio.rs` | 542 | machines | [SVC-19](#svc-19), [SVC-20](#svc-20) | ring flush/halt protocol; null output thread |
| `desktop/aizome/crates/aizome-media/src/av.rs` | 526 | none |  | FFmpeg RAII wrappers |
| `desktop/aizome/crates/aizome-media/src/clock.rs` | 164 | machines | [SVC-21](#svc-21) | playback clock anchor |
| `desktop/aizome/crates/aizome-media/src/lib.rs` | 35 | none |  | module list |
| `desktop/aizome/crates/aizome-media/src/pipewire.rs` | 435 | machines | [SVC-22](#svc-22) | PipeWire output thread and stream |
| `desktop/aizome/crates/aizome-media/src/player.rs` | 1799 | machines | [SVC-23](#svc-23), [SVC-24](#svc-24), [SVC-25](#svc-25), [SVC-26](#svc-26), [SVC-27](#svc-27), [SVC-32](#svc-32) | session, seeks, audio part, output fallback, wake, hw device |
| `desktop/aizome/crates/aizome-media/src/subs.rs` | 303 | none |  | subtitle decoding and parsing, pure |
| `desktop/aizome/crates/aizome-media/src/video.rs` | 934 | machines | [SVC-28](#svc-28), [SVC-29](#svc-29), [SVC-30](#svc-30), [SVC-31](#svc-31), [SVC-32](#svc-32), [SVC-33](#svc-33), [SVC-27](#svc-27) | video thread machines |
| `desktop/aizome/crates/aizome-media/tests/engine.rs` | 320 | machines | [SVC-34](#svc-34) | test event-wait driver |
| `desktop/aizome/crates/aizome-media/tests/fixtures/make.sh` | 56 | none |  | linear fixture generator script |
| `desktop/aizome/crates/aizome-portal/src/chooser/mod.rs` | 476 | machines | [SVC-35](#svc-35), [SVC-36](#svc-36), [SVC-37](#svc-37) | chooser answer, sheet, Desk redirect |
| `desktop/aizome/crates/aizome-portal/src/chooser/model.rs` | 456 | machines | [SVC-37](#svc-37) | browser location and listing |
| `desktop/aizome/crates/aizome-portal/src/chooser/view.rs` | 305 | none |  | pure view of SVC-37 state |
| `desktop/aizome/crates/aizome-portal/src/filter.rs` | 183 | none |  | glob and MIME matching, pure |
| `desktop/aizome/crates/aizome-portal/src/lib.rs` | 28 | none |  | constants |
| `desktop/aizome/crates/aizome-portal/src/main.rs` | 40 | machines | [SVC-35](#svc-35) | chooser process stdin request, stdout answer |
| `desktop/aizome/crates/aizome-portal/src/memory.rs` | 93 | machines | [SVC-35](#svc-35) | persisted chooser memory, whole-file save |
| `desktop/aizome/crates/aizome-portal/src/request.rs` | 414 | none |  | portal wire types and conversions |
| `desktop/aizome/crates/aizome-portal/src/service.rs` | 163 | machines | [SVC-38](#svc-38) | portal request lifecycle |
| `desktop/aizome/crates/aizome-portal/src/worker.rs` | 189 | machines | [SVC-39](#svc-39) | chooser subprocess supervision |
| `desktop/aizome/crates/aizome-portal/tests/chooser.rs` | 286 | none |  | tests (drive the chooser through SVC-40) |
| `desktop/aizome/crates/aizome-portal/tests/common/mod.rs` | 81 | machines | [SVC-40](#svc-40) | test home fixture and settle |
| `desktop/aizome/crates/aizome-portal/tests/goldens.rs` | 93 | none |  | tests (goldens) |
| `desktop/aizome/crates/aizome-portal/tests/service.rs` | 182 | machines | [SVC-41](#svc-41) | test private bus and stand-in choosers |
