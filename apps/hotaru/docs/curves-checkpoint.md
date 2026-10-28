# Rounded box and Powerline package checkpoint

The 178-sprite package adds seven rounded/diagonal box characters and all 18
geometric Powerline shapes from pinned Ghostty. It also includes the decorated
block-cursor correction. Its source is base `d607319` plus the qualified
[sprite candidate](../upstream/sprites/curves/summary.json); all 16 candidate
product hashes and all 233 runtime source-manifest entries match the worktree.

The package output is `/nix/store/ski9m1d1a86gwyxdbi82rssgkc7knp7p-hotaru-0.1.0`.
Its ELF SHA-256 is
`6b1e967e030942440c37155239716e3d637201fe880ae461eec833c11b880f1d`.
The prepared source is `/nix/store/4km5ikqc67ij0ng015p92k203s1j60a5-hotaru-source`;
227 source entries match, with only the standalone lockfile and five already
applied Sidestep patches omitted intentionally. Both paths have workstation
GC roots under `/home/callen/work/hotaru-curves-qualification-20261028/`.
The [Nix record](../upstream/nix-curves-checkpoint.json) retains the derivation,
source NAR, wrapper hash and installed-file inventory. Nix passed 57 tests;
seven explicitly ignored integrations/diagnostics retain their prior scope.

All eight native private-Sway suites passed against that exact installed
wrapper, with ambient font and library overrides cleared: clipboard/input,
fonts, Find, shell integration, links, cursor, pixel scrolling and sprites.
The clipboard suite verified the same three transfers over 1 MiB, cancelled
and stalled readers, closed-pane races and split/input behavior. All 28 scroll
captures exactly equal full-redraw RGBA captures. All 712 sprite instances
(178 at scales 1, 1.25, 2 and 1) match the unchanged Ghostty raster oracle,
comparing 2,294,064 RGB bytes. The fractional-scale capture was also inspected.

The vector implementation is safe Rust with bounded, fallible scratch storage;
it introduces no runtime dependency or lockfile change. The port retains the
MPL-2.0 z2d source license and complete z2d/Cairo notices alongside Ghostty's
MIT notice. Both Nix packages install all eight application notices. The two
new license files retain upstream bytes, including their original whitespace.
The [sprite audit](sprites.md) documents the 57,700 exact mask comparisons,
translation-order regression, resource bounds and remaining families.

This supersedes the earlier 153-sprite package only for component qualification.
It does not replace the frozen binary used in the [software GUI pilot](gui-pilot-sprites.md).
New padded diagonal/Powerline ink can require full-terminal damage and reject
pixel reuse; this efficiency cost remains open. Production GPU rendering,
history memory, complete Ghostty compatibility, physical-desktop qualification
and whole-application performance parity are still incomplete.

Compact [runtime evidence](../upstream/native-curves-checkpoint/package-runtime.json)
and [validation hashes](../upstream/native-curves-checkpoint/validation.json)
are committed. Full evidence remains in local
`artifacts/hotaru-curves-qualification-20261028/` and the workstation directory
above. The earlier package and its reports remain unchanged.
