# Review of PR 279: one display for a tiled monitor

These are review notes on PR 279 at `ca7033ea`, for whoever addresses
them. Remove this file before the PR is merged.

The review was a reading of the code. Nothing was built or run, and no VM
can present a tiled connector, so none of this has been tested on hardware.
Findings 1 to 4 were traced through the code at `ca7033ea`. The others come
from reading and have not been reproduced.

Unless noted, line numbers refer to
`desktop/aizome/crates/aizome-comp-core/src/backend/udev.rs` at `ca7033ea`.
In the examples, DP-1 is the Studio Display's first tile and DP-2 is the
hidden half.

## Bugs

### 1. Turning the monitor off turns half of it back on

`device_changed`, line 860. `covered_by` decides whether DP-2 is covered
using the mode DP-1's surface is currently showing. Turning an output off
removes its surface but keeps its connector mapping, so a DP-1 that is off
reports no mode. DP-2 then counts as uncovered, has no connector mapping of
its own, and `connector_connected_as` sets it up as a new 2560x2880 display
with a Desk.

Every way of turning DP-1 off does this: Settings, `aizomectl output DP-1
--off`, a kanshi profile, or `enabled = false` in the settings file. To get
the panel dark, the user also has to find DP-2 and turn it off.

`docs/compositor.md` (line 300) describes this as intended: "A disabled
first connector ... leaves the sibling available." To the user, though,
"off" lights half the panel. A first tile that the user turned off should
still cover its siblings, using the mode it would come back with, and DP-2
should stay hidden.

### 2. Every display change re-detects every connector

`refresh_tiles`, line 921, and the probe at line 833. After any
configuration change that sets `enabled` or `mode`, `refresh_tiles` reruns
the whole hotplug handler. That handler calls `get_connector(h, true)` for
every connector on the device, forcing a fresh detection and EDID read, and
`refresh_tiles` doesn't check whether the device has a tiled connector at
all. The caller is `State::configure_output` in `outputs.rs` (line 359).

So every Settings change, wdisplays or kanshi apply, and settings reload
blocks the compositor's event loop on a full re-probe, including on
machines with no tiled monitor. If one of those reads glitches (a DDC or
AUX failure that reads as disconnected), the `gone` path removes a monitor
the user never touched because they changed eDP-1's mode.

The tile check should reconcile from the connector state the hotplug
handler already read (`force_probe = false`, or cached `connector::Info`)
rather than detecting connectors again.

### 3. A hidden half that was turned off turns on, then vanishes

Line 876. Suppose DP-1 is off, so DP-2 is lit. The user turns DP-2 off,
then turns DP-1 back on at 5K. DP-2 is now covered but has no surface, so
it is kept as "disabled intent": it stays in `u.disabled` and
`disabled_outputs` and keeps its connector mapping, and so its CRTC.

If the user then turns DP-2 on in Settings, `configure_output` calls
`enable_output`, which sets DP-2 up at 2560x2880 next to DP-1's 5K picture,
and the call returns success. Then `refresh_tiles` runs, finds DP-2
covered with a surface, and calls `connector_disconnected`. That removes
the output, and its entry in the off list too. The panel flickers, Desks
and windows move, and a call that reported success leaves DP-2 gone.

Meanwhile, DP-2's reserved CRTC can leave a third monitor with "no free
CRTC". A covered sibling should not be offered as an output that can be
turned on, and it shouldn't hold a CRTC.

### 4. A failed turn-on leaves the output stuck

`enable_output`, line 1079. Turning an output off (line 656) removes its
surface but keeps its `device.connectors` entry. `enable_output` removes
the output from `u.disabled` and `disabled_outputs`, then calls
`connector_connected_as`. If setup fails, that function returns before
line 1188, so no surface is added and the old connector mapping stays.

The connector is then in none of the states the code handles.
`device_changed` skips it because `device.connectors` contains it, and
`aizomectl output DP-1 --on` reports "no output DP-1". The output stays
dark and its CRTC is leaked until the cable is replugged. For example,
re-enabling DP-1 at 5K can fail while DP-2 still holds the link or pipe.

This contradicts the new comment at line 897, "Only successful
initialization ... reserves the CRTC." On failure, `enable_output` should
restore the off-list entries, or drop the connector mapping so the next
hotplug pass retries it.

### 5. An unplug can flash a temporary display

Line 857. When the Thunderbolt cable is pulled, the probe can read DP-1 as
disconnected while DP-2 still reads as connected. The `gone` loop removes
DP-1, and in the same pass DP-2 has no first tile covering it, so it is set
up as a new output with a Desk, possibly taking DP-1's Desk. The next
uevent removes it.

Before this PR an unplug only removed DP-1. Now every out-of-order unplug
or DP-1 link flap causes a temporary display and Desk churn. A sibling
whose first tile was just removed could wait for the next pass before
being set up.

