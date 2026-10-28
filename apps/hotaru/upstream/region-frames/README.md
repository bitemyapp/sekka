# Remote partial-scroll comparison evidence

See [the interpretation and limits](../../docs/region-frame-comparison.md).
`summary.json` retains each accepted trial separately. `raw.tar.gz` preserves
all three calibrations, six measurements, controller receipts/activity, input
writes, captures, logs and the two unsuccessful setup attempts. Generated
observer binaries/protocol sources and temporary lock files are excluded;
observer build receipts and original source remain. `manifest.json` hashes the
generated archive and frozen controller sources. `summarize.py` only reduces
existing evidence and verifies trial receipt hashes; it does not launch apps.

Full evidence remains at workstation
`/home/callen/work/hotaru-region-frames-20261028/` and local
`artifacts/hotaru-region-frames-20261028/`. Run new builds and measurements only
through `ssh wx-workstation`, under the shared Hotaru workstation lock. Copy
`compare.py` and `tools/` into a new directory before a new run; the controller
deliberately refuses to reuse trial directories. Its pinned packages, font
paths, calibration gates, CPU roles and explicit pixel-copy control are part
of the recorded experiment and must not be silently replaced.
