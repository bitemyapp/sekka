# NVIDIA gaming component work, 28 October 2026

Source: `feat/nvidia-gaming`, based on main `3f4b559`, in the separate
`/home/callen/work/sekka-nvidia-gaming` worktree. The original checkout's
uncommitted work was left alone. The root dependency lock did not change.

## Hardware and scope

The workstation is a desktop with an NVIDIA GeForce RTX 5090, not the
Helios Neo laptop. NVIDIA's open kernel module and proprietary userspace
are 615.78.08, on Linux 7.3.0 / NixOS 26.11. The NVIDIA render node is
`/dev/dri/by-path/pci-0000:01:00.0-render`; an AMD integrated GPU is also
present. The active display is an ASUS PG27UCDM, DP-4, 3840×2160 at 240 Hz,
scale 2.

Read-only DRM queries reported sync objects, timeline sync objects and
atomic asynchronous page flips. The connected output reports VRR
capability and has an HDR metadata property. Those capabilities are not
proof of correct presentation: no physical KMS mode, VRR, HDR or tearing
transition was performed. The current desktop was not replaced or
restarted. No system generation was activated, Steam account modified,
game launch options rewritten, or image published.

## Verified components

- Real NVIDIA DMA-BUF and DRM timeline test: an unsignaled acquire point
  blocks the surface, while compositor IPC stays responsive. Signaling it
  maps the window. Release remains pending while the buffer is used and
  signals after unmapping, even after the protocol object is destroyed.
  Destroying 32 surfaces with abandoned acquire points does not accumulate
  descriptors. The software compositor does not advertise explicit sync.
- Xwayland-satellite 0.8.3 forwards syncobj requests without a dependency
  change. A Vulkan XCB cube completed 240 frames on the RTX 5090 in a
  private headless Aizome session. Its protocol trace contains timeline
  imports, acquire points and release points on both sides of the bridge.
- Configuration, IPC, compositor core and Settings unit suites: 110 tests
  passed. Display preview tests cover VRR persistence and rollback. Eight
  screen integration tests pass, including unsupported/disabled output
  refusals. Eight pacing tests and the fullscreen frame regression pass.
- Gaming Nix policy and six launcher tests pass: Steam's 32-bit graphics
  and controller support, WSI for both architectures, user groups,
  independent opt-outs, optional tearing, preserved package overrides,
  unchanged firewall defaults, argv boundaries, wrapper order, and refusal
  of HDR/tearing under Aizome's unsupported host pipeline.
- The website builds with corrected hardware-coverage prose. The installed
  host configuration evaluates with a Steam session alongside its existing
  Aizome, Tatami, GNOME and Hyprland sessions.
- Final Aizome and patched Gamescope Nix packages build successfully:
  `/nix/store/9kq71lsw4x63s0j3j6zkwm479s8iydvz-aizome-0.1.0` and
  `/nix/store/07k72ci8czfxyyx98xkap1mxpwsrnzka-gamescope-3.16.31`.
- The packaged `nvidia-smi` wrapper reports the RTX 5090, driver 615.78.08
  and 32607 MiB usable VRAM. The installed patched executable instead
  segfaults in ld.so before main: overlapping ELF load segments leave
  `PT_DYNAMIC` read-only when the loader writes `DT_DEBUG`. The wrapper
  uses the original vendor executable with the Nix loader and matching
  driver libraries; it does not modify the ELF or replace the driver.

## Gamescope regression

