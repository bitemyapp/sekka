# SMT: Sumi, Myaku and Tenki

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/apps/{sumi,myaku,tenki}`. Audited at `8e0acbe5`; line numbers refer to that commit.

58 files, 0 pending. 33 implicit machines: 10 High, 6 Medium, 17 Low.

## Machines

<a id="smt-01"></a>

### SMT-01 — GPU lens card choice, hover and projected frame
- **Where:** `desktop/aizome/apps/myaku/src/app.rs:111` (`Myaku.gpu`, `.hover_gpu` :113, `.projected` :115, `.reading` :108, `.frame` :107, `.lens` :117); also `desktop/aizome/apps/myaku/src/ui/gpu.rs:63`
- **Encoding:** `lens: Lens`, `gpu: Option<String>` (PCI address of the card shown alone), `hover_gpu: Option<String>`, and the memo key `projected: Option<(frames, Option<device>)>` that decides whether `frame` is re-derived from `reading` with `Frame::for_gpu`
- **States:** lens {Cpu, Memory, Energy, Gpu, Disk} × card {All, One(pci)} × hover {None, Some(pci)}; `frame` = `reading` or `reading.for_gpu(d)`; representable: `gpu` Some while lens ≠ Gpu (kept deliberately for return), `hover_gpu` naming a card no longer present
- **Transitions:** `Msg::Gpu` sets the card and forces the GPU lens (`desktop/aizome/apps/myaku/src/app.rs:488-492`); `Msg::Lens` changes lens and clears hover (`desktop/aizome/apps/myaku/src/app.rs:505-507`); `Msg::HoverGpu` enter/leave (`desktop/aizome/apps/myaku/src/app.rs:495-499`); `take()` falls back to Cpu when no card is read (`desktop/aizome/apps/myaku/src/app.rs:236-238`); `project()` drops `gpu` when its card is absent and re-projects when `(frames, device)` changes (`desktop/aizome/apps/myaku/src/app.rs:260-272`); `Msg::Launched --lens` (`desktop/aizome/apps/myaku/src/app.rs:603-605`); strip card press/hover (`desktop/aizome/apps/myaku/src/ui/gpu.rs:64-65`)
- **Hazards:** a single reading without the card (a transient sysfs/NVML failure) forgets the user's card choice for good (`desktop/aizome/apps/myaku/src/app.rs:261-263`); `hover_gpu` survives the card vanishing and `take()`'s lens fallback (only compared, so harmless)
- **Reify:** Low — a small memo plus two optional choices in one file.

<a id="smt-02"></a>

### SMT-02 — Table selection, expansion and launch request across readings
- **Where:** `desktop/aizome/apps/myaku/src/app.rs:124` (`Myaku.sel`, `.expanded` :122, `.rows` :123, `.inspector` :125, `.request` :131, `.frames` :134); also `desktop/aizome/apps/myaku/src/ui/view.rs:191`, `desktop/aizome/apps/myaku/src/ui/inspector.rs:41`
- **Encoding:** selection by identity (`Sel::Group(first member's (pid, start))`, `Sel::Process((pid, start))`); set of expanded group keys; `rows` (row index → frame index) rebuilt every reading; `request.pid` pending until the next reading; `frames == 1` triggers the focused-app auto-select
- **States:** NoSelection → Selected(key) → Orphaned (key absent: inspector "Nothing selected", `can_quit` "It has ended."); PendingPid(pid) → Selected | NotFound (notice); representable: inspector open on an orphaned key, `expanded` holding keys of ended groups
- **Transitions:** first reading auto-selects the focused group (`desktop/aizome/apps/myaku/src/app.rs:248-252`); pending pid resolved or failed in `take()` (`desktop/aizome/apps/myaku/src/app.rs:240-247`); `Msg::Launched --pid` re-arms it (`desktop/aizome/apps/myaku/src/app.rs:609-612`); `SelectRows` (`desktop/aizome/apps/myaku/src/app.rs:532-536`); `Activate` (`desktop/aizome/apps/myaku/src/app.rs:537-542`); `Select` (and expand its group) (`desktop/aizome/apps/myaku/src/app.rs:543-551`); `Toggle` (`desktop/aizome/apps/myaku/src/app.rs:526-531`); `Inspector` (`desktop/aizome/apps/myaku/src/app.rs:552`); rows rebuilt and re-sorted every reading (`desktop/aizome/apps/myaku/src/app.rs:254`, `305-367`)
- **Hazards:** `SelectRows`/`Activate` carry row indices resolved against `rows` at handling time, while `rows` is re-sorted every reading; a group's key is its first-started member's, so when that process exits the group gets a new key and the selection orphans; `expanded` is never pruned
- **Reify:** High — identity-based selection mixed with index-based messages over a list that reorders every second; suspected bug.
- **Suspected bug:** Right-click a row, wait a second, choose Inspect: the item carries the row index captured at right-click (`desktop/aizome/apps/myaku/src/ui/view.rs:205`), a reading re-sorts `rows` (by % CPU) before the click, and `Msg::Activate(i)` (`desktop/aizome/apps/myaku/src/app.rs:538-540`) then selects and inspects whatever row now sits at `i`, overriding the correct selection the right-click made (confidence: medium)

<a id="smt-03"></a>

### SMT-03 — Quit/pause confirmation and the status notice
- **Where:** `desktop/aizome/apps/myaku/src/app.rs:127` (`Myaku.notice`); alert payload `Msg::Signal(Sel, Signal)` `desktop/aizome/apps/myaku/src/app.rs:69`; also `desktop/aizome/apps/myaku/src/ui/view.rs:457`, `desktop/aizome/apps/myaku/src/ui/inspector.rs:308`
- **Encoding:** a kit alert whose buttons carry `Msg::Signal(sel, signal)` captured at ask time; `notice: Option<(String, error: bool)>`
- **States:** Idle → Confirming(sel, force) → Signalled | Cancelled; Pause/Resume signals at once; notice None → Shown(text, error), never back to None
- **Transitions:** `AskQuit` checks `can_quit`, opens the alert or sets an error notice (`desktop/aizome/apps/myaku/src/app.rs:564-588`); confirm → `signal()` re-resolves targets and sets the notice (`desktop/aizome/apps/myaku/src/app.rs:589`, `440-461`); `Pause` (`desktop/aizome/apps/myaku/src/app.rs:590-593`); unknown launch pid (`desktop/aizome/apps/myaku/src/app.rs:246`); Cancel just closes the alert (kit)
- **Hazards:** `can_quit` (kernel, other user, Myaku itself; `desktop/aizome/apps/myaku/src/app.rs:402-419`) is checked only at ask time; targets are re-resolved by key at confirm time, so the processes signalled may differ from the count the alert named; if the target ended meanwhile `targets()` is empty and `signal()` reports success "Asked to quit: ." with no name; nothing clears `notice`, and the status line prefers it to the summary (`desktop/aizome/apps/myaku/src/ui/view.rs:457-459`)
- **Reify:** High — a confirmation lifecycle split between alert payloads and a sticky field; suspected bug.
- **Suspected bug:** After any quit, pause or refused attempt, the status line keeps that message (in red for "It belongs to another user…" and the like) for the rest of the session, hiding the process/thread/uptime summary and the GPU lens's client count, because `notice` is never reset to None (confidence: medium)

<a id="smt-04"></a>

### SMT-04 — Energy Meters utility window
- **Where:** `desktop/aizome/apps/myaku/src/app.rs:104` (`Myaku.energy_meters`); also `desktop/aizome/apps/myaku/src/ui/energy.rs:78`
- **Encoding:** `Option<WindowKey>`
- **States:** Closed → Open(key)
- **Transitions:** `EnergyMeters` opens or reuses, then activates (`desktop/aizome/apps/myaku/src/app.rs:554-557`); `CloseEnergyMeters` from its Close button (`desktop/aizome/apps/myaku/src/app.rs:560-562`, `desktop/aizome/apps/myaku/src/ui/energy.rs:114`); `close_requested` clears it, and closing the main window closes it (`desktop/aizome/apps/myaku/src/app.rs:624-630`)
- **Hazards:** none observed
- **Reify:** Low — one Option, transitions in one file.

<a id="smt-05"></a>

### SMT-05 — Single-instance socket claim and hand-over (Myaku, Sumi, Tenki)
- **Where:** `desktop/aizome/apps/myaku/src/instance.rs:23` (`claim`, `serve` :42); also `desktop/aizome/apps/sumi/src/instance.rs:34`, `desktop/aizome/apps/tenki/src/instance.rs:31`, `desktop/aizome/apps/myaku/src/main.rs:98`, `desktop/aizome/apps/sumi/src/main.rs:28`, `desktop/aizome/apps/tenki/src/main.rs:41`
- **Encoding:** the socket file `$XDG_RUNTIME_DIR/aizome-app.<name>.sock`; `enum Claim {First, Handed, Alone}`; a listener parked in the app (`Myaku.listener`, `Sumi.instance`, `Tenki.instance`) then moved into a serve thread
- **States:** Probe → Handed (connect and write succeed; exit) | Stale (connect fails: remove the file) → First(listener) → Serving(thread) → Stopped (app's sender refuses) | Alone (bind failed)
- **Transitions:** connect/write (`desktop/aizome/apps/myaku/src/instance.rs:25-29`; `desktop/aizome/apps/sumi/src/instance.rs:35-39` with `shutdown(Write)`; `desktop/aizome/apps/tenki/src/instance.rs:33-37`); remove and bind (`desktop/aizome/apps/myaku/src/instance.rs:30-31`; `desktop/aizome/apps/sumi/src/instance.rs:43-44`; `desktop/aizome/apps/tenki/src/instance.rs:40-41`); serve thread, 500 ms read per connection, exit when send fails (`desktop/aizome/apps/myaku/src/instance.rs:43-53`; `desktop/aizome/apps/sumi/src/instance.rs:59-67`; `desktop/aizome/apps/tenki/src/instance.rs:52-58`); listener handed over in `launched` (`desktop/aizome/apps/myaku/src/app.rs:478-481`, `desktop/aizome/apps/sumi/src/app.rs:982-984`, `desktop/aizome/apps/tenki/src/app.rs:494-497`)
- **Hazards:** two launches racing both find no listener; the second removes the first's freshly bound socket, leaving two instances with the first unreachable; the socket file is never removed on exit (the next launch treats it as stale); `Alone` runs with no hand-over, and for Sumi two instances then share one autosave folder (SMT-16); a peer that accepts but never reads still counts as Handed
- **Reify:** Low — three copies of a short linear protocol; one shared helper matters more than a state enum.

<a id="smt-06"></a>

### SMT-06 — NVML library session and per-device sample cursor
- **Where:** `desktop/aizome/apps/myaku/src/nvml.rs:192` (`Nvml.api`, `.devices`, `.last_seen` :196); `Library` :102
- **Encoding:** a dlopen handle (`Library`, `dlclose` on drop), `nvmlInit`/`nvmlShutdown` paired in `Nvml::open`/`Drop`, device handles fixed at open, `last_seen[device]` timestamp cursor for `nvmlDeviceGetProcessUtilization`
- **States:** Absent (no library or symbol) → Initialised(devices) → Reading (cursor advances) → Shut down (shutdown, then dlclose)
- **Transitions:** `Api::open` (`desktop/aizome/apps/myaku/src/nvml.rs:125-166`); init, and shutdown when no device (`desktop/aizome/apps/myaku/src/nvml.rs:209-216`); cursor init (`desktop/aizome/apps/myaku/src/nvml.rs:223`); per-read cursor advance (`desktop/aizome/apps/myaku/src/nvml.rs:288-291`, newest `:367-368`); grow-buffer retry loops (`desktop/aizome/apps/myaku/src/nvml.rs:327-341`, `351-364`); `Drop` (`desktop/aizome/apps/myaku/src/nvml.rs:299-306`) before `Library` drop (`desktop/aizome/apps/myaku/src/nvml.rs:104-111`)
- **Hazards:** NVML is opened once, and only if a card's driver was nvidia when the sampler started (`desktop/aizome/apps/myaku/src/sampler.rs:336`), so a driver reload or added card is never seen and stale handles keep being queried; the first read asks for samples since 0, so its per-process busy figure averages the driver's whole retained buffer
- **Reify:** Low — lifecycle tied to one owner and `Drop`; the cursor is one field.
- **Related:** NVIDIA's `libnvidia-ml.so.1` (external)

<a id="smt-07"></a>

### SMT-07 — Sampler thread schedule and window commands
- **Where:** `desktop/aizome/apps/myaku/src/sampler.rs:154` (`spawn`: `interval` :158, `next` :162; `Sampler.lens`, `.focus` :305-306); also `desktop/aizome/apps/myaku/src/app.rs:128` (`Myaku.sampler`, `.live` :129, `.interval` :126, `.told` :135)
- **Encoding:** a detached thread reading `mpsc::Receiver<Command>`; a `next: Instant` deadline; on the window side the `Sender` and a last-told `(Lens, pids)` dedupe
- **States:** NotStarted (not live, or before `launched`) → Priming (`Sampler::new` and a first reading) → Running (wait for `next`, read, deliver) → Exited (sender dropped or delivery refused)
- **Transitions:** spawn in `launched` (`desktop/aizome/apps/myaku/src/app.rs:473-476`); priming reading and a first deadline 300 ms on (`desktop/aizome/apps/myaku/src/sampler.rs:161-162`); `Command::Interval` pulls `next` in (`desktop/aizome/apps/myaku/src/sampler.rs:165-168`, sent `desktop/aizome/apps/myaku/src/app.rs:594-598`); `Command::Focus` (`desktop/aizome/apps/myaku/src/sampler.rs:170-173`, sent by `tell_sampler` `desktop/aizome/apps/myaku/src/app.rs:421-438` after every update `:616`); tick (`desktop/aizome/apps/myaku/src/sampler.rs:178-186`); exit (`desktop/aizome/apps/myaku/src/sampler.rs:176`, `179-181`)
- **Hazards:** no liveness or restart: a panic in `Sampler::new` or `sample()` ends the thread silently, send errors are ignored (`desktop/aizome/apps/myaku/src/app.rs:435`, `597`) and `told` is updated regardless, so the window freezes on its last reading or shows "Reading the system…" forever (`desktop/aizome/apps/myaku/src/ui/view.rs:178-187`); a focus change waits for the next tick; lengthening the interval applies only after the pending tick
- **Reify:** Medium — a two-sided protocol with a dedupe cache and no explicit thread state.

<a id="smt-08"></a>

### SMT-08 — Per-process reading cache with staleness timers and a maps budget
- **Where:** `desktop/aizome/apps/myaku/src/sampler.rs:194` (`Known.status_at`, `.voluntary`, `.wakeups`, `.io`, `.io_at`, `.rates`, `.maps`, `.maps_rss`, `.maps_tick`); `Sampler.before` :290
- **Encoding:** `HashMap<(pid, start), Known>`; `Option<Instant>` read times; tick-based maps age; a per-reading `maps_budget`
- **States:** per process: New (`status_at` None) → Read (status/io baselines) → Stale (≥ 4.5 s since read: re-read even if idle); rates Measured | Zeroed (didn't run); maps Unread → Read(tick) → Old (≥ 6 × `maps_every` readings) | Moved (RSS moved); Gone (pruned)
- **Transitions:** take out (`desktop/aizome/apps/myaku/src/sampler.rs:428`); status and wake-up rate (`desktop/aizome/apps/myaku/src/sampler.rs:432-448`); io and rates (`desktop/aizome/apps/myaku/src/sampler.rs:452-469`); maps refresh within budget (`desktop/aizome/apps/myaku/src/sampler.rs:478-487`); put back (`desktop/aizome/apps/myaku/src/sampler.rs:494`); prune (`desktop/aizome/apps/myaku/src/sampler.rs:560`); budget unlimited for two readings then 48 (`desktop/aizome/apps/myaku/src/sampler.rs:393`); cadence follows the lens (`desktop/aizome/apps/myaku/src/sampler.rs:392`)
- **Hazards:** an own process whose `smaps_rollup` can't be read (non-dumpable) never updates `maps_tick`, so it counts as old every reading and spends a budget slot forever; with 48 such processes early in /proc order, later processes' memory figures stop refreshing; a running process whose `status` read fails keeps its previous wake-up rate
- **Reify:** Medium — nine fields with time- and tick-based validity rules.

<a id="smt-09"></a>

### SMT-09 — DRM client scan and busy-delta baselines
- **Where:** `desktop/aizome/apps/myaku/src/sampler.rs:291` (`Sampler.drm_fds`, `.clients` :292)
- **Encoding:** per process, the DRM fds found at the last scan (every tenth reading, or every reading while inspected); a global `(pdev, client id) → (DrmClient, Instant)` baseline map replaced every reading
- **States:** per process Unscanned → Scanned(fds) | NoFds; per client New (no baseline: busy unknown) → Measured (busy from the delta)
- **Transitions:** scan (`desktop/aizome/apps/myaku/src/sampler.rs:498-505`); delta against the previous baseline (`desktop/aizome/apps/myaku/src/sampler.rs:524-530`); baseline carried and replaced (`desktop/aizome/apps/myaku/src/sampler.rs:531`, `558`); prune (`desktop/aizome/apps/myaku/src/sampler.rs:561`)
- **Hazards:** a new GPU client appears up to ten readings late; a client fd inherited by a child is counted in both processes and twice in their group (the dedupe set is per process, `desktop/aizome/apps/myaku/src/sampler.rs:507`)
- **Reify:** Low — two caches with one refresh rule.

<a id="smt-10"></a>

### SMT-10 — Power metering across readings and sleep
- **Where:** `desktop/aizome/apps/myaku/src/sampler.rs:296` (`Sampler.rapl: Rapl {before, watts}` :221-225, `.last: Option<Moment>` :289, `.gpu_energy` :298, `.floor`, `.gpu_floors` :299-300, `.energy` :301)
- **Encoding:** partially explicit: RAPL's previous reading and rate in `Rapl` (transitions in `Rapl::read`, reset from outside); the last reading's two-clock `Moment`; per-card energy counter baselines; idle floors (minimum seen); per-process `(joules, seconds measured)`
- **States:** RAPL Unreadable → Primed (one reading) → Rated(watts) (the same counters again keep the rate); interval First | Awake(seconds) | Slept (seconds 0); per-process energy Unmeasured → Accumulating → Gone
- **Transitions:** `interval()` tells awake from slept (`desktop/aizome/apps/myaku/src/sampler.rs:275-280`, stored `:383-384`); sleep resets RAPL (`desktop/aizome/apps/myaku/src/sampler.rs:671-673`); `Rapl::read` reset/keep/rate/re-prime (`desktop/aizome/apps/myaku/src/sampler.rs:231-247`); card counter baselines (`desktop/aizome/apps/myaku/src/sampler.rs:657-665`); floors updated in `attribute` (`desktop/aizome/apps/myaku/src/sampler.rs:123`, `133`); energy accumulated and pruned (`desktop/aizome/apps/myaku/src/sampler.rs:679-692`)
- **Hazards:** floors only fall and are never reset: not after sleep, not when the processor's meter changes source (RAPL, package, battery); on battery the processor's share is `(battery − graphics).max(0)` (`desktop/aizome/apps/myaku/src/sampler.rs:118`), so one reading with the cards' meters above a lagging battery figure pins the floor at 0 for the session; energy keeps accumulating across a meter-source change without marking it
- **Reify:** Medium — several baselines whose validity rules interact (sleep, meter source).
- **Suspected bug:** On a laptop with a discrete card, on battery, a momentary card-meter reading above the battery's reported draw calls `floor.update(0)`, after which no idle floor is ever split by capacity share and every later estimate charges processes the whole draw by busy share (confidence: low)
- **Related:** `desktop/aizome/crates/aizome-sys/src/sysinfo/power.rs` (`Floor`, `meter_counters` answered by aizome-power-meter)

<a id="smt-11"></a>

### SMT-11 — Group identity cache and launcher-index refresh cadence
- **Where:** `desktop/aizome/apps/myaku/src/sampler.rs:313` (`Sampler.identities`, `.apps` :302, `.tick` :288); also `desktop/aizome/apps/myaku/src/group.rs:578`
- **Encoding:** `HashMap<(first member key, fingerprint), Identity>`; a tick-modulo schedule
- **States:** per group Miss → Cached(identity); whole cache Valid → Flushed every 30th reading
- **Transitions:** `AppIndex::refresh_if_changed` every 30 readings (`desktop/aizome/apps/myaku/src/sampler.rs:385-389`); flush (`desktop/aizome/apps/myaku/src/sampler.rs:715-717`); lookup or insert (`desktop/aizome/apps/myaku/src/sampler.rs:719-730`); prune unused (`desktop/aizome/apps/myaku/src/sampler.rs:753`); key from `fingerprint` (`desktop/aizome/apps/myaku/src/group.rs:578-590`)
- **Hazards:** a newly installed or renamed launcher entry names its group only after the next flush (30 s to 2.5 min); the fingerprint includes window focus and titles, so focus changes churn the cache
- **Reify:** Low — a cache with one flush rule.
- **Related:** the compositor's window list over `aizome_ipc::Request::Windows` (`desktop/aizome/apps/myaku/src/group.rs:448-461`), served by aizome-comp-core

<a id="smt-12"></a>

### SMT-12 — (test harness) scripted power meter that repeats its last reading
- **Where:** `desktop/aizome/apps/myaku/src/sampler.rs:932` (`Scripted.read`, `.reads`, `.start`)
- **Encoding:** `read: Option<u64>` (when the fake meter last read the counter) and a `reads` count
- **States:** NeverRead → Read(at); a question less than 1 s after `at` gets the same reading
- **Transitions:** `frame()` re-reads or repeats (`desktop/aizome/apps/myaku/src/sampler.rs:962-969`), then drives `sample_at` (`desktop/aizome/apps/myaku/src/sampler.rs:972`)
- **Hazards:** none observed
- **Reify:** Low — test fixture.
- **Related:** mirrors aizome-power-meter's once-a-second cache (`desktop/aizome/crates/aizome-sys/src/sysinfo/power.rs`)

<a id="smt-13"></a>

### SMT-13 — Close-after-save for dirty documents
- **Where:** `desktop/aizome/apps/sumi/src/app.rs:176` (`Sumi.closing: HashSet<WindowKey>`)
- **Encoding:** membership in `closing`, plus kit alerts whose buttons carry `Msg::Close(w, Save|Discard)`, `Msg::Overwrite`, `Msg::SaveUtf8`, `Msg::Reload`
- **States:** Open → Asking ("Save the changes?") → Discarded (closed) | SavePending (in `closing`) → Closed | still SavePending (save stopped by another alert or the panel)
- **Transitions:** `request_close` alert (`desktop/aizome/apps/sumi/src/app.rs:583-589`); `Close(Save)` inserts and saves (`desktop/aizome/apps/sumi/src/app.rs:1146-1148`); a successful `save` removes and closes (`desktop/aizome/apps/sumi/src/app.rs:542-544`); `SaveTo(None)` (panel cancelled) removes (`desktop/aizome/apps/sumi/src/app.rs:1098-1100`); `forget` removes (`desktop/aizome/apps/sumi/src/app.rs:598`); `Close(Discard)` (`desktop/aizome/apps/sumi/src/app.rs:1150`)
- **Hazards:** `save` returns false and leaves the window in `closing` on the changed-on-disk alert (`desktop/aizome/apps/sumi/src/app.rs:484-496`), the unencodable alert (`:498-512`) and a write failure (`:514-517`); none of their Cancel/OK paths remove it
- **Reify:** High — an intent flag outlives the alert chain that justified it; suspected bug.
- **Suspected bug:** Close a dirty document, choose Save, then Cancel the "changed by another program" or "can't be saved in Latin-1" alert (or hit a write error): the window stays open but stays in `closing`, so the next ordinary ⌘S, perhaps much later, saves and then closes the window unasked (`desktop/aizome/apps/sumi/src/app.rs:542-544`) (confidence: high)

<a id="smt-14"></a>

### SMT-14 — Link and Go to Line sheets
- **Where:** `desktop/aizome/apps/sumi/src/app.rs:155` (`enum Sheet`), `desktop/aizome/apps/sumi/src/app.rs:167` (`Sumi.sheets`); also `desktop/aizome/apps/sumi/src/view.rs:193`
- **Encoding:** partially explicit: `HashMap<sheet WindowKey, Sheet>`, each `Sheet` carrying its parent, field text and (Link) the selection range taken at open; the field messages `LinkText`, `LinkUrl`, `LinkDone`, `GotoText`, `GotoDone` carry no window key
- **States:** None → Link{parent, text, url, range} | Goto{parent, line, max} → Done(ok) | Cancelled | closed by request
- **Transitions:** open Link (`desktop/aizome/apps/sumi/src/app.rs:856-859`); open Goto (`desktop/aizome/apps/sumi/src/app.rs:959-961`); field edits (`desktop/aizome/apps/sumi/src/app.rs:1196-1209`, `1221-1227`); finish (`desktop/aizome/apps/sumi/src/app.rs:1210-1220`, `1228-1245`); `close_requested` removes (`desktop/aizome/apps/sumi/src/app.rs:1000-1002`)
- **Hazards:** every field message picks the first sheet of its kind from a `HashMap` (`sheet_of`, `desktop/aizome/apps/sumi/src/app.rs:968-970`), so with two sheets of one kind open over two documents the wrong one is edited, applied or closed; the Link range is fixed at open while menu commands aimed at the sheet go to its parent (`desktop/aizome/apps/sumi/src/app.rs:233`) and can change the text under it
- **Reify:** High — the enum carries data but messages lose which sheet they belong to; suspected bug.
- **Suspected bug:** Open Link… (⌘K) in document A, switch to document B and open Link… there: typing in B's sheet updates whichever Link sheet the map yields first, so B's field may not take the text, and Insert Link can insert A's link into A and close A's sheet while B's stays open (confidence: medium)

<a id="smt-15"></a>

### SMT-15 — Keyboard-target document tracking
- **Where:** `desktop/aizome/apps/sumi/src/app.rs:174` (`Sumi.focused`); also `desktop/aizome/apps/sumi/src/menus.rs:37`
- **Encoding:** `Option<WindowKey>` "as of the last message", with fallbacks in `target()` and `focused_doc()`
- **States:** None | Doc(w); stale once `w` closes (filtered where used)
- **Transitions:** every update from `cx.key_window()` (`desktop/aizome/apps/sumi/src/app.rs:1007-1011`); `Msg::On`, `Msg::Edit` (`desktop/aizome/apps/sumi/src/app.rs:1019`, `1023`); `new_window` (`desktop/aizome/apps/sumi/src/app.rs:271`); `open_path` (`desktop/aizome/apps/sumi/src/app.rs:330`); `forget` clears (`desktop/aizome/apps/sumi/src/app.rs:599-601`)
- **Hazards:** menus are built from `focused_doc()` (`desktop/aizome/apps/sumi/src/menus.rs:37-41`, falling back to the newest document) while commands go to `target()` (key window first, a sheet to its parent; `desktop/aizome/apps/sumi/src/app.rs:231-235`), so enabled and checked items can describe another document than the one a command reaches
- **Reify:** Low — one cached field with fallbacks.

<a id="smt-16"></a>

### SMT-16 — Autosave store worker, session file and startup/quit phases
- **Where:** `desktop/aizome/apps/sumi/src/autosave.rs:66` (`Store.dir`, `.tx`; `enum Job` :59); also `desktop/aizome/apps/sumi/src/app.rs:164` (`Sumi.store`), `desktop/aizome/apps/sumi/src/app.rs:188` (`Drop`)
- **Encoding:** a worker thread fed an ordered `Job` channel; `session.json` (open documents) and `<id>.txt` (present while a document has unsaved text); a flush rendezvous with a 10 s limit
- **States:** store NoDir (tests) | Running; app Launching (load session → tidy → restore each) → Ready (`LaunchDone`: an Untitled if none) → Quitting (autosave all → flush → quit); per document NoText ↔ TextKept
- **Transitions:** thread start (`desktop/aizome/apps/sumi/src/autosave.rs:85-110`); jobs queued (`desktop/aizome/apps/sumi/src/autosave.rs:138-151`, from `desktop/aizome/apps/sumi/src/app.rs:400`, `407`, `409`, `538`, `596`, `1122`); `flush` (`desktop/aizome/apps/sumi/src/autosave.rs:154-161`, called `desktop/aizome/apps/sumi/src/app.rs:191`, `604`, `1284`); `tidy` (`desktop/aizome/apps/sumi/src/autosave.rs:165-176`, `desktop/aizome/apps/sumi/src/app.rs:986`); `restore` (`desktop/aizome/apps/sumi/src/app.rs:416-471`); `LaunchDone` (`desktop/aizome/apps/sumi/src/app.rs:990`, `1270-1274`); `Quit` (`desktop/aizome/apps/sumi/src/app.rs:1275-1286`)
- **Hazards:** write errors are only logged (`desktop/aizome/apps/sumi/src/autosave.rs:106-108`) after `autosave_now` already set `autosaved = true` (`desktop/aizome/apps/sumi/src/app.rs:411`), so a full disk silently stops keeping a document's text until its next edit; `flush` gives up after 10 s and the process may exit with jobs queued; `tidy` removes only `*.txt`, so `*.tmp` from a crash mid-write remain; two instances (`Claim::Alone`, SMT-05) share the folder and each launch's `tidy` deletes the other's unsaved texts
- **Reify:** Medium — persisted phase across two files and a thread, but jobs are ordered and few.

<a id="smt-17"></a>

### SMT-17 — Document file, dirty and autosave lifecycle
- **Where:** `desktop/aizome/apps/sumi/src/doc.rs:178` (`Doc.path`, `.untitled` :180, `.saved` :185, `.text_dirty` :186, `.format_dirty` :188, `.stamp` :190, `.autosave_seq` :196, `.autosaved` :198, `.notice` :202); also `desktop/aizome/apps/sumi/src/app.rs:477`, `desktop/aizome/apps/sumi/src/files.rs:272`
- **Encoding:** nine fields; "pristine" derived (`desktop/aizome/apps/sumi/src/doc.rs:235-237`); conflict derived from `stamp` against the disk (`desktop/aizome/apps/sumi/src/doc.rs:373-378`); autosave debounce by `autosave_seq` echoed in `Msg::Autosave(w, seq)` 800 ms later
- **States:** Pristine Untitled → Untitled-dirty → Saved-clean ↔ Saved-dirty (text and/or format) → Conflict (changed on disk); orthogonal Autosaved ↔ AutosavePending; representable: `untitled > 0` with a path, `text_dirty` true with text equal to `saved` (only undo/redo recompute it)
- **Transitions:** `Doc::new` (`desktop/aizome/apps/sumi/src/doc.rs:206-228`); `install` (`desktop/aizome/apps/sumi/src/doc.rs:268-289`); `after_edit` marks dirty and bumps the seq (`desktop/aizome/apps/sumi/src/doc.rs:345-347`); undo/redo recompute (`desktop/aizome/apps/sumi/src/doc.rs:311-313`); `new_untitled_with` (`desktop/aizome/apps/sumi/src/app.rs:304-306`); save success (`desktop/aizome/apps/sumi/src/app.rs:522-539`); encoding and line-ending changes (`desktop/aizome/apps/sumi/src/app.rs:824`, `835`); `restore` (`desktop/aizome/apps/sumi/src/app.rs:433-461`); debounce timer (`desktop/aizome/apps/sumi/src/app.rs:747`) → `Autosave` (`:1152-1156`) → `autosave_now` (`:404-413`); `Quit` forces a fresh autosave (`:1280`)
- **Hazards:** `format_dirty` isn't kept in the session, and `restore` for a document without unsaved text reloads it with the detected encoding (`desktop/aizome/apps/sumi/src/app.rs:454`), ignoring the session's `encoding` and `line_ending`; the background reload path doesn't remove the autosave text (`desktop/aizome/apps/sumi/src/app.rs:1134`) as the small-file path does (`:1122`)
- **Reify:** High — nine fields mutated from a dozen sites with representable invalid combinations; suspected bug.
- **Suspected bug:** Change a document's Encoding or Line Endings (the title says "— Edited"), then Quit, which promises everything comes back: on relaunch the document is reloaded in its detected encoding and clean, so the unsaved format change is gone; a Reopen with Encoding choice is lost the same way (confidence: medium)

<a id="smt-18"></a>

### SMT-18 — Background file read generation
- **Where:** `desktop/aizome/apps/sumi/src/doc.rs:200` (`Doc.loading: Option<u64>`); also `desktop/aizome/apps/sumi/src/app.rs:177` (`Sumi.seq`), `desktop/aizome/apps/sumi/src/view.rs:62`
- **Encoding:** a request id in `loading`, echoed by `Msg::Loaded(w, seq, result)`; `path` and title set before the read starts
- **States:** Idle → Loading(seq) → Installed | Failed
- **Transitions:** `read_in_background` sets `loading`, `path`, title (`desktop/aizome/apps/sumi/src/app.rs:349-357`), from `open_path` (`:335-338`) and a big `Reload` (`:1113-1114`); stale answers dropped (`:1130-1132`); `install` clears (`desktop/aizome/apps/sumi/src/doc.rs:277`); failure clears the path and sets `untitled = 1` (`desktop/aizome/apps/sumi/src/app.rs:1135-1142`); edits refused while loading (`:1034-1036`)
- **Hazards:** only `Msg::Edit` is gated: Format commands, Replace All, Shift Right and Link edit the old text during a load and are then overwritten by `install`; the read can't be cancelled (closing the window just drops its answer); the failure path neither resets the title nor checks whether "Untitled" is taken
- **Reify:** High — a request id plus side fields with an inconsistent failure path; suspected bug.
- **Suspected bug:** Open a >2 MiB file that can be stat'ed but not read (a root-only log): after the alert, a window titled with the file's name remains holding an empty "Untitled" document; and a Revert or Reopen with Encoding of a >2 MiB document whose file became unreadable detaches the document from its file (path cleared), whereas the same failure for a small file keeps the path (`desktop/aizome/apps/sumi/src/app.rs:1125`) (confidence: medium)

<a id="smt-19"></a>

### SMT-19 — Find bar and search generation
- **Where:** `desktop/aizome/apps/sumi/src/doc.rs:19` (`Find.shown`, `.replace`, `.query`, `.matches`, `.current`, `.error`, `.version`, `.seq`); also `desktop/aizome/apps/sumi/src/app.rs:615`, `desktop/aizome/apps/sumi/src/view.rs:161`
- **Encoding:** visibility flags; matches valid for content `version`; `seq` drops stale background answers (`Msg::Searched`)
- **States:** Hidden ↔ Shown ↔ Shown+Replace; search Idle → Searching(seq) (on a thread above 1 MiB) → Found(version) | Error; Found turns Stale when `version` ≠ the content's
- **Transitions:** show (`desktop/aizome/apps/sumi/src/app.rs:891-909`); hide (`:910-916`); `search` (`:615-647`); `found` (`:649-676`); `fresh_matches` (`:679-687`); `find_step` (`:690-710`); re-search after an edit only when shown and small (`:748-750`); `install` invalidates (`desktop/aizome/apps/sumi/src/doc.rs:288`)
- **Hazards:** marks go to the editor whether or not the bar is shown (`desktop/aizome/apps/sumi/src/view.rs:161`) and aren't refreshed after edits while it's hidden; above 1 MiB `fresh_matches` only spawns a search, so `find_step` and `replace_one` go on with stale ranges, and `FindQuery` selects the previous query's current match (`desktop/aizome/apps/sumi/src/app.rs:934`); background searches are never cancelled (a full scan per keystroke)
- **Reify:** High — eight fields, nine mutation sites, a sync/async split; suspected bug.
- **Suspected bug:** Find a word, press Done (or Escape), then type above a match: the match highlights stay painted at their old character offsets and drift onto other text; and in a >1 MiB document, ⌘G right after an edit selects a range from the pre-edit match list (confidence: medium)

<a id="smt-20"></a>

### SMT-20 — Incremental highlight states and colouring-pass scheduling
- **Where:** `desktop/aizome/apps/sumi/src/syntax/cache.rs:41` (`Highlight.states`, `.valid`, `.pending`, `.guess`, `.incomplete` :55, `.scheduled` :57, `.pump` :60, `.notify` :67); also `desktop/aizome/apps/sumi/src/app.rs:181` (`Sumi.highlight_soon`), `desktop/aizome/apps/sumi/src/view.rs:22`
- **Encoding:** `Arc<Mutex<Highlight>>` per document: a valid prefix, pending edit regions and guesses; three pass flags in the cache and a coalescing flag in the app; `Msg::HighlightSoon` → 4 ms timer → `Msg::Highlight`
- **States:** Complete → Incomplete (a line guessed this pass) → Scheduled (notify sent) → Pumping (next `state_at` spends the budget) → Complete | Incomplete; an edit moves `valid` back
- **Transitions:** `edited` (`desktop/aizome/apps/sumi/src/syntax/cache.rs:107-140`); `advance` (`:144-193`); `state_at` takes `pump`, sets `incomplete`, starts guesses (`:226-248`); `reset` on a language change keeps notify, budget and rate (`:93-99`); the highlighter schedules (`desktop/aizome/apps/sumi/src/view.rs:28-33`); notify installed per window (`desktop/aizome/apps/sumi/src/app.rs:262-267`); `HighlightSoon` coalesces (`:1157-1165`); `Highlight` clears flags, sets `pump` and bumps `styles` for every document (`:1166-1176`)
- **Hazards:** the "one budget a pass" gate (`incomplete`) is cleared only by `Msg::Highlight`, so a document without `notify` would guess forever; every pass bumps `styles` of all open documents, re-styling every window to finish one; `pump` lingers on documents not redrawn
- **Reify:** High — five flags across three files and six mutation sites, crossing into the kit's editor.
- **Related:** the kit's text editor calling the highlighter per visible line (`desktop/aizome/crates/aizome-kit/src/widgets/text_editor.rs`)

<a id="smt-21"></a>

### SMT-21 — (test harness) background-work settle loop and crash simulation
- **Where:** `desktop/aizome/apps/sumi/tests/sumi.rs:76` (`settle`); also `desktop/aizome/apps/sumi/tests/sumi.rs:354`
- **Encoding:** a poll loop with a 30 s deadline pumping `Msg::Nothing`; `std::mem::forget(h)` to skip `Drop`'s flush
- **States:** Waiting → Done | TimedOut (panic)
- **Transitions:** loop (`desktop/aizome/apps/sumi/tests/sumi.rs:77-82`); "crash" (`:354`), then 300 ms for the autosave thread (`:356`)
- **Hazards:** the 300 ms sleep assumes the autosave worker finishes in time
- **Reify:** Low — test harness.

<a id="smt-22"></a>

### SMT-22 — Per-position forecast fetch, cache and refresh
- **Where:** `desktop/aizome/apps/tenki/src/app.rs:30` (`Weather.forecast`, `.fetched_at`, `.loading`, `.error`, `.tried_at`; `Tenki.weather` :127); also `desktop/aizome/apps/tenki/src/store.rs:95`, `desktop/aizome/apps/tenki/src/ui/mod.rs:175`
- **Encoding:** a map keyed by position (0.01°); age limits `STALE`, `HOURLY`, `RETRY` (`desktop/aizome/apps/tenki/src/app.rs:20-24`); a minute timer; cache files `forecast-<key>.json`
- **States:** Empty → Loading → Fresh (younger than the limit) → Stale → Loading (old forecast kept) → Fresh | Failed(error, tried_at, maybe an old forecast) → retried after `RETRY`
- **Transitions:** cache load (`desktop/aizome/apps/tenki/src/app.rs:231-244`); refresh gate and arm (`:289-296`); a sequential batch thread (`:309-322`) or inline (`:302-307`); `Fetched` (`:614-635`); the minute tick (`:498`, `636-641`); `Refresh` with stale 0 bypasses `RETRY` (`:605-608`); launch (`:504-506`)
- **Hazards:** one uncancellable thread per batch, a request at a time with 20 s timeouts (`desktop/aizome/apps/tenki/src/services.rs:84`), so N unreachable places can show "Updating…" for up to N × 20 s; `weather` is never pruned, and `Fetched` re-creates entries and writes cache files for places removed mid-batch (`desktop/aizome/apps/tenki/src/app.rs:615`, `626-628`) after `prune_forecasts` ran (`:250`); `loading` and `error` coexist during a retry (handled by `desktop/aizome/apps/tenki/src/ui/mod.rs:179-187`)
- **Reify:** Medium — a coherent struct with guarded combinations; an enum would make Loading and Failed exclusive.
- **Related:** Open-Meteo's forecast API over `ureq` (`desktop/aizome/apps/tenki/src/services.rs:92`)

<a id="smt-23"></a>

### SMT-23 — Place search debounce, generation and early Return
- **Where:** `desktop/aizome/apps/tenki/src/app.rs:43` (`Search.text`, `.generation`, `.busy`, `.results`, `.answered`, `.error`, `.add_first`; `Tenki.search` :128); also `desktop/aizome/apps/tenki/src/ui/mod.rs:117`
- **Encoding:** a generation counter echoed by `Msg::RunSearch(g)` and `Msg::Searched(g, …)`; a 250 ms debounce timer; `add_first` for Return typed before the answer
- **States:** Empty/Short → Debouncing(g) → Busy(g) → Answered(text, results) | Failed(text, error); with AddFirst armed, the answer adds its first place
- **Transitions:** a text change resets everything and arms the debounce (`desktop/aizome/apps/tenki/src/app.rs:539-557`); `RunSearch` (`:558-562`) → `run_search` (`:442-454`); `Searched` (`:563-582`); `AddFirst` (`:588-600`); `Add` → `add` resets under a new generation (`:583-587`, `437`); `ClearSearch` (`:601-603`)
- **Hazards:** seven fields must be reset together on every change (the comment at `desktop/aizome/apps/tenki/src/app.rs:546-549` records the busy-release trap); if an early-Return search answers before the 250 ms debounce fires, `RunSearch` finds its generation current and `busy` false and sends the same request again; superseded requests run to completion; `Msg::Add(i)` indexes `results` as rendered
- **Reify:** High — seven fields and eight handlers encode one request lifecycle.
- **Related:** Open-Meteo's geocoding API (`desktop/aizome/apps/tenki/src/places.rs:7`)

<a id="smt-24"></a>

### SMT-24 — Location consent and GeoClue lookup
- **Where:** `desktop/aizome/apps/tenki/src/app.rs:61` (`enum Locating`; `Tenki.current` :120, `.locating` :121, `.location_generation` :122, `.location_cancel` :123); also `desktop/aizome/apps/tenki/src/store.rs:186` (`enum Consent`), `desktop/aizome/apps/tenki/src/store.rs:180` (`State.located`)
- **Encoding:** partially explicit: persisted `Consent {Ask, Allowed, Declined}` in `tenki.toml`, `Locating {Idle, Busy, Failed}`, a generation counter and a per-request `Cancellation`, the last position in memory and in `state.json`
- **States:** Ask → Allowed{Idle | Busy | Failed} | Declined; position None | Some; representable: Declined with a position kept, Failed with an old position shown
- **Transitions:** position read at start (`desktop/aizome/apps/tenki/src/app.rs:148`); `launched` locates when allowed (`:499-501`); `locate` (`:367-378`); `cancel_location` (`:380-384`); `decline_location` (`:386-392`); removing the current row (`:412-415`); `AllowLocation` (`:647-657`); `ToggleLocation`, `SearchInstead` (`:659-670`); `Refresh` locates only without a position (`:609-611`); `Located` with the generation check (`:671-692`); `Drop` cancels (`:476-480`)
- **Hazards:** declining never clears `current`, and `save_state` (`desktop/aizome/apps/tenki/src/app.rs:254-258`) rewrites it to `state.json` on every selection; the position is refreshed only at launch, Allow and Try Again (the minute tick never re-locates), and a failed re-lookup with an old position shows the old place's weather with no failure note (`desktop/aizome/apps/tenki/src/ui/mod.rs:87-93`); Busy relies on the shared client's 20 s deadline (`desktop/aizome/apps/tenki/src/services.rs:131`)
- **Reify:** High — consent, lookup and position over six fields in two files with representable invalid combinations; suspected bug.
- **Suspected bug:** Allow location, let it resolve, then choose Not Now or turn Show Current Location off: the last GeoClue position stays in `state.json` and is written again whenever a place is selected, although the tests take care that answers arriving after a decline are not saved (`desktop/aizome/apps/tenki/tests/tenki.rs:596`) (confidence: medium)
- **Related:** the GeoClue client and `Cancellation` in `desktop/aizome/crates/aizome-sys/src/location.rs`; the GeoClue 2 D-Bus service

<a id="smt-25"></a>

### SMT-25 — Time-zone city bootstrap
- **Where:** `desktop/aizome/apps/tenki/src/app.rs:137` (`Tenki.timezone_busy`, `.timezone_tried` :138)
- **Encoding:** a busy flag and the last attempt's time (0 for never)
- **States:** NotNeeded (places kept, or location allowed) | Idle → Busy → Added | RetryAfter(RETRY)
- **Transitions:** `ensure_timezone_city` (`desktop/aizome/apps/tenki/src/app.rs:333-350`), called at launch, each minute, on refresh, decline and remove (`:502`, `641`, `612`, `391`, `426`); `TimezoneFound` (`:693-708`)
- **Hazards:** the answer is applied whenever `places` is still empty, even if location was allowed meanwhile; `timezone_tried == 0` doubles as "never tried"
- **Reify:** Low — two fields, two sites.

<a id="smt-26"></a>

### SMT-26 — Kept places order and selection persistence
- **Where:** `desktop/aizome/apps/tenki/src/app.rs:125` (`Tenki.selected`, `.places` :118); also `desktop/aizome/apps/tenki/src/store.rs:116`
- **Encoding:** `places.json` (order), `state.json` `selected` id, `fix_selection` repair
- **States:** Selected(id among the rows) | Dangling (repaired) | None
- **Transitions:** load (`desktop/aizome/apps/tenki/src/app.rs:146-158`) and repair (`:171`, `223-229`); `select` saves (`:352-359`); `move_row` (`:394-406`); `remove_row` (`:417-424`); `add` (`:432-439`); `AllowLocation` selects the current row (`:650-651`); `TimezoneFound` (`:702-705`)
- **Hazards:** `selected` may name the current row while consent isn't Allowed until a repair point runs
- **Reify:** Low — a persisted value with one repair function.

<a id="smt-27"></a>

### SMT-27 — Settings sheet
- **Where:** `desktop/aizome/apps/tenki/src/app.rs:135` (`Tenki.settings_sheet`); also `desktop/aizome/apps/tenki/src/ui/mod.rs:762`
- **Encoding:** `Option<WindowKey>`
- **States:** Closed → Open(key)
- **Transitions:** `ShowSettings` (`desktop/aizome/apps/tenki/src/app.rs:710-715`); `CloseSettings` from Done/Return/Escape (`desktop/aizome/apps/tenki/src/app.rs:716-720`, `desktop/aizome/apps/tenki/src/ui/mod.rs:762`)
- **Hazards:** Tenki has no `close_requested`, so any other way of closing the sheet would leave the key stale and make `ShowSettings` a no-op; today the kit sends close requests to the parent and refuses them while a sheet is up
- **Reify:** Low — one Option.

<a id="smt-28"></a>

### SMT-28 — (test harness) recorded Open-Meteo and GeoClue services
- **Where:** `desktop/aizome/apps/tenki/src/services.rs:146` (`Recorded.state: RecordedState {now, offline, located, timezone, asked}` :151)
- **Encoding:** a mutex-held fake clock, offline switch, scripted location answer and request log; `inline()` true runs all work on the program's thread
- **States:** Online | Offline; clock as the test sets it
- **Transitions:** setters (`desktop/aizome/apps/tenki/src/services.rs:194-209`); `read` (`:216-222`); calls logged (`:237-243`); inline mode (`:263-265`, used `desktop/aizome/apps/tenki/src/app.rs:270-271`, `302-307`)
- **Hazards:** inline mode queues answers before the requesting update returns, a different interleaving from the threaded app
- **Reify:** Low — test fixture.

<a id="smt-29"></a>

### SMT-29 — Deferred file writer with per-path coalescing
- **Where:** `desktop/aizome/apps/tenki/src/store.rs:209` (`writer()` channel in a `OnceLock`; `enum Job` :203; `Store.deferred` :37); also `desktop/aizome/apps/tenki/src/main.rs:48`
- **Encoding:** a process-wide writer thread writing the latest text per path in each batch; a flush rendezvous with a 5 s limit
- **States:** NotStarted → Running; per path Queued → Written; Flushed
- **Transitions:** start on first use (`desktop/aizome/apps/tenki/src/store.rs:209-216`); enqueue, or write at once if the thread is gone (`:75-83`); batch write (`:218-243`); `flush` (`:246-252`), at exit (`desktop/aizome/apps/tenki/src/main.rs:48`)
- **Hazards:** `prune_forecasts` deletes and `load_*` read synchronously (`desktop/aizome/apps/tenki/src/store.rs:104-114`, `89-93`) while writes are deferred, so a queued forecast write can land after its prune; `flush` gives up after 5 s (`:250`) and the process exits; `*.tmp<pid>` files from a crash (`:262`) are never pruned
- **Reify:** Low — coalescing is centralised in `write_from`.

<a id="smt-30"></a>

### SMT-30 — Place list press, drag and hover gesture
- **Where:** `desktop/aizome/apps/tenki/src/ui/widgets.rs:118` (`ListState.press`, `.drag: Option<Dragging>`, `.hover`; `Dragging` :127); also `desktop/aizome/apps/tenki/src/ui/mod.rs:76`
- **Encoding:** widget state kept by the kit across rebuilds; row indices and pointer positions
- **States:** Idle → Pressed(row, start) → Dragging{from, grab, y} → Idle (drop emits Move) | Cancelled (Escape); hover None | Row
- **Transitions:** left press selects, arms and captures (`desktop/aizome/apps/tenki/src/ui/widgets.rs:256-264`); moving past the 4 pt slop starts the drag (`:279-288`); release emits Move if it moved (`:295-310`); Escape cancels (`:326-335`); right press selects and opens the menu (`:266-271`); leave clears hover (`:312-317`)
- **Hazards:** `from`, `to` and the context menu's `Msg::Move/Remove(i)` (`desktop/aizome/apps/tenki/src/ui/mod.rs:76-83`) are row indices fixed at press or open time, wrong if the rows change underneath (the location row appearing or going); the kit has no pointer-cancel event, so a lost release leaves `press`/`drag` armed until the next release; pressing to drag first selects the row
- **Reify:** Medium — a classic gesture machine in Options; an enum would make Pressed and Dragging exclusive.

<a id="smt-31"></a>

### SMT-31 — (test harness) private D-Bus daemon and fake GeoClue client
- **Where:** `desktop/aizome/apps/tenki/tests/geoclue.rs:17` (`TestBus.child`, `.address`, `.dir`; `Client.running` :114, `.start_delay` :109; `StopGate` :117)
- **Encoding:** a spawned `dbus-daemon` killed on drop; a fake client whose `running` flag gates the Location property; Stop rendezvous channels
- **States:** bus NotStarted → Running(address) → Killed; client Stopped → Started → Stopped; Stop Entered → Released
- **Transitions:** `TestBus::start` (`desktop/aizome/apps/tenki/tests/geoclue.rs:24-73`); `Drop` (`:83-89`); `Client::start` (`:124-132`); `stop` through the gate (`:134-140`); the Location property (`:143-145`)
- **Hazards:** none observed
- **Reify:** Low — test harness.
- **Related:** `desktop/aizome/crates/aizome-sys/src/location.rs` (the client under test)

<a id="smt-32"></a>

### SMT-32 — (test harness) held search replies and drain loops
- **Where:** `desktop/aizome/apps/tenki/tests/search_generation.rs:18` (`Delayed.requests`; `until` :68, `drain_reply` :80)
- **Encoding:** each search blocks on its own reply channel (5 s, then Offline); poll loops with 2 s deadlines, `drain_reply` proving handling through update stats
- **States:** request Held → Answered | TimedOut; loop Polling → Done | Panic
- **Transitions:** `search` (`desktop/aizome/apps/tenki/tests/search_generation.rs:28-32`); `until` (`:68-78`); `drain_reply` (`:80-94`)
- **Hazards:** none observed
- **Reify:** Low — test harness.

<a id="smt-33"></a>

### SMT-33 — (test harness) pending location service and poll loop
- **Where:** `desktop/aizome/apps/tenki/tests/tenki.rs:501` (`PendingLocation.requests`); poll loop `desktop/aizome/apps/tenki/tests/tenki.rs:563`
- **Encoding:** each `locate` hands the test its `Cancellation` and a reply sender, then waits 3 s (then TimedOut)
- **States:** Pending → Answered | Cancelled | TimedOut
- **Transitions:** `locate` (`desktop/aizome/apps/tenki/tests/tenki.rs:516-520`); polling for the second answer (`:563-567`)
- **Hazards:** none observed
- **Reify:** Low — test harness.

## Already explicit

- `desktop/aizome/apps/sumi/src/syntax/mod.rs:258` `enum Mode` — the code lexer's state carried to the next line (block comment depth, string, raw string, Nix indented string); transitions centralised in `fn lex` (`desktop/aizome/apps/sumi/src/syntax/code.rs:716`) through `fn normal` (`desktop/aizome/apps/sumi/src/syntax/code.rs:824`)
- `desktop/aizome/apps/sumi/src/syntax/mod.rs:273` `enum Block` (in `LineState`, `desktop/aizome/apps/sumi/src/syntax/mod.rs:286`) — Markdown fences, front matter and HTML comments spanning lines; transitions centralised in `fn line` (`desktop/aizome/apps/sumi/src/syntax/markdown.rs:148`)
- `desktop/aizome/apps/myaku/src/app.rs:21` `enum Show` — which processes the table lists; transitions centralised in `update`'s `Msg::Show` arm (`desktop/aizome/apps/myaku/src/app.rs:510`)

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/apps/myaku/examples/groups.rs` | 37 | none |  | one-shot attribution probe CLI |
| `desktop/aizome/apps/myaku/src/app.rs` | 733 | machines | [SMT-01](#smt-01), [SMT-02](#smt-02), [SMT-03](#smt-03), [SMT-04](#smt-04), [SMT-05](#smt-05), [SMT-07](#smt-07) | window state and message handlers; Show is explicit |
| `desktop/aizome/apps/myaku/src/fixture.rs` | 409 | none |  | deterministic fixture data generator |
| `desktop/aizome/apps/myaku/src/group.rs` | 966 | machines | [SMT-11](#smt-11) | pure grouping; fingerprint() keys SMT-11 cache |
| `desktop/aizome/apps/myaku/src/history.rs` | 161 | none |  | fixed ring buffers of graph history |
| `desktop/aizome/apps/myaku/src/instance.rs` | 55 | machines | [SMT-05](#smt-05) | single-instance socket claim and serve |
| `desktop/aizome/apps/myaku/src/lib.rs` | 23 | none |  | module declarations |
| `desktop/aizome/apps/myaku/src/main.rs` | 104 | machines | [SMT-05](#smt-05) | claims instance; --print one-shot sampling |
| `desktop/aizome/apps/myaku/src/model.rs` | 717 | none |  | plain data and formatting; value enums |
| `desktop/aizome/apps/myaku/src/nvml.rs` | 442 | machines | [SMT-06](#smt-06) | NVML session and sample cursor |
| `desktop/aizome/apps/myaku/src/sampler.rs` | 1127 | machines | [SMT-06](#smt-06), [SMT-07](#smt-07), [SMT-08](#smt-08), [SMT-09](#smt-09), [SMT-10](#smt-10), [SMT-11](#smt-11), [SMT-12](#smt-12) | sampler thread and its caches; Rapl partially explicit |
| `desktop/aizome/apps/myaku/src/ui/columns.rs` | 307 | none |  | column tables and sort comparators |
| `desktop/aizome/apps/myaku/src/ui/energy.rs` | 123 | machines | [SMT-04](#smt-04) | meter window view and Close |
| `desktop/aizome/apps/myaku/src/ui/gpu.rs` | 150 | machines | [SMT-01](#smt-01) | card strip emits Gpu and HoverGpu |
| `desktop/aizome/apps/myaku/src/ui/graph.rs` | 232 | none |  | paint-only graph widget |
| `desktop/aizome/apps/myaku/src/ui/inspector.rs` | 321 | machines | [SMT-02](#smt-02), [SMT-03](#smt-03) | inspector view emits Select AskQuit Pause |
| `desktop/aizome/apps/myaku/src/ui/mod.rs` | 8 | none |  | module declarations |
| `desktop/aizome/apps/myaku/src/ui/view.rs` | 528 | machines | [SMT-02](#smt-02), [SMT-03](#smt-03), [SMT-07](#smt-07) | context menu row index; notice display |
| `desktop/aizome/apps/myaku/tests/energy.rs` | 133 | none |  | tests |
| `desktop/aizome/apps/myaku/tests/gpu.rs` | 329 | none |  | tests |
| `desktop/aizome/apps/myaku/tests/look.rs` | 131 | none |  | tests (goldens) |
| `desktop/aizome/apps/myaku/tests/perf.rs` | 122 | none |  | tests (frame budget) |
| `desktop/aizome/apps/myaku/tests/ui.rs` | 266 | none |  | tests |
| `desktop/aizome/apps/sumi/src/app.rs` | 1304 | machines | [SMT-05](#smt-05), [SMT-13](#smt-13), [SMT-14](#smt-14), [SMT-15](#smt-15), [SMT-16](#smt-16), [SMT-17](#smt-17), [SMT-18](#smt-18), [SMT-19](#smt-19), [SMT-20](#smt-20) | document windows and message handlers |
| `desktop/aizome/apps/sumi/src/autosave.rs` | 225 | machines | [SMT-16](#smt-16) | autosave worker thread and session file |
| `desktop/aizome/apps/sumi/src/doc.rs` | 415 | machines | [SMT-17](#smt-17), [SMT-18](#smt-18), [SMT-19](#smt-19) | document and find-bar state |
| `desktop/aizome/apps/sumi/src/files.rs` | 429 | machines | [SMT-17](#smt-17) | pure codecs and atomic save; stamp() feeds conflict check |
| `desktop/aizome/apps/sumi/src/find.rs` | 151 | none |  | pure search and replace functions |
| `desktop/aizome/apps/sumi/src/format.rs` | 684 | none |  | pure Markdown edit functions |
| `desktop/aizome/apps/sumi/src/instance.rs` | 72 | machines | [SMT-05](#smt-05) | single-instance socket claim and serve |
| `desktop/aizome/apps/sumi/src/lib.rs` | 20 | none |  | module declarations |
| `desktop/aizome/apps/sumi/src/main.rs` | 34 | machines | [SMT-05](#smt-05) | claims instance |
| `desktop/aizome/apps/sumi/src/menus.rs` | 188 | machines | [SMT-15](#smt-15) | menus derived from focused_doc |
| `desktop/aizome/apps/sumi/src/recent.rs` | 307 | none |  | xbel text editing; pure functions |
| `desktop/aizome/apps/sumi/src/settings.rs` | 73 | none |  | plain configuration load and save |
| `desktop/aizome/apps/sumi/src/syntax/cache.rs` | 395 | machines | [SMT-20](#smt-20) | line-state cache and pass flags |
| `desktop/aizome/apps/sumi/src/syntax/code.rs` | 1225 | explicit |  | Mode transitions in lex; keyword tables |
| `desktop/aizome/apps/sumi/src/syntax/markdown.rs` | 645 | explicit |  | Block transitions in line |
| `desktop/aizome/apps/sumi/src/syntax/mod.rs` | 373 | explicit |  | Mode, Block and LineState enums |
| `desktop/aizome/apps/sumi/src/view.rs` | 295 | machines | [SMT-14](#smt-14), [SMT-18](#smt-18), [SMT-19](#smt-19), [SMT-20](#smt-20) | marks, highlighter closure, sheet views |
| `desktop/aizome/apps/sumi/tests/sumi.rs` | 647 | machines | [SMT-21](#smt-21) | settle loop and crash simulation |
| `desktop/aizome/apps/tenki/src/app.rs` | 785 | machines | [SMT-05](#smt-05), [SMT-22](#smt-22), [SMT-23](#smt-23), [SMT-24](#smt-24), [SMT-25](#smt-25), [SMT-26](#smt-26), [SMT-27](#smt-27) | program state and message handlers |
| `desktop/aizome/apps/tenki/src/forecast.rs` | 433 | none |  | pure forecast parsing and derivation |
| `desktop/aizome/apps/tenki/src/instance.rs` | 83 | machines | [SMT-05](#smt-05) | single-instance socket claim and serve |
| `desktop/aizome/apps/tenki/src/lib.rs` | 22 | none |  | module declarations |
| `desktop/aizome/apps/tenki/src/location.rs` | 83 | none |  | error mapping over aizome-sys GeoClue client |
| `desktop/aizome/apps/tenki/src/main.rs` | 49 | machines | [SMT-05](#smt-05), [SMT-29](#smt-29) | claims instance; flushes writer at exit |
| `desktop/aizome/apps/tenki/src/places.rs` | 215 | none |  | place types and pure parsing |
| `desktop/aizome/apps/tenki/src/services.rs` | 297 | machines | [SMT-28](#smt-28) | live HTTP with timeouts; recorded fake services |
| `desktop/aizome/apps/tenki/src/store.rs` | 337 | machines | [SMT-24](#smt-24), [SMT-26](#smt-26), [SMT-29](#smt-29) | persisted files; Consent enum; writer thread |
| `desktop/aizome/apps/tenki/src/time.rs` | 239 | none |  | pure date and clock formatting |
| `desktop/aizome/apps/tenki/src/ui/mod.rs` | 768 | machines | [SMT-22](#smt-22), [SMT-23](#smt-23), [SMT-24](#smt-24), [SMT-27](#smt-27), [SMT-30](#smt-30) | views of fetch, search, location state; row menus |
| `desktop/aizome/apps/tenki/src/ui/widgets.rs` | 974 | machines | [SMT-30](#smt-30) | place list drag gesture; other widgets paint-only |
| `desktop/aizome/apps/tenki/src/units.rs` | 130 | none |  | pure unit conversion |
| `desktop/aizome/apps/tenki/src/weather.rs` | 273 | none |  | pure code and colour tables |
| `desktop/aizome/apps/tenki/tests/geoclue.rs` | 286 | machines | [SMT-31](#smt-31) | private bus and fake GeoClue |
| `desktop/aizome/apps/tenki/tests/search_generation.rs` | 178 | machines | [SMT-32](#smt-32) | held search replies |
| `desktop/aizome/apps/tenki/tests/tenki.rs` | 617 | machines | [SMT-33](#smt-33) | pending location service |
