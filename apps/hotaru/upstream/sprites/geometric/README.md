# Corner-triangle sprite qualification

This isolated slice adds the eight character rasters registered by pinned
Ghostty's `geometric_shapes.zig`: U+25E2–25E5, U+25F8–25FA and U+25FF. The total
qualified character set is 442. This is the source's supported subset of the
Geometric Shapes block, not an implementation of every character in that block.
The private Rust raster code remains safe and bounded. Native metrics, mask
ownership, the 512-entry/4 MiB admission cache, row caching, padded-ink damage
and normal font fallback for unsupported families retain their existing APIs.

## Exact sources and numerical behavior

Ghostty is pinned to `6ba144158577d033ccb6ed4b574e4013957c9161`;
`src/font/sprite/draw/geometric_shapes.zig` has SHA-256
`e81950c543d683d394a499b1fe47e7071f1fb97db723f7b45394f207f7617d0c`.
The eight point tables are adapted under its MIT notice. Closed inset, miter
joins and nonzero 4× coverage follow z2d commit
`40af3e84a4e15a9f31d126b3f281f23ce6a3dae5`, retaining its MPL-2.0 notices.
There is no Zig runtime or foreign raster code in the application.

The first 18,464-mask matrix exposed nine one-sample antialiasing differences.
The port's platform `f64::hypot` rounded differently from the pinned Zig standard
library. A safe translation of that library's f64 fused algorithm, including
normal/subnormal scaling, matches 131,216 actual Zig observations; platform
libm differs on 341 of those cases. The helper-only change clears all nine mask
differences. Thirty differing bit patterns are permanent unit regressions.
The exact std source and distribution license are retained as
`evidence/zig-hypot.zig` and `evidence/LICENSE.zig`. Both Nix packages install
that unchanged license as the ninth notice, `Zig-MIT.txt`. It does not add a
runtime dependency or change either Cargo lockfile.

An expanded fixture then caught U+25F9 at 9×12, thickness 6. Its actual inset
points are `(8,6)`, `(7.999999999999999,5.999999999999997)`, `(8,6)`. The reference
has a special close-path branch when the final point already equals the first;
the initial Rust cyclic loop skipped that join. The correction preserves both
opposite joins instead of discarding the tiny contour or using an epsilon.
The captured red test, green test and actual offset diagnostic are retained.
Miter dot-product grouping also follows the pinned operation order explicitly.

## Qualification

The final pure ReleaseSafe actual-library comparison passes:

- 43,128 complete triangle masks and placement/ink bounds across 5,391 metrics;
- 1,111,842,592 exact Alpha8 bytes and all 32 relevant committed PNG crops;
- all accepted thicknesses for the small-metric grid, exact and adjacent
  Pythagorean inradius cases, fractional-scale metrics and maximum mask bounds;
- 57,700 existing rounded/diagonal/Powerline masks and bounds, 2,454,494,850
  Alpha8 bytes and 100 committed PNG checks after shared normalization changes.

The permanent triangle fixture has 77 real expectations: 32 PNG crops, nine
original rounding failures and 36 degeneracy/boundary cases. It adds to all
previous family fixtures. The standalone crate passes 13 tests; the current
native candidate passes 62 ordinary tests and strict package Clippy. Seven
existing native diagnostic tests remain ignored. Pure all-target Clippy passes.
The native source base includes the preceding bounded region-scroll correction.

The real private Sway/pixman capture compares all 442 glyphs at 1×, 1.25×, 2×
and back to 1×: 1,768 glyph crops and 5,696,496 RGB bytes exact. Initial/final 1×
capture hashes agree. These captures use the unchanged ReleaseFast oracle
because the earlier Braille family has a documented ReleaseSafe assertion
failure. Pure triangle/shared-vector qualification uses ReleaseSafe. Each
library has its own recorded hash; build mode is not silently substituted.

These are raster/native correctness checks, not comparative throughput,
arbitrary fractional surface placement, hardware presentation or whole-app
performance acceptance. The remaining inventory lists 643 registered character
slots and 11 internal rasters still unqualified. Some registered slots may be
intentional blanks; ordinary font or native decoration drawing does not prove
sprite equivalence.

## Provenance and reproduction

`summary.json` records the exact patch, 13 product hashes, 236 tested source
hashes, Rust binary, actual libraries and counts. The text patch is
`native/candidate.patch`; copy the separate lossless `geometric.rle` fixture
with its recorded SHA-256 after applying it. Native package integration and
Nix runtime qualification remain separate from this isolated evidence.

One passing intermediate report, `evidence/hypot-geometric.json`, is explicitly
**unqualified**: its binary hash was read before acquiring the shared lock while
its build was pending. Raw data is preserved. Library loading and hashing now
happen inside the lock. The locked replacement and final expanded report have
consistent binary identities; no reference pixels or tolerances were changed.
`evidence/provenance-note.json` records this correction.

All compilation, tests and benchmarks run only through `ssh wx-workstation`.
Use the shared `/home/callen/.cache/hotaru-workstation.lock` for build/runtime
commands. The Python comparators acquire that lock themselves; do not hold a
second lock around them. `tools/prepare-geometric.py` and the base helper verify
the exact Ghostty checkout, compiler hash and unchanged z2d sources, then create
a separate oracle source directory. The captured build command is in
`evidence/preparation.json`; execute it from that directory. The separate
ReleaseFast command is in `evidence/fast-oracle.json`.

To recreate the diagnostic Rust transport, copy production sprite sources,
tests and licenses into `rust/`; use `tools/prototype-Cargo.toml` and
`tools/prototype-main.rs`. Build with Cargo 1.95 release mode on the workstation.
Run `compare-geometric.py --root /absolute/scratch --output /absolute/report.json`
and `compare-existing.py` with the same arguments. Both expect the pinned
external reference checkout and Pillow. `compare-hypot.py` checks the separate
actual std-library diagnostic and Rust helper example. Captured initial tools,
helper source and original failing reports remain available for causal audit.

Native build and capture commands are preserved in `native/check.sh` and
`native/capture.sh`, using the validated Aizome/Hotaru font directories. The
actual library path is
`/home/callen/work/hotaru-sprites-geometric-20261028/oracle-expanded/libsprites-fast.so`,
SHA-256 `4855a67ebb19e6f4c2713553001ad875287c62337a0c765ea22df6fd1bb41891`.
The executable/source manifest and original-resolution images are under
`native/`; no bitmap is resized for qualification.
