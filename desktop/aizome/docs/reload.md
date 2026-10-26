# Live reload

Aizome can be worked on while it runs, the way a game engine lets you
edit a level that's playing: the compositor's window management, the
shell and the apps are rebuilt and swapped in without closing a window or
signing out. The same machinery is how you recover from a bug in any of
them.

| What changed | What happens | Your windows |
|---|---|---|
| The compositor's logic (`crates/aizome-comp-logic`: window management, tab groups, Desks, bindings, focus, decorations, overlays, motion, the frame's scene) | A new build is loaded between two frames; its state (every window's place, group, Desk, focus, what was minimised or filled) carries over | Stay connected, nothing redraws but what changed |
| The shell (`crates/aizome-shell`) | A new shell starts beside the old one and takes over its bar, Deskbar and desktop in the same frame; notifications, their history and the Deskbar's state carry over | Untouched; kit apps find the new shell within a few milliseconds |
| An app | Closed politely (as its close box would) and started again from the new build | That app's windows close and reopen |
| Settings, themes, key bindings | Read again (they apply live anyway) | Untouched |
| The compositor's core (`crates/aizome-comp-core`), `aizome-ipc`, `aizome-config`, `aizome-theme`, Sidestep, the lock file, the toolchain or the build flags | Can't be reloaded: a logic built against it is refused and the change runs at the next login | — |

## Using it

- **`aizomectl reload`** reloads whatever changed since the session
  started (or since the last reload): the installed logic, the installed
  shell, the settings. It says, part by part, what it did and what needs
  a new session. `--logic` and `--shell` reload those even if they didn't
  change (a fresh start from saved state, which is also how to get out of
  a stuck state); `--from PATH` takes a logic library, a shell binary, or
  a build directory holding either; `--check` only says what would
  happen.
- **⌃❖R** (and **Reload Aizome** in the command palette, and in the
  Ctrl+Alt+Del recovery overlay) does the same as a plain `aizomectl
  reload` and says what happened in a notification. ⌃❖R belongs to the
  core, so it works even when the logic has been set aside; another chord
  can be bound to `reload` in `[keys.bindings]`.
- **After `nixos-rebuild switch`** a notification ("Aizome was updated")
  says what a reload would pick up from the new system, and its *Reload
  Now* button does it (`aizome-update.path` watches `/run/current-system`
  and runs `aizomectl reload --offer`). Nothing reloads on its own. When
  the new package's compositor core differs from the running one, the
  notification says it runs at the next login; the new logic and shell
  can't load before then either, since they're built against the new
  core.
- **`aizomectl stats`** has a `logic:` line: the build running, where it
  came from, how many reloads and rollbacks, and the last error.

## Hacking on Aizome live

From any checkout or worktree, replace one app in your current session:

```sh
just replace yukimi
```

The recipe passes the directory containing its `justfile` to `aizome-dev`,
so it also works from a subdirectory and does not use another checkout
named by `AIZOME_CHECKOUT`. Recipes automatically enter `nix develop .#aizome`
unless `IN_NIX_SHELL` already identifies a Nix shell. The shared recipe
shell in `scripts/just-shell` handles this for all ordinary recipes.
`just` must itself be on your PATH; if it is not installed yet, bootstrap
with `nix develop .#aizome -c just replace yukimi`.
Use any app name (`hako`, `sumi`, …). It builds before asking the old app
to close, respects save prompts, and starts the result even if the app
was not already running. It changes this running instance; subsequent
desktop launches use the installed version. Run it again after more edits.

`aizome-dev` (in the package) builds a checkout the way the running
session was built and hands the result to the session:

```sh
aizome-dev watch                      # ~/work/sekka/desktop/aizome, or --checkout PATH / $AIZOME_CHECKOUT
aizome-dev reload logic               # build once and reload
aizome-dev reload shell
aizome-dev reload hako                # an app: closed, rebuilt, started again
aizome-dev env                        # how the session was built, and the cargo command used
```

`watch` builds the logic and the shell once, then on every save:

- a file of the logic → rebuild it (about 6 s for a one-line change
  here) and swap it in;
- a file of the shell → rebuild it and restart the shell beside the old
  one;
- a file of an app → rebuild it and, if it's running, close it and start
  the new build;
- a shared crate (the kit, Sidestep, `aizome-sys`) → rebuild the logic,
  the shell and the apps running from the checkout; a change that reaches
  the compositor's core makes the logic's reload refused, with which part
  of the core differs, and it runs at the next login.

The logic builds at `opt-level = 1` by default, which compiles several
times faster; `--optimised` builds it as the session's own.

### Why a build from your checkout loads into the packaged session

The logic is a library linked against the compositor's core, a Rust
`dylib`: it calls the core's functions by their mangled names and shares
its types. A library loads only if it was built against the very build of
the core that's running, which for Rust means the same:

- **sources** of the core and everything it's built from (path packages
  and the registry crates in `Cargo.lock`);
