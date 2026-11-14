# TAP: Tatami settings apps

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/tatami/{activity,network,bluetooth,sound,displays}`. Audited at `8e0acbe5`; line numbers refer to that commit.

50 files audited. 33 implicit machines: 9 High, 11 Medium, 13 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

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
- **Reproduction:** Confirmed (unit): [summary](reproductions.md#tap-03)

<a id="tap-04"></a>

### TAP-04 — Activity process table: columns per view and remembered sorts
- **Where:** `desktop/tatami/activity/src/table.rs:304` (`Table.lens`), `.sorts` (:306), `.swapping` (:308), `.mode` (:312), `Filter.mode` (:268); closure-local `last` (:568)
- **Encoding:** the current view; a per-view (column, order) map; a reentrancy flag; the last primary column the sorter handler saw; a deferred idle re-sort; the grouping mode held twice (`Table.mode`, `Filter.mode`)
- **States:** `Settled(lens, sort)` → `Swapping` (columns removed and re-added) → `Settled`; on a header click `UserSorted` → (numeric, newly primary, ascending) `FlipPending` (idle) → `Settled(descending)`
- **Transitions:** `set_lens` (`table.rs:663-691`); the sorter's `changed` handler ignores changes while swapping (:574), records `sorts` (:584-588), schedules the flip (:589-598); `set_root` sets the mode, refilters, keeps or resets the selection (:640-659); `resort` keeps a list scrolled to the top at the top (:694-718); `refilter` expands top-level rows while searching (:733-753)
- **Hazards:** `last` isn't updated while `swapping`, so at start-up and after every view switch it doesn't name the actual primary column. `Table.mode` and `Filter.mode` must agree; cells capture the mode at bind (:484)
- **Reify:** High — four cooperating fields plus a deferred idle action, with a reproducible bug
- **Suspected bug:** At start-up `last` is None, and `set_lens` sorts the CPU view by % CPU descending while `swapping` is set. The person clicks "% CPU" to sort ascending. GTK toggles it to ascending, the handler sees `changed` (None ≠ % CPU) and schedules a flip back to descending (:589-597), so the click is undone and only a second click reverses the sort. The same happens to the first click on the restored sort column after each view switch, whenever `last` names a different column. Clicking the column `last` does name leaves it ascending: smallest first, against the rule (confidence: high)
- **Reproduction:** Confirmed (unit): [summary](reproductions.md#tap-04)

<a id="tap-06"></a>

### TAP-06 — Window singleton, deferred first presentation and launch requests held until the first reading (Activity, Network, Sound)
- **Where:** `desktop/tatami/activity/src/window.rs:65` (`UI`), `Ui.presented` (:58), `State.pending` (:43), `State.pids` (:40); also `desktop/tatami/network/src/window.rs:99` (`UI`), :93 (`presented`), `State.loaded`/`.pending_join` (:31, :37); `desktop/tatami/sound/src/window.rs:147` (`UI`), :125 (`presented`)
- **Encoding:** a thread-local `Option<Rc<Ui>>`; a once flag for `present`; a one-shot timeout (600 ms in Activity, 250 ms in Network and Sound); a request (Activity: a PID and whether exact; Network: an SSID) consumed after a reading
- **States:** `NotBuilt` → `Built(hidden, request?)` → `Presented` (by the timeout or the first reading); request `Pending` → `Consumed`. The slot is never cleared; the process ends with the window
- **Transitions:** Activity: first launch builds and stores (`desktop/tatami/activity/src/window.rs:103-115`), the first frame takes `pending` and presents (:324-330), a second launch acts at once (:90-101). Network: first launch (`desktop/tatami/network/src/window.rs:104-117`); the first reading sets `loaded`, presents and takes `pending_join` (:376-391); `join_by_name` queues until loaded (:419-423). Sound: first launch (`desktop/tatami/sound/src/window.rs:152-164`); a reading presents (:401)
- **Hazards:** Activity's second launch acts on `state.pids` even before the first frame. A second launch presents the window before its first reading while `presented` stays false, so the first reading presents it again
- **Reify:** High — the same wait-until-loaded shape copied three times, one copy without the guard
- **Suspected bug:** `tatami-activity --pid N`, run while the window exists but before its first frame (within about half a second of the first launch), calls `select_pid` with an empty `pids` map (`desktop/tatami/activity/src/window.rs:97-98`, :441-444). It toasts "Process N has ended" for a live process and selects nothing, where Network guards the same case with `loaded` (`desktop/tatami/network/src/window.rs:420-422`) (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#tap-06)

<a id="tap-08"></a>

### TAP-08 — Activity's Quit, Force Quit and Pause on a target taken from the latest frame
- **Where:** `desktop/tatami/activity/src/window.rs:696` (`confirm_quit`: `target` moved into the response closure); also `target` (:621), `update_actions` (:660)
- **Encoding:** a `Target` (PIDs, first processes, mine, paused, session), computed from the selection and the latest frame; the dialog's lifetime; `paused` derived from the kernel's state 'T'
- **States:** `Idle` → `Confirming(target)` → `Signalled` | `Cancelled`; per selection `Running` ⇄ `Paused`, observed only at the next frame
- **Transitions:** dialog opened (`window.rs:697-745`); response sends SIGTERM to the first processes or SIGKILL to all (:735-743); Pause (SIGSTOP) and Resume (SIGCONT) act at once (:762-785); actions enabled from `target` (:660-681)
- **Hazards:** when the dialog is answered, the PIDs are not checked again against their start time or key. The Pause/Resume button changes only after the next frame (up to 5 s), so it can be pressed twice
- **Reify:** High — signals go to PIDs captured an unbounded time earlier
- **Suspected bug:** The person opens "Quit “X”?" and leaves it open. X exits on its own and another of their processes reuses its PID. Choosing Force Quit then SIGKILLs the unrelated process (:741), because `target.all` holds bare PIDs from the frame shown when the dialog opened (confidence: low)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#tap-08)

<a id="tap-13"></a>

### TAP-13 — Bluetooth readings and actions serialised with a revision counter
- **Where:** `desktop/tatami/bluetooth/src/window.rs:40-44` (`Ui.busy`, `.reading`, `.updating`, `.closing`, `.revision`); also :23 (`snapshot`), :38 (`selected`)
- **Encoding:** one reading at a time (`reading`); one action at a time (`busy`); a revision bumped at the start and the end of every action, so overlapping readings are discarded and repeated; `updating` silences widget handlers during `render`; a 3 s poll
- **States:** `Idle` → `Reading(rev)` → `Rendered` | `Discarded → Reading`; `Acting(action)`, overlapping readings; `Failed` (snapshot emptied, Try Again shown)
- **Transitions:** poll (`window.rs:79-85`); `refresh` guard and start (:339-347), result accepted when the revision is unchanged (:353-391), an error empties the snapshot (:393-401), rerun when the revision or backend changed (:405-408); `act` guard, busy, revision bump (:623-637), completion clears busy, bumps the revision, refreshes (:670-689); `render` under `updating` (:413, :547); the power switch and adapter handlers skip while updating (:252, :266)
- **Hazards:** the error branch doesn't check `revision`. While busy the status line isn't updated (:376). An action refused because `busy` is set is dropped without a word; the forget confirmation's response is one such path (:851-859). The snapshot is stored before the awaited register and scan (:355)
- **Reify:** High — five flags and a counter across six mutation sites, with invariants (no action against a stale snapshot) kept by convention
- **Related:** BlueZ ObjectManager (`desktop/tatami/bluetooth/src/bluez.rs:58`)

<a id="tap-15"></a>

### TAP-15 — Bluetooth window close: hide, drain, release, destroy
- **Where:** `desktop/tatami/bluetooth/src/window.rs:43` (`Ui.closing`), :67 (`UI`), :294-326 (close handler)
- **Encoding:** the `closing` latch; an application hold; a future that cancels pairing, polls `reading` every 20 ms, stops discovery and the agent, destroys the window and clears `UI`
- **States:** `Open` → `Closing(hidden, draining)` → `Destroyed`
- **Transitions:** first close latches, hides, closes the prompt, sets `stopped`, holds the application (`window.rs:298-308`); drain (:309-324); later closes are refused (:298-300). `closing` gates refresh (:340), actions (:624), the poll (:80) and new service owners (:112)
- **Hazards:** `open` presents whatever `UI` holds (:71-73), including a window that is closing. The drain has no overall deadline beyond each call's timeout (CancelPairing 30 s, each stop call 5 s)
- **Reify:** High — a latch plus a hidden but live window that `open` can reach
- **Suspected bug:** Relaunching Bluetooth while the close drain runs (for example while CancelPairing waits on a slow adapter) presents the hidden window again (:71-72). Every control is dead, because `closing` gates refresh and actions, and when the drain finishes it destroys the window under the person (:321-323) (confidence: low)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#tap-15)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#tap-19)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#tap-21)

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
- **Reproduction:** Confirmed (end-to-end): [summary](reproductions.md#tap-25)
- **Follow-up:** bug fixed in [#302](https://github.com/bitemyapp/sekka/pull/302), not reified. The password reaches nmcli through a pipe (`passwd-file /dev/stdin`), so no file can be left behind. Closing the window while a join runs only hides it until the join and its rollback are over. Unchanged: the untracked asking phase, "Timeout" counted as a refused password, and a session that kills the program mid-join, which still leaves a failed new profile (but no password).
