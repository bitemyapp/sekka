# Hotaru against Ghostty, 2026-10-30

Hotaru `2f2f73e0` was compared with unchanged Ghostty
`6ba144158577d033ccb6ed4b574e4013957c9161` in three ways:

1. The core feed comparison against the pinned libghostty-vt on the reviewed
   corpus.
2. The matched software GUI pilot.
3. Ghostty's own benchmark suite.

Everything ran on `wx-workstation` (Threadripper PRO 9985WX) under the shared
workstation lock, with load averages between 0.6 and 1.2 where the tools
record them. No trial was rejected,
and no compiler ran during any measured interval. These are measurements of
named builds in named conditions, not whole-application acceptance; see the
limits under each section.

The builds involved:

- **Hotaru package:** `/nix/store/mgqvnfa0a52dl9hd563jl4ga1hr1k2wc-hotaru-0.1.0`.
- **Native `hotaru-replay`:** `36f856f6…`. Rust 1.95.0, `-C target-cpu=native`,
  thin LTO; `upstream/candidate-clipboard-build.json`.
- **Pinned library and reference adapter:** `70e1bdcf…`.
- **`ghostty-bench`:** `bcdaba09…`, ReleaseFast. **`ghostty-gen`:** `8a482e3e…`.

## Core feed on the reviewed corpus

`tools/benchmark.py`, unchanged:

- Each of the four reviewed 8 MiB patterns runs on CPU 8, in 4 rounds of 5
  timed feeds after warmups, alternating engine order.
- History is zero and width is legacy.
- The final terminal states matched for every pattern before timing.
- Reports: `upstream/benchmarks/clipboard-checkpoint-*.json`.

| Pattern | Ghostty | Hotaru | Hotaru ÷ Ghostty | Hotaru at the rich-copy checkpoint |
| --- | ---: | ---: | ---: | ---: |
| ASCII | 6.44 ms | 5.66 ms | 0.878 | 7.12 ms (1.115) |
| Styled text | 32.65 ms | 27.70 ms | 0.848 | 19.32 ms (0.606) |
| Unicode | 30.90 ms | 33.99 ms | 1.100 | 35.27 ms (1.143) |
| Cursor/erase | 751.3 ms | 92.0 ms | 0.122 | 76.85 ms (0.102) |

Ghostty's times are within 3 % of the 2026-10-28 runs. Hotaru's ASCII is now
faster than the reference, and Unicode is a little faster than at the rich-copy
checkpoint. **Styled text (+43 %) and cursor/erase (+20 %) are slower than at
the rich-copy checkpoint.** The commits between them haven't been bisected. The
Unicode-width mode (DEC 2027) that native panes use was not measured here; the
[margins evidence](../upstream/horizontal-margins/README.md) records it 12 %
slower than the previous head on mixed Unicode.

## Matched software GUI pilot

This is the [first pilot](gui-pilot-sprites.md)'s method, unchanged:

- **Setup:** private Sway/pixman at 1920×1440, 120×40 cells of 11×24 pixels,
  14-point IBM Plex Mono with the same fallbacks, zero history, fresh
  configuration per trial.
- **CPUs:** apps on 8–11, compositor 12–13, observer 14, controller 15, fixture 16.
- **Calibration:** Hotaru's package was newly calibrated (1349×1077, the same
  geometry as before). Ghostty's binary is unchanged, so the first pilot's
  calibration was reused.
- **Trials:** four alternating startup pairs and two sustained pairs (the first
  pilot had one).

Summary, calibration and compressed raw trials:
[`upstream/gui-pilot-clipboard/`](../upstream/gui-pilot-clipboard/).

| Measurement | Hotaru | Ghostty |
| --- | ---: | ---: |
| Launch to PTY ready, median of four | 53.0 ms | 465.9 ms |
| Launch to first matching capture, median of four | 74.2 ms | 678.7 ms |
| Keyboard injection to capture, p50 (500 per trial) | 25.6 / 25.5 ms | 45.6 / 45.5 ms |
| p95 | 34.5 / 34.5 ms | 53.1 / 53.2 ms |
| p99 | 35.0 / 35.0 ms | 56.2 / 54.5 ms |
| Max | 35.3 / 35.3 ms | 57.6 / 61.3 ms |
| App CPU over each 30-second static control | 0 ms (10 ms resolution) | 1,850–1,890 ms |
| App RSS at rest | 61 MiB | 314–329 MiB |
| App CPU per 120-update stream | 40–120 ms | about 4,500 ms |

Both apps expose all 120 updates of the marker and sparse streams at 30 and
60 Hz, and of the scroll stream at 30 Hz.

**At 60 Hz scrolling the observer again sees 114/120 Hotaru sequences** in both
trials, against 120/120 for Ghostty, as in the first pilot. The compositor
spends 230–260 ms of CPU on Hotaru's scroll streams, against 130–180 ms for
Ghostty's.

Ghostty runs on Mesa llvmpipe with CPU presentation here, not its hardware path,
which inflates its CPU and memory. Hotaru's CPU rasterization is its normal path.
Captures end at observer receipt, not physical scanout.

