# Implicit state machines

This is a catalog of the implicit state machines in Sekka, Aizome first, and
the ledger of the audit that found them. A follow-up will turn some of them
into explicit state machines. The rest stay implicit, but they are recorded
here too, so their states, transitions and hazards are known before anyone
changes the code around them.

The audit covered `main` at `8e0acbe5`. Line numbers refer to that commit.

This page holds the audit status, the index of every machine with its
follow-up status, and the suspected bugs with the outcome of reproducing
them. The code is divided into 33
slices; each has a page under [`implicit-state-machines/`](implicit-state-machines/)
with its entries, the machines that are already explicit, and a ledger
listing every file in the slice with the result of its audit.

Claude Code agents carried out the audit, one per slice, each reading every
file in its slice. All 13,432 `path:line` citations were then checked
mechanically against `8e0acbe5`: the file exists and the line is in range.
Where an identifier is named beside a citation, the check also looked for it
at the cited line or in the function enclosing it. The citations it flagged
were re-read against the code and corrected, or rewritten with full paths
where the file was ambiguous. The audit read the code; it did not build or
run it. The suspected bugs on High machines were then reproduced with tests,
as [Reproductions](implicit-state-machines/reproductions.md) describes.

## What counts

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
carrying its data, transitions in one place) are listed on each slice page
under **Already explicit**, so the audit shows they were seen.

## Reading an entry

Each entry has a stable ID (`CPA-07`: slice prefix and number) and these
fields:

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
  test written to reproduce it, linked to the test and its output in
  [Reproductions](implicit-state-machines/reproductions.md). Suspected bugs
  on Medium and Low machines were not reproduced and remain leads.
- **Related** (optional): the other side of a protocol in another component.

Entries whose name starts with `(test harness)` or `(dev tool)` are machines
in tests and development tooling. They are recorded for completeness and are
normally Low.

## Scope

In scope: every tracked first-party file with code or behaviour: Rust, C,
Python, shell, Nix, systemd and D-Bus units, Lua and other configuration that
drives behaviour, CI workflows, and the patches Sekka carries against
upstream projects. Tests and their fixtures are included.

Out of scope, and absent from the ledger:

- `apps/hotaru/upstream/`: archived experiment sources and evidence for the
  Ghostty port. They are not workspace members and nothing ships them.
- `apps/hotaru/vendor/simdutf8` and `desktop/aizome/crates/hotaru-link-regex/vendor/`
  (Oniguruma): vendored third-party code.
- Images, fonts, media fixtures, recorded logs and JSON evidence, lockfiles,
  `Cargo.toml` manifests, desktop entries, prose documentation and the
  untracked files of a working checkout.

## Audit status

Each slice has its own page with its entries and a per-file ledger. `Pending` counts files not yet audited.

| Slice | Component | Files | Lines | Pending | Machines | High | Medium | Low |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| [CPA](implicit-state-machines/comp-a.md) | Compositor core: backends, recovery, pacing and rendering | 28 | 9257 | 0 | 32 | 10 | 8 | 14 |
| [CPB](implicit-state-machines/comp-b.md) | Compositor core: state, input, focus and protocols | 31 | 10023 | 0 | 34 | 12 | 12 | 10 |
| [WM](implicit-state-machines/wm-comp-tests.md) | Window manager logic and compositor integration tests | 62 | 22613 | 0 | 38 | 9 | 8 | 21 |
| [SHA](implicit-state-machines/shell-a.md) | Shell: main loop, startup, menubar, deskbar, desktop and palette | 19 | 12122 | 0 | 26 | 7 | 9 | 10 |
| [SHB](implicit-state-machines/shell-b.md) | Shell: lock, notifications, recording, systray and services | 61 | 13838 | 0 | 30 | 7 | 7 | 16 |
| [SVC](implicit-state-machines/kit-services.md) | Applications index, media, IPC, portal and greeter | 52 | 19784 | 0 | 41 | 4 | 10 | 27 |
| [AZN](implicit-state-machines/aizome-small-nix.md) | Small crates, session launcher, Nix modules and VM tests | 114 | 18340 | 0 | 29 | 5 | 2 | 22 |
| [SYA](implicit-state-machines/sys-a.md) | System clients: launch, audio, Bluetooth, disks, MPRIS | 20 | 10434 | 0 | 19 | 5 | 9 | 5 |
| [SYB](implicit-state-machines/sys-b.md) | System clients: network, notifications, polkit, power, tray, users | 20 | 9551 | 0 | 16 | 5 | 8 | 3 |
| [SYC](implicit-state-machines/sys-c-config.md) | System clients: printers and sysinfo; config, theme and face | 55 | 21551 | 0 | 28 | 5 | 3 | 20 |
| [KIT](implicit-state-machines/kit-core.md) | Toolkit runtime | 60 | 20272 | 0 | 33 | 7 | 9 | 17 |
| [UIW](implicit-state-machines/sukumo-ui-kit-widgets.md) | Sukumo UI and toolkit widgets | 34 | 21698 | 0 | 34 | 8 | 11 | 15 |
| [SKB](implicit-state-machines/sukumo-backend.md) | Sukumo engine: Wayland client backend | 30 | 14374 | 0 | 42 | 5 | 20 | 17 |
| [SKR](implicit-state-machines/sukumo-render.md) | Sukumo engine: animation, text and raster | 30 | 18798 | 0 | 17 | 2 | 6 | 9 |
| [STA](implicit-state-machines/settei-a.md) | Settei: general, keyboard, network, sound and other panes | 29 | 12991 | 0 | 22 | 7 | 3 | 12 |
| [STB](implicit-state-machines/settei-b.md) | Settei: app shell, appearance, Bluetooth and displays | 42 | 12593 | 0 | 20 | 4 | 8 | 8 |
| [HAKO](implicit-state-machines/hako.md) | Hako file manager | 45 | 14983 | 0 | 31 | 11 | 6 | 14 |
| [HOT](implicit-state-machines/hotaru-app.md) | Hotaru's Aizome frontend and link matcher | 51 | 19217 | 0 | 40 | 10 | 9 | 21 |
| [KOY](implicit-state-machines/koyomi.md) | Koyomi calendar | 31 | 12322 | 0 | 19 | 10 | 5 | 4 |
| [YUK](implicit-state-machines/yukimi.md) | Yukimi system manager (Aizome app and core crates) | 62 | 20011 | 0 | 30 | 11 | 9 | 10 |
| [NTU](implicit-state-machines/nisshi-tokei-utsushi.md) | Nisshi, Tokei and Utsushi | 56 | 17286 | 0 | 33 | 6 | 9 | 18 |
| [KKO](implicit-state-machines/kura-koe.md) | Kura and Koe | 52 | 18603 | 0 | 37 | 9 | 8 | 20 |
| [TSH](implicit-state-machines/tansu-shiori.md) | Tansu and Shiori | 42 | 17040 | 0 | 32 | 8 | 8 | 16 |
| [SMT](implicit-state-machines/sumi-myaku-tenki.md) | Sumi, Myaku and Tenki | 58 | 20675 | 0 | 33 | 10 | 6 | 17 |
| [SAP](implicit-state-machines/small-apps.md) | Shashin, Soroban, Fusen and Annai | 48 | 13573 | 0 | 26 | 10 | 3 | 13 |
| [HCO](implicit-state-machines/hotaru-core.md) | Hotaru terminal engine | 41 | 16639 | 0 | 26 | 7 | 8 | 11 |
| [HTT](implicit-state-machines/hotaru-tests-tools.md) | Hotaru engine tests and development tools | 102 | 14734 | 0 | 28 | 2 | 7 | 19 |
| [GEN](implicit-state-machines/genkan.md) | Genkan installer | 83 | 19128 | 0 | 37 | 7 | 8 | 22 |
| [TTL](implicit-state-machines/tatami-tool.md) | Tatami command, display watcher and configuration | 39 | 11922 | 0 | 29 | 8 | 9 | 12 |
| [TAP](implicit-state-machines/tatami-apps.md) | Tatami settings apps | 50 | 19991 | 0 | 33 | 9 | 11 | 13 |
| [KTL](implicit-state-machines/kakehashi-tools.md) | Kakehashi, respin tools and scripts | 47 | 10261 | 0 | 31 | 8 | 10 | 13 |
| [INX](implicit-state-machines/infra-nix.md) | Nix integration, carried patches and live-media modules | 128 | 13538 | 0 | 46 | 5 | 4 | 37 |
| [ICI](implicit-state-machines/infra-ci-tests.md) | CI workflows, websites and repository tests | 77 | 16866 | 0 | 27 | 3 | 5 | 19 |
| **Total** | | **1699** | **525028** | **0** | **999** | **236** | **258** | **505** |

Every in-scope file has been audited; no ledger row is `pending`.

## Index

`Follow-up` starts as `implicit` for every machine. Change it to `reified (#PR)` when a machine becomes an explicit state machine, or `kept implicit` with a reason after review. Keep the ID either way.

### Aizome

