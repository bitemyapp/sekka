# Reproductions of suspected bugs

The [catalog](../implicit-state-machines.md) records suspected bugs as
readings of the source. This page records the attempt to reproduce every
suspected bug on a machine rated **Reify: High**: 176 of the catalog's 208.
The other 32, on Medium and Low machines, were not attempted and remain
unverified leads.

Each reproduction ran against `8e0acbe5`, the commit the catalog audited.
Where a bug reproduced, the test asserts the **correct** behaviour, so it
fails at `8e0acbe5` and passes once the bug is fixed. A fix can then adopt it
as its regression test.

## Verdicts and levels

- **Confirmed**: the test fails at `8e0acbe5` the way the catalog entry says.
- **Partly confirmed**: part of the claim reproduces; the entry says which.
- **Not confirmed**: the code behaves correctly; the entry says why the
  reading was wrong.
- **Not reproduced (environment)**: the mechanism needs hardware, an
  operating system, or a display backend that was not available. The entry
  says what would be needed.

The level says how much of the real program a reproduction drives:

- `unit`: a function or state object, called directly;
- `integration`: the real crate with fakes for its services, for example a
  headless app through Aizome's kit `Harness`, a private `dbus-daemon`, or
  stub executables;
- `end-to-end`: the real program, headless or nested, such as Aizome's
  compositor through `aizome-comp`'s test harness.

Some tests set state directly that the real program reaches only by timing
(an in-flight counter, a late reply delivered by hand). Their **Notes** say
so, and the independent review below rates each sampled test's
reachability.

## How the reproductions were made and checked

Claude Code agents wrote and ran the tests, one agent per group of related
components. Every command ran through [`reproductions/isolate`](reproductions/isolate):

- an empty environment with a pinned dev shell of the root flake;
- private home, configuration and runtime directories;
- no display and no session or system bus;
- no setuid wrappers.

Nothing reached the live desktop session, a real disk or block device,
a privileged helper, logind, NetworkManager, BlueZ, polkitd or the network.
Disk tools were reproduced through their fake backends and plans, the
privileged Yukimi helper inside an unprivileged user and mount namespace with
stub Nix tools, and CI and publication scripts against stub executables.

A second pair of agents then reviewed a stratified sample of 65 confirmed
reproductions adversarially. For each one they re-ran the test, checked that
the failing assertion tests the catalog's claim, and judged whether the
failure is reachable through real inputs. Every test the first agents had
flagged as setting state directly was included in the sample. The review's
verdicts are summarised below and attached to each sampled entry.

## Re-running a reproduction

The tests are kept out of the tree, so CI is unaffected. Apply them to a
checkout of `8e0acbe5`:

```sh
git switch --detach 8e0acbe5
git apply docs/implicit-state-machines/reproductions/reproductions.patch
docs/implicit-state-machines/reproductions/isolate aizome /tmp/repro \
  cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-koyomi --test repro_koy_18
```

The patch adds one file per reproduction, named after its ID
(`repro_koy_18.rs`). It also adds a one-line `#[cfg(test)] mod repro_…;` to
the 24 modules whose private state a test needs; it changes no other code.
Each entry below names its files and gives its command; run the command
through `isolate` with the dev shell it names. Tests that need a session bus
start their own.

## Summary

| Verdict | Bugs |
| --- | ---: |
| Confirmed | 160 |
| Partly confirmed | 9 |
| Not confirmed | 2 |
| Not reproduced (environment) | 5 |
| **Total** | **176** |

| Level | Bugs |
| --- | ---: |
| unit | 26 |
| integration | 126 |
| end-to-end | 21 |
| no reproduction | 3 |

| Slice | Confirmed | Partly | Not confirmed | Environment |
| --- | ---: | ---: | ---: | ---: |
| [CPA](comp-a.md) | 4 | 0 | 0 | 2 |
| [CPB](comp-b.md) | 6 | 1 | 0 | 1 |
| [WM](wm-comp-tests.md) | 4 | 0 | 0 | 0 |
| [SHA](shell-a.md) | 5 | 0 | 0 | 0 |
| [SHB](shell-b.md) | 7 | 0 | 0 | 0 |
| [SVC](kit-services.md) | 3 | 0 | 0 | 0 |
| [AZN](aizome-small-nix.md) | 3 | 0 | 0 | 0 |
| [SYA](sys-a.md) | 3 | 1 | 0 | 0 |
| [SYB](sys-b.md) | 4 | 0 | 0 | 0 |
| [SYC](sys-c-config.md) | 4 | 1 | 0 | 0 |
| [KIT](kit-core.md) | 5 | 0 | 0 | 0 |
| [UIW](sukumo-ui-kit-widgets.md) | 7 | 0 | 0 | 0 |
| [SKB](sukumo-backend.md) | 1 | 0 | 1 | 0 |
| [SKR](sukumo-render.md) | 2 | 0 | 0 | 0 |
| [STA](settei-a.md) | 5 | 0 | 1 | 0 |
| [STB](settei-b.md) | 2 | 1 | 0 | 0 |
| [HAKO](hako.md) | 11 | 0 | 0 | 0 |
| [HOT](hotaru-app.md) | 6 | 0 | 0 | 0 |
| [KOY](koyomi.md) | 10 | 0 | 0 | 0 |
| [YUK](yukimi.md) | 7 | 0 | 0 | 0 |
| [NTU](nisshi-tokei-utsushi.md) | 5 | 0 | 0 | 0 |
| [KKO](kura-koe.md) | 2 | 1 | 0 | 0 |
| [TSH](tansu-shiori.md) | 5 | 0 | 0 | 0 |
| [SMT](sumi-myaku-tenki.md) | 8 | 0 | 0 | 0 |
| [SAP](small-apps.md) | 9 | 0 | 0 | 0 |
| [HCO](hotaru-core.md) | 4 | 0 | 0 | 0 |
| [HTT](hotaru-tests-tools.md) | 2 | 0 | 0 | 0 |
| [GEN](genkan.md) | 4 | 1 | 0 | 0 |
| [TTL](tatami-tool.md) | 6 | 0 | 0 | 0 |
| [TAP](tatami-apps.md) | 8 | 0 | 0 | 0 |
| [KTL](kakehashi-tools.md) | 4 | 1 | 0 | 2 |
| [INX](infra-nix.md) | 2 | 2 | 0 | 0 |
| [ICI](infra-ci-tests.md) | 2 | 0 | 0 | 0 |

## Independent review

The review sampled 65 of the reproductions first marked Confirmed: every
third one in each group, plus every test whose author said it set state
directly. All 65 were re-run, and every one failed with the message its
report quotes.

| Review verdict | Sampled |
| --- | ---: |
| Upheld | 40 |
| Upheld with caveat | 23 |
| Overturned | 2 |

| Reachability | Sampled |
| --- | ---: |
| Real: driven only by inputs a person, client, service or timing can produce | 36 |
| Staged: state or timing forced, but the reviewer traced the real program reaching it | 27 |
| Unreachable as described | 2 |

The two overturned reproductions are recorded here with the review's
verdict instead of the reproducer's:

- **[STA-05](#sta-05)**, now **Not confirmed**. The test replaced localed with
  a fake that never wakes the window. The real service wakes it when a request
  finishes, even after failure, and that wake reloads the Formats preview. The
  spinner lasts only while the polkit prompt is up.
- **[SYA-17](#sya-17)**, now **Partly confirmed**. The level meter's own
  defect is real: it keeps the last peak and never reconnects. But Settei's
  80 ms poll sees the default input disappear during pipewire-pulse's 2 s
  reconnect pause, drops the meter and builds a new one, so no frozen level
  is shown.

Patterns the reviewers noted:

- **Two-layer claims are the weak spot.** A test that asserts the lower
  layer's contract while the reproducer only read the upper layer can
  overstate the effect, as SYA-17 did. A fake must reproduce the live
  service's side effects (wakes, signals, retries), not only its return
  value, as STA-05 shows.
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
  KOY-04, SMT-19, SHA-02. Their notes say so.
- **One bug, two IDs:** SHB-03 and SYB-09 reproduce the same retry loop from
  the shell side and from aizome-sys.
- **Brittle as regression tests:** SYA-06 and KOY-01 assert one fix location;
  a valid fix elsewhere would leave them failing.

97 confirmations were not reviewed. At the sample's rate, a few of them may
also overstate what a user would see.

## Results

| ID | Machine | Verdict | Level | Review | Claim |
| --- | --- | --- | --- | --- | --- |
| [CPA-04](#cpa-04) | libseat session pause and resume (VT switching) | Not reproduced (environment) | — |  | a monitor plugged in while the session is paused (user on another VT) stays dark after they return: `device_changed` runs while paused and reserves the connector, `initialize_output` fails on the inactive device, `ActivateSession` never rescans, and later `Changed` events skip the connector as already known. |
| [CPA-05](#cpa-05) | DRM device and connector hotplug, and output enablement | Not reproduced (environment) | — |  | (1) any setup failure leaves the connector reserved without a surface, and it never lights until it is replugged; (2) `enable_output` drops the `disabled` entry before its fallible steps, so a failed enable leaves the output listed as off but refused ("isn't off"), or unknown to IPC; (3) `device_removed` misses outputs turned off on a removed GPU, so stale `disabled` entries remain and a replug can add a duplicate. |
| [CPA-16](#cpa-16) | Recovery overlay session (open and closed) | Confirmed | end-to-end | upheld | if the display the recovery overlay opened on leaves the layout (lid closed, unplugged), `recovery.output` still names it, so the overlay shows only the scrim and no panel while it keeps taking input. |
| [CPA-24](#cpa-24) | Process-info worker: request coalescing, CPU baselines and kills | Confirmed | unit |  | when the worker's drain starts with a Kill and finds a Sample queued behind it, `s => req = s` overwrites the Kill: no SIGKILL and no `Killed` report. |
| [CPA-27](#cpa-27) | Supervised helper process lifecycle (shell, xwayland-satellite) | Confirmed | end-to-end |  | (1) a helper whose first spawn fails is never retried (`restart_at` stays None); (2) a `restart_shell` whose new binary fails to exec has already moved the running shell to `retiring` without a handover, so that shell is unsupervised, and later restarts leave it running beside the next one, never stopped. |
| [CPA-32](#cpa-32) | Main-loop stall watcher, systemd watchdog and the stall marker | Confirmed | unit | upheld with caveat | the stall watcher's signal handler allocates (`Backtrace::force_capture`). If the stalled thread holds the allocator's lock, the handler deadlocks on it, turning a recoverable stall into a permanent one. |
| [CPB-07](#cpb-07) | Idle-inhibitor registry | Confirmed | end-to-end | upheld | idle inhibitors are kept one per surface, but smithay calls `inhibit`/`uninhibit` once per inhibitor object, so when a surface has two inhibitors and one is destroyed, inhibition ends while the other is still alive. |
| [CPB-08](#cpb-08) | Pointer constraint activation (lock and confine) | Confirmed | integration |  | (1) `Confined` constraints are activated but never enforced, so a confined pointer leaves the window freely. (2) Motion while the pointer is locked returns before `note_activity`, so mouse-only use of a pointer-locked app counts as no input for the idle timers and ext-idle-notify. |
| [CPB-18](#cpb-18) | Popup tracking and popup-layer stacking (xdg and input-method popups) | Confirmed | end-to-end |  | input-method popup surfaces (`zwp_input_popup_surface_v2`) are tracked by smithay's PopupManager but never pushed to `popup_order`, so `placed_popups` skips them and they are never drawn. |
| [CPB-20](#cpb-20) | Foreign-toplevel handle synchronisation (wlr management and ext list) | Confirmed | end-to-end | upheld | `refresh` compares `Snapshot.parent`, but the wlr foreign-toplevel `parent` event is sent only at announce, so a dialog parented after it maps is never re-parented in taskbars. |
| [CPB-21](#cpb-21) | Gamma-control ownership per output | Confirmed | unit |  | the per-output `gamma_client` flag is not tied to a control, and a failed control keeps `Some(output)`. So when A's failed control is destroyed after B has taken the output, `release` clears B's hold and Night Light takes the ramps back. |
| [CPB-22](#cpb-22) | ext-image-copy-capture session and frame lifecycle | Partly confirmed | end-to-end |  | a frame copied with a fence is replaced by `CreateFrame` before the fence signals. Its copy has already advanced the stream's damage, `ready` drops it, and the new frame finds `Unchanged`, so an idle screen stalls the resumed portal stream. |
| [CPB-24](#cpb-24) | wlr-output-management configuration lifecycle | Not reproduced (environment) | end-to-end |  | when a two-head apply fails on the second head (for example udev's "has no mode WxH"), the first head's change stays applied while the client is told `failed`. |
| [CPB-25](#cpb-25) | wlr-screencopy frame lifecycle and stream pool | Confirmed | end-to-end |  | (1) a wlr-screencopy stream is reused after a scale change (same mode, same region) with its old tracker scale, so surfaces are drawn at the wrong size. (2) A client that destroys its manager right after requesting a frame leaves a full-output stream behind for as long as the output exists. |
| [WM-04](#wm-04) | Interactive resize and its edge anchor | Confirmed | end-to-end |  | the left-edge resize anchor outlives its gesture. After the window is moved, a commit with a size different from the model's puts the left edge at `anchor.start.right() - width`, so the window jumps back. |
| [WM-10](#wm-10) | Sticky keys (latched modifiers) | Confirmed | end-to-end |  | with sticky keys, if the logic is replaced (a reload) or sticky keys are turned off while a key carrying latched modifiers is down, `Access.applied` is dropped and the synthetic modifier presses are never released, so later keys arrive with those modifiers. |
| [WM-12](#wm-12) | Area picker (screenshots and IPC `pick-area`) | Confirmed | end-to-end | upheld | a pick requested over IPC (`pick-area`) that is closed by `quiesce`/`close_all` (a logic reload, or Reset Input) ends without `AreaPicked{area: None}`, so the requester waits forever. |
| [WM-15](#wm-15) | Desks overview | Confirmed | end-to-end |  | when the display showing the Desks overview leaves the layout, nothing closes or moves the overview: nothing is drawn, yet it keeps grabbing the keyboard and swallowing every key. |
| [SHA-02](#sha-02) | Shell-replacement carry handoff (state file, final word, retiring) | Confirmed | integration | upheld with caveat | when a daemon that isn't a replacement shell (dunst or mako started by hand) takes `org.freedesktop.Notifications`, the shell gets `NameLost`, writes a final state and sets `retiring` for good. It keeps running but never saves what it carries again, and it never asks for the name back. |
| [SHA-03](#sha-03) | Deskbar auto-hide visibility | Confirmed | integration |  | if the compositor closes an auto-hide Deskbar while the pointer is over it (its output went), `inside` stays true. The Deskbar reopens on the remaining display with no hide armed, and every `AutoHide` timer is refused, so it stays revealed although the pointer never reached the new surface. |
| [SHA-12](#sha-12) | Palette open/close and its drawn-ahead spare window | Confirmed | integration |  | if the palette is open when its output goes, `close_requested` forgets only the surface, and `Shell::palette` keeps the dead window. The next open (typing in the Toolchest, or `aizomectl palette QUERY`) only re-ranks into that dead window, so no palette shows. |
| [SHA-13](#sha-13) | Palette file search, index lifetime and per-opening extras | Confirmed | unit | upheld with caveat | the palette's file-search `seq` restarts at 0 at every opening. A search from an earlier opening that returns late has the same `seq` as the new opening's first search, so it's accepted, with `files_for` set to the new query. |
| [SHA-25](#sha-25) | Torn-off palettes (persisted list, shown surfaces, strip drag) | Confirmed | integration |  | torn-off palettes are keyed by their index in `tearoffs`. (1) Tearing off again a menu that already has a palette shifts the later entries, while the shown windows keep the old indices: windows show each other's menus, and the re-torn palette isn't moved where it was dropped. (2) When a palette's output goes, `tear_shown[i]` keeps the dead window, so a pinned palette never opens again. |
| [SHB-03](#shb-03) | Polkit authentication dialog | Confirmed | integration | upheld with caveat | with polkit's helper unreachable, `Session::start` emits Error then Failed at once and the shell's Failed handler calls `authenticate` again, so the dialog spins through connect attempts until cancelled. |
| [SHB-06](#shb-06) | Session confirmation countdown (Restart, Shut Down, Log Out, power key) | Confirmed | integration |  | if the Restart/Shut Down/Log Out dialog's output goes during the countdown, `close_requested` only drops the `surfaces` entry, `confirm` stays `Some`, the ticks go on and the verb is carried out with no dialog to cancel it; asking again only resets `left` and never reopens a window. |
| [SHB-09](#shb-09) | Compositor event follower (connect, snapshot, subscribe, reconnect) | Confirmed | end-to-end | upheld with caveat | events between the follower's snapshot requests and `Subscribe` are lost, because the compositor's `Subscribe` only sets `subscribed` and replays nothing, so a window mapped in that gap is missing from the shell's model. |
| [SHB-12](#shb-12) | Lock screen lifecycle (partially explicit) | Confirmed | integration | upheld with caveat | whatever PAM says is replaced by "That password didn't work", so a lockout (faillock) or an expired password makes the right password look wrong on every try. |
| [SHB-14](#shb-14) | Critical notification cards surface | Confirmed | integration |  | when the critical cards' output goes, `close_requested` drops only the `surfaces` entry; `cards` keeps the dead key, `sync_cards` keeps resizing it and never reopens, so critical cards are invisible. |
| [SHB-15](#shb-15) | Bar notification line (banner) | Confirmed | integration |  | replacing the notification on the bar's line (same id) arms a second `BannerDone(id)` while the first is pending; the first ends the line at the original deadline and, for a transient notification, closes the just-updated one. |
| [SHB-20](#shb-20) | Screen recording session (partially explicit) | Confirmed | integration | upheld | `adopt_recording` never arms the indicator tick (only `Started` does), so after a shell restart mid-recording the systray clock advances only when something else repaints the bar. |
| [SVC-05](#svc-05) | Greeter screen stage (people, password, typed name) | Confirmed | integration |  | with `user_list = false` (or nobody listed), a mistyped user name can't be corrected. A rejected password begins the same name again with no way back, and if `CreateSession` fails, Return does nothing (`begin` needs `user()`). Only restarting the greeter helps. |
| [SVC-23](#svc-23) | Playback session lifecycle | Confirmed | integration |  | (1) stepping forward then pressing Play jumps back to where stepping started, because steps don't move the clock and `play` seeks to `clock.now()`. (2) A speed change on unseekable media with audio drops the tempo filter but skips the rebase seek, so the clock keeps the old rate while the audio plays at the new tempo. |
| [SVC-29](#svc-29) | Video suspension out of sight, and catching up | Confirmed | integration | upheld | a seek while the window is hidden (`set_visible(false)`) stalls playback. `seek_to` waits for a frame of the new generation, but the video thread is still suspended locally, so nothing is presented, `Seeked` never comes and the audio stays silent until the window is shown. |
| [AZN-01](#azn-01) | Keep Awake requested/protected status (setting file against logind's locks) | Confirmed | end-to-end |  | `aizome-awake toggle` evaluates `requested()` before taking the edit lock, so two overlapping toggles both turn Keep Awake on and the second toggle is lost. |
| [AZN-04](#azn-04) | Agent registration with NetworkManager (retry, re-register on restart) | Confirmed | integration |  | two consecutive "already registered" refusals (another agent still holds the identifier) set `done = asked = true`, so the agent stops retrying and is never registered once the other agent disconnects. |
| [AZN-13](#azn-13) | Time-zone ownership across activations (saved zone file and `/etc/localtime` link) | Confirmed | unit | upheld with caveat | with Aizome enabled, a declared `time.timeZone` is overridden at priority 90 and never used as the starting zone. A fresh system installed without Genkan (no `/etc/localtime` link, no saved zone) runs in UTC. |
| [SYA-05](#sya-05) | Optimistic audio levels awaiting server confirmation | Confirmed | integration |  | `set_volume_in` stores a new level before the server answers. When the server refuses, nothing restores the old level and no change event follows, so the snapshot keeps a level the server never applied. |
| [SYA-06](#sya-06) | Bluetooth pairing-agent registration worker | Confirmed | integration | upheld | BlueZ forgets agents when bluetoothd restarts, but the agent worker keeps its `Registration` and nothing registers again. Pairing a device that needs a confirmation then fails with no prompt. |
| [SYA-13](#sya-13) | Destructive-operation confirmation token | Confirmed | unit |  | a `Confirm<NewPartitionTable>` made for USB stick A still passes `check` after A is swapped for a same-size partitioned stick B at the same kernel name. The fingerprint has no drive identity and no partition-table identity, and a whole partitioned disk has empty `IdType`/`IdUUID`, so B would be wiped. |
| [SYA-17](#sya-17) | Level meter thread lifecycle | Partly confirmed | integration | overturned | every error exit of the level meter's thread leaves `level` at the last peak. The thread never reconnects, and the owner can't tell it's dead, so when pipewire-pulse restarts, Settei's Sound pane shows a frozen input level and the meter never follows the input again. |
| [SYB-01](#syb-01) | Airplane mode across rfkill, NetworkManager, BlueZ and a marker file | Confirmed | integration |  | after airplane mode's fallback, turning Wi-Fi back on with its switch leaves the marker behind. A later airplane-on rewrites it as "" (hci0 is still off), so airplane-off never powers Bluetooth back on, and the stale marker makes a plain Wi-Fi-off read as airplane mode. |
| [SYB-02](#syb-02) | Wi-Fi join / activation transaction with in-memory edit and rollback | Confirmed | integration |  | joining a saved network with a new password that takes longer than 45 s: `wait_active` returns Timeout and `rejoin_with` reloads the old profile from disk, but NM's activation with the in-memory settings is left running, so it may connect while the UI says it failed and the working password is never saved. |
| [SYB-09](#syb-09) | polkit authentication session and helper conversation | Confirmed | integration | upheld with caveat | when the helper can't be reached (connect error) or exits without SUCCESS/FAILURE (EOF), Failed is emitted at once. The shell's Failed handler calls `authenticate` again immediately, so the dialog spins, spawning helpers or logging warnings as fast as the loop runs. |
| [SYB-12](#syb-12) | logind sleep-delay inhibitor and lock-before-sleep handshake | Confirmed | integration |  | if the lock isn't confirmed before logind's delay limit and the machine sleeps with `sleep_pending` set, the delay is retaken on resume. `lock_confirmed` then calls `sleep_ready()` and drops that fresh inhibitor, so the next suspend isn't delayed and can sleep before the screen locks. |
| [SYC-01](#syc-01) | Keyboard settings: inherited session defaults vs saved choices | Confirmed | integration | upheld | an unrelated save (the shell saving its Deskbar section) writes every `Keyboard` field, pinning the session's `XKB_DEFAULT_*` layout, so a later system layout change never reaches the session. |
| [SYC-02](#syc-02) | One-shot migration of the legacy `[keys] terminal` key | Confirmed | integration |  | any save drops the `skip_serializing` legacy `[keys] terminal` key before the migration has succeeded, so the retried migration reads `None` and ❖Return starts Hotaru instead of the person's terminal. |
| [SYC-03](#syc-03) | Settings file lifecycle: load, fallback to defaults, atomic save, cross-process writers | Confirmed | integration |  | (1) `load_or_default` turns a broken file into the defaults, and the next shell save writes them over everything. (2) Writers sharing the fixed `settings.toml.tmp` truncate each other's temp file: a rename publishes torn text and the other rename fails with ENOENT. |
| [SYC-06](#syc-06) | Drawn element press, drag, drag-out and hover gesture | Partly confirmed | integration |  | when the compositor takes the pointer mid-drag, Drawn resets its press silently at `drawn.rs:376` with no `DragPhase::End`, so `desktop.band` stays `Some` (the band stays drawn) and the Deskbar's app drag holds it open. |
| [SYC-07](#syc-07) | Text field caret, selection, IME preedit, blink and Caps Lock state | Confirmed | integration | upheld | in a `secure` field the IME preedit is drawn unmasked (`all = shown(value) + preedit`), so characters being composed in the lock-screen, polkit and greeter password fields show in clear text. |
| [KIT-04](#kit-04) | Menu publication to the shell (dedup and debounce) | Confirmed | integration | upheld with caveat | a shell `Activate{id}` for a menu item the app has disabled since its last publication (within the 40 ms debounce) runs the item anyway, because `shell::find` returns disabled actions and `Core::perform` never checks `enabled`. |
| [KIT-07](#kit-07) | File panel lifecycle and browsing state | Confirmed | integration |  | `Cx::close_window` on a window whose save panel is up leaves the `PanelState` in `Core.panels` for good, and the panel's `done` never fires, so the program never hears `Saved(None)`. |
| [KIT-13](#kit-13) | Pointer hover, press and capture | Confirmed | integration |  | after a program-started drag (`Cx::start_drag` from the source's press-and-move messages, as Hako does), `Win.active` still names the drag source. Later moves go to it, and the next click's release reaches it instead of the clicked control. |
| [KIT-17](#kit-17) | Tooltip delay, show and dismissal | Confirmed | integration | upheld | (1) hovering a button and clicking it within 650 ms still shows its tooltip after the click; (2) each pointer leave with a tooltip pending leaks one `TimerUse::Tooltip` in `Core.timers`. |
| [KIT-29](#kit-29) | Node arena: liveness, layout cache and per-layout outboxes | Confirmed | integration |  | a relayout that compacts the arena (`engine.rs:907-912`) leaves `local_damage`/`scrolls` holding pre-compaction indices of nodes mounted during that layout. `take_local_damage` then indexes past the end and panics (or lands damage on another node). |
| [UIW-01](#uiw-01) | Press-then-release click pairing in buttons, switches, checkboxes, radios and segmented controls | Confirmed | integration |  | a press on inert background followed by a release over a button, switch, checkbox or segmented control fires it, though it was never pressed. |
| [UIW-08](#uiw-08) | Menu interaction: hover, open submenu, pointer aim, type-to-filter, strip tear-off | Confirmed | integration | upheld with caveat | (1) a menu drawn in a window (torn-off palette, `menu_view`) whose items shrink under a hovered row past the new end panics on the next pointer event; (2) after ← closes a submenu, the parent's `open_sub` still names it. Hovering that row doesn't reopen it, and rightward moves still count as aiming. |
| [UIW-09](#uiw-09) | In-window menu bar's open title | Confirmed | integration |  | clicking the in-window menu bar's open title reopens its menu instead of closing it, because the engine closes the window's menus before routing the press, which deletes `BarState`. |
| [UIW-11](#uiw-11) | Scroll-bar visibility and thumb drag | Confirmed | integration |  | with more than one scroll event (anything over ~20 ms), only the first arms the bar tick. It fires while `shown_until` is still ahead, nothing re-arms, and the scroll bar stays drawn until something else repaints. |
| [UIW-15](#uiw-15) | Selection cursor and anchor in lists, tables and outlines | Confirmed | integration | upheld | the kit's cursor isn't clamped or remapped when the program's rows change. (1) In an outline, collapsing the branch holding the cursor then pressing Return panics. (2) In a table re-sorted by the program (selection remapped by name), Return activates the old cursor index and ↓ moves from the old row. |
| [UIW-18](#uiw-18) | Editor viewport, position-bar linger and caret blink on one tick | Confirmed | integration |  | (1) focusing an editor while a scroll tick is pending drops the focus tick request. The pending tick comes before `next_blink`, isn't due, and nothing re-arms, so the caret stops blinking. (2) An unfocused editor scrolled by several events keeps its position bar drawn. |
| [UIW-27](#uiw-27) | Session lock | Confirmed | integration | upheld with caveat | a `SessionLockFinished` for a lock the program has already withdrawn turns the next lock's `Asked` into `Unlocked` and closes its surfaces. The new lock's `SessionLocked` is then ignored, so the session is locked with no lock surface while the program believes it is unlocked. |
| [SKB-11](#skb-11) | Window configure, size and geometry lifecycle | Confirmed | end-to-end | upheld with caveat | after a size-changing configure, a hover change over Sukumo's own header before the main thread presents makes `refresh_decor` commit the new window geometry and frame around the old-size content, so one frame shows an undrawn band. |
| [SKB-27](#skb-27) | Selection and primary-selection ownership | Not confirmed | — |  | `cleared_by_us` is set by a clear the compositor never echoes, so a later clear (a client that copied and then quit) is swallowed as ours, and paste returns the dead client's read-ahead text. |
| [SKR-03](#skr-03) | Retained paints and per-host frame scheduling (dirty / moving / last snapshot) | Confirmed | unit |  | `Compositor::frame` clips every damage rectangle pushed for a target to the current host's clip, so with two animating hosts in one window surface (disjoint clips), the first host visited is never redrawn while its `last` snapshot is advanced. |
| [SKR-04](#skr-04) | Layer contents, canvas generations and their image-cache keys | Confirmed | unit |  | when a commit replaces a layer's canvas (a resize, or contents set to none or an image), the old canvas is dropped without `images::forget`, so the render thread's image cache keeps its pixels until LRU eviction. |
| [STA-05](#sta-05) | Language & Region formats preview with a stale-locale guard | Not confirmed | integration | **overturned** | if localed refuses a Region formats choice (polkit dismissed), `Region` has already cleared `preview` and `Done(Err)` doesn't call `refresh_preview`, so the Formats section stays an indeterminate spinner. |
| [STA-10](#sta-10) | Keyboard setup: catching the key beside the space bar | Confirmed | integration |  | while the keyboard setup sheet waits for a key, opening Settei at another pane runs `hidden` → `stop_waiting`. The sheet stays up without "Waiting for the key…", the key beside the space bar does nothing, and nothing restarts the wait. |
| [STA-12](#sta-12) | Pane sheets against the window's single sheet slot (keyboard's three, network, printers, users) | Confirmed | integration |  | with Add a Layout's picker open, the shell's `--pane keyboard.setup --keyboard …` launch starts a key catch while `sheet()` still shows the picker. The next key is taken as the setup key, and after the picker closes "Add a Command…" shows the leftover setup sheet. |
| [STA-15](#sta-15) | Connection details page: navigation, profile reads and the edit buffer | Confirmed | integration | upheld with caveat | a change to connection A that comes back after B's details page opened re-reads A, and `loaded` swaps the edit buffer to A's address, router and DNS. B's page then shows A's values, and Apply on B writes A's static address to B. |
| [STA-19](#sta-19) | Add-a-printer sheet: discovery, driver lookup, adding | Confirmed | integration |  | choose one plugged-in printer that needs a driver, then another before CUPS's driver list comes back: the first device's `Drivers` reply fills `drivers` for the second, `ready` turns true, and Add installs the second printer with the first one's PPD. |
| [STA-22](#sta-22) | New account: its password applied once AccountsService lists it | Confirmed | integration |  | a new account's password is applied only in Users & Groups' `system_changed`, which runs only while that pane is current. Add a user with a password and switch panes or quit at once, and the account doesn't get the password. |
| [STB-09](#stb-09) | Bluetooth pane activity: pairing agent, discovery and discoverability while shown | Confirmed | integration | upheld with caveat | quitting Settei while Bluetooth shows never calls `hidden`, so the adapter stays discoverable after Settei is gone. The next run, finding it already discoverable, never sets `made_discoverable` and so never undoes it. |
| [STB-13](#stb-13) | Display change: apply, "Keep these display settings?" countdown, keep or revert | Partly confirmed | integration |  | (a) closing the window or quitting (⌘Q) during the 15 s "Keep these display settings?" countdown exits without `revert`, leaving the risky change applied. (b) A modeset slower than the 10 s read timeout returns `Err`, so no countdown starts and the change stays live, unconfirmed and unpersisted. |
| [STB-17](#stb-17) | settings.toml write pipeline: ordered writer, pending count, reload debounce | Confirmed | integration |  | while Settei's own writes are pending (a slider dragged), `reread` drops the watcher's one notice of another program's write, and the next `change` builds on the cached settings, so the shell's change (e.g. the Deskbar width) is overwritten. |
| [HAKO-05](#hako-05) | Transfer placement with conflict prompt and replace-aside protocol | Confirmed | integration | upheld | Put Back (and Undo of Move to Trash) run `restore` with `trash: None`, so answering Replace at a conflict permanently deletes the newer item instead of moving it to the Trash as the sheet says. |
| [HAKO-06](#hako-06) | Copy-to-partial commit and cross-filesystem move (copy, fsync, delete arrived sources) | Confirmed | integration |  | in a cross-filesystem move, when a nested folder's `sync_dir` fails, the report says it "couldn't be written safely", yet its files (already in `arrived.leaves`) are deleted from the source. |
| [HAKO-08](#hako-08) | Trash can item protocol (trashinfo claim, rename, directorysizes, forget) | Confirmed | integration |  | Put Back of a folder merged into an existing folder, skipping a conflicting child, still calls `trash.forget(entry)`, so what's left in `files/<name>/` loses its `.trashinfo` and lists as an orphan of unknown origin. |
| [HAKO-09](#hako-09) | Undo/redo history with in-flight inversion jobs | Confirmed | integration | upheld | an undo that fails yields an empty inverse that `History::undone` discards, so the step leaves both stacks and the next ⌘Z silently undoes the step before it. |
| [HAKO-11](#hako-11) | inotify folder watcher and its watch-descriptor map | Confirmed | unit |  | a shown folder deleted and recreated under the same path keeps its dead watch descriptor in the map, `set` skips it by path, and changes inside the new folder are never heard. |
| [HAKO-16](#hako-16) | Volume mount, unlock and two-phase eject tasks | Confirmed | integration |  | ejecting a volume a window shows sends that window home with `Browser::go` directly, skipping `start_loads`, `update_watch` and the title, so the Home column stays `Load::Wanted` (blank) until the person navigates. |
| [HAKO-17](#hako-17) | Thumbnail request pipeline (queue, asked set, running count) | Confirmed | integration | upheld | pictures queued for thumbnails and pruned from `thumb_wanted` when the person leaves stay in `thumb_asked`, so on coming back `want_thumbs` skips them and they show generic icons for the session. |
| [HAKO-19](#hako-19) | Column listing load, reload debounce and refresh reconciliation | Confirmed | integration | upheld | a change heard while its column is `Loading` is dropped: `reload_dir` skips `Loading` columns and nothing re-wants them when the stale read lands, so the new file stays missing until the next change. |
| [HAKO-20](#hako-20) | Rename session (inline field, new-folder auto-rename) | Confirmed | integration | upheld | `rename_when_loaded` stays armed when New Folder runs while its column is mid-load, and much later selecting that folder unexpectedly opens the rename field. |
| [HAKO-25](#hako-25) | Drop-target spot registry | Confirmed | integration | upheld | in List/Icons view after opening a subfolder, stale spot 0 (the parent's listing) resolves drags first, so a drop moves files into whatever parent folder was at that row. In search, drops off a folder row fall through to stale column spots. |
| [HAKO-28](#hako-28) | Job lifecycle with blocking questions | Confirmed | integration |  | if a job's window closes before it asks, the question opens over another window but `job.window` stays the dead key. When that host window closes, `reparent_questions` (matching only `j.window == gone`) doesn't re-ask, and the job hangs "Waiting for an answer". |
| [HOT-07](#hot-07) | Automatic-link activation pipeline (match, hover, resolve, open) | Confirmed | integration | upheld | after a ❖-click on an automatic link, any new hover request (the pointer moving one cell) cancels the in-flight path resolution, so the link silently never opens. |
| [HOT-12](#hot-12) | Settings persistence, reload and deferred window close | Confirmed | integration |  | if ⇧❖, (Reload Configuration) comes while a deferred close or "open hotaru.toml" waits for a save in flight, the stale save answer leaves `closing`/`open_after_save` latched, and a later unrelated settings save closes the window or opens the file unexpectedly. |
| [HOT-14](#hot-14) | Appearance ownership between the desktop watcher and Hotaru's theme | Confirmed | integration |  | under a Night override, a desktop switch from Day to Night changes nothing visible (so no `Msg::Theme`), so `desktop_appearance` stays Day, and choosing Follow Desktop then shows Day while the desktop is Night. |
| [HOT-17](#hot-17) | Single-instance claim and launch handoff | Confirmed | integration | upheld with caveat | if the first instance takes over 2 s from `claim` to `serve`, a second launch times out on the acknowledgement and runs standalone, while the first later reads the buffered request and opens a tab too: one launch, two terminals. |
| [HOT-21](#hot-21) | PTY worker lifecycle (spawn, run, EOF, hang-up, exit report, stop) | Confirmed | integration |  | when the shell exits but a background job still holds the PTY slave, there is no EOF, `try_wait` is never polled (`session.rs:787-790`), and the pane neither ends nor shows "Session ended" until the job exits. |
| [HOT-27](#hot-27) | Pointer drag selection and held-button tracking | Confirmed | integration |  | if a drag-select's release is lost (the compositor takes the pointer: leave with the button down), `drag`/`buttons` stay set, so later plain hover keeps extending the selection. |
| [KOY-01](#koy-01) | Koyomi agent service lifecycle (unit, bus connect retry, listener threads, wake loop) | Confirmed | unit | upheld | every notification Open spawns `aizome-koyomi --show DATE` and drops the `Child` without `wait()`, so each exited child stays a zombie under the long-running agent. |
| [KOY-02](#koy-02) | Alert delivery state: due → fired → shown → snoozed/closed | Confirmed | integration |  | an alert is marked fired before Notify succeeds, so an alert whose Notify failed (the shell not yet holding `org.freedesktop.Notifications`) is saved as fired and never posted. After an agent restart, Snooze and Open on notifications still on screen do nothing because `shown` is empty. |
| [KOY-03](#koy-03) | Settings and window-state persistence (unsaved settings, debounced state, agent's stale copy) | Confirmed | integration |  | View ▸ Week Numbers and View ▸ First Day of the Week change `settings` without saving them, so the choices are lost at relaunch. The running agent reads `snooze_minutes` once, so a new "Snooze for" only applies after an agent restart. |
| [KOY-04](#koy-04) | Navigation and selection: view, anchor, cursor, mini month, selected occurrence | Confirmed | integration | upheld | a day clicked in Month (or Week) view moves only `cursor`. ❖1 then shows `anchor`'s day while the agenda and New Event use the clicked day. ⌫ deletes the selected event even after navigating away from it or hiding its calendar. |
| [KOY-05](#koy-05) | Modal sheet slots: editor, import, name, settings | Confirmed | integration |  | a second launch's `--new` while the editor is open, or a second .ics while the import sheet is up, replaces the slot without closing the old sheet. That sheet's key then renders the whole main window as a stacked sheet. |
| [KOY-10](#koy-10) | Calendar save pipeline: dirty → debounce → background write → stamp | Confirmed | integration |  | when a background calendar write fails, `dirty` has already been taken and `Saved` records the unchanged file's stamp. Nothing writes the edit again, so it is lost at quit and discarded by the next external change's reload. |
| [KOY-12](#koy-12) | Recurring series split state: master rule, EXDATE/RDATE, RECURRENCE-ID overrides | Confirmed | integration | upheld | setting Repeat to Never (or changing the frequency) for All Events from another instance clears or changes the master's rule but keeps the RECURRENCE-ID override, which still shows as a separate event. |
| [KOY-14](#koy-14) | Store's view of the calendar folder: stamps, reload, trash | Confirmed | unit |  | an unreadable .ics is counted by `files()` but not loaded, so `changed_on_disk` stays true: the app reloads on every Poll and clears undo. Separately, a calendar whose `create` couldn't write stays sorted into `calendars` without `reindex`, and the Day view's agenda then indexes past an event list and panics. |
| [KOY-17](#koy-17) | Time-grid pointer drag: move, resize, create; edge hover and scroll | Confirmed | integration |  | a time-grid drag holds the picked-up block by its index in the current blocks. Pressing → mid-drag shows the next week, and the drop moves whichever block has that index there. |
| [KOY-18](#koy-18) | All-day strip drag | Confirmed | integration | upheld | an all-day strip drag holds the bar by index. Pressing → into a week with fewer bars panics on the next pointer move; with enough bars, the drop moves a different event. |
| [YUK-02](#yuk-02) | Update receipt and system completion advice | Confirmed | unit | upheld | after a Yukimi activation fails past commit, the receipt stays `activating` for S1, and `system_state` keeps answering Unknown "Activation did not finish successfully" even after the person runs `nixos-rebuild switch` to a different system S2 and restarts into it. |
| [YUK-09](#yuk-09) | Privileged change transaction (back up, edit, update, build, commit, activate) | Confirmed | end-to-end |  | `change --channels` runs `nix-channel --update` before building, but only files are backed up, so after a failed or stopped build root's channels stay advanced and the next unrelated change silently builds on them. |
| [YUK-10](#yuk-10) | Saved per-generation configuration snapshots | Confirmed | end-to-end |  | saved configurations are keyed by generation number. After returning to N, an out-of-band `nix-collect-garbage -d` and a `nixos-rebuild switch` that reuses N+1, Yukimi keeps the stale `generation-<N+1>` copy, so a later return to N+1 restores another build's flake.lock. |
| [YUK-11](#yuk-11) | Rollback and clean-up sequences | Confirmed | end-to-end | upheld | Clean Up "older than a week" deletes generations, then collects garbage. Stop during `nix-store --gc`, or a GC failure, returns before `commit()` and `switch-to-configuration boot`, so the boot menu still lists the deleted generations. |
| [YUK-15](#yuk-15) | Package index lifecycle | Confirmed | integration |  | (1) after a live update moves the system's Nixpkgs, `ensure_index` returns early because an index exists and isn't stale, so Discover keeps the old index, and an in-flight index for the old Nixpkgs is marked current when it lands. (2) After `make` fails, every Search keystroke starts another `nix eval`. |
| [YUK-19](#yuk-19) | Desktops page choice drafts | Confirmed | integration |  | after unticking Aizome (Yukimi moves Default desktop to Tatami), the Desktops group's Undo removes only the desktops draft, so Apply writes `calamares.defaultDesktop = "tatami"`. |
| [YUK-20](#yuk-20) | Sheet lifecycles: package details, branch chooser, comparison | Confirmed | integration | upheld with caveat | `Msg::Branches` carries neither the input nor the sheet key, so after the person cancels one input's branch chooser and opens another's, the first GitHub answer fills the open sheet, and the person can Follow another repository's branch. |
| [NTU-01](#ntu-01) | Nisshi system-log unlock (root reader through pkexec) | Confirmed | integration | upheld | if the user opens an unreadable /var/log file, then picks All Messages while the password dialog is up, root `Ready` replays `after_unlock` and switches the window back to the file the user left. |
| [NTU-03](#ntu-03) | Nisshi journal read pipeline: newest page, earlier pages, follow, whole-boot search | Confirmed | unit |  | if a selected row's fields read fails while the 50,000-entry newest page is still streaming, `Failed` sets `loading = false` with `Want::Load` still pending. "Reading…" vanishes, Read Earlier enables, and a pending `--cursor` gives up early. |
| [NTU-17](#ntu-17) | Tokei alarm schedule: alarms.json against systemd transient timers | Confirmed | integration |  | if a once-alarm is ringing when the user opens Tokei, `launched` → `sync_alarms` runs `systemctl --user stop …timer …service`, which kills `--ring` before it turns the alarm off. `schedule` then re-arms the alarm for the same time tomorrow. |
| [NTU-19](#ntu-19) | Tokei's in-memory alarm list, its saves and the undo slot | Confirmed | integration | upheld | if Tokei stays open while a once-alarm rings and is stopped (file now `enabled: false`), then toggling any other alarm saves the window's stale list with the alarm on again, and the next sync arms it. |
| [NTU-26](#ntu-26) | Utsushi playback item lifecycle | Confirmed | integration |  | if another file is opened while a film plays at 45:00, `Msg::Open` replaces `queue` before `load(0)` calls `leave()`. The new file is recorded at the film's position and length and starts there "Resumed". If the film had ended, the new file's own resume point is erased instead. |
| [KKO-01](#kko-01) | Dictation on/off: koe.toml `enabled` gating the user unit and the daemon it runs | Partly confirmed | integration |  | if a crash loop under `Restart=on-failure`/`RestartSec=2` trips systemd's start limit, `systemctl start` fails. `Session::start` then spawns an unmanaged `aizome-koe daemon` with stderr to /dev/null, and the user sees only "Koe's service didn't start" instead of the journal reason. |
| [KKO-30](#kko-30) | Kura scan lifecycle and page | Confirmed | integration |  | if a second scan starts while one runs, the first scan's `Scanned(Err(Cancelled))` tears down the second's `scanning` and timer. The window shows "The scan didn't finish" while the second scan runs unseen and can't be stopped, and its result later replaces the page unannounced. |
| [KKO-31](#kko-31) | Analysis and index writing after a scan or a change | Confirmed | integration | upheld | if a first scan's slow analysis (duplicate hashing) finishes after a later scan's analysis, its `Indexed` lands last. The Suggestions show the earlier tree's hints, and index.json and index.md describe the older scan. |
| [TSH-02](#tsh-02) | Shiori document open and worker-restart lifecycle | Confirmed | integration |  | if the supervisor's respawn after a crash fails, it sends `OpenFailed(Worker)`, and `app.rs:1167` excludes Worker errors. The showing document becomes `Load::Failed`, and Reload reopens it as a new document at the top. |
| [TSH-08](#tsh-08) | Copy waiting for pages' text | Confirmed | integration |  | if the worker gives up (`engine` None, Load still Open) and the user copies a selection over pages whose text isn't loaded, `copy` stores a `Copying` that `ask_copy_text` can never feed. Nothing is copied, nothing says why, and `idle()` stays false. |
| [TSH-09](#tsh-09) | Find session | Confirmed | integration | upheld with caveat | if the user closes Find and reopens it, the new `Find` restarts its serial at 0 and `run_find` makes it 1 again. The worker's late `Found{serial:1}` for the old query passes the filter, is stored as a new-query result, and sets the current hit. `current` then points past that page's real hits. |
| [TSH-25](#tsh-25) | Tansu sheet lifecycle (forms holding segment indexes) | Confirmed | integration |  | if the stick is unplugged while Format… is open on its partition, `refresh` falls back to the first drive and the sheet's `seg` now indexes that drive's layout. Add to Pending queues a format of another partition, with no typed name if that drive is removable. |
| [TSH-26](#tsh-26) | Tansu apply session (Apply sheet → Run → Running sheet) | Confirmed | integration |  | if a second launch hands over a path during a run, `open_sheet(Attach)` closes the Running sheet. When the run finishes nothing clears `run`: every action answers "Wait for the operations to finish.", and the status line keeps resetting to "Applied N operations." |
| [SMT-02](#smt-02) | Table selection, expansion and launch request across readings | Confirmed | integration | upheld | A row's context-menu Inspect carries the row index from the right-click, so if a reading re-sorts the table before you click it, Inspect selects and inspects whichever row now sits at that index. |
| [SMT-03](#smt-03) | Quit/pause confirmation and the status notice | Confirmed | integration |  | `notice` is never cleared, so after a refused or completed quit or pause, the status line keeps showing that message instead of the process/thread/uptime summary for the rest of the session. |
| [SMT-13](#smt-13) | Close-after-save for dirty documents | Confirmed | integration |  | If you choose Close → Save and the save is stopped by another alert (Latin-1 can't hold the text, or the write fails), the window stays in `closing`, so a later ordinary ⌘S saves and then closes the window without asking. |
| [SMT-14](#smt-14) | Link and Go to Line sheets | Confirmed | integration | upheld | With a Link sheet (or Go to Line sheet) open over each of two documents, the field messages pick the first sheet of that kind in a `HashMap`. Typing, Insert Link and Go then edit, apply or close the wrong sheet. |
| [SMT-17](#smt-17) | Document file, dirty and autosave lifecycle | Confirmed | integration |  | If a document's only unsaved change is its Encoding or Line Endings, or it was reopened in a chosen encoding, Quit and relaunch reloads it in the detected encoding and clean, so that choice is lost. |
| [SMT-18](#smt-18) | Background file read generation | Confirmed | integration |  | A background read (over 2 MiB) that fails clears the path and sets the document to Untitled, but leaves the file's name as the window title. A failed Revert of a big document detaches it from its file, whereas a small one keeps its path. |
| [SMT-19](#smt-19) | Find bar and search generation | Confirmed | integration | upheld | After Done or Escape, the find matches are still passed to the editor and are not updated after edits, so highlights drift onto other text. In a document over 1 MiB, ⌘G right after an edit uses the pre-edit match list. |
| [SMT-24](#smt-24) | Location consent and GeoClue lookup | Confirmed | integration |  | Turning the current location off never clears `current`, so the last GeoClue position stays in `state.json` and is written there again whenever a place is selected. |
| [SAP-02](#sap-02) | Single-instance socket claim and hand-off (Annai, Shashin, Soroban; Fusen through Sumi's copy) | Confirmed | unit | upheld with caveat | two launches started together both fail to connect, and the second's `remove_file` unlinks the first's freshly bound socket (or its bind fails), so two instances run and the first can no longer be raised or handed files. |
| [SAP-05](#sap-05) | Fusen stickies: windows, saved places and the login item | Confirmed | integration |  | restoring N stickies at login rewrites `stickies.json` with `place: None` for stickies 2..N (each `open_sticky` → `save_stickies` looks places up in the list it is shrinking), and nothing restores them without Aizome's IPC or after a quit within 4 s. |
| [SAP-06](#sap-06) | Fusen launch, request hand-off and Drop-time save | Confirmed | end-to-end |  | a second launch while Fusen runs builds a full `Fusen`, gets `Claim::Handed`, and its `Drop` → `save_all` → `save_stickies` writes `{"stickies": []}` and deletes the stickies' login item. |
| [SAP-07](#sap-07) | Fusen highlighting passes (incomplete, scheduled, notify) | Confirmed | integration | upheld | Fusen's highlighter never sets `scheduled` or calls `notify`, so once a pass sets `incomplete`, `Msg::Highlight` never comes. `state_at` then guesses for every line after the last edit, and fenced code under an edit shows as prose. |
| [SAP-10](#sap-10) | Shashin picture loading (serial, decode, folder listing) | Confirmed | integration |  | open folder A, then folder B before A is listed: A's late `Listed` passes while nothing shows (no generation), shows A's first picture, and B's listing is then dropped by the same-folder check, so asking for B ends up showing A. |
| [SAP-11](#sap-11) | Shashin unsaved turns, saving and quitting | Confirmed | integration |  | two saves of the same unsaved turn (❖S twice during a slow rewrite, or ❖Q → Save, then ❖Q → Save again while the first `SaveAllAndQuit` runs) each run `save_turned` from the file's current pixels, so a picture turned 90° is saved at 180°. |
| [SAP-13](#sap-13) | Shashin stage drawing requests (busy, want, reported) | Confirmed | integration | upheld with caveat | zoom in while a large photo decodes: `run_jobs` drops the stage's inexact report because the picture isn't decoded. After `install` the zoom, centre and view are unchanged, so the report key is the same and the stage never asks again. The picture stays the fit drawing scaled up. |
| [SAP-18](#sap-18) | Shashin Move to Trash with Undo and Redo | Confirmed | integration |  | trash a picture of folder A, open folder B, press ❖Z: the file returns to A, but its path is inserted into B's list and shown there with B's pictures as its neighbours. |
| [SAP-25](#sap-25) | Soroban calculator entry (entry, answer, error, 2nd, undo) | Confirmed | unit |  | after ❖Z, a key that does nothing (`)` with no parenthesis open, a second `.`, a digit outside the radix) still runs `remember()`, which pushes a no-op undo step and empties the redo stack, so ❖⇧Z stops working. |
| [HCO-07](#hco-07) | Kitty clipboard session password grants and paste-event one-time passwords | Confirmed | integration | upheld | one-time paste-event grants share the 32-entry FIFO with remembered grants, and eviction ignores `one_time`, so 32 pastes whose passwords are never spent evict the password the user chose to remember. |
| [HCO-18](#hco-18) | Cursor style (DECSCUSR, DEC mode 12, configured default) | Confirmed | unit |  | a cursor-default change while on the 1049 alternate screen updates only the alternate screen's shape; after 1049 exit the primary keeps the old default shape although `cursor_is_default` is true. |
| [HCO-24](#hco-24) | PTY child lifecycle and background reaper (HUP → grace → KILL → wait) | Confirmed | integration |  | `REAPER.get_or_init` caches an `Err` for the life of the process, so one failed reaper-thread creation (EAGAIN) makes every later `Pty::spawn` fail after the condition clears. |
| [HCO-26](#hco-26) | Shell prompt hooks: load guards and last-reported-cwd de-duplication (OSC 7) | Confirmed | integration | upheld | the shell hooks' last-reported-PWD cache is never invalidated when something else reports OSC 7, so after a nested integrated shell reports another directory and exits, the outer prompt stays silent and the terminal keeps the nested directory. |
| [HTT-10](#htt-10) | (dev tool) Frame benchmark controller run lifecycle | Confirmed | end-to-end |  | on frame-matrix's 600 s timeout the harness gets SIGINT; `KeyboardInterrupt` bypasses `except Exception`, so `finally` writes `result.json` still saying `"status": "running"`. |
| [HTT-14](#htt-14) | (dev tool) Frame matrix batch status and trial acceptance | Confirmed | integration |  | `--phase throughput` trials are accepted even if an external compiler ran during the throughput flood, because the measured interval ends at the last stream capture and the flood comes after the streams. |
| [GEN-03](#gen-03) | Selected-disk identity: discovered → reviewed → revalidated before and after the first write | Confirmed | unit | upheld with caveat | revalidating the selected disk does a full rediscovery that fails on *any* listed disk, so when another disk is unplugged or re-enumerated between `lsblk` and the per-disk `stat`, the revalidation after `wipefs`/`parted` (install.rs:573) or after formatting (install.rs:628) aborts a half-written installation over an unrelated disk. |
| [GEN-07](#gen-07) | Install transaction: non-destructive preparation stages, then destructive steps 1–6 | Confirmed | integration |  | on the deferred route (downloads larger than live memory), the "selected system needs at least N GiB on the root filesystem" guard (install.rs:479–488) never runs. Preparation therefore accepts a selection that can't fit on the target, and the build on the target then fails after the disk was erased. |
| [GEN-28](#gen-28) | Disk scan, selection and installed-memory read | Confirmed | integration |  | a failed `/proc/meminfo` read sets swap off but leaves `memory: None`, which also means "still reading". If swap is turned back on, the Disk page says "Reading installed memory…" and Next says "Still reading installed memory; try again in a moment." indefinitely, with no retry. |
| [GEN-33](#gen-33) | Keyboard step: catching the key beside the space bar through the compositor | Partly confirmed | unit |  | clicking Review while the key wait is open leaves it open. A key typed into a prompt raised during the review (Wi-Fi wallet or Secret Agent), or into the Polkit prompt within the 500 ms poll after `Reviewed`, is swallowed by the compositor's catch: the password loses its first character and `super_key` silently becomes `AsIs`. |
| [GEN-34](#gen-34) | Review, background preparation and confirmation (GUI side of the helper session) | Confirmed | integration | upheld | if the person confirms before preparation finishes and preparation then fails before any disk write, `show_failure` takes its `installing` branch. It marks step 1 "Partition the disk" failed, heads the page "Installation Failed" and says "The disk may have been modified", and the person cannot go back (the passwords have been wiped). |
| [TTL-03](#ttl-03) | Keep Awake: intent marker, inhibitor holder unit and verification | Confirmed | integration | upheld with caveat | One failed or slow logind `ListInhibitors` reply makes `awake hold` return an error and exit. In the real unit that ends `systemd-inhibit` and releases the sleep/idle/lid block until it restarts and is re-verified. |
| [TTL-04](#ttl-04) | Wallpaper choice and per-output overrides | Confirmed | integration |  | If the global wallpaper is on a drive that isn't mounted yet when `background ensure` runs (tatami-swaybg's `ExecStartPre`), the choice is replaced by the shipped default for good. The same image chosen per output survives. |
| [TTL-08](#ttl-08) | Screen recording start, fallback and stop | Confirmed | integration |  | If the user stops a recording while the starting invocation is still in its `started(15)` probe, the starter takes the missing recorder for "gsr cannot run" and starts the wf-recorder fallback on the same file. |
| [TTL-10](#ttl-10) | Display reconciliation commit (plan execution) | Confirmed | integration | upheld | Close the lid while docked. If `wait_disabled` times out but the modeset completes later, `reconcile` returns an error before saving. The next tick saves a record without `lid_off`, so opening the lid never turns the panel back on. |
| [TTL-17](#ttl-17) | Live reload pipeline (`tatami reload`, `update-session`) | Confirmed | integration |  | `tatami reload` try-restarts every running Tatami unit except Keep Awake, including `tatami-hyprsunset` and `tatami-mako`. This silently turns off Night Light and Do Not Disturb. |
| [TTL-25](#ttl-25) | Weather report cache and fetch coordination | Confirmed | integration |  | If the user chooses a new place while a background fetch for the old one is in flight, `refresh` returns at the lock. The running fetch then writes the old place's report with a fresh `fetched`, so the bar shows the old place for up to 15 minutes. |
| [TAP-03](#tap-03) | Activity sampler's baselines, caches and energy integrals between readings | Confirmed | unit | upheld | Read every second through the power meter service (which repeats its last reading within 1 s), the processor shows 0 W then about twice its power, the first 0 W pins the idle floor at 0 for the session, and a failed read doubles the next reading. |
| [TAP-04](#tap-04) | Activity process table: columns per view and remembered sorts | Confirmed | unit |  | The sorter handler's `last` isn't updated while `set_lens` swaps columns, so the first click on the current sort column is undone at start-up and after a view switch, and a column `last` still names can be made primary in ascending order. |
| [TAP-06](#tap-06) | Window singleton, deferred first presentation and launch requests held until the first reading (Activity, Network, Sound) | Confirmed | integration |  | `tatami-activity --pid N` while the window exists but before its first frame toasts "Process N has ended" for a live process and selects nothing. |
| [TAP-08](#tap-08) | Activity's Quit, Force Quit and Pause on a target taken from the latest frame | Confirmed | integration | upheld with caveat | If the Force Quit dialog is answered after its process exited and the PID was reused, the unrelated process gets SIGKILL, because the response uses bare PIDs from the frame shown when the dialog opened. |
| [TAP-15](#tap-15) | Bluetooth window close: hide, drain, release, destroy | Confirmed | integration |  | Relaunching Bluetooth during the close drain presents the hidden, closing window, and the drain then destroys it under the person. |
| [TAP-19](#tap-19) | Displays readings and applies (concurrent changes refused, stale readings discarded) | Confirmed | integration |  | A monitor plugged in while a slider's change applies (no refresh follows `quietly`), or while a reading runs, isn't shown until another Hyprland event, because those refreshes are dropped rather than queued. |
| [TAP-21](#tap-21) | Night Shift requests on unordered threads | Confirmed | integration | upheld | Night Shift requests run unordered: "on" then "off" within a second or two leaves the screen tinted with the switch off, and a debounced temperature request after "off" turns the tint back on. |
| [TAP-25](#tap-25) | Joining a Wi-Fi network: one at a time, with a password retry and rollback | Confirmed | end-to-end |  | Closing the Network window while a new network is being joined ends the process mid-join, so the plaintext passwd-file stays in `$XDG_RUNTIME_DIR` and the failed join's new profile is never deleted. |
| [KTL-02](#ktl-02) | Kakehashi nightly download session | Confirmed | unit | upheld with caveat | dropping a file during a download drops the session without cancelling its worker. Messages are matched by build id only, so after a restart of the same build the orphan's messages drive the new session. Both workers then share one unlocked `.part`, and a fallback truncates it under the other writer. |
| [KTL-03](#ktl-03) | Kakehashi write job, GUI side of the helper protocol | Confirmed | unit |  | when the helper dies by a signal mid-write, `elevate::run` returns `Ran::Failed("the helper was killed")`, and `JobEnded` maps every `Ran::Failed` to `Failed { touched: false }`, so no "partly written" warning is shown. |
| [KTL-04](#ktl-04) | Kakehashi drive list polling and chosen drive | Confirmed | integration |  | "Erase …?" describes the drive chosen when the dialog opens, but OK looks the chosen drive up again by name in a list that keeps refreshing. If the stick is swapped for one with the same name, the new drive is planned and erased. |
| [KTL-12](#ktl-12) | Helper write/verify pipeline and `touched` | Not reproduced (environment) | unit |  | on Windows, `platform::open` deletes the drive layout (windows.rs:309) before `*touched = true` (write.rs:199). A failure or Stop after that point reports `Failed { touched: false }` even though the partition table is gone. |
| [KTL-13](#ktl-13) | Rootless builder VM host driver | Confirmed | integration | upheld | the host waits for the whole guest `build-iso` through `run()`, capped at 7200 s, while the guest's `nix flake check` and `nix build .#iso` each have their own 7200 s cap. A build whose steps each fit but together pass two hours is killed by the host. |
| [KTL-20](#ktl-20) | (test harness) qemu-test install/reboot run and resumable evidence | Partly confirmed | integration |  | `boot-installed` rebuilds the fixture's `verify` command from the current CALAMARES_TEST_* environment, while result.json records only `encrypted`. A run installed with `CALAMARES_TEST_GITHUB` set and resumed in a fresh shell silently skips the GitHub assertions, and the reverse fails a good install. |
| [KTL-24](#ktl-24) | flash_usb privileged write and verify step | Not reproduced (environment) | unit |  | on macOS, `flash_usb`'s `write` flushes the raw `/dev/rdiskN` handle with `device.sync_all()` (scripts/flash_usb.rs:637). That is F_FULLFSYNC, which raw disks reject with ENOTTY, so every macOS flash fails after the whole image is written, before verification and ejection. |
| [INX-18](#inx-18) | Copy-to-RAM of the live media in the initrd | Partly confirmed | unit |  | copytoram sizes the media by the block device holding `/sysroot/iso`. On a whole USB stick, the stick's capacity rather than the image is compared with RAM, so copy-to-RAM is skipped even though the image would fit. |
| [INX-29](#inx-29) | IBus Wayland frontend keymaps, seat and system-keymap mode (patched) | Partly confirmed | unit |  | After a live layout change (US → German) with `use-system-keyboard-layout` on, the early return at patch `:175-178` keeps the old `key_sys`. Engines then get keysyms and modifiers from the old layout, which would explain the known limit in DEVELOPERS.md. |
| [INX-34](#inx-34) | Binary-cache publication pipeline | Confirmed | integration | upheld | The upstream-validity filter runs in process substitutions, so a `nix-store --check-validity` failure part-way is ignored. The script uploads a subset, prints "Published N of M" and exits 0. |
| [INX-36](#inx-36) | Self-hosted runner supervisor and per-job admission | Confirmed | integration |  | When `docker run` cannot start the job container, the supervisor registers a new JIT runner with GitHub every ~5 s (plus the git-cache refresh), with no backoff. |
| [ICI-03](#ici-03) | Trusted PR admission, recheck and commit-status lifecycle | Confirmed | integration |  | If main moves while an admitted PR waits for the builder, the merge recheck fails. The `result` job then marks the head `failure` ("Build and tests: failure") although nothing was built or tested. |
| [ICI-11](#ici-11) | Video playback state (poster/loading/playing/paused/ended/failed) with seek-drag resume | Confirmed | integration | upheld | While dragging the seek thumb during playback, pressing K twice makes `play()` create source S1. On pointer-up, `release()` still sees `resume` and calls `begin()` again, so two soundtracks play. The orphan can't be paused, and its `onended=finish` later jumps the player to `ended`. |

## Details

<a id="cpa-04"></a>

### CPA-04 — Not reproduced (environment)

Catalog entry: [CPA-04](comp-a.md#cpa-04), libseat session pause and resume (VT switching).

- **Claim:** a monitor plugged in while the session is paused (user on another VT) stays dark after they return: `device_changed` runs while paused and reserves the connector, `initialize_output` fails on the inactive device, `ActivateSession` never rescans, and later `Changed` events skip the connector as already known.
- **Level:** none (code reading only; no lower-level seam exists)
- **Method:** Looked for a way to drive the state logic with fakes. There is none. `device_changed`, `connector_connected_as` and the session notifier (`backend/udev.rs:219-262`, `:809-850`, `:943-1066`) work directly on `State.backend = Backend::Udev(Udev)`. The only constructor, `Udev::new`, opens a `LibSeatSession`, and `Device.manager` is smithay's `DrmOutputManager` over an open DRM fd. There is no trait or pure function to substitute, and the headless backend has its own output code, so the `tests/common` harness never reaches this bookkeeping. By reading, I confirmed the parts that need no hardware. The `UdevEvent::Changed` handler (`udev.rs:336`) has no `session.is_active()` guard, `ActivateSession` doesn't call `device_changed`, and `device_changed` skips any connector already in `Device.connectors` (`udev.rs:823`). The one link that decides the bug is unverified: whether smithay's `initialize_output` actually fails on a paused device, rather than deferring the modeset.
- **Command:** none run (opening DRM, libseat or a VT is out of bounds here)
- **Observed:** nothing executed
- **Notes:** Needs a disposable VM, for example the existing udev scenario (`desktop/aizome/crates/aizome-comp/vm/udev.nix`: virtio-gpu, logind and libseat on tty1), extended with a second connector that can be hot-plugged. vkms with configfs connectors would do, or a QEMU virtio-gpu head enabled through QMP. Steps: start Aizome on tty1, `chvt 2`, plug the connector, `chvt 1`, then check `aizomectl outputs` and the log for `setting up …` warnings. Do it once with an unplug and replug as a control.

<a id="cpa-05"></a>

### CPA-05 — Not reproduced (environment)

Catalog entry: [CPA-05](comp-a.md#cpa-05), DRM device and connector hotplug, and output enablement.

- **Claim:** (1) any setup failure leaves the connector reserved without a surface, and it never lights until it is replugged; (2) `enable_output` drops the `disabled` entry before its fallible steps, so a failed enable leaves the output listed as off but refused ("isn't off"), or unknown to IPC; (3) `device_removed` misses outputs turned off on a removed GPU, so stale `disabled` entries remain and a replug can add a duplicate.
- **Level:** none (code reading only; no lower-level seam exists)
- **Method:** Same search as CPA-04: the connector maps live in `Udev` and `Device`, which can only be built with libseat and real DRM devices. By reading at `8e0acbe5`: (1) `device_changed` inserts into `connectors` (`udev.rs:834`) before `connector_connected_as`, whose early returns (`:962` no modes, `:1010` no renderer, `:1037` init error) never undo it. Only `connector_disconnected` (`:1083`) does, and only for a CRTC with a surface. (2) `enable_output` runs `u.disabled.remove(name)` (`:928`) before `get(&node)` (`:929`) and `get_connector` (`:934`). `disabled_outputs` is cleared at `:935`, before `connector_connected_as` can fail. (3) `device_removed` collects only `d.surfaces.keys()` (`:853-857`), so `connector_disconnected`'s cleanup of `disabled` entries (`:1072-1078`) never runs for an output that is off.
- **Command:** none run
- **Observed:** nothing executed
- **Notes:** The reading supports all three parts, but none was executed. A VM needs: (1) a connector whose setup fails: one with no modes, which vkms configfs can make, or an EDID-less head that QEMU refuses to drive; then a second `Changed` event, and check that it never comes up. (2) An output turned off with `aizomectl output … --off`, then made to fail on enable (unplug it, then `--on`), then `aizomectl outputs`. (3) A second GPU, such as a vkms device or a USB DisplayLink in a VM with passthrough: turn one of its outputs off, remove the device, then replug it and enable.

<a id="cpa-16"></a>

### CPA-16 — Confirmed

Catalog entry: [CPA-16](comp-a.md#cpa-16), Recovery overlay session (open and closed).

- **Claim:** if the display the recovery overlay opened on leaves the layout (lid closed, unplugged), `recovery.output` still names it, so the overlay shows only the scrim and no panel while it keeps taking input.
- **Level:** end-to-end (the headless compositor, two outputs, synthetic lid switch)
- **Method:** Uses outputs `eDP-1` and `HEADLESS-2`. Pointer on eDP-1, then IPC `recovery {state: true}`, which opens the overlay. The test records the title strip's colour and how many pixels have it, then closes the lid over IPC `synthesize lid closed`. It asserts that while the overlay is still open, its panel is drawn on the remaining display (title-strip pixels present) or the overlay has closed. File: `repro_cpa_16.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp --test repro_cpa_16` (in `nix develop .#aizome`)
- **Observed:** `the overlay is open (taking every key and click) but its panel is drawn on no display: title-strip pixels 18747 before, 0 after; layout now 1280x800 (HEADLESS-2 at Rect { x: 1280, y: 0, w: 1280, h: 800 }); reported panel Some(Rect { x: 330, y: 213, w: 620, h: 268 }) inside HEADLESS-2: false`
- **Notes:** `recovery-state` still reports the panel at the departed eDP-1 coordinates, which is the geometry hit testing uses (`view.rs:159`). Not exercised separately: keys acting unseen, and a click closing it.
- **Files:** `desktop/aizome/crates/aizome-comp/tests/repro_cpa_16.rs`
- **Independent review:** upheld; re-run: Fails: title-strip pixels before > 0, after 0; panel reported at eDP-1's place outside HEADLESS-2; asserts the claim: Yes; reachability: real: a synthetic lid goes through the same `lid_switched` as libinput's switch. Nothing resets `recovery.output`, and `draw`/`hit` key on it (view.rs:159, :270)

<a id="cpa-24"></a>

### CPA-24 — Confirmed

Catalog entry: [CPA-24](comp-a.md#cpa-24), Process-info worker: request coalescing, CPU baselines and kills.

- **Claim:** when the worker's drain starts with a Kill and finds a Sample queued behind it, `s => req = s` overwrites the Kill: no SIGKILL and no `Killed` report.
- **Level:** unit (the real `procinfo::run` worker loop, driven directly)
- **Method:** The test starts a `sleep 30` child. It queues `Request::Kill` for that pid, then `Request::Sample` (the state left when Force Quit is pressed while the worker is busy and the 1 s sample follows), drops the sender, and runs `run(rx, report_tx)` on the test thread. It collects the reports through a calloop `EventLoop`. It asserts that the child died within 2 s and a `Killed` report arrived. The victim is killed in cleanup either way. Files: `repro_cpa_24.rs` plus a one-line `mod` at the end of `recovery/procinfo.rs` (`INSERT.txt`).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp-core --lib repro_cpa_` (in `nix develop .#aizome`)
- **Observed:** `Force Quit was lost: victim pid 902926 still running = true, reports = ["Samples(1)"]`
- **Notes:** Pre-loading the queue stands in for "the worker was busy" (the 250 ms fresh-app sleep, slow X11 lookups) and makes the test deterministic. The X11 linger-step consequence was not exercised.
- **Files:** `desktop/aizome/crates/aizome-comp-core/src/recovery/repro_cpa_24.rs`
- **Module line added to:** `desktop/aizome/crates/aizome-comp-core/src/recovery/procinfo.rs`

<a id="cpa-27"></a>

### CPA-27 — Confirmed

Catalog entry: [CPA-27](comp-a.md#cpa-27), Supervised helper process lifecycle (shell, xwayland-satellite).

- **Claim:** (1) a helper whose first spawn fails is never retried (`restart_at` stays None); (2) a `restart_shell` whose new binary fails to exec has already moved the running shell to `retiring` without a handover, so that shell is unsupervised, and later restarts leave it running beside the next one, never stopped.
- **Level:** end-to-end (the headless compositor with `--shell` and `AIZOME_SHELL_BIN` set to fake shells)
- **Method:** Each fake shell is a script the test writes: it appends `$$` to a file, then `exec sleep 120`. A guard SIGKILLs every recorded pid that is still that `sleep`. The broken build is an executable whose shebang names `/nonexistent/interpreter`. (1) `AIZOME_SHELL_BIN` points at a missing file. The test waits for the logged start failure, creates the file, and asserts a shell starts within 8 s. (2a) After the shell runs, IPC `reload --shell-from broken` (action `failed`), then SIGKILL the running fake shell; asserts a shell comes back within 8 s. (2b) After the failed restart, the build is "fixed" and the recovery action `restart-shell` starts a new shell. Then a normal `reload --shell-from good`, then IPC `quit`. Asserts that at most one shell is alive at each step and none after the compositor quits. File: `repro_cpa_27.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp --test repro_cpa_27` (in `nix develop .#aizome`)
- **Observed:** (1) `the shell that failed to start at login was never retried; reload --shell --check says ReloadPart { action: "skipped", detail: "no shell is running" }` (log: `WARN aizome_comp_core::session: aizome-shell: No such file or directory (os error 2)`) (2a) `after a failed restart, shell 911204 was no longer supervised: reload --shell --check said … "no shell is running" while it ran, and nothing started a shell after it died` (2b) `shells piled up: alive after Restart Shell [911205, 911211], after the next restart's handover [911205, 911435], after the compositor quit [911205] (first shell 911205)`
- **Notes:** The route to "two shells" differs from the entry's wording. A second IPC `restart_shell` cannot overwrite `retiring` with None, because `shell_binary_changed` returns None while `child` is None, so IPC answers "no shell is running". Two shells come instead from Restart Shell (`restart_supervised`), which starts a new child beside the retiring one. The next `restart_shell` then overwrites `retiring` with that child, dropping the first shell's handle, and the first shell outlives the compositor. All effects the entry names reproduce.
- **Files:** `desktop/aizome/crates/aizome-comp/tests/repro_cpa_27.rs`

<a id="cpa-32"></a>

### CPA-32 — Confirmed

Catalog entry: [CPA-32](comp-a.md#cpa-32), Main-loop stall watcher, systemd watchdog and the stall marker.

- **Claim:** the stall watcher's signal handler allocates (`Backtrace::force_capture`). If the stalled thread holds the allocator's lock, the handler deadlocks on it, turning a recoverable stall into a permanent one.
- **Level:** unit (mechanism: the real `install_handler`/`on_signal` and `signal()` in a child process, with real glibc malloc)
- **Method:** The test re-runs itself as a child process, since the child may deadlock. In the child, a thread blocks inside glibc `malloc_stats()` writing to stderr, which is a full pipe. That holds the arena lock the way an allocation stalled on mmap or brk does. A probe allocation of 64 KiB, past the thread cache, confirms the lock is held. The child sends the watcher's signal with `tgkill`, as `capture` does, and waits 800 ms for `CAPTURED`. It then ends the stall by draining the pipe and checks whether the thread returns. A control run skips the signal. `GLIBC_TUNABLES=glibc.malloc.arena_max=1` gives the test thread the main arena, which the compositor's main thread always uses. The child itself never allocates after the stall begins. Files: `repro_cpa_32.rs` plus a one-line `mod` at the end of `watchdog.rs` (`INSERT.txt`).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp-core --lib repro_cpa_` (in `nix develop .#aizome`)
- **Observed:** `control: RESULT recovered captured=no arena-lock-held-in-stall=yes wchan=` `with the watcher's signal: RESULT stuck captured=no arena-lock-held-in-stall=yes wchan=futex_do_wait`
- **Notes:** This shows the mechanism, not a compositor run. Whether real stalls happen inside malloc's critical section is the entry's own low-confidence premise. With the signal, `CAPTURED` never sets, so the real watcher would log "didn't answer" and move on, leaving the thread in `futex_do_wait` for good. Without systemd's watchdog nothing would kill it.
- **Files:** `desktop/aizome/crates/aizome-comp-core/src/repro_cpa_32.rs`
- **Module line added to:** `desktop/aizome/crates/aizome-comp-core/src/watchdog.rs`
- **Independent review:** upheld with caveat; re-run: Fails: control `recovered`, signalled `stuck … wchan=futex_do_wait`; asserts the claim: Yes (mechanism); reachability: staged: `malloc_stats` writing to a full pipe stands in for a stall inside malloc's arena lock; the real `install_handler`, `signal()` and tgkill are used. Real async-signal-unsafe allocation. "Permanent" only without systemd: under `aizome.service` the 30 s watchdog still kills it. Needs a stall of 6 s or more inside malloc

<a id="cpb-07"></a>

### CPB-07 — Confirmed

Catalog entry: [CPB-07](comp-b.md#cpb-07), Idle-inhibitor registry.

- **Claim:** idle inhibitors are kept one per surface, but smithay calls `inhibit`/`uninhibit` once per inhibitor object, so when a surface has two inhibitors and one is destroyed, inhibition ends while the other is still alive.
- **Level:** end-to-end (the real `aizome` binary, headless with pixman, and a real Wayland client)
- **Method:** a client puts two `zwp_idle_inhibitor_v1` on one bare surface and watches ext-idle-notify (`get_idle_notification(300 ms)`). It asserts there is no `idled` while both inhibitors are held (passes), then destroys one and asserts there is still no `idled` for 1.5 s (fails). After both are destroyed it checks that idleness does come. `repro_cpb_07.rs` (new file in `aizome-comp/tests/`)
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp --test repro_cpb_07` (in `nix develop .#aizome`)
- **Observed:** `assertion left == right failed: idle while the call's inhibitor on the same surface was still alive (the video's going ended inhibition)  left: 1  right: 0`
- **Notes:** Aizome's own idle timers check the same set (`idle.rs:89`), so the lock and monitors-off timers are affected too, not only ext-idle-notify clients. The cause matches the reading: smithay's `Request::Destroy` calls `uninhibit(surface)` (smithay 0.7 `idle_inhibit/inhibitor.rs:39`), and `drop_idle_inhibitor` removes the surface's only entry.
- **Files:** `desktop/aizome/crates/aizome-comp/tests/repro_cpb_07.rs`
- **Independent review:** upheld; re-run: FAIL as reported: `idled` 1 after one of two inhibitors was destroyed; asserts the claim: Yes; reachability: real (protocol-legal client). smithay 0.7 `inhibitor.rs:39` calls `uninhibit(surface)` once per object, and `drop_idle_inhibitor` removes the surface's only set entry.

<a id="cpb-08"></a>

### CPB-08 — Confirmed

Catalog entry: [CPB-08](comp-b.md#cpb-08), Pointer constraint activation (lock and confine).

- **Claim:** (1) `Confined` constraints are activated but never enforced, so a confined pointer leaves the window freely. (2) Motion while the pointer is locked returns before `note_activity`, so mouse-only use of a pointer-locked app counts as no input for the idle timers and ext-idle-notify.
- **Level:** integration (the real core `State` in the test process, with the headless backend, pixman, no logic library so the recovery path places and focuses windows, and a real Wayland client on a thread)
- **Method:** the test builds `State` as `run()` does and pumps `event_loop.dispatch` + `on_loop_iteration`. Relative motion goes in through `State::pointer_motion_relative`, the function `process_input` calls for libinput mice; IPC's synthetic motion is absolute only. A client maps a 400×300 toplevel and the pointer enters it. (1) The client confines the pointer (`confined` is received). Ten moves of +40 px go past the window's right edge, still on the output. The test asserts that `pointer_pos` stays inside the window and that the client gets no `leave`. (2) The client locks the pointer (`locked` is received). One relative motion is fed in, and the client's relative-pointer event arrives. The test asserts that `az.idle.last_activity` advanced. `repro_cpb_08.rs`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp --test repro_cpb_08` (in `nix develop .#aizome`)
- **Observed:** (1) `the pointer was confined to the window at Rect { x: 440, y: 250, w: 400, h: 300 } (the client heard 'confined'), yet it went to (1040, 400) and the client got 1 leave(s): Seen { enters: 1, leaves: 1, confined: false, relative: 5, .. }`. (2) `locked: pointer (640, 400) -> (640, 400); last activity moved Some(0ns)`, followed by the assertion `... yet the idle state's last activity didn't move (note_activity never ran)`.
- **Notes:** the logic never touches constraints (only the core does), so loading it would not change either result. The lock itself is enforced: the pointer stayed at (640, 400). The udev backend hands libinput events straight to `process_input` (`backend/udev.rs:212`), and no other path calls `note_activity` for motion. Once focus left, smithay deactivated the confinement itself (the client got `unconfined`).
- **Files:** `desktop/aizome/crates/aizome-comp/tests/repro_cpb_08.rs`

<a id="cpb-18"></a>

### CPB-18 — Confirmed

Catalog entry: [CPB-18](comp-b.md#cpb-18), Popup tracking and popup-layer stacking (xdg and input-method popups).

- **Claim:** input-method popup surfaces (`zwp_input_popup_surface_v2`) are tracked by smithay's PopupManager but never pushed to `popup_order`, so `placed_popups` skips them and they are never drawn.
- **Level:** end-to-end (the real binary with its logic, headless with pixman, one output at scale 2)
- **Method:** a terminal-like client with text-input-v3 (the harness's `Spec::text_input`) takes the keys, and the input method activates. The input method then creates a popup surface with a 160×48 magenta buffer. Precondition: the popup's surface receives `preferred_buffer_scale(2)`. Only the logic's scale refresh sends that, and only to surfaces it finds through `PopupManager::popups_for_surface(window)`, so receiving it proves smithay maps the popup into the shown window's popup tree. The test then asserts the screenshot contains at least half the popup's area in magenta. `repro_cpb_18.rs`; the screenshot is `repro_cpb_18.png`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp --test repro_cpb_18` (in `nix develop .#aizome`)
- **Observed:** `the input method's 160×48 candidate popup (parent: the text field of the window at Rect { x: 120, y: 62, w: 400, h: 300 }, in that window's popup tree: its surface was told buffer scale Some(2)) isn't on screen: 0 magenta pixels`
- **Notes:** scale 2 is needed only for the precondition, because smithay's `send_surface_state` sends nothing when the scale is 1. Hit-testing and window capture also read `placed_popups`, so they miss the popup in the same way; this was not tested separately.
- **Files:** `desktop/aizome/crates/aizome-comp/tests/repro_cpb_18.rs`
- **Support files:** [`repro_cpb_18.png`](reproductions/support/cpb-18/repro_cpb_18.png)

<a id="cpb-20"></a>

### CPB-20 — Confirmed

Catalog entry: [CPB-20](comp-b.md#cpb-20), Foreign-toplevel handle synchronisation (wlr management and ext list).

- **Claim:** `refresh` compares `Snapshot.parent`, but the wlr foreign-toplevel `parent` event is sent only at announce, so a dialog parented after it maps is never re-parented in taskbars.
- **Level:** end-to-end (the real binary with its logic, headless with pixman)
- **Method:** one client maps a main window and a dialog with no parent. A taskbar client binds `zwlr_foreign_toplevel_manager_v1` v3 and records each handle's app id and last `parent`. The app then calls `set_parent(main)` on the mapped dialog. The test waits until IPC `windows` shows the dialog's `parent` as the main window's id, which proves the logic took it. It then asserts that the dialog's handle gets `parent(main handle)` within 3 s. `repro_cpb_20.rs`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp --test repro_cpb_20` (in `nix develop .#aizome`)
- **Observed:** `the compositor knows the dialog's parent is window 1 (IPC), but the taskbar's handle for it never got a 'parent' event (2 parent events in all, all at announce): ... "org.test.dialog", None ...  left: None  right: Some(zwlr_foreign_toplevel_handle_v1@4278190080)`
- **Notes:** the snapshot change does produce a `done` on the handle with nothing in it, as the reading said.
- **Files:** `desktop/aizome/crates/aizome-comp/tests/repro_cpb_20.rs`
- **Independent review:** upheld; re-run: FAIL as reported: the dialog handle never gets `parent`; asserts the claim: Yes; reachability: real (`set_parent` on a mapped toplevel). `refresh` (foreign_toplevel.rs:168-205) never sends `parent`, and `window_changed` is empty.

<a id="cpb-21"></a>

### CPB-21 — Confirmed

Catalog entry: [CPB-21](comp-b.md#cpb-21), Gamma-control ownership per output.

- **Claim:** the per-output `gamma_client` flag is not tied to a control, and a failed control keeps `Some(output)`. So when A's failed control is destroyed after B has taken the output, `release` clears B's hold and Night Light takes the ramps back.
- **Level:** unit (the real `Dispatch<Control, ControlData>` handlers and `release` on a real core `State` with the headless backend; the clients are raw sockets inserted into the display, and requests are written on the wire)
- **Method:** a `#[cfg(test)]` module in `gamma_control.rs`. The headless backend has no gamma LUT, so `GetGammaControl` refuses every output there. The test therefore makes the grant the way that branch does: it sets `gamma_client` and creates the control with `ControlData { output: Some(o) }`. Steps:
  1. A's real `set_gamma` request fails, since headless `set_gamma` always errors, as udev's does while the VT is away. A hears `failed` and the output is released (both asserted).
  2. B is granted the output, and a quiet loop turn keeps the hold (control).
  3. Test 1: A sends `destroy` on its failed control, and the test asserts B still holds. Test 2: A sends `set_gamma` again, and the test asserts B still holds.
  `repro_cpb_21.rs`, inserted per `INSERT.txt`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp-core --lib repro_cpb_21` (in `nix develop .#aizome`)
- **Observed:** test 1: `client A destroyed its control that had failed (and been released) earlier, and that released client B's hold on HEADLESS-1`. Test 2: `client A's failed control set gamma on HEADLESS-1 again while client B held it ...; A heard [(4278190080, 1)]` (a second `failed`, which released B's hold again).
- **Notes:** on headless, the Night Light overwrite itself can't be seen because no output takes gamma; the flag is what `update_night_light` consults. Test 2 shows the related hazard: a failed control can still `set_gamma`. Under udev that call would succeed and overwrite B's ramps instead of releasing them. The emulated grant sets `gamma_client` directly, so a fix that replaces the flag with an owner needs the helper updated.
- **Files:** `desktop/aizome/crates/aizome-comp-core/src/protocols/repro_cpb_21.rs`
- **Module line added to:** `desktop/aizome/crates/aizome-comp-core/src/protocols/gamma_control.rs`

<a id="cpb-22"></a>

### CPB-22 — Partly confirmed

Catalog entry: [CPB-22](comp-b.md#cpb-22), ext-image-copy-capture session and frame lifecycle.

- **Claim:** a frame copied with a fence is replaced by `CreateFrame` before the fence signals. Its copy has already advanced the stream's damage, `ready` drops it, and the new frame finds `Unchanged`, so an idle screen stalls the resumed portal stream.
- **Level:** end-to-end (the real binary, headless with pixman, and a real ext-image-copy-capture client)
- **Method:** an output session gets its first frame. The window turns orange once and the screen then stays still; a screenshot confirms the orange. The client then creates frame 1, creates frame 2 (which replaces frame 1), and captures both. The test asserts that frame 2, the session's current frame, comes `ready` within 3 s with the orange change. `repro_cpb_22.rs`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp --test repro_cpb_22` (in `nix develop .#aizome`)
- **Observed:** `the session's current frame got nothing in 3 s on an idle screen; the replaced frame was never answered, and its buffer holds the orange change: its copy took the stream's damage (failed: [])`
- **Notes:** 
  - What reproduces: the downstream mechanism in the claim. A replaced frame's copy consumes the stream's damage, `ready` drops it, and the current frame waits indefinitely on an idle screen.
  - How it was triggered: by the entry's Hazard, since `capture` queues a frame that is no longer the session's current one.
  - What does not reproduce: the fence race itself. Under pixman, copies finish synchronously inside `flush`, so nothing is ever in flight. The race needs GLES rendering into a dmabuf on a render node, which was not used because DRM devices are off-limits here. It also needs a `CreateFrame` that lands between the copy and the fence signal, which a client can't order deterministically; it would take an injected fence or a backend hook.
  - The existing test `a_second_frame_replaces_the_first` covers a replacement before any copy, which works.
- **Files:** `desktop/aizome/crates/aizome-comp/tests/repro_cpb_22.rs`

<a id="cpb-24"></a>

### CPB-24 — Not reproduced (environment)

Catalog entry: [CPB-24](comp-b.md#cpb-24), wlr-output-management configuration lifecycle.

- **Claim:** when a two-head apply fails on the second head (for example udev's "has no mode WxH"), the first head's change stays applied while the client is told `failed`.
- **Level:** end-to-end (an attempt on headless)
- **Method:** a wlr-output-management v4 client applies kanshi's style of profile. HEADLESS-1 gets scale 2, and HEADLESS-2 gets `set_custom_mode(1234, 567)`, a mode it doesn't list. The test reports the result and the outputs before and after. If the answer is `failed`, it asserts that HEADLESS-1 is unchanged. `repro_cpb_24.rs`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp --test repro_cpb_24 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** `apply: succeeded; HEADLESS-1 (scale, mode, modes) (1.0, Some((1280, 800)), 1) -> (2.0, Some((1280, 800)), 1); HEADLESS-2 (1.0, Some((1280, 800)), 1) -> (1.0, Some((1234, 567)), 2)`. The test passes vacuously.
- **Notes:** 
  - Why headless can't reach the failure: its `configure_output` adds whatever mode it is given (`backend/headless.rs:166-169`). Every other error, unknown names and scale range, is checked before anything is applied (`output_management.rs:328-337`). So no second head can fail.
  - The winit backend needs a host display, which is off-limits.
  - By reading, the loop at `output_management.rs:339-345` has no rollback. Udev's `configure_output` can fail late with `has no mode {w}x{h}` (`backend/udev.rs:680`), a `use_mode` error, or "turn it on with enable_output".
  - What is needed: Aizome on the udev backend in a disposable VM with two KMS connectors (for example QEMU virtio-gpu with two heads), then a `wlr-randr` or kanshi apply whose second head names a mode it lacks.
- **Files:** `desktop/aizome/crates/aizome-comp/tests/repro_cpb_24.rs`

<a id="cpb-25"></a>

### CPB-25 — Confirmed

Catalog entry: [CPB-25](comp-b.md#cpb-25), wlr-screencopy frame lifecycle and stream pool.

- **Claim:** (1) a wlr-screencopy stream is reused after a scale change (same mode, same region) with its old tracker scale, so surfaces are drawn at the wrong size. (2) A client that destroys its manager right after requesting a frame leaves a full-output stream behind for as long as the output exists.
- **Level:** end-to-end (the real binary, headless with pixman, and a real wlr-screencopy client)
- **Method:** 
  - (1) A whole-output copy is made at scale 1. IPC sets `HEADLESS-1` to scale 2 (same mode). A screenshot is taken, and the same manager makes another copy. Control: a new manager's copy matches the screenshot (passes). The test then asserts that the reused stream's copy matches the screenshot at 15 points across the window.
  - (2) The test reads the compositor's `RssAnon` (`/proc/<pid>/status`, pid from IPC `version`). The control is 40 managers each destroyed after its copy is ready. The bug case is 40 managers each sending `capture_output` + `copy` + `destroy` in one batch. It asserts that the bug case grows less than ten 4000 KiB buffers.
  `repro_cpb_25.rs`; the images are `repro_cpb_25_screenshot.png` and `repro_cpb_25_screencopy.png`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp --test repro_cpb_25 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** (1) `after the scale went 1 → 2, the recording differs from the screen inside the window at Rect { x: 246, y: 169, w: 149, h: 87 } (screen, recording): [((521, 425), [51, 102, 204], [26, 27, 38]), ... 13 of 15 points]`. The saved copy shows the window's surface and title text at half size inside a full-size frame. (2) `RssAnon: 21808 KiB, 21808 KiB after 40 managers destroyed after their copies (+0), 170040 KiB after 40 destroyed right after asking (+148232; 37.1 full-output buffers' worth)`
- **Notes:** (1) The fresh-manager control isolates the cause to stream reuse; the ext-image-copy-capture path resets on a scale change. (2) The leak is about 4 MB per dead manager at 1280×800; it is anonymous memory from pixman's offscreen image. The first few leaked buffers reuse heap memory freed by the control loop, hence 37 rather than 40.
- **Files:** `desktop/aizome/crates/aizome-comp/tests/repro_cpb_25.rs`
- **Support files:** [`repro_cpb_25_screencopy.png`](reproductions/support/cpb-25/repro_cpb_25_screencopy.png), [`repro_cpb_25_screenshot.png`](reproductions/support/cpb-25/repro_cpb_25_screenshot.png)

<a id="wm-04"></a>

### WM-04 — Confirmed

Catalog entry: [WM-04](wm-comp-tests.md#wm-04), Interactive resize and its edge anchor.

- **Claim:** the left-edge resize anchor outlives its gesture. After the window is moved, a commit with a size different from the model's puts the left edge at `anchor.start.right() - width`, so the window jumps back.
- **Level:** end-to-end (the headless compositor and a minimal hand-written xdg-shell client that can commit a size of its own)
- **Method:** The client is SSD, 400×300, and obeys every configure. The test widens it by 100 from the left frame edge with a synthetic press, drag and release, and asserts the right edge stays put. It drags the window by its tab with a synthetic pointer, then the client grows itself to 560 wide (a commit without a configure, as a terminal changing its font does). It asserts the window grows where it was moved. File: `repro_wm_04.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp --test repro_wm_04` (in `nix develop .#aizome`)
- **Observed:** `the window jumped when its app resized it: at Rect { x: 540, y: 362, w: 500, h: 300 } after the move, then Rect { x: 280, y: 362, w: 560, h: 300 }; the old resize's right edge was x = 840` (280 = 840 − 560)
- **Notes:** A custom client was needed because the harness's test client always commits the configured size, and every compositor-initiated size change (Fill, IPC Resize, relayout) sets the model rect first. Through it the stale anchor is not reached. A client clamping to its own minimum takes the same path.
- **Files:** `desktop/aizome/crates/aizome-comp/tests/repro_wm_04.rs`

<a id="wm-10"></a>

### WM-10 — Confirmed

Catalog entry: [WM-10](wm-comp-tests.md#wm-10), Sticky keys (latched modifiers).

- **Claim:** with sticky keys, if the logic is replaced (a reload) or sticky keys are turned off while a key carrying latched modifiers is down, `Access.applied` is dropped and the synthetic modifier presses are never released, so later keys arrive with those modifiers.
- **Level:** end-to-end (the headless compositor, a test client recording keys and modifiers)
- **Method:** Sticky keys on. The test taps Ctrl (latch), presses A (A arrives with Ctrl), then does one of two things: IPC `reload --logic` (report `reloaded`), or rewriting settings to `sticky_keys = false` followed by `reload-config`. It then releases A, types B, and asserts B arrives without Ctrl. File: `repro_wm_10.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp --test repro_wm_10` (in `nix develop .#aizome`)
- **Observed:** `Ctrl stayed held after the reload: B came with modifiers 0x4` `Ctrl stayed held after sticky keys went off: B came with modifiers 0x4`
- **Notes:** The entry's example triggers the reload with a ⌃❖R binding pressed through the latch. The test reloads over IPC between A's press and release instead. Same mechanism: `quiesce` takes `access`.
- **Files:** `desktop/aizome/crates/aizome-comp/tests/repro_wm_10.rs`

<a id="wm-12"></a>

### WM-12 — Confirmed

Catalog entry: [WM-12](wm-comp-tests.md#wm-12), Area picker (screenshots and IPC `pick-area`).

- **Claim:** a pick requested over IPC (`pick-area`) that is closed by `quiesce`/`close_all` (a logic reload, or Reset Input) ends without `AreaPicked{area: None}`, so the requester waits forever.
- **Level:** end-to-end (the headless compositor, a subscribed IPC event stream)
- **Method:** The test subscribes, sends `pick-area` with tag `share`, and waits for `PickStarted`. It then runs the recovery action `reset-input` or a `reload --logic`, and asserts that an `AreaPicked` for `share` with no area arrives within 3 s. It then clicks to see whether the picker was still up (a click would pick the display). File: `repro_wm_12.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp --test repro_wm_12` (in `nix develop .#aizome`)
- **Observed:** `after Reset Input the asker heard nothing (picker still up and picking on a click: false)` (left `None`, right `Some(None)`) The same for `after a logic reload …`
- **Notes:** The picker is gone, not hidden: a later click produced no pick. Only the compositor side was exercised. `aizome-share-picker` and the shell's recording flow were not run; they wait on exactly this event.
- **Files:** `desktop/aizome/crates/aizome-comp/tests/repro_wm_12.rs`
- **Independent review:** upheld; re-run: Fails both: `after Reset Input/a logic reload the asker heard nothing`; asserts the claim: Yes; reachability: real: Reset Input and a logic reload over IPC. `close_all` sets `area = None` without `emit`; `aizome-share-picker` loops on `AreaPicked`

<a id="wm-15"></a>

### WM-15 — Confirmed

Catalog entry: [WM-15](wm-comp-tests.md#wm-15), Desks overview.

- **Claim:** when the display showing the Desks overview leaves the layout, nothing closes or moves the overview: nothing is drawn, yet it keeps grabbing the keyboard and swallowing every key.
- **Level:** end-to-end (the headless compositor with `eDP-1` and `HEADLESS-2`, synthetic lid switch, a test client)
- **Method:** A window sits on eDP-1 with the keyboard. IPC `overview {state: true}` opens the overview there. The overview's backdrop is the desktop colour in this theme, so the test checks instead that its heading band (the "Desks" title, summary and filter field) appears. The test closes the lid, waits for the window to move to HEADLESS-2, and taps B. It asserts that B reaches the window, or that the overview is drawn on HEADLESS-2; either is acceptable for a fix. File: `repro_wm_15.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp --test repro_wm_15` (in `nix develop .#aizome`)
- **Observed:** `the overview, drawn on no display (heading pixels: 10170 while it showed on the panel, 0 on HEADLESS-2 now, a 1280x800 layout), swallowed B: the window has keyboard focus false and heard keys []`
- **Notes:** The capture after the lid closes shows the ordinary desktop with the window and an unfocused tab. Pointer presses being taken was not tested separately.
- **Files:** `desktop/aizome/crates/aizome-comp/tests/repro_wm_15.rs`

<a id="sha-02"></a>

### SHA-02 — Confirmed

Catalog entry: [SHA-02](shell-a.md#sha-02), Shell-replacement carry handoff (state file, final word, retiring).

- **Claim:** when a daemon that isn't a replacement shell (dunst or mako started by hand) takes `org.freedesktop.Notifications`, the shell gets `NameLost`, writes a final state and sets `retiring` for good. It keeps running but never saves what it carries again, and it never asks for the name back.
- **Level:** integration (the real `NoteServer` on a private `dbus-daemon`; the real `Shell` through the kit `Harness` with `opts.live` and `carry_ready` set; `AIZOME_SHELL_STATE` points at a file of the test's own)
- **Method:** (1) The test folds Mail (`DeskbarMsg::Fold`) and checks that the state file has it. A second bus connection then requests the name with `ReplaceExisting|DoNotQueue`, as a hand-started daemon would. The server reports `NameLost`, and the test feeds that event to the shell. It then folds Editor and asserts that the state file contains Editor's fold. (2) With the same setup, the stray daemon disconnects. The test asserts that the shell's server gets `NameAcquired`, meaning it holds the name again. Files: `repro_sha_02.rs` (goes in `crates/aizome-shell/tests/`)
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shell --test repro_sha_02` (in `nix develop .#aizome`)
- **Observed:** (1) `the state file must follow the running shell after a stray daemon took the name: live [("org.example.Editor", true), ("org.example.Mail", true)], saved [("org.example.Mail", true)], retiring true`; (2) `left: Err(Timeout) right: Ok(NameAcquired)`, with the owner query giving `NameHasNoOwner`. After dunst quits, nobody serves notifications.
- **Notes:** The trigger is real D-Bus behaviour: the shell holds the name with `AllowReplacement`, so any `ReplaceExisting` request takes it. Because the shell also used `DoNotQueue`, it isn't queued for the name. `watch_name` requests the name only once, and `NameAcquired` doesn't clear `retiring`. A fix has to tell a replacement shell apart from a stray daemon (for example by checking the new owner). Test (1) only says the stray-daemon case must keep saving. The entry's other hazards were not tested: folds and expansions dropped by `Msg::Carried`, a late final word after 8 s, and the +1000 id skip.
- **Files:** `desktop/aizome/crates/aizome-shell/tests/repro_sha_02.rs`
- **Independent review:** upheld with caveat; re-run: FAIL ×2 as reported: Editor fold unsaved, `retiring true`; no `NameAcquired`; asserts the claim: Yes; reachability: real. Any `ReplaceExisting` request takes the name, because the shell uses AllowReplacement\|DoNotQueue. `carry_ready`/`live` set by hand match a non-replacement live shell.. Real, but the trigger needs someone to start a replacing notification daemon. Whether the shell should reclaim the name is a design choice.

<a id="sha-03"></a>

### SHA-03 — Confirmed

Catalog entry: [SHA-03](shell-a.md#sha-03), Deskbar auto-hide visibility.

- **Claim:** if the compositor closes an auto-hide Deskbar while the pointer is over it (its output went), `inside` stays true. The Deskbar reopens on the remaining display with no hide armed, and every `AutoHide` timer is refused, so it stays revealed although the pointer never reached the new surface.
- **Level:** integration (headless shell through the kit `Harness` and `tests/common`'s scene; the compositor's side is played by `request_close` and `set_outputs`)
- **Method:** The scene has two displays and auto-hide on. `SetDeskbarCollapsed(false)` reveals the Deskbar, and the pointer enters it. The laptop's display then goes, in the order `aizome-comp-core/src/outputs.rs` `detach` uses: every layer surface on that output is closed (`request_close` for the Deskbar, bar and desktop), then the output is removed. The test asserts that the Deskbar reopened on the monitor, advances the clock 2 s, and asserts that it has hidden. Files: `repro_sha_03.rs` (goes in `crates/aizome-shell/tests/`)
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shell --test repro_sha_03` (in `nix develop .#aizome`)
- **Observed:** `the reopened auto-hide Deskbar must hide when the pointer isn't over it; still revealed after 2 s (visible: true, pending timers: 1)`. The pending timer isn't an AutoHide timer: those run for 350 ms and would have fired within the 2 s. Reading the code, the reopen path (`reopened == false`) arms none, and `inside == true` would refuse one anyway.
- **Notes:** The entry's open question was whether a leave event arrives first. It doesn't reach the program. `detach` sends `closed` to the layers before anything moves the pointer, and the kit drops presence events for windows it has already closed (`Runtime::pointer_presence` returns early when the window is gone). The kit closes the Deskbar's window as soon as `close_requested` returns true. When the main display changes without an output going away, the shell closes the Deskbar itself through `sync_deskbar`, which resets `inside` and re-arms; that path works. This was not run against a real compositor.
- **Files:** `desktop/aizome/crates/aizome-shell/tests/repro_sha_03.rs`

<a id="sha-12"></a>

### SHA-12 — Confirmed

Catalog entry: [SHA-12](shell-a.md#sha-12), Palette open/close and its drawn-ahead spare window.

- **Claim:** if the palette is open when its output goes, `close_requested` forgets only the surface, and `Shell::palette` keeps the dead window. The next open (typing in the Toolchest, or `aizomectl palette QUERY`) only re-ranks into that dead window, so no palette shows.
- **Level:** integration (headless shell through the kit `Harness` and `tests/common`'s scene)
- **Method:** The scene has two displays, and the palette is open on the laptop. The palette's layer is closed (`request_close`), then the laptop's output is removed. The test then sends (1) `Msg::ToolchestTyped("bold")` or (2) `Action::Palette { query: "bold" }`, and asserts that `palette.window` is an open, shown window. Files: `repro_sha_12.rs` (goes in `crates/aizome-shell/tests/`)
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shell --test repro_sha_12` (in `nix develop .#aizome`)
- **Observed:** both: `the palette must open in a live window; palette state Some((WindowKey(6), "bold")), windows ["Menu bar", "Desktop", "Deskbar"]`. The query lands in the dead window's state, and there's no Palette window.
- **Notes:** Not tested: recovery by a toggle (`toggle_palette` sees `Some` and closes the ghost), and the drawn-ahead spare's dead window. `spare_palette_on` drops a spare that belongs to another output id, so the spare is mostly replaced after an output change.
- **Files:** `desktop/aizome/crates/aizome-shell/tests/repro_sha_12.rs`

<a id="sha-13"></a>

### SHA-13 — Confirmed

Catalog entry: [SHA-13](shell-a.md#sha-13), Palette file search, index lifetime and per-opening extras.

- **Claim:** the palette's file-search `seq` restarts at 0 at every opening. A search from an earlier opening that returns late has the same `seq` as the new opening's first search, so it's accepted, with `files_for` set to the new query.
- **Level:** unit (the mechanism: the real shell through the kit `Harness`, a real `FileIndex` on a temp folder, and real searches on the kit pool; the late reply is delivered by hand)
- **Method:** First opening: the test gives the palette the index, types "alpha" (`search == 1`), waits for the reply and keeps its contents. It presses Esc, opens the palette again, gives it the index, types "beta", checks that `search == 1` again, and waits for beta's reply. It then delivers `PaletteEvent::Files(1, <alpha's reply>)`, the earlier opening's search returning after this one's. The test asserts that the palette still shows `beta-plan.txt` and not `alpha-report.txt`. Files: `repro_sha_13.rs` (goes in `crates/aizome-shell/tests/`)
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shell --test repro_sha_13` (in `nix develop .#aizome`)
- **Observed:** `query "beta", files_for "beta", files ["alpha-report.txt"]`, which fails `the palette shows the files for "beta", not an earlier opening's "alpha"`.
- **Notes:** The test confirms the acceptance logic deterministically. It does not show how likely the timing is. The old search has to outlast Esc, the reopening, the index reload from disk and the new search. The kit pool has 2–8 threads, so a slow broad search can finish after a narrower later one. The wrong rows stay until the next keystroke re-ranks. A per-opening (or global) counter would make the test pass.
- **Files:** `desktop/aizome/crates/aizome-shell/tests/repro_sha_13.rs`
- **Independent review:** upheld with caveat; re-run: FAIL as reported: query "beta", files `[alpha-report.txt]`; asserts the claim: Yes; reachability: staged. The first opening's reply is replayed after the second's. Really reaching this needs a search that outlasts Esc, reopening, the index reload and the new search.. Real acceptance bug (`search` restarts at 0). The window is narrow.

<a id="sha-25"></a>

### SHA-25 — Confirmed

Catalog entry: [SHA-25](shell-a.md#sha-25), Torn-off palettes (persisted list, shown surfaces, strip drag).

- **Claim:** torn-off palettes are keyed by their index in `tearoffs`. (1) Tearing off again a menu that already has a palette shifts the later entries, while the shown windows keep the old indices: windows show each other's menus, and the re-torn palette isn't moved where it was dropped. (2) When a palette's output goes, `tear_shown[i]` keeps the dead window, so a pinned palette never opens again.
- **Level:** integration (headless shell through the kit `Harness` and `tests/common`'s scene; each window's menu is read from the menu widget's semantics title, and its place from the stand-in platform's layer margins)
- **Method:** (a) The test tears off Editor's File at (100, 100) and Edit at (400, 100), then File again at (700, 300). It asserts that Edit's palette is still at (400, 100) and File's is at (700, 300). (b) It tears off only File, at (100, 100), then again at (700, 300), and asserts the window is where `tearoffs[0]` says. (c) On two displays, File is torn off and pinned. The laptop's layers are closed and its output removed, and the test asserts that one torn-off window shows. It asserts the same again when the laptop comes back. Files: `repro_sha_25.rs` (goes in `crates/aizome-shell/tests/`)
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shell --test repro_sha_25` (in `nix develop .#aizome`)
- **Observed:** (a) remembered `[(["Edit"], 400, 100), (["File"], 700, 300)]` but `window 4 at (100, 100): shows ["Edit"]` and `window 5 at (400, 100): shows ["File"]`, so `Edit's palette stays where it was left: (100, 100) right: (400, 100)`; (b) `remembered at (700, 300); shown [(100, 100)]`; (c) `the pinned palette shows on the display left left: 0 right: 1`
- **Notes:** In (a) the two palettes swap menus: each window now draws the other entry's menu and was never moved. Drags then act on the swapped entries; this was not separately asserted. In (c) the palette shows no window on any display until it's closed or the shell restarts. The test stopped at the first assertion, so the "display back" step is reasoned from the code, not observed.
- **Files:** `desktop/aizome/crates/aizome-shell/tests/repro_sha_25.rs`

<a id="shb-03"></a>

### SHB-03 — Confirmed

Catalog entry: [SHB-03](shell-b.md#shb-03), Polkit authentication dialog.

- **Claim:** with polkit's helper unreachable, `Session::start` emits Error then Failed at once and the shell's Failed handler calls `authenticate` again, so the dialog spins through connect attempts until cancelled.
- **Level:** integration (the real shell, headless in the kit harness, fed by the real aizome-sys `PolkitAgent::register_with` on a private dbus-daemon with a fake polkitd; `Helper::Socket` pointing at a socket path that doesn't exist)
- **Method:** the fake polkitd owns `org.freedesktop.PolicyKit1`, calls `BeginAuthentication` on the agent; the agent's events go through a channel into `h.send(Msg::Auth(..))`, as the shell's message sender delivers them. Nobody types. Asserts the conversation is restarted at most 3 times in 2 s. Files: `repro_shb_03.rs` (aizome-shell `tests/`)
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shell --test repro_shb_03` (in `nix develop .#aizome`)
- **Observed:** `the dialog restarted the conversation 500 times (500 errors from the agent) in 381.616044ms with no one typing; it shows Some((Some("That didn't work. Try again."), false))` (500 is the test's cap)
- **Notes:** the loop has no delay at all. The connect error is reported synchronously from inside `authenticate`, so each Failed→authenticate→Error+Failed round trip costs only one pass of the shell's message loop. The real agent and the real shell handler both take part, so a fix on either side (backoff or a terminal outcome in aizome-sys, or no automatic retry after a failure that came without a prompt in the shell) makes this pass. SYB-09 is the aizome-sys side.
- **Files:** `desktop/aizome/crates/aizome-shell/tests/repro_shb_03.rs`
- **Independent review:** upheld with caveat; re-run: Fails: 500 restarts in 304 ms, no one typing; asserts the claim: Yes; reachability: staged: `Helper::Socket` at a missing path. `Helper::find` checks only at registration, so the real loop needs the helper to vanish or refuse later (polkit restart or upgrade, a socket unit that failed). Real shell handler and real agent; no backoff anywhere

<a id="shb-06"></a>

### SHB-06 — Confirmed

Catalog entry: [SHB-06](shell-b.md#shb-06), Session confirmation countdown (Restart, Shut Down, Log Out, power key).

- **Claim:** if the Restart/Shut Down/Log Out dialog's output goes during the countdown, `close_requested` only drops the `surfaces` entry, `confirm` stays `Some`, the ticks go on and the verb is carried out with no dialog to cancel it; asking again only resets `left` and never reopens a window.
- **Level:** integration (the headless shell in the kit harness; the compositor taking the layer away is the harness's `request_close`, which is how the kit delivers a layer surface's `closed` (sidestep-ui `layer_shell.rs` docs), then `set_outputs` without that output)
- **Method:** two outputs (eDP-1, DP-1). Shut Down opens the dialog on eDP-1. After 5 s eDP-1 goes (close request for the dialog, then outputs = [DP-1]). Shut Down is asked again, then 61 s pass. Asserts that no countdown runs without a visible dialog, and that nothing is carried out without one. Off-live, the shell only records the verb (`session_done`), so nothing powers off. Files: `repro_shb_06.rs`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shell --test repro_shb_06` (in `nix develop .#aizome`)
- **Observed:** `a countdown runs with no dialog to cancel it: after the compositor closed the dialog: countdown Some((ShutDown, 55)), dialog shown false; asked again: dialog shown false; 61 s later the shell did Some(ShutDown)`
- **Notes:** live, `do_session(ShutDown)` calls logind's `PowerOff`. That last step was not run (safety); the test stops at the off-live record of the verb, on the same code path.
- **Files:** `desktop/aizome/crates/aizome-shell/tests/repro_shb_06.rs`

<a id="shb-09"></a>

### SHB-09 — Confirmed

Catalog entry: [SHB-09](shell-b.md#shb-09), Compositor event follower (connect, snapshot, subscribe, reconnect).

- **Claim:** events between the follower's snapshot requests and `Subscribe` are lost, because the compositor's `Subscribe` only sets `subscribed` and replays nothing, so a window mapped in that gap is missing from the shell's model.
- **Level:** end-to-end for the compositor (the real `aizome` binary, headless, through aizome-comp's `tests/common` harness, with a real Wayland test client). The shell's `ipc::connect` is private, so its request sequence (Outputs, Desks, Windows, FocusedWindow on one connection, then `Client::events()` = Subscribe) is replayed verbatim with `aizome_ipc::Client`.
- **Method:** take the snapshot, map `repro.gap` and wait until the compositor lists it, then subscribe. Then `AddDesk` serves as a marker: the follower reads events until the marker's `DesksChanged`. Asserts the window is in the snapshot or in the events received before the marker. Files: `repro_shb_09.rs` (aizome-comp `tests/`)
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp --test repro_shb_09` (in `nix develop .#aizome`)
- **Observed:** `window 1 (repro.gap) is in neither the snapshot (0 windows) nor the events after Subscribe (["DesksChanged"]); the compositor lists it: true`
- **Notes:** the gap is forced deterministically here. In the live shell it is the few milliseconds between `Windows` and `Subscribe`. "Until the next reconnect" is too strong: the shell's `Wm::apply` also adds a window on `WindowChanged` (`aizome-shell/src/wm.rs:117`-`119`), so any later title, geometry or state change heals the model. `WindowFocused` doesn't add it. A window that never changes stays missing.
- **Files:** `desktop/aizome/crates/aizome-comp/tests/repro_shb_09.rs`
- **Independent review:** upheld with caveat; re-run: Fails: `window 1 … in neither the snapshot (0) nor the events`; asserts the claim: Yes; reachability: staged: the gap between the `Windows` reply and `Subscribe` is held open; requests replayed verbatim from `ipc::connect`. Real lost-event gap, a few ms long live, and healed by any later `WindowChanged`

<a id="shb-12"></a>

### SHB-12 — Confirmed

Catalog entry: [SHB-12](shell-b.md#shb-12), Lock screen lifecycle (partially explicit).

- **Claim:** whatever PAM says is replaced by "That password didn't work", so a lockout (faillock) or an expired password makes the right password look wrong on every try.
- **Level:** integration (the headless shell's real lock flow, with an `Authenticator` stand-in that returns what `aizome_shell::pam::Pam` returns on such a refusal: the module's last message (`pam.rs:178`) or `pam_strerror`'s text)
- **Method:** for five refusals (faillock's "The account is locked due to 3 failed logins." and "(10 minutes left to unlock)", pam_unix's "You are required to change your password immediately (password expired)", `pam_strerror(PAM_NEW_AUTHTOK_REQD)`, and "Your account has expired; …"), the test locks, confirms the lock, types the password and presses Return. Asserts that the error the lock surface shows (`lock.error`, which the view draws verbatim, `lock.rs:263`-`275`, `479`) is not the wrong-password text. Files: `repro_shb_12.rs`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shell --test repro_shb_12` (in `nix develop .#aizome`)
- **Observed:** all five refusals show `Some("That password didn't work. Try again.")`
- **Notes:** real libpam was not exercised: that needs a PAM service, a locked or expired account, and root. The strings are those modules' messages. `Pam::authenticate` does pass the reason on; the shell only logs it at info level and discards it (`lock.rs:426`-`428`).
- **Files:** `desktop/aizome/crates/aizome-shell/tests/repro_shb_12.rs`
- **Independent review:** upheld with caveat; re-run: Fails: all 5 refusals shown as "That password didn't work."; asserts the claim: Yes; reachability: staged: a stand-in `Authenticator` returns what `Pam::authenticate` returns (`conv.said.last()` or `pam_strerror`). lock.rs:426-428 discards every reason. Needs a locked or expired account (faillock is not on by default)

<a id="shb-14"></a>

### SHB-14 — Confirmed

Catalog entry: [SHB-14](shell-b.md#shb-14), Critical notification cards surface.

- **Claim:** when the critical cards' output goes, `close_requested` drops only the `surfaces` entry; `cards` keeps the dead key, `sync_cards` keeps resizing it and never reopens, so critical cards are invisible.
- **Level:** integration (headless shell in the kit harness; the layer's close and output removal as in SHB-06)
- **Method:** two outputs; a critical notification opens the cards on eDP-1; eDP-1 goes (close request, outputs = [DP-1]); a second critical notification arrives. Asserts a "Notifications" surface shows while there are critical cards. Files: `repro_shb_14.rs`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shell --test repro_shb_14` (in `nix develop .#aizome`)
- **Observed:** `2 critical cards and no surface showing them; after the close: (shown, cards) = (false, Some((WindowKey(6), 1, 247.0))); now cards = Some((WindowKey(6), 2, 247.0)) (window WindowKey(6) was closed)`
- **Notes:** the count on the dead key goes from 1 to 2, which is the `set_layer_size` branch at `cards.rs:134`-`141` acting on a closed window.
- **Files:** `desktop/aizome/crates/aizome-shell/tests/repro_shb_14.rs`

<a id="shb-15"></a>

### SHB-15 — Confirmed

Catalog entry: [SHB-15](shell-b.md#shb-15), Bar notification line (banner).

- **Claim:** replacing the notification on the bar's line (same id) arms a second `BannerDone(id)` while the first is pending; the first ends the line at the original deadline and, for a transient notification, closes the just-updated one.
- **Level:** integration (headless shell, harness clock)
- **Method:** a transient notification with id 50 is posted, then replaced in place 3 s later. Asserts that 2.5 s after the update (5.5 s after the first post, with `banner_seconds` 5) the line still shows id 50 and the notification still exists. Files: `repro_shb_15.rs`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shell --test repro_shb_15` (in `nix develop .#aizome`)
- **Observed:** `2.5 s into its 5 s the updated notification's line is gone (banner None), and the transient notification is closed as expired`
- **Notes:** a progress notification updated every 2 s gets 5 s of line from its first post. After that, each update shows until the earlier timer for the same id fires, then closes if it's transient.
- **Files:** `desktop/aizome/crates/aizome-shell/tests/repro_shb_15.rs`

<a id="shb-20"></a>

### SHB-20 — Confirmed

Catalog entry: [SHB-20](shell-b.md#shb-20), Screen recording session (partially explicit).

- **Claim:** `adopt_recording` never arms the indicator tick (only `Started` does), so after a shell restart mid-recording the systray clock advances only when something else repaints the bar.
- **Level:** integration (headless shell; the "recorder" is a `sleep 60` the test starts and kills, noted in `$XDG_RUNTIME_DIR/aizome/recording.json` under the wrapper's private runtime dir; `adopt_recording` called as the live shell's start does)
- **Method:** adopt a recording that started 5 s earlier, then 3 × (1.01 s wall clock + 1.01 s harness clock). Asserts the indicator's tick ran (`tick >= 2`). For comparison, a recording started by the shell in test mode is run the same way. Files: `repro_shb_20.rs`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shell --test repro_shb_20` (in `nix develop .#aizome`)
- **Observed:** `the adopted recording's indicator never ticked: tick Some(0) (a fresh one's: Some(4)); label ["Recording 0:05"] then ["Recording 0:05"] after 3 s, bar repainted: false`
- **Notes:** `adopt_recording(&mut self)` takes no `Cx`, so it can't arm a timer. The label then follows only unrelated repaints, such as the minute clock.
- **Files:** `desktop/aizome/crates/aizome-shell/tests/repro_shb_20.rs`
- **Independent review:** upheld; re-run: Fails: tick 0 (fresh recording 4), label stuck at 0:05; asserts the claim: Yes; reachability: real: `adopt_recording` as `Shell` start calls it (shell.rs:1164), with a real child process. `adopt_recording` has no `Cx` and never arms the tick

<a id="svc-05"></a>

### SVC-05 — Confirmed

Catalog entry: [SVC-05](kit-services.md#svc-05), Greeter screen stage (people, password, typed name).

- **Claim:** with `user_list = false` (or nobody listed), a mistyped user name can't be corrected. A rejected password begins the same name again with no way back, and if `CreateSession` fails, Return does nothing (`begin` needs `user()`). Only restarting the greeter helps.
- **Level:** integration (the real `Greeter` program in the kit's headless harness, `live = true`, its own link thread talking over `$GREETD_SOCK` to a scripted fake greetd in the test process, under the wrapper's private runtime directory; never real greetd)
- **Method:** Each scenario runs in a subprocess with `GREETD_SOCK` set. The test types "alcie" in the Name stage and presses Return. In scenario A, the fake asks for a password and rejects it with `auth_error`; the test presses Escape and asserts the name field (`greeter-name`) is back. In scenario B, the fake fails `CreateSession`; the test types a character, presses Return in the field that says "Press Return to try again", and asserts the login begins again or the name field returns. File: `repro_svc_05.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-greeter --test repro_svc_05 --no-fail-fast -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** 
  - Scenario A: greetd was asked `[CreateSession { "alcie" }, PostAuthMessageResponse { Some("hunter2") }, CancelSession, CreateSession { "alcie" }]`, then `no way back to the name: after Escape the login is still for Some("alcie") and the screen shows only ["14:24", "alcie", "Session: Aizome", "That password didn't work. Try again.", "ganymede"]`.
  - Scenario B: `field held "x" before Return, "" after: Return reached submit → begin`, then `Return did nothing: step Failed, greetd asked again: false, user Some("alcie")`.
- **Notes:** Both dead ends reproduce through the real socket link. In scenario A the greeter keeps asking for the password of the mistyped name indefinitely.
- **Files:** `desktop/aizome/crates/aizome-greeter/tests/repro_svc_05.rs`

<a id="svc-23"></a>

### SVC-23 — Confirmed

Catalog entry: [SVC-23](kit-services.md#svc-23), Playback session lifecycle.

- **Claim:** (1) stepping forward then pressing Play jumps back to where stepping started, because steps don't move the clock and `play` seeks to `clock.now()`. (2) A speed change on unseekable media with audio drops the tempo filter but skips the rebase seek, so the clock keeps the old rate while the audio plays at the new tempo.
- **Level:** integration (the real `Player` engine as in `tests/engine.rs`: null audio output, detached video sink, software decoding, the tiny fixtures)
- **Method:** File `repro_svc_23.rs`.
  1. The test opens `clip.mp4` (25 fps) paused, steps forward ten times (waiting until ten more frames have shown, which puts the frame at 0.40 s on screen), presses Play, and asserts the `Seeked` target is about 0.40.
  2. The test generates a 14 s, 24 kHz mono WAV and plays it through a FIFO (mkfifo) in the private TMPDIR, so FFmpeg can't seek it. It measures the clock rate over 1 s, sets the speed to 2.0, waits 2.6 s for the old-tempo samples in the ring to drain, measures again, and waits for `Ended`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-media --test repro_svc_23 --no-fail-fast -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** 
  1. `showing the frame at 0.40 s, position said 0.000; Play seeked to 0.000` → `Play after ten steps forward went back to 0.000 s`
  2. `clock rate at 1×: 1.00; after set_speed(2.0): 1.00; the 14 s tone ended 8.48 s after opening, the clock then saying 8.49 s`
- **Notes:** In part 2 the audio did play at 2×: 14 s of tone ended 8.48 s in, which fits 2× from about 3.5 s. The clock still ran at 1× and was 5.5 s short at the end, exactly the drift the entry describes. While stepping, `position()` also stays at 0.000 with the frame at 0.40 on screen.
- **Files:** `desktop/aizome/crates/aizome-media/tests/repro_svc_23.rs`

<a id="svc-29"></a>

### SVC-29 — Confirmed

Catalog entry: [SVC-29](kit-services.md#svc-29), Video suspension out of sight, and catching up.

- **Claim:** a seek while the window is hidden (`set_visible(false)`) stalls playback. `seek_to` waits for a frame of the new generation, but the video thread is still suspended locally, so nothing is presented, `Seeked` never comes and the audio stays silent until the window is shown.
- **Level:** integration (the real `Player` engine, `chaptered.mkv` fixture, null audio, detached sink)
- **Method:** The test plays, calls `set_visible(false)`, waits for `stats().suspended`, then `seek(1.0, true)`. It asserts that `Seeked(1.0)` and `State(Playing)` arrive and that the position advances over 0.6 s while still hidden. File: `repro_svc_29.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-media --test repro_svc_29 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** `after the hidden seek: Seeked heard: false, Playing again: false, position 1.000 then 1.000 0.6 s later; stats Stats { shown: 1, ..., suspensions: 1, suspended: false, ... }`; the only events were `[ClearCues, Cues([... "Second line" ...])]`.
- **Notes:** `stats().suspended` reads `false` (the shared flag `seek_to` cleared) while the video thread stays suspended locally. That is the invalid combination the entry names. Speed and track changes, and the GPU-to-software fallback, seek through the same `seek_to`, so they would stall the same way; not run separately.
- **Files:** `desktop/aizome/crates/aizome-media/tests/repro_svc_29.rs`
- **Independent review:** upheld; re-run: Fails: `Seeked false, Playing false, position 1.000 → 1.000`; asserts the claim: Yes; reachability: real: `set_visible(false)` then `seek` on the real `Player`. Local `suspended` survives `seek_to`; `stats().suspended` false shows the split

<a id="azn-01"></a>

### AZN-01 — Confirmed

Catalog entry: [AZN-01](aizome-small-nix.md#azn-01), Keep Awake requested/protected status (setting file against logind's locks).

- **Claim:** `aizome-awake toggle` evaluates `requested()` before taking the edit lock, so two overlapping toggles both turn Keep Awake on and the second toggle is lost.
- **Level:** end-to-end (the real `aizome-awake` binary, twice, against stand-ins)
- **Method:** the setup is:
  - a fake `systemctl` first on PATH, which records start/stop in a file;
  - a private `dbus-daemon`, given to the children as `DBUS_SYSTEM_BUS_ADDRESS`, with a zbus fake logind whose `ListInhibitors` lists Keep Awake's block lock while that file exists;
  - a private `XDG_STATE_HOME`.
  The test holds `keep-awake.lock` (a third change in progress), starts two `toggle`s, waits until `/proc/locks` shows both blocked on that inode (both have read `requested()` by then), releases the lock, and asserts that the two toggles from off end off. Files: `repro_azn_01.rs` (→ `desktop/aizome/crates/aizome-awake/tests/`).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-awake --test repro_azn_01 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** `first toggle: ok=true "Keep Awake is on"` · `second toggle: ok=true "Keep Awake is on"` · systemctl calls: `--user start aizome-keep-awake.service` ×2 → `two toggles from off left Keep Awake on (setting file present: true, service started: true)`
- **Notes:** the lock was held externally to make the overlap deterministic. Without it the window is the few microseconds between `requested()` and `flock`. As the entry says, the menu bar sends `on`/`off`, so only CLI and script callers of `toggle` are affected.
- **Files:** `desktop/aizome/crates/aizome-awake/tests/repro_azn_01.rs`

<a id="azn-04"></a>

### AZN-04 — Confirmed

Catalog entry: [AZN-04](aizome-small-nix.md#azn-04), Agent registration with NetworkManager (retry, re-register on restart).

- **Claim:** two consecutive "already registered" refusals (another agent still holds the identifier) set `done = asked = true`, so the agent stops retrying and is never registered once the other agent disconnects.
- **Level:** integration (the real `aizome_network_agent::run` on a private bus)
- **Method:** a scripted AgentManager, as in `tests/credentials.rs`, answers PermissionDenied "An agent with this ID is already registered for this user." twice, then success. The test runs `run(agent)` and waits three `REGISTER_RETRY` periods plus 1 s for a third `RegisterWithCapabilities`. Files: `repro_azn_04.rs` (→ `desktop/aizome/crates/aizome-network-agent/tests/`).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-network-agent --test repro_azn_04 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** `RegisterWithCapabilities calls over 7.008760367s: 2; first seen at [(0, 1.6µs), (1, 10.056387ms), (2, 2.002307474s)]` → `assertion left == right failed: after two "already registered" refusals the agent stopped asking … left: 2 right: 3`
- **Notes:** a fix that sets `asked` only for a lost reply (not a refusal) keeps the existing "timed out, then refused" test passing.
- **Files:** `desktop/aizome/crates/aizome-network-agent/tests/repro_azn_04.rs`

<a id="azn-13"></a>

### AZN-13 — Confirmed

Catalog entry: [AZN-13](aizome-small-nix.md#azn-13), Time-zone ownership across activations (saved zone file and `/etc/localtime` link).

- **Claim:** with Aizome enabled, a declared `time.timeZone` is overridden at priority 90 and never used as the starting zone. A fresh system installed without Genkan (no `/etc/localtime` link, no saved zone) runs in UTC.
- **Level:** unit (evaluation of the module, then its own activation snippets run against an empty root)
- **Method:** `nix eval` (no build) of a minimal NixOS configuration: Sekka's pinned Nixpkgs, fetched `--offline`; the worktree's `desktop/aizome/nix/module.nix`; `programs.aizome.enable = true; time.timeZone = "Europe/Paris"`. A control adds `mutableTimeZone = false`. The script then runs the evaluated `aizomeTimeZoneSave` and `aizomeTimeZoneRestore` text, in their dependency order, against a temporary root standing for a fresh target. `/etc/localtime`, `/etc/zoneinfo/` and `/var/lib/aizome` are rewritten into that root, whose `etc/zoneinfo` is the tzdata link the `etc` activation makes. It asserts that `/etc/localtime` ends up pointing at Europe/Paris. Files: `repro_azn_13.sh` (runs from anywhere).
- **Command:** `bash reproductions/support/azn-13/repro_azn_13.sh` (in `nix develop`)
- **Observed:** `time.timeZone = null  environment.etc ? localtime = false  AIZOME_TIMEZONE_MANAGED = "0"` · `activation deps: save=[] etc=[…,"aizomeTimeZoneSave"] restore=["etc"]` · control: `time.timeZone = "Europe/Paris"  etc.localtime -> "/etc/zoneinfo/Europe/Paris"` · `fresh install, after activation: no /etc/localtime (glibc and systemd use UTC); saved zone: none` → `FAIL: the declared time.timeZone (Europe/Paris) was discarded on a fresh system`
- **Notes:** 
  - The override itself is intended: `timezone-policy-test.nix` asserts the `"0"` policy for exactly this configuration. The defect is that the declared zone isn't even the starting zone on a system that never had NixOS's link, which `module.nix:256–259` implies it should be ("starts from the one the system is set to now").
  - The snippet run rewrites paths into a temporary root; it is not a real activation. A VM `nixos-install` of this configuration would confirm it end to end (`readlink /etc/localtime` and `timedatectl` after first boot).
- **Support files:** [`repro_azn_13.sh`](reproductions/support/azn-13/repro_azn_13.sh)
- **Independent review:** upheld with caveat; re-run: Fails: `time.timeZone = null`, no `/etc/localtime`, saved zone none; asserts the claim: Yes; reachability: real: evaluation of the real module, then its own activation snippets on an empty root. The override is intended (`timezone-policy-test.nix`). The defect is narrower: no starting zone on non-Genkan installs

<a id="sya-05"></a>

### SYA-05 — Confirmed

Catalog entry: [SYA-05](sys-a.md#sya-05), Optimistic audio levels awaiting server confirmation.

- **Claim:** `set_volume_in` stores a new level before the server answers. When the server refuses, nothing restores the old level and no change event follows, so the snapshot keeps a level the server never applied.
- **Level:** integration (the real `Audio` service against a private PipeWire with its own pipewire-pulse: null sink, no session manager, no hardware)
- **Method:** The test sets the speakers to 0.5 and waits for confirmation. With `pw-cli permissions`, it gives this client's PipeWire client (application.name "Aizome") no access to the speakers, then `r-x` (read and execute, no write), as a sandbox's permission store would. The sink drops out of the snapshot and comes back. `set_sink_volume(0.2)` is then refused. A second `Audio` instance reads the server's real level. After 1.5 s, past the expectation window, the test asserts that the first snapshot matches the server. Files: `repro_sya_05.rs` (goes in `crates/aizome-sys/tests/`)
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-sys --test repro_sya_05` (in `nix develop .#aizome`)
- **Observed:** `15: r-x--` permissions; `set_sink_volume(0.2): Err(NotAuthorized("Access denied"))`; `server level 0.5, snapshot level 0.19999695`, which fails `after a refused change the snapshot must show the level the server kept (0.5)`.
- **Notes:** The refusal comes from PipeWire permissions on a sink, not from a sink input with `volume_writable` false as in the entry. Both go through the same `set_volume_in` and `finish` path (every `Kind`). The level stays wrong until some later event for that object. Not tested: the mute path, the optimistic default (which has no `Expect`), Balance's dropped `done`, and lazy expiry.
- **Files:** `desktop/aizome/crates/aizome-sys/tests/repro_sya_05.rs`

<a id="sya-06"></a>

### SYA-06 — Confirmed

Catalog entry: [SYA-06](sys-a.md#sya-06), Bluetooth pairing-agent registration worker.

- **Claim:** BlueZ forgets agents when bluetoothd restarts, but the agent worker keeps its `Registration` and nothing registers again. Pairing a device that needs a confirmation then fails with no prompt.
- **Level:** integration (the real `Bluetooth` service and agent worker against a fake BlueZ on a private `dbus-daemon`, adapted from `tests/bluetooth.rs`)
- **Method:** The test registers the agent with fake bluetoothd #1 and checks that it's registered as the default agent. It drops #1's connection (BlueZ vanishes, `available` goes false), then starts a fresh #2 that knows no agents and waits until the service sees it again. It asserts that #2 eventually has an agent and that `pair(SPEAKER)`, which asks the agent for `RequestConfirmation(123456)`, succeeds after prompting. Files: `repro_sya_06.rs` (goes in `crates/aizome-sys/tests/`)
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-sys --test repro_sya_06` (in `nix develop .#aizome`)
- **Observed:** `agent registered with the new bluetoothd: false; pair: Err(Rejected("no agent")); prompts: []`, which fails `the agent must be registered with the bluetoothd that runs now`.
- **Notes:** The Settei half was checked by reading only (the pane wasn't driven). `panes/bluetooth.rs` registers only while `!self.agent`, and resets `agent` only on hide or a failed registration, so it doesn't recover while shown. The test asserts the fix in aizome-sys (re-register on `Appeared`). A fix in Settei instead (re-register when BlueZ reappears) would need a pane-level test. Not tested: a failed re-Register leaving no agent, and the cost of `Drop` blocking the worker.
- **Files:** `desktop/aizome/crates/aizome-sys/tests/repro_sya_06.rs`
- **Independent review:** upheld; re-run: FAIL as reported: no agent on the new bluetoothd; `Rejected("no agent")`, no prompts; asserts the claim: Yes (the test assumes the fix is in aizome-sys); reachability: real (a bluetoothd restart forgets agents). I checked the Settei half: `agent` resets only on hide or a failed registration (panes/bluetooth.rs:453, :490, :621).

<a id="sya-13"></a>

### SYA-13 — Confirmed

Catalog entry: [SYA-13](sys-a.md#sya-13), Destructive-operation confirmation token.

- **Claim:** a `Confirm<NewPartitionTable>` made for USB stick A still passes `check` after A is swapped for a same-size partitioned stick B at the same kernel name. The fingerprint has no drive identity and no partition-table identity, and a whole partitioned disk has empty `IdType`/`IdUUID`, so B would be wiped.
- **Level:** unit (`Confirm::check` against fake `DisksState` records; no bus, no disks)
- **Method:** Sticks A and B are both `/org/freedesktop/UDisks2/block_devices/sdb`, `/dev/sdb`, 32 017 047 552 bytes, with empty id fields and no `Partition`. They differ in drive (`SanDisk_Ultra_<serial>`, different serials) and partition table (gpt vs dos). The test makes `Confirm::<NewPartitionTable>::new(&a)`, checks it passes against A, and asserts that `check` fails against B. Files: `repro_sya_13.rs` (goes in `crates/aizome-sys/src/disks/`), plus a one-line `#[path]` mod at the end of `src/disks/confirm.rs` (`INSERT.txt`)
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-sys --lib repro_sya_13` (in `nix develop .#aizome`)
- **Observed:** `check against B: Ok("/dev/sdb")`, which fails `the confirmation given for stick A must not let stick B be wiped`.
- **Notes:** The entry says no app makes these tokens yet. Tansu does: `apps/tansu/src/run.rs` `Run::agree` makes every operation's token from the state when the person applies. A swap therefore has to happen between Apply and the operation running (or between the sheet's state and Apply, if that state is stale). Not tested: the check-then-act window between `check` and the UDisks call, and the monotonic `Instant` not counting suspend.
- **Files:** `desktop/aizome/crates/aizome-sys/src/disks/repro_sya_13.rs`
- **Module line added to:** `desktop/aizome/crates/aizome-sys/src/disks/confirm.rs`

<a id="sya-17"></a>

### SYA-17 — Partly confirmed

Catalog entry: [SYA-17](sys-a.md#sya-17), Level meter thread lifecycle.

- **Claim:** every error exit of the level meter's thread leaves `level` at the last peak. The thread never reconnects, and the owner can't tell it's dead, so when pipewire-pulse restarts, Settei's Sound pane shows a frozen input level and the meter never follows the input again.
- **Level:** integration (the real `Meter` thread against a private PipeWire with its own pipewire-pulse; the last peak is injected)
- **Method:** (1) The test starts `Meter::start_on(server, "test-mic")` and waits until its PipeWire client ("Aizome level meter") exists. It stores 0.5 in `meter.level`, the atomic the read callback writes peaks to, then kills pipewire-pulse and asserts that `level()` goes to 0 within 3 s. (2) It starts a meter, kills pipewire-pulse and checks that the meter's client went, restarts pipewire-pulse on the same socket, and asserts that the meter connects again within 8 s. Files: `repro_sya_17.rs` (goes in `crates/aizome-sys/src/`), plus a one-line `#[path]` mod at the end of `src/meter.rs` (`INSERT.txt`)
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-sys --lib repro_sya_17` (in `nix develop .#aizome`)
- **Observed:** (1) `3 s after pipewire-pulse went the meter shows 0.5`, which fails `a meter whose server left shows no level, not the last peak (0.5)`; (2) `meter reconnected within 8 s: false`
- **Notes:** Without a session manager nothing links or drives the private graph, so the server sends no real peaks, and the "last peak" had to be put in the atomic by hand. Only the pipewire-pulse restart branch was reproduced. The other branch is not reproduced (environment): the source disappearing under `DONT_MOVE` while the context stays `Ready`. It needs a session manager (a private WirePlumber with its ALSA and BlueZ monitors off) to link the stream, then the source removed and re-created. Settei's part was checked by reading only: it polls `level()` and makes a new meter only when the default input's name changes (`panes/sound.rs:208-216`).
- **Files:** `desktop/aizome/crates/aizome-sys/src/repro_sya_17.rs`
- **Module line added to:** `desktop/aizome/crates/aizome-sys/src/meter.rs`
- **Review override:** Downgraded by the independent review: the meter's own defect (keeps the last peak, never reconnects) is real, but Settei's 80 ms poll sees the default input disappear during pipewire-pulse's 2 s reconnect pause and replaces the meter, so no frozen level is shown. The reproducer's verdict was Confirmed.
- **Independent review:** overturned; re-run: FAIL ×2 as reported: level stays 0.5; no reconnect in 8 s; asserts the claim: Partly. It asserts the `Meter` contract, not what Settei shows.; reachability: unreachable as user-visible. On a pipewire-pulse restart the audio service publishes `AudioState::default()` and waits 2 s before reconnecting (audio.rs:935-946). Settei polls every 80 ms (sound.rs:30, :478-486), sees `default_input()` = None, drops the meter (`follow_input`, :215), then builds a new one when the input returns. A removed source empties `default_input()` the same way.. The Meter-level defect (no zeroing, no reconnect) is real but latent. Its only owner discards and recreates it, so no frozen level is shown. The report's "by reading" check missed the None transition.

<a id="syb-01"></a>

### SYB-01 — Confirmed

Catalog entry: [SYB-01](sys-b.md#syb-01), Airplane mode across rfkill, NetworkManager, BlueZ and a marker file.

- **Claim:** after airplane mode's fallback, turning Wi-Fi back on with its switch leaves the marker behind. A later airplane-on rewrites it as "" (hci0 is still off), so airplane-off never powers Bluetooth back on, and the stale marker makes a plain Wi-Fi-off read as airplane mode.
- **Level:** integration (`Network::with_bus` against a fake NetworkManager and a fake BlueZ on a private dbus-daemon, with temporary files: a 0444 file standing in for `/dev/rfkill` (so the fallback runs), a fake sysfs with an unblocked wlan radio and an unblocked bluetooth radio, and a marker in a temp dir; never real rfkill, NM or BlueZ)
- **Method:** two tests. (a) Airplane on, then Wi-Fi on by hand, then Wi-Fi off by hand: asserts `airplane_mode` is false. (b) Airplane on, Wi-Fi on by hand, airplane on, airplane off: asserts hci0 is powered again. Files: `repro_syb_01.rs` (aizome-sys `tests/`)
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-sys --test repro_syb_01` (in `nix develop .#aizome`)
- **Observed:** (a) `plain Wi-Fi off reads as airplane mode: airplane_mode=true, radios=[… Wlan, soft_blocked: false …, … Bluetooth, soft_blocked: false …], marker left behind=Some("/org/bluez/hci0")`. (b) `Bluetooth stays off after airplane mode ends: the marker was rewritten as Some("") by the second airplane mode (hci0 was still off)`
- **Notes:** (a) needs rfkill radios to be visible. Without any, `build` already counts Wi-Fi-off plus WWAN-off as airplane mode (`model.rs:881`-`885`), marker or not. (b)'s expectation, that Bluetooth comes back because the first airplane mode turned it off, is the entry's reading. The person never turned it back on themselves.
- **Files:** `desktop/aizome/crates/aizome-sys/tests/repro_syb_01.rs`

<a id="syb-02"></a>

### SYB-02 — Confirmed

Catalog entry: [SYB-02](sys-b.md#syb-02), Wi-Fi join / activation transaction with in-memory edit and rollback.

- **Claim:** joining a saved network with a new password that takes longer than 45 s: `wait_active` returns Timeout and `rejoin_with` reloads the old profile from disk, but NM's activation with the in-memory settings is left running, so it may connect while the UI says it failed and the working password is never saved.
- **Level:** integration (`Network::connect` against a fake NetworkManager whose activation stays in state 1, "activating", as with slow DHCP; about 46 s, because `JOIN_TIMEOUT` is a constant)
- **Method:** join "Home" (saved, `Filename` set) with a new password. Records Update2, Save, LoadConnections and DeactivateConnection. Asserts that after a failed join the activation was deactivated (not left running with the rolled-back password). Files: `repro_syb_02.rs`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-sys --test repro_syb_02 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** `join: Err(Timeout) after 45.027922359s; Update2 [(Some("new-password"), 2)]; saved 0; LoadConnections [["/etc/NetworkManager/system-connections/Home.nmconnection"]]; activations [".../ActiveConnection/1"]; deactivated []; activation still running: true`
- **Notes:** the Aizome side reproduces exactly: Timeout is reported, the disk profile is reloaded, nothing is saved, and the activation is never deactivated. That NM then completes on its applied (in-memory) connection is NetworkManager's own behaviour. The fake models it; it was not observed with a real NM, which would need a VM with NM and an access point with slow DHCP. Plain saved joins (`activate_and_wait` alone) also leave the activation running on timeout. `add_and_wait` instead deletes the profile on timeout.
- **Files:** `desktop/aizome/crates/aizome-sys/tests/repro_syb_02.rs`

<a id="syb-09"></a>

### SYB-09 — Confirmed

Catalog entry: [SYB-09](sys-b.md#syb-09), polkit authentication session and helper conversation.

- **Claim:** when the helper can't be reached (connect error) or exits without SUCCESS/FAILURE (EOF), Failed is emitted at once. The shell's Failed handler calls `authenticate` again immediately, so the dialog spins, spawning helpers or logging warnings as fast as the loop runs.
- **Level:** integration (aizome-sys `PolkitAgent::register_with` on a private dbus-daemon with a fake polkitd; fake helpers: a stub `polkit-agent-helper-1` script in a temp dir that reads the cookie, appends a line to a counter file and exits 1 without output, or `Helper::Socket` at a path that doesn't exist; never the real polkitd or helper)
- **Method:** a driver does what the shell's handler does (`authenticate` on Begin and again on every Failed, through a queue as the shell's message loop does) for 2 s or 200 failures, then cancels. Asserts at most 3 helper starts (EOF case) and at most 3 failed attempts (socket case) with no one typing. Files: `repro_syb_09.rs`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-sys --test repro_syb_09` (in `nix develop .#aizome`)
- **Observed:** EOF: `200 helper processes started in 273.332047ms with no one typing (200 Failed, 0 prompts)`. Socket: `200 connection attempts failed in 1.421281ms with no one typing (errors shown: ["Not found: No such file or directory (os error 2)", …])` (200 is the test's cap)
- **Notes:** with a real setuid helper, each spin starts a root process (PAM included). On the connect-error path, Error and Failed are delivered from inside `authenticate` on the caller's thread. A UI that retried directly in the callback, rather than through a queue, would recurse until the stack overflows. This test asserts on the aizome-sys side; SHB-03 is the same loop through the real shell handler.
- **Files:** `desktop/aizome/crates/aizome-sys/tests/repro_syb_09.rs`
- **Independent review:** upheld with caveat; re-run: Both fail: 200 helpers in 341 ms; 200 connect failures in 1.2 ms; asserts the claim: Yes; reachability: staged: a stub helper that exits silently, or a missing socket; a driver plays the shell's handler. Same loop as SHB-03, from the aizome-sys side. Not an independent second bug

<a id="syb-12"></a>

### SYB-12 — Confirmed

Catalog entry: [SYB-12](sys-b.md#syb-12), logind sleep-delay inhibitor and lock-before-sleep handshake.

- **Claim:** if the lock isn't confirmed before logind's delay limit and the machine sleeps with `sleep_pending` set, the delay is retaken on resume. `lock_confirmed` then calls `sleep_ready()` and drops that fresh inhibitor, so the next suspend isn't delayed and can sleep before the screen locks.
- **Level:** integration (the real shell, headless, with the real `aizome_sys::power::Logind` follower in `Shell.sys.logind`, against a fake logind on a private dbus-daemon whose `Inhibit` hands out pipe write ends; `AIZOME_SESSION=1` is set in the test process so `session_events` runs; nothing suspends: "sleep" is the fake emitting `PrepareForSleep(true)` then `(false)`)
- **Method:** take the delay as `start_lock` does; PrepareForSleep(true) → the shell locks with `sleep_pending`; the lock isn't confirmed; PrepareForSleep(false) → the shell's Resumed handler takes the delay again; then the compositor confirms the lock (`h.confirm_lock()`). Asserts some inhibitor is still held for the next sleep. Files: `repro_syb_12.rs` (aizome-shell `tests/`)
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shell --test repro_syb_12` (in `nix develop .#aizome`)
- **Observed:** `no sleep delay is held for the next sleep: inhibitors taken 2, held now [false, false], LogindState.sleep_delay_held false, sleep_pending false`
- **Notes:** in this order the follower doesn't retake the delay on resume, because the first fd is still held. The shell's `take_sleep_delay` replaces it, and `lock_confirmed` releases the replacement. If the compositor confirms the lock before the Resumed event is handled instead, the follower retakes the delay and all is well. So the bug needs the lock confirmation to arrive after resume, which is likely when the compositor was frozen mid-lock. Exactly one sleep is unprotected: the resume after it takes the delay again.
- **Files:** `desktop/aizome/crates/aizome-shell/tests/repro_syb_12.rs`

<a id="syc-01"></a>

### SYC-01 — Confirmed

Catalog entry: [SYC-01](sys-c-config.md#syc-01), Keyboard settings: inherited session defaults vs saved choices.

- **Claim:** an unrelated save (the shell saving its Deskbar section) writes every `Keyboard` field, pinning the session's `XKB_DEFAULT_*` layout, so a later system layout change never reaches the session.
- **Level:** integration (real `aizome-config` load/save in subprocesses, each with a private `XDG_CONFIG_HOME` under the wrapper's TMPDIR and its own `XKB_DEFAULT_*`)
- **Method:** Session 1 runs with `XKB_DEFAULT_LAYOUT=us`. It does what `Shell::new` and `save_section("deskbar")` do: `load_or_default`, `set_shell_section("deskbar", collapsed)`, `save()`. Session 2 runs with `XKB_DEFAULT_LAYOUT=de`, and the test asserts `load_or_default().input.keyboard.layouts == ["de"]`. File: `repro_syc_01.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-config --test repro_syc_01 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** `the session's XKB_DEFAULT_LAYOUT=de is ignored: the Deskbar save pinned ["us"]` (`left: ["us"] right: ["de"]`). The saved file holds a full `[input.keyboard]` with `layouts = ["us"]`, `variant = ""`, `options = ""` and `model = ""`.
- **Notes:** The real `~/.config/aizome` was never touched. The shell's own `Shell` struct wasn't built; the test makes the same `aizome_config` calls in the same order.
- **Files:** `desktop/aizome/crates/aizome-config/tests/repro_syc_01.rs`
- **Independent review:** upheld; re-run: Fails: `XKB_DEFAULT_LAYOUT=de is ignored … pinned ["us"]`; asserts the claim: Yes; reachability: real: the same `aizome_config` calls `save_section("deskbar")` makes. `Settings::save` serialises every `Keyboard` field filled by serde defaults

<a id="syc-02"></a>

### SYC-02 — Confirmed

Catalog entry: [SYC-02](sys-c-config.md#syc-02), One-shot migration of the legacy `[keys] terminal` key.

- **Claim:** any save drops the `skip_serializing` legacy `[keys] terminal` key before the migration has succeeded, so the retried migration reads `None` and ❖Return starts Hotaru instead of the person's terminal.
- **Level:** integration (real `aizome-config` in subprocesses with a private `XDG_CONFIG_HOME`)
- **Method:** The test writes `settings.toml` with `[keys] terminal = ["kitty", "--single-instance"]`. In the first session, `load_or_default` sees the key, then the shell's Deskbar save runs. In the next session the test asserts that `keys.legacy_terminal` still holds the choice, since no migration marker exists. File: `repro_syc_02.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-config --test repro_syc_02 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** `the retry of the failed migration gets None: the shell's Deskbar save dropped `[keys] terminal`` (`left: None right: Some(["kitty", "--single-instance"])`)
- **Notes:** The precondition was not reproduced: the first migration failing in `with_index` / `set_role_default`, for example with a read-only `xdg-terminals.list`. That part is assumed; the key loss it leads to is shown directly. The loss happens on any save before a successful migration, whatever the order with the compositor.
- **Files:** `desktop/aizome/crates/aizome-config/tests/repro_syc_02.rs`

<a id="syc-03"></a>

### SYC-03 — Confirmed

Catalog entry: [SYC-03](sys-c-config.md#syc-03), Settings file lifecycle: load, fallback to defaults, atomic save, cross-process writers.

- **Claim:** (1) `load_or_default` turns a broken file into the defaults, and the next shell save writes them over everything. (2) Writers sharing the fixed `settings.toml.tmp` truncate each other's temp file: a rename publishes torn text and the other rename fails with ENOENT.
- **Level:** integration (real `Settings` load/save in subprocesses with a private `XDG_CONFIG_HOME`)
- **Method:** Three tests, file `repro_syc_03.rs`:
  1. The test writes personal settings (a binding, pinned apps), then a hand edit with an unquoted string. It runs the shell's reload (`load_or_default`) and a Deskbar save, then asserts the binding and pins survive.
  2. A step-by-step interleaving. Settei's `write_atomically` is modelled as its two halves: `File::create(tmp)` (O_TRUNC), the write, then `rename`. The shell's real `Settings::save_to` runs between Settei's open and its write. The test asserts Settei's rename succeeds and the published file parses to one whole save.
  3. No stand-in: two threads make 2000 real `save_to` calls each, while a reader re-parses the file. The test asserts no failed saves and no torn reads. Each save is first checked to read back as itself.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-config --test repro_syc_03 --no-fail-fast -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** 
  1. `the person's bindings and pinned apps were replaced by the defaults`: the saved file has an empty `[keys.bindings]` and `pinned = []`.
  2. `Settei's rename: Err(Os { code: 2, kind: NotFound, ... }); the published file (2688 bytes) parses: false`. The file is Settei's complete text followed by the tail of the shell's (`key with no value, expected '='`).
  3. `4000 saves: 825 failed; reads that saw neither save whole: 6202`. An earlier run gave 802 and 4340.
- **Notes:** All three fail. The missing fsync and the stale-copy last-writer-wins case were not tested separately; the latter is the same mechanism as part 1. The race in part 3 is nondeterministic, but it failed on every run, with hundreds of collisions.
- **Files:** `desktop/aizome/crates/aizome-config/tests/repro_syc_03.rs`

<a id="syc-06"></a>

### SYC-06 — Partly confirmed

Catalog entry: [SYC-06](sys-c-config.md#syc-06), Drawn element press, drag, drag-out and hover gesture.

- **Claim:** when the compositor takes the pointer mid-drag, Drawn resets its press silently at `drawn.rs:376` with no `DragPhase::End`, so `desktop.band` stays `Some` (the band stays drawn) and the Deskbar's app drag holds it open.
- **Level:** integration (the real `drawn` element in the kit's headless harness; the consumers' drag logic copied verbatim from `desktop_glue.rs:224-233` and `deskbar_glue.rs:510-535`)
- **Method:** Both tests press, drag, `leave` (the compositor's grab), then `enter` and move with no button down, and assert the consumer's drag state is cleared. The band test uses a non-drag-only element, like the desktop and the Deskbar app rows. The edge test uses a `drags_only` element, like the Deskbar's resize edge. File: `repro_syc_06.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-face --test repro_syc_06 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** 
  - Band: `the band is still drawn after the pointer came back with no button down; phases heard: [Start((50.0, 50.0)), Move((110.0, 90.0)), Move((200.0, 200.0)), Move((210.0, 200.0))]`
  - Edge: `the resize is still under way: Drawn reset its press without a phase; phases heard: [Start((3.0, 100.0)), Move((33.0, 100.0))]`
- **Notes:** What the user sees is confirmed: the consumer is left mid-drag in both cases. The mechanism the entry gives holds only for drag-only elements. The kit lets go of a press on leave only when the active widget is `drags_only` (`aizome-kit/src/engine.rs:2065-2072`). The desktop band and the Deskbar app rows are not drag-only, so for them there is no silent reset: the drag goes on, sending `Move` with no button down, and the band follows the pointer until the next click's release sends `End`. The silent reset at `drawn.rs:374-377` happens for the Deskbar edge, whose `deskbar_resize` stays `Some`. The existing test at `drawn.rs:615` asserts exactly this no-phase behaviour.
- **Files:** `desktop/aizome/crates/aizome-face/tests/repro_syc_06.rs`

<a id="syc-07"></a>

### SYC-07 — Confirmed

Catalog entry: [SYC-07](sys-c-config.md#syc-07), Text field caret, selection, IME preedit, blink and Caps Lock state.

- **Claim:** in a `secure` field the IME preedit is drawn unmasked (`all = shown(value) + preedit`), so characters being composed in the lock-screen, polkit and greeter password fields show in clear text.
- **Level:** integration (widget level: the real `field` widget, styled as the lock screen's, rendered in the kit's headless harness)
- **Method:** The test renders, at 2×, a focused secure field holding "pa" while an IME composes "WW". It renders the same field style, not secure, showing "••WW", and the secure field with no preedit. It asserts the composing render differs from the clear-text one. File: `repro_syc_07.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-face --test repro_syc_07 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** `pixels differing from the clear-text field: 0; from the field with no preedit: 781` → `the secure field composing "WW" draws exactly what a plain field showing "••WW" draws: the composed text is in clear`
- **Notes:** The renders are pixel for pixel the same. The field also enables text input with no purpose hint (`ContentPurpose::Normal`, `sidestep-engine/src/backend/textinput.rs:121`), so the IME doesn't know it is a password field. Not run with a real fcitx5 or ibus.
- **Files:** `desktop/aizome/crates/aizome-face/tests/repro_syc_07.rs`
- **Independent review:** upheld; re-run: Fails: 0 pixels differ from a plain "••WW" field; asserts the claim: Yes; reachability: real: IME preedit through the harness; Aizome implements input-method-v2/text-input. `all = shown(value) + preedit` in secure mode; no password purpose hint

<a id="kit-04"></a>

### KIT-04 — Confirmed

Catalog entry: [KIT-04](kit-core.md#kit-04), Menu publication to the shell (dedup and debounce).

- **Claim:** a shell `Activate{id}` for a menu item the app has disabled since its last publication (within the 40 ms debounce) runs the item anyway, because `shell::find` returns disabled actions and `Core::perform` never checks `enabled`.
- **Level:** integration (real `Runtime` and `ShellLink` over a Unix socket; the test plays the shell)
- **Method:** `Harness::with_shell` with a program whose File ▸ Save is `.enabled(self.dirty)`. Edit, wait for the published menus (Save enabled), then revert (Save disabled in the app, republish still debounced) and send `Activate{save.id}` from the fake shell. The test asserts the program never hears `Save`. File `repro_kit_04.rs` (`tests/`).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-kit --test repro_kit_04 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** `log after Activate of the disabled Save: ["save"]` → `a disabled item must not run when the shell activates it: log ["save"]`
- **Notes:** the debounce isn't needed to hit this: any `Activate` for an id whose action is now disabled runs it. Keyboard shortcuts (`find_shortcut`) and in-window menus (`activate_row`) both check `enabled`, so only the shell path skips the check. I did not exercise the separate hazard where a failed send is not retried.
- **Files:** `desktop/aizome/crates/aizome-kit/tests/repro_kit_04.rs`
- **Independent review:** upheld with caveat; re-run: Fails: `log ["save"]`; asserts the claim: Yes; reachability: staged: Activate sent inside the 40 ms republish debounce; a real stale shell copy gives the same. `shell::find` returns disabled items and `perform` never checks `enabled`. Needs a stale shell menu

<a id="kit-07"></a>

### KIT-07 — Confirmed

Catalog entry: [KIT-07](kit-core.md#kit-07), File panel lifecycle and browsing state.

- **Claim:** `Cx::close_window` on a window whose save panel is up leaves the `PanelState` in `Core.panels` for good, and the panel's `done` never fires, so the program never hears `Saved(None)`.
- **Level:** integration (kit `Harness`; the module is in-crate only to read `Core.panels`)
- **Method:** two windows, `save_panel` on the second, then the program closes that window. The test asserts the completion arrives as `None` and that `Core.panels` is empty. File `repro_kit_07.rs`, mod line in `INSERT.txt`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-kit --lib repro_kit_07 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** `panels left: 1, completions heard: []`; `assertion left == right failed: the panel's done callback must fire (as a cancel) when its window closes  left: []  right: [None]`
- **Notes:** `close_win` drops the window's alerts but not its panels (`engine.rs:628-630`). Whichever fix is chosen (cancel the panel, or refuse the close), the leak assertion stays valid.
- **Files:** `desktop/aizome/crates/aizome-kit/src/repro_kit_07.rs`
- **Module line added to:** `desktop/aizome/crates/aizome-kit/src/testing.rs`

<a id="kit-13"></a>

### KIT-13 — Confirmed

Catalog entry: [KIT-13](kit-core.md#kit-13), Pointer hover, press and capture.

- **Claim:** after a program-started drag (`Cx::start_drag` from the source's press-and-move messages, as Hako does), `Win.active` still names the drag source. Later moves go to it, and the next click's release reaches it instead of the clicked control.
- **Level:** integration (kit `Harness`, real engine pointer routing, the stand-in platform's native drag)
- **Method:** a `mouse_area(..).on_drag` whose `Move` handler calls `cx.start_drag`, next to a button. Press and move start the drag; the pointer leaves, `end_drag(Copy)`, re-enters over the button and clicks it. The test asserts the source hears nothing more and the button is pressed. File `repro_kit_13.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-kit --test repro_kit_13 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** `other pressed: 0, drag events after the drop: ["Move { start: (55.0, 37.0), at: (182.0, 37.0), .. }", "Move { .. }", "End { start: (55.0, 37.0), at: (182.0, 37.0) }"]`: the click was lost to the stale drag source.
- **Notes:** the widget path (`EventOut.drag`, `engine.rs:1633-1638`) clears `active`; `Core::start_drag` from `Cx` does not. The mechanism matches Hako's `drag_out` (`apps/hako/src/ui/actions.rs:220`), but I did not run Hako itself.
- **Files:** `desktop/aizome/crates/aizome-kit/tests/repro_kit_13.rs`

<a id="kit-17"></a>

### KIT-17 — Confirmed

Catalog entry: [KIT-17](kit-core.md#kit-17), Tooltip delay, show and dismissal.

- **Claim:** (1) hovering a button and clicking it within 650 ms still shows its tooltip after the click; (2) each pointer leave with a tooltip pending leaks one `TimerUse::Tooltip` in `Core.timers`.
- **Level:** integration (kit `Harness`; in-crate to count `Core.timers`)
- **Method:** (1) hover, wait 300 ms, click, wait 400 ms; the test asserts no tooltip popup. (2) hover for 100 ms then leave the window, five times; the test asserts no `TimerUse::Tooltip` is left. File `repro_kit_17.rs`, mod line in `INSERT.txt`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-kit --lib repro_kit_17 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** (1) `tooltip popups 400 ms after the click: 1`; (2) `platform timers pending: 0, TimerUse::Tooltip entries in Core.timers: 5` (`left: 5 right: 0`)
- **Notes:** the leak only grows memory: `Menus` debounce lookups don't match `Tooltip` entries, so nothing else misbehaves. The existing test `tests/tooltip_refresh.rs` treats click dismissal as intended for shown tooltips.
- **Files:** `desktop/aizome/crates/aizome-kit/src/repro_kit_17.rs`
- **Module line added to:** `desktop/aizome/crates/aizome-kit/src/testing.rs`
- **Independent review:** upheld; re-run: Both fail: 1 tooltip popup after the click; 5 `TimerUse::Tooltip` leaked; asserts the claim: Yes; reachability: real: move, click, leave. Leak is memory-only and tiny; click-dismiss semantics follow the existing shown-tooltip test

<a id="kit-29"></a>

### KIT-29 — Confirmed

Catalog entry: [KIT-29](kit-core.md#kit-29), Node arena: liveness, layout cache and per-layout outboxes.

- **Claim:** a relayout that compacts the arena (`engine.rs:907-912`) leaves `local_damage`/`scrolls` holding pre-compaction indices of nodes mounted during that layout. `take_local_damage` then indexes past the end and panics (or lands damage on another node).
- **Level:** integration (kit `Harness`, real `virtual_list`, `text_editor`, relayout, compaction and settle)
- **Method:** (a) real widgets: a `virtual_list` of 50 rows, each a `text_editor` of 400 lines. The test wheel-scrolls inside the first row's editor 80 times and checks each frame with `check_frame`. Each editor scroll relayouts, the list remounts its rows, and the editor's layout pushes `scrolled` and `invalidate_rect`. (b) mechanism: 2-pt rows of a public-API `Widget` that calls `cx.invalidate_rect` in `layout`, while the list itself scrolls. Both assert a correct frame (no panic). File `repro_kit_29.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-kit --test repro_kit_29 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** (a) `panicked at crates/aizome-kit/src/tree.rs:686:41: index out of bounds: the len is 12 but the index is 268`; (b) `… the len is 304 but the index is 604`
- **Notes:** (a) needs only ~33 wheel events over an editor in a virtual row (about 8 rows visible), with no program message in between, since a rebuild builds a fresh tree. Stale indices below the live count would misplace damage silently instead (`take_scrolls` uses `.get`, so it skips rather than panics).
- **Files:** `desktop/aizome/crates/aizome-kit/tests/repro_kit_29.rs`

<a id="uiw-01"></a>

### UIW-01 — Confirmed

Catalog entry: [UIW-01](sukumo-ui-kit-widgets.md#uiw-01), Press-then-release click pairing in buttons, switches, checkboxes, radios and segmented controls.

- **Claim:** a press on inert background followed by a release over a button, switch, checkbox or segmented control fires it, though it was never pressed.
- **Level:** integration (kit `Harness`, real controls and engine routing)
- **Method:** for each control: press at an empty spot of the window, move over the control (the second segment for `segmented`), release. The test asserts nothing fired. File `repro_uiw_01.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-kit --test repro_uiw_01 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** `controls that fired on a release with no press: [("button", ["Button"]), ("toggle", ["Toggle(true)"]), ("check", ["Check(true)"]), ("seg", ["Segment(1)"])]`
- **Notes:** radios share `Check::event` (not separately driven).
- **Files:** `desktop/aizome/crates/aizome-kit/tests/repro_uiw_01.rs`

<a id="uiw-08"></a>

### UIW-08 — Confirmed

Catalog entry: [UIW-08](sukumo-ui-kit-widgets.md#uiw-08), Menu interaction: hover, open submenu, pointer aim, type-to-filter, strip tear-off.

- **Claim:** (1) a menu drawn in a window (torn-off palette, `menu_view`) whose items shrink under a hovered row past the new end panics on the next pointer event; (2) after ← closes a submenu, the parent's `open_sub` still names it. Hovering that row doesn't reopen it, and rightward moves still count as aiming.
- **Level:** integration (kit `Harness`; in-crate only to deliver pointer moves to a popup, which `Harness` has no public call for)
- **Method:** (1) `menu_view` of 10 items in a window: hover row 9, the program shrinks it to 3 items, move over row 0. (2) a context menu Alpha / More ▸ / Omega: ↓↓ → opens More, ← in it closes it, then the pointer comes over More. The test asserts its submenu reopens. (3) the pointer opens More by hovering, ← closes it, the pointer moves right-down onto Omega, Return. The test asserts Omega is chosen. File `repro_uiw_08.rs`, mod line in `INSERT.txt`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-kit --lib repro_uiw_08 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** (1) `panicked at crates/aizome-kit/src/widgets/menus.rs:423:27: index out of bounds: the len is 3 but the index is 9` (backtrace: `pointer_move → MenuView::event → set_hover → row_rect`); (2) `popups after hovering More again: 1` (`left: 1 right: 2`); (3) `chosen: [], popups: 2`: Return reopened More, because the hover was held back by the 220 ms aim.
- **Notes:** the shell's tear-off renders the same `menu_view`, but I did not run the shell. The Enter/→ `rows[i]` indexing (`menus.rs:493`, `:930`) is the same stale `hover` and was not separately driven.
- **Files:** `desktop/aizome/crates/aizome-kit/src/repro_uiw_08.rs`
- **Module line added to:** `desktop/aizome/crates/aizome-kit/src/testing.rs`
- **Independent review:** upheld with caveat; re-run: All 3 fail: panic `len is 3 but the index is 9`; popups 1 not 2; `chosen []`; asserts the claim: Yes; reachability: staged: (1) a test program shrinks a `menu_view` (the shell's tear-off renders live app menus the same way); (2) real keys and pointer; (3) Return within the 220 ms aim. (2) is a lasting bug. (3) lasts only the aim window. (1) needs a dynamic menu torn off

<a id="uiw-09"></a>

### UIW-09 — Confirmed

Catalog entry: [UIW-09](sukumo-ui-kit-widgets.md#uiw-09), In-window menu bar's open title.

- **Claim:** clicking the in-window menu bar's open title reopens its menu instead of closing it, because the engine closes the window's menus before routing the press, which deletes `BarState`.
- **Level:** integration (kit `Harness` with the in-window menu bar)
- **Method:** click File (opens), click File again. The test asserts no menu is open. File `repro_uiw_09.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-kit --test repro_uiw_09 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** `menus open after clicking the open title: 1` (`left: 1 right: 0`)
- **Notes:** the harness delivers the press to the parent window directly. Under an xdg_popup grab a compositor sends a click on the client's own parent surface to that surface (the entry's precondition), but this was not checked under a real compositor. The bar is used only without the Aizome shell.
- **Files:** `desktop/aizome/crates/aizome-kit/tests/repro_uiw_09.rs`

<a id="uiw-11"></a>

### UIW-11 — Confirmed

Catalog entry: [UIW-11](sukumo-ui-kit-widgets.md#uiw-11), Scroll-bar visibility and thumb drag.

- **Claim:** with more than one scroll event (anything over ~20 ms), only the first arms the bar tick. It fires while `shown_until` is still ahead, nothing re-arms, and the scroll bar stays drawn until something else repaints.
- **Level:** integration (kit `Harness`, real `virtual_list`, `check_frame` composing frames from damage)
- **Method:** scroll a list twice 100 ms apart, then let 2 s pass. The test asserts the frame composed from damage equals a whole redraw. Control: a single scroll event, same wait, passes. The widgets read `Instant::now()`, so the test advances the harness clock and sleeps the wall clock together in 10 ms steps. File `repro_uiw_11.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-kit --test repro_uiw_11 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** control: `FrameCheck { differs: 0.0, damaged: 3600.0 }` (the bar was redrawn away); two events: `FrameCheck { differs: 0.0008, damaged: 0.0, moves: 0 }` (the bar's pixels are stale, and nothing was invalidated)
- **Notes:** the test uses real sleeps (about 4 s) because there is no injectable clock. The margins are ≥80 ms, so it should be stable on a loaded machine. The same path (`Scroller`) serves `scroll`, `list`, `table` and `tree`; only the list was driven.
- **Files:** `desktop/aizome/crates/aizome-kit/tests/repro_uiw_11.rs`

<a id="uiw-15"></a>

### UIW-15 — Confirmed

Catalog entry: [UIW-15](sukumo-ui-kit-widgets.md#uiw-15), Selection cursor and anchor in lists, tables and outlines.

- **Claim:** the kit's cursor isn't clamped or remapped when the program's rows change. (1) In an outline, collapsing the branch holding the cursor then pressing Return panics. (2) In a table re-sorted by the program (selection remapped by name), Return activates the old cursor index and ↓ moves from the old row.
- **Level:** integration (kit `Harness`, real `tree` and `table`)
- **Method:** (1) open Folder by its triangle, click file 24 (row 25), close Folder by its triangle, Return. (2) click `b.txt` (row 0), click the sortable header. The program sorts to a, b, c and keeps `b.txt` selected (row 1), the way the portal's chooser remaps by path. Then Return (the test asserts `b.txt` is activated) or ↓ (the test asserts `c.txt`). File `repro_uiw_15.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-kit --test repro_uiw_15 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** (1) `panicked at crates/aizome-kit/src/widgets/table.rs:1150:57: index out of bounds: the len is 2 but the index is 25`; (2) `activated after sort: ["a.txt"] (selected [1])`; `after ↓: selected ["b.txt"]` (expected `c.txt`)
- **Notes:** the portal chooser itself was not run; its remapping (`chooser/model.rs:231-254`) is mirrored by the test program. In the chooser, part (2) means the wrong file is returned to the requesting app.
- **Files:** `desktop/aizome/crates/aizome-kit/tests/repro_uiw_15.rs`
- **Independent review:** upheld; re-run: All 3 fail: panic table.rs:1150 `len is 2, index 25`; `activated ["a.txt"]`; ↓ → `b.txt`; asserts the claim: Yes; reachability: real: clicks and keys; the remap mirrors `chooser/model.rs::refresh` (keep by path). The portal's file chooser would return the wrong file

<a id="uiw-18"></a>

### UIW-18 — Confirmed

Catalog entry: [UIW-18](sukumo-ui-kit-widgets.md#uiw-18), Editor viewport, position-bar linger and caret blink on one tick.

- **Claim:** (1) focusing an editor while a scroll tick is pending drops the focus tick request. The pending tick comes before `next_blink`, isn't due, and nothing re-arms, so the caret stops blinking. (2) An unfocused editor scrolled by several events keeps its position bar drawn.
- **Level:** integration (kit `Harness`, real `text_editor`, `check_frame`)
- **Method:** (1) scroll the editor, click into it 600 ms later (the scroll tick is due at 920 ms). From 1.0 s to 2.6 s, the test asserts caret damage and a pending tick. Control: click into a still editor (passes). (2) two scroll events 100 ms apart, 2 s later the test asserts a correct composed frame. Harness and wall clocks advance together, as in UIW-11. File `repro_uiw_18.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-kit --test repro_uiw_18 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** control `caret damage 3, timers pending 1`; (1) `after scroll then focus: caret damage 0, timers pending 0`; (2) `FrameCheck { differs: 0.0006, damaged: 0.0, moves: 0 }`
- **Notes:** the caret stays frozen in whatever phase it had, until the next focus change. The test uses real sleeps (about 7 s).
- **Files:** `desktop/aizome/crates/aizome-kit/tests/repro_uiw_18.rs`

<a id="uiw-27"></a>

### UIW-27 — Confirmed

Catalog entry: [UIW-27](sukumo-ui-kit-widgets.md#uiw-27), Session lock.

- **Claim:** a `SessionLockFinished` for a lock the program has already withdrawn turns the next lock's `Asked` into `Unlocked` and closes its surfaces. The new lock's `SessionLocked` is then ignored, so the session is locked with no lock surface while the program believes it is unlocked.
- **Level:** integration (real `sidestep-ui` `App` loop with the null render thread; the compositor's `finished` injected with `testing::inject_session_lock_finished`)
- **Method:** lock and open a lock surface. On `session_locked`, `unlock_session()`, `lock_session()`, open a second lock surface, and inject the first lock's `finished` as already in flight. The null render thread then confirms the second lock. The test asserts the second surface stays open and `is_session_locked()` is true. File `repro_uiw_27.rs` (`sidestep-ui/tests/`, standard harness).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/sukumo/Cargo.toml -p sidestep-ui --test repro_uiw_27 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** `program believes locked: false; second lock surface open: false; session_locked/finished heard: (1, 1); render thread saw: [SessionLock { locked: true }, SessionLock { locked: false }, SessionLock { locked: true }]`
- **Notes:** this confirms the main-thread state machine (no lock generation). The trigger needs a compositor to end or refuse the old lock just as the program re-locks. That timing was injected, not produced by a real compositor (the entry's low confidence is about its likelihood). The render thread's `finished` handler calls `unlock(self)`, which could also drop a newer lock; that was not exercised.
- **Files:** `desktop/aizome/sukumo/crates/sidestep-ui/tests/repro_uiw_27.rs`
- **Independent review:** upheld with caveat; re-run: Fails: `second lock surface open: false`, locked false; asserts the claim: Yes; reachability: staged: `finished` for lock 1 injected right after unlock+relock. No lock generation on the main thread. The trigger needs a compositor `finished` racing a re-lock (low likelihood)

<a id="skb-11"></a>

### SKB-11 — Confirmed

Catalog entry: [SKB-11](sukumo-backend.md#skb-11), Window configure, size and geometry lifecycle.

- **Claim:** after a size-changing configure, a hover change over Sukumo's own header before the main thread presents makes `refresh_decor` commit the new window geometry and frame around the old-size content, so one frame shows an undrawn band.
- **Level:** end-to-end (the real compositor headless through `aizome-comp/tests/common`, and a real sidestep-ui program as the client)
- **Method:** `examples/repro_skb_11.rs` is a sidestep-ui window (320 × 200, green) run with `SIDESTEP_DECORATIONS=client` and `WAYLAND_DEBUG=1`. Its `draw` holds the main thread for 2.5 s when asked to draw a new size, and writes marker files at the start and end of the stall. The test (`tests/repro_skb_11.rs`) rests the pointer on the header's close button and fills the window (`Request::Fill`, which sends a configure with a new size, as the maximize button does). Once the stall marker appears it moves the pointer 3 px. It asserts that the client's `WAYLAND_DEBUG` log has no `xdg_surface.set_window_geometry` between `REPRO stall begin` and `REPRO stall end`. A control test does the same without hovering a control. Files: `tests/repro_skb_11.rs`, `examples/repro_skb_11.rs`, screenshots and logs in `evidence/`.
- **Command:** `cargo build --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp --example repro_skb_11` then `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-comp --test repro_skb_11 -- --nocapture --test-threads=1` (in `nix develop .#aizome`)
- **Observed:** `the new geometry was committed around the old content before the main thread presented: ["[20:24:37.438947]  -> xdg_surface#35.set_window_geometry(0, 0, 1280, 778)"] (7 commits during the stall)`. The log shows `wl_pointer.leave` → `set_window_geometry(0, 0, 1280, 778)` → a 1280-wide header buffer attached → `wl_surface#37.commit()` (content, no new buffer) → `wl_surface#34.commit()` (frame), all 1.8 s before `REPRO stall end`. The control passes (`geometry requests []`). `evidence/during-stall-hover.png` shows a maximized 1280-wide header over the old 320 × 200 content, with the rest of the window undrawn. `during-stall-still.png` shows the intact old window.
- **Notes:** In this run the trigger was the compositor's own `wl_pointer.leave`, sent when Fill moved the window out from under the resting pointer (`decor::pointer_left`: hover Some → None). The test's 3 px move came later. This is the same `refresh_decor` path as the claim's pointer move after a click on the maximize button. The band lasts as long as the main thread takes to draw the new size. Here the stall was forced to 2.5 s; in real use it is one display pass. Audit confidence was low, but the mechanism is real. The test used Sukumo's client-side header (forced with `SIDESTEP_DECORATIONS=client`; it is also the default where the compositor asks for client-side decorations). Under Aizome's server-side decorations there is no Sukumo header to hover. `refresh_decor` is also called by `ToRender::MenuBar` (`menubar.rs:118`), `SetStyle` (`mod.rs:1036`) and colour-scheme changes (`mod.rs:1181`), which would commit the same way in that window; those callers were not tested.
- **Files:** `desktop/aizome/crates/aizome-comp/examples/repro_skb_11.rs`, `desktop/aizome/crates/aizome-comp/tests/repro_skb_11.rs`
- **Support files:** [`evidence/client-hover.log`](reproductions/support/skb-11/evidence/client-hover.log), [`evidence/client-still.log`](reproductions/support/skb-11/evidence/client-still.log), [`evidence/during-stall-hover.png`](reproductions/support/skb-11/evidence/during-stall-hover.png), [`evidence/during-stall-still.png`](reproductions/support/skb-11/evidence/during-stall-still.png)
- **Independent review:** upheld with caveat; re-run: Fails: `set_window_geometry(0,0,1280,778)` during the stall; control passes; asserts the claim: Yes; reachability: staged: a 2.5 s stall in `draw` and forced client-side decorations. Real commit path. In real use it is a one-frame glitch, and only where the compositor wants CSD (Aizome uses SSD)

<a id="skb-27"></a>

### SKB-27 — Not confirmed

Catalog entry: [SKB-27](sukumo-backend.md#skb-27), Selection and primary-selection ownership.

- **Claim:** `cleared_by_us` is set by a clear the compositor never echoes, so a later clear (a client that copied and then quit) is swallowed as ours, and paste returns the dead client's read-ahead text.
- **Level:** none: no caller reaches the path, so there is nothing to drive (call-site audit of the tree at 8e0acbe5)
- **Method:** I traced every way to `cleared_by_us = true`. Only `selection::set(state, None)` (`selection.rs:137-141`) and `primary::set(state, None)` set it. They are reached only through `ToRender::SetSelection`/`SetPrimarySelection { contents: None }`, which only `clipboard::Shared::offer_prepared(None)` sends (`clipboard.rs:495-498`). Every caller of `offer`/`offer_prepared` passes `Some`: sidestep-ui `set_prepared_clipboard` and `set_prepared_primary` (`app.rs:1108`, `:1124`), Shashin (`apps/shashin/src/clipboard.rs:14`), Utsushi (`apps/utsushi/src/picture.rs:23`) and Hako (`apps/hako/src/ui/clipboard.rs:32`). The `pending` retry re-sends what was pending, which is always `Some`. No test was written.
- **Command:** `grep -rn --include=*.rs -E "offer(_prepared)?\(None\)|SetSelection \{ contents: None|SetPrimarySelection \{ contents: None" .` (no matches); `grep -rln --include=*.rs "clipboard::shared" .` (the four callers above, all `Some`)
- **Observed:** no call site clears the selection. `cleared_by_us` therefore stays `false`, and every `selection(NULL)` the compositor sends reaches `shared().foreign_offered(Vec::new(), false)` (`selection.rs:536-538`).
- **Notes:** This is a latent hazard, not a live bug: it becomes reachable as soon as a clear API exists. Then it would bite in two cases: on compositors that don't echo a no-op clear (wlroots' `wlr_seat_set_selection` is believed to return early when the source is unchanged; not checked here), and on Aizome's compositor when the client clears without keyboard focus. smithay 0.7 silently denies a clear from an unfocused client (`data_device/device.rs:118-130`). For a focused client it always echoes `unset_selection` (`seat_data.rs:61-75`, `:190-197`), which consumes the flag. The other hazards in the entry (the flag is global across devices; pending contents wait for a keyboard enter; cancellation isn't reported) were not exercised.

<a id="skr-03"></a>

### SKR-03 — Confirmed

Catalog entry: [SKR-03](sukumo-render.md#skr-03), Retained paints and per-host frame scheduling (dirty / moving / last snapshot).

- **Claim:** `Compositor::frame` clips every damage rectangle pushed for a target to the current host's clip, so with two animating hosts in one window surface (disjoint clips), the first host visited is never redrawn while its `last` snapshot is advanced.
- **Level:** unit (`ca::tree::Compositor` driven directly: commit, a window paint with two composites, then frames)
- **Method:** Two 20 × 20 layers with ten-second opacity animations are composited by one `Target::Root` paint. Host A is clipped to x 0–100 and host B to x 100–200. The test calls `frame(window, t)` at t = 0.5, 1.0, 1.5 and 2.0, and asserts that each frame's redraw and uncovered rectangles touch both halves. File: `repro_skr_03.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/sukumo/Cargo.toml -p sidestep-engine --test repro_skr_03 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** every frame gives `damage [Rect { x0: 9.0, y0: 39.0, x1: 31.0, y1: 61.0 }]; host A redrawn: true, host B redrawn: false`, ending in `frames where an animating host was not redrawn: [(0.5, "B"), (1.0, "B"), (1.5, "B"), (2.0, "B")]`.
- **Notes:** Which host loses depends on the `HashMap` order (B in this run, the first host visited). It loses every frame, not just one. `wants_frame` stays true, so the window keeps scheduling frames that redraw only one spinner.
- **Files:** `desktop/aizome/sukumo/crates/sidestep-engine/tests/repro_skr_03.rs`

<a id="skr-04"></a>

### SKR-04 — Confirmed

Catalog entry: [SKR-04](sukumo-render.md#skr-04), Layer contents, canvas generations and their image-cache keys.

- **Claim:** when a commit replaces a layer's canvas (a resize, or contents set to none or an image), the old canvas is dropped without `images::forget`, so the render thread's image cache keeps its pixels until LRU eviction.
- **Level:** unit (on one thread, the render thread's steps: `Compositor::apply`, `Compositor::paint`, then rasterizing the expanded ops with `raster::paint`, which caches the canvas image on that thread)
- **Method:** The test commits a 20 × 20 canvas layer, composites and rasterizes it, and keeps a `Weak` to the canvas pixels from the emitted `Op::Image`. It then commits a 40 × 40 canvas (test 1) or `ContentUpdate::None` (test 2), and asserts the weak is dead. As a control it then calls `images::forget(&[old key])` by hand and reports whether the pixels go. File: `repro_skr_04.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/sukumo/Cargo.toml -p sidestep-engine --test repro_skr_04 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** `canvas resized 20 → 40 px: the replaced canvas's 1600 bytes (key 1) are still kept; still kept after images::forget(&[1]): false`; `canvas replaced by ContentUpdate::None: the replaced canvas's 1600 bytes (key 2) are still kept; still kept after images::forget(&[2]): false`.
- **Notes:** Forgetting the key by hand frees the pixels, so the cache entry is the only holder. Each size step of a live resize leaves one dead canvas behind, bounded only by the 256 MiB budget. Raster workers' caches (also in the entry) were not exercised.
- **Files:** `desktop/aizome/sukumo/crates/sidestep-engine/tests/repro_skr_04.rs`

<a id="sta-05"></a>

### STA-05 — Not confirmed

Catalog entry: [STA-05](settei-a.md#sta-05), Language & Region formats preview with a stale-locale guard.

- **Claim:** if localed refuses a Region formats choice (polkit dismissed), `Region` has already cleared `preview` and `Done(Err)` doesn't call `refresh_preview`, so the Formats section stays an indeterminate spinner.
- **Level:** integration
- **Method:** the real Settei window and General pane over the sample, opened at `general.region`, with the pane wrapped in a pass-through `Tap` (shared `Rc<RefCell<General>>`, can hold back replies). The sample localed always accepts and wakes the window, so for the request the test swaps in a sample `Cell` with `Wake::none()` (localed that takes the request without signalling), holds the `Done` reply, puts the original, unchanged localed back, and delivers `Done(_, Err("Not authorized: …"))` as a dismissed prompt would. It asserts the preview is back ("Week starts" shown). Files: `repro_sta_05.rs` (plus a one-line `mod` in `language.rs`).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-settei --lib repro_sta_05` (in `nix develop .#aizome`)
- **Observed:** `after the refused Region choice the Formats section is a spinner: preview None, reading None, locale shown "en_GB.UTF-8", status Some("Language & Region: Not authorized: the request was dismissed")`
- **Notes:** with the stock sample this never shows, because the sample's `Cell::update` wakes the window and `system_changed` refills the preview. The spinner stays until the page is shown again (`shown` → `refresh_preview`) or localed signals some other change.
- **Files:** `desktop/aizome/apps/settei/src/panes/general/repro_sta_05.rs`
- **Module line added to:** `desktop/aizome/apps/settei/src/panes/general/language.rs`
- **Review override:** Overturned by the independent review: the reproduction's fake localed never wakes the window, but the real service wakes it when the request finishes, even after failure, and the wake reloads the Formats preview. The spinner lasts only while the polkit prompt is up. The reproducer's verdict was Confirmed.
- **Independent review:** **overturned**; re-run: Fails: `preview None, reading None … Not authorized`; asserts the claim: Yes, but under a fake that behaves unlike the live service; reachability: **unreachable**: the test swaps in a localed with `Wake::none()`. Live `Locale::set_locale` runs through `bus::spawn` → `Task::spawn`, whose `Done::complete` calls the pane's wake on success *and* error ("Wake before notifying", aizome-sys service.rs:157-163; `Model::new` puts the pane's `cx.wake()` first, service.rs:184). That `AppMsg::SystemChanged` reaches `General::system_changed` → `refresh_preview` on the Language page and refills the preview. The live path refreshes the preview when the refused request returns. The spinner lasts only while the polkit prompt is up. `ManagedBySystem` (no wake) can't happen: the Region control is replaced by a "managed" row

<a id="sta-10"></a>

### STA-10 — Confirmed

Catalog entry: [STA-10](settei-a.md#sta-10), Keyboard setup: catching the key beside the space bar.

- **Claim:** while the keyboard setup sheet waits for a key, opening Settei at another pane runs `hidden` → `stop_waiting`. The sheet stays up without "Waiting for the key…", the key beside the space bar does nothing, and nothing restarts the wait.
- **Level:** integration
- **Method:** Settei over the sample, started at `keyboard.setup` (as the shell's prompt does). Then `AppMsg::Launched(["--pane","mouse"])` (a second launch or the palette), and a press of the sample keyboard's Left Alt (`SampleKey(56)`, the sample's stand-in for the compositor's catch). The same is repeated after a launch back to `--pane keyboard`. It asserts that a setup sheet still showing is still waiting and takes the key. File: `repro_sta_10.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-settei --test repro_sta_10` (in `nix develop .#aizome`)
- **Observed:** `over Mouse the setup sheet stays up without "Waiting for the key…": ["Set Up a Keyboard", …, "Not Now", "Same as Other Keyboards", "Save"]` / `the key beside the space bar did nothing` / `back on Keyboard, the key still does nothing` / `the sheet's buttons: ["Not Now enabled=true", "Same as Other Keyboards enabled=true", "Save enabled=false"]` (no "Press Again").
- **Notes:** live, the catch thread gives up because the counter moved. The sample models this with `waiting`, which `SampleKey` checks. Only Not Now and Same as Other Keyboards get out, as the entry says. The 500 ms catch-poll hazard was not exercised (it needs the compositor).
- **Files:** `desktop/aizome/apps/settei/tests/repro_sta_10.rs`

<a id="sta-12"></a>

### STA-12 — Confirmed

Catalog entry: [STA-12](settei-a.md#sta-12), Pane sheets against the window's single sheet slot (keyboard's three, network, printers, users).

- **Claim:** with Add a Layout's picker open, the shell's `--pane keyboard.setup --keyboard …` launch starts a key catch while `sheet()` still shows the picker. The next key is taken as the setup key, and after the picker closes "Add a Command…" shows the leftover setup sheet.
- **Level:** integration
- **Method:** Settei over the sample at `keyboard`: click "Add a Layout…", then `AppMsg::Launched(["--pane","keyboard.setup","--keyboard","Apple Inc. Magic Keyboard (05ac:029c)"])`, then `SampleKey(56)` (a key typed while the picker shows; the compositor's catch in the sample), then `PickerCancel`, then `AddCommand`. It asserts the command sheet shows and no setup key was taken. File: `repro_sta_12.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-settei --test repro_sta_12` (in `nix develop .#aizome`)
- **Observed:** after the launch (and after the key) the sheet shows `["Add a Keyboard Layout", "", "598 layouts", "Cancel", "Add"]`. After Cancel the sheet window is `None`. Then `Add a Command… shows another sheet: ["Set Up a Keyboard", …, "Apple Inc. Magic Keyboard", "05ac:029c", "Press Again", "Beside the space bar: ⌥ Alt (Option)", …, "Swap left Alt and Super", …]`: the key typed over the picker was taken as the setup key.
- **Notes:** live, the compositor swallowing the key (`aizome-comp-core` input path) is modelled by `SampleKey`; the compositor itself was not run. The keyboard's sheet priority (picker > setup > command) and the untracked `setup` behave exactly as the entry reads them.
- **Files:** `desktop/aizome/apps/settei/tests/repro_sta_12.rs`

<a id="sta-15"></a>

### STA-15 — Confirmed

Catalog entry: [STA-15](settei-a.md#sta-15), Connection details page: navigation, profile reads and the edit buffer.

- **Claim:** a change to connection A that comes back after B's details page opened re-reads A, and `loaded` swaps the edit buffer to A's address, router and DNS. B's page then shows A's values, and Apply on B writes A's static address to B.
- **Level:** integration
- **Method:** the real Network pane (wrapped in a pass-through `Tap` for state access) in the real window over the sample NetworkManager. A = `c-studio` gets a manual 10.9.8.7/24 via 10.9.8.1. Then `send_all([DnsChoice(A, Cloudflare), Back, Open(Details(B = c-hotel))])`: the sample answers the change on a `cx.wait` thread, so `Changed(A)` is deterministically handled after B's page opened, as with a polkit prompt. The test waits for A's DNS change to land, chooses Manually on B (DHCP), reads the `network-address`/`network-router` fields, then Apply. It asserts the edit buffer is B's, the fields aren't A's, and B gets no 10.9.8.7/24. Files: `repro_sta_15.rs` (plus a one-line `mod` in `network/mod.rs`).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-settei --lib repro_sta_15` (in `nix develop .#aizome`)
- **Observed:** `on B's page the edit buffer is A's: Edit { uuid: "c-studio", address: "10.9.8.7/24", router: "10.9.8.1", … }` / `B's IP address field shows Some("10.9.8.7/24"), its router Some("10.9.8.1"): A's` / `Apply on B wrote A's static address to B: ["10.9.8.7/24"], router Some("10.9.8.1")`
- **Notes:** because B is DHCP, the wrong values show once Manually is chosen. A manual B would show them at once. The other hazards (an unapplied edit wiped by a same-uuid reload, out-of-order live reads) were not separately tested.
- **Files:** `desktop/aizome/apps/settei/src/panes/network/repro_sta_15.rs`
- **Module line added to:** `desktop/aizome/apps/settei/src/panes/network/mod.rs`
- **Independent review:** upheld with caveat; re-run: Fails: edit buffer `c-studio`, B's fields show A's, Apply wrote `10.9.8.7/24` to B; asserts the claim: Yes; reachability: staged: `send_all([DnsChoice A, Back, Open B])` orders things before A's change returns. `loaded` replaces `edit` for any uuid ≠ `edit.uuid`, and the view never checks it. Live needs a change slower than Back+Open (polkit, or slow NM)

<a id="sta-19"></a>

### STA-19 — Confirmed

Catalog entry: [STA-19](settei-a.md#sta-19), Add-a-printer sheet: discovery, driver lookup, adding.

- **Claim:** choose one plugged-in printer that needs a driver, then another before CUPS's driver list comes back: the first device's `Drivers` reply fills `drivers` for the second, `ready` turns true, and Add installs the second printer with the first one's PPD.
- **Level:** integration
- **Method:** the real Printers pane (in a `Tap` that can hold its messages) in the real window over the sample CUPS. `AddPrinter`, then a fake discovery answer `Found(Ok([HP LaserJet P1102 usb, Samsung ML-2160 usb]))`, both non-driverless. `Choose(0)` then `Choose(1)`, with the sample's instant driver answers held back (CUPS still looking). Then the first lookup's answer is delivered late: `Drivers(Ok([HP's hpcups PPD]))`. It asserts no driver is taken for the Samsung and Add stays disabled. It then presses Add and reports what the sample CUPS added. Files: `repro_sta_19.rs` (plus a one-line `mod` in `printers.rs`).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-settei --lib repro_sta_19` (in `nix develop .#aizome`)
- **Observed:** `with Some("usb://Samsung/ML-2160%20Series?serial=…") chosen, the HP's driver list fills the sheet: driver Some("drv:///hpcups.drv/hp-laserjet_professional_p1102.ppd"), Add enabled true, Add pressed → the sample CUPS added [("Samsung_ML_2160", Some("usb://Samsung/…"), Some(false))]`. The sheet shows `"Driver", "HP LaserJet Professional P1102, hpcups 3.25"` under the chosen Samsung.
- **Notes:** the sample's add records only `driverless`, not the PPD. That Submit sends `DriverChoice::Model(<HP PPD>)` for the Samsung URI follows from the sheet state shown (Submit reads `drivers[driver]`, `printers.rs:530-535`). Live `add_printer` was not run (it needs CUPS).
- **Files:** `desktop/aizome/apps/settei/src/panes/repro_sta_19.rs`
- **Module line added to:** `desktop/aizome/apps/settei/src/panes/printers.rs`

<a id="sta-22"></a>

### STA-22 — Confirmed

Catalog entry: [STA-22](settei-a.md#sta-22), New account: its password applied once AccountsService lists it.

- **Claim:** a new account's password is applied only in Users & Groups' `system_changed`, which runs only while that pane is current. Add a user with a password and switch panes or quit at once, and the account doesn't get the password.
- **Level:** integration
- **Method:** the real Users pane (in a `Tap` holding `Created`) in the real window, over sample-shaped accounts with `Wake::none()`. Live AccountsService answers CreateUser before the snapshot lists the account: `create_user` resolves on the D-Bus reply, while the follower thread adds the user from `UserAdded` and a property read. The sample adds it at once, so the test takes kai out of the snapshot until after `Created(Ok)` is handled. It then opens Mouse, puts kai back and sends `AppMsg::SystemChanged` (the service's wake). Test 1 then returns to Users with a wake. Test 2 quits with ⌘Q instead. Both assert kai's `password_mode` is `Regular` (password set). Files: `repro_sta_22.rs` (plus a one-line `mod` in `users.rs`).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-settei --lib repro_sta_22` (in `nix develop .#aizome`)
- **Observed:** `kai's password wasn't set after leaving Users & Groups (pending, in memory: Some(("kai", "kamenozoki-7"))); back on the pane with a wake: Some(Regular)` (left `Some(SetAtLogin)`). Quit variant: `Settei quit with kai's password only in memory (Some(("kai", "kamenozoki-7"))): kai is left to choose one at first login`.
- **Notes:** "never" holds if Settei quits, or if Users & Groups isn't shown while some service wakes the window. Returning to the pane with a wake does apply it (observed `Regular`). The race between the CreateUser reply and the snapshot is narrow in practice, matching the entry's low confidence. The test confirms the mechanism, not how often it happens.
- **Files:** `desktop/aizome/apps/settei/src/panes/repro_sta_22.rs`
- **Module line added to:** `desktop/aizome/apps/settei/src/panes/users.rs`

<a id="stb-09"></a>

### STB-09 — Confirmed

Catalog entry: [STB-09](settei-b.md#stb-09), Bluetooth pane activity: pairing agent, discovery and discoverability while shown.

- **Claim:** quitting Settei while Bluetooth shows never calls `hidden`, so the adapter stays discoverable after Settei is gone. The next run, finding it already discoverable, never sets `made_discoverable` and so never undoes it.
- **Level:** integration
- **Method:** two tests. (1) Sample: the real Bluetooth pane in a `Tap` counting `hidden` calls. Leaving the pane counts one (the witness works), then ⌘Q with the pane showing; it asserts `hidden` ran and `showing` is false. (2) Live BlueZ: a fake `org.bluez` (powered adapter with settable `Discoverable`, discovery, an agent manager; `fake-bluez/`, zbus 5.19 built offline outside the worktree) on a private `dbus-daemon` listening at the wrapper's `DBUS_SYSTEM_BUS_ADDRESS` (the test refuses any other address). Settei runs live with only Bluetooth and Mouse. Run 1 shows Bluetooth (made discoverable), then ⌘Q. Run 2 shows it, then leaves for Mouse. It asserts the adapter ends not discoverable both times. Files: `repro_stb_09.rs` (plus a one-line `mod` in `bluetooth.rs`) and `fake-bluez/`.
- **Command:** (helper target dir removed after the run; rebuild first) `CARGO_TARGET_DIR=$CARGO_TARGET_DIR cargo build --offline --manifest-path <reproductions/support/stb-09/fake-bluez>/Cargo.toml`, then `env REPRO_FAKE_BLUEZ=$CARGO_TARGET_DIR/debug/repro-fake-bluez cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-settei --lib repro_stb_09` (in `nix develop .#aizome`)
- **Observed:** (1) `Settei quit with Bluetooth showing: hidden ran 0 time(s) on quitting, the pane still showing true`. (2) `run 1 made it discoverable (true, Some("discoverable=true")); after ⌘Q: Some("discoverable=true"). Run 2 found it so (made_discoverable false); after leaving the pane (1 hidden): Some("discoverable=true")`
- **Notes:** Settei has no quit hook. The kit's Quit only asks `close_requested`, which forgets the sheet, and `main.rs` only flushes settings. Both runs share the process's cached system-bus connection. That is still faithful, because BlueZ's `Discoverable` is an adapter property, not tied to the client (discovery is). The fake has no `DiscoverableTimeout`. BlueZ's default is 180 s, so the leak lasts forever only where it is set to 0. The leftover agent registration was not checked.
- **Files:** `desktop/aizome/apps/settei/src/panes/repro_stb_09.rs`
- **Support files:** [`fake-bluez/Cargo.lock`](reproductions/support/stb-09/fake-bluez/Cargo.lock), [`fake-bluez/Cargo.toml`](reproductions/support/stb-09/fake-bluez/Cargo.toml), [`fake-bluez/src/main.rs`](reproductions/support/stb-09/fake-bluez/src/main.rs)
- **Module line added to:** `desktop/aizome/apps/settei/src/panes/bluetooth.rs`
- **Independent review:** upheld with caveat; re-run: Both fail: `hidden` ran 0 times on ⌘Q; fake BlueZ left `discoverable=true` after both runs; asserts the claim: Yes; reachability: real: ⌘Q with the pane shown, fake `org.bluez` on a private bus. No quit hook. The leak is bounded by BlueZ's default `DiscoverableTimeout` (180 s; Sekka doesn't set 0)

<a id="stb-13"></a>

### STB-13 — Partly confirmed

Catalog entry: [STB-13](settei-b.md#stb-13), Display change: apply, "Keep these display settings?" countdown, keep or revert.

- **Claim:** (a) closing the window or quitting (⌘Q) during the 15 s "Keep these display settings?" countdown exits without `revert`, leaving the risky change applied. (b) A modeset slower than the 10 s read timeout returns `Err`, so no countdown starts and the change stays live, unconfirmed and unpersisted.
- **Level:** integration
- **Method:** (a) the real Displays pane (in a pass-through `Tap`) over the sample compositor, which keeps what it's told: DP-2 to 2560×1440 → countdown sheet. Then ⌘Q, in a second test a title-bar close request on the main window; harness dropped as the process ends. It asserts the compositor isn't left at the unkept mode (if the app stays, the countdown must go back). (b) Settei live with every pane, against a fake compositor on `$XDG_RUNTIME_DIR/stb13.sock` (`AIZOME_SOCKET`). It lists two displays, applies `output-config` at once and answers it 11 s later. DP-1 goes 144 → 60 Hz through the Refresh rate dropdown. It asserts the keep sheet appears, or the compositor is back at 144 Hz. Files: `repro_stb_13.rs` (in-crate, plus a one-line `mod` in `displays/mod.rs`) and `tests_repro_stb_13.rs` (= `tests/repro_stb_13.rs`).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-settei --lib repro_stb_13 -- --nocapture` and `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-settei --test repro_stb_13` (in `nix develop .#aizome`)
- **Observed:** (a) passes: `⌘Q refused while the keep sheet shows (1 windows open); the countdown went back` / `the close request was refused while the keep sheet shows (window still open)`. (b) fails: `after 10.37s: no countdown (sheet None), status "Couldn't change the displays: Resource temporarily unavailable (os error 11)"; DP-1 in the compositor is now Mode { 3840×2160, refresh: 60000 }, the pane shows ["… 3840 × 2160 at 60 Hz", "60 Hz"]; settings.toml: ""`
- **Notes:** (a) is not confirmed: aizome-kit guards both paths while any sheet is open. `Core::perform` returns early for `MenuRole::Quit` when a window has a modal, and `settle_app_menu` disables Quit in the published menu. `Runtime::close_requested` ignores closes of a window with a modal. Only an external kill or logout (SIGTERM) or a crash would end the countdown unreverted, and the entry doesn't name those. (b) is confirmed. An error mid-sequence (`system.rs:180-182`) takes the same `Err` path but was not run separately. The "`Loaded` not guarded by `keep`" hazard was not tested.
- **Files:** `desktop/aizome/apps/settei/src/panes/displays/repro_stb_13.rs`, `desktop/aizome/apps/settei/tests/repro_stb_13.rs`
- **Support files:** [`tests_repro_stb_13.rs`](reproductions/support/stb-13/tests_repro_stb_13.rs)
- **Module line added to:** `desktop/aizome/apps/settei/src/panes/displays/mod.rs`

<a id="stb-17"></a>

### STB-17 — Confirmed

Catalog entry: [STB-17](settei-b.md#stb-17), settings.toml write pipeline: ordered writer, pending count, reload debounce.

- **Claim:** while Settei's own writes are pending (a slider dragged), `reread` drops the watcher's one notice of another program's write, and the next `change` builds on the cached settings, so the shell's change (e.g. the Deskbar width) is overwritten.
- **Level:** integration
- **Method:** Settei over the sample with settings in the test's directory, using the real `Shared` writer thread. `settings.toml.tmp` is made a FIFO so the writer's first write blocks until the test reads it: "pending" lasts exactly as long as needed. Sequence: keyboard `Delay(300)` (write 1 pending); the shell writes `settings.toml` with `[deskbar] width = 88`; `AppMsg::SettingsOnDisk`; `Delay(400)`; the FIFO is drained; `flush`; `SettingsOnDisk` again. It asserts the final file has both `repeat_delay = 400` and `deskbar.width = 88`. File: `repro_stb_17.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-settei --test repro_stb_17` (in `nix develop .#aizome`)
- **Observed:** `repeat_delay` is 400 (Settei's change is in), then `the shell's Deskbar width written during the burst is gone (shown while pending: None; window now has None); the file's [deskbar] is {}` (left `None`, right `Some(88)`).
- **Notes:** the FIFO only makes the pending window deterministic. Live, the window is the write and rename time across a burst of changes. Because the one notice was consumed, the window never shows the shell's edit either. The temporary-name collision with `aizome-config`'s own save and the writer-spawn failure hazards were not tested.
- **Files:** `desktop/aizome/apps/settei/tests/repro_stb_17.rs`

<a id="hako-05"></a>

### HAKO-05 — Confirmed

Catalog entry: [HAKO-05](hako.md#hako-05), Transfer placement with conflict prompt and replace-aside protocol.

- **Claim:** Put Back (and Undo of Move to Trash) run `restore` with `trash: None`, so answering Replace at a conflict permanently deletes the newer item instead of moving it to the Trash as the sheet says.
- **Level:** integration
- **Method:** real `trash_items` then `restore` (and `undo::invert(Record::Trash)`) on files in a sandbox Trash (`Trash { home: <tmp>/data/Trash, topdirs: Some([]) }`), a newer `a.txt` made at the original place, asker `Always(Replace, …)`. Asserts the old item is back and the newer one is in the Trash with `original == home/docs/a.txt`. Files: `repro_hako_05.rs` (tests/).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hako --test repro_hako_05` (in `nix develop .#aizome`)
- **Observed:** both tests fail. `restore report: Report { done: [(…/data/Trash/files/a.txt, …/home/docs/a.txt)], problems: [], … }` then `the item it replaced should be in the Trash (the sheet says so); the Trash holds []`. Undo: `the item undo replaced should be in the Trash, not deleted`. The report is clean, so the person gets no warning.
- **Notes:** the cause is `Worker { trash: None }` in `restore` (`transfer.rs:141`), which makes `dispose` fall back to `remove_quietly` (`transfer.rs:469`). The Restore job (`jobs.rs:253`) and undo (`undo.rs:167`) both use it. The Copy/Move path, which passes `Some(trash)`, trashes correctly (existing test `replaced_items_go_to_the_trash`). `duplicate` also passes `None`, but it can't hit a conflict through Replace (it uses free names), so that wasn't tested.
- **Files:** `desktop/aizome/apps/hako/tests/repro_hako_05.rs`
- **Independent review:** upheld; re-run: FAIL ×2 as reported: Trash holds [], report clean; asserts the claim: Yes; reachability: real (Put Back or ⌘Z, then Replace at the sheet). `restore` builds `Worker{trash: None}` (transfer.rs:141), so `dispose` calls `remove_quietly` (:469). The sheet says "moves the one there to the Trash" (sheets.rs:322-326).

<a id="hako-06"></a>

### HAKO-06 — Confirmed

Catalog entry: [HAKO-06](hako.md#hako-06), Copy-to-partial commit and cross-filesystem move (copy, fsync, delete arrived sources).

- **Claim:** in a cross-filesystem move, when a nested folder's `sync_dir` fails, the report says it "couldn't be written safely", yet its files (already in `arrived.leaves`) are deleted from the source.
- **Level:** integration
- **Method:** real `transfer(…, Mode::Move, …)` on a tree `src/{top.txt, inner/{x.txt,y.txt}}` in TMPDIR. Faults are injected in that test binary only, by defining C `renameat2` (EXDEV for the moved folder, so the copy path runs without a second filesystem) and `fsync` (EIO when the fd is `…/.src.hako-part-*/inner`, like a flaky USB stick). All other calls go to the real syscalls. Asserts the report names `src/inner` as not written safely and that `src/inner/x.txt` and `y.txt` still exist. Files: `repro_hako_06.rs` (tests/).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hako --test repro_hako_06` (in `nix develop .#aizome`)
- **Observed:** `problems: [(…/from/src/inner, "it couldn't be written safely: Input/output error")]`, `fsync failed on: [".../to/.src.hako-part-6b7364d65149/inner"]`, then `the source files of a folder reported as not written safely must stay; src/inner now holds [], dest holds x.txt: true`. The item is reported in `done`.
- **Notes:** `copy_dir` returns `Skip` on the sync failure without removing its partial subfolder, so the unsynced copy is still renamed into place. Here the bytes exist at the destination. With a real EIO their durability is unknown, so the deleted sources were the only safe copy. The emptied `src/inner` folder itself stays.
- **Files:** `desktop/aizome/apps/hako/tests/repro_hako_06.rs`

<a id="hako-08"></a>

### HAKO-08 — Confirmed

Catalog entry: [HAKO-08](hako.md#hako-08), Trash can item protocol (trashinfo claim, rename, directorysizes, forget).

- **Claim:** Put Back of a folder merged into an existing folder, skipping a conflicting child, still calls `trash.forget(entry)`, so what's left in `files/<name>/` loses its `.trashinfo` and lists as an orphan of unknown origin.
- **Level:** integration
- **Method:** trash `home/f/{keep.txt, clash.txt}`, make a new `home/f/clash.txt`, then `restore` with a scripted asker (Merge for `f`, Skip for `clash.txt`). Asserts the leftover Trash entry (holding `clash.txt`) still has `original == home/f` and can be put back later. Files: `repro_hako_08.rs` (tests/).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hako --test repro_hako_08` (in `nix develop .#aizome`)
- **Observed:** `asked about ["f", "clash.txt"]; report Report { done: [(…/Trash/files/f, …/home/f)], skipped: [], problems: [] … }`; `the Trash now: [(…/Trash/files/f, None, None)]`, then `the folder left in the Trash lost its .trashinfo … left: None right: Some(".../home/f")`.
- **Notes:** `merge` returns `Ok` with `complete = false` (`transfer.rs:592`), `place_top` reports `Done`, and `restore` forgets on `Done` (`transfer.rs:164-166`). With `original: None`, a later Put Back takes the `"where it came from isn't known"` branch (`transfer.rs:138`). The test stops at the assertion before that step. The skip also isn't in the report (`skipped` and `problems` are empty).
- **Files:** `desktop/aizome/apps/hako/tests/repro_hako_08.rs`

<a id="hako-09"></a>

### HAKO-09 — Confirmed

Catalog entry: [HAKO-09](hako.md#hako-09), Undo/redo history with in-flight inversion jobs.

- **Claim:** an undo that fails yields an empty inverse that `History::undone` discards, so the step leaves both stacks and the next ⌘Z silently undoes the step before it.
- **Level:** integration
- **Method:** headless Hako (kit `Harness`, sandbox options, real jobs). Duplicate `a.txt` (step "Duplicate"), rename `a.txt` to `b.txt` (step "Rename"), make a new `a.txt`, then `Msg::Undo` (renaming back fails because the name is taken) and `Msg::Undo` again. Asserts the undo label is still "Rename" after the failure and that the copy survives the second ⌘Z. Files: `repro_hako_09.rs` (tests/, uses `tests/common`).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hako --test repro_hako_09` (in `nix develop .#aizome`)
- **Observed:** `after the failed undo: undo Some("Duplicate"), redo None, folder ["a copy.txt", "a.txt", "b.txt"]`; `after a second ⌘Z: folder ["a.txt", "b.txt"], items in the Trash 1`; `the step that failed to undo is gone from the history; the next ⌘Z then trashed the Duplicate's copy`.
- **Notes:** only the rename-onto-a-taken-name variant was run. The "Stop at a move's conflict prompt" variant reaches the same discard (`transfer_items` leaves `done` empty, giving `Record::Move { pairs: [] }`), but wasn't run.
- **Files:** `desktop/aizome/apps/hako/tests/repro_hako_09.rs`
- **Independent review:** upheld; re-run: FAIL as reported: label `Duplicate`, and the second ⌘Z trashed the copy; asserts the claim: Yes; reachability: real. The failed rename-back leaves an empty inverse, which is discarded. Whether dropping failed steps is a design choice is debatable, but the silent second ⌘Z is real.

<a id="hako-11"></a>

### HAKO-11 — Confirmed

Catalog entry: [HAKO-11](hako.md#hako-11), inotify folder watcher and its watch-descriptor map.

- **Claim:** a shown folder deleted and recreated under the same path keeps its dead watch descriptor in the map, `set` skips it by path, and changes inside the new folder are never heard.
- **Level:** unit + integration
- **Method:** (1) unit: the real `Watcher`, `set([proj, proj/dist])`, `rm -rf dist && mkdir dist`, `set` with the same dirs again (as `update_watch` would), write `dist/after.txt`, and assert an event for `dist`. (2) Headless Hako with `opts.watch = true` (real inotify thread) showing `home/proj/dist`. After `rm -rf && mkdir`, wait for the column to show the new, empty dist, write `dist/bundle.js`, then pump for 2 s (advancing the debounce clock) and assert it shows. Files: `repro_hako_11.rs` (tests/).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hako --test repro_hako_11` (in `nix develop .#aizome`)
- **Observed:** unit: `heard after writing into the new dist: []; watched [".../proj", ".../proj/dist"]`, then `a change inside the new dist was not heard`. Its first step passes: the change before the recreate was heard. Window: `bundle.js, made in the new dist, never showed: column Some((Ready, []))`.
- **Notes:** the other hazards in the entry (IN_Q_OVERFLOW dropped, a link and its target sharing one wd) weren't tested.
- **Files:** `desktop/aizome/apps/hako/tests/repro_hako_11.rs`

<a id="hako-16"></a>

### HAKO-16 — Confirmed

Catalog entry: [HAKO-16](hako.md#hako-16), Volume mount, unlock and two-phase eject tasks.

- **Claim:** ejecting a volume a window shows sends that window home with `Browser::go` directly, skipping `start_loads`, `update_watch` and the title, so the Home column stays `Load::Wanted` (blank) until the person navigates.
- **Level:** integration
- **Method:** headless Hako. UDisks is replaced by `Disks::with_bus(None, Wake::none())`, whose tasks fail at once. A mounted, ejectable `Volume` at `<tmp>/media/stick` is pushed into `volumes`. The window goes there (column Ready, title "stick"), then `Msg::Eject(block)`, then pumping for 1.5 s. Asserts the window is home, every column is Ready with Home's file, and the title is "Home". Files: `repro_hako_16.rs` (tests/).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hako --test repro_hako_16` (in `nix develop .#aizome`)
- **Observed:** `after Eject: columns [(Folder(".../home"), Wanted, 0)], title "stick"`, then `the Home column was never read (blank, no message)`.
- **Notes:** the bug is in the synchronous part of `Hako::eject` (`actions.rs:509-513`), before any UDisks task. With a real UDisks, the later `DisksChanged` harvesting (`actions.rs:429-454`) starts no loads either. The skipped `update_watch` wasn't observed (the sandbox has no watcher).
- **Files:** `desktop/aizome/apps/hako/tests/repro_hako_16.rs`

<a id="hako-17"></a>

### HAKO-17 — Confirmed

Catalog entry: [HAKO-17](hako.md#hako-17), Thumbnail request pipeline (queue, asked set, running count).

- **Claim:** pictures queued for thumbnails and pruned from `thumb_wanted` when the person leaves stay in `thumb_asked`, so on coming back `want_thumbs` skips them and they show generic icons for the session.
- **Level:** integration
- **Method:** in-crate test with a headless Hako and the real thumbnail pipeline and cache. `thumb_running = 2` is seeded to stand for two thumbnails of another folder still being made (the cap), so the 6 PNGs of `pics` only queue. The person goes to `other`, the two in-flight thumbnails finish (`Msg::Thumb(elsewhere, None)` delivered by hand), and the queue is pruned as Hako does. The person goes back to `pics` and the test waits for `thumbs_pending()` to clear. Asserts all 6 have images. A control (same pictures, no leaving) asserts all 6 are made. Files: `repro_hako_17.rs`, `INSERT.txt`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hako --lib repro_hako_17` (in `nix develop .#aizome`)
- **Observed:** control `ok`. Main: `on coming back: wanted 0, running 0, asked 6, images 0`, then `6 of 6 pictures never get a thumbnail`.
- **Notes:** seeding the in-flight count keeps the test deterministic. Unseeded, tiny PNGs are made faster than navigation, so in real use this needs a folder with many or large pictures, as the entry says.
- **Files:** `desktop/aizome/apps/hako/src/ui/repro_hako_17.rs`
- **Module line added to:** `desktop/aizome/apps/hako/src/ui/app.rs`
- **Independent review:** upheld; re-run: FAIL as reported: asked 6, images 0. Control passes.; asserts the claim: Yes; reachability: staged. `thumb_running=2` is seeded. In real use the folder's own first two jobs fill the cap, and `run_thumbs_again` prunes the rest on their `Thumb` (actions.rs:358-366, program.rs:515-523).. `thumb_asked` is never reconciled with the pruned queue (app.rs:677).

<a id="hako-19"></a>

### HAKO-19 — Confirmed

Catalog entry: [HAKO-19](hako.md#hako-19), Column listing load, reload debounce and refresh reconciliation.

- **Claim:** a change heard while its column is `Loading` is dropped: `reload_dir` skips `Loading` columns and nothing re-wants them when the stale read lands, so the new file stays missing until the next change.
- **Level:** integration
- **Method:** in-crate test, headless Hako with no watcher (the test plays it), and a folder `d` of 200 files. (1) After `GoPath(d)` the column is caught `Loading` (first attempt in every run). (2) The test's own inotify `IN_CLOSE_NOWRITE` on `d` shows Hako's read has finished, so the listing predates the new file. (3) `new.txt` is written. (4) `DirChanged(d)` and the `Reload(d, g)` its 150 ms timer would send are handled in one turn, before the queued stale listing, as when a slow read is still out after the debounce. Then 25 more turns are pumped (500 ms virtual), and the test asserts `new.txt` shows. Files: `repro_hako_19.rs`, `INSERT.txt`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hako --lib repro_hako_19` (in `nix develop .#aizome`)
- **Observed:** `caught d loading after 1 GoPath attempt(s)`; `column d: Ready, 200 rows, new.txt shown: false`, then `new.txt, made while d was being read, never shows`. Same result in 4 runs.
- **Notes:** the timer's own later `Reload` is ignored because `reloads` was already cleared, exactly as in the real sequence.
- **Files:** `desktop/aizome/apps/hako/src/ui/repro_hako_19.rs`
- **Module line added to:** `desktop/aizome/apps/hako/src/ui/app.rs`
- **Independent review:** upheld; re-run: FAIL as reported: Ready, 200 rows, no `new.txt`; asserts the claim: Yes; reachability: staged. `DirChanged`+`Reload` are delivered before the queued listing. The real program gets there when a read outlasts the 150 ms debounce (a big or slow folder).. `reload_dir` skips `Loading` (app.rs:645), and `loaded` never re-wants it.

<a id="hako-20"></a>

### HAKO-20 — Confirmed

Catalog entry: [HAKO-20](hako.md#hako-20), Rename session (inline field, new-folder auto-rename).

- **Claim:** `rename_when_loaded` stays armed when New Folder runs while its column is mid-load, and much later selecting that folder unexpectedly opens the rename field.
- **Level:** integration
- **Method:** in-crate test with the same orchestration as HAKO-19 (column caught `Loading`, the test's inotify confirms the read is done). Then `Msg::NewFolder`, a later change in `d` (`DirChanged` + 200 ms) so the folder shows, a trip to `other` and back, and `Msg::Select` of the folder. Asserts no rename field is open. Files: `repro_hako_20.rs`, `INSERT.txt`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hako --lib repro_hako_20` (in `nix develop .#aizome`)
- **Observed:** `after New Folder: rename None, armed Some((WindowKey(1), ".../home/d/untitled folder"))`; `after selecting it: rename Some(".../home/d/untitled folder"), armed None`, then `selecting a folder made long ago opened its rename field`.
- **Notes:** the entry's confidence was low, but it reproduces every time. It depends on the HAKO-19 skip: `new_folder`'s `reload_dir` ignores the `Loading` column, and the stale read consumes `pending` without a match.
- **Files:** `desktop/aizome/apps/hako/src/ui/repro_hako_20.rs`
- **Module line added to:** `desktop/aizome/apps/hako/src/ui/app.rs`
- **Independent review:** upheld; re-run: FAIL as reported: still armed, and selecting later opens rename; asserts the claim: Yes; reachability: staged (as HAKO-19). With the real watcher the folder shows unselected and the flag stays armed in the same way.. It depends on the HAKO-19 skip.

<a id="hako-25"></a>

### HAKO-25 — Confirmed

Catalog entry: [HAKO-25](hako.md#hako-25), Drop-target spot registry.

- **Claim:** in List/Icons view after opening a subfolder, stale spot 0 (the parent's listing) resolves drags first, so a drop moves files into whatever parent folder was at that row. In search, drops off a folder row fall through to stale column spots.
- **Level:** integration
- **Method:** headless Hako, painted with `h.render`, using the real drag gesture `h.drag`. (1) List view at Home (`Alpha`, `Beta`, `work`), painted, then into `work` (columns [Home, work], focus 1), painted again. Checks `spots.at` at `b.txt`'s row and drags `a.txt` onto `b.txt`'s row. Asserts the target is spot 1 / `work` and `a.txt` stays. (2) Columns at `work` (`Inner`, `b.txt`, `c.txt`) painted, then a search for `b.txt`, painted. Checks the point where `Inner`'s row was and drags the result there. Asserts no column spot resolves and `b.txt` stays. Files: `repro_hako_25.rs` (tests/).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hako --test repro_hako_25` (in `nix develop .#aizome`)
- **Observed:** List: `target at b.txt's row: Target { dest: Folder(".../home/Beta"), spot: 0, row: Some(1) }`; `after the drag: work ["b.txt"], Beta ["a.txt"]`. Search: `target where Inner's row was: Target { dest: Folder(".../work/Inner"), spot: 1, row: Some(0) }`; `after the drop: work ["Inner", "c.txt"], Inner ["b.txt"]`. Both tests fail.
- **Notes:** Icons view and the "switching from Columns" variant weren't run separately; they use the same single-view path (`view.rs:407-408`) and the same retain rule (`view.rs:25-27`).
- **Files:** `desktop/aizome/apps/hako/tests/repro_hako_25.rs`
- **Independent review:** upheld; re-run: FAIL ×2 as reported: `a.txt` moved into Beta, `b.txt` into Inner; asserts the claim: Yes; reachability: real (GoPath as a double-click, real render and drag). A stale spot 0 or column spot resolves the drop. Data moves into a folder that isn't on screen.

<a id="hako-28"></a>

### HAKO-28 — Confirmed

Catalog entry: [HAKO-28](hako.md#hako-28), Job lifecycle with blocking questions.

- **Claim:** if a job's window closes before it asks, the question opens over another window but `job.window` stays the dead key. When that host window closes, `reparent_questions` (matching only `j.window == gone`) doesn't re-ask, and the job hangs "Waiting for an answer".
- **Level:** integration
- **Method:** headless Hako with two windows. `Msg::DropUrls` (a copy into `b`, which already has `x.txt`) from the first window and `Msg::CloseWindow(first)` are handled in one turn, before the job's `Ask` arrives. The test waits for the question, checks it shows over the second window, then closes the second. Asserts some open window still shows the question (its "Keep Both" button). Files: `repro_hako_28.rs` (tests/).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hako --test repro_hako_28` (in `nix develop .#aizome`)
- **Observed:** the precondition passes (question over the second window). `job window WindowKey(1) (closed: first WindowKey(1)), sheet Some(WindowKey(3))`; after closing: `job status "Waiting for an answer", sheet Some(WindowKey(3)), ops Some(WindowKey(4)), windows [WindowKey(4)]`, then `no window shows the question any more; the job says "Waiting for an answer" and can only be stopped`.
- **Notes:** the Operations window offers only Pause and Stop (`sheets.rs:446-451`), so there's no way left to answer. `j.sheet` still names the sheet the kit removed with its parent.
- **Files:** `desktop/aizome/apps/hako/tests/repro_hako_28.rs`

<a id="hot-07"></a>

### HOT-07 — Confirmed

Catalog entry: [HOT-07](hotaru-app.md#hot-07), Automatic-link activation pipeline (match, hover, resolve, open).

- **Claim:** after a ❖-click on an automatic link, any new hover request (the pointer moving one cell) cancels the in-flight path resolution, so the link silently never opens.
- **Level:** integration (real `Hotaru` in the kit `Harness`, real automatic-link and resolver workers; the second test drives real pointer gestures through `TerminalView`)
- **Method:** `repro_hot_07.rs` (crate-private module of `app`, see `INSERT.txt`). (1) Message level, deterministic: an automatic `https://example.test/path` link is hovered and valid; `OpenLink` and the next motion's `MatchLink` (cell 3, same URL) are queued in one event-loop turn with `send_all`; asserts the URL opens or a notice appears. (2) Pointer level: ❖-hover `./notes.txt` with a link directory set, ❖-click (spawns the helper process), move one cell; asserts an opened URL or a notice. Two controls run the same flows without the motion and pass.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hotaru --lib repro_hot_07` (in `nix develop .#aizome`)
- **Observed:** `❖-click on https://example.test/path then one cell of motion: opened [], new pane status "" (resolution still pending after the motion: false)`; pointer: `❖-click on ./notes.txt then one cell of motion: nothing opened ([]) and no notice ("")`. Controls: `control_without_motion_opens_the_link ... ok`, `control_pointer_click_without_motion_has_an_outcome ... ok`.
- **Notes:** `cancel_automatic` (`app.rs:539-549`) clears `resolver_request`, and `ResolvedLink` then finds no response. Even if the resolver were kept, `valid_link_activation` at `ResolvedLink` requires the hover request to still be current, so a fix has to touch both. In the pointer test the helper is the test binary (it rejects the helper arguments), so a fixed Hotaru would show "Could not resolve link" there; the real app would open the path. That test depends on the helper outliving one `move_to` (it needs at least one 5 ms poll), so a timing slip could only make it pass, never fail falsely. The message-level test is deterministic.
- **Files:** `desktop/aizome/apps/hotaru/src/repro_hot_07.rs`
- **Module line added to:** `desktop/aizome/apps/hotaru/src/app.rs`
- **Independent review:** upheld; re-run: 2 of 4 fail (both controls pass): `opened [], new pane status ""`; asserts the claim: Yes; reachability: real: the pointer test is a ❖-click then a one-cell move; the message test is staged ordering. Any `MatchLink` runs `cancel_automatic`, which also drops `resolver_request`

<a id="hot-12"></a>

### HOT-12 — Confirmed

Catalog entry: [HOT-12](hotaru-app.md#hot-12), Settings persistence, reload and deferred window close.

- **Claim:** if ⇧❖, (Reload Configuration) comes while a deferred close or "open hotaru.toml" waits for a save in flight, the stale save answer leaves `closing`/`open_after_save` latched, and a later unrelated settings save closes the window or opens the file unexpectedly.
- **Level:** integration (real `Hotaru` with a real settings worker writing a temp `hotaru.toml`, kit `Harness`)
- **Method:** `repro_hot_12.rs`. (1) One turn: `SetCopyFormat` (save starts), `CloseTab` (the last tab; the close waits for the save), `ReloadConfig`. After the worker settles, asserts the window is closed; then a later `SetCopyFormat` is saved. (2) One turn: `OpenConfigFile` (no file yet, so it saves first) and `ReloadConfig`. A later unrelated `SetCopyFormat` must not open anything.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hotaru --test repro_hot_12` (in `nix develop .#aizome`)
- **Observed:** `the deferred close was lost: window still open with 0 tabs once the reload landed; a later unrelated settings change then closed the window`; `an unrelated settings change opened ["file:///…/hot12-open-850465/hotaru.toml"] (the earlier request opened [])`
- **Notes:** I drove the last-tab path (`remove_tab`, `app.rs:498-502`) because `send_all` makes it deterministic. The window-close-button path (`close_requested`, `app.rs:2651-2654`) sets the same `closing` latch, but I didn't drive it: `Harness::request_close` isn't a queued message, so its order against the worker's reply would be a race. In the last-tab case the window is left open with no terminal at all. The entry rated this low confidence because the window is a few milliseconds of fsync; the mechanism itself is deterministic.
- **Files:** `desktop/aizome/apps/hotaru/tests/repro_hot_12.rs`

<a id="hot-14"></a>

### HOT-14 — Confirmed

Catalog entry: [HOT-14](hotaru-app.md#hot-14), Appearance ownership between the desktop watcher and Hotaru's theme.

- **Claim:** under a Night override, a desktop switch from Day to Night changes nothing visible (so no `Msg::Theme`), so `desktop_appearance` stays Day, and choosing Follow Desktop then shows Day while the desktop is Night.
- **Level:** integration (real `Hotaru` in the kit `Harness`; `Harness::set_appearance` is the same `core.retheme` call the kit's settings watcher makes)
- **Method:** `repro_hot_14.rs`. Launch in Day, `SetTheme(Night)`, desktop → Night via `set_appearance`, `SetTheme(Desktop)`; asserts the theme is dark.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hotaru --test repro_hot_14` (in `nix develop .#aizome`)
- **Observed:** `Follow Desktop shows Day while the desktop is Night (Hotaru's cached desktop theme: Day)`
- **Notes:** the retheme to the desktop's Night is an exact no-op (`engine.rs:2854-2860`: same appearance), so the view emits no `Msg::Theme`. The other hazard (desktop changes to fields other than the theme get reverted under an override) was not tested.
- **Files:** `desktop/aizome/apps/hotaru/tests/repro_hot_14.rs`

<a id="hot-17"></a>

### HOT-17 — Confirmed

Catalog entry: [HOT-17](hotaru-app.md#hot-17), Single-instance claim and launch handoff.

- **Claim:** if the first instance takes over 2 s from `claim` to `serve`, a second launch times out on the acknowledgement and runs standalone, while the first later reads the buffered request and opens a tab too: one launch, two terminals.
- **Level:** integration (real `instance::claim` on a private 0700 `XDG_RUNTIME_DIR`; the second launch is a thread calling `claim`; the first instance is served by a real `Hotaru` in the `Harness`, so `launched()` → `serve`)
- **Method:** `repro_hot_17.rs`. The first `claim` returns `First(instance)` and is not served yet (the cold start). The second `claim` returns after its 2 s read timeout. Then the first instance starts with `app.instance = Some(instance)`. Asserts it's not the case that the second ran standalone and the first opened an extra tab. A control asserts a Hotaru without a socket starts with one tab.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hotaru --test repro_hot_17` (in `nix develop .#aizome`)
- **Observed:** `second claim: Err(Os { code: 11, kind: WouldBlock, … }) after 2.038183454s; first instance tabs: 2` → `one launch, two terminals: …`
- **Notes:** the connection waits in the listener backlog with the request in its buffer. After the client gives up and closes, `serve` still reads the length and the JSON (`instance.rs:136-148`) and sends `Launch`. The failed ack write is ignored.
- **Files:** `desktop/aizome/apps/hotaru/tests/repro_hot_17.rs`
- **Independent review:** upheld with caveat; re-run: Fails: second claim `WouldBlock` after 2.06 s, first instance 2 tabs; asserts the claim: Yes; reachability: staged: the first instance just doesn't serve for 2 s (a cold start); real `claim` and `serve`. Real handshake flaw. Needs a startup over 2 s from claim to `launched()`

<a id="hot-21"></a>

### HOT-21 — Confirmed

Catalog entry: [HOT-21](hotaru-app.md#hot-21), PTY worker lifecycle (spawn, run, EOF, hang-up, exit report, stop).

- **Claim:** when the shell exits but a background job still holds the PTY slave, there is no EOF, `try_wait` is never polled (`session.rs:787-790`), and the pane neither ends nor shows "Session ended" until the job exits.
- **Level:** integration (real `Hotaru` with a real PTY `Session` running `/bin/sh` in the `Harness`)
- **Method:** `repro_hot_21.rs`. `/bin/sh -c "(trap '' HUP; exec sleep 30) & echo $! > pidfile; exit 0"` with `wait_after_command = true`. Once the pid file exists (so the shell is done), asserts the pane ends within 3 s. A drop guard SIGKILLs the test's own `sleep`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hotaru --test repro_hot_21` (in `nix develop .#aizome`)
- **Observed:** `the shell exited, but 3 s later its pane has not ended (status Some("Shell")); the background job (pid 850861) still holds the PTY slave`
- **Notes:** the job ignores SIGHUP itself. This stands in for interactive Bash job control, which keeps `sleep 600 &` out of the kernel's session-leader hangup (non-interactive `sh` jobs would otherwise die of it). The entry notes this may be an accepted trade-off for lossless output. The pane would end when the job exits; I didn't wait for that.
- **Files:** `desktop/aizome/apps/hotaru/tests/repro_hot_21.rs`

<a id="hot-27"></a>

### HOT-27 — Confirmed

Catalog entry: [HOT-27](hotaru-app.md#hot-27), Pointer drag selection and held-button tracking.

- **Claim:** if a drag-select's release is lost (the compositor takes the pointer: leave with the button down), `drag`/`buttons` stay set, so later plain hover keeps extending the selection.
- **Level:** integration (kit `Harness` pointer events: press, move, `leave`, `enter`, move, into the real `TerminalView` of a preview `Hotaru`)
- **Method:** `repro_hot_27.rs`. Drag from column 0 to column 5, `h.leave(win)` with no release, then `enter` and hover to column 30 with no button held. Asserts the selection is unchanged.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-hotaru --test repro_hot_27` (in `nix develop .#aizome`)
- **Observed:** `plain hover after the lost release moved the selection's end (drag Some(Selection { anchor: (0,1), end: (0,6) }) → hover Some(Selection { anchor: (0,1), end: (0,31) }))`
- **Notes:** two parts of the claim were not asserted: button-held motion reports to mouse-tracking programs (the same stale `st.buttons` at `terminal_view.rs:1189`, read only) and copy-on-select never firing. The kit's leave handling (`engine.rs:2065-2072`) resets the press only for `drags_only` widgets, as the entry says.
- **Files:** `desktop/aizome/apps/hotaru/tests/repro_hot_27.rs`

<a id="koy-01"></a>

### KOY-01 — Confirmed

Catalog entry: [KOY-01](koyomi.md#koy-01), Koyomi agent service lifecycle (unit, bus connect retry, listener threads, wake loop).

- **Claim:** every notification Open spawns `aizome-koyomi --show DATE` and drops the `Child` without `wait()`, so each exited child stays a zombie under the long-running agent.
- **Level:** unit (the private `open_app` called directly; a real child process is spawned)
- **Method:** `open_app("2026-10-19")` spawns `current_exe()` (the test binary, which libtest rejects at once: `error: Unrecognized option: 'show'`, so it exits like a quick hand-off launch). The test scans `/proc/*/stat` for new children of the test process, waits for the child to exit, gives a reaper 2 s, and asserts that no child is left in state `Z`. It reaps whatever is left with `waitpid` before asserting. Files: `repro_koy_01.rs` (goes in `src/`), plus a one-line `#[path]` mod at the end of `src/agent.rs` (`INSERT.txt`)
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-koyomi --lib repro_koy_01` (in `nix develop .#aizome`)
- **Observed:** `the child Open started has exited but is still a zombie of the agent 2 s later: pids [788433] (children: [(788433, 'Z')])`
- **Notes:** In the agent, the child either hands off and exits at once or becomes the app and exits when the person quits it. Either way it ends as a zombie until the agent exits. The test drives `open_app`, so a fix placed only in `run()` (for example ignoring SIGCHLD there) would not make it pass; it would need to call that setup too. The entry's other hazards were not reproduced separately: no bus reconnect, settings and zone read once (the snooze part is reproduced in KOY-03), and the monotonic `recv_timeout` across suspend.
- **Files:** `desktop/aizome/apps/koyomi/src/repro_koy_01.rs`
- **Module line added to:** `desktop/aizome/apps/koyomi/src/agent.rs`
- **Independent review:** upheld; re-run: FAIL as reported: child in state `Z` 2 s later; asserts the claim: Yes; reachability: real. `open_app` drops `Child`. No SIGCHLD handling anywhere in koyomi or the kit (grep).

<a id="koy-02"></a>

### KOY-02 — Confirmed

Catalog entry: [KOY-02](koyomi.md#koy-02), Alert delivery state: due → fired → shown → snoozed/closed.

- **Claim:** an alert is marked fired before Notify succeeds, so an alert whose Notify failed (the shell not yet holding `org.freedesktop.Notifications`) is saved as fired and never posted. After an agent restart, Snooze and Open on notifications still on screen do nothing because `shown` is empty.
- **Level:** integration (the real `Agent` and the real `Dbus` notifier/listener, over a private `dbus-daemon` with a fake Notifications server; socket under the wrapper's `XDG_RUNTIME_DIR`)
- **Method:** (1) With no Notifications owner, `due` at 09:20 returns the stand-up and `post` fails (ServiceUnknown). Then the fake server takes the name and the agent runs turns at +30/60/90/300 s, followed by a restart from `agent.json`. A control alert ("Later", due at 09:25) checks that posting works once the server is up. The test asserts that both alerts are posted. (2) A first agent posts two alerts and saves its state. A second agent starts from `AgentState::load()` and listens. The server emits ActionInvoked `snooze` for one id and `open` for the other. The test asserts that Open gives the day and Snooze brings the alert back 10 min later. Files: `repro_koy_02.rs`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-koyomi --test repro_koy_02` (in `nix develop .#aizome`)
- **Observed:** (1) `left: ["Later"]  right: ["Later", "Stand-up"]` with `agent.json already listed 1 fired alert(s) after the failed Notify`; (2) `after a restart, Open should give the day to show (got None) and Snooze should bring the alert back 10 min later (due at 09:33: [])`
- **Notes:** The restarted agent does receive the signals: the shell's server broadcasts ActionInvoked, and the test receives both replies. `handle` drops them because the ids aren't in `shown`. The dead `replaces` path and the rewrite of agent.json on every wake were not asserted.
- **Files:** `desktop/aizome/apps/koyomi/tests/repro_koy_02.rs`

<a id="koy-03"></a>

### KOY-03 — Confirmed

Catalog entry: [KOY-03](koyomi.md#koy-03), Settings and window-state persistence (unsaved settings, debounced state, agent's stale copy).

- **Claim:** View ▸ Week Numbers and View ▸ First Day of the Week change `settings` without saving them, so the choices are lost at relaunch. The running agent reads `snooze_minutes` once, so a new "Snooze for" only applies after an agent restart.
- **Level:** integration (headless app through the kit `Harness` with `persist` on, writing only the wrapper's private `XDG_CONFIG_HOME`/`XDG_STATE_HOME`; the agent through its per-turn API)
- **Method:** (1) The test sends `Msg::WeekNumbers(false)` and `Msg::FirstDay(Sunday)` (what the View menu items send), quits through the window's close request, relaunches with `Koyomi::new` reading settings from file, and asserts `(false, Sunday)`. (2) The agent is built from `Settings::load()` with 10 min. In the app, Settings ▸ Snooze 30 then Done saves 30, which the test asserts. The agent then runs `refresh`, `due` and `post`, gets `handle(Snooze)`, and the test asserts the alert comes back after 30 min. Files: `repro_koy_03.rs`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-koyomi --test repro_koy_03` (in `nix develop .#aizome`)
- **Observed:** (1) `left: (true, Locale)  right: (false, Sunday)` with `koyomi.toml exists: false`; (2) `Snooze should use the length set in Settings (30 min), not the one read at start  left: 10  right: 30`
- **Notes:** `run()` is an endless loop on the session bus, so (2) drives the agent's turn methods. Settings are read only before the loop (`agent.rs:346`, `:105`), and no turn re-reads them. A fix that re-reads settings in `run()` but not in `refresh()` would need the test adjusted.
- **Files:** `desktop/aizome/apps/koyomi/tests/repro_koy_03.rs`

<a id="koy-04"></a>

### KOY-04 — Confirmed

Catalog entry: [KOY-04](koyomi.md#koy-04), Navigation and selection: view, anchor, cursor, mini month, selected occurrence.

- **Claim:** a day clicked in Month (or Week) view moves only `cursor`. ❖1 then shows `anchor`'s day while the agenda and New Event use the clicked day. ⌫ deletes the selected event even after navigating away from it or hiding its calendar.
- **Level:** integration (headless app through the kit `Harness`: a real click on the day cell, ❖1, ←/→ and ⌫ keys)
- **Method:** (1) Month view, click the empty part of Wednesday 14 October, press ❖1, then New Event. The test asserts that the Day view shows the 14th, the agenda isn't empty, and New Event is on the 14th. (2) Week view, click Design review, press →, press ⌫, and assert the review still exists. (3) The same with Work hidden instead of →. Files: `repro_koy_04.rs`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-koyomi --test repro_koy_04` (in `nix develop .#aizome`)
- **Observed:** (1) `the Day view shows 2026-10-17 (title "Saturday 17 October 2026"; the 17th's Farmers' market in the grid: true), the cursor is 2026-10-14, the agenda says "Nothing on this day.": true, and New Event opens on 2026-10-14`; (2) `selected: true, shown: false; note: Some(("Deleted “Design review: Koyomi” · ❖Z to undo", 1))`; (3) the same deletion with the calendar hidden
- **Notes:** One detail differs from the entry. The agenda filters `occ`, which covers the anchor's day, by the cursor's day, so it reads "Nothing on this day." beside a grid full of the 17th's events rather than listing the 14th. The status bar's week number wasn't checked, because the 14th and 17th share a week. Undo restores the deleted event.
- **Files:** `desktop/aizome/apps/koyomi/tests/repro_koy_04.rs`
- **Independent review:** upheld; re-run: FAIL ×3 as reported; asserts the claim: Yes; reachability: real (real click, ❖1, →, ⌫; the hide is a direct `ToggleCalendar`). Day view shows the anchor while the cursor drives the agenda and New Event. ⌫ deletes an off-screen selection (an undo note is shown).

<a id="koy-05"></a>

### KOY-05 — Confirmed

Catalog entry: [KOY-05](koyomi.md#koy-05), Modal sheet slots: editor, import, name, settings.

- **Claim:** a second launch's `--new` while the editor is open, or a second .ics while the import sheet is up, replaces the slot without closing the old sheet. That sheet's key then renders the whole main window as a stacked sheet.
- **Level:** integration (headless app through the kit `Harness`; `Msg::Instance` is what the single-instance socket delivers)
- **Method:** The test counts the main toolbar's "Today" text across all layers of the window. A control (one editor or import sheet, cancelled, then ❖2) shows ❖2 working. Then it opens the editor, sends `Msg::Instance(["--new"])`, cancels, and asserts one copy of the main window and that ❖2 switches to Week. The same is done with two `.ics` files and ImportCancel. Files: `repro_koy_05.rs`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-koyomi --test repro_koy_05` (in `nix develop .#aizome`)
- **Observed:** `copies of the main window showing: 1 alone, 1 with the editor, 2 after the second --new, 2 after Cancel (editor gone: true); ❖2 then gives Month`; the import case gives the same `1, 1, 2, 2 … Month`
- **Notes:** After Cancel, the orphan stays stacked as a modal copy of the main window and menu shortcuts stay blocked; the control shows ❖2 works otherwise. How a person would get rid of the orphan (Escape or a click inside the copy) was not explored.
- **Files:** `desktop/aizome/apps/koyomi/tests/repro_koy_05.rs`

<a id="koy-10"></a>

### KOY-10 — Confirmed

Catalog entry: [KOY-10](koyomi.md#koy-10), Calendar save pipeline: dirty → debounce → background write → stamp.

- **Claim:** when a background calendar write fails, `dirty` has already been taken and `Saved` records the unchanged file's stamp. Nothing writes the edit again, so it is lost at quit and discarded by the next external change's reload.
- **Level:** integration (headless app with `background_saves = true`; the write runs on the kit pool, driven with `advance` and `settle`; the test's own temporary folder is made read-only with chmod 0555, never a real disk or mount)
- **Method:** The test recolours Work and makes the folder read-only. It advances 300 ms for the flush and settles until the "couldn't be saved" note appears. Then it makes the folder writable again. (1) Close the main window and allow 3 s for writes; assert `work.ics` has the new colour. (2) Another program changes `home.ics`; advance 3 s for Poll; assert the in-memory colour survives. Files: `repro_koy_10.rs`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-koyomi --test repro_koy_10` (in `nix develop .#aizome`)
- **Observed:** (1) `an edit whose write failed should reach the file by quit (in memory: Some(Green))  left: Hanada  right: Green`; (2) `the unsaved recolour should survive the reload (the reload happened: true; on disk: Hanada)  left: Some(Hanada)  right: Some(Green)`
- **Notes:** The note tells the person the save failed, but nothing retries. The entry's other hazards were not reproduced: unawaited writes at exit, shared temp names with out-of-order finishes, and a write recreating a removed calendar.
- **Files:** `desktop/aizome/apps/koyomi/tests/repro_koy_10.rs`

<a id="koy-12"></a>

### KOY-12 — Confirmed

Catalog entry: [KOY-12](koyomi.md#koy-12), Recurring series split state: master rule, EXDATE/RDATE, RECURRENCE-ID overrides.

- **Claim:** setting Repeat to Never (or changing the frequency) for All Events from another instance clears or changes the master's rule but keeps the RECURRENCE-ID override, which still shows as a separate event.
- **Level:** integration (headless app: select the 19 October instance, Edit, `Field::Freq`, Save, click "All Events" in the scope alert)
- **Method:** The fixture is a weekly series from 5 October with the 12 October instance moved to 15:00. The test sets Repeat to Never and asserts that October's view shows only 5 Oct 10:00. It then sets Repeat to Monthly and asserts the same, because the 5th is the only October instance. Files: `repro_koy_12.rs`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-koyomi --test repro_koy_12` (in `nix develop .#aizome`)
- **Observed:** Never: `left: ["2026-10-05T10:00", "2026-10-12T15:00"]  right: ["2026-10-05T10:00"]`, `overrides left in the file: ["RECURRENCE-ID 2026-10-12T10:00:00 at 2026-10-12T15:00:00"]`; Monthly: the same leftover 12 October occurrence and override
- **Notes:** Leftover EXDATEs after a frequency change were not tested separately. The other hazards were not asserted: `Scope::Future` moving past instances, unmoved overrides not following a series move, and SEQUENCE bumped twice.
- **Files:** `desktop/aizome/apps/koyomi/tests/repro_koy_12.rs`
- **Independent review:** upheld; re-run: FAIL ×2 as reported: 12 Oct 15:00 override survives Never and Monthly; asserts the claim: Yes; reachability: real (the editor, Save, the "All Events" alert). The rule changes but the RECURRENCE-ID override is kept and shown on its own.

<a id="koy-14"></a>

### KOY-14 — Confirmed

Catalog entry: [KOY-14](koyomi.md#koy-14), Store's view of the calendar folder: stamps, reload, trash.

- **Claim:** an unreadable .ics is counted by `files()` but not loaded, so `changed_on_disk` stays true: the app reloads on every Poll and clears undo. Separately, a calendar whose `create` couldn't write stays sorted into `calendars` without `reindex`, and the Day view's agenda then indexes past an event list and panics.
- **Level:** unit (`Store::open` and `changed_on_disk`) and integration (headless app)
- **Method:** (a) The fixture folder has an extra `broken.ics` set to mode 000 (the tests run as uid 1000, not root). Store-level check: assert `!changed_on_disk()` right after `open`. App-level check: start with the file present, recolour Work, advance 7 s (two Polls), press ❖Z, and assert the colour is restored. A control without the unreadable file passes. (b) Day view of the 17th, make the folder read-only (0555), then New Calendar "Aaa" and NameDone under `catch_unwind`; assert no panic. Files: `repro_koy_14.rs`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-koyomi --test repro_koy_14` (in `nix develop .#aizome`)
- **Observed:** (a) `nothing changed since the store read the folder, but changed_on_disk says so (problems: [".../broken.ics: Permission denied (os error 13)"])`; ❖Z after Polls `left: Some(Green)  right: Some(Hanada)`; the control `ok`. (b) `panicked at apps/koyomi/src/ui/mod.rs:424:36: index out of bounds: the len is 0 but the index is 7`, with calendars `["home", "work"]` becoming `["aaa", "home", "work"]` and the note `The calendar couldn't be made: Permission denied (os error 13)`
- **Notes:** The agent's index rebuild on every wake follows from the same `changed_on_disk` result but was not measured. The `finish_import` path to the same stale index was not run separately. Permissions are restored by drop guards.
- **Files:** `desktop/aizome/apps/koyomi/tests/repro_koy_14.rs`

<a id="koy-17"></a>

### KOY-17 — Confirmed

Catalog entry: [KOY-17](koyomi.md#koy-17), Time-grid pointer drag: move, resize, create; edge hover and scroll.

- **Claim:** a time-grid drag holds the picked-up block by its index in the current blocks. Pressing → mid-drag shows the next week, and the drop moves whichever block has that index there.
- **Level:** integration (headless app: a real pointer press/move/release in the time grid and → from the Keys widget, which has focus)
- **Method:** One calendar holds Alpha on Wednesday 14th at 10:00 and Bravo on Wednesday 21st at 15:00, the only block in each week. The test presses on Alpha, drags it down an hour, presses →, and releases. It asserts that Bravo is unchanged. Files: `repro_koy_17.rs`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-koyomi --test repro_koy_17` (in `nix develop .#aizome`)
- **Observed:** `dropping Alpha moved Bravo, which it never touched (Alpha now at 2026-10-14T10:00:00, Bravo at 2026-10-21T11:00:00; note Some(("Moved “Bravo” to Wed 21 Oct, 11:00–12:00 · ❖Z to undo", 1)))`
- **Notes:** The Poll-mid-drag variant (an external change replacing the blocks) was not run, but it goes through the same index lookup (`week.rs:549`, `:589`).
- **Files:** `desktop/aizome/apps/koyomi/tests/repro_koy_17.rs`

<a id="koy-18"></a>

### KOY-18 — Confirmed

Catalog entry: [KOY-18](koyomi.md#koy-18), All-day strip drag.

- **Claim:** an all-day strip drag holds the bar by index. Pressing → into a week with fewer bars panics on the next pointer move; with enough bars, the drop moves a different event.
- **Level:** integration (headless app: a real press/move/release on the all-day strip, with → during the drag)
- **Method:** Alpha (Tuesday) and Bravo (Thursday) are in the week of the 12th; Charlie (Wednesday) is in the week of the 19th. (a) Drag Bravo, which is bar 1, one column, press →, and move under `catch_unwind`; assert no panic. (b) Drag Alpha (bar 0) the same way and release; assert Charlie stays on the 21st. Files: `repro_koy_18.rs`
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-koyomi --test repro_koy_18` (in `nix develop .#aizome`)
- **Observed:** (a) `panicked at apps/koyomi/src/ui/week.rs:1109:42: index out of bounds: the len is 1 but the index is 1`; (b) `dropping Alpha moved Charlie, which it never touched (Alpha on 2026-10-13, Charlie on 2026-10-22; range (2026-10-19, 2026-10-25))`
- **Notes:** In the app this panic would end the process: the panic is in widget event handling, and nothing catches it. That last point is inferred, not run end-to-end.
- **Files:** `desktop/aizome/apps/koyomi/tests/repro_koy_18.rs`
- **Independent review:** upheld; re-run: FAIL ×2 as reported: panic at week.rs:1109 index 1 len 1; Charlie moved; asserts the claim: Yes; reachability: real (press, move, →, move, release). The bar is held by index across week navigation. The panic in event handling is uncaught.

<a id="yuk-02"></a>

### YUK-02 — Confirmed

Catalog entry: [YUK-02](yukimi.md#yuk-02), Update receipt and system completion advice.

- **Claim:** after a Yukimi activation fails past commit, the receipt stays `activating` for S1, and `system_state` keeps answering Unknown "Activation did not finish successfully" even after the person runs `nixos-rebuild switch` to a different system S2 and restarts into it.
- **Level:** unit (`system_state` driven directly); also end-to-end (real helper worker and `sekka-update --status`, Nix stubbed)
- **Method:** `repro_yuk_02.rs` (an integration-test file of `yukimi-system`) calls `completion::system_state(S2, S2, S2, receipt{S1, boot-1, activating}, "boot-2")` and asserts the answer is not Unknown. A second test covers a case the entry does not name: `nix-env --set` failed, then a plain restart into S0. `repro_yuk_02.sh` runs the real `yukimi-helper __worker … change --packages htop` with the shared namespace harness (described under YUK-09). The stand-in `switch-to-configuration` fails, so the receipt is left `activating`. The script then simulates `nixos-rebuild switch` to S2 plus a restart: it repoints the profile and `/run/{current,booted}-system`, and replaces the receipt's boot id with an earlier one. Finally it runs the real `sekka-update --status --json` and asserts the action is not `unknown`.
- **Command:** `cargo test --locked --manifest-path apps/yukimi/Cargo.toml -p yukimi-system --test repro_yuk_02`; `bash apps/yukimi/yukimi-helper/tests/repro_yuk_02.sh $CARGO_TARGET_DIR/debug/yukimi-helper` (after `cargo build --locked --manifest-path apps/yukimi/Cargo.toml -p yukimi-helper`) (in `nix develop`)
- **Observed:** `a later healthy boot into S2 still reports the old failed activation of S1: Completion { action: Unknown, reasons: ["Activation did not finish successfully. Review the update log before restarting."] }`. The S0 case fails the same way. End-to-end: the helper result is `"System changes had already begun; the configuration was not restored. Switching to the new system failed…"` with receipt `"state":"activating"`, and after the repair and restart `sekka-update --status --json` returns `{"action":"unknown","reasons":["Activation did not finish successfully. Review the update log before restarting."]}`.
- **Notes:** the end-to-end restart is simulated by editing the receipt's boot id, because the real `/proc/sys/kernel/random/boot_id` can't change here. The guard at `completion.rs:484-495` only clears the receipt when its own system is booted. Nothing outside Yukimi's change and rollback rewrites the receipt (writers are only at `main.rs:978-989` and `:1034-1057`). The same-boot case (switch to S2 without restarting) also reads as Unknown, but no test asserts it.
- **Files:** `apps/yukimi/crates/yukimi-system/tests/repro_yuk_02.rs`, `apps/yukimi/yukimi-helper/tests/repro_yuk_02.sh`
- **Independent review:** upheld; re-run: Both unit tests fail (Unknown); end-to-end `sekka-update --status` → `unknown`; asserts the claim: Yes; reachability: staged: the reboot is simulated by editing the boot id and the `/run` links; real helper worker and stubbed Nix. completion.rs:484-495 clears `activating` only for its own system

<a id="yuk-09"></a>

### YUK-09 — Confirmed

Catalog entry: [YUK-09](yukimi.md#yuk-09), Privileged change transaction (back up, edit, update, build, commit, activate).

- **Claim:** `change --channels` runs `nix-channel --update` before building, but only files are backed up, so after a failed or stopped build root's channels stay advanced and the next unrelated change silently builds on them.
- **Level:** end-to-end (the real `yukimi-helper` worker; Nix tools are stand-ins)
- **Method:** `repro_yuk_09.sh` re-execs itself in `unshare --user --map-root-user --mount`. There it puts private tmpfs over `/run`, `/nix/var/nix`, `/var/lib/yukimi` and `/etc/yukimi`, and binds a temporary directory over `/etc/nixos`. As a result the helper's fixed PATH (`/run/current-system/sw/bin`) and fixed paths (profile, receipt, saved configurations, lock, operations directory) all resolve to stand-ins. The stand-ins `nix`, `nix-env`, `nix-channel` (creates `channels-<n+1>-link`), `nix-store`, `switch-to-configuration`, `git`, `loginctl` and `systemctl` log their arguments. The script checks that isolation holds before it runs `yukimi-helper __worker DIR change --channels --mode auto`. Case A: the build fails. Case B: SIGINT (what the client forwards on Stop) arrives once the stand-in build has started. After each case the script asserts that the channels profile still points at `channels-1-link`. After case A it also runs `change --packages htop`, whose stand-in build records which channels generation it saw.
- **Command:** `bash apps/yukimi/yukimi-helper/tests/repro_yuk_09.sh $CARGO_TARGET_DIR/debug/yukimi-helper` (in `nix develop`)
- **Observed:** case A: `{"error":"Building the new system failed","ok":false}`, `ok: configuration file put back`, `FAIL: root's channels put back after the failed change — expected channels-1-link, actual channels-2-link`. Next change: `FAIL: the package install is built on the channels the system had — actual: channels=channels-2-link`. Case B: `{"error":"Stopped","ok":false}`, `FAIL: … actual: channels-2-link`.
- **Notes:** the claim that the Updates page then calls the channels current was not run: it needs real channel store paths and the channels.nixos.org check. It follows from `channels::root()` reading the advanced profile. The namespace's root is the unprivileged user, and every helper command resolved to a stand-in (checked before each run). After the runs, the real `/etc/nixos`, `/var/lib/yukimi` and profiles have unchanged timestamps. The pkexec/`systemd-run` client layer is bypassed by calling `__worker` directly.
- **Files:** `apps/yukimi/yukimi-helper/tests/repro_yuk_09.sh`

<a id="yuk-10"></a>

### YUK-10 — Confirmed

Catalog entry: [YUK-10](yukimi.md#yuk-10), Saved per-generation configuration snapshots.

- **Claim:** saved configurations are keyed by generation number. After returning to N, an out-of-band `nix-collect-garbage -d` and a `nixos-rebuild switch` that reuses N+1, Yukimi keeps the stale `generation-<N+1>` copy, so a later return to N+1 restores another build's flake.lock.
- **Level:** end-to-end (the real `yukimi-helper` worker for every Yukimi step; Nix tools and the out-of-band steps are stand-ins)
- **Method:** `repro_yuk_10.sh` uses the namespace harness described under YUK-09, on a flake system. The steps are:
  1. Yukimi runs `change --update nixpkgs`, making generation 2 from lock-B.
  2. Yukimi runs `rollback 1`.
  3. Outside Yukimi, `system-2-link` is removed (garbage collection).
  4. Outside Yukimi, the lock is set to lock-C and a new generation 2 is linked (`nixos-rebuild`).
  5. Yukimi runs `change --packages htop`, making generation 3.
  6. Yukimi runs `rollback 2`.
  The script asserts that `configurations/generation-2/flake.lock` is lock-C after step 5, and that `/etc/nixos/flake.lock` is lock-C after step 6.
- **Command:** `bash apps/yukimi/yukimi-helper/tests/repro_yuk_10.sh $CARGO_TARGET_DIR/debug/yukimi-helper` (in `nix develop`)
- **Observed:** steps 1–2 pass (`generation 2 kept with lock-B`, `flake.lock back to lock-A`). After step 5: `FAIL: generation 2's saved configuration is the one that built it — expected lock-C, actual lock-B`. After step 6: `FAIL: flake.lock after going back to generation 2 — expected lock-C, actual lock-B`. The rollback reports `"ok":true,"left":[]`, so nothing tells the person.
- **Notes:** Nix's generation-number reuse was simulated. The stand-ins number generations the way Nix's `createGeneration` does (last existing + 1), so deleting N+1 and rebuilding gives N+1 again. The History/Updates reading was not run, but `update_history::History::load` reads `generation-<n>` by number (`update_history.rs:17-26`), so it would show lock-B's revisions. The entry rated this low confidence; the mechanism reproduces as described.
- **Files:** `apps/yukimi/yukimi-helper/tests/repro_yuk_10.sh`

<a id="yuk-11"></a>

### YUK-11 — Confirmed

Catalog entry: [YUK-11](yukimi.md#yuk-11), Rollback and clean-up sequences.

- **Claim:** Clean Up "older than a week" deletes generations, then collects garbage. Stop during `nix-store --gc`, or a GC failure, returns before `commit()` and `switch-to-configuration boot`, so the boot menu still lists the deleted generations.
- **Level:** end-to-end (the real `yukimi-helper` worker; Nix tools are stand-ins)
- **Method:** `repro_yuk_11.sh` uses the namespace harness described under YUK-09. It starts with generations 1–3, current 3, and a boot menu of all three. The stand-in `switch-to-configuration` rewrites the boot menu from the profile's links, `nix-env --delete-generations` removes the non-current links, and `nix-store --gc` succeeds, fails, or blocks as each case needs. It runs `clean --older-than-days 7` three times: a control run where GC succeeds, case A with SIGINT once GC has started, and case B where GC fails. Each time it asserts that the boot menu equals the generations that still exist.
- **Command:** `bash apps/yukimi/yukimi-helper/tests/repro_yuk_11.sh $CARGO_TARGET_DIR/debug/yukimi-helper` (in `nix develop`)
- **Observed:** control: `{"ok":true}`, and the menu becomes `system-3-link`. A: `{"error":"Stopped","ok":false}`, `generations left: system-3-link`, `FAIL: boot menu … after Stop — expected system-3-link, actual system-1-link system-2-link system-3-link`. B: `{"error":"Collecting garbage failed","ok":false}` with the same menu. In A and B, `switch-to-configuration` is never called.
- **Notes:** the boot menu is modelled by the stand-in's list. The later consequence (a GC removes those closures, then the entries fail to boot) needs a real bootloader and store, ideally a disposable VM, and was not run. `prune_saved` also already deleted the saved configurations of generations 1 and 2 before the stop.
- **Files:** `apps/yukimi/yukimi-helper/tests/repro_yuk_11.sh`
- **Independent review:** upheld; re-run: Fails A and B: boot menu still lists 1, 2, 3; control ok; asserts the claim: Yes; reachability: real: Stop (SIGINT) during gc and a gc failure on the real worker; Nix stubbed in a namespace. `wait`/`!success` return before `commit()` and `switch-to-configuration boot` (main.rs:1095-1107)

<a id="yuk-15"></a>

### YUK-15 — Confirmed

Catalog entry: [YUK-15](yukimi.md#yuk-15), Package index lifecycle.

- **Claim:** (1) after a live update moves the system's Nixpkgs, `ensure_index` returns early because an index exists and isn't stale, so Discover keeps the old index, and an in-flight index for the old Nixpkgs is marked current when it lands. (2) After `make` fails, every Search keystroke starts another `nix eval`.
- **Level:** integration (the real app through the kit `Harness`)
- **Method:** `repro_yuk_15.rs` builds `Yukimi::with(Source::System(Paths{temp etc, elsewhere}))`, with facts naming `nixpkgs-A` or `nixpkgs-B` (hello 2.10 or 2.12) and a temporary `XDG_CACHE_HOME`. A stand-in `nix` on PATH answers the index's `nix eval` from files: it logs each call, can be gated with a release file, and can fail. There are three tests:
  - (a) A's index is cached, the system moves to B (the test rewrites `system.json`, then sends `Reload`), and the test asserts the index is B's.
  - (b) A's index is mid-evaluation, the system moves to B, A's evaluation is released, and the test asserts the index is B's or marked stale.
  - (c) A's evaluation fails, then come five `Search` keystrokes, and the test asserts one evaluation.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-yukimi --no-fail-fast --test repro_yuk_15` (in `nix develop .#aizome`)
- **Observed:** (a) `system now uses nixpkgs-B; index still A's (stale flag false) left: Some("2.10") right: Some("2.12")`. (b) `A's late index is in use (hello Some("2.10")) and marked current (stale false); evals ["nixpkgs-A"]`. (c) `6 evaluations …; notes [(4, "error: evaluation of nixpkgs-A aborted"), (5, …), (6, …)]`, so one note per keystroke too.
- **Notes:** both suspected bugs reproduce. The reads ran in the background: `Store::load`, generations, completion and the channels profile are read-only on the real host, as in `headless.rs`'s `nothing_is_changed_when_looking_at_another_system`. The stand-in fails every other `nix` call, so no real Nix ran.
- **Files:** `desktop/aizome/apps/yukimi/tests/repro_yuk_15.rs`

<a id="yuk-19"></a>

### YUK-19 — Confirmed

Catalog entry: [YUK-19](yukimi.md#yuk-19), Desktops page choice drafts.

- **Claim:** after unticking Aizome (Yukimi moves Default desktop to Tatami), the Desktops group's Undo removes only the desktops draft, so Apply writes `calamares.defaultDesktop = "tatami"`.
- **Level:** integration (the real app through the kit `Harness`, sample system, mock runner)
- **Method:** `repro_yuk_19.rs` shows Desktops, picks `calamares.desktops` aizome off (and checks the default moved to tatami), then sends `UndoChoice("calamares.desktops")` and `ApplyChoices` and clicks the alert's Apply. It asserts that no choice is still drafted and that no operation ran.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-yukimi --no-fail-fast --test repro_yuk_19` (in `nix develop .#aizome`)
- **Observed:** `after undoing the desktops, still drafted: [("calamares.defaultDesktop", ["tatami"])]; Apply ran: [Choose { choices: [("calamares.defaultDesktop", ["tatami"])], what: "the default desktop" }]`
- **Notes:** the desktops themselves are restored correctly. Only the moved dependent survives the Undo, because `UndoChoice` (`app.rs:1415-1418`) skips `set_choice`'s move-back logic.
- **Files:** `desktop/aizome/apps/yukimi/tests/repro_yuk_19.rs`

<a id="yuk-20"></a>

### YUK-20 — Confirmed

Catalog entry: [YUK-20](yukimi.md#yuk-20), Sheet lifecycles: package details, branch chooser, comparison.

- **Claim:** `Msg::Branches` carries neither the input nor the sheet key, so after the person cancels one input's branch chooser and opens another's, the first GitHub answer fills the open sheet, and the person can Follow another repository's branch.
- **Level:** integration (the real app through the kit `Harness`, sample system; the worker's reply is injected)
- **Method:** `repro_yuk_20.rs` runs `ChooseBranch("home-manager")`, `CloseSheet`, then `ChooseBranch("nixos-hardware")`. It then sends the reply home-manager's worker would send, `Msg::Branches(Ok([master, release-25.05, release-25.11]))`, and asserts the nixos-hardware chooser still has no branch list. Before asserting, it picks `release-25.05` and confirms the Follow, to show what would run.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-yukimi --no-fail-fast --test repro_yuk_20` (in `nix develop .#aizome`)
- **Observed:** `nixos-hardware's chooser lists home-manager's branches: Some(Ok(["master", "release-25.05", "release-25.11"])); picking one ran [FollowBranch { input: "nixos-hardware", branch: "release-25.05" }]`
- **Notes:** the sample source never spawns the GitHub request (`app.rs:911-916`), and the real one would reach the network, so the late reply is injected in the exact shape `choose_branch`'s worker sends. A fix that adds the input or sheet key to `Msg::Branches` will need this `send` updated to the new shape. The helper failing on a branch the repository lacks was not run, because it needs network access.
- **Files:** `desktop/aizome/apps/yukimi/tests/repro_yuk_20.rs`
- **Independent review:** upheld with caveat; re-run: Fails: nixos-hardware chooser lists home-manager's branches; Follow ran; asserts the claim: Yes; reachability: staged: the late GitHub reply injected in the worker's exact shape. `Msg::Branches` has no input or sheet key. Needs the first reply to land after the second chooser opens

<a id="ntu-01"></a>

### NTU-01 — Confirmed

Catalog entry: [NTU-01](nisshi-tokei-utsushi.md#ntu-01), Nisshi system-log unlock (root reader through pkexec).

- **Claim:** if the user opens an unreadable /var/log file, then picks All Messages while the password dialog is up, root `Ready` replays `after_unlock` and switches the window back to the file the user left.
- **Level:** integration
- **Method:** the real window drives the real `aizome-nisshi-helper`, run as the user over the fixture's stand-in journalctl. A stand-in pkexec (`NISSHI_PKEXEC`) holds "the dialog" until a `go` file appears. The test makes a mode-000 `log/secret.log` and sends `OpenFile` (so `Unlock::Asking`), then `Scope(All)`, then gives the password. It asserts that once Unlocked the scope is still `All`. File: `repro_ntu_01.rs` (new `tests/` file).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-nisshi --test repro_ntu_01` (in `nix develop .#aizome`)
- **Observed:** `unlocking switched the window back to the file the user had left` — `left: File(".../nisshi-repro-ntu01-888188/log/secret.log")`, `right: All`
- **Notes:** the real pkexec is never run; the helper runs unprivileged. The stand-in pkexec gives up after 30 s if `go` never appears, so no process is left behind.
- **Files:** `desktop/aizome/apps/nisshi/tests/repro_ntu_01.rs`
- **Independent review:** upheld; re-run: FAIL as reported: scope `File(…secret.log)` ≠ `All`; asserts the claim: Yes; reachability: real. The auth dialog is a fixed-size Overlay layer with exclusive *keyboard* only (`auth.rs:250-254`), so a mouse click on All Messages reaches Nisshi.. Root `Ready` replays `after_unlock` (app.rs:795-797) with no check of the current scope.

<a id="ntu-03"></a>

### NTU-03 — Confirmed

Catalog entry: [NTU-03](nisshi-tokei-utsushi.md#ntu-03), Nisshi journal read pipeline: newest page, earlier pages, follow, whole-boot search.

- **Claim:** if a selected row's fields read fails while the 50,000-entry newest page is still streaming, `Failed` sets `loading = false` with `Want::Load` still pending. "Reading…" vanishes, Read Earlier enables, and a pending `--cursor` gives up early.
- **Level:** unit
- **Method:** a child module of `app` builds `Nisshi::sample`. It sets `loading = true`, `store.complete = false`, pending `(0,1) Want::Load` and `(0,2) Want::Fields(seq)`, and `want_cursor` (as `load`/`fetch`/`--cursor` leave them). It wraps this in the kit `Harness` and sends the reader's `Backend(0, Failed{id:2})`, then more `Entries` for the page. It asserts `loading` stays true, Read Earlier (the menu's `!in_file && !complete && !loading`) stays disabled, and no cursor notice appears. Files: `repro_ntu_03.rs`, plus `INSERT.txt` (one `#[cfg(test)] #[path] mod` line appended to `apps/nisshi/src/app.rs`).
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-nisshi --lib repro_` (in `nix develop .#aizome`)
- **Observed:** `a failed fields read, with the newest page still being read: loading false ("Reading…"), Read Earlier enabled true, --cursor notice Some(("That message isn't among those read: try another boot, or Read Earlier.", true))`
- **Notes:** this is a unit-level reproduction of the handler. Any pending request's `Failed` has the same effect (Boots and Files too), because `app.rs:889-890` clears `loading` for whatever failed. Plausible triggers are a vacuumed cursor or journalctl failing on the one-entry query. "Too many reads at once" needs 8 concurrent reads (`RUNNING = 8`), which is rarer.
- **Files:** `desktop/aizome/apps/nisshi/src/repro_ntu_03.rs`
- **Module line added to:** `desktop/aizome/apps/nisshi/src/app.rs`

<a id="ntu-17"></a>

### NTU-17 — Confirmed

Catalog entry: [NTU-17](nisshi-tokei-utsushi.md#ntu-17), Tokei alarm schedule: alarms.json against systemd transient timers.

- **Claim:** if a once-alarm is ringing when the user opens Tokei, `launched` → `sync_alarms` runs `systemctl --user stop …timer …service`, which kills `--ring` before it turns the alarm off. `schedule` then re-arms the alarm for the same time tomorrow.
- **Level:** integration
- **Method:** Tokei's real `Systemd` scheduler runs over stand-in `systemctl` and `systemd-run` first on PATH. They keep units as files: `stop` deletes them and `list-units` lists timers. The ringing alarm's `aizome-alarm-0000beef.service` exists, and a once-alarm at 21:42 is opened at 21:42:30 Berlin on a fake clock. The test asserts that the service unit survives Tokei's launch. File: `repro_ntu_17.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-tokei --test repro_ntu_17` (in `nix develop .#aizome`)
- **Observed:** `Tokei's launch stopped the ringing alarm's service (its --ring)…` with the calls `systemctl --user stop aizome-alarm-0000beef.timer aizome-alarm-0000beef.service` and `systemd-run … --unit=aizome-alarm-0000beef … --on-calendar=2026-10-18 21:42:00 … --ring 0000beef`
- **Notes:** a stand-in replaces systemd. That `stop` on the service ends the running `--ring` (and so the notification's Snooze/Stop have nothing to answer) is standard systemd behaviour I did not run here. This only applies when Tokei was not already running, since a second launch hands off without syncing. `ring.rs` turns a once-alarm off only after Stop/Timeout, so the killed ring leaves it enabled.
- **Files:** `desktop/aizome/apps/tokei/tests/repro_ntu_17.rs`

<a id="ntu-19"></a>

### NTU-19 — Confirmed

Catalog entry: [NTU-19](nisshi-tokei-utsushi.md#ntu-19), Tokei's in-memory alarm list, its saves and the undo slot.

- **Claim:** if Tokei stays open while a once-alarm rings and is stopped (file now `enabled: false`), then toggling any other alarm saves the window's stale list with the alarm on again, and the next sync arms it.
- **Level:** integration
- **Method:** the real window opens with two alarms and the `Recorder` scheduler. The real `ring::ring_with` (scripted Stop) turns Tea off in alarms.json. The window then gets `ToggleAlarm(gym, false)`. The test asserts that Tea is still off in the file. File: `repro_ntu_19.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-tokei --test repro_ntu_19` (in `nix develop .#aizome`)
- **Observed:** `turning Gym off saved the window's stale list: Tea is on again (window showed it on: true); the next sync arms ["aizome-alarm-0000beef"]`
- **Notes:** none.
- **Files:** `desktop/aizome/apps/tokei/tests/repro_ntu_19.rs`
- **Independent review:** upheld; re-run: FAIL as reported: Tea on again, sync arms `aizome-alarm-0000beef`; asserts the claim: Yes; reachability: real. The real `ring_with` writes `enabled=false`. Tokei loads alarms only in `new` (app.rs:220) and saves its stale list.

<a id="ntu-26"></a>

### NTU-26 — Confirmed

Catalog entry: [NTU-26](nisshi-tokei-utsushi.md#ntu-26), Utsushi playback item lifecycle.

- **Claim:** if another file is opened while a film plays at 45:00, `Msg::Open` replaces `queue` before `load(0)` calls `leave()`. The new file is recorded at the film's position and length and starts there "Resumed". If the film had ended, the new file's own resume point is erased instead.
- **Level:** integration
- **Method:** the real window and `aizome-media` player run headless (null audio, software decode). The film is a generated 400 s quiet WAV. Test 1 plays the film, seeks to 200 s, then opens the engine fixture `song.flac`. It asserts the song gets no resume record and `resumed == None`. Test 2 is the ended variant: the film has a stored place of 100/400, `song.flac` plays to Ended, then the film is opened. It asserts the film resumes at 100 s. File: `repro_ntu_26.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-utsushi --test repro_ntu_26` (in `nix develop .#aizome`)
- **Observed:** (1) `the new file was remembered at the film's place and length (Some(Left { position: 200.0, duration: 400.0, … })) and started "Resumed at" Some(200.0)`; (2) `the ended clip's leave() was recorded under the film: its place is None, it starts at None`
- **Notes:** both halves of the claim reproduce. The film's own place is recorded correctly by the first `leave()`.
- **Files:** `desktop/aizome/apps/utsushi/tests/repro_ntu_26.rs`

<a id="kko-01"></a>

### KKO-01 — Partly confirmed

Catalog entry: [KKO-01](kura-koe.md#kko-01), Dictation on/off: koe.toml `enabled` gating the user unit and the daemon it runs.

- **Claim:** if a crash loop under `Restart=on-failure`/`RestartSec=2` trips systemd's start limit, `systemctl start` fails. `Session::start` then spawns an unmanaged `aizome-koe daemon` with stderr to /dev/null, and the user sees only "Koe's service didn't start" instead of the journal reason.
- **Level:** integration
- **Method:** the real `aizome-koe enable` runs as the session's service would (the default koe.toml and socket, in private XDG dirs, so `unit == true`). A stand-in `systemctl` refuses `start` with systemd's "restarted too often" message, and a stand-in `journalctl` prints the unit's last `aizome-koe: …` error. Test 1 makes the socket folder a file, so the unmanaged daemon fails the same way. It asserts the error names the journal reason or systemd's refusal. Test 2 lets the daemon start. It asserts that no daemon answers on the socket after systemd refused the unit. File: `repro_kko_01.rs`. Cleanup runs `aizome-koe disable` to shut down the stray daemon over its socket.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-koe --test repro_kko_01` (in `nix develop .#aizome`)
- **Observed:** (1) `Koe said only "aizome-koe: Koe's service didn't start\n"` after the fixed 10 s wait, and journalctl was never asked. (2) `systemd refused the unit, yet a daemon answers on the session's socket, started outside it (stdout "Dictation's shortcut: ⌥❖D\nDictation is on\n", …)`
- **Notes:** Koe's side reproduces: any `systemctl start` failure falls back to a daemon outside the unit with stderr discarded, `why()` is skipped, and the message is generic. The trigger does not reproduce here and needs a real systemd user manager. By the defaults (5 starts per 10 s, a fixed window), restarts 2 s apart alone stay within the limit. Tripping it needs extra manual starts during the loop, for example the shortcut or the switch, which the claim does mention. Whether falling back to an unmanaged daemon is wrong when a unit exists is a design call; the code documents it as the "no unit" path.
- **Files:** `desktop/aizome/apps/koe/tests/repro_kko_01.rs`

<a id="kko-30"></a>

### KKO-30 — Confirmed

Catalog entry: [KKO-30](kura-koe.md#kko-30), Kura scan lifecycle and page.

- **Claim:** if a second scan starts while one runs, the first scan's `Scanned(Err(Cancelled))` tears down the second's `scanning` and timer. The window shows "The scan didn't finish" while the second scan runs unseen and can't be stopped, and its result later replaces the page unannounced.
- **Level:** integration
- **Method:** the real window and scanner run over two small temporary trees (400 and 6,000 empty files). `send_all([Scan(first), Scan(second)])` starts both scans before either answer is taken in. The test settles until the window stops showing the second scan, then asserts that this happened because of the second scan's own result. File: `repro_kko_30.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-kura --test repro_kko_30` (in `nix develop .#aizome`)
- **Observed:** `left: (Disks, Some("The scan didn't finish: the scan was stopped."), None)` vs `right: (Results, None, Some(".../second"))`; `Stop possible: false; its result came later: true, then showing (Results, Some("The scan didn't finish: the scan was stopped."))`
- **Notes:** the Rescan that starts a third scan without cancelling one was not exercised. It follows from `scanning == None` (Rescan is enabled only when `!scanning`).
- **Files:** `desktop/aizome/apps/kura/tests/repro_kko_30.rs`

<a id="kko-31"></a>

### KKO-31 — Confirmed

Catalog entry: [KKO-31](kura-koe.md#kko-31), Analysis and index writing after a scan or a change.

- **Claim:** if a first scan's slow analysis (duplicate hashing) finishes after a later scan's analysis, its `Indexed` lands last. The Suggestions show the earlier tree's hints, and index.json and index.md describe the older scan.
- **Level:** integration
- **Method:** the real window and analysis (`duplicates` on, `min_bytes = 1`) run over two small trees. Tree one has a duplicate pair plus a same-size `slow.bin`. After the scan, which the test runs with the real scanner before handing its result in, `slow.bin` is swapped for a FIFO. Analysis one then blocks reading it, which is a deterministic barrier. The test scans tree two to completion, then opens the FIFO's write end so analysis one finishes. It asserts the Suggestions and index.json still describe tree two. File: `repro_kko_31.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-kura --test repro_kko_31` (in `nix develop .#aizome`)
- **Observed:** `the Suggestions show ["duplicates"] about .../one (stale true), and index.json's root is .../one` (tree two's analysis had written index.json first; the stale one overwrote it)
- **Notes:** the FIFO stands in for slow hashing. The shown tree is tree two, so the stale hints come with Move to Trash for paths of a tree no longer shown.
- **Files:** `desktop/aizome/apps/kura/tests/repro_kko_31.rs`
- **Independent review:** upheld; re-run: FAIL as reported: Suggestions and index.json describe `/one`; asserts the claim: Yes; reachability: staged. A FIFO holds analysis one. A slow duplicate hash of a big tree, plus the always-enabled Scan buttons (view.rs:92, :143), gives the same order.. `Indexed` carries no generation, so the last result to arrive wins.

<a id="tsh-02"></a>

### TSH-02 — Confirmed

Catalog entry: [TSH-02](tansu-shiori.md#tsh-02), Shiori document open and worker-restart lifecycle.

- **Claim:** if the supervisor's respawn after a crash fails, it sends `OpenFailed(Worker)`, and `app.rs:1167` excludes Worker errors. The showing document becomes `Load::Failed`, and Reload reopens it as a new document at the top.
- **Level:** integration
- **Method:** real hayro workers run through a symlink to the worker program. The test opens a 12-page document and goes to page 9. It then removes the link and SIGKILLs the worker it started, so the respawn fails with ENOENT. It asserts the document stays `Open`. It then restores the link, sends Reload, and asserts the document is back on page 9. File: `repro_tsh_02.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shiori --test repro_tsh_02` (in `nix develop .#aizome`)
- **Observed:** `the document that showed became Failed("Shiori's worker couldn't be started: …/worker: No such file or directory (os error 2)"); Reload then opened it on page 1 (it showed page 9)`
- **Notes:** the trigger is ENOENT, not EAGAIN/ENOMEM, but it takes the same `spawn` Err path. ENOENT is realistic: `worker` is `current_exe()`, which a system upgrade followed by garbage collection removes while Shiori runs. The claim's "find/copy stay pending" part was not exercised.
- **Files:** `desktop/aizome/apps/shiori/tests/repro_tsh_02.rs`

<a id="tsh-08"></a>

### TSH-08 — Confirmed

Catalog entry: [TSH-08](tansu-shiori.md#tsh-08), Copy waiting for pages' text.

- **Claim:** if the worker gives up (`engine` None, Load still Open) and the user copies a selection over pages whose text isn't loaded, `copy` stores a `Copying` that `ask_copy_text` can never feed. Nothing is copied, nothing says why, and `idle()` stays false.
- **Level:** integration
- **Method:** real workers run on a 30-page document. The worker is lost the way the existing tests lose it (the file is mode 000 when the restarted worker opens it), leaving `engine` None and `Load::Open`. The test sets a page 1–20 selection directly; page 20's text was never read. It sends `Copy`, advances 2 s, and asserts no copy waits and Shiori is idle. File: `repro_tsh_08.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shiori --test repro_tsh_08` (in `nix develop .#aizome`)
- **Observed:** `the copy waits for Some(18) pages' text that no worker will read (clipboard None, status Some("The page reader couldn't open the file again; Reload (❖R) tries again."), idle false)`
- **Notes:** the selection is set directly in place of a drag made while the worker lived.
- **Files:** `desktop/aizome/apps/shiori/tests/repro_tsh_08.rs`

<a id="tsh-09"></a>

### TSH-09 — Confirmed

Catalog entry: [TSH-09](tansu-shiori.md#tsh-09), Find session.

- **Claim:** if the user closes Find and reopens it, the new `Find` restarts its serial at 0 and `run_find` makes it 1 again. The worker's late `Found{serial:1}` for the old query passes the filter, is stored as a new-query result, and sets the current hit. `current` then points past that page's real hits.
- **Level:** integration
- **Method:** real workers run on the four-page sample. The test searches "needle", closes Find, reopens it and types "wide" (which is on page 3 only). In the turn the new search starts (`FindRun`), it delivers the worker's own earlier "needle" `Found` for page 2. That matches the ⌘F-with-selection path, which searches without the 200 ms debounce, or a page that takes longer than the debounce. The worker sends the in-flight page's `Found` after StopFind (`worker.rs:199-203`). The test asserts page 2 gets no hits and that the current hit is page 3's. File: `repro_tsh_09.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shiori --test repro_tsh_09` (in `nix develop .#aizome`)
- **Observed:** `serial 1 (the closed one's was 1), page 2 given 2 "needle" hits, current Some((1, 0)); once the worker answered "wide": page 2 has Some(0) hits, current Some((1, 0))`
- **Notes:** the late message is injected, but it is exactly what the worker sent. The view did not visibly move because page 2 was already in view.
- **Files:** `desktop/aizome/apps/shiori/tests/repro_tsh_09.rs`
- **Independent review:** upheld with caveat; re-run: FAIL as reported: serial 1 = 1, page 2 gets 2 "needle" hits, current (1,0); asserts the claim: Yes; reachability: staged. The late `Found` is injected. The worker does send the in-flight page's `Found` after StopFind (worker.rs:94-97, :199-203), and ⌘F with a selection runs `run_find` immediately (app.rs:1625-1632). This needs a page slower than Esc+⌘F.. The mechanism is exact. The timing window needs a slow PDF page.

<a id="tsh-25"></a>

### TSH-25 — Confirmed

Catalog entry: [TSH-25](tansu-shiori.md#tsh-25), Tansu sheet lifecycle (forms holding segment indexes).

- **Claim:** if the stick is unplugged while Format… is open on its partition, `refresh` falls back to the first drive and the sheet's `seg` now indexes that drive's layout. Add to Pending queues a format of another partition, with no typed name if that drive is removable.
- **Level:** integration
- **Method:** the in-memory mock only, using the sample disks reordered so the SD card is listed first and is the fallback. The test selects the Cruzer's `sdc2`, unmounts it, and opens Format…. It then unplugs the stick (the mock's `power_off` removes it as UDisks would) and sends `Changed`, then Submit. It asserts nothing is queued on a disk other than the stick. File: `repro_tsh_25.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-tansu --test repro_tsh_25` (in `nix develop .#aizome`)
- **Observed:** `the gone stick's Format sheet (its segment now Some("mmcblk0p1") on Some("SD SC64G")) queued ["Format on /dev/mmcblk0p1"]; Apply would ask to type []`
- **Notes:** the drive order was chosen to make the fallback another removable disk, the claim's worst case. The default sample order (NVMe first) and the Resize, Label, Flags and Unlock variants were not run. No real device was involved.
- **Files:** `desktop/aizome/apps/tansu/tests/repro_tsh_25.rs`

<a id="tsh-26"></a>

### TSH-26 — Confirmed

Catalog entry: [TSH-26](tansu-shiori.md#tsh-26), Tansu apply session (Apply sheet → Run → Running sheet).

- **Claim:** if a second launch hands over a path during a run, `open_sheet(Attach)` closes the Running sheet. When the run finishes nothing clears `run`: every action answers "Wait for the operations to finish.", and the status line keeps resetting to "Applied N operations."
- **Level:** integration
- **Method:** the mock sits behind a stand-in `Backend` that delays reporting its state, as UDisks reports a step's effect late, so the run waits mid-step. The test queues Label on `sdc1` and applies, then sends `Launched([image.iso])`. It releases the state so the run ends, dismisses the Attach sheet, and sends two more messages. It asserts `can(Label)` is Ok and `run` is cleared. File: `repro_tsh_26.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-tansu --test repro_tsh_26` (in `nix develop .#aizome`)
- **Observed:** `after the run ended under the Some("attach") sheet, Tansu is stuck: Err("Wait for the operations to finish.") (run kept: true), the status line still says Some(("Applied 1 operation.", false))`; the mock's calls were only `SetLabel(PHOTOS) /dev/sdc1`
- **Notes:** the image path is never opened or attached.
- **Files:** `desktop/aizome/apps/tansu/tests/repro_tsh_26.rs`

<a id="smt-02"></a>

### SMT-02 — Confirmed

Catalog entry: [SMT-02](sumi-myaku-tenki.md#smt-02), Table selection, expansion and launch request across readings.

- **Claim:** A row's context-menu Inspect carries the row index from the right-click, so if a reading re-sorts the table before you click it, Inspect selects and inspects whichever row now sits at that index.
- **Level:** integration (real Myaku in the kit's headless `Harness`, with real pointer and popup-keyboard events and fixture readings in place of the sampler)
- **Method:** The test right-clicks the group at row 3 (Signal), which selects it and opens its menu. It then sends a reading that makes Signal the busiest group, so it sorts to row 0 and Spotify moves to row 3. The menu is still open at this point (checked). It then picks Inspect (ArrowDown, Enter in the popup) and asserts that the selection is still Signal and the inspector is open. File: `repro_smt_02.rs`
- **Command:** `cargo test --locked --no-fail-fast --manifest-path desktop/aizome/Cargo.toml -p aizome-myaku --test repro_smt_02` (in `nix develop .#aizome`)
- **Observed:** `Inspect on Signal's row inspects Signal, not Some("Spotify") (now at row 3)`, `left: Some(Group((39, 3939)))`, `right: Some(Group((21, 2121)))`
- **Notes:** The open popup survives the re-render caused by a reading, so a real user can follow this path. Pressing Return on the table (`on_activate`) uses an index taken at the time of the key press, so it does not have this race.
- **Files:** `desktop/aizome/apps/myaku/tests/repro_smt_02.rs`
- **Independent review:** upheld; re-run: FAIL as reported: Spotify inspected instead of Signal; asserts the claim: Yes; reachability: real (right-click, a reading re-sorts, Inspect through the popup keyboard). The menu item carries the index captured at right-click.

<a id="smt-03"></a>

### SMT-03 — Confirmed

Catalog entry: [SMT-03](sumi-myaku-tenki.md#smt-03), Quit/pause confirmation and the status notice.

- **Claim:** `notice` is never cleared, so after a refused or completed quit or pause, the status line keeps showing that message instead of the process/thread/uptime summary for the rest of the session.
- **Level:** integration (headless `Harness`, fixture readings, recorder signaller)
- **Method:** The test selects a root-owned process and sends Pause, which is refused with a red notice. It then selects Kura and runs 60 readings, advancing the clock 1 s after each. It asserts the refusal text is gone and the summary is shown. A control test checks that the expected summary string does appear when `notice` is None, so the test can pass once this is fixed. A second test covers a hazard from the entry: Quit is confirmed after Signal's first process ended (the group's key changed in the next reading). It asserts that the result is not reported as a success. File: `repro_smt_03.rs`
- **Command:** `cargo test --locked --no-fail-fast --manifest-path desktop/aizome/Cargo.toml -p aizome-myaku --test repro_smt_03` (in `nix develop .#aizome`)
- **Observed:** `a minute and a new selection later the status still says "It belongs to another user: quit it as them, or as root." (notice Some((…, true)))`. Second test: `nothing was signalled, yet the status reports Some(("Asked to quit: .", false))`. The control passes.
- **Notes:** The test accepts any fix that clears the notice within a minute of a new selection, whether by clearing on selection change, on a later reading, or on a timer. Exactly when the notice should clear is a design choice. The "Asked to quit: ." result (a success notice with no name, and nothing signalled) also confirms the entry's hazard that targets are re-resolved at confirm time.
- **Files:** `desktop/aizome/apps/myaku/tests/repro_smt_03.rs`

<a id="smt-13"></a>

### SMT-13 — Confirmed

Catalog entry: [SMT-13](sumi-myaku-tenki.md#smt-13), Close-after-save for dirty documents.

- **Claim:** If you choose Close → Save and the save is stopped by another alert (Latin-1 can't hold the text, or the write fails), the window stays in `closing`, so a later ordinary ⌘S saves and then closes the window without asking.
- **Level:** integration (headless `Harness`, real files in a scratch folder)
- **Method:** 
  - *Latin-1 case:* open a Latin-1 file and type "ok ✓". Press ⌘W, then Return (Save). The "can't be saved in Latin-1" alert appears; press Escape (Cancel). The window stays open and the file is unchanged. Then switch Encoding to UTF-8 and press ⌘S. The test asserts the file is saved and the window is still open.
  - *Write-error case:* make the file 0444 and its folder 0555 (the test checks first that the write really fails). Press ⌘W, then Save. The "couldn't be saved" alert appears; press OK. Restore the permissions, press ⌘S, and assert the window is still open.
  - File: `repro_smt_13.rs`
- **Command:** `cargo test --locked --no-fail-fast --manifest-path desktop/aizome/Cargo.toml -p aizome-sumi --test repro_smt_13` (in `nix develop .#aizome`)
- **Observed:** Both tests fail with `an ordinary ⌘S closed the window it saved`. In each, the assertion just before it, `⌘S saved`, passed.
- **Notes:** The "changed by another program" alert's Cancel uses the same path (`save` returns false and `closing` is not cleared), but I did not test it separately. While the file still differs on disk, a later ⌘S raises that alert again, so the unwanted close happens after Save Anyway, or after a Revert followed by an edit and a save.
- **Files:** `desktop/aizome/apps/sumi/tests/repro_smt_13.rs`

<a id="smt-14"></a>

### SMT-14 — Confirmed

Catalog entry: [SMT-14](sumi-myaku-tenki.md#smt-14), Link and Go to Line sheets.

- **Claim:** With a Link sheet (or Go to Line sheet) open over each of two documents, the field messages pick the first sheet of that kind in a `HashMap`. Typing, Insert Link and Go then edit, apply or close the wrong sheet.
- **Level:** integration (headless `Harness`; typing goes into each sheet's own field window)
- **Method:** Two Markdown documents, A ("alpha ") and B ("beta "). Open Link… over A, then over B. Click into A's sheet's text field and type "AAA", then into B's and type "BBB". Press Return in B's sheet. The test asserts that each sheet holds its own text, that B gets the link, that A is unchanged, and that only B's sheet closes. A second test does the same with Go to Line: type "2" in A's sheet and "4" in B's, press Go in B's, and expect B's caret on line 4 with A's sheet still open. File: `repro_smt_14.rs`
- **Command:** `cargo test --locked --no-fail-fast --manifest-path desktop/aizome/Cargo.toml -p aizome-sumi --test repro_smt_14` (in `nix develop .#aizome`)
- **Observed:** 
  - Link: `left: ((Some("B"), Some("")), ("alpha [B]()", false, false, true))`, `right: ((Some("AAA"), Some("BBB")), ("alpha ", true, true, false))`. B's keystrokes overwrote A's sheet, B's field stayed empty, Insert Link in B's sheet inserted `[B]()` into A and closed A's sheet, and B's sheet stayed open.
  - Go to Line (where the map happened to put B first): `left: ((Some(""), Some("24")), (0, 4, true, false))`. The "2" typed in A went to B's sheet, so Go in B used "24" (clamped to line 5).
- **Notes:** Which sheet wins depends on `HashMap`'s random order and changes from run to run. The test still fails every time, because `sheet_of` returns the same key for both sheets, so one of the two sheets is always wrong. The entry's second hazard (the Link range is fixed when the sheet opens, while commands can still change the text) was not tested.
- **Files:** `desktop/aizome/apps/sumi/tests/repro_smt_14.rs`
- **Independent review:** upheld; re-run: FAIL ×2 (A's sheet text lost; Go used "24"); the losing sheet varies with HashMap order, as reported; asserts the claim: Yes; reachability: real (two windows, each with its own sheet). `sheet_of` returns one key for both sheets.

<a id="smt-17"></a>

### SMT-17 — Confirmed

Catalog entry: [SMT-17](sumi-myaku-tenki.md#smt-17), Document file, dirty and autosave lifecycle.

- **Claim:** If a document's only unsaved change is its Encoding or Line Endings, or it was reopened in a chosen encoding, Quit and relaunch reloads it in the detected encoding and clean, so that choice is lost.
- **Level:** integration (headless `Harness`, two launches sharing a real state folder)
- **Method:** 
  - *Format change:* open a UTF-8/LF file, set the encoding to UTF-16 LE and line endings to CRLF (the title shows "— Edited"), Quit, and launch again with the same `Places`. The test asserts the path, encoding, line endings, dirty flag and title.
  - *Reopen:* open a UTF-8 "café" file, use Reopen ▸ Latin-1 (giving "cafÃ©", clean), Quit, and relaunch. The test asserts the encoding and text.
  - File: `repro_smt_17.rs`
- **Command:** `cargo test --locked --no-fail-fast --manifest-path desktop/aizome/Cargo.toml -p aizome-sumi --test repro_smt_17` (in `nix develop .#aizome`)
- **Observed:** 
  - Format change: `left: (Some(".../plain.txt"), Utf8, Lf, false, "plain.txt")`, `right: (…, Utf16Le, Crlf, true, "plain.txt — Edited")`
  - Reopen: `left: (…, Utf8, "café\n")`, `right: (…, Latin1, "cafÃ©\n")`
- **Notes:** The session does record `encoding` and `line_ending`. However, `restore` ignores them for a document without unsaved text (it calls `doc::load(&path, None)`), and `format_dirty` is not saved at all. The entry's other hazard (the background reload doesn't remove the autosave text) was not tested.
- **Files:** `desktop/aizome/apps/sumi/tests/repro_smt_17.rs`

<a id="smt-18"></a>

### SMT-18 — Confirmed

Catalog entry: [SMT-18](sumi-myaku-tenki.md#smt-18), Background file read generation.

- **Claim:** A background read (over 2 MiB) that fails clears the path and sets the document to Untitled, but leaves the file's name as the window title. A failed Revert of a big document detaches it from its file, whereas a small one keeps its path.
- **Level:** integration (headless `Harness`, real 3 MiB files made unreadable with chmod 000; the tests run as uid 1000 and check that the file really can't be opened)
- **Method:** 
  - *Failed open:* open an unreadable 3 MiB file into the starting Untitled window. Wait for the background read to finish, then assert that the "couldn't be opened" alert appears and that the window title matches `Doc::title()`.
  - *Failed Revert:* open a small file and, separately, a 3 MiB file. Make each unreadable and send `Msg::Reload(w, None)`, the message the Revert button sends. Compare the path and title afterwards.
  - File: `repro_smt_18.rs`
- **Command:** `cargo test --locked --no-fail-fast --manifest-path desktop/aizome/Cargo.toml -p aizome-sumi --test repro_smt_18` (in `nix develop .#aizome`)
- **Observed:** 
  - Failed open: `left: ("root-only.log", None, 0)`, `right: ("Untitled", None, 0)`
  - Failed Revert: `[("small.txt", Some(".../small.txt"), "small.txt"), ("big.log", None, "big.log")]`, while the expected big entry is `("big.log", Some(".../big.log"), "big.log")`. The big document lost its path but kept the title "big.log".
- **Notes:** A failed Reopen with Encoding goes through the same `Msg::Reload` path. The entry's other hazard (Format, Replace All, Shift Right and Link edits are not blocked during a load) was not tested.
- **Files:** `desktop/aizome/apps/sumi/tests/repro_smt_18.rs`

<a id="smt-19"></a>

### SMT-19 — Confirmed

Catalog entry: [SMT-19](sumi-myaku-tenki.md#smt-19), Find bar and search generation.

- **Claim:** After Done or Escape, the find matches are still passed to the editor and are not updated after edits, so highlights drift onto other text. In a document over 1 MiB, ⌘G right after an edit uses the pre-edit match list.
- **Level:** integration (headless `Harness`, with rendered pixels used to show the marks are painted)
- **Method:** 
  - *Drift:* type "needle one\nneedle two\nthree\n". Press ⌘F, type "needle" (2 matches), then Escape. Put the caret at 0 and type a new first line. Collect the text under each mark the editor is given. Then render the window with those marks, clear the query (while the bar stays hidden) and render again. The renders differ, which shows the marks are painted. The test asserts that any painted mark covers "needle".
  - *Big document:* open a file of about 1.1 MiB with "needle" on its first and last lines, show Find, search "needle", and wait for the background search to finish. Insert "> " at 0, send FindNext at once, and assert the selected text is "needle".
  - File: `repro_smt_19.rs`
- **Command:** `cargo test --locked --no-fail-fast --manifest-path desktop/aizome/Cargo.toml -p aizome-sumi --test repro_smt_19` (in `nix develop .#aizome`)
- **Observed:** 
  - Drift: `after Done and an edit the editor paints 2 mark(s) over ["a new ", " line\n"] in "a new first line\nneedle one\nneedle two\nthree\n"`
  - Big document: `⌘G selected 1134573..1134579, "r\nneed"`. That is the last needle's old position, shifted by the two inserted characters. The correct result is the first needle at 2..8.
- **Notes:** Whether highlights should stay visible at all after Done is a design question, since view.rs drops only the current match when the bar is hidden. The test accepts either fix: clear the marks, or keep them correct. The `FindQuery` stale-current hazard and the lack of cancellation for background searches were not tested.
- **Files:** `desktop/aizome/apps/sumi/tests/repro_smt_19.rs`
- **Independent review:** upheld; re-run: FAIL ×2 as reported: marks over "a new ", " line\n"; ⌘G selects `"r\nneed"`; asserts the claim: Yes; reachability: real. The marks pass to the editor even when the bar is hidden (view.rs:161). Above 1 MiB, `find_step` uses stale ranges.

<a id="smt-24"></a>

### SMT-24 — Confirmed

Catalog entry: [SMT-24](sumi-myaku-tenki.md#smt-24), Location consent and GeoClue lookup.

- **Claim:** Turning the current location off never clears `current`, so the last GeoClue position stays in `state.json` and is written there again whenever a place is selected.
- **Level:** integration (headless `Harness`, the `Recorded` Open-Meteo/GeoClue fixtures and a real `Store` on disk; no network, no D-Bus)
- **Method:** Start from a store with Tokyo and Chicago and location declined. Send AllowLocation, which resolves inline to 35.69, 139.69 and writes it to `state.json` (precondition). Then turn location off in each of three ways: ToggleLocation (Show Current Location), Remove(0) (removing the current row) and DeclineLocation. Read `state.json`. Then overwrite `state.json` with `located: None`, as if turning it off had cleared it, select Chicago, and read it again. The test asserts None both times. File: `repro_smt_24.rs`
- **Command:** `cargo test --locked --no-fail-fast --manifest-path desktop/aizome/Cargo.toml -p aizome-tenki --test repro_smt_24` (in `nix develop .#aizome`)
- **Observed:** `left: [("toggle", Some((35.69, 139.69)), Some((35.69, 139.69))), ("remove", …same…), ("decline", …same…)]`, `right: [("toggle", None, None), …]`. All three paths still hold the position in memory: `[("toggle", Some((35.69, 139.69))), …]`.
- **Notes:** Both halves reproduce: the position is not cleared when location is turned off, and selecting a place writes it back from memory. "Not Now" (DeclineLocation) is only shown while consent is Ask, so after Allow the user-facing ways to turn it off are the toggle and removing the row. DeclineLocation was tested by calling its handler. `State.located` is documented as a cache for showing the forecast offline. Keeping it after the user declines is the privacy concern the existing test at tenki.rs:596 guards against for late answers.
- **Files:** `desktop/aizome/apps/tenki/tests/repro_smt_24.rs`

<a id="sap-02"></a>

### SAP-02 — Confirmed

Catalog entry: [SAP-02](small-apps.md#sap-02), Single-instance socket claim and hand-off (Annai, Shashin, Soroban; Fusen through Sumi's copy).

- **Claim:** two launches started together both fail to connect, and the second's `remove_file` unlinks the first's freshly bound socket (or its bind fails), so two instances run and the first can no longer be raised or handed files.
- **Level:** unit
- **Method:** in each of the four copies of the claim code, threads released together by a `Barrier` call the real `claim` (Annai, Shashin, Soroban) or `aizome_sumi::instance::claim_at` (Fusen's copy) on a fresh socket name per round. The test counts the launches that would keep running (`First` or `Alone`) and asserts that exactly one does per round. When two get `First`, a probe connection checks which listener the socket path still reaches. Files: `{annai,shashin,soroban,fusen}/repro_sap_02.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shashin -p aizome-annai -p aizome-soroban -p aizome-fusen --test repro_sap_02 --no-fail-fast` (in `nix develop .#aizome`)
- **Observed:** with 2 launches × 1000 rounds: Shashin `898 of 1000 rounds of 2 simultaneous launches ran more than one Shashin (609 instances left unreachable by the socket); first rounds (round, First, Alone): [(1, 2, 0), (2, 1, 1), (3, 2, 0), …]`; Annai 882/1000 (446 unreachable); Soroban 860/1000 (494); Fusen/Sumi 878/1000 (532). An earlier run with 8 launches × 300 rounds failed 299–300 of 300 rounds in every copy.
- **Notes:** two outcomes reproduce. `(2, 0)` is the claimed one: the second launch unlinks the first's socket and binds its own, and the first instance becomes unreachable. `(1, 1)` is a variant: the second launch's bind fails with EADDRINUSE and it runs `Alone`, unserved. Both leave two windows. The test uses threads in one process. Real launches are separate processes with millisecond start-up jitter, while the window between a failed connect and the bind is microseconds, so real collisions are rarer than this rate. The mechanism is the real code, though, and it is identical in all four copies.
- **Files:** `desktop/aizome/apps/annai/tests/repro_sap_02.rs`, `desktop/aizome/apps/fusen/tests/repro_sap_02.rs`, `desktop/aizome/apps/shashin/tests/repro_sap_02.rs`, `desktop/aizome/apps/soroban/tests/repro_sap_02.rs`
- **Independent review:** upheld with caveat; re-run: FAIL ×4 as reported: about 86–90% of 1000 rounds ran two instances; asserts the claim: Yes; reachability: staged. A barrier releases threads in one process. A real trigger exists: Hako opens a multi-selection as one launch per file, all at once (app.rs:966-967, `launch_app` spawns a thread each).. Real non-atomic probe/unlink/bind in all four copies. Real-world collision rate is unknown (microsecond window against process start-up jitter).

<a id="sap-05"></a>

### SAP-05 — Confirmed

Catalog entry: [SAP-05](small-apps.md#sap-05), Fusen stickies: windows, saved places and the login item.

- **Claim:** restoring N stickies at login rewrites `stickies.json` with `place: None` for stickies 2..N (each `open_sticky` → `save_stickies` looks places up in the list it is shrinking), and nothing restores them without Aizome's IPC or after a quit within 4 s.
- **Level:** integration
- **Method:** the test writes three notes and a `stickies.json` that gives each sticky a place (300×260, desk 2). The real `Fusen` starts with `--stickies` in the kit's headless `Harness` (`compositor: false`, so there is no IPC, as in a session without Aizome's compositor) and quits at once. The test asserts that the file still holds every place and that at the next login every sticky opens at 300×260. Files: `repro_sap_05.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-fusen --test repro_sap_05` (in `nix develop .#aizome`)
- **Observed:** `restoring rewrote stickies.json without the places of ["Beta.md", "Gamma.md"]; at the next login [("Beta", 260.0, 240.0), ("Gamma", 260.0, 240.0)] (of [("Alpha", 300.0, 260.0), …]) opened at the default size`. The file has `"note": "Beta.md", "place": null` and `"note": "Gamma.md", "place": null`.
- **Notes:** at the first login the windows themselves get the right size, because `open_sticky` receives each place from the cloned list; only the saved record loses them. With Aizome's IPC, the first `Track` → `Placed` (after 4 s) would write the places back, since the entries still exist with `place: None`. That path was not exercised.
- **Files:** `desktop/aizome/apps/fusen/tests/repro_sap_05.rs`

<a id="sap-06"></a>

### SAP-06 — Confirmed

Catalog entry: [SAP-06](small-apps.md#sap-06), Fusen launch, request hand-off and Drop-time save.

- **Claim:** a second launch while Fusen runs builds a full `Fusen`, gets `Claim::Handed`, and its `Drop` → `save_all` → `save_stickies` writes `{"stickies": []}` and deletes the stickies' login item.
- **Level:** end-to-end
- **Method:** the running Fusen is the real program in the headless `Harness`, serving the instance socket at `$XDG_RUNTIME_DIR/aizome-app.fusen.sock`, with one sticky open. That leaves `stickies.json` and `autostart/org.aizome.Fusen.Stickies.desktop` in place. The second launch is the real `aizome-fusen --new "From Services\n…"` binary (`CARGO_BIN_EXE_aizome-fusen`), with the same private HOME and XDG dirs and no display. The test waits until the running Fusen has written the handed note, then asserts that the sticky list and the login item are intact. Files: `repro_sap_06.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-fusen --test repro_sap_06` (in `nix develop .#aizome`)
- **Observed:** `after a second launch handed its request over: stickies.json = "{\n  \"stickies\": []\n}", login item present = false`. The hand-off itself worked: `Notes/From Services.md` was created by the running instance.
- **Notes:** the running Fusen does not repair the files when it takes the request (`new_note` doesn't call `save_stickies`). Its next `save_stickies` does: an autosave, a sticky opened or closed, Float, Delete, or its own clean quit through `Drop`. So the loss persists only if it is killed or the session ends without `Drop` before then. The test's final step (a kill simulated with `mem::forget`, then the next `--stickies` login) was not reached, because the earlier assertion failed. With the observed empty list, `restore_stickies` has nothing to open.
- **Files:** `desktop/aizome/apps/fusen/tests/repro_sap_06.rs`
- **Fixed:** in [#301](https://github.com/bitemyapp/sekka/pull/301), which adopts this test as `desktop/aizome/apps/fusen/tests/second_launch.rs`. With the fix the final step is reached, and the sticky comes back at the next login.

<a id="sap-07"></a>

### SAP-07 — Confirmed

Catalog entry: [SAP-07](small-apps.md#sap-07), Fusen highlighting passes (incomplete, scheduled, notify).

- **Claim:** Fusen's highlighter never sets `scheduled` or calls `notify`, so once a pass sets `incomplete`, `Msg::Highlight` never comes. `state_at` then guesses for every line after the last edit, and fenced code under an edit shows as prose.
- **Level:** integration
- **Method:** the test opens a 4004-line note that is one fenced block (opened on line 1) in the real Fusen in the `Harness`. It clicks into the editor, presses ❖↓ and renders, so the editor styles the far end and the pass guesses (precondition: `incomplete` is set). It then gives the program 100 render-and-settle passes, returns to the top, moves down 20 lines and types a character. Finally it asks Fusen's own `view::highlighter` for lines 21–25 and asserts that they are styled as code (mono). Files: `repro_sap_07.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-fusen --test repro_sap_07` (in `nix develop .#aizome`)
- **Observed:** `lines [21, 22, 23, 24, 25] inside the fence show as prose after an edit on line 20 (incomplete still set after 100 passes: true; states valid to line 20)`
- **Notes:** before the jump the same lines are code (precondition asserted). The trigger needs a jump more than ~3000 lines past the worked-out states (`GUESSED_RATE` 1000 lines/ms × the 3 ms budget) or a budget overrun, so only long notes are affected. After that, every edit leaves the lines below it guessed. Only `reload` (another program changed the file) or a formatting command (`apply`) resets it.
- **Files:** `desktop/aizome/apps/fusen/tests/repro_sap_07.rs`
- **Independent review:** upheld; re-run: FAIL as reported: lines 21–25 prose, `incomplete` still set after 100 passes; asserts the claim: Yes; reachability: real (open a 4000-line note, ❖↓, ❖↑, edit). Fusen never schedules or notifies, so `incomplete` sticks and `state_at` guesses from then on.

<a id="sap-10"></a>

### SAP-10 — Confirmed

Catalog entry: [SAP-10](small-apps.md#sap-10), Shashin picture loading (serial, decode, folder listing).

- **Claim:** open folder A, then folder B before A is listed: A's late `Listed` passes while nothing shows (no generation), shows A's first picture, and B's listing is then dropped by the same-folder check, so asking for B ends up showing A.
- **Level:** integration
- **Method:** the real Shashin runs in the `Harness`. The test sends `Msg::Open([A])`, waits 300 ms so A's listing job finishes (its reply then waits in the kit's background queue), and sends `Msg::Raise([B])` (a second launch's hand-off). The kit's `flush` handles the queued `Raise` before draining background replies, so B is asked for first and A's `Listed` lands after. The test settles and asserts that the picture shown is from B. Files: `repro_sap_10.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shashin --test repro_sap_10` (in `nix develop .#aizome`)
- **Observed:** `asked for B last, shown from Some(".../repro-sap-10-a-…"): ["app.ico", "beach.png", "dance.gif", "IMG_2.jpg", "IMG_10.jpg", "logo.svg", "old.bmp", "scan.tiff", "sticker.webp"]`. This was A's list, and the result was the same in 3 of 3 runs.
- **Notes:** the 300 ms wait only lets the background listing finish; the order of handling is then deterministic. In real use, A's listing has to land after B is asked for but before B's listing does. That happens when A is still listing as B is dropped or handed over (a big or slow folder A) and B's listing takes longer than what is left of A's, or when A's listing finishes in the same event-loop turn as B arrives.
- **Files:** `desktop/aizome/apps/shashin/tests/repro_sap_10.rs`

<a id="sap-11"></a>

### SAP-11 — Confirmed

Catalog entry: [SAP-11](small-apps.md#sap-11), Shashin unsaved turns, saving and quitting.

- **Claim:** two saves of the same unsaved turn (❖S twice during a slow rewrite, or ❖Q → Save, then ❖Q → Save again while the first `SaveAllAndQuit` runs) each run `save_turned` from the file's current pixels, so a picture turned 90° is saved at 180°.
- **Level:** integration
- **Method:** two tests drive the real Shashin in the `Harness`. (1) A 96×64 PNG is turned once and `Msg::Save` (what ❖S sends) is sent. The test waits until the rewrite is on disk, before `Saved` is handled, sends `Msg::Save` again, then asserts the file is 64×96 and equals `rotate90` of the original. (2) Two 4000×3000 PNGs are turned. ❖Q (a close request) opens the alert, and its Save button is clicked. The test waits until the job has written the first picture while it is still on the other (precondition asserted), sends ❖Q again (the alert asks again because the turns are still set), clicks Save, and asserts the first picture is 3000×4000. Files: `repro_sap_11.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shashin --test repro_sap_11` (in `nix develop .#aizome`)
- **Observed:** (1) `turned 90° once, saved as (96, 64)` (left `(96, 64)`, right `(64, 96)`). (2) `b.png, turned 90° once, was saved as (4000, 3000) (the other picture was already written when Save was asked again: false)`. Both failed the same way in 3 of 3 runs.
- **Notes:** in (1), the second job must read the file after the first job wrote it. That happens when its job waits for a pool thread (read-ahead or thumbnails busy) or starts after the first finished but before `Saved` is handled. Two jobs that truly run at once both read the original and both write 90°, but they share the temp name `.name.shashin-PID`. (2) needs no contention: several large pictures are enough. In the real app the first `SavedAll` quits, which may end the process while the second job runs. Pictures it has already rewritten stay at 180°, and a half-written temp file can be left behind.
- **Files:** `desktop/aizome/apps/shashin/tests/repro_sap_11.rs`

<a id="sap-13"></a>

### SAP-13 — Confirmed

Catalog entry: [SAP-13](small-apps.md#sap-13), Shashin stage drawing requests (busy, want, reported).

- **Claim:** zoom in while a large photo decodes: `run_jobs` drops the stage's inexact report because the picture isn't decoded. After `install` the zoom, centre and view are unchanged, so the report key is the same and the stage never asks again. The picture stays the fit drawing scaled up.
- **Level:** integration
- **Method:** the real Shashin opens a 7680×4400 camera JPEG (with EXIF preview) in the `Harness`. The test waits for the header while the picture is still decoding, sends `Msg::ZoomIn` (the toolbar's Zoom In; ❖= sends the same), and renders, so the stage reports `Seen { exact: false }`. Precondition: the report was handled while the picture was still decoding. The test then settles, draws 50 more frames, and asserts there is a drawing whose `z` is the zoom shown. Files: `repro_sap_13.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shashin --test repro_sap_13 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** `zoom 0.16666666666666666: centre (3840.0, 2200.0) while decoding, (3840.0, 2200.0) once decoded`, then `no drawing made for zoom 0.16666666666666666 once decoded; drawings at [0.125, 0.125]`. Only the preview and the fit drawing exist.
- **Notes:** the bug depends on the geometry. It happens when the fitted picture sits on whole device pixels (7680×4400 in the default 960×610 stage: 12.5%, 30 px above and below). With a 6000×4000 JPEG the same test passed: the fit origin is 22.5 px, and its rounding put Zoom In's centre at (2996.7, 2000). `install` recentred it to (3000, 2000), which changed the rounded visible rect, so the stage reported again by accident. That run was a diagnostic and was not kept. A zoom about the pointer while decoding (pinch, ❖-scroll) is instead undone by `install`'s recentring, which is a separate hazard and was not tested.
- **Files:** `desktop/aizome/apps/shashin/tests/repro_sap_13.rs`
- **Independent review:** upheld with caveat; re-run: FAIL as reported: tiles only at 0.125, zoom 0.1667; asserts the claim: Yes; reachability: real (Zoom In during decode). It depends on geometry: whole-pixel fit.. Same report key after `install`, so it never re-asks. The report says honestly that a 6000×4000 picture escapes by accident.

<a id="sap-18"></a>

### SAP-18 — Confirmed

Catalog entry: [SAP-18](small-apps.md#sap-18), Shashin Move to Trash with Undo and Redo.

- **Claim:** trash a picture of folder A, open folder B, press ❖Z: the file returns to A, but its path is inserted into B's list and shown there with B's pictures as its neighbours.
- **Level:** integration
- **Method:** the real Shashin in the `Harness` (trash can inside the private test dir) shows `A/IMG_2.jpg`. The test sends `Msg::Trash`, then `Msg::Open([B/yak.png])`, then `Msg::Undo`. It asserts the file is back in A and that every picture in the list being stepped through is in the shown picture's folder. Files: `repro_sap_18.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-shashin --test repro_sap_18` (in `nix develop .#aizome`)
- **Observed:** `showing Some(".../repro-sap-18-a-…/IMG_2.jpg") with another folder's pictures as its neighbours: [".../repro-sap-18-a-…/IMG_2.jpg", ".../repro-sap-18-b-…/yak.png", ".../repro-sap-18-b-…/zebra.png"]`. The same result appeared in 3 of 3 runs.
- **Notes:** the restore itself is correct (the file is back in A). Only the list, and so ←/→ and the filmstrip, mixes the two folders.
- **Files:** `desktop/aizome/apps/shashin/tests/repro_sap_18.rs`

<a id="sap-25"></a>

### SAP-25 — Confirmed

Catalog entry: [SAP-25](small-apps.md#sap-25), Soroban calculator entry (entry, answer, error, 2nd, undo).

- **Claim:** after ❖Z, a key that does nothing (`)` with no parenthesis open, a second `.`, a digit outside the radix) still runs `remember()`, which pushes a no-op undo step and empties the redo stack, so ❖⇧Z stops working.
- **Level:** unit and integration
- **Method:** (1) `Calc` is driven directly for each of the three keys: type, undo, press the no-op key (precondition: the expression is unchanged), redo, and assert that the expression is back. (2) The real Soroban runs in the `Harness`: type `12`, ❖Z, type `)`, ❖⇧Z, and assert the display shows `12`. Files: `repro_sap_25.rs`.
- **Command:** `cargo test --locked --manifest-path desktop/aizome/Cargo.toml -p aizome-soroban --test repro_sap_25` (in `nix develop .#aizome`)
- **Observed:** (1) `` `)` with nothing open: Redo offered = false, expression after ❖⇧Z "1", wanted "12" ``, `` a second `.`: Redo offered = false, … "1.", wanted "1.2" ``, `a digit outside the radix: Redo offered = false, … "1", wanted "12"`. (2) `❖⇧Z after a key that did nothing` (left `"1"`, right `"12"`).
- **Notes:** the Redo menu item is also disabled after the no-op key (`can_redo()` is false).
- **Files:** `desktop/aizome/apps/soroban/tests/repro_sap_25.rs`

<a id="hco-07"></a>

### HCO-07 — Confirmed

Catalog entry: [HCO-07](hotaru-core.md#hco-07), Kitty clipboard session password grants and paste-event one-time passwords.

- **Claim:** one-time paste-event grants share the 32-entry FIFO with remembered grants, and eviction ignores `one_time`, so 32 pastes whose passwords are never spent evict the password the user chose to remember.
- **Level:** integration (public `Terminal` API: OSC 5522 feed, `kitty_clipboard_grant`, `kitty_paste_event`)
- **Method:** `repro_hco_07.rs`. Remember `secret` for reads (checked: granted), enable mode 5522, then 32 paste events, each followed by a listing-only read with its password. Asserts the `secret` read is still granted.
- **Command:** `cargo test --locked --manifest-path apps/hotaru/Cargo.toml -p hotaru-core --test repro_hco_07` (in `nix develop .#aizome`)
- **Observed:** `the remembered password was evicted by 32 unspent one-time paste-event passwords; the program is asked again`
- **Notes:** listings never spend a grant (by design, `kitty_clipboard.rs:1045-1048`), so unspent one-time grants pile up until `entries.remove(0)` (`:648-650`) drops the oldest entry, here the remembered one.
- **Files:** `apps/hotaru/crates/hotaru-core/tests/repro_hco_07.rs`
- **Independent review:** upheld; re-run: Fails: remembered password evicted after 32 paste events; asserts the claim: Yes; reachability: real: OSC 5522 bytes and paste events through the public `Terminal` API. `Grants::grant` evicts `entries[0]` regardless of `one_time`; unspent one-time grants pile up

<a id="hco-18"></a>

### HCO-18 — Confirmed

Catalog entry: [HCO-18](hotaru-core.md#hco-18), Cursor style (DECSCUSR, DEC mode 12, configured default).

- **Claim:** a cursor-default change while on the 1049 alternate screen updates only the alternate screen's shape; after 1049 exit the primary keeps the old default shape although `cursor_is_default` is true.
- **Level:** unit (`Terminal` driven directly)
- **Method:** `repro_hco_18.rs`. Default Block, `CSI ?1049h`, `set_default_cursor(Bar)`, `CSI ?1049l`; asserts the primary's shape is Bar. A second test covers the entry's hazard: `CSI ?12h`, then a default change; asserts blinking stays on, as `Terminal::set_default_cursor`'s doc says.
- **Command:** `cargo test --locked --manifest-path apps/hotaru/Cargo.toml -p hotaru-core --test repro_hco_18` (in `nix develop .#aizome`)
- **Observed:** `primary screen keeps the old default though no DECSCUSR is in force — left: Block, right: Bar`; hazard: `a default change undid the program's DECSET 12`
- **Notes:** the pinned Ghostty cursor fixtures have no case for a default changed on the alternate screen. The expected values come from the "following the default" contract and the doc comment, not from a reference terminal.
- **Files:** `apps/hotaru/crates/hotaru-core/tests/repro_hco_18.rs`

<a id="hco-24"></a>

### HCO-24 — Confirmed

Catalog entry: [HCO-24](hotaru-core.md#hco-24), PTY child lifecycle and background reaper (HUP → grace → KILL → wait).

- **Claim:** `REAPER.get_or_init` caches an `Err` for the life of the process, so one failed reaper-thread creation (EAGAIN) makes every later `Pty::spawn` fail after the condition clears.
- **Level:** integration (real `Pty::spawn` in its own test binary, so the process-wide `OnceLock` starts empty)
- **Method:** `repro_hco_24.rs`. Lower this test process's own `RLIMIT_NPROC` soft limit to 0, call `Pty::spawn` (thread creation fails), restore the limit, check that `std::thread::spawn` works again, then call `Pty::spawn` again; asserts it succeeds.
- **Command:** `cargo test --locked --manifest-path apps/hotaru/Cargo.toml -p hotaru-io --test repro_hco_24` (in `nix develop .#aizome`)
- **Observed:** `after the condition cleared, spawn still fails: Some(Custom { kind: Other, error: "Resource temporarily unavailable (os error 11)" }) (the first failure was … the same)`
- **Notes:** the rlimit change touches only the test process: soft limit lowered and then restored, hard limit untouched. Within the lowered window it creates no other processes.
- **Files:** `apps/hotaru/crates/hotaru-io/tests/repro_hco_24.rs`

<a id="hco-26"></a>

### HCO-26 — Confirmed

Catalog entry: [HCO-26](hotaru-core.md#hco-26), Shell prompt hooks: load guards and last-reported-cwd de-duplication (OSC 7).

- **Claim:** the shell hooks' last-reported-PWD cache is never invalidated when something else reports OSC 7, so after a nested integrated shell reports another directory and exits, the outer prompt stays silent and the terminal keeps the nested directory.
- **Level:** integration (real Bash 5.3 with the real `rc.bash`/`hotaru.bash` in a `hotaru-io` PTY, output parsed by `hotaru-core::Terminal` for `WorkingDirectory` events; disposable `HOME`; `NOSYSBASHRC=1` so the host's `/etc/bashrc` doesn't run, since it execs fish)
- **Method:** `repro_hco_26.rs`. The outer shell starts in `…/src`, runs a nested `bash --rcfile …/rc.bash -i`, which does `cd …/other` and `exit`; at the outer prompt the test sends `exit`. Asserts the last OSC 7 names `…/src`.
- **Command:** `cargo test --locked --manifest-path apps/hotaru/Cargo.toml -p hotaru-io --test repro_hco_26` (in `nix develop .#aizome`)
- **Observed:** `back at the outer prompt in …/src (… prompts shown), the terminal's last OSC 7 still names the nested shell's …/other; reports: [(0, src), (1, src), (2, other)]` — no report at the fourth (outer) prompt
- **Notes:** I tested Bash only. `hotaru.fish` and `hotaru.zsh` de-duplicate the same way (`test "$PWD" = "$_hotaru_last_pwd"`, `$cwd != ${_hotaru_last_pwd-}`), but I didn't run them: there is no zsh in the env, and I didn't drive fish's interactive startup. Hotaru takes the latest report as the pane's directory (`session.rs:423-434`), so new tabs and the title follow `…/other`.
- **Files:** `apps/hotaru/crates/hotaru-io/tests/repro_hco_26.rs`
- **Independent review:** upheld; re-run: Fails: last OSC 7 = `…/other`, reports `[(0,src),(1,src),(2,other)]`; asserts the claim: Yes (bash only); reachability: real: real Bash with the real rc files in a PTY. The de-duplication cache never learns of others' OSC 7

<a id="htt-10"></a>

### HTT-10 — Confirmed

Catalog entry: [HTT-10](hotaru-tests-tools.md#htt-10), (dev tool) Frame benchmark controller run lifecycle.

- **Claim:** on frame-matrix's 600 s timeout the harness gets SIGINT; `KeyboardInterrupt` bypasses `except Exception`, so `finally` writes `result.json` still saying `"status": "running"`.
- **Level:** end-to-end of the tool with stubbed children (real `frame-benchmark.py` `main()`/`run()` in its own session, as `run_trial` starts it)
- **Method:** `repro_htt_10.py`. A patched `build_observer` returns a stub observer that prints `ready`. Stub `dbus-daemon` and `sway` on PATH only bind their sockets; the "application" stub records that it started and waits. Private `--lock` and CPU sets 0–4. Once the app stub has started (so the harness is blocked in `listener.accept()`), the driver sends SIGINT, as `frame-matrix.py:241` does, and reads `result.json`. Asserts the status isn't `running`.
- **Command:** `python3 apps/hotaru/tools/repro_htt_10.py` (in `nix develop`)
- **Observed:** `harness exit code: -2`; `interrupted at: File ".../frame-benchmark.py", line 486, in run | KeyboardInterrupt`; `result.json status: running | error: None`; `stub processes still running after the harness exited: []`; `AssertionError: an interrupted run is recorded as status "running"`
- **Notes:** the `finally` teardown itself worked: all stubs got SIGTERM and exited, and `result.json` was written. The other hazard (a second `TimeoutExpired` inside `finally`) was not tested. No real D-Bus, compositor, terminal or workstation lock was used.
- **Files:** `apps/hotaru/tools/repro_htt_10.py`

<a id="htt-14"></a>

### HTT-14 — Confirmed

Catalog entry: [HTT-14](hotaru-tests-tools.md#htt-14), (dev tool) Frame matrix batch status and trial acceptance.

- **Claim:** `--phase throughput` trials are accepted even if an external compiler ran during the throughput flood, because the measured interval ends at the last stream capture and the flood comes after the streams.
- **Level:** integration (real `frame-matrix.py` `main()`: argument checks, calibration copy, trial gates, interval selection, `compiler_overlap`, status; with `preflight` and `run_trial` stubbed to return fake `result.json` data and activity samples)
- **Method:** `repro_htt_14.py`. In the fake trial, startup runs 10–11 s, streams end at 20 s and the throughput flood runs 21–41 s, ordered as frame-benchmark records them. An external `rustc` appears in the samples only at 25–35 s. Asserts the trial is rejected with "compiler activity overlaps measurement interval". `--control` moves the compiler to 15–17 s.
- **Command:** `python3 apps/hotaru/tools/repro_htt_14.py` (control: append `--control`) (in `nix develop`)
- **Observed:** `batch status 'passed'`; `first trial (hotaru): accepted=True measured=[10 s, 20 s] compiler_overlap_samples=0 reasons=[]`; `flood [21 s, 41 s]; external rustc [25 s, 35 s]` → `AssertionError`. Control: `accepted=False … compiler_overlap_samples=3 reasons=['compiler activity overlaps measurement interval']`, `PASS`.
- **Notes:** the gate works within its window (as the control shows); only the window's end (`frame-matrix.py:380-382`) leaves out `result["throughput"]`. No real `/proc` sampling went into the decision, so other agents' builds on the host can't affect it.
- **Files:** `apps/hotaru/tools/repro_htt_14.py`

<a id="gen-03"></a>

### GEN-03 — Confirmed

Catalog entry: [GEN-03](genkan.md#gen-03), Selected-disk identity: discovered → reviewed → revalidated before and after the first write.

- **Claim:** revalidating the selected disk does a full rediscovery that fails on *any* listed disk, so when another disk is unplugged or re-enumerated between `lsblk` and the per-disk `stat`, the revalidation after `wipefs`/`parted` (install.rs:573) or after formatting (install.rs:628) aborts a half-written installation over an unrelated disk.
- **Level:** unit (`disk::revalidate` driven directly, with a fake `lsblk` and a fake sysfs entry)
- **Method:** the library is built with `CALAMARES_TOOL_PATH` naming a directory of fakes, so `lsblk` is a script printing a fixed inventory. The test re-runs itself under `unshare -Urm` with private tmpfs mounts on `/run` and `/sys/dev/block`. The "target" is `/dev/loop0` (only `stat`ed; this user can't open it) with a fake non-virtual `/sys/dev/block/7:0`. It then calls `disk::revalidate(target)` for three inventories: the target alone (control), the target plus an unplugged `/dev/sdz` (case A), and the target plus `/dev/loop1` that `lsblk` reports as 8:16 (stale numbers, case B). It asserts that A and B revalidate the unchanged target. Files: `repro_gen_03.rs` (→ `installer/genkan/rust/tests/`).
- **Command:** `env CALAMARES_TOOL_PATH=$SCRATCH/tools cargo test --locked --manifest-path installer/genkan/rust/Cargo.toml --test repro_gen_03 -- --nocapture` (in `nix develop`)
- **Observed:** `control (target only): Ok(())` · `A (unrelated /dev/sdz unplugged after lsblk): Err(No such file or directory (os error 2))` · `B (unrelated /dev/loop1 re-enumerated, lsblk said 8:16): Err(Disk changed during discovery)` → `revalidating the unchanged selected disk failed because of another disk`. Neither error names the disk at fault.
- **Notes:** this reproduces the mechanism, not a real install. The "after the disk was wiped" consequence follows from code order: in `prepare_storage`, `wipefs` (install.rs:555), `parted` (:559) and `blockdev --rereadpt` (:562) run before the `disk::revalidate` at :573, which this test calls, and nothing is rolled back. A disposable-VM run would need a target virtio disk plus a hot-pluggable USB disk, removed through QMP `device_del` between `parted` and the :573 revalidation (for example, with the helper paused at `blockdev --rereadpt`). Expect `Failed` with ENOENT and a partitioned but unformatted target.
- **Files:** `installer/genkan/rust/tests/repro_gen_03.rs`
- **Independent review:** upheld with caveat; re-run: Fails: A `ENOENT`, B `Disk changed during discovery`; asserts the claim: Yes; reachability: staged: a fake `lsblk` inventory names a disk that is gone or renumbered; the race window is lsblk → per-disk stat. `discover_with_mappings` uses `?` on every disk (disk.rs:127, :136). The revalidation at install.rs:573 follows wipefs/parted. Narrow window

<a id="gen-07"></a>

### GEN-07 — Confirmed

Catalog entry: [GEN-07](genkan.md#gen-07), Install transaction: non-destructive preparation stages, then destructive steps 1–6.

- **Claim:** on the deferred route (downloads larger than live memory), the "selected system needs at least N GiB on the root filesystem" guard (install.rs:479–488) never runs. Preparation therefore accepts a selection that can't fit on the target, and the build on the target then fails after the disk was erased.
- **Level:** integration (the real `install::prepare` with fake tools and a fake root)
- **Method:** the library is built with `CALAMARES_TOOL_PATH` naming a directory of fakes. `findmnt` reports tmpfs `/` and iso9660 `/iso`, `lsblk` lists one 40 GiB disk, and the formatter and modprobe checks succeed. `nixos-generate-config` prints an empty module. `nix build --dry-run` prints a valid result and `these 1500 paths will be fetched (60.00 GiB download, 300.00 GiB unpacked)`. No wipefs, parted, mkfs or copy stand-in exists. `prepare` needs euid 0, root-owned settings paths and `/run`, so the test re-runs itself under `unshare -Urm` and chroots into a tmpfs root. That root has `/proc`, `/sys`, `/dev`, `/nix`, `/bin`, `/usr` and the tool directory bind-mounted, a private `/run`, a two-line `/etc`, and a fake `/sys/dev/block/7:0`. The test builds a real plan (`RawRequest::parse`), calls `install::prepare`, and asserts a refusal mentioning the root filesystem. Files: `repro_gen_07.rs` (→ `installer/genkan/rust/tests/`).
- **Command:** `env CALAMARES_TOOL_PATH=$SCRATCH/tools cargo test --locked --manifest-path installer/genkan/rust/Cargo.toml --test repro_gen_07 -- --nocapture` (in `nix develop`)
- **Observed:** `live RAM 60.4 GiB total, 29.7 GiB available; target disk 40 GiB` · tools run: findmnt ×2, lsblk, mkfs.ext4 -V, modprobe ext4, mkfs.fat --help, modprobe vfat, nixos-generate-config, `nix build --dry-run …` · `prepare: Ok(Prepared { deferred: true, closure_bytes: 0, downloaded_bytes: 0, root_bytes: 39.0 GiB })` → `preparation accepted a selection with 300 GiB of unpacked downloads for a 39.0 GiB root filesystem (deferred route); expected a refusal before erasure, got None`
- **Notes:** the later "No space left" during step 4 was not run. A disposable VM would need about 4 GiB of RAM, a 24–30 GiB target and a selection whose unpacked downloads exceed the root, to show `Failed` after `wipefs`/`parted`. The test's `contains("root filesystem")` follows the existing guard's wording; a fix with other wording would need the assertion adjusted. The review then shows a 0-byte closure for the deferred route, consistent with GEN-07's other hazard.
- **Files:** `installer/genkan/rust/tests/repro_gen_07.rs`

<a id="gen-28"></a>

### GEN-28 — Confirmed

Catalog entry: [GEN-28](genkan.md#gen-28), Disk scan, selection and installed-memory read.

- **Claim:** a failed `/proc/meminfo` read sets swap off but leaves `memory: None`, which also means "still reading". If swap is turned back on, the Disk page says "Reading installed memory…" and Next says "Still reading installed memory; try again in a moment." indefinitely, with no retry.
- **Level:** integration (Genkan headless through `aizome_kit::testing::Harness`, on the preview's sample data)
- **Method:** opens the preview `disk` page and resets `memory` to `None` (its value from launch until the read answers; the preview pre-fills it). It sends `Msg::Memory(Err(…))`, then `Msg::Swap(true)`, advances the clock 60 s and sends `Msg::Next`. It asserts that neither the page nor Next claims memory is still being read. Files: `repro_gen_28.rs` (→ `installer/genkan/ui/tests/`).
- **Command:** `NM=/nix/store/yl9lagnfzmlqmn1v3vl839q84c320nak-networkmanager-1.58.1/lib; env CALAMARES_TOOL_PATH=$SCRATCH/tools CALAMARES_PKEXEC=/nonexistent/pkexec LIBRARY_PATH=$NM LD_LIBRARY_PATH=$NM TZDIR=/etc/zoneinfo cargo test --locked --manifest-path installer/genkan/ui/Cargo.toml --test repro_gen_28 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** `memory=None swap=true page_says_reading=true step_after_next=Disk next_says_still_reading=true` → `after the read failed, the Disk page still says it is reading installed memory (page: true, Next: true, step Disk, memory None)`
- **Notes:** the trigger (unreadable /proc/meminfo) is rare. The `aizome` shell lacks libnm, which the backend's `network` feature links, so the UI commands add `LIBRARY_PATH`/`LD_LIBRARY_PATH` from the `default` shell's NetworkManager. `CALAMARES_TOOL_PATH`/`CALAMARES_PKEXEC` point any accidental tool or pkexec spawn at fakes or nothing.
- **Files:** `installer/genkan/ui/tests/repro_gen_28.rs`

<a id="gen-33"></a>

### GEN-33 — Partly confirmed

Catalog entry: [GEN-33](genkan.md#gen-33), Keyboard step: catching the key beside the space bar through the compositor.

- **Claim:** clicking Review while the key wait is open leaves it open. A key typed into a prompt raised during the review (Wi-Fi wallet or Secret Agent), or into the Polkit prompt within the 500 ms poll after `Reviewed`, is swallowed by the compositor's catch: the password loses its first character and `super_key` silently becomes `AsIs`.
- **Level:** unit (GUI side, in-crate) and integration (IPC side, Genkan headless with a stand-in compositor)
- **Method:** 
  - (1) `src/repro_gen_33.rs`, in-crate: on the preview keyboard page it sets an open wait (`key_waits = 7`, `KeyCheck::Waiting`), sends `SwapAltSuper(true)`, then in one turn `Next` (which starts the review; the worker later fails at `Settings::load`) and `KeyCaught(7, Some(KEY_A))`. It asserts that the wait was given up and the chosen arrangement kept.
  - (2) `tests/repro_gen_33.rs`: `AIZOME_SOCKET` points at a stand-in compositor in the private runtime dir. The test sends `ListenForKey`; the stand-in answers Subscribe/CatchKey; then the test sends `StopListening` (the same `stop_listening` that `go(Review)` runs after `Reviewed`). The stand-in waits 100 ms for a release (`catch-key` without a tag, or a close), and if none comes it routes a key to the catch. It asserts the catch was released first.
  - Files: `src/repro_gen_33.rs` (+ one-line mod in `app.rs`, see INSERT.txt), `tests/repro_gen_33.rs`.
- **Command:** `NM=/nix/store/yl9lagnfzmlqmn1v3vl839q84c320nak-networkmanager-1.58.1/lib; env CALAMARES_TOOL_PATH=$SCRATCH/tools CALAMARES_PKEXEC=/nonexistent/pkexec LIBRARY_PATH=$NM LD_LIBRARY_PATH=$NM TZDIR=/etc/zoneinfo cargo test --locked --manifest-path installer/genkan/ui/Cargo.toml --lib repro_gen_33 -- --nocapture` for (1), and the same with `--test repro_gen_33` in place of `--lib repro_gen_33` for (2) (in `nix develop .#aizome`)
- **Observed:** 
  - (1) `reviewing=true step=Keyboard key_waits=7 key_check=Caught { … "a key that can't be ❖" … } super_key=AsIs` → `the review left the compositor's key wait open (key_waits still 7) and a key caught during it replaced the chosen arrangement`.
  - (2) `no release within 100 ms of giving up; the key typed then went to Genkan's catch; Genkan then closed the connection 104 ms after giving up` · `after: key_check=Asking super_key=SwapAlt` → `the compositor still held Genkan's key catch 100 ms after Genkan gave the wait up`.
- **Notes:** 
  - What reproduces: the review doesn't end the wait; a key caught while reviewing overrides `super_key`; and after the wait is given up, the IPC thread keeps the catch until its next 500 ms poll, so a key typed in that window is consumed.
  - What doesn't: for the Polkit prompt after `Reviewed`, the GUI drops the consumed key as stale (`key_caught` checks the wait number), so `super_key` does not change there. The override needs a key caught *before* `Reviewed` (the wallet or Secret Agent case).
  - The compositor's "hold back the next key from every app" was not exercised; a stand-in played the compositor. Whether the review actually raises a wallet prompt (the snapshot's `GetSecrets` "can consult the live user's Secret Agent", wifi/nm.rs:107) needs a live session.
- **Files:** `installer/genkan/ui/src/repro_gen_33.rs`, `installer/genkan/ui/tests/repro_gen_33.rs`
- **Module line added to:** `installer/genkan/ui/src/app.rs`

<a id="gen-34"></a>

### GEN-34 — Confirmed

Catalog entry: [GEN-34](genkan.md#gen-34), Review, background preparation and confirmation (GUI side of the helper session).

- **Claim:** if the person confirms before preparation finishes and preparation then fails before any disk write, `show_failure` takes its `installing` branch. It marks step 1 "Partition the disk" failed, heads the page "Installation Failed" and says "The disk may have been modified", and the person cannot go back (the passwords have been wiped).
- **Level:** integration (Genkan headless on the preview's review page; helper events go through the real `Msg::Session` handler)
- **Method:** opens the preview `review` page (preparation still working, generation 0) and types the phrase, consents and confirms. It then delivers `Session(0, Event::Failed { "The installed system could not be built; the target disk has not been erased: …" })` and `Session(0, Finished(Err(… exit status: 1 …)))`, in the order the helper sends them. It asserts that no step is marked failed and no "disk may have been modified" text appears. Files: `repro_gen_34.rs` (→ `installer/genkan/ui/tests/`).
- **Command:** `NM=/nix/store/yl9lagnfzmlqmn1v3vl839q84c320nak-networkmanager-1.58.1/lib; env CALAMARES_TOOL_PATH=$SCRATCH/tools CALAMARES_PKEXEC=/nonexistent/pkexec LIBRARY_PATH=$NM LD_LIBRARY_PATH=$NM TZDIR=/etc/zoneinfo cargo test --locked --manifest-path installer/genkan/ui/Cargo.toml --test repro_gen_34 -- --nocapture` (in `nix develop .#aizome`)
- **Observed:** `after confirming: "Finishing preparation; the installation starts as soon as it is ready."` · `step=Install install_step=0 failed_steps=[1] heading_failed=true disk_may_be_modified=true back_offered=false next_label="Close Installer" reachable(Review)=false password_cleared=true`
- **Notes:** preview mode stands in for the session only at confirmation (`confirm_install` skips `session.confirm`). Every state change after that is the real handler. The helper's message itself says the disk was not erased, but the page contradicts it.
- **Files:** `installer/genkan/ui/tests/repro_gen_34.rs`
- **Independent review:** upheld; re-run: Fails: `failed_steps=[1] heading_failed=true disk_may_be_modified=true back_offered=false`; asserts the claim: Yes; reachability: staged: helper events injected through the real `Msg::Session`, in the shape and order the helper sends when the build fails (install.rs:466). `show_failure`'s `installing` branch marks step `max(1)-1 = 0` failed whatever the phase

<a id="ttl-03"></a>

### TTL-03 — Confirmed

Catalog entry: [TTL-03](tatami-tool.md#ttl-03), Keep Awake: intent marker, inhibitor holder unit and verification.

- **Claim:** One failed or slow logind `ListInhibitors` reply makes `awake hold` return an error and exit. In the real unit that ends `systemd-inhibit` and releases the sleep/idle/lid block until it restarts and is re-verified.
- **Level:** integration (the real binary in a private PID namespace; a fake busctl stands in for logind)
- **Method:** `tatami awake hold` runs as PID 2 under the namespace's init, which stands in for systemd-inhibit. The fake busctl returns a valid block row owned by PID 1, except on call 4 (the third once-a-second poll), which exits 1 the way `busctl --timeout=3` does on a timeout. The test asserts that the holder is still running after six calls. The control case, where no call fails, passes. Files: `repro_ttl_03.py`
- **Command:** `bash -c 'cargo build --locked --manifest-path desktop/tatami/tool/Cargo.toml && PATH=$B:$PATH python3 tests/repro_ttl_03.py $CARGO_TARGET_DIR/debug/tatami -v'` (in `nix develop`)
- **Observed:** `AssertionError: 1 is not None : holder exited (status 1) after 4 ListInhibitors calls, one of which failed: 'tatami: busctl exited with exit status: 1'`
- **Notes:** The holder's exit is reproduced. What follows comes from reading the unit, not from a run: `systemd-inhibit` exits with its child and its block fd closes, then `Restart=always`/`RestartSec=1` plus the `ExecStartPost` verify retake the lock. The length of the unprotected window would need a real user manager and logind, which means a VM.
- **Files:** `tests/repro_ttl_03.py`
- **Independent review:** upheld with caveat; re-run: FAIL as reported: holder exit 1 after 4 calls. Control passes.; asserts the claim: Yes (holder exit; the release that follows is by reading); reachability: staged (the fake busctl fails one call, as a `--timeout=3` expiry does). `protected(...)?` (awake.rs:138) treats a query error as a lost inhibitor. The doc comment (:127-128) says losing logind should force reacquisition, so this is partly deliberate. The cost is a window of about 1 s plus verification.

<a id="ttl-04"></a>

### TTL-04 — Confirmed

Catalog entry: [TTL-04](tatami-tool.md#ttl-04), Wallpaper choice and per-output overrides.

- **Claim:** If the global wallpaper is on a drive that isn't mounted yet when `background ensure` runs (tatami-swaybg's `ExecStartPre`), the choice is replaced by the shipped default for good. The same image chosen per output survives.
- **Level:** integration (real CLI, private XDG state)
- **Method:** `background set IMG` is pointed at an image under a "drive" directory. The directory is renamed away (not mounted), `background ensure` runs, then the directory is renamed back (mounted). The test asserts the symlink still points at IMG. The control case does the same with `set IMG --output DP-1`, then checks `background status`, and passes. Files: `repro_ttl_04.py`
- **Command:** `bash -c 'cargo build --locked --manifest-path desktop/tatami/tool/Cargo.toml && python3 tests/repro_ttl_04.py $CARGO_TARGET_DIR/debug/tatami -v'` (in `nix develop`)
- **Observed:** `AssertionError: '/etc/xdg/tatami/background' != '/tmp/…/media/alice/PHOTOS/beach.png'` (the per-output control passes)
- **Notes:** The relinking is deliberate, per `ensure`'s comment ("one whose image is gone after an update"). The confirmed fault is that it also fires on media that is only temporarily absent. It also fires when the drive is simply unplugged at login. Whether udiskie really loses the race at login is not shown here; that needs a session in a VM.
- **Files:** `tests/repro_ttl_04.py`

<a id="ttl-08"></a>

### TTL-08 — Confirmed

Catalog entry: [TTL-08](tatami-tool.md#ttl-08), Screen recording start, fallback and stop.

- **Claim:** If the user stops a recording while the starting invocation is still in its `started(15)` probe, the starter takes the missing recorder for "gsr cannot run" and starts the wf-recorder fallback on the same file.
- **Level:** integration (the real binary twice, in one PID namespace with fake recorders)
- **Method:** `tatami screenrecord` starts a fake `gpu-screen-recorder` (comm `gpu-screen-reco`) that runs until SIGINT, with fake slurp, hyprctl and notify-send. After 0.5 s, `tatami screenrecord --stop` runs. The test asserts that only gpu-screen-recorder was ever started and that nothing is still recording. Files: `repro_ttl_08.py`
- **Command:** `bash -c 'cargo build --locked --manifest-path desktop/tatami/tool/Cargo.toml && PATH=$B:$PATH python3 tests/repro_ttl_08.py $CARGO_TARGET_DIR/debug/tatami -v'` (in `nix develop`)
- **Observed:** `recorders started: ['gpu-screen-recorder', 'wf-recorder']`. Timeline relative to the stop returning: gsr SIGINTed at −5.02 s, then `wf-recorder -g 10,10 640x480 --no-damage -f /home/alice/Videos/screenrecording-…mp4` (the same file) started at −4.94 s. The stop invocation then waited its full 5 s, sent SIGTERM and notified `Screen recording error … The video may be incomplete.`
- **Notes:** In this interleaving the stop's force-terminate ends the fallback. The just-saved gsr file is reopened by wf-recorder and ends up holding a terminated wf-recorder clip, and the user sees an error. If the stop's check happens to fall between gsr's exit and wf-recorder's spawn, it would report "saved" and the fallback would keep recording. That variant was not forced here.
- **Files:** `tests/repro_ttl_08.py`

<a id="ttl-10"></a>

### TTL-10 — Confirmed

Catalog entry: [TTL-10](tatami-tool.md#ttl-10), Display reconciliation commit (plan execution).

- **Claim:** Close the lid while docked. If `wait_disabled` times out but the modeset completes later, `reconcile` returns an error before saving. The next tick saves a record without `lid_off`, so opening the lid never turns the panel back on.
- **Level:** integration (real binary against `tests/test_display_recovery.py`'s fake Hyprland and logind in bwrap)
- **Method:** This subclasses the shared harness. The fake applies `disabled = true` 2.5 s after accepting it, mimicking a slow dock modeset. The sequence is: docked recover → lid closed → recover (fails "did not turn off") → panel goes off → recover (lid still closed) → lid open → recover. The test asserts eDP-1 is back on. The harness's own `test_closing_the_lid_while_docked…`, with a prompt modeset, runs as the control and passes. Files: `repro_ttl_10.py` (imports `tests/test_display_recovery.py`)
- **Command:** `bash -c 'cargo build --locked --manifest-path desktop/tatami/tool/Cargo.toml && PATH=$B:$PATH python3 tests/repro_ttl_10.py $CARGO_TARGET_DIR/debug/tatami'` (in `nix develop`)
- **Observed:** `panel stayed off after the lid opened; session after close: {'displays': ['desc:DP-1', 'desc:eDP-1'], 'lid_off': [], 'lid_closed': True, 'off': {}} … monitors-off: []`
- **Notes:** An unmirror-wait failure takes the same `Err` path and was not run separately. Note also that `window::switch_off` never records the panel, so monitors-off misses it as well.
- **Files:** `tests/repro_ttl_10.py`
- **Independent review:** upheld; re-run: FAIL as reported: `lid_off: []`, panel still off. Control passes.; asserts the claim: Yes; reachability: staged (the fake applies `disabled` after 2.5 s; a slow dock modeset). The Err before `save` loses the record that the lid turned the panel off.

<a id="ttl-17"></a>

### TTL-17 — Confirmed

Catalog entry: [TTL-17](tatami-tool.md#ttl-17), Live reload pipeline (`tatami reload`, `update-session`).

- **Claim:** `tatami reload` try-restarts every running Tatami unit except Keep Awake, including `tatami-hyprsunset` and `tatami-mako`. This silently turns off Night Light and Do Not Disturb.
- **Level:** integration (real binary in bwrap with fake systemctl, hyprctl, makoctl and busctl, plus a fake `/etc/xdg/tatami/units.json` listing the module's 14 units)
- **Method:** Night Light and Do Not Disturb are turned on with Tatami's own commands (`nightlight on`, `toggle notifications`), then `tatami reload` runs. The test asserts that `nightlight status` and `indicator notifications` read the same as before. The fake systemctl restarts a unit's fake daemon from its defaults: hyprsunset at its identity profile (6000 K, `config/hypr/hyprsunset.conf`) and mako in mode `default`. Files: `repro_ttl_17.py`
- **Command:** `bash -c 'cargo build --locked --manifest-path desktop/tatami/tool/Cargo.toml && PATH=$B:$PATH python3 tests/repro_ttl_17.py $CARGO_TARGET_DIR/debug/tatami -v'` (in `nix develop`)
- **Observed:** `{'nightlight': 'off 4000', 'notifications': 'inactive'} != {'nightlight': '4000', 'notifications': 'active'}`. The reload ran `systemctl --user try-restart tatami-hyprsunset.service tatami-mako.service tatami-waybar.service`.
- **Notes:** The restart list comes from the real code. "A restart loses the state" is the fakes' model of hyprsunset and mako: neither keeps that state across a restart, and hyprsunset's config is identity-only. The Yukimi `update-session` path was not traced.
- **Files:** `tests/repro_ttl_17.py`

<a id="ttl-25"></a>

### TTL-25 — Confirmed

Catalog entry: [TTL-25](tatami-tool.md#ttl-25), Weather report cache and fetch coordination.

- **Claim:** If the user chooses a new place while a background fetch for the old one is in flight, `refresh` returns at the lock. The running fetch then writes the old place's report with a fresh `fetched`, so the bar shows the old place for up to 15 minutes.
- **Level:** integration (real binary in bwrap; fake curl answers Open-Meteo and the geocoder with a 1.5 s delay, fake Walker types "Newtown")
- **Method:** The saved place is Oldtown and the cached report is 20 minutes old. `tatami weather fetch` starts, and once it is waiting on Open-Meteo, `tatami weather place` runs (the real Setup › Weather flow). The test asserts the final report and bar tooltip are Newtown's. The control case (no fetch in flight) passes. Files: `repro_ttl_25.py`
- **Command:** `bash -c 'cargo build --locked --manifest-path desktop/tatami/tool/Cargo.toml && PATH=$B:$PATH python3 tests/repro_ttl_25.py $CARGO_TARGET_DIR/debug/tatami -v'` (in `nix develop`)
- **Observed:** `'Oldtown' != 'Newtown'`. The facts: `place_file` is Newtown, `report_place: 'Oldtown'`, `report_age: 0` (fresh, so not due), and only one Open-Meteo call (latitude 50.1) was made.
- **Notes:** The real-world window is one Open-Meteo request (capped at 5 s), or longer during the retry pauses (3 s, 10 s).
- **Files:** `tests/repro_ttl_25.py`

<a id="tap-03"></a>

### TAP-03 — Confirmed

Catalog entry: [TAP-03](tatami-apps.md#tap-03), Activity sampler's baselines, caches and energy integrals between readings.

- **Claim:** Read every second through the power meter service (which repeats its last reading within 1 s), the processor shows 0 W then about twice its power, the first 0 W pins the idle floor at 0 for the session, and a failed read doubles the next reading.
- **Level:** unit (the sampler's `power()` driven directly) and integration (the sampler's real `sample()` over this machine's `/proc`, on `spawn()`'s schedule)
- **Method:** `repro_tap_03.rs`, a child of `sampler.rs`, builds a `Sampler` with no GPUs and its `rapl` pointed at a fake `intel-rapl:0` in TMPDIR, so `read_direct` succeeds and nothing reaches the host. A fake service answers with `power_meter.rs:100-107`'s rule, transcribed (fresh at most once per second, else the cached value), for a steady 20 W package. Four tests, each asserting every reading is 20 W ± 20 % and the floor stays above 10 W: (1) readings on a 1 s grid, counter asked 250–320 ms in; (2) the default 2 s interval, with `spawn`'s first frame 350 ms after the baseline; (3) one failed read (file removed; the shim refuses the fallback socket); (4) `spawn`'s real loop at 1 s for 14 readings with the real scan, `energy_uj` a FIFO answered when it is read. Files: `repro_tap_03.rs`, `repro-shim.c`, `gtkrun.sh`, `bus.conf`.
- **Command:** `bash -c '$TMPDIR/gtkrun.sh $CARGO_TARGET_DIR/debug/deps/tatami_activity-b2ffa685bc466d82 repro_tap_03 --test-threads=1'` (setup — private Xvfb :217, shim, private bus — in the header of `reproductions/support/tatami-gtk/gtkrun.sh`) (in `nix develop`)
- **Observed:** (1) `readings [none, 0.0, 40.4, 0.0, 39.6, 0.0, 40.2, 0.0] W, idle floor now 0 W`; (2) `baseline and frames [none, 0.0, 24.5, 20.0, 20.0] W, idle floor now 0 W`; (3) `its tick-2 read failed: readings [none, 20.0, none, 40.0] W`; (4) `13 of 14 readings wrong for a steady 20 W: [0.0, 31.5, 0.0, 37.1, 28.6, 0.0, 36.9, 24.9, 0.0, 56.8, 9.1, 22.4, 0.0, 37.4] W; idle floor 0 W; ms between questions [450(cached), 1126, 908(cached), 947, 1430, 612(cached), 1231, 1247, 521(cached), 2317, …]; scan ms [99, 226, 133, 80, 510, 122, 354, 601, 122, 1440, …]`
- **Notes:** The real `serve()` can't run here: it needs socket activation and reads root-only `/sys/class/powercap`, so its cache rule is transcribed. This is worse than the entry says. At the default 2 s interval, `spawn`'s first frame asks 350 ms plus a scan after the baseline, so that answer is always cached. The floor is pinned at 0 W on every window opening that uses the service, which is every non-root session; at 2 s the next frame then reads about 22 % high. At 1 s, most readings are wrong, not just occasional ones: the scan on this host varies from 80 to 1440 ms. In an early run of (4), before I fixed the FIFO handling, the fake gave a garbled answer; the sampler then fell back to `/run/tatami/power.sock`, and the shim refused it. That fallback is the reason the shim is needed. The effect on later per-process shares follows from `split()` with a 0 W floor; I read it in the code but didn't assert it.
- **Files:** `desktop/tatami/activity/src/repro_tap_03.rs`
- **Harness:** [`support/tatami-gtk/gtkrun.sh`](reproductions/support/tatami-gtk/gtkrun.sh) (private Xvfb, private session bus and a preload shim refusing host services). Run the test through it as Cargo's runner: `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER=$PWD/docs/implicit-state-machines/reproductions/support/tatami-gtk/gtkrun.sh cargo test --locked --manifest-path desktop/tatami/<app>/Cargo.toml <filter> -- --test-threads=1`.
- **Module line added to:** `desktop/tatami/activity/src/sampler.rs`
- **Independent review:** upheld; re-run: All 4 fail: e.g. `[none, 0.0, 40.4, 0.0, …]`, floor 0 W; real loop 13/14 wrong; asserts the claim: Yes; reachability: real: the real sampler; the service's 1 s cache rule transcribed faithfully (`EVERY` = 1 s, power_meter.rs:100). At the default 2 s interval too, the first frame (350 ms) is always cached, which pins the floor at 0

<a id="tap-04"></a>

### TAP-04 — Confirmed

Catalog entry: [TAP-04](tatami-apps.md#tap-04), Activity process table: columns per view and remembered sorts.

- **Claim:** The sorter handler's `last` isn't updated while `set_lens` swaps columns, so the first click on the current sort column is undone at start-up and after a view switch, and a column `last` still names can be made primary in ascending order.
- **Level:** unit (the real `Table`, under GTK on the private Xvfb)
- **Method:** `repro_tap_04.rs`, a child of `table.rs`, makes tables as the window does (`Table::new`, `set_root(Apps)`, `set_lens(Cpu)`). `click()` emulates a header click the way GtkColumnViewTitle does (`gtk_column_view_sorter_toggle_column`): it flips the primary column, or makes another column primary in ascending order, applied with `sort_by_column`. It then drains idles. The test asserts (1) at start-up, one click on % CPU leaves it ascending; (2) after Threads, then Memory view, one click on Memory leaves it ascending; (3) after Threads and % CPU, then Energy view (sorted by Power), clicking % CPU makes it primary in descending order. File: `repro_tap_04.rs`.
- **Command:** `bash -c '$TMPDIR/gtkrun.sh $CARGO_TARGET_DIR/debug/deps/tatami_activity-b2ffa685bc466d82 table::repro_tap_04 --test-threads=1'` (setup — private Xvfb :217, shim, private bus — in the header of `reproductions/support/tatami-gtk/gtkrun.sh`) (in `nix develop`)
- **Observed:** `"start-up, one click on % CPU (Descending→Ascending): now (Some(Cpu), Descending)"`, `"Memory view after a switch, one click on Memory (Descending→Ascending): now (Some(Memory), Descending)"`, `"Energy view sorted by Power, one click on % CPU (newly primary): now (Some(Cpu), Ascending)"`
- **Notes:** The click is emulated: no input tool is available on Xvfb. The emulation gives the sorter the same primary column and order as GTK's toggle, and the handler reads only those. All three effects the entry describes reproduce.
- **Files:** `desktop/tatami/activity/src/repro_tap_04.rs`
- **Harness:** [`support/tatami-gtk/gtkrun.sh`](reproductions/support/tatami-gtk/gtkrun.sh) (private Xvfb, private session bus and a preload shim refusing host services). Run the test through it as Cargo's runner: `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER=$PWD/docs/implicit-state-machines/reproductions/support/tatami-gtk/gtkrun.sh cargo test --locked --manifest-path desktop/tatami/<app>/Cargo.toml <filter> -- --test-threads=1`.
- **Module line added to:** `desktop/tatami/activity/src/table.rs`

<a id="tap-06"></a>

### TAP-06 — Confirmed

Catalog entry: [TAP-06](tatami-apps.md#tap-06), Window singleton, deferred first presentation and launch requests held until the first reading (Activity, Network, Sound).

- **Claim:** `tatami-activity --pid N` while the window exists but before its first frame toasts "Process N has ended" for a live process and selects nothing.
- **Level:** integration (the real `open()` second-launch branch, `select_pid` and `apply`; the window is `build`'s without its sampler thread)
- **Method:** `repro_tap_06.rs`, a child of `window.rs`, makes `build`'s `Ui` without `sampler::spawn`, which would read the host's power meter service and NVML. Frames come from the test into a copy of `build`'s frame loop (`window.rs:316-332`). It performs `open`'s first launch (`window.rs:103-115`), then 100 ms later calls the real `open(app, Request { pid: Some(P) })`, where P is a live `sleep` the test started. It then sends a first frame with P among busier applications. The test asserts no "has ended" toast (found by walking the ToastOverlay's labels) and that P's application is selected. Files: `repro_tap_06.rs`, `gtkrun.sh`, `bus.conf`, `repro-shim.c`.
- **Command:** `bash -c '$TMPDIR/gtkrun.sh $CARGO_TARGET_DIR/debug/deps/tatami_activity-b2ffa685bc466d82 window::repro_tap_06 --test-threads=1'` (setup — private Xvfb :217, shim, private bus — in the header of `reproductions/support/tatami-gtk/gtkrun.sh`) (in `nix develop`)
- **Observed:** `after `--pid 930994` for a live process: toast "Process 930994 has ended"; selected after the first reading: None`
- **Notes:** The request isn't kept for later: `pending` holds only the first launch's request. Network's `join_by_name` guards the same case with `loaded`.
- **Files:** `desktop/tatami/activity/src/repro_tap_06.rs`
- **Harness:** [`support/tatami-gtk/gtkrun.sh`](reproductions/support/tatami-gtk/gtkrun.sh) (private Xvfb, private session bus and a preload shim refusing host services). Run the test through it as Cargo's runner: `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER=$PWD/docs/implicit-state-machines/reproductions/support/tatami-gtk/gtkrun.sh cargo test --locked --manifest-path desktop/tatami/<app>/Cargo.toml <filter> -- --test-threads=1`.
- **Module line added to:** `desktop/tatami/activity/src/window.rs`

<a id="tap-08"></a>

### TAP-08 — Confirmed

Catalog entry: [TAP-08](tatami-apps.md#tap-08), Activity's Quit, Force Quit and Pause on a target taken from the latest frame.

- **Claim:** If the Force Quit dialog is answered after its process exited and the PID was reused, the unrelated process gets SIGKILL, because the response uses bare PIDs from the frame shown when the dialog opened.
- **Level:** integration (the real `target`, `confirm_quit`, its response handler and `signal`; the PID reuse is simulated by frames)
- **Method:** `repro_tap_08.rs` uses TAP-06's window. A PID can't be reused on demand, so the frames state the reuse. The frame shown when the dialog opens has "Doomed" (start 111) at PID P, which is really the test's own `sleep 600`. The test selects it, calls `confirm_quit(ui, true)` and takes the dialog from `visible_dialog()`. It then applies the next frame (P is now `sleep` with its real start) and emits `response` with "force". It asserts the `sleep` is still alive. Only the test's own child can be signalled. Files: `repro_tap_08.rs` and `repro_tap_06.rs`.
- **Command:** `bash -c '$TMPDIR/gtkrun.sh $CARGO_TARGET_DIR/debug/deps/tatami_activity-b2ffa685bc466d82 window::repro_tap_08 --test-threads=1'` (setup — private Xvfb :217, shim, private bus — in the header of `reproductions/support/tatami-gtk/gtkrun.sh`) (in `nix develop`)
- **Observed:** `Force Quit for “Doomed” (PID 931054, start 111) ended PID 931054, now `sleep` (start 9251159): Some(ExitStatus(unix_wait_status(9)))`
- **Notes:** The mechanism is confirmed: nothing compares the PIDs with their start or key when the dialog is answered, even though `state.pids` holds the keys. In practice it is unlikely, as the entry's low confidence says: here `pid_max` is 4194304, so PIDs would have to wrap while the dialog is open. The same capture applies to Quit's SIGTERM.
- **Files:** `desktop/tatami/activity/src/repro_tap_08.rs`
- **Harness:** [`support/tatami-gtk/gtkrun.sh`](reproductions/support/tatami-gtk/gtkrun.sh) (private Xvfb, private session bus and a preload shim refusing host services). Run the test through it as Cargo's runner: `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER=$PWD/docs/implicit-state-machines/reproductions/support/tatami-gtk/gtkrun.sh cargo test --locked --manifest-path desktop/tatami/<app>/Cargo.toml <filter> -- --test-threads=1`.
- **Module line added to:** `desktop/tatami/activity/src/window.rs`
- **Independent review:** upheld with caveat; re-run: Fails: Force Quit SIGKILLed the reused PID (`unix_wait_status(9)`); asserts the claim: Yes; reachability: staged: PID reuse stated by frames. Real TOCTOU (no start-time check). With `pid_max` 4194304, wrap during an open dialog is very unlikely

<a id="tap-15"></a>

### TAP-15 — Confirmed

Catalog entry: [TAP-15](tatami-apps.md#tap-15), Bluetooth window close: hide, drain, release, destroy.

- **Claim:** Relaunching Bluetooth during the close drain presents the hidden, closing window, and the drain then destroys it under the person.
- **Level:** integration (the real `open`, `watch` and close handler with its drain; a fake BlueZ on a private dbus-daemon)
- **Method:** `repro_tap_15.rs`, a child of `window.rs`, starts the crate's test bus (`data/test-bus.conf`). Its minimal fake BlueZ holds the GetManagedObjects reply, so the drain waits for that reading (`window.rs:316-318`), as it would wait for a slow CancelPairing. It runs the real first launch `open(&app)`; the system-bus lookup fails here, and `watch` is then pointed at the private bus, as `ui_test.rs` does. It closes the window, calls `open(&app)` again, releases the reply and waits 1.5 s. It asserts that the window shown after the relaunch is visible and not closing, and still is after the drain. Files: `repro_tap_15.rs`, `gtkrun.sh`, `bus.conf`, `repro-shim.c`.
- **Command:** `bash -c '$TMPDIR/gtkrun.sh $CARGO_TARGET_DIR/debug/deps/tatami_bluetooth-c18016033c64ec3c repro_tap_15 --test-threads=1'` (setup — private Xvfb :217, shim, private bus — in the header of `reproductions/support/tatami-gtk/gtkrun.sh`) (in `nix develop`)
- **Observed:** `relaunched during the close drain: (visible, closing) of the window shown right after = Some((true, true)); 1.5 s later, once the drain ended = None; the window shown destroyed: true`
- **Notes:** The drain was held by an in-flight reading rather than by CancelPairing. Both waits are in the same future, and `open` presents the window either way. I didn't assert that the controls are dead; the `closing` checks on refresh, act and the poll were read in the code.
- **Files:** `desktop/tatami/bluetooth/src/repro_tap_15.rs`
- **Harness:** [`support/tatami-gtk/gtkrun.sh`](reproductions/support/tatami-gtk/gtkrun.sh) (private Xvfb, private session bus and a preload shim refusing host services). Run the test through it as Cargo's runner: `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER=$PWD/docs/implicit-state-machines/reproductions/support/tatami-gtk/gtkrun.sh cargo test --locked --manifest-path desktop/tatami/<app>/Cargo.toml <filter> -- --test-threads=1`.
- **Module line added to:** `desktop/tatami/bluetooth/src/window.rs`

<a id="tap-19"></a>

### TAP-19 — Confirmed

Catalog entry: [TAP-19](tatami-apps.md#tap-19), Displays readings and applies (concurrent changes refused, stale readings discarded).

- **Claim:** A monitor plugged in while a slider's change applies (no refresh follows `quietly`), or while a reading runs, isn't shown until another Hyprland event, because those refreshes are dropped rather than queued.
- **Level:** integration (the real `build()` window, with stubs for `hyprctl` and `tatami` and a fake Hyprland event socket)
- **Method:** `repro_tap_19.rs`, a child of `window.rs`, writes stubs into TMPDIR and puts them first on PATH. The `hyprctl -j monitors all` stub answers from a JSON file and can hold its answer; `tatami displays apply` holds until released; `tatami displays lid` and `nightlight status` are answered by the stubs. Hyprland's `.socket2.sock` is a listener under a private `HYPRLAND_INSTANCE_SIGNATURE` in the private runtime dir. (A) The test moves the "Brightness of SDR content" slider of an HDR display (debounced into `quietly`). While the apply is held, it adds DP-2 to the JSON, sends `monitoradded` and `monitoraddedv2`, then releases. (B) While a reading triggered by `configreloaded` is held, it adds DP-2 and sends `monitoradded`. Each case asserts 2 display tiles 1.5 s later. Files: `repro_tap_19.rs`, `gtkrun.sh`, `bus.conf`, `repro-shim.c`.
- **Command:** `bash -c '$TMPDIR/gtkrun.sh $CARGO_TARGET_DIR/debug/deps/tatami_displays-c1f19eab98f57988 repro_tap_19 --test-threads=1'` (setup — private Xvfb :217, shim, private bus — in the header of `reproductions/support/tatami-gtk/gtkrun.sh`) (in `nix develop`)
- **Observed:** `"A: plugged in during a slider's apply: 1 display shown, 2 connected"`, `"B: plugged in during a reading: 1 display shown, 2 connected"`. The stub log shows no `hyprctl monitors: read` after either `monitoradded` (A: `apply: started` … `TEST: DP-2 plugged in` … `apply: finished`, then nothing; B: `read 1 display(s)` … `TEST: DP-2 plugged in during that reading` … `answered`, then nothing).
- **Notes:** I didn't test the hung-helper hazard (no timeout on `tatami displays lid` and `nightlight status`).
- **Files:** `desktop/tatami/displays/src/repro_tap_19.rs`
- **Harness:** [`support/tatami-gtk/gtkrun.sh`](reproductions/support/tatami-gtk/gtkrun.sh) (private Xvfb, private session bus and a preload shim refusing host services). Run the test through it as Cargo's runner: `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER=$PWD/docs/implicit-state-machines/reproductions/support/tatami-gtk/gtkrun.sh cargo test --locked --manifest-path desktop/tatami/<app>/Cargo.toml <filter> -- --test-threads=1`.
- **Module line added to:** `desktop/tatami/displays/src/window.rs`

<a id="tap-21"></a>

### TAP-21 — Confirmed

Catalog entry: [TAP-21](tatami-apps.md#tap-21), Night Shift requests on unordered threads.

- **Claim:** Night Shift requests run unordered: "on" then "off" within a second or two leaves the screen tinted with the switch off, and a debounced temperature request after "off" turns the tint back on.
- **Level:** integration (the real window's switch and slider handlers and `set_night_shift` threads, plus the real `tatami nightlight` helper built from `desktop/tatami/tool`; hyprsunset and its systemd unit are stubs)
- **Method:** `repro_tap_21.rs` uses TAP-19's stubs and the real `build()`. The `systemctl --user … tatami-hyprsunset.service` and `hyprctl hyprsunset temperature [K]` stubs model hyprsunset: its socket is up 1.2 s after start, and state is kept in a file under flock. Each test has its own `XDG_STATE_HOME`. (a) Switch on, then off 0.4 s later. (b) Starting tinted at 4000 K: slider to 3000 K, then the switch off at once. Each waits until both helper runs finish and asserts the screen isn't tinted. The helper's `signal_waybar` would signal the host's waybar. The shim refuses that `kill()`, and the `systemctl` stub kills any helper process that lacks the shim before it gets there. Files: `repro_tap_21.rs`, `repro_tap_19.rs`, `gtkrun.sh`, `bus.conf`, `repro-shim.c`.
- **Command:** `bash -c 'export REPRO_TATAMI_BIN=$CARGO_TARGET_DIR/debug/tatami; $TMPDIR/gtkrun.sh $CARGO_TARGET_DIR/debug/deps/tatami_displays-c1f19eab98f57988 repro_tap_21_switch --test-threads=1; $TMPDIR/gtkrun.sh …tatami_displays-c1f19eab98f57988 repro_tap_21_temperature --test-threads=1'` (setup — private Xvfb :217, shim, private bus — in the header of `reproductions/support/tatami-gtk/gtkrun.sh`) (in `nix develop`)
- **Observed:** (a) `Night Shift switched on, then off 0.4 s later: the switch shows off, the screen is tinted Some(4000) K`. Log: `on: started` 218.112, `off: started` 218.512, `hyprsunset not up`, `off: finished` 218.523, then `hyprsunset: tint 4000 K` 219.351, `on: finished` 219.356. (b) `temperature changed, then Night Shift off within 300 ms: the switch shows off, the screen is tinted Some(3000) K`. Log: `off` → `tint 6500 K` 220.302, then `nightlight 3000: started` 220.583 → `tint 3000 K`.
- **Notes:** The 1.2 s startup is an assumption about hyprsunset; the helper itself allows 2 s for it. Any startup longer than the gap between the two requests gives the same result. No waybar runs on this host, so the shim logged no refused kills.
- **Files:** `desktop/tatami/displays/src/repro_tap_21.rs`
- **Harness:** [`support/tatami-gtk/gtkrun.sh`](reproductions/support/tatami-gtk/gtkrun.sh) (private Xvfb, private session bus and a preload shim refusing host services). Run the test through it as Cargo's runner: `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER=$PWD/docs/implicit-state-machines/reproductions/support/tatami-gtk/gtkrun.sh cargo test --locked --manifest-path desktop/tatami/<app>/Cargo.toml <filter> -- --test-threads=1`.
- **Module line added to:** `desktop/tatami/displays/src/window.rs`
- **Independent review:** upheld; re-run: Both fail: tinted 4000 K with the switch off; tinted 3000 K after off; asserts the claim: Yes; reachability: real: switch and slider handlers and the real `tatami nightlight`; hyprsunset stubbed (1.2 s startup). (b) is deterministic (300 ms debounce after "off"). (a) depends on hyprsunset's startup outlasting the gap

<a id="tap-25"></a>

### TAP-25 — Confirmed

Catalog entry: [TAP-25](tatami-apps.md#tap-25), Joining a Wi-Fi network: one at a time, with a password retry and rollback.

- **Claim:** Closing the Network window while a new network is being joined ends the process mid-join, so the plaintext passwd-file stays in `$XDG_RUNTIME_DIR` and the failed join's new profile is never deleted.
- **Level:** end-to-end (the program's own `main()` in a child process, headless on Xvfb, with a stub `nmcli`; the UI steps are driven from an in-process timer)
- **Method:** `repro_tap_25.rs`, a child of `window.rs`, has a parent test that re-runs its own binary as `window::repro_tap_25::repro_tap_25_child`. The child schedules a timer and calls `crate::main()`. The timer waits for the first reading, calls `join::network` for "Cafe" (WPA2), types a password in the real dialog and clicks its Join button. Once the stub reports `connection up` waiting, it calls `ui.window.close()`, which is what Escape does on the first page. The stub `nmcli` lists one network, keeps added profiles as files, and fails `--wait 45 connection up` after 2 s with "Secrets were required, but not provided". When the child has exited and the stub has failed, the parent asserts that no `tatami-network-<child pid>-*` file remains and no profile is left. Files: `repro_tap_25.rs`, `gtkrun.sh`, `bus.conf`, `repro-shim.c`.
- **Command:** `bash -c '$TMPDIR/gtkrun.sh $CARGO_TARGET_DIR/debug/deps/tatami_network-c76ea3050131eed7 window::repro_tap_25::repro_tap_25_close_while_joining --exact --test-threads=1'` (setup — private Xvfb :217, shim, private bus — in the header of `reproductions/support/tatami-gtk/gtkrun.sh`) (in `nix develop`)
- **Observed:** `passwd-files left in XDG_RUNTIME_DIR ["tatami-network-946611-e1592839-…"]; profiles left ["77ecf448-…"]`. Log: `connection add: Cafe` 242.617, `connection up …: waiting (passwd-file …/rt/tatami-network-946611-…)` 242.620, `window closed while joining` 242.663, `main returned ExitCode(0); the process exits` 242.664, `connection up …: failed` 244.622, and no `connection delete`.
- **Notes:** `main` returned 1 ms after the window closed: nothing holds the application while `background` work runs. Like the real nmcli, the stub nmcli kept running after its parent was gone. With real NetworkManager, the activation would fail on its own and the profile would remain, without a stored password. The test removed the leftover test passwd-file from the private runtime dir.
- **Files:** `desktop/tatami/network/src/repro_tap_25.rs`
- **Harness:** [`support/tatami-gtk/gtkrun.sh`](reproductions/support/tatami-gtk/gtkrun.sh) (private Xvfb, private session bus and a preload shim refusing host services). Run the test through it as Cargo's runner: `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER=$PWD/docs/implicit-state-machines/reproductions/support/tatami-gtk/gtkrun.sh cargo test --locked --manifest-path desktop/tatami/<app>/Cargo.toml <filter> -- --test-threads=1`.
- **Module line added to:** `desktop/tatami/network/src/window.rs`
- **Fixed:** in [#302](https://github.com/bitemyapp/sekka/pull/302). Its regression test runs this scenario against real NetworkManager rather than the stub: the `network-vm` check's subtest "Closing the window while joining leaves nothing behind" (`nix/network-vm-test.nix`). On main, the password file was there while nmcli waited and afterwards, and Office stayed saved.

<a id="ktl-02"></a>

### KTL-02 — Confirmed

Catalog entry: [KTL-02](kakehashi-tools.md#ktl-02), Kakehashi nightly download session.

- **Claim:** dropping a file during a download drops the session without cancelling its worker. Messages are matched by build id only, so after a restart of the same build the orphan's messages drive the new session. Both workers then share one unlocked `.part`, and a fallback truncates it under the other writer.
- **Level:** unit (window state) + integration (core download against a local HTTP server)
- **Method:** Window half (`app/src/repro_ktl_02.rs`): builds a `Kakehashi` directly (`Task::ready`, `Subscription::new`), puts a restarted session for build B in place, then applies an orphan's `Msg::Fetch` and `Msg::Fetched` for B through the real `apply`. It asserts that the session is still running or was told to stop. Core half (`core/tests/repro_ktl_02.rs`): two real `nightly::download` calls for the same build into one directory, against a server that ignores Range. The orphan pauses in `report` after writing at least 1 MiB. The restarted download pauses at its first `Downloading` report, just after the fallback's `set_len(0)`. The orphan then finishes and the restarted one is stopped. The test asserts that the file `download()` returned has the build's SHA-256.
- **Command:** `cargo test --locked --offline --manifest-path apps/kakehashi/Cargo.toml -p kakehashi-core --test repro_ktl_02` and `… -p kakehashi --bin kakehashi repro_ktl_02` (in `nix develop .#kakehashi`)
- **Observed:** core: `download() returned …/sekka-repro.iso as the verified build, but its SHA-256 differs: the restarted download truncated the shared .part to 0 bytes while the orphan had written 1114059; the file now starts with 1114059 zero bytes; the sidecar still says "7c92c49e…ac08  sekka-repro.iso"; the restarted session ended with Err("cancelled")`. Window: `the orphan's Fetched ended the restarted session (download = None, error = Some("the download ended at 11000000000 …")) while that session's worker was never told to stop (cancel = false); before that, the window showed the orphan's progress as the new session's: Some(Downloading { done: 11000000000, … })`
- **Notes:** The trigger itself was established by reading: `this.download = None` at view.rs:638 stores no cancel. The drop listener lives inside `render`, and GPUI test support can't be resolved from Kakehashi's Cargo.lock (`cargo tree --locked --features gpui-kit/test-support` fails with `no matching package named futures-concurrency`), so the listener could not be driven. Correction to the entry: the session that *enters* the fallback second truncates, because `fallback` calls `set_len(0)` on entry. In practice that is usually the restarted session, not an orphan already in fallback. The orphan hashes its own stream, so it still verifies and renames a file that has a hole in it. By reading: if the orphan's `Fetched(Ok)` arrives while the restarted session is current, `apply` calls `choose_image(path, Some(sha))`, which marks the image Verified without hashing it. A later write would only catch the bad image in the helper's copy SHA check (write.rs:208), after the drive is touched.
- **Files:** `apps/kakehashi/app/src/repro_ktl_02.rs`, `apps/kakehashi/core/tests/repro_ktl_02.rs`
- **Module line added to:** `apps/kakehashi/app/src/view.rs`
- **Independent review:** upheld with caveat; re-run: FAIL ×2 as reported: core SHA differs (hole of 1114059 zero bytes); window session ended with `cancel=false`; asserts the claim: Partly. The window half asserts the orphan's messages driving a restart. The core half shows the *entering* session truncating, not "an orphan in fallback" (the report corrects this).; reachability: staged. The window state is built directly, because the drop listener can't be driven (view.rs:633-638 stores no cancel). The core half needs a server that ignores Range, plus forced pauses.. The orphaned worker and id-only matching are real. The corruption outcome needs the range fallback.

<a id="ktl-03"></a>

### KTL-03 — Confirmed

Catalog entry: [KTL-03](kakehashi-tools.md#ktl-03), Kakehashi write job, GUI side of the helper protocol.

- **Claim:** when the helper dies by a signal mid-write, `elevate::run` returns `Ran::Failed("the helper was killed")`, and `JobEnded` maps every `Ran::Failed` to `Failed { touched: false }`, so no "partly written" warning is shown.
- **Level:** unit + integration
- **Method:** `repro_ktl_03.rs`. The unit test applies `Authorizing`, `Job(Preparing)`, `Job(Writing 1 of 4 GiB)` and `JobEnded(Ran::Failed("the helper was killed"))` to a directly built window and asserts `touched`. The integration test runs the real `start_write` worker and `elevate::run`. `repro_ktl_03.sh` uses bwrap to mask `/run/wrappers/bin/pkexec` with a marked fake script, which appends Preparing and Writing lines to the progress file and then `kill -KILL $$`. Real pkexec execs the helper, so a killed helper reaches the GUI the same way. A guard refuses to call `start_write` unless the fake is in place. The real pkexec, the helper and devices are never touched, and the sandbox has no network.
- **Command:** `bash apps/kakehashi/app/repro_ktl_03.sh` (in `nix develop .#kakehashi`)
- **Observed:** `the helper died by SIGKILL after reporting Writing (worker messages: ["Preparing", "Writing { done: 1073741824, total: 4294967296 }", "JobEnded(Failed(\"the helper was killed\"))"]), and the window ends with Failed { message: "the helper was killed", touched: false }: no "partly written" warning`. The unit test fails the same way.
- **Notes:** The `Ran::Exited(code)` path guesses `touched` from the stage (view.rs:322), but the signal path does not. `Ran::Failed` also carries the pre-helper failures (view.rs:561, :567), where `touched: false` is correct, so a fix has to tell the two apart.
- **Files:** `apps/kakehashi/app/repro_ktl_03.sh`, `apps/kakehashi/app/src/repro_ktl_03.rs`
- **Module line added to:** `apps/kakehashi/app/src/view.rs`

<a id="ktl-04"></a>

### KTL-04 — Confirmed

Catalog entry: [KTL-04](kakehashi-tools.md#ktl-04), Kakehashi drive list polling and chosen drive.

- **Claim:** "Erase …?" describes the drive chosen when the dialog opens, but OK looks the chosen drive up again by name in a list that keeps refreshing. If the stick is swapped for one with the same name, the new drive is planned and erased.
- **Level:** integration
- **Method:** `repro_ktl_04.rs`, run by `repro_ktl_04.sh`. The window is built directly with an image and drive list [SanDisk Ultra `sdb`, serial …29115034], with `chosen = "sdb"`. The test takes `chosen_drive()` as `confirm` does (view.rs:479), then applies the refresh `Msg::Drives([Kingston DataTraveler `sdb`, serial …A9580D51])` and calls the real `start_write`. The bwrap'd fake pkexec copies the plan it receives and exits. The test asserts that the job's drive and the plan's drive are `same()` as the drive the dialog named, or that no write started. The same guard and sandbox as KTL-03 apply.
- **Command:** `bash apps/kakehashi/app/repro_ktl_04.sh` (in `nix develop .#kakehashi`)
- **Observed:** `the dialog asked "Erase SanDisk Ultra?" (sdb, serial …29115034); after the refresh chosen = Some("sdb"), and OK started a write of Some("Kingston DataTraveler") (Some("serial …A9580D51")), and the helper's plan names Some("Kingston DataTraveler") (Some("serial …A9580D51")), which passes still_there for the new stick: true`
- **Notes:** The last value is `plan.drive.same(&second)`, the comparison the helper's `still_there` makes (write.rs:61). The refresh pauses only while a job runs (view.rs:208), not while the modal is open. In practice the swap has to happen within the dialog's lifetime and the kernel has to reuse the name (`sdX`, `diskN`).
- **Files:** `apps/kakehashi/app/repro_ktl_04.sh`, `apps/kakehashi/app/src/repro_ktl_04.rs`
- **Module line added to:** `apps/kakehashi/app/src/view.rs`

<a id="ktl-12"></a>

### KTL-12 — Not reproduced (environment)

Catalog entry: [KTL-12](kakehashi-tools.md#ktl-12), Helper write/verify pipeline and `touched`.

- **Claim:** on Windows, `platform::open` deletes the drive layout (windows.rs:309) before `*touched = true` (write.rs:199). A failure or Stop after that point reports `Failed { touched: false }` even though the partition table is gone.
- **Level:** unit (attempted)
- **Method:** I looked for a lower-level seam and found none. `execute` has no injectable platform backend. On Linux, `still_there` runs `lsblk` (found first through the compile-time `KAKEHASHI_TOOL_PATH`), and `open` refuses anything that isn't a block device (linux.rs:112). A regular file can't stand in, and a loop or block device needs root. Linux `open` also changes nothing on the disk, so `touched: false` is correct there. The destructive step is Windows-only code, and this toolchain has no Windows std, so it can't even be type-checked here. The ordering is plain in the source: `open`, then the size check (:184), the second `still_there` (:186), the fit check (:188-194) and the cancel check (:196), all before :199. Windows `open` can also fail after the layout delete (size/geometry/sector checks, windows.rs:310-316).
- **Command:** none run
- **Observed:** n/a
- **Notes:** Needs a disposable Windows VM (QEMU/KVM) with a scratch image that has a partition table, attached as USB mass storage (`-device qemu-xhci -drive if=none,id=stick,file=stick.img,format=raw -device usb-storage,drive=stick`). Make a plan for that `PhysicalDriveN`, create `PROGRESS.cancel` before starting `kakehashi-helper --plan … --progress PROGRESS`, then check that the last progress line is `{"stage":"failed",…,"touched":false}` while `sfdisk -d stick.img` on the host shows no partition table. An image larger than the stick also works: it fails the fit check after `open`.

<a id="ktl-13"></a>

### KTL-13 — Confirmed

Catalog entry: [KTL-13](kakehashi-tools.md#ktl-13), Rootless builder VM host driver.

- **Claim:** the host waits for the whole guest `build-iso` through `run()`, capped at 7200 s, while the guest's `nix flake check` and `nix build .#iso` each have their own 7200 s cap. A build whose steps each fit but together pass two hours is killed by the host.
- **Level:** integration
- **Method:** `repro_ktl_13.rs` (a `rust/tests` file). The test re-executes itself as the host role, which calls the real `support::run` on a guest-role process. The guest role runs `sleep 4000` twice through the real `support::run`, as `build_iso` runs its two nix steps. The test compiles an LD_PRELOAD shim that scales CLOCK_MONOTONIC, `nanosleep` and `clock_nanosleep` by 1000 in every process, so the run takes about 8 s of real time. The shim is checked before use (`sleep 1000` must end in under 5 real s; the host refuses to start if a 5 s sleep takes 1 s of real time or more). The test asserts that the host's `run` returns Ok.
- **Command:** `cargo test --locked --offline --manifest-path rust/Cargo.toml --test repro_ktl_13` (in `nix develop`)
- **Observed:** `the host gave up on the guest's build-iso after 7200 s (scaled clock): Child process timed out; the guest had reported ["step 1 of 4000 s finished within its 7200 s cap"]`
- **Notes:** The host's window also covers `check_selections`, `export_wifi_tools`, the ISO copy and its SHA-256. Code under test is unmodified; only the clock is scaled. This does not show whether real cold builds take that long with `--max-jobs 1`.
- **Files:** `rust/tests/repro_ktl_13.rs`
- **Independent review:** upheld; re-run: FAIL as reported: host `Child process timed out` after guest step 1; asserts the claim: Yes; reachability: staged (LD_PRELOAD scales the clock; real `support::run`, 7200 s cap at support.rs:67). The caps don't compose: the host's single 7200 s cap covers two guest steps capped at 7200 s each. Whether cold builds take over 2 h is not shown.

<a id="ktl-20"></a>

### KTL-20 — Partly confirmed

Catalog entry: [KTL-20](kakehashi-tools.md#ktl-20), (test harness) qemu-test install/reboot run and resumable evidence.

- **Claim:** `boot-installed` rebuilds the fixture's `verify` command from the current CALAMARES_TEST_* environment, while result.json records only `encrypted`. A run installed with `CALAMARES_TEST_GITHUB` set and resumed in a fresh shell silently skips the GitHub assertions, and the reverse fails a good install.
- **Level:** integration
- **Method:** `repro_ktl_20.rs` and `repro_ktl_20.py`. The test runs the real `vm::main` (`qemu-test ISO --install --firmware bios`) and `vm::boot_installed` with `qemu-system-x86_64` on PATH replaced by a Python fake. The fake answers QMP and the guest agent on the real sockets, writes `install.log`, exits on `guest-shutdown` and records every guest command. The repo is a temporary git repo, the 40 GiB disk is sparse, and nothing boots. Scenarios: install with GITHUB=1 then resume fresh; install with GITHUB=0 then resume fresh; install with GITHUB=1 then resume with GITHUB=0. For each, the test compares the install command's effective GitHub setting with the resumed verify's, using the fixture's `flag` semantics (unset means on; only "0"/"false" mean off, vm-fixture main.rs:216-218).
- **Command:** `cargo test --locked --offline --manifest-path rust/Cargo.toml --test repro_ktl_20` (in `nix develop`)
- **Observed:** `result.json records only ["encrypted"]:` / `installed with GITHUB=1, resumed fresh: install ["CALAMARES_TEST_GITHUB=1"] (GitHub step true), verify [] (GitHub checks true)` / `installed with GITHUB=0, resumed fresh: install ["CALAMARES_TEST_GITHUB=0"] (GitHub step false), verify [] (GitHub checks true)` / `installed with GITHUB=1, resumed with GITHUB=0: install ["CALAMARES_TEST_GITHUB=1"] (GitHub step true), verify ["CALAMARES_TEST_GITHUB=0"] (GitHub checks false)`
- **Notes:** Confirmed: the verify options come from the resuming shell and are not recorded. The entry's example does not reproduce. Setting GITHUB and resuming fresh is consistent, because the fixture treats unset as on. The two failures happen with the opposite settings. Installed with `=0` and resumed fresh, verify demands an SSH key and sshd that were never installed: a false failure. Installed in a default shell and resumed with `=0`, verify skips the key, sshd and Git checks without saying so. Of the forwarded variables, only GITHUB affects `verify`. The fixture's verify reads desktops, swap and tuning from the installed configuration, and ENCRYPT is recorded.
- **Files:** `rust/tests/repro_ktl_20.py`, `rust/tests/repro_ktl_20.rs`

<a id="ktl-24"></a>

### KTL-24 — Not reproduced (environment)

Catalog entry: [KTL-24](kakehashi-tools.md#ktl-24), flash_usb privileged write and verify step.

- **Claim:** on macOS, `flash_usb`'s `write` flushes the raw `/dev/rdiskN` handle with `device.sync_all()` (scripts/flash_usb.rs:637). That is F_FULLFSYNC, which raw disks reject with ENOTTY, so every macOS flash fails after the whole image is written, before verification and ejection.
- **Level:** unit (attempted)
- **Method:** The platform-independent part is confirmed by source. Rust std's `sync_all` on `target_vendor = "apple"` is `fcntl(fd, F_FULLFSYNC)` (library/std/src/sys/fs/unix.rs:1268-1271 in the local stable toolchain's rust-src). `write` calls it on the handle from macOS `open` (flash_usb.rs:287-298) and propagates the error with `?`. The ENOTTY itself is macOS kernel behaviour and can't be exercised on Linux. In-repo evidence: commit 47957b09 changed Kakehashi to `DKIOCSYNCHRONIZECACHE`, and DEVELOPERS.md says raw disks "reject with ENOTTY even after all image bytes were written". The opt-in test `flush_authorized_usb_without_writing` (core/src/platform/macos.rs:221-229) asserts exactly that ENOTTY on a real USB stick. `flash_usb.rs` was not given the same fix.
- **Command:** none run
- **Observed:** n/a
- **Notes:** Needs a Mac with a disposable USB stick. The quickest check doesn't write anything: `KAKEHASHI_CHECK_USB=diskN cargo test -p kakehashi-core -- --ignored flush_authorized_usb_without_writing` reproduces the ENOTTY from `sync_all` on the raw device. Full check: `scripts/flash_usb.rs small.img` on the stick, expecting `Inappropriate ioctl for device (os error 25)` after Writing, no "Reading back", and no `.flash.json`. A RAM disk (`hdiutil attach -nomount ram://…`, then `/dev/rdiskN`) might stand in for the fcntl check; I have not verified that.

<a id="inx-18"></a>

### INX-18 — Partly confirmed

Catalog entry: [INX-18](infra-nix.md#inx-18), Copy-to-RAM of the live media in the initrd.

- **Claim:** copytoram sizes the media by the block device holding `/sysroot/iso`. On a whole USB stick, the stick's capacity rather than the image is compared with RAM, so copy-to-RAM is skipped even though the image would fit.
- **Level:** unit (the exact `text` of `nix/iso-copytoram.nix` under writeShellApplication's `errexit/nounset/pipefail`)
- **Method:** The script runs in bwrap with a fake `/proc/meminfo` (30 GiB available). The mount and inspection tools are stubbed and logged (findmnt, blockdev, mount, umount, mountpoint, mktemp, cp), so nothing is mounted, copied or read. The case is a 64 GB stick at `/dev/sdz` holding a 3.5 GB image. The test asserts the script copies (`complete;`). The control is the partition `/dev/sdz1` at the image's size, and it passes. Files: `repro_inx_18.py`
- **Command:** `bash -c 'PATH=$B:$PATH python3 tests/repro_inx_18.py -v'` (in `nix develop`)
- **Observed:** `Sekka copy-to-RAM: insufficient RAM (media 64023257088 bytes, available 32212254720, reserve 4294967296); continuing from installation media.`, after only `findmnt … /sysroot/iso` and `blockdev --getsize64 /dev/sdz`.
- **Notes:** Reproduced: the decision uses the device's size, not the image's. Not reproduced: that a raw-written hybrid ISO really mounts from the whole disk. On a hybrid stick, both `sdX` and `sdX1` carry the ISO9660 label, and udev's `60-persistent-storage.rules` gives both a `by-label` link, so which one wins is the open question. Settling it needs a VM booting the ISO from a `usb-storage` disk larger than the image, with copytoram on, then checking `findmnt /sysroot/iso` and the journal.
- **Files:** `tests/repro_inx_18.py`

<a id="inx-29"></a>

### INX-29 — Partly confirmed

Catalog entry: [INX-29](infra-nix.md#inx-29), IBus Wayland frontend keymaps, seat and system-keymap mode (patched).

- **Claim:** After a live layout change (US → German) with `use-system-keyboard-layout` on, the early return at patch `:175-178` keeps the old `key_sys`. Engines then get keysyms and modifiers from the old layout, which would explain the known limit in DEVELOPERS.md.
- **Level:** unit (a C harness: the struct, the keymap updater and `input_method_keyboard_keymap`'s body copied verbatim from the patch's `+` lines; `create_system_xkb_keymap` reconstructed; real libxkbcommon and xkeyboard-config; a recording virtual keyboard)
- **Method:** The compositor sends the US keymap, then the German one. The harness compares KEY_Y in IBus's active (system-mode) keymap with the keymap the virtual keyboard received, and also computes the keysym an engine gets using `input_method_keyboard_key`'s two lines. It runs three modes: `engine` (key_user from the xkb:us engine), `seeded` (an engine with no layout, so key_user comes from the first system keymap), and `control` (German from the start). Files: `repro_inx_29.c`
- **Command:** `bash -c 'cc -o $TMPDIR/inx29 tests/repro_inx_29.c $(pkg-config --cflags --libs xkbcommon) && for m in engine seeded control; do XKB_CONFIG_ROOT=/nix/store/ghnpw9rvjiz7iski1gl7ifmkrvf8pkrx-xkeyboard-config-2.48/share/X11/xkb $TMPDIR/inx29 $m; done'` (in `nix develop`)
- **Observed:** 
  - `engine`: virtual keyboard KEY_Y → z, key_sys → y, keysym to the engine → y. Fails.
  - `seeded`: virtual keyboard → z, key_sys → y, keysym to the engine → y. Fails.
  - `control`: key_sys → z. Passes.
- **Notes:** Confirmed: `key_sys`, and a `key_user` seeded from the system keymap, keep the first keymap while the virtual keyboard follows the change. Not as claimed:
  - With an engine's own layout, the keysym an engine receives comes from `key_user` in both modes, so it is unaffected. A stale `key_sys` then only changes Multi_key detection and the modifier masks.
  - The patched code forwards every new keymap to the virtual keyboard, so this early return does not explain the documented "virtual-keyboard path using the previous keymap".
  - The real frontend was not rebuilt: the patched IBus source isn't in the store. The live symptom needs `tests/aizome-ibus.py` in a VM with a layout switch mid-session.
- **Files:** `tests/repro_inx_29.c`

<a id="inx-34"></a>

### INX-34 — Confirmed

Catalog entry: [INX-34](infra-nix.md#inx-34), Binary-cache publication pipeline.

- **Claim:** The upstream-validity filter runs in process substitutions, so a `nix-store --check-validity` failure part-way is ignored. The script uploads a subset, prints "Published N of M" and exits 0.
- **Level:** integration (the exact `text` of `nix/push-cache.nix` under writeShellApplication's options; stub `nix`, `nix-store`, `aws`; nothing else of the host on PATH)
- **Method:** The stubs serve a 32-path closure (12 Sekka paths, 20 Nixpkgs paths). In the failure case, cache.nixos.org's check prints the invalid paths among the first 10, then exits 1 with an HTTP 503 error. The `aws` stub only records what `s3 sync` would upload. The test asserts a non-zero exit. The control (all caches answer, 12 of 32 published, exit 0) passes. Files: `repro_inx_34.py`
- **Command:** `python3 tests/repro_inx_34.py -v` (in `nix develop`)
- **Observed:** `exited 0 with stdout 'Published 5 of 32 paths; the other caches serve the rest' after uploading 5 of 12 narinfos; stderr: "error: unable to download 'https://cache.nixos.org/xyz.narinfo': HTTP error 503"`
- **Notes:** The stub follows Nix's `--print-invalid` loop, which prints as it goes and then throws. Even if nothing were printed before the error, the script would publish 0 and still exit 0. The same applies to the later two upstreams and to `nix path-info` (`:61`).
- **Files:** `tests/repro_inx_34.py`
- **Independent review:** upheld; re-run: FAIL as reported: exit 0, "Published 5 of 32". Control passes.; asserts the claim: Yes; reachability: real (an upstream 503; the exact script text under writeShellApplication's options). `mapfile < <(…)` ignores nix-store's exit status (push-cache.nix:61-66).

<a id="inx-36"></a>

### INX-36 — Confirmed

Catalog entry: [INX-36](infra-nix.md#inx-36), Self-hosted runner supervisor and per-job admission.

- **Claim:** When `docker run` cannot start the job container, the supervisor registers a new JIT runner with GitHub every ~5 s (plus the git-cache refresh), with no backoff.
- **Level:** integration (the exact supervisor `text` of `nix/runner.nix` with its interpolations filled from the file; git-cache script stubbed; the token path moved to a temp file)
- **Method:** Stub `docker` makes `wait` find no container and `run` exit 125 with "bind source path does not exist: /var/cache/sekka-ci-git". Stub `curl` logs the generate-jitconfig POST and returns a made-up config. Stub `sleep` records each pause and ends the run after six registrations. jq, timeout and date are real. The test asserts at least 60 s of pauses between registrations that follow a failed start, matching the script's other failure paths. Files: `repro_inx_36.py`
- **Command:** `bash -c 'JQ=/nix/store/9l0j15jg8634hkb6d4c9y95mpxl0nhrd-jq-1.8.2-bin/bin/jq python3 tests/repro_inx_36.py -v'` (in `nix develop`)
- **Observed:** `6 JIT registrations with pauses [5.0, 5.0, 5.0, 5.0, 5.0] s between them, each after docker run exited 125; supervisor said: […'ended with status 125.', '… is waiting for a job.', '… ended with status 125.']`
- **Notes:** The supervisor cannot tell "container never started" (125) from a finished job. In production each iteration also runs the git-cache refresh (up to 180 s), so the real rate is lower than one registration per 5 s. A missing image would also make `docker run` try a pull from Docker Hub each time; that is from reading only.
- **Files:** `tests/repro_inx_36.py`

<a id="ici-03"></a>

### ICI-03 — Confirmed

Catalog entry: [ICI-03](infra-ci-tests.md#ici-03), Trusted PR admission, recheck and commit-status lifecycle.

- **Claim:** If main moves while an admitted PR waits for the builder, the merge recheck fails. The `result` job then marks the head `failure` ("Build and tests: failure") although nothing was built or tested.
- **Level:** integration (the real `scripts/check-pr-trust.py` run as `__main__`, with only `urlopen` replaced by a fake GitHub API over a temporary git repository; the real `run:` blocks extracted from `.github/workflows/pr.yml`; a stub `gh` records the status POSTs; Actions job semantics emulated)
- **Method:** 
  1. trust gate → the pending status is posted.
  2. A nightly-style commit lands on main, and GitHub's test merge is recomputed: new merge `[B1, head]`.
  3. The recheck gate runs, then "Require the same admitted merge" (fails) → build job result `failure`.
  4. "Report against the tested PR head" runs.
  The test asserts the final status is not `failure` when nothing was built. The control, with main unchanged, gives `pending → success` and passes. Files: `repro_ici_03.py`
- **Command:** `python3 tests/repro_ici_03.py -v` (in `nix develop`)
- **Observed:** `nothing was built, yet the head 37f3985471b2 got [('pending', 'Building and testing the verified merge commit'), ('failure', 'Build and tests: failure')]`
- **Notes:** GitHub Actions itself was not run; the step and job ordering is emulated. "Nothing re-admits the PR" is from reading the triggers, which are only PR events and manual dispatch. The other hazards in the entry, a late `error` overwriting a same-head rerun's `pending` and a denied run posting nothing, were not exercised.
- **Files:** `tests/repro_ici_03.py`

<a id="ici-11"></a>

### ICI-11 — Confirmed

Catalog entry: [ICI-11](infra-ci-tests.md#ici-11), Video playback state (poster/loading/playing/paused/ended/failed) with seek-drag resume.

- **Claim:** While dragging the seek thumb during playback, pressing K twice makes `play()` create source S1. On pointer-up, `release()` still sees `resume` and calls `begin()` again, so two soundtracks play. The orphan can't be paused, and its `onended=finish` later jumps the player to `ended`.
- **Level:** integration (the real `site/video.js` loaded in Node 24 with a fake DOM, a recording Web Audio context and a fake Wasm glue module)
- **Method:** The test clicks Play, sends pointerdown on the seek bar, K, K, then pointerup on the window. It asserts exactly one source is playing. A second test covers pause during a drag then pointer-up and asserts the player stays paused with nothing playing. The control, a plain drag during playback, passes. Files: `repro_ici_11.cjs`
- **Command:** `/nix/store/j1j9b0w3mhpccdli73ycj5w7nf6fkzqz-nodejs-24.21.0/bin/node --test-reporter=spec tests/repro_ici_11.cjs` (in `nix develop`)
- **Observed:** 
  - `soundtracks playing after pointer-up: [2,3]`.
  - `after K (pause): ["paused",[2]]`: the orphan keeps sounding while the player says paused.
  - After S2's `onended`, the state is `"ended"`.
  - Second test: `pointer-up after pausing restarted playback`.
- **Notes:** The fake's sources don't stop by themselves at their end, so the `[2]` after the orphan's end is an artefact of the harness. What matters is that `finish()` stopped the current source and jumped to `ended`. A real browser was not driven.
- **Files:** `tests/repro_ici_11.cjs`
- **Independent review:** upheld; re-run: FAIL ×2 as reported: sources [2,3] playing; pause during a drag restarts. Control passes.; asserts the claim: Yes; reachability: real (keydown reaches the player while dragging; the fake DOM drives the real video.js). `release()` calls `begin()` without `halt()`, and `resume` survives a pause (video.js:340-351).