## Ghostty's own benchmark suite

This follows Ghostty's `src/benchmark/AGENTS.md`:

- `ghostty-gen` made each input once, as a 64 MiB file, from the generators'
  own documented examples:
  - plain unbroken ASCII, which wraps at the margin;
  - the four `styled` variants: plain lines, heavy styling, Unicode-heavy, and
    mixed with OSC 8 hyperlinks;
  - random UTF-8;
  - random OSC.
- Kitty graphics was left out: Hotaru doesn't implement it.
- `hyperfine` timed whole commands, with 3 warmups and 20 runs, pinned to CPU
  8, at 120×80:
  - `ghostty-bench +terminal-stream --data=FILE`;
  - `hotaru-replay --bench --data FILE`.
- Before timing, each file's final screen was compared between the pinned
  library and `hotaru-replay`.
- Ghostty's stream benchmark keeps its terminal's default history (10,000
  bytes, at least one page); Hotaru ran with zero and with 1,000 rows of
  history to bracket that.

Scripts, reports and `hyperfine` exports:
[`upstream/ghostty-suite/`](../upstream/ghostty-suite/).

Whole commands, median of 20:

| Input | Screens match | Ghostty | Hotaru, no history | Hotaru, 1,000 rows | Ratio (no history) |
| --- | --- | ---: | ---: | ---: | ---: |
| Empty file | yes | 0.88 ms | 0.65 ms | 0.65 ms | 0.74 |
| ASCII stream | yes | 30.6 ms | 40.4 ms | 48.0 ms | 1.32 |
| Styled: plain lines | yes | 64.0 ms | 88.2 ms | 102.9 ms | 1.38 |
| Styled: heavy styling | yes | 474.4 ms | 374.2 ms | 388.1 ms | 0.79 |
| Styled: Unicode-heavy | yes | 570.5 ms | 613.1 ms | 625.8 ms | 1.07 |
| Styled: mixed, hyperlinks | yes | 3,116.7 ms | 952.1 ms | 982.7 ms | 0.31 |
| Random UTF-8 | **no** | 555.3 ms | 678.2 ms | 725.0 ms | not equivalent |
| Random OSC | **no** | 3,762.7 ms | 1,553.4 ms | 1,586.3 ms | not equivalent |

A whole command includes reading the input. `ghostty-bench` streams the file,
but `hotaru-replay` reads all 64 MiB before feeding it. A feed-only cross-check
used the same files with the in-process adapters `tools/benchmark.py` uses: the
input is read before timing, history is zero, and each engine runs 4 rounds of
5 feeds in alternating order.

| Input | Ghostty feed | Hotaru feed | Hotaru ÷ Ghostty |
| --- | ---: | ---: | ---: |
| ASCII stream | 25.8 ms | 19.4 ms | 0.75 |
| Styled: plain lines | 59.7 ms | 69.8 ms | 1.17 |
| Styled: heavy styling | 575.6 ms | 356.6 ms | 0.62 |
| Styled: Unicode-heavy | 480.8 ms | 588.8 ms | 1.23 |
| Styled: mixed, hyperlinks | 1,376.2 ms | 937.1 ms | 0.68 |

About 20 ms of Hotaru's whole-command time on the plain inputs is the replay
tool's up-front read, not the terminal. Feed-only, **random plain lines (1.17)
and Unicode-heavy lines (1.23) are the real gaps**. The reviewed ASCII corpus
repeats one sentence and doesn't show the first. Ghostty's two configurations
differ widely on the mixed input: 3.1 s whole-command with its default history,
1.4 s through the zero-history adapter. Hotaru's 1,000 rows of history add 8–15
ms per 64 MiB on plain inputs.

**The random UTF-8 and random OSC inputs leave different screens**: correctness
differences that Ghostty's own generators turned up.

- **Random UTF-8:** the screens differ from the first cell, and the cursor ends
  on row 65 in Hotaru against row 79 in Ghostty.
- **Random OSC:** Hotaru prints replacement characters and stray bytes where
  Ghostty's top row is blank, so the two end some malformed OSC sequences
  differently.

Both were traced and fixed after this comparison, together with the related
parser, CSI and width rules that fuzzing and a CSI sweep found. All eight
inputs now leave the reference's screen; see
[stream-parser.md](stream-parser.md).

## Follow-up

- Bisect the styled (+43 %) and cursor/erase (+20 %) core slowdowns since the
  rich-copy checkpoint.
- Close the feed gaps on random plain lines and Unicode-heavy text, and measure
  mode 2027.
- Find where random UTF-8 and malformed OSC first diverge from the pinned
  library, then fix or name each difference. Done; see
  [stream-parser.md](stream-parser.md).
- Investigate the 60 Hz scroll capture shortfall and its compositor cost.
- Let `hotaru-replay --bench` stream its input, so whole-command comparisons
  with `ghostty-bench` stop counting a read Ghostty doesn't do.
