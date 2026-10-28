# Native terminal sprites

Hotaru now ports 153 rectilinear Ghostty sprites to a safe Rust raster and
uses them in Hotaru's existing Aizome/Sidestep software drawing path. It has no
Ghostty runtime or Zig dependency. It does not establish whole-application or
GPU performance parity.

## Qualified raster scope

The port covers U+2500–U+256C, U+2574–U+257F and all U+2580–U+259F. Rounded
corners and diagonal strokes U+256D–U+2573, braille, powerline, geometric,
legacy-computing and branch sprites remain unported. Those codepoints use the
ordinary native font path. Supported characters with additional combining or
presentation-selector scalars also retain ordinary font shaping pending an
exact upstream selection-policy port.

The pure `hotaru-sprites` crate owns a padded Alpha8 mask. Metrics are final
physical cell width, height and box thickness. All drawing uses checked safe
slices; there is no unsafe code or runtime dependency. Invalid metrics,
unsupported scalars, masks above 4 MiB and reservation failures are explicit
errors. The separate Ghostty MIT notice accompanies adapted logic and fixtures.

The historical isolated raster qualification remains under
`upstream/sprites/reference/`. It used unchanged pinned Ghostty drawing files
and actual pinned z2d surfaces: 310,896 complete padded masks across 2,032 metric
sets, plus exact placement/ink bounds, and 612 comparisons to upstream PNG
atlases. These reports retain their original source identities. The current
crate's ordinary Cargo tests independently compare all 612 lossless upstream
PNG expectations; expectations are never generated from the Rust raster.

## Native metrics and painting

Sprites split normal shaping runs without changing terminal columns. The
worker records ordinary span style, selection/Find state, hyperlinks and sprite
instances together in the immutable frame. A sprite uses the existing span
foreground, including inverse/dim/hidden, selection and Find colors. Underline,
strike and overline remain native decoration commands; the block cursor
repaints the same mask using the cursor glyph color.

Box thickness is separate from native decoration thickness. The Linux reference
uses positive raw `post.underlineThickness`, scaled by FreeType's integer
`y_ppem` and rounded upward to at least one physical pixel. The native path
reproduces the 26.6 point/DPI/ppem rounding. Its fallback follows the reference's
15% ex-height, with cap/ascent fallback; old fonts lacking OS/2 heights use
Skrifa's unhinted H/x bounds, so exact hinted-FreeType fallback parity is **not**
claimed. This limitation is visible in diagnostic metric metadata.

The Sidestep Alpha8 operation owns immutable bounded mask data and tints while
blending, with the same premultiplied arithmetic as native glyphs. It introduces
no new unsafe code. Rectangular clips, banded damage and canvas origins are
honored. Generic shadow/path-mask/non-normal blending/rotated drawing uses the
existing image implementation, with one lazy bounded white RGBA template per
mask. It never silently ignores an unsupported graphics state. Final-scale
masks map one sample to one device pixel; nearest resampling is used only during
the existing coherent retained-frame transition to a new output scale.

The full quarter-cell padded raster is retained. The worker flags actual
nonzero ink outside a cell. These rare frames paint all backgrounds first and
then foregrounds with full sprite bounds; ordinary text and decorations retain
their own clips. Both entering and leaving such a frame force full terminal
damage, including when prior-frame admission rejected its charge. Pixel-scroll
reuse rejects either old or new overhang. Block cursor backgrounds precede
foreground glyphs, and tint follows the glyph source cell, so neighboring
padded ink survives a cursor over a blank cell. Cursor blink/input also invalidates
the full affected terminal for these rare frames. Normal masks retain the
existing incremental row damage and pixel-scroll behavior.

## Ownership and bounds

The font worker's sprite cache uses exact `(scalar,width,height,thickness)` keys
and strong immutable ownership. It admits at most 512 entries and 4 MiB of
conservative mask charges, evicting the oldest use. A mask too large for cache
admission remains owned by the current frame and is not silently dropped. Metrics outside the pure raster
limits select ordinary font shaping before span classification, preserving
surrounding text instead of rejecting the entire frame.
Cache eviction cannot invalidate a visible frame. The cache's map allocation,
required current-frame ownership and existing image-renderer cache are separate;
this is not a 4 MiB process-RSS claim.

Row-cache and prior-frame admission count instance arrays and mask ownership,
including the potential lazy RGBA template. Shared masks are deliberately
counted repeatedly for conservative admission. Previous-frame admission and
row-cache caps continue to apply. Generation keys, owned RowRef identities and
exact snapshot/frame pairing retain the earlier renderer ownership rules.

## Qualification state

The isolated combined candidate passes 56 ordinary native tests, six pure raster
tests (including 612 committed-PNG expectations), four Alpha8 raster tests and
the generic-state UI test. Strict native package Clippy passes. The standalone
workspace's locked metadata resolves the new crate. The Linux ppem calculation
matches 126 recorded FreeType size/scale cases. Final source identities and
checks are in `upstream/sprites/native/summary.json`; no package acceptance is
claimed until the integrated Nix build and package suites pass.

The native regressions include mask ownership/eviction, shaping boundaries,
source styling/links, all four exact pixel-scroll checks, every pixel of padded
ink over neighboring backgrounds, and leaving an oversized retained frame.
Unsupported metrics preserve surrounding normal text and recover when metrics
return to range. A separate red-cursor fixture checks neighboring ink crossing
into a blank cursor cell and tinting only the cursor's own source glyph.

`tests/wayland/sprites.py` creates a private borderless Sway/pixman session,
retaining Hotaru's Aizome chrome. It locates an exact full-cell marker and checks
every RGB byte of every padded sprite against the actual pinned Ghostty oracle.
The final run passes 612 glyph comparisons at scales 1, 1.25, 2 and back to 1:
1,971,864 RGB bytes, with identical initial/final scale-1 screenshot hashes.
The report records actual client/output geometry; at 1.25 the logical surface
is exactly 1536×1520 at origin zero, mapping to 1920×1900 physical pixels.
Captures are original-resolution software output, not a GPU presentation test.

Two rejected runs are preserved. A decorated fractional surface produced
filtered edge pixels, consistent with compositor resampling; general unaligned
fractional window qualification remains open. The first borderless run then
exposed a distinct one-pixel mask sampling error at a half-pixel widget origin.
The corrected exact-size path uses the same f32 pixel-center convention as the
canvas, with explicit half±epsilon, negative-origin and x/y tile tests. No
expected raster pixels or comparison tolerance were changed. The passing
probe waits for complete image equality rather than treating a PTY response or
an elapsed delay as proof of a rendered frame.
