# Terminals: Hotaru against Ghostty, gnome-terminal, Konsole, GNOME Console and foot

Hotaru is Aizome's terminal. This compares it with Ghostty, with GNOME's
two terminals (gnome-terminal and its successor, Console) and KDE's
(Konsole), and with foot, Sekka's fallback terminal: how fast each starts,
how soon a keystroke shows, how much output it reads per second, what it
costs at rest and while typing, in CPU, wake-ups, GPU time, socket energy
and memory.

**Throughput measured again with history (2 November 2026, afternoon).** The
first throughput run gave every terminal zero scrollback, as the earlier
library-level comparison
([ghostty-comparison-2026-10-30.md](../../apps/hotaru/docs/ghostty-comparison-2026-10-30.md))
had. Mitchell Hashimoto, Ghostty's author, reran that comparison and
[pointed out](https://x.com/mitchellh/status/2117374562333872319) that zero
scrollback turns off a lot of libghostty-vt's optimizations, which is fair
for the alternate screen but not for output on the normal screen (where
these corpora are written), and that Hotaru gets slower as its history
grows ([the post, kept here](data/terminals/ghostty-author-2026-11-02/README.md)).
So those figures favoured Hotaru twice over. Throughput is now measured again
with history in every terminal: Ghostty with its defaults (50 MB), Hotaru
with the same 50 MB, and the four terminals whose history is counted only in
lines with 1,000,000 lines, Hotaru's row limit. Ghostty's main branch at
`0b1729e` joins the release Sekka installs (1.4.0). See
[Throughput with history](#throughput-with-history); the zero-scrollback
figures are kept, labelled as such. Nothing else here depends on history:
startup, typing latency, the paced streams (which scroll inside a scroll
region, so no line reaches history) and idle never fill it.

Measured on runnymede on 2 November 2026 ([the machine](README.md#the-machine)),
every terminal in its own private headless sway on the processor's
integrated Radeon. These are component measurements on one machine and one
compositor, timed to the compositor's captured frame: no physical display
or photon is involved.

## In short

| | Hotaru | Ghostty | gnome-terminal | Konsole | Console | foot |
|---|---:|---:|---:|---:|---:|---:|
| Launch → first frame | **50 ms** | 178 ms | 130 ms | 130 ms | 130 ms | 66 ms |
| Key → frame, p50 / p99 | 26.8 / 35.6 ms | 30.0 / 38.0 ms | **11.6** / 20.2 ms | 21.2 / 29.9 ms | **11.6** / 20.3 ms | 11.8 / **20.1** ms |
| ASCII output, no scrollback | 1,047 MiB/s | 1,018 MiB/s | 407 MiB/s | 74 MiB/s | 418 MiB/s | 441 MiB/s |
| Styled output, no scrollback | 348 MiB/s | 258 MiB/s | 216 MiB/s | 86 MiB/s | 256 MiB/s | 431 MiB/s |
| Unicode output, no scrollback | 188 MiB/s | 215 MiB/s | 198 MiB/s | 93 MiB/s | 193 MiB/s | 171 MiB/s |
| Cursor/erase output, no scrollback | 127 MiB/s | 22 MiB/s | 89 MiB/s | 90 MiB/s | 95 MiB/s | 99 MiB/s |
| Energy for 64 MiB of ASCII over idle, no scrollback | 2.0 J | 2.5 J | 2.9 J | 12.3 J | 2.4 J | 2.9 J |
| Idle wake-ups/s, blink off | 1.0 | 28.5 | **0** | **0** | 2.6 | **0** |
| Idle wake-ups/s, out of the box | 7.7 (blinks) | 28.5 (blinks) | 0.5 | **0** | 4.6 | **0** |
| Memory (PSS) after start | 47 MiB | 98 MiB + 103 MiB GPU | 21 MiB | 72 MiB | 48 MiB + 36 MiB GPU | **7 MiB** |

Bold is the best in its row (the no-scrollback output rows aren't marked:
see above). Medians; the sections below give the spread, the number of
trials and the other measurements.

With history in every terminal (50 MB for Hotaru and Ghostty, 1,000,000
lines for the rest; six rounds each; [details](#throughput-with-history)):

| | Hotaru | Ghostty 1.4.0 | Ghostty main `0b1729e` | gnome-terminal | Konsole | Console | foot |
|---|---:|---:|---:|---:|---:|---:|---:|
| ASCII output, MiB/s | 705 | 720 | **794** | 230 | 3.3 | 232 | 146 |
| Styled output, MiB/s | 301 | 271 | **346** | 156 | 0.7 | 163 | 334 |
| Unicode output, MiB/s | 165 | 72 | **251** | 88 | 0.6 | 88 | 151 |
| Cursor/erase output, MiB/s | 126 | 22 | **181** | 25 | 90 | 24 | 104 |
| Energy for 64 MiB of ASCII, over idle | **2.8 J** | 3.4 J | 2.9 J | 4.3 J | 376 J | 4.2 J | 5.4 J |
| Memory (PSS) after the four floods | 94 MiB | 109 MiB | 120 MiB | **38 MiB** | 942 MiB | 69 MiB | 1,523 MiB |

## What it shows

- **Hotaru starts fastest.** Its first frame shows 50 ms after launch:
  foot's comes a frame later, gnome-terminal's, Konsole's and Console's
  at 130 ms, Ghostty's at 178 ms, with warm caches all round.
- **With history, Ghostty's main branch reads every corpus fastest, and
  Hotaru is next on Unicode and cursor/erase output.** Ghostty main reads
  794, 346, 251 and 181 MiB/s (ASCII, styled, Unicode, cursor/erase);
  Hotaru 705, 301, 165 and 126, which is 66–89% of main's speed. Hotaru
  is third on ASCII, behind both Ghostty builds, and third on styled text,
  behind Ghostty main and foot (334). Against the Ghostty release Sekka
  installs (1.4.0), Hotaru is about even on ASCII and ahead on the rest,
  most of all on cursor/erase output (126 against 22) and Unicode (165
  against 72), the cases Ghostty has since been fixed for.
- **History costs the others more.** gnome-terminal and Console read ASCII
  at about 230 MiB/s with history (407 and 418 without), and cursor/erase
  output at about 25 MiB/s (89 and 95 without), since clearing the screen
  sends its lines to their history. foot reads ASCII at 146 MiB/s (441
  without). Konsole, keeping 1,000,000 lines in its fixed-size history, is
  busy on the CPU for the whole flood and reads ASCII at 3.3 MiB/s and
  Unicode at 0.6 (74 and 93 without); its file-backed "unlimited" history
  wasn't measured.
- **Memory with full history follows how each keeps it.** The VTE
  terminals keep history in compressed files on disk and hold 38 and 69 MiB;
  Hotaru (94 MiB) and Ghostty (109 and 120) keep 50 MB in memory, partly
  compressed; Konsole holds 942 MiB and foot 1,523 MiB for their 1,000,000
  uncompressed lines.
- **Zero scrollback flattered Hotaru.** Without history it read ASCII at
  1,047 MiB/s; with 50 MB it reads 705. Its other three corpora changed
  less (styled 348 → 301, Unicode 188 → 165, cursor/erase 127 → 126).
- **Hotaru loses on typing latency, by about a frame.** foot,
  gnome-terminal and Console show a keystroke at the next frame (11.6–11.8
  ms at the median), Konsole about half a frame later (21.2 ms), Hotaru
  (26.8 ms) and Ghostty (30.0 ms) about a frame later than foot. All of it
  is between the program's output and the frame; input reaches every
  terminal's program within a third of a millisecond.
- **At rest, Hotaru with its cursor still is nearly silent** (one wake-up a
  second, 0.01 ms of CPU a second); foot, gnome-terminal and Konsole are
  silent. Ghostty wakes 28 times a second and uses the GPU for 4.3 ms a
  second even with blinking off; Console spends 6.2 ms of CPU a second
  watching its tab's processes. **Out of the box, Hotaru blinks its cursor
  forever** (every 600 ms, 7.7 wake-ups a second); the VTE terminals stop
  blinking after GNOME's ten seconds, Konsole and foot don't blink.
- **The socket's power can't tell the idle terminals apart.** Every idle
  and typing window is within half a watt of its baseline, inside the
  sensor's noise; Ghostty's typing is the only one above it in all three
  trials (0.4 W). The floods are well clear of it.
- **CPU-drawn buffers cost the compositor more.** Typing in Hotaru costs
  sway 4.3 ms of CPU a second, in Ghostty 1.7 and in Konsole 1.6; a
  60 Hz one-row scroll costs sway 91 ms per 120 updates with Hotaru and
  43 with Ghostty, as on the earlier machines.
- **foot is the smallest by far** (7 MiB), gnome-terminal's server the
  next (21 MiB); Hotaru (47 MiB) is beside Console (48 MiB, plus 36 MiB the
  GPU driver holds for it) and well under Konsole (72 MiB) and Ghostty
  (98 MiB, plus 103 MiB of GPU memory).

## The terminals

Hotaru is Sekka's own package at `d7ecbac`; the other five come from the
Nixpkgs Sekka pins (`26.11.20261030.1c89083`), so they're the builds a
Sekka system installs.

| Terminal | Version | Store path (`/nix/store/…`) | How it draws | Processes counted |
|---|---|---|---|---|
| Hotaru | 0.1.0, Sekka `d7ecbac` (`.#hotaru`) | `d8hm9pp1g9l43awvhz0mxrl27ma8vzn3-hotaru-0.1.0` | on the CPU (Sukumo/Sidestep), shared-memory buffers; maps no GL or Vulkan driver | `hotaru` |
| Ghostty | 1.4.0 | `pda6j5833xwmiqh8w8bpsccs22hxqlk3-ghostty-1.4.0` | OpenGL 4.6 on the Radeon; GTK 4.22 presents through its GL renderer (Ghostty turns GTK's Vulkan off) | `ghostty` |
| Ghostty main (throughput with history only) | 1.4.1-dev `0b1729e` (ghostty-org/ghostty main, 2 November 2026), its flake's `ghostty-releasefast` | `iqhwwkk4hh74jkkk0pjp367z3qir6hw8-ghostty-1.4.1-dev+0b1729e-nix` | as Ghostty 1.4.0 | `ghostty` |
| gnome-terminal | 3.60.0 (GTK 3.24, VTE 0.84) | `2ywzyfkrx3nb53vv7irashipdif8zrfv-gnome-terminal-3.60.0` | Cairo on the CPU (GTK 3), shared-memory buffers | `gnome-terminal-server`, which the bus starts, plus the short-lived `gnome-terminal` client |
| Konsole | 26.08.2 (Qt 6.12, KF 6.31) | `9kf88zx0zhvilvlrrf9na9dsmax8m13z-konsole-26.08.2` | QPainter on the CPU, shared-memory buffers | `konsole` |
| GNOME Console (kgx) | 50.0 (GTK 4.22, libadwaita 1.9, VTE 0.84) | `gf6hqh7idil1qhfg1hc8dncjfz30bsq2-gnome-console-50.0` | GTK's Vulkan renderer (GskVulkanRenderer) on the Radeon, through RADV | `kgx` |
| foot | 1.28.0 | `0gkg75w7bmwd7klmf23qy8m697abypvc-foot-1.28.0` | pixman on the CPU, shared-memory buffers | `foot` |

Ghostty here is the Nixpkgs 1.4.0 release, not the pinned `1.4.1-dev`
reference the [earlier Hotaru comparisons](../../apps/hotaru/docs/gpu-comparison-2026-10-30.md)
built, which isn't in this machine's store. The throughput run with history
adds Ghostty's main branch, built from its own flake at `0b1729e` in
ReleaseFast, the same optimisation as the Nixpkgs release.

The GPU column is what the terminals did, checked in every trial: the
libraries each process mapped, the DRM devices it held, GTK's own line
(`GSK_DEBUG=renderer`) and Ghostty's log. Only Ghostty and Console use the
GPU; the other four hand the compositor CPU-drawn buffers, which sway
uploads and composites on the GPU.

## Configuration

Two configurations:

- **Matched**, for startup, latency, paced updates, throughput, memory,
  matched idle and typing: IBM Plex Mono at 14 points, cursor blink off,
  zero scrollback, opaque background, a 120×40 grid.
- **Default**, for the out-of-the-box idle measurement: each terminal with
  no configuration at all in an empty home, as a person first meets it.

| Terminal | Matched configuration | Cell | Scrollback (matched / default) | Blinking (default) |
|---|---|---|---|---|
| Hotaru | `hotaru.toml`: `font_size = 14.0`, `scrollback = 0`, `theme = "night"`, `shell_integration = false`, `cursor_blink = false` | 11×24 | 0 / 10,000 lines or 50 MB, whichever comes first | yes, every 600 ms, no timeout |
| Ghostty | `font-family = IBM Plex Mono`, `font-size = 14`, `scrollback-limit-bytes = 0`, `scrollback-limit-lines = 0`, `cursor-style-blink = false`, `background-opacity = 1`, `adjust-cell-height = -1`, `gtk-single-instance = false`, `shell-integration = none` | 11×24 (25 without the adjustment) | 0 / 50 MB, unlimited lines | yes |
| gnome-terminal | default profile (dconf): `font = 'IBM Plex Mono 14'`, `use-system-font = false`, `cursor-blink-mode = 'off'`, `scrollback-lines = 0`, `scrollback-unlimited = false`, `use-transparent-background = false`, `audible-bell = false` | **11×26** | 0 / 10,000 lines | follows GNOME: 1.2 s period, stops after 10 s idle |
| Konsole | profile `Bench.profile` (default in `konsolerc`): `Font=IBM Plex Mono,14`, `HistoryMode=0`, `BlinkingCursorEnabled=false` | 11×24 | none / 1,000 lines | no |
| Console (kgx) | dconf: `custom-font = 'IBM Plex Mono 14'`, `use-system-font = false`, `scrollback-lines = 0`; `org.gnome.desktop.interface cursor-blink = false` (Console has no blink setting of its own) | **11×26** | 0 / 10,000 lines | follows GNOME: 1.2 s period, stops after 10 s idle |
| foot | `font=IBM Plex Mono:size=14`, `line-height=24px`, `[scrollback] lines=0`, `[cursor] blink=no`, `alpha=1.0` | 11×24 (26 without `line-height`) | 0 / 1,000 lines | no |

For the throughput run with history, Ghostty's two scrollback lines were
left out, so both builds kept their defaults (`scrollback-limit-bytes =
50000000`, `scrollback-limit-lines = unlimited`, scrollback compression on),
and Hotaru had `scrollback = 1000000`, the most rows it allows, with its
50 MB byte limit as it is. Each counts its 50 MB in its own way: both limit
the uncompressed history as it holds it, not the bytes written. The other
four count history only in lines, and had the same 1,000,000 lines as
Hotaru's row limit, at least as much as 50 MB holds in Ghostty:
gnome-terminal `scrollback-lines = 1000000`, Console `scrollback-lines =
int64 1000000`, Konsole `HistoryMode=1` (fixed size) with `HistorySize =
1000000`, foot `[scrollback] lines = 1000000`. The other settings are the
matched ones.

The matched cells are 11×24 except VTE's. Hotaru and Konsole draw 24-pixel
rows at 14 points; Ghostty draws 25 and foot 26, and both have a supported
setting that gives 24. VTE (gnome-terminal and Console) draws 26-pixel
rows, as foot does by default, and offers no way to make them smaller
(`cell-height-scale` only grows them), so their 40 rows are 1,040 pixels
tall rather than 960: about 8 % more pixels to draw and composite. That
favours the other four, slightly.

Each terminal's window was calibrated until its PTY reported 120×40, so the
outer sizes differ with each one's chrome (header bars, Konsole's toolbar
and scrollbar): Hotaru 1349×1077, Ghostty 1331×984, gnome-terminal
1346×1077, Konsole 1349×1020, Console 1340×1089, foot 1328×981.

In the default configuration, the fonts are still only the trial's font set
(below), so a terminal whose default is "monospace" or a font that isn't
there gets fontconfig's nearest match, and the grids differ: the window
keeps its matched size. The idle figures don't depend on which font it is.

## Method

The harness is `tests/performance/terminals/`, extended from Hotaru's
frame benchmark (`apps/hotaru/tools/frame-benchmark.py`, `frame-matrix.py`,
`frame-observer.c`; [frame-benchmark.md](../../apps/hotaru/docs/frame-benchmark.md)).
What it adds is said where it matters below.

**Where.** Each trial starts its own sway 1.12, headless, with one
1920×1440 output at 60 Hz, rendering with GLES2 on the Ryzen's Radeon
(`WLR_RENDER_DRM_DEVICE=/dev/dri/renderD128`, Mesa's EGL vendor), so the
compositor's GPU work is inside the socket power sensor. Every terminal
gets Mesa's EGL vendor file and only Mesa's Radeon Vulkan driver
(`VK_DRIVER_FILES`), so GL and Vulkan drawing lands on the same GPU. The
NVIDIA card, which drives the real displays, isn't used.

**Fresh, private, warm.** Each trial gets an empty home, configuration,
data and state directory, its own session bus, and a fontconfig file
naming only Aizome's font folders (IBM Plex Mono, Archivo, Noto Sans
Symbols 2, Noto Sans CJK, Noto Color Emoji). The environment is an
allowlist (no host theme, input method, GSettings overrides, Qt platform
theme or `/etc/xdg`), with `LANG=C.UTF-8`. Caches are warm, as on a
machine in use: each terminal's trials start from a copy of the cache its
calibration runs left (Mesa's and RADV's shader caches, among others), the
runtime directory has the same path every trial so path-keyed caches stay
valid, and fontconfig's cache is shared. The earlier Hotaru comparisons
started cold, so their Ghostty startup included compiling its shaders.

**The session bus.** gnome-terminal is a client: it asks
`gnome-terminal-server` for a window over D-Bus and exits. The trial's bus
may start that server (its own `.service` file) and dconf's service, and
nothing else. Every process started inside the trial's session that isn't
the harness's own counts as the terminal's: for gnome-terminal that is the
server (wherever it was reparented) and the client while it lives. GTK
looks for an accessibility bus and doesn't find one.

**CPUs.** The terminal (and the bus, so a server it starts inherits them)
runs on CPUs 2–5, sway on 6 and 14, the observer on 7, the controller and
power sampler on 15, the PTY program on 1. Before each trial a preflight
waited (up to two minutes) for a one-minute load average under 1.5 and
under 10 % activity on those CPUs and their SMT siblings; a monitor
sampled them, and looked for compilers, every two seconds through the
trial.

**What the screen shows.** A Python program runs in the terminal's PTY
and paints a 120×40 screen whose first and last rows are markers: 76
coloured cells carrying a 32-bit sequence number and its CRC. A C
observer copies sway's output with wlr-screencopy, one capture in flight,
and decodes both markers; it also types on a private virtual keyboard.
The cell's interior pixels must be exact, so a frame counts only when it
shows the whole update (both markers equal, CRC right). This copy of the
observer takes the cell height from the bottom marker and samples a third
of a cell in from each edge, because Console's header bar casts a
six-pixel shadow over the top of the first row; the synthetic decoder
tests cover that case.

**Alternation.** Every round runs every terminal once, in an order rotated
by the round; each trial launches a fresh terminal. A calibration pass
(two launches of each) came first and isn't counted.

**Phases.**

1. *Startup* (10 rounds): launch to the PTY program's first instruction
   (`launch → PTY ready`) and launch to the first captured frame showing
   its first screen. Then the grid is calibrated to 120×40, and two
   seconds later the terminal's memory is read.
2. *Steady* (2 rounds): 500 presses of `a` per trial, each 40–80 ms after
   the previous one showed,
   each answered by the program with a small update (one cell and both
   markers): press to captured frame. Then paced streams of 120 updates
   (marker only, a sparse cell, a one-row scroll) at 30 Hz, 60 Hz and
   unpaced, with the terminal's and sway's CPU for each.
3. *Throughput* (4 rounds): the four 64 MiB corpora (`ascii`, `styled`,
   `unicode`, `cursor`: 8 repeats of each 8 MiB file from
   `apps/hotaru/tools/corpus.py`, byte-identical to the earlier runs')
   written into the PTY as fast as the terminal reads them; then a
   marker frame. Each corpus is preceded by four idle seconds, the
   baseline for its energy. The observer stops during the flood, so its
   captures add no work to the energy window, and restarts when the writes
   end to time the closing frame (the earlier runs kept capturing; the
   consume time doesn't depend on it).
4. *Energy* (3 rounds): 20 s with sway running and no terminal
   (baseline), then the terminal with a prompt (`$ `, cursor visible,
   colours reset to its theme's) idle for 60 s with the observer stopped,
   then typing: 600 presses at 10 per second for 60 s, each echoed by the
   program as a shell would, then the terminal closes and 10 more seconds
   of baseline.
5. *Idle, default configuration* (3 rounds): the same baseline, the
   default-configured terminal with the same prompt, 60 s idle, baseline.

**Power.** The AMD socket power sensor (amdgpu hwmon `PPT`: CPU cores,
integrated GPU, I/O die) sampled every 10 ms by `ppt-sampler.c`, with
`CLOCK_MONOTONIC` time stamps, integrated afterwards over each window the
harness timed (trapezoids, as the shared `powermeter` does). It doesn't
see RAM, the NVIDIA card or the power supply. Its idle floor here is about
18 W, so the figures to compare are each window's mean power minus its own
trial's baseline (the mean of the 20 s before the terminal started and the
10 s after it closed), and for throughput the energy over the four idle
seconds before that corpus.

**CPU, wakeups, GPU.** For each window: the terminal's CPU time (per-thread
schedstat nanoseconds, all its threads and processes), its voluntary and
involuntary context switches (each voluntary switch is a sleep, so a
wake-up follows), the same for sway, and GPU time from the DRM driver's
per-client counters (`drm-engine-gfx` in fdinfo) for the terminal and for
sway.

**Memory.** `smaps_rollup` of every process the terminal has: PSS (shared
pages split among their users), RSS and USS (private pages). Plus the GPU
memory the driver charges its DRM clients (`drm-total-vram` + `drm-total-gtt`
in fdinfo; on an integrated GPU both are system RAM) for the terminal and,
separately, for sway, which holds textures of every client's buffers.

## Results

### Startup

Ten fresh launches each, warm caches, matched configuration. *PTY ready*
is when the program in the terminal has started and reports in (it is a
Python script, and its own start, the same in every terminal, is
included); *first frame* is the first captured frame showing its whole
first screen. Medians, with the range.

| | Hotaru | Ghostty | gnome-terminal | Konsole | Console | foot |
|---|---:|---:|---:|---:|---:|---:|
| Launch → PTY ready, ms | **44.1** (43.4–44.9) | 160.5 (159.0–163.3) | 107.7 (107.1–108.4) | 98.9 (97.8–100.7) | 99.5 (98.9–100.4) | 47.5 (46.7–48.0) |
| Launch → first frame, ms | **49.6** (49.2–66.1) | 178.1 (177.7–193.9) | 129.9 (129.7–131.0) | 130.1 (129.6–130.2) | 130.2 (130.0–146.3) | 65.7 (49.9–66.3) |

Every first frame already had the 120×40 grid. First frames fall on the
output's 60 Hz frames, so their times cluster 16.7 ms apart; Hotaru's
first frame came at the earlier of its two frames in nine launches of ten,
foot's in two. For gnome-terminal
the launch includes the bus starting `gnome-terminal-server`: the client
starts the server, as on a desktop where none is running yet.

### Typing latency

Two trials of 500 presses each: the observer's virtual keyboard presses
`a`, the terminal sends it to the program, the program answers with a
one-cell update, and the time ends at the captured frame that shows it.

| | Hotaru | Ghostty | gnome-terminal | Konsole | Console | foot |
|---|---:|---:|---:|---:|---:|---:|
| p50, ms | 26.8 | 30.0 | **11.6** | 21.2 | **11.6** | 11.8 |
| p95, ms | 34.8 | 36.8 | 19.2 | 28.9 | 19.2 | **19.1** |
| p99, ms | 35.6 | 38.0 | 20.2 | 29.9 | 20.3 | **20.1** |
| max, ms | 36.9 | 38.7 | 20.6 | 31.3 | 34.3 | 20.9 |
| p50 in 60 Hz frames | 1.61 | 1.80 | 0.70 | 1.27 | 0.70 | 0.71 |
| p99 in frames | 2.14 | 2.28 | 1.21 | 1.79 | 1.22 | 1.20 |
| key → program reads it, p50 ms | 0.21 | 0.30 | 0.23 | 0.31 | 0.25 | 0.12 |
| program writes → frame, p50 ms | 26.6 | 29.7 | 11.4 | 21.0 | 11.4 | 11.6 |

Both trials of each terminal agreed within half a millisecond at the
median. Input reaches every terminal's program within a third of a
millisecond; the differences are all in getting the update onto the
screen. foot and the two VTE terminals show it at the next frame the
compositor makes (0.7 frames on average, the wait for that frame plus
the capture). Konsole takes about half a frame more. Hotaru and Ghostty
take about one frame more than foot: Hotaru 15 ms more at the median,
Ghostty 18 ms. Hotaru's Sukumo toolkit paces drawing by the compositor's
frame callbacks; an update that waits for the next callback before it's
drawn would cost about that one frame, but this run doesn't isolate it.
A later check points away from the toolkit: under the same kind of
observer, a plain text view in Sukumo shows a keystroke at the next frame
(8.4 ms at the median, the same as GTK 4 and Qt), while Hotaru, in smoke
runs of 30 keys, still lands many keys a frame late (12–17 ms at the
median, 29 at the 90th percentile, against foot's 10.5 and 14). The extra
frame is in Hotaru's own path from the PTY to the frame, not in Sukumo's
drawing or pacing ([why-toolkits.md](why-toolkits.md#hotarus-extra-frame)).
Against Ghostty, Hotaru is 3.2 ms faster at the median, as it was on wx
and the laptop on 30 October (1.7 and 2.7 ms there).

### Paced updates

120 updates per stream at 30 and 60 Hz: the marker rows only, a sparse
cell, and a one-row scroll. *Seen* counts distinct updates the observer
captured (both trials); CPU is per stream of 120 updates, median of two.

| | Hotaru | Ghostty | gnome-terminal | Konsole | Console | foot |
|---|---:|---:|---:|---:|---:|---:|
| seen at 30 Hz, all streams | 120/120 | 120/120 | 120/120 | 120/120 | 120/120 | 120/120 |
| seen at 60 Hz, marker | 120 | 120 | 116 | 120 | 116 | 120 |
| seen at 60 Hz, sparse | 120 | 120 | 116–117 | 120 | 114–115 | 120 |
| seen at 60 Hz, scroll | 120 | 120 | 116 | 119–120 | 116 | 120 |
| terminal CPU, marker 60 Hz, ms | **24** | 39 | 110 | 55 | 112 | 212 |
| terminal CPU, sparse 60 Hz, ms | 42 | **41** | 117 | 88 | 110 | 210 |
| terminal CPU, scroll 60 Hz, ms | 73 | **63** | 115 | 99 | 118 | 199 |
| terminal CPU, scroll 30 Hz, ms | 104 | **89** | 142 | 99 | 140 | 219 |
| sway CPU, marker 60 Hz, ms | **41** | 46 | 80 | 50 | 56 | 90 |
| sway CPU, scroll 60 Hz, ms | 91 | **43** | 77 | 75 | 53 | 66 |
| sway CPU, scroll 30 Hz, ms | 168 | 101 | 158 | 109 | **100** | 134 |

The VTE terminals miss a few updates at 60 Hz (now and then two updates
land in one of their frames, so the observer never sees the first);
everyone else shows all of them. Hotaru spends the least CPU of its own
on the marker and sparse updates (tied with Ghostty on sparse ones at
60 Hz), and Ghostty the least on scrolling. Scrolling costs the compositor more with Hotaru than with any
other terminal: its CPU-drawn buffer changes over the whole scrolled
region each update and sway uploads it (as on wx and the laptop), while
Ghostty and Console hand sway finished GPU buffers. foot spends the most
CPU of its own: it wakes its render worker threads (one per CPU the
machine has, 16 here) for every frame.

### Throughput without scrollback

All six in the matched configuration, with zero scrollback. Four
alternating rounds; each corpus is 64 MiB written into the PTY as
fast as the terminal reads it. *Time* is how long the terminal took to
take it all; *to final frame* adds the frame showing the closing marker.
Energy is the socket's, from the first byte to that frame; *over idle*
subtracts the mean power of the four idle seconds just before, times the
window's length. Medians (range).

| | Hotaru | Ghostty | gnome-terminal | Konsole | Console | foot |
|---|---:|---:|---:|---:|---:|---:|
| **ascii** time, ms | **61** (60–62) | 63 (61–64) | 157 (144–165) | 859 (852–867) | 153 (142–160) | 145 (143–148) |
| MiB/s | **1,047** | 1,018 | 407 | 74 | 418 | 441 |
| to final frame, ms | **83** | 86 | 194 | 880 | 179 | 171 |
| energy over idle, J | **2.0** (1.7–2.2) | 2.5 (2.3–2.6) | 2.9 (2.7–3.0) | 12.3 (10.9–12.7) | 2.4 (2.3–2.8) | 2.9 (2.8–3.0) |
| **styled** time, ms | 184 (180–208) | 248 (246–249) | 296 (283–300) | 743 (733–749) | 251 (234–282) | **149** (146–156) |
| MiB/s | 348 | 258 | 216 | 86 | 256 | **431** |
| energy over idle, J | 4.3 (3.8–4.3) | 7.1 (6.3–8.2) | 5.0 (4.6–5.1) | 10.3 (10.3–10.6) | 4.4 (4.1–4.6) | **3.1** (2.7–3.4) |
| **unicode** time, ms | 341 (340–349) | **298** (296–304) | 323 (305–331) | 689 (687–698) | 332 (307–339) | 374 (369–375) |
| MiB/s | 188 | **215** | 198 | 93 | 193 | 171 |
| energy over idle, J | 7.2 (5.4–7.4) | 8.0 (7.5–9.1) | **5.5** (5.4–5.6) | 9.4 (8.5–10.6) | **5.5** (5.0–5.7) | 5.9 (5.7–6.5) |
| **cursor** time, ms | **506** (500–509) | 2,868 (2,862–2,874) | 717 (709–731) | 708 (700–718) | 676 (651–723) | 647 (640–661) |
| MiB/s | **127** | 22 | 89 | 90 | 95 | 99 |
| energy over idle, J | 9.7 (9.2–10.3) | 36.1 (35.0–37.7) | 10.3 (10.1–10.8) | 10.4 (9.2–10.9) | **9.5** (9.3–9.9) | 10.1 (9.5–10.5) |

CPU and GPU time over the same windows (medians):

| | Hotaru | Ghostty | gnome-terminal | Konsole | Console | foot |
|---|---:|---:|---:|---:|---:|---:|
| ascii: terminal CPU / GPU, ms | 109 / 0 | 124 / 49 | 157 / 0 | 858 / 0 | 156 / 8 | 152 / 0 |
| styled | 257 / 0 | 351 / 181 | 299 / 0 | 740 / 0 | 291 / 4 | 157 / 0 |
| unicode | 427 / 0 | 415 / 222 | 330 / 0 | 691 / 0 | 346 / 7 | 387 / 0 |
| cursor | 595 / 0 | 3,076 / 1,125 | 707 / 0 | 704 / 0 | 694 / 9 | 665 / 0 |

sway's medians stayed under 12 ms of CPU and 16 ms of GPU in every
flood: with the observer stopped, a terminal that reads 64 MiB in a
fraction of a second shows only a handful of frames meanwhile.

### Throughput with history

The same floods, the same corpora and the same method, every terminal with
history (Configuration, above): Hotaru, Ghostty 1.4.0 and Ghostty main in
six alternating rounds between 16:41 and 16:49 (18 trials), then
gnome-terminal, Konsole, Console and foot in six alternating rounds between
17:05 and 17:38 (24 trials), all accepted. The four corpora run one after
another in the same terminal, ASCII first; 64 MiB of ASCII is about
1.2 million lines, which fills 50 MB or 1,000,000 lines of history, so most
of each flood runs with full history being trimmed as it goes. Medians
(range).

| | Hotaru | Ghostty 1.4.0 | Ghostty main |
|---|---:|---:|---:|
| **ascii** time, ms | 91 (84–100) | 89 (78–90) | **81** (74–85) |
| MiB/s | 705 | 720 | **794** |
| energy over idle, J | **2.8** (2.3–3.1) | 3.4 (2.5–4.2) | 2.9 (2.4–3.3) |
| terminal CPU / GPU, ms | 151 / 7 | 162 / 69 | 151 / 63 |
| **styled** time, ms | 213 (207–231) | 236 (227–249) | **185** (170–209) |
| MiB/s | 301 | 271 | **346** |
| energy over idle, J | **4.8** (3.8–5.7) | 5.6 (4.1–7.3) | 5.6 (4.8–6.0) |
| terminal CPU / GPU, ms | 286 / 5 | 338 / 179 | 278 / 138 |
| **unicode** time, ms | 389 (380–398) | 893 (854–939) | **255** (252–265) |
| MiB/s | 165 | 72 | **251** |
| energy over idle, J | 7.4 (6.6–8.6) | 16.8 (13.8–26.4) | **7.0** (5.7–8.6) |
| terminal CPU / GPU, ms | 479 / 6 | 1,118 / 501 | 371 / 184 |
| **cursor** time, ms | 507 (505–520) | 2,886 (2,873–2,980) | **353** (344–362) |
| MiB/s | 126 | 22 | **181** |
| energy over idle, J | 9.6 (6.5–10.1) | 36.7 (3.3–45.9) | **8.9** (7.6–9.5) |
| terminal CPU / GPU, ms | 597 / 6 | 3,118 / 1,145 | 468 / 243 |
| PSS after the floods, MiB | **94.5** | 109.2 | 120.4 |
| RSS after the floods, MiB | **101.1** | 174.2 | 174.9 |

The other four, with 1,000,000 lines:

| | gnome-terminal | Konsole | Console | foot |
|---|---:|---:|---:|---:|
| **ascii** time, ms | 279 (249–292) | 19,496 (17,181–20,749) | **276** (257–298) | 439 (435–461) |
| MiB/s | 230 | 3.3 | **232** | 146 |
| energy over idle, J | 4.3 (4.0–4.9) | 376 (238–437) | **4.2** (3.2–5.9) | 5.4 (2.9–8.2) |
| terminal CPU, ms | 281 | 19,495 | 291 | 455 |
| **styled** time, ms | 411 (391–460) | 94,955 (86,258–102,992) | 393 (369–424) | **191** (184–195) |
| MiB/s | 156 | 0.7 | 163 | **334** |
| energy over idle, J | 6.2 (5.0–7.4) | 2,000 (1,675–2,488) | 6.2 (4.7–6.7) | **3.7** (2.0–4.6) |
| terminal CPU, ms | 431 | 94,829 | 432 | 196 |
| **unicode** time, ms | 728 (720–800) | 115,315 (101,460–121,776) | 725 (711–796) | **423** (406–429) |
| MiB/s | 88 | 0.6 | 88 | **151** |
| energy over idle, J | 11.1 (7.2–13.3) | 2,112 (1,616–2,353) | 10.0 (5.8–15.5) | **6.2** (4.8–8.3) |
| terminal CPU, ms | 762 | 115,155 | 772 | 436 |
| **cursor** time, ms | 2,538 (2,458–2,614) | 708 (702–728) | 2,620 (2,587–2,647) | **618** (603–634) |
| MiB/s | 25 | 90 | 24 | **104** |
| energy over idle, J | 30.7 (26.3–44.4) | 10.3 (5.9–12.9) | 27.0 (20.9–35.7) | **8.9** (7.3–12.5) |
| terminal CPU, ms | 2,457 | 704 | 2,450 | 631 |
| PSS after the floods, MiB | **38.2** | 942.5 | 69.4 | 1,522.9 |
| RSS after the floods, MiB | **68.4** | 979.9 | 120.3 | 1,533.1 |

Konsole's terminal CPU equals its flood time: with a fixed history of
1,000,000 lines it is busy the whole while, and slower on each corpus as the
history fills (its cursor/erase corpus, which doesn't add lines, is as fast
as without history). Konsole's own default is 1,000 lines; its "unlimited"
history, kept in a file, wasn't measured. VTE (gnome-terminal and Console)
keeps history in compressed files on disk, which is why it holds so little
memory with it; its cursor/erase throughput falls from about 90 to 25 MiB/s
with history, because clearing the screen moves the screen's lines into
history.

(GPU is the terminal's and sway's together; Hotaru's is sway's composing.)
With history, every terminal reads ASCII more slowly than without it:
Hotaru 1,047 → 705 MiB/s, Ghostty 1.4.0 1,018 → 720, gnome-terminal
407 → 230, Console 418 → 232, foot 441 → 146, Konsole 74 → 3.3. Ghostty 1.4.0 also
reads Unicode far more slowly with history (215 → 72 MiB/s); Ghostty main
doesn't share that. Hotaru's other corpora change by at most 14%. These
are whole terminals on screen: reading, parsing, keeping history and
drawing, not the VT library alone.

### Against Ghostty's author's library-level figures

The graphs in [his post](data/terminals/ghostty-author-2026-11-02/README.md)
put the same two terminals in a different order on some corpora. Ghostty
main's speed as a multiple of Hotaru's (higher is better for Ghostty):

| | His run: VT libraries only, Ghostty with 50 MB, Hotaru with none | These runs: whole terminals on screen, both with 50 MB |
|---|---:|---:|
| ASCII | 0.78× | 1.13× |
| Styled | 1.35× | 1.15× |
| Unicode | 1.35× | 1.52× |
| Cursor/erase | 1.67× | 1.44× |
| Hyperlinks | 1.17× | not measured |

The two setups differ in four ways:

- **What is timed.** He fed bytes to libghostty-vt and to Hotaru's core
  with `hotaru-replay`, drawing nothing. These runs time each terminal
  reading a PTY, keeping history and putting frames on screen.
- **Hotaru's history.** Hotaru's feed-only mode keeps none, so in his runs
  only Ghostty paid for history; here both did. That is what turns ASCII
  around. It is the corpus history slows Hotaru most (his third graph:
  +109% with 50,000 lines; here 1,047 → 705 MiB/s), so with history on
  both sides Ghostty main is ahead.
- **Builds.** Ghostty main here is `0b1729e`, three commits after his
  `91973f2b2`: the same fixes, plus one that skips erasing and scanning
  rows already blank (ghostty-org/ghostty#15035), which bears on
  cursor/erase. His Hotaru is Sekka `59786be0` (30 October). Ours,
  `d7ecbac`, includes #159, which changed Hotaru's PTY session
  (`session.rs`), a path a feed-only run doesn't take.
- **Inputs and machine.** His inputs are the library comparison's four
  reviewed 8 MiB patterns, a hyperlink pattern and `ghostty-gen`'s data.
  These are 64 MiB floods of four kinds, on runnymede. His post doesn't
  name his machine.

These records don't explain the rest. Ghostty's lead on styled text is
smaller here than in his runs, and its lead on Unicode larger. Drawing is
what these runs add, and the likeliest place for both, but nothing here
measures it apart.

### At rest and typing

Each energy trial: 20 s of sway alone (the baseline), the terminal at a
`$ ` prompt with its cursor showing for 60 s, then 600 keystrokes at ten a
second for 60 s, each echoed as a shell would; then the terminal closes and
10 s more of baseline. The default-configuration trials are the same up to
the end of the idle minute. The observer doesn't capture during any of
it. Three trials each; medians (range). *Wake-ups* are the terminal's
context switches per second, all threads; *over baseline* is the window's
mean socket power minus the same trial's baseline.

**Idle, matched (blink off)** (3 trials each)

| | Hotaru | Ghostty | gnome-terminal | Konsole | Console | foot |
|---|---:|---:|---:|---:|---:|---:|
| terminal CPU, ms/s | 0.01 (0.01–0.02) | 1.35 (1.26–1.35) | 0.00 (0.00–0.00) | 0.00 (0.00–0.00) | 6.21 (6.04–6.21) | 0.00 (0.00–0.00) |
| terminal wake-ups/s | 1.0 (1.0–1.0) | 28.5 (28.5–28.7) | 0.0 (0.0–0.0) | 0.0 (0.0–0.1) | 2.6 (2.6–2.7) | 0.0 (0.0–0.0) |
| sway CPU, ms/s | 0.00 (0.00–0.00) | 0.20 (0.19–0.20) | 0.00 (0.00–0.00) | 0.00 (0.00–0.00) | 0.05 (0.05–0.06) | 0.00 (0.00–0.00) |
| GPU busy, terminal + sway, ms/s | 0.00 | 4.26 | 0.00 | 0.00 | 0.26 | 0.00 |
| socket power, W | 17.98 (17.93–18.08) | 17.85 (17.84–18.01) | 17.94 (17.85–18.00) | 17.88 (17.79–17.96) | 17.98 (17.97–18.00) | 17.96 (17.92–18.09) |
| over baseline, W | -0.01 (-0.35 to 0.01) | 0.10 (-0.09 to 0.15) | -0.05 (-0.18 to -0.01) | -0.01 (-0.18 to 0.02) | 0.05 (0.01–0.05) | -0.09 (-0.15 to 0.14) |

**Idle, out of the box** (3 trials each)

| | Hotaru | Ghostty | gnome-terminal | Konsole | Console | foot |
|---|---:|---:|---:|---:|---:|---:|
| terminal CPU, ms/s | 0.24 (0.23–0.26) | 1.31 (1.30–1.34) | 0.07 (0.06–0.07) | 0.00 (0.00–0.00) | 6.30 (6.19–6.58) | 0.00 (0.00–0.00) |
| terminal wake-ups/s | 7.7 (7.7–7.7) | 28.5 (28.3–28.6) | 0.5 (0.5–0.5) | 0.0 (0.0–0.0) | 4.6 (4.5–4.6) | 0.0 (0.0–0.0) |
| sway CPU, ms/s | 0.27 (0.26–0.28) | 0.20 (0.20–0.21) | 0.07 (0.06–0.07) | 0.00 (0.00–0.00) | 0.20 (0.12–0.27) | 0.00 (0.00–0.00) |
| GPU busy, terminal + sway, ms/s | 0.02 | 4.28 | 0.01 | 0.00 | 1.09 | 0.00 |
| socket power, W | 17.93 (17.75–19.60) | 18.00 (17.82–18.33) | 17.80 (17.68–18.05) | 17.85 (17.80–17.94) | 17.98 (17.95–18.39) | 17.94 (17.87–18.32) |
| over baseline, W | -0.05 (-0.31 to 0.35) | -0.27 (-0.33 to -0.05) | -0.09 (-0.15 to 0.04) | -0.09 (-0.18 to 0.30) | 0.03 (-0.14 to 0.42) | -0.24 (-0.60 to 0.11) |

**Typing, 10 keys/s** (3 trials each)

| | Hotaru | Ghostty | gnome-terminal | Konsole | Console | foot |
|---|---:|---:|---:|---:|---:|---:|
| terminal CPU, ms/s | 10.41 (10.26–10.43) | 10.62 (10.33–11.30) | 5.72 (5.60–5.81) | 6.10 (5.91–6.43) | 12.36 (12.32–12.37) | 11.57 (11.42–11.60) |
| terminal wake-ups/s | 262.7 (253.1–264.9) | 341.6 (341.5–343.0) | 30.2 (30.0–30.2) | 130.9 (126.3–131.4) | 31.8 (30.7–31.8) | 441.4 (438.3–442.8) |
| sway CPU, ms/s | 4.29 (4.28–4.29) | 1.67 (1.60–1.80) | 3.73 (3.66–3.79) | 1.60 (1.53–1.74) | 3.05 (3.04–3.07) | 3.58 (3.55–3.60) |
| GPU busy, terminal + sway, ms/s | 0.04 | 34.15 | 0.06 | 0.00 | 10.53 | 0.08 |
| socket power, W | 18.01 (18.00–18.04) | 18.28 (18.21–18.30) | 17.95 (17.90–18.62) | 17.96 (17.80–18.17) | 17.97 (17.78–18.07) | 18.03 (17.96–18.44) |
| over baseline, W | -0.07 (-0.29 to 0.09) | 0.41 (0.36–0.46) | 0.05 (-0.06 to 0.44) | -0.02 (-0.25 to 0.40) | 0.02 (-0.15 to 0.11) | 0.14 (-0.15 to 0.36) |

What the columns say:

- **Idle, cursor still.** foot, gnome-terminal and Konsole don't run at
  all. Hotaru wakes once a second, for 0.01 ms of CPU a second. Console's 6
  ms of CPU a second (0.6 % of one CPU) is its process watcher
  (`KgxWatcher`, through libgtop), which lists the machine's processes a
  few times a second to mark tabs running `sudo` or `ssh`. Ghostty wakes 28 times a
  second across its threads and keeps the GPU busy about 4.3 ms a second
  with nothing changing on screen.
- **Idle, out of the box.** Hotaru blinks (600 ms, no timeout), so it and
  sway redraw the cursor cell: 7.7 wake-ups and 0.24 ms of Hotaru's CPU a
  second, and 0.27 ms of sway's. gnome-terminal and Console blink until
  GNOME's ten-second timeout, about eight seconds into the window. Ghostty blinks too, at no extra cost over its idle with blinking
  off.
- **Typing.** gnome-terminal and Konsole use the least CPU (5.7 and 6.1 ms
  a second) and gnome-terminal and Console the fewest wake-ups (30 a
  second). Hotaru (263 a second), Ghostty (342) and foot (441) wake several
  threads for each frame. Hotaru costs sway the most CPU (4.3 ms a second); Ghostty's
  GPU drawing costs the GPU 34 ms a second and Console's 11.
- **Power.** None of the idle or typing windows differs from its baseline
  by more than the sensor's noise: whole-watt readings over an 18 W floor
  that wanders by half a watt. Ghostty's typing is the one window above
  its baseline in all three trials (0.36–0.46 W), which fits its GPU
  drawing, but even that is within what the floor's wander allows. The
  figures are there for completeness; they don't rank the terminals. A
  ten-key-a-second typist costs every one of them well under a watt here.

### Memory

After startup (2 s after the grid reached 120×40, ten trials) and after the
four floods (four trials), matched configuration with zero scrollback (for
memory after the floods with 50 MB of history, see
[Throughput with history](#throughput-with-history)).
*GPU memory* is what the amdgpu driver charges the terminal's own DRM
client (VRAM plus GTT; on an integrated GPU both are system RAM). sway
holds its own copies of every client's buffers as textures; that column
is sway's, with that terminal on screen.

| | Hotaru | Ghostty | gnome-terminal | Konsole | Console | foot |
|---|---:|---:|---:|---:|---:|---:|
| PSS after startup, MiB | 47.2 (47.1–47.4) | 98.0 (97.6–98.5) | 20.6 (20.4–20.7) | 71.6 (71.4–77.2) | 48.4 (48.2–59.2) | 6.8 (6.7–6.8) |
| RSS after startup, MiB | 53.8 | 155.9 | 46.2 | 97.4 | 90.3 | 16.1 |
| USS after startup, MiB | 44.3 | 77.0 | 13.0 | 59.4 | 36.8 | 3.3 |
| GPU memory, terminal, MiB | 0.0 | 102.8 | 0.0 | 0.0 | 35.7 | 0.0 |
| GPU memory, sway, MiB | 74.7 | 68.3 | 73.8 | 85.0 | 82.1 | 70.5 |
| PSS after the floods, MiB | 48.7 (48.7–48.8) | 101.8 (101.3–102.1) | 46.7 (44.0–68.7) | 72.9 (72.7–73.2) | 70.5 (69.0–82.1) | 11.6 (11.5–11.6) |
| RSS after the floods, MiB | 55.3 | 159.8 | 73.9 | 93.4 | 114.1 | 21.2 |
| PSS idle, out of the box, MiB | 47.5 (47.3–47.6) | 98.0 (97.9–98.2) | 21.7 (21.6–21.7) | 73.6 (73.6–73.8) | 60.7 (60.7–60.7) | 7.3 (7.2–7.3) |
| threads | 16 | 28 | 6 | 4 | 10 | 17 |

PSS shares out pages several processes map (libraries, and the
shared-memory buffers a CPU-drawn terminal and sway both map); RSS counts
them whole; USS is only the terminal's own. gnome-terminal's figure is
`gnome-terminal-server`'s, the process that holds the terminal. VTE's
memory grows after a flood even with no scrollback (21 → 47 MiB for
gnome-terminal, 48 → 71 MiB for Console); Hotaru, Ghostty and Konsole grow
by 1–4 MiB, foot by 5 MiB.

## Hotaru and Ghostty, from the code

Read from Ghostty's source at `0b1729e` and Hotaru's at `d7ecbac`, to
explain the results above; nothing here was measured for this section.

**Ghostty is incremental on the CPU, not on the GPU.** Its renderer
(`src/renderer/generic.zig`) tracks which rows changed and rebuilds the cell
data of those rows only, skipping the frame when nothing changed. A frame
it does draw is drawn whole: the cell buffers for the whole grid are
uploaded (`cells_bg.sync`, `syncFromArrayLists`), and a background pass
and every glyph, as instanced draws, cover the whole surface. On GTK the
frame goes into a DMA-BUF queue (`apprt/gtk/class/render_surface.zig`);
GTK's main thread takes it at its own frame clock, draws it as a texture
with its own GL renderer, and the compositor gets the whole window as
changed. One keystroke is a full-window GPU draw, a second GPU pass inside
GTK, and a full-window composite.

**Hotaru is incremental all the way to the compositor.** A reader thread
feeds the session thread that parses (`session.rs`, modelled on Ghostty's
two-stage reader); the session publishes an immutable snapshot of the rows
at most every 8 ms and only after the last one was used, sharing the rows
that didn't change. The view (`terminal_view.rs`) compares rows by
identity and invalidates only those that changed, moves the pixels already
drawn when the screen scrolls, and Sukumo draws only the damaged
rectangles, on the CPU, into a shared-memory buffer. The compositor gets
only the damage.

What follows from that, against the results:

- **Hotaru's lead is cost per change.** Startup (no GL context, shaders or
  GTK: 50 ms against 178), rest (1 wake-up a second and no GPU, against
  28 and 4.3 ms of GPU a second), memory (47 MiB against 98 plus 103 of
  GPU memory) and typing (about no GPU against 34 ms a second at 10 keys
  a second) all come from drawing only what changed, on the CPU, with no
  GPU context.
- **Throughput is not where the design helps.** Both read and parse on
  separate threads and both coalesce frames during a flood, so frames
  aren't the bottleneck. With history Ghostty's main branch is faster on
  every corpus; that's its VT core (its paged screen, and recent styled
  and Unicode work), and history eviction, which Hotaru has since fixed
  (#261).
- **The compositor pays for Hotaru's buffers.** Ghostty's frames are
  already on the GPU; each of Hotaru's damaged regions has to be copied
  from shared memory into a texture by the compositor. That's why sway
  spends 0.76 ms per one-row scroll update on Hotaru against 0.36 on
  Ghostty, and 4.3 ms a second of typing against 1.7.
- **Latency:** Ghostty's extra frame fits its design (the frame waits for
  GTK's clock and a second render). Hotaru's isn't explained by the code
  as read: Sukumo waits for a frame callback only after a present, and the
  session publishes at once when idle. A trace of the PTY-to-frame path is
  the next step.

### Next: a zero-copy buffer for Hotaru

Hotaru should hand the compositor buffers it can use without copying:
draw on the CPU into a linear buffer the GPU can read in place (a
`udmabuf` or a linear GBM buffer exported as a DMA-BUF through
`zwp_linux_dmabuf_v1`) instead of `wl_shm`. The compositor would then
import the buffer as a texture instead of uploading every damaged region,
which is the cost behind the compositor rows above. The CPU drawing,
damage tracking and pixel-moving scroll stay as they are. Because Hotaru
draws through Sukumo, the change belongs in Sukumo's backend
(`sukumo/crates/sidestep-engine/src/backend/`), where every kit app would
get it; the same cost shows in the kit's 4K and scrolling smoke runs
([why-toolkits.md](why-toolkits.md)).

## Limits

- **Headless, not a display.** Times end when the private compositor's
  frame is captured, not at scanout; there is no photon in this. The
  output's 60 Hz clock is synthetic, and the screencopy that times frames
  adds compositor work of its own (it is off during the throughput floods
  and the energy windows, on during startup, typing latency and paced
  streams).
- **One compositor.** sway 1.12 with GLES2, not Aizome's compositor,
  GNOME Shell or KWin. A compositor that scans out a client's buffer
  directly, or that copies shared memory differently, changes the
  compositor columns and the GPU terminals' advantage there.
- **One machine, one GPU.** The Radeon in a desktop Ryzen 7 9800X3D, with
  the `performance` governor. A laptop's GPU, a discrete GPU or a power-
  saving governor would move the numbers, Ghostty's and Console's most.
  The earlier Hotaru/Ghostty runs on an NVIDIA workstation and an Intel
  laptop ([gpu-comparison-2026-10-30.md](../../apps/hotaru/docs/gpu-comparison-2026-10-30.md))
  are the other data points.
- **The power sensor is coarse.** PPT reports whole watts about every
  7 ms, for the whole socket, over an idle floor of about 18 W that
  wanders by half a watt from one five-second stretch to the next. Bursts
  (startup, throughput) stand well clear of it; an idle terminal doesn't:
  every idle difference here was within that noise, so idle cost is
  reported as CPU time, wake-ups and GPU time, which the sensor can't
  resolve. It doesn't see RAM, the NVIDIA card or the power supply.
- **Matched isn't identical.** VTE's rows are 26 pixels where the others'
  are 24 (above); the chrome differs (header bars, Konsole's toolbar and
  scrollbar, none for foot); and the terminals' default themes differ in
  the default runs.
- **What markers prove.** A captured frame counts when both marker rows show
  the update's sequence number and CRC; that proves which update the frame
  shows, not that every glyph is right. The fixture's screens are plain
  ASCII apart from the throughput corpora, which aren't checked for
  rendering at all.
- **Default configuration, but not a default desktop.** The trial's
  session has no accessibility bus, no settings portal, no keyring and
  only Aizome's fonts. On a GNOME or Plasma desktop, GTK and Qt would
  find more services and fonts; the idle figures assume none of those
  wake the terminal. Console's idle CPU comes from watching the processes
  in its tabs, which reads `/proc`; it would cost less on a machine with
  fewer processes than this one's several hundred.
- **Memory is the terminal's own.** PSS counts every process of the
  terminal (gnome-terminal's server, not its brief client); GPU memory is
  what the driver charges the terminal's DRM clients. sway's textures of
  the terminal's buffers are its own, given separately. With zero
  scrollback, memory after throughput says nothing about history; with
  their default scrollback (10,000 lines for Hotaru, gnome-terminal and
  Console, 50 MB for Ghostty, 1,000 lines for Konsole and foot) they would
  hold more.
- **Packaged builds.** Ghostty is Nixpkgs' 1.4.0 release (a ReleaseFast
  build: `optimize=fast` in its log), not the pinned development reference
  of the earlier runs. Each terminal is the build Sekka installs.

## Reproducing it

On a NixOS machine with an AMD processor and integrated graphics (the
power sensor and the render node are the AMD ones; elsewhere, change
`--render-node` and expect no `PPT` sensor), from the repository root:

```sh
export WORK=${TMPDIR:-/tmp}/aizome-perf/terminals   # run.sh's default
# Shell environment (sway, grim, dbus, dconf, Python, cc, the protocol XML
# and every terminal from Sekka's Nixpkgs pin), the observer and power
# sampler, and the 8 MiB corpora.
tests/performance/terminals/run.sh env

# Window, cell and warm cache for each terminal (two launches each).
tests/performance/terminals/run.sh calibrate

# The measured phases: PHASE ROUNDS [FIRST_ROUND [SUFFIX]].
tests/performance/terminals/run.sh startup 10
tests/performance/terminals/run.sh steady 2
tests/performance/terminals/run.sh throughput 4
# Throughput with history: Ghostty main built, calibrated on its own, merged
# with the others' calibration, and the floods with Ghostty's defaults.
nix build github:ghostty-org/ghostty/0b1729e945978fca4bcd6165d8ac408bf675a7f2#ghostty-releasefast --out-link ghostty-main
CALIBRATION=$WORK/calibration-main APPS=ghostty-main GHOSTTY_MAIN=$PWD/ghostty-main/bin/ghostty \
  tests/performance/terminals/run.sh calibrate
#   then $WORK/calibration-history/calibration.json: hotaru and ghostty from
#   $WORK/calibration, ghostty-main from $WORK/calibration-main
SCROLLBACK=ghostty-default CALIBRATION=$WORK/calibration-history DATA=$WORK/data-history \
  APPS=hotaru,ghostty,ghostty-main GHOSTTY_MAIN=$PWD/ghostty-main/bin/ghostty \
  tests/performance/terminals/run.sh throughput 6 0 -a
#   and the other four (their calibration added from $WORK/calibration):
SCROLLBACK=ghostty-default CALIBRATION=$WORK/calibration-history DATA=$WORK/data-history \
  APPS=gnome-terminal,konsole,kgx,foot tests/performance/terminals/run.sh throughput 6 0 -b
tests/performance/terminals/run.sh energy 2 0 -a
tests/performance/terminals/run.sh energy 1 2 -b
tests/performance/terminals/run.sh idle-default 1 0 -a
tests/performance/terminals/run.sh idle-default 2 1 -b

# Tables and summary.json from everything under $WORK/data, and the
# subjects' identities.
source $WORK/shell-env.sh
python3 tests/performance/terminals/terminal-summary.py $WORK/data docs/performance/data/terminals
python3 tests/performance/terminals/terminal-summary.py $WORK/data-history /tmp/history  # summary-history.*
python3 tests/performance/terminals/record-env.py docs/performance/data/terminals/environment.json
```

With `LOCKS=tests/performance`, `run.sh` wraps each phase in the shared
measurement lock (`lock-measure`; calibration in `lock-build`), as it ran
here; without `LOCKS` the phases just run.
`APPS=hotaru,foot` restricts a run; `alacritty` and `kitty` have adapters
too, untested and not measured. One trial alone:

```sh
source $WORK/shell-env.sh
fallback=$(sed -n "s/^export HOTARU_FONT_DIR=\${HOTARU_FONT_DIR-'\(.*\)'}$/\1/p" "$TERMBENCH_HOTARU")
python3 tests/performance/terminals/terminal-bench.py foot "$TERMBENCH_FOOT" --phase steady \
  --output /tmp/one --observer $WORK/observer --font-dir "$TERMBENCH_FONTS" \
  --fallback-dir "$fallback" --keys 100 --frames 60
```

The decoder's synthetic tests (`frame-observer-test.c`, 664 cases including
the shadowed first row) build against the same generated protocol code as
the observer:

```sh
cc -O2 -std=c11 -Wall -Wextra -Werror -I$WORK/observer tests/performance/terminals/frame-observer-test.c \
  $WORK/observer/*-protocol.c $(pkg-config --cflags --libs wayland-client xkbcommon) -o /tmp/observer-test && /tmp/observer-test
```

## Records

[`data/terminals/`](data/terminals/) ([README](data/terminals/README.md)):

- `summary.json` and `summary.md`: every figure above, from
  `terminal-summary.py`, with the spread and the trial counts;
- `environment.json`: store paths, versions and closure sizes of the
  terminals, the tools, the commit and the host's CPU and GPU driver;
- `calibration.json`: each terminal's window, cell, processes, GPU evidence,
  exact configuration files and warm-cache contents;
- one `.tar.xz` per matrix run (`startup`, `steady`, `throughput`,
  `energy-a`, `energy-b`, `idle-default-a`, `idle-default-b`) and the
  calibration runs: each trial's result, power samples, frame and fixture
  logs, application and compositor logs, the tools as they ran, and the
  final capture of each terminal's first round.
- the throughput runs with history: `throughput-history-a.tar.xz` (Hotaru
  and both Ghostty builds) and `throughput-history-b.tar.xz` (the other
  four), `summary-history.json` and `summary-history.md` (both together),
  `calibration-history.json` (the calibration they used) and
  `calibration-ghostty-main.tar.xz` (Ghostty main's calibration runs).

132 trials ran, in four lock holds between 04:31 and 09:14 local time;
all 132 were accepted, and every one started on a quiet machine (load
under 1.5, the measured CPUs under 10 % busy). Each archive's `tools/`
holds the harness exactly as it ran; the repository's copy differs from it
only in the untested Alacritty and kitty adapters and in its default
scratch and lock paths.
