# SYC: System clients: printers and sysinfo; config, theme and face

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/crates/aizome-sys, aizome-config, aizome-theme, aizome-face`. Audited at `8e0acbe5`; line numbers refer to that commit.

55 files, 0 pending. 28 implicit machines: 5 High, 3 Medium, 20 Low.

## Machines

<a id="syc-01"></a>

### SYC-01 — Keyboard settings: inherited session defaults vs saved choices
- **Where:** `desktop/aizome/crates/aizome-config/src/lib.rs:304` (`Keyboard.layouts`, `.variant`, `.options`, `.model`); defaults from the environment at `lib.rs:321`
- **Encoding:** whether the `[input.keyboard]` keys are absent from `settings.toml`. Absent means "use the session's `XKB_DEFAULT_*`", which serde fills in through `#[serde(default)]` (`lib.rs:303`, `lib.rs:325-330`). Present means "the user's choice". Nothing records which case applies.
- **States:** `Inherited` (keys absent, re-read from the environment on every load) → `Pinned` (keys written). There is no transition back except deleting the keys by hand.
- **Transitions:** a load with the keys missing fills them from the environment (`lib.rs:325-330`); any save by any process, for any section, serialises every `Keyboard` field (`lib.rs:677-678`, `lib.rs:686-693`), which moves Inherited→Pinned
- **Hazards:** unrelated saves (Deskbar, location, notifications) silently pin the session keyboard. Processes started with different environments can pin different values. `desktop/aizome/crates/aizome-config/tests/keyboard_defaults.rs:49` asserts only that loading creates no file, never that saving keeps the inheritance.
- **Reify:** High — user-visible, with a suspected bug; needs explicit provenance (for example `Option` fields, or skipping values that equal the environment default)
- **Suspected bug:** the user never opens keyboard settings but once collapses the Deskbar, and the shell saves (`desktop/aizome/crates/aizome-shell/src/shell.rs:1012`). The file now holds `layouts = ["us"]`. The admin later sets `services.xserver.xkb.layout = "de"`, which reaches `XKB_DEFAULT_LAYOUT` through `desktop/aizome/nix/module.nix:126`, but the session stays on US (confidence: medium)
- **Related:** `desktop/aizome/nix/module.nix:126` (sets `XKB_DEFAULT_*`); `desktop/aizome/crates/aizome-shell/src/shell.rs:1004` (`save_section`)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#syc-01)

<a id="syc-02"></a>

### SYC-02 — One-shot migration of the legacy `[keys] terminal` key
- **Where:** `desktop/aizome/crates/aizome-config/src/lib.rs:523` (`Keys.legacy_terminal`, `#[serde(rename = "terminal", skip_serializing)]`)
- **Encoding:** the presence of `terminal` in `settings.toml`, plus the compositor's marker file in `state_dir()` (`desktop/aizome/crates/aizome-comp-core/src/terminal.rs:71`)
- **States:** `Legacy` (key present, no marker) → `Migrated` (marker written); or `KeyLost` (any save happens before a successful migration: no key, no marker)
- **Transitions:** deserialising reads the key (`lib.rs:523`); every `to_toml`/`save_to` drops it (`lib.rs:677`, `lib.rs:686`); the compositor captures it at session start (`desktop/aizome/crates/aizome-comp-core/src/session.rs:176`) and writes the marker only on success (`desktop/aizome/crates/aizome-comp-core/src/terminal.rs:93-94`)
- **Hazards:** the migration's input lives in a file that every process rewrites, and its completion marker lives in another file with no ordering between them. A failed migration is "tried again next session" (`desktop/aizome/crates/aizome-comp-core/src/terminal.rs:79`), but by then its input may be gone.
- **Reify:** High — loses the user's choice (suspected bug)
- **Suspected bug:** the first session after the upgrade fails in `with_index` (`desktop/aizome/crates/aizome-comp-core/src/terminal.rs:76-81`). During that session the shell saves any section, which drops `terminal`. The next session retries with `None`, so ❖Return starts Hotaru instead of the user's previous terminal (confidence: medium)
- **Related:** `desktop/aizome/crates/aizome-comp-core/src/terminal.rs:70` (`move_to_hotaru_once`), `desktop/aizome/crates/aizome-comp-core/src/session.rs:176`
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#syc-02)

<a id="syc-03"></a>

### SYC-03 — Settings file lifecycle: load, fallback to defaults, atomic save, cross-process writers
- **Where:** `desktop/aizome/crates/aizome-config/src/lib.rs:686` (`Settings::save_to`: fixed temp path at `lib.rs:691`); `lib.rs:656` (`load_from`); `lib.rs:666` (`load_or_default`)
- **Encoding:** `settings.toml`, a fixed sibling `settings.toml.tmp`, and one in-memory `Settings` copy per process (compositor, shell, Settei, apps). There is no lock, version or generation.
- **States:** `Absent` (defaults) → `Valid` ⇄ `Writing` (temp file exists) → `Valid`; `Broken` (fails to parse). Each process's copy can be `Fresh` or `Stale` (another process wrote since it loaded). Broken and Absent collapse into the same in-memory defaults.
- **Transitions:** NotFound→defaults (`lib.rs:659`); parse error→`Err` (`lib.rs:674`), which `load_or_default` turns into defaults with a warning (`lib.rs:667-670`); save: `create_dir_all` (`lib.rs:689`), write the temp file (`lib.rs:692`), rename it over the target (`lib.rs:693`)
- **Hazards:** Settei's `write_atomically` uses the same `settings.toml.tmp` (`desktop/aizome/apps/settei/src/shared.rs:256`). Concurrent writers truncate each other's temp file: one rename can publish the other writer's half-written text, and the losing rename fails with ENOENT. There is no fsync before the rename. A read-modify-write from a stale copy undoes other processes' edits (last writer wins). `load_or_default` cannot tell "missing" from "broken", so a later save replaces a broken but recoverable file with defaults.
- **Reify:** High — data loss across processes
- **Suspected bug:** a hand edit with one typo makes the watcher fire, and the shell reloads defaults (`desktop/aizome/crates/aizome-shell/src/shell.rs:1273`). The next Deskbar or menu bar change saves those defaults over every other setting (`desktop/aizome/crates/aizome-shell/src/shell.rs:1012`); the location update path does the same (`desktop/aizome/crates/aizome-shell/src/location.rs:85-87`) (confidence: medium). Separately, a shell save racing a Settei save on the shared temp file can publish truncated TOML (confidence: low).
- **Related:** `desktop/aizome/apps/settei/src/shared.rs:252` (`write_atomically`), `desktop/aizome/crates/aizome-shell/src/location.rs:85`, `desktop/aizome/crates/aizome-shell/src/shell.rs:385`/`:1273`, `desktop/aizome/crates/aizome-greeter/src/displays.rs:62`
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#syc-03)