The pinned Gamescope 3.16.31 rendered the Vulkan workload but crashed on
exit. GDB first caught `gamescope-shdr` in the NVIDIA shader compiler. The
thread-join fix from
[ThomasEricB, 2e9a5a5](https://github.com/ThomasEricB/gamescope-lanczos-downscaling/commit/2e9a5a5fb3f3d7bfaf84c9f2341f5695d059b62b)
was adapted to the pinned source. Further testing caught a second fault
in the main thread's exit handlers: `CVulkanCmdBuffer` called
`FreeCommandBuffers` after its ICD had unloaded. The local patch releases
GPU-owning containers explicitly after compositor threads finish.

`tests/gaming-gpu-smoke.sh` reproduces the Xwayland test and then runs five
nested Gamescope clients, including four early exits, with NVIDIA's shader
disk cache disabled for those processes. It does not delete user caches,
acquire DRM master, or use the physical outputs. Build and run commands
are in [DEVELOPERS.md](../../DEVELOPERS.md#nvidia-and-steam-gaming).

The final packaged binaries passed the full smoke script: the X11 cube
completed and all five Gamescope runs exited successfully. Logs were
retained locally in `/tmp/sekka-gaming-gpu.7h0PXK`. This is render-node
evidence; the private headless host has no real panel or PipeWire capture.

## Steam scheduling follow-up

The requested policy is Steam at nice −5 and its same-user descendants at a
−10 baseline, including unlisted games and Proton/runtime wrappers. Stronger
named and per-thread nice priorities and existing real-time CPU policies are
preserved. Fossilize shader-cache processing and its children remain idle.
Ananicy owns this policy; GameMode does not compete with it. The mechanism
and ancestry limits are documented in [tuning.md](../tuning.md#steam-process-trees).

Both component checks pass:

```sh
nix build .#checks.x86_64-linux.tuning-policy .#checks.x86_64-linux.tuning \
  --no-update-lock-file --no-link -L
```

The policy check compiles and exercises the process-tree classifier against
PID reuse, UID changes, reparenting, exec transitions and real thread-group
identity. The disposable NixOS VM runs the packaged daemon with ordinary-user
Steam/game stand-ins. It verifies −5/−10 priorities on all threads, known and
unknown executables, a generic background runtime, shell wrappers, wineserver
at −12, a game thread retaining nice −15 and SCHED_FIFO, background shader
helper descendants, fork without exec, late threads after launcher exit, and
classification of existing Steam trees after daemon restart. Existing Nix,
shell/compiler, AI workload and Aizome launcher checks also pass.

This tests scheduling mechanics, not an actual Steam or Proton game. The host
daemon and installed system generation remain unchanged.

## Steam window geometry follow-up

The host remembered `steam` as `[469,352,107,100]`. Its compositor log showed
both the sign-in window and the main client mapping at 107×100 logical points.
Steam's main X11 window declared a 2020×1200 physical minimum on this 2×
display. Steam also uses the same app ID for unparented fixed-size helpers;
one observed popup declared a fixed 214×199 physical size. Aizome previously
saved any closing free toplevel's geometry by app ID and restored it without
checking the new window's constraints.

The logic now excludes fixed-size windows from both restoring and saving
app-wide geometry, and rejects remembered dimensions outside a client's
minimum/maximum. Last committed constraints survive unmapping, and a logic
reload reconstructs them from the connected surfaces. This is a general
window-memory fix, with no Steam title or executable special case.

Validation:

- 65 compositor-logic unit tests and all seven headless layout tests pass.
- The added protocol regression runs at 1× and 2×. It seeds stale 107×100
  memory, opens a main window with a larger minimum, saves a deliberate
  resize, opens and closes a fixed-size helper with the same app ID across
  a logic reload, and checks that the main window's size survives. It also
  rejects saved dimensions above a new window's maximum.
- The pre-fix packaged Aizome is a failing control: the new main-window
  fixture stays 107×100 instead of its requested 1100×720.
- For a live fix without logout, the same logic-only patch was applied to
  the exact Nix source of the running compositor, built through the root
  flake's development shell using `aizome-dev` and its recorded toolchain,
  flags and target. All seven layout tests pass against that optimized build.
- The compatible library was loaded into the current session at 19:24 UTC.
  The compositor reports healthy logic, one reload and zero rollbacks. Other
  windows retained their geometry, groups and Desks. The stale Steam entry
  was backed up and removed; Steam's main UI was closed and reopened on its
  original Desk. It mapped at 1685×1034 logical points, was visually checked,
  and focus returned to the original terminal. Screenshot retained locally
  at `/tmp/sekka-steam-window-after.png`, at its original resolution; it is
  not published because it includes the user's Steam library.

The live library is `~/.local/state/aizome/hotfixes/steam-window-2026-10-28.so`;
the geometry backup is `~/.local/state/aizome/windows.before-steam-window-fix-2026-10-28.json`.
This hot reload applies the window fix to the current session only. No system
generation was activated; future logins need the updated packaged logic.

## X11 fullscreen startup and Turok

After the user activated `162eff1` and rebooted, both current and booted
generations were `/nix/store/0mnrp8d2avbhf2g82r3arbfcbzccs50v-nixos-system-runnymede-26.11.20261020.02c087f`.
The running Aizome was `/nix/store/np92v2p3i29yl1l6flpnbbd4hcdrg7y8-aizome-0.1.0`.
The user reported Turok's fullscreen setting sometimes opening inside the
desktop while its windowed, borderless setting covered the monitor.

The pinned xwayland-satellite ignored the initial `_NET_WM_STATE` property,
dropped fullscreen messages received before creating a Wayland toplevel,
and guessed fullscreen for any window matching a monitor's size. The scoped
Aizome patch reads initial state on mapping, queues early requests, clears
cached intent on withdrawal, and gives explicit windowed state precedence
over a fallback limited to undecorated/unmanaged legacy windows. This follows
the [EWMH initial-state requirement](https://specifications.freedesktop.org/wm/latest/ar01s05.html).

Validation:

- The package builds with all 85 upstream unit tests (including the new
  early-request regression) and 29 upstream Xwayland integration tests.
- `checks.x86_64-linux.xwayland-fullscreen` passes 16 real X11 scenarios
  across 1× and 2× displays. It verifies initial fullscreen/windowed hints,
  requests and cancellation before mapping, toggling, withdrawal/remapping,
  clearing old intent, decorated monitor-sized windows, legacy borderless
  and unmanaged windows, and ordinary smaller windows. Both Aizome's geometry
  and X11's reported state are checked. These use private software-rendered
  headless sessions, not the user's display.
- The same integration test fails with the unpatched package: a small window
  with an initial fullscreen property remains windowed. Negative-control log:
  `/tmp/sekka-xwayland-fullscreen-negative.log`; passing check log:
  `/tmp/sekka-xwayland-fullscreen-passed.log`.
- Evaluating this workstation's actual configuration with the worktree input
  selects `/nix/store/gkg8ki04a6p4yjamc4z80fm30srkbfxg-xwayland-satellite-0.8.3`.
  No system generation or lock file was changed by that evaluation.
- On the physical RTX 5090/DP-4 output at 3840×2160, 240 Hz and scale 2,
  Turok's `v_windowed=0` and `v_windowed=1, v_borderless=1` runs both
  covered all 1920×1080 logical points, without the Deskbar. X11 reported
  `_NET_WM_STATE_FULLSCREEN` and Aizome reported `vrr_enabled=true` during
  each run; it returned to false after closing the game. This verifies the
  physical session's fullscreen/VRR policy transition, not the panel's actual
  refresh range or direct scanout.
  The test used a separate patched bridge on `:158`, a copied executable
  and configuration, and the installed game assets. Only those test
  launches used `SDL_VIDEODRIVER=x11` (to exercise the bridge with
  `steam-run`'s SDL) and null audio. No Steam launch option was changed;
  the original game configuration was restored after the initial diagnosis.
  Focus returned to the original terminal. Original 3840×2160 captures of
  the title scene are retained locally as
  `/tmp/sekka-turok-fullscreen/physical-0.png` and `physical-1.png`;
  the run log is `physical-final-results.log` in the same directory.

At this stage the exact Turok inversion was intermittent: initial reruns through the old
bridge also successfully entered fullscreen. This is a deterministic protocol
fix with a failing control, not proof that every cause of the original game
symptom has been eliminated. At 3840×2160, `v_windowed=1` plus
`v_borderless=1` legitimately produces a monitor-sized borderless window.

The new bridge was tested separately from the desktop's existing X11 server;
it was not substituted underneath running Steam or other X11 apps. Install
the branch update and log out/in (or reboot) to use it for ordinary launches.

### Follow-up: native Steam focus loss and the 60 FPS limit

After activating `6539f7b`, the user reproduced the decorated-window failure
again. Testing the actual Steam launch revealed the missing transition:
the 2018 native Turok starts fullscreen, but SDL 2.32.10 auto-minimizes
exclusive fullscreen on focus loss. Satellite logged an unhandled
`WM_CHANGE_STATE`, and the game returned as a decorated 1685×1034 window.
The previous startup-only tests did not exercise this path.

The bridge now translates the ICCCM iconification request into
`xdg_toplevel.set_minimized`, announces `WM_STATE=Iconic` and
`_NET_WM_STATE_HIDDEN`, then announces restoration on keyboard focus.
Updating fullscreen modifies only its own state atom, preserving HIDDEN
while SDL temporarily releases fullscreen. This follows the
[EWMH hidden-state semantics](https://specifications.freedesktop.org/wm/latest/ar01s05.html).

Validation of this follow-up:

- All 85 upstream unit and 29 integration tests still pass. The packaged
  16-scenario X11 check now also exercises three minimize/restore cycles
  at each scale, checking compositor visibility, ICCCM state, EWMH hidden
  state, preservation of hidden state during fullscreen removal, and
  re-entry to fullscreen.
- The prior `6539f7b` bridge fails the new check waiting for minimization:
  `/tmp/sekka-minimize-negative.log`. The passing final check log is
  `/tmp/sekka-minimize-check-passed.log`.
- Real Steam launches used its pressure-vessel runtime and mapped
  `sdl2-classic/libSDL2-2.0.so.0.3200.10`. Only Turok was temporarily routed
  to the patched bridge on display `:158`; the desktop's normal bridge
  remained running. Two exclusive-fullscreen launches, including a restart
  with `v_windowed=0`, passed three focus-away/restore cycles each. Each
  return covered 1920×1080 logical points at 3840×2160/240 Hz, with
  `fullscreen=true` and VRR policy enabled. A separate borderless launch
  (`v_windowed=1`, `v_borderless=1`) stayed fullscreen and unminimized
  across two focus cycles. No global SDL focus-loss override was used.
- The recorded restart cycles are in
  `/tmp/sekka-turok-native-steam-restoration.jsonl`; original 3840×2160
  captures are `/tmp/sekka-turok-native-restored.png` and
  `/tmp/sekka-turok-native-restart-restored.png`.

The native executable is dated July 26, 2018. Its `kexSessionLocal::RunGame`
explicitly clamps `cl_maxfps` above 60 to 60 (compare against `0x3c` at
`0x4b1c22`, followed by `kexCvar::Set(int)`). Setting the config to 240 was
therefore ineffective and was reset by the engine. Xwayland reported
239.98 Hz while Aizome reported 240000 mHz; this was not an output stuck
at 60 Hz. Nightdive's
[KEX 4 announcement](https://store.steampowered.com/news/app/405820/view/516329008965617018)
adds high-refresh support and warns that old saves are incompatible.
The user approved testing the newer Windows release through Proton after
the native fullscreen issue was fixed. Native config/saves were backed up
under `~/.cache/sekka-turok-fullscreen/native-backup`.

Proton Experimental launched the Windows build dated December 10, 2025,
using Vulkan on the RTX 5090. Its engine log reports a 240 Hz fixed rate
(`0.004166666666666667` seconds) with VSync enabled; Steam's counter showed
240 FPS in the title scene and main menu. The original 3840×2160 capture is
`/tmp/sekka-turok-proton-main-menu.png`. This is a startup/menu observation,
not a gameplay benchmark. The user ended testing here to move to another
game. Turok remains selected for Proton Experimental; the temporary
`DISPLAY=:158` launch option and separate test bridge were removed.
The normal desktop still requires the branch update and a fresh login to
load the fullscreen restoration fix.

## Repository-wide evaluation

The broad `nix flake check --no-build --no-update-lock-file` did not pass.
The first attempt hit transient source-path validity errors. A serial
retry evaluated the component checks but found two failures: an absent
cached nixpkgs source for `iso-store-integrity`, and the live-compatibility
Xfce assertion in `xapp-packages`. Fetching the cached source fixed the
targeted `iso-store-integrity.drvPath` evaluation. The Xfce assertion also
fails when evaluated at the untouched base commit `3f4b5593c04b83ac533c7f3d48d7dc0a22839967`.
It is not introduced by this branch. The focused gaming check and package
builds pass; this record does not claim a green repository-wide check.

## Remaining validation and limits

Aizome now implements explicit sync and VRR policy with KMS integration,
plus more useful scanout format feedback. Turok's fullscreen/VRR policy
transition was observed on the physical desktop as described above. Direct
scanout and the panel's variable-refresh range still need measurement.
Native HDR colour management and
`tearing-control-v1` remain absent from Aizome; the configured Gamescope
login session supplies a separate route that also needs physical testing.
Do not infer working HDR from a 10-bit framebuffer or an advertised
connector property.

No Steam/Proton game benchmark, frame-time comparison, suspend/resume,
multi-GPU offload, display hotplug, standalone Steam login or ISO
installation was performed. These are component results, not release
qualification or an FPS improvement claim. Publication remains inhibited.
