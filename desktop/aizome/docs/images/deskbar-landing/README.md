# Deskbar landing: renders

What dragging the Deskbar's grip shows: an outline of the Deskbar at the
other edge, dashed while letting go would leave it where it is, solid in
the focus colour once letting go would move it there.

These are renders, not screenshots of a session. The ignored test
`pictures_of_the_grip_drag` in `crates/aizome-shell/tests/deskbar_designs.rs`
runs the real shell headless on the kit's offscreen renderer at 2×, over
the boards' made-up scene from `tests/common`, with the Deskbar at the
right. It sends the grip's drag messages (a start on the grip, then a move
over the Deskbar's own half and one over the other half) and composes the
desktop, the menu bar, the Deskbar and the landing surface where the
compositor would place them. The pointer isn't drawn. The wallpaper is
`desktop/tatami/wallpapers/da-nang-at-night.jpg`, credited in that
folder's README.

Rerun it from `crates/aizome-shell` with absolute paths:

```sh
DESKBAR_DESIGNS=/path/to/folder \
DESKBAR_WALLPAPER=/path/to/sekka/desktop/tatami/wallpapers/da-nang-at-night.jpg \
  cargo test -p aizome-shell --test deskbar_designs -- --ignored pictures_of_the_grip_drag
```

It writes eight full-screen pictures (2880 × 1800): two widths, two steps,
night and day. The three here are cropped to the screen's top 540pt, where
the drag happens (`magick … -crop 2880x1080+0+0`), and recompressed
losslessly (oxipng, pixel-identical).

| File | What |
|---|---|
| `grip-drag-standard-1-held-night.png` | Night, Standard width, the pointer 200pt left of the Deskbar and still over its half: the grip pressed in with its dots in `focus`, and a dashed outline at the left edge saying **Drag here to move** |
| `grip-drag-standard-2-leaving-night.png` | The pointer over the left half: the outline solid in `focus`, **Release to move here**, and the Deskbar's rows faded |
| `grip-drag-rail-2-leaving-day.png` | Day, at the Rail's width (Icons Only), over the left half: the badge keeps only its arrow |
