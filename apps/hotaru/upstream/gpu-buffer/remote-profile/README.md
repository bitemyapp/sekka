# Remote scene phases and contiguous indexing

This is an isolated diagnostic over the exact native-scene snapshots committed
in `d8b63a7`. Production Hotaru remains CPU/SHM.

`compute.patch` applies to `../native-scene/source/`; `preparation.patch` applies
to `../native-scene/preparation/source/` in the complete isolated tree described
there. The patches include the cancellation/cache bounds and contiguous index.
`*-sources.json` records exact resulting source hashes. The original Intel
source and results are unchanged.

`summary.json` contains all per-scene phase distributions, current and original
preparation medians, runtime scope and validation counts. `raw/*.json.gz` retains
every original sample, loader mapping, source/ELF hash and cleanup event. Its
uncompressed and compressed hashes are both recorded. `initial-render-node-mismatch.json`
preserves the first unsuccessful selector attempt; the subsequent explicit
EGL-device selection is diagnostic, not a host permission change.

The standalone runs render 18 scenes 21 times in each of two modes: attempted
export and skipped export. Export remains unavailable on NVIDIA. All 756 images
match exactly, all cleanup records succeed, and processes are reaped. Strict
compute Clippy passes. CPU/prepare profiling uses the same original RGBA fixture
files, a warm banded CPU glyph cache and full transparent destination clearing.
It is not a damaged-frame or Ghostty application comparison.

The task lock serialized these runs, but there was no exclusive-load preflight
or CPU-affinity acceptance gate. Treat timing distributions as investigation,
not a speed guarantee. Readback and CPU verification are diagnostic-only costs.
The source patch does not add a new production backend.

Full mirrors, executables and logs remain on wx-workstation at
`/home/callen/work/hotaru-gpu-profile-isolated` and
`/home/callen/work/hotaru-gpu-preparation-profile`. `profile.py` and
`preparation-profile.sh` retain those explicit task paths. All commands must run
on wx-workstation under `/home/callen/.cache/hotaru-workstation.lock`; no local
build, test or benchmark is part of this slice.

Independent review found no concrete tile-index bounds/order, cancellation or
offscreen-descriptor lifetime defect for validated scenes. The known raw versus
padded admission mismatch is documented and intentionally left visible for the
new full-scene fallback bridge, rather than silently reclassifying that failure.
