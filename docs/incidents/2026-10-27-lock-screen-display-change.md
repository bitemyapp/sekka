# Lock screen black after a display change

Reported 2026-10-27. Component: Aizome's lock screen (`aizome-shell`,
`aizome-comp-core`, `aizome-kit`). Fix:
[#126](https://github.com/bitemyapp/sekka/pull/126).

## Report

While the session was locked, the external display was removed and the
laptop panel was restored. The panel stayed black, with no password prompt.
The session log from the report shows that sequence before recovery
restarted the shell.

## Cause

The shell created lock surfaces only when the lock began, one for each
output it knew then, and put the password field only on the output that had
the focus at that moment. An output that appeared later, including a laptop
panel coming back as a newly advertised output, got no lock surface. While
locked, Aizome draws only the cursor and Ctrl+Alt+Del's recovery overlay on
an output without a lock surface, so the panel was black and there was
nowhere to type. The session itself stayed locked; no desktop content
showed.

Restarting the shell through recovery gets past this because a restarted
shell is asked to lock again, and the new lock covers every output present
then.

## Related defects found while testing the fix

- After the display whose prompt had the keyboard went away, the surviving
  prompt could get no keys: the compositor left the keyboard on the departed
  display's lock surface until something else chose again.
- Lock surfaces kept their old size after an output's mode, scale or
  rotation changed.

## Fix

- The shell reconciles its lock surfaces with the advertised outputs for the
  whole lock, before and after the compositor confirms it. A departed
  output's surface is destroyed, a new or returning output gets one, and a
  lock surface that couldn't be created is tried again. Every display shows
  the whole prompt, sharing one password and authentication state.
  Disabled outputs stay off and mirrors show their source's prompt.
- The compositor reconfigures lock surfaces when an output's mode, scale or
  rotation changes, chooses the keyboard's lock surface again after output
  changes and as the pointer moves between displays, and skips dead or
  mirrored-away lock surfaces.
- Aizome's toolkit views windows again when outputs change, so an existing
  prompt is laid out for its display's new size.

## Checks

The commands are in [DEVELOPERS.md](../../DEVELOPERS.md#aizome).

- `aizome-shell`'s lock tests: authentication from every display's field,
  output loss before the lock is confirmed, intervals with no outputs, a
  returning panel, a typed password surviving one display's removal,
  surface replacement, and views updated by an output description alone.
- `aizome-comp`'s `lock_displays`: lock surfaces refitting mode, scale and
  rotation changes, and the keyboard following the pointer between lock
  surfaces, with a test lock client. With the built shell, its desktop cases
  cover repeated removal and restoration, disabled outputs, lid close then
  undock then lid open, DPMS, mirrors, input isolation, and which prompt has
  the keyboard (`aizomectl input-state` names it).
- Negative controls reproduced the original shell failure, the missing
  reconfiguration, the stale keyboard after the focused display left, and
  the keyboard staying put when only the pointer moves.

These are component checks over private Wayland sockets. Physical
suspend/resume, docking on hardware and exact-image ISO qualification were
not run for the fix.
