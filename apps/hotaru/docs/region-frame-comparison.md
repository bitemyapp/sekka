# Partial-scroll software comparison

The combined 434-sprite/region-scroll package was compared with its pixel-copy
path explicitly disabled and with unchanged Ghostty. All three calibrations
and six trials passed on `wx-workstation`: two rounds ordered
copy/redraw/Ghostty, then Ghostty/redraw/copy. This is a small software-rendered
comparison, not whole-application performance acceptance.

Pixel copying reduced application CPU during the scroll fixture, but did not
change the 60 Hz capture gap. Both Hotaru modes exposed 114 of 120 sequences in
each round; Ghostty exposed all 120. This rules out pixel-copy cost alone as
the explanation. Frame scheduling and compositor/capture interactions still
need attribution; an unobserved intermediate sequence does not prove lost input.

| Observation, round 1 / round 2 | Hotaru copy | Hotaru redraw | Ghostty |
| --- | ---: | ---: | ---: |
| Key injection to capture, median of 64 | 24.90 / 24.83 ms | 24.81 / 24.86 ms | 47.18 / 46.34 ms |
| Key injection to capture, p95 | 34.34 / 34.41 ms | 34.07 / 34.46 ms | 52.92 / 52.61 ms |
| Launch to matching capture | 73.99 / 74.23 ms | 74.29 / 74.09 ms | 662.60 / 663.01 ms |
| 60 Hz scroll sequences observed | 114 / 114 | 114 / 114 | 120 / 120 |
| 60 Hz scroll application CPU | 70 / 70 ms | 200 / 200 ms | 4530 / 4590 ms |
| 30 Hz scroll application CPU | 120 / 110 ms | 240 / 230 ms | 4540 / 4570 ms |

Each stream writes 120 updates, taking approximately two seconds at 60 Hz or
four seconds at 30 Hz. Process CPU sums threads and has 10 ms resolution.
The copying path therefore uses about 65% less process CPU than forced redraw
in this short 60 Hz fixture. That is not a whole-application speedup. All marker
and sparse 60 Hz streams expose all 120 sequences; Ghostty's second sparse
30 Hz trial exposes 119. All runs reach the correct final marker, including
unpaced bursts (one observed Hotaru sequence, two Ghostty sequences). Raw
samples preserve these outcomes without selecting only favorable streams.

Both applications use a 120×40 grid, 11×24 physical cells, 14-point IBM Plex
Mono, identical fallback fonts, zero history and a private 1920×1440 Sway/pixman
output. Hotaru uses native CPU rasterization and SHM. Ghostty uses llvmpipe;
its logs report failed DMA-BUF exports and fallback to CPU presentation. This
does not represent Ghostty's normal hardware presentation path. Timings include
virtual input, the real PTY, compositor and capture scheduling, not physical
input or scanout. The complete-marker CRC validates identity, not all pixels.

Application CPUs are 24–27, compositor 28–29, observer 30, controller 31 and
fixture 32; their SMT siblings and activity samples are recorded. No compiler
overlap was observed during accepted intervals. A preceding attempt was
interrupted while waiting for the shared lock, before launching applications;
its proposed environment-only pixel switch would have been sanitized. The
canonical harness now takes and records an explicit `--no-pixel-scroll` option.
A second attempt rejected a busy CPU 8 during preflight. The accepted attempt
used available cores instead of waiting for the whole workstation to be idle.

Exact executable, wrapper, font, configuration and startup/final geometry
checks passed; all child processes were reaped. The Hotaru ELF is
`1d4e649540eed06b5ec4b43e756e1d9b5b8bdbb9a715487903f3613963d0b262`,
the [region package](region-checkpoint.md), before the later triangle expansion.
Ghostty's ELF remains
`ea356b4e5f33e0d0d55af99bb978afe419eed2a933edd31e457554db8970f61f`.

The [summary](../upstream/region-frames/summary.json) and compressed raw evidence
retain all keyboard events, stream/control counters, activity, captures, logs,
calibrations and setup failures. The one-second static controls and 64-key
samples are too short for strong idle-CPU or tail-latency conclusions. Earlier
[GUI pilot](gui-pilot-sprites.md) measurements use different CPUs and an older
153-sprite package and remain separate. Hardware rendering, retained-history
memory and broader feature/performance parity remain open.
