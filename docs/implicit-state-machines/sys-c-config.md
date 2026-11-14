# SYC: System clients: printers and sysinfo; config, theme and face

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/crates/aizome-sys, aizome-config, aizome-theme, aizome-face`. Audited at `8e0acbe5`; line numbers refer to that commit.

55 files audited. 28 implicit machines: 5 High, 3 Medium, 20 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#syc-01)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#syc-02)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#syc-03)

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
- **Reproduction:** Partly confirmed (integration): [summary](reproductions.md#syc-06)

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
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#syc-07)
