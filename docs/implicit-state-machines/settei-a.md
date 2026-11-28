# STA: Settei: general, keyboard, network, sound and other panes

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/apps/settei`. Audited at `8e0acbe5`; line numbers refer to that commit.

29 files audited. 22 implicit machines: 7 High, 3 Medium, 12 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

<a id="sta-05"></a>

### STA-05 — Language & Region formats preview with a stale-locale guard
- **Where:** `desktop/aizome/apps/settei/src/panes/general/language.rs:52` (`State.preview`), `:54` (`.reading`)
- **Encoding:** `preview: Option<(locale, Formats)>`, `reading: Option<locale>`, and the locale localed reports now (`shown_locale`, `general/language.rs:280-282`); results are keyed by locale.
- **States:** `Empty` (both `None`, spinner) → `Reading(l)` → `Shown(l)`; `Shown(l)` with the system locale now `m` → `Reading(m)` (old preview still shown) → `Shown(m)`; a Region choice → `Empty`.
- **Transitions:** `refresh_preview` → Reading (`general/language.rs:313-314`), Shown in the sample (`:311`), no-op when shown or being read (`:307-309`); `Preview(l)` clears `reading` (`:341-343`), then Shown (`:346-347`) or re-reads (`:349`); `Region` → Empty (`:370`); `Done(Ok)` re-reads (`:372-375`); `shown` (`:300`); `system_changed` (`:320-324`, from `general/mod.rs:257-262`, Language page only).
- **Hazards:** `Region` empties the preview before localed has accepted the change, and `Done(Err)` (`general/language.rs:376`) doesn't refresh; `system_changed` reaches the pane only while General is current and on the Language page (`desktop/aizome/apps/settei/src/app.rs:562-566`); `reading` holds one locale, so a read for an older locale still runs and is discarded.
- **Reify:** High — suspected bug; three handlers decide the preview's phase.
- **Suspected bug:** choose a Region format and cancel the polkit prompt (or have localed refuse): `preview` stays `None` and `Done(Err)` doesn't call `refresh_preview`, so the Formats section shows an indeterminate spinner until the page is shown again or localed signals some other change (confidence: high).
- **Related:** systemd-localed through `aizome_sys::datetime::Locale` (`desktop/aizome/crates/aizome-sys/src/datetime`)
- **Reproduction:** Not confirmed (integration): [summary](reproductions.md#sta-05)

<a id="sta-10"></a>

### STA-10 — Keyboard setup: catching the key beside the space bar
- **Where:** `desktop/aizome/apps/settei/src/panes/keyboard/mod.rs:118` (`Keyboard.setup`), `:120` (`.waits`); also `desktop/aizome/apps/settei/src/panes/keyboard/setup.rs:86` (`Setup.waiting`, `.keyboard`, `.beside`, `.error`, `.modifiers`, `.from`, `.unset`, `.chosen`, `.asked`)
- **Encoding:** `setup: Option<Setup>` (sheet open), `Setup.waiting`, `keyboard`/`beside` (key caught), `error`, an `Arc<AtomicU64>` wait counter whose value tags the one live `catch_key` thread (`keyboard/setup.rs:132-134`), and the compositor's single catch slot.
- **States:** `Closed` → `Waiting(n)` → `Caught(keyboard, beside)` | `Retrying(error)` (waiting again) | `Stalled(error)` (IPC failed, not waiting) → `Saved` | `SameAsOthers` | `Cancelled` → `Closed`; representable: `waiting == false` with no key caught and no retry control, `setup.is_some()` while the window's sheet shows something else (STA-12).
- **Transitions:** `SetupOpen` Closed→Waiting (`keyboard/mod.rs:1099-1110`); `wait_for_key` bumps the counter and spawns (`:734-744`); `SetupCaught(n)` dropped unless `n` is current (`:1123`); `caught`: Escape → Closed (`:787-789`), wrong key or unknown keyboard → Retrying (`:794-796`, `:814`), good key → Caught (`:798-811`); IPC error → Stalled (`:1129-1133`); `SetupAgain` Caught→Waiting (`:1142-1148`); `SampleKey` (`:1137-1141`); `SetupKeyboards` fills `unset` (`:1111-1116`); `focus_entry("setup")` opens, or re-chooses the asked keyboard (`:976-994`); `finish_setup` writes the profile, closes and sets `keys_of` (`:757-775`); `close_setup` (`:777-781`); `hidden` → `stop_waiting` (`:967-970`, `:746-751`).
- **Hazards:** `hidden` stops the wait but leaves the sheet open, and the app keeps a pane's sheet up over another pane (`desktop/aizome/apps/settei/src/app.rs:608-614`); nothing restarts the wait; after `Stalled` there is no retry ("Press Again" needs a caught key, `keyboard/mod.rs:662-671`); `SetupKeyboards` replies are untagged, so one from an earlier opening can fill a newer sheet's `unset`; the wait thread checks the counter only every 500 ms, so a key pressed just after Cancel is still swallowed by the compositor and then discarded.
- **Reify:** High — nine fields, a counter shared with a thread, transitions in a dozen handlers.
- **Suspected bug:** while the setup sheet waits, open Settei at another pane (the palette, `aizome-settei --pane …`): `hidden` stops the catch, the sheet stays up over the new pane without "Waiting for the key…", and pressing the key beside the space bar does nothing; only Not Now or Same as Other Keyboards get out (confidence: medium).
- **Related:** `desktop/aizome/crates/aizome-ipc/src/setup.rs:66` (`catch_key`, 500 ms read timeout at `:77`); `desktop/aizome/crates/aizome-comp-core/src/input.rs:476-485` (the compositor swallows the caught key); `desktop/aizome/crates/aizome-shell/src/launch.rs:62` (the shell's setup launch)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#sta-10)

<a id="sta-12"></a>

### STA-12 — Pane sheets against the window's single sheet slot (keyboard's three, network, printers, users)
- **Where:** `desktop/aizome/apps/settei/src/panes/keyboard/mod.rs:108` (`Keyboard.picker`), `:112` (`.command`), `:118` (`.setup`); also `desktop/aizome/apps/settei/src/panes/network/mod.rs:431` (`Network.sheet`), `desktop/aizome/apps/settei/src/panes/printers.rs:145` (`Printers.add`), `desktop/aizome/apps/settei/src/panes/users.rs:45` (`Users.sheet`)
- **Encoding:** each pane's `Option<…>` says what its sheet shows; the window's one sheet is `Settei.sheet: Option<(pane, WindowKey)>` (`desktop/aizome/apps/settei/src/app.rs:108`), driven by `cx.open_sheet()`/`cx.close_sheet()` messages; keyboard shows one of its three by fixed priority picker > setup > command (`keyboard/mod.rs:1311-1319`).
- **States:** `Closed` → `Open(kind)` → `Closed`; representable: two or three keyboard Options `Some` at once; a pane's Option `Some` while the window's sheet belongs to another pane or is closed.
- **Transitions:** keyboard open: `AddLayout` (`keyboard/mod.rs:1004-1008`), `SetupOpen` (`:1099-1110`), `AddCommand` (`:1232-1236`); close: `PickerAdd`/`PickerCancel` (`:1016-1017`, `:1034-1037`), `close_setup` (`:777-781`), `CommandCancel`/`CommandSave` (`:1254-1257`, `:1274-1275`); network `open_sheet`/`close_sheet` (`network/mod.rs:567-570`, `:592-599`); printers `AddPrinter`, `Added`, `CloseSheet`, `back` (`printers.rs:422-426`, `:568-569`, `:580-583`, `:679-682`); users open (`users.rs:285-295`, `:329-340`, `:448-452`) and close (`users.rs:321-322`, `:325-328`, `:379-380`, `:459-460`); the app's `OpenSheet` closes whatever sheet window is up (`desktop/aizome/apps/settei/src/app.rs:581-587`).
- **Hazards:** one pane's `open_sheet` closes another pane's sheet window without telling it, so that pane's Option stays `Some`: a stale printers `add` keeps CUPS polled while hidden (`printers.rs:310`), a stale network `sheet` makes `open_sheet_after` queue behind a sheet nobody sees (`network/mod.rs:575-576`), a stale keyboard `setup` outranks a later command sheet; `focus_entry("setup")` opens the setup sheet whenever `setup` is `None` (`keyboard/mod.rs:980-981`), even with the picker or command sheet open.
- **Reify:** High — invalid combinations reachable from an external launch, with a swallowed-key consequence.
- **Suspected bug:** with Add a Layout's picker open, plug in a keyboard and accept the shell's setup prompt (`aizome-settei --pane keyboard.setup --keyboard …`): `SetupOpen` starts a compositor key catch but `sheet()` still shows the picker, so the next key typed into the layout search is swallowed and taken as the setup key; after the picker closes, "Add a Command…" shows the leftover setup sheet instead of the command sheet (confidence: medium).
- **Related:** `desktop/aizome/apps/settei/src/app.rs:581` (window sheet slot); `desktop/aizome/crates/aizome-shell/src/launch.rs:62` (setup launch)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#sta-12)

<a id="sta-13"></a>

### STA-13 — Wi-Fi join: one at a time, the sheet reopened as it was on failure
- **Where:** `desktop/aizome/apps/settei/src/panes/network/mod.rs:433` (`Network.joining`), `:436` (`.tried`), `:439` (`.queued`), `:431` (`.sheet`); `enum Sheet` at `:235` (partially explicit)
- **Encoding:** `joining: Option<ssid>`, `tried: Option<(ssid, Sheet)>` (what was typed, to reopen), `queued: Option<Sheet>` (one sheet waiting for the open one to close), `sheet`, and the `retry` flag carried in `Msg::Joined`; rows derive spinners and disabled presses from `joining` (`network/view.rs:304`, `:326`, `:437`).
- **States:** `Idle` → `Asking(sheet)` → `Joining(ssid[, tried])` → `Joined` | `Reasking(tried + error)` | `AskingAgain` (saved password refused) | `Failed` (status strip) → `Idle`; `Reasking`/`AskingAgain` → `Queued` while another sheet is open; representable: `queued.is_some()` with `sheet.is_none()`.
- **Transitions:** `join` (`network/mod.rs:642-681`): known or open → Joining (`:660-670`), password → Asking (`:673-679`), enterprise or not in range → Asking (`:644-649`, `:671`); `JoinOther` (`:1124-1129`); `ChangePassword` (`:827-846`); `submit` Asking→Joining (`:713-736`, `:737-758`) or stays with an error when too short, busy or out of range (`:715-731`, `:741-744`); `run_join` (`:627-631`); `Joined` (`:1130-1166`: `joining` cleared `:1131`, `tried` taken `:1132`, reopen `:1137-1146`, refused saved password `:1147-1163`); `open_sheet_after` (`:574-581`); `close_sheet` drains `queued` (`:592-599`); `SheetCancel` (`:1297`); `Hotspot(true)` opens a sheet without the joining check (`:1201-1205`).
- **Hazards:** nothing times out in Settei: `joining` clears only when `Joined` arrives (a wait thread that fails to start, `desktop/aizome/apps/settei/src/pane.rs:655-660`, leaves "Still joining" for good); `queued` has one slot and is overwritten; it drains only through this pane's `close_sheet`, so after another pane replaced the sheet window (STA-12) the failure sheet never appears; a reopened sheet can appear over another pane.
- **Reify:** High — four fields across ten handlers, with a sheet queue.
- **Related:** NetworkManager through `aizome_sys::network` (`desktop/aizome/crates/aizome-sys/src/network`); ported from `desktop/tatami/network`

<a id="sta-15"></a>

### STA-15 — Connection details page: navigation, profile reads and the edit buffer
- **Where:** `desktop/aizome/apps/settei/src/panes/network/mod.rs:448` (`Network.edit`), `:445` (`.profiles`), `:428` (`.page`), `:430` (`.from_saved`); `Edit` at `:347-360`; `enum Page` at `:225` (partially explicit)
- **Encoding:** `page` (`Main`/`Saved`/`Details(uuid)`), `from_saved` (Back's target, meaningful only on `Details`), `profiles: HashMap<uuid, Result<Profile>>` (absent = "Reading its settings…"), and one `Edit` for the typed IPv4/DNS fields with its own `uuid`, `manual`/`custom` ("chosen, not applied yet") and errors; every `Changed` re-reads the profile.
- **States:** page `Main` ⇄ `Saved` ⇄ `Details(uuid, from_saved)`; profile per uuid `Unread` → `Read` | `Unreadable`, re-read after each change; edit `Pristine(uuid)` → `Typing` → `ManualPending` / `CustomPending` → `Applying` (flags cleared, change in flight) → `Pristine` on reload; representable: `edit.uuid` ≠ the page's uuid, which the view never checks.
- **Transitions:** `open` resets `edit` for a new uuid and reads (`network/mod.rs:683-693`); `back` (`:695-708`); `focus_entry` sets `page` directly (`:1088-1100`); `Forgotten` backs out (`:1242-1244`); `load`/`loaded` (`:517-551`, edit replaced when `:535-536` hold); `Ipv4(1)` → ManualPending (`:848-852`); `Ipv4(0)` (`:854-864`); `AddressText`/`RouterText` (`:866-873`); `ApplyAddress` (`:874-910`, `manual = false` `:898`); `DnsChoice` (`:911-921`); `DnsText` (`:922-925`); `ApplyDns` (`:926-935`); `Changed` failure clears `manual`/`custom` (`:1214-1219`), reapply (`:1220-1226`), re-read (`:1229`); `Reapplied` (`:1231-1238`); `shown` re-reads the page's profile (`:1070-1072`).
- **Hazards:** `loaded` replaces `edit` whenever the profile read is for a uuid other than `edit.uuid` (`network/mod.rs:536`), not only for the page shown, and the view reads `edit` without checking its uuid (`network/view.rs:583-584`, `:602`, `:632`, `:655`); a reload of the same uuid wipes typed, unapplied text unless `manual`/`custom` is set, which it isn't when the profile is already manual or custom (toggling "Join automatically" mid-edit loses the typed address); `Changed` failure clears the flags whatever uuid it was for; reads of one uuid are untagged and can land out of order.
- **Reify:** High — suspected bug with a wrong-configuration consequence; state over four fields.
- **Suspected bug:** change DNS (or metering) on a connection A that has a static address, go Back and open connection B before A's change returns (polkit asking, say): A's `Changed` re-reads A and `loaded` swaps the edit buffer to A's address, router and DNS; B's page then shows A's values in its fields, and Apply on B's IPv4 writes A's static address to B (confidence: medium).
- **Related:** NetworkManager settings through `aizome_sys::network` (`desktop/aizome/crates/aizome-sys/src/network`)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#sta-15)

<a id="sta-19"></a>

### STA-19 — Add-a-printer sheet: discovery, driver lookup, adding
- **Where:** `desktop/aizome/apps/settei/src/panes/printers.rs:145` (`Printers.add`); `Add` at `:52-66` (`devices`, `chosen`, `by_address`, `drivers`, `driver`, `adding`, `error`)
- **Encoding:** `devices: Option<Result<…>>` (looking, found, failed), `chosen: Option<usize>` and `by_address` (the target), `drivers: Option<Result<…>>` and `driver` (for a device that needs one), `adding` (submit in flight), `error`; readiness derived in `ready` (`printers.rs:104-110`).
- **States:** `Closed` → `Discovering` → `Choosing` → `NeedsDriver(loading)` → `DriverChosen` | `ByAddress` → `Adding` → `Closed` (added) | `Choosing(error)`; representable: `by_address && chosen.is_some()` (a manual backend chosen, `printers.rs:456-457`), `drivers` for a device other than `chosen`, `adding` after the sheet was cancelled.
- **Transitions:** `AddPrinter` (`printers.rs:422-426`); `Found` (`:427-442`); `Choose` resets drivers and may request them (`:443-473`); `ByAddress` (`:474-483`); `Address`/`Name`/`Location` (`:484-506`); `Drivers` (`:507-514`); `Driver` (`:515-519`); `Submit` → Adding (`:520-565`, `adding = true` `:548`); `Added` Ok → Closed (`:567-572`) or Err (`:573-578`); `CloseSheet` (`:580-583`); `back` (`:679-682`).
- **Hazards:** `Found` and `Drivers` replies aren't tied to a sheet opening or a device; `Added(Ok)` clears `add` and closes the window's sheet even if the user cancelled and opened a new add sheet meanwhile (`printers.rs:568-569`); Cancel stays enabled while adding (`:958`), so a cancelled add can still succeed.
- **Reify:** High — seven interdependent fields and a suspected bug.
- **Suspected bug:** choose a plugged-in printer that needs a driver, then another before CUPS's driver list comes back: the first device's `Drivers` reply fills `drivers` for the second (`printers.rs:507-513` doesn't check `chosen`), `ready` turns true, and Add installs the second printer with the first one's PPD (`DriverChoice::Model`, `:530-535`) (confidence: medium).
- **Related:** CUPS tools through `aizome_sys::printers` (`desktop/aizome/crates/aizome-sys/src/printers/mod.rs`)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#sta-19)

<a id="sta-22"></a>

### STA-22 — New account: its password applied once AccountsService lists it
- **Where:** `desktop/aizome/apps/settei/src/panes/users.rs:49` (`Users.pending_password`)
- **Encoding:** `Option<(login, password)>` set at `Create` and consumed in `system_changed` once the accounts snapshot has the login; the password waits in memory in clear.
- **States:** `Idle` → `Creating` → `AwaitingAccount` → `Applying` (set the password, or require one at first login) → `Idle`; `Creating` → `Idle` on failure; `AwaitingAccount` with an empty password and `SetAtLogin` → `Idle` without a task.
- **Transitions:** `Create` → Creating (`users.rs:378`); `Created(Ok)` calls `system_changed` (`:383-386`); `Created(Err)` clears (`:387-390`); `system_changed` → Applying (`:470-484`, cleared `:475`, skipped `:477-479`); `Done` (`:465-466`).
- **Hazards:** `system_changed` reaches only the current pane (`desktop/aizome/apps/settei/src/app.rs:562-566`), so an account that appears after you leave Users & Groups waits for you to return and for some service to change, and the password is lost if Settei quits first; a second `Create` before the first account appears overwrites the first pending password; the sheet closes before the create finishes, so a failure loses what was typed.
- **Reify:** High — suspected bug; the account can end up without the password chosen.
- **Suspected bug:** add a user with a password and switch panes or quit at once: `Created(Ok)` arrives when CreateUser answers, before the snapshot lists the account, `system_changed` finds nothing, later wakes go to the other pane, and the new account never gets its password (confidence: low).
- **Related:** AccountsService through `aizome_sys::users` (`desktop/aizome/crates/aizome-sys/src/users.rs:398`, `create_user` resolves on the D-Bus reply; the snapshot learns of the account separately)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#sta-22)
