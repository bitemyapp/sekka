# Source-cell cursor decoration correction

Baseline: terminal_view.rs SHA256
005e153a8ef92944d010c443cae7de0d28e3bdbd022aa6c9135eded1ebd2c93e
from the frozen153-sprite product. Candidate:
63e11655cc55dfef2e4a991721017a34742489ba69edf566cda29a0841804f6f.

Pinned Ghostty6ba144158577d033ccb6ed4b574e4013957c9161:
`src/renderer/generic.zig`3261draws underline before text, then overline;
3358draws strike after text. Their source-grid-position quads have no
IS_CURSOR_GLYPH flag. `src/renderer/shaders/glsl/cell_text.v.glsl`142–145
replaces all such cursor-cell colors with cursor-text color, including wide
cursor cells. Ghostty’s padding source-cell semantics are preserved.

The ordinary Hotaru path previously erased decorations with its late block
background and redrew glyphs only. The oversized-sprite path retained decorations
with their original colors. This focused correction restores/tints ordinary
cursor decorations and clips rare-path decoration colors without resetting the
original dotted/dashed/curly phase. Underline/overline precede text/sprites;
strike follows. Exact upstream decoration raster geometry remains separate.

`qualification.json` records commands and source identities. `red.log` shows
the frozen renderer’s actual pixel assertion failure; `green.log` records57
ordinary native tests passing(7existing diagnostics ignored). `clippy.log`
records strict all-target package Clippy. The new test covers five underline
kinds with narrow and wide cursors, both paint paths, explicit underline color,
opaque sprite/decor layering and exact partial/full damage.

No packaged runtime or whole-app efficiency claim is made by this isolated test.
