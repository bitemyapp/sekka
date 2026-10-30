# Native links and VT compatibility checkpoint

This checkpoint combines snapshot-bound OSC 8 activation, a bounded desktop
URL opener, REP and supported DEC mode save/restore, corrected alternate-screen
transitions, decoded-C1 handling and the repaired SIMD UTF-8 dependency.
It remains a partial Linux port with substantial feature and efficiency work.

## Exact package and source

Built on `ssh wx-workstation` from the frozen Sekka worktree:

* Output: `/nix/store/53x0b2m9aafkcaxzlw91srad9yrnzg8k-hotaru-0.1.0`
* Prepared source: `/nix/store/x38xicnnqrlyaddw55wpmg660nn175aw-hotaru-source`
* Source NAR: `sha256-qfGgABpPvIvsG0zbshdk4ZyBoCbwIK/MSruSyFNP0RM=`
* ELF SHA-256: `bd9199bf737568e494e0e936e901f94da571608a7d573d6eb36950a60f883f3d`
* Wrapper SHA-256: `b4165a2cd0132729f52ae209c7904ad29378a86ddbdc90859beaa0f0e940f11f`

[`nix-links-vt-checkpoint.json`](../upstream/nix-links-vt-checkpoint.json)
records the derivation, build log identity, release profile, installed files
and package test summaries. The Nix build passed 38 ordinary native tests;
six environment-dependent tests are explicitly ignored there.

All 140 source-manifest entries match the local worktree. Of these, 135 match
the prepared source directly. The five intentional omissions are the separate
core workspace lock and the four Sidestep patch inputs already applied by Nix.
Seven installed shell/terminfo resource files, CJK and emoji fonts with OFL
notices, six application notices and ELF runtime dependencies were verified.
The [runtime report](../upstream/native-links-vt/package-runtime.json) retains
the complete comparisons.

## Native qualification

Five independent private headless Sway sessions used the exact installed
package and the software pixman renderer. Their small JSON receipts are in
[`upstream/native-links-vt/`](../upstream/native-links-vt/).

| Suite | Observed behavior |
| --- | --- |
| Clipboard/input | Exact 1,151,999-byte plain copy, 1,152,060-byte HTML copy and 1,150,000-byte paste; stalled and cancelled readers; closed-pane copy race; split lifecycle and native keyboard/mouse input |
| Fonts | CJK, emoji and combining text at scale 1, 1.25 and 2, returning to scale 1 |
| Find | 120 retained-history matches, forward/backward/wrapped navigation, copied match and query replacement |
| Shell | Packaged hooks, working-directory inheritance including spaces/Japanese text, and installed terminfo |
| Links | Exact literal OSC 8 URI delivered as one opener argument, wide-cell tail activation, cancelled gestures/stale frame/invalid text, visible opener failure with a stationary pointer |

The final link screenshot was visually inspected: the link remains visible
and underlined, and the status bar displays the opener error while the pointer
stays over the link. Full original-resolution captures and logs are retained
locally under `artifacts/hotaru-combined-checkpoint-nix/` and remotely under
`/home/callen/work/hotaru-combined-qualification-20261028/`.

## Core qualification and limits

The standalone release replay has SHA-256
`37081f680a4632d6bfdee8f0fcec8e07623b3ac4ef860928c73085687155533b`, built from
source manifest `a3e74ef33281a79744da2fa15e63d019699950490bbcc94eef783997391ca128`.
This is a separate native-CPU benchmark executable, not the packaged GUI ELF.
Its [build and validation records](../upstream/vt-controls/combined/) contain
108 core tests, 23 ordinary I/O tests, the replay fixture test and strict
workspace Clippy. Two real-shell helper tests are explicitly ignored in that
run; shell behavior is exercised separately by the packaged suite above.

The exact replay passes 3,462 new [VT-control comparisons](vt-controls.md)
and 444 earlier corpus comparisons against the pinned real Ghostty library.
The comparisons cover their declared fields, not every protocol or effect.
Paired Rust diagnostics improve dense Han processing by about 15%, while mixed
Unicode grows by about 7–10% and cursor processing by about 5% compared with
the earlier extent-cache baseline. These are recorded regressions, not a claim
of general improvement. Source and binary hashes accompany every measurement.

The reviewed 8 MiB, zero-history feed workloads below use the same pinned
Ghostty library, one CPU, alternating engine order and 20 samples per engine.
Each workload passed its declared state/effect comparison before timing. Ratios
are Hotaru time divided by Ghostty time; below one is faster. The Unicode-aware
reference adapter and its exact build metadata are retained alongside these
reports. An initial invocation with the older adapter rejected the Unicode
option; it produced no Unicode result and was replaced by this complete run.

| Workload | Legacy width | Unicode width |
| --- | ---: | ---: |
| ASCII | 1.246 | 1.221 |
| Mixed Unicode | 1.181 | 1.155 |
| Styled ASCII | 0.830 | 0.833 |
| Cursor operations | 0.120 | 0.120 |

ASCII and mixed Unicode are still slower than Ghostty. These four narrow
patterns exclude history, graphics and rendering, and do not establish
whole-terminal performance parity.

Software-compositor tests do not qualify real Aizome presentation, hardware
frame pacing, startup/idle resource use, input latency or GPU behavior. History
memory/throughput parity and many terminal features remain open. The separate
page-owner experiments are not part of this packaged checkpoint.
