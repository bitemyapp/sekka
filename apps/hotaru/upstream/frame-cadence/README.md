# Frame-cadence diagnostic

See [the attribution report](../../docs/frame-cadence.md). No product change
is proposed by this archive.

`summary.json` records accepted and rejected observations. `raw-traces.tar.gz`
contains all three diagnostic setup/run directories, complete raw logs and
screenshots, copied harness sources, manifests, and upstream source identities.
Generated observer build artifacts are excluded. `archive-manifest.json`
records the archive and analysis hashes.

To reproduce analysis on `wx-workstation`, extract the archive into a new
directory, then run `python3 analyze_trace.py <directory>/trace-434-v2` and
the corresponding command for `trace-format-434`. The archived `compare.py`
controllers reproduce runs only where their exact Nix packages are present;
they acquire `/home/callen/.cache/hotaru-workstation.lock` and verify executable,
font, configuration, geometry and observed compiler-interference gates.

All builds, tests and measurements belong on `wx-workstation`. The raw original
root is `/home/callen/work/hotaru-frame-cadence-20261028`. Initial failed setup
and externally overlapped trials remain in the archive and must not be
relabeled as accepted measurements.
