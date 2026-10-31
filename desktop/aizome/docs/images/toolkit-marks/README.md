# Toolkit marks: mocks

Two styles for marking which toolkit each app is made with, beside its
name in the Toolchest and the command palette: a word in a small tag
(`*-text-*`) or a glyph (`*-icon-*`), with Aizome's own apps carrying the
mark. In both, the palette's detail pane gives the toolkit's full name.

These are renders, not screenshots of a session. `crates/aizome-shell/tests/toolkit_mocks.rs`
runs the real shell headless on the kit's offscreen renderer at 2×, over
the boards' scene from `tests/common`, with made-up apps whose toolkits
are given by hand: nothing here detects a toolkit yet. For the Toolchest
the test draws the desktop, the menu bar and the open menus where they'd
show, then crops to the menus; the palette is its window alone. Rerun it
with `cargo test -p aizome-shell --test toolkit_mocks`; the pictures go to
`target/shell-shots/`.

The `compare-*` sheets put the two styles side by side, labelled, made
from the renders here with ImageMagick (`magick … +append`); the renders
themselves are unchanged. Every PNG was then recompressed losslessly
(oxipng, pixel-identical).

| File | What |
|---|---|
| `compare-toolchest-system-night.png` | Toolchest ▸ Applications ▸ System, tags beside glyphs |
| `compare-toolchest-internet-night.png` | Toolchest ▸ Applications ▸ Internet, tags beside glyphs |
| `compare-palette-night.png` | The palette's Apps scope for "s", tags beside glyphs, Settei selected |
| `toolchest-{system,internet}-{text,icon}-{night,day}.png` | Each Toolchest render |
| `palette-{text,icon}-{night,day}.png` | Each palette render |
