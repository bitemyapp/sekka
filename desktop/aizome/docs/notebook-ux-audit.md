# Notebook UI/UX audit

Transcribed from the photographed notebook supplied on 2026-10-20. Crossed-out
items and tentative margin notes are retained below so they are not mistaken
for omissions. Failures were reproduced against Aizome `a4dd2fa`; the final
branch also includes main through `b4e9cd6`. The independent repository audit
checked Sekka main `70e1819`, Sidestep main `626422b`, and the open PRs in all
three repositories. The original evidence below is historical; the reconciliation
against current main and its validation are recorded at the end.

## Transcription and disposition

| # | Note | Evidence / disposition |
|---|---|---|
| 1 | Deskbar re-ordering | Fixed: drag app headers or use Move Up/Down in their menu. Order persists independently of pin membership and remembers absent apps. Interaction tests cover reordering after scrolling and preserving window dragging. |
| 2 | “Sudo modal?” | Already implemented as the native polkit authentication overlay; `docs/shell.md`, `crates/aizome-shell/src/auth.rs`, `crates/aizome-shell/tests/auth.rs`. This is an authorization dialog, not a general terminal sudo replacement. |
| 3 | “aizome.computer!” | Domain/branding reminder; no UI failure or requested domain operation specified. |
| 4 | Zed and VS Code should be in Dev Toolchest | Fixed: Development takes precedence over generic Utility/TextEditor categories. Reproduced using the installed Zed and Code desktop-entry categories; regression failed before the change. |
| 5 | Get exclusive fullscreen working | Fixed: preserve fullscreen requests before the first map/configure and honor the requested output, including moving an already-fullscreen window. Real Wayland clients reproduce both failures on the baseline. |
| 6 | ~~Fix third party systray occupancy~~ | Already on main: `fcdb02f`, StatusNotifierItem support for Steam, Discord, Telegram. |
| 7 | Make exclusive fullscreen efficient | Fixed: frame callbacks follow actual renderer visibility, throttling clients/layers covered by fullscreen while letting them resume afterward. The baseline covered client rendered 18 frames during 12 foreground frames. Direct DRM scanout was already enabled; hardware scanout is not inferred from this headless test. |
| 8 | Get a native aarch64 build working in UTM on macOS Apple Silicon | Existing [Sekka PR #42](https://github.com/bitemyapp/sekka/pull/42), head `d22b97c`, contains the ARM64 UTM work. Its recorded installation tests cover Plasma/Tatami; Aizome-specific ARM runtime coverage is not established by that evidence. |
| 9 | Fix Deskbar overlap with the apps | Already merged as [Aizome PR #2](https://github.com/bitemyapp/aizome/pull/2), `cd8a627`. Real headless compositor layout and desktop tests cover initial/restored windows, launcher closure, output changes and slow clients. |
| 10 | Check Deskbar overflow | Fixed: cap the surface to available output height and scroll its content, keeping the Shelf and final app reachable. |
| 11 | Make keyboard shortcuts for window management | Already implemented: `docs/ipc.md` lists bindings, including Alt+Super+arrows, Alt+Super+S and Ctrl+Super+F. All seven compositor keyboard tests pass. |
| 12 | Fix radial top half disappearing. Titlebar is usually at the top anyway | Fixed: the quick-click path now clamps its center as the hold path does. Real pointer input and a screenshot verify that the top action is visible at the output edge. |
| 13 | Add an option to make the Deskbar auto-hide | Fixed: Settings → Desktop provides the option. Hovering the menu-bar app control reveals it; leaving hides it after a short grace period. Re-entry, menus and dragging prevent premature hiding. A transient reveal overlays apps without resizing them. Verified with a real shell and compositor. |
| 14 | Add a button to manually hide and un-hide the Deskbar as well | Fixed: a visible menu-bar Hide Deskbar control complements the existing “N apps” restore control. Both day/night menu-bar goldens checked. |
| 15 | Screenshot cursor doesn't properly persist until the SS is taken | Fixed: effective cursor policy keeps the area/window picker cursor through motion and selection, then returns to the normal cursor. Real-client regression fails before the change. |
| 16 | “add window” (screenshot margin) | Already implemented: press Space in the area picker to select a window. Picker/overlay integration tests pass. |
| 17 | Add options for overriding the modifier keys [in] the settings app | Already on main: `53719cb`, Keyboard → Modifier keys; matching goldens corrected in `58dde5a`. |
| 18 | Raising a sub-menu in command palette should go to the app when it already exists | Fixed: activation prefers the app's most recent window on the current Desk, falling back to another Desk; New Window still launches. Settings forwards the requested pane and explicitly raises its existing window. Unit regressions and a real shell/Settings test cover the hidden-window case. |
| 19 | Clicking on a notification should raise en place or go to the app that popped the notification | Fixed: activate the originating app while preserving its default action and notification residency. Without an action, focus the running app or launch a known desktop entry. Tests cover identity/recency, closed known apps, unknown senders, default actions, residency and dismissal; a real notification click raises hidden Settings on another Desk. |
| 20 | Make Steam games show up in a games menu in the Toolchest | Fixed: the `Game` desktop-entry category maps to a Games menu. Verified local installed Steam game entries use this category; regression failed before the change. |
| 21 | Don't show a settings menu when there are none | Fixed: standard kit menus omit the unimplemented Settings role. Apps opt in with a real action, as Tenki and Koyomi already do. A negative-control run confirmed the old standard menu fails the new regression. |
| 22 | ~~How do I remove an app from a tab group?~~ “alt-click, should this be super” | Current documented gesture is Alt-drag for an active tab, ordinary drag for a background tab. Keep this distinct from a confirmed request to remap it. |
| 23 | Make a heterogeneous tab group window demo | Added a repeatable private-desktop Chrome + Soroban demo. It groups client/server-decorated windows, switches tabs and Alt-drags a tab out, saving three screenshots. Run instructions are in `docs/ipc.md`; all steps passed and screenshots were inspected. |
| 24 | Add extensibility as an optional thing, wasm and rust! | Architectural feature reminder. Rust kit/shell/IPC APIs already exist; optional Wasm hosting needs an execution/capability/API contract before it is a defined bug fix. |
| 25 | “Computer use! agentic sympathy” | Existing compositor IPC input/screenshot facilities and kit semantic inspection are relevant. [Sidestep PR #4](https://github.com/bitemyapp/sidestep/pull/4) adds a story/driver harness; avoid duplicating it. |
| 26 | Tenki GeoClue takes too long, maybe doesn't work | Fixed: the whole GeoClue request has a deadline, cancellation and bounded cleanup; disabling location/retrying cannot apply stale results. City search is available immediately while pending or after failure. Fake private D-Bus tests cover stalled calls and cancellation; pending/error UI states were visually checked. |
| 27 | Two different Wi-Fi systray icons is annoying; looks like a GNOME or vanilla wpa_supplicant icon maybe | Existing [Aizome PR #6](https://github.com/bitemyapp/aizome/pull/6) replaces nm-applet with a native credential agent while preserving authentication. Do not fix by hiding an icon or disabling the only credential agent. |
| 28 | Crossed-out margin “print screen”, “from”, “SS” (partly unclear) | Print Screen opens the picker on main (`5d8bdd5`). No additional legible requirement beyond the screenshot notes above. |

## Existing work checked

Sidestep's Aizome toolkit is developed on the `aizome` branch;
[PR #3](https://github.com/bitemyapp/sidestep/pull/3) targets `main`.
Aizome pins `dae957e`; the audited PR head `16a1e376`
adds test/environment changes, not a missing runtime fix for these notes.

The related [Aizome PR #7](https://github.com/bitemyapp/aizome/pull/7) fixes stray
button releases after a picker closes. It is distinct from cursor persistence.
The screen-sharing portal fix is already on this baseline as PR #9.

## Validation record

The regression tests run in three layers:

- Model and kit tests: Toolchest categories, opt-in Settings menus, strict
  application identities and focus history, notification actions, Deskbar
  scrolling/reordering/visibility, and Settings persistence.
- Real Wayland sessions: private headless compositors, native test clients,
  the actual shell and Settings, and Chrome/Soroban. These exercise fullscreen
  configuration and output choice, covered-client frame callbacks, pointer
  cursors, marking menus, layout, activation, and heterogeneous tab groups.
  `docs/ipc.md` records the optional desktop test and demo commands.
- Private D-Bus services: GeoClue setup/read deadlines, cancellation, provider
  refusal, cleanup and stale results; existing polkit authentication tests.

Visual review includes day/night Deskbar and menu bars, the standard app menu,
Tenki's pending/error states, the top-edge marking menu, Settings activation,
and the three checked-in [mixed-window demo images](images/heterogeneous-group-calculator.png).

The baseline regressions failed before their corresponding fixes. The two
activation integration checks additionally verify that the same Settings
window is raised, rather than accepting a newly created replacement.

The final review also caught a race in the existing real-desktop layout test:
the shell's palette model changes before its Wayland layer receives keyboard
focus. In 12/12 instrumented opens, an immediate Escape still went to the app.
The desktop fixture now waits for compositor-confirmed layer focus before
sending palette keys, rather than relying on model state or a longer sleep.

Original integrated checks (inside `nix develop`, with the distribution's font
directory supplied as `AIZOME_FONT_DIR`):

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` and `git diff --check` | Passed. |
| Strict Clippy, all targets of shell, face, config, kit, sys, Tenki, Settings, compositor core/logic/executable | Passed with `-D warnings`; the two native desktop targets were checked again after the focus-wait change. |
| Shell, config, kit, sys, Tenki and Settings suites, excluding wallpaper goldens and timing tests | 816 passed in the grouped run. The one audio latency failure during compiler load passed in the separate run below. Existing ignored hardware/session tests remained ignored. |
| Compositor core and logic library tests | 60 passed. |
| Fullscreen, picker cursor, marking edges, layout, protocols and recovery integration suites | 28 passed; one subprocess helper remained ignored as intended. |
| Fullscreen frame callbacks and pre-map fullscreen recovery | Both passed. The latter builds and loads the panic-injection logic variant. |
| Actual shell/Settings and Chrome/Soroban desktop suites | All five passed, including launcher layout, auto-hide, mixed tab groups and both activation paths. |
| Separate shell/Tenki timing and audio checks | All six passed under `/tmp/aizome-benchmark.lock`; shell frame p95 was at most 1.84 ms, Tenki interaction p95 at most 3.14 ms. These are local test-build measurements, not release or hardware scanout benchmarks. |
| Settings general and hardware-pane budget suites | Both passed separately under the same lock; their release-only timing assertions are disabled in this test build. |

The grouped UI command uses `cargo test --locked` for `aizome-shell`,
`aizome-config`, `aizome-kit`, `aizome-sys`, `aizome-tenki` and `aizome-settei`.
The compositor integration commands use `cargo test --locked -p aizome-comp`
with the test target names listed above. Native desktop commands and their
required executable variables are documented in `docs/ipc.md`.

The full-screen desktop and lock-screen shell goldens depend on wallpaper
files outside this repository at
`/home/callen/work/aizome-design/reference/wallpapers`, which are absent here.
Those existing golden failures are not overwritten or counted as passing.

No installed OS, physical input, or login-session configuration was changed.
The fullscreen protocol change alters the core ABI: installing it requires a
compositor restart, rather than reloading only the logic library.
There is no new hardware scanout measurement or Apple Silicon/UTM installation
claim. ARM64 UTM and duplicate Wi-Fi-agent work remain in the existing PRs
linked above; the domain and optional Wasm notes remain design reminders
without a specified implementation contract.

## Follow-up: Myaku application attribution

The later screenshot and report add an issue outside the 28 handwritten
notes: Fusen (and sometimes Claude) incorrectly includes unrelated browser,
editor, terminal and build processes. This was not fixed in the audited main
branches or existing PRs.

The cause is reproducible: the old grouping joins every non-manager parent
edge, but does not recognize `aizome-shell` as a manager. Its applications
become one group; whichever window is encountered first supplies the label.
The old identity cache also misses same-size membership/metadata changes.
A regression using the actual shell/app/worker topology fails on that code.

The fix makes windows and dedicated application/service units explicit
boundaries in process trees, preserves genuine helpers and commands, and
uses unit evidence to reunite orphaned workers only when ownership is
unambiguous. Both sides of an application boundary remain separate even
when the parent has no open window. Session scopes, slices, manager units,
generic runtimes and ambiguous executable matches cannot claim unrelated
applications. Labels are independent of window focus/order, and the cache
tracks all membership and naming inputs.

The inspector explains each group's evidence and links a member to its
group separately from its actual Parent. Unit-derived labels say where the
name came from. The `groups` example in `apps/myaku/examples` reads the same
sampler without opening a window or sending signals, providing a repeatable
probe for attribution on a real desktop.

Validation includes negative controls for the shell merge and the
windowless-parent/shared-scope case, process and identity regressions,
inspector navigation/layout tests, and both day/night inspector goldens.
The live probe on the affected desktop separates Fusen into its one process
(about 5.7 MiB in the initial fixed sample), with Claude and ChatGPT in their
own groups. Real helper and command descendants remain grouped with their
owning application. No process was signalled and the installed Myaku was
not replaced.

The final targeted run passed 39 Myaku tests (26 library, nine interaction,
three visual and one frame/memory check) and 11 application-index tests;
one existing live-launcher test remained ignored. Strict Clippy for all
Myaku/sys targets, formatting and diff checks passed. The memory check
stayed at 228416 KiB over 200 additional readings. Under the shared benchmark
lock, test-build frame medians were 3.81 ms per reading, 0.65 ms per row
click and 0.84 ms per scroll at 2× with 600 fixture processes; this is not
a release-build budget measurement.

## Reconciliation with main, 2026-10-24

PR #17 is partially overlapping, not redundant. It now includes main through
`f05a379` and retains the distinct notebook fixes above. The integration keeps
the newer fixes and their regression coverage:

| Merged work | Combined behavior |
|---|---|
| [#25](https://github.com/bitemyapp/aizome/pull/25), application activation | Keep strict desktop/WM_CLASS identity, current-Desk preference, tab-group focus history, and alias-aware Hide/Quit. Settings-pane and notification activation use that same selection policy. |
| [#45](https://github.com/bitemyapp/aizome/pull/45), unsupported menu roles | Omit standard Settings as requested by notebook item 21. Explicit handler-less Settings/Help roles remain unavailable; message-backed application actions retain their declared enabled states. |
| [#44](https://github.com/bitemyapp/aizome/pull/44), Tenki city search | Preserve search-generation race protection alongside GeoClue deadlines, cancellation, stale-location rejection and manual search. |
| [#48](https://github.com/bitemyapp/aizome/pull/48), Myaku launch grouping | Keep the shell separate from its apps and treat every new `app-aizome-*.scope` as an independent launch, even with the same app ID and visible windows. Extend this with window/service boundaries, unambiguous orphan attribution, complete cache invalidation and inspector evidence. Browser helper scopes remain with their owner unless explicitly launched by Aizome. |
| [#61](https://github.com/bitemyapp/aizome/pull/61), Myaku GPU reporting | Keep per-device GPU accounting and inspector details alongside the new grouping explanation. |

Picker-release, modal-input, lid-output, greeter-scaling and authentication
changes from main are retained. They are not additional implementations in
this PR.

The combined activation regressions also exposed a case-folding bug for app
IDs that themselves end in `.desktop`: `ORG.TELEGRAM.DESKTOP` lost part of its
identity. Matching now checks the complete ID before interpreting the suffix
as a desktop filename, without introducing executable or suffix guesses.

Current validation uses the pinned Nix development environment and packaged
fonts. These results supersede the historical counts above:

| Check | Result |
|---|---|
| Shell, config, kit, sys, face, Tenki, Settings and Myaku unit/integration suites | 947 checks passed across the grouped run and targeted reruns. Includes the merged modal, search-generation, authentication, GPU and activation regressions. 39 existing hardware/session tests remain ignored. |
| Compositor core and logic libraries | 68 passed. |
| Fullscreen, frame callbacks, picker, marking edges, layout, headless, displays, protocols and recovery | 65 passed; one subprocess helper remains ignored. |
| Actual shell/Settings and Chrome/Soroban desktop suites | All six passed, including both original notebook activation cases and the merged launcher activation test. |
| Myaku images and frame/memory checks | All five image tests and both performance/memory checks passed. Updated and visually reviewed both GPU inspector images to include Grouping while retaining per-device readings. Test-build timing is not a release/hardware benchmark. |
| Editor regression tests after current-Clippy compatibility cleanup | All 13 passed; range-vector test data is unchanged. |
| Fullscreen grant followed by logic failure before mapping | Passed with the separately built panic-injection variant. |
| Formatting, diff checks and strict Clippy | Passed for all targets of shell, config, kit, sys, face, Tenki, Settings, Myaku and compositor core/logic/executable. |
| Packaged NixOS VM checks | `standard-menu` and `myaku-grouping` both passed. The former exercises omitted Settings, unavailable Help, working About and actual Koyomi/Tenki Settings. The latter launches real apps, opens Sumi from Hako, and checks independent groups and their controls. |

Three shell wallpaper goldens remain excluded because their external wallpaper
assets are absent, as recorded above. The grouped run also excluded seven
timing checks; the two Myaku checks were subsequently run. No other golden
expectations were relaxed.
