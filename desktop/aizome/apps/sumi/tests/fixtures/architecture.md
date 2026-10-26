# Sidestep's architecture

Sidestep draws every window itself, on the CPU, and hands the
compositor finished buffers. This is how a frame gets made,
from an event to pixels on the screen, and what it costs.

## Frames

A frame starts with an event: a key, the pointer, a timer, a
message from another thread. The program updates its state
and views it; the toolkit lays the view out, diffs it with the
last one, and invalidates what changed.

- Record: the widgets that need it paint into a display list.
- Rasterise: tiny-skia fills the damaged pixels, tile by tile.
- Present: the buffer goes to the compositor with its damage.

The budget is *4 ms* from the event to the frame, on this
laptop, at **2880 × 1800**.

## Text

Text is shaped once per run and kept: a layout cache keyed by
the text, its style and the width it was broken at means a
view rebuilt every frame shapes nothing new.

Glyphs are rasterised by `swash` into a cache of their own,
one entry per glyph, size and subpixel offset.

## Threads

The program runs on the main thread. Rasterising runs on a
render thread with the last frame's display list, so a long
update never stalls a scroll that's already under way.

> Nothing blocks the main thread for longer than a frame.

## Scale

Operations stay in points. Each window renders at its
output's scale: the integer one wherever it can, the
fractional one only when asked. Every surface gets a
buffer of `points × scale` pixels, so the compositor maps
buffer pixels to screen pixels one to one instead of
resampling them.

### Damage
A canvas fills only the pixels whose centres its
damage contains, so neighbouring fills share edges
without gaps or overlaps at any scale, and a pixel
never depends on how damage was cut up.

- Clock tick: one rect, the clock's own.
- Caret blink: two points wide, one line tall.
- Window move: no raster, only composition.

### Tiles