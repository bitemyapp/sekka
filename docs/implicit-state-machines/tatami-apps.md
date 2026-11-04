# TAP: Tatami settings apps

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/tatami/{activity,network,bluetooth,sound,displays}`. Audited at `8e0acbe5`; line numbers refer to that commit.

50 files, 0 pending. 33 implicit machines: 9 High, 11 Medium, 13 Low.

## Machines

<a id="tap-01"></a>

### TAP-01 — Activity inspector's subscription to the selected row, with a layout shape fixed at selection
- **Where:** `desktop/tatami/activity/src/inspector.rs:80` (`Inspector.current`, `.rows` :79, `.gpus` :83); stack pages "empty"/"details" (`inspector.rs:187-189`)
- **Encoding:** `current: Option<(Entry, SignalHandlerId)>`; the stack's visible child; row closures built for a shape decided once in `build`: `single` (a process or a one-process group, :293), the GPU list (:292), the parent PID (:539)
- **States:** `Empty` → `Showing(entry, handler, shape)`. Invalid but representable: shape `single` while the entry now has several processes (or the reverse); GPU rows for a GPU list captured at build
- **Transitions:** `show(None)` → Empty (`inspector.rs:225-230`); `show(Some)` disconnects the old handler, builds, connects `changed`, shows "details" (:224-241); each reading `update(.., true)` (:234-237, :588)
- **Hazards:** the layout is rebuilt only when the selection changes. An application selected while it had one process keeps the per-process rows (Priority, Command, Started, per-GPU) after it starts a second process. Those rows then read `d.process` = None and show "—" or hide, and no Processes or Windows rows appear. The Parent row keeps the PPID captured at build (:539-576)
- **Reify:** Low — one Option and a page; the shape could be part of the state and rebuilt when it changes
- **Related:** `desktop/tatami/activity/src/entry.rs:170` (`update_group` clears `process` for groups of several processes)

<a id="tap-02"></a>

### TAP-02 — Activity sampler thread lifecycle and schedule
- **Where:** `desktop/tatami/activity/src/sampler.rs:34` (`spawn`: locals `interval`, `next`; `Sampler.lens`, `.focus` :109-110); also `desktop/tatami/activity/src/main.rs:92`
- **Encoding:** a named thread; an mpsc receiver of `Command::{Interval, Focus}`; a deadline `next: Instant`; a bounded frame channel (capacity 2 in the window, `desktop/tatami/activity/src/window.rs:281`; 1 for `--print`)
- **States:** `Baseline` (first `sample()` not sent, :41) → `Waiting(next)` → `Sampling` → `Sending` (may block) → `Waiting`; `Exited` when the command sender disconnects (:57) or a frame send fails (:60)
- **Transitions:** first frame at +350 ms (`sampler.rs:43`); `Interval` brings the deadline forward (:46-49); `Focus` replaces lens and PIDs (:51-54); timeout → sample, send, `next += interval`, resync when late (:59-67); `--print` throws away the first frame (`desktop/tatami/activity/src/main.rs:93-94`)
- **Hazards:** `send_blocking` blocks the sampler while the window doesn't drain frames, which also delays `Interval`/`Focus`. `Focus` doesn't trigger a reading, so a new selection's memory maps and folder wait for the next tick. The window's thread-local keeps the command sender, so the thread ends only with the process
- **Reify:** Low — a small, local loop

<a id="tap-03"></a>

### TAP-03 — Activity sampler's baselines, caches and energy integrals between readings
- **Where:** `desktop/tatami/activity/src/sampler.rs:114-132` (`Sampler.last`, `.stat`, `.previous`, `.cache`, `.smaps`, `.drm`, `.clients`, `.rapl_before`, `.gpu_energy_before`, `.cpu_floor`, `.gpu_floors`, `.energy`, `.total_energy`, `.disks_before`, `.disk_totals`); also `desktop/tatami/activity/src/nvml.rs:159` (`Nvml.last_seen`), `desktop/tatami/activity/src/power.rs:139` (`Floor`)
- **Encoding:** per-PID maps validated by process start time; tick arithmetic that gates refreshes (`tick % 30`, `% 60`, `(tick + pid) % 5`, `% 10`, the `named`/`grouped` stamps); the previous counters behind every rate; a running minimum per power meter; accumulated (joules, seconds) per process or group key
- **States:** per baseline `None` (first reading, rates 0) → `Have(previous)`; per cache `Fresh` | `Stale(by tick)` | `Invalid(start changed)`; per meter floor `Unset` → `Min(watts)`
- **Transitions:** `last`/`tick` (`sampler.rs:230-231`); launcher entries and passwd reloaded (:233-240); `stat` (:247); dead PIDs pruned (:265-268); per-process baseline validated and replaced (:578, :595); maps due or stale (:608-619); names and cgroup refreshed (:628, :665, :675); DRM fds rescanned (:698); DRM clients replaced each reading (:838); GPU energy counters (:318); RAPL (:866-879); floors (:895, :909; `power.rs:142-145`); energy integrals (:385, :942, :949); group energy pruned (:405); disks (:520-540); NVML sample cursor (`nvml.rs:246-248`)
- **Hazards:** a failed RAPL read keeps the older `rapl_before` (:877-879), so the next reading divides two intervals' joules by one `dt` (:866-874). One zero reading pins `cpu_floor` at 0 for the rest of the session (`power.rs:142-145`), which changes every later per-process share. A group's energy is keyed by its first process (:375), so it restarts from zero when that process exits. `disks_before` is never pruned. NVML is never shut down
- **Reify:** High — fifteen fields with independent validity rules, plus a user-visible bug
- **Suspected bug:** With "Update Every Second", the processor counters come from the power meter service, which returns its cached text when asked within 1 s of its last fresh read (`desktop/tatami/tool/src/power_meter.rs:100-101`). The power read happens after the per-process scan, whose length varies, so requests regularly arrive just under 1 s after the last fresh read. That reading's delta is 0 (0 W), the next is about twice the real power, and the first 0 W reading pins the idle floor at 0 for the session. A service read that times out doubles the next reading the same way (confidence: high)
- **Related:** `desktop/tatami/tool/src/power_meter.rs:61` (`serve`, its 1 s cache `last`)
- **Reproduction:** Confirmed (unit): [test and output](reproductions.md#tap-03)

<a id="tap-04"></a>

### TAP-04 — Activity process table: columns per view and remembered sorts
- **Where:** `desktop/tatami/activity/src/table.rs:304` (`Table.lens`), `.sorts` (:306), `.swapping` (:308), `.mode` (:312), `Filter.mode` (:268); closure-local `last` (:568)
- **Encoding:** the current view; a per-view (column, order) map; a reentrancy flag; the last primary column the sorter handler saw; a deferred idle re-sort; the grouping mode held twice (`Table.mode`, `Filter.mode`)
- **States:** `Settled(lens, sort)` → `Swapping` (columns removed and re-added) → `Settled`; on a header click `UserSorted` → (numeric, newly primary, ascending) `FlipPending` (idle) → `Settled(descending)`
- **Transitions:** `set_lens` (`table.rs:663-691`); the sorter's `changed` handler ignores changes while swapping (:574), records `sorts` (:584-588), schedules the flip (:589-598); `set_root` sets the mode, refilters, keeps or resets the selection (:640-659); `resort` keeps a list scrolled to the top at the top (:694-718); `refilter` expands top-level rows while searching (:733-753)
- **Hazards:** `last` isn't updated while `swapping`, so at start-up and after every view switch it doesn't name the actual primary column. `Table.mode` and `Filter.mode` must agree; cells capture the mode at bind (:484)
- **Reify:** High — four cooperating fields plus a deferred idle action, with a reproducible bug
- **Suspected bug:** At start-up `last` is None, and `set_lens` sorts the CPU view by % CPU descending while `swapping` is set. The person clicks "% CPU" to sort ascending. GTK toggles it to ascending, the handler sees `changed` (None ≠ % CPU) and schedules a flip back to descending (:589-597), so the click is undone and only a second click reverses the sort. The same happens to the first click on the restored sort column after each view switch, whenever `last` names a different column. Clicking the column `last` does name leaves it ascending: smallest first, against the rule (confidence: high)
- **Reproduction:** Confirmed (unit): [test and output](reproductions.md#tap-04)

<a id="tap-05"></a>

### TAP-05 — Activity table cells bound to a row's `changed` signal
- **Where:** `desktop/tatami/activity/src/table.rs:322` (`HANDLER` object data); `keep` (:324), `release` (:333)
- **Encoding:** `(Entry, SignalHandlerId)` stored on the cell widget with `set_data`/`steal_data`
- **States:** `Unbound` → `Bound(entry, handler)` → `Unbound`; a rebind without an unbind releases first (:326)
- **Transitions:** bind (`table.rs:498`, :509); unbind (:512-521)
- **Hazards:** the unsafe typed object data relies on one writer type; none observed otherwise
- **Reify:** Low — small and centralised

<a id="tap-06"></a>

### TAP-06 — Window singleton, deferred first presentation and launch requests held until the first reading (Activity, Network, Sound)
- **Where:** `desktop/tatami/activity/src/window.rs:65` (`UI`), `Ui.presented` (:58), `State.pending` (:43), `State.pids` (:40); also `desktop/tatami/network/src/window.rs:99` (`UI`), :93 (`presented`), `State.loaded`/`.pending_join` (:31, :37); `desktop/tatami/sound/src/window.rs:147` (`UI`), :125 (`presented`)
- **Encoding:** a thread-local `Option<Rc<Ui>>`; a once flag for `present`; a one-shot timeout (600 ms in Activity, 250 ms in Network and Sound); a request (Activity: a PID and whether exact; Network: an SSID) consumed after a reading
- **States:** `NotBuilt` → `Built(hidden, request?)` → `Presented` (by the timeout or the first reading); request `Pending` → `Consumed`. The slot is never cleared; the process ends with the window
- **Transitions:** Activity: first launch builds and stores (`desktop/tatami/activity/src/window.rs:103-115`), the first frame takes `pending` and presents (:324-330), a second launch acts at once (:90-101). Network: first launch (`desktop/tatami/network/src/window.rs:104-117`); the first reading sets `loaded`, presents and takes `pending_join` (:376-391); `join_by_name` queues until loaded (:419-423). Sound: first launch (`desktop/tatami/sound/src/window.rs:152-164`); a reading presents (:401)
- **Hazards:** Activity's second launch acts on `state.pids` even before the first frame. A second launch presents the window before its first reading while `presented` stays false, so the first reading presents it again
- **Reify:** High — the same wait-until-loaded shape copied three times, one copy without the guard
- **Suspected bug:** `tatami-activity --pid N`, run while the window exists but before its first frame (within about half a second of the first launch), calls `select_pid` with an empty `pids` map (`desktop/tatami/activity/src/window.rs:97-98`, :441-444). It toasts "Process N has ended" for a live process and selects nothing, where Network guards the same case with `loaded` (`desktop/tatami/network/src/window.rs:420-422`) (confidence: medium)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#tap-06)

<a id="tap-07"></a>

### TAP-07 — Activity's view and grouping mode, held in seven places
- **Where:** `desktop/tatami/activity/src/window.rs:31-32` (`State.lens`, `.mode`); also `desktop/tatami/activity/src/table.rs:304`, :312, :268; the `interval`, `mode`, `everyone` and `kernel` stateful actions (`window.rs:836-892`); `Sampler.lens`/`.focus` (`desktop/tatami/activity/src/sampler.rs:109-110`)
- **Encoding:** the ToggleGroups (the source); `State`; `Table`; `Filter`; the `mode` action's state; the summary stack's page; the sampler thread (through `Command::Focus`); the update interval in the action state and in the sampler; a tree store kept in sync only in Tree mode
- **States:** lens ∈ {Cpu, Memory, Energy, Gpu, Disk} × mode ∈ {Apps, All, Tree} × (everyone, kernel) × interval
- **Transitions:** lens toggle handler (`window.rs:335-348`); mode toggle handler sets the state and the action, syncs the tree, sets the root (:350-375); the `mode` action routes to the toggle (:862-866); the interval action sets its state and tells the sampler (:842-852); filter toggles (:872-890); `focus` sends the lens and the selection's members (:421-429)
- **Hazards:** the tree store goes stale outside Tree mode and is resynced on entry (:363-365, :549-552). `Focus` sends the members as they were at selection, so processes the application starts later get their memory maps read only every fifth reading. Action states are written only in their own handlers, so any other path that set the toggles would leave them out of step
- **Reify:** Medium — one writer per handler, but seven copies, one on another thread

<a id="tap-08"></a>

### TAP-08 — Activity's Quit, Force Quit and Pause on a target taken from the latest frame
- **Where:** `desktop/tatami/activity/src/window.rs:696` (`confirm_quit`: `target` moved into the response closure); also `target` (:621), `update_actions` (:660)
- **Encoding:** a `Target` (PIDs, first processes, mine, paused, session), computed from the selection and the latest frame; the dialog's lifetime; `paused` derived from the kernel's state 'T'
- **States:** `Idle` → `Confirming(target)` → `Signalled` | `Cancelled`; per selection `Running` ⇄ `Paused`, observed only at the next frame
- **Transitions:** dialog opened (`window.rs:697-745`); response sends SIGTERM to the first processes or SIGKILL to all (:735-743); Pause (SIGSTOP) and Resume (SIGCONT) act at once (:762-785); actions enabled from `target` (:660-681)
- **Hazards:** when the dialog is answered, the PIDs are not checked again against their start time or key. The Pause/Resume button changes only after the next frame (up to 5 s), so it can be pressed twice
- **Reify:** High — signals go to PIDs captured an unbounded time earlier
- **Suspected bug:** The person opens "Quit “X”?" and leaves it open. X exits on its own and another of their processes reuses its PID. Choosing Force Quit then SIGKILLs the unrelated process (:741), because `target.all` holds bare PIDs from the frame shown when the dialog opened (confidence: low)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#tap-08)

<a id="tap-09"></a>

### TAP-09 — Bluetooth pairing: the Pair call, the Agent1 prompt and cancellation
- **Where:** `desktop/tatami/bluetooth/src/agent.rs:29` (`Prompt.invocation` :32, `.finished` :34); also `desktop/tatami/bluetooth/src/window.rs:24-25` (`Ui.pairing`, `.prompt`), :45 (`pairing_cancelled`), :40 (`busy`)
- **Encoding:** the device being paired (`pairing: Option<String>`); the open prompt with its pending D-Bus invocation and a `finished` latch; a cancel flag checked between pairing and connecting; `busy`
- **States:** `Idle` → `Pairing(path)` (Pair call, 120 s) → `Prompting(method, invocation?)` → `Pairing` → `Paired` → `Connecting` (30 s) → `Done` | `Failed` | `Cancelled`; display-only prompts hold no invocation
- **Transitions:** `act(Pair)` sets `pairing` and clears the flag (`window.rs:632-635`); agent calls are accepted only from BlueZ's owner and for `pairing` (`agent.rs:63-93`); `show` closes any older prompt and stores the new one (`agent.rs:119`, :199-211); the answer returns or rejects the invocation, clears `prompt`, and cancels pairing unless it was "pair" (`agent.rs:215-247`); `finish` returns Canceled, latches and force-closes (`agent.rs:38-44`); BlueZ `Cancel`/`Release` (`agent.rs:72-81`); Pair returns: prompt closed, `pairing` cleared, Connect skipped when cancelled (`window.rs:645-660`), cleanup (:673-676); `cancel_pairing` (`window.rs:692-708`); service vanishes (`window.rs:123-125`); window close sends CancelPairing (`window.rs:311-314`)
- **Hazards:** `finished` must be set before `force_close`, because the close emits the "cancel" response, which would cancel a later Pair. The 3 s poll's `render` re-enables Cancel Pairing (`window.rs:528`) after `cancel_pairing` disabled it (:698), so a second CancelPairing can fail and toast an error for a cancellation that worked. `pairing_cancelled` is reset only when the next Pair starts
- **Reify:** Medium — latched and tested, but spread over two files and five fields
- **Related:** BlueZ `org.bluez.Device1.Pair`/`CancelPairing` and `org.bluez.Agent1` (`desktop/tatami/bluetooth/src/bluez.rs:108`)

<a id="tap-10"></a>

### TAP-10 — BlueZ backend session tied to the service's bus owner, with agent registration
- **Where:** `desktop/tatami/bluetooth/src/bluez.rs:16` (`Bluez.owner`, `.agent_ready`, `.stopped`); also `desktop/tatami/bluetooth/src/window.rs:22` (`Ui.backend`), :99-133 (`watch`), :330 (`current`)
- **Encoding:** `backend: Option<Rc<Bluez>>`, replaced for each name owner; `current` compares pointers; `agent_ready` is set by RegisterAgent and cleared by Release or stop
- **States:** `NoService` → `Present(owner, agent: Unregistered → Registered)` → `Stopping` → `Stopped`; `Present` → `NoService` when the name vanishes; each new owner gets a new session
- **Transitions:** name appeared (`window.rs:111-118`); vanished clears backend, pairing, prompt, snapshot and busy (`window.rs:119-131`); registration during a reading when not ready (`window.rs:360-367`; `bluez.rs:74-84`); Release (`desktop/tatami/bluetooth/src/agent.rs:72-79`); stop (`bluez.rs:194-209`)
- **Hazards:** the agent object is exported once per connection (`agent.rs:47`) while registration follows sessions. The vanished handler clears `busy` while the old action's future is still pending; `current` makes its result ignored
- **Reify:** Medium — the session is mostly an object; the agent's state is a loose bool

<a id="tap-11"></a>

### TAP-11 — Bluetooth discovery session owned by this connection
- **Where:** `desktop/tatami/bluetooth/src/bluez.rs:21` (`Bluez.scan: Option<String>`)
- **Encoding:** the adapter this window started discovery on, set only after StartDiscovery succeeds and cleared by `observe` when a snapshot shows that adapter not discovering
- **States:** `Idle` → (`Starting`, not represented) → `Scanning(adapter)` → (`Stopping`, not represented) → `Idle`; `Scanning` → `Idle` when BlueZ reports it lost (power off, adapter gone)
- **Transitions:** `observe` (`bluez.rs:141-151`); `scan` stops the previous session, restoring it on unexpected errors (:157-176), and starts a new one unless `stopped` (:177-189); `stop` (:194-196). Serialised by `Ui.reading`: called only from `refresh` and, on close, after `reading` drains (`desktop/tatami/bluetooth/src/window.rs:372`, :316-319)
- **Hazards:** `observe` forgets the session, without StopDiscovery, whenever a snapshot shows `Discovering` false. If BlueZ reported that while still counting this client (for example while it suspends discovery for pairing), the next StartDiscovery could fail as already in progress, and every later one too, since nothing takes the session back (not verified against BlueZ). `stopped` is checked only before SetDiscoveryFilter
- **Reify:** Medium — Starting and Stopping exist only as awaits, and correctness depends on serialisation outside the type

<a id="tap-12"></a>

### TAP-12 — (test harness) Fake BlueZ on a private dbus-daemon
- **Where:** `desktop/tatami/bluetooth/src/ui_test.rs:96` (`Fake.pending`, `.registered`, `.agent`, `.mode`, `.reject_set`, `.reject_connect`); also `ui_test.rs:18` (`Bus`, killed on Drop), `desktop/tatami/bluetooth/data/test-bus.conf`, `desktop/tatami/bluetooth/package.nix:42-46`
- **Encoding:** a mutable object tree; one pending Pair invocation; an agent-registered flag; the agent method to script; polling waits
- **States:** daemon `Spawned` → `Owned(org.bluez)` ⇄ `Unowned` → `Killed`; Pair `Pending` → `Asked agent` → `Paired` | `Rejected` | `Canceled`
- **Transitions:** daemon spawned and its address read (`ui_test.rs:318-322`); name owned (:338); Pair stores the invocation and asks the agent (:215-245); CancelPairing fails the pending one (:246-254); name released and owned again (:490, :494); `wait_until` polls every 10 ms for up to 8 s (:289-298); `loaded` waits for `!reading && !busy` (:299-301)
- **Hazards:** `loaded` can pass between a finished reading and the next queued one; one `pending` slot, so concurrent Pairs would overwrite each other
- **Reify:** Low — test only

<a id="tap-13"></a>

### TAP-13 — Bluetooth readings and actions serialised with a revision counter
- **Where:** `desktop/tatami/bluetooth/src/window.rs:40-44` (`Ui.busy`, `.reading`, `.updating`, `.closing`, `.revision`); also :23 (`snapshot`), :38 (`selected`)
- **Encoding:** one reading at a time (`reading`); one action at a time (`busy`); a revision bumped at the start and the end of every action, so overlapping readings are discarded and repeated; `updating` silences widget handlers during `render`; a 3 s poll
- **States:** `Idle` → `Reading(rev)` → `Rendered` | `Discarded → Reading`; `Acting(action)`, overlapping readings; `Failed` (snapshot emptied, Try Again shown)
- **Transitions:** poll (`window.rs:79-85`); `refresh` guard and start (:339-347), result accepted when the revision is unchanged (:353-391), an error empties the snapshot (:393-401), rerun when the revision or backend changed (:405-408); `act` guard, busy, revision bump (:623-637), completion clears busy, bumps the revision, refreshes (:670-689); `render` under `updating` (:413, :547); the power switch and adapter handlers skip while updating (:252, :266)
- **Hazards:** the error branch doesn't check `revision`. While busy the status line isn't updated (:376). An action refused because `busy` is set is dropped without a word; the forget confirmation's response is one such path (:851-859). The snapshot is stored before the awaited register and scan (:355)
- **Reify:** High — five flags and a counter across six mutation sites, with invariants (no action against a stale snapshot) kept by convention
- **Related:** BlueZ ObjectManager (`desktop/tatami/bluetooth/src/bluez.rs:58`)

<a id="tap-14"></a>

### TAP-14 — Bluetooth device settings dialog
- **Where:** `desktop/tatami/bluetooth/src/window.rs:39` (`Ui.detail: Option<Rc<Detail>>`)
- **Encoding:** the open dialog and the device path it belongs to
- **States:** `Closed` → `Open(path)` → `Closed` (its `closed` signal, or force-closed when the device vanishes)
- **Transitions:** `detail` builds and stores it (`window.rs:717-788`, :862); `closed` clears it (:790-794); `render` refreshes it or force-closes it (:529-546); its switches act unless `updating` (:816-833)
- **Hazards:** the Forget confirmation uses the device captured when the dialog opened (:840-859). A second `detail` would replace the slot and the first dialog's `closed` would clear the second's (not reachable while the dialog covers the window)
- **Reify:** Low

<a id="tap-15"></a>

### TAP-15 — Bluetooth window close: hide, drain, release, destroy
- **Where:** `desktop/tatami/bluetooth/src/window.rs:43` (`Ui.closing`), :67 (`UI`), :294-326 (close handler)
- **Encoding:** the `closing` latch; an application hold; a future that cancels pairing, polls `reading` every 20 ms, stops discovery and the agent, destroys the window and clears `UI`
- **States:** `Open` → `Closing(hidden, draining)` → `Destroyed`
- **Transitions:** first close latches, hides, closes the prompt, sets `stopped`, holds the application (`window.rs:298-308`); drain (:309-324); later closes are refused (:298-300). `closing` gates refresh (:340), actions (:624), the poll (:80) and new service owners (:112)
- **Hazards:** `open` presents whatever `UI` holds (:71-73), including a window that is closing. The drain has no overall deadline beyond each call's timeout (CancelPairing 30 s, each stop call 5 s)
- **Reify:** High — a latch plus a hidden but live window that `open` can reach
- **Suspected bug:** Relaunching Bluetooth while the close drain runs (for example while CancelPairing waits on a slow adapter) presents the hidden window again (:71-72). Every control is dead, because `closing` gates refresh and actions, and when the drain finishes it destroys the window under the person (:321-323) (confidence: low)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#tap-15)

<a id="tap-16"></a>

### TAP-16 — Displays arrangement tile drag
- **Where:** `desktop/tatami/displays/src/arrangement.rs:31` (`Inner.drag_start`)
- **Encoding:** one shared `Option<(f64, f64)>` start position for whichever tile is dragged; the gesture's offset; cursor names
- **States:** `Idle` → `Dragging(start)` → `Clicked` (under 4 px: select) | `Dropped` (layout position reported)
- **Transitions:** begin (`arrangement.rs:129-135`); update moves the widget (:136-144); end (:147-162); `set_tiles` removes and rebuilds every tile (:82-100) on each `render` (`desktop/tatami/displays/src/window.rs:524`)
- **Hazards:** `set_tiles` neither clears `drag_start` nor cancels a drag. A `render` mid-drag (the refresh 800 ms after an apply, or one after a Hyprland event) removes the tile being dragged. The gesture's end then reports a drop through the new layout factor, or an update moves a widget that is no longer a child of `fixed`
- **Reify:** Medium — small, but it races asynchronous re-renders

<a id="tap-17"></a>

### TAP-17 — A display's use (main, extended, mirror, off) and the main display
- **Where:** `desktop/tatami/displays/src/model.rs:528` (`enum Use`), `use_as` (:539); the state is in `Settings.enabled`, `Settings.mirror` and `main: Option<String>`
- **Encoding:** partially explicit. `Use` is only an argument: the state is `enabled` + `mirror` for each display plus the global `main`, which the helper keeps in `displays.json` and which `main_display` derives again (:627), as does the window (`desktop/tatami/displays/src/window.rs:656-664`)
- **States:** `Off` | `Extended` | `Main` | `Mirror(source)`. Invalid but representable: `enabled == false` with a mirror; a mirror of a display that is off or itself mirroring; `main` naming a display not in the arrangement
- **Transitions:** `use_as` (`model.rs:539-603`): enabled and mirror set (:546-561), main moved (:562-572), mirrors of a display leaving the arrangement dropped (:573-579), a display coming on placed beside the others (:580-594), main repaired (:596-601); `main_display` falls back from kept to workspace 1 to the first one on (:627-649); the window offers only valid sources (`window.rs:639-655`)
- **Hazards:** the helper's kept `main` can name a display that is off; it is repaired only where it is read
- **Reify:** Low — transitions centralised in `use_as` and tested
- **Related:** `desktop/tatami/tool/src/displays.rs:315` (writes `displays.json`), `desktop/tatami/tool/src/displays.rs:417` (`apply_command`)

<a id="tap-18"></a>

### TAP-18 — Displays' Hyprland event-socket watcher thread
- **Where:** `desktop/tatami/displays/src/system.rs:208` (`watch`)
- **Encoding:** a detached thread reading `.socket2.sock` lines and sending `()` for each relevant event
- **States:** `NotStarted` (no Hyprland environment) → `Connected` → `Exited` (EOF, error, or the receiver dropped)
- **Transitions:** environment check (`system.rs:209-214`); connect (:216-219); read loop (:220-234)
- **Hazards:** no reconnect. After a Hyprland restart or a socket error the window silently stops following monitors and the lid
- **Reify:** Low

<a id="tap-19"></a>

### TAP-19 — Displays readings and applies (concurrent changes refused, stale readings discarded)
- **Where:** `desktop/tatami/displays/src/window.rs:73-75` (`Ui.applying`, `.refreshing`, `.revision`); `State` (:53-64)
- **Encoding:** one apply at a time; one refresh at a time; a revision bumped for each apply so an overlapping refresh is thrown away; widgets insensitive while applying; optimistic `assume` until Hyprland reports
- **States:** `Idle` → `Refreshing(rev)` → `Rendered` | `Discarded`; `Applying` → `Assumed` → (+800 ms) `Refreshing`; a slider's `Applying` (`quietly`) → `Assumed`, with no refresh
- **Transitions:** `refresh` returns early while applying or refreshing (`window.rs:195-197`), reads on a thread (:200-211), discards when applying or the revision changed (:218-221), stores and selects (:222-238); each Hyprland event refreshes after 300 ms (:173-180); `apply_async` refuses while applying, with a toast, bumps the revision and disables the widgets (:251-258), then re-enables them (:269-273); `commit` assumes, rereads the kept settings, schedules a refresh, asks to keep (:350-379); `quietly` only assumes (:1057-1073)
- **Hazards:** a refresh asked for while one runs, or while applying, is dropped rather than queued; a discarded refresh isn't repeated; only `commit` schedules a refresh afterwards. The refresh thread runs `tatami displays lid` and `tatami nightlight status` with no timeout (`desktop/tatami/displays/src/system.rs:164-172`), so a hung helper leaves `refreshing` set and drops every later refresh. A change refused as "still in progress" leaves its control showing the new value until the next render
- **Reify:** High — three flags and a counter, drop-not-queue semantics, user-visible staleness
- **Suspected bug:** A monitor is plugged in while an SDR brightness or saturation change applies (`quietly`, which schedules no refresh). The `monitoradded` refresh 300 ms later finds `applying` set and returns (:195), or the refresh already running is discarded by the revision check (:219). Nothing reads again, so the window doesn't show the new display until another Hyprland event. A second event that arrives while a refresh runs is likewise dropped, and that reading may predate it (confidence: medium)
- **Related:** `desktop/tatami/tool/src/displays.rs:417` (`tatami displays apply`, run by `desktop/tatami/displays/src/system.rs:83`)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#tap-19)

<a id="tap-20"></a>

### TAP-20 — Keep-or-revert countdown after a risky display change
- **Where:** `desktop/tatami/displays/src/window.rs:391` (`ask_to_keep`: locals `settled`, `left`; `CONFIRM_SECONDS` :22)
- **Encoding:** a `settled` latch shared by the dialog's response, a 1 s timer and the revert closure; the seconds left; the settings from before the change
- **States:** `Asking(n)` → `Kept` | `Reverting` (button, close, or timeout) → `Reverted` | `RevertRefused`
- **Transitions:** "keep" settles (`window.rs:447-450`); any other response reverts (:451); the timer counts down, force-closes and reverts at 0 (:432-446); revert latches and commits the earlier settings of the displays that changed (:406-426); the close response is "revert" (:404)
- **Hazards:** the revert goes through `apply_async`, which refuses while another apply runs ("still in progress"). The latch is already set, so a refused revert is never retried. The countdown lives only in this process: if the app exits during it (killed, or closed during the revert's own apply), nothing reverts the change
- **Reify:** Medium — latched and local, but when it fails a display may stay blank
- **Related:** `desktop/tatami/tool/src/display_recovery.rs` (the helper's own recovery)

<a id="tap-21"></a>

### TAP-21 — Night Shift requests on unordered threads
- **Where:** `desktop/tatami/displays/src/window.rs:63` (`State.night`), `night_shift_group` (:1096); also `desktop/tatami/displays/src/system.rs:188` (`set_night_shift`)
- **Encoding:** the window's belief (tint in use, night temperature); the switch; a 300 ms debounce for the slider; each request a detached thread running `tatami nightlight on|off|K`
- **States:** `Off` → `TurningOn` (hyprsunset starting, up to about 4 s) → `On(K)` → `Off`; the window's belief vs the actual tint
- **Transitions:** the slider sets the state and debounces a temperature request (`window.rs:1128-1136`); the switch sets the state and requests on or off at once (:1139-1147); a refresh reads the status (`system.rs:171-172`)
- **Hazards:** requests are neither ordered nor serialised. "on" waits for hyprsunset and retries for seconds, while "off" returns at once when nothing is tinted yet (`desktop/tatami/tool/src/toggle.rs:156-157`, :99-120). Turning the switch off doesn't cancel a pending slider debounce, and a temperature request turns the tint on (`toggle.rs:158-164`)
- **Reify:** High — concurrent unordered writers, a user-visible wrong state
- **Suspected bug:** Night Shift is turned on and then off within a second or two. The "off" thread finds nothing tinted yet and does nothing, then the "on" thread finishes tinting, so the screen stays warm with the switch off. Likewise, dragging the temperature and turning the switch off within 300 ms: the debounced temperature request runs after "off" and turns the tint back on (confidence: high)
- **Related:** `desktop/tatami/tool/src/toggle.rs:147` (`nightlight_command`)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#tap-21)

<a id="tap-22"></a>

### TAP-22 — Displays' debounced slider writes
- **Where:** `desktop/tatami/displays/src/window.rs:1077` (`debounce`); used for brightness at 40 ms (:849-855), SDR brightness and saturation at 250 ms (:988-999), the Night Shift temperature at 300 ms (:1126-1136)
- **Encoding:** `Rc<RefCell<Option<SourceId>>>` for each slider
- **States:** `Idle` → `Pending(timer)` → `Fired` → `Idle`; each new value replaces the timer
- **Transitions:** replace (`window.rs:1082-1090`); fire, clear and run (:1086-1089)
- **Hazards:** a `render` rebuilds the sliders while a timer is pending. The old timer still fires, and the new slider shows the state's older value until the next render (SDR values are assumed without a render)
- **Reify:** Low — a small, nearly explicit helper

<a id="tap-23"></a>

### TAP-23 — Network connection details page: the open page, its settings and their version
- **Where:** `desktop/tatami/network/src/details.rs:30` (`Page.settings`), `.updating` (:33); also `desktop/tatami/network/src/window.rs:94-95` (`Ui.details`, `.saved_list`), :312-325 (`connect_popped`)
- **Encoding:** the open page in `Ui.details`; the Saved networks page in `saved_list`; `settings: Option<Settings>` carrying NetworkManager's VersionId; `updating` silencing handlers during `build`
- **States:** `Closed` → `Open(uuid, settings: Loading → Loaded(version))` → `Applying` → `Reloading` → `Loaded`; `Open` → `Closed` when popped, or popped automatically when the connection disappears
- **Transitions:** `open` (`details.rs:45-90`); `update` after each reading, popping a forgotten connection (:149-158); `load` (:222-238); `apply` with the loaded version, settings forgotten on error, then reloaded (:242-269); `build` under `updating` (:271-295); an IPv6 error reloads (:632); forgetting pops (:686-702); a pop clears the slot for its tag (`window.rs:312-325`)
- **Hazards:** two quick DNS or IPv6 changes: the second uses the version loaded before the first applied, and the profile update refuses it (`desktop/tatami/network/src/profile.rs:110`). Forgetting can pop twice when a reading has already popped the page (:154-155, :690), taking the Saved networks page with it. A details page opened from another (Join on an inactive profile whose SSID is in use, `desktop/tatami/network/src/join.rs:51-53`) replaces `Ui.details`, and popping the top page clears it, so the page underneath stops updating
- **Reify:** Medium — the page is an object, but `Ui.details` and the version are kept by convention
- **Related:** NetworkManager `Settings.Connection` `VersionId`/`Update2` (`desktop/tatami/network/src/profile.rs:75`)

<a id="tap-24"></a>

### TAP-24 — IPv6 DNS servers suspended in the NetworkManager profile while IPv6 is off
- **Where:** `desktop/tatami/network/src/dns.rs:7` (`BACKUP_KEY` in the profile's `user.data`); also `desktop/tatami/network/src/profile.rs:16` (`Profile.version` :19), `desktop/tatami/network/src/model.rs:585` (`Settings.suspended_dns`), `desktop/tatami/network/src/nm.rs:343` (`modify`, which routes DNS changes)
- **Encoding:** persisted in the profile as `org.sekka.tatami.ipv6-dns=v1|manual4|v4|manual6|v6`, together with `ipv6.method`, `ipv{4,6}.dns` and `ignore-auto-dns`; written with Update2 guarded by `version-id`
- **States:** `Ipv6On(dns)` → `Ipv6Off(backup v1)` → `Ipv6On(restored)`; `Ipv6Off(no backup)` (older profiles); `Ipv6Off(stale or malformed backup)` → refused until DNS is set again
- **Transitions:** turning off backs up and clears the IPv6 DNS (`dns.rs:89-106`), refused for DNS that isn't plain addresses (:92-96); turning on validates, restores and drops the backup (:107-125); a DNS edit while off replaces or removes the backup (:64-85); validation (:21-50); display (`model.rs:628-636`); update with a version check and Update2 (`profile.rs:105-167`); reading with a version recheck (`nm.rs:135-154`)
- **Hazards:** a backup left behind while IPv6 is on (an outside edit) is removed only at the next DNS edit. The version the page loaded must still be current (TAP-23). `Update2` asks for interactive authorization, with a 30 s timeout (`profile.rs:37-38`), on a worker thread
- **Reify:** Medium — persisted, versioned and carefully validated; the phase is a string format in another program's store
- **Related:** NetworkManager `Settings.Connection.Update2` with `version-id`

<a id="tap-25"></a>

### TAP-25 — Joining a Wi-Fi network: one at a time, with a password retry and rollback
- **Where:** `desktop/tatami/network/src/join.rs:40` (`busy`, over `State.joining` at `desktop/tatami/network/src/window.rs:33`); also `desktop/tatami/network/src/nm.rs:162` (`Secret`), `nm.rs:262` (`join`), `nm.rs:331` (`rejoin`)
- **Encoding:** `joining: Option<String>` while a join runs; open password or form dialogs (not represented); a retry closure for a refused saved password; a passwd-file removed on Drop; a new profile deleted on failure
- **States:** `Idle` → `Asking` (dialog, untracked) → `Joining(ssid)` → `Connected` | `Failed` | `Refused → Asking(retry) → Joining`; a new profile `Added` → `Activating` (45 s) → `Kept` | `Deleted`
- **Transitions:** entry points refuse while joining (`join.rs:57`, :145, :152); `run` checks again, sets `joining`, redraws (:200-205); completion clears it, retries when refused, toasts, reads (:207-221); the secret is written with mode 0600 (`nm.rs:167-192`) and removed on drop (`nm.rs:199-203`); add, then up, delete on failure (`nm.rs:323-326`)
- **Hazards:** dialogs are open while `joining` is None, so a second dialog can stack, and its answer is dropped with "Still joining". `refused` counts any "Timeout" as a refused password (`desktop/tatami/network/src/model.rs:641-646`), so a slow DHCP asks for the password again. Neither the worker thread nor nmcli is cancelled when the window closes
- **Reify:** High — the asking phase is untracked, and cleanup depends on the worker finishing
- **Suspected bug:** Closing the window (Escape) while a new network is being joined ends the app, since GTK quits when its last window goes. The worker thread dies before `nmcli --wait 45 connection up` returns. `Secret`'s Drop never removes the plaintext passwd-file from `$XDG_RUNTIME_DIR` (`nm.rs:199-203`), and a failed join's new profile is never deleted (`nm.rs:324-326`), so a saved network with the mistyped password stays behind (confidence: medium)
- **Related:** NetworkManager through nmcli; `desktop/tatami/network/src/main.rs:58`
- **Reproduction:** Confirmed (end-to-end): [test and output](reproductions.md#tap-25)

<a id="tap-26"></a>

### TAP-26 — Network's `nmcli monitor` child process
- **Where:** `desktop/tatami/network/src/nm.rs:371` (`MONITOR: Mutex<Option<Child>>`)
- **Encoding:** the child process in a static; a reader thread calling back for each line
- **States:** `NotStarted` → `Running` → `Ended` (EOF) | `Killed` (shutdown)
- **Transitions:** spawn and store (`nm.rs:375-390`); the reader thread calls back for each line (:391-395); `stop_monitor` kills it at application shutdown (:398-405; `desktop/tatami/network/src/main.rs:58`)
- **Hazards:** no restart. If `nmcli monitor` exits, the window falls back to the 4 s poll without saying so; a failed spawn is silent too
- **Reify:** Low

<a id="tap-27"></a>

### TAP-27 — Network readings: one at a time with one queued, change bursts and the poll
- **Where:** `desktop/tatami/network/src/window.rs:91-92` (`Ui.reading`, `.read_again`); also `State.snapshot`/`.loaded` (:30-31), `updating` (:90)
- **Encoding:** one reading at a time with at most one more queued; a 150 ms debounce of monitor bursts; a 4 s poll while visible; `updating` around the Wi-Fi switch
- **States:** `Idle` → `Reading` → `Reading + Again` → `Reading` → `Idle`
- **Transitions:** `read` (`window.rs:369-396`); monitor bursts (:329-343); poll (:345-353); the switch brought in line under `updating` (:465-467)
- **Hazards:** snapshots carry no revision, so a reading started before a change can show the old state until the queued reading. Failed nmcli calls become empty fields (`desktop/tatami/network/src/nm.rs:47`), shown as nothing
- **Reify:** Low — standard coalescing, centralised in `read`

<a id="tap-28"></a>

### TAP-28 — Network's Wi-Fi scan with a fixed six-second spinner
- **Where:** `desktop/tatami/network/src/window.rs:399` (`scan`; the state is the header stack's visible child)
- **Encoding:** the `gtk::Stack` showing "button" or "spinner"
- **States:** `Idle(button)` → `Scanning(spinner, 6 s)` → `Idle`, then a reading
- **Transitions:** start (`window.rs:400-403`); the worker asks for a rescan and sleeps (:405-409); end (:410-413); every 30 s while the window is active (:355-363); after Wi-Fi turns on (:303-305)
- **Hazards:** a widget holds the machine's state, and the spinner doesn't follow the actual scan
- **Reify:** Low

<a id="tap-29"></a>

### TAP-29 — Network's Wi-Fi radio and VPN switches, dispatched without serialisation
- **Where:** `desktop/tatami/network/src/window.rs:35` (`State.busy: Vec<String>`); also the Wi-Fi switch handler (:290-308), `set_vpn` (:760-784), the details page's Connect/Disconnect (`desktop/tatami/network/src/details.rs:93-145`)
- **Encoding:** the UUIDs of VPNs being switched (their switches insensitive while listed); radio requests on independent worker threads; each switch's own state until the next reading
- **States:** per VPN `Idle` → `Switching` → `Idle`; radio `Requested(on | off)`, possibly several at once
- **Transitions:** push (`window.rs:761`), remove on completion (:773), read (:781); the row's switch (:750-753), insensitive while busy (:742); the radio worker, then a reading and a scan (:296-307)
- **Hazards:** `set_vpn` doesn't redraw, so the switch stays sensitive until the next reading. A second toggle pushes the UUID again, and the first completion's `retain` removes both while the second still runs. A quick Wi-Fi off and on runs two `nmcli radio wifi` processes in either order, so the radio can end in the earlier request's state (the switch then follows the reading)
- **Reify:** Medium — several writers, guarded only after a re-render

<a id="tap-30"></a>

### TAP-30 — Sound's `pactl subscribe` with a restart loop
- **Where:** `desktop/tatami/sound/src/audio.rs:166` (`MONITOR: Mutex<(Option<Child>, bool)>`)
- **Encoding:** the running child and a stopped flag in a static; a thread that loops
- **States:** `Spawning` → `Subscribed` → `Ended` → `Waiting(2 s)` → `Spawning`; `Stopped` (the flag) at shutdown
- **Transitions:** loop and stop check (`audio.rs:189-196`); spawn and store (:197-211); read at once and on each shown line (:213-221); reap (:222-226); report and wait (:227-228); `stop_monitor` (:233-241; `desktop/tatami/sound/src/main.rs:78`)
- **Hazards:** while the service is down, each 2 s iteration reports twice (:215, :227), and each report starts a failing reading. After the window is gone the thread runs until shutdown sets the flag
- **Reify:** Low — small and centralised
- **Related:** pipewire-pulse

<a id="tap-31"></a>

### TAP-31 — Sound's input level meter: the libpulse thread and its follower
- **Where:** `desktop/tatami/sound/src/meter.rs:28` (`Meter.level`, `.stop`, `.ended`); also `desktop/tatami/sound/src/window.rs:128-130` (`Ui.meter: Option<(String, Meter, Instant)>`, `.level`, `.shown_level`)
- **Encoding:** atomics shared with the thread; on the window's side the followed input's name, the start time and the level bar
- **States:** `Connecting` → `Ready` → `Recording` → `Ended(error)` | `Stopped`; window side `None` → `Following(name, started)` → `Restarting` (another input, or ended and 10 s old)
- **Transitions:** start (`meter.rs:36-58`); connect and wait for Ready (:99-113); record with PEAK_DETECT and DONT_MOVE (:158-162); loop until stopped, ending on context or stream failure (:165-181); Drop sets `stop` (:73-76); `follow_input` after each shown reading (`window.rs:847-865`, called at :418); closing the window drops it (`window.rs:295`); drawn with decay (`window.rs:829-844`)
- **Hazards:** while connecting, the thread blocks in `iterate(true)` (`meter.rs:105`) and notices `stop` only after the next event, so an unresponsive server can leave abandoned threads. Restarts happen only after a shown reading, and readings are discarded while changes wait
- **Reify:** Medium — one lifecycle split across a thread and the window
- **Related:** pipewire-pulse (libpulse)

<a id="tap-32"></a>

### TAP-32 — Sound's serial change queue and reading scheduler
- **Where:** `desktop/tatami/sound/src/window.rs:120-124` (`Ui.busy`, `.stale`, `.pending`); also `.snapshot` (:115), `.updating` (:118); `desktop/tatami/sound/src/audio.rs:109` (`Action::replaces`)
- **Encoding:** one worker job at a time; a FIFO of actions, a slider's earlier values coalesced; a stale flag; readings shown only when no change waits; `updating` silencing handlers during render; each command run under `timeout 4` with a kill after 1 s (`audio.rs:116-131`)
- **States:** `Idle` → `Applying(action)` → `Idle(stale)` → `Reading` → `Rendered` | `Discarded` (changes waiting) | `Disconnected` (queue dropped, banner shown, content insensitive)
- **Transitions:** `queue` ignores calls during render, coalesces and pumps (`window.rs:342-352`); `pump` (:355-404): apply, then mark stale and pump (:365-378); read (:380-381); show only when nothing is pending (:389-392); an error clears the queue (:394-399); service events and the poll mark stale (:313, :323); render under `updating` (:407, :417)
- **Hazards:** a failed reading drops every queued change. `snapshot()` runs six pactl calls in sequence (`audio.rs:137-150`), so a hung service holds `busy` for seconds per attempt. A move is two pw-metadata runs (`audio.rs:153-158`), and a failure between them leaves `target.object` set without `target.node`. Coalescing can move a slider's value after later mute or port actions
- **Reify:** Medium — partially explicit (the queue and the discard rule are clear), but four flags share the invariant
- **Related:** pipewire-pulse through pactl; PipeWire's default metadata through pw-metadata

<a id="tap-33"></a>

### TAP-33 — Section rows cached by a content key and updated by bindings (Sound, Network)
- **Where:** `desktop/tatami/sound/src/window.rs:44` (`Section.rows`, `.bindings`, `.key`); also `desktop/tatami/network/src/window.rs:41` (`Section`, without bindings)
- **Encoding:** the key of what the rows show; rows rebuilt only when it changes, otherwise brought in line by bindings under `updating`
- **States:** `Hidden` → `Shown(key)` ⇄ `Rebuilt(key')`
- **Transitions:** `show` (`desktop/tatami/sound/src/window.rs:66-80`; `desktop/tatami/network/src/window.rs:59-72`); `hide`/`clear` (sound :83-94); `update` (sound :96-100)
- **Hazards:** a rebuild while a slider is dragged (an application appears in the list) destroys the control under the pointer; the cache is only as good as the key is complete
- **Reify:** Low

## Already explicit

- None in this slice.

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/tatami/activity/package.nix` | 47 | none |  | build recipe |
| `desktop/tatami/activity/src/apps.rs` | 888 | none |  | pure grouping and naming; bounded one-shot Hyprland requests |
| `desktop/tatami/activity/src/entry.rs` | 254 | none |  | row data, bounded history, lazy children store |
| `desktop/tatami/activity/src/gpu.rs` | 288 | none |  | sysfs and fdinfo parsing |
| `desktop/tatami/activity/src/graph.rs` | 363 | none |  | drawing widgets over ring buffers |
| `desktop/tatami/activity/src/inspector.rs` | 653 | machines | [TAP-01](#tap-01) | selected-row subscription with fixed layout shape |
| `desktop/tatami/activity/src/main.rs` | 220 | machines | [TAP-02](#tap-02) | print() drives one sampler and drops its first frame |
| `desktop/tatami/activity/src/model.rs` | 590 | none |  | data types and formatting |
| `desktop/tatami/activity/src/nvml.rs` | 321 | machines | [TAP-03](#tap-03) | NVML sample cursor last_seen |
| `desktop/tatami/activity/src/power.rs` | 216 | machines | [TAP-03](#tap-03) | Floor running minimum; otherwise pure |
| `desktop/tatami/activity/src/procfs.rs` | 486 | none |  | pure /proc parsing |
| `desktop/tatami/activity/src/sampler.rs` | 955 | machines | [TAP-02](#tap-02), [TAP-03](#tap-03) | sampler thread; baselines, caches, energy integrals |
| `desktop/tatami/activity/src/summary.rs` | 610 | none |  | widgets updated per frame; GPU cards rebuilt on count change |
| `desktop/tatami/activity/src/table.rs` | 900 | machines | [TAP-04](#tap-04), [TAP-05](#tap-05), [TAP-07](#tap-07) | per-view sort memory; cell bind handlers; mode copy |
| `desktop/tatami/activity/src/theme.rs` | 97 | none |  | stylesheet lookup |
| `desktop/tatami/activity/src/window.rs` | 991 | machines | [TAP-06](#tap-06), [TAP-07](#tap-07), [TAP-08](#tap-08) | singleton and pending request; replicated lens/mode; quit dialog |
| `desktop/tatami/bluetooth/data/test-bus.conf` | 12 | machines | [TAP-12](#tap-12) | private bus config for the harness |
| `desktop/tatami/bluetooth/package.nix` | 59 | machines | [TAP-12](#tap-12) | checkPhase runs the harness under xvfb and dbus-run-session |
| `desktop/tatami/bluetooth/src/agent.rs` | 267 | machines | [TAP-09](#tap-09), [TAP-10](#tap-10) | Agent1 prompt and invocation; Release clears agent_ready |
| `desktop/tatami/bluetooth/src/bluez.rs` | 210 | machines | [TAP-10](#tap-10), [TAP-11](#tap-11) | owner-pinned session and agent; discovery session |
| `desktop/tatami/bluetooth/src/main.rs` | 18 | none |  | app entry |
| `desktop/tatami/bluetooth/src/model.rs` | 231 | none |  | pure snapshot parsing and validation |
| `desktop/tatami/bluetooth/src/theme.rs` | 114 | none |  | stylesheet lookup and install |
| `desktop/tatami/bluetooth/src/ui_test.rs` | 536 | machines | [TAP-12](#tap-12) | test harness: fake BlueZ on a private dbus-daemon |
| `desktop/tatami/bluetooth/src/window.rs` | 867 | machines | [TAP-09](#tap-09), [TAP-10](#tap-10), [TAP-13](#tap-13), [TAP-14](#tap-14), [TAP-15](#tap-15) | pairing state; backend watch; read/act revision; detail dialog; close drain |
| `desktop/tatami/displays/package.nix` | 47 | none |  | build recipe |
| `desktop/tatami/displays/src/arrangement.rs` | 225 | machines | [TAP-16](#tap-16) | tile drag gesture |
| `desktop/tatami/displays/src/main.rs` | 107 | none |  | activate presents the active window |
| `desktop/tatami/displays/src/model.rs` | 932 | machines | [TAP-17](#tap-17) | display use and main display; otherwise pure layout math |
| `desktop/tatami/displays/src/system.rs` | 236 | machines | [TAP-18](#tap-18), [TAP-19](#tap-19), [TAP-21](#tap-21) | event watcher thread; untimed helper calls; night shift threads |
| `desktop/tatami/displays/src/theme.rs` | 97 | none |  | stylesheet lookup |
| `desktop/tatami/displays/src/window.rs` | 1171 | machines | [TAP-16](#tap-16), [TAP-17](#tap-17), [TAP-19](#tap-19), [TAP-20](#tap-20), [TAP-21](#tap-21), [TAP-22](#tap-22) | apply/refresh flags; keep countdown; night shift; debounces |
| `desktop/tatami/network/package.nix` | 47 | none |  | build recipe |
| `desktop/tatami/network/src/details.rs` | 723 | machines | [TAP-23](#tap-23), [TAP-29](#tap-29) | details page lifecycle and version; connect button |
| `desktop/tatami/network/src/dns.rs` | 252 | machines | [TAP-24](#tap-24) | suspended IPv6 DNS backup in user.data |
| `desktop/tatami/network/src/join.rs` | 456 | machines | [TAP-25](#tap-25) | one join at a time with password retry |
| `desktop/tatami/network/src/main.rs` | 78 | machines | [TAP-06](#tap-06), [TAP-26](#tap-26) | --join request; stop_monitor at shutdown |
| `desktop/tatami/network/src/model.rs` | 876 | machines | [TAP-24](#tap-24), [TAP-25](#tap-25) | pure parsing; refresh_dns and refused used by entries |
| `desktop/tatami/network/src/nm.rs` | 405 | machines | [TAP-24](#tap-24), [TAP-25](#tap-25), [TAP-26](#tap-26), [TAP-27](#tap-27) | nmcli calls; Secret file; monitor child; snapshot |
| `desktop/tatami/network/src/profile.rs` | 319 | machines | [TAP-24](#tap-24) | VersionId-guarded Update2; VM fixture assertion test |
| `desktop/tatami/network/src/theme.rs` | 97 | none |  | stylesheet lookup |
| `desktop/tatami/network/src/window.rs` | 882 | machines | [TAP-06](#tap-06), [TAP-23](#tap-23), [TAP-25](#tap-25), [TAP-27](#tap-27), [TAP-28](#tap-28), [TAP-29](#tap-29), [TAP-33](#tap-33) | read coalescing; scan; switches; pages; sections |
| `desktop/tatami/sound/package.nix` | 65 | none |  | build recipe and PATH wrapping |
| `desktop/tatami/sound/src/audio.rs` | 320 | machines | [TAP-30](#tap-30), [TAP-32](#tap-32) | pactl subscribe restart loop; actions and coalescing |
| `desktop/tatami/sound/src/main.rs` | 82 | machines | [TAP-30](#tap-30) | stop_monitor at shutdown |
| `desktop/tatami/sound/src/meter.rs` | 201 | machines | [TAP-31](#tap-31) | libpulse level meter thread |
| `desktop/tatami/sound/src/model.rs` | 896 | none |  | pure pactl JSON parsing and naming |
| `desktop/tatami/sound/src/theme.rs` | 96 | none |  | stylesheet lookup |
| `desktop/tatami/sound/src/window.rs` | 1095 | machines | [TAP-06](#tap-06), [TAP-31](#tap-31), [TAP-32](#tap-32), [TAP-33](#tap-33) | serial queue and reads; meter follower; sections |
| `desktop/tatami/wallpapers/package.nix` | 93 | none |  | linkFarm data layout |
