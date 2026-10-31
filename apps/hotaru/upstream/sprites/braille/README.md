# Braille sprite qualification

This isolated slice adds all 256 U+2800–28FF patterns to the preceding 178
box/block/Powerline sprites. It adds one private safe Rust module and dispatch;
the native runtime adapter, ownership model, cache limits, metrics API and
installed notices are unchanged. Braille logic retains Ghostty's MIT notice.
No Zig runtime, dependency, lockfile or additional license change is required.
Product package acceptance is separate from these isolated checks.

## Actual reference behavior and one assertion correction

The pinned source is `src/font/sprite/draw/braille.zig` at Ghostty commit
`6ba144158577d033ccb6ed4b574e4013957c9161`, SHA-256
`8093913ed854b6d46a8b627c465d7f1ac7b2deedd35c208603f06c05bf9a6a8c`.
Dots are **squares** with the reference's integer sizing/spacing/margin priority;
stroke thickness is ignored by this family. Tiny metrics may produce empty
patterns. No rounded-dot approximation or font glyph is substituted.

The unchanged ReleaseSafe reference completed 225,792 comparisons before
aborting at cell 14×23, thickness 1, even for U+2800. It asserts that at least
one remaining-pixel count is ≤1. At that metric both residuals are 2, but another
dot-width increment needs **two horizontal and four vertical pixels**. Existing
dots fit: x=[2,8], y=[2,7,12,17], dot width 2. The source assertion is therefore
overstrict. `evidence/oracle-panic.log`, `upstream-releasesafe-panic-metric.json`
and `upstream-assertion.json` preserve the actual failure and derivation.

The Rust port corrects only that debug invariant to `x_left < 2 || y_left < 4`.
Coordinates, priority order and alpha pixels are unchanged. It also retains
inside-cell bounds checks and exhaustively tests all 4096² dimension pairs
without allocating masks, including pairs rejected by the public mask-size cap.
Thus the assertion is not simply disabled. No reference drawing source was
modified. The complete pixel oracle uses unchanged **ReleaseFast** code and is
explicitly labeled; its hash differs from the retained ReleaseSafe library.

## Qualification and limits

`evidence/qualified-braille.json` reports:

- 594,266 complete Alpha8 masks and origin/ink-bound comparisons;
- 2,330 metric sets, 2,245,723,934 bytes exact;
- all 256 patterns at ordinary/small/random metrics;
- blank, eight basis dots and full pattern for the largest masks;
- all 1,024 crops from four committed upstream PNG atlases.

No tolerance, expected pixel or reference source was altered. The permanent
lossless fixture stores those 1,024 actual PNG crops, retaining all earlier 735
box/curve/Powerline expectations. Union, stroke-invariance and bounds tests
cover ownership-independent Braille properties. The pure crate passes 11 tests;
57 ordinary native tests and strict pure/native all-target Clippy pass. Seven
existing native diagnostic tests remain ignored, as recorded in the logs.

`native/wayland/result.json` compares the full combined 434-sprite set at
1×, 1.25×, 2× and back to 1×: 1,736 glyph comparisons and 5,593,392 RGB bytes
exact. Initial/final scale-1 screenshots match. The private borderless
Sway/pixman test retains Aizome chrome and real font-worker/cache behavior;
31 columns with two-cell spacing keep every padded crop disjoint. The runtime
sprite cache is still bounded to 512 entries/4 MiB conservative admission; the
expanded eviction test retains and verifies masks after eviction. Native font
and cache tests now exercise all 434 supported characters and styled/linked
shaping boundaries. Braille itself stays inside its source cell; pre-existing
curved/Powerline overhang handling is unchanged.

This proves aligned software rendering at the recorded physical metrics,
not arbitrary fractional window placement, hardware presentation, throughput
or whole-app efficiency parity. `remaining-inventory.json` records 651 pending
character slots and 11 internal sprite rasters. Font fallback/native decoration
painting is not evidence of exact sprite equivalence.

## Identities and reproduction

The qualified actual library is
`/home/callen/work/hotaru-sprites-braille-20261028/oracle-expanded/libsprites-fast.so`,
SHA-256 `5ef2a69155f38e0d33cadecd88f29224de5430b03029238fe978ea7c27a8c5d5`.
`evidence/build-final.json` identifies both libraries, Rust binary, copied source
and exact compiler. Native executable/source identities are separate in
`summary.json` and `native/source-manifest.json`; reports retain their original
historical hashes.

`tools/prepare-braille.py` validates the pinned checkout/toolchain, unchanged
raster sources and all 50 z2d files through `prepare-oracle.py`, then validates
and copies the unchanged Braille/Powerline files and explicit oracle adapter.
Pass `--reference /home/callen/work/ghostty-reference-20261028`,
`--zig /home/callen/.cache/hotaru-reference/zig-x86_64-linux-0.17.0/zig`, and a
fresh `--output /absolute/scratch/oracle-expanded`. It writes both safe and fast
command arrays there. The original executed preparation script is retained as
`prepare-braille-captured.py`; the small current helper also emits the fast
command and writes its metadata in the output directory.

Build from that directory by loading and executing `build-command-fast.json`
with Python `subprocess.run(..., check=True)`, under the shared workstation lock.
To build the diagnostic Rust transport, copy production `hotaru-sprites/src/`,
`tests/` and notices to sibling `rust/`, then copy `tools/prototype-Cargo.toml`
to `rust/Cargo.toml` and `tools/prototype-main.rs` to `rust/src/main.rs`.
Run Cargo 1.95 release build under the same lock. The transport is never shipped
in the application.

Then run `tools/compare-braille.py --root /absolute/scratch --output
/absolute/scratch/report.json --oracle
/absolute/scratch/oracle-expanded/libsprites-fast.so --oracle-build-mode ReleaseFast`.
The comparator requires Pillow, expects the exact external reference path above,
and takes the shared lock itself; do not hold a second lock around it. Its large
metric subset is recorded explicitly. `generate-fixtures.py` is the captured
PNG-only expectation generator, never a Rust-output snapshot generator.

Native reproduction uses `tests/wayland/sprites.py BINARY LIBRARY EVIDENCE` under
the workstation lock with the validated font directories, as preserved in
`native/check.sh` and `capture.sh`. The native Cargo binary is `aizome-hotaru`;
archived evidence renames it to `hotaru`.