- **toolchain** (`rustc -vV`; Cargo's `-C metadata` hashes include it);
- **packages built together**: Cargo resolves features per invocation,
  and the features of the core's dependencies go into every crate's
  `-C metadata`, so `cargo build -p aizome-comp-logic` alone gives the
  core other symbols than `-p aizome-comp -p aizome-comp-logic`;
- **target**: `--target x86_64-unknown-linux-gnu` (as the Nix build has
  it) gives other symbols than no `--target`, even on that host;
- **profile settings** (opt-level, debug info, codegen units, LTO, panic
  strategy) and the codegen flags (`-Cforce-frame-pointers=yes` from
  nixpkgs' Cargo setup);
- **no `RUSTC_WORKSPACE_WRAPPER`**: Cargo hashes its path into every
  workspace crate's `-C metadata`. nixpkgs runs Cargo through
  cargo-auditable, which is one, so the package builds the compositor
  with plain Cargo (the apps keep cargo-auditable), and aizome-dev
  clears the variable.

The directory the build runs in doesn't matter: Cargo hashes a path
dependency by its path relative to the workspace, which is why the
workspace reaches Sidestep through a `sidestep` link inside it
(`sidestep -> ../sidestep-aizome`), and registry crates by their source,
not where they're unpacked. Neither do the linker and its arguments.

The package records its side of this in
`share/aizome/build-env.json` (the toolchain's store paths, split in two
so the package doesn't keep the compiler alive, the profile, target and
packages built together) and the core reports its own build over IPC
(`aizome --build-info`; the `version` request's `build`). `aizome-dev`
builds with exactly that: the recorded `rustc` and `cargo` (realised from
the binary cache if they aren't in the store), the core's flags, the same
`-p` set, profile and target, in `target/aizome-dev` beside the
checkout's own builds (other flags there would rebuild everything each
time). Only the logic's own profile is changed, through `--config
profile.release.package.aizome-comp-logic…`, which leaves every other
crate as it was.

Verified, with the package built by Nix (in `/build/aizome-source`)
and the same commit unpacked in another directory and built there by the
package's own `aizome-dev`: the two `libaizome_comp_core.so` export the
same 25,113 symbols (the crate's mangling hash, e.g.
`_RNvCsaqWaqXcvhOR_16aizome_comp_core3run`, is the same), the core's
fingerprint is the same, and the checkout's logic loaded into the
packaged compositor, running headless with foot open, in 16 ms. A
one-line change to the logic then rebuilt in 6 s and reloaded with foot
still there and its tab drawn in the change's colour. The first attempt
was refused, rightly: the Sidestep checkout beside it had moved on since
the package's lock, and the fingerprint's `sources` part said so (now
`aizome-dev` names the packages, and warns before building).


The ABI fingerprint (below) catches a mismatch in sources, lock file,
toolchain, profile, flags and target; a mismatch it can't see (other
features resolved) shows as undefined symbols when the library is
opened with `RTLD_NOW`, and the reload is refused with that said. Either
way nothing half-loads.

### Sidestep

The checkout's `../sidestep-aizome` must be at the revision the
package was built from (the flake's `sidestep` input): Sidestep's crates
are part of the core. The package records it (`sidestep_rev` in
`build-env.json`); `aizome-dev` says before building when the checkout's
Sidestep is elsewhere or has changes, with the command that fixes it,
and a refused reload names the packages whose sources differ.

## How it's built

### Core and logic

The compositor is two Rust dylibs and a small binary:

- **`aizome-comp-core`** owns everything a client's connection depends
  on: the Wayland display and clients, smithay's protocol states, the
  seat, outputs, backends and renderer, the event loop, IPC, the session's
  helpers (the shell, xwayland-satellite), screencopy and the other
  protocols, the recovery overlay and the watchdog, and the drawing pieces
  they share (text labels, icons, the painter). It re-exports smithay and
  the rest, so there's one copy of each and of their statics.
- **`aizome-comp-logic`** is what people hack on: the window-management
  model, placement, tab groups and Desks, bindings and the command key,
  focus policy, grabs, decorations, the overlays (switcher, overview,
  marking menu, area picker), motion, and each frame's scene.
- **`aizome`** is `fn main() { aizome_comp_core::run() }`; it finds the
  logic in `../lib/aizome` (the package) or beside itself (a build), or
  takes `--logic PATH` / `$AIZOME_LOGIC`.

The core calls the logic through a table of function pointers
(`LogicVTable`) that the library's exported `aizome_logic_entry` returns:
`init`/`save`/`quiesce`/`release`, `publish`, window lifecycle
(`first_configure`, `window_mapped`, `window_committed`,
`window_unmapped`, `toplevel_request`), outputs (`usable_area_changed`,
`outputs_changed`), input after the core's own keys (`decide_key`, `key`,
`key_action`, `pointer_motion`, `pointer_button`, `pointer_axis`,
`touch_down`), the frame (`scene`, `window_bounds`), IPC requests the core
doesn't answer, timers, settings changes and a per-iteration hook.

The core never reads the logic's types. The logic publishes what the core
needs of the desktop (`View`: each window's rectangle, whether it shows,
its Desk, parent and state; focus, stacking and focus history; whether an
overlay has the keyboard; whether something is animating), and each frame
it returns a scene of renderer-free items (`SceneItem`: a surface tree, a
solid rectangle, a texture from memory, a scaled thumbnail) that the core
turns into render elements. Timers are the core's (`State::schedule`,
dropped when the logic changes); pointer grabs the logic starts go through
`State::set_logic_pointer_grab`, which wraps them so a panic in one is
caught and the grab ended at a reload.

### A reload

1. The new library is checked: its ABI mark (`AIZOME-LOGIC-ABI:` and the
   core's fingerprint, embedded in the file) must match the running
   core's. Then it's copied to `$XDG_CACHE_HOME/aizome/logic/<pid>/`
   (a file the build can't overwrite under it) and opened with
   `RTLD_NOW | RTLD_LOCAL`; its table's ABI and the `TypeId` of the
   core's `State` are checked again.
2. The running logic is quiesced (overlays closed, grabs ended, its
   timers cancelled) and saves its state as JSON (`Saved`: the model, the
   sizes asked of clients, and notification cards in mid-slide, which the
   new logic carries on moving).
3. The new logic starts from that state (every field `#[serde(default)]`,
   so a build with new fields reads an old state), adopts the windows,
   publishes its view, and the outputs redraw.
4. If anything fails, the old logic starts again from the same state.
   Libraries are never closed: code or data from them may still be
   referenced, and the few hundred kilobytes of each are what a reload
   costs in memory.

A reload takes 2 to 16 ms here (once, straight after a build, a
second); the clients see one frame with the new logic's drawing.

The fingerprint is computed by the core's `build.rs`: a hash of the
library sources of every path package the core is built from (not their
programs: editing aizomectl changes nothing the core links), the registry
crates in `Cargo.lock` it uses, `rustc -vV`, the target, profile,
opt-level, debug settings, panic strategy, target features, codegen
flags and explicit `--target`, and the core's features. `aizome
--build-info` prints it with its parts, and a refused reload says which
part differs.

### When the logic panics

Every call into the logic runs under `catch_unwind`. A panic marks the
logic failed; at the end of that loop iteration the core has it save its
state (or, if even that fails, takes the state saved at the last reload)
and rolls back to the logic that ran before it, or, without one, starts
the same build again from that state (from nothing, if the state won't
load); after three panics in a minute it sets the logic
aside and the recovery path runs the desktop (windows stay where they
were and can be focused, moved and closed; the overlays and Desks wait).
Pointer grabs end, the user gets a notification with the panic's message,
and `aizomectl stats` and the `logic-failed` event say what happened. A
reload brings a logic back.

### The shell

`aizomectl reload --shell` starts a new shell beside the running one
(with `AIZOME_SHELL_PREVIOUS_PID`). Each layer surface the new shell
draws replaces the old shell's surface of the same namespace on the same
output in the same frame, so the bar, the Deskbar and the desktop never
leave the screen, and windows aren't refitted meanwhile; once the old
shell has no surfaces left (or after eight seconds) it's stopped. The
new shell takes over `org.freedesktop.Notifications` (both hold it
allowing replacement), and the shell socket that kit apps talk to; kit
apps reconnect at once and publish their menus again.

The shell keeps what it carries across in
`$XDG_RUNTIME_DIR/aizome-shell-state.$WAYLAND_DISPLAY.json`, written on
every change: the notifications and their history, what the Deskbar
folds and which apps show all their notifications. The old shell writes
its last word there as it loses the bus name; the new one reads the file
as it starts and takes in what came to the old one meanwhile.
Notification ids go on from the old shell's (well past them, so the two
never give the same id).

A shell that crashes is started again the same way (without the overlap),
and the recovery overlay's Restart Shell kills a hung one first.

## Rules for logic code

The logic is unloaded only in the sense that it stops being called: its
code stays mapped, and a new build's code runs beside the old. So:

- **State lives in `Logic`** (through `state.lg.m()`), and what must
  survive a reload goes in `Saved`, with `#[serde(default)]` on every
  field and a sensible default. Never put the logic's types in smithay's
  `UserDataMap`s or anywhere else the core keeps: a rebuilt library's
  types keep their `TypeId` but may change their layout, so a downcast
  would succeed and read garbage.
- **No statics that must survive** a reload: each build has its own, and
  starts them afresh.
- **No closures of your own on the event loop or in the core**
  (calloop sources, callbacks handed to smithay): they'd keep calling the
  old build. Ask the core for timers (`State::schedule`, with a token
  your `timer` hook dispatches) and start pointer grabs with
  `State::set_logic_pointer_grab`.
- **No threads** that run logic code after a hook returns.
- **Don't hold** borrows of `Logic` across calls back into the core that
  might call the logic again: take what you need, let go, then call.
- **Panics are caught**, but a panic leaves your state as it was midway,
  and that's what the next logic starts from: keep `save` and `init`
  robust to a half-done change (a window the model doesn't know is
  adopted again; one it knows that's gone is dropped).
- Changing the core (its types, the vtable, `View`, `SceneItem`) is a
  core change: the session must be restarted to use it.

## Build profiles and LTO

Rust can't run LTO across a `dylib` boundary, and a crate that links a
dylib can't use LTO either, so the `release` profile has none (`lto =
false`), and the Nix package builds the compositor (core, logic and the
`aizome` binary) with `release` and everything else with `release-lto`
(thin LTO, as before). Frame times before and after, from
`examples/frame_bench` on a 2880×1800 output at scale 2 (CPU per frame,
median of three runs, p50 / p99 in ms):

| Scenario | Before, thin LTO | Before, no LTO | Core + logic |
|---|---|---|---|
| GLES: move a window | 0.34 / 1.10 | 0.38 / 1.04 | 0.42 / 0.93 |
| GLES: animating client | 0.28 / 0.74 | 0.29 / 0.87 | 0.33 / 1.62 |
| GLES: Desk switches | 0.29 / 0.93 | 0.33 / 0.74 | 0.36 / 1.21 |
| GLES: 10 windows, one moving | 0.64 / 1.03 | 0.72 / 1.33 | 0.75 / 1.88 |
| GLES: full-screen client | 0.30 / 0.98 | 0.35 / 0.69 | 0.36 / 2.09 |
| GLES: overview open/close | 0.87 / 5.50 | 1.04 / 4.79 | 0.71 / 5.29 |
| pixman: move a window | 0.41 / 0.91 | 0.39 / 0.86 | 0.38 / 0.99 |
| pixman: animating client | 0.79 / 2.03 | 0.77 / 1.74 | 0.80 / 2.65 |
| pixman: Desk switches | 0.49 / 1.18 | 0.48 / 1.11 | 0.48 / 0.83 |
| pixman: 10 windows, one moving | 0.64 / 0.95 | 0.71 / 1.24 | 0.71 / 1.48 |
| pixman: full-screen client | 1.97 / 3.03 | 2.14 / 8.11 | 2.15 / 6.33 |
| pixman: overview open/close | 5.15 / 20.9 | 7.86 / 26.7 | 6.45 / 27.4 |


"Before" is main as this branch last merged it, "core + logic" this
branch on top of it; all three were built against the same Sidestep and
run interleaved; the machine was shared with other builds (load 8 to
23 on 8 cores), so the p99 column is mostly other processes, and only
differences in p50 of more than a few percent mean anything. Under GLES
the split costs 0.04 to 0.08 ms a frame at the median: about half is
LTO's absence (no LTO at all, same code: 4 to 16 % slower), the rest the
boundary itself (the scene built as a list of items and then turned into
render elements, calls between the two libraries that can't be inlined,
every hook through `catch_unwind`). Under pixman, where the time goes to
pixman itself, the three are within noise. Every steady scenario stays
well inside the 4 ms budget (the slowest median, a full-screen client
under pixman, is 2.15 ms). Thin LTO for the compositor would need it to
be one binary again; the logic's reloadability is worth the 0.05 ms.


The core needs `-C prefer-dynamic` (it links std dynamically so the
binary and every logic share one std); `scripts/rustc-wrapper` adds it
for that crate alone, and `nix develop .#aizome` and the package set it as
`RUSTC_WRAPPER`. The package installs `libaizome_comp_core.so`,
`libaizome_comp_logic.so` and the toolchain's `libstd-*.so` in
`lib/aizome`, found through `$ORIGIN` rpaths.

## Tests

- `crates/aizome-comp/tests/reload.rs` builds the compositor and two
  logic libraries from the same checkout (as it is, and with
  `AIZOME_LOGIC_VARIANT=red`, which draws the focus colour pure red and
  takes a `test-panic` action), starts a headless session with test
  clients, foot and Electron, reloads into the variant and checks that
  every client is connected, every window kept its place, group, Desk
  and focus, and that the screen changed; then makes the variant panic
  and checks the rollback (the desktop as before, the screen as before).
  It also starts a session without any logic (the recovery path) and
  reloads one in, refuses a library whose ABI mark says another core,
  and restarts the shell on a D-Bus of the test's own with a test client,
  foot and Tokei running: notifications from before and during the
  restart are kept, the apps stay, Tokei finds the new shell, and every
  screenshot taken through the overlap shows the bar and the desktop.
- `tests/recovery.rs` checks the recovery overlay's "Reload Aizome".
- `nix/vm-test.nix` runs `aizomectl reload` and `aizomectl reload
  --logic --shell` in a NixOS VM session with foot open: foot's window is
  still there, the compositor is the same process, the shell a new one.