<a id="syc-04"></a>

### SYC-04 — Settings file watcher thread
- **Where:** `desktop/aizome/crates/aizome-config/src/lib.rs:813` (`Watcher.stop`); thread-local `last` at `lib.rs:833`
- **Encoding:** `stop: Arc<AtomicBool>` plus the poll thread's `last: Option<(SystemTime, u64)>` (mtime and length; `None` while the file is missing)
- **States:** `Running(last)` → `Stopping` (`stop` set, thread still asleep) → `Exited`. The stamp moves between `Missing` and `Present(mtime, len)`.
- **Transitions:** `spawn_for` takes the initial stamp (`lib.rs:833`); every tick sleeps, re-stamps and fires `on_change` when the stamp differs (`lib.rs:835-840`); `Drop` sets `stop` (`lib.rs:850`); the loop exits at its next check (`lib.rs:834`)
- **Hazards:** the thread is detached and checks `stop` before sleeping, not after, so it can call `on_change` once more after the `Watcher` is dropped. It fires on the process's own saves. Two writes with the same length inside the filesystem's mtime granularity go unnoticed. A failed thread spawn panics (`lib.rs:843`).
- **Reify:** Low — small; consumers tolerate an extra callback
- **Related:** `desktop/aizome/crates/aizome-comp-core/src/lib.rs:198`, `desktop/aizome/crates/aizome-kit/src/app.rs:759`, `desktop/aizome/crates/aizome-shell/src/shell.rs:1124`, `desktop/aizome/apps/settei/src/app.rs:491`

<a id="syc-05"></a>

