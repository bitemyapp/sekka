# STB: Settei: app shell, appearance, Bluetooth and displays

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/apps/settei`. Audited at `8e0acbe5`; line numbers refer to that commit.

42 files audited. 20 implicit machines: 4 High, 8 Medium, 8 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

<a id="stb-09"></a>

### STB-09 — Bluetooth pane activity: pairing agent, discovery and discoverability while shown
- **Where:** `desktop/aizome/apps/settei/src/panes/bluetooth.rs:259` (`Bluetooth.made_discoverable`, `.agent` `:260`, `.showing` `:262`, `.looking` `:263`, `.pinned` `:267`)
- **Encoding:** four bools and the pinned adapter path, set against the adapter's `Discoverable`/`Discovering` in BlueZ and the agent's registration
- **States:** `Hidden` → `Shown(adapter unknown or off)` → `Looking` (+ `MadeDiscoverable`) → `Hidden`; agent `Unregistered` → `Registering` (`agent = true`) → `Registered` | `Failed`. Representable: `looking` true after the adapter was powered off
- **Transitions:** `shown` sets `showing`, clears `looking` (`bluetooth.rs:416-417`), registers the agent if `!agent` (`bluetooth.rs:434-455`) and looks (`bluetooth.rs:456`); `look` makes the adapter discoverable if it isn't and starts discovery (`bluetooth.rs:308-315`); `system_changed` retries `look` until it takes (`bluetooth.rs:461-463`); `Power` pins the adapter (`bluetooth.rs:498`); `Powered(true)` looks (`bluetooth.rs:504`); `Agent(Err)` clears `agent` (`bluetooth.rs:618-622`); `hidden` stops discovery, undoes discoverability, unregisters the agent (`bluetooth.rs:475-491`)
- **Hazards:** `made_discoverable` is set only if the adapter wasn't discoverable when looked at, so a run that finds it already discoverable never turns it off; powering off leaves `looking = true`, so powering on from elsewhere doesn't restart discovery while the pane shows; a late `Agent(Err)` from an earlier showing clears `agent` while a newer registration is live (registering replaces the old one, so this only costs a redundant re-register)
- **Reify:** High — five fields across six handlers, with an effect on privacy (discoverable)
- **Suspected bug:** quitting Settei while Bluetooth shows never calls `hidden` (STB-01), so the adapter stays discoverable after Settei is gone until BlueZ's `DiscoverableTimeout` expires (never, where it is 0), and the next run, finding it already discoverable, doesn't take it over (`bluetooth.rs:308`) (confidence: medium)
- **Related:** `desktop/aizome/crates/aizome-sys/src/bluetooth/mod.rs` (`set_discoverable`, `start_discovery`, `register_agent`), `desktop/aizome/crates/aizome-sys/src/bluetooth/agent.rs` (`Registration` unregisters on drop)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#stb-09)

<a id="stb-10"></a>

### STB-10 — Pairing questions: prompt, responders and per-device busy
- **Where:** `desktop/aizome/apps/settei/src/panes/bluetooth.rs:254` (`Bluetooth.busy`, `.prompt` `:255`, `.responders` `:256`, `.next_prompt` `:257`)
- **Encoding:** `prompt: Option<Prompt>` (the question shown and the code typed), `responders: Arc<Mutex<HashMap<u64, Responder>>>` filled on the agent's thread, `busy` (the device paths with a task of ours running), the sample's id counter, and the window's sheet (STB-02)
- **States:** per device `Idle` → `Busy` → `Idle`; question `None` → `Asking(id, request, typed)` → `Answered` | `Superseded` | `Cancelled` | `Moot` (pairing ended, or the device paired by itself)
- **Transitions:** `Connect`/`Disconnect`/`Pair` mark the device busy (`bluetooth.rs:521`, `:526`, `:531`); the sample's `Pair` makes up a prompt (`bluetooth.rs:540-548`); the agent thread stores the responder, then sends `Prompted` (`bluetooth.rs:443-451`); `Prompted(Cancelled)` closes that device's question (`bluetooth.rs:588-594`); any other `Prompted` drops the old responder (which rejects) and opens the sheet (`bluetooth.rs:601-607`); `PromptTyped` (`bluetooth.rs:612-616`); `Answer`, or `back`, → `answer` responds, cancels a display-only pairing, clears busy on reject (`bluetooth.rs:332-370`, `:753-755`); `Done` clears busy and closes our pairing's question (`bluetooth.rs:625-631`); `system_changed` closes a shown code once the device has paired (`bluetooth.rs:466-472`)
- **Hazards:** a `Cancelled` prompt's `Responder::none` is stored under a fresh id that nothing removes, so `responders` leaks one entry per cancel; a question from device B silently rejects A's; `busy` clears only when the task returns, so a task that never completes spins forever; `close_prompt` closes the window's sheet whoever owns it (STB-02)
- **Reify:** High — five handlers and an agent thread share the map, and "dropped means rejected" is an implicit transition
- **Related:** `desktop/aizome/crates/aizome-sys/src/bluetooth/agent.rs` (BlueZ `Agent1`; a dropped `Responder` rejects)

<a id="stb-13"></a>

### STB-13 — Display change: apply, "Keep these display settings?" countdown, keep or revert
- **Where:** `desktop/aizome/apps/settei/src/panes/displays/mod.rs:110` (`Displays.applying`, `.revision` `:111`, `.keep: Option<Keep>` `:112`); `Keep` at `desktop/aizome/apps/settei/src/panes/displays/mod.rs:49` (`before`, `before_main`, `names`, `left`, `timer`); also `desktop/aizome/apps/settei/src/panes/displays/system.rs:168` (`Compositor::apply`), `desktop/aizome/apps/settei/src/panes/displays/view.rs:395` (`keep_sheet`)
- **Encoding:** the `applying` bool, the `revision` counter and the `keep` option, which carries the old settings and a repeating 1 s timer; the sheet (STB-02); the settings file is written only on Keep
- **States:** `Idle` → `Applying` → `Idle` (persisted) | `Keeping(left)` → `Kept` (persisted) | `Reverting` → `Applying(revert)` → `Idle`; `Applying` → `Failed` → `Reading`. Representable: `keep` without a timer; a revert's `Applying` with nothing left to fall back to
- **Transitions:** `change` refuses while applying or keeping (`displays/mod.rs:214-217`); `commit` sets `applying`, bumps `revision`, assumes the new settings, spawns `apply` (`displays/mod.rs:263-269`); `Applied Ok` clears `applying` (`displays/mod.rs:555`), then either arms `every(1 s, Tick)`, `keep` and the sheet (`displays/mod.rs:578-581`) or persists (`displays/mod.rs:583`); `Applied Err` bumps `revision` and reads again (`displays/mod.rs:587-591`); `Tick` counts down and reverts at 0 (`displays/mod.rs:594-605`); `Keep` → `end_keep` + persist (`displays/mod.rs:607-611`); `Revert`/`back` → `revert` → `commit(confirm = false, persist = false)` (`displays/mod.rs:330-344`, `:612`, `:626-628`)
- **Hazards:** the countdown lives only in Settei: no `hidden`/quit hook ends it, and the compositor has no timeout of its own; `Compositor::apply` sends the requests one by one and returns early on error (`system.rs:180-182`), so an I/O error mid-sequence, or the 10 s read timeout (`system.rs:179`), returns `Err` after some requests already applied, and the error path starts no countdown; a revert whose apply fails isn't retried, leaving the risky settings live (the file still has the old ones); `Loaded` is guarded by `applying`/`revision` but not by `keep` (`displays/mod.rs:496`), so a read that finishes during the countdown (the pane shown again) takes `main` from the file's old `primary`, and Keep then persists the old main display
- **Reify:** High — three or more fields, eight handlers, a timer, and a property that protects against a blank screen
- **Suspected bug:** closing the window or quitting (⌘Q) during the 15 s countdown exits without `revert`, leaving a resolution, rotation, mirror or off change applied in the compositor though it was never kept (and isn't in the file) (confidence: medium). Also: a modeset slower than the 10 s read timeout returns `Err`, so no countdown starts and the change stays live, unconfirmed and unpersisted (confidence: low)
- **Related:** `output-config`/`mirror-output` handling in `desktop/aizome/crates/aizome-comp-core/src/requests.rs` and `desktop/aizome/crates/aizome-comp-core/src/outputs.rs` (`configure_output`)
- **Reproduction:** Partly confirmed (integration): [summary](reproductions.md#stb-13)

<a id="stb-17"></a>

### STB-17 — settings.toml write pipeline: ordered writer, pending count, reload debounce
- **Where:** `desktop/aizome/apps/settei/src/shared.rs:37` (`Shared.reload_pending`, `.writer: Option<Writer>` `:38`); `Writer` at `desktop/aizome/apps/settei/src/shared.rs:204` (`sent`, the `done` counter and its condvar); also `desktop/aizome/apps/settei/src/main.rs:52`
- **Encoding:** a writer thread, started lazily and fed by a channel; `sent` (main thread) against `done` (writer) gives `pending()`; `reload_pending` debounces `reload-config` to one per 120 ms; the file's mtime and length, polled each second by `aizome-config`'s watcher
- **States:** `NoWriter` → `Idle` (`pending == 0`) ⇄ `Writing(n)`; reload `Idle` → `Scheduled` → `Requested`; file `Fresh` | `OursPending` | `ExternalUnseen`
- **Transitions:** `change` reads the file fresh only when nothing is pending (`shared.rs:93-107`), applies the edit, starts the writer once (`shared.rs:119`), bumps `sent` and queues the text (`shared.rs:120-121`); the writer writes atomically, bumps `done`, reports `Written` or `Said` (`shared.rs:217-227`); `written` arms the reload once (`shared.rs:127-130`); `reload` clears the flag and asks the compositor (`shared.rs:142-161`); `reread` refuses while anything is pending (`shared.rs:74-76`); `flush` waits for `done == sent` at exit (`shared.rs:134-137`, `main.rs:52`)
- **Hazards:** while writes are pending, `change` builds on the cached settings and `reread` drops the watcher's one notice for that stamp, so an external edit landing mid-burst is neither shown nor merged, and the next queued write overwrites it; the temporary name `settings.toml.tmp` (`shared.rs:256`) is the one `aizome-config`'s own save uses (`desktop/aizome/crates/aizome-config/src/lib.rs:691`), so a concurrent save by the shell or another app can rename the other's half-written file, or fail; if the writer thread fails to spawn (ignored, `shared.rs:216`), `pending()` never drains: rereads stop for good and `flush` hangs at quit
- **Reify:** High — risk of data loss, counters shared across threads, and ordering across processes
- **Suspected bug:** while a slider is dragged (writes pending throughout), the shell writes `settings.toml` (e.g. the Deskbar's edge dragged to a new width). `reread` drops the watcher's notice (`shared.rs:74`), the next `change` uses the cached settings (`shared.rs:95`), and the shell's change is overwritten (confidence: medium)
- **Related:** `aizome_config::Watcher` (`desktop/aizome/crates/aizome-config/src/lib.rs:823`); `reload-config` in `desktop/aizome/crates/aizome-comp-core/src/requests.rs`
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#stb-17)
