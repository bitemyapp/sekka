# SAP: Shashin, Soroban, Fusen and Annai

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/apps/{shashin,soroban,fusen,annai}`. Audited at `8e0acbe5`; line numbers refer to that commit.

48 files, 0 pending. 26 implicit machines: 10 High, 3 Medium, 13 Low.

## Machines

<a id="sap-01"></a>

### SAP-01 — Annai tour page and scene clock
- **Where:** `desktop/aizome/apps/annai/src/app.rs:43` (`Annai.page`, `.t`, `.ticker`, `.motion`)
- **Encoding:** `page` (0..PAGES), `t` ms into the scene, `ticker: Option<TimerHandle>` (33 ms `Msg::Tick`), `motion` copied from `cx.theme().motion()`; loop lengths and key moments from `art::loop_ms`/`art::key_ms` (`desktop/aizome/apps/annai/src/art.rs:18`, `desktop/aizome/apps/annai/src/art.rs:24`); the window open or closed
- **States:** per page × {`Playing` (ticker Some, t cycling) | `Still` (ticker None, t = key_ms)} → `Closed`; representable after launch: `ticker.is_none()` while the theme's motion is on again
- **Transitions:** `launched` picks Playing/Still and starts the ticker (`desktop/aizome/apps/annai/src/app.rs:125-129`); `go()` resets t (`desktop/aizome/apps/annai/src/app.rs:84-87`); Next/Back/Go (`desktop/aizome/apps/annai/src/app.rs:138-150`), Next on the last page closes (`desktop/aizome/apps/annai/src/app.rs:142`); Tick advances t (`desktop/aizome/apps/annai/src/app.rs:160`) or, once reduced motion is on, cancels the ticker → Still (`desktop/aizome/apps/annai/src/app.rs:153-158`); Done/Escape closes (`desktop/aizome/apps/annai/src/app.rs:168`)
- **Hazards:** one-way: nothing re-creates `ticker`, so reduced motion turned off after it was on (or at launch) leaves the scenes still for the session; Still is only noticed on a Tick
- **Reify:** Low — small and local; a `Playing { ticker } | Still` enum would expose the missing re-enable

<a id="sap-02"></a>

### SAP-02 — Single-instance socket claim and hand-off (Annai, Shashin, Soroban; Fusen through Sumi's copy)
- **Where:** `desktop/aizome/apps/annai/src/instance.rs:18` (`enum Claim`; `claim` at `desktop/aizome/apps/annai/src/instance.rs:25`, `serve` at `desktop/aizome/apps/annai/src/instance.rs:43`); also `desktop/aizome/apps/shashin/src/instance.rs:20`, `desktop/aizome/apps/soroban/src/instance.rs:20`; listener held in `desktop/aizome/apps/annai/src/app.rs:55`, `desktop/aizome/apps/shashin/src/app.rs:397`, `desktop/aizome/apps/soroban/src/app.rs:142`, `desktop/aizome/apps/fusen/src/app.rs:216`
- **Encoding:** the socket file `$XDG_RUNTIME_DIR/aizome-app.<name>.sock` (its presence and whether anything listens), the `Claim` result, `instance: Option<UnixListener>` taken in `launched`, a detached "instance" thread
- **States:** `Probing` → `Handed` (process exits) | `StaleRemoved` → `First` (bound, unserved until `launched`) → `Serving` | `Alone`
- **Transitions:** connect + write → Handed (`desktop/aizome/apps/annai/src/instance.rs:27-31`, `desktop/aizome/apps/shashin/src/instance.rs:32-37`, `desktop/aizome/apps/soroban/src/instance.rs:32-36`); remove_file + bind (`desktop/aizome/apps/annai/src/instance.rs:32-38`, `desktop/aizome/apps/shashin/src/instance.rs:40-46`, `desktop/aizome/apps/soroban/src/instance.rs:39-45`); `main` keeps the listener (`desktop/aizome/apps/annai/src/main.rs:24-28`, `desktop/aizome/apps/shashin/src/main.rs:25-29`, `desktop/aizome/apps/soroban/src/main.rs:10-14`, `desktop/aizome/apps/fusen/src/main.rs:43-47`); `launched` starts `serve` (`desktop/aizome/apps/annai/src/app.rs:131-133`, `desktop/aizome/apps/shashin/src/app.rs:1158-1160`, `desktop/aizome/apps/soroban/src/app.rs:527-529`, `desktop/aizome/apps/fusen/src/app.rs:709-711`); serve reads each connection to EOF (500 ms timeout) and stops when the app's channel closes (`desktop/aizome/apps/annai/src/instance.rs:45-53`)
- **Hazards:** probe, remove and bind are not atomic; a hung instance, or one between `claim` and `launched`, accepts the write so the new launch exits with nothing raised; connections are served one at a time; the socket file is never unlinked at exit; four copies with different wire formats (Annai `raise\n`, Shashin NUL-terminated paths without shutdown, Soroban NUL-joined args plus `\n` that the server ignores, Sumi NUL-joined plus `shutdown(Write)`)
- **Reify:** High — one lifecycle duplicated four times with a reachable race; one shared, lock-guarded implementation would fix all
- **Suspected bug:** two launches started together (two pictures opened from Hako at once) both fail to connect; the second's `remove_file` unlinks the first's freshly bound socket (`desktop/aizome/apps/shashin/src/instance.rs:40-42`) and binds its own, so two instances run and the first can never be raised or handed files again (confidence: medium)
- **Related:** Sumi's shared copy used by Fusen, `desktop/aizome/apps/sumi/src/instance.rs:34` (aizome-sumi)
- **Reproduction:** Confirmed (unit): [test and output](reproductions.md#sap-02)

<a id="sap-03"></a>

### SAP-03 — Annai first-run record and "Show at login"
- **Where:** `desktop/aizome/apps/annai/src/state.rs:10` (`State.shown`, `.show_at_login`); held in `desktop/aizome/apps/annai/src/app.rs:48` (`Annai.state`, `.state_path`)
- **Encoding:** `$XDG_STATE_HOME/aizome/annai/state.json` (`shown: Option<u64>`, `show_at_login`), written through `state.json.new` and a rename; the `--first-run` gate in `main`
- **States:** `NeverShown` (shown None) → `Shown` (shown Some), × `ShowAtLogin` on/off; `due()` = NeverShown || ShowAtLogin (`desktop/aizome/apps/annai/src/state.rs:19-21`)
- **Transitions:** the gate exits when not due (`desktop/aizome/apps/annai/src/main.rs:20-22`); `launched` records `shown` and saves (`desktop/aizome/apps/annai/src/app.rs:121-124`); the checkbox toggles and saves (`desktop/aizome/apps/annai/src/app.rs:163-166`); `save` (`desktop/aizome/apps/annai/src/state.rs:44-51`); `load` falls back to the default (`desktop/aizome/apps/annai/src/state.rs:39-41`)
- **Hazards:** recorded when the window opens, before anything is read; save errors are only logged (`desktop/aizome/apps/annai/src/app.rs:75-81`), so an unwritable state folder shows the welcome at every login; corrupt JSON silently means NeverShown; the gate and `Annai::new` read the file separately
- **Reify:** Low — two persisted fields with one writer
- **Related:** the autostart entry `desktop/aizome/apps/annai/data/org.aizome.Annai.Welcome.desktop` (`Exec=aizome-annai --first-run`, 2 s autostart delay; not in this slice's file list)

<a id="sap-04"></a>

### SAP-04 — Fusen note draft, autosave and protected close
- **Where:** `desktop/aizome/apps/fusen/src/app.rs:65` (`Open.dirty`, `.save_error`, `.seq`); unwritten = empty `Note.path` (`desktop/aizome/apps/fusen/src/notes.rs:98`, `Fusen::on_disk` at `desktop/aizome/apps/fusen/src/app.rs:275-277`); `Meta.auto_named` (`desktop/aizome/apps/fusen/src/notes.rs:81`); `Fusen.main` (`desktop/aizome/apps/fusen/src/app.rs:210`)
- **Encoding:** per open note `dirty`, `save_error: Option<String>`, the `seq` debounce counter and a 500 ms `Msg::Save(id, seq)`; an empty path for notes not yet written; `auto_named` persisted in `~/Notes/.fusen.json`; the 3 s `Msg::Rescan` timer; the main window's key; `impl Drop`
- **States:** `Unwritten` → `UnwrittenDraft` (dirty, no path) → `Clean` ⇄ `Dirty(seq)` → `Failed` (dirty + save_error) → `Clean`; window `Open` → `CloseBlocked` (alert) → `Closed`; representable: `save_error.is_some() && !dirty` (cleared together today), a sticky on an Unwritten note
- **Transitions:** `edited` → Dirty, seq + 1, schedules Save (`desktop/aizome/apps/fusen/src/app.rs:524-525`, `desktop/aizome/apps/fusen/src/app.rs:548`); `Msg::Save` drops a stale seq (`desktop/aizome/apps/fusen/src/app.rs:957-962`); `write` → Clean (`desktop/aizome/apps/fusen/src/app.rs:477-480`), renames an auto-named file (`desktop/aizome/apps/fusen/src/app.rs:445-457`), clears an empty unwritten note without a file (`desktop/aizome/apps/fusen/src/app.rs:432-436`); `save_failed` → Failed (`desktop/aizome/apps/fusen/src/app.rs:485-491`); `new_note` with text writes at once (`desktop/aizome/apps/fusen/src/app.rs:411-416`); `select` drops an empty unwritten note left behind (`desktop/aizome/apps/fusen/src/app.rs:373-380`); `rescan` reloads clean opens, keeps dirty drafts, drops gone notes (`desktop/aizome/apps/fusen/src/app.rs:307-333`); `close` blocks on a failed write and alerts with Retry (`desktop/aizome/apps/fusen/src/app.rs:681-697`, `desktop/aizome/apps/fusen/src/app.rs:504-515`); `RetrySave` (`desktop/aizome/apps/fusen/src/app.rs:954-956`); `Quit` (`desktop/aizome/apps/fusen/src/app.rs:1017-1029`); Drop runs `save_all` (`desktop/aizome/apps/fusen/src/app.rs:224-228`); Delete discards (`desktop/aizome/apps/fusen/src/app.rs:777-788`); the notice (`desktop/aizome/apps/fusen/src/view.rs:317-359`)
- **Hazards:** a failed autosave isn't retried until the next edit, Retry, close or quit; another program's write to a dirty note is overwritten by the next save (no mtime check); a rename that succeeds before a failed write moves the note's identity (`desktop/aizome/apps/fusen/src/app.rs:452-456`); `Note.text` keeps only the first 4096 characters while dirty (`desktop/aizome/apps/fusen/src/app.rs:528-536`); MoveTo failures are silent (`desktop/aizome/apps/fusen/src/app.rs:859-865`)
- **Reify:** High — the phase spans five fields and a dozen handlers, with data-loss consequences
- **Related:** the VM test `desktop/aizome/nix/vm-fusen-save-test.nix` (with `vm-fusen-save-test.py`) drives real save failures, blocked close and retry

<a id="sap-05"></a>

### SAP-05 — Fusen stickies: windows, saved places and the login item
- **Where:** `desktop/aizome/apps/fusen/src/app.rs:211` (`Fusen.stickies`), `desktop/aizome/apps/fusen/src/app.rs:212` (`.saved`), `desktop/aizome/apps/fusen/src/app.rs:221` (`.tracking`); `SavedSticky` (`desktop/aizome/apps/fusen/src/app.rs:104`); `desktop/aizome/apps/fusen/src/wm.rs:44`
- **Encoding:** open sticky windows `BTreeMap<WindowKey, Sticky { note, title, float }>`; `$XDG_STATE_HOME/aizome/fusen/stickies.json` (note path key, `place`, `float`); `$XDG_CONFIG_HOME/autostart/org.aizome.Fusen.Stickies.desktop`, present iff stickies are saved; compositor float and position through `aizome_ipc` on worker threads; the 4 s `Msg::Track` timer
- **States:** per sticky `Opening` (settle thread polling up to 3 s) → `Placed` → `Tracked` → `Closed`; app `NoStickies` (no login item) ⇄ `HasStickies`; `Restoring` at login
- **Transitions:** `open_sticky` (`desktop/aizome/apps/fusen/src/app.rs:560-583`), its settle spawn (`desktop/aizome/apps/fusen/src/app.rs:573-576`) and Track start (`desktop/aizome/apps/fusen/src/app.rs:577-580`); the settle poll (`desktop/aizome/apps/fusen/src/wm.rs:46-55`); `Float` (`desktop/aizome/apps/fusen/src/app.rs:822-835`); `Track`/`Placed` refresh places (`desktop/aizome/apps/fusen/src/app.rs:968-995`); a title change retitles stickies (`desktop/aizome/apps/fusen/src/app.rs:537-546`); `close_sticky` (`desktop/aizome/apps/fusen/src/app.rs:670-679`); Delete closes them (`desktop/aizome/apps/fusen/src/app.rs:777-782`); `restore_stickies` (`desktop/aizome/apps/fusen/src/app.rs:619-626`); `save_stickies` rebuilds the list, writes the JSON, adds or removes the login item (`desktop/aizome/apps/fusen/src/app.rs:587-617`)
- **Hazards:** places are keyed by note path and carried over only on an exact key (`desktop/aizome/apps/fusen/src/app.rs:595`, `desktop/aizome/apps/fusen/src/app.rs:981`), so an auto-rename or MoveTo (which never calls `save_stickies`, `desktop/aizome/apps/fusen/src/app.rs:843-866`) drops the place, and a kill after MoveTo leaves a path restore can't find (`desktop/aizome/apps/fusen/src/app.rs:623`); `Placed` matches windows by title, which edits change (`desktop/aizome/apps/fusen/src/app.rs:978`); the Track timer is never cancelled; the login item is only written when missing; write errors are ignored
- **Reify:** High — three files, a timer and threads must agree on one list
- **Suspected bug:** restoring N stickies at login: the first `open_sticky` → `save_stickies` replaces `saved.stickies` with that one sticky (`desktop/aizome/apps/fusen/src/app.rs:598`), so stickies 2..N are re-added with `place: None` (`desktop/aizome/apps/fusen/src/app.rs:595`) and written out; only a later `Placed` (4 s on, Aizome's IPC only) puts their places back. Quitting or logging out within 4 s, or a session without the IPC, loses their positions for good (confidence: high)
- **Related:** compositor IPC `Windows`, `Float`, `MoveToDesk`, `Resize` (`desktop/aizome/apps/fusen/src/wm.rs:35`, `desktop/aizome/apps/fusen/src/wm.rs:58-65`) answered by aizome-comp; SAP-06 for the wipe on a second launch
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#sap-05)

<a id="sap-06"></a>

### SAP-06 — Fusen launch, request hand-off and Drop-time save
- **Where:** `desktop/aizome/apps/fusen/src/app.rs:215` (`Fusen.request`), `desktop/aizome/apps/fusen/src/app.rs:220` (`.notify`), `desktop/aizome/apps/fusen/src/app.rs:224` (`impl Drop`); `desktop/aizome/apps/fusen/src/main.rs:42`
- **Encoding:** `request: Request { new, files, stickies_only }` consumed once; `notify: Option<MsgSender>` set at launch; the instance listener; `Drop` running `save_all`, which always runs `save_stickies`
- **States:** `Constructed` (index and stickies.json loaded, folder scanned) → `Handed` (dropped, never launched) | `Launched` → `RequestTaken` → `Running` (later `Msg::Launch`) → `Quitting` → `Dropped`
- **Transitions:** `Fusen::new` (`desktop/aizome/apps/fusen/src/app.rs:231-261`); `main` builds, then claims (`desktop/aizome/apps/fusen/src/main.rs:41-48`); `launched` sets notify, serves, restores stickies, starts rescans, sends LaunchDone (`desktop/aizome/apps/fusen/src/app.rs:707-715`); `LaunchDone` takes the request and selects the first note (`desktop/aizome/apps/fusen/src/app.rs:1009-1016`); `take_request` shows the main window unless `stickies_only` with stickies (`desktop/aizome/apps/fusen/src/app.rs:628-657`); `Launch` forces `stickies_only: false` (`desktop/aizome/apps/fusen/src/app.rs:1005-1008`); `Quit` (`desktop/aizome/apps/fusen/src/app.rs:1017-1029`); Drop (`desktop/aizome/apps/fusen/src/app.rs:224-228`)
- **Hazards:** Drop assumes a launched app; notes opened before `launched` get no highlight notify (`desktop/aizome/apps/fusen/src/app.rs:345`); Services hand-off files are deleted only under `$XDG_RUNTIME_DIR/aizome-services` (`desktop/aizome/apps/fusen/src/app.rs:645-649`)
- **Reify:** High — a process lifecycle whose never-launched state still runs persistence
- **Suspected bug:** a second launch while Fusen runs (the shell's Services "New Note", `aizome-fusen --new …`, a launcher click) builds a full `Fusen` (`desktop/aizome/apps/fusen/src/main.rs:42`), gets `Claim::Handed` and returns (`desktop/aizome/apps/fusen/src/main.rs:44`); Drop → `save_all` → `save_stickies` with no stickies writes `{"stickies": []}` (`desktop/aizome/apps/fusen/src/app.rs:598-603`) and deletes the login item (`desktop/aizome/apps/fusen/src/app.rs:607-608`). The running Fusen rewrites both only at its next `save_stickies`; if it's killed or the session ends first, no stickies come back at the next login (confidence: high)
- **Related:** Services launch `desktop/aizome/crates/aizome-shell/src/services.rs:99-111` (aizome-shell); claim in `desktop/aizome/apps/sumi/src/instance.rs:34` (aizome-sumi)
- **Reproduction:** Confirmed (end-to-end): [test and output](reproductions.md#sap-06)
- **Follow-up:** bug fixed in [#301](https://github.com/bitemyapp/sekka/pull/301), not reified. `main` claims the instance socket before it builds a `Fusen`, and the kept stickies are a state: `Kept::Unopened` until `launched` reopens them, then `Kept::Reopened`. Only a reopened list is written, so a `Fusen` that never launched can't save over it. The request hand-off and the Drop-time save of notes stay implicit.

<a id="sap-07"></a>

### SAP-07 — Fusen highlighting passes (incomplete, scheduled, notify)
- **Where:** `desktop/aizome/apps/fusen/src/app.rs:67` (`Open.highlight`), `desktop/aizome/apps/fusen/src/app.rs:68` (`.styles`); `desktop/aizome/apps/fusen/src/view.rs:64`
- **Encoding:** Sumi's `Highlight` flags `incomplete`, `scheduled`, `pump` and its `notify` callback (`desktop/aizome/apps/sumi/src/syntax/cache.rs:55-67`); per open note a `styles` generation handed to the editor (`desktop/aizome/apps/fusen/src/view.rs:261`); `Msg::Highlight`
- **States:** `Complete` → `Incomplete` (lines styled from guesses) → `Scheduled` (notify sent) → `Pumped` (flags cleared, styles + 1) → `Complete`
- **Transitions:** `ensure_open` installs notify (`desktop/aizome/apps/fusen/src/app.rs:345-350`); edits narrow the valid lines (`desktop/aizome/apps/fusen/src/app.rs:895-901`); `reload` and `apply` reset (`desktop/aizome/apps/fusen/src/app.rs:94-97`, `desktop/aizome/apps/fusen/src/app.rs:664-666`); `Msg::Highlight` clears the flags of every open note (`desktop/aizome/apps/fusen/src/app.rs:996-1004`)
- **Hazards:** Fusen's highlighter never checks `incomplete`, sets `scheduled` or calls `notify` (`desktop/aizome/apps/fusen/src/view.rs:67-70`; Sumi's does, `desktop/aizome/apps/sumi/src/view.rs:28-33`), so `Msg::Highlight` is never sent; Fusen's handler doesn't set `pump` (Sumi's does, `desktop/aizome/apps/sumi/src/app.rs:1166-1176`); the handler repaints every open note
- **Reify:** High — the protocol lives in another crate and Fusen implements half of it
- **Suspected bug:** once a pass sets `incomplete` (a jump more than ~3000 lines past what's worked out at the guessed rate, fewer on a slow machine, or a 3 ms budget overrun), `state_at` never works states out again (`desktop/aizome/apps/sumi/src/syntax/cache.rs:226`), so every line after the last edit is styled from a default guess (in a long note, lines under an edit inside a fenced code block show as prose) until a reload or formatting command resets it (confidence: medium)
- **Related:** `desktop/aizome/apps/sumi/src/syntax/cache.rs:218-251` (aizome-sumi)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#sap-07)

<a id="sap-08"></a>

### SAP-08 — Modal sheets: Fusen's New Folder, Shashin's Get Info
- **Where:** `desktop/aizome/apps/fusen/src/app.rs:214` (`Fusen.folder_sheet`); also `desktop/aizome/apps/shashin/src/app.rs:347` (`Shashin.info_sheet`)
- **Encoding:** `Option<(WindowKey, String)>` and `Option<WindowKey>` naming the sheet window; Fusen also stops rescans while one is up
- **States:** `Closed` → `Open(key[, name])` → `Closed`
- **Transitions:** Fusen `NewFolder` (`desktop/aizome/apps/fusen/src/app.rs:729-734`), `FolderName` (`desktop/aizome/apps/fusen/src/app.rs:735-739`), `FolderDone` (`desktop/aizome/apps/fusen/src/app.rs:740-752`), `close` (`desktop/aizome/apps/fusen/src/app.rs:682-683`), rescans gated (`desktop/aizome/apps/fusen/src/app.rs:963-967`); Shashin `Info` guarded (`desktop/aizome/apps/shashin/src/app.rs:1426-1431`), `CloseInfo` (`desktop/aizome/apps/shashin/src/app.rs:1432-1437`), `close_requested` (`desktop/aizome/apps/shashin/src/app.rs:1559-1562`)
- **Hazards:** Fusen's `NewFolder` doesn't check for an open sheet; a second one orphans the first, whose window then renders the main view (`desktop/aizome/apps/fusen/src/app.rs:1035-1062`) and whose close no longer clears anything; rescans stay off while a sheet is up; a failed folder creation is silent
- **Reify:** Low — one Option each

<a id="sap-09"></a>

### SAP-09 — Fusen note Trash move
- **Where:** `desktop/aizome/apps/fusen/src/notes.rs:335` (`fn trash`)
- **Encoding:** `Trash/info/<name>.trashinfo` and `Trash/files/<name>` on disk
- **States:** `InPlace` → `InfoWritten` → `Moved` (rename) | `Copied` → `Removed`; a failure stops at `InfoWritten` or `Copied`
- **Transitions:** free-name probe (`desktop/aizome/apps/fusen/src/notes.rs:345-352`); info written (`desktop/aizome/apps/fusen/src/notes.rs:365-368`); rename, else copy and remove (`desktop/aizome/apps/fusen/src/notes.rs:369-372`); the list drops the note only on success (`desktop/aizome/apps/fusen/src/app.rs:762-776`)
- **Hazards:** the name is chosen by `exists()` and then written without `O_EXCL`; no rollback, so a failed move leaves an orphan `.trashinfo`, and copy-then-failed-remove leaves the note in both places while Fusen reports it couldn't be trashed; always the home Trash, copying across filesystems
- **Reify:** Low — one function; Shashin's version (SAP-18) has the `O_EXCL` claim and rollback

<a id="sap-10"></a>

### SAP-10 — Shashin picture loading (serial, decode, folder listing)
- **Where:** `desktop/aizome/apps/shashin/src/app.rs:353` (`Shashin.pic`), `desktop/aizome/apps/shashin/src/app.rs:354` (`.serial`), `desktop/aizome/apps/shashin/src/app.rs:349` (`.folder`, `.index`, `.chosen`), `desktop/aizome/apps/shashin/src/app.rs:384` (`.animation`), `desktop/aizome/apps/shashin/src/app.rs:400` (`.opened`); `Picture.decoded` (`desktop/aizome/apps/shashin/src/app.rs:158`)
- **Encoding:** a serial generation; `pic: Option<Picture { serial, decoded, info.error, tiles, frame }>`; the frame timer; folder list, index and `chosen`; `opened`/`initial` for the first open
- **States:** `Empty` → `Loading(serial)` (placeholder, then header, then preview) → `Shown` | `Failed(error)`, optionally `Animating`; folder `Single` → `Listed`; representable: a placeholder made with `Prepared::failed` and its error cleared (`desktop/aizome/apps/shashin/src/app.rs:587-597`)
- **Transitions:** `open_paths` (`desktop/aizome/apps/shashin/src/app.rs:524-554`); `list` (`desktop/aizome/apps/shashin/src/app.rs:556-562`); `show` bumps serial, resets zoom, pan, busy and want, cancels the animation, reuses read-ahead or spawns `prepare` (`desktop/aizome/apps/shashin/src/app.rs:565-611`); `Header`/`Preview` accepted for the current undecoded serial (`desktop/aizome/apps/shashin/src/app.rs:1222-1236`); `Decoded` installs or goes to read-ahead (`desktop/aizome/apps/shashin/src/app.rs:1237-1244`); `install` marks decoded, recentres, starts frames, asks for a turned view (`desktop/aizome/apps/shashin/src/app.rs:614-645`); `Frame` (`desktop/aizome/apps/shashin/src/app.rs:1479-1488`); `Listed` adopts the folder (`desktop/aizome/apps/shashin/src/app.rs:1193-1221`); `open` ignored after the first (`desktop/aizome/apps/shashin/src/app.rs:1167-1171`); `Raise` opens handed paths (`desktop/aizome/apps/shashin/src/app.rs:1187-1192`)
- **Hazards:** `Listed` carries no generation; `install` and `Header` recentre (`desktop/aizome/apps/shashin/src/app.rs:631-634`, `desktop/aizome/apps/shashin/src/app.rs:1228`), discarding a zoom about the pointer made while loading; a job that panics never answers (the kit's pool has no `catch_unwind`, `desktop/aizome/crates/aizome-kit/src/cx.rs:74-81`), leaving the picture Loading
- **Reify:** High — the phase spans six fields and ten handlers
- **Suspected bug:** open folder A, then folder B (a drop, the Open panel or a second launch) before A is listed: both `Listed` results pass while `current_path()` is None (`desktop/aizome/apps/shashin/src/app.rs:1198`); whichever lands first shows its first picture and the other is dropped by the same-folder check (`desktop/aizome/apps/shashin/src/app.rs:1198-1201`), so asking for B can end showing A (confidence: medium)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#sap-10)

<a id="sap-11"></a>

### SAP-11 — Shashin unsaved turns, saving and quitting
- **Where:** `desktop/aizome/apps/shashin/src/app.rs:356` (`Shashin.turns`); `desktop/aizome/apps/shashin/src/save.rs:22` (`save_turned`)
- **Encoding:** `turns: HashMap<PathBuf, u8>` of unsaved quarter turns, the "— Turned" title, the `errands` counter, the close alert
- **States:** per picture `Upright` → `Turned(t)` → `Saving` (nothing marks it) → `Saved` | `SaveFailed`; app `Open` → `AskingToQuit` → `SavingAll` → `Quit` | `PartialFailure`
- **Transitions:** `Rotate` (`desktop/aizome/apps/shashin/src/app.rs:1329-1342`); `Save` spawns (`desktop/aizome/apps/shashin/src/app.rs:1343-1352`); `Saved` clears the turn and reloads (`desktop/aizome/apps/shashin/src/app.rs:1353-1373`); `Revert` (`desktop/aizome/apps/shashin/src/app.rs:1374-1382`); Trash clears (`desktop/aizome/apps/shashin/src/app.rs:885`); `close_requested` alert (`desktop/aizome/apps/shashin/src/app.rs:1563-1574`); `SaveAllAndQuit` (`desktop/aizome/apps/shashin/src/app.rs:1494-1507`); `SavedAll` (`desktop/aizome/apps/shashin/src/app.rs:1508-1527`); `DiscardAndQuit` (`desktop/aizome/apps/shashin/src/app.rs:1528-1531`); Save enabled whenever turned (`desktop/aizome/apps/shashin/src/ui.rs:674`)
- **Hazards:** `save_turned` works from the file's current orientation or pixels (`desktop/aizome/apps/shashin/src/save.rs:30-31`, `desktop/aizome/apps/shashin/src/save.rs:53`), so it isn't idempotent; a Rotate or Revert during a save is overridden when `Saved` arrives (`desktop/aizome/apps/shashin/src/app.rs:1357`, `desktop/aizome/apps/shashin/src/app.rs:1361`); quitting on the first `SavedAll` while another save writes can leave `.name.shashin-PID` temp files (`desktop/aizome/apps/shashin/src/save.rs:97`)
- **Reify:** High — no in-flight state, with consequences for the user's files
- **Suspected bug:** ❖S twice during a slow rewrite (a large PNG or TIFF), or ❖Q → Save and then ❖Q → Save again while the first `SaveAllAndQuit` runs (turns stay set until `SavedAll`, so the alert returns), runs `save_turned` twice with the same turns: a picture turned 90° is saved at 180° (confidence: high)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#sap-11)

<a id="sap-12"></a>

### SAP-12 — Shashin zoom mode and centre
- **Where:** `desktop/aizome/apps/shashin/src/app.rs:357` (`Shashin.zoom`), `desktop/aizome/apps/shashin/src/app.rs:359` (`.center`); `enum Zoom` (`desktop/aizome/apps/shashin/src/view.rs:11`)
- **Encoding:** `Zoom::Fit | Fixed(z)` plus `center`, which matters only in Fixed; `view`/`scale` learned from the stage
- **States:** `Fit` ⇄ `Fixed(z, center)`; representable and harmless: a stale `center` while Fit
- **Transitions:** `show` → Fit (`desktop/aizome/apps/shashin/src/app.rs:568`); `zoom_to` → Fixed (`desktop/aizome/apps/shashin/src/app.rs:822-831`); ZoomOut snaps to Fit (`desktop/aizome/apps/shashin/src/app.rs:1278-1286`); ActualSize, Fit, ZoomPick (`desktop/aizome/apps/shashin/src/app.rs:1287-1293`); ZoomBy with the pinch snap (`desktop/aizome/apps/shashin/src/app.rs:1294-1303`); DoubleClick toggles (`desktop/aizome/apps/shashin/src/app.rs:1320-1328`); Rotate and Revert → Fit (`desktop/aizome/apps/shashin/src/app.rs:1338`, `desktop/aizome/apps/shashin/src/app.rs:1377`); the slideshow → Fit (`desktop/aizome/apps/shashin/src/app.rs:1009`); `pan` refuses while Fit (`desktop/aizome/apps/shashin/src/app.rs:1580-1591`)
- **Hazards:** transitions in ten handlers; the loading path resets the centre (SAP-10)
- **Reify:** Low — explicit enum; only the centre's validity is implicit

<a id="sap-13"></a>

### SAP-13 — Shashin stage drawing requests (busy, want, reported)
- **Where:** `desktop/aizome/apps/shashin/src/app.rs:363` (`Shashin.busy`), `desktop/aizome/apps/shashin/src/app.rs:364` (`.want`), `desktop/aizome/apps/shashin/src/app.rs:387` (`.reported`); `Picture.tiles` (`desktop/aizome/apps/shashin/src/app.rs:155`)
- **Encoding:** a one-job-at-a-time `busy`, the latest request `want: Option<Seen>`, the stage's last report hash `reported: Rc<Cell<u64>>`, and the tile list (newest six, under 320 MiB)
- **States:** `Idle` → `Wanted` → `Drawing(busy)` → `Idle` (tile added); the stage is `Exact` or `Approximate(last key)`
- **Transitions:** paint reports `Seen` when inexact or the view changed, once per key (`desktop/aizome/apps/shashin/src/ui.rs:356-383`); `Msg::Seen` stores it (`desktop/aizome/apps/shashin/src/app.rs:1260-1267`); `ask_for_view` (`desktop/aizome/apps/shashin/src/app.rs:648-665`); `run_jobs` takes it, drops it if stale or undecoded, spawns and sets busy (`desktop/aizome/apps/shashin/src/app.rs:719-798`); `TileMade` clears busy, adds the tile, runs the next (`desktop/aizome/apps/shashin/src/app.rs:1250-1259`); `show` resets busy and want (`desktop/aizome/apps/shashin/src/app.rs:570-571`); `add_tile` evicts (`desktop/aizome/apps/shashin/src/app.rs:166-174`)
- **Hazards:** `show` clears `busy` while an old job runs, so jobs overlap and the old `TileMade` clears the new job's flag; `idle()` (`desktop/aizome/apps/shashin/src/app.rs:490-498`) can be true with a job outstanding; a panicking job leaves `busy` set until the next picture
- **Reify:** High — three fields across the paint/update boundary, with a stuck-render bug
- **Suspected bug:** zoom with the toolbar's Zoom In or ❖= (`desktop/aizome/apps/shashin/src/ui.rs:277`) while a large photo decodes: the stage reports `Seen { exact: false }`, `run_jobs` takes and drops it because the picture isn't decoded (`desktop/aizome/apps/shashin/src/app.rs:723-727`); after `install` the zoom, centre and view are unchanged, so the report key is the same and the stage never reports again (`desktop/aizome/apps/shashin/src/ui.rs:378`). The picture stays drawn from the fit drawing, nearest-pixel scaled, until the view changes (`install` re-asks only for turned pictures, `desktop/aizome/apps/shashin/src/app.rs:640-642`) (confidence: medium)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#sap-13)

<a id="sap-14"></a>

### SAP-14 — Shashin background errands counter
- **Where:** `desktop/aizome/apps/shashin/src/app.rs:366` (`Shashin.errands`)
- **Encoding:** a count of copy, wallpaper and save jobs in flight, read only by `idle()`
- **States:** `0` ⇄ `n > 0`
- **Transitions:** + 1 in `set_wallpaper` (`desktop/aizome/apps/shashin/src/app.rs:932`), `copy` (`desktop/aizome/apps/shashin/src/app.rs:971`, `desktop/aizome/apps/shashin/src/app.rs:987`), `Save` (`desktop/aizome/apps/shashin/src/app.rs:1350`), `SaveAllAndQuit` (`desktop/aizome/apps/shashin/src/app.rs:1496`); − 1 (saturating) in `Saved` (`desktop/aizome/apps/shashin/src/app.rs:1354`), `Copied` (`desktop/aizome/apps/shashin/src/app.rs:1385`), `WallpaperSet` (`desktop/aizome/apps/shashin/src/app.rs:1396`), `SavedAll` (`desktop/aizome/apps/shashin/src/app.rs:1509`)
- **Hazards:** saturation hides mismatches; a panicked job never decrements; wallpaper jobs load, modify and save the settings file without a lock (`desktop/aizome/apps/shashin/src/app.rs:939-947`), so two in quick succession (Night for one picture, Day for another) can lose one
- **Reify:** Low — a counter serving tests' `idle()`

<a id="sap-15"></a>

### SAP-15 — Shashin read-ahead cache
- **Where:** `desktop/aizome/apps/shashin/src/app.rs:368` (`Shashin.ahead`), `desktop/aizome/apps/shashin/src/app.rs:369` (`.reading_ahead`)
- **Encoding:** prepared neighbours (under 640 MiB), a one-at-a-time flag, freshness by file mtime (`fresh`, `desktop/aizome/apps/shashin/src/app.rs:1024-1026`)
- **States:** `Idle` → `Reading(path)` → `Cached` → `Used` | `Evicted` | `Stale`
- **Transitions:** `read_ahead` gated on not reading, loading or busy (`desktop/aizome/apps/shashin/src/app.rs:669-689`); `Prefetched` (`desktop/aizome/apps/shashin/src/app.rs:1245-1249`); a stale `Decoded` is kept (`desktop/aizome/apps/shashin/src/app.rs:1242`); `keep_ahead` keeps neighbours only (`desktop/aizome/apps/shashin/src/app.rs:691-714`); `show` uses a fresh one (`desktop/aizome/apps/shashin/src/app.rs:582-585`); Trash and Saved drop it (`desktop/aizome/apps/shashin/src/app.rs:887`, `desktop/aizome/apps/shashin/src/app.rs:1359`)
- **Hazards:** a panicked prefetch leaves `reading_ahead` true for the session; freshness is mtime only; drawn for the view size of the moment
- **Reify:** Low — guarded and local

<a id="sap-16"></a>

### SAP-16 — Shashin filmstrip thumbnails and their cache
- **Where:** `desktop/aizome/apps/shashin/src/app.rs:371` (`Shashin.thumbs`), `desktop/aizome/apps/shashin/src/app.rs:372` (`.thumb_queue`), `desktop/aizome/apps/shashin/src/app.rs:373` (`.thumbs_running`), `desktop/aizome/apps/shashin/src/app.rs:370` (`.filmstrip`); `enum Thumb` (`desktop/aizome/apps/shashin/src/app.rs:333`); `desktop/aizome/apps/shashin/src/thumbs.rs:48`
- **Encoding:** `Thumb::Pending | Ready | Failed` per path, a nearest-first queue, a running count (at most two), the filmstrip toggle; the freedesktop cache PNG stamped `Thumb::MTime` (whole seconds), written to a temp file and renamed
- **States:** `Absent` → `Queued` → `Pending` → `Ready` | `Failed`; cache entry `Valid` | `Stale`
- **Transitions:** `queue_thumbs` rebuilds the queue (`desktop/aizome/apps/shashin/src/app.rs:841-860`); `run_thumbs`, held while a picture loads (`desktop/aizome/apps/shashin/src/app.rs:862-874`); `Msg::Thumb` (`desktop/aizome/apps/shashin/src/app.rs:1444-1450`); `Filmstrip` (`desktop/aizome/apps/shashin/src/app.rs:1438-1443`); Trash and Saved invalidate (`desktop/aizome/apps/shashin/src/app.rs:886`, `desktop/aizome/apps/shashin/src/app.rs:1358`); `Decoded` resumes (`desktop/aizome/apps/shashin/src/app.rs:1240`); cache check (`desktop/aizome/apps/shashin/src/thumbs.rs:55-63`) and store (`desktop/aizome/apps/shashin/src/thumbs.rs:79-106`)
- **Hazards:** partially explicit (an enum per path, the counter and queue beside it); two panicked jobs pin `thumbs_running` at 2 and stop the filmstrip; with mtime in seconds a rewrite in the same second as the stamp reads as fresh; `Thumb::Size` is written but not checked
- **Reify:** Medium — the enum exists; the queue and counter are implicit

<a id="sap-17"></a>

### SAP-17 — Shashin full screen and slideshow
- **Where:** `desktop/aizome/apps/shashin/src/app.rs:375` (`Shashin.fullscreen`), `desktop/aizome/apps/shashin/src/app.rs:376` (`.slideshow`), `desktop/aizome/apps/shashin/src/app.rs:378` (`.slideshow_full`)
- **Encoding:** a `fullscreen` flag set when asked and corrected by `window_state`, the slideshow's repeating timer, whether the slideshow itself entered full screen
- **States:** `Windowed` → `FullScreen` → `Slideshow(entered | already full screen)` → back
- **Transitions:** `start_slideshow` (`desktop/aizome/apps/shashin/src/app.rs:999-1010`); `stop_slideshow` (`desktop/aizome/apps/shashin/src/app.rs:1012-1020`); `FullScreen` (`desktop/aizome/apps/shashin/src/app.rs:1451-1459`); `Escape` (`desktop/aizome/apps/shashin/src/app.rs:1471-1478`); `Slide` steps (`desktop/aizome/apps/shashin/src/app.rs:1467-1469`); `window_state` takes the compositor's state and ends a slideshow on leaving (`desktop/aizome/apps/shashin/src/app.rs:1535-1545`)
- **Hazards:** `fullscreen` is set before the compositor confirms, so a configure without full screen arriving after the request ends the slideshow at once; the slideshow keeps ticking with no pictures left (`step` returns at `desktop/aizome/apps/shashin/src/app.rs:804`); leaving full screen while a slideshow entered it asks to un-fullscreen twice
- **Reify:** Medium — three fields, five handlers and a compositor callback
- **Related:** the kit's `set_fullscreen` and `WindowState` from the compositor's configure (aizome-kit)

<a id="sap-18"></a>

### SAP-18 — Shashin Move to Trash with Undo and Redo
- **Where:** `desktop/aizome/apps/shashin/src/app.rs:379` (`Shashin.undo`), `desktop/aizome/apps/shashin/src/app.rs:380` (`.redo`); `desktop/aizome/apps/shashin/src/trash.rs:69` (`Trash::trash`), `desktop/aizome/apps/shashin/src/trash.rs:113` (`restore`)
- **Encoding:** an undo stack of `Trashed { original, file, info }`, a redo stack of paths, the `.trashinfo` claimed with `O_EXCL` before the rename, in the home or the filesystem's can
- **States:** per picture `InFolder` → `InfoClaimed` → `Trashed` (undoable) → `Restored` (redoable), rolled back on failure
- **Transitions:** `trash_current` (`desktop/aizome/apps/shashin/src/app.rs:878-898`); `undo_trash` (`desktop/aizome/apps/shashin/src/app.rs:900-916`); `Redo` (`desktop/aizome/apps/shashin/src/app.rs:1416-1425`); claim and rollback (`desktop/aizome/apps/shashin/src/trash.rs:92-105`); restore refuses a taken name, renames back, removes the info (`desktop/aizome/apps/shashin/src/trash.rs:113-123`)
- **Hazards:** both stacks outlive opening another folder or set of files; a failed restore pops the undo step for good; a failed re-trash in Redo loses that redo entry; the item may have been emptied or restored elsewhere meanwhile
- **Reify:** High — a persisted two-phase move plus history that outlives its context
- **Suspected bug:** move a picture of folder A to the Trash, open folder B, press ❖Z: the file returns to A, but its path is inserted into B's list (`desktop/aizome/apps/shashin/src/app.rs:905-907`) and shown there with B's pictures as its neighbours (confidence: high)
- **Reproduction:** Confirmed (integration): [test and output](reproductions.md#sap-18)

<a id="sap-19"></a>

### SAP-19 — Seq-matched transient timers (Shashin's toolbar note, Soroban's key flash)
- **Where:** `desktop/aizome/apps/shashin/src/app.rs:381` (`Shashin.note`, `.note_seq`); also `desktop/aizome/apps/soroban/src/app.rs:136` (`Soroban.flash`, `.flash_seq`)
- **Encoding:** `Option<(payload, seq)>` and a counter; a one-shot `cx.after` carrying the seq
- **States:** `None` → `Showing(seq)` → `None` (only when the firing timer's seq matches)
- **Transitions:** Shashin `say` (`desktop/aizome/apps/shashin/src/app.rs:504-508`), `Said` (`desktop/aizome/apps/shashin/src/app.rs:1489-1493`); Soroban `flash` (`desktop/aizome/apps/soroban/src/app.rs:270-274`), `Unflash` (`desktop/aizome/apps/soroban/src/app.rs:550-554`), shown pressed (`desktop/aizome/apps/soroban/src/app.rs:326`, `desktop/aizome/apps/soroban/src/keypad.rs:262`)
- **Hazards:** none observed (superseded timers fire harmlessly)
- **Reify:** Low — fine as it is

<a id="sap-20"></a>

### SAP-20 — Shashin pan drag
- **Where:** `desktop/aizome/apps/shashin/src/app.rs:383` (`Shashin.panning`)
- **Encoding:** `Option<Point>`, the last pointer position; the stage forwards moves only while its snapshot says panning (`desktop/aizome/apps/shashin/src/ui.rs:140`, `desktop/aizome/apps/shashin/src/ui.rs:176`)
- **States:** `Idle` → `Panning(last)` → `Idle`
- **Transitions:** single left press → `PanStart` (`desktop/aizome/apps/shashin/src/ui.rs:257-261`, `desktop/aizome/apps/shashin/src/app.rs:1304-1307`); `PanTo` (`desktop/aizome/apps/shashin/src/ui.rs:262`, `desktop/aizome/apps/shashin/src/app.rs:1308-1314`); `PanEnd` (`desktop/aizome/apps/shashin/src/ui.rs:263`, `desktop/aizome/apps/shashin/src/app.rs:1315`); `show` clears (`desktop/aizome/apps/shashin/src/app.rs:569`)
- **Hazards:** a lost release (focus taken mid-drag) leaves `panning` set and button-less moves pan; every single click starts a pan
- **Reify:** Low — a small gesture

<a id="sap-21"></a>

### SAP-21 — (test harness) settle and poll loops
- **Where:** `desktop/aizome/apps/shashin/tests/common/mod.rs:265` (`settle`); also `desktop/aizome/apps/shashin/tests/shashin.rs:432`, `desktop/aizome/apps/fusen/tests/fusen.rs:236`
- **Encoding:** `quiet` (three idle rounds needed) within 2000 rounds of 4 ms; deadline and iteration polls that pump `Msg::Nothing`
- **States:** `Busy` → `Quiet(n)` → `Settled` | panic ("didn't settle", deadline)
- **Transitions:** render, pump, check `idle()` (`desktop/aizome/apps/shashin/tests/common/mod.rs:268-279`); the hand-off poll (`desktop/aizome/apps/shashin/tests/shashin.rs:432-438`); Fusen's second-launch poll until the note file exists (`desktop/aizome/apps/fusen/tests/fusen.rs:236-241`)
- **Hazards:** `idle()` misses overlapping tile jobs and panicked jobs (SAP-13, SAP-14), turning those into an 8 s stall and a panic
- **Reify:** Low — test-only

<a id="sap-22"></a>

### SAP-22 — Soroban state persistence debounce
- **Where:** `desktop/aizome/apps/soroban/src/app.rs:140` (`Soroban.persist_seq`), `desktop/aizome/apps/soroban/src/app.rs:139` (`.state_path`)
- **Encoding:** a counter and a 700 ms `Msg::Persist(seq)`; `$XDG_STATE_HOME/aizome/soroban/state.json` written through `state.json.new` and a rename
- **States:** `Clean` → `Pending(seq)` → `Written`
- **Transitions:** `persist_soon` (`desktop/aizome/apps/soroban/src/app.rs:253-259`) from Key, Typed, SetMode, Radix, Word, Signed, ToggleTape, ClearTape (`desktop/aizome/apps/soroban/src/app.rs:537`, `desktop/aizome/apps/soroban/src/app.rs:547`, `desktop/aizome/apps/soroban/src/app.rs:559`, `desktop/aizome/apps/soroban/src/app.rs:564`, `desktop/aizome/apps/soroban/src/app.rs:569`, `desktop/aizome/apps/soroban/src/app.rs:574`, `desktop/aizome/apps/soroban/src/app.rs:580`, `desktop/aizome/apps/soroban/src/app.rs:605`); `Persist` writes only the latest (`desktop/aizome/apps/soroban/src/app.rs:652-656`); `close_requested` writes now (`desktop/aizome/apps/soroban/src/app.rs:744-747`); `persist_now` (`desktop/aizome/apps/soroban/src/app.rs:239-250`)
- **Hazards:** persisted `ans`/`int_ans` changed by ToggleBit, Undo/Redo, Recall, Paste, Cut or ± on an answer schedule no save (`desktop/aizome/apps/soroban/src/app.rs:576-598`, `desktop/aizome/apps/soroban/src/entry.rs:1027-1036`); an exit without `close_requested` loses up to 700 ms; write errors are silent
- **Reify:** Low — a small debounce

<a id="sap-23"></a>

### SAP-23 — Soroban Save Tape sheet and replace confirmation
- **Where:** `desktop/aizome/apps/soroban/src/app.rs:146` (`Soroban.save`); `SaveSheet.replacing` (`desktop/aizome/apps/soroban/src/app.rs:159`)
- **Encoding:** `Option<SaveSheet { key, name, folder, folders, replacing }>`
- **States:** `Closed` → `Naming` → `ConfirmReplace` (replacing) → `Writing` → `Closed` | alert
- **Transitions:** `SaveTape` (`desktop/aizome/apps/soroban/src/app.rs:607-614`); `SaveName`/`SaveFolder` disarm (`desktop/aizome/apps/soroban/src/app.rs:615-626`); `SaveConfirm` arms or commits (`desktop/aizome/apps/soroban/src/app.rs:627-638`); `SaveCancel` (`desktop/aizome/apps/soroban/src/app.rs:639-643`); `SaveTapeTo` writes (`desktop/aizome/apps/soroban/src/app.rs:644-651`)
- **Hazards:** existence is checked at confirm and the file written later with a plain `fs::write`, so a file appearing in between is replaced unasked; `close_requested` (`desktop/aizome/apps/soroban/src/app.rs:744-747`) doesn't clear `save`, so a sheet closed that way would leave Save Tape refusing (`desktop/aizome/apps/soroban/src/app.rs:608`) until restart
- **Reify:** Medium — a two-step confirm across five handlers

<a id="sap-24"></a>

### SAP-24 — Soroban bit grid hover and keyboard cursor
- **Where:** `desktop/aizome/apps/soroban/src/bits.rs:168` (`GridState.hover`, `.cursor`)
- **Encoding:** widget state the kit keeps across rebuilds
- **States:** `hover: None | Some(bit)`; `cursor: bit`
- **Transitions:** PointerMove/Leave (`desktop/aizome/apps/soroban/src/bits.rs:281-294`); PointerDown sets the cursor and flips (`desktop/aizome/apps/soroban/src/bits.rs:295-301`); arrows, Home, End move, Enter/Space flip (`desktop/aizome/apps/soroban/src/bits.rs:303-329`)
- **Hazards:** the cursor is clamped only on key events (`desktop/aizome/apps/soroban/src/bits.rs:306`); after a smaller word size it's drawn on an inactive bit (`desktop/aizome/apps/soroban/src/bits.rs:257`) until a key moves it
- **Reify:** Low — widget-local

<a id="sap-25"></a>

### SAP-25 — Soroban calculator entry (entry, answer, error, 2nd, undo)
- **Where:** `desktop/aizome/apps/soroban/src/entry.rs:360` (`Calc.pieces`, `.fresh`, `.from`, `.error`, `.second`, `.undo`, `.redo`)
- **Encoding:** the piece list, `fresh` (an answer shows), `error: Option<String>`, `from`, a latched `second`, snapshot stacks (at most 200), beside the persisted `ans`/`int_ans`/`memory`/`tape` (SAP-22)
- **States:** `Empty` → `Entering(pieces)` → `Error(pieces, message)`; `Entering` → `Answer` (fresh) → `Entering` (a digit starts over, an operator or function goes on from the answer); `second` on/off; representable but guarded: `fresh && !pieces.is_empty()`, `fresh && error.is_some()`
- **Transitions:** `press` (`desktop/aizome/apps/soroban/src/entry.rs:702-904`) with `start_over` (`desktop/aizome/apps/soroban/src/entry.rs:640-644`) or `continue_from_answer` (`desktop/aizome/apps/soroban/src/entry.rs:668-673`) per key; `equals` → Answer or Error (`desktop/aizome/apps/soroban/src/entry.rs:964-994`); `back` (`desktop/aizome/apps/soroban/src/entry.rs:997-1022`); `negate` (`desktop/aizome/apps/soroban/src/entry.rs:1026-1061`); `type_char` (`desktop/aizome/apps/soroban/src/entry.rs:1064-1127`); `paste` folds its undo steps (`desktop/aizome/apps/soroban/src/entry.rs:1172-1211`); `recall` (`desktop/aizome/apps/soroban/src/entry.rs:1214-1224`); a mode crossing clears history and sets fresh (`desktop/aizome/apps/soroban/src/entry.rs:1245-1271`); `toggle_bit` → Answer (`desktop/aizome/apps/soroban/src/entry.rs:1322-1333`); `undo`/`redo` (`desktop/aizome/apps/soroban/src/entry.rs:623-637`)
- **Hazards:** the `if self.fresh { … }` branch is repeated in about fifteen places; `remember()` runs before each key's validity check (`desktop/aizome/apps/soroban/src/entry.rs:738`) and always clears redo (`desktop/aizome/apps/soroban/src/entry.rs:612`)
- **Reify:** High — seven fields and twenty-odd transitions
- **Suspected bug:** after ❖Z, typing a key that does nothing — `)` with no parenthesis open (`desktop/aizome/apps/soroban/src/entry.rs:833-835`), a second `.` (`desktop/aizome/apps/soroban/src/entry.rs:764-766`), or a digit outside the radix (`desktop/aizome/apps/soroban/src/entry.rs:742-747`) — still runs `remember()`, which pushes a no-op undo step and empties the redo stack, so ❖⇧Z stops working though nothing changed (confidence: high)
- **Reproduction:** Confirmed (unit): [test and output](reproductions.md#sap-25)

<a id="sap-26"></a>

### SAP-26 — Soroban paper tape follow-the-end scrolling
- **Where:** `desktop/aizome/apps/soroban/src/tape.rs:45` (`TapeState.from_end`, `.hover`, `.cursor`, `.seen`)
- **Encoding:** widget state: scroll offset from the end (0 follows new lines), hover, keyboard cursor, the entry count last laid out
- **States:** `Following` (from_end 0) ⇄ `ScrolledBack`; cursor `None | Some(i)`
- **Transitions:** layout snaps to the end on more entries and clamps the cursor (`desktop/aizome/apps/soroban/src/tape.rs:97-107`); Scroll (`desktop/aizome/apps/soroban/src/tape.rs:213-222`); keys move the cursor into view, Enter recalls (`desktop/aizome/apps/soroban/src/tape.rs:223-255`); Focus picks the last line (`desktop/aizome/apps/soroban/src/tape.rs:256-264`); hover (`desktop/aizome/apps/soroban/src/tape.rs:190-204`)
- **Hazards:** `seen` compares counts only; at `TAPE_MAX` (`desktop/aizome/apps/soroban/src/entry.rs:976-978`) a new line keeps the count, so a scrolled-back tape doesn't follow and the cursor and hover silently point one entry newer
- **Reify:** Low — widget-local

## Already explicit

- `desktop/aizome/apps/soroban/src/entry.rs:18` `enum Mode` — the Basic, Scientific or Programmer keypad; transitions centralised in `fn set_mode` (`desktop/aizome/apps/soroban/src/entry.rs:1245`), with the window resize and save in `Msg::SetMode` (`desktop/aizome/apps/soroban/src/app.rs:555-561`)

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/apps/annai/src/app.rs` | 235 | machines | [SAP-01](#sap-01), [SAP-02](#sap-02), [SAP-03](#sap-03) | tour page and scene clock; serves instance; records first run |
| `desktop/aizome/apps/annai/src/art.rs` | 934 | none |  | pure scene drawing as a function of t; loop_ms/key_ms feed SAP-01 |
| `desktop/aizome/apps/annai/src/instance.rs` | 55 | machines | [SAP-02](#sap-02) | single-instance claim and serve thread |
| `desktop/aizome/apps/annai/src/lib.rs` | 13 | none |  | module list |
| `desktop/aizome/apps/annai/src/main.rs` | 30 | machines | [SAP-02](#sap-02), [SAP-03](#sap-03) | first-run gate then instance claim |
| `desktop/aizome/apps/annai/src/pages.rs` | 175 | none |  | view builders (words, dots, footer) |
| `desktop/aizome/apps/annai/src/state.rs` | 75 | machines | [SAP-03](#sap-03) | persisted first-run state file |
| `desktop/aizome/apps/annai/tests/annai.rs` | 147 | none |  | tests |
| `desktop/aizome/apps/annai/tests/budget.rs` | 80 | none |  | tests (frame timing) |
| `desktop/aizome/apps/fusen/src/app.rs` | 1155 | machines | [SAP-04](#sap-04), [SAP-05](#sap-05), [SAP-06](#sap-06), [SAP-07](#sap-07), [SAP-08](#sap-08), [SAP-02](#sap-02) | autosave, stickies, launch/Drop, highlight, folder sheet |
| `desktop/aizome/apps/fusen/src/lib.rs` | 12 | none |  | module list |
| `desktop/aizome/apps/fusen/src/main.rs` | 49 | machines | [SAP-06](#sap-06), [SAP-02](#sap-02) | builds Fusen before claim; Drop on Handed |
| `desktop/aizome/apps/fusen/src/notes.rs` | 417 | machines | [SAP-09](#sap-09), [SAP-04](#sap-04) | Trash move; index and auto_named used by autosave |
| `desktop/aizome/apps/fusen/src/view.rs` | 388 | machines | [SAP-04](#sap-04), [SAP-07](#sap-07) | save notice; highlighter never notifies |
| `desktop/aizome/apps/fusen/src/wm.rs` | 88 | machines | [SAP-05](#sap-05) | compositor settle poll, float, places |
| `desktop/aizome/apps/fusen/tests/fusen.rs` | 347 | machines | [SAP-21](#sap-21) | tests; second-launch poll loop |
| `desktop/aizome/apps/fusen/tests/save_errors.rs` | 143 | none |  | tests (fault-injected save failures) |
| `desktop/aizome/apps/shashin/examples/drawbench.rs` | 84 | none |  | benchmark |
| `desktop/aizome/apps/shashin/examples/samples.rs` | 14 | none |  | fixture writer |
| `desktop/aizome/apps/shashin/src/app.rs` | 1592 | machines | [SAP-08](#sap-08), [SAP-10](#sap-10), [SAP-11](#sap-11), [SAP-12](#sap-12), [SAP-13](#sap-13), [SAP-14](#sap-14), [SAP-15](#sap-15), [SAP-16](#sap-16), [SAP-17](#sap-17), [SAP-18](#sap-18), [SAP-19](#sap-19), [SAP-20](#sap-20), [SAP-02](#sap-02) | loading, turns, zoom, tiles, errands, read-ahead, thumbs, slideshow, trash, toast, pan |
| `desktop/aizome/apps/shashin/src/clipboard.rs` | 22 | none |  | one-shot clipboard offer |
| `desktop/aizome/apps/shashin/src/folder.rs` | 117 | none |  | pure listing and natural sort |
| `desktop/aizome/apps/shashin/src/instance.rs` | 73 | machines | [SAP-02](#sap-02) | single-instance claim and serve thread |
| `desktop/aizome/apps/shashin/src/lib.rs` | 22 | none |  | module list |
| `desktop/aizome/apps/shashin/src/load.rs` | 388 | none |  | pure decoding; font db OnceLock |
| `desktop/aizome/apps/shashin/src/main.rs` | 32 | machines | [SAP-02](#sap-02) | instance claim |
| `desktop/aizome/apps/shashin/src/orient.rs` | 273 | none |  | pure EXIF orientation rewriting |
| `desktop/aizome/apps/shashin/src/platform.rs` | 11 | none |  | full-screen wrapper |
| `desktop/aizome/apps/shashin/src/save.rs` | 148 | machines | [SAP-11](#sap-11) | save_turned relative to file state; temp+rename |
| `desktop/aizome/apps/shashin/src/thumbs.rs` | 138 | machines | [SAP-16](#sap-16) | mtime-stamped thumbnail cache |
| `desktop/aizome/apps/shashin/src/trash.rs` | 264 | machines | [SAP-18](#sap-18) | O_EXCL info claim, rename, restore |
| `desktop/aizome/apps/shashin/src/ui.rs` | 745 | machines | [SAP-13](#sap-13), [SAP-20](#sap-20), [SAP-11](#sap-11) | stage reporter dedup, pan events, Save enablement |
| `desktop/aizome/apps/shashin/src/view.rs` | 222 | machines | [SAP-12](#sap-12) | Zoom enum; otherwise pure zoom/pan math |
| `desktop/aizome/apps/shashin/tests/budget.rs` | 144 | none |  | tests (frame timing) |
| `desktop/aizome/apps/shashin/tests/common/mod.rs` | 282 | machines | [SAP-21](#sap-21) | settle loop harness; fixture pictures |
| `desktop/aizome/apps/shashin/tests/modal_commands.rs` | 37 | none |  | tests |
| `desktop/aizome/apps/shashin/tests/shashin.rs` | 515 | machines | [SAP-21](#sap-21) | tests; instance hand-off poll loop |
| `desktop/aizome/apps/soroban/src/app.rs` | 753 | machines | [SAP-22](#sap-22), [SAP-23](#sap-23), [SAP-19](#sap-19), [SAP-02](#sap-02) | persist debounce, save sheet, key flash |
| `desktop/aizome/apps/soroban/src/bits.rs` | 346 | machines | [SAP-24](#sap-24) | bit grid hover/cursor widget state |
| `desktop/aizome/apps/soroban/src/display.rs` | 123 | none |  | paint only |
| `desktop/aizome/apps/soroban/src/entry.rs` | 1578 | machines | [SAP-25](#sap-25) | calculator entry; Mode enum explicit |
| `desktop/aizome/apps/soroban/src/grouped.rs` | 108 | none |  | text layout helpers |
| `desktop/aizome/apps/soroban/src/instance.rs` | 62 | machines | [SAP-02](#sap-02) | single-instance claim and serve thread |
| `desktop/aizome/apps/soroban/src/keypad.rs` | 463 | machines | [SAP-19](#sap-19) | paints key flash; press state from kit |
| `desktop/aizome/apps/soroban/src/lib.rs` | 15 | none |  | module list |
| `desktop/aizome/apps/soroban/src/main.rs` | 16 | machines | [SAP-02](#sap-02) | instance claim |
| `desktop/aizome/apps/soroban/src/tape.rs` | 280 | machines | [SAP-26](#sap-26) | tape follow-the-end scroll and cursor |
| `desktop/aizome/apps/soroban/tests/soroban.rs` | 363 | none |  | tests |
