# Implicit state machines

This is a catalog of the implicit state machines in Sekka, Aizome first. A
follow-up will turn some of them into explicit state machines. The rest stay
implicit, but they are recorded here too, so they are known before anyone
changes the code around them.

The audit covered `main` at `8e0acbe5`. Line numbers refer to that commit.

This page holds the method, the audit status and the index of every machine
with its priority, follow-up status and location. The code is divided into 33
slices; each has a page under [`implicit-state-machines/`](implicit-state-machines/)
with the entries of its machines rated High: their states, transitions and
hazards. The other machines are recorded by their index rows.
[Suspected bugs](implicit-state-machines/reproductions.md) lists every
suspected bug with the outcome of reproducing it.

This catalog is pared down from the full audit. The audit also wrote an entry
for every Medium and Low machine, listed the machines that were already
explicit, kept a ledger of every file with the result of its audit, and kept
each reproduction's test and output. Those are in the history of
[#282](https://github.com/bitemyapp/sekka/pull/282).

## Method

Claude Code agents carried out the audit, one per slice, each reading every
file in its slice. All 13,432 `path:line` citations were then checked
mechanically against `8e0acbe5`: the file exists and the line is in range.
Where an identifier is named beside a citation, the check also looked for it
at the cited line or in the function enclosing it. The citations it flagged
were re-read against the code and corrected, or rewritten with full paths
where the file was ambiguous. The audit read the code; it did not build or
run it. The suspected bugs on High machines were then reproduced with tests,
as [Suspected bugs](#suspected-bugs) describes.

Each auditor rated its machines High, Medium or Low by the criteria under
**Reify** in [Reading an entry](#reading-an-entry).

### What counts

A state machine is implicit when behaviour depends on a phase or mode that
persists across events, calls, ticks, processes or boots, but the states and
transitions are not one enum with one place that changes it. Typical
encodings:

- several `bool`, `Option`, counter or `Instant` fields that together encode
  a phase, so that invalid combinations are representable;
- an enum whose companion fields hold phase-dependent data, or whose
  transitions are scattered over many handlers ("partially explicit");
- in-flight and dirty coalescing, generation or serial counters that drop
  stale results, deadlines, debounce and grace periods, retry and backoff;
- process, thread and helper lifecycles (spawn, handshake, exit, kill), and
  `Drop` impls whose cleanup depends on the phase;
- Wayland, D-Bus, PAM, polkit and IPC object lifecycles;
- interaction modes such as drag, resize, press and capture, menus, modal
  sheets, IME composition, scroll momentum and animation phases;
- state carried in files, branches, systemd ordering and marker files, and in
  the upstream patches Sekka carries.

A compute-once cache, a lone render `dirty` flag, configuration values and
pure functions are not entries. Machines that are already explicit (one enum
carrying its data, transitions in one place) are not entries either.

### Reading an entry

Every machine has a stable ID (`CPA-07`: slice prefix and number) and a row
in the [index](#index). A machine rated High also has an entry on its slice
page, with these fields:

- **Where**: the primary state and the files involved.
- **Encoding**: the fields, variables, files or units that hold the state.
- **States**: the reachable states, and the invalid combinations the
  encoding can represent.
- **Transitions**: the events that move between states, with locations.
- **Hazards**: invariants the code relies on without enforcing them, races,
  missing timeouts or exits, stale results, cleanup on drop.
- **Reify**: the priority for making the machine explicit. **High** means
  state spread across three or more fields or handlers, reachable invalid
  combinations, user-visible, privilege, data-loss or hang consequences, or
  a suspected bug. **Medium** means moderate spread or guarded invalid
  combinations. **Low** means small and local; leaving it implicit is fine.
- **Suspected bug** (optional): a concrete failure scenario found while
  tracing transitions.
- **Reproduction**: for a suspected bug on a High machine, the verdict of the
  test written to reproduce it and the test's level, linked to the bug's row
  in [Suspected bugs](implicit-state-machines/reproductions.md). Suspected
  bugs on Medium and Low machines were not reproduced and remain leads.
- **Related** (optional): the other side of a protocol in another component.
- **Follow-up** (optional): what a later change did to the machine or its
  bug.

Machines whose name starts with `(test harness)` or `(dev tool)` are
in tests and development tooling. They are recorded for completeness and are
normally Low.

### Scope

In scope: every tracked first-party file with code or behaviour: Rust, C,
Python, shell, Nix, systemd and D-Bus units, Lua and other configuration that
drives behaviour, CI workflows, and the patches Sekka carries against
upstream projects. Tests and their fixtures are included.

Out of scope:

- `apps/hotaru/upstream/`: archived experiment sources and evidence for the
  Ghostty port. They are not workspace members and nothing ships them.
- `apps/hotaru/vendor/simdutf8` and `desktop/aizome/crates/hotaru-link-regex/vendor/`
  (Oniguruma): vendored third-party code.
- Images, fonts, media fixtures, recorded logs and JSON evidence, lockfiles,
  `Cargo.toml` manifests, desktop entries, prose documentation and the
  untracked files of a working checkout.

## Audit status

Each slice has its own page with the entries of its High machines. `Files` and `Lines` count what the audit read.

| Slice | Component | Files | Lines | Machines | High | Medium | Low |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| [CPA](implicit-state-machines/comp-a.md) | Compositor core: backends, recovery, pacing and rendering | 28 | 9257 | 32 | 10 | 8 | 14 |
| [CPB](implicit-state-machines/comp-b.md) | Compositor core: state, input, focus and protocols | 31 | 10023 | 34 | 12 | 12 | 10 |
| [WM](implicit-state-machines/wm-comp-tests.md) | Window manager logic and compositor integration tests | 62 | 22613 | 38 | 9 | 8 | 21 |
| [SHA](implicit-state-machines/shell-a.md) | Shell: main loop, startup, menubar, deskbar, desktop and palette | 19 | 12122 | 26 | 7 | 9 | 10 |
| [SHB](implicit-state-machines/shell-b.md) | Shell: lock, notifications, recording, systray and services | 61 | 13838 | 30 | 7 | 7 | 16 |
| [SVC](implicit-state-machines/kit-services.md) | Applications index, media, IPC, portal and greeter | 52 | 19784 | 41 | 4 | 10 | 27 |
| [AZN](implicit-state-machines/aizome-small-nix.md) | Small crates, session launcher, Nix modules and VM tests | 114 | 18340 | 29 | 5 | 2 | 22 |
| [SYA](implicit-state-machines/sys-a.md) | System clients: launch, audio, Bluetooth, disks, MPRIS | 20 | 10434 | 19 | 5 | 9 | 5 |
| [SYB](implicit-state-machines/sys-b.md) | System clients: network, notifications, polkit, power, tray, users | 20 | 9551 | 16 | 5 | 8 | 3 |
| [SYC](implicit-state-machines/sys-c-config.md) | System clients: printers and sysinfo; config, theme and face | 55 | 21551 | 28 | 5 | 3 | 20 |
| [KIT](implicit-state-machines/kit-core.md) | Toolkit runtime | 60 | 20272 | 33 | 7 | 9 | 17 |
| [UIW](implicit-state-machines/sukumo-ui-kit-widgets.md) | Sukumo UI and toolkit widgets | 34 | 21698 | 34 | 8 | 11 | 15 |
| [SKB](implicit-state-machines/sukumo-backend.md) | Sukumo engine: Wayland client backend | 30 | 14374 | 42 | 5 | 20 | 17 |
| [SKR](implicit-state-machines/sukumo-render.md) | Sukumo engine: animation, text and raster | 30 | 18798 | 17 | 2 | 6 | 9 |
| [STA](implicit-state-machines/settei-a.md) | Settei: general, keyboard, network, sound and other panes | 29 | 12991 | 22 | 7 | 3 | 12 |
| [STB](implicit-state-machines/settei-b.md) | Settei: app shell, appearance, Bluetooth and displays | 42 | 12593 | 20 | 4 | 8 | 8 |
| [HAKO](implicit-state-machines/hako.md) | Hako file manager | 45 | 14983 | 31 | 11 | 6 | 14 |
| [HOT](implicit-state-machines/hotaru-app.md) | Hotaru's Aizome frontend and link matcher | 51 | 19217 | 40 | 10 | 9 | 21 |
| [KOY](implicit-state-machines/koyomi.md) | Koyomi calendar | 31 | 12322 | 19 | 10 | 5 | 4 |
| [YUK](implicit-state-machines/yukimi.md) | Yukimi system manager (Aizome app and core crates) | 62 | 20011 | 30 | 11 | 9 | 10 |
| [NTU](implicit-state-machines/nisshi-tokei-utsushi.md) | Nisshi, Tokei and Utsushi | 56 | 17286 | 33 | 6 | 9 | 18 |
| [KKO](implicit-state-machines/kura-koe.md) | Kura and Koe | 52 | 18603 | 37 | 9 | 8 | 20 |
| [TSH](implicit-state-machines/tansu-shiori.md) | Tansu and Shiori | 42 | 17040 | 32 | 8 | 8 | 16 |
| [SMT](implicit-state-machines/sumi-myaku-tenki.md) | Sumi, Myaku and Tenki | 58 | 20675 | 33 | 10 | 6 | 17 |
| [SAP](implicit-state-machines/small-apps.md) | Shashin, Soroban, Fusen and Annai | 48 | 13573 | 26 | 10 | 3 | 13 |
| [HCO](implicit-state-machines/hotaru-core.md) | Hotaru terminal engine | 41 | 16639 | 26 | 7 | 8 | 11 |
| [HTT](implicit-state-machines/hotaru-tests-tools.md) | Hotaru engine tests and development tools | 102 | 14734 | 28 | 2 | 7 | 19 |
| [GEN](implicit-state-machines/genkan.md) | Genkan installer | 83 | 19128 | 37 | 7 | 8 | 22 |
| [TTL](implicit-state-machines/tatami-tool.md) | Tatami command, display watcher and configuration | 39 | 11922 | 29 | 8 | 9 | 12 |
| [TAP](implicit-state-machines/tatami-apps.md) | Tatami settings apps | 50 | 19991 | 33 | 9 | 11 | 13 |
| [KTL](implicit-state-machines/kakehashi-tools.md) | Kakehashi, respin tools and scripts | 47 | 10261 | 31 | 8 | 10 | 13 |
| [INX](implicit-state-machines/infra-nix.md) | Nix integration, carried patches and live-media modules | 128 | 13538 | 46 | 5 | 4 | 37 |
| [ICI](implicit-state-machines/infra-ci-tests.md) | CI workflows, websites and repository tests | 77 | 16866 | 27 | 3 | 5 | 19 |
| **Total** | | **1699** | **525028** | **999** | **236** | **258** | **505** |

Every in-scope file was audited.

## Index

`Follow-up` starts as `implicit` for every machine. Change it to `reified (#PR)` when a machine becomes an explicit state machine, `implicit, bug fixed (#PR)` when a fix leaves it implicit, or `kept implicit` with a reason after review. Keep the ID either way. An ID that links has an entry on its slice page. `Bug` links to the bug's row in [Suspected bugs](implicit-state-machines/reproductions.md). `Where` is the first location the audit recorded for the machine, its primary state, at `8e0acbe5`.

### Aizome

#### [CPA: Compositor core: backends, recovery, pacing and rendering](implicit-state-machines/comp-a.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| CPA-01 | Headless output refresh pacing and buffer age | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/backend/headless.rs:43` |
| [CPA-02](implicit-state-machines/comp-a.md#cpa-02) | Per-output redraw scheduling (queued, due, drawn) across backends | High |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/backend/mod.rs:81` |
| CPA-03 | Udev per-CRTC page-flip state | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:96` |
| [CPA-04](implicit-state-machines/comp-a.md#cpa-04) | libseat session pause and resume (VT switching) | High | [environment](implicit-state-machines/reproductions.md#cpa-04) | implicit | `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:219` |
| [CPA-05](implicit-state-machines/comp-a.md#cpa-05) | DRM device and connector hotplug, and output enablement | High | [environment](implicit-state-machines/reproductions.md#cpa-05) | implicit | `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:130` |
| CPA-06 | Winit full-redraw countdown | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/backend/winit.rs:29` |
| CPA-07 | Blur backdrop cache validity | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/blur.rs:274` |
| CPA-08 | Fallback (no-logic) window focus and stacking | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/fallback.rs:84` |
| CPA-09 | Layer-surface ghosts (last frames kept after unmap) | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/ghosts.rs:33` |
| CPA-10 | Idle progression to lock and monitors-off | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/idle.rs:17` |
| CPA-11 | Night Light application | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/night.rs:18` |
| [CPA-12](implicit-state-machines/comp-a.md#cpa-12) | Output layout membership and wl_output global retirement | High |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/outputs.rs:62` |
| CPA-13 | FIFO barrier release and backstop | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/pacing.rs:45` |
| CPA-14 | Commit-timing (timestamped commit) release timer | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/pacing.rs:51` |
| [CPA-15](implicit-state-machines/comp-a.md#cpa-15) | Display power and lid docking layout | High |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/power.rs:16` |
| [CPA-16](implicit-state-machines/comp-a.md#cpa-16) | Recovery overlay session (open and closed) | High | [confirmed](implicit-state-machines/reproductions.md#cpa-16) | implicit | `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:176` |
| CPA-17 | Two-press confirmation (Log Out, Text Console) | Medium | [unverified](implicit-state-machines/reproductions.md#cpa-17) | implicit | `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:192` |
| CPA-18 | Overlay ownership of key and button releases | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:195` |
| CPA-19 | Pointer-constraint hold across the overlay and Reset Input | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:208` |
| CPA-20 | Recovery ping and sample cadence, and the sample cache | Medium | [unverified](implicit-state-machines/reproductions.md#cpa-20) | implicit | `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:197` |
| CPA-21 | Force Quit escalation (SIGKILL, then cut the connection) | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:976` |
| CPA-22 | Re-locking after a shell restart while locked | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:1029` |
| CPA-23 | Per-client ping and pong responsiveness | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/recovery/ping.rs:20` |
| [CPA-24](implicit-state-machines/comp-a.md#cpa-24) | Process-info worker: request coalescing, CPU baselines and kills | High | [confirmed](implicit-state-machines/reproductions.md#cpa-24) | implicit | `desktop/aizome/crates/aizome-comp-core/src/recovery/procinfo.rs:140` |
| CPA-25 | X11 PID lookup thread with a deadline | Low | [unverified](implicit-state-machines/reproductions.md#cpa-25) | implicit | `desktop/aizome/crates/aizome-comp-core/src/recovery/x11.rs:19` |
| CPA-26 | Deferred logic reload request | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/reload.rs:14` |
| [CPA-27](implicit-state-machines/comp-a.md#cpa-27) | Supervised helper process lifecycle (shell, xwayland-satellite) | High | [confirmed](implicit-state-machines/reproductions.md#cpa-27) | implicit | `desktop/aizome/crates/aizome-comp-core/src/session.rs:24` |
| [CPA-28](implicit-state-machines/comp-a.md#cpa-28) | Shell restart handover | High |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/session.rs:59` |
| CPA-29 | Session start-up and systemd notification sequence | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/session.rs:168` |
| CPA-30 | Pixman thumbnail cache | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/thumbs.rs:42` |
| CPA-31 | Logic timer generation guard | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/timers.rs:22` |
| [CPA-32](implicit-state-machines/comp-a.md#cpa-32) | Main-loop stall watcher, systemd watchdog and the stall marker | High | [confirmed](implicit-state-machines/reproductions.md#cpa-32) | implicit | `desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:40` |

#### [CPB: Compositor core: state, input, focus and protocols](implicit-state-machines/comp-b.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| CPB-01 | Live settings reconciliation against the last-applied snapshot | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/config.rs:27` |
| [CPB-02](implicit-state-machines/comp-b.md#cpb-02) | xdg_popup grab (menu chain) lifecycle | High |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/focus.rs:164` |
| CPB-03 | Keyboard focus arbitration (who should hold the keys) | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/focus.rs:388` |
| CPB-04 | Guarded logic pointer grab | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/focus.rs:430` |
| CPB-05 | Client drag-and-drop icon | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/state.rs:317` |
| CPB-06 | xdg-activation token validity | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/state.rs:337` |
| [CPB-07](implicit-state-machines/comp-b.md#cpb-07) | Idle-inhibitor registry | High | [confirmed](implicit-state-machines/reproductions.md#cpb-07) | implicit | `desktop/aizome/crates/aizome-comp-core/src/state.rs:335` |
| [CPB-08](implicit-state-machines/comp-b.md#cpb-08) | Pointer constraint activation (lock and confine) | High | [confirmed](implicit-state-machines/reproductions.md#cpb-08) | implicit | `desktop/aizome/crates/aizome-comp-core/src/handlers.rs:254` |
| CPB-09 | Input-method commit serial (bypassed) | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/handlers.rs:352` |
| CPB-10 | Rasterized asset caches (app icons, text labels) | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/icons.rs:38` |
| [CPB-11](implicit-state-machines/comp-b.md#cpb-11) | Key press/release pairing (suppressed and translated keys) | High |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/state.rs:329` |
| CPB-12 | Forwarded-button accounting | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/state.rs:347` |
| CPB-13 | IPC connection lifecycle | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/ipc.rs:20` |
| CPB-14 | Typing-keyboard keymap switching | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/keyboards.rs:92` |
| CPB-15 | IPC key-catch request | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/keyboards.rs:106` |
| CPB-16 | Compositor process startup and shutdown sequence | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/lib.rs:108` |
| [CPB-17](implicit-state-machines/comp-b.md#cpb-17) | Compositor logic library lifecycle (load, reload, poison, rollback, set aside) | High |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/logic.rs:229` |
| [CPB-18](implicit-state-machines/comp-b.md#cpb-18) | Popup tracking and popup-layer stacking (xdg and input-method popups) | High | [confirmed](implicit-state-machines/reproductions.md#cpb-18) | implicit | `desktop/aizome/crates/aizome-comp-core/src/state.rs:325` |
| CPB-19 | Capture stream damage and scratch-buffer state | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/protocols/capture.rs:65` |
| [CPB-20](implicit-state-machines/comp-b.md#cpb-20) | Foreign-toplevel handle synchronisation (wlr management and ext list) | High | [confirmed](implicit-state-machines/reproductions.md#cpb-20) | implicit | `desktop/aizome/crates/aizome-comp-core/src/protocols/foreign_toplevel.rs:25` |
| [CPB-21](implicit-state-machines/comp-b.md#cpb-21) | Gamma-control ownership per output | High | [confirmed](implicit-state-machines/reproductions.md#cpb-21) | implicit | `desktop/aizome/crates/aizome-comp-core/src/state.rs:153` |
| [CPB-22](implicit-state-machines/comp-b.md#cpb-22) | ext-image-copy-capture session and frame lifecycle | High | [partly](implicit-state-machines/reproductions.md#cpb-22) | implicit | `desktop/aizome/crates/aizome-comp-core/src/protocols/image_copy_capture.rs:73` |
| CPB-23 | Screen-share reporting (settle delay and last report) | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/protocols/image_copy_capture.rs:62` |
| [CPB-24](implicit-state-machines/comp-b.md#cpb-24) | wlr-output-management configuration lifecycle | High | [environment](implicit-state-machines/reproductions.md#cpb-24) | implicit | `desktop/aizome/crates/aizome-comp-core/src/protocols/output_management.rs:27` |
| [CPB-25](implicit-state-machines/comp-b.md#cpb-25) | wlr-screencopy frame lifecycle and stream pool | High | [confirmed](implicit-state-machines/reproductions.md#cpb-25) | implicit | `desktop/aizome/crates/aizome-comp-core/src/protocols/screencopy.rs:46` |
| CPB-26 | Toplevel icon build and freeze | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/protocols/toplevel_icon.rs:35` |
| CPB-27 | Layer-surface lifecycle and usable-area tracking | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/shell.rs:663` |
| CPB-28 | Decoration-mode negotiation | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/shell.rs:631` |
| CPB-29 | dmabuf-fence commit blockers | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/shell.rs:77` |
| [CPB-30](implicit-state-machines/comp-b.md#cpb-30) | Toplevel map/unmap lifecycle and window registry | High |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/state.rs:278` |
| CPB-31 | Session lock (partially explicit) | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/state.rs:186` |
| CPB-32 | Per-output frame and presentation state | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/state.rs:136` |
| CPB-33 | Loop-iteration pipeline and dirty-flag coalescing | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/state.rs:572` |
| CPB-34 | Default-terminal index and one-time Hotaru migration marker | Low |  | implicit | `desktop/aizome/crates/aizome-comp-core/src/terminal.rs:27` |

#### [WM: Window manager logic and compositor integration tests](implicit-state-machines/wm-comp-tests.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| WM-01 | Toplevel configure size negotiation (requested size until the client commits it) | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-logic/src/lib.rs:105` |
| WM-02 | Motion timelines (Desk slides, tab slides, overlay entrances) | Low |  | implicit | `desktop/aizome/crates/aizome-comp-logic/src/anim.rs:54` |
| WM-03 | Window move and tab-drag gesture | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:135` |
| [WM-04](implicit-state-machines/wm-comp-tests.md#wm-04) | Interactive resize and its edge anchor | High | [confirmed](implicit-state-machines/reproductions.md#wm-04) | implicit | `desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:322` |
| WM-05 | Tab slide and close/zoom box grabs | Low |  | implicit | `desktop/aizome/crates/aizome-comp-logic/src/grabs.rs:433` |
| WM-06 | Logic instance lifecycle and reload handoff | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:56` |
| WM-07 | View publication and change coalescing | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-logic/src/lib.rs:89` |
| WM-08 | Key decision pipeline (bound action, translation, suppression) | Low |  | implicit | `desktop/aizome/crates/aizome-comp-logic/src/lib.rs:87` |
| WM-09 | Slow keys | Low |  | implicit | `desktop/aizome/crates/aizome-comp-logic/src/input.rs:777` |
| [WM-10](implicit-state-machines/wm-comp-tests.md#wm-10) | Sticky keys (latched modifiers) | High | [confirmed](implicit-state-machines/reproductions.md#wm-10) | implicit | `desktop/aizome/crates/aizome-comp-logic/src/input.rs:775` |
| WM-11 | Pointer interaction bookkeeping (hover, double-click, overlay-owned presses) | Low |  | implicit | `desktop/aizome/crates/aizome-comp-logic/src/input.rs:31` |
| [WM-12](implicit-state-machines/wm-comp-tests.md#wm-12) | Area picker (screenshots and IPC `pick-area`) | High | [confirmed](implicit-state-machines/reproductions.md#wm-12) | implicit | `desktop/aizome/crates/aizome-comp-logic/src/overlays/area.rs:20` |
| [WM-13](implicit-state-machines/wm-comp-tests.md#wm-13) | Marking menu | High |  | implicit | `desktop/aizome/crates/aizome-comp-logic/src/overlays/marking.rs:119` |
| [WM-14](implicit-state-machines/wm-comp-tests.md#wm-14) | Overlay modality (which overlay owns input) | High |  | implicit | `desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:24` |
| [WM-15](implicit-state-machines/wm-comp-tests.md#wm-15) | Desks overview | High | [confirmed](implicit-state-machines/reproductions.md#wm-15) | implicit | `desktop/aizome/crates/aizome-comp-logic/src/overlays/overview.rs:26` |
| WM-16 | App switcher | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-logic/src/overlays/switcher.rs:28` |
| WM-17 | Notification card slides | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-logic/src/slides.rs:77` |
| [WM-18](implicit-state-machines/wm-comp-tests.md#wm-18) | Window geometry mode (placement, restore, display home) | High |  | implicit | `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:52` |
| [WM-19](implicit-state-machines/wm-comp-tests.md#wm-19) | Focus and visibility | High |  | implicit | `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:181` |
| WM-20 | Tab groups | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:136` |
| [WM-21](implicit-state-machines/wm-comp-tests.md#wm-21) | Displays and their Desks | High |  | implicit | `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:185` |
| WM-22 | Desk parking (suspended Desks) | Medium |  | implicit | `desktop/aizome/crates/aizome-comp-logic/src/wm/mod.rs:152` |
| WM-23 | (test harness) frame-time benchmark driver | Low |  | implicit | `desktop/aizome/crates/aizome-comp/examples/frame_bench.rs:19` |
| WM-24 | (test harness) sidestep test client modes | Low |  | implicit | `desktop/aizome/crates/aizome-comp/examples/sidestep_client.rs:31` |
| WM-25 | (test harness) raw Wayland probe clients recording protocol events | Low |  | implicit | `desktop/aizome/crates/aizome-comp/tests/chromium.rs:27` |
| WM-26 | (test harness) external client processes and poll-until loops | Low |  | implicit | `desktop/aizome/crates/aizome-comp/tests/common/desktop.rs:9` |
| WM-27 | (test harness) shell-like layer and popup client | Low |  | implicit | `desktop/aizome/crates/aizome-comp/tests/common/layer.rs:182` |
| WM-28 | (test harness) headless compositor process | Low |  | implicit | `desktop/aizome/crates/aizome-comp/tests/common/mod.rs:36` |
| WM-29 | (test harness) solid-colour toplevel test client | Low |  | implicit | `desktop/aizome/crates/aizome-comp/tests/common/mod.rs:540` |
| WM-30 | (test harness) session-lock client | Low |  | implicit | `desktop/aizome/crates/aizome-comp/tests/common/mod.rs:1024` |
| WM-31 | (test harness) text-field app with a grabbing context menu | Low |  | implicit | `desktop/aizome/crates/aizome-comp/tests/common/text.rs:125` |
| WM-32 | (test harness) ibus-like input method with keyboard grab | Low |  | implicit | `desktop/aizome/crates/aizome-comp/tests/common/text.rs:514` |
| WM-33 | (test harness) screen-capture clients (image-copy-capture and wlr-screencopy) | Low |  | implicit | `desktop/aizome/crates/aizome-comp/tests/image_copy_capture.rs:41` |
| WM-34 | (test harness) input method counting its serial as IBus does | Low |  | implicit | `desktop/aizome/crates/aizome-comp/tests/input_method.rs:26` |
| WM-35 | (test harness) recovery probe client (freeze, pointer lock, late move) | Low |  | implicit | `desktop/aizome/crates/aizome-comp/tests/recovery.rs:567` |
| WM-36 | (test harness) logic variant builds for reload tests | Low |  | implicit | `desktop/aizome/crates/aizome-comp/tests/reload.rs:25` |
| WM-37 | (test harness) shell restart scenario with a private D-Bus | Low |  | implicit | `desktop/aizome/crates/aizome-comp/tests/reload.rs:452` |
| WM-38 | (test harness) udev backend VM scenario | Low |  | implicit | `desktop/aizome/crates/aizome-comp/vm/udev.nix:52` |

#### [SHA: Shell: main loop, startup, menubar, deskbar, desktop and palette](implicit-state-machines/shell-a.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| [SHA-01](implicit-state-machines/shell-a.md#sha-01) | Bar menu open / slide / press-to-close, and the Toolchest-to-palette handover | High |  | implicit | `desktop/aizome/crates/aizome-shell/src/barmenus.rs:17` |
| [SHA-02](implicit-state-machines/shell-a.md#sha-02) | Shell-replacement carry handoff (state file, final word, retiring) | High | [confirmed](implicit-state-machines/reproductions.md#sha-02) | implicit | `desktop/aizome/crates/aizome-shell/src/carry.rs:23` |
| [SHA-03](implicit-state-machines/shell-a.md#sha-03) | Deskbar auto-hide visibility | High | [confirmed](implicit-state-machines/reproductions.md#sha-03) | implicit | `desktop/aizome/crates/aizome-shell/src/deskbar_glue.rs:20` |
| SHA-04 | Deskbar surface lifecycle (open, resize, reopen on display/kind/edge change) | Medium |  | implicit | `desktop/aizome/crates/aizome-shell/src/shell.rs:238` |
| SHA-05 | Deskbar row drags (reorder an app; window onto a Desk) | Medium |  | implicit | `desktop/aizome/crates/aizome-shell/src/shell.rs:317` |
| SHA-06 | Deskbar edge-resize drag and its preview surface | Medium |  | implicit | `desktop/aizome/crates/aizome-shell/src/shell.rs:327` |
| SHA-07 | Deskbar grip-move drag and its landing surface | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/shell.rs:332` |
| SHA-08 | Per-app Deskbar fold and notification-expansion toggles | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/shell.rs:244` |
| SHA-09 | Desktop icon selection, icon drag and rubber band | Medium |  | implicit | `desktop/aizome/crates/aizome-shell/src/desktop.rs:81` |
| SHA-10 | Wallpaper scaling pipeline | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/shell.rs:279` |
| SHA-11 | Change-polling watcher threads and the one-shot desktop refresh | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/desktop_glue.rs:400` |
| [SHA-12](implicit-state-machines/shell-a.md#sha-12) | Palette open/close and its drawn-ahead spare window | High | [confirmed](implicit-state-machines/reproductions.md#sha-12) | implicit | `desktop/aizome/crates/aizome-shell/src/shell.rs:260` |
| [SHA-13](implicit-state-machines/shell-a.md#sha-13) | Palette file search, index lifetime and per-opening extras | High | [confirmed](implicit-state-machines/reproductions.md#sha-13) | implicit | `desktop/aizome/crates/aizome-shell/src/shell.rs:264` |
| SHA-14 | File index cache: validity, rebuild and refresh timer | Medium | [unverified](implicit-state-machines/reproductions.md#sha-14) | implicit | `desktop/aizome/crates/aizome-shell/src/palette/mod.rs:811` |
| SHA-15 | Frecency store: refresh on staleness, record on a thread | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/shell.rs:263` |
| SHA-16 | Shelf items and parked-image files | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/shelf.rs:141` |
| SHA-17 | Per-output bar/desktop surfaces and the surface registry | Medium |  | implicit | `desktop/aizome/crates/aizome-shell/src/shell.rs:237` |
| SHA-18 | Desk rename in place | Medium | [unverified](implicit-state-machines/reproductions.md#sha-18) | implicit | `desktop/aizome/crates/aizome-shell/src/shell.rs:241` |
| SHA-19 | Launch waiting for an activation token | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/shell.rs:374` |
| SHA-20 | App index load and reload | Low | [unverified](implicit-state-machines/reproductions.md#sha-20) | implicit | `desktop/aizome/crates/aizome-shell/src/shell.rs:233` |
| SHA-21 | Clock tick timer cadence | Low | [unverified](implicit-state-machines/reproductions.md#sha-21) | implicit | `desktop/aizome/crates/aizome-shell/src/shell.rs:1474` |
| SHA-22 | logind sleep-delay handshake (session-events side) | Medium | [unverified](implicit-state-machines/reproductions.md#sha-22) | implicit | `desktop/aizome/crates/aizome-shell/src/shell.rs:2041` |
| [SHA-23](implicit-state-machines/shell-a.md#sha-23) | Launch attempt lifecycle (Application Launches) | High |  | implicit | `desktop/aizome/crates/aizome-shell/src/startup.rs:40` |
| SHA-24 | Application Launches window and its journal reads | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/startup.rs:87` |
| [SHA-25](implicit-state-machines/shell-a.md#sha-25) | Torn-off palettes (persisted list, shown surfaces, strip drag) | High | [confirmed](implicit-state-machines/reproductions.md#sha-25) | implicit | `desktop/aizome/crates/aizome-shell/src/tearoff.rs:28` |
| SHA-26 | Mirror of the compositor's state (windows, focus, Desks, lock) | Medium |  | implicit | `desktop/aizome/crates/aizome-shell/src/wm.rs:27` |

#### [SHB: Shell: lock, notifications, recording, systray and services](implicit-state-machines/shell-b.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| SHB-01 | App connection registry (hello, menus, goodbye) | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/apps/registry.rs:27` |
| SHB-02 | Shell socket ownership and per-connection threads | Medium |  | implicit | `desktop/aizome/crates/aizome-shell/src/apps/server.rs:25` |
| [SHB-03](implicit-state-machines/shell-b.md#shb-03) | Polkit authentication dialog | High | [confirmed](implicit-state-machines/reproductions.md#shb-03) | reified ([#298](https://github.com/bitemyapp/sekka/pull/298)) | `desktop/aizome/crates/aizome-shell/src/auth.rs:58` |
| SHB-04 | Keep Awake status (request, watcher, indicator) | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/awake.rs:80` |
| SHB-05 | Timed overlay surfaces (bell flash, OSD) | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/bell.rs:40` |
| [SHB-06](implicit-state-machines/shell-b.md#shb-06) | Session confirmation countdown (Restart, Shut Down, Log Out, power key) | High | [confirmed](implicit-state-machines/reproductions.md#shb-06) | implicit | `desktop/aizome/crates/aizome-shell/src/confirm.rs:36` |
| SHB-07 | Login display-scales publisher (coalescing worker) | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/greeter.rs:7` |
| SHB-08 | GTK look publication to dconf | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/gtk.rs:51` |
| [SHB-09](implicit-state-machines/shell-b.md#shb-09) | Compositor event follower (connect, snapshot, subscribe, reconnect) | High | [confirmed](implicit-state-machines/reproductions.md#shb-09) | implicit | `desktop/aizome/crates/aizome-shell/src/ipc.rs:122` |
| SHB-10 | Compositor request sender (lazy connection, one retry) | Medium |  | implicit | `desktop/aizome/crates/aizome-shell/src/ipc.rs:41` |
| SHB-11 | GeoClue location lookup | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/location.rs:48` |
| [SHB-12](implicit-state-machines/shell-b.md#shb-12) | Lock screen lifecycle (partially explicit) | High | [confirmed](implicit-state-machines/reproductions.md#shb-12) | reified ([#298](https://github.com/bitemyapp/sekka/pull/298)) | `desktop/aizome/crates/aizome-shell/src/lock.rs:47` |
| SHB-13 | Lock field shake animation | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/lock.rs:56` |
| [SHB-14](implicit-state-machines/shell-b.md#shb-14) | Critical notification cards surface | High | [confirmed](implicit-state-machines/reproductions.md#shb-14) | implicit | `desktop/aizome/crates/aizome-shell/src/notifications/cards.rs:105` |
| [SHB-15](implicit-state-machines/shell-b.md#shb-15) | Bar notification line (banner) | High | [confirmed](implicit-state-machines/reproductions.md#shb-15) | implicit | `desktop/aizome/crates/aizome-shell/src/notifications/glue.rs:184` |
| SHB-16 | Notifications server: bus name ownership and ids | Medium |  | implicit | `desktop/aizome/crates/aizome-shell/src/notifications/server.rs:161` |
| SHB-17 | Notification lifecycle in the store | Medium |  | implicit | `desktop/aizome/crates/aizome-shell/src/notifications/store.rs:182` |
| SHB-18 | PAM transaction | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/pam.rs:146` |
| SHB-19 | Power key: logind inhibitor and HandlePowerKey | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/power.rs:37` |
| [SHB-20](implicit-state-machines/shell-b.md#shb-20) | Screen recording session (partially explicit) | High | [confirmed](implicit-state-machines/reproductions.md#shb-20) | implicit | `desktop/aizome/crates/aizome-shell/src/recording/mod.rs:332` |
| SHB-21 | Recorder capability probe | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/recording/probe.rs:47` |
| SHB-22 | Recorder supervisor thread (fallback, settle, stop escalation) | Medium |  | implicit | `desktop/aizome/crates/aizome-shell/src/recording/run.rs:154` |
| SHB-23 | Recording handover note and adoption across shell restarts | Medium |  | implicit | `desktop/aizome/crates/aizome-shell/src/recording/run.rs:99` |
| SHB-24 | (test harness) Pretend recorders for the supervisor | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/recording/run.rs:479` |
| SHB-25 | Services selection wait before the pointer menu | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/services.rs:48` |
| SHB-26 | Screen-share indicator settle (vestigial) | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/sharing.rs:19` |
| SHB-27 | Systray panel popup and Wi-Fi password ask | Medium |  | implicit | `desktop/aizome/crates/aizome-shell/src/systray/glue.rs:18` |
| SHB-28 | Status item click and menu fetch | Low |  | implicit | `desktop/aizome/crates/aizome-shell/src/systray/items.rs:165` |
| SHB-29 | (test harness) Lock screen PAM settle loop | Low |  | implicit | `desktop/aizome/crates/aizome-shell/tests/lock.rs:38` |
| SHB-30 | (test harness) Private D-Bus daemon and fake MPRIS player | Low |  | implicit | `desktop/aizome/crates/aizome-shell/tests/media.rs:21` |

#### [SVC: Applications index, media, IPC, portal and greeter](implicit-state-machines/kit-services.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| SVC-01 | Icon lookup cache layers | Low |  | implicit | `desktop/aizome/crates/aizome-apps/src/icons.rs:262` |
| SVC-02 | Application index freshness (signature versus scan) | Medium |  | implicit | `desktop/aizome/crates/aizome-apps/src/index.rs:91` |
| SVC-03 | Index watcher thread lifecycle | Low |  | implicit | `desktop/aizome/crates/aizome-apps/src/index.rs:635` |
| SVC-04 | Default terminal moved from foot to Hotaru (one-shot migration) | Low |  | implicit | `desktop/aizome/crates/aizome-apps/src/mime.rs:590` |
| [SVC-05](implicit-state-machines/kit-services.md#svc-05) | Greeter screen stage (people, password, typed name) | High | [confirmed](implicit-state-machines/reproductions.md#svc-05) | reified ([#298](https://github.com/bitemyapp/sekka/pull/298)) | `desktop/aizome/crates/aizome-greeter/src/app.rs:108` |
| SVC-06 | greetd link thread | Low |  | implicit | `desktop/aizome/crates/aizome-greeter/src/app.rs:130` |
| SVC-07 | Greeter surfaces per display and the main display | Low |  | implicit | `desktop/aizome/crates/aizome-greeter/src/app.rs:123` |
| SVC-08 | Greeter wallpaper loads and the day/night tick | Low |  | implicit | `desktop/aizome/crates/aizome-greeter/src/app.rs:128` |
| SVC-09 | Wrong-password shake animation | Low |  | implicit | `desktop/aizome/crates/aizome-greeter/src/app.rs:118` |
| SVC-10 | Greeter power action and its error | Low |  | implicit | `desktop/aizome/crates/aizome-greeter/src/app.rs:135` |
| SVC-11 | Greeter's private compositor and process teardown | Low |  | implicit | `desktop/aizome/crates/aizome-greeter/src/compositor.rs:15` |
| [SVC-12](implicit-state-machines/kit-services.md#svc-12) | greetd login conversation | High |  | implicit | `desktop/aizome/crates/aizome-greeter/src/login.rs:48` |
| SVC-13 | aizome-dev watch-and-reload loop | Low |  | implicit | `desktop/aizome/crates/aizome-ipc/src/bin/aizome-dev.rs:555` |
| SVC-14 | aizome-dev app restart | Low |  | implicit | `desktop/aizome/crates/aizome-ipc/src/bin/aizome-dev.rs:686` |
| SVC-15 | Share picker's pick-area wait | Low |  | implicit | `desktop/aizome/crates/aizome-ipc/src/bin/aizome-share-picker.rs:105` |
| SVC-16 | `aizomectl reload --offer` flow | Low |  | implicit | `desktop/aizome/crates/aizome-ipc/src/bin/aizomectl.rs:789` |
| SVC-17 | IPC client connection modes | Low |  | implicit | `desktop/aizome/crates/aizome-ipc/src/lib.rs:1046` |
| SVC-18 | Catch-key wait for keyboard setup | Low |  | implicit | `desktop/aizome/crates/aizome-ipc/src/setup.rs:66` |
| SVC-19 | Audio ring flush, halt and segment protocol | Medium |  | implicit | `desktop/aizome/crates/aizome-media/src/audio.rs:33` |
| SVC-20 | Null audio output thread | Low |  | implicit | `desktop/aizome/crates/aizome-media/src/audio.rs:286` |
| SVC-21 | Playback clock anchor | Low |  | implicit | `desktop/aizome/crates/aizome-media/src/clock.rs:17` |
| SVC-22 | PipeWire output thread and stream | Low |  | implicit | `desktop/aizome/crates/aizome-media/src/pipewire.rs:47` |
| [SVC-23](implicit-state-machines/kit-services.md#svc-23) | Playback session lifecycle | High | [confirmed](implicit-state-machines/reproductions.md#svc-23) | implicit | `desktop/aizome/crates/aizome-media/src/player.rs:623` |
| SVC-24 | Seek bookkeeping across threads | Low |  | implicit | `desktop/aizome/crates/aizome-media/src/player.rs:264` |
| SVC-25 | Audio decode part (skip, rebase, drain) | Medium |  | implicit | `desktop/aizome/crates/aizome-media/src/player.rs:591` |
| SVC-26 | Audio output fallback to silence | Medium |  | implicit | `desktop/aizome/crates/aizome-media/src/player.rs:655` |
| SVC-27 | Wake coalescing in the player and video threads | Low |  | implicit | `desktop/aizome/crates/aizome-media/src/player.rs:39` |
| SVC-28 | Video presentation loop (poster, steps, generations) | Medium |  | implicit | `desktop/aizome/crates/aizome-media/src/video.rs:405` |
| [SVC-29](implicit-state-machines/kit-services.md#svc-29) | Video suspension out of sight, and catching up | High | [confirmed](implicit-state-machines/reproductions.md#svc-29) | implicit | `desktop/aizome/crates/aizome-media/src/video.rs:430` |
| SVC-30 | Late-frame skipping hysteresis | Low |  | implicit | `desktop/aizome/crates/aizome-media/src/video.rs:433` |
| SVC-31 | Frame path and the cache of exported surfaces | Medium |  | implicit | `desktop/aizome/crates/aizome-media/src/video.rs:420` |
| SVC-32 | GPU decoding falling back to software | Medium |  | implicit | `desktop/aizome/crates/aizome-media/src/video.rs:436` |
| SVC-33 | Ownership of shared-memory frame slots | Low |  | implicit | `desktop/aizome/crates/aizome-media/src/video.rs:207` |
| SVC-34 | (test harness) Media engine event-wait driver | Low |  | implicit | `desktop/aizome/crates/aizome-media/tests/engine.rs:20` |
| SVC-35 | File chooser's one-shot answer lifecycle | Medium |  | implicit | `desktop/aizome/crates/aizome-portal/src/chooser/mod.rs:102` |
| SVC-36 | Chooser sheet (Go to Folder, New Folder) | Low |  | implicit | `desktop/aizome/crates/aizome-portal/src/chooser/mod.rs:105` |
| SVC-37 | Chooser location, history and listing | Medium |  | implicit | `desktop/aizome/crates/aizome-portal/src/chooser/model.rs:49` |
| SVC-38 | Portal FileChooser request lifecycle | Medium |  | implicit | `desktop/aizome/crates/aizome-portal/src/service.rs:38` |
| SVC-39 | Chooser subprocess supervision | Low |  | implicit | `desktop/aizome/crates/aizome-portal/src/worker.rs:57` |
| SVC-40 | (test harness) Chooser home fixture and settle | Low |  | implicit | `desktop/aizome/crates/aizome-portal/tests/common/mod.rs:17` |
| SVC-41 | (test harness) Private session bus with stand-in choosers | Low |  | implicit | `desktop/aizome/crates/aizome-portal/tests/service.rs:19` |

#### [AZN: Small crates, session launcher, Nix modules and VM tests](implicit-state-machines/aizome-small-nix.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| [AZN-01](implicit-state-machines/aizome-small-nix.md#azn-01) | Keep Awake requested/protected status (setting file against logind's locks) | High | [confirmed](implicit-state-machines/reproductions.md#azn-01) | implicit | `desktop/aizome/crates/aizome-awake/src/lib.rs:49` |
| AZN-02 | Keep Awake holder service lifecycle (`aizome-awake hold`) | Low |  | implicit | `desktop/aizome/crates/aizome-awake/src/main.rs:171` |
| AZN-03 | GetSecrets request registry and cancellation (network agent) | Medium |  | implicit | `desktop/aizome/crates/aizome-network-agent/src/lib.rs:147` |
| [AZN-04](implicit-state-machines/aizome-small-nix.md#azn-04) | Agent registration with NetworkManager (retry, re-register on restart) | High | [confirmed](implicit-state-machines/reproductions.md#azn-04) | implicit | `desktop/aizome/crates/aizome-network-agent/src/lib.rs:370` |
| AZN-05 | Credential worker subprocess lifecycle (keyring, dialog, VPN helper) | Low |  | implicit | `desktop/aizome/crates/aizome-network-agent/src/process.rs:84` |
| AZN-06 | (test harness) private bus with a scripted AgentManager and a blocking dialog stand-in | Low |  | implicit | `desktop/aizome/crates/aizome-network-agent/tests/credentials.rs:301` |
| AZN-07 | Power meter socket activation, idle exit and reading throttle | Low |  | implicit | `desktop/aizome/crates/aizome-power-meter/src/main.rs:80` |
| AZN-08 | File index: query-narrowing cache and persisted index freshness | Low |  | implicit | `desktop/aizome/crates/aizome-search/src/files.rs:91` |
| AZN-09 | Frecency history file: staleness stamp and read-modify-write recording | Low |  | implicit | `desktop/aizome/crates/aizome-search/src/frecency.rs:37` |
| AZN-10 | Shell socket connection protocol (hello, menus, services-text serial, request/reply) | Low |  | implicit | `desktop/aizome/crates/aizome-shell-proto/src/lib.rs:27` |
| AZN-11 | Foot palette mode switched by shell signals | Low |  | implicit | `desktop/aizome/etc/foot/foot.ini:4` |
| AZN-12 | Login screen session handoff and X11 session teardown | Low |  | implicit | `desktop/aizome/nix/greeter.nix:26` |
| [AZN-13](implicit-state-machines/aizome-small-nix.md#azn-13) | Time-zone ownership across activations (saved zone file and `/etc/localtime` link) | High | [confirmed](implicit-state-machines/reproductions.md#azn-13) | implicit | `desktop/aizome/nix/module.nix:261` |
| AZN-14 | Per-login re-arming of session user units from persisted opt-ins | Low |  | implicit | `desktop/aizome/nix/module.nix:305` |
| AZN-15 | (test harness) ScreenCast portal client: request/Response chain and frame count | Low |  | implicit | `desktop/aizome/nix/screencast-check.py:32` |
| AZN-16 | (test harness) real user manager with gated stand-in compositor (launcher systemd test) | Low |  | implicit | `desktop/aizome/nix/session-launcher-systemd-test.py:41` |
| AZN-17 | (test harness) fake systemctl driven by marker files (launcher unit test) | Low |  | implicit | `desktop/aizome/nix/session-launcher-test.py:15` |
| AZN-18 | (test harness) disposable-device fixtures behind VM marker files and case modes | Low |  | implicit | `desktop/aizome/nix/vm-encrypted-volumes-test.py:98` |
| AZN-19 | (test harness) kit-inspection UI driver shared by the component VM tests | Low |  | implicit | `desktop/aizome/nix/vm-file-panel-multiple-test.py:60` |
| AZN-20 | (test harness) login/logout cycles with a lingering user manager | Low |  | implicit | `desktop/aizome/nix/vm-greeter-test.nix:131` |
| AZN-21 | (test harness) virtual radios, VPN server and NetworkManager activation driver | Low |  | implicit | `desktop/aizome/nix/vm-network-auth-test.py:100` |
| AZN-22 | (test harness) polkit outage and recovery across system switches | Low |  | implicit | `desktop/aizome/nix/vm-polkit-recovery-test.py:84` |
| AZN-23 | (test harness) Settings driver and ordered parts sharing system state | Low |  | implicit | `desktop/aizome/nix/vm-settings-test.py:100` |
| AZN-24 | (test harness) session smoke test: reload, recording, sharing and watchdog restart | Low |  | implicit | `desktop/aizome/nix/vm-test.nix:82` |
| [AZN-25](implicit-state-machines/aizome-small-nix.md#azn-25) | xdg-desktop-portal-wlr screencast capture lifecycle (carried patch) | High |  | implicit | `desktop/aizome/nix/xdg-desktop-portal-wlr-captures.patch:76` |
| AZN-26 | (test harness) private headless session drivers (development scripts) | Low |  | implicit | `desktop/aizome/scripts/check-foot-theme:53` |
| [AZN-27](implicit-state-machines/aizome-small-nix.md#azn-27) | Session launcher phases (`aizome-session`) | High |  | implicit | `desktop/aizome/session/aizome-session:17` |
| AZN-28 | Session unit graph: compositor binding, session and shutdown targets, service restart policies | Medium |  | implicit | `desktop/aizome/session/aizome-session.target:7` |
| AZN-29 | Post-update reload offer (path unit and a waiting oneshot) | Low |  | implicit | `desktop/aizome/session/aizome-update.path:13` |

#### [SYA: System clients: launch, audio, Bluetooth, disks, MPRIS](implicit-state-machines/sys-a.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| SYA-01 | App launch route cascade and observer event stream | Medium |  | implicit | `desktop/aizome/crates/aizome-sys/src/apps/launch.rs:181` |
| SYA-02 | systemd-run scope handshake (reader thread, timeout, 500 ms grace) | Medium |  | implicit | `desktop/aizome/crates/aizome-sys/src/apps/launch.rs:498` |
| SYA-03 | (test harness) launcher-lifetime marker files and probe wait loops | Low |  | implicit | `desktop/aizome/crates/aizome-sys/src/apps/launch.rs:977` |
| SYA-04 | PulseAudio connection and reconnect loop | Medium |  | implicit | `desktop/aizome/crates/aizome-sys/src/audio.rs:922` |
| [SYA-05](implicit-state-machines/sys-a.md#sya-05) | Optimistic audio levels awaiting server confirmation | High | [confirmed](implicit-state-machines/reproductions.md#sya-05) | implicit | `desktop/aizome/crates/aizome-sys/src/audio.rs:538` |
| [SYA-06](implicit-state-machines/sys-a.md#sya-06) | Bluetooth pairing-agent registration worker | High | [confirmed](implicit-state-machines/reproductions.md#sya-06) | implicit | `desktop/aizome/crates/aizome-sys/src/bluetooth/agent.rs:232` |
| SYA-07 | Pairing request conversation (prompt, responder, BlueZ cancel) | Medium |  | implicit | `desktop/aizome/crates/aizome-sys/src/bluetooth/agent.rs:118` |
| SYA-08 | ObjectManager follower cache (BlueZ, UDisks) | Medium | [unverified](implicit-state-machines/reproductions.md#sya-08) | implicit | `desktop/aizome/crates/aizome-sys/src/bluetooth/mod.rs:347` |
| SYA-09 | Multi-step BlueZ actions (unblock then power, pair then trust then connect) | Low |  | implicit | `desktop/aizome/crates/aizome-sys/src/bluetooth/mod.rs:424` |
| SYA-10 | Daemon follower (subscription, owner tracking, Appeared/Vanished/Idle) | Medium |  | implicit | `desktop/aizome/crates/aizome-sys/src/bus.rs:573` |
| SYA-11 | Single-object property follower with keep-on-exit (timedated, localed, hostnamed) | Medium |  | implicit | `desktop/aizome/crates/aizome-sys/src/bus.rs:757` |
| SYA-12 | Wait-until-serving ping retry loop | Low |  | implicit | `desktop/aizome/crates/aizome-sys/src/bus.rs:719` |
| [SYA-13](implicit-state-machines/sys-a.md#sya-13) | Destructive-operation confirmation token | High | [confirmed](implicit-state-machines/reproductions.md#sya-13) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) | `desktop/aizome/crates/aizome-sys/src/disks/confirm.rs:80` |
| SYA-14 | Disk image mount: attach, probe, mount, autoclear, clean up | Medium |  | implicit | `desktop/aizome/crates/aizome-sys/src/disks/mod.rs:387` |
| SYA-15 | Drive release before eject or power-off | Low |  | implicit | `desktop/aizome/crates/aizome-sys/src/disks/mod.rs:704` |
| SYA-16 | GeoClue locate request with deadline and cancellation | Low |  | implicit | `desktop/aizome/crates/aizome-sys/src/location.rs:160` |
| [SYA-17](implicit-state-machines/sys-a.md#sya-17) | Level meter thread lifecycle | High | [partly](implicit-state-machines/reproductions.md#sya-17) | implicit | `desktop/aizome/crates/aizome-sys/src/meter.rs:77` |
| SYA-18 | MPRIS player-set follower | Medium |  | implicit | `desktop/aizome/crates/aizome-sys/src/mpris/mod.rs:424` |
| [SYA-19](implicit-state-machines/sys-a.md#sya-19) | MPRIS position estimate and active-player counters | High |  | implicit | `desktop/aizome/crates/aizome-sys/src/mpris/mod.rs:99` |

#### [SYB: System clients: network, notifications, polkit, power, tray, users](implicit-state-machines/sys-b.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| [SYB-01](implicit-state-machines/sys-b.md#syb-01) | Airplane mode across rfkill, NetworkManager, BlueZ and a marker file | High | [confirmed](implicit-state-machines/reproductions.md#syb-01) | implicit | `desktop/aizome/crates/aizome-sys/src/network/mod.rs:72` |
| [SYB-02](implicit-state-machines/sys-b.md#syb-02) | Wi-Fi join / activation transaction with in-memory edit and rollback | High | [confirmed](implicit-state-machines/reproductions.md#syb-02) | implicit | `desktop/aizome/crates/aizome-sys/src/network/mod.rs:694` |
| SYB-03 | Daemon follower present/dirty coalescing (NetworkManager, logind, UPower, power profiles, AccountsService) | Medium |  | implicit | `desktop/aizome/crates/aizome-sys/src/network/mod.rs:501` |
| SYB-04 | rfkill watcher thread and radio table | Low |  | implicit | `desktop/aizome/crates/aizome-sys/src/network/rfkill.rs:132` |
| SYB-05 | Notification client: id matching, early-signal buffer, server loss | Medium |  | implicit | `desktop/aizome/crates/aizome-sys/src/notifications/client.rs:215` |
| SYB-06 | Open notification and its pausable expiry timer | Medium |  | implicit | `desktop/aizome/crates/aizome-sys/src/notifications/server.rs:120` |
| SYB-07 | Notification server lifecycle and bus-name ownership | Medium |  | implicit | `desktop/aizome/crates/aizome-sys/src/notifications/server.rs:406` |
| SYB-08 | One-value async channel | Low |  | implicit | `desktop/aizome/crates/aizome-sys/src/oneshot.rs:11` |
| [SYB-09](implicit-state-machines/sys-b.md#syb-09) | polkit authentication session and helper conversation | High | [confirmed](implicit-state-machines/reproductions.md#syb-09) | reified ([#298](https://github.com/bitemyapp/sekka/pull/298)) | `desktop/aizome/crates/aizome-sys/src/polkit.rs:195` |
| [SYB-10](implicit-state-machines/sys-b.md#syb-10) | polkit agent registration and reconnect loop | High |  | implicit | `desktop/aizome/crates/aizome-sys/src/polkit.rs:781-785` |
| SYB-11 | Backlight brightness shown before logind sets it, and the sysfs watcher | Medium |  | implicit | `desktop/aizome/crates/aizome-sys/src/power/backlight.rs:123` |
| [SYB-12](implicit-state-machines/sys-b.md#syb-12) | logind sleep-delay inhibitor and lock-before-sleep handshake | High | [confirmed](implicit-state-machines/reproductions.md#syb-12) | reified ([#298](https://github.com/bitemyapp/sekka/pull/298)) | `desktop/aizome/crates/aizome-sys/src/power/logind.rs:143` |
| SYB-13 | Background Task result slot | Low |  | implicit | `desktop/aizome/crates/aizome-sys/src/service.rs:45` |
| SYB-14 | Service model generation and the Stop latch | Medium |  | implicit | `desktop/aizome/crates/aizome-sys/src/service.rs:176` |
| SYB-15 | StatusNotifierWatcher service lifecycle and name ownership | Medium |  | implicit | `desktop/aizome/crates/aizome-sys/src/statusnotifier/mod.rs:162` |
| SYB-16 | Status item registry: register, re-read, gone | Medium |  | implicit | `desktop/aizome/crates/aizome-sys/src/statusnotifier/mod.rs:93` |

#### [SYC: System clients: printers and sysinfo; config, theme and face](implicit-state-machines/sys-c-config.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| [SYC-01](implicit-state-machines/sys-c-config.md#syc-01) | Keyboard settings: inherited session defaults vs saved choices | High | [confirmed](implicit-state-machines/reproductions.md#syc-01) | implicit | `desktop/aizome/crates/aizome-config/src/lib.rs:304` |
| [SYC-02](implicit-state-machines/sys-c-config.md#syc-02) | One-shot migration of the legacy `[keys] terminal` key | High | [confirmed](implicit-state-machines/reproductions.md#syc-02) | implicit | `desktop/aizome/crates/aizome-config/src/lib.rs:523` |
| [SYC-03](implicit-state-machines/sys-c-config.md#syc-03) | Settings file lifecycle: load, fallback to defaults, atomic save, cross-process writers | High | [confirmed](implicit-state-machines/reproductions.md#syc-03) | implicit | `desktop/aizome/crates/aizome-config/src/lib.rs:686` |
| SYC-04 | Settings file watcher thread | Low |  | implicit | `desktop/aizome/crates/aizome-config/src/lib.rs:813` |
| SYC-05 | Persisted GeoClue location and its time-zone validity | Low |  | implicit | `desktop/aizome/crates/aizome-config/src/location.rs:25` |
| [SYC-06](implicit-state-machines/sys-c-config.md#syc-06) | Drawn element press, drag, drag-out and hover gesture | High | [partly](implicit-state-machines/reproductions.md#syc-06) | implicit | `desktop/aizome/crates/aizome-face/src/widgets/drawn.rs:268` |
| [SYC-07](implicit-state-machines/sys-c-config.md#syc-07) | Text field caret, selection, IME preedit, blink and Caps Lock state | High | [confirmed](implicit-state-machines/reproductions.md#syc-07) | reified ([#298](https://github.com/bitemyapp/sekka/pull/298)) | `desktop/aizome/crates/aizome-face/src/widgets/field.rs:99` |
| SYC-08 | (test harness) MPRIS test player playback state and pw-play child | Low |  | implicit | `desktop/aizome/crates/aizome-sys/examples/test_player.rs:127` |
| SYC-09 | CUPS tool subprocess lifecycle in `run` | Medium |  | implicit | `desktop/aizome/crates/aizome-sys/src/printers/cli.rs:239` |
| SYC-10 | User's default printer in `~/.cups/lpoptions` | Low |  | implicit | `desktop/aizome/crates/aizome-sys/src/printers/cli.rs:724` |
| SYC-11 | Printers snapshot, poller and refresh turns | Medium |  | implicit | `desktop/aizome/crates/aizome-sys/src/printers/mod.rs:74` |
| SYC-12 | Streaming lpinfo driver parser and lpstat record parsers | Low |  | implicit | `desktop/aizome/crates/aizome-sys/src/printers/parse.rs:508` |
| SYC-13 | (test harness) Fake CUPS tool scripts with mutable canned cases | Low |  | implicit | `desktop/aizome/crates/aizome-sys/src/printers/tests.rs:14` |
| SYC-14 | CPU usage delta sampler | Low |  | implicit | `desktop/aizome/crates/aizome-sys/src/sysinfo/cpu.rs:163` |
| SYC-15 | Disk and network throughput samplers | Low |  | implicit | `desktop/aizome/crates/aizome-sys/src/sysinfo/io.rs:136` |
| SYC-16 | Power meter idle floor and RAPL readings | Low |  | implicit | `desktop/aizome/crates/aizome-sys/src/sysinfo/power.rs:188` |
| SYC-17 | Process sampler: per-pid cache validity and CPU interval | Medium |  | implicit | `desktop/aizome/crates/aizome-sys/src/sysinfo/process.rs:507` |
| SYC-18 | (test harness) Private PipeWire and pipewire-pulse | Low |  | implicit | `desktop/aizome/crates/aizome-sys/tests/audio.rs:13` |
| SYC-19 | (test harness) Fake BlueZ: agent registration, discovery and pairing | Low |  | implicit | `desktop/aizome/crates/aizome-sys/tests/bluetooth.rs:18` |
| SYC-20 | (test harness) Private dbus-daemon per test and the `eventually` wait | Low |  | implicit | `desktop/aizome/crates/aizome-sys/tests/common/mod.rs:14` |
| SYC-21 | (test harness) Fake UDisks2 object lifecycles with fault injection | Low |  | implicit | `desktop/aizome/crates/aizome-sys/tests/disks.rs:44` |
| SYC-22 | (test harness) Real UDisks loop-device lifetime driver (VM only) | Low |  | implicit | `desktop/aizome/crates/aizome-sys/tests/disks_real.rs:10` |
| SYC-23 | (test harness) Fake GeoClue2 client with call gates and stall injection | Low |  | implicit | `desktop/aizome/crates/aizome-sys/tests/location.rs:132` |
| SYC-24 | (test harness) Fake MPRIS players | Low |  | implicit | `desktop/aizome/crates/aizome-sys/tests/mpris.rs:78` |
| SYC-25 | (test harness) Fake NetworkManager activation lifecycle | Low |  | implicit | `desktop/aizome/crates/aizome-sys/tests/network.rs:43` |
| SYC-26 | (test harness) Fake polkitd authority, PAM helper and scripted answering UI | Low |  | implicit | `desktop/aizome/crates/aizome-sys/tests/polkit.rs:19` |
| SYC-27 | (test harness) Fake logind inhibitors, sleep signals and backlight; fake profile holds | Low |  | implicit | `desktop/aizome/crates/aizome-sys/tests/power.rs:272` |
| SYC-28 | (test harness) Fake AccountsService user creation and deletion | Low |  | implicit | `desktop/aizome/crates/aizome-sys/tests/users.rs:22` |

#### [KIT: Toolkit runtime](implicit-state-machines/kit-core.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| KIT-01 | (test harness) Gallery demo and benchmark driver | Low |  | implicit | `desktop/aizome/crates/aizome-kit/examples/gallery/views.rs:53` |
| KIT-02 | Draw-ahead of hidden windows (band drawing) | Medium |  | implicit | `desktop/aizome/crates/aizome-kit/src/ahead.rs:40` |
| KIT-03 | Kept picture for a shown window's first frame | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/app.rs:392` |
| [KIT-04](implicit-state-machines/kit-core.md#kit-04) | Menu publication to the shell (dedup and debounce) | High | [confirmed](implicit-state-machines/reproductions.md#kit-04) | implicit | `desktop/aizome/crates/aizome-kit/src/app.rs:62` |
| KIT-05 | Sidestep handle maps and request ids | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/app.rs:378` |
| KIT-06 | Alert sheet lifecycle | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/dialogs.rs:103` |
| [KIT-07](implicit-state-machines/kit-core.md#kit-07) | File panel lifecycle and browsing state | High | [confirmed](implicit-state-machines/reproductions.md#kit-07) | implicit | `desktop/aizome/crates/aizome-kit/src/dialogs.rs:298` |
| KIT-08 | Editor undo history with typing bursts | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/editor.rs:117` |
| KIT-09 | Window and sheet key registry | Medium |  | implicit | `desktop/aizome/crates/aizome-kit/src/engine.rs:288` |
| KIT-10 | Window view, build and relayout lifecycle | Medium |  | implicit | `desktop/aizome/crates/aizome-kit/src/engine.rs:150` |
| KIT-11 | Window hide/show with held ticks, timers and focus | Medium |  | implicit | `desktop/aizome/crates/aizome-kit/src/engine.rs:172` |
| KIT-12 | Engine timer registry (ticks, widget timers, app timers) | Medium |  | implicit | `desktop/aizome/crates/aizome-kit/src/engine.rs:290` |
| [KIT-13](implicit-state-machines/kit-core.md#kit-13) | Pointer hover, press and capture | High | [confirmed](implicit-state-machines/reproductions.md#kit-13) | implicit | `desktop/aizome/crates/aizome-kit/src/engine.rs:132` |
| [KIT-14](implicit-state-machines/kit-core.md#kit-14) | Keyboard focus across modal layers, with pending focus and IME | High |  | implicit | `desktop/aizome/crates/aizome-kit/src/engine.rs:143` |
| KIT-15 | Menu popup chain (menus, submenus, in-window bar menu) | Medium |  | implicit | `desktop/aizome/crates/aizome-kit/src/engine.rs:164` |
| KIT-16 | Popover popup reconciliation | Medium |  | implicit | `desktop/aizome/crates/aizome-kit/src/engine.rs:116` |
| [KIT-17](implicit-state-machines/kit-core.md#kit-17) | Tooltip delay, show and dismissal | High | [confirmed](implicit-state-machines/reproductions.md#kit-17) | implicit | `desktop/aizome/crates/aizome-kit/src/engine.rs:157` |
| KIT-18 | Video placement and opaque-region mirror | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/engine.rs:187` |
| KIT-19 | Session lock windows | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/engine.rs:635` |
| KIT-20 | Inspection files per window | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/inspect.rs:34` |
| [KIT-21](implicit-state-machines/kit-core.md#kit-21) | Shell connection: link, watcher and menu placement | High |  | implicit | `desktop/aizome/crates/aizome-kit/src/shell.rs:28` |
| KIT-22 | (test harness) TestPlatform virtual clock and timer queue | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/testing.rs:54` |
| KIT-23 | (test harness) TestPlatform visibility, picture and damage model | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/testing.rs:93` |
| KIT-24 | (test harness) Fake session lock | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/testing.rs:74` |
| KIT-25 | (test harness) Retained frame oracles | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/testing.rs:88` |
| KIT-26 | (test harness) Wall-clock polling loops | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/testing.rs:531` |
| KIT-27 | (test harness) Fake clipboard and primary selection | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/testing.rs:58` |
| KIT-28 | Appearance following and retheming | Medium |  | implicit | `desktop/aizome/crates/aizome-kit/src/theme.rs:341` |
| [KIT-29](implicit-state-machines/kit-core.md#kit-29) | Node arena: liveness, layout cache and per-layout outboxes | High | [confirmed](implicit-state-machines/reproductions.md#kit-29) | implicit | `desktop/aizome/crates/aizome-kit/src/tree.rs:60` |
| KIT-30 | Desktop opener worker (bounded xdg-open) | Medium |  | implicit | `desktop/aizome/crates/aizome-kit/src/urls.rs:76` |
| KIT-31 | Per-node widget state across rebuilds | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/widget.rs:417` |
| KIT-32 | (test harness) Menu-bar fixture's keyboard title walk | Low |  | implicit | `desktop/aizome/crates/aizome-kit/tests/menu_hooks.rs:15` |
| KIT-33 | (test harness) Fake shell socket server | Low |  | implicit | `desktop/aizome/crates/aizome-kit/tests/shell_link.rs:83` |

#### [UIW: Sukumo UI and toolkit widgets](implicit-state-machines/sukumo-ui-kit-widgets.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| [UIW-01](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-01) | Press-then-release click pairing in buttons, switches, checkboxes, radios and segmented controls | High | [confirmed](implicit-state-machines/reproductions.md#uiw-01) | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/controls.rs:421` |
| UIW-02 | Slider drag and commit | Medium |  | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/controls.rs:971` |
| UIW-03 | Hovered part of steppers and segmented controls | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/controls.rs:1224` |
| UIW-04 | Indeterminate progress sweep | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/controls.rs:1894` |
| UIW-05 | Image scaled on its second sighting | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/controls.rs:2243` |
| UIW-06 | Mouse area press, drag, drag-out and hover gesture | Medium |  | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/interact.rs:294` |
| UIW-07 | Previous-layout snapshots for partial redraw | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/interact.rs:614` |
| [UIW-08](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-08) | Menu interaction: hover, open submenu, pointer aim, type-to-filter, strip tear-off | High | [confirmed](implicit-state-machines/reproductions.md#uiw-08) | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/menus.rs:268` |
| [UIW-09](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-09) | In-window menu bar's open title | High | [confirmed](implicit-state-machines/reproductions.md#uiw-09) | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/menus.rs:1060` |
| UIW-10 | Split divider drag | Medium |  | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/panes.rs:130` |
| [UIW-11](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-11) | Scroll-bar visibility and thumb drag | High | [confirmed](implicit-state-machines/reproductions.md#uiw-11) | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/scroll.rs:52` |
| UIW-12 | Scroll offset, reveal latch and laid-out offset | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/scroll.rs:53` |
| UIW-13 | Row press that may become a drag out | Medium |  | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/scroll.rs:515` |
| UIW-14 | Table column resize | Medium |  | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/table.rs:333` |
| [UIW-15](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-15) | Selection cursor and anchor in lists, tables and outlines | High | [confirmed](implicit-state-machines/reproductions.md#uiw-15) | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/table.rs:37` |
| UIW-16 | Type-select prefix buffer | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/scroll.rs:701` |
| UIW-17 | Outline's own open set | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/table.rs:847` |
| [UIW-18](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-18) | Editor viewport, position-bar linger and caret blink on one tick | High | [confirmed](implicit-state-machines/reproductions.md#uiw-18) | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/text_editor.rs:281` |
| UIW-19 | Editor pointer selection and dragging selected text out | Medium |  | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/text_editor.rs:292` |
| UIW-20 | Editor caret reveal and goal column | Medium |  | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/text_editor.rs:298` |
| UIW-21 | Input-method composition in editors and text fields | Medium |  | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/text_editor.rs:286` |
| UIW-22 | Text field focus, caret blink and first-focus selection | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/text_field.rs:195` |
| UIW-23 | Text field pointer selection and dragging selected text out | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/text_field.rs:190` |
| UIW-24 | Text field undo history, typing groups and optimistic caret | Medium |  | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/text_field.rs:196` |
| UIW-25 | Video above or below the window's drawing | Low |  | implicit | `desktop/aizome/crates/aizome-kit/src/widgets/video.rs:38` |
| UIW-26 | Application run lifecycle and loop wake-up | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:650` |
| [UIW-27](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-27) | Session lock | High | [confirmed](implicit-state-machines/reproductions.md#uiw-27) | reified ([#298](https://github.com/bitemyapp/sekka/pull/298)) | `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:548` |
| UIW-28 | Touchpad coasting | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:787` |
| UIW-29 | Drag-and-drop offers over windows | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:794` |
| UIW-30 | Clipboard and primary-selection ownership | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-ui/src/app.rs:791` |
| [UIW-31](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-31) | Window showing and frame-pacing lifecycle | High |  | implicit | `desktop/aizome/sukumo/crates/sidestep-ui/src/window.rs:354` |
| UIW-32 | (test harness) Step-polled scenario drivers on the null render thread | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-ui/tests/desktop.rs:66` |
| UIW-33 | (test harness) Virtual pointer driver | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-ui/tests/support/pointer.rs:16` |
| UIW-34 | (test harness) Headless-sway scenario drivers | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-ui/tests/wayland_desktop.rs:555` |

#### [SKB: Sukumo engine: Wayland client backend](implicit-state-machines/sukumo-backend.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| SKB-01 | Client-side decoration presence and redraw cache | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:195` |
| SKB-02 | Header button hover and press tracking | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:211` |
| [SKB-03](implicit-state-machines/sukumo-backend.md#skb-03) | Incoming drag-and-drop session | High |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/dnd.rs:61` |
| SKB-04 | Outgoing drag source lifecycle | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/drag_out.rs:40` |
| SKB-05 | xdg-foreign parent import link | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:493` |
| SKB-06 | Compose (dead-key) sequence | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/keyboard.rs:72` |
| SKB-07 | Layer surface configure and close | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/layer.rs:55` |
| SKB-08 | Session lock lifecycle | Medium | [unverified](implicit-state-machines/reproductions.md#skb-08) | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:358` |
| SKB-09 | In-window menu bar presence and redraw | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/menubar.rs:28` |
| SKB-10 | Render thread lifecycle | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:174` |
| [SKB-11](implicit-state-machines/sukumo-backend.md#skb-11) | Window configure, size and geometry lifecycle | High | [confirmed](implicit-state-machines/reproductions.md#skb-11) | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:501` |
| SKB-12 | Window scale source | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:497` |
| [SKB-13](implicit-state-machines/sukumo-backend.md#skb-13) | Frame pacing and layer-tree animation gating | High |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:514` |
| SKB-14 | Shared-memory buffer reuse and staleness (window and tile buffers) | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:504` |
| SKB-15 | xdg_popup lifecycle and grab serial | Medium | [unverified](implicit-state-machines/reproductions.md#skb-15) | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1363` |
| SKB-16 | Activation tokens | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:380` |
| SKB-17 | Pointer passthrough and input-region application | Medium | [unverified](implicit-state-machines/reproductions.md#skb-17) | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:544` |
| SKB-18 | (test harness) Null render thread as a fake compositor | Medium | [unverified](implicit-state-machines/reproductions.md#skb-18) | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/null.rs:318` |
| SKB-19 | Output discovery, settle and learned scales | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/outputs.rs:34` |
| SKB-20 | Keyboard focus source, seat devices and first mouse | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/seat.rs:79` |
| SKB-21 | Pointer focus and content crossings | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/seat.rs:89` |
| SKB-22 | Click counting | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/seat.rs:132` |
| SKB-23 | Touchpad scroll gesture and axis frame | Medium | [unverified](implicit-state-machines/reproductions.md#skb-23) | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/seat.rs:104` |
| SKB-24 | Pinch gesture | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/seat.rs:107` |
| SKB-25 | Cursor visibility and shown cache | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/seat.rs:68` |
| SKB-26 | Key repeat timer | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/seat.rs:153` |
| [SKB-27](implicit-state-machines/sukumo-backend.md#skb-27) | Selection and primary-selection ownership | High | [not confirmed](implicit-state-machines/reproductions.md#skb-27) | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/selection.rs:64` |
| SKB-28 | Promised clipboard and drag data tokens | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/selection.rs:71` |
| SKB-29 | Bounded pipe transfers | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/selection.rs:295` |
| SKB-30 | Sheet placement and parent commit gating | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/sheet.rs:30` |
| SKB-31 | text-input-v3 focus, enable and event batching | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/textinput.rs:27` |
| SKB-32 | Scroll-layer tile lifecycle and raster queue | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/tiles.rs:84` |
| SKB-33 | Video subsurface placement sync | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:101` |
| SKB-34 | Video compositor buffer cache and frame release | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:85` |
| SKB-35 | Video capability discovery handshake | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/video.rs:77` |
| [SKB-36](implicit-state-machines/sukumo-backend.md#skb-36) | Foreign selection text, change count and identity | High |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:291` |
| SKB-37 | Clipboard read requests by token | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/clipboard.rs:230` |
| SKB-38 | Settings portal D-Bus session | Medium | [unverified](implicit-state-machines/reproductions.md#skb-38) | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/desktop.rs:155` |
| SKB-39 | Momentum coast phases | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/momentum.rs:25` |
| SKB-40 | Published outputs first-snapshot handshake | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/outputs.rs:48` |
| SKB-41 | Desktop appearance knowledge and change notification | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/settings.rs:32` |
| SKB-42 | Frame statistics collection and record/present pairing | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/stats.rs:30` |

#### [SKR: Sukumo engine: animation, text and raster](implicit-state-machines/sukumo-render.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| SKR-01 | Render-thread layer tree linkage (parent links and forward references) | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/ca/tree.rs:116` |
| SKR-02 | Transition snapshot lifecycle | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/ca/tree.rs:122` |
| [SKR-03](implicit-state-machines/sukumo-render.md#skr-03) | Retained paints and per-host frame scheduling (dirty / moving / last snapshot) | High | [confirmed](implicit-state-machines/reproductions.md#skr-03) | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/ca/tree.rs:137` |
| [SKR-04](implicit-state-machines/sukumo-render.md#skr-04) | Layer contents, canvas generations and their image-cache keys | High | [confirmed](implicit-state-machines/reproductions.md#skr-04) | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/ca/tree.rs:119` |
| SKR-05 | Animated GIF/WebP frame decoder cursor | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/codec.rs:242` |
| SKR-06 | Accent-keyed system palette cache | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/palette.rs:312` |
| SKR-07 | CoreGraphics path builder current point and derived caches | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/path.rs:45` |
| SKR-08 | Raster worker pool and scoped band latch | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/raster/bands.rs:29` |
| SKR-09 | Per-thread image texture cache (budget, LRU, generations, halvings, tints) | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/raster/images.rs:96` |
| SKR-10 | Op-stream group nesting while rasterizing | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/raster/ops.rs:108` |
| SKR-11 | Font collection open/prewarm and the squashed-name cache | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/text/fonts.rs:474` |
| SKR-12 | Font-file face registry (private families, by blob, by content, by name) | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/text/fonts.rs:109` |
| SKR-13 | Terminal grid `FontSet` (primary face, bounded faces, resolution and layout caches) | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/text/grid.rs:187` |
| SKR-14 | Two-generation text caches (layouts, glyph strikes, shared glyph images) | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/text/layout.rs:748` |
| SKR-15 | Incremental TextKit frame layout after edits | Medium |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/text/lines.rs:804` |
| SKR-16 | Text layout worker pool batches | Low |  | implicit | `desktop/aizome/sukumo/crates/sidestep-engine/src/text/pool.rs:23` |
| SKR-17 | (test harness) Headless sway runner lifecycle | Low | [unverified](implicit-state-machines/reproductions.md#skr-17) | implicit | `desktop/aizome/sukumo/scripts/headless-wayland:39` |

#### [STA: Settei: general, keyboard, network, sound and other panes](implicit-state-machines/settei-a.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| STA-01 | Pane background loads (Option as the loading spinner, re-read on every show) | Medium |  | implicit | `desktop/aizome/apps/settei/src/panes/general/about.rs:33` |
| STA-02 | Inline name drafts committed on Return or blur (computer name, host name, your name) | Low |  | implicit | `desktop/aizome/apps/settei/src/panes/general/about.rs:35` |
| STA-03 | Confirm-then-act alerts (the target carried in the confirm message) | Low |  | implicit | `desktop/aizome/apps/settei/src/panes/general/about.rs:317` |
| STA-04 | Visible-pane one-second tick timers (Date & Time clock, Performance poll) | Low |  | implicit | `desktop/aizome/apps/settei/src/panes/general/datetime.rs:50` |
| [STA-05](implicit-state-machines/settei-a.md#sta-05) | Language & Region formats preview with a stale-locale guard | High | [not confirmed](implicit-state-machines/reproductions.md#sta-05) | implicit | `desktop/aizome/apps/settei/src/panes/general/language.rs:52` |
| STA-06 | (test harness) Settle and advance loops in Settei's pane tests | Low |  | implicit | `desktop/aizome/apps/settei/src/panes/general/language.rs:470` |
| STA-07 | Boot splash pending-restart phase (running system vs next start) | Low |  | implicit | `desktop/aizome/apps/settei/src/panes/general/startup.rs:29` |
| STA-08 | Busy spinner animation clock | Low |  | implicit | `desktop/aizome/apps/settei/src/panes/hw.rs:31` |
| STA-09 | (test harness) Live-snapshot sample capture | Low |  | implicit | `desktop/aizome/apps/settei/src/panes/hw.rs:222` |
| [STA-10](implicit-state-machines/settei-a.md#sta-10) | Keyboard setup: catching the key beside the space bar | High | [confirmed](implicit-state-machines/reproductions.md#sta-10) | implicit | `desktop/aizome/apps/settei/src/panes/keyboard/mod.rs:118` |
| STA-11 | Shortcut recording mode and the recorder's held modifiers | Low |  | implicit | `desktop/aizome/apps/settei/src/panes/keyboard/mod.rs:110` |
| [STA-12](implicit-state-machines/settei-a.md#sta-12) | Pane sheets against the window's single sheet slot (keyboard's three, network, printers, users) | High | [confirmed](implicit-state-machines/reproductions.md#sta-12) | implicit | `desktop/aizome/apps/settei/src/panes/keyboard/mod.rs:108` |
| [STA-13](implicit-state-machines/settei-a.md#sta-13) | Wi-Fi join: one at a time, the sheet reopened as it was on failure | High |  | implicit | `desktop/aizome/apps/settei/src/panes/network/mod.rs:433` |
| STA-14 | Wi-Fi scan cycle | Low |  | implicit | `desktop/aizome/apps/settei/src/panes/network/mod.rs:442` |
| [STA-15](implicit-state-machines/settei-a.md#sta-15) | Connection details page: navigation, profile reads and the edit buffer | High | [confirmed](implicit-state-machines/reproductions.md#sta-15) | implicit | `desktop/aizome/apps/settei/src/panes/network/mod.rs:448` |
| STA-16 | Per-item in-flight sets (network connections, printers) | Low |  | implicit | `desktop/aizome/apps/settei/src/panes/network/mod.rs:441` |
| STA-17 | (test harness) Sample services emulating NetworkManager, CUPS and PipeWire | Low |  | implicit | `desktop/aizome/apps/settei/src/panes/network/sample.rs:205` |
| STA-18 | CUPS client lifetime tied to the pane's visibility and the add sheet | Medium |  | implicit | `desktop/aizome/apps/settei/src/panes/printers.rs:142` |
| [STA-19](implicit-state-machines/settei-a.md#sta-19) | Add-a-printer sheet: discovery, driver lookup, adding | High | [confirmed](implicit-state-machines/reproductions.md#sta-19) | implicit | `desktop/aizome/apps/settei/src/panes/printers.rs:145` |
| STA-20 | Palette file index and history reset by removing the shell's files | Low |  | implicit | `desktop/aizome/apps/settei/src/panes/search.rs:28` |
| STA-21 | Input level meter following the default input | Medium |  | implicit | `desktop/aizome/apps/settei/src/panes/sound.rs:86` |
| [STA-22](implicit-state-machines/settei-a.md#sta-22) | New account: its password applied once AccountsService lists it | High | [confirmed](implicit-state-machines/reproductions.md#sta-22) | implicit | `desktop/aizome/apps/settei/src/panes/users.rs:49` |

#### [STB: Settei: app shell, appearance, Bluetooth and displays](implicit-state-machines/settei-b.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| STB-01 | Pane navigation, history and the shown/hidden pane lifecycle | Medium | [unverified](implicit-state-machines/reproductions.md#stb-01) | implicit | `desktop/aizome/apps/settei/src/app.rs:100` |
| STB-02 | Sheet window ownership, split between the window and the panes | Medium | [unverified](implicit-state-machines/reproductions.md#stb-02) | implicit | `desktop/aizome/apps/settei/src/app.rs:108` |
| STB-03 | Generation-tagged transient highlights: the revealed option and the status line | Low |  | implicit | `desktop/aizome/apps/settei/src/app.rs:106` |
| STB-04 | Single-instance socket: claim, hand-off, listen, release | Medium | [unverified](implicit-state-machines/reproductions.md#stb-04) | implicit | `desktop/aizome/apps/settei/src/instance.rs:20` |
| STB-05 | Remembered theme and size before High Contrast and Larger text (persisted) | Low | [unverified](implicit-state-machines/reproductions.md#stb-05) | implicit | `desktop/aizome/apps/settei/src/panes/accessibility.rs:193` |
| STB-06 | Lazy pane data loads keyed on an empty Option (wallpapers, apps to pin) | Low |  | implicit | `desktop/aizome/apps/settei/src/panes/appearance.rs:26` |
| STB-07 | Battery pane services, charge history and logind read | Low |  | implicit | `desktop/aizome/apps/settei/src/panes/battery.rs:49` |
| STB-08 | Keep Awake switch: optimistic value, helper process, marker file | Medium | [unverified](implicit-state-machines/reproductions.md#stb-08) | implicit | `desktop/aizome/apps/settei/src/panes/battery.rs:54` |
| [STB-09](implicit-state-machines/settei-b.md#stb-09) | Bluetooth pane activity: pairing agent, discovery and discoverability while shown | High | [confirmed](implicit-state-machines/reproductions.md#stb-09) | implicit | `desktop/aizome/apps/settei/src/panes/bluetooth.rs:259` |
| [STB-10](implicit-state-machines/settei-b.md#stb-10) | Pairing questions: prompt, responders and per-device busy | High |  | implicit | `desktop/aizome/apps/settei/src/panes/bluetooth.rs:254` |
| STB-11 | Typed drafts committed on Return or blur | Low |  | implicit | `desktop/aizome/apps/settei/src/panes/bluetooth.rs:249` |
| STB-12 | Arrangement drag gesture | Medium | [unverified](implicit-state-machines/reproductions.md#stb-12) | implicit | `desktop/aizome/apps/settei/src/panes/displays/arrangement.rs:143` |
| [STB-13](implicit-state-machines/settei-b.md#stb-13) | Display change: apply, "Keep these display settings?" countdown, keep or revert | High | [partly](implicit-state-machines/reproductions.md#stb-13) | implicit | `desktop/aizome/apps/settei/src/panes/displays/mod.rs:110` |
| STB-14 | Displays read and watch, and Night Light state | Medium |  | implicit | `desktop/aizome/apps/settei/src/panes/displays/mod.rs:115` |
| STB-15 | Backlight brightness request coalescing | Medium |  | implicit | `desktop/aizome/apps/settei/src/panes/displays/mod.rs:60` |
| STB-16 | What a display is used as (main, extended, mirror, off): fields and `[[outputs]]` keys | Medium |  | implicit | `desktop/aizome/apps/settei/src/panes/displays/model.rs:147` |
| [STB-17](implicit-state-machines/settei-b.md#stb-17) | settings.toml write pipeline: ordered writer, pending count, reload debounce | High | [confirmed](implicit-state-machines/reproductions.md#stb-17) | implicit | `desktop/aizome/apps/settei/src/shared.rs:37` |
| STB-18 | Picture cache validated by file mtime | Low |  | implicit | `desktop/aizome/apps/settei/src/widgets.rs:854` |
| STB-19 | (test harness) settle loops that pump the window until background work lands | Low |  | implicit | `desktop/aizome/apps/settei/tests/budget_hw.rs:14` |
| STB-20 | (test harness) live headless-compositor scenario for Displays | Low |  | implicit | `desktop/aizome/apps/settei/tests/displays_live.rs:59` |

#### [HAKO: Hako file manager](implicit-state-machines/hako.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| HAKO-01 | FileManager1 show-in-folder service bus-name lifecycle | Low |  | implicit | `desktop/aizome/apps/hako/src/file_manager.rs:113` |
| HAKO-02 | bsdtar child process run (compress/extract) with pause, cancel and partial output | Medium |  | implicit | `desktop/aizome/apps/hako/src/fsops/archive.rs:275` |
| HAKO-03 | Operation pause/cancel control | Low |  | implicit | `desktop/aizome/apps/hako/src/fsops/control.rs:22` |
| HAKO-04 | In-kernel copy with buffered fallback | Low |  | implicit | `desktop/aizome/apps/hako/src/fsops/sys.rs:63` |
| [HAKO-05](implicit-state-machines/hako.md#hako-05) | Transfer placement with conflict prompt and replace-aside protocol | High | [confirmed](implicit-state-machines/reproductions.md#hako-05) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) | `desktop/aizome/apps/hako/src/fsops/transfer.rs:355` |
| [HAKO-06](implicit-state-machines/hako.md#hako-06) | Copy-to-partial commit and cross-filesystem move (copy, fsync, delete arrived sources) | High | [confirmed](implicit-state-machines/reproductions.md#hako-06) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) | `desktop/aizome/apps/hako/src/fsops/transfer.rs:481` |
| HAKO-07 | Ask-and-retry loops of the file operations | Low |  | implicit | `desktop/aizome/apps/hako/src/fsops/transfer.rs:342` |
| [HAKO-08](implicit-state-machines/hako.md#hako-08) | Trash can item protocol (trashinfo claim, rename, directorysizes, forget) | High | [confirmed](implicit-state-machines/reproductions.md#hako-08) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) | `desktop/aizome/apps/hako/src/fsops/trash.rs:298` |
| [HAKO-09](implicit-state-machines/hako.md#hako-09) | Undo/redo history with in-flight inversion jobs | High | [confirmed](implicit-state-machines/reproductions.md#hako-09) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) | `desktop/aizome/apps/hako/src/fsops/undo.rs:67` |
| HAKO-10 | Thumbnail disk cache validity and failure marks | Low |  | implicit | `desktop/aizome/apps/hako/src/model/thumbs.rs:261` |
| [HAKO-11](implicit-state-machines/hako.md#hako-11) | inotify folder watcher and its watch-descriptor map | High | [confirmed](implicit-state-machines/reproductions.md#hako-11) | implicit | `desktop/aizome/apps/hako/src/model/watch.rs:27` |
| HAKO-12 | Desks follower thread (compositor IPC) | Low |  | implicit | `desktop/aizome/apps/hako/src/ui/actions.rs:642` |
| HAKO-13 | Sheet registry over windows (Info, Quick Look, Ask, Confirm, Message, Unlock) | Medium |  | implicit | `desktop/aizome/apps/hako/src/ui/app.rs:276` |
| HAKO-14 | File clipboard (cut/copy ownership and cut fading) | Medium |  | implicit | `desktop/aizome/apps/hako/src/ui/app.rs:281` |
| HAKO-15 | Operations window presence | Low |  | implicit | `desktop/aizome/apps/hako/src/ui/app.rs:279` |
| [HAKO-16](implicit-state-machines/hako.md#hako-16) | Volume mount, unlock and two-phase eject tasks | High | [confirmed](implicit-state-machines/reproductions.md#hako-16) | implicit | `desktop/aizome/apps/hako/src/ui/app.rs:287` |
| [HAKO-17](implicit-state-machines/hako.md#hako-17) | Thumbnail request pipeline (queue, asked set, running count) | High | [confirmed](implicit-state-machines/reproductions.md#hako-17) | implicit | `desktop/aizome/apps/hako/src/ui/app.rs:294` |
| HAKO-18 | Background result caches (previews, folder sizes, free space, Trash count) | Low |  | implicit | `desktop/aizome/apps/hako/src/ui/app.rs:297` |
| [HAKO-19](implicit-state-machines/hako.md#hako-19) | Column listing load, reload debounce and refresh reconciliation | High | [confirmed](implicit-state-machines/reproductions.md#hako-19) | implicit | `desktop/aizome/apps/hako/src/ui/browser.rs:37` |
| [HAKO-20](implicit-state-machines/hako.md#hako-20) | Rename session (inline field, new-folder auto-rename) | High | [confirmed](implicit-state-machines/reproductions.md#hako-20) | implicit | `desktop/aizome/apps/hako/src/ui/browser.rs:165` |
| HAKO-21 | Search session (generation, running flag, index cache, worker threads) | Medium |  | implicit | `desktop/aizome/apps/hako/src/ui/browser.rs:135` |
| HAKO-22 | Drag session (in-window drag, spring-loading, drag-out and its return) | Medium |  | implicit | `desktop/aizome/apps/hako/src/ui/browser.rs:147` |
| HAKO-23 | Per-window navigation history (Back/Forward) | Low |  | implicit | `desktop/aizome/apps/hako/src/ui/browser.rs:163` |
| HAKO-24 | Location bar edit mode | Low |  | implicit | `desktop/aizome/apps/hako/src/ui/browser.rs:166` |
| [HAKO-25](implicit-state-machines/hako.md#hako-25) | Drop-target spot registry | High | [confirmed](implicit-state-machines/reproductions.md#hako-25) | implicit | `desktop/aizome/apps/hako/src/ui/dnd.rs:56` |
| HAKO-26 | File view gesture and keyboard state | Medium |  | implicit | `desktop/aizome/apps/hako/src/ui/fileview.rs:289` |
| HAKO-27 | Single-instance claim and hand-over socket | Low |  | implicit | `desktop/aizome/apps/hako/src/ui/instance.rs:102` |
| [HAKO-28](implicit-state-machines/hako.md#hako-28) | Job lifecycle with blocking questions | High | [confirmed](implicit-state-machines/reproductions.md#hako-28) | implicit | `desktop/aizome/apps/hako/src/ui/jobs.rs:128` |
| HAKO-29 | Column strip follow-scroll and divider resize | Low |  | implicit | `desktop/aizome/apps/hako/src/ui/strip.rs:40` |
| HAKO-30 | (test harness) settle and wait loops for the headless window | Low |  | implicit | `desktop/aizome/apps/hako/tests/common/mod.rs:123` |
| HAKO-31 | (test harness) private D-Bus daemon and FileManager1 service driver | Low |  | implicit | `desktop/aizome/apps/hako/tests/file_manager.rs:64` |

#### [HOT: Hotaru's Aizome frontend and link matcher](implicit-state-machines/hotaru-app.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| HOT-01 | (test harness) OSC 7 latency benchmark driver | Low |  | implicit | `desktop/aizome/apps/hotaru/examples/cwd_latency.rs:22` |
| [HOT-02](implicit-state-machines/hotaru-app.md#hot-02) | Pane lifecycle: live, ended, failed, removed, window teardown | High |  | implicit | `desktop/aizome/apps/hotaru/src/app.rs:136` |
| HOT-03 | Tab, pane focus, zoom and recency model | Medium |  | implicit | `desktop/aizome/apps/hotaru/src/app.rs:176` |
| HOT-04 | Focus-restore timer generation | Low |  | implicit | `desktop/aizome/apps/hotaru/src/app.rs:209` |
| HOT-05 | Pane directory provenance and title derivation | Medium |  | implicit | `desktop/aizome/apps/hotaru/src/app.rs:132` |
| HOT-06 | Pane font-shaping epoch and scale acceptance | Medium |  | implicit | `desktop/aizome/apps/hotaru/src/app.rs:146` |
| [HOT-07](implicit-state-machines/hotaru-app.md#hot-07) | Automatic-link activation pipeline (match, hover, resolve, open) | High | [confirmed](implicit-state-machines/reproductions.md#hot-07) | implicit | `desktop/aizome/apps/hotaru/src/app.rs:211` |
| HOT-08 | User copy/paste request ordering and paste notice | Medium |  | implicit | `desktop/aizome/apps/hotaru/src/app.rs:195` |
| HOT-09 | Copy-on-select primary-selection token | Low |  | implicit | `desktop/aizome/apps/hotaru/src/app.rs:229` |
| [HOT-10](implicit-state-machines/hotaru-app.md#hot-10) | Program clipboard request flow (OSC 52 / OSC 5522) and the single question | High |  | implicit | `desktop/aizome/apps/hotaru/src/app.rs:232` |
| HOT-11 | Desktop notification gating | Low |  | implicit | `desktop/aizome/apps/hotaru/src/app.rs:235` |
| [HOT-12](implicit-state-machines/hotaru-app.md#hot-12) | Settings persistence, reload and deferred window close | High | [confirmed](implicit-state-machines/reproductions.md#hot-12) | implicit | `desktop/aizome/apps/hotaru/src/app.rs:204` |
| HOT-13 | Settings window and shortcut recording mode | Medium |  | implicit | `desktop/aizome/apps/hotaru/src/app.rs:217` |
| [HOT-14](implicit-state-machines/hotaru-app.md#hot-14) | Appearance ownership between the desktop watcher and Hotaru's theme | High | [confirmed](implicit-state-machines/reproductions.md#hot-14) | implicit | `desktop/aizome/apps/hotaru/src/app.rs:208` |
| HOT-15 | Latest-request single-slot worker threads (six copies) | Medium |  | implicit | `desktop/aizome/apps/hotaru/src/automatic_links/mod.rs:46` |
| HOT-16 | Go to Tab palette lifecycle | Low |  | implicit | `desktop/aizome/apps/hotaru/src/go_to_tab.rs:311` |
| [HOT-17](implicit-state-machines/hotaru-app.md#hot-17) | Single-instance claim and launch handoff | High | [confirmed](implicit-state-machines/reproductions.md#hot-17) | implicit | `desktop/aizome/apps/hotaru/src/instance.rs:25` |
| HOT-18 | Link-path resolver helper child supervision | Low |  | implicit | `desktop/aizome/apps/hotaru/src/link_resolver.rs:159` |
| [HOT-19](implicit-state-machines/hotaru-app.md#hot-19) | Find: query, capture, whole-history search and navigation | High |  | implicit | `desktop/aizome/apps/hotaru/src/search.rs:17` |
| HOT-20 | Per-pane clipboard request admission tickets and the selections queue | Low |  | implicit | `desktop/aizome/apps/hotaru/src/selections.rs:75` |
| [HOT-21](implicit-state-machines/hotaru-app.md#hot-21) | PTY worker lifecycle (spawn, run, EOF, hang-up, exit report, stop) | High | [confirmed](implicit-state-machines/reproductions.md#hot-21) | implicit | `desktop/aizome/apps/hotaru/src/session.rs:71` |
| HOT-22 | PTY worker frame cadence, acknowledgement handshake and idle compression | Medium |  | implicit | `desktop/aizome/apps/hotaru/src/session.rs:69` |
| HOT-23 | PTY worker latest-state controls mailbox | Low |  | implicit | `desktop/aizome/apps/hotaru/src/session.rs:78` |
| HOT-24 | PTY input and reply back-pressure accounting | Medium |  | implicit | `desktop/aizome/apps/hotaru/src/session.rs:70` |
| HOT-25 | PTY reader (gather) thread | Low |  | implicit | `desktop/aizome/apps/hotaru/src/session.rs:265` |
| HOT-26 | Tab strip scroll-follow mode | Low |  | implicit | `desktop/aizome/apps/hotaru/src/tab_strip.rs:131` |
| [HOT-27](implicit-state-machines/hotaru-app.md#hot-27) | Pointer drag selection and held-button tracking | High | [confirmed](implicit-state-machines/reproductions.md#hot-27) | implicit | `desktop/aizome/apps/hotaru/src/terminal_view.rs:245` |
| HOT-28 | Modifier-click link press gesture | Medium |  | implicit | `desktop/aizome/apps/hotaru/src/terminal_view.rs:258` |
| [HOT-29](implicit-state-machines/hotaru-app.md#hot-29) | Hover and automatic-link request lifecycle in the terminal view | High |  | implicit | `desktop/aizome/apps/hotaru/src/terminal_view.rs:254` |
| HOT-30 | Layout-to-tick deferred notifications | Low |  | implicit | `desktop/aizome/apps/hotaru/src/terminal_view.rs:248` |
| HOT-31 | Cursor blink phase | Low |  | implicit | `desktop/aizome/apps/hotaru/src/terminal_view.rs:29` |
| HOT-32 | Held-key tracking for synthetic releases | Low |  | implicit | `desktop/aizome/apps/hotaru/src/terminal_view.rs:252` |
| HOT-33 | Retained previous frame for scroll reuse and overhang repair | Low |  | implicit | `desktop/aizome/apps/hotaru/src/terminal_view.rs:231` |
| HOT-34 | (test harness) Native harness waits and scripted PTY child programs | Low |  | implicit | `desktop/aizome/apps/hotaru/tests/native.rs:19` |
| HOT-35 | (test harness) Virtual pointer and keyboard probes with a stdin command protocol | Low |  | implicit | `desktop/aizome/apps/hotaru/tests/wayland/auto-link-pointer.c:43` |
| HOT-36 | (test harness) Stage-file fixtures acknowledged by window title | Low |  | implicit | `desktop/aizome/apps/hotaru/tests/wayland/auto_links.py:33` |
| HOT-37 | (test harness) Per-pane FIFO fixture for program clipboard requests | Low |  | implicit | `desktop/aizome/apps/hotaru/tests/wayland/selections.py:86` |
| HOT-38 | (test harness) Private headless-Sway driver lifecycle and polling waits | Low |  | implicit | `desktop/aizome/apps/hotaru/tests/wayland/smoke.py:20` |
| HOT-39 | (test harness) Marker-file PTY fixture handshake | Low |  | implicit | `desktop/aizome/apps/hotaru/tests/wayland/smoke.py:152` |
| HOT-40 | (test harness) Stalled data-control clipboard owner | Low |  | implicit | `desktop/aizome/apps/hotaru/tests/wayland/stall.c:14` |

#### [KOY: Koyomi calendar](implicit-state-machines/koyomi.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| [KOY-01](implicit-state-machines/koyomi.md#koy-01) | Koyomi agent service lifecycle (unit, bus connect retry, listener threads, wake loop) | High | [confirmed](implicit-state-machines/reproductions.md#koy-01) | implicit | `desktop/aizome/apps/koyomi/data/aizome-koyomi-agent.service:18` |
| [KOY-02](implicit-state-machines/koyomi.md#koy-02) | Alert delivery state: due → fired → shown → snoozed/closed | High | [confirmed](implicit-state-machines/reproductions.md#koy-02) | implicit | `desktop/aizome/apps/koyomi/src/agent.rs:52` |
| [KOY-03](implicit-state-machines/koyomi.md#koy-03) | Settings and window-state persistence (unsaved settings, debounced state, agent's stale copy) | High | [confirmed](implicit-state-machines/reproductions.md#koy-03) | implicit | `desktop/aizome/apps/koyomi/src/app.rs:233` |
| [KOY-04](implicit-state-machines/koyomi.md#koy-04) | Navigation and selection: view, anchor, cursor, mini month, selected occurrence | High | [confirmed](implicit-state-machines/reproductions.md#koy-04) | implicit | `desktop/aizome/apps/koyomi/src/app.rs:238` |
| [KOY-05](implicit-state-machines/koyomi.md#koy-05) | Modal sheet slots: editor, import, name, settings | High | [confirmed](implicit-state-machines/reproductions.md#koy-05) | implicit | `desktop/aizome/apps/koyomi/src/app.rs:249` |
| KOY-06 | Sequence-guarded one-shot timers: status note, state persist, grid scroll request | Low |  | implicit | `desktop/aizome/apps/koyomi/src/app.rs:253` |
| KOY-07 | Derived occurrence caches: index, generation, occ, view-model memo | Medium |  | implicit | `desktop/aizome/apps/koyomi/src/app.rs:255` |
| KOY-08 | Series-scope confirmation (`pending` + alert) | Medium |  | implicit | `desktop/aizome/apps/koyomi/src/app.rs:262` |
| KOY-09 | Undo/redo history | Medium |  | implicit | `desktop/aizome/apps/koyomi/src/app.rs:263` |
| [KOY-10](implicit-state-machines/koyomi.md#koy-10) | Calendar save pipeline: dirty → debounce → background write → stamp | High | [confirmed](implicit-state-machines/reproductions.md#koy-10) | implicit | `desktop/aizome/apps/koyomi/src/app.rs:265` |
| KOY-11 | Launch phase and argument hand-off | Low |  | implicit | `desktop/aizome/apps/koyomi/src/app.rs:272` |
| [KOY-12](implicit-state-machines/koyomi.md#koy-12) | Recurring series split state: master rule, EXDATE/RDATE, RECURRENCE-ID overrides | High | [confirmed](implicit-state-machines/reproductions.md#koy-12) | implicit | `desktop/aizome/apps/koyomi/src/model.rs:261` |
| KOY-13 | Single-instance socket: claim, hand off, serve | Medium |  | implicit | `desktop/aizome/apps/koyomi/src/instance.rs:26` |
| [KOY-14](implicit-state-machines/koyomi.md#koy-14) | Store's view of the calendar folder: stamps, reload, trash | High | [confirmed](implicit-state-machines/reproductions.md#koy-14) | implicit | `desktop/aizome/apps/koyomi/src/store.rs:29` |
| KOY-15 | Event editor draft | Medium |  | implicit | `desktop/aizome/apps/koyomi/src/ui/editor.rs:48` |
| KOY-16 | Month chip drag | Low |  | implicit | `desktop/aizome/apps/koyomi/src/ui/month.rs:239` |
| [KOY-17](implicit-state-machines/koyomi.md#koy-17) | Time-grid pointer drag: move, resize, create; edge hover and scroll | High | [confirmed](implicit-state-machines/reproductions.md#koy-17) | implicit | `desktop/aizome/apps/koyomi/src/ui/week.rs:181` |
| [KOY-18](implicit-state-machines/koyomi.md#koy-18) | All-day strip drag | High | [confirmed](implicit-state-machines/reproductions.md#koy-18) | implicit | `desktop/aizome/apps/koyomi/src/ui/week.rs:946` |
| KOY-19 | (test harness) private D-Bus daemon and fake notification server | Low |  | implicit | `desktop/aizome/apps/koyomi/tests/agent.rs:21` |

#### [YUK: Yukimi system manager (Aizome app and core crates)](implicit-state-machines/yukimi.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| [YUK-01](implicit-state-machines/yukimi.md#yuk-01) | Sekka update channel and legacy migration in flake.nix | High |  | implicit | `apps/yukimi/crates/yukimi-config/src/sekka.rs:28` |
| [YUK-02](implicit-state-machines/yukimi.md#yuk-02) | Update receipt and system completion advice | High | [confirmed](implicit-state-machines/reproductions.md#yuk-02) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) | `apps/yukimi/crates/yukimi-system/src/completion.rs:59` |
| YUK-03 | Tatami session update record and Keep Awake check | Medium |  | implicit | `apps/yukimi/crates/yukimi-system/src/completion.rs:63` |
| YUK-04 | Aizome live-session completion probe | Medium |  | implicit | `apps/yukimi/crates/yukimi-system/src/completion/aizome.rs:27` |
| YUK-05 | (test harness) fake Aizome IPC server and stubbed session launcher | Low |  | implicit | `apps/yukimi/crates/yukimi-system/src/completion/aizome.rs:1044` |
| YUK-06 | Nix activity tracker and the helper's stage protocol | Low |  | implicit | `apps/yukimi/crates/yukimi-system/src/log.rs:91` |
| YUK-07 | Stop handle and streamed child lifecycle | Medium |  | implicit | `apps/yukimi/crates/yukimi-system/src/nix.rs:68` |
| [YUK-08](implicit-state-machines/yukimi.md#yuk-08) | Helper cancellation phase (Working / Stopping / Committed) | High |  | implicit | `apps/yukimi/yukimi-helper/src/main.rs:83` |
| [YUK-09](implicit-state-machines/yukimi.md#yuk-09) | Privileged change transaction (back up, edit, update, build, commit, activate) | High | [confirmed](implicit-state-machines/reproductions.md#yuk-09) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) | `apps/yukimi/yukimi-helper/src/main.rs:804` |
| [YUK-10](implicit-state-machines/yukimi.md#yuk-10) | Saved per-generation configuration snapshots | High | [confirmed](implicit-state-machines/reproductions.md#yuk-10) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) | `apps/yukimi/yukimi-helper/src/main.rs:62` |
| [YUK-11](implicit-state-machines/yukimi.md#yuk-11) | Rollback and clean-up sequences | High | [confirmed](implicit-state-machines/reproductions.md#yuk-11) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) | `apps/yukimi/yukimi-helper/src/main.rs:1011` |
| [YUK-12](implicit-state-machines/yukimi.md#yuk-12) | Privileged operation client and worker (pkexec client, transient unit, result file) | High |  | implicit | `apps/yukimi/yukimi-helper/src/transaction.rs:45` |
| YUK-13 | X11 fallback nested Weston lifecycle | Low |  | implicit | `desktop/aizome/apps/yukimi/data/yukimi-launch:12` |
| YUK-14 | GUI reading epochs (configuration, store, Nixpkgs path, deferred launch) | Medium |  | implicit | `desktop/aizome/apps/yukimi/src/app.rs:345` |
| [YUK-15](implicit-state-machines/yukimi.md#yuk-15) | Package index lifecycle | High | [confirmed](implicit-state-machines/reproductions.md#yuk-15) | implicit | `desktop/aizome/apps/yukimi/src/app.rs:350` |
| YUK-16 | Update check (in-flight flag and persisted answer) | Medium |  | implicit | `desktop/aizome/apps/yukimi/src/app.rs:356` |
| YUK-17 | Search query coalescing | Low |  | implicit | `desktop/aizome/apps/yukimi/src/app.rs:363` |
| [YUK-18](implicit-state-machines/yukimi.md#yuk-18) | GUI job record and progress sheet | High |  | implicit | `desktop/aizome/apps/yukimi/src/app.rs:392` |
| [YUK-19](implicit-state-machines/yukimi.md#yuk-19) | Desktops page choice drafts | High | [confirmed](implicit-state-machines/reproductions.md#yuk-19) | implicit | `desktop/aizome/apps/yukimi/src/app.rs:385` |
| [YUK-20](implicit-state-machines/yukimi.md#yuk-20) | Sheet lifecycles: package details, branch chooser, comparison | High | [confirmed](implicit-state-machines/reproductions.md#yuk-20) | implicit | `desktop/aizome/apps/yukimi/src/app.rs:367` |
| YUK-21 | History generation diffs | Medium |  | implicit | `desktop/aizome/apps/yukimi/src/app.rs:372` |
| YUK-22 | Status line notes | Low |  | implicit | `desktop/aizome/apps/yukimi/src/app.rs:398` |
| YUK-23 | Restart or sign out to finish an update | Low |  | implicit | `desktop/aizome/apps/yukimi/src/app.rs:1195` |
| YUK-24 | Single-instance socket and hand-off | Medium |  | implicit | `desktop/aizome/apps/yukimi/src/instance.rs:20` |
| YUK-25 | Job step runner with pkexec authentication recovery | Medium |  | implicit | `desktop/aizome/apps/yukimi/src/job.rs:136` |
| YUK-26 | (test harness) fake pkexec and stop timers | Low |  | implicit | `desktop/aizome/apps/yukimi/src/job.rs:481` |
| YUK-27 | Unprivileged update preparation pipeline | Medium |  | implicit | `desktop/aizome/apps/yukimi/src/ops.rs:289` |
| YUK-28 | Falling snow animation ticks | Low |  | implicit | `desktop/aizome/apps/yukimi/src/ui/widgets.rs:483` |
| YUK-29 | Per-input revision phase from saved generations | Low |  | implicit | `desktop/aizome/apps/yukimi/src/update_history.rs:10` |
| YUK-30 | (test harness) settle loops and scripted Nix/helper stand-ins | Low |  | implicit | `desktop/aizome/apps/yukimi/tests/headless.rs:55` |

#### [NTU: Nisshi, Tokei and Utsushi](implicit-state-machines/nisshi-tokei-utsushi.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| [NTU-01](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-01) | Nisshi system-log unlock (root reader through pkexec) | High | [confirmed](implicit-state-machines/reproductions.md#ntu-01) | implicit | `desktop/aizome/apps/nisshi/src/app.rs:193` |
| NTU-02 | Nisshi reader generations and pending-request table | Medium |  | implicit | `desktop/aizome/apps/nisshi/src/app.rs:189` |
| [NTU-03](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-03) | Nisshi journal read pipeline: newest page, earlier pages, follow, whole-boot search | High | [confirmed](implicit-state-machines/reproductions.md#ntu-03) | implicit | `desktop/aizome/apps/nisshi/src/app.rs:228` |
| NTU-04 | Nisshi log-file scope and its read | Medium |  | implicit | `desktop/aizome/apps/nisshi/src/app.rs:202` |
| NTU-05 | Nisshi following the newest message, and the unseen count | Medium |  | implicit | `desktop/aizome/apps/nisshi/src/app.rs:216` |
| NTU-06 | Nisshi open-on-cursor pending selection | Low |  | implicit | `desktop/aizome/apps/nisshi/src/app.rs:197` |
| NTU-07 | Nisshi search typing debounce | Low |  | implicit | `desktop/aizome/apps/nisshi/src/app.rs:206` |
| NTU-08 | Nisshi view refresh coalescing and incremental extension | Medium |  | implicit | `desktop/aizome/apps/nisshi/src/app.rs:232` |
| NTU-09 | Nisshi selected message's fields fetch | Low |  | implicit | `desktop/aizome/apps/nisshi/src/app.rs:226` |
| NTU-10 | Nisshi timeline brush drag | Low |  | implicit | `desktop/aizome/apps/nisshi/src/app.rs:231` |
| NTU-11 | Nisshi reader client process (helper as the user, or through pkexec as root) | Medium |  | implicit | `desktop/aizome/apps/nisshi/src/backend.rs:91` |
| NTU-12 | Nisshi helper's running reads and process tree | Medium |  | implicit | `desktop/aizome/apps/nisshi/src/bin/helper.rs:59` |
| NTU-13 | (test harness) Nisshi stand-in journalctl | Low |  | implicit | `desktop/aizome/apps/nisshi/src/fixture.rs:621` |
| NTU-14 | Single-instance socket claim and hand-off (Nisshi, Tokei, Utsushi) | Low |  | implicit | `desktop/aizome/apps/nisshi/src/instance.rs:23` |
| NTU-15 | Nisshi `--print` one-shot read | Low | [unverified](implicit-state-machines/reproductions.md#ntu-15) | implicit | `desktop/aizome/apps/nisshi/src/main.rs:25` |
| NTU-16 | (test harness) Nisshi reader stage: stand-in pkexec, environment lock, polling waits | Low |  | implicit | `desktop/aizome/apps/nisshi/tests/reader.rs:18` |
| [NTU-17](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-17) | Tokei alarm schedule: alarms.json against systemd transient timers | High | [confirmed](implicit-state-machines/reproductions.md#ntu-17) | implicit | `desktop/aizome/apps/tokei/src/alarms.rs:33` |
| NTU-18 | (test harness) Tokei recording scheduler and fake-clock rig | Low |  | implicit | `desktop/aizome/apps/tokei/src/alarms.rs:320` |
| [NTU-19](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-19) | Tokei's in-memory alarm list, its saves and the undo slot | High | [confirmed](implicit-state-machines/reproductions.md#ntu-19) | implicit | `desktop/aizome/apps/tokei/src/app.rs:180` |
| NTU-20 | Tokei sheets: Add City and the alarm editor | Medium | [unverified](implicit-state-machines/reproductions.md#ntu-20) | implicit | `desktop/aizome/apps/tokei/src/app.rs:195` |
| NTU-21 | Tokei timer end check and tick phase | Low |  | implicit | `desktop/aizome/apps/tokei/src/app.rs:190` |
| NTU-22 | Tokei stopwatch run state and frame timer | Low |  | implicit | `desktop/aizome/apps/tokei/src/timing.rs:97` |
| NTU-23 | Tokei state-file persist debounce | Low |  | implicit | `desktop/aizome/apps/tokei/src/app.rs:198` |
| NTU-24 | Tokei alarm ring process (`aizome-tokei --ring`) | Medium | [unverified](implicit-state-machines/reproductions.md#ntu-24) | implicit | `desktop/aizome/apps/tokei/src/ring.rs:45` |
| NTU-25 | Tokei sound player child process | Low |  | implicit | `desktop/aizome/apps/tokei/src/sound.rs:128` |
| [NTU-26](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-26) | Utsushi playback item lifecycle | High | [confirmed](implicit-state-machines/reproductions.md#ntu-26) | implicit | `desktop/aizome/apps/utsushi/src/app.rs:157` |
| NTU-27 | Utsushi timeline scrubbing | Medium |  | implicit | `desktop/aizome/apps/utsushi/src/app.rs:163` |
| [NTU-28](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-28) | Utsushi full-screen controls auto-hide | High |  | implicit | `desktop/aizome/apps/utsushi/src/app.rs:183` |
| NTU-29 | Utsushi clock and subtitle wake timers | Low |  | implicit | `desktop/aizome/apps/utsushi/src/app.rs:191` |
| NTU-30 | Utsushi Media Info sheet | Low |  | implicit | `desktop/aizome/apps/utsushi/src/app.rs:148` |
| NTU-31 | Utsushi MPRIS service thread | Low |  | implicit | `desktop/aizome/apps/utsushi/src/mpris.rs:312` |
| NTU-32 | Utsushi persisted resume positions and recents | Low |  | implicit | `desktop/aizome/apps/utsushi/src/store.rs:53` |
| NTU-33 | (test harness) Utsushi real-player polling waits | Low |  | implicit | `desktop/aizome/apps/utsushi/tests/utsushi.rs:33` |

#### [KKO: Kura and Koe](implicit-state-machines/kura-koe.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| [KKO-01](implicit-state-machines/kura-koe.md#kko-01) | Dictation on/off: koe.toml `enabled` gating the user unit and the daemon it runs | High | [partly](implicit-state-machines/reproductions.md#kko-01) | implicit | `desktop/aizome/apps/koe/data/aizome-koe.service:15` |
| [KKO-02](implicit-state-machines/kura-koe.md#kko-02) | Koe window's view of the service switch | High |  | implicit | `desktop/aizome/apps/koe/src/app.rs:206` |
| [KKO-03](implicit-state-machines/kura-koe.md#kko-03) | Model download, set-up (download then enable), and closing while downloading | High |  | implicit | `desktop/aizome/apps/koe/src/app.rs:92` |
| KKO-04 | Window's status subscription reconnect loop | Low |  | implicit | `desktop/aizome/apps/koe/src/app.rs:495` |
| KKO-05 | Window's koe.toml and settings.toml change detection | Low |  | implicit | `desktop/aizome/apps/koe/src/app.rs:196` |
| KKO-06 | pw-record capture task | Low |  | implicit | `desktop/aizome/apps/koe/src/audio.rs:55` |
| [KKO-07](implicit-state-machines/kura-koe.md#kko-07) | Daemon dictation pipeline (record → transcribe → deliver) | High |  | implicit | `desktop/aizome/apps/koe/src/daemon.rs:41` |
| [KKO-08](implicit-state-machines/kura-koe.md#kko-08) | Teaching session lease | High |  | implicit | `desktop/aizome/apps/koe/src/daemon.rs:31` |
| KKO-09 | Deferred settings reload and model warmup | Medium |  | implicit | `desktop/aizome/apps/koe/src/daemon.rs:486` |
| KKO-10 | Daemon IPC connection: request, subscription and shutdown flush | Low |  | implicit | `desktop/aizome/apps/koe/src/daemon.rs:382` |
| KKO-11 | Paste delivery guard | Medium |  | implicit | `desktop/aizome/apps/koe/src/desktop.rs:118` |
| KKO-12 | Resident Whisper model | Medium |  | implicit | `desktop/aizome/apps/koe/src/engine.rs:17` |
| KKO-13 | Single-instance socket hand-off (Koe and Kura windows) | Low |  | implicit | `desktop/aizome/apps/koe/src/instance.rs:30` |
| KKO-14 | Daemon single-instance lock and control socket file | Low |  | implicit | `desktop/aizome/apps/koe/src/ipc.rs:90` |
| KKO-15 | CLI vocabulary edit retry | Low |  | implicit | `desktop/aizome/apps/koe/src/main.rs:530` |
| KKO-16 | Model partial-download files | Low |  | implicit | `desktop/aizome/apps/koe/src/models.rs:191` |
| KKO-17 | Tray mirror of daemon status and its own error | Low |  | implicit | `desktop/aizome/apps/koe/src/tray.rs:21` |
| [KKO-18](implicit-state-machines/kura-koe.md#kko-18) | Vocabulary page: request queue, stale-read guard, teaching client, closing | High |  | implicit | `desktop/aizome/apps/koe/src/vocab.rs:75` |
| KKO-19 | Vocabulary store: revision token, lock and staged saves | Medium |  | implicit | `desktop/aizome/apps/koe/src/vocabulary.rs:41` |
| KKO-20 | (test harness) Real-Aizome dictation regression driver | Low |  | implicit | `desktop/aizome/apps/koe/tests/aizome.rs:18` |
| KKO-21 | (test harness) Daemon process harness with fake microphone, ASR server, compositor and shell | Low |  | implicit | `desktop/aizome/apps/koe/tests/daemon.rs:32` |
| KKO-22 | (test harness) Window set-up rig with a gated service and a stalling model server | Low |  | implicit | `desktop/aizome/apps/koe/tests/setup.rs:29` |
| KKO-23 | (test harness) Private D-Bus tray roundtrip with host restart | Low |  | implicit | `desktop/aizome/apps/koe/tests/tray.rs:29` |
| KKO-24 | CLI scan progress ticker | Low |  | implicit | `desktop/aizome/apps/kura/src/cli.rs:190` |
| KKO-25 | Kura's on-disk caches: index.json/index.md and nix-store.json | Low |  | implicit | `desktop/aizome/apps/kura/src/index.rs:418` |
| KKO-26 | Parallel scanner work queue | Medium |  | implicit | `desktop/aizome/apps/kura/src/scan.rs:171` |
| KKO-27 | Freedesktop Trash moves | Low |  | implicit | `desktop/aizome/apps/kura/src/trash.rs:95` |
| KKO-28 | Tree arena tombstones and grafts | Medium |  | implicit | `desktop/aizome/apps/kura/src/tree.rs:48` |
| KKO-29 | Dragging into the Collector (sunburst and list rows) | Medium |  | implicit | `desktop/aizome/apps/kura/src/ui/drag.rs:39` |
| [KKO-30](implicit-state-machines/kura-koe.md#kko-30) | Kura scan lifecycle and page | High | [confirmed](implicit-state-machines/reproductions.md#kko-30) | implicit | `desktop/aizome/apps/kura/src/ui/mod.rs:52` |
| [KKO-31](implicit-state-machines/kura-koe.md#kko-31) | Analysis and index writing after a scan or a change | High | [confirmed](implicit-state-machines/reproductions.md#kko-31) | implicit | `desktop/aizome/apps/kura/src/ui/mod.rs:69` |
| KKO-32 | Launch-time background loads (disks, Nix store size) | Low |  | implicit | `desktop/aizome/apps/kura/src/ui/mod.rs:190` |
| KKO-33 | Sunburst centre transition animation | Low |  | implicit | `desktop/aizome/apps/kura/src/ui/mod.rs:214` |
| [KKO-34](implicit-state-machines/kura-koe.md#kko-34) | Destructive actions: confirm, move or empty, rescan, graft | High |  | implicit | `desktop/aizome/apps/kura/src/ui/mod.rs:102` |
| KKO-35 | Suggestions selection: focused hint, expanded, ticks and kept copies | Medium |  | implicit | `desktop/aizome/apps/kura/src/ui/mod.rs:241` |
| KKO-36 | Sunburst paint cache and invalidation counters | Low |  | implicit | `desktop/aizome/apps/kura/src/ui/sunburst.rs:151` |
| KKO-37 | (test harness) Kura window settle loops | Low |  | implicit | `desktop/aizome/apps/kura/tests/ui.rs:219` |

#### [TSH: Tansu and Shiori](implicit-state-machines/tansu-shiori.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| TSH-01 | (test harness) Corpus example's per-document worker driver | Low |  | implicit | `desktop/aizome/apps/shiori/examples/corpus.rs:24` |
| [TSH-02](implicit-state-machines/tansu-shiori.md#tsh-02) | Shiori document open and worker-restart lifecycle | High | [confirmed](implicit-state-machines/reproductions.md#tsh-02) | implicit | `desktop/aizome/apps/shiori/src/app.rs:128` |
| [TSH-03](implicit-state-machines/tansu-shiori.md#tsh-03) | Changed-file watch and reload in place | High |  | implicit | `desktop/aizome/apps/shiori/src/app.rs:267` |
| [TSH-04](implicit-state-machines/tansu-shiori.md#tsh-04) | Tile/text request coalescing and drawing caches | High |  | implicit | `desktop/aizome/apps/shiori/src/app.rs:287` |
| TSH-05 | Sidebar preview prefetch (ThumbRows) | Medium |  | implicit | `desktop/aizome/apps/shiori/src/app.rs:69` |
| TSH-06 | Pinch / ❖-scroll zoom settle debounce | Low |  | implicit | `desktop/aizome/apps/shiori/src/app.rs:313` |
| TSH-07 | Pointer press/drag gesture on the pages | Medium |  | implicit | `desktop/aizome/apps/shiori/src/app.rs:295` |
| [TSH-08](implicit-state-machines/tansu-shiori.md#tsh-08) | Copy waiting for pages' text | High | [confirmed](implicit-state-machines/reproductions.md#tsh-08) | implicit | `desktop/aizome/apps/shiori/src/app.rs:111` |
| [TSH-09](implicit-state-machines/tansu-shiori.md#tsh-09) | Find session | High | [confirmed](implicit-state-machines/reproductions.md#tsh-09) | implicit | `desktop/aizome/apps/shiori/src/app.rs:140` |
| TSH-10 | Shiori sheets (password, go to page, info) | Low |  | implicit | `desktop/aizome/apps/shiori/src/app.rs:718` |
| TSH-11 | Tick-counted notice expiry and scroll-bar visibility | Low |  | implicit | `desktop/aizome/apps/shiori/src/app.rs:731` |
| TSH-12 | Per-document view modes (sidebar, tab, outline expansion, full screen) | Low |  | implicit | `desktop/aizome/apps/shiori/src/app.rs:306` |
| TSH-13 | Window bookkeeping: empty placeholder window and command target | Low |  | implicit | `desktop/aizome/apps/shiori/src/app.rs:721` |
| [TSH-14](implicit-state-machines/tansu-shiori.md#tsh-14) | Document worker supervisor (spawn, watchdog, restart, give up, close) | High |  | implicit | `desktop/aizome/apps/shiori/src/engine.rs:55` |
| TSH-15 | Single-instance socket hand-off (Shiori and Tansu) | Medium | [unverified](implicit-state-machines/reproductions.md#tsh-15) | implicit | `desktop/aizome/apps/shiori/src/instance.rs:30` |
| TSH-16 | Pages widget incremental-damage snapshot | Low |  | implicit | `desktop/aizome/apps/shiori/src/pages.rs:304` |
| TSH-17 | Paint-to-program view-size feedback (Seen) | Low |  | implicit | `desktop/aizome/apps/shiori/src/pages.rs:37` |
| TSH-18 | ActualText marked-content stack | Low |  | implicit | `desktop/aizome/apps/shiori/src/pdftext.rs:119` |
| TSH-19 | Persisted reading place (places.json) | Low |  | implicit | `desktop/aizome/apps/shiori/src/places.rs:53` |
| TSH-20 | Stand-in pixel buffer pool | Low |  | implicit | `desktop/aizome/apps/shiori/src/standin.rs:30` |
| TSH-21 | Worker process job queue and document lifecycle | Medium |  | implicit | `desktop/aizome/apps/shiori/src/worker.rs:45` |
| TSH-22 | (test harness) Shiori settle and wait loops | Low |  | implicit | `desktop/aizome/apps/shiori/tests/budget.rs:20` |
| TSH-23 | Tansu UDisks snapshot refresh and drive fallback | Medium |  | implicit | `desktop/aizome/apps/tansu/src/app.rs:174` |
| TSH-24 | Tansu segment selection tracking | Medium |  | implicit | `desktop/aizome/apps/tansu/src/app.rs:180` |
| [TSH-25](implicit-state-machines/tansu-shiori.md#tsh-25) | Tansu sheet lifecycle (forms holding segment indexes) | High | [confirmed](implicit-state-machines/reproductions.md#tsh-25) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) | `desktop/aizome/apps/tansu/src/app.rs:184` |
| [TSH-26](implicit-state-machines/tansu-shiori.md#tsh-26) | Tansu apply session (Apply sheet → Run → Running sheet) | High | [confirmed](implicit-state-machines/reproductions.md#tsh-26) | implicit | `desktop/aizome/apps/tansu/src/app.rs:185` |
| TSH-27 | Tansu immediate operations in flight (mount, unlock, attach…) | Low |  | implicit | `desktop/aizome/apps/tansu/src/app.rs:187` |
| TSH-28 | Tansu filesystem-tool probing | Low |  | implicit | `desktop/aizome/apps/tansu/src/app.rs:200` |
| TSH-29 | (test harness) In-memory UDisks mock | Low |  | implicit | `desktop/aizome/apps/tansu/src/mock.rs:25` |
| TSH-30 | Tansu pending plan and undo history | Medium |  | implicit | `desktop/aizome/apps/tansu/src/plan.rs:112` |
| TSH-31 | Tansu run step engine (steps, effect waits, confirmation tokens) | Medium |  | implicit | `desktop/aizome/apps/tansu/src/run.rs:112` |
| TSH-32 | (test harness) Private dbus-daemon with a fake UDisks2 | Low |  | implicit | `desktop/aizome/apps/tansu/tests/udisks.rs:30` |

#### [SMT: Sumi, Myaku and Tenki](implicit-state-machines/sumi-myaku-tenki.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| SMT-01 | GPU lens card choice, hover and projected frame | Low |  | implicit | `desktop/aizome/apps/myaku/src/app.rs:111` |
| [SMT-02](implicit-state-machines/sumi-myaku-tenki.md#smt-02) | Table selection, expansion and launch request across readings | High | [confirmed](implicit-state-machines/reproductions.md#smt-02) | implicit | `desktop/aizome/apps/myaku/src/app.rs:124` |
| [SMT-03](implicit-state-machines/sumi-myaku-tenki.md#smt-03) | Quit/pause confirmation and the status notice | High | [confirmed](implicit-state-machines/reproductions.md#smt-03) | implicit | `desktop/aizome/apps/myaku/src/app.rs:127` |
| SMT-04 | Energy Meters utility window | Low |  | implicit | `desktop/aizome/apps/myaku/src/app.rs:104` |
| SMT-05 | Single-instance socket claim and hand-over (Myaku, Sumi, Tenki) | Low |  | implicit | `desktop/aizome/apps/myaku/src/instance.rs:23` |
| SMT-06 | NVML library session and per-device sample cursor | Low |  | implicit | `desktop/aizome/apps/myaku/src/nvml.rs:192` |
| SMT-07 | Sampler thread schedule and window commands | Medium |  | implicit | `desktop/aizome/apps/myaku/src/sampler.rs:154` |
| SMT-08 | Per-process reading cache with staleness timers and a maps budget | Medium |  | implicit | `desktop/aizome/apps/myaku/src/sampler.rs:194` |
| SMT-09 | DRM client scan and busy-delta baselines | Low |  | implicit | `desktop/aizome/apps/myaku/src/sampler.rs:291` |
| SMT-10 | Power metering across readings and sleep | Medium | [unverified](implicit-state-machines/reproductions.md#smt-10) | implicit | `desktop/aizome/apps/myaku/src/sampler.rs:296` |
| SMT-11 | Group identity cache and launcher-index refresh cadence | Low |  | implicit | `desktop/aizome/apps/myaku/src/sampler.rs:313` |
| SMT-12 | (test harness) scripted power meter that repeats its last reading | Low |  | implicit | `desktop/aizome/apps/myaku/src/sampler.rs:932` |
| [SMT-13](implicit-state-machines/sumi-myaku-tenki.md#smt-13) | Close-after-save for dirty documents | High | [confirmed](implicit-state-machines/reproductions.md#smt-13) | implicit | `desktop/aizome/apps/sumi/src/app.rs:176` |
| [SMT-14](implicit-state-machines/sumi-myaku-tenki.md#smt-14) | Link and Go to Line sheets | High | [confirmed](implicit-state-machines/reproductions.md#smt-14) | implicit | `desktop/aizome/apps/sumi/src/app.rs:155` |
| SMT-15 | Keyboard-target document tracking | Low |  | implicit | `desktop/aizome/apps/sumi/src/app.rs:174` |
| SMT-16 | Autosave store worker, session file and startup/quit phases | Medium |  | implicit | `desktop/aizome/apps/sumi/src/autosave.rs:66` |
| [SMT-17](implicit-state-machines/sumi-myaku-tenki.md#smt-17) | Document file, dirty and autosave lifecycle | High | [confirmed](implicit-state-machines/reproductions.md#smt-17) | implicit | `desktop/aizome/apps/sumi/src/doc.rs:178` |
| [SMT-18](implicit-state-machines/sumi-myaku-tenki.md#smt-18) | Background file read generation | High | [confirmed](implicit-state-machines/reproductions.md#smt-18) | implicit | `desktop/aizome/apps/sumi/src/doc.rs:200` |
| [SMT-19](implicit-state-machines/sumi-myaku-tenki.md#smt-19) | Find bar and search generation | High | [confirmed](implicit-state-machines/reproductions.md#smt-19) | implicit | `desktop/aizome/apps/sumi/src/doc.rs:19` |
| [SMT-20](implicit-state-machines/sumi-myaku-tenki.md#smt-20) | Incremental highlight states and colouring-pass scheduling | High |  | implicit | `desktop/aizome/apps/sumi/src/syntax/cache.rs:41` |
| SMT-21 | (test harness) background-work settle loop and crash simulation | Low |  | implicit | `desktop/aizome/apps/sumi/tests/sumi.rs:76` |
| SMT-22 | Per-position forecast fetch, cache and refresh | Medium |  | implicit | `desktop/aizome/apps/tenki/src/app.rs:30` |
| [SMT-23](implicit-state-machines/sumi-myaku-tenki.md#smt-23) | Place search debounce, generation and early Return | High |  | implicit | `desktop/aizome/apps/tenki/src/app.rs:43` |
| [SMT-24](implicit-state-machines/sumi-myaku-tenki.md#smt-24) | Location consent and GeoClue lookup | High | [confirmed](implicit-state-machines/reproductions.md#smt-24) | implicit | `desktop/aizome/apps/tenki/src/app.rs:61` |
| SMT-25 | Time-zone city bootstrap | Low |  | implicit | `desktop/aizome/apps/tenki/src/app.rs:137` |
| SMT-26 | Kept places order and selection persistence | Low |  | implicit | `desktop/aizome/apps/tenki/src/app.rs:125` |
| SMT-27 | Settings sheet | Low |  | implicit | `desktop/aizome/apps/tenki/src/app.rs:135` |
| SMT-28 | (test harness) recorded Open-Meteo and GeoClue services | Low |  | implicit | `desktop/aizome/apps/tenki/src/services.rs:146` |
| SMT-29 | Deferred file writer with per-path coalescing | Low |  | implicit | `desktop/aizome/apps/tenki/src/store.rs:209` |
| SMT-30 | Place list press, drag and hover gesture | Medium |  | implicit | `desktop/aizome/apps/tenki/src/ui/widgets.rs:118` |
| SMT-31 | (test harness) private D-Bus daemon and fake GeoClue client | Low |  | implicit | `desktop/aizome/apps/tenki/tests/geoclue.rs:17` |
| SMT-32 | (test harness) held search replies and drain loops | Low |  | implicit | `desktop/aizome/apps/tenki/tests/search_generation.rs:18` |
| SMT-33 | (test harness) pending location service and poll loop | Low |  | implicit | `desktop/aizome/apps/tenki/tests/tenki.rs:501` |

#### [SAP: Shashin, Soroban, Fusen and Annai](implicit-state-machines/small-apps.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| SAP-01 | Annai tour page and scene clock | Low |  | implicit | `desktop/aizome/apps/annai/src/app.rs:43` |
| [SAP-02](implicit-state-machines/small-apps.md#sap-02) | Single-instance socket claim and hand-off (Annai, Shashin, Soroban; Fusen through Sumi's copy) | High | [confirmed](implicit-state-machines/reproductions.md#sap-02) | implicit | `desktop/aizome/apps/annai/src/instance.rs:18` |
| SAP-03 | Annai first-run record and "Show at login" | Low |  | implicit | `desktop/aizome/apps/annai/src/state.rs:10` |
| [SAP-04](implicit-state-machines/small-apps.md#sap-04) | Fusen note draft, autosave and protected close | High |  | implicit | `desktop/aizome/apps/fusen/src/app.rs:65` |
| [SAP-05](implicit-state-machines/small-apps.md#sap-05) | Fusen stickies: windows, saved places and the login item | High | [confirmed](implicit-state-machines/reproductions.md#sap-05) | implicit | `desktop/aizome/apps/fusen/src/app.rs:211` |
| [SAP-06](implicit-state-machines/small-apps.md#sap-06) | Fusen launch, request hand-off and Drop-time save | High | [confirmed](implicit-state-machines/reproductions.md#sap-06) | implicit, bug fixed ([#301](https://github.com/bitemyapp/sekka/pull/301)) | `desktop/aizome/apps/fusen/src/app.rs:215` |
| [SAP-07](implicit-state-machines/small-apps.md#sap-07) | Fusen highlighting passes (incomplete, scheduled, notify) | High | [confirmed](implicit-state-machines/reproductions.md#sap-07) | implicit | `desktop/aizome/apps/fusen/src/app.rs:67` |
| SAP-08 | Modal sheets: Fusen's New Folder, Shashin's Get Info | Low |  | implicit | `desktop/aizome/apps/fusen/src/app.rs:214` |
| SAP-09 | Fusen note Trash move | Low |  | implicit | `desktop/aizome/apps/fusen/src/notes.rs:335` |
| [SAP-10](implicit-state-machines/small-apps.md#sap-10) | Shashin picture loading (serial, decode, folder listing) | High | [confirmed](implicit-state-machines/reproductions.md#sap-10) | implicit | `desktop/aizome/apps/shashin/src/app.rs:353` |
| [SAP-11](implicit-state-machines/small-apps.md#sap-11) | Shashin unsaved turns, saving and quitting | High | [confirmed](implicit-state-machines/reproductions.md#sap-11) | implicit | `desktop/aizome/apps/shashin/src/app.rs:356` |
| SAP-12 | Shashin zoom mode and centre | Low |  | implicit | `desktop/aizome/apps/shashin/src/app.rs:357` |
| [SAP-13](implicit-state-machines/small-apps.md#sap-13) | Shashin stage drawing requests (busy, want, reported) | High | [confirmed](implicit-state-machines/reproductions.md#sap-13) | implicit | `desktop/aizome/apps/shashin/src/app.rs:363` |
| SAP-14 | Shashin background errands counter | Low |  | implicit | `desktop/aizome/apps/shashin/src/app.rs:366` |
| SAP-15 | Shashin read-ahead cache | Low |  | implicit | `desktop/aizome/apps/shashin/src/app.rs:368` |
| SAP-16 | Shashin filmstrip thumbnails and their cache | Medium |  | implicit | `desktop/aizome/apps/shashin/src/app.rs:371` |
| SAP-17 | Shashin full screen and slideshow | Medium |  | implicit | `desktop/aizome/apps/shashin/src/app.rs:375` |
| [SAP-18](implicit-state-machines/small-apps.md#sap-18) | Shashin Move to Trash with Undo and Redo | High | [confirmed](implicit-state-machines/reproductions.md#sap-18) | implicit | `desktop/aizome/apps/shashin/src/app.rs:379` |
| SAP-19 | Seq-matched transient timers (Shashin's toolbar note, Soroban's key flash) | Low |  | implicit | `desktop/aizome/apps/shashin/src/app.rs:381` |
| SAP-20 | Shashin pan drag | Low |  | implicit | `desktop/aizome/apps/shashin/src/app.rs:383` |
| SAP-21 | (test harness) settle and poll loops | Low |  | implicit | `desktop/aizome/apps/shashin/tests/common/mod.rs:265` |
| SAP-22 | Soroban state persistence debounce | Low |  | implicit | `desktop/aizome/apps/soroban/src/app.rs:140` |
| SAP-23 | Soroban Save Tape sheet and replace confirmation | Medium |  | implicit | `desktop/aizome/apps/soroban/src/app.rs:146` |
| SAP-24 | Soroban bit grid hover and keyboard cursor | Low |  | implicit | `desktop/aizome/apps/soroban/src/bits.rs:168` |
| [SAP-25](implicit-state-machines/small-apps.md#sap-25) | Soroban calculator entry (entry, answer, error, 2nd, undo) | High | [confirmed](implicit-state-machines/reproductions.md#sap-25) | implicit | `desktop/aizome/apps/soroban/src/entry.rs:360` |
| SAP-26 | Soroban paper tape follow-the-end scrolling | Low |  | implicit | `desktop/aizome/apps/soroban/src/tape.rs:45` |

### Rest of Sekka

#### [HCO: Hotaru terminal engine](implicit-state-machines/hotaru-core.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| HCO-01 | Copy formatter open-scope emitter (style, link, deferred blanks) | Low |  | implicit | `apps/hotaru/crates/hotaru-core/src/formatter.rs:444` |
| HCO-02 | Scrollback block lifecycle and incremental compression cursor | Medium |  | implicit | `apps/hotaru/crates/hotaru-core/src/history.rs:306` |
| HCO-03 | Content identity tokens for asynchronous readers (epoch, origin, generation) | Medium |  | implicit | `apps/hotaru/crates/hotaru-core/src/history.rs:320` |
| HCO-04 | Kitty keyboard progressive-enhancement flag stack (per screen) | Low |  | implicit | `apps/hotaru/crates/hotaru-core/src/input.rs:146` |
| HCO-05 | Mouse report motion de-duplication | Low |  | implicit | `apps/hotaru/crates/hotaru-core/src/input.rs:724` |
| [HCO-06](implicit-state-machines/hotaru-core.md#hco-06) | Kitty clipboard (OSC 5522) write transaction | High |  | implicit | `apps/hotaru/crates/hotaru-core/src/kitty_clipboard.rs:965` |
| [HCO-07](implicit-state-machines/hotaru-core.md#hco-07) | Kitty clipboard session password grants and paste-event one-time passwords | High | [confirmed](implicit-state-machines/reproductions.md#hco-07) | implicit | `apps/hotaru/crates/hotaru-core/src/kitty_clipboard.rs:630` |
| HCO-08 | History byte-budget signal (dirty/peak/pause accounting) | Medium |  | implicit | `apps/hotaru/crates/hotaru-core/src/memory.rs:29` |
| HCO-09 | VT/DEC escape-sequence parser | Low |  | implicit | `apps/hotaru/crates/hotaru-core/src/parser.rs:43` |
| [HCO-10](implicit-state-machines/hotaru-core.md#hco-10) | Alternate screen switching (DEC modes 47, 1047, 1049) | High |  | implicit | `apps/hotaru/crates/hotaru-core/src/screen.rs:50` |
| HCO-11 | Saved cursor (DECSC/DECRC, SCOSC/SCORC, 1048, 1049) | Medium |  | implicit | `apps/hotaru/crates/hotaru-core/src/screen.rs:173` |
| [HCO-12](implicit-state-machines/hotaru-core.md#hco-12) | Pending wrap and soft-wrap row linkage | High |  | implicit | `apps/hotaru/crates/hotaru-core/src/screen.rs:175` |
| HCO-13 | Scroll region and left/right margins | Medium |  | implicit | `apps/hotaru/crates/hotaru-core/src/screen.rs:176` |
| HCO-14 | Active OSC 8 hyperlink | Low |  | implicit | `apps/hotaru/crates/hotaru-core/src/screen.rs:181` |
| HCO-15 | OSC 133 semantic prompt state | Medium |  | implicit | `apps/hotaru/crates/hotaru-core/src/screen.rs:185` |
| HCO-16 | DEC/ANSI terminal mode flags, XTSAVE/XTRESTORE and soft reset | Medium |  | implicit | `apps/hotaru/crates/hotaru-core/src/screen.rs:46` |
| HCO-17 | Character set designation and shifts (G0–G3, SI/SO, LS2/LS3, SS2/SS3) | Low |  | implicit | `apps/hotaru/crates/hotaru-core/src/screen.rs:705` |
| [HCO-18](implicit-state-machines/hotaru-core.md#hco-18) | Cursor style (DECSCUSR, DEC mode 12, configured default) | High | [confirmed](implicit-state-machines/reproductions.md#hco-18) | implicit | `apps/hotaru/crates/hotaru-core/src/screen.rs:709` |
| HCO-19 | Dynamic color overrides (OSC 4/10/11 vs configured defaults) | Low |  | implicit | `apps/hotaru/crates/hotaru-core/src/screen.rs:719` |
| HCO-20 | Pending reply/event queues and per-feed clipboard bound | Medium |  | implicit | `apps/hotaru/crates/hotaru-core/src/screen.rs:728` |
| HCO-21 | Nonblocking clipboard pipe transfer (writer and reader) | Low |  | implicit | `apps/hotaru/crates/hotaru-io/src/clipboard.rs:43` |
| HCO-22 | Thread-scoped SIGPIPE block guard | Low |  | implicit | `apps/hotaru/crates/hotaru-io/src/fd.rs:29` |
| HCO-23 | Default-shell integration bootstrap handoff (bash rcfile, zsh ZDOTDIR swap, fish init command) | Low |  | implicit | `apps/hotaru/crates/hotaru-io/src/launch.rs:19` |
| [HCO-24](implicit-state-machines/hotaru-core.md#hco-24) | PTY child lifecycle and background reaper (HUP → grace → KILL → wait) | High | [confirmed](implicit-state-machines/reproductions.md#hco-24) | implicit | `apps/hotaru/crates/hotaru-io/src/pty.rs:93` |
| HCO-25 | (test harness) Kitty clipboard embedder model replay | Low |  | implicit | `apps/hotaru/crates/hotaru-replay/src/bin/kitty-clipboard-replay.rs:65` |
| [HCO-26](implicit-state-machines/hotaru-core.md#hco-26) | Shell prompt hooks: load guards and last-reported-cwd de-duplication (OSC 7) | High | [confirmed](implicit-state-machines/reproductions.md#hco-26) | implicit | `apps/hotaru/resources/shell-integration/bash/hotaru.bash:3` |

#### [HTT: Hotaru engine tests and development tools](implicit-state-machines/hotaru-tests-tools.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| HTT-01 | (test harness) Clipboard transfer pump/poll drivers | Low |  | implicit | `apps/hotaru/crates/hotaru-io/tests/clipboard.rs:22` |
| HTT-02 | (test harness) SIGPIPE child re-exec and signal-mask fixtures | Low |  | implicit | `apps/hotaru/crates/hotaru-io/tests/clipboard.rs:117` |
| HTT-03 | (test harness) PTY child lifecycle waits | Low |  | implicit | `apps/hotaru/crates/hotaru-io/tests/pty.rs:12` |
| HTT-04 | (test harness) Shell-startup PTY driver (parent/child re-exec) | Low |  | implicit | `apps/hotaru/crates/hotaru-io/tests/shell.rs:190` |
| HTT-05 | (dev tool) Shared workstation measurement lock | Medium | [unverified](implicit-state-machines/reproductions.md#htt-05) | implicit | `apps/hotaru/tools/benchmark.py:48` |
| HTT-06 | (dev tool) Validated alternating benchmark round loops | Low |  | implicit | `apps/hotaru/tools/benchmark.py:125` |
| HTT-07 | (dev tool) Build provenance metadata gate (`build_confirmed`, hashes) | Medium | [unverified](implicit-state-machines/reproductions.md#htt-07) | implicit | `apps/hotaru/tools/build-candidate.py:83` |
| HTT-08 | (dev tool) libghostty-vt ctypes terminal session lifecycle | Low |  | implicit | `apps/hotaru/tools/differential.py:179` |
| HTT-09 | (dev tool) Frame benchmark PTY fixture protocol | Medium |  | implicit | `apps/hotaru/tools/frame-benchmark.py:105` |
| [HTT-10](implicit-state-machines/hotaru-tests-tools.md#htt-10) | (dev tool) Frame benchmark controller run lifecycle | High | [confirmed](implicit-state-machines/reproductions.md#htt-10) | implicit | `apps/hotaru/tools/frame-benchmark.py:362` |
| HTT-11 | (dev tool) JSON event reader thread and `until` waits | Low |  | implicit | `apps/hotaru/tools/frame-benchmark.py:206` |
| HTT-12 | (dev tool) Frame qualification → calibration → matrix → summary pipeline | Medium |  | implicit | `apps/hotaru/tools/frame-calibration.py:22` |
| HTT-13 | (test harness) Frame fixture and observer qualification drivers | Low |  | implicit | `apps/hotaru/tools/frame-harness-check.py:35` |
| [HTT-14](implicit-state-machines/hotaru-tests-tools.md#htt-14) | (dev tool) Frame matrix batch status and trial acceptance | High | [confirmed](implicit-state-machines/reproductions.md#htt-14) | implicit | `apps/hotaru/tools/frame-matrix.py:312` |
| HTT-15 | (dev tool) Frame matrix trial process lifecycle and orphan cleanup | Medium |  | implicit | `apps/hotaru/tools/frame-matrix.py:216` |
| HTT-16 | (dev tool) Frame matrix preflight quiet-wait retry loop | Low |  | implicit | `apps/hotaru/tools/frame-matrix.py:158` |
| HTT-17 | (dev tool) Frame observer screencopy capture cycle | Medium |  | implicit | `apps/hotaru/tools/frame-observer.c:39` |
| HTT-18 | (dev tool) Frame observer marker geometry cache | Low | [unverified](implicit-state-machines/reproductions.md#htt-18) | implicit | `apps/hotaru/tools/frame-observer.c:41` |
| HTT-19 | (dev tool) GPU buffer qualification build/run variants | Low |  | implicit | `apps/hotaru/tools/gpu-buffer-check.py:88` |
| HTT-20 | (dev tool) EGL offscreen probe stage and current-context sequence | Low |  | implicit | `apps/hotaru/tools/gpu-buffer-probe.c:86` |
| HTT-21 | (dev tool) GUI probe private session and DSR fixture | Low |  | implicit | `apps/hotaru/tools/gui-probe.py:114` |
| HTT-22 | (dev tool) Input oracle shared encoder/event handles across cases | Low |  | implicit | `apps/hotaru/tools/input-oracle.py:72` |
| HTT-23 | (dev tool) Upstream port-ledger per-file status | Low |  | implicit | `apps/hotaru/tools/inventory.py:44` |
| HTT-24 | (dev tool) Kitty clipboard oracle scripted embedder | Low |  | implicit | `apps/hotaru/tools/kitty-clipboard-oracle.py:350` |
| HTT-25 | (dev tool) Process-timing run watchdog | Low |  | implicit | `apps/hotaru/tools/measure.py:67` |
| HTT-26 | (dev tool) Pinned reference oracle prepare/build/metadata stages | Medium |  | implicit | `apps/hotaru/tools/reference.py:39` |
| HTT-27 | (dev tool) Two-phase differential report → compact fixture generators | Low |  | implicit | `apps/hotaru/tools/stream-parser-oracle.py:172` |
| HTT-28 | (dev tool) print_utf8 experiment variants (run accumulator, pending wrap) | Low |  | implicit | `apps/hotaru/tools/unicode-experiment.py:63` |

#### [GEN: Genkan installer](implicit-state-machines/genkan.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| GEN-01 | (test harness) Encrypted-storage and boot-splash VM drivers | Low |  | implicit | `installer/genkan/nix/encrypted-splash-test.nix:162` |
| GEN-02 | Generated initrd unlock sequence (root first, swap with root's passphrase, no device timeout) | Low |  | implicit | `installer/genkan/rust/src/config.rs:263` |
| [GEN-03](implicit-state-machines/genkan.md#gen-03) | Selected-disk identity: discovered → reviewed → revalidated before and after the first write | High | [confirmed](implicit-state-machines/reproductions.md#gen-03) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) | `installer/genkan/rust/src/disk.rs:19` |
| GEN-04 | LUKS mapping registry and its cleanup order | Medium |  | implicit | `installer/genkan/rust/src/encryption.rs:38` |
| GEN-05 | (test harness) Loop-device image guard in the reformat matrix | Low |  | implicit | `installer/genkan/rust/src/filesystem.rs:624` |
| [GEN-06](implicit-state-machines/genkan.md#gen-06) | Privileged helper session: request → preparation → confirmation or cancel → execution | High |  | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) | `installer/genkan/rust/src/helper.rs:24` |
| [GEN-07](implicit-state-machines/genkan.md#gen-07) | Install transaction: non-destructive preparation stages, then destructive steps 1–6 | High | [confirmed](implicit-state-machines/reproductions.md#gen-07) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) | `installer/genkan/rust/src/install.rs:267` |
| GEN-08 | Install target mount guard | Low |  | implicit | `installer/genkan/rust/src/install.rs:234` |
| GEN-09 | Installer exclusivity lock and the partition-table disk lock | Low |  | implicit | `installer/genkan/rust/src/install.rs:286` |
| GEN-10 | (test harness) Encrypted storage matrix in the disposable VM | Low |  | implicit | `installer/genkan/rust/src/install.rs:994` |
| GEN-11 | Nix internal-json activity tracker and copy-progress throttle | Low |  | implicit | `installer/genkan/rust/src/nixlog.rs:105` |
| GEN-12 | Sleep/idle inhibitor during installation | Low |  | implicit | `installer/genkan/rust/src/power.rs:13` |
| GEN-13 | Page-cache warming worker pool | Medium |  | implicit | `installer/genkan/rust/src/precache.rs:147` |
| GEN-14 | Subprocess run loop: spawn, drain, poll for deadline or cancel, kill the group | Low |  | implicit | `installer/genkan/rust/src/process.rs:115` |
| GEN-15 | GUI-side helper Session handle (pkexec child, writer and reader threads) | Medium |  | implicit | `installer/genkan/rust/src/session.rs:56` |
| GEN-16 | Plymouth: unanswered prompts per client connection, dropped when the client hangs up (patch) | Low |  | implicit | `installer/genkan/rust/system/boot-splash/plymouth/abandoned-prompts.patch:77` |
| GEN-17 | Plymouth: ownership of the local console terminal across renderer takeover (patch) | Low |  | implicit | `installer/genkan/rust/system/boot-splash/plymouth/console-owner.patch:19` |
| GEN-18 | Plymouth: which keyboard the splash reads, across keyboard removal (patch) | Low |  | implicit | `installer/genkan/rust/system/boot-splash/plymouth/keyboard-removal.patch:85` |
| GEN-19 | Sekka splash theme: rail, prompt and hint phases | Medium |  | implicit | `installer/genkan/rust/system/boot-splash/sekka.script:39` |
| GEN-20 | Aizome IBus frontend service: exit classification at logout and restart | Low |  | implicit | `installer/genkan/rust/system/input-method.nix:29` |
| GEN-21 | User-manager environment record and per-login reset | Low |  | implicit | `installer/genkan/rust/system/session-env/src/main.rs:25` |
| GEN-22 | USB keyboard and mouse wake path enablement | Low |  | implicit | `installer/genkan/rust/system/usb-wake.nix:22` |
| GEN-23 | wpa_supplicant pending scan-result handler left behind by a disabled interface (patch) | Low |  | implicit | `installer/genkan/rust/system/wpa-supplicant/clear-scan-res-handler.patch:38` |
| GEN-24 | (test harness) VM fixture's helper-session driver | Low |  | implicit | `installer/genkan/tests/vm-fixture/src/main.rs:52` |
| GEN-25 | (test harness) VM fixture stages and the PAM pty conversation | Low |  | implicit | `installer/genkan/tests/vm-fixture/src/main.rs:502` |
| GEN-26 | Wizard navigation and page locking | Medium |  | implicit | `installer/genkan/ui/src/app.rs:484` |
| GEN-27 | Bottom-bar activity slot and timed notes | Low |  | implicit | `installer/genkan/ui/src/app.rs:486` |
| [GEN-28](implicit-state-machines/genkan.md#gen-28) | Disk scan, selection and installed-memory read | High | [confirmed](implicit-state-machines/reproductions.md#gen-28) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) | `installer/genkan/ui/src/app.rs:496` |
| GEN-29 | Derived fields that follow until edited (username, Git name, Git email) | Low |  | implicit | `installer/genkan/ui/src/app.rs:507` |
| GEN-30 | GitHub account lookup | Medium |  | implicit | `installer/genkan/ui/src/app.rs:515` |
| GEN-31 | Time zone detection versus manual choice | Medium |  | implicit | `installer/genkan/ui/src/app.rs:544` |
| GEN-32 | World-map hover and click-to-pick | Low |  | implicit | `installer/genkan/ui/src/app.rs:545` |
| [GEN-33](implicit-state-machines/genkan.md#gen-33) | Keyboard step: catching the key beside the space bar through the compositor | High | [partly](implicit-state-machines/reproductions.md#gen-33) | implicit | `installer/genkan/ui/src/app.rs:560` |
| [GEN-34](implicit-state-machines/genkan.md#gen-34) | Review, background preparation and confirmation (GUI side of the helper session) | High | [confirmed](implicit-state-machines/reproductions.md#gen-34) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) | `installer/genkan/ui/src/app.rs:569` |
| [GEN-35](implicit-state-machines/genkan.md#gen-35) | Installation progress and outcome on the Install page | High |  | implicit | `installer/genkan/ui/src/app.rs:580` |
| GEN-36 | Speculative page-cache warming of the current selection | Medium |  | implicit | `installer/genkan/ui/src/app.rs:598` |
| GEN-37 | (test harness) Preview and sample-data driver | Low |  | implicit | `installer/genkan/ui/src/preview.rs:61` |

#### [TTL: Tatami command, display watcher and configuration](implicit-state-machines/tatami-tool.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| TTL-01 | Mako Do Not Disturb mode | Low |  | implicit | `desktop/tatami/config/mako/config:29` |
| TTL-02 | Tatami session unit lifecycle and restart policies | Low |  | implicit | `desktop/tatami/module.nix:206` |
| [TTL-03](implicit-state-machines/tatami-tool.md#ttl-03) | Keep Awake: intent marker, inhibitor holder unit and verification | High | [confirmed](implicit-state-machines/reproductions.md#ttl-03) | implicit | `desktop/tatami/tool/src/awake.rs:13` |
| [TTL-04](implicit-state-machines/tatami-tool.md#ttl-04) | Wallpaper choice and per-output overrides | High | [confirmed](implicit-state-machines/reproductions.md#ttl-04) | implicit | `desktop/tatami/tool/src/background.rs:160` |
| TTL-05 | Style › Background settings menu flow | Low |  | implicit | `desktop/tatami/tool/src/background.rs:355` |
| TTL-06 | Screenshot selection and screen freeze | Medium |  | implicit | `desktop/tatami/tool/src/capture.rs:99` |
| TTL-07 | Screenshot Edit notification action | Low |  | implicit | `desktop/tatami/tool/src/capture.rs:172` |
| [TTL-08](implicit-state-machines/tatami-tool.md#ttl-08) | Screen recording start, fallback and stop | High | [confirmed](implicit-state-machines/reproductions.md#ttl-08) | implicit | `desktop/tatami/tool/src/capture.rs:534` |
| [TTL-09](implicit-state-machines/tatami-tool.md#ttl-09) | Display session record: closed-display mode, lid edges, remembered off choices | High |  | implicit | `desktop/tatami/tool/src/display_policy.rs:95` |
| [TTL-10](implicit-state-machines/tatami-tool.md#ttl-10) | Display reconciliation commit (plan execution) | High | [confirmed](implicit-state-machines/reproductions.md#ttl-10) | implicit | `desktop/tatami/tool/src/display_recovery.rs:386` |
| TTL-11 | Display watcher event loop | Medium |  | implicit | `desktop/tatami/tool/src/display_recovery.rs:673` |
| TTL-12 | Lid state tracker (bindings plus logind poll) | Medium |  | implicit | `desktop/tatami/tool/src/display_recovery.rs:546` |
| TTL-13 | XWayland Xft.dpi retry | Medium |  | implicit | `desktop/tatami/tool/src/display_recovery.rs:586` |
| TTL-14 | Window rescue and settings-window fit grace periods | Low |  | implicit | `desktop/tatami/tool/src/display_recovery.rs:687` |
| TTL-15 | Displays apply transaction | Medium |  | implicit | `desktop/tatami/tool/src/displays.rs:417` |
| TTL-16 | Display settings store and legacy scale migration | Low |  | implicit | `desktop/tatami/tool/src/displays.rs:262` |
| [TTL-17](implicit-state-machines/tatami-tool.md#ttl-17) | Live reload pipeline (`tatami reload`, `update-session`) | High | [confirmed](implicit-state-machines/reproductions.md#ttl-17) | implicit | `desktop/tatami/tool/src/main.rs:165` |
| TTL-18 | Walker menu open/close handoff | Low |  | implicit | `desktop/tatami/tool/src/menu.rs:113` |
| [TTL-19](implicit-state-machines/tatami-tool.md#ttl-19) | Wi-Fi join flow | Low |  | implicit | `desktop/tatami/tool/src/network.rs:621` |
| TTL-20 | Lock, idle and DPMS | Low |  | implicit | `desktop/tatami/tool/src/power.rs:6` |
| TTL-21 | Power meter socket service and reading throttle | Low |  | implicit | `desktop/tatami/tool/src/power_meter.rs:79` |
| TTL-22 | Mixed-audio recorder children (SessionChild, AudioMix) | Medium |  | implicit | `desktop/tatami/tool/src/recording_audio.rs:14` |
| TTL-23 | Night Light (hyprsunset) | Medium |  | implicit | `desktop/tatami/tool/src/toggle.rs:8` |
| TTL-24 | Session update receipt | Medium |  | implicit | `desktop/tatami/tool/src/update.rs:9` |
| [TTL-25](implicit-state-machines/tatami-tool.md#ttl-25) | Weather report cache and fetch coordination | High | [confirmed](implicit-state-machines/reproductions.md#ttl-25) | implicit | `desktop/tatami/tool/src/weather.rs:50` |
| [TTL-26](implicit-state-machines/tatami-tool.md#ttl-26) | Session monitor state files (monitors-off, monitors.lua) | High |  | implicit | `desktop/tatami/tool/src/window.rs:291` |
| TTL-27 | Remembered Hyprland toggles | Low |  | implicit | `desktop/tatami/tool/src/window.rs:16` |
| TTL-28 | Displays window presentation and replacement | Medium |  | implicit | `desktop/tatami/tool/src/window.rs:560` |
| TTL-29 | Pop-out window toggle | Low |  | implicit | `desktop/tatami/tool/src/window.rs:196` |

#### [TAP: Tatami settings apps](implicit-state-machines/tatami-apps.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| TAP-01 | Activity inspector's subscription to the selected row, with a layout shape fixed at selection | Low |  | implicit | `desktop/tatami/activity/src/inspector.rs:80` |
| TAP-02 | Activity sampler thread lifecycle and schedule | Low |  | implicit | `desktop/tatami/activity/src/sampler.rs:34` |
| [TAP-03](implicit-state-machines/tatami-apps.md#tap-03) | Activity sampler's baselines, caches and energy integrals between readings | High | [confirmed](implicit-state-machines/reproductions.md#tap-03) | implicit | `desktop/tatami/activity/src/sampler.rs:114-132` |
| [TAP-04](implicit-state-machines/tatami-apps.md#tap-04) | Activity process table: columns per view and remembered sorts | High | [confirmed](implicit-state-machines/reproductions.md#tap-04) | implicit | `desktop/tatami/activity/src/table.rs:304` |
| TAP-05 | Activity table cells bound to a row's `changed` signal | Low |  | implicit | `desktop/tatami/activity/src/table.rs:322` |
| [TAP-06](implicit-state-machines/tatami-apps.md#tap-06) | Window singleton, deferred first presentation and launch requests held until the first reading (Activity, Network, Sound) | High | [confirmed](implicit-state-machines/reproductions.md#tap-06) | implicit | `desktop/tatami/activity/src/window.rs:65` |
| TAP-07 | Activity's view and grouping mode, held in seven places | Medium |  | implicit | `desktop/tatami/activity/src/window.rs:31-32` |
| [TAP-08](implicit-state-machines/tatami-apps.md#tap-08) | Activity's Quit, Force Quit and Pause on a target taken from the latest frame | High | [confirmed](implicit-state-machines/reproductions.md#tap-08) | implicit | `desktop/tatami/activity/src/window.rs:696` |
| TAP-09 | Bluetooth pairing: the Pair call, the Agent1 prompt and cancellation | Medium |  | implicit | `desktop/tatami/bluetooth/src/agent.rs:29` |
| TAP-10 | BlueZ backend session tied to the service's bus owner, with agent registration | Medium |  | implicit | `desktop/tatami/bluetooth/src/bluez.rs:16` |
| TAP-11 | Bluetooth discovery session owned by this connection | Medium |  | implicit | `desktop/tatami/bluetooth/src/bluez.rs:21` |
| TAP-12 | (test harness) Fake BlueZ on a private dbus-daemon | Low |  | implicit | `desktop/tatami/bluetooth/src/ui_test.rs:96` |
| [TAP-13](implicit-state-machines/tatami-apps.md#tap-13) | Bluetooth readings and actions serialised with a revision counter | High |  | implicit | `desktop/tatami/bluetooth/src/window.rs:40-44` |
| TAP-14 | Bluetooth device settings dialog | Low |  | implicit | `desktop/tatami/bluetooth/src/window.rs:39` |
| [TAP-15](implicit-state-machines/tatami-apps.md#tap-15) | Bluetooth window close: hide, drain, release, destroy | High | [confirmed](implicit-state-machines/reproductions.md#tap-15) | implicit | `desktop/tatami/bluetooth/src/window.rs:43` |
| TAP-16 | Displays arrangement tile drag | Medium |  | implicit | `desktop/tatami/displays/src/arrangement.rs:31` |
| TAP-17 | A display's use (main, extended, mirror, off) and the main display | Low |  | implicit | `desktop/tatami/displays/src/model.rs:528` |
| TAP-18 | Displays' Hyprland event-socket watcher thread | Low |  | implicit | `desktop/tatami/displays/src/system.rs:208` |
| [TAP-19](implicit-state-machines/tatami-apps.md#tap-19) | Displays readings and applies (concurrent changes refused, stale readings discarded) | High | [confirmed](implicit-state-machines/reproductions.md#tap-19) | implicit | `desktop/tatami/displays/src/window.rs:73-75` |
| TAP-20 | Keep-or-revert countdown after a risky display change | Medium |  | implicit | `desktop/tatami/displays/src/window.rs:391` |
| [TAP-21](implicit-state-machines/tatami-apps.md#tap-21) | Night Shift requests on unordered threads | High | [confirmed](implicit-state-machines/reproductions.md#tap-21) | implicit | `desktop/tatami/displays/src/window.rs:63` |
| TAP-22 | Displays' debounced slider writes | Low |  | implicit | `desktop/tatami/displays/src/window.rs:1077` |
| TAP-23 | Network connection details page: the open page, its settings and their version | Medium |  | implicit | `desktop/tatami/network/src/details.rs:30` |
| TAP-24 | IPv6 DNS servers suspended in the NetworkManager profile while IPv6 is off | Medium |  | implicit | `desktop/tatami/network/src/dns.rs:7` |
| [TAP-25](implicit-state-machines/tatami-apps.md#tap-25) | Joining a Wi-Fi network: one at a time, with a password retry and rollback | High | [confirmed](implicit-state-machines/reproductions.md#tap-25) | implicit, bug fixed ([#302](https://github.com/bitemyapp/sekka/pull/302)) | `desktop/tatami/network/src/join.rs:40` |
| TAP-26 | Network's `nmcli monitor` child process | Low |  | implicit | `desktop/tatami/network/src/nm.rs:371` |
| TAP-27 | Network readings: one at a time with one queued, change bursts and the poll | Low |  | implicit | `desktop/tatami/network/src/window.rs:91-92` |
| TAP-28 | Network's Wi-Fi scan with a fixed six-second spinner | Low |  | implicit | `desktop/tatami/network/src/window.rs:399` |
| TAP-29 | Network's Wi-Fi radio and VPN switches, dispatched without serialisation | Medium |  | implicit | `desktop/tatami/network/src/window.rs:35` |
| TAP-30 | Sound's `pactl subscribe` with a restart loop | Low |  | implicit | `desktop/tatami/sound/src/audio.rs:166` |
| TAP-31 | Sound's input level meter: the libpulse thread and its follower | Medium |  | implicit | `desktop/tatami/sound/src/meter.rs:28` |
| TAP-32 | Sound's serial change queue and reading scheduler | Medium |  | implicit | `desktop/tatami/sound/src/window.rs:120-124` |
| TAP-33 | Section rows cached by a content key and updated by bindings (Sound, Network) | Low |  | implicit | `desktop/tatami/sound/src/window.rs:44` |

#### [KTL: Kakehashi, respin tools and scripts](implicit-state-machines/kakehashi-tools.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| KTL-01 | Kakehashi image selection and checksum check | Medium |  | implicit | `apps/kakehashi/app/src/view.rs:75` |
| [KTL-02](implicit-state-machines/kakehashi-tools.md#ktl-02) | Kakehashi nightly download session | High | [confirmed](implicit-state-machines/reproductions.md#ktl-02) | implicit | `apps/kakehashi/app/src/view.rs:85` |
| [KTL-03](implicit-state-machines/kakehashi-tools.md#ktl-03) | Kakehashi write job, GUI side of the helper protocol | High | [confirmed](implicit-state-machines/reproductions.md#ktl-03) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) | `apps/kakehashi/app/src/view.rs:100` |
| [KTL-04](implicit-state-machines/kakehashi-tools.md#ktl-04) | Kakehashi drive list polling and chosen drive | High | [confirmed](implicit-state-machines/reproductions.md#ktl-04) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) | `apps/kakehashi/app/src/view.rs:117` |
| KTL-05 | Resumable nightly download on disk | Medium |  | implicit | `apps/kakehashi/core/src/nightly.rs:90` |
| KTL-06 | Adaptive range-connection controller | Low |  | implicit | `apps/kakehashi/core/src/nightly/parallel.rs:23` |
| KTL-07 | Range-support mode: probe, sequential fallback, recheck | Medium |  | implicit | `apps/kakehashi/core/src/nightly/parallel.rs:149` |
| KTL-08 | Ordered range-commit pipeline with per-range retry | Medium |  | implicit | `apps/kakehashi/core/src/nightly/parallel.rs:256` |
| KTL-09 | (test harness) Fault-injecting range HTTP server | Low |  | implicit | `apps/kakehashi/core/src/nightly/parallel_tests.rs:30` |
| KTL-10 | macOS authopen descriptor broker | Low |  | implicit | `apps/kakehashi/core/src/platform/macos/authopen.rs:28` |
| KTL-11 | Windows volume lock, dismount and hold | Medium |  | implicit | `apps/kakehashi/core/src/platform/windows.rs:36` |
| [KTL-12](implicit-state-machines/kakehashi-tools.md#ktl-12) | Helper write/verify pipeline and `touched` | High | [environment](implicit-state-machines/reproductions.md#ktl-12) | implicit | `apps/kakehashi/core/src/write.rs:171` |
| [KTL-13](implicit-state-machines/kakehashi-tools.md#ktl-13) | Rootless builder VM host driver | High | [confirmed](implicit-state-machines/reproductions.md#ktl-13) | implicit | `rust/src/builder.rs:61` |
| KTL-14 | Builder guest disk and store preparation markers | Medium |  | implicit | `rust/src/builder.rs:327` |
| KTL-15 | Wi-Fi test tools NAR cache | Low |  | implicit | `rust/src/builder.rs:459` |
| KTL-16 | (test harness) Live boot and input-delivery test phases | Low |  | implicit | `rust/src/live_test.rs:85` |
| KTL-17 | Child supervision and interrupt latch | Medium |  | implicit | `rust/src/support.rs:17` |
| KTL-18 | (test harness) QMP/QGA request correlation and guest-exec polling | Low |  | implicit | `rust/src/vm.rs:16` |
| KTL-19 | (test harness) QEMU VM handle lifecycle | Medium |  | implicit | `rust/src/vm.rs:84` |
| [KTL-20](implicit-state-machines/kakehashi-tools.md#ktl-20) | (test harness) qemu-test install/reboot run and resumable evidence | High | [partly](implicit-state-machines/reproductions.md#ktl-20) | implicit | `rust/src/vm.rs:759` |
| KTL-21 | (test harness) Wi-Fi test: host boot and guest simulated-AP lifecycle | Low |  | implicit | `rust/src/wifi_test.rs:163` |
| KTL-22 | PR trust gate merge-commit polling | Low |  | implicit | `scripts/check-pr-trust.py:45` |
| KTL-23 | CI GC-root generation retention | Medium |  | implicit | `scripts/ci-retain.py:56` |
| [KTL-24](implicit-state-machines/kakehashi-tools.md#ktl-24) | flash_usb privileged write and verify step | High | [environment](implicit-state-machines/reproductions.md#ktl-24) | implicit | `scripts/flash_usb.rs:619` |
| KTL-25 | flash_usb interactive selection and sudo hand-off | Low |  | implicit | `scripts/flash_usb.rs:770` |
| KTL-26 | Live diagnostics bounded command capture | Low |  | implicit | `scripts/live_diagnostics.rs:11` |
| KTL-27 | Release asset staging directory | Low |  | implicit | `scripts/prepare-release.py:19` |
| KTL-28 | Qualification report review and audit gate | Medium |  | implicit | `scripts/publish-nightly.py:137` |
| [KTL-29](implicit-state-machines/kakehashi-tools.md#ktl-29) | Nightly publication to R2 | High |  | implicit | `scripts/publish-nightly.py:300` |
| KTL-30 | CI Git object cache refresh and seed | Low |  | implicit | `scripts/runner-git-cache.sh:6` |
| KTL-31 | Runner host setup convergence and KVM access migration | Low |  | implicit | `scripts/runner-host-setup.sh:34` |

#### [INX: Nix integration, carried patches and live-media modules](implicit-state-machines/infra-nix.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| INX-01 | (test harness) Deadline-poll helpers and compositor-readiness waits shared by the VM test scripts | Low |  | implicit | `nix/activity-vm-test.nix:124` |
| INX-02 | (test harness) Activity: stand-in power meter and pause/resume probes | Low |  | implicit | `nix/activity-vm-test.nix:14` |
| INX-03 | (test harness) Greeter session cycle with remembered last session | Low |  | implicit | `nix/aizome-greeter-desktops-vm-test.nix:35` |
| INX-04 | (test harness) AccountsService session seed, readiness window and activation fixtures | Low |  | implicit | `nix/aizome-ibus-test.nix:51` |
| INX-05 | (test harness) Aizome login → session → logout environment lifecycle | Low |  | implicit | `nix/aizome-login-vm-test.nix:95` |
| INX-06 | (test harness) Wallpaper menu-panel driver with reject-keeps-state invariant | Low |  | implicit | `nix/background-validation-vm-test.nix:97` |
| INX-07 | (test harness) Boot-splash hold gate and Plymouth mode driver | Low |  | implicit | `nix/boot-splash-test.nix:25` |
| INX-08 | (test harness) Capture-cursor compositor driver | Low |  | implicit | `nix/capture-cursor-vm-test.nix:73` |
| INX-09 | ChatGPT executor plugin copy serialized per destination | Low |  | implicit | `nix/chatgpt/copy-plugin.cjs:5` |
| INX-10 | (test harness) CI GC-root retention driver | Low |  | implicit | `nix/ci-cache-test.nix:77` |
| INX-11 | (test harness) Display recovery: fake lid and dock/undock/lid driver | Low |  | implicit | `nix/display-recovery-vm-test.nix:17` |
| INX-12 | (test harness) Genkan keyboard-step driver over the kit's inspection files | Low |  | implicit | `nix/genkan-keyboard-test.py:91` |
| INX-13 | (test harness) Live-media guard mount-state fixture | Low |  | implicit | `nix/genkan-live-media-test.nix:82` |
| INX-14 | (test harness) Genkan helper preflight stage boundary | Low |  | implicit | `nix/genkan-preflight-test.nix:71` |
| INX-15 | Googlebook vboot kernel-partition attempt attributes | Low |  | implicit | `nix/googlebook-iso.nix:35` |
| INX-16 | (test harness) Hyprlauncher open/cancel/reopen with a resident daemon | Low |  | implicit | `nix/hyprlauncher-software-test.nix:138` |
| INX-17 | Installed flake lock: as-installed pin, then following `nightly` | Low |  | implicit | `nix/installed-lock.nix:26` |
| [INX-18](implicit-state-machines/infra-nix.md#inx-18) | Copy-to-RAM of the live media in the initrd | High | [partly](implicit-state-machines/reproductions.md#inx-18) | implicit | `nix/iso-copytoram.nix:42` |
| INX-19 | (test harness) Kakehashi helper write through pkexec with progress and cancel files | Low |  | implicit | `nix/kakehashi-vm-test.nix:74` |
| INX-20 | (test harness) Keep-awake inhibitor lifecycle | Low |  | implicit | `nix/keep-awake-test.nix:38` |
| INX-21 | (test harness) Network DNS/IPv6 page driver with settle detector and mirrored UI mode | Low |  | implicit | `nix/network-dns-vm-test.nix:82` |
| INX-22 | (test harness) Wi-Fi join flows over simulated access points | Low |  | implicit | `nix/network-vm-test.nix:96` |
| INX-23 | Aquamarine DRM async-commit-event flush during teardown (patched) | Low |  | implicit | `nix/patches/aquamarine-null-connector.patch:16` |
| INX-24 | Avahi startup SIGHUP window (patched) | Low |  | implicit | `nix/patches/avahi-startup-sighup.patch:11` |
| INX-25 | Hyprland cursor-free capture image and its per-frame zoom snapshot (patched) | Medium |  | implicit | `nix/patches/hyprland-capture-cursor-0.57.patch:116` |
| [INX-26](implicit-state-machines/infra-nix.md#inx-26) | Hyprland managed screencopy session lifecycle (patched) | High |  | implicit | `nix/patches/hyprland-screenshare-sessions-0.57.patch:173` |
| INX-27 | Hyprtoolkit window buffer allocation fallback ladder (patched) | Medium |  | implicit | `nix/patches/hyprtoolkit-buffer-allocation.patch:44` |
| INX-28 | IBus Wayland input-method commit serial (patched) | Low |  | implicit | `nix/patches/ibus-wayland-serial.patch:17` |
| [INX-29](implicit-state-machines/infra-nix.md#inx-29) | IBus Wayland frontend keymaps, seat and system-keymap mode (patched) | High | [partly](implicit-state-machines/reproductions.md#inx-29) | implicit | `nix/patches/ibus-wayland-system-keymap.patch:68` |
| INX-30 | PipeWire X11 bell module display/source teardown (patched) | Low |  | implicit | `nix/patches/pipewire-x11-bell-disconnect.patch:10` |
| INX-31 | (test harness) Uta MPRIS test waits for bus-name ownership before its caching proxy | Low |  | implicit | `nix/patches/uta-mpris-readiness.patch:8` |
| INX-32 | XApp portal backend bound to the graphical session (patched unit) | Low |  | implicit | `nix/patches/xapp-session-lifecycle.patch:7` |
| INX-33 | XApp Settings theme-change notification coalesced to idle (patched) | Low |  | implicit | `nix/patches/xapp-settings-theme-idle.patch:12` |
| [INX-34](implicit-state-machines/infra-nix.md#inx-34) | Binary-cache publication pipeline | High | [confirmed](implicit-state-machines/reproductions.md#inx-34) | implicit | `nix/push-cache.nix:52` |
| INX-35 | (test harness) Runner cancellation-order fixture | Low |  | implicit | `nix/runner-protocol-test.nix:9` |
| [INX-36](implicit-state-machines/infra-nix.md#inx-36) | Self-hosted runner supervisor and per-job admission | High | [confirmed](implicit-state-machines/reproductions.md#inx-36) | implicit | `nix/runner.nix:157` |
| INX-37 | AI-harness and background-job cgroup placement | Medium |  | implicit | `nix/scheduling/agent-workloads.py:142` |
| INX-38 | ananicy-cpp process-event classification (patched) | Medium |  | implicit | `nix/scheduling/ananicy-processes.patch:20` |
| INX-39 | (test harness) Screenshare session counting driver | Low |  | implicit | `nix/screenshare-sessions-vm-test.nix:81` |
| INX-40 | (test harness) One-shot screenshot notification action owner | Low |  | implicit | `nix/screenshot-notifications-vm-test.nix:92` |
| INX-41 | (test harness) Simulated Wi-Fi access-point bring-up | Low |  | implicit | `nix/simulated-wifi.nix:82` |
| INX-42 | (test harness) Tatami screen recording lifecycle with marker-file fault injection | Low |  | implicit | `nix/tatami-recording-test.nix:86` |
| INX-43 | (test harness) Tuning process-tree stand-ins with nice-value handshake | Low |  | implicit | `nix/tuning-test.nix:10` |
| INX-44 | (test harness) Yukimi update completion: activation survives its client, switch vs boot | Low |  | implicit | `nix/update-completion-test.nix:86` |
| INX-45 | (test harness) wpa_supplicant dropped-scan reproduction | Low |  | implicit | `nix/wpa-supplicant-scan-test.nix:79` |
| INX-46 | (test harness) Yukimi under X11: nested compositor and single-instance lifecycle | Low |  | implicit | `nix/yukimi-x11-test.nix:63` |

#### [ICI: CI workflows, websites and repository tests](implicit-state-machines/infra-ci-tests.md)

| ID | Machine | Reify | Bug | Follow-up | Where |
| --- | --- | --- | --- | --- | --- |
| ICI-01 | Main-branch cache publication run (coalesced queue → checks → binary cache) | Low |  | implicit | `.github/workflows/cache.yml:16` |
| [ICI-02](implicit-state-machines/infra-ci-tests.md#ici-02) | Nightly input update and main/nightly branch promotion | High |  | implicit | `.github/workflows/nightly.yml:12` |
| [ICI-03](implicit-state-machines/infra-ci-tests.md#ici-03) | Trusted PR admission, recheck and commit-status lifecycle | High | [confirmed](implicit-state-machines/reproductions.md#ici-03) | implicit | `.github/workflows/pr.yml:19` |
| ICI-04 | Manual release: tag → draft prerelease → published | Medium |  | implicit | `.github/workflows/release.yml:14` |
| ICI-05 | Per-site publish decision keyed on the revision the live site serves | Medium |  | implicit | `.github/workflows/sites.yml:89` |
| ICI-06 | Night/Day theme switch remembered in localStorage | Low | [unverified](implicit-state-machines/reproductions.md#ici-06) | implicit | `aizome-site/theme.js:7` |
| ICI-07 | Plymouth local-console ownership (carried patch) | Medium |  | implicit | `docs/upstream/plymouth/console-owner.patch:33` |
| ICI-08 | Plymouth splash keyboard binding across renderer replacement (carried patch) | Medium |  | implicit | `docs/upstream/plymouth/keyboard-removal.patch:51` |
| ICI-09 | Download page's nightly build list: edge-cached index with fallback | Low |  | implicit | `site/download-worker/src/lib.rs:63` |
| ICI-10 | Home-page video asset loading (lazy, single-flight, retried after failure) | Low |  | implicit | `site/video.js:22` |
| [ICI-11](implicit-state-machines/infra-ci-tests.md#ici-11) | Video playback state (poster/loading/playing/paused/ended/failed) with seek-drag resume | High | [confirmed](implicit-state-machines/reproductions.md#ici-11) | implicit | `site/video.js:183` |
| ICI-12 | Player controls auto-hide and touch-tap gating | Low |  | implicit | `site/video.js:315` |
| ICI-13 | Mute and volume level | Low |  | implicit | `site/video.js:31` |
| ICI-14 | wasm-bindgen glue: one-shot instance and closure refcounts (generated) | Low |  | implicit | `site/video/hype.js:935` |
| ICI-15 | (test harness) IBus session-switch driver: Aizome → Xfce → GNOME → Aizome | Low |  | implicit | `tests/aizome-ibus.py:27` |
| ICI-16 | (test harness) gdb-scripted Avahi startup signal window | Low |  | implicit | `tests/check_avahi_startup.py:16` |
| ICI-17 | (test harness) Spawn → wait-ready → inject → tear-down daemon drivers | Low |  | implicit | `tests/check_pipewire_x11_disconnect.py:43` |
| ICI-18 | (test harness) Chrome IBus probe: CDP request ids and a held-modifier tab cycle | Low |  | implicit | `tests/ibus-chrome.py:48` |
| ICI-19 | (test harness) Plasma Login greeter → session → logout driver (OCR-gated) | Low |  | implicit | `tests/plasma-login-session.py:137` |
| ICI-20 | (test harness) Responsiveness measurement: probes in systemd units, stop-file protocol | Medium | [unverified](implicit-state-machines/reproductions.md#ici-20) | implicit | `tests/responsiveness/run.sh:138` |
| ICI-21 | (test harness) wlr-screencopy / toplevel-export frame lifecycle client | Low |  | implicit | `tests/screencopy-sessions.c:37` |
| ICI-22 | (test harness) Session-environment baseline across activation and desktop switches | Low |  | implicit | `tests/session-environment.py:172` |
| ICI-23 | (test harness) FakeHyprland: stateful command and event sockets for Tatami's display recovery | Low |  | implicit | `tests/test_display_recovery.py:46` |
| ICI-24 | (test harness) RecordingR2: conditional-write object store for nightly publication | Low |  | implicit | `tests/test_publish_nightly.py:32` |
| ICI-25 | (test harness) Fake system commands steered by marker files in bwrap namespaces | Low |  | implicit | `tests/test_sekka_update.py:46` |
| ICI-26 | (test harness) Isolated PipeWire graph driving the Tatami Sound window, including a service restart | Low |  | implicit | `tests/test_sound.py:186` |
| ICI-27 | (test harness) Yukimi in Tatami: OCR-gated page walk, polkit cancel and rescale | Low |  | implicit | `tests/yukimi-tatami.py:57` |

## Suspected bugs

Auditors recorded 208 concrete failure scenarios while tracing transitions. The 176 on machines rated High were then tested at `8e0acbe5` with reproductions that assert the correct behaviour: 160 confirmed, 9 partly confirmed, 2 not confirmed, and 5 needing hardware, an operating system or a display backend that was not available. The other 32, on Medium and Low machines, are unverified leads. [Suspected bugs](implicit-state-machines/reproductions.md) lists all 208 with their status.

A reproduction that confirms a bug fails at `8e0acbe5` and passes once the
bug is fixed. The statuses are:

- **Confirmed**: the test fails at `8e0acbe5` the way the catalog entry says.
- **Partly confirmed**: part of the claim reproduces; the status says which.
- **Not confirmed**: the code behaves correctly; the status says why the
  reading was wrong.
- **Not reproduced (environment)**: the mechanism needs hardware, an
  operating system, or a display backend that was not available. The status
  says what would be needed.
- **Unverified**: not attempted.

The level in an entry's **Reproduction** field says how much of the real
program the test drives:

- `unit`: a function or state object, called directly;
- `integration`: the real crate with fakes for its services, for example a
  headless app through Aizome's kit `Harness`, a private `dbus-daemon`, or
  stub executables;
- `end-to-end`: the real program, headless or nested, such as Aizome's
  compositor through `aizome-comp`'s test harness.

Claude Code agents wrote and ran the tests, one agent per group of related
components. Nothing reached the live desktop session, a real disk or block
device, a privileged helper, logind, NetworkManager, BlueZ, polkitd or the
network. Disk tools were reproduced through their fake backends and plans,
the privileged Yukimi helper inside an unprivileged user and mount namespace
with stub Nix tools, and CI and publication scripts against stub executables.
Some tests set state directly that the real program reaches only by timing
(an in-flight counter, a late reply delivered by hand).

A second pair of agents then reviewed a stratified sample of 65 confirmed
reproductions adversarially. For each one they re-ran the test, checked that
the failing assertion tests the catalog's claim, and judged whether the
failure is reachable through real inputs. They upheld 40, upheld 23 with a
caveat and overturned 2: STA-05 is recorded as not confirmed and SYA-17 as
partly confirmed. 36 of the sampled failures were driven only by inputs a
person, client, service or timing can produce, and 27 forced state or timing
that the reviewer traced the real program reaching. 97 confirmations were not
reviewed. At the sample's rate, a few of them may also overstate what a user
would see.

The reviewers noted these limits on confirmed bugs:

- **Some consequences are bounded:**
  - STB-09: BlueZ's default 180 s `DiscoverableTimeout` ends the leak.
  - CPA-32: systemd's 30 s watchdog still restarts Aizome.
  - SHB-09: the next `WindowChanged` restores the missing window.
  - SKB-11: the glitch needs client-side decorations, which Aizome does not
    draw.
  - TAP-08: the PID would have to wrap at `pid_max`.
  - AZN-13: the override is intended; only the missing starting zone is a
    defect.
- **Some confirm behaviour that may be a design choice:** TTL-03, HAKO-09,
  KOY-04, SMT-19, SHA-02.
- **One bug, two IDs:** SHB-03 and SYB-09 reproduce the same retry loop from
  the shell side and from aizome-sys.

## Keeping this current

- When a machine becomes explicit, set its `Follow-up` in the index to
  `reified (#PR)` and update its entry, if it has one. Keep the ID, even if
  the machine disappears, so references from issues and pull requests stay
  valid.
- When a change adds a machine, number it after the slice's last ID and give
  it an index row. Give it an entry too if it is rated High.
- When a change substantially rewrites a machine, re-read it and update its
  locations in the index and in its entry; line numbers here refer to
  `8e0acbe5` until then.
- When a suspected bug is fixed, cite the fix in the index, in the entry and
  in [Suspected bugs](implicit-state-machines/reproductions.md). The test
  that reproduced it, in the history of [#282](https://github.com/bitemyapp/sekka/pull/282), can serve as the
  fix's regression test.
- A suspected bug that proves not to be a bug keeps its row, with its status
  saying why.
