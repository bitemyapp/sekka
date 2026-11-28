# KIT: Toolkit runtime

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/crates/aizome-kit`. Audited at `8e0acbe5`; line numbers refer to that commit.

60 files audited. 33 implicit machines: 7 High, 9 Medium, 17 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

<a id="kit-04"></a>

### KIT-04 — Menu publication to the shell (dedup and debounce)
- **Where:** `desktop/aizome/crates/aizome-kit/src/app.rs:62` (`Runtime.menus_published`); `desktop/aizome/crates/aizome-kit/src/engine.rs:90` (`TimerUse::Menus`); `desktop/aizome/crates/aizome-kit/src/shell.rs:252` (`find`)
- **Encoding:** hash of the last menus queued for publication, and whether a `TimerUse::Menus` sits in `Core.timers` (debounce armed)
- **States:** `Current(hash)` → `Pending(new hash, 40 ms timer)` → `Sent`; the shell's copy can lag by up to 40 ms, or until its next `AboutToShow`
- **Transitions:** `refresh_menus` after every update and rebuild sets the hash and arms the timer if none is armed (`app.rs:255-268`, called at `app.rs:177-178`, `:188`); the timer → `publish_menus` (`app.rs:279-284`); a new link publishes at once and resets the hash (`app.rs:124-128`); `AboutToShow` refreshes and publishes (`shell.rs:288-291`); `Activate{id}` → `find` → `perform` (`shell.rs:278-287`)
- **Hazards:** the hash is recorded before the send (`app.rs:261`) and send failures only log (`shell.rs:84-86`), so a failed publish isn't retried until the menus change; `perform` (`engine.rs:2481-2511`) rechecks modal scope but not `enabled`, and `find` returns disabled actions
- **Reify:** High — cross-process staleness with a suspected bug
- **Suspected bug:** the shell's open menu shows Save enabled; the app disables it (republished 40 ms later); a click in the shell sends `Activate{id}`, `find` returns the now-disabled Save and `perform` queues its message anyway (confidence: low)
- **Related:** aizome-shell menu server `desktop/aizome/crates/aizome-shell/src/shell.rs:820` (AboutToShow), `:833` (Activate)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#kit-04)

<a id="kit-07"></a>

### KIT-07 — File panel lifecycle and browsing state
- **Where:** `desktop/aizome/crates/aizome-kit/src/dialogs.rs:298` (`PanelState.dir`, `.entries`, `.selected`, `.name`, `.hidden`, `.error`, `.done`); `desktop/aizome/crates/aizome-kit/src/engine.rs:303` (`Core.panels`), `engine.rs:156` (`Win.panels`)
- **Encoding:** panel id in `Core.panels` and `Win.panels`; browsing state in `PanelState`; completion is the one-shot `done` callback
- **States:** `Open(dir, listed | error, selection, name)` → `Finished(Some(paths))` | `Cancelled(None)`; `chosen()` may navigate (Open→Open) instead of finishing
- **Transitions:** `show_panel` (`dialogs.rs:421-456`, pending focus `:455`); `Go`/`Up`/`Hidden` relist (`dialogs.rs:472-477`, `:497-500`); `Select` (`dialogs.rs:478-486`); `Activate`/`Confirm` → `chosen` (`dialogs.rs:487-495`, `:501`, navigation `:402-408`); `Cancel` (`dialogs.rs:502`); `finish` removes both and queues `done` (`dialogs.rs:458-465`)
- **Hazards:** `close_win` drops alerts but not panels (`engine.rs:628-630`); message indices index `entries` unchecked (`dialogs.rs:482`, `:403`, `:417`), so a stale index after a relist panics; `chosen()` mutates while deciding; listing runs synchronously on the UI thread (`dialogs.rs:340-379`)
- **Reify:** High — leak and lost completion
- **Suspected bug:** a program closes a window with `Cx::close_window` while its open or save panel is up: the `PanelState` stays in `Core.panels` for good and `done` never fires, so a program waiting for `Saved(None)` never leaves its saving state (confidence: high)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#kit-07)

<a id="kit-13"></a>

### KIT-13 — Pointer hover, press and capture
- **Where:** `desktop/aizome/crates/aizome-kit/src/engine.rs:132` (`Win.hot`), `engine.rs:136` (`.hot_under`), `engine.rs:137` (`.active`), `engine.rs:142` (`.press_under`), `engine.rs:149` (`.pointer`), `engine.rs:151` (`.cursor`)
- **Encoding:** options naming the hovered node, the node under a drags-only node, the node that took the press (it gets the moves and the release), and a deferred click for a drags-only press
- **States:** `Outside` → `Hovering(hot[, hot_under])` → `Pressed(active)` → `Hovering`; `DragsOnlyPressed(active, press_under)` → release without a drag → synthesized click under it; `NativeDrag` (a widget drag clears `active`); invalid: `press_under` naming a node other than `active`
- **Transitions:** `pointer_move` routes to `active` when set (`engine.rs:2026-2053`); `update_hot` (`engine.rs:1932-2024`); `pointer_leave` clears hover, and `active`/`press_under` only for drags-only (`engine.rs:2055-2086`); a press resets `press_under` (`engine.rs:2125`), `press` sets `active` only if none (`engine.rs:2254-2256`), drags-only keeps `press_under` (`engine.rs:2135-2141`); a release takes both and may `click_under` (`engine.rs:2143-2175`, `:2181-2196`); capture requests (`engine.rs:1674-1678`); a widget drag clears `active` (`engine.rs:1633-1638`); a modal change filters `active` (`engine.rs:1898`)
- **Hazards:** `active` ends only by a release here, a capture release, a widget drag or a modal change; `Core::start_drag` (`engine.rs:2376-2386`, from `Cx::start_drag` `cx.rs:582-585`) leaves it set although the compositor takes the release; `press` keeps a stale `active` (`engine.rs:2254`), so the next click's release goes to the stale node
- **Reify:** High — six fields, nine handlers, a suspected bug
- **Suspected bug:** Hako calls `Cx::start_drag` while the button that began the drag is down (`desktop/aizome/apps/hako/src/ui/actions.rs:220`); the drop sends no release to the window, so `active` keeps naming the drag source: moves after re-entry go to it, and the next click's PointerUp reaches it instead of the clicked control, losing that click (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#kit-13)

<a id="kit-14"></a>

### KIT-14 — Keyboard focus across modal layers, with pending focus and IME
- **Where:** `desktop/aizome/crates/aizome-kit/src/engine.rs:143` (`Win.focused`), `engine.rs:145` (`.modal_focus`), `engine.rs:146` (`.focus_modal`), `engine.rs:147` (`.focus_visible`), `engine.rs:148` (`.has_keyboard`), `engine.rs:161` (`.ime`), `engine.rs:298` (`Core.pending_focus`)
- **Encoding:** the focused node and whether it came from the keyboard, the modal layer it belongs to, saved focus per covered layer, focus requests by name waiting for a build, and a mirror of the platform's text-input state
- **States:** `Unfocused` → `Focused(node, visible)` in the top modal layer; `Suspended` (saved under the covered layer's id) → `Restored`; `Pending(name, window?)` → `Resolved`; IME `Off` ⇄ `On(caret)`
- **Transitions:** `set_focus` (`engine.rs:1848-1884`, IME off `:1869-1874`); Tab (`engine.rs:1905-1923`); click focus in `press` (`engine.rs:2212-2248`); `restore_modal_focus` on every build (`engine.rs:1888-1902`); focus lost with its node (`engine.rs:772-779`); `window_focus` (`engine.rs:2303-2317`); IME requests (`engine.rs:1685-1690`); `Cx::focus` (`cx.rs:346-363`) and panel focus (`dialogs.rs:455`) resolved in `install` (`engine.rs:715-720`); hide/show refocus (KIT-11)
- **Hazards:** pending names never expire: an entry whose element never appears waits until some later build grows an element of that name, which then takes the focus (`engine.rs:716-719`); `focus_name` counts the name of a killed node as found without focusing it (`engine.rs:1927-1929`; names aren't pruned, `tree.rs:84`); sheet layers are keyed by index (`engine.rs:697`), so a sheet replaced at the same index in one rebuild keeps `focus_modal` equal and skips save/restore
- **Reify:** High — seven fields, ten mutation sites

<a id="kit-17"></a>

### KIT-17 — Tooltip delay, show and dismissal
- **Where:** `desktop/aizome/crates/aizome-kit/src/engine.rs:157` (`Win.tooltip`: pending node and timer token), `engine.rs:158` (`.tooltip_shown`), `engine.rs:160` (`.tooltip_target`); `engine.rs:85` (`TimerUse::Tooltip`)
- **Encoding:** pending timer, shown popup, and the nearest tooltip source under the pointer (kept after a click so it isn't re-armed)
- **States:** `None` → `Pending(node, 650 ms)` → `Shown(popup)` → `ClickDismissed` (target kept) → `None` when the source changes or the pointer leaves
- **Transitions:** `update_hot` arms, keeps, cancels or refreshes (`engine.rs:1991-2018`); the timer → `show_tooltip` (`engine.rs:2744-2748`, `:2760-2772`); `pointer_leave` (`engine.rs:2073-2078`); a press closes the shown one (`engine.rs:2103-2105`); hide (`engine.rs:478-479`); `close_win` clears `tooltip_shown` (`engine.rs:609-611`); `retheme` (`engine.rs:2879`)
- **Hazards:** `pointer_leave` cancels the platform timer but never removes its `TimerUse::Tooltip` (`engine.rs:2073-2075`; contrast `:2001-2004`); a press closes only a shown tooltip, and a pending one keeps its deadline because `update_hot` returns early (`engine.rs:1960-1962`); hiding doesn't cancel the pending one (KIT-11)
- **Reify:** High — three fields, seven handlers, two suspected bugs
- **Suspected bug:** hover a button and click it within 650 ms: its tooltip still appears after the click, though a click dismisses tooltips (`tests/tooltip_refresh.rs:137-142` covers only a shown one) (confidence: medium); every pointer leave with a tooltip pending leaks one `Core.timers` entry (confidence: high)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#kit-17)

<a id="kit-21"></a>

### KIT-21 — Shell connection: link, watcher and menu placement
- **Where:** `desktop/aizome/crates/aizome-kit/src/shell.rs:28` (`ShellLink.alive` `:32`), `shell.rs:108` (`ShellWatch.found`, `.stop`); `desktop/aizome/crates/aizome-kit/src/app.rs:53` (`Runtime.shell`, `.shell_place`, `.shell_watch`); `desktop/aizome/crates/aizome-kit/src/engine.rs:294` (`Core.in_window_menus`)
- **Encoding:** an optional link whose reader thread clears `alive` on hangup, an optional watcher thread that polls the socket and parks a found link in a mutex slot, and `in_window_menus` putting the bars in the windows
- **States:** `OtherDesktop` (no socket path) | `IsShell` (nothing published) | `Watching` (in-window menus) → `Connected` → `Dead` → `Reconnected` | `Watching`
- **Transitions:** environment default (`engine.rs:327`); `launched` picks a mode (`app.rs:763-771`); `find_shell` (`app.rs:87-94`); `try_connect` spawns the reader, `alive=false` on hangup (`shell.rs:54-72`); the watcher loop (`shell.rs:117-142`), stopped on drop (`shell.rs:150-154`); `check_shell` on every wake: dead → reconnect, or watch and bars back (`app.rs:98-112`); found → link, bars out, publish (`app.rs:114-128`); `menus_go_in_windows` (`app.rs:132-139`); harness constructors (`testing.rs:475`, `:488`, `:502`)
- **Hazards:** `in_window_menus` is written in seven places and must agree with `shell.is_some()`; connect and reconnect handshake on the UI thread; window sizes include the bar only as computed at open (`engine.rs:376-384`), so a shell coming or going doesn't resize windows or their limits; the watcher sees `stop` only once per period and may connect after being dropped (the link is then dropped unused)
- **Reify:** High — two threads, seven writers of one flag, user-visible menus
- **Related:** aizome-shell menu server `desktop/aizome/crates/aizome-shell/src/shell.rs:820`; protocol client `desktop/aizome/crates/aizome-shell-proto/src/lib.rs` (`Client::hello`)

<a id="kit-29"></a>

### KIT-29 — Node arena: liveness, layout cache and per-layout outboxes
- **Where:** `desktop/aizome/crates/aizome-kit/src/tree.rs:60` (`Node.dead`), `tree.rs:62` (`Node.cache`), `tree.rs:81` (`Tree.by_id`, `.names`, `.popovers`, `.ticks`, `.widget_timers`, `.local_damage`, `.scrolls`)
- **Encoding:** tombstoned arena nodes addressed by index, the `by_id` index map, cached constraints, and index-based outboxes filled during layout and drained by the engine
- **States:** node `Mounted` → `LaidOut(cached)` → `Uncached` → `Dead` → `Compacted away`; outboxes `Empty` → `Filled (layout)` → `Drained (settle, build)`; invalid but reachable: an outbox index into a compacted arena
- **Transitions:** `mount_under` (`tree.rs:145-186`); the layout cache (`tree.rs:249-268`); `uncache`/`uncache_up` (`tree.rs:189-206`, `widget.rs:505-514`); `add_child` (`widget.rs:496-501`); `kill` through `clear_children` (`tree.rs:237-246`, `widget.rs:540-545`); outbox pushes (`widget.rs:530-532`, `:570-591`); `compact` when dead > 256 and > half (`engine.rs:908-911`, `tree.rs:209-234`); drains (`engine.rs:2891-2909`, `tree.rs:285-311`, `:683-693`)
- **Hazards:** `compact` remaps parents, children, `by_id`, `root` and `popovers` but not `scrolls[].node` or `local_damage[].0`, and `Core::relayout` calls it between `layout_window` (which fills them) and `settle` (which reads them) (`engine.rs:907-912`); `names` keeps killed nodes' names (KIT-14)
- **Reify:** High — index validity is implicit, and a suspected bug
- **Suspected bug:** in a relayout that compacts, a widget mounted during layout (an editor, scroll view or list inside a virtual list's row, after earlier rows died) records `invalidate_rect` or `scrolled` under its old index: its damage or pixel move lands on another node, and `take_local_damage` indexes `self.nodes[*i]` past the end once the old index is at least the live count (`tree.rs:686`), which panics (confidence: medium)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#kit-29)
