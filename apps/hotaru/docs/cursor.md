# Native terminal cursor

Hotaru follows DECSCUSR requests for blinking or steady block, underline, and
bar cursors. An omitted parameter or parameter 0 restores the configured default;
RIS and DECSTR do the same. DEC mode 25 controls visibility and mode 12 controls
blinking. Mode 12 participates in saved DEC modes. Cursor shape follows Ghostty's
screen transitions; ESC 7/ESC 8 save position and attributes without saving shape.

The standalone core starts with a steady block, matching Ghostty's C API.
The native app selects a blinking block before receiving PTY output, matching
Ghostty's default application configuration. `Terminal::set_default_cursor`
changes reset/default values without overriding a program's explicit DECSCUSR
request. Snapshots own typed shape, blink, visibility, and viewport presence.
A scrolled viewport can still contain the active cursor: its displayed row is
the active row plus the scroll offset. Wide-cell tails paint the entire glyph's
cell reservation, including when the cursor is on the continuation cell.

The native cursor blinks every 600 ms while focused, visible, and in the viewport.
Keyboard input, paste, and new output reset the visible phase. Output-only
resets are coalesced; cursor appearance/movement and input reset immediately.
An inactive pane/window shows a steady hollow block. Preedit shows a steady
block even when DEC mode 25 hides the normal cursor, provided the cursor remains
inside the viewport. Blink redraws invalidate only the cursor cell and reuse
already-shaped glyphs; the font worker does no work for blink phase changes.

Kit's keyed widget timers are separate from existing layout ticks. Each key has
one replaceable native token, cancelled tokens leave the dispatch map, and
queued cancelled callbacks are ignored. Timers suspend for hidden windows and
are removed when their node or window disappears. Keyed, indexed/unkeyed and
duplicate-key fallback identities include the concrete widget type, preserving
state for a same-type rebuild and ending both state and timer lifetime when
that type changes. No new recurring
application timer or worker thread is introduced for cursor animation.

The current UI does not expose custom default cursor shape, cursor color,
cursor text color, opacity, password-lock sprites, or animated cursor movement.
Its default cursor color is the terminal default foreground, and text under a
block uses the default background. Bar/underline strokes use rounded physical
pixels. These tests establish behavior; they do not claim exact pixel parity
with Ghostty's font sprite renderer or whole-application performance parity.

## Evidence

The candidate is based on Sekka commit `cb91125`, independently from the row
cache and mapped-history experiments. `tools/cursor-oracle.py` captures 756
fixtures from the unchanged pinned Ghostty library, including actual render
state and reply bytes. The tests replay them at three input chunk sizes:
2,268 comparisons. The fixture records the library hash and upstream revision.

Native tests cover shape differences, 600 ms transitions, cell-bounded damage,
focus/unfocus, paste reset, 1,000 input resets without accumulating timers,
wide heads/tails, preedit, scales 1/1.25/2, and pane teardown. Kit tests cover
same-type key/index stability, different-type replacement and return, queued stale
callbacks, hidden/show lifecycle, and window close. The independent review
found type-cycle state retention edges in keyed and unkeyed identity; the
regressions were recorded failing before their corrections and passing afterward.

The combined core qualification in
[`upstream/combined-core-cursor/summary.json`](../upstream/combined-core-cursor/summary.json)
passes 136 tests in both debug and release, strict Clippy, and all 6,174 comparisons
across this cursor slice and the earlier terminal fixtures. Its exact source and
binary hashes include the integrated grapheme-charge and row-construction changes.

`tests/wayland/cursor.py` runs in a disposable headless Sway/pixman session.
Actual screenshots verify the three steady shapes, both phases of all three
blinking shapes, default reset, hollow unfocused cursor, and refocus. The second
window uses a separate private runtime directory to avoid single-instance
handoff. This is native compositor-loop evidence, not VM or hardware-display
qualification.

## Decorated block-cursor correction

The subsequent native sprite review exposed an older decoration gap. Ordinary
block-cursor redraw erased underline, overline and strike; the sprite-overhang
path preserved them but retained their original colors. The pinned Ghostty
renderer tints all non-cursor source-cell quads, including decorations, with
cursor text color. The correction preserves underline/overline before glyphs
and strike after glyphs, clips cursor color to the source cell (including wide
cells), and keeps the original span geometry so dotted/dashed/curly patterns
retain their phase.

The focused regression fails on the previous source and passes after correction.
Ten paired narrow/wide underline-style cases compare ordinary and overhang paint
paths, expected cursor/neighbor pixels, opaque sprite overlap and exact partial
damage. All 57 native tests and strict all-target package Clippy pass. The
[qualification and patch](../upstream/cursor-decoration/README.md) preserve logs
and source identities; all 221 available source-manifest entries match the
tested candidate. Its applied vendor files match too; five uncompiled patch
inputs are intentionally absent from that isolated mirror. The earlier
`73ea25d` Nix package and GUI pilot remain frozen historical artifacts and do
not include this later correction.
