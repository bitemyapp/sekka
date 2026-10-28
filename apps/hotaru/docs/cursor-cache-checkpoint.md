# Cursor and shaped-row cache checkpoint

This checkpoint combines native cursor shapes and blinking, bounded widget
timers, immutable shaped-row reuse, unchanged grapheme-charge avoidance and
direct recycled-row construction. It remains a partial Linux port; the
whole-application Ghostty efficiency requirement is still open.

## Exact package and source

Built on `ssh wx-workstation` from the frozen Sekka worktree:

- Output: `/nix/store/26jmmbqz0m6j903y1vm3c1d9si3b7j41-hotaru-0.1.0`
- Prepared source: `/nix/store/bsxc055gdzr4p5kfsp4fwsrll7s2cqqw-hotaru-source`
- Source NAR: `sha256-+c6KYTAxixMUw4jbvtT8ZXmpBvOUebS163iLONmJrS8=`
- ELF SHA-256: `e3828214ade4069f23ae439c266830ca5dfb96bae33d67b006fcde35d32258be`
- Wrapper SHA-256: `b6d9c1a7b595d83e1aedb53bd54ed9e403015a695c6b960da57d12442164213c`

[`nix-cursor-cache-checkpoint.json`](../upstream/nix-cursor-cache-checkpoint.json)
records the derivation, build-log identity, release profile, installed files and
test summaries. Nix passed 44 native tests, with two timing diagnostics and five
environment-sensitive integrations explicitly ignored by that invocation.

All 206 source-manifest entries match the local worktree, including the expanded
Kit source/test inventory. Of these, 201 match the prepared package source;
the five intentional omissions are the standalone core lockfile and four
Sidestep patches already applied by Nix. Installed resources, both fallback
fonts and their OFL notices, six application notices, ELF runtime dependencies
and wrapper configuration were verified. The
[runtime report](../upstream/native-cursor-cache/package-runtime.json) preserves
these comparisons. The output and original prepared source have workstation GC
roots under `/home/callen/work/hotaru-cursor-cache-qualification-20261028/`.

## Behavior and integration checks

Six independent private headless Sway/pixman suites used the exact installed
package with ambient font/library overrides cleared:

| Suite | Observed behavior |
| --- | --- |
| Clipboard/input | Exact 1,151,999-byte plain copy, 1,152,060-byte HTML copy and 1,150,000-byte paste; stalled/cancelled readers, 16-request closed-pane race, split lifecycle and native keyboard/mouse input |
| Fonts | CJK, emoji and combining text at scales 1, 1.25 and 2, then return to scale 1 |
| Find | Retained-history matches, navigation/wrapping, copied match and query replacement |
| Shell | Packaged hooks, local working-directory inheritance including spaces/Japanese text, and installed terminfo |
| Links | Literal OSC 8 URI as one opener argument, wide-cell tail activation, rejected/stale gestures and visible opener failure |
| Cursor | Distinct steady block/underline/bar, both blink phases, configured default reset, inactive hollow block and refocus |

Compact receipts are in [`upstream/native-cursor-cache/`](../upstream/native-cursor-cache/).
The block cursor, inactive hollow cursor and fractional-scale CJK/emoji captures
were also visually inspected. Original-resolution images and complete logs are
retained locally in `artifacts/hotaru-cursor-cache-qualification/` and remotely
in the qualification directory above.

The isolated cursor/Kit slice passed 41 native tests, 160 Kit tests plus three
doctests, and 97 Hako tests. All 21 direct Kit consumers compiled with all targets.
These checks cover concrete-type identity changes in keyed, indexed/unkeyed and
duplicate-key fallback nodes; old state and timers end together when a type
changes. Both keyed and unkeyed regressions were observed failing before their
fixes and passing afterward. The combined source also passed four font-cache
correctness tests and strict all-target package Clippy with the test allocation
probe enabled. The ignored timing benchmark was deliberately not run under
unrelated compiler load. Exact counts, scope and log hashes are in
[`validation.json`](../upstream/native-cursor-cache/validation.json).

The separate standalone core passed 136 tests in both debug and release,
strict workspace Clippy, and 6,174 comparisons against the pinned Ghostty
library: 444 earlier cases, 3,462 VT-control comparisons and 2,268 cursor
comparisons. Two real-shell helpers are explicitly ignored in that invocation;
the packaged shell suite exercises the native integration separately.
Its source manifest is
`b9fcaa0ced20a2212e21685002a6da7b1fda89d3443394fe9afe82507c66ec3b`, and release
replay SHA-256 is
`82d9762f258059c84b78b71b62f67977055b29f3e94c544361d3d56f50b2a933`.
[The core report](../upstream/combined-core-cursor/summary.json) records all
declared comparison fields and exclusions. This replay is distinct from the
packaged GUI executable.

## Performance evidence and limits

The isolated [row-cache measurements](renderer-row-cache.md) reduce plain
single-cell and one-row-scroll preparation by roughly 15–33 times relative to
the previous Hotaru algorithm; full replacement is approximately unchanged.
This measures shaping preparation only, excluding rasterization, presentation
and frame retirement, and is not a Ghostty application comparison.

The selected [grapheme-charge experiment](grapheme-charge.md) still trails
Ghostty by about 20–21% on ASCII feed and 6–7% on mixed Unicode, while leading
on styled ASCII and cursor-operation streams. Those zero-history core timings
are provisional because unrelated builds were active; they do not measure
keyboard latency. [Recycled-row construction](history-row-construction.md)
removes temporary allocations with exact-content checks, but its timing is
likewise unqualified. No new combined-package timing claim is made here.

Mapped-history prototypes remain excluded after measured ingestion/restore
regressions. Native GPU presentation, software/hardware frame timing, startup,
idle resources, history efficiency, broader rendering compatibility and real
Aizome desktop qualification remain open. The frame harness and original
[renderer audit](renderer-audit.md) define the next measurement work.
