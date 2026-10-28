# Rectilinear sprite prototype

This isolated safe Rust library adapts 153 terminal sprites from Ghostty
`6ba144158577d033ccb6ed4b574e4013957c9161`. It is not integrated into Hotaru,
Sidestep, an atlas, or a presentation backend. It has no runtime dependencies,
FFI or unsafe code. Preserve `LICENSE.ghostty` with adapted sources.

## Exact scope

- U+2500–U+256C: straight, light/heavy, double, dashed and mixed box junctions.
- U+2574–U+257F: directional half-lines and mixed-weight straight lines.
- U+2580–U+259F: the complete Block Elements range, including shades/quadrants.

U+256D–U+2573 (rounded corners and diagonals) are explicitly unsupported. So
are braille, powerline, legacy-computing, geometric, branch and decoration
sprites. The excluded curved/diagonal paths use z2d antialiasing; this prototype
does not substitute an invented approximation or a font glyph for them.

`supports(cp)` advertises only implemented codepoints. `rasterize` returns an
explicit error for unsupported scalars, invalid physical metrics, an oversized
mask or allocation failure. A normal font fallback, if an application chooses
one, must remain visibly documented as an unported case.

## Pure API and ownership

```rust
let image = hotaru_sprites::rasterize(0x256c, hotaru_sprites::Metrics {
    cell_width: 11,
    cell_height: 24,
    box_thickness: 1,
})?;
```

`Metrics` contains already-rounded **physical pixels**, not font points or
logical units. Ghostty's `font/Metrics.zig` computes box thickness from
`max(1, ceil(underlineThickness()))`. The future integration must obtain that
quantity from the selected native font at the actual scale; it must not guess
thickness from glyph width or bold state. This API deliberately does not invent
a second font metric system.

`Raster` owns a row-major Alpha8 `Vec<u8>`, with stride `width`. The full image
includes Ghostty's quarter-cell padding on each side. `origin_x/y` are signed
physical offsets from the cell's top-left. `ink` is the minimal nonzero
half-open rectangle within that padded image, or `None` for an empty mask.
A cropped upload must be placed at:

```
cell_origin_physical + raster.origin + ink.{x, y}
```

Divide the physical placement/dimensions by output scale only when submitting
to a logical-coordinate canvas. Do not round that logical position again.
Damage and clipping must include any padded overhang; blindly clipping all
sprites to the cell would change thick-stroke output.

Dimensions are limited to 1–4096 pixels, thickness to 1–min(width,height), and
the padded allocation to 4 MiB. All checks precede reservation. A successful
`try_reserve_exact` precedes initialization, and all drawing uses checked safe
slices. These are per-raster bounds, not a claim about allocator RSS or a future
cache's total size. There is currently no global cache, hidden retained state,
thread, or graphics object. Returned pixels remain usable independently of the
renderer or input metrics. Cancellation can be checked between raster calls;
this deliberately bounded small operation has no asynchronous lifetime.

The port retains upstream details: positive fraction rounding, complementary
odd-half overlap, source overwrite rather than alpha blending, fixed shade
alpha 64/128/192, different horizontal/vertical dash spacing, tiny heavy-dash
fallback to a **light** solid stroke, and clipping of excessive stroke geometry
against the padded canvas.

## Reference and evidence

The oracle uses four **unchanged** pinned Ghostty files:
`draw/box.zig`, `draw/block.zig`, `draw/common.zig`, and `canvas.zig`.
It runs their actual drawing code and the actual pinned z2d Surface, rather
than reimplementing expected pixels in Python. A C ABI shim accepts physical
metrics and copies the entire padded Alpha8 buffer. The tiny font shim exposes
only the three metrics used by this subset; the global/quirks shims supply
reference I/O/assert infrastructure. They do not replace geometry or pixels.
Oracle code is a development reference only, never a product dependency.

`evidence/provenance.json` records the Ghostty revision, exact unchanged source
hashes, all 50 z2d source hashes, Zig 0.17.0 binary hash, and shim hashes.
`tools/generate-lines.py` verifies that pinned source hash before extracting
only the 109 scalar-to-arm-style mappings; all raster algorithms are separately
ported Rust. Raster expectations are never generated from the Rust code.

The initial run in `evidence/initial.json` passed 310,896 exact complete-mask
comparisons: every supported scalar at 2,032 physical metric sets, totaling
627,591,618 checked bytes. Metrics include exhaustive small dimensions and
stroke widths, deliberately oversized strokes, seeded larger cases, all four
upstream atlas metrics, and representative ceil-quantized dimensions for
scales 1, 1.25, 1.5, 1.75, 2, 2.25, 2.5 and 3. The scale cases validate supplied
physical metrics; they do not validate an application's font metric derivation.

A second independent reference check compared all 153 oracle glyphs at each of
the four metrics with Ghostty's committed PNG atlases: 612 exact matches. This
catches accidental changes from the narrow oracle environment. Reports retain
source and executable hashes, and later source/transport changes require a new
report rather than rewriting the original evidence.

The final run also passed 310,896 metadata comparisons: full dimensions, signed
origins and minimal ink bounds independently calculated from the Zig pixels.
Six permanent Cargo tests pass in debug and release, including a lossless
82,414-byte RLE fixture containing all 612 committed PNG crops; strict all-target
Clippy and rustfmt also pass. Independent review found no bounds, ownership or
allocation-failure defect in this pure API. The final validation manifest binds
these checks to the exact source and tool hashes.

To reproduce, prepare an isolated reference with `tools/prepare-oracle.py`, run
its emitted build command under the workstation lock, build this crate, then
run `tools/compare.py` against the sibling `oracle/` and `rust/` directories.
The comparison runner takes the workstation lock itself. `cargo test --locked`
requires neither Zig nor Pillow and always checks the committed golden crops.
The PNG extraction tool requires Pillow and verifies each PNG against the
audited hash before regenerating fixtures.

## Proposed native integration (not implemented)

1. Put the pure raster module in a focused first-party crate, or an explicitly
   licensed Sidestep extension patch. Keep it out of terminal parsing/state.
2. On the font worker, split supported standalone sprite cells out of ordinary
   font shaping. Preserve their original terminal columns, widths, style,
   selection/search state and immutable snapshot pairing. Unsupported scalars
   and grapheme/variation-selector combinations remain separate explicit gaps.
   Audit Ghostty's sprite selection rules before broadening interception.
3. Add immutable sprite instances to the worker layout alongside font runs.
   Deduplicate Alpha8 masks by `(codepoint, physical width, physical height,
   thickness)` in a bounded cache. Account for mask allocations, not just keys.
   Color, selection and inverse are paint inputs and need not duplicate masks.
4. Extend the native canvas/raster protocol with an Alpha8 mask operation, or
   integrate equivalent immutable masks with its existing glyph-mask backend.
   The current image path is premultiplied RGBA with per-color template caches;
   an RGBA expansion is a possible validation adapter but carries extra memory
   and must not be presented as an efficient final design without measurement.
5. Keep background, alpha multiplication, dim, hidden text, cursor inversion,
   decorations and selection behavior coherent with existing `draw_grid_text`.
   Add exact offscreen comparisons for adjacent box joins, odd/fractional scales,
   light/heavy/double mixes, selected/inverted shades, resize and row reuse.
6. Qualify actual private Wayland captures and measure the integrated path
   separately. Pixel equality here gives no GPU, frame, or speed parity claim.