## Problems on main that the PR relies on or exposes

### 6. Only the first free CRTC is tried

Line 895. The fix depends on DP-1's 5K mode coming up, but
`device_changed` only tries the first free CRTC. If setup fails, it
doesn't try another possible CRTC or fall back to a mode that CRTC can
drive. On hardware where only some pipes can drive DSC (Gen11 i915 refuses
DSC on transcoder A for external DP), DP-1 can get the one CRTC that can't
drive the whole-panel mode. It fails on every pass, so DP-2 lights and the
user gets half a monitor as a separate display.

### 7. Switching back from another VT doesn't recheck connectors

Line 236. Now that failed setup no longer reserves a CRTC, a monitor
plugged in while the session was paused could be lit on resume. But
`ActivateSession` never reruns `device_changed`. While paused,
`initialize_output` fails with `DeviceInactive`. If the user plugs in the
Studio Display on another VT and switches back, it stays dark until an
unrelated hotplug or configuration change, which in practice means
replugging it.

### 8. The hidden half is never turned off in KMS

Line 244. The design assumes DP-2 stays dark, but Aizome never explicitly
disables DP-2's connector in KMS. On resume, `activate(false)` skips
`reset_state`, so a CRTC another session lit on DP-2 keeps scanning out. If
a greeter or another user's compositor on a different VT drove both
connectors, switching back can leave the other session's picture on DP-2,
or knock the panel out of its single-stream mode. The leftover CRTC can
also make DP-1's next commit fail on bandwidth or pipe limits.

### 9. Removing a GPU leaves turned-off outputs listed

Line 929. The PR made `connector_disconnected` clean up outputs that were
turned off, but `device_removed` only passes it the CRTCs of surfaces.
Turned-off outputs on a removed eGPU or Thunderbolt dock GPU stay in
`u.disabled` and `disabled_outputs`, including a covered sibling that was
turned off. They stay listed as off displays in Settings and IPC. Turning
one on fails with "its device is gone" after `enable_output` has already
removed its `u.disabled` entry.

## Structure and tests

### 10. Tests cover only the pure helpers

Line 1382. Only `Tile::parse` and `covered_by` are tested. Nothing tests
the reconcile behaviour the second commit reworked: the sort order,
disconnecting before reassigning a CRTC, keeping disabled intent, reserving
a CRTC only on success, and coalescing in `refresh_tiles`. All of the
behaviour that commit fixed can regress without a failing test, and no VM
can present a tiled connector to catch it. Pulling the reconcile out as a
pure planner over (connector, state, tile, active mode, reserved CRTC)
would make these cases unit-testable.

### 11. The refresh duplicates existing batching

Line 908. The `tiles_pending` flag and `insert_idle` repeat batching the
compositor already does: `outputs_changed` sets `outputs_dirty`, and
`on_loop_iteration` handles it once per iteration. The flag adds per-device
state whose reset depends on `device_changed`, and `configure_output` is
the only caller. Any future path that changes a mode or turns an output on
without going through it will skip the tile check. A tile-only reconcile
run where `outputs_dirty` is already handled, fed cached connector state,
would cover every path without the extra flag.

### 12. Connector state is spread over three maps and plain tuples

Line 1016. A connector's state is split across `device.connectors`,
`device.surfaces` and `u.disabled`, and the new code passes positional
values: `Tiling<K> = (K, Option<Tile>, Option<(i32, i32)>)`, `Tile` with
six adjacent `u32` fields, and `whole() -> (u16, u16)`. The doc comment on
`Device.connectors` ("Connected connectors and the CRTCs they drive") is
stale now that covered and failed connectors are left out and disabled
ones are kept.

The stuck state in finding 4 exists only because "reserved, but with no
surface and no off-list entry" can be represented. A per-connector state
such as Lit, Off and Covered, plus named column, row and pixel-size types,
would let the compiler rule out that state and swapped width and height or
column and row values.

### 13. Reading the tile property is a property scan per connector

Line 835. `tile_of` now runs on every connected connector in every
`device_changed` pass, and so on every configuration change. Through
`connector_blob`, each call does a `get_property` ioctl for every property
to find one by name, and `connector_connected_as` repeats the scan for the
EDID. That is roughly 10 to 20 ioctls per connector per pass on the event
loop. One `get_properties` pass that finds the EDID and TILE blobs
together would do the same work.

### 14. The manager match is copied again

Line 886. The diff adds another copy of
`match &device.manager { Manager::Gles(m) => m.device(), Manager::Pixman(m) => m.device() }`,
which already appears in `device_changed`, `crtc_of`, `set_temperature`,
`enable_output` and `connector_connected_as`. An
`impl Manager { fn device(&self) -> &DrmDevice }` would replace every copy.

## Conventions

Formatting passes `desktop/aizome/rustfmt.toml`, and the PR keeps
component tests separate from hardware validation, as `AGENTS.md` asks.