#### [CPA: Compositor core: backends, recovery, pacing and rendering](implicit-state-machines/comp-a.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [CPA-01](implicit-state-machines/comp-a.md#cpa-01) | Headless output refresh pacing and buffer age | Low |  | implicit |
| [CPA-02](implicit-state-machines/comp-a.md#cpa-02) | Per-output redraw scheduling (queued, due, drawn) across backends | High |  | implicit |
| [CPA-03](implicit-state-machines/comp-a.md#cpa-03) | Udev per-CRTC page-flip state | Medium |  | implicit |
| [CPA-04](implicit-state-machines/comp-a.md#cpa-04) | libseat session pause and resume (VT switching) | High | [environment](implicit-state-machines/reproductions.md#cpa-04) | implicit |
| [CPA-05](implicit-state-machines/comp-a.md#cpa-05) | DRM device and connector hotplug, and output enablement | High | [environment](implicit-state-machines/reproductions.md#cpa-05) | implicit |
| [CPA-06](implicit-state-machines/comp-a.md#cpa-06) | Winit full-redraw countdown | Low |  | implicit |
| [CPA-07](implicit-state-machines/comp-a.md#cpa-07) | Blur backdrop cache validity | Medium |  | implicit |
| [CPA-08](implicit-state-machines/comp-a.md#cpa-08) | Fallback (no-logic) window focus and stacking | Low |  | implicit |
| [CPA-09](implicit-state-machines/comp-a.md#cpa-09) | Layer-surface ghosts (last frames kept after unmap) | Low |  | implicit |
| [CPA-10](implicit-state-machines/comp-a.md#cpa-10) | Idle progression to lock and monitors-off | Medium |  | implicit |
| [CPA-11](implicit-state-machines/comp-a.md#cpa-11) | Night Light application | Low |  | implicit |
| [CPA-12](implicit-state-machines/comp-a.md#cpa-12) | Output layout membership and wl_output global retirement | High |  | implicit |
| [CPA-13](implicit-state-machines/comp-a.md#cpa-13) | FIFO barrier release and backstop | Medium |  | implicit |
| [CPA-14](implicit-state-machines/comp-a.md#cpa-14) | Commit-timing (timestamped commit) release timer | Low |  | implicit |
| [CPA-15](implicit-state-machines/comp-a.md#cpa-15) | Display power and lid docking layout | High |  | implicit |
| [CPA-16](implicit-state-machines/comp-a.md#cpa-16) | Recovery overlay session (open and closed) | High | [confirmed](implicit-state-machines/reproductions.md#cpa-16) | implicit |
| [CPA-17](implicit-state-machines/comp-a.md#cpa-17) | Two-press confirmation (Log Out, Text Console) | Medium | unverified | implicit |
| [CPA-18](implicit-state-machines/comp-a.md#cpa-18) | Overlay ownership of key and button releases | Low |  | implicit |
| [CPA-19](implicit-state-machines/comp-a.md#cpa-19) | Pointer-constraint hold across the overlay and Reset Input | Low |  | implicit |
| [CPA-20](implicit-state-machines/comp-a.md#cpa-20) | Recovery ping and sample cadence, and the sample cache | Medium | unverified | implicit |
| [CPA-21](implicit-state-machines/comp-a.md#cpa-21) | Force Quit escalation (SIGKILL, then cut the connection) | Medium |  | implicit |
| [CPA-22](implicit-state-machines/comp-a.md#cpa-22) | Re-locking after a shell restart while locked | Medium |  | implicit |
| [CPA-23](implicit-state-machines/comp-a.md#cpa-23) | Per-client ping and pong responsiveness | Low |  | implicit |
| [CPA-24](implicit-state-machines/comp-a.md#cpa-24) | Process-info worker: request coalescing, CPU baselines and kills | High | [confirmed](implicit-state-machines/reproductions.md#cpa-24) | implicit |
| [CPA-25](implicit-state-machines/comp-a.md#cpa-25) | X11 PID lookup thread with a deadline | Low | unverified | implicit |
| [CPA-26](implicit-state-machines/comp-a.md#cpa-26) | Deferred logic reload request | Low |  | implicit |
| [CPA-27](implicit-state-machines/comp-a.md#cpa-27) | Supervised helper process lifecycle (shell, xwayland-satellite) | High | [confirmed](implicit-state-machines/reproductions.md#cpa-27) | implicit |
| [CPA-28](implicit-state-machines/comp-a.md#cpa-28) | Shell restart handover | High |  | implicit |
| [CPA-29](implicit-state-machines/comp-a.md#cpa-29) | Session start-up and systemd notification sequence | Low |  | implicit |
| [CPA-30](implicit-state-machines/comp-a.md#cpa-30) | Pixman thumbnail cache | Low |  | implicit |
| [CPA-31](implicit-state-machines/comp-a.md#cpa-31) | Logic timer generation guard | Low |  | implicit |
| [CPA-32](implicit-state-machines/comp-a.md#cpa-32) | Main-loop stall watcher, systemd watchdog and the stall marker | High | [confirmed](implicit-state-machines/reproductions.md#cpa-32) | implicit |

#### [CPB: Compositor core: state, input, focus and protocols](implicit-state-machines/comp-b.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [CPB-01](implicit-state-machines/comp-b.md#cpb-01) | Live settings reconciliation against the last-applied snapshot | Low |  | implicit |
| [CPB-02](implicit-state-machines/comp-b.md#cpb-02) | xdg_popup grab (menu chain) lifecycle | High |  | implicit |
| [CPB-03](implicit-state-machines/comp-b.md#cpb-03) | Keyboard focus arbitration (who should hold the keys) | Medium |  | implicit |
| [CPB-04](implicit-state-machines/comp-b.md#cpb-04) | Guarded logic pointer grab | Medium |  | implicit |
| [CPB-05](implicit-state-machines/comp-b.md#cpb-05) | Client drag-and-drop icon | Low |  | implicit |
| [CPB-06](implicit-state-machines/comp-b.md#cpb-06) | xdg-activation token validity | Medium |  | implicit |
| [CPB-07](implicit-state-machines/comp-b.md#cpb-07) | Idle-inhibitor registry | High | [confirmed](implicit-state-machines/reproductions.md#cpb-07) | implicit |
| [CPB-08](implicit-state-machines/comp-b.md#cpb-08) | Pointer constraint activation (lock and confine) | High | [confirmed](implicit-state-machines/reproductions.md#cpb-08) | implicit |
| [CPB-09](implicit-state-machines/comp-b.md#cpb-09) | Input-method commit serial (bypassed) | Low |  | implicit |
| [CPB-10](implicit-state-machines/comp-b.md#cpb-10) | Rasterized asset caches (app icons, text labels) | Low |  | implicit |
| [CPB-11](implicit-state-machines/comp-b.md#cpb-11) | Key press/release pairing (suppressed and translated keys) | High |  | implicit |
| [CPB-12](implicit-state-machines/comp-b.md#cpb-12) | Forwarded-button accounting | Medium |  | implicit |
| [CPB-13](implicit-state-machines/comp-b.md#cpb-13) | IPC connection lifecycle | Medium |  | implicit |
| [CPB-14](implicit-state-machines/comp-b.md#cpb-14) | Typing-keyboard keymap switching | Medium |  | implicit |
| [CPB-15](implicit-state-machines/comp-b.md#cpb-15) | IPC key-catch request | Low |  | implicit |
| [CPB-16](implicit-state-machines/comp-b.md#cpb-16) | Compositor process startup and shutdown sequence | Low |  | implicit |
| [CPB-17](implicit-state-machines/comp-b.md#cpb-17) | Compositor logic library lifecycle (load, reload, poison, rollback, set aside) | High |  | implicit |
| [CPB-18](implicit-state-machines/comp-b.md#cpb-18) | Popup tracking and popup-layer stacking (xdg and input-method popups) | High | [confirmed](implicit-state-machines/reproductions.md#cpb-18) | implicit |
| [CPB-19](implicit-state-machines/comp-b.md#cpb-19) | Capture stream damage and scratch-buffer state | Medium |  | implicit |
| [CPB-20](implicit-state-machines/comp-b.md#cpb-20) | Foreign-toplevel handle synchronisation (wlr management and ext list) | High | [confirmed](implicit-state-machines/reproductions.md#cpb-20) | implicit |
| [CPB-21](implicit-state-machines/comp-b.md#cpb-21) | Gamma-control ownership per output | High | [confirmed](implicit-state-machines/reproductions.md#cpb-21) | implicit |
| [CPB-22](implicit-state-machines/comp-b.md#cpb-22) | ext-image-copy-capture session and frame lifecycle | High | [partly](implicit-state-machines/reproductions.md#cpb-22) | implicit |
| [CPB-23](implicit-state-machines/comp-b.md#cpb-23) | Screen-share reporting (settle delay and last report) | Medium |  | implicit |
| [CPB-24](implicit-state-machines/comp-b.md#cpb-24) | wlr-output-management configuration lifecycle | High | [environment](implicit-state-machines/reproductions.md#cpb-24) | implicit |
| [CPB-25](implicit-state-machines/comp-b.md#cpb-25) | wlr-screencopy frame lifecycle and stream pool | High | [confirmed](implicit-state-machines/reproductions.md#cpb-25) | implicit |
| [CPB-26](implicit-state-machines/comp-b.md#cpb-26) | Toplevel icon build and freeze | Low |  | implicit |
| [CPB-27](implicit-state-machines/comp-b.md#cpb-27) | Layer-surface lifecycle and usable-area tracking | Medium |  | implicit |
| [CPB-28](implicit-state-machines/comp-b.md#cpb-28) | Decoration-mode negotiation | Low |  | implicit |
| [CPB-29](implicit-state-machines/comp-b.md#cpb-29) | dmabuf-fence commit blockers | Low |  | implicit |
| [CPB-30](implicit-state-machines/comp-b.md#cpb-30) | Toplevel map/unmap lifecycle and window registry | High |  | implicit |
| [CPB-31](implicit-state-machines/comp-b.md#cpb-31) | Session lock (partially explicit) | Medium |  | implicit |
| [CPB-32](implicit-state-machines/comp-b.md#cpb-32) | Per-output frame and presentation state | Medium |  | implicit |
| [CPB-33](implicit-state-machines/comp-b.md#cpb-33) | Loop-iteration pipeline and dirty-flag coalescing | Medium |  | implicit |
| [CPB-34](implicit-state-machines/comp-b.md#cpb-34) | Default-terminal index and one-time Hotaru migration marker | Low |  | implicit |

#### [WM: Window manager logic and compositor integration tests](implicit-state-machines/wm-comp-tests.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [WM-01](implicit-state-machines/wm-comp-tests.md#wm-01) | Toplevel configure size negotiation (requested size until the client commits it) | Medium |  | implicit |
| [WM-02](implicit-state-machines/wm-comp-tests.md#wm-02) | Motion timelines (Desk slides, tab slides, overlay entrances) | Low |  | implicit |
| [WM-03](implicit-state-machines/wm-comp-tests.md#wm-03) | Window move and tab-drag gesture | Medium |  | implicit |
| [WM-04](implicit-state-machines/wm-comp-tests.md#wm-04) | Interactive resize and its edge anchor | High | [confirmed](implicit-state-machines/reproductions.md#wm-04) | implicit |
| [WM-05](implicit-state-machines/wm-comp-tests.md#wm-05) | Tab slide and close/zoom box grabs | Low |  | implicit |
| [WM-06](implicit-state-machines/wm-comp-tests.md#wm-06) | Logic instance lifecycle and reload handoff | Medium |  | implicit |
| [WM-07](implicit-state-machines/wm-comp-tests.md#wm-07) | View publication and change coalescing | Medium |  | implicit |
| [WM-08](implicit-state-machines/wm-comp-tests.md#wm-08) | Key decision pipeline (bound action, translation, suppression) | Low |  | implicit |
| [WM-09](implicit-state-machines/wm-comp-tests.md#wm-09) | Slow keys | Low |  | implicit |
| [WM-10](implicit-state-machines/wm-comp-tests.md#wm-10) | Sticky keys (latched modifiers) | High | [confirmed](implicit-state-machines/reproductions.md#wm-10) | implicit |
| [WM-11](implicit-state-machines/wm-comp-tests.md#wm-11) | Pointer interaction bookkeeping (hover, double-click, overlay-owned presses) | Low |  | implicit |
| [WM-12](implicit-state-machines/wm-comp-tests.md#wm-12) | Area picker (screenshots and IPC `pick-area`) | High | [confirmed](implicit-state-machines/reproductions.md#wm-12) | implicit |
| [WM-13](implicit-state-machines/wm-comp-tests.md#wm-13) | Marking menu | High |  | implicit |
| [WM-14](implicit-state-machines/wm-comp-tests.md#wm-14) | Overlay modality (which overlay owns input) | High |  | implicit |
| [WM-15](implicit-state-machines/wm-comp-tests.md#wm-15) | Desks overview | High | [confirmed](implicit-state-machines/reproductions.md#wm-15) | implicit |
| [WM-16](implicit-state-machines/wm-comp-tests.md#wm-16) | App switcher | Medium |  | implicit |
| [WM-17](implicit-state-machines/wm-comp-tests.md#wm-17) | Notification card slides | Medium |  | implicit |
| [WM-18](implicit-state-machines/wm-comp-tests.md#wm-18) | Window geometry mode (placement, restore, display home) | High |  | implicit |
| [WM-19](implicit-state-machines/wm-comp-tests.md#wm-19) | Focus and visibility | High |  | implicit |
| [WM-20](implicit-state-machines/wm-comp-tests.md#wm-20) | Tab groups | Medium |  | implicit |
| [WM-21](implicit-state-machines/wm-comp-tests.md#wm-21) | Displays and their Desks | High |  | implicit |
| [WM-22](implicit-state-machines/wm-comp-tests.md#wm-22) | Desk parking (suspended Desks) | Medium |  | implicit |
| [WM-23](implicit-state-machines/wm-comp-tests.md#wm-23) | (test harness) frame-time benchmark driver | Low |  | implicit |
| [WM-24](implicit-state-machines/wm-comp-tests.md#wm-24) | (test harness) sidestep test client modes | Low |  | implicit |
| [WM-25](implicit-state-machines/wm-comp-tests.md#wm-25) | (test harness) raw Wayland probe clients recording protocol events | Low |  | implicit |
| [WM-26](implicit-state-machines/wm-comp-tests.md#wm-26) | (test harness) external client processes and poll-until loops | Low |  | implicit |
| [WM-27](implicit-state-machines/wm-comp-tests.md#wm-27) | (test harness) shell-like layer and popup client | Low |  | implicit |
| [WM-28](implicit-state-machines/wm-comp-tests.md#wm-28) | (test harness) headless compositor process | Low |  | implicit |
| [WM-29](implicit-state-machines/wm-comp-tests.md#wm-29) | (test harness) solid-colour toplevel test client | Low |  | implicit |
| [WM-30](implicit-state-machines/wm-comp-tests.md#wm-30) | (test harness) session-lock client | Low |  | implicit |
| [WM-31](implicit-state-machines/wm-comp-tests.md#wm-31) | (test harness) text-field app with a grabbing context menu | Low |  | implicit |
| [WM-32](implicit-state-machines/wm-comp-tests.md#wm-32) | (test harness) ibus-like input method with keyboard grab | Low |  | implicit |
| [WM-33](implicit-state-machines/wm-comp-tests.md#wm-33) | (test harness) screen-capture clients (image-copy-capture and wlr-screencopy) | Low |  | implicit |
| [WM-34](implicit-state-machines/wm-comp-tests.md#wm-34) | (test harness) input method counting its serial as IBus does | Low |  | implicit |
| [WM-35](implicit-state-machines/wm-comp-tests.md#wm-35) | (test harness) recovery probe client (freeze, pointer lock, late move) | Low |  | implicit |
| [WM-36](implicit-state-machines/wm-comp-tests.md#wm-36) | (test harness) logic variant builds for reload tests | Low |  | implicit |
| [WM-37](implicit-state-machines/wm-comp-tests.md#wm-37) | (test harness) shell restart scenario with a private D-Bus | Low |  | implicit |
| [WM-38](implicit-state-machines/wm-comp-tests.md#wm-38) | (test harness) udev backend VM scenario | Low |  | implicit |

#### [SHA: Shell: main loop, startup, menubar, deskbar, desktop and palette](implicit-state-machines/shell-a.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [SHA-01](implicit-state-machines/shell-a.md#sha-01) | Bar menu open / slide / press-to-close, and the Toolchest-to-palette handover | High |  | implicit |
| [SHA-02](implicit-state-machines/shell-a.md#sha-02) | Shell-replacement carry handoff (state file, final word, retiring) | High | [confirmed](implicit-state-machines/reproductions.md#sha-02) | implicit |
| [SHA-03](implicit-state-machines/shell-a.md#sha-03) | Deskbar auto-hide visibility | High | [confirmed](implicit-state-machines/reproductions.md#sha-03) | implicit |
| [SHA-04](implicit-state-machines/shell-a.md#sha-04) | Deskbar surface lifecycle (open, resize, reopen on display/kind/edge change) | Medium |  | implicit |
| [SHA-05](implicit-state-machines/shell-a.md#sha-05) | Deskbar row drags (reorder an app; window onto a Desk) | Medium |  | implicit |
| [SHA-06](implicit-state-machines/shell-a.md#sha-06) | Deskbar edge-resize drag and its preview surface | Medium |  | implicit |
| [SHA-07](implicit-state-machines/shell-a.md#sha-07) | Deskbar grip-move drag and its landing surface | Low |  | implicit |
| [SHA-08](implicit-state-machines/shell-a.md#sha-08) | Per-app Deskbar fold and notification-expansion toggles | Low |  | implicit |
| [SHA-09](implicit-state-machines/shell-a.md#sha-09) | Desktop icon selection, icon drag and rubber band | Medium |  | implicit |
| [SHA-10](implicit-state-machines/shell-a.md#sha-10) | Wallpaper scaling pipeline | Low |  | implicit |
| [SHA-11](implicit-state-machines/shell-a.md#sha-11) | Change-polling watcher threads and the one-shot desktop refresh | Low |  | implicit |
| [SHA-12](implicit-state-machines/shell-a.md#sha-12) | Palette open/close and its drawn-ahead spare window | High | [confirmed](implicit-state-machines/reproductions.md#sha-12) | implicit |
| [SHA-13](implicit-state-machines/shell-a.md#sha-13) | Palette file search, index lifetime and per-opening extras | High | [confirmed](implicit-state-machines/reproductions.md#sha-13) | implicit |
| [SHA-14](implicit-state-machines/shell-a.md#sha-14) | File index cache: validity, rebuild and refresh timer | Medium | unverified | implicit |
| [SHA-15](implicit-state-machines/shell-a.md#sha-15) | Frecency store: refresh on staleness, record on a thread | Low |  | implicit |
| [SHA-16](implicit-state-machines/shell-a.md#sha-16) | Shelf items and parked-image files | Low |  | implicit |
| [SHA-17](implicit-state-machines/shell-a.md#sha-17) | Per-output bar/desktop surfaces and the surface registry | Medium |  | implicit |
| [SHA-18](implicit-state-machines/shell-a.md#sha-18) | Desk rename in place | Medium | unverified | implicit |
| [SHA-19](implicit-state-machines/shell-a.md#sha-19) | Launch waiting for an activation token | Low |  | implicit |
| [SHA-20](implicit-state-machines/shell-a.md#sha-20) | App index load and reload | Low | unverified | implicit |
| [SHA-21](implicit-state-machines/shell-a.md#sha-21) | Clock tick timer cadence | Low | unverified | implicit |
| [SHA-22](implicit-state-machines/shell-a.md#sha-22) | logind sleep-delay handshake (session-events side) | Medium | unverified | implicit |
| [SHA-23](implicit-state-machines/shell-a.md#sha-23) | Launch attempt lifecycle (Application Launches) | High |  | implicit |
| [SHA-24](implicit-state-machines/shell-a.md#sha-24) | Application Launches window and its journal reads | Low |  | implicit |
| [SHA-25](implicit-state-machines/shell-a.md#sha-25) | Torn-off palettes (persisted list, shown surfaces, strip drag) | High | [confirmed](implicit-state-machines/reproductions.md#sha-25) | implicit |
| [SHA-26](implicit-state-machines/shell-a.md#sha-26) | Mirror of the compositor's state (windows, focus, Desks, lock) | Medium |  | implicit |

#### [SHB: Shell: lock, notifications, recording, systray and services](implicit-state-machines/shell-b.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [SHB-01](implicit-state-machines/shell-b.md#shb-01) | App connection registry (hello, menus, goodbye) | Low |  | implicit |
| [SHB-02](implicit-state-machines/shell-b.md#shb-02) | Shell socket ownership and per-connection threads | Medium |  | implicit |
| [SHB-03](implicit-state-machines/shell-b.md#shb-03) | Polkit authentication dialog | High | [confirmed](implicit-state-machines/reproductions.md#shb-03) | reified ([#298](https://github.com/bitemyapp/sekka/pull/298)) |
| [SHB-04](implicit-state-machines/shell-b.md#shb-04) | Keep Awake status (request, watcher, indicator) | Low |  | implicit |
| [SHB-05](implicit-state-machines/shell-b.md#shb-05) | Timed overlay surfaces (bell flash, OSD) | Low |  | implicit |
| [SHB-06](implicit-state-machines/shell-b.md#shb-06) | Session confirmation countdown (Restart, Shut Down, Log Out, power key) | High | [confirmed](implicit-state-machines/reproductions.md#shb-06) | implicit |
| [SHB-07](implicit-state-machines/shell-b.md#shb-07) | Login display-scales publisher (coalescing worker) | Low |  | implicit |
| [SHB-08](implicit-state-machines/shell-b.md#shb-08) | GTK look publication to dconf | Low |  | implicit |
| [SHB-09](implicit-state-machines/shell-b.md#shb-09) | Compositor event follower (connect, snapshot, subscribe, reconnect) | High | [confirmed](implicit-state-machines/reproductions.md#shb-09) | implicit |
| [SHB-10](implicit-state-machines/shell-b.md#shb-10) | Compositor request sender (lazy connection, one retry) | Medium |  | implicit |
| [SHB-11](implicit-state-machines/shell-b.md#shb-11) | GeoClue location lookup | Low |  | implicit |
| [SHB-12](implicit-state-machines/shell-b.md#shb-12) | Lock screen lifecycle (partially explicit) | High | [confirmed](implicit-state-machines/reproductions.md#shb-12) | reified ([#298](https://github.com/bitemyapp/sekka/pull/298)) |
| [SHB-13](implicit-state-machines/shell-b.md#shb-13) | Lock field shake animation | Low |  | implicit |
| [SHB-14](implicit-state-machines/shell-b.md#shb-14) | Critical notification cards surface | High | [confirmed](implicit-state-machines/reproductions.md#shb-14) | implicit |
| [SHB-15](implicit-state-machines/shell-b.md#shb-15) | Bar notification line (banner) | High | [confirmed](implicit-state-machines/reproductions.md#shb-15) | implicit |
| [SHB-16](implicit-state-machines/shell-b.md#shb-16) | Notifications server: bus name ownership and ids | Medium |  | implicit |
| [SHB-17](implicit-state-machines/shell-b.md#shb-17) | Notification lifecycle in the store | Medium |  | implicit |
| [SHB-18](implicit-state-machines/shell-b.md#shb-18) | PAM transaction | Low |  | implicit |
| [SHB-19](implicit-state-machines/shell-b.md#shb-19) | Power key: logind inhibitor and HandlePowerKey | Low |  | implicit |
| [SHB-20](implicit-state-machines/shell-b.md#shb-20) | Screen recording session (partially explicit) | High | [confirmed](implicit-state-machines/reproductions.md#shb-20) | implicit |
| [SHB-21](implicit-state-machines/shell-b.md#shb-21) | Recorder capability probe | Low |  | implicit |
| [SHB-22](implicit-state-machines/shell-b.md#shb-22) | Recorder supervisor thread (fallback, settle, stop escalation) | Medium |  | implicit |
| [SHB-23](implicit-state-machines/shell-b.md#shb-23) | Recording handover note and adoption across shell restarts | Medium |  | implicit |
| [SHB-24](implicit-state-machines/shell-b.md#shb-24) | (test harness) Pretend recorders for the supervisor | Low |  | implicit |
| [SHB-25](implicit-state-machines/shell-b.md#shb-25) | Services selection wait before the pointer menu | Low |  | implicit |
| [SHB-26](implicit-state-machines/shell-b.md#shb-26) | Screen-share indicator settle (vestigial) | Low |  | implicit |
| [SHB-27](implicit-state-machines/shell-b.md#shb-27) | Systray panel popup and Wi-Fi password ask | Medium |  | implicit |
| [SHB-28](implicit-state-machines/shell-b.md#shb-28) | Status item click and menu fetch | Low |  | implicit |
| [SHB-29](implicit-state-machines/shell-b.md#shb-29) | (test harness) Lock screen PAM settle loop | Low |  | implicit |
| [SHB-30](implicit-state-machines/shell-b.md#shb-30) | (test harness) Private D-Bus daemon and fake MPRIS player | Low |  | implicit |

#### [SVC: Applications index, media, IPC, portal and greeter](implicit-state-machines/kit-services.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [SVC-01](implicit-state-machines/kit-services.md#svc-01) | Icon lookup cache layers | Low |  | implicit |
| [SVC-02](implicit-state-machines/kit-services.md#svc-02) | Application index freshness (signature versus scan) | Medium |  | implicit |
| [SVC-03](implicit-state-machines/kit-services.md#svc-03) | Index watcher thread lifecycle | Low |  | implicit |
| [SVC-04](implicit-state-machines/kit-services.md#svc-04) | Default terminal moved from foot to Hotaru (one-shot migration) | Low |  | implicit |
| [SVC-05](implicit-state-machines/kit-services.md#svc-05) | Greeter screen stage (people, password, typed name) | High | [confirmed](implicit-state-machines/reproductions.md#svc-05) | reified ([#298](https://github.com/bitemyapp/sekka/pull/298)) |
| [SVC-06](implicit-state-machines/kit-services.md#svc-06) | greetd link thread | Low |  | implicit |
| [SVC-07](implicit-state-machines/kit-services.md#svc-07) | Greeter surfaces per display and the main display | Low |  | implicit |
| [SVC-08](implicit-state-machines/kit-services.md#svc-08) | Greeter wallpaper loads and the day/night tick | Low |  | implicit |
| [SVC-09](implicit-state-machines/kit-services.md#svc-09) | Wrong-password shake animation | Low |  | implicit |
| [SVC-10](implicit-state-machines/kit-services.md#svc-10) | Greeter power action and its error | Low |  | implicit |
| [SVC-11](implicit-state-machines/kit-services.md#svc-11) | Greeter's private compositor and process teardown | Low |  | implicit |
| [SVC-12](implicit-state-machines/kit-services.md#svc-12) | greetd login conversation | High |  | implicit |
| [SVC-13](implicit-state-machines/kit-services.md#svc-13) | aizome-dev watch-and-reload loop | Low |  | implicit |
| [SVC-14](implicit-state-machines/kit-services.md#svc-14) | aizome-dev app restart | Low |  | implicit |
| [SVC-15](implicit-state-machines/kit-services.md#svc-15) | Share picker's pick-area wait | Low |  | implicit |
| [SVC-16](implicit-state-machines/kit-services.md#svc-16) | `aizomectl reload --offer` flow | Low |  | implicit |
| [SVC-17](implicit-state-machines/kit-services.md#svc-17) | IPC client connection modes | Low |  | implicit |
| [SVC-18](implicit-state-machines/kit-services.md#svc-18) | Catch-key wait for keyboard setup | Low |  | implicit |
| [SVC-19](implicit-state-machines/kit-services.md#svc-19) | Audio ring flush, halt and segment protocol | Medium |  | implicit |
| [SVC-20](implicit-state-machines/kit-services.md#svc-20) | Null audio output thread | Low |  | implicit |
| [SVC-21](implicit-state-machines/kit-services.md#svc-21) | Playback clock anchor | Low |  | implicit |
| [SVC-22](implicit-state-machines/kit-services.md#svc-22) | PipeWire output thread and stream | Low |  | implicit |
| [SVC-23](implicit-state-machines/kit-services.md#svc-23) | Playback session lifecycle | High | [confirmed](implicit-state-machines/reproductions.md#svc-23) | implicit |
| [SVC-24](implicit-state-machines/kit-services.md#svc-24) | Seek bookkeeping across threads | Low |  | implicit |
| [SVC-25](implicit-state-machines/kit-services.md#svc-25) | Audio decode part (skip, rebase, drain) | Medium |  | implicit |
| [SVC-26](implicit-state-machines/kit-services.md#svc-26) | Audio output fallback to silence | Medium |  | implicit |
| [SVC-27](implicit-state-machines/kit-services.md#svc-27) | Wake coalescing in the player and video threads | Low |  | implicit |
| [SVC-28](implicit-state-machines/kit-services.md#svc-28) | Video presentation loop (poster, steps, generations) | Medium |  | implicit |
| [SVC-29](implicit-state-machines/kit-services.md#svc-29) | Video suspension out of sight, and catching up | High | [confirmed](implicit-state-machines/reproductions.md#svc-29) | implicit |
| [SVC-30](implicit-state-machines/kit-services.md#svc-30) | Late-frame skipping hysteresis | Low |  | implicit |
| [SVC-31](implicit-state-machines/kit-services.md#svc-31) | Frame path and the cache of exported surfaces | Medium |  | implicit |
| [SVC-32](implicit-state-machines/kit-services.md#svc-32) | GPU decoding falling back to software | Medium |  | implicit |
| [SVC-33](implicit-state-machines/kit-services.md#svc-33) | Ownership of shared-memory frame slots | Low |  | implicit |
| [SVC-34](implicit-state-machines/kit-services.md#svc-34) | (test harness) Media engine event-wait driver | Low |  | implicit |
| [SVC-35](implicit-state-machines/kit-services.md#svc-35) | File chooser's one-shot answer lifecycle | Medium |  | implicit |
| [SVC-36](implicit-state-machines/kit-services.md#svc-36) | Chooser sheet (Go to Folder, New Folder) | Low |  | implicit |
| [SVC-37](implicit-state-machines/kit-services.md#svc-37) | Chooser location, history and listing | Medium |  | implicit |
| [SVC-38](implicit-state-machines/kit-services.md#svc-38) | Portal FileChooser request lifecycle | Medium |  | implicit |
| [SVC-39](implicit-state-machines/kit-services.md#svc-39) | Chooser subprocess supervision | Low |  | implicit |
| [SVC-40](implicit-state-machines/kit-services.md#svc-40) | (test harness) Chooser home fixture and settle | Low |  | implicit |
| [SVC-41](implicit-state-machines/kit-services.md#svc-41) | (test harness) Private session bus with stand-in choosers | Low |  | implicit |

#### [AZN: Small crates, session launcher, Nix modules and VM tests](implicit-state-machines/aizome-small-nix.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [AZN-01](implicit-state-machines/aizome-small-nix.md#azn-01) | Keep Awake requested/protected status (setting file against logind's locks) | High | [confirmed](implicit-state-machines/reproductions.md#azn-01) | implicit |
| [AZN-02](implicit-state-machines/aizome-small-nix.md#azn-02) | Keep Awake holder service lifecycle (`aizome-awake hold`) | Low |  | implicit |
| [AZN-03](implicit-state-machines/aizome-small-nix.md#azn-03) | GetSecrets request registry and cancellation (network agent) | Medium |  | implicit |
| [AZN-04](implicit-state-machines/aizome-small-nix.md#azn-04) | Agent registration with NetworkManager (retry, re-register on restart) | High | [confirmed](implicit-state-machines/reproductions.md#azn-04) | implicit |
| [AZN-05](implicit-state-machines/aizome-small-nix.md#azn-05) | Credential worker subprocess lifecycle (keyring, dialog, VPN helper) | Low |  | implicit |
| [AZN-06](implicit-state-machines/aizome-small-nix.md#azn-06) | (test harness) private bus with a scripted AgentManager and a blocking dialog stand-in | Low |  | implicit |
| [AZN-07](implicit-state-machines/aizome-small-nix.md#azn-07) | Power meter socket activation, idle exit and reading throttle | Low |  | implicit |
| [AZN-08](implicit-state-machines/aizome-small-nix.md#azn-08) | File index: query-narrowing cache and persisted index freshness | Low |  | implicit |
| [AZN-09](implicit-state-machines/aizome-small-nix.md#azn-09) | Frecency history file: staleness stamp and read-modify-write recording | Low |  | implicit |
| [AZN-10](implicit-state-machines/aizome-small-nix.md#azn-10) | Shell socket connection protocol (hello, menus, services-text serial, request/reply) | Low |  | implicit |
| [AZN-11](implicit-state-machines/aizome-small-nix.md#azn-11) | Foot palette mode switched by shell signals | Low |  | implicit |
| [AZN-12](implicit-state-machines/aizome-small-nix.md#azn-12) | Login screen session handoff and X11 session teardown | Low |  | implicit |
| [AZN-13](implicit-state-machines/aizome-small-nix.md#azn-13) | Time-zone ownership across activations (saved zone file and `/etc/localtime` link) | High | [confirmed](implicit-state-machines/reproductions.md#azn-13) | implicit |
| [AZN-14](implicit-state-machines/aizome-small-nix.md#azn-14) | Per-login re-arming of session user units from persisted opt-ins | Low |  | implicit |
| [AZN-15](implicit-state-machines/aizome-small-nix.md#azn-15) | (test harness) ScreenCast portal client: request/Response chain and frame count | Low |  | implicit |
| [AZN-16](implicit-state-machines/aizome-small-nix.md#azn-16) | (test harness) real user manager with gated stand-in compositor (launcher systemd test) | Low |  | implicit |
| [AZN-17](implicit-state-machines/aizome-small-nix.md#azn-17) | (test harness) fake systemctl driven by marker files (launcher unit test) | Low |  | implicit |
| [AZN-18](implicit-state-machines/aizome-small-nix.md#azn-18) | (test harness) disposable-device fixtures behind VM marker files and case modes | Low |  | implicit |
| [AZN-19](implicit-state-machines/aizome-small-nix.md#azn-19) | (test harness) kit-inspection UI driver shared by the component VM tests | Low |  | implicit |
| [AZN-20](implicit-state-machines/aizome-small-nix.md#azn-20) | (test harness) login/logout cycles with a lingering user manager | Low |  | implicit |
| [AZN-21](implicit-state-machines/aizome-small-nix.md#azn-21) | (test harness) virtual radios, VPN server and NetworkManager activation driver | Low |  | implicit |
| [AZN-22](implicit-state-machines/aizome-small-nix.md#azn-22) | (test harness) polkit outage and recovery across system switches | Low |  | implicit |
| [AZN-23](implicit-state-machines/aizome-small-nix.md#azn-23) | (test harness) Settings driver and ordered parts sharing system state | Low |  | implicit |
| [AZN-24](implicit-state-machines/aizome-small-nix.md#azn-24) | (test harness) session smoke test: reload, recording, sharing and watchdog restart | Low |  | implicit |
| [AZN-25](implicit-state-machines/aizome-small-nix.md#azn-25) | xdg-desktop-portal-wlr screencast capture lifecycle (carried patch) | High |  | implicit |
| [AZN-26](implicit-state-machines/aizome-small-nix.md#azn-26) | (test harness) private headless session drivers (development scripts) | Low |  | implicit |
| [AZN-27](implicit-state-machines/aizome-small-nix.md#azn-27) | Session launcher phases (`aizome-session`) | High |  | implicit |
| [AZN-28](implicit-state-machines/aizome-small-nix.md#azn-28) | Session unit graph: compositor binding, session and shutdown targets, service restart policies | Medium |  | implicit |
| [AZN-29](implicit-state-machines/aizome-small-nix.md#azn-29) | Post-update reload offer (path unit and a waiting oneshot) | Low |  | implicit |

#### [SYA: System clients: launch, audio, Bluetooth, disks, MPRIS](implicit-state-machines/sys-a.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [SYA-01](implicit-state-machines/sys-a.md#sya-01) | App launch route cascade and observer event stream | Medium |  | implicit |
| [SYA-02](implicit-state-machines/sys-a.md#sya-02) | systemd-run scope handshake (reader thread, timeout, 500 ms grace) | Medium |  | implicit |
| [SYA-03](implicit-state-machines/sys-a.md#sya-03) | (test harness) launcher-lifetime marker files and probe wait loops | Low |  | implicit |
| [SYA-04](implicit-state-machines/sys-a.md#sya-04) | PulseAudio connection and reconnect loop | Medium |  | implicit |
| [SYA-05](implicit-state-machines/sys-a.md#sya-05) | Optimistic audio levels awaiting server confirmation | High | [confirmed](implicit-state-machines/reproductions.md#sya-05) | implicit |
| [SYA-06](implicit-state-machines/sys-a.md#sya-06) | Bluetooth pairing-agent registration worker | High | [confirmed](implicit-state-machines/reproductions.md#sya-06) | implicit |
| [SYA-07](implicit-state-machines/sys-a.md#sya-07) | Pairing request conversation (prompt, responder, BlueZ cancel) | Medium |  | implicit |
| [SYA-08](implicit-state-machines/sys-a.md#sya-08) | ObjectManager follower cache (BlueZ, UDisks) | Medium | unverified | implicit |
| [SYA-09](implicit-state-machines/sys-a.md#sya-09) | Multi-step BlueZ actions (unblock then power, pair then trust then connect) | Low |  | implicit |
| [SYA-10](implicit-state-machines/sys-a.md#sya-10) | Daemon follower (subscription, owner tracking, Appeared/Vanished/Idle) | Medium |  | implicit |
| [SYA-11](implicit-state-machines/sys-a.md#sya-11) | Single-object property follower with keep-on-exit (timedated, localed, hostnamed) | Medium |  | implicit |
| [SYA-12](implicit-state-machines/sys-a.md#sya-12) | Wait-until-serving ping retry loop | Low |  | implicit |
| [SYA-13](implicit-state-machines/sys-a.md#sya-13) | Destructive-operation confirmation token | High | [confirmed](implicit-state-machines/reproductions.md#sya-13) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) |
| [SYA-14](implicit-state-machines/sys-a.md#sya-14) | Disk image mount: attach, probe, mount, autoclear, clean up | Medium |  | implicit |
| [SYA-15](implicit-state-machines/sys-a.md#sya-15) | Drive release before eject or power-off | Low |  | implicit |
| [SYA-16](implicit-state-machines/sys-a.md#sya-16) | GeoClue locate request with deadline and cancellation | Low |  | implicit |
| [SYA-17](implicit-state-machines/sys-a.md#sya-17) | Level meter thread lifecycle | High | [partly](implicit-state-machines/reproductions.md#sya-17) | implicit |
| [SYA-18](implicit-state-machines/sys-a.md#sya-18) | MPRIS player-set follower | Medium |  | implicit |
| [SYA-19](implicit-state-machines/sys-a.md#sya-19) | MPRIS position estimate and active-player counters | High |  | implicit |

#### [SYB: System clients: network, notifications, polkit, power, tray, users](implicit-state-machines/sys-b.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [SYB-01](implicit-state-machines/sys-b.md#syb-01) | Airplane mode across rfkill, NetworkManager, BlueZ and a marker file | High | [confirmed](implicit-state-machines/reproductions.md#syb-01) | implicit |
| [SYB-02](implicit-state-machines/sys-b.md#syb-02) | Wi-Fi join / activation transaction with in-memory edit and rollback | High | [confirmed](implicit-state-machines/reproductions.md#syb-02) | implicit |
| [SYB-03](implicit-state-machines/sys-b.md#syb-03) | Daemon follower present/dirty coalescing (NetworkManager, logind, UPower, power profiles, AccountsService) | Medium |  | implicit |
| [SYB-04](implicit-state-machines/sys-b.md#syb-04) | rfkill watcher thread and radio table | Low |  | implicit |
| [SYB-05](implicit-state-machines/sys-b.md#syb-05) | Notification client: id matching, early-signal buffer, server loss | Medium |  | implicit |
| [SYB-06](implicit-state-machines/sys-b.md#syb-06) | Open notification and its pausable expiry timer | Medium |  | implicit |
| [SYB-07](implicit-state-machines/sys-b.md#syb-07) | Notification server lifecycle and bus-name ownership | Medium |  | implicit |
| [SYB-08](implicit-state-machines/sys-b.md#syb-08) | One-value async channel | Low |  | implicit |
| [SYB-09](implicit-state-machines/sys-b.md#syb-09) | polkit authentication session and helper conversation | High | [confirmed](implicit-state-machines/reproductions.md#syb-09) | reified ([#298](https://github.com/bitemyapp/sekka/pull/298)) |
| [SYB-10](implicit-state-machines/sys-b.md#syb-10) | polkit agent registration and reconnect loop | High |  | implicit |
| [SYB-11](implicit-state-machines/sys-b.md#syb-11) | Backlight brightness shown before logind sets it, and the sysfs watcher | Medium |  | implicit |
| [SYB-12](implicit-state-machines/sys-b.md#syb-12) | logind sleep-delay inhibitor and lock-before-sleep handshake | High | [confirmed](implicit-state-machines/reproductions.md#syb-12) | reified ([#298](https://github.com/bitemyapp/sekka/pull/298)) |
| [SYB-13](implicit-state-machines/sys-b.md#syb-13) | Background Task result slot | Low |  | implicit |
| [SYB-14](implicit-state-machines/sys-b.md#syb-14) | Service model generation and the Stop latch | Medium |  | implicit |
| [SYB-15](implicit-state-machines/sys-b.md#syb-15) | StatusNotifierWatcher service lifecycle and name ownership | Medium |  | implicit |
| [SYB-16](implicit-state-machines/sys-b.md#syb-16) | Status item registry: register, re-read, gone | Medium |  | implicit |

#### [SYC: System clients: printers and sysinfo; config, theme and face](implicit-state-machines/sys-c-config.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [SYC-01](implicit-state-machines/sys-c-config.md#syc-01) | Keyboard settings: inherited session defaults vs saved choices | High | [confirmed](implicit-state-machines/reproductions.md#syc-01) | implicit |
| [SYC-02](implicit-state-machines/sys-c-config.md#syc-02) | One-shot migration of the legacy `[keys] terminal` key | High | [confirmed](implicit-state-machines/reproductions.md#syc-02) | implicit |
| [SYC-03](implicit-state-machines/sys-c-config.md#syc-03) | Settings file lifecycle: load, fallback to defaults, atomic save, cross-process writers | High | [confirmed](implicit-state-machines/reproductions.md#syc-03) | implicit |
| [SYC-04](implicit-state-machines/sys-c-config.md#syc-04) | Settings file watcher thread | Low |  | implicit |
| [SYC-05](implicit-state-machines/sys-c-config.md#syc-05) | Persisted GeoClue location and its time-zone validity | Low |  | implicit |
| [SYC-06](implicit-state-machines/sys-c-config.md#syc-06) | Drawn element press, drag, drag-out and hover gesture | High | [partly](implicit-state-machines/reproductions.md#syc-06) | implicit |
| [SYC-07](implicit-state-machines/sys-c-config.md#syc-07) | Text field caret, selection, IME preedit, blink and Caps Lock state | High | [confirmed](implicit-state-machines/reproductions.md#syc-07) | reified ([#298](https://github.com/bitemyapp/sekka/pull/298)) |
| [SYC-08](implicit-state-machines/sys-c-config.md#syc-08) | (test harness) MPRIS test player playback state and pw-play child | Low |  | implicit |
| [SYC-09](implicit-state-machines/sys-c-config.md#syc-09) | CUPS tool subprocess lifecycle in `run` | Medium |  | implicit |
| [SYC-10](implicit-state-machines/sys-c-config.md#syc-10) | User's default printer in `~/.cups/lpoptions` | Low |  | implicit |
| [SYC-11](implicit-state-machines/sys-c-config.md#syc-11) | Printers snapshot, poller and refresh turns | Medium |  | implicit |
| [SYC-12](implicit-state-machines/sys-c-config.md#syc-12) | Streaming lpinfo driver parser and lpstat record parsers | Low |  | implicit |
| [SYC-13](implicit-state-machines/sys-c-config.md#syc-13) | (test harness) Fake CUPS tool scripts with mutable canned cases | Low |  | implicit |
| [SYC-14](implicit-state-machines/sys-c-config.md#syc-14) | CPU usage delta sampler | Low |  | implicit |
| [SYC-15](implicit-state-machines/sys-c-config.md#syc-15) | Disk and network throughput samplers | Low |  | implicit |
| [SYC-16](implicit-state-machines/sys-c-config.md#syc-16) | Power meter idle floor and RAPL readings | Low |  | implicit |
| [SYC-17](implicit-state-machines/sys-c-config.md#syc-17) | Process sampler: per-pid cache validity and CPU interval | Medium |  | implicit |
| [SYC-18](implicit-state-machines/sys-c-config.md#syc-18) | (test harness) Private PipeWire and pipewire-pulse | Low |  | implicit |
| [SYC-19](implicit-state-machines/sys-c-config.md#syc-19) | (test harness) Fake BlueZ: agent registration, discovery and pairing | Low |  | implicit |
| [SYC-20](implicit-state-machines/sys-c-config.md#syc-20) | (test harness) Private dbus-daemon per test and the `eventually` wait | Low |  | implicit |
| [SYC-21](implicit-state-machines/sys-c-config.md#syc-21) | (test harness) Fake UDisks2 object lifecycles with fault injection | Low |  | implicit |
| [SYC-22](implicit-state-machines/sys-c-config.md#syc-22) | (test harness) Real UDisks loop-device lifetime driver (VM only) | Low |  | implicit |
| [SYC-23](implicit-state-machines/sys-c-config.md#syc-23) | (test harness) Fake GeoClue2 client with call gates and stall injection | Low |  | implicit |
| [SYC-24](implicit-state-machines/sys-c-config.md#syc-24) | (test harness) Fake MPRIS players | Low |  | implicit |
| [SYC-25](implicit-state-machines/sys-c-config.md#syc-25) | (test harness) Fake NetworkManager activation lifecycle | Low |  | implicit |
| [SYC-26](implicit-state-machines/sys-c-config.md#syc-26) | (test harness) Fake polkitd authority, PAM helper and scripted answering UI | Low |  | implicit |
| [SYC-27](implicit-state-machines/sys-c-config.md#syc-27) | (test harness) Fake logind inhibitors, sleep signals and backlight; fake profile holds | Low |  | implicit |
| [SYC-28](implicit-state-machines/sys-c-config.md#syc-28) | (test harness) Fake AccountsService user creation and deletion | Low |  | implicit |

#### [KIT: Toolkit runtime](implicit-state-machines/kit-core.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [KIT-01](implicit-state-machines/kit-core.md#kit-01) | (test harness) Gallery demo and benchmark driver | Low |  | implicit |
| [KIT-02](implicit-state-machines/kit-core.md#kit-02) | Draw-ahead of hidden windows (band drawing) | Medium |  | implicit |
| [KIT-03](implicit-state-machines/kit-core.md#kit-03) | Kept picture for a shown window's first frame | Low |  | implicit |
| [KIT-04](implicit-state-machines/kit-core.md#kit-04) | Menu publication to the shell (dedup and debounce) | High | [confirmed](implicit-state-machines/reproductions.md#kit-04) | implicit |
| [KIT-05](implicit-state-machines/kit-core.md#kit-05) | Sidestep handle maps and request ids | Low |  | implicit |
| [KIT-06](implicit-state-machines/kit-core.md#kit-06) | Alert sheet lifecycle | Low |  | implicit |
| [KIT-07](implicit-state-machines/kit-core.md#kit-07) | File panel lifecycle and browsing state | High | [confirmed](implicit-state-machines/reproductions.md#kit-07) | implicit |
| [KIT-08](implicit-state-machines/kit-core.md#kit-08) | Editor undo history with typing bursts | Low |  | implicit |
| [KIT-09](implicit-state-machines/kit-core.md#kit-09) | Window and sheet key registry | Medium |  | implicit |
| [KIT-10](implicit-state-machines/kit-core.md#kit-10) | Window view, build and relayout lifecycle | Medium |  | implicit |
| [KIT-11](implicit-state-machines/kit-core.md#kit-11) | Window hide/show with held ticks, timers and focus | Medium |  | implicit |
| [KIT-12](implicit-state-machines/kit-core.md#kit-12) | Engine timer registry (ticks, widget timers, app timers) | Medium |  | implicit |
| [KIT-13](implicit-state-machines/kit-core.md#kit-13) | Pointer hover, press and capture | High | [confirmed](implicit-state-machines/reproductions.md#kit-13) | implicit |
| [KIT-14](implicit-state-machines/kit-core.md#kit-14) | Keyboard focus across modal layers, with pending focus and IME | High |  | implicit |
| [KIT-15](implicit-state-machines/kit-core.md#kit-15) | Menu popup chain (menus, submenus, in-window bar menu) | Medium |  | implicit |
| [KIT-16](implicit-state-machines/kit-core.md#kit-16) | Popover popup reconciliation | Medium |  | implicit |
| [KIT-17](implicit-state-machines/kit-core.md#kit-17) | Tooltip delay, show and dismissal | High | [confirmed](implicit-state-machines/reproductions.md#kit-17) | implicit |
| [KIT-18](implicit-state-machines/kit-core.md#kit-18) | Video placement and opaque-region mirror | Low |  | implicit |
| [KIT-19](implicit-state-machines/kit-core.md#kit-19) | Session lock windows | Low |  | implicit |
| [KIT-20](implicit-state-machines/kit-core.md#kit-20) | Inspection files per window | Low |  | implicit |
| [KIT-21](implicit-state-machines/kit-core.md#kit-21) | Shell connection: link, watcher and menu placement | High |  | implicit |
| [KIT-22](implicit-state-machines/kit-core.md#kit-22) | (test harness) TestPlatform virtual clock and timer queue | Low |  | implicit |
| [KIT-23](implicit-state-machines/kit-core.md#kit-23) | (test harness) TestPlatform visibility, picture and damage model | Low |  | implicit |
| [KIT-24](implicit-state-machines/kit-core.md#kit-24) | (test harness) Fake session lock | Low |  | implicit |
| [KIT-25](implicit-state-machines/kit-core.md#kit-25) | (test harness) Retained frame oracles | Low |  | implicit |
| [KIT-26](implicit-state-machines/kit-core.md#kit-26) | (test harness) Wall-clock polling loops | Low |  | implicit |
| [KIT-27](implicit-state-machines/kit-core.md#kit-27) | (test harness) Fake clipboard and primary selection | Low |  | implicit |
| [KIT-28](implicit-state-machines/kit-core.md#kit-28) | Appearance following and retheming | Medium |  | implicit |
| [KIT-29](implicit-state-machines/kit-core.md#kit-29) | Node arena: liveness, layout cache and per-layout outboxes | High | [confirmed](implicit-state-machines/reproductions.md#kit-29) | implicit |
| [KIT-30](implicit-state-machines/kit-core.md#kit-30) | Desktop opener worker (bounded xdg-open) | Medium |  | implicit |
| [KIT-31](implicit-state-machines/kit-core.md#kit-31) | Per-node widget state across rebuilds | Low |  | implicit |
| [KIT-32](implicit-state-machines/kit-core.md#kit-32) | (test harness) Menu-bar fixture's keyboard title walk | Low |  | implicit |
| [KIT-33](implicit-state-machines/kit-core.md#kit-33) | (test harness) Fake shell socket server | Low |  | implicit |

#### [UIW: Sukumo UI and toolkit widgets](implicit-state-machines/sukumo-ui-kit-widgets.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [UIW-01](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-01) | Press-then-release click pairing in buttons, switches, checkboxes, radios and segmented controls | High | [confirmed](implicit-state-machines/reproductions.md#uiw-01) | implicit |
| [UIW-02](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-02) | Slider drag and commit | Medium |  | implicit |
| [UIW-03](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-03) | Hovered part of steppers and segmented controls | Low |  | implicit |
| [UIW-04](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-04) | Indeterminate progress sweep | Low |  | implicit |
| [UIW-05](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-05) | Image scaled on its second sighting | Low |  | implicit |
| [UIW-06](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-06) | Mouse area press, drag, drag-out and hover gesture | Medium |  | implicit |
| [UIW-07](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-07) | Previous-layout snapshots for partial redraw | Low |  | implicit |
| [UIW-08](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-08) | Menu interaction: hover, open submenu, pointer aim, type-to-filter, strip tear-off | High | [confirmed](implicit-state-machines/reproductions.md#uiw-08) | implicit |
| [UIW-09](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-09) | In-window menu bar's open title | High | [confirmed](implicit-state-machines/reproductions.md#uiw-09) | implicit |
| [UIW-10](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-10) | Split divider drag | Medium |  | implicit |
| [UIW-11](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-11) | Scroll-bar visibility and thumb drag | High | [confirmed](implicit-state-machines/reproductions.md#uiw-11) | implicit |
| [UIW-12](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-12) | Scroll offset, reveal latch and laid-out offset | Low |  | implicit |
| [UIW-13](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-13) | Row press that may become a drag out | Medium |  | implicit |
| [UIW-14](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-14) | Table column resize | Medium |  | implicit |
| [UIW-15](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-15) | Selection cursor and anchor in lists, tables and outlines | High | [confirmed](implicit-state-machines/reproductions.md#uiw-15) | implicit |
| [UIW-16](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-16) | Type-select prefix buffer | Low |  | implicit |
| [UIW-17](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-17) | Outline's own open set | Low |  | implicit |
| [UIW-18](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-18) | Editor viewport, position-bar linger and caret blink on one tick | High | [confirmed](implicit-state-machines/reproductions.md#uiw-18) | implicit |
| [UIW-19](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-19) | Editor pointer selection and dragging selected text out | Medium |  | implicit |
| [UIW-20](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-20) | Editor caret reveal and goal column | Medium |  | implicit |
| [UIW-21](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-21) | Input-method composition in editors and text fields | Medium |  | implicit |
| [UIW-22](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-22) | Text field focus, caret blink and first-focus selection | Low |  | implicit |
| [UIW-23](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-23) | Text field pointer selection and dragging selected text out | Low |  | implicit |
| [UIW-24](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-24) | Text field undo history, typing groups and optimistic caret | Medium |  | implicit |
| [UIW-25](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-25) | Video above or below the window's drawing | Low |  | implicit |
| [UIW-26](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-26) | Application run lifecycle and loop wake-up | Low |  | implicit |
| [UIW-27](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-27) | Session lock | High | [confirmed](implicit-state-machines/reproductions.md#uiw-27) | reified ([#298](https://github.com/bitemyapp/sekka/pull/298)) |
| [UIW-28](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-28) | Touchpad coasting | Medium |  | implicit |
| [UIW-29](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-29) | Drag-and-drop offers over windows | Medium |  | implicit |
| [UIW-30](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-30) | Clipboard and primary-selection ownership | Low |  | implicit |
| [UIW-31](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-31) | Window showing and frame-pacing lifecycle | High |  | implicit |
| [UIW-32](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-32) | (test harness) Step-polled scenario drivers on the null render thread | Low |  | implicit |
| [UIW-33](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-33) | (test harness) Virtual pointer driver | Low |  | implicit |
| [UIW-34](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-34) | (test harness) Headless-sway scenario drivers | Low |  | implicit |

#### [SKB: Sukumo engine: Wayland client backend](implicit-state-machines/sukumo-backend.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [SKB-01](implicit-state-machines/sukumo-backend.md#skb-01) | Client-side decoration presence and redraw cache | Medium |  | implicit |
| [SKB-02](implicit-state-machines/sukumo-backend.md#skb-02) | Header button hover and press tracking | Low |  | implicit |
| [SKB-03](implicit-state-machines/sukumo-backend.md#skb-03) | Incoming drag-and-drop session | High |  | implicit |
| [SKB-04](implicit-state-machines/sukumo-backend.md#skb-04) | Outgoing drag source lifecycle | Medium |  | implicit |
| [SKB-05](implicit-state-machines/sukumo-backend.md#skb-05) | xdg-foreign parent import link | Low |  | implicit |
| [SKB-06](implicit-state-machines/sukumo-backend.md#skb-06) | Compose (dead-key) sequence | Low |  | implicit |
| [SKB-07](implicit-state-machines/sukumo-backend.md#skb-07) | Layer surface configure and close | Low |  | implicit |
| [SKB-08](implicit-state-machines/sukumo-backend.md#skb-08) | Session lock lifecycle | Medium | unverified | implicit |
| [SKB-09](implicit-state-machines/sukumo-backend.md#skb-09) | In-window menu bar presence and redraw | Low |  | implicit |
| [SKB-10](implicit-state-machines/sukumo-backend.md#skb-10) | Render thread lifecycle | Medium |  | implicit |
| [SKB-11](implicit-state-machines/sukumo-backend.md#skb-11) | Window configure, size and geometry lifecycle | High | [confirmed](implicit-state-machines/reproductions.md#skb-11) | implicit |
| [SKB-12](implicit-state-machines/sukumo-backend.md#skb-12) | Window scale source | Low |  | implicit |
| [SKB-13](implicit-state-machines/sukumo-backend.md#skb-13) | Frame pacing and layer-tree animation gating | High |  | implicit |
| [SKB-14](implicit-state-machines/sukumo-backend.md#skb-14) | Shared-memory buffer reuse and staleness (window and tile buffers) | Medium |  | implicit |
| [SKB-15](implicit-state-machines/sukumo-backend.md#skb-15) | xdg_popup lifecycle and grab serial | Medium | unverified | implicit |
| [SKB-16](implicit-state-machines/sukumo-backend.md#skb-16) | Activation tokens | Low |  | implicit |
| [SKB-17](implicit-state-machines/sukumo-backend.md#skb-17) | Pointer passthrough and input-region application | Medium | unverified | implicit |
| [SKB-18](implicit-state-machines/sukumo-backend.md#skb-18) | (test harness) Null render thread as a fake compositor | Medium | unverified | implicit |
| [SKB-19](implicit-state-machines/sukumo-backend.md#skb-19) | Output discovery, settle and learned scales | Medium |  | implicit |
| [SKB-20](implicit-state-machines/sukumo-backend.md#skb-20) | Keyboard focus source, seat devices and first mouse | Medium |  | implicit |
| [SKB-21](implicit-state-machines/sukumo-backend.md#skb-21) | Pointer focus and content crossings | Medium |  | implicit |
| [SKB-22](implicit-state-machines/sukumo-backend.md#skb-22) | Click counting | Low |  | implicit |
| [SKB-23](implicit-state-machines/sukumo-backend.md#skb-23) | Touchpad scroll gesture and axis frame | Medium | unverified | implicit |
| [SKB-24](implicit-state-machines/sukumo-backend.md#skb-24) | Pinch gesture | Low |  | implicit |
| [SKB-25](implicit-state-machines/sukumo-backend.md#skb-25) | Cursor visibility and shown cache | Low |  | implicit |
| [SKB-26](implicit-state-machines/sukumo-backend.md#skb-26) | Key repeat timer | Medium |  | implicit |
| [SKB-27](implicit-state-machines/sukumo-backend.md#skb-27) | Selection and primary-selection ownership | High | [not confirmed](implicit-state-machines/reproductions.md#skb-27) | implicit |
| [SKB-28](implicit-state-machines/sukumo-backend.md#skb-28) | Promised clipboard and drag data tokens | Medium |  | implicit |
| [SKB-29](implicit-state-machines/sukumo-backend.md#skb-29) | Bounded pipe transfers | Low |  | implicit |
| [SKB-30](implicit-state-machines/sukumo-backend.md#skb-30) | Sheet placement and parent commit gating | Low |  | implicit |
| [SKB-31](implicit-state-machines/sukumo-backend.md#skb-31) | text-input-v3 focus, enable and event batching | Medium |  | implicit |
| [SKB-32](implicit-state-machines/sukumo-backend.md#skb-32) | Scroll-layer tile lifecycle and raster queue | Medium |  | implicit |
| [SKB-33](implicit-state-machines/sukumo-backend.md#skb-33) | Video subsurface placement sync | Low |  | implicit |
| [SKB-34](implicit-state-machines/sukumo-backend.md#skb-34) | Video compositor buffer cache and frame release | Medium |  | implicit |
| [SKB-35](implicit-state-machines/sukumo-backend.md#skb-35) | Video capability discovery handshake | Low |  | implicit |
| [SKB-36](implicit-state-machines/sukumo-backend.md#skb-36) | Foreign selection text, change count and identity | High |  | implicit |
| [SKB-37](implicit-state-machines/sukumo-backend.md#skb-37) | Clipboard read requests by token | Medium |  | implicit |
| [SKB-38](implicit-state-machines/sukumo-backend.md#skb-38) | Settings portal D-Bus session | Medium | unverified | implicit |
| [SKB-39](implicit-state-machines/sukumo-backend.md#skb-39) | Momentum coast phases | Low |  | implicit |
| [SKB-40](implicit-state-machines/sukumo-backend.md#skb-40) | Published outputs first-snapshot handshake | Low |  | implicit |
| [SKB-41](implicit-state-machines/sukumo-backend.md#skb-41) | Desktop appearance knowledge and change notification | Medium |  | implicit |
| [SKB-42](implicit-state-machines/sukumo-backend.md#skb-42) | Frame statistics collection and record/present pairing | Low |  | implicit |

#### [SKR: Sukumo engine: animation, text and raster](implicit-state-machines/sukumo-render.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [SKR-01](implicit-state-machines/sukumo-render.md#skr-01) | Render-thread layer tree linkage (parent links and forward references) | Medium |  | implicit |
| [SKR-02](implicit-state-machines/sukumo-render.md#skr-02) | Transition snapshot lifecycle | Medium |  | implicit |
| [SKR-03](implicit-state-machines/sukumo-render.md#skr-03) | Retained paints and per-host frame scheduling (dirty / moving / last snapshot) | High | [confirmed](implicit-state-machines/reproductions.md#skr-03) | implicit |
| [SKR-04](implicit-state-machines/sukumo-render.md#skr-04) | Layer contents, canvas generations and their image-cache keys | High | [confirmed](implicit-state-machines/reproductions.md#skr-04) | implicit |
| [SKR-05](implicit-state-machines/sukumo-render.md#skr-05) | Animated GIF/WebP frame decoder cursor | Low |  | implicit |
| [SKR-06](implicit-state-machines/sukumo-render.md#skr-06) | Accent-keyed system palette cache | Low |  | implicit |
| [SKR-07](implicit-state-machines/sukumo-render.md#skr-07) | CoreGraphics path builder current point and derived caches | Low |  | implicit |
| [SKR-08](implicit-state-machines/sukumo-render.md#skr-08) | Raster worker pool and scoped band latch | Medium |  | implicit |
| [SKR-09](implicit-state-machines/sukumo-render.md#skr-09) | Per-thread image texture cache (budget, LRU, generations, halvings, tints) | Medium |  | implicit |
| [SKR-10](implicit-state-machines/sukumo-render.md#skr-10) | Op-stream group nesting while rasterizing | Low |  | implicit |
| [SKR-11](implicit-state-machines/sukumo-render.md#skr-11) | Font collection open/prewarm and the squashed-name cache | Low |  | implicit |
| [SKR-12](implicit-state-machines/sukumo-render.md#skr-12) | Font-file face registry (private families, by blob, by content, by name) | Low |  | implicit |
| [SKR-13](implicit-state-machines/sukumo-render.md#skr-13) | Terminal grid `FontSet` (primary face, bounded faces, resolution and layout caches) | Medium |  | implicit |
| [SKR-14](implicit-state-machines/sukumo-render.md#skr-14) | Two-generation text caches (layouts, glyph strikes, shared glyph images) | Low |  | implicit |
| [SKR-15](implicit-state-machines/sukumo-render.md#skr-15) | Incremental TextKit frame layout after edits | Medium |  | implicit |
| [SKR-16](implicit-state-machines/sukumo-render.md#skr-16) | Text layout worker pool batches | Low |  | implicit |
| [SKR-17](implicit-state-machines/sukumo-render.md#skr-17) | (test harness) Headless sway runner lifecycle | Low | unverified | implicit |

#### [STA: Settei: general, keyboard, network, sound and other panes](implicit-state-machines/settei-a.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [STA-01](implicit-state-machines/settei-a.md#sta-01) | Pane background loads (Option as the loading spinner, re-read on every show) | Medium |  | implicit |
| [STA-02](implicit-state-machines/settei-a.md#sta-02) | Inline name drafts committed on Return or blur (computer name, host name, your name) | Low |  | implicit |
| [STA-03](implicit-state-machines/settei-a.md#sta-03) | Confirm-then-act alerts (the target carried in the confirm message) | Low |  | implicit |
| [STA-04](implicit-state-machines/settei-a.md#sta-04) | Visible-pane one-second tick timers (Date & Time clock, Performance poll) | Low |  | implicit |
| [STA-05](implicit-state-machines/settei-a.md#sta-05) | Language & Region formats preview with a stale-locale guard | High | [not confirmed](implicit-state-machines/reproductions.md#sta-05) | implicit |
| [STA-06](implicit-state-machines/settei-a.md#sta-06) | (test harness) Settle and advance loops in Settei's pane tests | Low |  | implicit |
| [STA-07](implicit-state-machines/settei-a.md#sta-07) | Boot splash pending-restart phase (running system vs next start) | Low |  | implicit |
| [STA-08](implicit-state-machines/settei-a.md#sta-08) | Busy spinner animation clock | Low |  | implicit |
| [STA-09](implicit-state-machines/settei-a.md#sta-09) | (test harness) Live-snapshot sample capture | Low |  | implicit |
| [STA-10](implicit-state-machines/settei-a.md#sta-10) | Keyboard setup: catching the key beside the space bar | High | [confirmed](implicit-state-machines/reproductions.md#sta-10) | implicit |
| [STA-11](implicit-state-machines/settei-a.md#sta-11) | Shortcut recording mode and the recorder's held modifiers | Low |  | implicit |
| [STA-12](implicit-state-machines/settei-a.md#sta-12) | Pane sheets against the window's single sheet slot (keyboard's three, network, printers, users) | High | [confirmed](implicit-state-machines/reproductions.md#sta-12) | implicit |
| [STA-13](implicit-state-machines/settei-a.md#sta-13) | Wi-Fi join: one at a time, the sheet reopened as it was on failure | High |  | implicit |
| [STA-14](implicit-state-machines/settei-a.md#sta-14) | Wi-Fi scan cycle | Low |  | implicit |
| [STA-15](implicit-state-machines/settei-a.md#sta-15) | Connection details page: navigation, profile reads and the edit buffer | High | [confirmed](implicit-state-machines/reproductions.md#sta-15) | implicit |
| [STA-16](implicit-state-machines/settei-a.md#sta-16) | Per-item in-flight sets (network connections, printers) | Low |  | implicit |
| [STA-17](implicit-state-machines/settei-a.md#sta-17) | (test harness) Sample services emulating NetworkManager, CUPS and PipeWire | Low |  | implicit |
| [STA-18](implicit-state-machines/settei-a.md#sta-18) | CUPS client lifetime tied to the pane's visibility and the add sheet | Medium |  | implicit |
| [STA-19](implicit-state-machines/settei-a.md#sta-19) | Add-a-printer sheet: discovery, driver lookup, adding | High | [confirmed](implicit-state-machines/reproductions.md#sta-19) | implicit |
| [STA-20](implicit-state-machines/settei-a.md#sta-20) | Palette file index and history reset by removing the shell's files | Low |  | implicit |
| [STA-21](implicit-state-machines/settei-a.md#sta-21) | Input level meter following the default input | Medium |  | implicit |
| [STA-22](implicit-state-machines/settei-a.md#sta-22) | New account: its password applied once AccountsService lists it | High | [confirmed](implicit-state-machines/reproductions.md#sta-22) | implicit |

#### [STB: Settei: app shell, appearance, Bluetooth and displays](implicit-state-machines/settei-b.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [STB-01](implicit-state-machines/settei-b.md#stb-01) | Pane navigation, history and the shown/hidden pane lifecycle | Medium | unverified | implicit |
| [STB-02](implicit-state-machines/settei-b.md#stb-02) | Sheet window ownership, split between the window and the panes | Medium | unverified | implicit |
| [STB-03](implicit-state-machines/settei-b.md#stb-03) | Generation-tagged transient highlights: the revealed option and the status line | Low |  | implicit |
| [STB-04](implicit-state-machines/settei-b.md#stb-04) | Single-instance socket: claim, hand-off, listen, release | Medium | unverified | implicit |
| [STB-05](implicit-state-machines/settei-b.md#stb-05) | Remembered theme and size before High Contrast and Larger text (persisted) | Low | unverified | implicit |
| [STB-06](implicit-state-machines/settei-b.md#stb-06) | Lazy pane data loads keyed on an empty Option (wallpapers, apps to pin) | Low |  | implicit |
| [STB-07](implicit-state-machines/settei-b.md#stb-07) | Battery pane services, charge history and logind read | Low |  | implicit |
| [STB-08](implicit-state-machines/settei-b.md#stb-08) | Keep Awake switch: optimistic value, helper process, marker file | Medium | unverified | implicit |
| [STB-09](implicit-state-machines/settei-b.md#stb-09) | Bluetooth pane activity: pairing agent, discovery and discoverability while shown | High | [confirmed](implicit-state-machines/reproductions.md#stb-09) | implicit |
| [STB-10](implicit-state-machines/settei-b.md#stb-10) | Pairing questions: prompt, responders and per-device busy | High |  | implicit |
| [STB-11](implicit-state-machines/settei-b.md#stb-11) | Typed drafts committed on Return or blur | Low |  | implicit |
| [STB-12](implicit-state-machines/settei-b.md#stb-12) | Arrangement drag gesture | Medium | unverified | implicit |
| [STB-13](implicit-state-machines/settei-b.md#stb-13) | Display change: apply, "Keep these display settings?" countdown, keep or revert | High | [partly](implicit-state-machines/reproductions.md#stb-13) | implicit |
| [STB-14](implicit-state-machines/settei-b.md#stb-14) | Displays read and watch, and Night Light state | Medium |  | implicit |
| [STB-15](implicit-state-machines/settei-b.md#stb-15) | Backlight brightness request coalescing | Medium |  | implicit |
| [STB-16](implicit-state-machines/settei-b.md#stb-16) | What a display is used as (main, extended, mirror, off): fields and `[[outputs]]` keys | Medium |  | implicit |
| [STB-17](implicit-state-machines/settei-b.md#stb-17) | settings.toml write pipeline: ordered writer, pending count, reload debounce | High | [confirmed](implicit-state-machines/reproductions.md#stb-17) | implicit |
| [STB-18](implicit-state-machines/settei-b.md#stb-18) | Picture cache validated by file mtime | Low |  | implicit |
| [STB-19](implicit-state-machines/settei-b.md#stb-19) | (test harness) settle loops that pump the window until background work lands | Low |  | implicit |
| [STB-20](implicit-state-machines/settei-b.md#stb-20) | (test harness) live headless-compositor scenario for Displays | Low |  | implicit |

#### [HAKO: Hako file manager](implicit-state-machines/hako.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [HAKO-01](implicit-state-machines/hako.md#hako-01) | FileManager1 show-in-folder service bus-name lifecycle | Low |  | implicit |
| [HAKO-02](implicit-state-machines/hako.md#hako-02) | bsdtar child process run (compress/extract) with pause, cancel and partial output | Medium |  | implicit |
| [HAKO-03](implicit-state-machines/hako.md#hako-03) | Operation pause/cancel control | Low |  | implicit |
| [HAKO-04](implicit-state-machines/hako.md#hako-04) | In-kernel copy with buffered fallback | Low |  | implicit |
| [HAKO-05](implicit-state-machines/hako.md#hako-05) | Transfer placement with conflict prompt and replace-aside protocol | High | [confirmed](implicit-state-machines/reproductions.md#hako-05) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) |
| [HAKO-06](implicit-state-machines/hako.md#hako-06) | Copy-to-partial commit and cross-filesystem move (copy, fsync, delete arrived sources) | High | [confirmed](implicit-state-machines/reproductions.md#hako-06) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) |
| [HAKO-07](implicit-state-machines/hako.md#hako-07) | Ask-and-retry loops of the file operations | Low |  | implicit |
| [HAKO-08](implicit-state-machines/hako.md#hako-08) | Trash can item protocol (trashinfo claim, rename, directorysizes, forget) | High | [confirmed](implicit-state-machines/reproductions.md#hako-08) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) |
| [HAKO-09](implicit-state-machines/hako.md#hako-09) | Undo/redo history with in-flight inversion jobs | High | [confirmed](implicit-state-machines/reproductions.md#hako-09) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) |
| [HAKO-10](implicit-state-machines/hako.md#hako-10) | Thumbnail disk cache validity and failure marks | Low |  | implicit |
| [HAKO-11](implicit-state-machines/hako.md#hako-11) | inotify folder watcher and its watch-descriptor map | High | [confirmed](implicit-state-machines/reproductions.md#hako-11) | implicit |
| [HAKO-12](implicit-state-machines/hako.md#hako-12) | Desks follower thread (compositor IPC) | Low |  | implicit |
| [HAKO-13](implicit-state-machines/hako.md#hako-13) | Sheet registry over windows (Info, Quick Look, Ask, Confirm, Message, Unlock) | Medium |  | implicit |
| [HAKO-14](implicit-state-machines/hako.md#hako-14) | File clipboard (cut/copy ownership and cut fading) | Medium |  | implicit |
| [HAKO-15](implicit-state-machines/hako.md#hako-15) | Operations window presence | Low |  | implicit |
| [HAKO-16](implicit-state-machines/hako.md#hako-16) | Volume mount, unlock and two-phase eject tasks | High | [confirmed](implicit-state-machines/reproductions.md#hako-16) | implicit |
| [HAKO-17](implicit-state-machines/hako.md#hako-17) | Thumbnail request pipeline (queue, asked set, running count) | High | [confirmed](implicit-state-machines/reproductions.md#hako-17) | implicit |
| [HAKO-18](implicit-state-machines/hako.md#hako-18) | Background result caches (previews, folder sizes, free space, Trash count) | Low |  | implicit |
| [HAKO-19](implicit-state-machines/hako.md#hako-19) | Column listing load, reload debounce and refresh reconciliation | High | [confirmed](implicit-state-machines/reproductions.md#hako-19) | implicit |
| [HAKO-20](implicit-state-machines/hako.md#hako-20) | Rename session (inline field, new-folder auto-rename) | High | [confirmed](implicit-state-machines/reproductions.md#hako-20) | implicit |
| [HAKO-21](implicit-state-machines/hako.md#hako-21) | Search session (generation, running flag, index cache, worker threads) | Medium |  | implicit |
| [HAKO-22](implicit-state-machines/hako.md#hako-22) | Drag session (in-window drag, spring-loading, drag-out and its return) | Medium |  | implicit |
| [HAKO-23](implicit-state-machines/hako.md#hako-23) | Per-window navigation history (Back/Forward) | Low |  | implicit |
| [HAKO-24](implicit-state-machines/hako.md#hako-24) | Location bar edit mode | Low |  | implicit |
| [HAKO-25](implicit-state-machines/hako.md#hako-25) | Drop-target spot registry | High | [confirmed](implicit-state-machines/reproductions.md#hako-25) | implicit |
| [HAKO-26](implicit-state-machines/hako.md#hako-26) | File view gesture and keyboard state | Medium |  | implicit |
| [HAKO-27](implicit-state-machines/hako.md#hako-27) | Single-instance claim and hand-over socket | Low |  | implicit |
| [HAKO-28](implicit-state-machines/hako.md#hako-28) | Job lifecycle with blocking questions | High | [confirmed](implicit-state-machines/reproductions.md#hako-28) | implicit |
| [HAKO-29](implicit-state-machines/hako.md#hako-29) | Column strip follow-scroll and divider resize | Low |  | implicit |
| [HAKO-30](implicit-state-machines/hako.md#hako-30) | (test harness) settle and wait loops for the headless window | Low |  | implicit |
| [HAKO-31](implicit-state-machines/hako.md#hako-31) | (test harness) private D-Bus daemon and FileManager1 service driver | Low |  | implicit |

#### [HOT: Hotaru's Aizome frontend and link matcher](implicit-state-machines/hotaru-app.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [HOT-01](implicit-state-machines/hotaru-app.md#hot-01) | (test harness) OSC 7 latency benchmark driver | Low |  | implicit |
| [HOT-02](implicit-state-machines/hotaru-app.md#hot-02) | Pane lifecycle: live, ended, failed, removed, window teardown | High |  | implicit |
| [HOT-03](implicit-state-machines/hotaru-app.md#hot-03) | Tab, pane focus, zoom and recency model | Medium |  | implicit |
| [HOT-04](implicit-state-machines/hotaru-app.md#hot-04) | Focus-restore timer generation | Low |  | implicit |
| [HOT-05](implicit-state-machines/hotaru-app.md#hot-05) | Pane directory provenance and title derivation | Medium |  | implicit |
| [HOT-06](implicit-state-machines/hotaru-app.md#hot-06) | Pane font-shaping epoch and scale acceptance | Medium |  | implicit |
| [HOT-07](implicit-state-machines/hotaru-app.md#hot-07) | Automatic-link activation pipeline (match, hover, resolve, open) | High | [confirmed](implicit-state-machines/reproductions.md#hot-07) | implicit |
| [HOT-08](implicit-state-machines/hotaru-app.md#hot-08) | User copy/paste request ordering and paste notice | Medium |  | implicit |
| [HOT-09](implicit-state-machines/hotaru-app.md#hot-09) | Copy-on-select primary-selection token | Low |  | implicit |
| [HOT-10](implicit-state-machines/hotaru-app.md#hot-10) | Program clipboard request flow (OSC 52 / OSC 5522) and the single question | High |  | implicit |
| [HOT-11](implicit-state-machines/hotaru-app.md#hot-11) | Desktop notification gating | Low |  | implicit |
| [HOT-12](implicit-state-machines/hotaru-app.md#hot-12) | Settings persistence, reload and deferred window close | High | [confirmed](implicit-state-machines/reproductions.md#hot-12) | implicit |
| [HOT-13](implicit-state-machines/hotaru-app.md#hot-13) | Settings window and shortcut recording mode | Medium |  | implicit |
| [HOT-14](implicit-state-machines/hotaru-app.md#hot-14) | Appearance ownership between the desktop watcher and Hotaru's theme | High | [confirmed](implicit-state-machines/reproductions.md#hot-14) | implicit |
| [HOT-15](implicit-state-machines/hotaru-app.md#hot-15) | Latest-request single-slot worker threads (six copies) | Medium |  | implicit |
| [HOT-16](implicit-state-machines/hotaru-app.md#hot-16) | Go to Tab palette lifecycle | Low |  | implicit |
| [HOT-17](implicit-state-machines/hotaru-app.md#hot-17) | Single-instance claim and launch handoff | High | [confirmed](implicit-state-machines/reproductions.md#hot-17) | implicit |
| [HOT-18](implicit-state-machines/hotaru-app.md#hot-18) | Link-path resolver helper child supervision | Low |  | implicit |
| [HOT-19](implicit-state-machines/hotaru-app.md#hot-19) | Find: query, capture, whole-history search and navigation | High |  | implicit |
| [HOT-20](implicit-state-machines/hotaru-app.md#hot-20) | Per-pane clipboard request admission tickets and the selections queue | Low |  | implicit |
| [HOT-21](implicit-state-machines/hotaru-app.md#hot-21) | PTY worker lifecycle (spawn, run, EOF, hang-up, exit report, stop) | High | [confirmed](implicit-state-machines/reproductions.md#hot-21) | implicit |
| [HOT-22](implicit-state-machines/hotaru-app.md#hot-22) | PTY worker frame cadence, acknowledgement handshake and idle compression | Medium |  | implicit |
| [HOT-23](implicit-state-machines/hotaru-app.md#hot-23) | PTY worker latest-state controls mailbox | Low |  | implicit |
| [HOT-24](implicit-state-machines/hotaru-app.md#hot-24) | PTY input and reply back-pressure accounting | Medium |  | implicit |
| [HOT-25](implicit-state-machines/hotaru-app.md#hot-25) | PTY reader (gather) thread | Low |  | implicit |
| [HOT-26](implicit-state-machines/hotaru-app.md#hot-26) | Tab strip scroll-follow mode | Low |  | implicit |
| [HOT-27](implicit-state-machines/hotaru-app.md#hot-27) | Pointer drag selection and held-button tracking | High | [confirmed](implicit-state-machines/reproductions.md#hot-27) | implicit |
| [HOT-28](implicit-state-machines/hotaru-app.md#hot-28) | Modifier-click link press gesture | Medium |  | implicit |
| [HOT-29](implicit-state-machines/hotaru-app.md#hot-29) | Hover and automatic-link request lifecycle in the terminal view | High |  | implicit |
| [HOT-30](implicit-state-machines/hotaru-app.md#hot-30) | Layout-to-tick deferred notifications | Low |  | implicit |
| [HOT-31](implicit-state-machines/hotaru-app.md#hot-31) | Cursor blink phase | Low |  | implicit |
| [HOT-32](implicit-state-machines/hotaru-app.md#hot-32) | Held-key tracking for synthetic releases | Low |  | implicit |
| [HOT-33](implicit-state-machines/hotaru-app.md#hot-33) | Retained previous frame for scroll reuse and overhang repair | Low |  | implicit |
| [HOT-34](implicit-state-machines/hotaru-app.md#hot-34) | (test harness) Native harness waits and scripted PTY child programs | Low |  | implicit |
| [HOT-35](implicit-state-machines/hotaru-app.md#hot-35) | (test harness) Virtual pointer and keyboard probes with a stdin command protocol | Low |  | implicit |
| [HOT-36](implicit-state-machines/hotaru-app.md#hot-36) | (test harness) Stage-file fixtures acknowledged by window title | Low |  | implicit |
| [HOT-37](implicit-state-machines/hotaru-app.md#hot-37) | (test harness) Per-pane FIFO fixture for program clipboard requests | Low |  | implicit |
| [HOT-38](implicit-state-machines/hotaru-app.md#hot-38) | (test harness) Private headless-Sway driver lifecycle and polling waits | Low |  | implicit |
| [HOT-39](implicit-state-machines/hotaru-app.md#hot-39) | (test harness) Marker-file PTY fixture handshake | Low |  | implicit |
| [HOT-40](implicit-state-machines/hotaru-app.md#hot-40) | (test harness) Stalled data-control clipboard owner | Low |  | implicit |

#### [KOY: Koyomi calendar](implicit-state-machines/koyomi.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [KOY-01](implicit-state-machines/koyomi.md#koy-01) | Koyomi agent service lifecycle (unit, bus connect retry, listener threads, wake loop) | High | [confirmed](implicit-state-machines/reproductions.md#koy-01) | implicit |
| [KOY-02](implicit-state-machines/koyomi.md#koy-02) | Alert delivery state: due → fired → shown → snoozed/closed | High | [confirmed](implicit-state-machines/reproductions.md#koy-02) | implicit |
| [KOY-03](implicit-state-machines/koyomi.md#koy-03) | Settings and window-state persistence (unsaved settings, debounced state, agent's stale copy) | High | [confirmed](implicit-state-machines/reproductions.md#koy-03) | implicit |
| [KOY-04](implicit-state-machines/koyomi.md#koy-04) | Navigation and selection: view, anchor, cursor, mini month, selected occurrence | High | [confirmed](implicit-state-machines/reproductions.md#koy-04) | implicit |
| [KOY-05](implicit-state-machines/koyomi.md#koy-05) | Modal sheet slots: editor, import, name, settings | High | [confirmed](implicit-state-machines/reproductions.md#koy-05) | implicit |
| [KOY-06](implicit-state-machines/koyomi.md#koy-06) | Sequence-guarded one-shot timers: status note, state persist, grid scroll request | Low |  | implicit |
| [KOY-07](implicit-state-machines/koyomi.md#koy-07) | Derived occurrence caches: index, generation, occ, view-model memo | Medium |  | implicit |
| [KOY-08](implicit-state-machines/koyomi.md#koy-08) | Series-scope confirmation (`pending` + alert) | Medium |  | implicit |
| [KOY-09](implicit-state-machines/koyomi.md#koy-09) | Undo/redo history | Medium |  | implicit |
| [KOY-10](implicit-state-machines/koyomi.md#koy-10) | Calendar save pipeline: dirty → debounce → background write → stamp | High | [confirmed](implicit-state-machines/reproductions.md#koy-10) | implicit |
| [KOY-11](implicit-state-machines/koyomi.md#koy-11) | Launch phase and argument hand-off | Low |  | implicit |
| [KOY-12](implicit-state-machines/koyomi.md#koy-12) | Recurring series split state: master rule, EXDATE/RDATE, RECURRENCE-ID overrides | High | [confirmed](implicit-state-machines/reproductions.md#koy-12) | implicit |
| [KOY-13](implicit-state-machines/koyomi.md#koy-13) | Single-instance socket: claim, hand off, serve | Medium |  | implicit |
| [KOY-14](implicit-state-machines/koyomi.md#koy-14) | Store's view of the calendar folder: stamps, reload, trash | High | [confirmed](implicit-state-machines/reproductions.md#koy-14) | implicit |
| [KOY-15](implicit-state-machines/koyomi.md#koy-15) | Event editor draft | Medium |  | implicit |
| [KOY-16](implicit-state-machines/koyomi.md#koy-16) | Month chip drag | Low |  | implicit |
| [KOY-17](implicit-state-machines/koyomi.md#koy-17) | Time-grid pointer drag: move, resize, create; edge hover and scroll | High | [confirmed](implicit-state-machines/reproductions.md#koy-17) | implicit |
| [KOY-18](implicit-state-machines/koyomi.md#koy-18) | All-day strip drag | High | [confirmed](implicit-state-machines/reproductions.md#koy-18) | implicit |
| [KOY-19](implicit-state-machines/koyomi.md#koy-19) | (test harness) private D-Bus daemon and fake notification server | Low |  | implicit |

#### [YUK: Yukimi system manager (Aizome app and core crates)](implicit-state-machines/yukimi.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [YUK-01](implicit-state-machines/yukimi.md#yuk-01) | Sekka update channel and legacy migration in flake.nix | High |  | implicit |
| [YUK-02](implicit-state-machines/yukimi.md#yuk-02) | Update receipt and system completion advice | High | [confirmed](implicit-state-machines/reproductions.md#yuk-02) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) |
| [YUK-03](implicit-state-machines/yukimi.md#yuk-03) | Tatami session update record and Keep Awake check | Medium |  | implicit |
| [YUK-04](implicit-state-machines/yukimi.md#yuk-04) | Aizome live-session completion probe | Medium |  | implicit |
| [YUK-05](implicit-state-machines/yukimi.md#yuk-05) | (test harness) fake Aizome IPC server and stubbed session launcher | Low |  | implicit |
| [YUK-06](implicit-state-machines/yukimi.md#yuk-06) | Nix activity tracker and the helper's stage protocol | Low |  | implicit |
| [YUK-07](implicit-state-machines/yukimi.md#yuk-07) | Stop handle and streamed child lifecycle | Medium |  | implicit |
| [YUK-08](implicit-state-machines/yukimi.md#yuk-08) | Helper cancellation phase (Working / Stopping / Committed) | High |  | implicit |
| [YUK-09](implicit-state-machines/yukimi.md#yuk-09) | Privileged change transaction (back up, edit, update, build, commit, activate) | High | [confirmed](implicit-state-machines/reproductions.md#yuk-09) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) |
| [YUK-10](implicit-state-machines/yukimi.md#yuk-10) | Saved per-generation configuration snapshots | High | [confirmed](implicit-state-machines/reproductions.md#yuk-10) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) |
| [YUK-11](implicit-state-machines/yukimi.md#yuk-11) | Rollback and clean-up sequences | High | [confirmed](implicit-state-machines/reproductions.md#yuk-11) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) |
| [YUK-12](implicit-state-machines/yukimi.md#yuk-12) | Privileged operation client and worker (pkexec client, transient unit, result file) | High |  | implicit |
| [YUK-13](implicit-state-machines/yukimi.md#yuk-13) | X11 fallback nested Weston lifecycle | Low |  | implicit |
| [YUK-14](implicit-state-machines/yukimi.md#yuk-14) | GUI reading epochs (configuration, store, Nixpkgs path, deferred launch) | Medium |  | implicit |
| [YUK-15](implicit-state-machines/yukimi.md#yuk-15) | Package index lifecycle | High | [confirmed](implicit-state-machines/reproductions.md#yuk-15) | implicit |
| [YUK-16](implicit-state-machines/yukimi.md#yuk-16) | Update check (in-flight flag and persisted answer) | Medium |  | implicit |
| [YUK-17](implicit-state-machines/yukimi.md#yuk-17) | Search query coalescing | Low |  | implicit |
| [YUK-18](implicit-state-machines/yukimi.md#yuk-18) | GUI job record and progress sheet | High |  | implicit |
| [YUK-19](implicit-state-machines/yukimi.md#yuk-19) | Desktops page choice drafts | High | [confirmed](implicit-state-machines/reproductions.md#yuk-19) | implicit |
| [YUK-20](implicit-state-machines/yukimi.md#yuk-20) | Sheet lifecycles: package details, branch chooser, comparison | High | [confirmed](implicit-state-machines/reproductions.md#yuk-20) | implicit |
| [YUK-21](implicit-state-machines/yukimi.md#yuk-21) | History generation diffs | Medium |  | implicit |
| [YUK-22](implicit-state-machines/yukimi.md#yuk-22) | Status line notes | Low |  | implicit |
| [YUK-23](implicit-state-machines/yukimi.md#yuk-23) | Restart or sign out to finish an update | Low |  | implicit |
| [YUK-24](implicit-state-machines/yukimi.md#yuk-24) | Single-instance socket and hand-off | Medium |  | implicit |
| [YUK-25](implicit-state-machines/yukimi.md#yuk-25) | Job step runner with pkexec authentication recovery | Medium |  | implicit |
| [YUK-26](implicit-state-machines/yukimi.md#yuk-26) | (test harness) fake pkexec and stop timers | Low |  | implicit |
| [YUK-27](implicit-state-machines/yukimi.md#yuk-27) | Unprivileged update preparation pipeline | Medium |  | implicit |
| [YUK-28](implicit-state-machines/yukimi.md#yuk-28) | Falling snow animation ticks | Low |  | implicit |
| [YUK-29](implicit-state-machines/yukimi.md#yuk-29) | Per-input revision phase from saved generations | Low |  | implicit |
| [YUK-30](implicit-state-machines/yukimi.md#yuk-30) | (test harness) settle loops and scripted Nix/helper stand-ins | Low |  | implicit |

#### [NTU: Nisshi, Tokei and Utsushi](implicit-state-machines/nisshi-tokei-utsushi.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [NTU-01](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-01) | Nisshi system-log unlock (root reader through pkexec) | High | [confirmed](implicit-state-machines/reproductions.md#ntu-01) | implicit |
| [NTU-02](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-02) | Nisshi reader generations and pending-request table | Medium |  | implicit |
| [NTU-03](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-03) | Nisshi journal read pipeline: newest page, earlier pages, follow, whole-boot search | High | [confirmed](implicit-state-machines/reproductions.md#ntu-03) | implicit |
| [NTU-04](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-04) | Nisshi log-file scope and its read | Medium |  | implicit |
| [NTU-05](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-05) | Nisshi following the newest message, and the unseen count | Medium |  | implicit |
| [NTU-06](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-06) | Nisshi open-on-cursor pending selection | Low |  | implicit |
| [NTU-07](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-07) | Nisshi search typing debounce | Low |  | implicit |
| [NTU-08](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-08) | Nisshi view refresh coalescing and incremental extension | Medium |  | implicit |
| [NTU-09](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-09) | Nisshi selected message's fields fetch | Low |  | implicit |
| [NTU-10](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-10) | Nisshi timeline brush drag | Low |  | implicit |
| [NTU-11](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-11) | Nisshi reader client process (helper as the user, or through pkexec as root) | Medium |  | implicit |
| [NTU-12](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-12) | Nisshi helper's running reads and process tree | Medium |  | implicit |
| [NTU-13](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-13) | (test harness) Nisshi stand-in journalctl | Low |  | implicit |
| [NTU-14](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-14) | Single-instance socket claim and hand-off (Nisshi, Tokei, Utsushi) | Low |  | implicit |
| [NTU-15](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-15) | Nisshi `--print` one-shot read | Low | unverified | implicit |
| [NTU-16](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-16) | (test harness) Nisshi reader stage: stand-in pkexec, environment lock, polling waits | Low |  | implicit |
| [NTU-17](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-17) | Tokei alarm schedule: alarms.json against systemd transient timers | High | [confirmed](implicit-state-machines/reproductions.md#ntu-17) | implicit |
| [NTU-18](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-18) | (test harness) Tokei recording scheduler and fake-clock rig | Low |  | implicit |
| [NTU-19](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-19) | Tokei's in-memory alarm list, its saves and the undo slot | High | [confirmed](implicit-state-machines/reproductions.md#ntu-19) | implicit |
| [NTU-20](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-20) | Tokei sheets: Add City and the alarm editor | Medium | unverified | implicit |
| [NTU-21](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-21) | Tokei timer end check and tick phase | Low |  | implicit |
| [NTU-22](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-22) | Tokei stopwatch run state and frame timer | Low |  | implicit |
| [NTU-23](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-23) | Tokei state-file persist debounce | Low |  | implicit |
| [NTU-24](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-24) | Tokei alarm ring process (`aizome-tokei --ring`) | Medium | unverified | implicit |
| [NTU-25](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-25) | Tokei sound player child process | Low |  | implicit |
| [NTU-26](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-26) | Utsushi playback item lifecycle | High | [confirmed](implicit-state-machines/reproductions.md#ntu-26) | implicit |
| [NTU-27](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-27) | Utsushi timeline scrubbing | Medium |  | implicit |
| [NTU-28](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-28) | Utsushi full-screen controls auto-hide | High |  | implicit |
| [NTU-29](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-29) | Utsushi clock and subtitle wake timers | Low |  | implicit |
| [NTU-30](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-30) | Utsushi Media Info sheet | Low |  | implicit |
| [NTU-31](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-31) | Utsushi MPRIS service thread | Low |  | implicit |
| [NTU-32](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-32) | Utsushi persisted resume positions and recents | Low |  | implicit |
| [NTU-33](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-33) | (test harness) Utsushi real-player polling waits | Low |  | implicit |

#### [KKO: Kura and Koe](implicit-state-machines/kura-koe.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [KKO-01](implicit-state-machines/kura-koe.md#kko-01) | Dictation on/off: koe.toml `enabled` gating the user unit and the daemon it runs | High | [partly](implicit-state-machines/reproductions.md#kko-01) | implicit |
| [KKO-02](implicit-state-machines/kura-koe.md#kko-02) | Koe window's view of the service switch | High |  | implicit |
| [KKO-03](implicit-state-machines/kura-koe.md#kko-03) | Model download, set-up (download then enable), and closing while downloading | High |  | implicit |
| [KKO-04](implicit-state-machines/kura-koe.md#kko-04) | Window's status subscription reconnect loop | Low |  | implicit |
| [KKO-05](implicit-state-machines/kura-koe.md#kko-05) | Window's koe.toml and settings.toml change detection | Low |  | implicit |
| [KKO-06](implicit-state-machines/kura-koe.md#kko-06) | pw-record capture task | Low |  | implicit |
| [KKO-07](implicit-state-machines/kura-koe.md#kko-07) | Daemon dictation pipeline (record → transcribe → deliver) | High |  | implicit |
| [KKO-08](implicit-state-machines/kura-koe.md#kko-08) | Teaching session lease | High |  | implicit |
| [KKO-09](implicit-state-machines/kura-koe.md#kko-09) | Deferred settings reload and model warmup | Medium |  | implicit |
| [KKO-10](implicit-state-machines/kura-koe.md#kko-10) | Daemon IPC connection: request, subscription and shutdown flush | Low |  | implicit |
| [KKO-11](implicit-state-machines/kura-koe.md#kko-11) | Paste delivery guard | Medium |  | implicit |
| [KKO-12](implicit-state-machines/kura-koe.md#kko-12) | Resident Whisper model | Medium |  | implicit |
| [KKO-13](implicit-state-machines/kura-koe.md#kko-13) | Single-instance socket hand-off (Koe and Kura windows) | Low |  | implicit |
| [KKO-14](implicit-state-machines/kura-koe.md#kko-14) | Daemon single-instance lock and control socket file | Low |  | implicit |
| [KKO-15](implicit-state-machines/kura-koe.md#kko-15) | CLI vocabulary edit retry | Low |  | implicit |
| [KKO-16](implicit-state-machines/kura-koe.md#kko-16) | Model partial-download files | Low |  | implicit |
| [KKO-17](implicit-state-machines/kura-koe.md#kko-17) | Tray mirror of daemon status and its own error | Low |  | implicit |
| [KKO-18](implicit-state-machines/kura-koe.md#kko-18) | Vocabulary page: request queue, stale-read guard, teaching client, closing | High |  | implicit |
| [KKO-19](implicit-state-machines/kura-koe.md#kko-19) | Vocabulary store: revision token, lock and staged saves | Medium |  | implicit |
| [KKO-20](implicit-state-machines/kura-koe.md#kko-20) | (test harness) Real-Aizome dictation regression driver | Low |  | implicit |
| [KKO-21](implicit-state-machines/kura-koe.md#kko-21) | (test harness) Daemon process harness with fake microphone, ASR server, compositor and shell | Low |  | implicit |
| [KKO-22](implicit-state-machines/kura-koe.md#kko-22) | (test harness) Window set-up rig with a gated service and a stalling model server | Low |  | implicit |
| [KKO-23](implicit-state-machines/kura-koe.md#kko-23) | (test harness) Private D-Bus tray roundtrip with host restart | Low |  | implicit |
| [KKO-24](implicit-state-machines/kura-koe.md#kko-24) | CLI scan progress ticker | Low |  | implicit |
| [KKO-25](implicit-state-machines/kura-koe.md#kko-25) | Kura's on-disk caches: index.json/index.md and nix-store.json | Low |  | implicit |
| [KKO-26](implicit-state-machines/kura-koe.md#kko-26) | Parallel scanner work queue | Medium |  | implicit |
| [KKO-27](implicit-state-machines/kura-koe.md#kko-27) | Freedesktop Trash moves | Low |  | implicit |
| [KKO-28](implicit-state-machines/kura-koe.md#kko-28) | Tree arena tombstones and grafts | Medium |  | implicit |
| [KKO-29](implicit-state-machines/kura-koe.md#kko-29) | Dragging into the Collector (sunburst and list rows) | Medium |  | implicit |
| [KKO-30](implicit-state-machines/kura-koe.md#kko-30) | Kura scan lifecycle and page | High | [confirmed](implicit-state-machines/reproductions.md#kko-30) | implicit |
| [KKO-31](implicit-state-machines/kura-koe.md#kko-31) | Analysis and index writing after a scan or a change | High | [confirmed](implicit-state-machines/reproductions.md#kko-31) | implicit |
| [KKO-32](implicit-state-machines/kura-koe.md#kko-32) | Launch-time background loads (disks, Nix store size) | Low |  | implicit |
| [KKO-33](implicit-state-machines/kura-koe.md#kko-33) | Sunburst centre transition animation | Low |  | implicit |
| [KKO-34](implicit-state-machines/kura-koe.md#kko-34) | Destructive actions: confirm, move or empty, rescan, graft | High |  | implicit |
| [KKO-35](implicit-state-machines/kura-koe.md#kko-35) | Suggestions selection: focused hint, expanded, ticks and kept copies | Medium |  | implicit |
| [KKO-36](implicit-state-machines/kura-koe.md#kko-36) | Sunburst paint cache and invalidation counters | Low |  | implicit |
| [KKO-37](implicit-state-machines/kura-koe.md#kko-37) | (test harness) Kura window settle loops | Low |  | implicit |

#### [TSH: Tansu and Shiori](implicit-state-machines/tansu-shiori.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [TSH-01](implicit-state-machines/tansu-shiori.md#tsh-01) | (test harness) Corpus example's per-document worker driver | Low |  | implicit |
| [TSH-02](implicit-state-machines/tansu-shiori.md#tsh-02) | Shiori document open and worker-restart lifecycle | High | [confirmed](implicit-state-machines/reproductions.md#tsh-02) | implicit |
| [TSH-03](implicit-state-machines/tansu-shiori.md#tsh-03) | Changed-file watch and reload in place | High |  | implicit |
| [TSH-04](implicit-state-machines/tansu-shiori.md#tsh-04) | Tile/text request coalescing and drawing caches | High |  | implicit |
| [TSH-05](implicit-state-machines/tansu-shiori.md#tsh-05) | Sidebar preview prefetch (ThumbRows) | Medium |  | implicit |
| [TSH-06](implicit-state-machines/tansu-shiori.md#tsh-06) | Pinch / ❖-scroll zoom settle debounce | Low |  | implicit |
| [TSH-07](implicit-state-machines/tansu-shiori.md#tsh-07) | Pointer press/drag gesture on the pages | Medium |  | implicit |
| [TSH-08](implicit-state-machines/tansu-shiori.md#tsh-08) | Copy waiting for pages' text | High | [confirmed](implicit-state-machines/reproductions.md#tsh-08) | implicit |
| [TSH-09](implicit-state-machines/tansu-shiori.md#tsh-09) | Find session | High | [confirmed](implicit-state-machines/reproductions.md#tsh-09) | implicit |
| [TSH-10](implicit-state-machines/tansu-shiori.md#tsh-10) | Shiori sheets (password, go to page, info) | Low |  | implicit |
| [TSH-11](implicit-state-machines/tansu-shiori.md#tsh-11) | Tick-counted notice expiry and scroll-bar visibility | Low |  | implicit |
| [TSH-12](implicit-state-machines/tansu-shiori.md#tsh-12) | Per-document view modes (sidebar, tab, outline expansion, full screen) | Low |  | implicit |
| [TSH-13](implicit-state-machines/tansu-shiori.md#tsh-13) | Window bookkeeping: empty placeholder window and command target | Low |  | implicit |
| [TSH-14](implicit-state-machines/tansu-shiori.md#tsh-14) | Document worker supervisor (spawn, watchdog, restart, give up, close) | High |  | implicit |
| [TSH-15](implicit-state-machines/tansu-shiori.md#tsh-15) | Single-instance socket hand-off (Shiori and Tansu) | Medium | unverified | implicit |
| [TSH-16](implicit-state-machines/tansu-shiori.md#tsh-16) | Pages widget incremental-damage snapshot | Low |  | implicit |
| [TSH-17](implicit-state-machines/tansu-shiori.md#tsh-17) | Paint-to-program view-size feedback (Seen) | Low |  | implicit |
| [TSH-18](implicit-state-machines/tansu-shiori.md#tsh-18) | ActualText marked-content stack | Low |  | implicit |
| [TSH-19](implicit-state-machines/tansu-shiori.md#tsh-19) | Persisted reading place (places.json) | Low |  | implicit |
| [TSH-20](implicit-state-machines/tansu-shiori.md#tsh-20) | Stand-in pixel buffer pool | Low |  | implicit |
| [TSH-21](implicit-state-machines/tansu-shiori.md#tsh-21) | Worker process job queue and document lifecycle | Medium |  | implicit |
| [TSH-22](implicit-state-machines/tansu-shiori.md#tsh-22) | (test harness) Shiori settle and wait loops | Low |  | implicit |
| [TSH-23](implicit-state-machines/tansu-shiori.md#tsh-23) | Tansu UDisks snapshot refresh and drive fallback | Medium |  | implicit |
| [TSH-24](implicit-state-machines/tansu-shiori.md#tsh-24) | Tansu segment selection tracking | Medium |  | implicit |
| [TSH-25](implicit-state-machines/tansu-shiori.md#tsh-25) | Tansu sheet lifecycle (forms holding segment indexes) | High | [confirmed](implicit-state-machines/reproductions.md#tsh-25) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) |
| [TSH-26](implicit-state-machines/tansu-shiori.md#tsh-26) | Tansu apply session (Apply sheet → Run → Running sheet) | High | [confirmed](implicit-state-machines/reproductions.md#tsh-26) | implicit |
| [TSH-27](implicit-state-machines/tansu-shiori.md#tsh-27) | Tansu immediate operations in flight (mount, unlock, attach…) | Low |  | implicit |
| [TSH-28](implicit-state-machines/tansu-shiori.md#tsh-28) | Tansu filesystem-tool probing | Low |  | implicit |
| [TSH-29](implicit-state-machines/tansu-shiori.md#tsh-29) | (test harness) In-memory UDisks mock | Low |  | implicit |
| [TSH-30](implicit-state-machines/tansu-shiori.md#tsh-30) | Tansu pending plan and undo history | Medium |  | implicit |
| [TSH-31](implicit-state-machines/tansu-shiori.md#tsh-31) | Tansu run step engine (steps, effect waits, confirmation tokens) | Medium |  | implicit |
| [TSH-32](implicit-state-machines/tansu-shiori.md#tsh-32) | (test harness) Private dbus-daemon with a fake UDisks2 | Low |  | implicit |

#### [SMT: Sumi, Myaku and Tenki](implicit-state-machines/sumi-myaku-tenki.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [SMT-01](implicit-state-machines/sumi-myaku-tenki.md#smt-01) | GPU lens card choice, hover and projected frame | Low |  | implicit |
| [SMT-02](implicit-state-machines/sumi-myaku-tenki.md#smt-02) | Table selection, expansion and launch request across readings | High | [confirmed](implicit-state-machines/reproductions.md#smt-02) | implicit |
| [SMT-03](implicit-state-machines/sumi-myaku-tenki.md#smt-03) | Quit/pause confirmation and the status notice | High | [confirmed](implicit-state-machines/reproductions.md#smt-03) | implicit |
| [SMT-04](implicit-state-machines/sumi-myaku-tenki.md#smt-04) | Energy Meters utility window | Low |  | implicit |
| [SMT-05](implicit-state-machines/sumi-myaku-tenki.md#smt-05) | Single-instance socket claim and hand-over (Myaku, Sumi, Tenki) | Low |  | implicit |
| [SMT-06](implicit-state-machines/sumi-myaku-tenki.md#smt-06) | NVML library session and per-device sample cursor | Low |  | implicit |
| [SMT-07](implicit-state-machines/sumi-myaku-tenki.md#smt-07) | Sampler thread schedule and window commands | Medium |  | implicit |
| [SMT-08](implicit-state-machines/sumi-myaku-tenki.md#smt-08) | Per-process reading cache with staleness timers and a maps budget | Medium |  | implicit |
| [SMT-09](implicit-state-machines/sumi-myaku-tenki.md#smt-09) | DRM client scan and busy-delta baselines | Low |  | implicit |
| [SMT-10](implicit-state-machines/sumi-myaku-tenki.md#smt-10) | Power metering across readings and sleep | Medium | unverified | implicit |
| [SMT-11](implicit-state-machines/sumi-myaku-tenki.md#smt-11) | Group identity cache and launcher-index refresh cadence | Low |  | implicit |
| [SMT-12](implicit-state-machines/sumi-myaku-tenki.md#smt-12) | (test harness) scripted power meter that repeats its last reading | Low |  | implicit |
| [SMT-13](implicit-state-machines/sumi-myaku-tenki.md#smt-13) | Close-after-save for dirty documents | High | [confirmed](implicit-state-machines/reproductions.md#smt-13) | implicit |
| [SMT-14](implicit-state-machines/sumi-myaku-tenki.md#smt-14) | Link and Go to Line sheets | High | [confirmed](implicit-state-machines/reproductions.md#smt-14) | implicit |
| [SMT-15](implicit-state-machines/sumi-myaku-tenki.md#smt-15) | Keyboard-target document tracking | Low |  | implicit |
| [SMT-16](implicit-state-machines/sumi-myaku-tenki.md#smt-16) | Autosave store worker, session file and startup/quit phases | Medium |  | implicit |
| [SMT-17](implicit-state-machines/sumi-myaku-tenki.md#smt-17) | Document file, dirty and autosave lifecycle | High | [confirmed](implicit-state-machines/reproductions.md#smt-17) | implicit |
| [SMT-18](implicit-state-machines/sumi-myaku-tenki.md#smt-18) | Background file read generation | High | [confirmed](implicit-state-machines/reproductions.md#smt-18) | implicit |
| [SMT-19](implicit-state-machines/sumi-myaku-tenki.md#smt-19) | Find bar and search generation | High | [confirmed](implicit-state-machines/reproductions.md#smt-19) | implicit |
| [SMT-20](implicit-state-machines/sumi-myaku-tenki.md#smt-20) | Incremental highlight states and colouring-pass scheduling | High |  | implicit |
| [SMT-21](implicit-state-machines/sumi-myaku-tenki.md#smt-21) | (test harness) background-work settle loop and crash simulation | Low |  | implicit |
| [SMT-22](implicit-state-machines/sumi-myaku-tenki.md#smt-22) | Per-position forecast fetch, cache and refresh | Medium |  | implicit |
| [SMT-23](implicit-state-machines/sumi-myaku-tenki.md#smt-23) | Place search debounce, generation and early Return | High |  | implicit |
| [SMT-24](implicit-state-machines/sumi-myaku-tenki.md#smt-24) | Location consent and GeoClue lookup | High | [confirmed](implicit-state-machines/reproductions.md#smt-24) | implicit |
| [SMT-25](implicit-state-machines/sumi-myaku-tenki.md#smt-25) | Time-zone city bootstrap | Low |  | implicit |
| [SMT-26](implicit-state-machines/sumi-myaku-tenki.md#smt-26) | Kept places order and selection persistence | Low |  | implicit |
| [SMT-27](implicit-state-machines/sumi-myaku-tenki.md#smt-27) | Settings sheet | Low |  | implicit |
| [SMT-28](implicit-state-machines/sumi-myaku-tenki.md#smt-28) | (test harness) recorded Open-Meteo and GeoClue services | Low |  | implicit |
| [SMT-29](implicit-state-machines/sumi-myaku-tenki.md#smt-29) | Deferred file writer with per-path coalescing | Low |  | implicit |
| [SMT-30](implicit-state-machines/sumi-myaku-tenki.md#smt-30) | Place list press, drag and hover gesture | Medium |  | implicit |
| [SMT-31](implicit-state-machines/sumi-myaku-tenki.md#smt-31) | (test harness) private D-Bus daemon and fake GeoClue client | Low |  | implicit |
| [SMT-32](implicit-state-machines/sumi-myaku-tenki.md#smt-32) | (test harness) held search replies and drain loops | Low |  | implicit |
| [SMT-33](implicit-state-machines/sumi-myaku-tenki.md#smt-33) | (test harness) pending location service and poll loop | Low |  | implicit |

#### [SAP: Shashin, Soroban, Fusen and Annai](implicit-state-machines/small-apps.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [SAP-01](implicit-state-machines/small-apps.md#sap-01) | Annai tour page and scene clock | Low |  | implicit |
| [SAP-02](implicit-state-machines/small-apps.md#sap-02) | Single-instance socket claim and hand-off (Annai, Shashin, Soroban; Fusen through Sumi's copy) | High | [confirmed](implicit-state-machines/reproductions.md#sap-02) | implicit |
| [SAP-03](implicit-state-machines/small-apps.md#sap-03) | Annai first-run record and "Show at login" | Low |  | implicit |
| [SAP-04](implicit-state-machines/small-apps.md#sap-04) | Fusen note draft, autosave and protected close | High |  | implicit |
| [SAP-05](implicit-state-machines/small-apps.md#sap-05) | Fusen stickies: windows, saved places and the login item | High | [confirmed](implicit-state-machines/reproductions.md#sap-05) | implicit |
| [SAP-06](implicit-state-machines/small-apps.md#sap-06) | Fusen launch, request hand-off and Drop-time save | High | [confirmed](implicit-state-machines/reproductions.md#sap-06) | implicit |
| [SAP-07](implicit-state-machines/small-apps.md#sap-07) | Fusen highlighting passes (incomplete, scheduled, notify) | High | [confirmed](implicit-state-machines/reproductions.md#sap-07) | implicit |
| [SAP-08](implicit-state-machines/small-apps.md#sap-08) | Modal sheets: Fusen's New Folder, Shashin's Get Info | Low |  | implicit |
| [SAP-09](implicit-state-machines/small-apps.md#sap-09) | Fusen note Trash move | Low |  | implicit |
| [SAP-10](implicit-state-machines/small-apps.md#sap-10) | Shashin picture loading (serial, decode, folder listing) | High | [confirmed](implicit-state-machines/reproductions.md#sap-10) | implicit |
| [SAP-11](implicit-state-machines/small-apps.md#sap-11) | Shashin unsaved turns, saving and quitting | High | [confirmed](implicit-state-machines/reproductions.md#sap-11) | implicit |
| [SAP-12](implicit-state-machines/small-apps.md#sap-12) | Shashin zoom mode and centre | Low |  | implicit |
| [SAP-13](implicit-state-machines/small-apps.md#sap-13) | Shashin stage drawing requests (busy, want, reported) | High | [confirmed](implicit-state-machines/reproductions.md#sap-13) | implicit |
| [SAP-14](implicit-state-machines/small-apps.md#sap-14) | Shashin background errands counter | Low |  | implicit |
| [SAP-15](implicit-state-machines/small-apps.md#sap-15) | Shashin read-ahead cache | Low |  | implicit |
| [SAP-16](implicit-state-machines/small-apps.md#sap-16) | Shashin filmstrip thumbnails and their cache | Medium |  | implicit |
| [SAP-17](implicit-state-machines/small-apps.md#sap-17) | Shashin full screen and slideshow | Medium |  | implicit |
| [SAP-18](implicit-state-machines/small-apps.md#sap-18) | Shashin Move to Trash with Undo and Redo | High | [confirmed](implicit-state-machines/reproductions.md#sap-18) | implicit |
| [SAP-19](implicit-state-machines/small-apps.md#sap-19) | Seq-matched transient timers (Shashin's toolbar note, Soroban's key flash) | Low |  | implicit |
| [SAP-20](implicit-state-machines/small-apps.md#sap-20) | Shashin pan drag | Low |  | implicit |
| [SAP-21](implicit-state-machines/small-apps.md#sap-21) | (test harness) settle and poll loops | Low |  | implicit |
| [SAP-22](implicit-state-machines/small-apps.md#sap-22) | Soroban state persistence debounce | Low |  | implicit |
| [SAP-23](implicit-state-machines/small-apps.md#sap-23) | Soroban Save Tape sheet and replace confirmation | Medium |  | implicit |
| [SAP-24](implicit-state-machines/small-apps.md#sap-24) | Soroban bit grid hover and keyboard cursor | Low |  | implicit |
| [SAP-25](implicit-state-machines/small-apps.md#sap-25) | Soroban calculator entry (entry, answer, error, 2nd, undo) | High | [confirmed](implicit-state-machines/reproductions.md#sap-25) | implicit |
| [SAP-26](implicit-state-machines/small-apps.md#sap-26) | Soroban paper tape follow-the-end scrolling | Low |  | implicit |

### Rest of Sekka

#### [HCO: Hotaru terminal engine](implicit-state-machines/hotaru-core.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [HCO-01](implicit-state-machines/hotaru-core.md#hco-01) | Copy formatter open-scope emitter (style, link, deferred blanks) | Low |  | implicit |
| [HCO-02](implicit-state-machines/hotaru-core.md#hco-02) | Scrollback block lifecycle and incremental compression cursor | Medium |  | implicit |
| [HCO-03](implicit-state-machines/hotaru-core.md#hco-03) | Content identity tokens for asynchronous readers (epoch, origin, generation) | Medium |  | implicit |
| [HCO-04](implicit-state-machines/hotaru-core.md#hco-04) | Kitty keyboard progressive-enhancement flag stack (per screen) | Low |  | implicit |
| [HCO-05](implicit-state-machines/hotaru-core.md#hco-05) | Mouse report motion de-duplication | Low |  | implicit |
| [HCO-06](implicit-state-machines/hotaru-core.md#hco-06) | Kitty clipboard (OSC 5522) write transaction | High |  | implicit |
| [HCO-07](implicit-state-machines/hotaru-core.md#hco-07) | Kitty clipboard session password grants and paste-event one-time passwords | High | [confirmed](implicit-state-machines/reproductions.md#hco-07) | implicit |
| [HCO-08](implicit-state-machines/hotaru-core.md#hco-08) | History byte-budget signal (dirty/peak/pause accounting) | Medium |  | implicit |
| [HCO-09](implicit-state-machines/hotaru-core.md#hco-09) | VT/DEC escape-sequence parser | Low |  | implicit |
| [HCO-10](implicit-state-machines/hotaru-core.md#hco-10) | Alternate screen switching (DEC modes 47, 1047, 1049) | High |  | implicit |
| [HCO-11](implicit-state-machines/hotaru-core.md#hco-11) | Saved cursor (DECSC/DECRC, SCOSC/SCORC, 1048, 1049) | Medium |  | implicit |
| [HCO-12](implicit-state-machines/hotaru-core.md#hco-12) | Pending wrap and soft-wrap row linkage | High |  | implicit |
| [HCO-13](implicit-state-machines/hotaru-core.md#hco-13) | Scroll region and left/right margins | Medium |  | implicit |
| [HCO-14](implicit-state-machines/hotaru-core.md#hco-14) | Active OSC 8 hyperlink | Low |  | implicit |
| [HCO-15](implicit-state-machines/hotaru-core.md#hco-15) | OSC 133 semantic prompt state | Medium |  | implicit |
| [HCO-16](implicit-state-machines/hotaru-core.md#hco-16) | DEC/ANSI terminal mode flags, XTSAVE/XTRESTORE and soft reset | Medium |  | implicit |
| [HCO-17](implicit-state-machines/hotaru-core.md#hco-17) | Character set designation and shifts (G0–G3, SI/SO, LS2/LS3, SS2/SS3) | Low |  | implicit |
| [HCO-18](implicit-state-machines/hotaru-core.md#hco-18) | Cursor style (DECSCUSR, DEC mode 12, configured default) | High | [confirmed](implicit-state-machines/reproductions.md#hco-18) | implicit |
| [HCO-19](implicit-state-machines/hotaru-core.md#hco-19) | Dynamic color overrides (OSC 4/10/11 vs configured defaults) | Low |  | implicit |
| [HCO-20](implicit-state-machines/hotaru-core.md#hco-20) | Pending reply/event queues and per-feed clipboard bound | Medium |  | implicit |
| [HCO-21](implicit-state-machines/hotaru-core.md#hco-21) | Nonblocking clipboard pipe transfer (writer and reader) | Low |  | implicit |
| [HCO-22](implicit-state-machines/hotaru-core.md#hco-22) | Thread-scoped SIGPIPE block guard | Low |  | implicit |
| [HCO-23](implicit-state-machines/hotaru-core.md#hco-23) | Default-shell integration bootstrap handoff (bash rcfile, zsh ZDOTDIR swap, fish init command) | Low |  | implicit |
| [HCO-24](implicit-state-machines/hotaru-core.md#hco-24) | PTY child lifecycle and background reaper (HUP → grace → KILL → wait) | High | [confirmed](implicit-state-machines/reproductions.md#hco-24) | implicit |
| [HCO-25](implicit-state-machines/hotaru-core.md#hco-25) | (test harness) Kitty clipboard embedder model replay | Low |  | implicit |
| [HCO-26](implicit-state-machines/hotaru-core.md#hco-26) | Shell prompt hooks: load guards and last-reported-cwd de-duplication (OSC 7) | High | [confirmed](implicit-state-machines/reproductions.md#hco-26) | implicit |

#### [HTT: Hotaru engine tests and development tools](implicit-state-machines/hotaru-tests-tools.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [HTT-01](implicit-state-machines/hotaru-tests-tools.md#htt-01) | (test harness) Clipboard transfer pump/poll drivers | Low |  | implicit |
| [HTT-02](implicit-state-machines/hotaru-tests-tools.md#htt-02) | (test harness) SIGPIPE child re-exec and signal-mask fixtures | Low |  | implicit |
| [HTT-03](implicit-state-machines/hotaru-tests-tools.md#htt-03) | (test harness) PTY child lifecycle waits | Low |  | implicit |
| [HTT-04](implicit-state-machines/hotaru-tests-tools.md#htt-04) | (test harness) Shell-startup PTY driver (parent/child re-exec) | Low |  | implicit |
| [HTT-05](implicit-state-machines/hotaru-tests-tools.md#htt-05) | (dev tool) Shared workstation measurement lock | Medium | unverified | implicit |
| [HTT-06](implicit-state-machines/hotaru-tests-tools.md#htt-06) | (dev tool) Validated alternating benchmark round loops | Low |  | implicit |
| [HTT-07](implicit-state-machines/hotaru-tests-tools.md#htt-07) | (dev tool) Build provenance metadata gate (`build_confirmed`, hashes) | Medium | unverified | implicit |
| [HTT-08](implicit-state-machines/hotaru-tests-tools.md#htt-08) | (dev tool) libghostty-vt ctypes terminal session lifecycle | Low |  | implicit |
| [HTT-09](implicit-state-machines/hotaru-tests-tools.md#htt-09) | (dev tool) Frame benchmark PTY fixture protocol | Medium |  | implicit |
| [HTT-10](implicit-state-machines/hotaru-tests-tools.md#htt-10) | (dev tool) Frame benchmark controller run lifecycle | High | [confirmed](implicit-state-machines/reproductions.md#htt-10) | implicit |
| [HTT-11](implicit-state-machines/hotaru-tests-tools.md#htt-11) | (dev tool) JSON event reader thread and `until` waits | Low |  | implicit |
| [HTT-12](implicit-state-machines/hotaru-tests-tools.md#htt-12) | (dev tool) Frame qualification → calibration → matrix → summary pipeline | Medium |  | implicit |
| [HTT-13](implicit-state-machines/hotaru-tests-tools.md#htt-13) | (test harness) Frame fixture and observer qualification drivers | Low |  | implicit |
| [HTT-14](implicit-state-machines/hotaru-tests-tools.md#htt-14) | (dev tool) Frame matrix batch status and trial acceptance | High | [confirmed](implicit-state-machines/reproductions.md#htt-14) | implicit |
| [HTT-15](implicit-state-machines/hotaru-tests-tools.md#htt-15) | (dev tool) Frame matrix trial process lifecycle and orphan cleanup | Medium |  | implicit |
| [HTT-16](implicit-state-machines/hotaru-tests-tools.md#htt-16) | (dev tool) Frame matrix preflight quiet-wait retry loop | Low |  | implicit |
| [HTT-17](implicit-state-machines/hotaru-tests-tools.md#htt-17) | (dev tool) Frame observer screencopy capture cycle | Medium |  | implicit |
| [HTT-18](implicit-state-machines/hotaru-tests-tools.md#htt-18) | (dev tool) Frame observer marker geometry cache | Low | unverified | implicit |
| [HTT-19](implicit-state-machines/hotaru-tests-tools.md#htt-19) | (dev tool) GPU buffer qualification build/run variants | Low |  | implicit |
| [HTT-20](implicit-state-machines/hotaru-tests-tools.md#htt-20) | (dev tool) EGL offscreen probe stage and current-context sequence | Low |  | implicit |
| [HTT-21](implicit-state-machines/hotaru-tests-tools.md#htt-21) | (dev tool) GUI probe private session and DSR fixture | Low |  | implicit |
| [HTT-22](implicit-state-machines/hotaru-tests-tools.md#htt-22) | (dev tool) Input oracle shared encoder/event handles across cases | Low |  | implicit |
| [HTT-23](implicit-state-machines/hotaru-tests-tools.md#htt-23) | (dev tool) Upstream port-ledger per-file status | Low |  | implicit |
| [HTT-24](implicit-state-machines/hotaru-tests-tools.md#htt-24) | (dev tool) Kitty clipboard oracle scripted embedder | Low |  | implicit |
| [HTT-25](implicit-state-machines/hotaru-tests-tools.md#htt-25) | (dev tool) Process-timing run watchdog | Low |  | implicit |
| [HTT-26](implicit-state-machines/hotaru-tests-tools.md#htt-26) | (dev tool) Pinned reference oracle prepare/build/metadata stages | Medium |  | implicit |
| [HTT-27](implicit-state-machines/hotaru-tests-tools.md#htt-27) | (dev tool) Two-phase differential report → compact fixture generators | Low |  | implicit |
| [HTT-28](implicit-state-machines/hotaru-tests-tools.md#htt-28) | (dev tool) print_utf8 experiment variants (run accumulator, pending wrap) | Low |  | implicit |

#### [GEN: Genkan installer](implicit-state-machines/genkan.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [GEN-01](implicit-state-machines/genkan.md#gen-01) | (test harness) Encrypted-storage and boot-splash VM drivers | Low |  | implicit |
| [GEN-02](implicit-state-machines/genkan.md#gen-02) | Generated initrd unlock sequence (root first, swap with root's passphrase, no device timeout) | Low |  | implicit |
| [GEN-03](implicit-state-machines/genkan.md#gen-03) | Selected-disk identity: discovered → reviewed → revalidated before and after the first write | High | [confirmed](implicit-state-machines/reproductions.md#gen-03) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) |
| [GEN-04](implicit-state-machines/genkan.md#gen-04) | LUKS mapping registry and its cleanup order | Medium |  | implicit |
| [GEN-05](implicit-state-machines/genkan.md#gen-05) | (test harness) Loop-device image guard in the reformat matrix | Low |  | implicit |
| [GEN-06](implicit-state-machines/genkan.md#gen-06) | Privileged helper session: request → preparation → confirmation or cancel → execution | High |  | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) |
| [GEN-07](implicit-state-machines/genkan.md#gen-07) | Install transaction: non-destructive preparation stages, then destructive steps 1–6 | High | [confirmed](implicit-state-machines/reproductions.md#gen-07) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) |
| [GEN-08](implicit-state-machines/genkan.md#gen-08) | Install target mount guard | Low |  | implicit |
| [GEN-09](implicit-state-machines/genkan.md#gen-09) | Installer exclusivity lock and the partition-table disk lock | Low |  | implicit |
| [GEN-10](implicit-state-machines/genkan.md#gen-10) | (test harness) Encrypted storage matrix in the disposable VM | Low |  | implicit |
| [GEN-11](implicit-state-machines/genkan.md#gen-11) | Nix internal-json activity tracker and copy-progress throttle | Low |  | implicit |
| [GEN-12](implicit-state-machines/genkan.md#gen-12) | Sleep/idle inhibitor during installation | Low |  | implicit |
| [GEN-13](implicit-state-machines/genkan.md#gen-13) | Page-cache warming worker pool | Medium |  | implicit |
| [GEN-14](implicit-state-machines/genkan.md#gen-14) | Subprocess run loop: spawn, drain, poll for deadline or cancel, kill the group | Low |  | implicit |
| [GEN-15](implicit-state-machines/genkan.md#gen-15) | GUI-side helper Session handle (pkexec child, writer and reader threads) | Medium |  | implicit |
| [GEN-16](implicit-state-machines/genkan.md#gen-16) | Plymouth: unanswered prompts per client connection, dropped when the client hangs up (patch) | Low |  | implicit |
| [GEN-17](implicit-state-machines/genkan.md#gen-17) | Plymouth: ownership of the local console terminal across renderer takeover (patch) | Low |  | implicit |
| [GEN-18](implicit-state-machines/genkan.md#gen-18) | Plymouth: which keyboard the splash reads, across keyboard removal (patch) | Low |  | implicit |
| [GEN-19](implicit-state-machines/genkan.md#gen-19) | Sekka splash theme: rail, prompt and hint phases | Medium |  | implicit |
| [GEN-20](implicit-state-machines/genkan.md#gen-20) | Aizome IBus frontend service: exit classification at logout and restart | Low |  | implicit |
| [GEN-21](implicit-state-machines/genkan.md#gen-21) | User-manager environment record and per-login reset | Low |  | implicit |
| [GEN-22](implicit-state-machines/genkan.md#gen-22) | USB keyboard and mouse wake path enablement | Low |  | implicit |
| [GEN-23](implicit-state-machines/genkan.md#gen-23) | wpa_supplicant pending scan-result handler left behind by a disabled interface (patch) | Low |  | implicit |
| [GEN-24](implicit-state-machines/genkan.md#gen-24) | (test harness) VM fixture's helper-session driver | Low |  | implicit |
| [GEN-25](implicit-state-machines/genkan.md#gen-25) | (test harness) VM fixture stages and the PAM pty conversation | Low |  | implicit |
| [GEN-26](implicit-state-machines/genkan.md#gen-26) | Wizard navigation and page locking | Medium |  | implicit |
| [GEN-27](implicit-state-machines/genkan.md#gen-27) | Bottom-bar activity slot and timed notes | Low |  | implicit |
| [GEN-28](implicit-state-machines/genkan.md#gen-28) | Disk scan, selection and installed-memory read | High | [confirmed](implicit-state-machines/reproductions.md#gen-28) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) |
| [GEN-29](implicit-state-machines/genkan.md#gen-29) | Derived fields that follow until edited (username, Git name, Git email) | Low |  | implicit |
| [GEN-30](implicit-state-machines/genkan.md#gen-30) | GitHub account lookup | Medium |  | implicit |
| [GEN-31](implicit-state-machines/genkan.md#gen-31) | Time zone detection versus manual choice | Medium |  | implicit |
| [GEN-32](implicit-state-machines/genkan.md#gen-32) | World-map hover and click-to-pick | Low |  | implicit |
| [GEN-33](implicit-state-machines/genkan.md#gen-33) | Keyboard step: catching the key beside the space bar through the compositor | High | [partly](implicit-state-machines/reproductions.md#gen-33) | implicit |
| [GEN-34](implicit-state-machines/genkan.md#gen-34) | Review, background preparation and confirmation (GUI side of the helper session) | High | [confirmed](implicit-state-machines/reproductions.md#gen-34) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) |
| [GEN-35](implicit-state-machines/genkan.md#gen-35) | Installation progress and outcome on the Install page | High |  | implicit |
| [GEN-36](implicit-state-machines/genkan.md#gen-36) | Speculative page-cache warming of the current selection | Medium |  | implicit |
| [GEN-37](implicit-state-machines/genkan.md#gen-37) | (test harness) Preview and sample-data driver | Low |  | implicit |

#### [TTL: Tatami command, display watcher and configuration](implicit-state-machines/tatami-tool.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [TTL-01](implicit-state-machines/tatami-tool.md#ttl-01) | Mako Do Not Disturb mode | Low |  | implicit |
| [TTL-02](implicit-state-machines/tatami-tool.md#ttl-02) | Tatami session unit lifecycle and restart policies | Low |  | implicit |
| [TTL-03](implicit-state-machines/tatami-tool.md#ttl-03) | Keep Awake: intent marker, inhibitor holder unit and verification | High | [confirmed](implicit-state-machines/reproductions.md#ttl-03) | implicit |
| [TTL-04](implicit-state-machines/tatami-tool.md#ttl-04) | Wallpaper choice and per-output overrides | High | [confirmed](implicit-state-machines/reproductions.md#ttl-04) | implicit |
| [TTL-05](implicit-state-machines/tatami-tool.md#ttl-05) | Style › Background settings menu flow | Low |  | implicit |
| [TTL-06](implicit-state-machines/tatami-tool.md#ttl-06) | Screenshot selection and screen freeze | Medium |  | implicit |
| [TTL-07](implicit-state-machines/tatami-tool.md#ttl-07) | Screenshot Edit notification action | Low |  | implicit |
| [TTL-08](implicit-state-machines/tatami-tool.md#ttl-08) | Screen recording start, fallback and stop | High | [confirmed](implicit-state-machines/reproductions.md#ttl-08) | implicit |
| [TTL-09](implicit-state-machines/tatami-tool.md#ttl-09) | Display session record: closed-display mode, lid edges, remembered off choices | High |  | implicit |
| [TTL-10](implicit-state-machines/tatami-tool.md#ttl-10) | Display reconciliation commit (plan execution) | High | [confirmed](implicit-state-machines/reproductions.md#ttl-10) | implicit |
| [TTL-11](implicit-state-machines/tatami-tool.md#ttl-11) | Display watcher event loop | Medium |  | implicit |
| [TTL-12](implicit-state-machines/tatami-tool.md#ttl-12) | Lid state tracker (bindings plus logind poll) | Medium |  | implicit |
| [TTL-13](implicit-state-machines/tatami-tool.md#ttl-13) | XWayland Xft.dpi retry | Medium |  | implicit |
| [TTL-14](implicit-state-machines/tatami-tool.md#ttl-14) | Window rescue and settings-window fit grace periods | Low |  | implicit |
| [TTL-15](implicit-state-machines/tatami-tool.md#ttl-15) | Displays apply transaction | Medium |  | implicit |
| [TTL-16](implicit-state-machines/tatami-tool.md#ttl-16) | Display settings store and legacy scale migration | Low |  | implicit |
| [TTL-17](implicit-state-machines/tatami-tool.md#ttl-17) | Live reload pipeline (`tatami reload`, `update-session`) | High | [confirmed](implicit-state-machines/reproductions.md#ttl-17) | implicit |
| [TTL-18](implicit-state-machines/tatami-tool.md#ttl-18) | Walker menu open/close handoff | Low |  | implicit |
| [TTL-19](implicit-state-machines/tatami-tool.md#ttl-19) | Wi-Fi join flow | Low |  | implicit |
| [TTL-20](implicit-state-machines/tatami-tool.md#ttl-20) | Lock, idle and DPMS | Low |  | implicit |
| [TTL-21](implicit-state-machines/tatami-tool.md#ttl-21) | Power meter socket service and reading throttle | Low |  | implicit |
| [TTL-22](implicit-state-machines/tatami-tool.md#ttl-22) | Mixed-audio recorder children (SessionChild, AudioMix) | Medium |  | implicit |
| [TTL-23](implicit-state-machines/tatami-tool.md#ttl-23) | Night Light (hyprsunset) | Medium |  | implicit |
| [TTL-24](implicit-state-machines/tatami-tool.md#ttl-24) | Session update receipt | Medium |  | implicit |
| [TTL-25](implicit-state-machines/tatami-tool.md#ttl-25) | Weather report cache and fetch coordination | High | [confirmed](implicit-state-machines/reproductions.md#ttl-25) | implicit |
| [TTL-26](implicit-state-machines/tatami-tool.md#ttl-26) | Session monitor state files (monitors-off, monitors.lua) | High |  | implicit |
| [TTL-27](implicit-state-machines/tatami-tool.md#ttl-27) | Remembered Hyprland toggles | Low |  | implicit |
| [TTL-28](implicit-state-machines/tatami-tool.md#ttl-28) | Displays window presentation and replacement | Medium |  | implicit |
| [TTL-29](implicit-state-machines/tatami-tool.md#ttl-29) | Pop-out window toggle | Low |  | implicit |

#### [TAP: Tatami settings apps](implicit-state-machines/tatami-apps.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [TAP-01](implicit-state-machines/tatami-apps.md#tap-01) | Activity inspector's subscription to the selected row, with a layout shape fixed at selection | Low |  | implicit |
| [TAP-02](implicit-state-machines/tatami-apps.md#tap-02) | Activity sampler thread lifecycle and schedule | Low |  | implicit |
| [TAP-03](implicit-state-machines/tatami-apps.md#tap-03) | Activity sampler's baselines, caches and energy integrals between readings | High | [confirmed](implicit-state-machines/reproductions.md#tap-03) | implicit |
| [TAP-04](implicit-state-machines/tatami-apps.md#tap-04) | Activity process table: columns per view and remembered sorts | High | [confirmed](implicit-state-machines/reproductions.md#tap-04) | implicit |
| [TAP-05](implicit-state-machines/tatami-apps.md#tap-05) | Activity table cells bound to a row's `changed` signal | Low |  | implicit |
| [TAP-06](implicit-state-machines/tatami-apps.md#tap-06) | Window singleton, deferred first presentation and launch requests held until the first reading (Activity, Network, Sound) | High | [confirmed](implicit-state-machines/reproductions.md#tap-06) | implicit |
| [TAP-07](implicit-state-machines/tatami-apps.md#tap-07) | Activity's view and grouping mode, held in seven places | Medium |  | implicit |
| [TAP-08](implicit-state-machines/tatami-apps.md#tap-08) | Activity's Quit, Force Quit and Pause on a target taken from the latest frame | High | [confirmed](implicit-state-machines/reproductions.md#tap-08) | implicit |
| [TAP-09](implicit-state-machines/tatami-apps.md#tap-09) | Bluetooth pairing: the Pair call, the Agent1 prompt and cancellation | Medium |  | implicit |
| [TAP-10](implicit-state-machines/tatami-apps.md#tap-10) | BlueZ backend session tied to the service's bus owner, with agent registration | Medium |  | implicit |
| [TAP-11](implicit-state-machines/tatami-apps.md#tap-11) | Bluetooth discovery session owned by this connection | Medium |  | implicit |
| [TAP-12](implicit-state-machines/tatami-apps.md#tap-12) | (test harness) Fake BlueZ on a private dbus-daemon | Low |  | implicit |
| [TAP-13](implicit-state-machines/tatami-apps.md#tap-13) | Bluetooth readings and actions serialised with a revision counter | High |  | implicit |
| [TAP-14](implicit-state-machines/tatami-apps.md#tap-14) | Bluetooth device settings dialog | Low |  | implicit |
| [TAP-15](implicit-state-machines/tatami-apps.md#tap-15) | Bluetooth window close: hide, drain, release, destroy | High | [confirmed](implicit-state-machines/reproductions.md#tap-15) | implicit |
| [TAP-16](implicit-state-machines/tatami-apps.md#tap-16) | Displays arrangement tile drag | Medium |  | implicit |
| [TAP-17](implicit-state-machines/tatami-apps.md#tap-17) | A display's use (main, extended, mirror, off) and the main display | Low |  | implicit |
| [TAP-18](implicit-state-machines/tatami-apps.md#tap-18) | Displays' Hyprland event-socket watcher thread | Low |  | implicit |
| [TAP-19](implicit-state-machines/tatami-apps.md#tap-19) | Displays readings and applies (concurrent changes refused, stale readings discarded) | High | [confirmed](implicit-state-machines/reproductions.md#tap-19) | implicit |
| [TAP-20](implicit-state-machines/tatami-apps.md#tap-20) | Keep-or-revert countdown after a risky display change | Medium |  | implicit |
| [TAP-21](implicit-state-machines/tatami-apps.md#tap-21) | Night Shift requests on unordered threads | High | [confirmed](implicit-state-machines/reproductions.md#tap-21) | implicit |
| [TAP-22](implicit-state-machines/tatami-apps.md#tap-22) | Displays' debounced slider writes | Low |  | implicit |
| [TAP-23](implicit-state-machines/tatami-apps.md#tap-23) | Network connection details page: the open page, its settings and their version | Medium |  | implicit |
| [TAP-24](implicit-state-machines/tatami-apps.md#tap-24) | IPv6 DNS servers suspended in the NetworkManager profile while IPv6 is off | Medium |  | implicit |
| [TAP-25](implicit-state-machines/tatami-apps.md#tap-25) | Joining a Wi-Fi network: one at a time, with a password retry and rollback | High | [confirmed](implicit-state-machines/reproductions.md#tap-25) | implicit |
| [TAP-26](implicit-state-machines/tatami-apps.md#tap-26) | Network's `nmcli monitor` child process | Low |  | implicit |
| [TAP-27](implicit-state-machines/tatami-apps.md#tap-27) | Network readings: one at a time with one queued, change bursts and the poll | Low |  | implicit |
| [TAP-28](implicit-state-machines/tatami-apps.md#tap-28) | Network's Wi-Fi scan with a fixed six-second spinner | Low |  | implicit |
| [TAP-29](implicit-state-machines/tatami-apps.md#tap-29) | Network's Wi-Fi radio and VPN switches, dispatched without serialisation | Medium |  | implicit |
| [TAP-30](implicit-state-machines/tatami-apps.md#tap-30) | Sound's `pactl subscribe` with a restart loop | Low |  | implicit |
| [TAP-31](implicit-state-machines/tatami-apps.md#tap-31) | Sound's input level meter: the libpulse thread and its follower | Medium |  | implicit |
| [TAP-32](implicit-state-machines/tatami-apps.md#tap-32) | Sound's serial change queue and reading scheduler | Medium |  | implicit |
| [TAP-33](implicit-state-machines/tatami-apps.md#tap-33) | Section rows cached by a content key and updated by bindings (Sound, Network) | Low |  | implicit |

#### [KTL: Kakehashi, respin tools and scripts](implicit-state-machines/kakehashi-tools.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [KTL-01](implicit-state-machines/kakehashi-tools.md#ktl-01) | Kakehashi image selection and checksum check | Medium |  | implicit |
| [KTL-02](implicit-state-machines/kakehashi-tools.md#ktl-02) | Kakehashi nightly download session | High | [confirmed](implicit-state-machines/reproductions.md#ktl-02) | implicit |
| [KTL-03](implicit-state-machines/kakehashi-tools.md#ktl-03) | Kakehashi write job, GUI side of the helper protocol | High | [confirmed](implicit-state-machines/reproductions.md#ktl-03) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) |
| [KTL-04](implicit-state-machines/kakehashi-tools.md#ktl-04) | Kakehashi drive list polling and chosen drive | High | [confirmed](implicit-state-machines/reproductions.md#ktl-04) | reified ([#290](https://github.com/bitemyapp/sekka/pull/290)) |
| [KTL-05](implicit-state-machines/kakehashi-tools.md#ktl-05) | Resumable nightly download on disk | Medium |  | implicit |
| [KTL-06](implicit-state-machines/kakehashi-tools.md#ktl-06) | Adaptive range-connection controller | Low |  | implicit |
| [KTL-07](implicit-state-machines/kakehashi-tools.md#ktl-07) | Range-support mode: probe, sequential fallback, recheck | Medium |  | implicit |
| [KTL-08](implicit-state-machines/kakehashi-tools.md#ktl-08) | Ordered range-commit pipeline with per-range retry | Medium |  | implicit |
| [KTL-09](implicit-state-machines/kakehashi-tools.md#ktl-09) | (test harness) Fault-injecting range HTTP server | Low |  | implicit |
| [KTL-10](implicit-state-machines/kakehashi-tools.md#ktl-10) | macOS authopen descriptor broker | Low |  | implicit |
| [KTL-11](implicit-state-machines/kakehashi-tools.md#ktl-11) | Windows volume lock, dismount and hold | Medium |  | implicit |
| [KTL-12](implicit-state-machines/kakehashi-tools.md#ktl-12) | Helper write/verify pipeline and `touched` | High | [environment](implicit-state-machines/reproductions.md#ktl-12) | implicit |
| [KTL-13](implicit-state-machines/kakehashi-tools.md#ktl-13) | Rootless builder VM host driver | High | [confirmed](implicit-state-machines/reproductions.md#ktl-13) | implicit |
| [KTL-14](implicit-state-machines/kakehashi-tools.md#ktl-14) | Builder guest disk and store preparation markers | Medium |  | implicit |
| [KTL-15](implicit-state-machines/kakehashi-tools.md#ktl-15) | Wi-Fi test tools NAR cache | Low |  | implicit |
| [KTL-16](implicit-state-machines/kakehashi-tools.md#ktl-16) | (test harness) Live boot and input-delivery test phases | Low |  | implicit |
| [KTL-17](implicit-state-machines/kakehashi-tools.md#ktl-17) | Child supervision and interrupt latch | Medium |  | implicit |
| [KTL-18](implicit-state-machines/kakehashi-tools.md#ktl-18) | (test harness) QMP/QGA request correlation and guest-exec polling | Low |  | implicit |
| [KTL-19](implicit-state-machines/kakehashi-tools.md#ktl-19) | (test harness) QEMU VM handle lifecycle | Medium |  | implicit |
| [KTL-20](implicit-state-machines/kakehashi-tools.md#ktl-20) | (test harness) qemu-test install/reboot run and resumable evidence | High | [partly](implicit-state-machines/reproductions.md#ktl-20) | implicit |
| [KTL-21](implicit-state-machines/kakehashi-tools.md#ktl-21) | (test harness) Wi-Fi test: host boot and guest simulated-AP lifecycle | Low |  | implicit |
| [KTL-22](implicit-state-machines/kakehashi-tools.md#ktl-22) | PR trust gate merge-commit polling | Low |  | implicit |
| [KTL-23](implicit-state-machines/kakehashi-tools.md#ktl-23) | CI GC-root generation retention | Medium |  | implicit |
| [KTL-24](implicit-state-machines/kakehashi-tools.md#ktl-24) | flash_usb privileged write and verify step | High | [environment](implicit-state-machines/reproductions.md#ktl-24) | implicit |
| [KTL-25](implicit-state-machines/kakehashi-tools.md#ktl-25) | flash_usb interactive selection and sudo hand-off | Low |  | implicit |
| [KTL-26](implicit-state-machines/kakehashi-tools.md#ktl-26) | Live diagnostics bounded command capture | Low |  | implicit |
| [KTL-27](implicit-state-machines/kakehashi-tools.md#ktl-27) | Release asset staging directory | Low |  | implicit |
| [KTL-28](implicit-state-machines/kakehashi-tools.md#ktl-28) | Qualification report review and audit gate | Medium |  | implicit |
| [KTL-29](implicit-state-machines/kakehashi-tools.md#ktl-29) | Nightly publication to R2 | High |  | implicit |
| [KTL-30](implicit-state-machines/kakehashi-tools.md#ktl-30) | CI Git object cache refresh and seed | Low |  | implicit |
| [KTL-31](implicit-state-machines/kakehashi-tools.md#ktl-31) | Runner host setup convergence and KVM access migration | Low |  | implicit |

#### [INX: Nix integration, carried patches and live-media modules](implicit-state-machines/infra-nix.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [INX-01](implicit-state-machines/infra-nix.md#inx-01) | (test harness) Deadline-poll helpers and compositor-readiness waits shared by the VM test scripts | Low |  | implicit |
| [INX-02](implicit-state-machines/infra-nix.md#inx-02) | (test harness) Activity: stand-in power meter and pause/resume probes | Low |  | implicit |
| [INX-03](implicit-state-machines/infra-nix.md#inx-03) | (test harness) Greeter session cycle with remembered last session | Low |  | implicit |
| [INX-04](implicit-state-machines/infra-nix.md#inx-04) | (test harness) AccountsService session seed, readiness window and activation fixtures | Low |  | implicit |
| [INX-05](implicit-state-machines/infra-nix.md#inx-05) | (test harness) Aizome login → session → logout environment lifecycle | Low |  | implicit |
| [INX-06](implicit-state-machines/infra-nix.md#inx-06) | (test harness) Wallpaper menu-panel driver with reject-keeps-state invariant | Low |  | implicit |
| [INX-07](implicit-state-machines/infra-nix.md#inx-07) | (test harness) Boot-splash hold gate and Plymouth mode driver | Low |  | implicit |
| [INX-08](implicit-state-machines/infra-nix.md#inx-08) | (test harness) Capture-cursor compositor driver | Low |  | implicit |
| [INX-09](implicit-state-machines/infra-nix.md#inx-09) | ChatGPT executor plugin copy serialized per destination | Low |  | implicit |
| [INX-10](implicit-state-machines/infra-nix.md#inx-10) | (test harness) CI GC-root retention driver | Low |  | implicit |
| [INX-11](implicit-state-machines/infra-nix.md#inx-11) | (test harness) Display recovery: fake lid and dock/undock/lid driver | Low |  | implicit |
| [INX-12](implicit-state-machines/infra-nix.md#inx-12) | (test harness) Genkan keyboard-step driver over the kit's inspection files | Low |  | implicit |
| [INX-13](implicit-state-machines/infra-nix.md#inx-13) | (test harness) Live-media guard mount-state fixture | Low |  | implicit |
| [INX-14](implicit-state-machines/infra-nix.md#inx-14) | (test harness) Genkan helper preflight stage boundary | Low |  | implicit |
| [INX-15](implicit-state-machines/infra-nix.md#inx-15) | Googlebook vboot kernel-partition attempt attributes | Low |  | implicit |
| [INX-16](implicit-state-machines/infra-nix.md#inx-16) | (test harness) Hyprlauncher open/cancel/reopen with a resident daemon | Low |  | implicit |
| [INX-17](implicit-state-machines/infra-nix.md#inx-17) | Installed flake lock: as-installed pin, then following `nightly` | Low |  | implicit |
| [INX-18](implicit-state-machines/infra-nix.md#inx-18) | Copy-to-RAM of the live media in the initrd | High | [partly](implicit-state-machines/reproductions.md#inx-18) | implicit |
| [INX-19](implicit-state-machines/infra-nix.md#inx-19) | (test harness) Kakehashi helper write through pkexec with progress and cancel files | Low |  | implicit |
| [INX-20](implicit-state-machines/infra-nix.md#inx-20) | (test harness) Keep-awake inhibitor lifecycle | Low |  | implicit |
| [INX-21](implicit-state-machines/infra-nix.md#inx-21) | (test harness) Network DNS/IPv6 page driver with settle detector and mirrored UI mode | Low |  | implicit |
| [INX-22](implicit-state-machines/infra-nix.md#inx-22) | (test harness) Wi-Fi join flows over simulated access points | Low |  | implicit |
| [INX-23](implicit-state-machines/infra-nix.md#inx-23) | Aquamarine DRM async-commit-event flush during teardown (patched) | Low |  | implicit |
| [INX-24](implicit-state-machines/infra-nix.md#inx-24) | Avahi startup SIGHUP window (patched) | Low |  | implicit |
| [INX-25](implicit-state-machines/infra-nix.md#inx-25) | Hyprland cursor-free capture image and its per-frame zoom snapshot (patched) | Medium |  | implicit |
| [INX-26](implicit-state-machines/infra-nix.md#inx-26) | Hyprland managed screencopy session lifecycle (patched) | High |  | implicit |
| [INX-27](implicit-state-machines/infra-nix.md#inx-27) | Hyprtoolkit window buffer allocation fallback ladder (patched) | Medium |  | implicit |
| [INX-28](implicit-state-machines/infra-nix.md#inx-28) | IBus Wayland input-method commit serial (patched) | Low |  | implicit |
| [INX-29](implicit-state-machines/infra-nix.md#inx-29) | IBus Wayland frontend keymaps, seat and system-keymap mode (patched) | High | [partly](implicit-state-machines/reproductions.md#inx-29) | implicit |
| [INX-30](implicit-state-machines/infra-nix.md#inx-30) | PipeWire X11 bell module display/source teardown (patched) | Low |  | implicit |
| [INX-31](implicit-state-machines/infra-nix.md#inx-31) | (test harness) Uta MPRIS test waits for bus-name ownership before its caching proxy | Low |  | implicit |
| [INX-32](implicit-state-machines/infra-nix.md#inx-32) | XApp portal backend bound to the graphical session (patched unit) | Low |  | implicit |
| [INX-33](implicit-state-machines/infra-nix.md#inx-33) | XApp Settings theme-change notification coalesced to idle (patched) | Low |  | implicit |
| [INX-34](implicit-state-machines/infra-nix.md#inx-34) | Binary-cache publication pipeline | High | [confirmed](implicit-state-machines/reproductions.md#inx-34) | implicit |
| [INX-35](implicit-state-machines/infra-nix.md#inx-35) | (test harness) Runner cancellation-order fixture | Low |  | implicit |
| [INX-36](implicit-state-machines/infra-nix.md#inx-36) | Self-hosted runner supervisor and per-job admission | High | [confirmed](implicit-state-machines/reproductions.md#inx-36) | implicit |
| [INX-37](implicit-state-machines/infra-nix.md#inx-37) | AI-harness and background-job cgroup placement | Medium |  | implicit |
| [INX-38](implicit-state-machines/infra-nix.md#inx-38) | ananicy-cpp process-event classification (patched) | Medium |  | implicit |
| [INX-39](implicit-state-machines/infra-nix.md#inx-39) | (test harness) Screenshare session counting driver | Low |  | implicit |
| [INX-40](implicit-state-machines/infra-nix.md#inx-40) | (test harness) One-shot screenshot notification action owner | Low |  | implicit |
| [INX-41](implicit-state-machines/infra-nix.md#inx-41) | (test harness) Simulated Wi-Fi access-point bring-up | Low |  | implicit |
| [INX-42](implicit-state-machines/infra-nix.md#inx-42) | (test harness) Tatami screen recording lifecycle with marker-file fault injection | Low |  | implicit |
| [INX-43](implicit-state-machines/infra-nix.md#inx-43) | (test harness) Tuning process-tree stand-ins with nice-value handshake | Low |  | implicit |
| [INX-44](implicit-state-machines/infra-nix.md#inx-44) | (test harness) Yukimi update completion: activation survives its client, switch vs boot | Low |  | implicit |
| [INX-45](implicit-state-machines/infra-nix.md#inx-45) | (test harness) wpa_supplicant dropped-scan reproduction | Low |  | implicit |
| [INX-46](implicit-state-machines/infra-nix.md#inx-46) | (test harness) Yukimi under X11: nested compositor and single-instance lifecycle | Low |  | implicit |

#### [ICI: CI workflows, websites and repository tests](implicit-state-machines/infra-ci-tests.md)

| ID | Machine | Reify | Bug | Follow-up |
| --- | --- | --- | --- | --- |
| [ICI-01](implicit-state-machines/infra-ci-tests.md#ici-01) | Main-branch cache publication run (coalesced queue → checks → binary cache) | Low |  | implicit |
| [ICI-02](implicit-state-machines/infra-ci-tests.md#ici-02) | Nightly input update and main/nightly branch promotion | High |  | implicit |
| [ICI-03](implicit-state-machines/infra-ci-tests.md#ici-03) | Trusted PR admission, recheck and commit-status lifecycle | High | [confirmed](implicit-state-machines/reproductions.md#ici-03) | implicit |
| [ICI-04](implicit-state-machines/infra-ci-tests.md#ici-04) | Manual release: tag → draft prerelease → published | Medium |  | implicit |
| [ICI-05](implicit-state-machines/infra-ci-tests.md#ici-05) | Per-site publish decision keyed on the revision the live site serves | Medium |  | implicit |
| [ICI-06](implicit-state-machines/infra-ci-tests.md#ici-06) | Night/Day theme switch remembered in localStorage | Low | unverified | implicit |
| [ICI-07](implicit-state-machines/infra-ci-tests.md#ici-07) | Plymouth local-console ownership (carried patch) | Medium |  | implicit |
| [ICI-08](implicit-state-machines/infra-ci-tests.md#ici-08) | Plymouth splash keyboard binding across renderer replacement (carried patch) | Medium |  | implicit |
| [ICI-09](implicit-state-machines/infra-ci-tests.md#ici-09) | Download page's nightly build list: edge-cached index with fallback | Low |  | implicit |
| [ICI-10](implicit-state-machines/infra-ci-tests.md#ici-10) | Home-page video asset loading (lazy, single-flight, retried after failure) | Low |  | implicit |
| [ICI-11](implicit-state-machines/infra-ci-tests.md#ici-11) | Video playback state (poster/loading/playing/paused/ended/failed) with seek-drag resume | High | [confirmed](implicit-state-machines/reproductions.md#ici-11) | implicit |
| [ICI-12](implicit-state-machines/infra-ci-tests.md#ici-12) | Player controls auto-hide and touch-tap gating | Low |  | implicit |
| [ICI-13](implicit-state-machines/infra-ci-tests.md#ici-13) | Mute and volume level | Low |  | implicit |
| [ICI-14](implicit-state-machines/infra-ci-tests.md#ici-14) | wasm-bindgen glue: one-shot instance and closure refcounts (generated) | Low |  | implicit |
| [ICI-15](implicit-state-machines/infra-ci-tests.md#ici-15) | (test harness) IBus session-switch driver: Aizome → Xfce → GNOME → Aizome | Low |  | implicit |
| [ICI-16](implicit-state-machines/infra-ci-tests.md#ici-16) | (test harness) gdb-scripted Avahi startup signal window | Low |  | implicit |
| [ICI-17](implicit-state-machines/infra-ci-tests.md#ici-17) | (test harness) Spawn → wait-ready → inject → tear-down daemon drivers | Low |  | implicit |
| [ICI-18](implicit-state-machines/infra-ci-tests.md#ici-18) | (test harness) Chrome IBus probe: CDP request ids and a held-modifier tab cycle | Low |  | implicit |
| [ICI-19](implicit-state-machines/infra-ci-tests.md#ici-19) | (test harness) Plasma Login greeter → session → logout driver (OCR-gated) | Low |  | implicit |
| [ICI-20](implicit-state-machines/infra-ci-tests.md#ici-20) | (test harness) Responsiveness measurement: probes in systemd units, stop-file protocol | Medium | unverified | implicit |
| [ICI-21](implicit-state-machines/infra-ci-tests.md#ici-21) | (test harness) wlr-screencopy / toplevel-export frame lifecycle client | Low |  | implicit |
| [ICI-22](implicit-state-machines/infra-ci-tests.md#ici-22) | (test harness) Session-environment baseline across activation and desktop switches | Low |  | implicit |
| [ICI-23](implicit-state-machines/infra-ci-tests.md#ici-23) | (test harness) FakeHyprland: stateful command and event sockets for Tatami's display recovery | Low |  | implicit |
| [ICI-24](implicit-state-machines/infra-ci-tests.md#ici-24) | (test harness) RecordingR2: conditional-write object store for nightly publication | Low |  | implicit |
| [ICI-25](implicit-state-machines/infra-ci-tests.md#ici-25) | (test harness) Fake system commands steered by marker files in bwrap namespaces | Low |  | implicit |
| [ICI-26](implicit-state-machines/infra-ci-tests.md#ici-26) | (test harness) Isolated PipeWire graph driving the Tatami Sound window, including a service restart | Low |  | implicit |
| [ICI-27](implicit-state-machines/infra-ci-tests.md#ici-27) | (test harness) Yukimi in Tatami: OCR-gated page walk, polkit cancel and rescale | Low |  | implicit |

## Suspected bugs

Auditors recorded 208 concrete failure scenarios while tracing transitions. The 176 on machines rated High were then tested at `8e0acbe5` with reproductions that assert the correct behaviour: 160 confirmed, 9 partly confirmed, 2 not confirmed, and 5 needing hardware, an operating system or a display backend that was not available. [Reproductions](implicit-state-machines/reproductions.md) has each test, its command and output, and an independent review of a sample. The other 32, on Medium and Low machines, are unverified leads.

### Confirmed (160)

- [CPA-16](implicit-state-machines/comp-a.md#cpa-16) Recovery overlay session (open and closed): open the overlay with the pointer on a laptop panel while an external display is connected, then close the lid (or unplug the display the overlay opened on). That output leaves the layout (CPA-15, CPA-05) but `recovery.output` still names it, so the overlay shows only the scrim with no panel. Keys still act unseen, and a click closes it (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#cpa-16))
- [CPA-24](implicit-state-machines/comp-a.md#cpa-24) Process-info worker: request coalescing, CPU baselines and kills: when the first request the drain sees is a Kill, a Sample queued behind it overwrites it (`s => req = s`, `desktop/aizome/crates/aizome-comp-core/src/recovery/procinfo.rs:152`). With the worker busy (the fresh-app 250 ms sleep, slow X11 lookups for a hung X app), pressing Force Quit followed by the open overlay's 1 s sample (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:305`) silently loses the kill: no SIGKILL and no `Killed` report, and for X11 apps the linger step skips satellite (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#cpa-24))
- [CPA-27](implicit-state-machines/comp-a.md#cpa-27) Supervised helper process lifecycle (shell, xwayland-satellite): (1) A helper whose first spawn fails (`desktop/aizome/crates/aizome-comp-core/src/session.rs:457`) keeps `restart_at` at None and is never retried (`desktop/aizome/crates/aizome-comp-core/src/session.rs:483`), so a transient spawn failure at login leaves the session with no shell, or no X11, until Restart Shell (confidence: high). (2) If the new binary given to `restart_shell` fails to exec, the old shell has already been moved to `retiring` (`desktop/aizome/crates/aizome-comp-core/src/session.rs:339`), and the call returns before any handover is set (`desktop/aizome/crates/aizome-comp-core/src/session.rs:346`). The running shell is no longer supervised, and a second `restart_shell` overwrites `retiring` with None, dropping its handle: two shells then run, and the old one is never stopped (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#cpa-27))
- [CPA-32](implicit-state-machines/comp-a.md#cpa-32) Main-loop stall watcher, systemd watchdog and the stall marker: the signal handler allocates (`Backtrace::force_capture`, `desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:169`). If the stall is the main thread slowly allocating (malloc in mmap or brk under memory pressure, holding an arena lock), the handler's own allocation deadlocks it for good, turning a recoverable stall into a permanent one; without systemd's watchdog (`desktop/aizome/crates/aizome-comp-core/src/watchdog.rs:35`) nothing kills it (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#cpa-32))
- [CPB-07](implicit-state-machines/comp-b.md#cpb-07) Idle-inhibitor registry: a client with two inhibitors on one surface (say video playback plus a call in one browser window) destroys one; the surface leaves the set, inhibition ends and the display idles or locks while the other inhibitor is still alive (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#cpb-07))
- [CPB-08](implicit-state-machines/comp-b.md#cpb-08) Pointer constraint activation (lock and confine): (1) a game or VM (or Xwayland's confine-to) confines the pointer and gets `confined`, but the pointer leaves the window freely (confidence: high). (2) In a pointer-locked app driven only by the mouse (an FPS, a VM viewer), `note_activity` never runs, so Aizome's idle timers and ext-idle-notify dim or lock the screen mid-use unless the app also inhibits idle (confidence: medium). ([reproduction](implicit-state-machines/reproductions.md#cpb-08))
- [CPB-18](implicit-state-machines/comp-b.md#cpb-18) Popup tracking and popup-layer stacking (xdg and input-method popups): input-method popup surfaces (zwp_input_popup_surface_v2, as fcitx5 uses for its candidate list) are tracked but never pushed to `popup_order`, so `placed_popups` skips them: they are never drawn (`render.rs:155`) or hit-tested (`popups.rs:74-80`) (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#cpb-18))
- [CPB-20](implicit-state-machines/comp-b.md#cpb-20) Foreign-toplevel handle synchronisation (wlr management and ext list): `Snapshot.parent` is compared in `refresh` (`foreign_toplevel.rs:173`) but no `parent` event is ever sent after the announce (`foreign_toplevel.rs:179-200`), so a dialog whose parent is set or changed after mapping is never re-parented in taskbars (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#cpb-20))
- [CPB-21](implicit-state-machines/comp-b.md#cpb-21) Gamma-control ownership per output: client A's `SetGamma` fails (for example while the VT is switched away), so A's control is failed and released; client B (or A again) takes the output; A then destroys its failed control, `release` clears B's hold and Night Light overwrites B's ramps (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#cpb-21))
- [CPB-25](implicit-state-machines/comp-b.md#cpb-25) wlr-screencopy frame lifecycle and stream pool: (1) wf-recorder records a whole output and the user changes only its scale (same mode); the region is unchanged, so the old stream is reused with its old tracker scale and surfaces are drawn at the wrong size; the ext path resets on a scale change (`image_copy_capture.rs:515-521`), this one doesn't (confidence: medium). (2) A client that destroys its manager right after requesting a frame leaves one stream, with a full-output offscreen buffer, per manager for as long as the output exists (confidence: low). ([reproduction](implicit-state-machines/reproductions.md#cpb-25))
- [WM-04](implicit-state-machines/wm-comp-tests.md#wm-04) Interactive resize and its edge anchor: resize a window from its left (or top) edge, then drag it elsewhere by its tab; the next commit whose size differs from the model's rect (an unfill or display relayout the client clamps to its minimum size, a client resizing itself) runs `r.x = a.start.right() - size.0` (`desktop/aizome/crates/aizome-comp-logic/src/actions.rs:1041-1043`) and the window jumps back to where the old resize's right edge was, possibly on another display. (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#wm-04))
- [WM-10](implicit-state-machines/wm-comp-tests.md#wm-10) Sticky keys (latched modifiers): with sticky keys on, tap ⌃ and ❖ (both latch), then press R: the synthetic ⌃ and ❖ presses go out with R (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:681-688`), the core takes ⌃❖R and reloads at the end of that loop iteration while R is still down (`desktop/aizome/crates/aizome-comp-core/src/input.rs:518`); the reload drops `applied` (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:97`; unlike Reset Input it releases no keys), so R's release reaches the new logic as a plain release (`desktop/aizome/crates/aizome-comp-logic/src/input.rs:698`) and ⌃ and ❖ stay pressed in the seat: the next letter typed arrives as a ⌃❖ chord (Q would lock the session). Turning sticky keys off while a latched chord is held does the same. (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#wm-10))
- [WM-12](implicit-state-machines/wm-comp-tests.md#wm-12) Area picker (screenshots and IPC `pick-area`): while the share picker (or the shell's recording "pick an area") waits, a logic reload or Ctrl+Alt+Del → Reset Input runs `quiesce` → `close_all` (`desktop/aizome/crates/aizome-comp-logic/src/hooks.rs:92`, `desktop/aizome/crates/aizome-comp-logic/src/overlays/mod.rs:81`), which drops the pick without `AreaPicked{area: None}`; `aizome-share-picker` loops on events forever and the shell's recording stays in its picking phase. (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#wm-12))
- [WM-15](implicit-state-machines/wm-comp-tests.md#wm-15) Desks overview: unplug (or lid-close) the display showing the overview: `items` never matches the gone output (`desktop/aizome/crates/aizome-comp-logic/src/overlays/overview.rs:262-265`), so nothing is drawn, yet `grabs_keyboard` stays true, every key is swallowed (`:119-121`, `:166`) and every press is taken (`:223`) — the desktop looks normal but ignores input until a blind Esc. (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#wm-15))
- [SHA-02](implicit-state-machines/shell-a.md#sha-02) Shell-replacement carry handoff (state file, final word, retiring):  - The scenario: another notification daemon (dunst or mako started by hand, or any client asking for `org.freedesktop.Notifications` with replace) takes the name. That fires `NameLost` (`notifications/server.rs:251`–`notifications/server.rs:253`). - The shell then writes a final state, sets `retiring = true` for good and keeps running. Nobody stops it, and `NameAcquired` never clears the flag. - From then on it never saves its notifications, folds or expansions again, so a later crash or reload restores stale state. It never asks for the name again. - Confidence: medium. ([reproduction](implicit-state-machines/reproductions.md#sha-02))
- [SHA-03](implicit-state-machines/shell-a.md#sha-03) Deskbar auto-hide visibility:  - The scenario: the compositor closes an auto-hide Deskbar while the pointer is over it, because its output went or the main display changed. - `close_requested` leaves `inside=true`, and `sync_deskbar` reopens the Deskbar through `(true, None)` with `reopened=false`, so no hide is armed. - `AutoHide` then rejects every timer, and the Deskbar stays revealed until the pointer enters and leaves the new surface. - Confidence: low; it depends on whether the kit delivers a leave event first. ([reproduction](implicit-state-machines/reproductions.md#sha-03))
- [SHA-12](implicit-state-machines/shell-a.md#sha-12) Palette open/close and its drawn-ahead spare window:  - The scenario: the palette is open when its output is unplugged. - The compositor closes the layer and `close_requested` forgets only the surface, so `self.palette` stays `Some(dead window)`. - Typing in the Toolchest then calls `toolchest_typed` → `open_palette`, which re-ranks into the dead window. Nothing shows, and the handover timer is not armed. An `aizomectl` palette open with a query is ignored the same way. - It recovers only after a toggle closes the ghost. - Confidence: medium. ([reproduction](implicit-state-machines/reproductions.md#sha-12))
- [SHA-13](implicit-state-machines/shell-a.md#sha-13) Palette file search, index lifetime and per-opening extras:  - The scenario: type a query, close the palette with Esc before its search returns, then reopen and type again. The new search also gets `seq=1`. - The old `Files(1, …)` is accepted for the new opening, with `files_for` set to the new query. - If it arrives after the new results, the old query's files stay shown until the next keystroke. - Confidence: low; the old search must outlast closing, reopening and an index load. ([reproduction](implicit-state-machines/reproductions.md#sha-13))
- [SHA-25](implicit-state-machines/shell-a.md#sha-25) Torn-off palettes (persisted list, shown surfaces, strip drag):  - Two scenarios. - (1) Re-tearing a menu that already has a palette goes through `tear_off`. Its `retain` shifts later palettes down while `tear_shown` and `Surface::TearOff(i)` keep the old indices. - `sync_tearoffs` then only resizes the existing windows. Palettes show each other's menus at each other's positions, and drags move the wrong entry. - With a single palette, the re-torn one stays where it was: the new x and y are saved, but no margins are set. - Confidence: high. - (2) If a palette's output is unplugged, the compositor closes the layer and `close_requested` forgets only the surface. `tear_shown[i]` keeps the dead window. - A pinned or Toolchest palette, which is always visible, therefore never reopens: not on the primary output, and not when the monitor returns, until the user closes it or the shell restarts. - Confidence: medium. ([reproduction](implicit-state-machines/reproductions.md#sha-25))
- [SHB-03](implicit-state-machines/shell-b.md#shb-03) Polkit authentication dialog: when the helper can't be reached (`polkit-agent-helper` socket missing, spawn error), `Session::start` emits Error then Failed at once (`desktop/aizome/crates/aizome-sys/src/polkit.rs:247`-`252`); the shell's Failed handler calls `authenticate` again (`auth.rs:294`-`295`), which fails the same way, so the shell spins through connect attempts and warnings on its event loop until the user cancels (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#shb-03)) Fixed in [#298](https://github.com/bitemyapp/sekka/pull/298).
- [SHB-06](implicit-state-machines/shell-b.md#shb-06) Session confirmation countdown (Restart, Shut Down, Log Out, power key): the dialog is a layer on the focused output (`confirm.rs:118`-`122`); if that output goes (dock unplugged, monitor off) during the minute, `close_requested` removes only the `surfaces` entry (`shell.rs:1712`), `confirm` stays `Some`, ticks continue and the machine shuts down/restarts/logs out with no dialog to cancel; asking again only resets `left` (`confirm.rs:110`-`115`) and never reopens a window (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#shb-06))
- [SHB-09](implicit-state-machines/shell-b.md#shb-09) Compositor event follower (connect, snapshot, subscribe, reconnect): events between the snapshot calls and `Subscribe` are lost: the compositor's `Subscribe` only sets `subscribed` with no replay (`desktop/aizome/crates/aizome-comp-core/src/ipc.rs:207`-`211`), so a window mapped in that gap (autostart apps at login, or while the shell reconnects after a compositor restart) is missing from the Deskbar and menus until the next reconnect (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#shb-09))
- [SHB-12](implicit-state-machines/shell-b.md#shb-12) Lock screen lifecycle (partially explicit): whatever PAM says is logged and replaced by "That password didn't work" (`lock.rs:427`-`428`), so an expired password (`pam_acct_mgmt` → new token required) or a faillock lockout reports the correct password as wrong on every try and the user can't learn why (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#shb-12)) Fixed in [#298](https://github.com/bitemyapp/sekka/pull/298).
- [SHB-14](implicit-state-machines/shell-b.md#shb-14) Critical notification cards surface: when the cards' output goes, the compositor closes the layer and `close_requested` drops only the `surfaces` entry (`shell.rs:1712`); `cards` keeps the dead key, so `sync_cards` keeps resizing it (`cards.rs:135`) and never reopens, and critical cards stay invisible until every one is read elsewhere (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#shb-14))
- [SHB-15](implicit-state-machines/shell-b.md#shb-15) Bar notification line (banner): a replacement of the notification on the line (same id through `replaces_id`) arms a second `BannerDone(id)` while the first is still pending; the first ends the line at the original deadline (`glue.rs:198`-`201`) and, for a transient one, closes the just-updated notification. A progress notification updated every 2 s gets its line cut short and flickering (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#shb-15))
- [SHB-20](implicit-state-machines/shell-b.md#shb-20) Screen recording session (partially explicit): `adopt_recording` never arms the indicator tick (only `Started` does, `recording_glue.rs:173`), so after a shell restart mid-recording the systray clock only advances when something else repaints the bar, about once a minute (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#shb-20))
- [SVC-05](implicit-state-machines/kit-services.md#svc-05) Greeter screen stage (people, password, typed name): with `user_list = false` (or nobody listed) a mistyped user name can't be corrected. PAM's auth error begins the same name again every time (`desktop/aizome/crates/aizome-greeter/src/login.rs:182-183`). If `CreateSession` itself fails, the step becomes `Failed` and Return does nothing (`app.rs:259` → `begin` sees `user() == None`). Only restarting the greeter helps. (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#svc-05)) Fixed in [#298](https://github.com/bitemyapp/sekka/pull/298).
- [SVC-23](implicit-state-machines/kit-services.md#svc-23) Playback session lifecycle: changing speed on non-seekable media with audio drops the tempo filter (`player.rs:777-779`), but the seek that would rebase the ring is skipped (`player.rs:1123-1125`). The ring's segment keeps the old speed, so the clock runs at the old rate while the audio plays at the new tempo: the video drifts and the position is wrong. (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#svc-23))
- [SVC-29](implicit-state-machines/kit-services.md#svc-29) Video suspension out of sight, and catching up: a seek while the window is hidden stalls playback. Utsushi reports xdg suspension through `set_visible(false)` (`desktop/aizome/apps/utsushi/src/app.rs:1149`), and MPRIS seeks arrive at `app.rs:649`. `seek_to` stops the audio and waits for the first frame of the flushed generation (`player.rs:1146`, `player.rs:1344`). The video thread is still suspended locally, so `check_sight` returns before presenting (`video.rs:778-782`), and the audio stays silent until the window is shown again. Speed and track changes seek too, so they stall the same way. (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#svc-29))
- [AZN-01](implicit-state-machines/aizome-small-nix.md#azn-01) Keep Awake requested/protected status (setting file against logind's locks): two concurrent `aizome-awake toggle` runs (the case named by the lock's doc comment at `main.rs:85`) both evaluate `!requested()` = true before either takes the lock, so both turn Keep Awake on and the second toggle is lost (confidence: medium; the menu bar sends `on`/`off`, so only CLI and script callers of `toggle` are affected). ([reproduction](implicit-state-machines/reproductions.md#azn-01))
- [AZN-04](implicit-state-machines/aizome-small-nix.md#azn-04) Agent registration with NetworkManager (retry, re-register on restart): suppose another agent with the same identifier is still registered for the user for longer than `REGISTER_RETRY` (the case the comment at `desktop/aizome/crates/aizome-network-agent/src/lib.rs:389`–`:391` anticipates: "another still is, until its connection closes"). The first refusal sets `asked` with `done` false; the second identical refusal sets `done = asked = true`. The loop then blocks on `owners.recv()` and never registers once the other agent disconnects. NetworkManager is left with no agent and credential prompts never appear until NetworkManager restarts (confidence: medium). `tests/credentials.rs:350`–`:356` covers only "timed out, then refused". ([reproduction](implicit-state-machines/reproductions.md#azn-04))
- [AZN-13](implicit-state-machines/aizome-small-nix.md#azn-13) Time-zone ownership across activations (saved zone file and `/etc/localtime` link): on a fresh system installed without Genkan (plain `nixos-install` with `programs.aizome.enable` and `time.timeZone = "Europe/Paris"`), there is no link and no saved zone. The declared zone is therefore discarded and the machine runs in UTC until someone sets the zone in Settings. Genkan works around this by seeding the link (`installer/genkan/rust/src/install.rs:699`) (confidence: medium). ([reproduction](implicit-state-machines/reproductions.md#azn-13))
- [SYA-05](implicit-state-machines/sys-a.md#sya-05) Optimistic audio levels awaiting server confirmation: Settings changes the volume of a stream the server refuses to change (for example a sink input with `volume_writable` false): `set_volume_in` has already stored the new level and marked the model dirty, the callback reports `Err`, no event follows, so the snapshot and slider show the unapplied level until another event for that stream (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#sya-05))
- [SYA-06](implicit-state-machines/sys-a.md#sya-06) Bluetooth pairing-agent registration worker: bluetoothd restarts while Settei's Bluetooth pane is shown; Settei keeps `agent = true` (`desktop/aizome/apps/settei/src/panes/bluetooth.rs:453`, reset only on a failed registration at `:621` or on hide at `:490`) and the worker keeps its `Registration`, so BlueZ has no agent and pairing a device that needs a passkey or confirmation fails with no prompt until the pane is hidden and shown again (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#sya-06))
- [SYA-13](implicit-state-machines/sys-a.md#sya-13) Destructive-operation confirmation token: a UI that holds a `Confirm<NewPartitionTable>` made for USB stick A (32 GB, GPT) while A is swapped for an identical-size partitioned stick B that comes up as `sdb` again: path, device, size and empty id fields all match, so B is wiped (confidence: low; no app makes these tokens yet) ([reproduction](implicit-state-machines/reproductions.md#sya-13)) Fixed in [#290](https://github.com/bitemyapp/sekka/pull/290).
- [SYB-01](implicit-state-machines/sys-b.md#syb-01) Airplane mode across rfkill, NetworkManager, BlueZ and a marker file: Airplane mode turns on through the fallback (the marker lists hci0). The person then turns Wi-Fi back on with the Wi-Fi switch: `airplane_mode` reads false, but the marker stays. When they turn airplane mode on again, `bluetooth_on` returns `[]` because hci0 is still off, and `std::fs::write` (`desktop/aizome/crates/aizome-sys/src/network/mod.rs:116`) replaces the list with "". Turning airplane mode off then never powers Bluetooth back on. Separately, the stale marker makes a later plain Wi-Fi-off read as airplane on (`mod.rs:578`) (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#syb-01))
- [SYB-02](implicit-state-machines/sys-b.md#syb-02) Wi-Fi join / activation transaction with in-memory edit and rollback: Joining a saved network with a new password that takes longer than 45 s to come up: `wait_active` returns Timeout and `rejoin_with` reloads the old profile from disk (`mod.rs:717-724`). NM keeps activating with the in-memory settings it already applied and may connect. The UI reported failure and the working password was never saved, so the next reconnect fails (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#syb-02))
- [SYB-09](implicit-state-machines/sys-b.md#syb-09) polkit authentication session and helper conversation: Retry loop without backoff. When the helper can't be reached (connect error, `polkit.rs:245-252`) or exits without printing SUCCESS/FAILURE (EOF → Failed, `polkit.rs:324`), for example a broken setuid wrapper, a socket unit refusing, or PAM refusing before any prompt, Failed is emitted at once. The shell's Failed handler immediately calls `authenticate` again (`desktop/aizome/crates/aizome-shell/src/auth.rs:289-296`), which fails again, so the dialog spins and spawns helpers or logs warnings as fast as the UI loop runs (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#syb-09)) Fixed in [#298](https://github.com/bitemyapp/sekka/pull/298).
- [SYB-12](implicit-state-machines/sys-b.md#syb-12) logind sleep-delay inhibitor and lock-before-sleep handshake: The lock isn't confirmed before logind's delay limit expires, so the machine sleeps with the shell's `sleep_pending` still set. On resume the delay is retaken (`logind.rs:276-278`, `shell.rs:2063`). Then `lock_confirmed` (`desktop/aizome/crates/aizome-shell/src/lock.rs:365-367`) calls `sleep_ready()` and drops the fresh inhibitor. Nothing retakes it until the next resume, so the next suspend isn't delayed and can sleep before the screen locks (confidence: medium-low) ([reproduction](implicit-state-machines/reproductions.md#syb-12)) Fixed in [#298](https://github.com/bitemyapp/sekka/pull/298).
- [SYC-01](implicit-state-machines/sys-c-config.md#syc-01) Keyboard settings: inherited session defaults vs saved choices: the user never opens keyboard settings but once collapses the Deskbar, and the shell saves (`desktop/aizome/crates/aizome-shell/src/shell.rs:1012`). The file now holds `layouts = ["us"]`. The admin later sets `services.xserver.xkb.layout = "de"`, which reaches `XKB_DEFAULT_LAYOUT` through `desktop/aizome/nix/module.nix:126`, but the session stays on US (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#syc-01))
- [SYC-02](implicit-state-machines/sys-c-config.md#syc-02) One-shot migration of the legacy `[keys] terminal` key: the first session after the upgrade fails in `with_index` (`desktop/aizome/crates/aizome-comp-core/src/terminal.rs:76-81`). During that session the shell saves any section, which drops `terminal`. The next session retries with `None`, so ❖Return starts Hotaru instead of the user's previous terminal (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#syc-02))
- [SYC-03](implicit-state-machines/sys-c-config.md#syc-03) Settings file lifecycle: load, fallback to defaults, atomic save, cross-process writers: a hand edit with one typo makes the watcher fire, and the shell reloads defaults (`desktop/aizome/crates/aizome-shell/src/shell.rs:1273`). The next Deskbar or menu bar change saves those defaults over every other setting (`desktop/aizome/crates/aizome-shell/src/shell.rs:1012`); the location update path does the same (`desktop/aizome/crates/aizome-shell/src/location.rs:85-87`) (confidence: medium). Separately, a shell save racing a Settei save on the shared temp file can publish truncated TOML (confidence: low). ([reproduction](implicit-state-machines/reproductions.md#syc-03))
- [SYC-07](implicit-state-machines/sys-c-config.md#syc-07) Text field caret, selection, IME preedit, blink and Caps Lock state: with an input method composing (fcitx5 or ibus active), the characters being composed in the lock-screen password field (`desktop/aizome/crates/aizome-shell/src/lock.rs:239`) are drawn in clear text beside the dots until they are committed; the same holds for `desktop/aizome/crates/aizome-shell/src/auth.rs:131` and `desktop/aizome/crates/aizome-greeter/src/view.rs:245` (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#syc-07)) Fixed in [#298](https://github.com/bitemyapp/sekka/pull/298).
- [KIT-04](implicit-state-machines/kit-core.md#kit-04) Menu publication to the shell (dedup and debounce): the shell's open menu shows Save enabled; the app disables it (republished 40 ms later); a click in the shell sends `Activate{id}`, `find` returns the now-disabled Save and `perform` queues its message anyway (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#kit-04))
- [KIT-07](implicit-state-machines/kit-core.md#kit-07) File panel lifecycle and browsing state: a program closes a window with `Cx::close_window` while its open or save panel is up: the `PanelState` stays in `Core.panels` for good and `done` never fires, so a program waiting for `Saved(None)` never leaves its saving state (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#kit-07))
- [KIT-13](implicit-state-machines/kit-core.md#kit-13) Pointer hover, press and capture: Hako calls `Cx::start_drag` while the button that began the drag is down (`desktop/aizome/apps/hako/src/ui/actions.rs:220`); the drop sends no release to the window, so `active` keeps naming the drag source: moves after re-entry go to it, and the next click's PointerUp reaches it instead of the clicked control, losing that click (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#kit-13))
- [KIT-17](implicit-state-machines/kit-core.md#kit-17) Tooltip delay, show and dismissal: hover a button and click it within 650 ms: its tooltip still appears after the click, though a click dismisses tooltips (`tests/tooltip_refresh.rs:137-142` covers only a shown one) (confidence: medium); every pointer leave with a tooltip pending leaks one `Core.timers` entry (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#kit-17))
- [KIT-29](implicit-state-machines/kit-core.md#kit-29) Node arena: liveness, layout cache and per-layout outboxes: in a relayout that compacts, a widget mounted during layout (an editor, scroll view or list inside a virtual list's row, after earlier rows died) records `invalidate_rect` or `scrolled` under its old index: its damage or pixel move lands on another node, and `take_local_damage` indexes `self.nodes[*i]` past the end once the old index is at least the live count (`tree.rs:686`), which panics (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#kit-29))
- [UIW-01](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-01) Press-then-release click pairing in buttons, switches, checkboxes, radios and segmented controls: press on inert background (nothing handles it, `active` stays `None`), move onto a button and let go: the engine bubbles the `PointerUp` to the button, which emits `on_press` though it was never pressed; the same for switches, checkboxes, radios and segments (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#uiw-01))
- [UIW-08](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-08) Menu interaction: hover, open submenu, pointer aim, type-to-filter, strip tear-off: (1) a torn-off palette (`menu_view`, rendered live by the shell) whose menu shrinks while a row past the new end is hovered panics on the next pointer move, leave or arrow key: `set_hover` calls `row_rect(old)` (`menus.rs:460` → `self.rows[i]` `menus.rs:423`), and Enter/→ index `self.rows[i]` (`menus.rs:493`, `:930`); (2) after ← closes a submenu (`MenuBack` closes only the child window), the parent's `open_sub` still names it, so hovering that row again doesn't reopen it (`menus.rs:470`) and rightward moves still count as aiming (`menus.rs:842`) (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#uiw-08))
- [UIW-09](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-09) In-window menu bar's open title: clicking the open title to close its menu reopens it: the engine closes a window's menus on any press before routing the press (`engine.rs:2107-2122`), which deletes `BarState`, so the strip sees `open == None` and takes the open arm (`menus.rs:1172`) instead of the close arm (`menus.rs:1167`) (confidence: medium — needs the compositor to deliver the press to the parent while the popup grabs, as wlroots does) ([reproduction](implicit-state-machines/reproductions.md#uiw-09))
- [UIW-11](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-11) Scroll-bar visibility and thumb drag: scroll for more than ~20 ms (any touchpad gesture, several wheel notches): only the first `scroll_by` arms the tick, which fires ~920 ms later while `shown_until`, pushed on by later events, is still ahead; nothing is invalidated and nothing re-arms, so the bars stay drawn until something else repaints them — in every `scroll`, `list`, `table` and `tree` (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#uiw-11))
- [UIW-15](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-15) Selection cursor and anchor in lists, tables and outlines: (1) outline: click a row deep in an open branch (cursor 25), close its parent by the triangle (rows shrink, cursor untouched), press Return, ← or → → `self.rows[c]` out of bounds panics (`table.rs:1130`, `:1135`, `:1150`) (confidence: high); (2) table: in the portal's file chooser, click file A, sort by a header (the chooser remaps its selection by path, `desktop/aizome/crates/aizome-portal/src/chooser/model.rs:231-254`), press Return → `on_activate` gets the old cursor index (`table.rs:660-662`), so another file is chosen and returned to the requesting app; ↓ likewise moves from the old row (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#uiw-15))
- [UIW-18](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-18) Editor viewport, position-bar linger and caret blink on one tick: (1) focus the editor while an earlier tick is pending (scroll it, click into it 0.4–0.9 s later; or Tab away and back within half a second): the focus tick request is dropped (`engine.rs:1723`), the pending tick fires before `next_blink`, isn't due (`text_editor.rs:1340`), so nothing re-arms and the caret stops blinking until the next focus change; (2) an unfocused editor scrolled for more than ~20 ms keeps its position bar drawn, as in UIW-11 (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#uiw-18))
- [UIW-27](implicit-state-machines/sukumo-ui-kit-widgets.md#uiw-27) Session lock: a lock screen that unlocks and at once locks again while the old lock's `finished` is in flight: the stale `SessionLockFinished` turns the new `Asked` into `Unlocked` and closes the new lock surfaces (`app.rs:1612-1614`); the backend then creates the new lock, the compositor locks, and the `SessionLocked` is ignored as not `Asked` (`app.rs:1606`) — the session is locked with no lock surface and the program believes it unlocked (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#uiw-27)) Fixed in [#298](https://github.com/bitemyapp/sekka/pull/298).
- [SKB-11](implicit-state-machines/sukumo-backend.md#skb-11) Window configure, size and geometry lifecycle: after the header's maximize button is clicked, the configure arrives; a pointer move over the header before the main thread presents changes the hover (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/decor.rs:837`), so `refresh_decor` commits the maximized geometry and frame around the old-size content buffer: one frame with an undrawn band (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#skb-11))
- [SKR-03](implicit-state-machines/sukumo-render.md#skr-03) Retained paints and per-host frame scheduling (dirty / moving / last snapshot): `frame` clips damage to "where the host may draw" by target, not by host (`tree.rs:598` filters `damage` on `*t == target`), so every rect earlier hosts of the same target pushed is intersected with this host's clip too; with disjoint clips those rects become empty and are dropped (`tree.rs:603`) while their host's `last` was already advanced (`tree.rs:600`). Two layer-backed views animating at once in one window surface (two spinners): the host the stable `HashMap` order visits first never gets its animation redrawn while the other animates (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#skr-03))
- [SKR-04](implicit-state-machines/sukumo-render.md#skr-04) Layer contents, canvas generations and their image-cache keys: when a canvas is replaced (resize, `tree.rs:258-265`) or swapped for `None`/`Image` (`tree.rs:250`, `:254`), the old `Canvas` is dropped without `images::forget(&[old.key])`; the render thread's cache keeps `Base::Shared(old px)` until LRU eviction against the 256 MiB budget (`raster/images.rs:105`, `:229-238`). Live-resizing a window with a layer-backed view makes a new canvas per size step, so the render thread retains up to the whole budget in dead canvas pixels (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#skr-04))
- [STA-10](implicit-state-machines/settei-a.md#sta-10) Keyboard setup: catching the key beside the space bar: while the setup sheet waits, open Settei at another pane (the palette, `aizome-settei --pane …`): `hidden` stops the catch, the sheet stays up over the new pane without "Waiting for the key…", and pressing the key beside the space bar does nothing; only Not Now or Same as Other Keyboards get out (confidence: medium). ([reproduction](implicit-state-machines/reproductions.md#sta-10))
- [STA-12](implicit-state-machines/settei-a.md#sta-12) Pane sheets against the window's single sheet slot (keyboard's three, network, printers, users): with Add a Layout's picker open, plug in a keyboard and accept the shell's setup prompt (`aizome-settei --pane keyboard.setup --keyboard …`): `SetupOpen` starts a compositor key catch but `sheet()` still shows the picker, so the next key typed into the layout search is swallowed and taken as the setup key; after the picker closes, "Add a Command…" shows the leftover setup sheet instead of the command sheet (confidence: medium). ([reproduction](implicit-state-machines/reproductions.md#sta-12))
- [STA-15](implicit-state-machines/settei-a.md#sta-15) Connection details page: navigation, profile reads and the edit buffer: change DNS (or metering) on a connection A that has a static address, go Back and open connection B before A's change returns (polkit asking, say): A's `Changed` re-reads A and `loaded` swaps the edit buffer to A's address, router and DNS; B's page then shows A's values in its fields, and Apply on B's IPv4 writes A's static address to B (confidence: medium). ([reproduction](implicit-state-machines/reproductions.md#sta-15))
- [STA-19](implicit-state-machines/settei-a.md#sta-19) Add-a-printer sheet: discovery, driver lookup, adding: choose a plugged-in printer that needs a driver, then another before CUPS's driver list comes back: the first device's `Drivers` reply fills `drivers` for the second (`printers.rs:507-513` doesn't check `chosen`), `ready` turns true, and Add installs the second printer with the first one's PPD (`DriverChoice::Model`, `:530-535`) (confidence: medium). ([reproduction](implicit-state-machines/reproductions.md#sta-19))
- [STA-22](implicit-state-machines/settei-a.md#sta-22) New account: its password applied once AccountsService lists it: add a user with a password and switch panes or quit at once: `Created(Ok)` arrives when CreateUser answers, before the snapshot lists the account, `system_changed` finds nothing, later wakes go to the other pane, and the new account never gets its password (confidence: low). ([reproduction](implicit-state-machines/reproductions.md#sta-22))
- [STB-09](implicit-state-machines/settei-b.md#stb-09) Bluetooth pane activity: pairing agent, discovery and discoverability while shown: quitting Settei while Bluetooth shows never calls `hidden` (STB-01), so the adapter stays discoverable after Settei is gone until BlueZ's `DiscoverableTimeout` expires (never, where it is 0), and the next run, finding it already discoverable, doesn't take it over (`bluetooth.rs:308`) (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#stb-09))
- [STB-17](implicit-state-machines/settei-b.md#stb-17) settings.toml write pipeline: ordered writer, pending count, reload debounce: while a slider is dragged (writes pending throughout), the shell writes `settings.toml` (e.g. the Deskbar's edge dragged to a new width). `reread` drops the watcher's notice (`shared.rs:74`), the next `change` uses the cached settings (`shared.rs:95`), and the shell's change is overwritten (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#stb-17))
- [HAKO-05](implicit-state-machines/hako.md#hako-05) Transfer placement with conflict prompt and replace-aside protocol: Put Back (and Undo of Move to Trash, `undo.rs:167`) runs `restore` with `trash: None`, so when an item of that name is now at the original location and the person picks Replace (the sheet says "Replacing moves the one there to the Trash", `sheets.rs:322-326`), the newer item is permanently deleted by `remove_quietly` (`transfer.rs:469`) (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#hako-05)) Fixed in [#290](https://github.com/bitemyapp/sekka/pull/290).
- [HAKO-06](implicit-state-machines/hako.md#hako-06) Copy-to-partial commit and cross-filesystem move (copy, fsync, delete arrived sources): in a cross-filesystem move, when a nested folder's `sync_dir` fails (`transfer.rs:758-760`, e.g. EIO on a flaky USB stick) the folder is reported as "couldn't be written safely", but the files under it were already pushed to `arrived.leaves`; if the top-level partial and its parent still sync, `place` deletes those source files (`transfer.rs:543-551`), emptying a source folder the report says was kept (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#hako-06)) Fixed in [#290](https://github.com/bitemyapp/sekka/pull/290).
- [HAKO-08](implicit-state-machines/hako.md#hako-08) Trash can item protocol (trashinfo claim, rename, directorysizes, forget): Put Back of a folder onto an existing folder with Merge, skipping a conflicting child: `merge` returns Ok, `place_top` reports Done, and `restore` calls `trash.forget(entry)` (`transfer.rs:164-166`) though the skipped child is still in `files/<name>/`; its info is gone, so it lists as an orphan of unknown origin that Put Back refuses ("where it came from isn't known", `transfer.rs:138`) (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#hako-08)) Fixed in [#290](https://github.com/bitemyapp/sekka/pull/290).
- [HAKO-09](implicit-state-machines/hako.md#hako-09) Undo/redo history with in-flight inversion jobs: an undo that fails or is stopped (rename back onto a name taken since, or Stop at a move's conflict prompt) yields an empty inverse that `undone` discards (`undo.rs:108`), so the step is gone from both stacks and the next ⌘Z silently undoes the step before it (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#hako-09)) Fixed in [#290](https://github.com/bitemyapp/sekka/pull/290).
- [HAKO-11](implicit-state-machines/hako.md#hako-11) inotify folder watcher and its watch-descriptor map: a shown folder that's deleted and recreated under the same path (`rm -rf dist && mkdir dist`, a build tool or checkout) stays in the map under its dead wd; the parent's reload keeps the column (same path), `update_watch` passes the same dirs, `set` skips it (`watch.rs:70`), and changes inside the new folder never show until the person navigates away and back (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#hako-11))
- [HAKO-16](implicit-state-machines/hako.md#hako-16) Volume mount, unlock and two-phase eject tasks: ejecting a volume a window is showing sends that window home with `Browser::go` directly (`actions.rs:511`), skipping `start_loads`, `update_watch` and the title; its new Home column stays `Load::Wanted` (blank, no message) because nothing else starts a `Wanted` load (`reload_dir` only refreshes Ready/Failed, `app.rs:645`) until the person navigates (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#hako-16))
- [HAKO-17](implicit-state-machines/hako.md#hako-17) Thumbnail request pipeline (queue, asked set, running count): opening a folder of many pictures and leaving before they're all made prunes the rest from `thumb_wanted` (`actions.rs:364`) but leaves them in `thumb_asked`; coming back, `want_thumbs` skips them (`app.rs:677`), so they show generic icons for the rest of the session (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#hako-17))
- [HAKO-19](implicit-state-machines/hako.md#hako-19) Column listing load, reload debounce and refresh reconciliation: a change heard while its column is `Loading` is dropped: `reload_dir` skips `Loading` columns (`app.rs:645`) and nothing re-wants them when the stale read lands, so in a big or slow folder a file created after the read passed its slot stays missing until the next change (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#hako-19))
- [HAKO-20](implicit-state-machines/hako.md#hako-20) Rename session (inline field, new-folder auto-rename): `rename_when_loaded` is cleared only when the focused column is Ready with exactly that folder selected; if New Folder runs while its column is mid-load (the read predates the mkdir, so `pending` finds nothing), it stays armed, and much later clicking that folder (whose next column's `Loaded` calls `maybe_rename_new`) unexpectedly opens the rename field (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#hako-20))
- [HAKO-25](implicit-state-machines/hako.md#hako-25) Drop-target spot registry: in List or Icons view, after opening a subfolder (columns `[Home, work]`, focus 1) or switching from Columns, spot 0 keeps the parent listing's full-size rect and rows; dragging an item in the current view resolves through stale spot 0 first, so a drop moves the files into whichever parent folder was at that row (or into the parent folder itself) with no highlight shown; in search, drops off a folder row fall through to stale column spots the same way (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#hako-25))
- [HAKO-28](implicit-state-machines/hako.md#hako-28) Job lifecycle with blocking questions: if a job's window closes before it asks, the question opens over the operations window or another window but `job.window` stays the dead key (`app.rs:774-786`); `reparent_questions` matches only `j.window == gone` (`actions.rs:702`), so closing that host window destroys the sheet without re-asking, and the job hangs "Waiting for an answer" until stopped from Operations (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#hako-28))
- [HOT-07](implicit-state-machines/hotaru-app.md#hot-07) Automatic-link activation pipeline (match, hover, resolve, open): ❖-click a relative path (resolved by a spawned helper, up to 250 ms) and move the pointer one cell or out of the pane before the helper answers: `terminal_view.rs:441-455`/`:484` emits `MatchLink`, `app.rs:1929`/`:1945` cancels `resolver_request`, `ResolvedLink` finds nothing and the link silently never opens (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#hot-07))
- [HOT-12](implicit-state-machines/hotaru-app.md#hot-12) Settings persistence, reload and deferred window close: close the window (or end its last tab) while a save is in flight, then press ⇧❖, before it lands: the token bump (`app.rs:1370-1371`) makes `ConfigSaved` stale (`:1727`), `ConfigLoaded` never checks `closing`, so the window stays open with `closing` latched and a later unrelated settings change closes the window and Settings unexpectedly (`:1751-1754`); `open_after_save` latches the same way and opens hotaru.toml later (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#hot-12))
- [HOT-14](implicit-state-machines/hotaru-app.md#hot-14) Appearance ownership between the desktop watcher and Hotaru's theme: with Hotaru set to Night and the desktop on Day, switch the desktop to Night (no visible change in Hotaru, so no `Msg::Theme`), then choose Follow Desktop: `apply_appearance` uses the stale Day `desktop_appearance` and Hotaru shows Day while the desktop is Night, until the desktop changes again; the Appearance page previews use the same stale base (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#hot-14))
- [HOT-17](implicit-state-machines/hotaru-app.md#hot-17) Single-instance claim and launch handoff: if the first instance takes over 2 s from `claim` to `serve` (cold start), a second `aizome-hotaru` times out on the ack (`instance.rs:115`, `:119`) and opens its own window, while the first later reads the buffered request and opens a tab too: one launch, two terminals (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#hot-17))
- [HOT-21](implicit-state-machines/hotaru-app.md#hot-21) PTY worker lifecycle (spawn, run, EOF, hang-up, exit report, stop): in bash, `sleep 600 &` then `exit`: the background job still holds the PTY slave, so EOF never arrives, `try_wait` is never polled (`:787-790`) and the pane neither closes nor shows "Session ended" until the job exits; terminals that watch the child report the exit at once (may be an accepted trade-off for lossless output) (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#hot-21))
- [HOT-27](implicit-state-machines/hotaru-app.md#hot-27) Pointer drag selection and held-button tracking: begin a drag-select, then let the compositor take the pointer (one of its own grabs; the kit comment at `engine.rs:2065-2068` says the release then goes elsewhere): `drag` and `buttons` stay set, so later plain hover over the pane keeps extending the selection and reports button-held motion to mouse-tracking programs, and copy-on-select never fires, until the next click (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#hot-27))
- [KOY-01](implicit-state-machines/koyomi.md#koy-01) Koyomi agent service lifecycle (unit, bus connect retry, listener threads, wake loop): every Open click spawns a child (`desktop/aizome/apps/koyomi/src/agent.rs:339`) whose `Child` is dropped without `wait()`. SIGCHLD isn't ignored, so each child stays a zombie under the long-running agent until the agent exits (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#koy-01))
- [KOY-02](implicit-state-machines/koyomi.md#koy-02) Alert delivery state: due → fired → shown → snoozed/closed: if `Notify` fails because the shell hasn't yet taken org.freedesktop.Notifications at login or is restarting, the alert stays in `fired`, is saved, and is never retried, so catch-up alerts at login can vanish (confidence: medium). After an agent restart (`Restart=on-failure`), Snooze and Open on its notifications still on screen (timeout 0, `desktop/aizome/apps/koyomi/src/agent.rs:324`) do nothing, because `handle` finds no `shown` entry (`desktop/aizome/apps/koyomi/src/agent.rs:237`, `:244`) (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#koy-02))
- [KOY-03](implicit-state-machines/koyomi.md#koy-03) Settings and window-state persistence (unsaved settings, debounced state, agent's stale copy): View ▸ Week Numbers and View ▸ First Day of the Week (`desktop/aizome/apps/koyomi/src/ui/sheets.rs:197-205`, `:225-228`) change `settings` without saving it. The choice is lost at relaunch unless the Settings sheet is later closed with Done (confidence: high). "Snooze for" in Settings (`desktop/aizome/apps/koyomi/src/ui/sheets.rs:138-143`) is saved, but the running agent read `snooze_minutes` once (`desktop/aizome/apps/koyomi/src/agent.rs:105`, `:346`). A new snooze length only takes effect when the agent restarts, normally at the next login (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#koy-03))
- [KOY-04](implicit-state-machines/koyomi.md#koy-04) Navigation and selection: view, anchor, cursor, mini month, selected occurrence: single-click a day in Month or Week view (GridAction::Day sets only `cursor`, `desktop/aizome/apps/koyomi/src/app.rs:1431`), then press ⌘1 or Day. The Day view shows `anchor` (`desktop/aizome/apps/koyomi/src/app.rs:362`; title `desktop/aizome/apps/koyomi/src/app.rs:382`), but the agenda (`desktop/aizome/apps/koyomi/src/ui/mod.rs:411`), the status bar's week (`desktop/aizome/apps/koyomi/src/ui/mod.rs:338`) and New Event (`desktop/aizome/apps/koyomi/src/app.rs:1646`) use the clicked `cursor` day (confidence: high). Select an event, then press → or hide its calendar, then press ⌫. `Msg::Delete` acts on `selected` (`desktop/aizome/apps/koyomi/src/app.rs:1678-1681`; the menu item is enabled by `selected.is_some()`, `desktop/aizome/apps/koyomi/src/ui/sheets.rs:157`), so it deletes an event no longer on screen, while Edit does nothing (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#koy-04))
- [KOY-05](implicit-state-machines/koyomi.md#koy-05) Modal sheet slots: editor, import, name, settings: with the editor open, the desktop action New Event (`aizome-koyomi --new`, `desktop/aizome/apps/koyomi/data/org.aizome.Koyomi.desktop:19`) reaches take_args → NewEvent (`desktop/aizome/apps/koyomi/src/app.rs:1301-1305`). A new Editor replaces the slot (`desktop/aizome/apps/koyomi/src/app.rs:1023`) without closing the old sheet, whose key now renders `ui::view`, the whole main window, as a stacked sheet. Opening an .ics from Files while the import sheet is open does the same to `import` (`desktop/aizome/apps/koyomi/src/app.rs:1333`, `:1234`) (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#koy-05))
- [KOY-10](implicit-state-machines/koyomi.md#koy-10) Calendar save pipeline: dirty → debounce → background write → stamp: when a background write fails (disk full, read-only folder), `dirty` has already been taken, and Saved records the unchanged file's stamp (`desktop/aizome/apps/koyomi/src/app.rs:1774-1775`). Nothing retries, so the edit lives only in memory. It's lost at quit, because close flushes only `dirty` (`desktop/aizome/apps/koyomi/src/app.rs:1828`), and the next external change's reload discards it (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#koy-10))
- [KOY-12](implicit-state-machines/koyomi.md#koy-12) Recurring series split state: master rule, EXDATE/RDATE, RECURRENCE-ID overrides: take a weekly series with one moved instance, open another instance, set Repeat to Never, and Save → All Events. The master's rule is cleared (`desktop/aizome/apps/koyomi/src/app.rs:1141`) but the override stays, so the moved instance still shows as a separate event (`desktop/aizome/apps/koyomi/src/occur.rs:199-206`). Changing the frequency leaves overrides and EXDATEs for instances that no longer exist in the same way (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#koy-12))
- [KOY-14](implicit-state-machines/koyomi.md#koy-14) Store's view of the calendar folder: stamps, reload, trash: an unreadable .ics in the folder (permission denied) is counted by `files()` but not loaded, so `changed_on_disk` stays true forever (`desktop/aizome/apps/koyomi/src/store.rs:99-101`). The app then reloads every 3 s and clears undo/redo each time (`desktop/aizome/apps/koyomi/src/app.rs:1755-1762`), and the agent rebuilds its index at every wake (`desktop/aizome/apps/koyomi/src/agent.rs:114-116`) (confidence: medium). If `create` fails to write, the new calendar stays sorted into `calendars` (`desktop/aizome/apps/koyomi/src/store.rs:143-147`), but NameDone and finish_import skip `reindex` (`desktop/aizome/apps/koyomi/src/app.rs:1536`, `:1249-1252`), so `occ` and the index point at shifted calendars. In Day view the agenda's `calendars[o.cal].events[o.event]` (`desktop/aizome/apps/koyomi/src/ui/mod.rs:423-424`) can then index past the end and panic (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#koy-14))
- [KOY-17](implicit-state-machines/koyomi.md#koy-17) Time-grid pointer drag: move, resize, create; edge hover and scroll: drag an event in Week view and press → (or ⌘→) before releasing, to carry it into the next week. The `Drag.block` index (`desktop/aizome/apps/koyomi/src/ui/week.rs:484`) is re-resolved against the next week's blocks (`desktop/aizome/apps/koyomi/src/ui/week.rs:549`, `:589`), so the drop moves whichever event has that index there, or nothing, instead of the event that was picked up. An external change read by Poll mid-drag does the same (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#koy-17))
- [KOY-18](implicit-state-machines/koyomi.md#koy-18) All-day strip drag: drag an all-day bar and press → (Keys has the focus since the press) into a week with fewer all-day bars: PointerMove indexes `self.data.bars[i]` directly (`desktop/aizome/apps/koyomi/src/ui/week.rs:1109`, and the drop at `:1136`) and panics. With enough bars, it moves a different event (`desktop/aizome/apps/koyomi/src/ui/week.rs:1142-1150`) (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#koy-18))
- [YUK-02](implicit-state-machines/yukimi.md#yuk-02) Update receipt and system completion advice: after a Yukimi activation fails past the commit point (`switch-to-configuration` or `nix-env --set` failing at `apps/yukimi/yukimi-helper/src/main.rs:984`), the receipt stays `activating` for system S1; if the user repairs things with `nixos-rebuild switch` to a different system S2 and even reboots into it, `system_state` still returns Unknown "Activation did not finish successfully" on every read (`apps/yukimi/crates/yukimi-system/src/completion.rs:484-495`) until another Yukimi operation rewrites the receipt. (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#yuk-02)) Fixed in [#290](https://github.com/bitemyapp/sekka/pull/290).
- [YUK-09](implicit-state-machines/yukimi.md#yuk-09) Privileged change transaction (back up, edit, update, build, commit, activate): `--channels` runs `nix-channel --update` (`apps/yukimi/yukimi-helper/src/main.rs:951-956`), but root's channels profile isn't in the backup, so after a failed or stopped build only files are put back: the channels stay advanced, the Updates page calls them current, and the next unrelated change (installing a package) silently rebuilds the whole system on the new channel. (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#yuk-09)) Fixed in [#290](https://github.com/bitemyapp/sekka/pull/290).
- [YUK-10](implicit-state-machines/yukimi.md#yuk-10) Saved per-generation configuration snapshots: return to generation N, run `nix-collect-garbage -d` (deletes N+1…, leaves `generation-<N+1>`), then `nixos-rebuild switch` makes a new N+1; the next Yukimi change calls `save_configuration(N+1, false)` and keeps the stale copy (`apps/yukimi/yukimi-helper/src/main.rs:329-331`), so History and Updates show the old build's revisions and a later return to N+1 restores another build's flake.lock and configuration. (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#yuk-10)) Fixed in [#290](https://github.com/bitemyapp/sekka/pull/290).
- [YUK-11](implicit-state-machines/yukimi.md#yuk-11) Rollback and clean-up sequences: Clean Up with "older than a week", then Stop during the long `nix-store --gc`: `wait()` returns "Stopped" (`apps/yukimi/yukimi-helper/src/main.rs:1095`) before `commit()` and `switch-to-configuration boot` (`:1106-1107`), so the generations are deleted but the boot menu still lists them; the next collection removes their closures and those entries no longer boot. The same happens if GC fails. (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#yuk-11)) Fixed in [#290](https://github.com/bitemyapp/sekka/pull/290).
- [YUK-15](implicit-state-machines/yukimi.md#yuk-15) Package index lifecycle: when `make` fails (`index.rs:142-145`), `indexing` drops to false with the index still None or stale, so every Search keystroke, Discover visit or reload starts another minute-long, multi-GB `nix eval` and posts another note. (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#yuk-15))
- [YUK-19](implicit-state-machines/yukimi.md#yuk-19) Desktops page choice drafts: untick Aizome under Desktops (Yukimi moves the default desktop to Tatami, `app.rs:1077-1085`), then press that group's Undo: `UndoChoice` removes only the desktops draft (`:1415-1418`), so Default desktop stays drafted as "Tatami (chosen)" and Apply writes `calamares.defaultDesktop = "tatami"`, a change the user never made. (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#yuk-19))
- [YUK-20](implicit-state-machines/yukimi.md#yuk-20) Sheet lifecycles: package details, branch chooser, comparison: `Msg::Branches` carries neither input nor sheet key (`app.rs:915`, `:1397-1401`): open the branch chooser for one GitHub input, cancel, open it for another, and whichever GitHub answer arrives last fills the open sheet, so the user can pick another repository's branch and start a Follow that fails in the helper. (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#yuk-20))
- [NTU-01](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-01) Nisshi system-log unlock (root reader through pkexec): open an unreadable /var/log file: `read_file` sets `after_unlock` and the password dialog opens; while it is up the user picks All Messages; on authenticating, root `Ready` takes `after_unlock` and switches the window back to the file the user left (`desktop/aizome/apps/nisshi/src/app.rs:524`, `desktop/aizome/apps/nisshi/src/app.rs:795-797`). (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#ntu-01))
- [NTU-03](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-03) Nisshi journal read pipeline: newest page, earlier pages, follow, whole-boot search: select a row while the 50,000-entry newest page is still streaming; its fields read fails (cursor vacuumed, or "too many reads at once") and `Failed` sets `loading = false` with `Want::Load` still pending: "Reading…" vanishes, Read Earlier enables (`desktop/aizome/apps/nisshi/src/ui/menus.rs:95`), and a pending `--cursor` gives up early (NTU-06) (`desktop/aizome/apps/nisshi/src/app.rs:889-890`). (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#ntu-03))
- [NTU-17](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-17) Tokei alarm schedule: alarms.json against systemd transient timers: a once-alarm rings; the user opens Tokei (the notification names Tokei's desktop entry) → `launched` → `sync_alarms` → `systemctl --user stop aizome-alarm-<id>.timer aizome-alarm-<id>.service` kills `--ring` before it turns the alarm off, then `schedule` arms it for the same time tomorrow; the resident notification's Snooze and Stop now reach nothing (`desktop/aizome/apps/tokei/src/app.rs:451`, `desktop/aizome/apps/tokei/src/alarms.rs:259`, `desktop/aizome/apps/tokei/src/alarms.rs:266-268`). (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#ntu-17))
- [NTU-19](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-19) Tokei's in-memory alarm list, its saves and the undo slot: Tokei stays open while a once-alarm rings and is stopped (file now `enabled: false`); the window still shows it on with "Next: … tomorrow"; toggling any other alarm saves the stale list with it enabled, and the next launch or login `--sync` arms it again (`desktop/aizome/apps/tokei/src/app.rs:220`, `desktop/aizome/apps/tokei/src/app.rs:529-533`, `desktop/aizome/apps/tokei/src/app.rs:324-330`). (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#ntu-19))
- [NTU-26](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-26) Utsushi playback item lifecycle: watching a two-hour film at 45:00, open another file (Open, drop, Open Recent or a second launch): `Msg::Open` sets `queue = paths` (`desktop/aizome/apps/utsushi/src/app.rs:792`), then `load(0)` calls `leave()` (`desktop/aizome/apps/utsushi/src/app.rs:314`) with `current` still 0, recording the new file at 45:00 with the film's length; `resume_at` (`desktop/aizome/apps/utsushi/src/app.rs:327`) then starts the new file at 45:00 under "Resumed at 45:00". If the film had ended, it erases the new file's own resume point instead. (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#ntu-26))
- [KKO-30](implicit-state-machines/kura-koe.md#kko-30) Kura scan lifecycle and page: while a scan runs, press ⌘D and Scan another disk (or ⌘N/⌘O, or launch `aizome-kura PATH` again): `start_scan` cancels the first scan, whose `Scanned(Err(Cancelled))` then arrives and `finish_scan` cancels the second scan's poll timer and clears `scanning` (`desktop/aizome/apps/kura/src/ui/mod.rs:589`), so the window shows "The scan didn't finish: the scan was stopped" while the second scan runs unseen and can no longer be stopped; its result later replaces the page unannounced, and a Rescan meanwhile starts a third scan without cancelling it (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#kko-30))
- [KKO-31](implicit-state-machines/kura-koe.md#kko-31) Analysis and index writing after a scan or a change: scan the home folder (duplicate hashing keeps its analysis busy for a while), then scan a small folder, or trash something so a second analysis starts and finishes first: the first analysis's `Indexed` lands last, so the Suggestions panel shows hints and totals for the earlier tree (another root, or paths already in the Trash) with live Move to Trash buttons, and index.json/index.md describe the older scan (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#kko-31))
- [TSH-02](implicit-state-machines/tansu-shiori.md#tsh-02) Shiori document open and worker-restart lifecycle: after a crash, the supervisor's respawn fails (fork EAGAIN/ENOMEM, `desktop/aizome/apps/shiori/src/engine.rs:140-145`) and sends `OpenFailed(Worker)`. Because `desktop/aizome/apps/shiori/src/app.rs:1167` excludes `Worker` errors, the document that was showing becomes `Load::Failed` (`:1183`): "can't open" covers its pages, find/copy stay pending, and Reload reopens it as a new document at the top, because `pending` was already taken (`:1326-1329`). (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#tsh-02))
- [TSH-08](implicit-state-machines/tansu-shiori.md#tsh-08) Copy waiting for pages' text: the worker gives up and `engine` becomes None, but Load stays Open. The person then selects across pages whose text isn't loaded (pruned, or never read) and copies: `copy` stores the `Copying` (`desktop/aizome/apps/shiori/src/app.rs:1593`), but `ask_copy_text` returns without an engine (`:424`). Nothing is copied, nothing says why (the lasting notice occupies the status), and `idle()` stays false until a reload swap. (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#tsh-08))
- [TSH-09](implicit-state-machines/tansu-shiori.md#tsh-09) Find session: search "foo", press Escape (`find = None`, StopFind), then ⌘F with "bar" selected, or type it, while the worker is still on its current page. The new `Find` starts at serial 0, and `run_find` makes it 1 again (`desktop/aizome/apps/shiori/src/app.rs:1643`). The worker's late `Found{serial: 1}` for "foo" therefore passes the filter at `:1205`, is stored as a "bar" result and, with `jump` set, moves the view to a "foo" hit (`:1208-1211`). `current` can then point past that page's real hits once "bar" overwrites the entry. (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#tsh-09))
- [TSH-25](implicit-state-machines/tansu-shiori.md#tsh-25) Tansu sheet lifecycle (forms holding segment indexes): open Format… on a USB stick's partition, then unplug the stick (or let another tool repartition the disk). `refresh` falls back to the first drive (`desktop/aizome/apps/tansu/src/app.rs:308-311`), so the open sheet's `seg` now indexes that drive's layout. The sheet's title quietly changes to the other partition (`desktop/aizome/apps/tansu/src/ui/sheets.rs:137-140`), and Add to Pending queues a format of it (`desktop/aizome/apps/tansu/src/app.rs:732`); if the fallback is another removable disk, Apply asks for no typed name. Resize, Label and Flags behave the same, and Unlock tries the passphrase on another volume (`:871-873`). (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#tsh-25)) Fixed in [#290](https://github.com/bitemyapp/sekka/pull/290).
- [TSH-26](implicit-state-machines/tansu-shiori.md#tsh-26) Tansu apply session (Apply sheet → Run → Running sheet): while operations run, a second `aizome-tansu image.iso` (or an image opened with Tansu) hands its path over. `Msg::Launched` → `open_sheet(Attach)` closes the Running sheet (`desktop/aizome/apps/tansu/src/app.rs:1104`, `:480-482`). The run finishes in the background, but nothing can now clear `run`: every action answers "Wait for the operations to finish." (`:408-409`), Apply/Undo/Clear/Power Off stay disabled, pending ops are never pruned, and every message resets the status line to "Applied N operations." (`:559-562`) until Tansu restarts. Meanwhile the progress view and Stop After This One are gone. (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#tsh-26))
- [SMT-02](implicit-state-machines/sumi-myaku-tenki.md#smt-02) Table selection, expansion and launch request across readings: Right-click a row, wait a second, choose Inspect: the item carries the row index captured at right-click (`desktop/aizome/apps/myaku/src/ui/view.rs:205`), a reading re-sorts `rows` (by % CPU) before the click, and `Msg::Activate(i)` (`desktop/aizome/apps/myaku/src/app.rs:538-540`) then selects and inspects whatever row now sits at `i`, overriding the correct selection the right-click made (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#smt-02))
- [SMT-03](implicit-state-machines/sumi-myaku-tenki.md#smt-03) Quit/pause confirmation and the status notice: After any quit, pause or refused attempt, the status line keeps that message (in red for "It belongs to another user…" and the like) for the rest of the session, hiding the process/thread/uptime summary and the GPU lens's client count, because `notice` is never reset to None (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#smt-03))
- [SMT-13](implicit-state-machines/sumi-myaku-tenki.md#smt-13) Close-after-save for dirty documents: Close a dirty document, choose Save, then Cancel the "changed by another program" or "can't be saved in Latin-1" alert (or hit a write error): the window stays open but stays in `closing`, so the next ordinary ⌘S, perhaps much later, saves and then closes the window unasked (`desktop/aizome/apps/sumi/src/app.rs:542-544`) (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#smt-13))
- [SMT-14](implicit-state-machines/sumi-myaku-tenki.md#smt-14) Link and Go to Line sheets: Open Link… (⌘K) in document A, switch to document B and open Link… there: typing in B's sheet updates whichever Link sheet the map yields first, so B's field may not take the text, and Insert Link can insert A's link into A and close A's sheet while B's stays open (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#smt-14))
- [SMT-17](implicit-state-machines/sumi-myaku-tenki.md#smt-17) Document file, dirty and autosave lifecycle: Change a document's Encoding or Line Endings (the title says "— Edited"), then Quit, which promises everything comes back: on relaunch the document is reloaded in its detected encoding and clean, so the unsaved format change is gone; a Reopen with Encoding choice is lost the same way (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#smt-17))
- [SMT-18](implicit-state-machines/sumi-myaku-tenki.md#smt-18) Background file read generation: Open a >2 MiB file that can be stat'ed but not read (a root-only log): after the alert, a window titled with the file's name remains holding an empty "Untitled" document; and a Revert or Reopen with Encoding of a >2 MiB document whose file became unreadable detaches the document from its file (path cleared), whereas the same failure for a small file keeps the path (`desktop/aizome/apps/sumi/src/app.rs:1125`) (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#smt-18))
- [SMT-19](implicit-state-machines/sumi-myaku-tenki.md#smt-19) Find bar and search generation: Find a word, press Done (or Escape), then type above a match: the match highlights stay painted at their old character offsets and drift onto other text; and in a >1 MiB document, ⌘G right after an edit selects a range from the pre-edit match list (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#smt-19))
- [SMT-24](implicit-state-machines/sumi-myaku-tenki.md#smt-24) Location consent and GeoClue lookup: Allow location, let it resolve, then choose Not Now or turn Show Current Location off: the last GeoClue position stays in `state.json` and is written again whenever a place is selected, although the tests take care that answers arriving after a decline are not saved (`desktop/aizome/apps/tenki/tests/tenki.rs:596`) (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#smt-24))
- [SAP-02](implicit-state-machines/small-apps.md#sap-02) Single-instance socket claim and hand-off (Annai, Shashin, Soroban; Fusen through Sumi's copy): two launches started together (two pictures opened from Hako at once) both fail to connect; the second's `remove_file` unlinks the first's freshly bound socket (`desktop/aizome/apps/shashin/src/instance.rs:40-42`) and binds its own, so two instances run and the first can never be raised or handed files again (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#sap-02))
- [SAP-05](implicit-state-machines/small-apps.md#sap-05) Fusen stickies: windows, saved places and the login item: restoring N stickies at login: the first `open_sticky` → `save_stickies` replaces `saved.stickies` with that one sticky (`desktop/aizome/apps/fusen/src/app.rs:598`), so stickies 2..N are re-added with `place: None` (`desktop/aizome/apps/fusen/src/app.rs:595`) and written out; only a later `Placed` (4 s on, Aizome's IPC only) puts their places back. Quitting or logging out within 4 s, or a session without the IPC, loses their positions for good (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#sap-05))
- [SAP-06](implicit-state-machines/small-apps.md#sap-06) Fusen launch, request hand-off and Drop-time save: a second launch while Fusen runs (the shell's Services "New Note", `aizome-fusen --new …`, a launcher click) builds a full `Fusen` (`desktop/aizome/apps/fusen/src/main.rs:42`), gets `Claim::Handed` and returns (`desktop/aizome/apps/fusen/src/main.rs:44`); Drop → `save_all` → `save_stickies` with no stickies writes `{"stickies": []}` (`desktop/aizome/apps/fusen/src/app.rs:598-603`) and deletes the login item (`desktop/aizome/apps/fusen/src/app.rs:607-608`). The running Fusen rewrites both only at its next `save_stickies`; if it's killed or the session ends first, no stickies come back at the next login (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#sap-06))
- [SAP-07](implicit-state-machines/small-apps.md#sap-07) Fusen highlighting passes (incomplete, scheduled, notify): once a pass sets `incomplete` (a jump more than ~3000 lines past what's worked out at the guessed rate, fewer on a slow machine, or a 3 ms budget overrun), `state_at` never works states out again (`desktop/aizome/apps/sumi/src/syntax/cache.rs:226`), so every line after the last edit is styled from a default guess (in a long note, lines under an edit inside a fenced code block show as prose) until a reload or formatting command resets it (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#sap-07))
- [SAP-10](implicit-state-machines/small-apps.md#sap-10) Shashin picture loading (serial, decode, folder listing): open folder A, then folder B (a drop, the Open panel or a second launch) before A is listed: both `Listed` results pass while `current_path()` is None (`desktop/aizome/apps/shashin/src/app.rs:1198`); whichever lands first shows its first picture and the other is dropped by the same-folder check (`desktop/aizome/apps/shashin/src/app.rs:1198-1201`), so asking for B can end showing A (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#sap-10))
- [SAP-11](implicit-state-machines/small-apps.md#sap-11) Shashin unsaved turns, saving and quitting: ❖S twice during a slow rewrite (a large PNG or TIFF), or ❖Q → Save and then ❖Q → Save again while the first `SaveAllAndQuit` runs (turns stay set until `SavedAll`, so the alert returns), runs `save_turned` twice with the same turns: a picture turned 90° is saved at 180° (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#sap-11))
- [SAP-13](implicit-state-machines/small-apps.md#sap-13) Shashin stage drawing requests (busy, want, reported): zoom with the toolbar's Zoom In or ❖= (`desktop/aizome/apps/shashin/src/ui.rs:277`) while a large photo decodes: the stage reports `Seen { exact: false }`, `run_jobs` takes and drops it because the picture isn't decoded (`desktop/aizome/apps/shashin/src/app.rs:723-727`); after `install` the zoom, centre and view are unchanged, so the report key is the same and the stage never reports again (`desktop/aizome/apps/shashin/src/ui.rs:378`). The picture stays drawn from the fit drawing, nearest-pixel scaled, until the view changes (`install` re-asks only for turned pictures, `desktop/aizome/apps/shashin/src/app.rs:640-642`) (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#sap-13))
- [SAP-18](implicit-state-machines/small-apps.md#sap-18) Shashin Move to Trash with Undo and Redo: move a picture of folder A to the Trash, open folder B, press ❖Z: the file returns to A, but its path is inserted into B's list (`desktop/aizome/apps/shashin/src/app.rs:905-907`) and shown there with B's pictures as its neighbours (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#sap-18))
- [SAP-25](implicit-state-machines/small-apps.md#sap-25) Soroban calculator entry (entry, answer, error, 2nd, undo): after ❖Z, typing a key that does nothing — `)` with no parenthesis open (`desktop/aizome/apps/soroban/src/entry.rs:833-835`), a second `.` (`desktop/aizome/apps/soroban/src/entry.rs:764-766`), or a digit outside the radix (`desktop/aizome/apps/soroban/src/entry.rs:742-747`) — still runs `remember()`, which pushes a no-op undo step and empties the redo stack, so ❖⇧Z stops working though nothing changed (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#sap-25))
- [HCO-07](implicit-state-machines/hotaru-core.md#hco-07) Kitty clipboard session password grants and paste-event one-time passwords: a program enables mode 5522 and, after each paste event, reads only the listing (or nothing); 32 pastes later the password the user chose to remember is evicted by `entries.remove(0)` and the program is prompted again (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#hco-07))
- [HCO-18](implicit-state-machines/hotaru-core.md#hco-18) Cursor style (DECSCUSR, DEC mode 12, configured default): the user changes the cursor preference while vim/less runs on the 1049 alternate screen (`desktop/aizome/apps/hotaru/src/session.rs:529` calls `set_default_cursor`); only `alternate.cursor.shape` changes, and on exit the primary keeps the old default shape though `cursor_is_default` is true, until the next DECSCUSR or preference change (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#hco-18))
- [HCO-24](implicit-state-machines/hotaru-core.md#hco-24) PTY child lifecycle and background reaper (HUP → grace → KILL → wait): `get_or_init` stores an `Err` permanently, so if creating the reaper thread fails once (EAGAIN under RLIMIT_NPROC or memory pressure), every later `Pty::spawn` in that long-lived Hotaru process fails with the cached error after the condition clears (confidence: medium — certain once thread creation fails, which is rare) ([reproduction](implicit-state-machines/reproductions.md#hco-24))
- [HCO-26](implicit-state-machines/hotaru-core.md#hco-26) Shell prompt hooks: load guards and last-reported-cwd de-duplication (OSC 7): the last-PWD cache is never invalidated when something else reports OSC 7. In `~/src`, run a nested integrated shell (or a program that emits OSC 7), `cd /tmp`, then exit. The outer prompt's PWD equals `_hotaru_last_pwd`, so it stays silent, and Hotaru keeps `/tmp` as the pane's directory and title for new tabs (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#hco-26))
- [HTT-10](implicit-state-machines/hotaru-tests-tools.md#htt-10) (dev tool) Frame benchmark controller run lifecycle: on frame-matrix's 600 s timeout the harness gets SIGINT (`frame-matrix.py:241`); `KeyboardInterrupt` bypasses `except Exception` (`apps/hotaru/tools/frame-benchmark.py:626`), so `finally` writes `result.json` still saying `"status": "running"` (`apps/hotaru/tools/frame-benchmark.py:403`, `apps/hotaru/tools/frame-benchmark.py:640`) for a terminated run (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#htt-10))
- [HTT-14](implicit-state-machines/hotaru-tests-tools.md#htt-14) (dev tool) Frame matrix batch status and trial acceptance: `--phase throughput` trials can be accepted even if an external compiler ran during the throughput flood. The measured interval ends at the last stream capture (`:380-382`), but frame-benchmark runs throughput after the streams (`frame-benchmark.py:599-619`), so `compiler_overlap` (`apps/hotaru/tools/frame-matrix.py:384`) never examines that window (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#htt-14))
- [GEN-03](implicit-state-machines/genkan.md#gen-03) Selected-disk identity: discovered → reviewed → revalidated before and after the first write: If another USB disk is unplugged or re-enumerated between `lsblk` and the per-disk `fs::metadata` during the post-partition or post-format revalidation (`install.rs:573`, `install.rs:628`), the installation aborts with an error about an unrelated disk. The target disk has already been wiped and partitioned (confidence: medium-low). ([reproduction](implicit-state-machines/reproductions.md#gen-03)) Fixed in [#290](https://github.com/bitemyapp/sekka/pull/290).
- [GEN-07](implicit-state-machines/genkan.md#gen-07) Install transaction: non-destructive preparation stages, then destructive steps 1–6: The deferred route skips the "selected system needs at least N GiB on the root filesystem" guard (`install.rs:479`–`488`), which runs only after an in-memory build. A low-RAM machine with a large selection on a small disk therefore passes preparation. It then fails with "No space left" while building on the target, after the disk was erased. `missing.fetch_bytes` is known by then and could be checked (confidence: medium). ([reproduction](implicit-state-machines/reproductions.md#gen-07)) Fixed in [#290](https://github.com/bitemyapp/sekka/pull/290).
- [GEN-28](implicit-state-machines/genkan.md#gen-28) Disk scan, selection and installed-memory read: If `/proc/meminfo` can't be read, `Msg::Memory(Err)` turns swap off and leaves `memory` as `None` (`app.rs:1989`–`1992`). If the person turns swap back on, the Disk page says "Reading installed memory…" and Next says "Still reading installed memory; try again in a moment." indefinitely (`app.rs:916`–`918`, `app.rs:951`–`953`) (confidence: high in code; the trigger is rare). ([reproduction](implicit-state-machines/reproductions.md#gen-28)) Fixed in [#290](https://github.com/bitemyapp/sekka/pull/290).
- [GEN-34](implicit-state-machines/genkan.md#gen-34) Review, background preparation and confirmation (GUI side of the helper session): Suppose the person confirms before preparation finishes and preparation then fails (for example, a download error during the in-memory build, "the target disk has not been erased"). `show_failure` takes the `installing` branch (`app.rs:1768`–`1776`). It marks step 1 "Partition the disk" as failed, heads the page "Installation Failed", and shows "The disk may have been modified" (`installer/genkan/ui/src/view/install.rs:73`). No disk write happened, the person can't go back, and they must re-enter every password (confidence: medium). ([reproduction](implicit-state-machines/reproductions.md#gen-34)) Fixed in [#290](https://github.com/bitemyapp/sekka/pull/290).
- [TTL-03](implicit-state-machines/tatami-tool.md#ttl-03) Keep Awake: intent marker, inhibitor holder unit and verification: one slow logind reply (`busctl --timeout=3`, `awake.rs:21-22`) makes `protected(...)?` return Err in `hold` (`awake.rs:138`); the holder exits, `systemd-inhibit` releases the block, and for `RestartSec=1` plus re-verification nothing blocks lid-close or idle sleep (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#ttl-03))
- [TTL-04](implicit-state-machines/tatami-tool.md#ttl-04) Wallpaper choice and per-output overrides: a global wallpaper on a removable or late-mounted drive (udiskie automounts concurrently, `desktop/tatami/module.nix:311`) is a dangling symlink when `tatami-swaybg`'s `ExecStartPre=background ensure` runs at login; `ensure` replaces it with the default for good, whereas the same image as a per-output choice would survive (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#ttl-04))
- [TTL-08](implicit-state-machines/tatami-tool.md#ttl-08) Screen recording start, fallback and stop: stopping within ~3 s of starting (Alt+Print, the bar indicator or the menu) SIGINTs gsr while the starting invocation is still in `started(15)`; it sees no recorder, takes that for "gsr cannot run" and starts the wf-recorder or mixed fallback on the same file (`capture.rs:584-605`), so recording resumes after the user stopped it (or contends with the just-saved file) (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#ttl-08))
- [TTL-10](implicit-state-machines/tatami-tool.md#ttl-10) Display reconciliation commit (plan execution): closing the lid while docked, if a step errs after the panel was disabled (e.g. `wait_disabled`'s 2 s, `:153-165`, expires on a slow modeset that then completes, or an unmirror wait fails), returns Err before `display_policy::save` (`:399-400`); next tick the panel is already off, so `plan` does not add it to `lid_off` (`desktop/tatami/tool/src/display_policy.rs:274-279`) and saves a record without it; opening the lid then never turns the built-in display back on (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#ttl-10))
- [TTL-17](implicit-state-machines/tatami-tool.md#ttl-17) Live reload pipeline (`tatami reload`, `update-session`): `tatami reload` try-restarts every running own unit except Keep Awake (`main.rs:231`), including an active `tatami-hyprsunset` and `tatami-mako`; hyprsunset returns at its identity profile (`desktop/tatami/config/hypr/hyprsunset.conf:2-5`) and mako without `do-not-disturb`, so an appearance reload or a Yukimi `update-session` silently turns off Night Light and Do Not Disturb (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#ttl-17))
- [TTL-25](implicit-state-machines/tatami-tool.md#ttl-25) Weather report cache and fetch coordination: choosing a new place while a background fetch attempt is in flight (it read `weather-place.json` at `weather.rs:319`) makes `refresh` return at the lock (`:554-556`); the running fetch writes the old place's report with a fresh `fetched`, so `due` stays false and the bar shows the old place for up to 15 minutes (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#ttl-25))
- [TAP-03](implicit-state-machines/tatami-apps.md#tap-03) Activity sampler's baselines, caches and energy integrals between readings: With "Update Every Second", the processor counters come from the power meter service, which returns its cached text when asked within 1 s of its last fresh read (`desktop/tatami/tool/src/power_meter.rs:100-101`). The power read happens after the per-process scan, whose length varies, so requests regularly arrive just under 1 s after the last fresh read. That reading's delta is 0 (0 W), the next is about twice the real power, and the first 0 W reading pins the idle floor at 0 for the session. A service read that times out doubles the next reading the same way (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#tap-03))
- [TAP-04](implicit-state-machines/tatami-apps.md#tap-04) Activity process table: columns per view and remembered sorts: At start-up `last` is None, and `set_lens` sorts the CPU view by % CPU descending while `swapping` is set. The person clicks "% CPU" to sort ascending. GTK toggles it to ascending, the handler sees `changed` (None ≠ % CPU) and schedules a flip back to descending (:589-597), so the click is undone and only a second click reverses the sort. The same happens to the first click on the restored sort column after each view switch, whenever `last` names a different column. Clicking the column `last` does name leaves it ascending: smallest first, against the rule (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#tap-04))
- [TAP-06](implicit-state-machines/tatami-apps.md#tap-06) Window singleton, deferred first presentation and launch requests held until the first reading (Activity, Network, Sound): `tatami-activity --pid N`, run while the window exists but before its first frame (within about half a second of the first launch), calls `select_pid` with an empty `pids` map (`desktop/tatami/activity/src/window.rs:97-98`, :441-444). It toasts "Process N has ended" for a live process and selects nothing, where Network guards the same case with `loaded` (`desktop/tatami/network/src/window.rs:420-422`) (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#tap-06))
- [TAP-08](implicit-state-machines/tatami-apps.md#tap-08) Activity's Quit, Force Quit and Pause on a target taken from the latest frame: The person opens "Quit “X”?" and leaves it open. X exits on its own and another of their processes reuses its PID. Choosing Force Quit then SIGKILLs the unrelated process (:741), because `target.all` holds bare PIDs from the frame shown when the dialog opened (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#tap-08))
- [TAP-15](implicit-state-machines/tatami-apps.md#tap-15) Bluetooth window close: hide, drain, release, destroy: Relaunching Bluetooth while the close drain runs (for example while CancelPairing waits on a slow adapter) presents the hidden window again (:71-72). Every control is dead, because `closing` gates refresh and actions, and when the drain finishes it destroys the window under the person (:321-323) (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#tap-15))
- [TAP-19](implicit-state-machines/tatami-apps.md#tap-19) Displays readings and applies (concurrent changes refused, stale readings discarded): A monitor is plugged in while an SDR brightness or saturation change applies (`quietly`, which schedules no refresh). The `monitoradded` refresh 300 ms later finds `applying` set and returns (:195), or the refresh already running is discarded by the revision check (:219). Nothing reads again, so the window doesn't show the new display until another Hyprland event. A second event that arrives while a refresh runs is likewise dropped, and that reading may predate it (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#tap-19))
- [TAP-21](implicit-state-machines/tatami-apps.md#tap-21) Night Shift requests on unordered threads: Night Shift is turned on and then off within a second or two. The "off" thread finds nothing tinted yet and does nothing, then the "on" thread finishes tinting, so the screen stays warm with the switch off. Likewise, dragging the temperature and turning the switch off within 300 ms: the debounced temperature request runs after "off" and turns the tint back on (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#tap-21))
- [TAP-25](implicit-state-machines/tatami-apps.md#tap-25) Joining a Wi-Fi network: one at a time, with a password retry and rollback: Closing the window (Escape) while a new network is being joined ends the app, since GTK quits when its last window goes. The worker thread dies before `nmcli --wait 45 connection up` returns. `Secret`'s Drop never removes the plaintext passwd-file from `$XDG_RUNTIME_DIR` (`nm.rs:199-203`), and a failed join's new profile is never deleted (`nm.rs:324-326`), so a saved network with the mistyped password stays behind (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#tap-25))
- [KTL-02](implicit-state-machines/kakehashi-tools.md#ktl-02) Kakehashi nightly download session: dropping a file onto the window during a download sets `this.download = None` (`apps/kakehashi/app/src/view.rs:638`) without storing `cancel`, so the worker keeps downloading into Downloads in the background; the Download buttons re-enable, and restarting the same build runs a second worker on the same `.part` (no lock, `apps/kakehashi/core/src/nightly.rs:138`), the orphan's `Msg::Fetched` with the same id ends the new session (`apps/kakehashi/app/src/view.rs:280`), and an orphan in fallback mode truncates the shared file (`apps/kakehashi/core/src/nightly/parallel.rs:210`) (confidence: high for the orphaned download; medium for the double-writer outcome) ([reproduction](implicit-state-machines/reproductions.md#ktl-02))
- [KTL-03](implicit-state-machines/kakehashi-tools.md#ktl-03) Kakehashi write job, GUI side of the helper protocol: when the helper dies by a signal mid-write on Linux (OOM killer, `kill`), `elevate::run` returns `Ran::Failed("the helper was killed")` (`apps/kakehashi/app/src/elevate.rs:51`) and `JobEnded` maps every `Ran::Failed` to `Failed { touched: false }` (`apps/kakehashi/app/src/view.rs:319`) whatever the last `Running(Writing|Verifying)` stage was, so the window says "Not written" without the "partly written" warning (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#ktl-03)) Fixed in [#290](https://github.com/bitemyapp/sekka/pull/290).
- [KTL-04](implicit-state-machines/kakehashi-tools.md#ktl-04) Kakehashi drive list polling and chosen drive: `confirm` describes the drive captured when the dialog opens (`apps/kakehashi/app/src/view.rs:479-490`), but OK calls `start_write` (`apps/kakehashi/app/src/view.rs:502`), which re-reads `chosen_drive()` (`apps/kakehashi/app/src/view.rs:511`) from a list that keeps refreshing under the modal; if the stick is swapped for another that gets the same name (`sdb`, `disk4`) between two refreshes, the plan and the helper's `still_there` both use the new drive, and a drive other than the one named in "Erase …?" is erased (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#ktl-04)) Fixed in [#290](https://github.com/bitemyapp/sekka/pull/290).
- [KTL-13](implicit-state-machines/kakehashi-tools.md#ktl-13) Rootless builder VM host driver: the host waits for the guest's whole `build-iso` through `run()` (`rust/src/builder.rs:306`), capped at 7200 s (`rust/src/support.rs:67`), while the guest runs `nix flake check` and `nix build .#iso` under separate 7200 s caps (`rust/src/builder.rs:396`, `rust/src/builder.rs:429`) with `--max-jobs 1`; a cold build whose two steps together pass two hours is killed from the host ("Child process timed out") although each guest step is within its own limit (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#ktl-13))
- [INX-34](implicit-state-machines/infra-nix.md#inx-34) Binary-cache publication pipeline: `:61` and `:64` use process substitution, whose exit status `errexit`/`pipefail` ignore (the comment at `:40-41` fixed this only for the build). If `nix-store --store https://cache.nixos.org --check-validity` fails part-way (network error, 5xx), `own` keeps only the paths checked before the failure, the script uploads that subset or nothing, prints "Published N of M" and exits 0: the nightly reports success while the cache lacks Sekka's packages and installs compile them (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#inx-34))
- [INX-36](implicit-state-machines/infra-nix.md#inx-36) Self-hosted runner supervisor and per-job admission: if `docker run` fails to start (image `sekka-runner:latest` not loaded, missing bind source `/var/cache/sekka-ci-git`), the loop registers a new JIT runner with GitHub every ~5 s plus refresh time (`:176-207`) with no backoff, leaving a trail of never-connected runner registrations and API calls until someone notices (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#inx-36))
- [ICI-03](implicit-state-machines/infra-ci-tests.md#ici-03) Trusted PR admission, recheck and commit-status lifecycle: a PR queued behind the nightly (up to 12 h) or behind another merge: main moves, the recheck returns a new merge (or none), `test "$CURRENT_MERGE" = "$EXPECTED_MERGE"` fails, and the head is marked `failure` ("Build and tests: failure") even though nothing was built or tested. Nothing re-admits the PR (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#ici-03))
- [ICI-11](implicit-state-machines/infra-ci-tests.md#ici-11) Video playback state (poster/loading/playing/paused/ended/failed) with seek-drag resume: while dragging the seek thumb during playback, pressing K twice pauses and then calls `play()`, whose `begin()` creates source S1. On pointer-up, `release()` sees `resume` still true and calls `begin()` again, creating S2 without stopping S1. S1 is orphaned: it plays over S2, cannot be paused, and its `onended=finish` later jumps the player to `ended` (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#ici-11))

### Partly confirmed (9)

- [CPB-22](implicit-state-machines/comp-b.md#cpb-22) ext-image-copy-capture session and frame lifecycle: a frame copied with a fence is replaced (`CreateFrame`) before the fence signals; its copy already advanced the stream's damage, `ready` drops it, and the new frame then finds `Unchanged` and waits for the next change, so an idle screen stalls the resumed portal stream (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#cpb-22))
- [SYA-17](implicit-state-machines/sys-a.md#sya-17) Level meter thread lifecycle: pipewire-pulse restarts, or the default microphone disappears and comes back under the same name, while Settei's Sound pane is open: the meter thread exits (or idles on a dead stream) with `level` at its last peak, and Settei, which polls `level()` on a timer and recreates the meter only when the default input's name changes (`desktop/aizome/apps/settei/src/panes/sound.rs:208-216`), shows a frozen input level (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#sya-17))
- [SYC-06](implicit-state-machines/sys-c-config.md#syc-06) Drawn element press, drag, drag-out and hover gesture: during a desktop rubber-band drag, or a Deskbar app drag, the compositor takes the pointer before the release. When the pointer returns, Drawn resets silently at `drawn.rs:376`, so `desktop.band` stays `Some` and the band stays drawn (`desktop/aizome/crates/aizome-shell/src/desktop_glue.rs:225`/`:233`). For the Deskbar drag, `deskbar_visibility.dragging` stays set and holds the Deskbar open (`desktop/aizome/crates/aizome-shell/src/deskbar_glue.rs:371`/`:376`) (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#syc-06))
- [STB-13](implicit-state-machines/settei-b.md#stb-13) Display change: apply, "Keep these display settings?" countdown, keep or revert: closing the window or quitting (⌘Q) during the 15 s countdown exits without `revert`, leaving a resolution, rotation, mirror or off change applied in the compositor though it was never kept (and isn't in the file) (confidence: medium). Also: a modeset slower than the 10 s read timeout returns `Err`, so no countdown starts and the change stays live, unconfirmed and unpersisted (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#stb-13))
- [KKO-01](implicit-state-machines/kura-koe.md#kko-01) Dictation on/off: koe.toml `enabled` gating the user unit and the daemon it runs: a crash loop (e.g. a startup error) under `Restart=on-failure`/`RestartSec=2` can trip systemd's default start limit; turning dictation on or pressing the shortcut then makes `systemctl start` fail, so `Session::start` spawns an unmanaged `aizome-koe daemon` outside the unit with stderr to /dev/null (`desktop/aizome/apps/koe/src/service.rs:103-115`), and the window shows only "Koe's service didn't start" instead of the journal reason (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#kko-01))
- [GEN-33](implicit-state-machines/genkan.md#gen-33) Keyboard step: catching the key beside the space bar through the compositor: Suppose the person clicks Review while the key wait is still open. The review worker's Wi-Fi snapshot (`app.rs:1614`) can raise a Secret Agent or wallet prompt, and the Polkit prompt can appear within the 500 ms poll window after `Reviewed`. The compositor holds back the first key typed into that prompt and delivers it to Genkan as the caught key. The password loses its first character, and the GUI's `super_key` silently becomes `AsIs` (confidence: low). ([reproduction](implicit-state-machines/reproductions.md#gen-33))
- [KTL-20](implicit-state-machines/kakehashi-tools.md#ktl-20) (test harness) qemu-test install/reboot run and resumable evidence: `boot_installed` rebuilds the fixture's `verify` command from the current CALAMARES_TEST_* environment (`fixture_options()` at `rust/src/vm.rs:687`), but `result.json` records only `encrypted` (`rust/src/vm.rs:848`); a run installed with `CALAMARES_TEST_GITHUB` set and resumed in a fresh shell silently skips the fixture's SSH-key, sshd and Git assertions (`installer/genkan/tests/vm-fixture/src/main.rs:831`), and the reverse fails a good install (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#ktl-20))
- [INX-18](implicit-state-machines/infra-nix.md#inx-18) Copy-to-RAM of the live media in the initrd: the media size is the size of the block device holding `/sysroot/iso` (`:23`), not of the ISO. If that device is a whole USB stick (a raw-written hybrid ISO found by label on the disk rather than its partition), `bytes` is the stick's capacity, so a 64 GB stick on a 32 GB machine is reported "insufficient RAM" and copytoram is silently skipped though the image would fit (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#inx-18))
- [INX-29](implicit-state-machines/infra-nix.md#inx-29) IBus Wayland frontend keymaps, seat and system-keymap mode (patched): change the Aizome layout (e.g. US → German) while `aizome-ibus` runs with `use-system-keyboard-layout` on: the early return at `:175-178` keeps the old `key_sys`, so engines get keysyms and modifiers from the old layout. This is the probable mechanism of the known limit in DEVELOPERS.md:368-373 (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#inx-29))

### Not confirmed (2)

- [SKB-27](implicit-state-machines/sukumo-backend.md#skb-27) Selection and primary-selection ownership: we clear an already-empty selection on a compositor that doesn't echo a no-op clear; later a client copies, then quits: its clear is swallowed as ours (`selection.rs:536`), so the shared state keeps the dead client's types and read-ahead text and our paste returns it (confidence: low) ([reproduction](implicit-state-machines/reproductions.md#skb-27))
- [STA-05](implicit-state-machines/settei-a.md#sta-05) Language & Region formats preview with a stale-locale guard: choose a Region format and cancel the polkit prompt (or have localed refuse): `preview` stays `None` and `Done(Err)` doesn't call `refresh_preview`, so the Formats section shows an indeterminate spinner until the page is shown again or localed signals some other change (confidence: high). ([reproduction](implicit-state-machines/reproductions.md#sta-05))

### Not reproduced (environment) (5)

- [CPA-04](implicit-state-machines/comp-a.md#cpa-04) libseat session pause and resume (VT switching): a monitor plugged in while the user is on another VT stays dark after they return. The udev `Changed` event runs `device_changed` while paused and reserves the connector (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:834`); `initialize_output` fails on the inactive device (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:1037`); `ActivateSession` never calls `device_changed`; and later `Changed` events skip the connector as already known (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:823`) until it is unplugged (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#cpa-04))
- [CPA-05](implicit-state-machines/comp-a.md#cpa-05) DRM device and connector hotplug, and output enablement: (1) Any setup failure leaves the connector reserved without a surface: no modes (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:962`), no renderer (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:1010`), or `initialize_output` (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:1037`). Later `Changed` events skip it as known (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:823`), so that monitor never lights until it is replugged (confidence: medium). (2) `enable_output` removes the `disabled` entry (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:928`) before it looks up the device (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:929`), reads the connector (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:934`) and sets the output up (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:936`). On failure the output either still shows as off but is refused by the next enable as "isn't off", or (past `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:935`) becomes unknown to IPC and Settings entirely (confidence: medium). (3) `device_removed` disconnects only the CRTCs that have surfaces (`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:853`), so an output turned off on a removed GPU (for example a DisplayLink dock) stays in `Udev.disabled` and `disabled_outputs`. Enabling it fails with "its device is gone" after the entry has been dropped. Unless the settings also say off (which dedupes at `desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs:1002`), a replug adds a driven output with the same name beside the stale one (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#cpa-05))
- [CPB-24](implicit-state-machines/comp-b.md#cpb-24) wlr-output-management configuration lifecycle: kanshi applies a profile of two heads and the second fails (for example "has no mode WxH" from `backend/udev.rs`); the first head's change stays applied while the client is told `failed`, so the layout is half-changed against the protocol's rule to revert on failure (`output_management.rs:339-346`) (confidence: high) ([reproduction](implicit-state-machines/reproductions.md#cpb-24))
- [KTL-12](implicit-state-machines/kakehashi-tools.md#ktl-12) Helper write/verify pipeline and `touched`: on Windows `platform::open` deletes the drive layout (`apps/kakehashi/core/src/platform/windows.rs:309`) before `*touched = true` (`apps/kakehashi/core/src/write.rs:199`); a failed size check, second `still_there`, fit check, or a Stop seen at `apps/kakehashi/core/src/write.rs:196` then reports `Failed { touched: false }`, with no "partly written" warning, although the partition table is already gone (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#ktl-12))
- [KTL-24](implicit-state-machines/kakehashi-tools.md#ktl-24) flash_usb privileged write and verify step: on macOS the raw device opened at `scripts/flash_usb.rs:290-298` is flushed with `device.sync_all()` (`scripts/flash_usb.rs:637`), i.e. F_FULLFSYNC, which Kakehashi documents and tests as failing with ENOTTY on `/dev/rdiskN` (`apps/kakehashi/core/src/platform/macos.rs:177-194`, test `apps/kakehashi/core/src/platform/macos.rs:229`); every macOS flash therefore fails after writing the whole image, before verification and ejection (confidence: medium) ([reproduction](implicit-state-machines/reproductions.md#ktl-24))

### Unverified (32)

- [CPA-17](implicit-state-machines/comp-a.md#cpa-17) Two-press confirmation (Log Out, Text Console): after the first Text Console press, the Text Console button reads "Really Log Out?" (the label is hard-coded for any pending confirmation, `desktop/aizome/crates/aizome-comp-core/src/recovery/view.rs:377`), on the screen meant to rescue a stuck session (confidence: high)
- [CPA-20](implicit-state-machines/comp-a.md#cpa-20) Recovery ping and sample cadence, and the sample cache: after an app with a CRASHED sample is relaunched with the same app ID, IPC `RecoveryState` with the overlay closed (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:1071`) keeps reporting it as crashed, since nothing is sampled while closed. At the next Ctrl+Alt+Del it is preselected as the first app in trouble (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:518`), and Enter force-quits the healthy app (`desktop/aizome/crates/aizome-comp-core/src/recovery/mod.rs:758`) until the first new sample lands (confidence: medium)
- [CPA-25](implicit-state-machines/comp-a.md#cpa-25) X11 PID lookup thread with a deadline: with xwayland-satellite's X server hung, every sample while the overlay is open spawns, for each X11 app (twice for a fresh app), a thread that never ends; keeping the overlay open for a minute leaks on the order of a hundred threads and sockets (confidence: medium)
- [SHA-14](implicit-state-machines/shell-a.md#sha-14) File index cache: validity, rebuild and refresh timer:  - The scenario: at login the cache is older than `reindex_minutes`, so the startup `index_soon(false)` begins a rebuild. Opening the palette before it finishes runs `index_soon(true)`, which sees the same stale cache and builds again. - Both threads `File::create` the same `.tmp` and rename it. This wastes a full home-directory scan and can tear the cache. - The same stacking happens when a refresh tick fires while a slow build is still running. - Confidence: medium.
- [SHA-18](implicit-state-machines/shell-a.md#sha-18) Desk rename in place:  - The scenario: while Desk 3 is being renamed, a lower desk is removed (shortcut, `aizomectl`, another bar), so the desks renumber. - The field jumps to whichever desk now has index 3. If none does, it vanishes while `renaming` stays `Some` and reappears on the next desk created as 3. - Enter renames the wrong desk. - Confidence: medium.
- [SHA-20](implicit-state-machines/shell-a.md#sha-20) App index load and reload:  - The scenario: a `home-manager` or `nixos-rebuild` switch rewrites the application dirs in a burst. - An early reload that saw a partial tree finishes last. Apps then stay missing from the Toolchest, palette and Deskbar until the next change. - Confidence: low.
- [SHA-21](implicit-state-machines/shell-a.md#sha-21) Clock tick timer cadence:  - The scenario: turning on `[menubar] clock_seconds` makes the bar label show seconds immediately. - The pending tick is still due at the next minute (`clock.rs:111`–`clock.rs:114`), so the seconds freeze for up to 59 s. - Confidence: medium.
- [SHA-22](implicit-state-machines/shell-a.md#sha-22) logind sleep-delay handshake (session-events side):  - The scenario: a lock was requested moments before suspend, so `phase == Locking`. This happens with `loginctl lock-session && systemctl suspend`, or an idle lock racing a lid close. - The `else` branch calls `sleep_ready()` at once, so the machine sleeps before the compositor confirms the lock. - On resume, the unlocked desktop can show briefly. - Confidence: medium.
- [SYA-08](implicit-state-machines/sys-a.md#sya-08) ObjectManager follower cache (BlueZ, UDisks): UDisks is started by activation (`disks/mod.rs:236`, `activate = true`) and is still enumerating when `GetManagedObjects` hits the 2 s query timeout; Tansu and the desktop then show UDisks as available with no drives until udisksd restarts (confidence: low)
- [SKB-08](implicit-state-machines/sukumo-backend.md#skb-08) Session lock lifecycle: the lock screen asks to lock and unlocks almost at once (authentication done before the compositor's `locked` reaches this thread): `unlock` drops the unconfirmed lock, the compositor sees `destroy` after sending `locked`, kills the client with `invalid_destroy`, and the session stays locked with no lock client (confidence: low)
- [SKB-15](implicit-state-machines/sukumo-backend.md#skb-15) xdg_popup lifecycle and grab serial: a menu opened from a sheet (or a lock surface) fails in `create_popup` (`desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1373`); the main thread is never told (no `CloseRequested` or `PopupDone`), so it keeps a popup window that never configures, never shows and never closes on its own (confidence: medium)
- [SKB-17](implicit-state-machines/sukumo-backend.md#skb-17) Pointer passthrough and input-region application: `apply_input` updates the content and decorations but not an existing menu bar, so a toplevel with a menu bar that becomes passthrough still takes clicks on the bar, and a bar made while passthrough keeps an empty region after passthrough ends (confidence: medium)
- [SKB-18](implicit-state-machines/sukumo-backend.md#skb-18) (test harness) Null render thread as a fake compositor: a `Request` for a window the null thread doesn't have takes `continue` (`null.rs:449`), skipping `HANDLED` (`null.rs:570`); from then on `testing::render_idle()` (`desktop/aizome/sukumo/crates/sidestep-ui/src/testing.rs:75`) is false for the rest of the process and tests polling it spin to their timeout (confidence: high)
- [SKB-23](implicit-state-machines/sukumo-backend.md#skb-23) Touchpad scroll gesture and axis frame: a two-finger scroll whose window closes (or that a popup grab interrupts) never gets `axis_stop` on our surface: the old window never hears Ended, and the next touchpad scroll in any window starts with Changed and no Began (confidence: low)
- [SKB-38](implicit-state-machines/sukumo-backend.md#skb-38) Settings portal D-Bus session: at login, when D-Bus activation of xdg-desktop-portal takes over the 2 s timeout, ReadOne and Read for `color-scheme` both time out; the scheme stays the light default until the user changes it, so apps started then show light on a dark desktop (confidence: low)
- [SKR-17](implicit-state-machines/sukumo-render.md#skr-17) (test harness) Headless sway runner lifecycle: under `set -eu` (`headless-wayland:15`) a failing `grim` (`headless-wayland:53`, `:57`; missing binary, compositor gone) aborts the script before `kill $app_pid` and `kill $sway_pid`, leaving the app and sway running in the container (confidence: medium)
- [STB-01](implicit-state-machines/settei-b.md#stb-01) Pane navigation, history and the shown/hidden pane lifecycle: View ▸ Back (⌘[) is enabled only when `!back.is_empty()` (`app.rs:621`), but the title strip's Back button also counts a sub-page crumb (`app.rs:372`). Opened with `--pane general.about` and no history, ⌘[ does nothing while the on-screen Back works (confidence: medium)
- [STB-02](implicit-state-machines/settei-b.md#stb-02) Sheet window ownership, split between the window and the panes: each `DisplayPasskey` update (one per key typed on the remote device) arrives as `Prompted` and calls `open_sheet` again (`bluetooth.rs:607`), and `OpenSheet` closes and reopens the sheet window every time (`app.rs:582-586`), so the sheet flickers and loses focus (confidence: medium)
- [STB-04](implicit-state-machines/settei-b.md#stb-04) Single-instance socket: claim, hand-off, listen, release: two launches at once (a double-click on the launcher): both fail to connect, A binds, B's `remove_file` (`instance.rs:29`) unlinks A's socket and B binds. Two windows run, A is unreachable, and whichever quits first unlinks the other's socket, so the next launch starts a third (confidence: medium)
- [STB-05](implicit-state-machines/settei-b.md#stb-05) Remembered theme and size before High Contrast and Larger text (persisted): start at Small, turn Larger text on (remembers "small"), choose Medium in Appearance, then Large, then turn Larger text off: it restores Small, not Medium, because `Msg::Size` never removes `size_before_larger` (confidence: medium)
- [STB-08](implicit-state-machines/settei-b.md#stb-08) Keep Awake switch: optimistic value, helper process, marker file: if Keep Awake is turned off from the menu bar's cup while Battery & Power shows, the pane's switch stays on until the pane is shown again, because `awake` is read only in `shown` (`battery.rs:337`) (confidence: medium)
- [STB-12](implicit-state-machines/settei-b.md#stb-12) Arrangement drag gesture: pressing an arrow key while holding a tile down sends `Moved` (`arrangement.rs:426-427`); `commit` sets `applying` (`displays/mod.rs:263`) and the view disables the widget (`displays/view.rs:118`); the `PointerUp` that follows is ignored, leaving the pointer captured and the tile drawn as a ghost until the next press (confidence: low)
- [NTU-15](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-15) Nisshi `--print` one-shot read: `aizome-nisshi --print --system` with the password dialog left unanswered for two minutes prints nothing and exits 0 (`desktop/aizome/apps/nisshi/src/main.rs:27`, `desktop/aizome/apps/nisshi/src/main.rs:64`), indistinguishable from an empty journal. (confidence: medium)
- [NTU-20](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-20) Tokei sheets: Add City and the alarm editor: delete alarm D, open Edit on X, choose Edit ▸ Undo Delete (still in the menu bar) → D is reinserted before X; Save then writes X's draft over the alarm now at X's old index, dropping it from the file while its timer stays armed (`desktop/aizome/apps/tokei/src/app.rs:549-552`, `desktop/aizome/apps/tokei/src/app.rs:625-628`). (confidence: low — needs the menu bar live while the sheet is up)
- [NTU-24](implicit-state-machines/nisshi-tokei-utsushi.md#ntu-24) Tokei alarm ring process (`aizome-tokei --ring`): with Tokei open, a once-alarm starts ringing; the user adds alarm C (file now has C, C's timer armed), then presses Stop → `ring_with` saves its snapshot without C (`desktop/aizome/apps/tokei/src/ring.rs:53`, `desktop/aizome/apps/tokei/src/ring.rs:95`); at C's time `--ring C` logs "alarm C is gone" and stays silent. (confidence: medium)
- [TSH-15](implicit-state-machines/tansu-shiori.md#tsh-15) Single-instance socket hand-off (Shiori and Tansu): two launches at the same moment both fail to connect, then both `remove_file` and `bind` (`desktop/aizome/apps/shiori/src/instance.rs:40-41`). The second unlinks the first's socket, so the first instance keeps running but can't be reached, later launches go to the second, and the two instances save `places.json` over each other (two Tansu windows could each queue and apply plans). (confidence: medium)
- [SMT-10](implicit-state-machines/sumi-myaku-tenki.md#smt-10) Power metering across readings and sleep: On a laptop with a discrete card, on battery, a momentary card-meter reading above the battery's reported draw calls `floor.update(0)`, after which no idle floor is ever split by capacity share and every later estimate charges processes the whole draw by busy share (confidence: low)
- [HTT-05](implicit-state-machines/hotaru-tests-tools.md#htt-05) (dev tool) Shared workstation measurement lock: `paired-feed-benchmark.py` times two builds (`apps/hotaru/tools/paired-feed-benchmark.py:66-71`) without taking the lock, so a `build-candidate.py` compile can run concurrently and skew the ratio; load averages are recorded but not used as a gate (confidence: medium)
- [HTT-07](implicit-state-machines/hotaru-tests-tools.md#htt-07) (dev tool) Build provenance metadata gate (`build_confirmed`, hashes): `benchmark.py` validates candidate metadata against the binary (`apps/hotaru/tools/benchmark.py:63`) before taking the lock (`apps/hotaru/tools/benchmark.py:74-75`), then re-hashes inside the lock (`apps/hotaru/tools/benchmark.py:76`) and later compares only against that in-lock hash (`apps/hotaru/tools/benchmark.py:173-178`). If `build-candidate.py` holds the lock while benchmark.py waits and rebuilds `hotaru-replay`, the new binary is measured but the report records the stale metadata's source hash and toolchain (`apps/hotaru/tools/benchmark.py:104-108`), and the end check still passes (confidence: medium)
- [HTT-18](implicit-state-machines/hotaru-tests-tools.md#htt-18) (dev tool) Frame observer marker geometry cache: if the first capture after `reset` still shows pre-resize content (the grid grew, so the old bottom marker is now an interior row), discovery locks `bottom_y` to that row; sequence 1's marker on the new last row never decodes, and `observations.until(... sequence == 1)` (`frame-benchmark.py:538`) times out (confidence: low)
- [ICI-06](implicit-state-machines/infra-ci-tests.md#ici-06) Night/Day theme switch remembered in localStorage: where `setItem` throws (quota or private mode) or storage is blocked, switching to Day before the `load` event is undone by the initial `pageshow`, which fires after `load` and calls `show(remembered())` → Night (confidence: low)
- [ICI-20](implicit-state-machines/infra-ci-tests.md#ici-20) (test harness) Responsiveness measurement: probes in systemd units, stop-file protocol: stopping a measurement early (`systemctl --user stop sekka-responsiveness`) leaves the `sekka-resp-*` probe units running indefinitely. Each frame probe burns about 24% of a core (2 ms of work per 8.3 ms frame), and fio runs for up to an hour. The next `run.sh measure` then fails at `start_probes`, because `systemd-run --unit=sekka-resp-wake` finds the unit still present (confidence: medium)

## Keeping this current

- When a machine becomes explicit, set its `Follow-up` in the index to
  `reified (#PR)` and update its entry. Keep the ID, even if the machine
  disappears, so references from issues and pull requests stay valid.
- When a change adds a file in scope, add it to the ledger as `pending`, then
  audit it. Number new machines after the slice's last ID.
- When a change substantially rewrites a machine, re-read it and update the
  entry's locations; line numbers here refer to `8e0acbe5` until then.
- When a reproduced bug is fixed, adopt its test from
  `implicit-state-machines/reproductions/reproductions.patch` as the fix's
  regression test, and cite the fix in the entry and in Reproductions.
- A suspected bug that proves not to be a bug keeps its entry text, with its
  reproduction verdict saying why.
