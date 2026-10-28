# Native terminal pixel scrolling

Hotaru now connects compatible row scrolling to Aizome Kit's existing pixel
move operation. The integrated files exactly match the qualified candidate
identified in [the result](../upstream/pixel-scroll/result.json), based on
product checkpoint `e2b1ff7`. This change adds no unsafe code and changes no
native buffer ownership.

Content epoch and absolute viewport position propose a translation. Each
destination row must also have identical owned `RowRef` and shaped-span `Arc`
identities before its pixels can be reused. Font revision, metrics, palette,
default colors, geometry, scale and theme must agree. The rectangle covers only
terminal cells and requires integer physical coordinates. Kit additionally
rejects incompatible clipping, covered regions and nested scroll operations.
Selection, Find highlights, hovered links and IME fall back to normal damage.
Exposed or unproven rows repaint, together with the old cursor row, new cursor
row and translated old cursor row, including wide glyphs and hollow cursors.

Each mounted terminal view may retain one previous shaped frame, admitted only
when its conservative logical ownership charge is at most 8 MiB. The font
worker computes this charge; cached rows reuse it without revisiting glyph
layouts. Arithmetic saturates, so overflow rejects admission. Shared data may
be counted repeatedly. This limit does not bound process RSS, toolkit font
caches, the row cache or the existing current frame. Oversize replacement and
window teardown release the retained frame. Hidden-window tests verify that
intermediate completed frames are released.

The isolated Hotaru suite passed 51 tests with seven intentional ignores; Kit
passed 160 tests and three doctests with one intentional ignore. Strict Hotaru
all-target Clippy passed. Four exact software-frame tests cover scroll
directions, queued moves, retained-row edits, cursor styles and focus, scales
1/1.25/2, resize, palette changes, clipping, covering siblings, selection, Find,
links and IME. This includes clearing IME and scrolling before another paint.
Every partial framebuffer equals a fresh full rasterization without tolerance.

The final native private-Sway comparison used one binary with the existing
`AIZOME_NO_PIXEL_SCROLL` switch enabled and disabled: all 28 captured frame
pairs had identical full-window RGBA bytes. Wayland traces exercised three SHM
buffers, 60 attaches and 59 consecutive buffer switches in each run. Source
review by a second agent found no concrete ownership or damage defect.

Across nine software scroll transitions, the implementation copied 3,294,720
pixels and rasterized 829,440 unique damaged pixels out of 5,529,600 total
window pixels. One single-row transition rasterized 69,120 of 614,400 pixels.
These are damage coverage counts, not elapsed-time or GPU performance results.
They do not establish relative application performance against Ghostty.

Compact receipts and unmodified qualification logs are in
[`upstream/pixel-scroll/`](../upstream/pixel-scroll/). Full screenshots, traces
and the original candidate patch remain under
`/home/callen/work/hotaru-pixel-scroll-isolated/artifacts/pixel-scroll/` locally
and on `wx-workstation`; the qualified binary is retained on the workstation.
The integrated source hashes match all five qualified files exactly. A new
combined Nix package qualification remains separate from this isolated result.
