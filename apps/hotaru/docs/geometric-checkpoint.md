# Corner-triangle package checkpoint

The combined product at `7c4e28a` passed its release Nix build and all eight
native Wayland suites on `wx-workstation`. It includes 442 exact-raster sprites,
decorated cursors and bounded partial-region pixel reuse. This checkpoint adds
the eight [corner triangles](../upstream/sprites/geometric/README.md) to the
previously qualified package.

The output is `/nix/store/8nf099h9921x5jpgw625xvp7k72b3hns-hotaru-0.1.0`.
Its ELF SHA-256 is
`4e5aebd998ee33734c49321867b31570e1044c651798194eacefa2be1c6698b1`;
the wrapper hash is
`cf828e495187279001b0b2fd1734ca1f73ad67b26a469f1841531345cca5e094`.
The prepared source is `/nix/store/bwkn2hyavs53za6qbhf5hyb6mgvv99kw-hotaru-source`.
Both have workstation GC roots under
`/home/callen/work/hotaru-geometric-package-20261028/`.

Nix passed 62 native tests with seven existing explicit ignores. All 241 runtime
source-manifest entries match the integrated worktree; 235 match the prepared
source, with only the standalone lockfile and five applied Sidestep patches
intentionally omitted. Installed resources, fonts and all nine application
notices match, including the exact Zig MIT notice for the safe numerical helper.
The [package record](../upstream/nix-geometric-checkpoint.json) preserves the
derivation, source NAR and installed inventory.

Clipboard/input, fonts, Find, shell integration, links, cursor, pixel scrolling
and sprite suites all pass against the installed wrapper with ambient font and
library overrides cleared. Clipboard checks retain exact transfers over 1 MiB,
stalled/cancelled readers and closed-pane races. All 42 copy-versus-redraw
full-window RGBA pairs match. All 1,768 sprite instances at scales 1, 1.25, 2
and back to 1 match the pinned Ghostty ReleaseFast oracle: 5,696,496 RGB bytes
exact. The separate pure triangle and shared-vector gates use ReleaseSafe;
the earlier Braille assertion limitation remains documented with its raw failure.

The [runtime report](../upstream/native-geometric-checkpoint/package-runtime.json)
and [validation record](../upstream/native-geometric-checkpoint/validation.json)
retain exact identities and hashes. Full logs and captures remain in local
`artifacts/hotaru-geometric-package-20261028/` and the workstation directory
above. Earlier package identities and measurements are unchanged.

This is combined component qualification, not physical-desktop or performance
acceptance. The product still uses CPU/SHM rendering; GPU work is isolated.
The [latest frame comparison](region-frame-comparison.md) measures the preceding
434-sprite package and retains its unresolved 60 Hz scrolling gap. History
memory, complete feature coverage and whole-application parity remain open.
