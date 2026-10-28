# First matched software GUI pilot

The [combined sprite/scrolling package](sprites-checkpoint.md), from product
commit `73ea25d`, was compared with unchanged Ghostty
`6ba144158577d033ccb6ed4b574e4013957c9161`. Four alternating startup pairs and
one sustained pair passed all measurement gates on `wx-workstation`.
This small software-rendering pilot is not whole-application acceptance.

Both applications use a 120×40 grid, 11×24 physical cells, 14-point IBM Plex Mono,
the same fallback fonts, zero history and a private 1920×1440 Sway/pixman output.
Each trial has fresh application configuration/cache directories; kernel page
cache is not cleared. Ghostty uses Mesa llvmpipe; Hotaru uses native CPU
rasterization and SHM. This limits conclusions about hardware-accelerated Ghostty.
Application CPUs are 8–11, compositor 12–13, observer 14, controller 15 and PTY
fixture 16. The workstation lock excludes our builds; no external compiler
overlaps any accepted interval. Periodic observations cannot prove exclusive
host use. Exact executable, wrapper, font, configuration and geometry gates pass.

## Startup and keyboard observations

| Measurement | Hotaru | Ghostty |
| --- | ---: | ---: |
| Median launch to PTY ready, four trials | 52.6 ms | 468.7 ms |
| Median launch to matching capture, four trials | 74.1 ms | 678.8 ms |
| Keyboard injection to matching capture, p50 of 500 | 25.9 ms | 46.2 ms |
| Keyboard injection to matching capture, p95 | 34.5 ms | 54.7 ms |
| Keyboard injection to matching capture, p99 | 35.0 ms | 56.6 ms |

Startup capture ranges are 73.45–74.73 ms and 678.33–679.20 ms; keyboard maximums
are 35.31 ms and 85.10 ms. Keyboard timing begins at a successfully flushed
virtual-keyboard event, passes through the real PTY, and ends at a capture with
matching complete top/bottom markers. It includes compositor/capture scheduling,
not physical input or scanout. Marker CRC checks sequence identity, not all text
pixels.

## Sustained output, overhead and memory

Each stream writes 120 updates. Both applications expose all 120 marker updates
and sparse text updates at 30 and 60 Hz, and all 120 scrolling updates at 30 Hz.
At **60 Hz scrolling**, the observer sees **114/120 Hotaru sequences versus
120/120 Ghostty sequences**. Both producers take about 1,983 ms; final
write-to-capture intervals are 33.41 ms and 29.45 ms. All captures have valid
complete markers. Median capture cadence is 17.78 ms versus 16.58 ms, with six
unobserved Hotaru sequences roughly every 16 updates. Application, compositor
and screencopy costs need investigation; this does not establish lost input.

Unpaced 120-update bursts expose one unique final sequence in Hotaru and two
sequences in Ghostty. Both reach the correct final marker. Intermediate
coalescing here is not a throughput or correctness failure.

Four 30-second static controls alternate capture off/on twice. Hotaru uses
0/10/0/0 ms of process CPU; Ghostty uses 1,860/1,880/1,850/1,890 ms. Accounting
resolution is 10 ms, so zero does not prove zero work. During active capture,
observer CPU is 50 ms per interval for either app; compositor CPU is 510 ms for
Hotaru versus 560–570 ms for Ghostty. These controls quantify overhead without
subtracting it from latency samples.

Static process RSS is about 61.3 MiB versus 324.3 MiB in this software-rendered,
zero-history fixture. It excludes separate compositor/observer processes and
does not demonstrate retained-history memory parity. The earlier history RSS
deficit remains unresolved. CPU costs and memory for every stream are preserved.

## Evidence and limits

The [summary](../upstream/gui-pilot-sprites/summary.json) retains every startup
sample and sustained control/stream. Compressed raw trial archives include all
keyboard samples, captures, PTY writes, activity observations, logs and final
images. Exact calibration and controller/observer sources accompany them in
[`upstream/gui-pilot-sprites/`](../upstream/gui-pilot-sprites/). Full directories
remain in local `artifacts/hotaru-sprites-frame-20261028/` and workstation
`/home/callen/work/hotaru-sprites-frame-20261028/`.

One sustained pair and four startup pairs cannot establish statistical parity,
a universal advantage or the causal effect of an optimization. Hardware app
rendering, physical presentation, broader workloads, history memory and the
60 Hz scrolling observation still require work. These GUI results are distinct
from the separate core-feed measurements.