### SYC-05 — Persisted GeoClue location and its time-zone validity
- **Where:** `desktop/aizome/crates/aizome-config/src/location.rs:25` (`Location.latitude`, `.longitude`, `.zone`); `location.rs:215` (`system_timezone` 1 s cache `LAST`); `location.rs:276` (`zone_coordinates` cache)
- **Encoding:** the `[location]` table written by the shell, the current system time zone (re-read at most once a second), the `[night_light] latitude`/`longitude` override, and a per-zone coordinate cache that also stores `None`
- **States:** `Empty` → `Located(zone Z)`, valid while the system zone is Z → `Stale` (the zone changed, so the zone's city is used) → `Located` again when the shell re-locates. `Located(zone None)` counts as valid in every zone.
- **Transitions:** the shell writes `Location::found` (`desktop/aizome/crates/aizome-config/src/location.rs:43`, via `desktop/aizome/crates/aizome-shell/src/location.rs:42`); a time-zone change is noticed when `system_timezone` refreshes (`desktop/aizome/crates/aizome-config/src/location.rs:217-223`); `position_in` rejects a mismatched zone (`desktop/aizome/crates/aizome-config/src/location.rs:54-57`); a manual place short-circuits everything (`desktop/aizome/crates/aizome-config/src/location.rs:181-183`)
- **Hazards:** a position saved without a zone never goes stale. Latitude without longitude can be represented (and is ignored). `zone_coordinates` keeps a `None` for the process's lifetime (`desktop/aizome/crates/aizome-config/src/location.rs:286`).
- **Reify:** Low — small, and resolution is centralised in `place_in`
- **Related:** `desktop/aizome/crates/aizome-shell/src/location.rs:48` (`locating`, `located_at`, `AGAIN` retry window)

<a id="syc-06"></a>

### SYC-06 — Drawn element press, drag, drag-out and hover gesture
- **Where:** `desktop/aizome/crates/aizome-face/src/widgets/drawn.rs:268` (`PressState.from`, `.dragging`, `.over`); handler flags at `drawn.rs:242-246`
- **Encoding:** per-widget `PressState` in the kit's state map, plus the kit's pointer capture and `cx.is_pressed()`
- **States:** `Idle` (`from` None) → `Pressed` (`from` set, not dragging) → `Dragging` (sends `DragPhase`) or `DraggingOut` (the system's DnD) → `Idle`. Hover (`over`) is orthogonal.
- **Transitions:** left press sets `from`, clears `dragging` and captures when draggable (`drawn.rs:362-367`); a double click short-circuits (`drawn.rs:353-358`); moving 4pt starts the drag and emits `Start` (`drawn.rs:400-403`) or starts a DnD (`drawn.rs:387-394`); `Move` (`drawn.rs:404-406`); release takes the state, releases capture, and emits `End` or a click (`drawn.rs:411-431`); an abandoned press is reset (`drawn.rs:374-377`); hover enter and leave (`drawn.rs:378-383`, `drawn.rs:469-470`)
- **Hazards:** the abandoned-press reset clears the state without emitting `DragPhase::End` (and there is no Cancel phase), so consumers that set state on `Start` and clear it on `End` are left mid-drag. The test at `drawn.rs:615` checks only that no further phases arrive.
- **Reify:** High — consumers' drag state depends on an `End` that may never come (suspected bug)
- **Suspected bug:** during a desktop rubber-band drag, or a Deskbar app drag, the compositor takes the pointer before the release. When the pointer returns, Drawn resets silently at `drawn.rs:376`, so `desktop.band` stays `Some` and the band stays drawn (`desktop/aizome/crates/aizome-shell/src/desktop_glue.rs:225`/`:233`). For the Deskbar drag, `deskbar_visibility.dragging` stays set and holds the Deskbar open (`desktop/aizome/crates/aizome-shell/src/deskbar_glue.rs:371`/`:376`) (confidence: medium)
- **Related:** `desktop/aizome/crates/aizome-shell/src/desktop_glue.rs:225`, `desktop/aizome/crates/aizome-shell/src/deskbar_glue.rs:369`, `desktop/aizome/crates/aizome-shell/src/tearoff.rs:289`
- **Reproduction:** Partly confirmed (integration): [test and output](reproductions.md#syc-06)

<a id="syc-07"></a>

### SYC-07 — Text field caret, selection, IME preedit, blink and Caps Lock state
- **Where:** `desktop/aizome/crates/aizome-face/src/widgets/field.rs:99` (`FieldState.caret`, `.anchor`, `.seen`, `.preedit`, `.blink_off`, `.caps`)
- **Encoding:** per-widget `FieldState` against the program-owned `value`, plus kit focus, the text-input enable, and a tick timer
- **States:** Focused/Unfocused × blink On/Off × `Composing` (preedit non-empty)/`Idle` × selection/collapsed caret × `Synced` (`seen == value`)/`Diverged`. Caps is `Unknown` (`None`), `On` or `Off`.
- **Transitions:** `settle` moves the caret to the end when the program changed `value` (`field.rs:131-146`, called at `field.rs:202`/`:277`/`:420`); `replace` sets `seen` and emits `on_change` (`field.rs:153-167`); `Focus(on)` enables text input and restarts the blink (`field.rs:284-293`); `Tick` toggles the blink while focused (`field.rs:294-302`); IME `Preedit` sets (`field.rs:308-311`); `Commit` clears and inserts (`field.rs:303-306`); a key press updates caps and emits `on_caps` (`field.rs:314-323`)
- **Hazards:** the preedit is never cleared on focus loss, on a program value change, or through `reset_text_input`. In `secure` mode the preedit is drawn unmasked: `all = shown(value) + preedit` at `field.rs:223`, and the caret position at `field.rs:235` includes it. Text input is enabled with `ContentPurpose::Normal` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/textinput.rs:121`). If the program does not adopt an edit, `settle` jumps the caret to the end. Caps is learned only from key presses.
- **Reify:** High — password fields on the lock screen, greeter and polkit dialog (suspected bug)
- **Suspected bug:** with an input method composing (fcitx5 or ibus active), the characters being composed in the lock-screen password field (`desktop/aizome/crates/aizome-shell/src/lock.rs:239`) are drawn in clear text beside the dots until they are committed; the same holds for `desktop/aizome/crates/aizome-shell/src/auth.rs:131` and `desktop/aizome/crates/aizome-greeter/src/view.rs:245` (confidence: medium)
- **Related:** `desktop/aizome/sukumo/crates/sidestep-ui/src/window.rs:1001` (`set_text_input`), `:1014` (`reset_text_input`, never called by Field)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#syc-07)
- **Follow-up:** reified as typestate in [#298](https://github.com/bitemyapp/sekka/pull/298). Its suspected bug is fixed, and the mistake no longer compiles.

<a id="syc-08"></a>

### SYC-08 — (test harness) MPRIS test player playback state and pw-play child
- **Where:** `desktop/aizome/crates/aizome-sys/examples/test_player.rs:127` (`Player.playing`, `.stopped`, `.track`); `test_player.rs:61` (`Sound.child`)
- **Encoding:** two bools for `PlaybackStatus`, a track index, and at most one `pw-play` child
- **States:** `Playing` (child running) | `Paused` | `Stopped`. `playing && stopped` can be represented and is reported as Playing (`test_player.rs:188-192`).
- **Transitions:** `set` spawns or kills the child (`test_player.rs:139-151`); PlayPause/Play/Pause/Stop (`test_player.rs:163-175`); Next/Previous (`test_player.rs:153-158`, `:176-181`); Quit exits the process (`test_player.rs:94-96`)
- **Hazards:** the tone ends after 300 s while the status still says Playing. Pausing and resuming restarts the tone from the beginning. Quit leaves `pw-play` running and the temporary directory in place (`test_player.rs:248`). A spawn error leaves `playing` true with no child.
- **Reify:** Low — manual fixture
- **Related:** `desktop/aizome/crates/aizome-sys/src/mpris` (client under test)

<a id="syc-09"></a>

### SYC-09 — CUPS tool subprocess lifecycle in `run`
- **Where:** `desktop/aizome/crates/aizome-sys/src/printers/cli.rs:239` (`run`: `child`, pipe-reader threads and `receiver` at `cli.rs:268-288`, `started`/`pause` at `cli.rs:291-292`, `deadline` at `cli.rs:316`)
- **Encoding:** local variables: the child process in its own session, reader threads feeding a channel, a backoff interval, a timeout, an optional `Stop`
- **States:** `Refused` (already stopped) → `Spawning` (up to 20 ETXTBSY retries) → `Running` → `Exited` | `Cancelled` | `TimedOut` | `IoError` → `Draining` (≤ `PIPE_GRACE`) → `Done`
- **Transitions:** early cancel (`cli.rs:245-247`); spawn with retry (`cli.rs:266`, `cli.rs:330-341`); a reader-thread spawn failure kills the child (`cli.rs:283-286`); `try_wait` sees the exit (`cli.rs:295`); kill on error, stop or timeout (`cli.rs:297-310`); backoff from 1 to 20 ms (`cli.rs:311-312`); drain against the deadline (`cli.rs:317-324`)
- **Hazards:** on a normal exit the rest of the session is not killed. A descendant holding a pipe makes the drain give up after 2 s with that stream empty, and leaks the blocked reader thread. Reader threads are never joined on the kill paths. `stop` is seen only between polls. `kill(-pid)` relies on `setsid` having made the child a group leader (`cli.rs:261-263`).
- **Reify:** Medium — several exit paths with partial-output semantics
- **Related:** `desktop/aizome/crates/aizome-sys/src/service.rs` (`Stop`); `desktop/aizome/crates/aizome-sys/src/printers/mod.rs:639` (`act`)

<a id="syc-10"></a>

### SYC-10 — User's default printer in `~/.cups/lpoptions`
- **Where:** `desktop/aizome/crates/aizome-sys/src/printers/cli.rs:724` (`clear_user_default`); derivation at `cli.rs:484-490`; `Tools.env_default` at `cli.rs:77`
- **Encoding:** the `Default NAME` line in the lpoptions file (written by CUPS's `lpoptions -d`, rewritten by Aizome into `Dest`, `desktop/aizome/crates/aizome-sys/src/printers/parse.rs:436`), the temporary `lpoptions.aizome-new`, and `LPDEST`/`PRINTER`, captured once when `Tools` is built (`cli.rs:111-113`)
- **States:** `None` → `File(name)` → `None`; `Env(name)` overrides both
- **Transitions:** `set_user_default` runs `lpoptions -d` (`desktop/aizome/crates/aizome-sys/src/printers/mod.rs:705-707`); clearing writes the temp file and renames it (`cli.rs:735-742`), removing the temp file if the rename fails (`cli.rs:743-745`); every query re-reads the file (`cli.rs:484-487`)
- **Hazards:** the read-modify-write has no lock, so it races a concurrent `lpoptions` write. A failed write leaves a partial `.aizome-new`. With `LPDEST` set, clearing succeeds but has no effect. The environment value is a snapshot.
- **Reify:** Low

<a id="syc-11"></a>

### SYC-11 — Printers snapshot, poller and refresh turns
- **Where:** `desktop/aizome/crates/aizome-sys/src/printers/mod.rs:74` (`PrintersState.loaded`, `.available`, `.error`, lists); `mod.rs:556` (`Printers.stop`); `mod.rs:567` (`Shared.refreshing`)
- **Encoding:** the published `PrintersState` (`loaded`, `available`, `error`, kept lists), a poll thread governed by `Stop`, and a mutex that serialises refreshes
- **States:** `NotLoaded` → `Available` | `Unavailable` (lists cleared) | `Degraded` (error set; previous lists and `available` kept). Poller: `Running` → `Stopping` → `Exited`. If the first refresh fails with a non-Unavailable error, the state is `loaded && !available` with an error such as Timeout, which looks like "no CUPS".
- **Transitions:** poll loop (`mod.rs:598-609`); a thread-spawn failure sets `loaded` and an error (`mod.rs:611-616`); `Ok` → `set_if_changed` (`mod.rs:811-813`); an Unavailable error resets the state (`mod.rs:822`); other errors keep the previous state (`mod.rs:823-825`); a stop mid-query publishes nothing (`mod.rs:807-809`); each action refreshes after its work (`mod.rs:642-646`); `Drop` stops the poller (`mod.rs:798`)
- **Hazards:** refreshes are not coalesced: each action and each `refresh()` queues a full query on `refreshing`, so an action's `Task` completes only after any in-flight poll and its own query. Action threads keep running and publishing after `Drop`.
- **Reify:** Medium — the phase is spread over three fields with error-dependent merging
- **Related:** `desktop/aizome/crates/aizome-sys/src/service.rs` (`Model::set_if_changed`, `Task`, `Stop`); Settei's printers pane

<a id="syc-12"></a>

### SYC-12 — Streaming lpinfo driver parser and lpstat record parsers
- **Where:** `desktop/aizome/crates/aizome-sys/src/printers/parse.rs:508` (`DriverLines.pending`); also `parse.rs:91-92` (`printer_status`: `current`, `first_detail`), `parse.rs:254` and `parse.rs:456` (`current`)
- **Encoding:** `Option<record being built>`, plus a flag for "the first detail line is the state message"
- **States:** `NoRecord` → `Accumulating(record)` → emitted at the next header, at a margin line (lpstat) or at the end. `DriverLines` also has `Skipping` (a nameless `Model:` clears `pending`).
- **Transitions:** a named `Model:` replaces `pending` and yields the previous record (`parse.rs:516-519`, `:541-543`); a nameless `Model:` yields and then skips its details (`parse.rs:520-523`); indented `key = value` lines attach (`parse.rs:525-534`); short-form lines start a record (`parse.rs:535-540`); the end yields (`parse.rs:545`); lpstat headers and margin lines (`parse.rs:94-104`); first detail (`parse.rs:107-118`)
- **Hazards:** in `DriverLines` an unparsable margin line (for example a blank line) does not end `pending`, so later indented lines still attach to the earlier model, unlike `printer_status`, which ends a record at any margin line. The iterator state persists across `next()` calls.
- **Reify:** Low — local and well tested

<a id="syc-13"></a>

### SYC-13 — (test harness) Fake CUPS tool scripts with mutable canned cases
- **Where:** `desktop/aizome/crates/aizome-sys/src/printers/tests.rs:14` (`Fake.dir`); `tests.rs:61` (`tool`); `tests.rs:94` (`set`)
- **Encoding:** a generated shell script per tool, plus `{name}.{i}.out/err/status` files per case and append-only `{name}.calls`/`{name}.env` logs
- **States:** case table fixed at creation; each case's answer can change mid-test; the call log only grows; directory removed on `Drop`
- **Transitions:** `tool` writes the script and cases (`tests.rs:61-91`); `set` rewrites a case (`tests.rs:94-99`; used at `tests.rs:319`, `:382`, `:390`, `:414`); each run appends to the logs (`tests.rs:66-68`); `Drop` removes everything (`tests.rs:132-136`); `wait_for` polls (`tests.rs:217-223`)
- **Hazards:** `set` writes three files non-atomically while the poller may be running the script, so a run can see new output with the old status. Rewriting scripts causes the ETXTBSY that production `spawn` retries (`desktop/aizome/crates/aizome-sys/src/printers/cli.rs:328-341`).
- **Reify:** Low

<a id="syc-14"></a>

### SYC-14 — CPU usage delta sampler
- **Where:** `desktop/aizome/crates/aizome-sys/src/sysinfo/cpu.rs:163` (`CpuSampler.previous_total`, `.previous_cores`)
- **Encoding:** the previous `/proc/stat` counters, in total and per core
- **States:** `Fresh` (no previous; the first sample is the average since boot) → `Primed`. Each core is `Unknown` → `Known`, and is forgotten when it is absent.
- **Transitions:** total delta and re-baseline (`cpu.rs:205-214`); per-core delta and replacement (`cpu.rs:216-229`); an unreadable file returns zeros and leaves the state alone (`cpu.rs:191-197`)
- **Hazards:** the first sample means something different and callers must discard it. A core that comes back online reports its since-boot average once. Counters going backwards give 0 and then a new baseline.
- **Reify:** Low

<a id="syc-15"></a>

### SYC-15 — Disk and network throughput samplers
- **Where:** `desktop/aizome/crates/aizome-sys/src/sysinfo/io.rs:136` (`DiskIoSampler.previous`, `.classes`, `.last`); `io.rs:292` (`NetSampler.previous`, `.classes`, `.last`)
- **Encoding:** previous counters by name, a cached classification by name, and the previous sample's `Instant`
- **States:** `Fresh` (`last` None, zero rates) → `Primed`. Each device is `Unknown` → `Known` (counters plus cached class) → forgotten when absent.
- **Transitions:** set `last` (`io.rs:179-180`, `io.rs:335-336`); prune classes (`io.rs:181`, `io.rs:337`); classify once (`io.rs:185-189`, `io.rs:341-344`); compute rates (`io.rs:199-205`, `io.rs:353-356`); replace previous (`io.rs:212`, `io.rs:363`); a read failure leaves the state alone (`io.rs:171-174`, `io.rs:327-330`)
- **Hazards:** a device that keeps its name but changes role (a repartitioned disk, a re-parented interface) keeps its stale class until it disappears from the file. A zero-length interval gives no rates but still re-baselines the counters.
- **Reify:** Low

<a id="syc-16"></a>

### SYC-16 — Power meter idle floor and RAPL readings
- **Where:** `desktop/aizome/crates/aizome-sys/src/sysinfo/power.rs:188` (`Floor(Option<f64>)`); readings at `power.rs:33` (`RaplReading.at`); `joules` at `power.rs:147`
- **Encoding:** a running minimum per meter, kept by the caller's sampler. Readings carry the meter's `CLOCK_BOOTTIME` read time.
- **States:** `Unknown` (None) → `Floor(min)`, which only ever decreases and is never reset
- **Transitions:** `update` (`power.rs:191-195`)
- **Hazards:** one low reading pins the floor for the sampler's lifetime, for example a 0 J interval when a counter wraps with `range` 0 (`power.rs:152-154`); it has no decay after a suspend. The meter answers from a cache of up to 1 s (`power.rs:95-96`), so callers must skip readings with an unchanged `at` (myaku does: `desktop/aizome/apps/myaku/src/sampler.rs:237`).
- **Reify:** Low
- **Related:** `desktop/aizome/crates/aizome-power-meter/src/main.rs` (server-side cached reading); `desktop/aizome/apps/myaku/src/sampler.rs:123`; `desktop/tatami/activity/src/power.rs:139` (copy)

<a id="syc-17"></a>

### SYC-17 — Process sampler: per-pid cache validity and CPU interval
- **Where:** `desktop/aizome/crates/aizome-sys/src/sysinfo/process.rs:507` (`ProcessSampler.known`, `.users`, `.previous`, `.generation`, `.boot_time`); `Known` at `process.rs:488`
- **Encoding:** per pid, `Known { start_ticks, comm, cpu_ticks, seen, … }`; a generation counter for eviction; `previous: Option<(Instant, Option<u64>)>`; a lazily cached boot time; a uid→name cache
- **States:** sampler `Unprimed` (0% for everyone) → `Primed`. Per pid: `Unseen` → `Fresh` (younger than 5 s, details re-read every sample) → `Cached` → `Evicted`. The entry is invalidated on pid reuse (`start_ticks` changed) or on exec (`comm` changed).
- **Transitions:** interval computed and `previous` replaced (`process.rs:560-564`); `generation += 1` (`process.rs:565`); re-read when new, changed or young (`process.rs:624-630`); CPU from previous ticks or "new within the interval" (`process.rs:633-643`); update `cpu_ticks`/`seen` (`process.rs:645-646`); evict unseen pids (`process.rs:594-595`); cache the boot time on the first `btime` (`process.rs:609-612`)
- **Hazards:** exec is detected by `comm` alone, so execing a program with the same name keeps stale args and exe. A uid or cgroup change after 5 s is never seen. A transient `status` read failure drops the process for one sample and evicts its entry, so the next sample loses CPU continuity. The `users` cache is never refreshed. `Instant` (monotonic) and uptime (boottime) disagree across a suspend.
- **Reify:** Medium — validity rules spread over five conditions

<a id="syc-18"></a>

### SYC-18 — (test harness) Private PipeWire and pipewire-pulse
- **Where:** `desktop/aizome/crates/aizome-sys/tests/audio.rs:13` (`PrivatePipeWire.children`, `.dir`)
- **Encoding:** a list of child processes and a runtime directory
- **States:** `Configured` → `PipeWireUp` (socket exists) → `PulseUp` → `Dropped`; `Skipped` when the binaries are missing
- **Transitions:** spawn pipewire (`audio.rs:98-104`); wait for `pipewire-0` (`audio.rs:105-107`); spawn pulse (`audio.rs:108`); wait for `pulse.sock` (`audio.rs:109-111`); `Drop` kills the children in reverse order and removes the directory (`audio.rs:120-132`)
- **Hazards:** a failure before `pw` exists leaks the directory (`audio.rs:27-80`). Readiness means "the socket file exists", not "the daemon is ready".
- **Reify:** Low
- **Related:** `desktop/aizome/crates/aizome-sys/src/audio.rs`, `desktop/aizome/crates/aizome-sys/src/meter.rs`

<a id="syc-19"></a>

### SYC-19 — (test harness) Fake BlueZ: agent registration, discovery and pairing
- **Where:** `desktop/aizome/crates/aizome-sys/tests/bluetooth.rs:18` (`Shared.agent`, `.default_agent`, `.slow`); `Adapter` at `bluetooth.rs:53`; `Device` at `bluetooth.rs:131` (`paired`, `trusted`, `connected`)
- **Encoding:** shared mutex state, per-object properties, and "blocked" encoded as `alias == "blocked"` (`bluetooth.rs:77`)
- **States:** agent `Unregistered` → `Registered(sender, path)` (+ default); adapter powered and discovering; device `Unpaired` → `Pairing` (asking the agent) → `Paired` | rejected/cancelled; connected/disconnected
- **Transitions:** register, set default, unregister (`bluetooth.rs:32-50`); start and stop discovery, adding the FOUND device (`bluetooth.rs:96-111`); pair through `RequestConfirmation` (`bluetooth.rs:195-226`); connect and disconnect (`bluetooth.rs:228-238`)
- **Hazards:** `unregister_agent` does not clear `default_agent`, so the check at `bluetooth.rs:355` cannot catch a re-registration that skips `RequestDefaultAgent`. Discovery re-adds FOUND at a fixed path. `set_powered` ignores the blocked state.
- **Reify:** Low
- **Related:** `desktop/aizome/crates/aizome-sys/src/bluetooth` (agent and pairing client)

<a id="syc-20"></a>

### SYC-20 — (test harness) Private dbus-daemon per test and the `eventually` wait
- **Where:** `desktop/aizome/crates/aizome-sys/tests/common/mod.rs:14` (`TestBus.child`, `.address`, `.dir`); also a duplicate at `desktop/aizome/crates/aizome-sys/tests/location.rs:17`; `eventually` at `common/mod.rs:97`
- **Encoding:** a child process, its printed address, and a configuration directory
- **States:** `Spawned` → `Ready` (address read) → `Dropped` (killed, reaped, directory removed); `Skipped` (no dbus-daemon)
- **Transitions:** spawn (`common/mod.rs:51-63`); read the address (`common/mod.rs:64-67`); an empty address kills the daemon (`common/mod.rs:68-71`); `Drop` (`common/mod.rs:86-92`); `eventually` polls for 5 s and then checks once more (`common/mod.rs:97-106`)
- **Hazards:** if `stdout.take()` or `read_line` fails (`common/mod.rs:64-66`; `desktop/aizome/crates/aizome-sys/tests/location.rs:66`), the function returns `None` without killing the daemon or removing the directory, which orphans a dbus-daemon (dropping a std `Child` does not kill it)
- **Reify:** Low

<a id="syc-21"></a>

### SYC-21 — (test harness) Fake UDisks2 object lifecycles with fault injection
- **Where:** `desktop/aizome/crates/aizome-sys/tests/disks.rs:44` (`Log` with `loop_no_filesystem`, `loop_already_mounted`, `mount_error`, `autoclear_error`, `delete_error`); `FakeEncrypted.cleartext` (`disks.rs:389`); `FakeLoop.autoclear` (`disks.rs:437`); `FakeFs.mount_points` (`disks.rs:242`)
- **Encoding:** objects added to and removed from the fake's object server at fixed paths, plus fault flags shared with the test
- **States:** `loop0`: `Absent` → `Attached` (with or without a filesystem, possibly already mounted) → `Mounted` → `Autoclear` → `Deleted`. LUKS: `Locked` → `Unlocked` (a `dm_2d0` object) → `Locked`. The format job lives for 400 ms. `sdb2`: `Absent` → `Created` → `Deleted`.
- **Transitions:** `loop_setup` (`disks.rs:77-105`); `set_autoclear` (`disks.rs:453-460`); loop `delete` (`disks.rs:461-475`); mount and unmount (`disks.rs:260-287`); unlock and lock (`disks.rs:400-434`); format with a job (`disks.rs:216-239`); `create_partition` (`disks.rs:318-340`); partition `delete` (`disks.rs:373-386`)
- **Hazards:** fixed object paths: a second setup, unlock or create before cleanup gets `Ok(false)` from `server.at` and the old object stays. `Format(dos)` leaves `FakeTable` unchanged.
- **Reify:** Low
- **Related:** `desktop/aizome/crates/aizome-sys/src/disks`

<a id="syc-22"></a>

### SYC-22 — (test harness) Real UDisks loop-device lifetime driver (VM only)
- **Where:** `desktop/aizome/crates/aizome-sys/tests/disks_real.rs:10` (`fixture`, guarded by the marker file `/etc/aizome-loop-test-vm` at `disks_real.rs:11`); `wait` at `disks_real.rs:19`; `attached` at `disks_real.rs:46`
- **Encoding:** backing image files under `/var/tmp`, real loop devices observed through the `Disks` snapshot, and the VM marker file
- **States:** per image: `Created` → `Attached` (manual: no autoclear; mounted: autoclear and read-only) → `Detached` (by explicit delete, or by autoclear on unmount). A failed mount releases only its own loop.
- **Transitions:** `loop_setup` (`disks_real.rs:62`); `loop_delete` (`disks_real.rs:93`); `mount_image` (`disks_real.rs:119`); `unmount` (`disks_real.rs:138`); failure cases (`disks_real.rs:153-168`)
- **Hazards:** fixed 2 s sleeps stand in for persistence checks (`disks_real.rs:64`, `:121`); images are never removed
- **Reify:** Low
- **Related:** `desktop/aizome/nix/vm-loop-images-test.nix`; `desktop/aizome/crates/aizome-sys/src/disks`

<a id="syc-23"></a>

### SYC-23 — (test harness) Fake GeoClue2 client with call gates and stall injection
- **Where:** `desktop/aizome/crates/aizome-sys/tests/location.rs:132` (`Client.running`, `.desktop_id`, `.accuracy`, `.start_gate`, `.stop_gate`); `CallGate` at `location.rs:145`; `Manager.deleted` at `location.rs:115`; `Slow` at `location.rs:99`
- **Encoding:** client properties, rendezvous channels that hold a service call open, and a stall selector
- **States:** client `Idle` → `Started` (`running`) → `Stopped`. The Location property reads "/" unless running and not `NoPosition`. DeleteClient only increments a counter.
- **Transitions:** `start` (`location.rs:159-170`); `stop` (`location.rs:172-177`); `set_desktop_id` (`location.rs:193-198`); `location` (`location.rs:180-185`); `get_client`/`delete_client` (`location.rs:120-129`); `CallGate::wait` (`location.rs:151-154`)
- **Hazards:** GetClient always returns the same object and DeleteClient does not remove it, so `running` carries over between requests within a test. A gate never released panics inside the handler after 15 s.
- **Reify:** Low
- **Related:** `desktop/aizome/crates/aizome-sys/src/location.rs` (`locate_on_cancellable`); `desktop/aizome/crates/aizome-shell/src/location.rs`

<a id="syc-24"></a>

### SYC-24 — (test harness) Fake MPRIS players
- **Where:** `desktop/aizome/crates/aizome-sys/tests/mpris.rs:78` (`Player.status`, `.track`, `.position`, `.volume`, `.shuffle`, `.loop_status`); `Calls` at `mpris.rs:28`
- **Encoding:** a status string and playback fields, plus flags and a method log
- **States:** `Playing` | `Paused` | `Stopped`; track 0–2; position
- **Transitions:** PlayPause/Play/Pause/Stop (`mpris.rs:118-135`); Next/Previous (`mpris.rs:136-149`); Seek/SetPosition (`mpris.rs:150-168`); Quit only sets a flag, and the player leaves when the test drops its connection (`mpris.rs:44-46`, `:450`)
- **Hazards:** the position never advances with time; Stop resets the position but not the track
- **Reify:** Low
- **Related:** `desktop/aizome/crates/aizome-sys/src/mpris`

<a id="syc-25"></a>

### SYC-25 — (test harness) Fake NetworkManager activation lifecycle
- **Where:** `desktop/aizome/crates/aizome-sys/tests/network.rs:43` (`Manager.active`, `.next`, `.wireless_enabled`); `Device.reason` at `network.rs:256`; `Connection.settings` at `network.rs:209`
- **Encoding:** a list of active connection paths, a shared counter, the device state reason, and per-connection settings
- **States:** connection `Saved` → `Active` (state 2) | `Failed` (no Active object; device reason 7, NO_SECRETS); `Active` → `Deactivated`; `Saved` → `Deleted`; settings updated with flags 1 (to disk) or 2 (in memory)
- **Transitions:** `activate` (`network.rs:64-93`); `activate_connection` (`network.rs:131-142`); `add_and_activate_connection2` (`network.rs:144-166`); `deactivate_connection` (`network.rs:168-178`); `delete` (`network.rs:223-232`); `update2` (`network.rs:234-245`); `save` (`network.rs:247-249`)
- **Hazards:** `reason` is set to 7 and never reset, so later successful activations still report NO_SECRETS. A failed activation returns a path that never exists. `get_connection_by_uuid` scans hard-coded paths (`network.rs:195`).
- **Reify:** Low
- **Related:** `desktop/aizome/crates/aizome-sys/src/network`

<a id="syc-26"></a>

### SYC-26 — (test harness) Fake polkitd authority, PAM helper and scripted answering UI
- **Where:** `desktop/aizome/crates/aizome-sys/tests/polkit.rs:19` (`Registrations.owner`, `.failures`, `.lost_replies`, `.unregister_failures`, counters); helper script at `polkit.rs:117`; socket helper thread at `polkit.rs:310`; `answering_ui` at `polkit.rs:170` (`answers`, `current`)
- **Encoding:** a single-owner registration with countdown fault injectors; a line protocol in the helper; and a UI that consumes answers in order and keeps one current request
- **States:** authority `NoAgent` → `Registered(owner)` → `NoAgent`, with the injectors `Failing(n)`, `LostReply(n)` (registers but replies after 3 s) and `UnregisterFailing(n)`. Helper: cookie → info → prompt → password → `SUCCESS` | `FAILURE`. UI: Begin → answer, wait or cancel; Failed → authenticate again.
- **Transitions:** register (`polkit.rs:37-71`); unregister (`polkit.rs:72-89`); script lines (`polkit.rs:123-125`); socket conversations (`polkit.rs:312-338`); UI events (`polkit.rs:175-204`)
- **Hazards:** `current` holds a single request, so overlapping BeginAuthentication calls would answer the wrong one. The lost-reply delay occupies the fake's executor for 3 s.
- **Reify:** Low
- **Related:** `desktop/aizome/crates/aizome-sys/src/polkit.rs` (agent registration worker and helper conversation)

<a id="syc-27"></a>

### SYC-27 — (test harness) Fake logind inhibitors, sleep signals and backlight; fake profile holds
- **Where:** `desktop/aizome/crates/aizome-sys/tests/power.rs:272` (`LogindLog.inhibits`, `.inhibitor_reads`, `.brightness`); `Profiles.holds` at `power.rs:195`; `Kbd.level` at `power.rs:105`
- **Encoding:** the read ends of inhibitor pipes kept by the fake (released when the client closes its write end), a list of holds, and brightness mirrored into a fake sysfs
- **States:** inhibitor `Held` → `Released` (a read returns 0, `power.rs:384-389`); PrepareForSleep true/false emitted by the test; holds by cookie
- **Transitions:** `inhibit` (`power.rs:317-324`); emit PrepareForSleep (`power.rs:418-427`); session `set_brightness` writes sysfs (`power.rs:362-365`); `hold_profile`/`release_profile` (`power.rs:228-235`); `set_active_profile` (`power.rs:205-211`)
- **Hazards:** the hold cookie is `holds.len() + 1`, so after a release it can collide with a live hold. Read ends are kept until the test ends.
- **Reify:** Low
- **Related:** `desktop/aizome/crates/aizome-sys/src/power` (logind sleep delay, inhibitors, backlight)

<a id="syc-28"></a>

### SYC-28 — (test harness) Fake AccountsService user creation and deletion
- **Where:** `desktop/aizome/crates/aizome-sys/tests/users.rs:22` (`Manager.users`); `FakeUser` at `users.rs:68`
- **Encoding:** a cached-users list plus `User*` objects on the object server
- **States:** user object `Absent` → `Created` (always `User1002`) → `Deleted`
- **Transitions:** `create_user` (`users.rs:31-45`); `delete_user` (`users.rs:47-60`); setters (`users.rs:108-125`)
- **Hazards:** a second CreateUser reuses `User1002` (`server.at` returns `Ok(false)`), yet still pushes the path and emits UserAdded, so the list and the object server can diverge
- **Reify:** Low
- **Related:** `desktop/aizome/crates/aizome-sys/src/users.rs`

## Already explicit

- `desktop/aizome/crates/aizome-sys/src/printers/mod.rs:241` `enum JobState` — a print job's IPP state, recomputed on every poll in one place, `parse::job_state` (`desktop/aizome/crates/aizome-sys/src/printers/parse.rs:315`); `PrinterState` (`desktop/aizome/crates/aizome-sys/src/printers/mod.rs:200`) is likewise set only in `status_line` (`desktop/aizome/crates/aizome-sys/src/printers/parse.rs:160`)
- `desktop/aizome/crates/aizome-sys/src/sysinfo/process.rs:61` `enum ProcessState` — the kernel's process state letter, mapped in one place, `ProcessState::from_char` (`desktop/aizome/crates/aizome-sys/src/sysinfo/process.rs:87`)

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/crates/aizome-config/src/compositor.rs` | 348 | none |  | extra-key accessors and Night Light schedule math; pure |
| `desktop/aizome/crates/aizome-config/src/greeter.rs` | 109 | none |  | public display-scale filter; pure conversion |
| `desktop/aizome/crates/aizome-config/src/lib.rs` | 1097 | machines | [SYC-01](#syc-01), [SYC-02](#syc-02), [SYC-03](#syc-03), [SYC-04](#syc-04) | env keyboard defaults, legacy key, load/save, watcher |
| `desktop/aizome/crates/aizome-config/src/location.rs` | 400 | machines | [SYC-05](#syc-05) | persisted location validity; tz TTL cache |
| `desktop/aizome/crates/aizome-config/src/shell.rs` | 587 | none |  | typed section views and merge-write; quiet hours pure |
| `desktop/aizome/crates/aizome-config/tests/keyboard_defaults.rs` | 74 | none |  | tests; subprocess per env case (see SYC-01) |
| `desktop/aizome/crates/aizome-face/src/lib.rs` | 6 | none |  | module list |
| `desktop/aizome/crates/aizome-face/src/look.rs` | 185 | none |  | drawing helpers; pure |
| `desktop/aizome/crates/aizome-face/src/wallpaper.rs` | 178 | none |  | decode and cover scaling; ready state lives in shell |
| `desktop/aizome/crates/aizome-face/src/widgets/drawn.rs` | 635 | machines | [SYC-06](#syc-06) | press/drag/drag-out/hover gesture |
| `desktop/aizome/crates/aizome-face/src/widgets/field.rs` | 487 | machines | [SYC-07](#syc-07) | caret, selection, IME preedit, blink, caps |
| `desktop/aizome/crates/aizome-face/src/widgets/keys.rs` | 63 | none |  | stateless key-to-message wrapper |
| `desktop/aizome/crates/aizome-face/src/widgets/mod.rs` | 6 | none |  | module list |
| `desktop/aizome/crates/aizome-sys/examples/probe.rs` | 640 | none |  | read-only diagnostic; bounded settle poll |
| `desktop/aizome/crates/aizome-sys/examples/status-items.rs` | 59 | none |  | diagnostic; fixed wait then snapshot |
| `desktop/aizome/crates/aizome-sys/examples/test_player.rs` | 267 | machines | [SYC-08](#syc-08) | manual MPRIS fixture with pw-play child |
| `desktop/aizome/crates/aizome-sys/src/printers/cli.rs` | 748 | machines | [SYC-09](#syc-09), [SYC-10](#syc-10) | subprocess run lifecycle; lpoptions user default |
| `desktop/aizome/crates/aizome-sys/src/printers/mod.rs` | 832 | machines | [SYC-11](#syc-11) | snapshot and poller; JobState/PrinterState explicit |
| `desktop/aizome/crates/aizome-sys/src/printers/parse.rs` | 1203 | machines | [SYC-12](#syc-12) | streaming driver parser; record accumulators |
| `desktop/aizome/crates/aizome-sys/src/printers/tests.rs` | 967 | machines | [SYC-13](#syc-13) | fake CUPS tool scripts |
| `desktop/aizome/crates/aizome-sys/src/sysinfo/cpu.rs` | 362 | machines | [SYC-14](#syc-14) | delta sampler, first sample since boot |
| `desktop/aizome/crates/aizome-sys/src/sysinfo/detail.rs` | 297 | none |  | stateless /proc readers; buffer reuse only |
| `desktop/aizome/crates/aizome-sys/src/sysinfo/dmi.rs` | 395 | none |  | DMI read and naming; pure |
| `desktop/aizome/crates/aizome-sys/src/sysinfo/drm.rs` | 256 | none |  | fdinfo parse; deltas take caller-held readings |
| `desktop/aizome/crates/aizome-sys/src/sysinfo/gpu.rs` | 416 | none |  | one-shot card enumeration and pci.ids lookup |
| `desktop/aizome/crates/aizome-sys/src/sysinfo/hardware.rs` | 497 | none |  | one-shot hardware summary; local cpuinfo parser |
| `desktop/aizome/crates/aizome-sys/src/sysinfo/io.rs` | 553 | machines | [SYC-15](#syc-15) | disk and network delta samplers |
| `desktop/aizome/crates/aizome-sys/src/sysinfo/load.rs` | 116 | none |  | loadavg/uptime/btime parsers; pure |
| `desktop/aizome/crates/aizome-sys/src/sysinfo/memory.rs` | 172 | none |  | meminfo parser; pure |
| `desktop/aizome/crates/aizome-sys/src/sysinfo/mod.rs` | 304 | none |  | OnceLock constants; TestTree fixture builder |
| `desktop/aizome/crates/aizome-sys/src/sysinfo/os_release.rs` | 243 | none |  | os-release parser; pure |
| `desktop/aizome/crates/aizome-sys/src/sysinfo/power.rs` | 344 | machines | [SYC-16](#syc-16) | idle floor ratchet; RAPL readings |
| `desktop/aizome/crates/aizome-sys/src/sysinfo/process.rs` | 1148 | machines | [SYC-17](#syc-17) | per-pid cache validity; ProcessState explicit |
| `desktop/aizome/crates/aizome-sys/src/sysinfo/sensors.rs` | 305 | none |  | hwmon read and CPU sensor choice; pure |
| `desktop/aizome/crates/aizome-sys/src/sysinfo/signal.rs` | 285 | none |  | one-shot pidfd pin, uid/start check, signal |
| `desktop/aizome/crates/aizome-sys/tests/apps.rs` | 78 | none |  | tests; fake app logs calls |
| `desktop/aizome/crates/aizome-sys/tests/audio.rs` | 255 | machines | [SYC-18](#syc-18) | private PipeWire harness |
| `desktop/aizome/crates/aizome-sys/tests/bluetooth.rs` | 434 | machines | [SYC-19](#syc-19) | fake BlueZ |
| `desktop/aizome/crates/aizome-sys/tests/common/mod.rs` | 122 | machines | [SYC-20](#syc-20) | private dbus-daemon and eventually |
| `desktop/aizome/crates/aizome-sys/tests/datetime.rs` | 311 | none |  | tests; fakes with settable properties |
| `desktop/aizome/crates/aizome-sys/tests/disks.rs` | 801 | machines | [SYC-21](#syc-21) | fake UDisks2 with fault flags |
| `desktop/aizome/crates/aizome-sys/tests/disks_encrypted_real.rs` | 50 | none |  | tests; VM-marker guard and bounded poll |
| `desktop/aizome/crates/aizome-sys/tests/disks_real.rs` | 180 | machines | [SYC-22](#syc-22) | real UDisks loop driver, VM only |
| `desktop/aizome/crates/aizome-sys/tests/disks_visibility.rs` | 104 | none |  | tests; pure visibility assertions |
| `desktop/aizome/crates/aizome-sys/tests/display_scales.rs` | 59 | none |  | tests; fake property |
| `desktop/aizome/crates/aizome-sys/tests/live.rs` | 260 | none |  | tests; read-only live probes |
| `desktop/aizome/crates/aizome-sys/tests/location.rs` | 403 | machines | [SYC-23](#syc-23), [SYC-20](#syc-20) | fake GeoClue; duplicate TestBus |
| `desktop/aizome/crates/aizome-sys/tests/mpris.rs` | 618 | machines | [SYC-24](#syc-24) | fake MPRIS players |
| `desktop/aizome/crates/aizome-sys/tests/network.rs` | 782 | machines | [SYC-25](#syc-25) | fake NetworkManager |
| `desktop/aizome/crates/aizome-sys/tests/notifications.rs` | 502 | none |  | tests; asserts real server lifecycle |
| `desktop/aizome/crates/aizome-sys/tests/polkit.rs` | 509 | machines | [SYC-26](#syc-26) | fake polkitd, helper, answering UI |
| `desktop/aizome/crates/aizome-sys/tests/power.rs` | 521 | machines | [SYC-27](#syc-27) | fake logind inhibitors and profile holds |
| `desktop/aizome/crates/aizome-sys/tests/statusnotifier.rs` | 285 | none |  | tests; fake items with mutable icon/status |
| `desktop/aizome/crates/aizome-sys/tests/users.rs` | 207 | machines | [SYC-28](#syc-28) | fake AccountsService |
| `desktop/aizome/crates/aizome-theme/src/lib.rs` | 486 | none |  | design tokens; data tables |
