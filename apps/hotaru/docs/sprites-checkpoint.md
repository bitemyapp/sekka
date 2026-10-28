# Native sprite and scrolling package checkpoint

This package combines the packed ASCII cell stores, native pixel scrolling and
153 Rust box/block sprites with the earlier cursor and shaped-row cache work.
It was built from product commit `73ea25d` on `ssh wx-workstation`.

## Package identity

- Output: `/nix/store/k8hj5hzc316icb0g64fbglrh6gpjfwh6-hotaru-0.1.0`
- Prepared source: `/nix/store/jgycq122glknsb59fi9bbmjlryy2c7zx-hotaru-source`
- ELF SHA-256: `64e7975b8d52e7c825544751640589eb7e44ed48165e88bce47c91865d241e86`
- Wrapper SHA-256: `3f93fb648898205338936899991fa32a8fc42d532e7f746592f2de9ce95c3774`

The [package record](../upstream/nix-sprites-checkpoint.json) includes its
derivation, source NAR, release profile and installed-file inventory. Nix
passed 56 native tests; two timing diagnostics and five environment-sensitive
integrations remain explicitly ignored by that invocation. The output and
prepared source have GC roots in
`/home/callen/work/hotaru-sprites-qualification-20261028/` on the workstation.

All 226 runtime source-manifest entries match the local worktree. Of these,
220 match the prepared source; the six intentional omissions are the standalone
Cargo lockfile and five Sidestep patches already applied by Nix. Installed
resources, fallback fonts and their OFL notices, six application license notices,
wrapper configuration and ELF dependencies were checked. The Sidestep gitlink
is unchanged; its modifications remain represented by the tracked patch series.

## Runtime checks

Eight independent private headless Sway/pixman suites passed against the exact
installed wrapper with ambient font and library overrides cleared:

| Suite | Evidence |
| --- | --- |
| Clipboard/input | Exact 1,151,999-byte plain copy, 1,152,060-byte HTML copy and 1,150,000-byte paste; stalled/cancelled readers, 16-request closed-pane race, splits and native keyboard/mouse input |
| Fonts | CJK, emoji and combining text at scales 1, 1.25 and 2, then return to scale 1 |
| Find | 120 retained-history matches, navigation/wrapping, copied match and query replacement |
| Shell | Installed hooks/terminfo and working-directory inheritance through spaces and Japanese text |
| Links | Literal OSC 8 opener argument, wide-cell tail activation, stale/rejected gestures and visible opener failure |
| Cursor | Distinct block/underline/bar, blink phases, configured reset, inactive hollow block and refocus |
| Pixel scrolling | All 28 captured frames exactly equal the corresponding full-redraw RGBA frames |
| Sprites | All 612 glyph instances across scales 1, 1.25, 2 and 1 match the pinned Ghostty raster oracle byte for byte |

The [runtime reports](../upstream/native-sprites-checkpoint/package-runtime.json)
and [validation record](../upstream/native-sprites-checkpoint/validation.json)
preserve exact hashes and source comparisons. The fractional-scale sprite and
final scrolling captures were visually inspected and retained at original
resolution. Complete logs and captures remain in local
`artifacts/hotaru-sprites-qualification-20261028/` and the workstation directory
above. The [sprite implementation audit](sprites.md) and
[pixel scrolling audit](pixel-scroll.md) describe their narrower oracle and
ownership checks.

These are component checks, not Aizome VM or physical-desktop qualification.
The cursor fixture does not cover decorated block-cursor text: a separate
source review identified missing decoration preservation/tint in that case,
which remains a follow-up for this frozen package. Additional sprite families,
the production GPU renderer, complete Ghostty compatibility, resident-memory
efficiency and whole-application performance parity remain open. The
[direct core measurements](ascii-cell-store.md) use a separate replay binary;
they are not timings of this packaged application.
