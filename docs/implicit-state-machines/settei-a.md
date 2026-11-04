# STA: Settei: general, keyboard, network, sound and other panes

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/apps/settei`. Audited at `8e0acbe5`; line numbers refer to that commit.

29 files, 0 pending. 22 implicit machines: 7 High, 3 Medium, 12 Low.

## Machines

<a id="sta-01"></a>

### STA-01 — Pane background loads (Option as the loading spinner, re-read on every show)
- **Where:** `desktop/aizome/apps/settei/src/panes/general/about.rs:33` (`State.hardware`, `.displays` :38, `.disk` :40); also `desktop/aizome/apps/settei/src/panes/general/datetime.rs:46` (`zones`), `desktop/aizome/apps/settei/src/panes/general/defaults.rs:27` (`index`, `rows`), `desktop/aizome/apps/settei/src/panes/general/language.rs:50` (`available`), `desktop/aizome/apps/settei/src/panes/general/login.rs:30` (`items`, `apps`), `desktop/aizome/apps/settei/src/panes/general/startup.rs:29` (`splash`), `desktop/aizome/apps/settei/src/panes/general/storage.rs:34` (`volumes`), `desktop/aizome/apps/settei/src/panes/general/update.rs:27` (`system`), `desktop/aizome/apps/settei/src/panes/keyboard/mod.rs:105` (`registry`), `desktop/aizome/apps/settei/src/panes/notifications.rs:18` (`apps`), `desktop/aizome/apps/settei/src/panes/performance.rs:36` (`stats`), `desktop/aizome/apps/settei/src/panes/search.rs:28` (`index`)
- **Encoding:** one `Option<T>` per pane (`None` = spinner or "…", `Some` = loaded; startup's `Option<Option<Splash>>` adds "not offered", performance's `Option<Result<…>>` adds "compositor not answering"), filled by a `cx.spawn`/`cx.compositor` result message; no in-flight flag, request id or generation.
- **States:** `Unloaded` → `Loading` (still `None`) → `Loaded`; on a later show `Loaded` → `Reloading` (old value shown, a second read in flight) → `Loaded`; `Loading` vs `Reloading` and "two reads in flight" are not distinguishable from the fields.
- **Transitions:** spawn on show: `general/about.rs:234`, `:245`, `:253`; `general/datetime.rs:198-204` (only while `None`); `general/defaults.rs:162`; `general/language.rs:292-298`; `general/login.rs:201`; `general/startup.rs:91`; `general/storage.rs:183`; `general/update.rs:76`; `keyboard/mod.rs:957-963`; `notifications.rs:119-126`; `search.rs:115`; results land: `general/about.rs:285-287`, `general/datetime.rs:240-243`, `general/defaults.rs:171-174`, `general/language.rs:339`, `general/login.rs:210-213`, `general/startup.rs:97`, `general/storage.rs:189`, `general/update.rs:82`, `keyboard/mod.rs:1003`, `notifications.rs:138-148`, `performance.rs:194`, `search.rs:192`; local optimistic edits of the loaded value: defaults `Set` (`general/defaults.rs:175-196`), login `Toggle`/`Add`/`ConfirmRemove` (`general/login.rs:214-269`), search `Rebuild` (`search.rs:174`).
- **Hazards:** panes that guard on `is_none()` (about hardware, datetime zones, language locales, keyboard registry, notification apps) still spawn again on each show until the first read lands; General's `shown` calls `about.shown` and then `page_shown`, which on the About page calls it again (`general/mod.rs:249-250`, `:138`), doubling the hardware probe, the compositor `Outputs` request and the statvfs; results are untagged, so a read spawned before a local edit can land after it and undo it on screen (login `Loaded` after `Toggle`/`Add`; defaults `Loaded` replacing `rows` and the shared `AppIndex` after `Set`); about's `displays`/`disk` replies from two shows can arrive out of order.
- **Reify:** Medium — a shared `Loadable<T>` with a request id would remove the duplicate reads and the overwrite races in a dozen panes.

<a id="sta-02"></a>

### STA-02 — Inline name drafts committed on Return or blur (computer name, host name, your name)
- **Where:** `desktop/aizome/apps/settei/src/panes/general/about.rs:35` (`State.draft`), `:36` (`.host_draft`); also `desktop/aizome/apps/settei/src/panes/users.rs:46` (`Users.name_draft`)
- **Encoding:** `Option<String>` draft per field (`None` = show the service's value), the service snapshot (`HostnameState`, the AccountsService user) and an in-flight `cx.wait` task; the host name adds a confirm alert whose value rides in `Msg::ConfirmHostName`.
- **States:** `Showing` (draft `None`) → `Editing(text)` → `Saving` (draft `None`, task in flight) → `Saved` | `Failed`; host name: `Editing` → `Unchanged` | `Invalid` (draft dropped) | `Confirming` (alert open, draft already dropped) → `Saving` → `Saved` | `Failed`.
- **Transitions:** `Name` Showing→Editing (`general/about.rs:289`); `SaveName` from `on_submit`/`on_blur` (`general/about.rs:385-386`) Editing→Saving (`general/about.rs:291`, `:301`); `HostName` (`general/about.rs:303`); `SaveHostName` (`general/about.rs:305` take, `:307` unchanged, `:310-316` invalid, `:317-324` confirm); `ConfirmHostName` Confirming→Saving (`general/about.rs:326-338`); `Done` (`general/about.rs:340-341`); users `Name` (`users.rs:266`), `SaveName` (`users.rs:267-270`, wired at `users.rs:539-540`).
- **Hazards:** the draft is dropped before the write lands, so the field shows the old value until the service reports the change (and keeps it on failure); a cancelled confirm or an invalid host name throws away what was typed; `SaveName` writes even when the text equals the current name (no comparison, unlike the host name at `general/about.rs:307`); Return then blur both send `SaveName`, harmless only because the first `take()` empties the draft; users `SaveName` drops the draft when there is no current user (`users.rs:268`).
- **Reify:** Low — local and short-lived.

<a id="sta-03"></a>

### STA-03 — Confirm-then-act alerts (the target carried in the confirm message)
- **Where:** `desktop/aizome/apps/settei/src/panes/general/about.rs:317` (host name); also `desktop/aizome/apps/settei/src/panes/general/login.rs:249`, `desktop/aizome/apps/settei/src/panes/keyboard/mod.rs:1192` and `:1222`, `desktop/aizome/apps/settei/src/panes/network/mod.rs:962`, `desktop/aizome/apps/settei/src/panes/printers.rs:381`, `desktop/aizome/apps/settei/src/panes/search.rs:177`, `desktop/aizome/apps/settei/src/panes/users.rs:399`, `:409`, `:429`
- **Encoding:** no pane field: the open alert lives in the kit (`PaneCx::confirm`) and the target is captured in the `on_confirm` message (`ConfirmHostName(name)`, `ConfirmRemove(id)`, `Rebind(action, chord)`, `RestoreConfirmed`, `ForgetNow(uuid)`, `RemoveNow(name)`, `Forgotten`, `TypeConfirmed(user, admin)`, `AutoLoginConfirmed(who)`).
- **States:** `Idle` → `Asking` (alert open) → `Acting` (task in flight, for the async ones) → `Idle`; Cancel returns to `Idle` without any message (no `on_cancel` is used in this slice).
- **Transitions:** ask at the sites above; act on confirm: `general/about.rs:326`, `general/login.rs:259`, `keyboard/mod.rs:1204` and `:1231`, `network/mod.rs:964`, `printers.rs:391`, `search.rs:186`, `users.rs:419` and `:438`.
- **Hazards:** the target is a snapshot from ask time and the list can change while the alert is open (guards like `let … else { return }` cover removed items); cancel tells the pane nothing, so state consumed before asking (about's `host_draft`, keyboard's `recording` taken at `keyboard/mod.rs:1177`) is not put back; `AutoLoginConfirmed(None)` turns off whoever has automatic login at act time (`users.rs:442`), not who had it when asked.
- **Reify:** Low — two messages each; the kit owns the modal.
- **Related:** `desktop/aizome/apps/settei/src/pane.rs:682` (`PaneCx::confirm`, a kit alert over the window)

<a id="sta-04"></a>

### STA-04 — Visible-pane one-second tick timers (Date & Time clock, Performance poll)
- **Where:** `desktop/aizome/apps/settei/src/panes/general/datetime.rs:50` (`State.tick`, with `.now` :49); also `desktop/aizome/apps/settei/src/panes/performance.rs:39` (`Performance.tick`, with `.stats` :36, `.outputs` :37, `.own` :38)
- **Encoding:** `Option<TimerHandle>` set in `shown` and cancelled in `hidden`; Date & Time is hidden through General's `page_hidden` on a page change or pane hide.
- **States:** `Stopped` → `Ticking` → `Stopped`; the sample never ticks.
- **Transitions:** datetime `shown` Stopped→Ticking (`general/datetime.rs:207-209`); `hidden` (`general/datetime.rs:212-216`) from `go` (`general/mod.rs:159`) and General's `hidden` (`general/mod.rs:149-153`, `:253-255`); `Tick` (`general/datetime.rs:244`); performance `shown` (`performance.rs:178-183`), `hidden` (`performance.rs:185-189`), `Tick` → `poll` (`performance.rs:193`, `:102-127`), `Reset` sends a reset then a `Tick` (`performance.rs:203-205`).
- **Hazards:** each Performance tick sends two compositor requests with no in-flight check (`performance.rs:117-126`); if the compositor answers slowly they queue on the spawn pool and land out of order, an older `Stats` replacing a newer one; `Reset`'s extra `Tick` doubles a poll; only the DateTime page has a hidden hook in General, so another ticking page would leak its timer.
- **Reify:** Low — one field each, symmetric start and stop.

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
- **Reproduction:** Not confirmed (integration): [test and output](reproductions.md#sta-05)

<a id="sta-06"></a>

### STA-06 — (test harness) Settle and advance loops in Settei's pane tests
- **Where:** `desktop/aizome/apps/settei/src/panes/general/language.rs:470` (`settle`); also `desktop/aizome/apps/settei/src/panes/network/mod.rs:1366` (`settle`), `desktop/aizome/apps/settei/src/panes/printers.rs:1146` and `:1152` (fixed `advance` loops)
- **Encoding:** a loop counter polling a predicate over the harness's texts, sending `AppMsg::Nothing` to drain spawned results; printers advance the harness clock a fixed amount.
- **States:** `Waiting` (up to 400 × 5 ms) → `Met` | `TimedOut` (returns silently); printers: 20 × 50 ms, then assert.
- **Transitions:** predicate true (`general/language.rs:472-473`, `network/mod.rs:1368-1369`); loop exhausted (`general/language.rs:471`, `network/mod.rs:1367`).
- **Hazards:** a timeout doesn't fail by itself, so a slow spawn pool shows up as a wrong-text assertion; the printers test assumes the sample's updates land within one simulated second.
- **Reify:** Low — test scaffolding.

<a id="sta-07"></a>

### STA-07 — Boot splash pending-restart phase (running system vs next start)
- **Where:** `desktop/aizome/apps/settei/src/panes/general/startup.rs:29` (`State.splash`); `Splash { now, next }` at `:21-24`
- **Encoding:** two files read into `now`/`next` (`general/startup.rs:69-74`): `/etc/yukimi/system.json` (running) and `/nix/var/nix/profiles/system/etc/yukimi/system.json` (next start); `None` = not read, `Some(None)` = not offered.
- **States:** `Unread` → `NotOffered` | `On` | `Off` | `OnFromNextStart` | `OffFromNextStart` (`general/startup.rs:77-84`).
- **Transitions:** `shown` reads (`general/startup.rs:87-93`); `Read` (`:97`); `Propose` hands the change to Yukimi (`:98`), whose build moves the system profile (now ≠ next); a restart brings `/etc` level (now = next).
- **Hazards:** nothing re-reads after `Propose`, so the row keeps the old words until the page is shown again; an unreadable profile copy falls back to `now` (`general/startup.rs:72`), hiding a pending change.
- **Reify:** Low — derived from two files; the transitions belong to Yukimi and the boot.
- **Related:** Yukimi's `--choose` (`desktop/aizome/apps/yukimi/src/app.rs`), which prepares the next system profile

<a id="sta-08"></a>

### STA-08 — Busy spinner animation clock
- **Where:** `desktop/aizome/apps/settei/src/panes/hw.rs:31` (`Spin.started`); also `:26` (`Spinner.moving`)
- **Encoding:** per-widget kit state `Spin { started: Option<Instant> }`, the theme's motion flag copied into `moving` at layout, and a self-rearming tick request.
- **States:** `Still` (reduced motion, three dots) | `Spinning(started)`; frame = elapsed / 100 ms mod 8 (`hw.rs:63-64`).
- **Transitions:** layout sets `moving` and starts the clock (`hw.rs:42-47`); each `Tick` invalidates and re-arms while moving (`hw.rs:83-86`).
- **Hazards:** none observed (the clock never resets, which only shifts the phase).
- **Reify:** Low — self-contained animation.

<a id="sta-09"></a>

### STA-09 — (test harness) Live-snapshot sample capture
- **Where:** `desktop/aizome/apps/settei/src/panes/hw.rs:222` (`live_snapshot`); called from `desktop/aizome/apps/settei/src/panes/network/mod.rs:483`, `desktop/aizome/apps/settei/src/panes/printers.rs:290`, `desktop/aizome/apps/settei/src/panes/sound.rs:307`
- **Encoding:** a local `deadline: Instant` and a poll loop on a readiness predicate, gated by `SETTEI_LIVE_SNAPSHOT`.
- **States:** `Off` (variable unset) → `Starting` (service created) → `WaitingReady` (25 ms polls, 4 s deadline) → `Settling` (300 ms) → `Copied` (service dropped).
- **Transitions:** env check (`hw.rs:227`); start (`:228`); wait loop (`:229-232`); settle (`:234`); copy (`:235`).
- **Hazards:** runs inside `shown` on the UI thread, blocking the window up to 4.3 s; readiness predicates are approximate (sound: a sink exists; printers: `loaded`) and the fixed settle can still miss late objects.
- **Reify:** Low — developer tooling behind an environment variable.

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
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#sta-10)

<a id="sta-11"></a>

### STA-11 — Shortcut recording mode and the recorder's held modifiers
- **Where:** `desktop/aizome/apps/settei/src/panes/keyboard/mod.rs:110` (`Keyboard.recording`); also `desktop/aizome/apps/settei/src/panes/keyboard/record.rs:42` (`Held`), `:37` (`Recorder.held`)
- **Encoding:** `recording: Option<action>` (which row shows the recorder), the recorder widget's kit state `Held(Modifiers)` mirrored into `held` at layout (`keyboard/record.rs:50`), and focus on `"recorder"`.
- **States:** `Idle` → `Recording(action)` (held modifiers shown) → `ConfirmingConflict` (alert open, `recording` already `None`) → `Idle`; `Recording` re-armed after a bare key.
- **Transitions:** `Record` Idle→Recording and focus (`keyboard/mod.rs:1163-1166`); `RecordCancel` (`:1167`; Escape at `keyboard/record.rs:113-116`; Cancel button `keyboard/mod.rs:496`); `RecordClear` (`:1168-1175`; Backspace/Delete at `keyboard/record.rs:117-120`); `Recorded` (`keyboard/mod.rs:1176-1203`: re-arm `:1179`, conflict alert `:1192`, direct `Rebind` `:1201`); `Tab` and `hidden` → Idle (`:1001`, `:968`); held modifiers follow each key (`keyboard/record.rs:100-104`) and clear on focus loss (`:130-133`).
- **Hazards:** losing focus doesn't end recording, so the row stays a recorder until Cancel, a tab switch or a hide; cancelling the conflict alert drops the recording silently; the same widget in the command sheet maps Escape to `CommandCancel` (`keyboard/mod.rs:600`), closing the whole sheet and losing the typed command.
- **Reify:** Low — one Option plus widget-local state.

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
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#sta-12)

<a id="sta-13"></a>

### STA-13 — Wi-Fi join: one at a time, the sheet reopened as it was on failure
- **Where:** `desktop/aizome/apps/settei/src/panes/network/mod.rs:433` (`Network.joining`), `:436` (`.tried`), `:439` (`.queued`), `:431` (`.sheet`); `enum Sheet` at `:235` (partially explicit)
- **Encoding:** `joining: Option<ssid>`, `tried: Option<(ssid, Sheet)>` (what was typed, to reopen), `queued: Option<Sheet>` (one sheet waiting for the open one to close), `sheet`, and the `retry` flag carried in `Msg::Joined`; rows derive spinners and disabled presses from `joining` (`network/view.rs:304`, `:326`, `:437`).
- **States:** `Idle` → `Asking(sheet)` → `Joining(ssid[, tried])` → `Joined` | `Reasking(tried + error)` | `AskingAgain` (saved password refused) | `Failed` (status strip) → `Idle`; `Reasking`/`AskingAgain` → `Queued` while another sheet is open; representable: `queued.is_some()` with `sheet.is_none()`.
- **Transitions:** `join` (`network/mod.rs:642-681`): known or open → Joining (`:660-670`), password → Asking (`:673-679`), enterprise or not in range → Asking (`:644-649`, `:671`); `JoinOther` (`:1124-1129`); `ChangePassword` (`:827-846`); `submit` Asking→Joining (`:713-736`, `:737-758`) or stays with an error when too short, busy or out of range (`:715-731`, `:741-744`); `run_join` (`:627-631`); `Joined` (`:1130-1166`: `joining` cleared `:1131`, `tried` taken `:1132`, reopen `:1137-1146`, refused saved password `:1147-1163`); `open_sheet_after` (`:574-581`); `close_sheet` drains `queued` (`:592-599`); `SheetCancel` (`:1297`); `Hotspot(true)` opens a sheet without the joining check (`:1201-1205`).
- **Hazards:** nothing times out in Settei: `joining` clears only when `Joined` arrives (a wait thread that fails to start, `desktop/aizome/apps/settei/src/pane.rs:655-660`, leaves "Still joining" for good); `queued` has one slot and is overwritten; it drains only through this pane's `close_sheet`, so after another pane replaced the sheet window (STA-12) the failure sheet never appears; a reopened sheet can appear over another pane.
- **Reify:** High — four fields across ten handlers, with a sheet queue.
- **Related:** NetworkManager through `aizome_sys::network` (`desktop/aizome/crates/aizome-sys/src/network`); ported from `desktop/tatami/network`

<a id="sta-14"></a>

### STA-14 — Wi-Fi scan cycle
- **Where:** `desktop/aizome/apps/settei/src/panes/network/mod.rs:442` (`Network.scanning`), `:443` (`.scan_timer`)
- **Encoding:** `scanning: bool` (spinner), a 30 s repeating timer while shown, and an untracked 6 s one-shot `ScanDone` per scan (`network/mod.rs:42-43`).
- **States:** `Idle` → `Scanning` (6 s) → `Idle`; timer `Off` | `Every30s`.
- **Transitions:** `scan` Idle→Scanning (`network/mod.rs:496-504`); `ScanDone` (`:1122`); `Scan` from timer or button (`:1121`); `shown` scans and starts the timer (`:1073-1076`); `hidden` cancels it (`:1079-1083`); `Wifi(true)` forces `scanning = false` and scans (`:1112-1115`).
- **Hazards:** the spinner is time-based and ignores the scan's result (`:502`); turning Wi-Fi on during a scan leaves the earlier `ScanDone` pending, which ends the new spinner early; the one-shot isn't cancelled on hide.
- **Reify:** Low — cosmetic.

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
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#sta-15)

<a id="sta-16"></a>

### STA-16 — Per-item in-flight sets (network connections, printers)
- **Where:** `desktop/aizome/apps/settei/src/panes/network/mod.rs:441` (`Network.busy`); also `desktop/aizome/apps/settei/src/panes/printers.rs:144` (`Printers.busy`)
- **Encoding:** `HashSet<String>` of connection UUIDs or printer names with a request in flight; printers carry the name in the `Done` key after a `\u{1f}` separator (`printers.rs:214`, `:585`).
- **States:** per item `Idle` → `Busy` (spinner, controls disabled) → `Idle`.
- **Transitions:** network insert at `Connect` for non-Wi-Fi (`network/mod.rs:791`), `Disconnect` (`:798`), `Vpn` (`:1172`); remove at `VpnDone` (`:1177`); printers insert in `act` (`printers.rs:211`) and `TestPage` (`:335`), remove at `Done` (`:587`) and `Printed` (`:342`).
- **Hazards:** a set, not a count: two overlapping actions on one printer (Pause, then "Take new jobs", whose toggle isn't disabled while busy, `printers.rs:770`) clear the spinner when the first finishes; an entry stays for good if its wait thread never reports (`desktop/aizome/apps/settei/src/pane.rs:655-660`); entries for removed printers or forgotten connections linger.
- **Reify:** Low — small; a per-item counter or request id would fix the overlap.

<a id="sta-17"></a>

### STA-17 — (test harness) Sample services emulating NetworkManager, CUPS and PipeWire
- **Where:** `desktop/aizome/apps/settei/src/panes/network/sample.rs:205` (`join`), `:238` (`set_wifi`), `:257` (`set_vpn`), `:276` (`forget`); also `desktop/aizome/apps/settei/src/panes/network/mod.rs:91-105` (`Net::connect`), `desktop/aizome/apps/settei/src/panes/printers.rs:217` (`with_printer`) and `:352-356` (pause), `desktop/aizome/apps/settei/src/panes/sound.rs:50` (`Snd::sample`), `desktop/aizome/apps/settei/src/panes/keyboard/mod.rs:1137` (`SampleKey`)
- **Encoding:** a `Cell<State>` per service (`crate::system::Cell`) changed as the real service would, returning a ready `Task`.
- **States:** network: `Connected(Kohaku)` → `Joined(other)` | `Refused` (password "wrong") | `OutOfRange`; Wi-Fi `On` ⇄ `Off` (networks cleared, Wi-Fi connections inactive); VPN `Down` ⇄ `Up` (an `ActiveConnection` added or removed); printers `Idle` ⇄ `Paused`.
- **Transitions:** `join` (`network/sample.rs:206-234`), `set_wifi` (`:239-253`), `set_vpn` (`:258-272`), `forget` (`:277-284`); called from `network/mod.rs:68`, `:76-79`, `:97`, `:134`, `:145-151`, `:163`.
- **Hazards:** `set_wifi` in either direction leaves `active` pointing at the old Wi-Fi connection; `join` never passes through `connecting`; `set_airplane(false)` turns Wi-Fi on even if it was off before airplane mode.
- **Reify:** Low — fixtures.

<a id="sta-18"></a>

### STA-18 — CUPS client lifetime tied to the pane's visibility and the add sheet
- **Where:** `desktop/aizome/apps/settei/src/panes/printers.rs:142` (`Printers.spool`); `enum Spool` at `:27` (partially explicit)
- **Encoding:** `Spool::None | Live(Cups) | Sample(Cell)`, created in `shown` and dropped in `hidden` only while `add` is `None`; a live client polls CUPS every 5 s until dropped.
- **States:** `None` → `Live(loading)` ("Asking CUPS…", `printers.rs:605-608`) → `Live(loaded)` | `Live(unavailable)` → `None` on hide; `Live` kept across a hide while adding; `Sample` permanent.
- **Transitions:** `shown` creates (`printers.rs:286-300`) or refreshes (`:301-304`); `hidden` drops (`:307-313`).
- **Hazards:** kept alive across a hide by `add`, then nothing drops it when `add` clears while another pane shows (`Added`, or a stale `add` from STA-12), so CUPS stays polled until Printers is shown and hidden again; every show starts over at "Asking CUPS…".
- **Reify:** Medium — the drop condition depends on another machine's state.
- **Related:** `desktop/aizome/crates/aizome-sys/src/printers/mod.rs:574` (`Printers::new`; 5 s poll at `:64`, stopped on drop at `:796`)

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
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#sta-19)

<a id="sta-20"></a>

### STA-20 — Palette file index and history reset by removing the shell's files
- **Where:** `desktop/aizome/apps/settei/src/panes/search.rs:28` (`Search.index`)
- **Encoding:** the shell's index cache (`FileIndex::default_cache_path`) and frecency file: present = indexed / remembered, absent = rebuild at the palette's next open / history empty; `index: Option<(size, mtime)>` mirrors the cache.
- **States:** `Unknown` → `Indexed(size, when)` | `NotIndexed`; Rebuild → `NotIndexed` → (shell rebuilds) `Indexed`; history `Remembered` → Forget → `Empty`.
- **Transitions:** `shown` stats the cache (`search.rs:110-119`); `Index` (`:192`); `Rebuild` removes it and sets `(0, None)` (`:170-176`); `Forget` confirm (`:177-185`); `Forgotten` removes the frecency file (`:186-191`).
- **Hazards:** the status is optimistic and not re-read until the next show; a failed removal is ignored (`let _ =`), so "Not indexed yet" or "history is gone" can be untrue; the shell's own staleness rules (a `.key` file and a maximum age) also trigger rebuilds this pane doesn't show.
- **Reify:** Low — a file-presence handshake; the shell re-reads both files itself.
- **Related:** `desktop/aizome/crates/aizome-shell/src/palette/mod.rs:811` (`index_soon`, key and age at `:819-823`); `desktop/aizome/crates/aizome-search/src/frecency.rs:99` (`is_stale`), `:107` (`record` re-reads)

<a id="sta-21"></a>

### STA-21 — Input level meter following the default input
- **Where:** `desktop/aizome/apps/settei/src/panes/sound.rs:86` (`Sound.meter`), `:87` (`.meter_timer`), `:88` (`.level`), `:89` (`.peak`), `:91` (`.ticks`)
- **Encoding:** `meter: Option<(device name, Meter)>` (a pulse connection on its own thread, stopped on drop), an 80 ms repeating timer, the level and decaying peak shown, and the sample's tick count.
- **States:** `Off` (hidden) → `Waiting` (timer on, no default input known) → `Following(name)` → `Following(other)` on a default change → `Off`.
- **Transitions:** `follow_input` starts or stops the meter and the timer (`sound.rs:208-222`); `shown` (`:330`); `Meter` re-follows when the default differs (`:478-485`) and updates level and peak (`:497-498`); `Default(Input)` drops and re-follows (`:354-357`); `hidden` drops both and zeroes the levels (`:333-340`).
- **Hazards:** `Default(Input)` re-follows before PipeWire has switched, so it restarts a meter on the old input that the next tick replaces (two extra connections); a meter whose thread failed stays `Some` at level 0 and isn't retried until the default changes.
- **Reify:** Medium — three handlers around a thread-owning resource.
- **Related:** `desktop/aizome/crates/aizome-sys/src/meter.rs:35` (`Meter::start`; drop stops it at `:62-66`)

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
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#sta-22)

## Already explicit

- `desktop/aizome/apps/settei/src/panes/general/mod.rs:25` `enum Page` — General's sub-pages; transitions centralised in `fn go` (`desktop/aizome/apps/settei/src/panes/general/mod.rs:155`), which runs the hidden and shown hooks
- `desktop/aizome/apps/settei/src/panes/keyboard/mod.rs:88` `enum Tab` — Typing or Shortcuts; set only in `update`'s `Msg::Tab` (`desktop/aizome/apps/settei/src/panes/keyboard/mod.rs:999`) and `focus_entry` (`:973`)
- `desktop/aizome/apps/settei/src/panes/users.rs:19` `enum Sheet` — the Users sheet's content (password, add, remove) with its own data, opened and closed in `update` (`desktop/aizome/apps/settei/src/panes/users.rs:263`); its pairing with the window's sheet is STA-12

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/apps/settei/src/panes/general/about.rs` | 510 | machines | [STA-01](#sta-01), [STA-02](#sta-02), [STA-03](#sta-03) | hardware/displays/disk loads; name drafts; host-name confirm |
| `desktop/aizome/apps/settei/src/panes/general/datetime.rs` | 576 | machines | [STA-04](#sta-04), [STA-01](#sta-01) | 1 s clock tick; zone list load; map widget stateless; units test |
| `desktop/aizome/apps/settei/src/panes/general/defaults.rs` | 230 | machines | [STA-01](#sta-01) | app index load re-read each show; optimistic Set |
| `desktop/aizome/apps/settei/src/panes/general/language.rs` | 499 | machines | [STA-05](#sta-05), [STA-06](#sta-06), [STA-01](#sta-01) | formats preview with locale guard; locale list load; settle test loop |
| `desktop/aizome/apps/settei/src/panes/general/login.rs` | 314 | machines | [STA-01](#sta-01), [STA-03](#sta-03) | login items load with optimistic edits; remove confirm |
| `desktop/aizome/apps/settei/src/panes/general/mod.rs` | 326 | machines | [STA-01](#sta-01), [STA-04](#sta-04), [STA-05](#sta-05) | Page enum explicit (go); double about.shown; hooks drive STA-04/05 |
| `desktop/aizome/apps/settei/src/panes/general/startup.rs` | 152 | machines | [STA-07](#sta-07), [STA-01](#sta-01) | splash now/next pending-restart phase; tests |
| `desktop/aizome/apps/settei/src/panes/general/storage.rs` | 266 | machines | [STA-01](#sta-01) | volume read on each show; statvfs helper |
| `desktop/aizome/apps/settei/src/panes/general/update.rs` | 124 | machines | [STA-01](#sta-01) | system profile read on each show |
| `desktop/aizome/apps/settei/src/panes/hw.rs` | 236 | machines | [STA-08](#sta-08), [STA-09](#sta-09) | spinner clock; live-snapshot wait loop; other widgets stateless |
| `desktop/aizome/apps/settei/src/panes/keyboard/keys.rs` | 459 | none |  | pure chord parsing and rebind tables; tests |
| `desktop/aizome/apps/settei/src/panes/keyboard/mod.rs` | 1320 | machines | [STA-10](#sta-10), [STA-11](#sta-11), [STA-12](#sta-12), [STA-01](#sta-01), [STA-03](#sta-03) | setup key catch; recording; three sheets; registry load; confirms |
| `desktop/aizome/apps/settei/src/panes/keyboard/modifiers.rs` | 75 | none |  | pure XKB option mapping; tests |
| `desktop/aizome/apps/settei/src/panes/keyboard/record.rs` | 141 | machines | [STA-11](#sta-11) | recorder widget held-modifier state |
| `desktop/aizome/apps/settei/src/panes/keyboard/setup.rs` | 196 | machines | [STA-10](#sta-10) | Setup struct and catch_key wait tag; tests |
| `desktop/aizome/apps/settei/src/panes/lock.rs` | 221 | none |  | stateless; settings writes only |
| `desktop/aizome/apps/settei/src/panes/menubar.rs` | 185 | none |  | stateless; settings writes only |
| `desktop/aizome/apps/settei/src/panes/mouse.rs` | 165 | none |  | stateless; settings writes only |
| `desktop/aizome/apps/settei/src/panes/network/logic.rs` | 445 | none |  | pure derivations from the NM snapshot; tests |
| `desktop/aizome/apps/settei/src/panes/network/mod.rs` | 1408 | machines | [STA-13](#sta-13), [STA-14](#sta-14), [STA-15](#sta-15), [STA-16](#sta-16), [STA-03](#sta-03), [STA-06](#sta-06), [STA-09](#sta-09), [STA-12](#sta-12), [STA-17](#sta-17) | join, scan, details edit, busy set; Net lazy init; tests |
| `desktop/aizome/apps/settei/src/panes/network/sample.rs` | 285 | machines | [STA-17](#sta-17) | sample NetworkManager fixture transitions |
| `desktop/aizome/apps/settei/src/panes/network/view.rs` | 849 | machines | [STA-13](#sta-13), [STA-15](#sta-15), [STA-16](#sta-16) | views read joining, busy and the edit buffer |
| `desktop/aizome/apps/settei/src/panes/notifications.rs` | 266 | machines | [STA-01](#sta-01) | sender list load; quiet-hours pair is config |
| `desktop/aizome/apps/settei/src/panes/performance.rs` | 461 | machines | [STA-04](#sta-04), [STA-01](#sta-01) | 1 s stats poll; budget meter widget stateless |
| `desktop/aizome/apps/settei/src/panes/printers.rs` | 1158 | machines | [STA-18](#sta-18), [STA-19](#sta-19), [STA-16](#sta-16), [STA-03](#sta-03), [STA-06](#sta-06), [STA-09](#sta-09), [STA-12](#sta-12), [STA-17](#sta-17) | CUPS spool lifetime; add sheet; busy set; tests |
| `desktop/aizome/apps/settei/src/panes/search.rs` | 281 | machines | [STA-20](#sta-20), [STA-01](#sta-01), [STA-03](#sta-03) | index status; file-removal resets; forget confirm |
| `desktop/aizome/apps/settei/src/panes/sound.rs` | 991 | machines | [STA-21](#sta-21), [STA-09](#sta-09), [STA-17](#sta-17) | level meter; Snd lazy init; pw-play spawned and reaped; tests |
| `desktop/aizome/apps/settei/src/panes/trackpad.rs` | 224 | none |  | touchpad presence read synchronously on show; tests |
| `desktop/aizome/apps/settei/src/panes/users.rs` | 628 | machines | [STA-22](#sta-22), [STA-02](#sta-02), [STA-03](#sta-03), [STA-12](#sta-12) | pending new-account password; name draft; confirms; sheet enum |
