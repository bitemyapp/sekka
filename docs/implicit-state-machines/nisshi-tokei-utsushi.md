# NTU: Nisshi, Tokei and Utsushi

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/apps/{nisshi,tokei,utsushi}`. Audited at `8e0acbe5`; line numbers refer to that commit.

56 files audited. 33 implicit machines: 6 High, 9 Medium, 18 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

<a id="ntu-01"></a>

### NTU-01 — Nisshi system-log unlock (root reader through pkexec)
- **Where:** `desktop/aizome/apps/nisshi/src/app.rs:193` (`Nisshi.unlock`, `.root` :186, `.root_gen` :188, `.access` :190, `.user_access` :192, `.after_unlock` :195); `enum Unlock` `desktop/aizome/apps/nisshi/src/app.rs:43`; also `desktop/aizome/apps/nisshi/data/org.aizome.Nisshi.policy.in:17`
- **Encoding:** partially explicit: the `Unlock` enum (Locked/Asking/Unlocked/Refused) plus `root: Option<Client>`, `root_gen`, `access`/`user_access`, the deferred file `after_unlock`, and polkit's `auth_admin_keep` cached authorization outside the process.
- **States:** `Locked` → `Asking` → `Unlocked` | `Refused(why)` | `Locked` (cancelled); `Unlocked` → `Locked` (Lock, or the root reader went). Representable invalid: `Asking && root.is_none()` (sample mode, `desktop/aizome/apps/nisshi/src/app.rs:1185`); `after_unlock.is_some()` after the user moved to another scope; a root-read `Scope::File` still shown after the root reader died.
- **Transitions:** `start_root` Locked/Refused→Asking (`desktop/aizome/apps/nisshi/src/app.rs:405`); `Msg::Unlock` (`desktop/aizome/apps/nisshi/src/app.rs:1183`), auto-unlock from `OpenFile` (`desktop/aizome/apps/nisshi/src/app.rs:1207-1209`) and `Launched` (`desktop/aizome/apps/nisshi/src/app.rs:1339-1341`); root `Ready` Asking→Unlocked, access=System, cancel every read, replay `after_unlock`, reload (`desktop/aizome/apps/nisshi/src/app.rs:789-799`); root `Gone` Asking→Locked+notice when Cancelled (`desktop/aizome/apps/nisshi/src/app.rs:909`), Asking→Refused (`desktop/aizome/apps/nisshi/src/app.rs:910`), Unlocked→Locked+notice+reload as the user (`desktop/aizome/apps/nisshi/src/app.rs:912-920`); `Msg::Lock` → Locked, root requests dropped, File scope→All, reload (`desktop/aizome/apps/nisshi/src/app.rs:1189-1200`).
- **Hazards:** `after_unlock` is cleared only by `Gone` and `Launched` (`desktop/aizome/apps/nisshi/src/app.rs:907`, `desktop/aizome/apps/nisshi/src/app.rs:1332`), never by a later scope change; `Gone` from an unlocked root does not leave a root-read file as `Lock` does (`desktop/aizome/apps/nisshi/src/app.rs:1196-1198` vs `desktop/aizome/apps/nisshi/src/app.rs:915-920`); a late user `Ready` overwrites `access` with `user_access` even while Unlocked (`desktop/aizome/apps/nisshi/src/app.rs:801-802`); polkit keeps the authorization for minutes, so a second unlock can pass through `Asking` without a prompt.
- **Reify:** High — privilege boundary, six fields, eight mutation sites.
- **Suspected bug:** open an unreadable /var/log file: `read_file` sets `after_unlock` and the password dialog opens; while it is up the user picks All Messages; on authenticating, root `Ready` takes `after_unlock` and switches the window back to the file the user left (`desktop/aizome/apps/nisshi/src/app.rs:524`, `desktop/aizome/apps/nisshi/src/app.rs:795-797`). (confidence: medium)
- **Related:** helper `desktop/aizome/apps/nisshi/src/bin/helper.rs` (NTU-12); polkit agent dialog `desktop/aizome/crates/aizome-shell/src/auth.rs`.
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#ntu-01)

<a id="ntu-03"></a>

### NTU-03 — Nisshi journal read pipeline: newest page, earlier pages, follow, whole-boot search
- **Where:** `desktop/aizome/apps/nisshi/src/app.rs:228` (`Nisshi.loading`, `.journal_search` :210, `.live_on` :214, `.store` :200); `Store.complete` `desktop/aizome/apps/nisshi/src/model.rs:21`; `Want::Load/Older/Follow` `desktop/aizome/apps/nisshi/src/app.rs:171-173`
- **Encoding:** one `loading` bool shared by every kind of read; `store.complete`; `journal_search: Option<String>` drives a self-continuing chain of `Older` pages; `live_on` and `boot` gate following.
- **States:** `Idle` → `Loading` → `Following` (Live, current or all boots) | `Paused`; `Paging` (Older); `SearchingBoot` → `Searched`. Representable invalid: `!loading` with a `Want::Load` pending; `journal_search.is_some()` with no `Older` in flight and `!store.complete`.
- **Transitions:** `load()` cancel, clear, ask (`desktop/aizome/apps/nisshi/src/app.rs:413-431`); `Done(Load)` complete, follow, continue search (`desktop/aizome/apps/nisshi/src/app.rs:864-872`); `load_older` (`desktop/aizome/apps/nisshi/src/app.rs:438-457`); `Done(Older)` (`desktop/aizome/apps/nisshi/src/app.rs:873-880`); `follow()` (`desktop/aizome/apps/nisshi/src/app.rs:461-483`); `Msg::Live` (`desktop/aizome/apps/nisshi/src/app.rs:1117-1128`); `SearchJournal` (`desktop/aizome/apps/nisshi/src/app.rs:1139-1140`); `StopSearchingJournal` (`desktop/aizome/apps/nisshi/src/app.rs:1144-1145`); `set_search` drops the boot search when the text differs (`desktop/aizome/apps/nisshi/src/app.rs:697-699`); empty journal → complete+follow (`desktop/aizome/apps/nisshi/src/app.rs:891-896`).
- **Hazards:** `Done(File)` (`desktop/aizome/apps/nisshi/src/app.rs:882`) and any `Failed`, including `Want::Fields` and `Boots` (`desktop/aizome/apps/nisshi/src/app.rs:890`), clear the shared `loading` mid-load; an `Older` failure leaves `journal_search` set with nothing in flight and the chip reading "Searching the whole boot…" (`desktop/aizome/apps/nisshi/src/ui/view.rs:231`); follow is not retried (NTU-02).
- **Reify:** High — four fields, about fifteen sites, user-visible progress.
- **Suspected bug:** select a row while the 50,000-entry newest page is still streaming; its fields read fails (cursor vacuumed, or "too many reads at once") and `Failed` sets `loading = false` with `Want::Load` still pending: "Reading…" vanishes, Read Earlier enables (`desktop/aizome/apps/nisshi/src/ui/menus.rs:95`), and a pending `--cursor` gives up early (NTU-06) (`desktop/aizome/apps/nisshi/src/app.rs:889-890`). (confidence: medium)
- **Reproduction:** Confirmed (unit): [summary](reproductions.md#ntu-03)

<a id="ntu-17"></a>

### NTU-17 — Tokei alarm schedule: alarms.json against systemd transient timers
- **Where:** `desktop/aizome/apps/tokei/src/alarms.rs:33` (`Alarm.enabled`); file `alarms_path` `desktop/aizome/apps/tokei/src/alarms.rs:148`; `sync` `desktop/aizome/apps/tokei/src/alarms.rs:194`; `Systemd` `desktop/aizome/apps/tokei/src/alarms.rs:211`; also `desktop/aizome/apps/tokei/src/main.rs:36-43`
- **Encoding:** the persisted `enabled` flag plus external user units `aizome-alarm-<id>.timer/.service` and `…-snooze` (transient, `--collect`), reconciled by `sync`; a once-alarm's calendar is a concrete date computed when scheduled (`desktop/aizome/apps/tokei/src/alarms.rs:122-125`).
- **States:** per alarm `Off` → `Armed` → `Ringing` (service runs `--ring`) → `Snoozed` | `Armed` (repeating) | `Off` (once, disabled by the ring); `Lapsed` at logout → `Armed` by `--sync`.
- **Transitions:** `sync` cancels the unwanted (`desktop/aizome/apps/tokei/src/alarms.rs:196-200`) and schedules the enabled (`desktop/aizome/apps/tokei/src/alarms.rs:202-206`); `Systemd::schedule` stops timer and service, then `systemd-run` (`desktop/aizome/apps/tokei/src/alarms.rs:265-269`); `cancel` (`desktop/aizome/apps/tokei/src/alarms.rs:271-274`); `snooze` (`desktop/aizome/apps/tokei/src/alarms.rs:276-291`); login `--sync` (`desktop/aizome/apps/tokei/src/main.rs:36-43`, unit `desktop/aizome/nix/module.nix:305`); Tokei's launch (`desktop/aizome/apps/tokei/src/app.rs:451`).
- **Hazards:** `stop` includes `<unit>.service` (`desktop/aizome/apps/tokei/src/alarms.rs:259`), so rescheduling a ringing alarm kills its ring; `sync`'s documented "unless it's a once-alarm whose time has gone" (`desktop/aizome/apps/tokei/src/alarms.rs:192-193`) isn't implemented: a once-alarm re-arms for its next occurrence until a ring disables it; `scheduled()` hides snooze timers (`desktop/aizome/apps/tokei/src/alarms.rs:311`).
- **Reify:** High — state split between a JSON file, systemd and two other writers (NTU-19, NTU-24).
- **Suspected bug:** a once-alarm rings; the user opens Tokei (the notification names Tokei's desktop entry) → `launched` → `sync_alarms` → `systemctl --user stop aizome-alarm-<id>.timer aizome-alarm-<id>.service` kills `--ring` before it turns the alarm off, then `schedule` arms it for the same time tomorrow; the resident notification's Snooze and Stop now reach nothing (`desktop/aizome/apps/tokei/src/app.rs:451`, `desktop/aizome/apps/tokei/src/alarms.rs:259`, `desktop/aizome/apps/tokei/src/alarms.rs:266-268`). (confidence: high)
- **Related:** systemd user manager; `desktop/aizome/nix/module.nix:305` (`aizome-tokei-alarms`); ring `desktop/aizome/apps/tokei/src/ring.rs` (NTU-24).
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#ntu-17)

<a id="ntu-19"></a>

### NTU-19 — Tokei's in-memory alarm list, its saves and the undo slot
- **Where:** `desktop/aizome/apps/tokei/src/app.rs:180` (`Tokei.alarms`, `.selected_alarm` :184, `.deleted` :185, `.schedule_error` :187)
- **Encoding:** loaded once in `new` (`desktop/aizome/apps/tokei/src/app.rs:220`) and written whole by `save_alarms` (`desktop/aizome/apps/tokei/src/app.rs:324-330`) on every change; `deleted: Option<(index, Alarm)>`.
- **States:** `InSync` → `Stale` (a ring process changed the file) → `Overwritten` (the next save); `Deleted(i, a)` → `Undone`.
- **Transitions:** load (`desktop/aizome/apps/tokei/src/app.rs:220`); toggle (`desktop/aizome/apps/tokei/src/app.rs:529-535`); delete (`desktop/aizome/apps/tokei/src/app.rs:536-548`); undo (`desktop/aizome/apps/tokei/src/app.rs:549-557`); save from the sheet (`desktop/aizome/apps/tokei/src/app.rs:618-645`); `reschedule` (`desktop/aizome/apps/tokei/src/app.rs:333-343`).
- **Hazards:** never re-read, so a ring's `enabled = false` (`desktop/aizome/apps/tokei/src/ring.rs:94-95`) is undone by the next unrelated save; undo reinserts at a stale index without re-sorting; `selected_alarm` is an index.
- **Reify:** High — two processes write one file with no reload, merge or lock.
- **Suspected bug:** Tokei stays open while a once-alarm rings and is stopped (file now `enabled: false`); the window still shows it on with "Next: … tomorrow"; toggling any other alarm saves the stale list with it enabled, and the next launch or login `--sync` arms it again (`desktop/aizome/apps/tokei/src/app.rs:220`, `desktop/aizome/apps/tokei/src/app.rs:529-533`, `desktop/aizome/apps/tokei/src/app.rs:324-330`). (confidence: high)
- **Related:** `desktop/aizome/apps/tokei/src/ring.rs` (NTU-24); NTU-17.
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#ntu-19)

<a id="ntu-26"></a>

### NTU-26 — Utsushi playback item lifecycle
- **Where:** `desktop/aizome/apps/utsushi/src/app.rs:157` (`Utsushi.status`, `.queue` :151, `.current` :152, `.item` :155, `.info` :156, `.error` :158, `.pos` :162, `.cues` :173, `.resumed` :196, `.resumed_timer` :197); `enum Status` `desktop/aizome/apps/utsushi/src/app.rs:131`
- **Encoding:** partially explicit `Status`; the item is named by `current` into `queue` (each replaceable on its own); an `item` counter guards covers only; "first Opened" compares `info.url`.
- **States:** `Empty` → `Loading` → `Paused` ⇄ `Playing` → `Ended` → next, repeat or stay | `Failed`; `Close` → `Empty`.
- **Transitions:** `load` (`desktop/aizome/apps/utsushi/src/app.rs:313-340`); `seek` Ended→Paused (`desktop/aizome/apps/utsushi/src/app.rs:361-363`); Opened (`desktop/aizome/apps/utsushi/src/app.rs:546-566`); Failed (`desktop/aizome/apps/utsushi/src/app.rs:567-572`); State, with Ended→Next/Repeat (`desktop/aizome/apps/utsushi/src/app.rs:573-608`); Seeked Loading→Paused (`desktop/aizome/apps/utsushi/src/app.rs:612-614`); `Msg::Open` (`desktop/aizome/apps/utsushi/src/app.rs:785-795`); Next, Previous, PlayItem (`desktop/aizome/apps/utsushi/src/app.rs:860-869`); `Close` (`desktop/aizome/apps/utsushi/src/app.rs:1104-1123`); `leave()` writes the resume record (`desktop/aizome/apps/utsushi/src/app.rs:343-350`).
- **Hazards:** player events carry no item id, so a `State(Ended)`, `Failed` or `Cues` queued for the previous item lands on the new one (an old Ended skips the new item, `desktop/aizome/apps/utsushi/src/app.rs:588`); `leave()` names the item by `current` into `queue`, which `Msg::Open` replaces first.
- **Reify:** High — ten fields, about fifteen sites, a data-loss bug.
- **Suspected bug:** watching a two-hour film at 45:00, open another file (Open, drop, Open Recent or a second launch): `Msg::Open` sets `queue = paths` (`desktop/aizome/apps/utsushi/src/app.rs:792`), then `load(0)` calls `leave()` (`desktop/aizome/apps/utsushi/src/app.rs:314`) with `current` still 0, recording the new file at 45:00 with the film's length; `resume_at` (`desktop/aizome/apps/utsushi/src/app.rs:327`) then starts the new file at 45:00 under "Resumed at 45:00". If the film had ended, it erases the new file's own resume point instead. (confidence: high)
- **Related:** `desktop/aizome/crates/aizome-media/src/player.rs:305` (`open`; events emitted untagged, its own `generation` at :637); store NTU-32.
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#ntu-26)

<a id="ntu-28"></a>

### NTU-28 — Utsushi full-screen controls auto-hide
- **Where:** `desktop/aizome/apps/utsushi/src/app.rs:183` (`Utsushi.controls`, `.over_controls` :185, `.controls_menus` :188, `.controls_timer` :189, `.controls_shown_at` :190, `.fullscreen` :178, `.hidden` :181)
- **Encoding:** five fields plus playing and has-video; a count of open menus; a 2.5 s timer re-armed at most four times a second.
- **States:** `Windowed` → `FullShown(timer)` → `FullHidden` (cursor hidden, picture scanned out) → `FullShown`; held while the pointer is over the controls or a menu is open; shown while paused.
- **Transitions:** `show_controls` (`desktop/aizome/apps/utsushi/src/app.rs:439-460`); `rest_controls` (`desktop/aizome/apps/utsushi/src/app.rs:468-473`); `controls_menu` counts up (`desktop/aizome/apps/utsushi/src/app.rs:481`); `ControlsMenuClosed` counts down (`desktop/aizome/apps/utsushi/src/app.rs:1089-1094`); `Pointer` (`desktop/aizome/apps/utsushi/src/app.rs:967-973`); `OverControls` (`desktop/aizome/apps/utsushi/src/app.rs:974-979`); `HideControls` (`desktop/aizome/apps/utsushi/src/app.rs:980-990`); paused shows (`desktop/aizome/apps/utsushi/src/app.rs:600-602`); `window_state` (`desktop/aizome/apps/utsushi/src/app.rs:1152-1164`); `pointer_presence` (`desktop/aizome/apps/utsushi/src/app.rs:1174-1177`); inputs `desktop/aizome/apps/utsushi/src/ui.rs:65-67`, `desktop/aizome/apps/utsushi/src/ui.rs:134`.
- **Hazards:** `controls_menus` is counted up before `cx.show_menu`, which does nothing if the window can't be found (`desktop/aizome/crates/aizome-kit/src/cx.rs:453`), and is never reset on leaving full screen or changing item, so a missed close keeps the controls and clock ticking over the picture for good; fourteen tests (`desktop/aizome/apps/utsushi/tests/utsushi.rs:117-284`) each pin one path, a sign of the spread.
- **Reify:** High — seven fields, ten handlers, battery and scan-out consequences.
