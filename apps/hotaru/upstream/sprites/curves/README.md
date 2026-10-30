# Expanded box and Powerline sprite qualification

This isolated component extends the earlier 153-sprite implementation to 178.
It is based on product commit `2364746`, preserving the cursor-decoration fix.
`summary.json` records the exact product patch, source manifests and results.
It does not claim Nix package acceptance or whole-application performance parity.

## Exact scope and evidence

The new ranges are U+256D–2573 and U+E0B0–E0BF, U+E0D2, U+E0D4.
These 18 Powerline glyphs are every geometric Powerline sprite function in the
pinned reference, not every glyph marketed as a Powerline font symbol.
`remaining-inventory.json` lists all 54 registered ranges: 1,085 scalar slots,
178 qualified here, 907 still pending, plus 11 internal decoration/cursor
sprites. Registration alone does not prove that every slot produces ink.

`evidence/qualified-expanded.json` checks 57,700 complete Alpha8 masks,
57,700 origin/ink bounds and 2,454,494,850 bytes across 2,308 metrics, including
minimum/degenerate and maximum accepted dimensions and fractional-derived
physical sizes. There are 100 exact checks against committed Ghostty PNG crops.
The permanent Cargo fixture adds those 100 plus 23 translation-order regressions,
while retaining all 612 earlier PNG expectations. No expected pixels or tolerance
were altered to make a test pass.

`native/wayland/result.json` checks all 178 sprites at output scales
1, 1.25, 2 and back to 1: 712 complete glyph comparisons, 2,294,064 RGB bytes.
The initial/final scale-1 screenshots are identical. The run uses an aligned
private Sway/pixman surface, with Aizome chrome and the real font worker/cache.
It is neither arbitrary fractional-position qualification nor GPU presentation.
`native/tests.log`, `native/clippy.log` and `native/standalone-tests.log` retain
57 ordinary native tests, strict all-target package Clippy and 7 pure-crate tests.

The original red mask report `evidence/initial-curves.json` is retained. Ghostty
translates the path into its padded surface **before** computing stroke geometry;
translating only the finished outline changed 23 masks. `translated-curves.json`
records the corrected exact result. The circular Bézier coefficient is the
observed Zig compile-time f64 `0.5522847498307935`; `coefficient.json` and
`tools/numbers.zig` preserve that one-ULP-sensitive observation.

## Implementation, licensing and bounds

Ghostty drawing definitions retain their MIT notice. `vector.rs` is a safe Rust
port of the needed z2d path, stroke, offset and multisample operations at pinned
commit `40af3e84a4e15a9f31d126b3f281f23ce6a3dae5`. Its source remains MPL-2.0,
with the complete z2d/Cairo notices retained. Both package expressions install
the two added notice files; no runtime dependency, platform source or lockfile
change is introduced. The crate declares `MIT AND MPL-2.0`.

The vector implementation accepts only fixed internal glyph paths. It preserves
reference flattening tolerance, round/miter joins, butt caps, inner offsets,
nonzero four-times multisample coverage and integer Alpha8 source-over. It is
not a general arbitrary-path API. Internal vectors have a 32,768-element bound,
fallible reservation and a depth-32 flattening cap; mask dimensions and 4 MiB
allocation limits are checked before raster work. There is no unsafe code.

The native adapter uses the already-qualified metrics and Alpha8 cache unchanged:
512 entries/4 MiB conservative admission, immutable masks, strong frame ownership,
ordinary fallback for unsupported metrics, row and prior-frame charge limits.
New diagonals and open Powerline shapes can have padded ink at ordinary metrics.
That invokes the existing correct two-pass/full-terminal damage path and rejects
pixel-scroll reuse for either old or new overhang. This is a remaining efficiency
cost, not hidden behind the exact-pixel result.

`evidence/cold-cost.json` is a candidate-only cold raster diagnostic. Typical
median allocation+raster+bounds+drop was about 4–10 microseconds per new glyph;
the maximum-area case was about 21.4 ms. It held the workstation lock but did not
have a host-idle acceptance gate. It provides no comparative performance claim.

## Reproduction on wx-workstation

The reference is an external unchanged checkout at
`/home/callen/work/ghostty-reference-20261028`, Ghostty commit
`6ba144158577d033ccb6ed4b574e4013957c9161`. The preparation helper verifies the
pin, Zig executable, original raster sources, all 50 z2d source files, and the
expanded Powerline source/adapter against recorded hashes. Choose a fresh scratch
root and run `tools/prepare-expanded.py` with:

```
--reference /home/callen/work/ghostty-reference-20261028
--zig /home/callen/.cache/hotaru-reference/zig-x86_64-linux-0.17.0/zig
--output /absolute/fresh/scratch/oracle-expanded
--candidate-source /absolute/sekka/apps/hotaru/crates/hotaru-sprites
```

This copies a diagnostic Rust byte transport into sibling `rust/`; it does not
change the product. Execute the emitted Zig command from `oracle-expanded/` and
the emitted Cargo command under
`flock -x /home/callen/.cache/hotaru-workstation.lock`. The helper only prepares;
it does not build. Then run `tools/compare-final.py --root /absolute/fresh/scratch
--output /absolute/fresh/scratch/report.json`. That comparator takes the lock
internally: do not hold the same lock around it. It expects Pillow and the exact
external reference path above, and checks both full masks and committed PNGs.
The comparison transport and wrapper manifest are archived alongside the tools.

For native checks, use the product `tests/wayland/sprites.py` with the expanded
library. The qualified external library is
`/home/callen/work/hotaru-sprites-curves-20261028/oracle-expanded/libsprites.so`,
SHA-256 `b4446973820f50cbea0ca1be181e4add0a1180f4410542c5e03dc2240f580bdd`.
The old 153-sprite library intentionally rejects the new ranges. Native reports
record the precise library, executable, metrics and source hashes.
