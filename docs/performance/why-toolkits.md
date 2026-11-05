# Toolkits at work: latency and cost per interaction (incomplete)

[why-apps.md](why-apps.md) took apart what an app costs before its first
frame. This page is about what happens after: how quickly each toolkit gets
a keystroke onto the screen, what each interaction costs in CPU, GPU and
power, in the app and in the compositor, and how much each can draw.
It compares Aizome's toolkit (Sidestep, which kit apps draw through: CPU
drawing with tiny-skia on a render thread, into shared-memory buffers)
with GTK 4, libadwaita, Qt 6 Widgets and Qt 6 Quick.

**Status.** The run was stopped after its first measured chunk, on
2 November 2026. What finished is the typing-latency set below: five
toolkits, three trials each, under the measurement lock. Everything else
here comes from smoke runs made while building the harness: single trials,
not all of them under the exclusive lock, kept because they point
somewhere. They're marked as smoke wherever they appear, and none of them
should be quoted as a result. What wasn't run is listed at the end.

## In short

- **Every toolkit shows a keystroke at the next frame.** Under the same
  compositor, the median is 7.9–8.8 ms at 60 Hz, half a frame, which is
  the wait for the next frame. The 99th percentile is under one frame,
  16–17 ms. The kit is neither ahead of nor behind GTK and Qt here. The
  one exception is Qt Quick in a 10,000-line document: 10.8 ms at the
  median and 20.5 at the 99th.
- **The kit does the least work per keystroke.** Its matched program used
  0.24 ms of CPU per key and no GPU. Qt Widgets, the other toolkit that
  draws on the CPU, used 0.33 ms; GTK 4 and libadwaita 0.6–0.8 ms of CPU
  plus about 0.8 ms of GPU; Qt Quick 1.0–3.5 ms of CPU plus 1.4 ms of GPU.
  These are whole-trial costs divided by keys, caret blinking included;
  the clean per-key measurement didn't run.
- **Hotaru's extra frame of typing latency is Hotaru's** (smoke). The kit's
  text view has no such delay, while Hotaru, in the same harness, shows
  many keys a frame late.
- **On whole-window redraws the GPU toolkits come out ahead** (smoke). At
  4K, a new page every frame, the kit and Qt Quick drew about as many
  frames a second (322 and 345), but the kit's frames cost the compositor
  1.3 ms of CPU and 3.1 ms of GPU each, uploading 28.8 MB of pixels, where
  Qt Quick's GPU buffers cost it almost nothing. That's the same trade-off
  as Hotaru against Ghostty, and the reason for the zero-copy work
  proposed in [terminals.md](terminals.md#next-a-zero-copy-buffer-for-hotaru).

## Method

The harness is `tests/performance/why-toolkits/`, built from the terminal
harness's ([terminals.md](terminals.md#method)).

**Matched programs.** One small program a toolkit, in `apps/`: the kit
(`apps/kit`), GTK 4 and libadwaita (`tk-gtk.c`, built twice),
Qt Widgets (`tk-qtwidgets.cpp`) and Qt Quick (`tk-qtquick.cpp`). Each
shows either a text view of a file, with the caret at the start of line 20,
or a list of 10,000 rows of 24 px, in a 1280 × 800 window, in IBM Plex Mono
or Archivo at 14 px. Nothing else is changed from each toolkit's defaults.
Testing small programs rather than apps keeps the toolkit apart from what
an app does on top of it; real apps were only smoke-tested.

**Where.** Each trial starts its own sway 1.12, headless, with one
1920 × 1080 output at 60 Hz and scale 1, drawing with GLES2 on the Ryzen's
Radeon (renderD128, Mesa 26.2.6). GTK and Qt draw on the same GPU, with
Mesa's EGL and only Mesa's Radeon Vulkan driver visible, as they would on
a desktop; the kit draws on the CPU. The observer keeps sway composing
on its 60 Hz grid, so every trial sees the same frame clock. The
environment is an allowlist (no host theme, input method or Qt platform
theme; `LANG=C.UTF-8`), and fontconfig sees only Aizome's fonts. The
program runs on CPUs 2–5, sway on 6 and 14, the tools on 7 and the
harness on 15.

**Typing latency.** 150 key presses a trial after a warm-up, 40–90 ms
apart at random, typed through sway's virtual keyboard. A key's latency
runs from the press to the first frame sway composes whose copy of the
window's region (wlr-screencopy) differs from the frame before. Nothing
runs inside the program. `lat-fullcopy` copies the whole output every
frame instead, as the terminal harness's observer does. Three trials per
toolkit and scenario, run under `tests/performance/lock-measure`, between
23:18 and 23:42 on 2 November.

**Fixtures** ([data/why-toolkits/fixtures](data/why-toolkits/fixtures)):
`small.txt` (60 lines) and `large.txt` (10,000 lines) of code-like words.
The smoke runs of real apps used a folder of 10,000 small files named
`file-00001.txt` and so on.

## Typing latency

From [`tables-sway.md`](data/why-toolkits/tables-sway.md); 450 keys
(three trials) per cell, no key missed.

| 60-line file | p50 ms | p90 ms | p99 ms | max ms | p50 frames |
|---|---:|---:|---:|---:|---:|
| Kit | 8.36 | 14.75 | 16.89 | 22.78 | 0.50 |
| GTK 4 | 8.50 | 15.20 | 16.61 | 17.04 | 0.51 |
| libadwaita | 8.44 | 14.91 | 16.57 | 17.10 | 0.51 |
| Qt Widgets | 7.92 | 14.45 | 15.95 | 16.43 | 0.48 |
| Qt Quick | 8.76 | 15.01 | 16.88 | 17.70 | 0.53 |

| 10,000-line file | p50 ms | p90 ms | p99 ms | max ms | p50 frames |
|---|---:|---:|---:|---:|---:|
| Kit | 8.34 | 14.97 | 18.25 | 30.41 | 0.50 |
| GTK 4 | 8.60 | 14.91 | 16.43 | 17.05 | 0.52 |
| libadwaita | 8.58 | 14.86 | 16.59 | 17.37 | 0.51 |
| Qt Widgets | 8.11 | 14.52 | 16.14 | 16.48 | 0.49 |
| Qt Quick | **10.76** | 17.87 | 20.48 | 20.93 | 0.65 |

| 60-line file, whole output copied | p50 ms | p90 ms | p99 ms | max ms |
|---|---:|---:|---:|---:|
| Kit | 7.50 | 13.90 | 15.90 | 17.16 |
| GTK 4 | 7.19 | 13.25 | 15.58 | 16.00 |
| libadwaita | 7.50 | 13.34 | 15.76 | 16.15 |
| Qt Widgets | 7.49 | 14.04 | 15.95 | 16.23 |
| Qt Quick | 7.33 | 13.50 | 15.85 | 16.09 |

A key pressed at a random moment waits on average half a frame (8.3 ms at
60 Hz) for the next frame the compositor makes. Every toolkit's median is
within half a millisecond of that, and the 99th percentiles are under a
frame, so every toolkit's response is in the very next frame. Typing
latency doesn't tell these toolkits apart under the same compositor. The
desktop-level gap measured in the VM ([desktops.md](desktops.md)) is the
compositors' scheduling, not the toolkits ([why-desktop.md](why-desktop.md)).

Two exceptions are small. Qt Quick in the large file is about 2.4 ms later
at the median and 3.6 ms at the 99th percentile: its text layout costs 3.5 ms
of CPU per key there (below). The kit has a few late keys in the large
file (max 30.4 ms, 22.8 in the small one), each a frame late; their cause
wasn't looked into.

The whole-output copy adds compositor work to every frame; with it, the
time until the region stops changing has a long tail in every toolkit
(90th percentile 22–28 ms) that wasn't investigated either. The first
change, in the table, isn't affected.

### Cost per keystroke

The latency trials also recorded the CPU and GPU time each program used
over the trial. Divided by the 150 keys, as a rough per-key cost:

| Per key | App CPU, 60 lines | App CPU, 10,000 lines | App GPU |
|---|---:|---:|---:|
| Kit | 0.24 ms | 0.23 | none |
| Qt Widgets | 0.33 | 0.34 | none |
| GTK 4 | 0.59 | 0.78 | 0.77 ms |
| libadwaita | 0.61 | 0.78 | 0.81 |
| Qt Quick | 1.04 | **3.47** | 1.43 |

These include everything the program did during the 23-second trial
(caret blinking among it), and they don't include the compositor: sway's
figures in these trials are dominated by the observer copying the screen
every frame. The `cost-*` scenarios, typing at 10 keys a second with sway
idle between frames and the screen not copied, are the clean measurement,
and they didn't run.

## Hotaru's extra frame

*Smoke: two trials of 30 keys per terminal.* [terminals.md](terminals.md)
found Hotaru about a frame behind foot on typing latency and couldn't say
whether Sukumo's frame pacing caused it. Under this harness, with a
raw-mode program answering each key in one write:

| | p50 ms | p90 ms | max ms |
|---|---:|---:|---:|
| foot | 10.6, 10.5 | 14.7, 13.5 | 19.1, 16.9 |
| Hotaru | 12.4, 16.6 | 28.9, 29.3 | 31.9, 32.9 |

Hotaru lands many keys a frame late; the kit's own text view, above, never
does. So the extra frame is in Hotaru's path from the PTY to the frame
(its reader and session threads, the 8 ms snapshot pacing, the hand-off to
the view), not in Sukumo's drawing or pacing. The proxy traces of both
terminals (`smoke-t/trace-*` in `runs-smoke.tar.xz`) record when each key
reached the terminal, its commit and its frame callback, and haven't been
analysed yet.

## Throughput and whole-window redraws

*Smoke: one trial each, the kit and Qt Quick only.*

| | Kit | Qt Quick |
|---|---:|---:|
| **4K, a new page every frame** (3840 × 2160, scale 2, output at 1000 Hz) | | |
| Frames a second | 322 | 345 |
| App CPU per frame | 3.54 ms | 1.85 |
| App GPU per frame | none | 1.86 ms |
| Compositor CPU per frame | 1.33 ms | 0.03 |
| Compositor GPU per frame | 3.10 ms | 1.04 |
| Pixels uploaded per frame | 28.8 MB | none (GPU buffers) |
| **Fast scroll, about eight rows a frame** (1080p, 60 Hz) | | |
| Frames a second | 61.4 | 61.0 |
| App CPU per frame | 1.33 ms | 2.41 |
| App GPU per frame | none | 0.62 ms |
| Compositor CPU per frame | 0.49 ms | 0.31 |
| Compositor GPU per frame | 1.92 ms | 1.03 |

Both kept up. In the scroll the kit's program does less work than Qt
Quick's, because it moves the pixels it already drew and draws only the
rows coming into view; but the compositor has to upload the kit's damaged
regions from shared memory every frame, so the totals are close. When the
whole window changes every frame, the kit draws every pixel on the CPU and
the compositor uploads all of them, and Qt Quick's GPU path is cheaper
end to end.

A zero-copy buffer would remove the compositor's upload: the kit would
draw into a linear buffer the GPU can read in place (a `udmabuf` or linear
GBM buffer shared as a DMA-BUF) instead of `wl_shm`. It's proposed for
Hotaru first, in Sukumo's backend so every kit app gets it
([terminals.md](terminals.md#next-a-zero-copy-buffer-for-hotaru)).

## Each toolkit under its own compositor

*Smoke: one trial of 30 keys each.* The native set runs each toolkit under
its own desktop's compositor, headless on the same Radeon: the kit under
Aizome's compositor, GTK 4 and libadwaita under GNOME Shell 50.4, and Qt
under KWin 6.8.2. These runs only checked that the harness works; they
record cost, not latency. Per key, with 5 seconds of idle recorded before
the typing for comparison:

| Per key | App CPU | App GPU | Compositor CPU | Compositor GPU | Compositor idle, CPU / GPU per s |
|---|---:|---:|---:|---:|---:|
| Kit under Aizome's compositor | 0.43 ms | none | 0.80 ms | 1.22 ms | 1.2 / 2.3 ms |
| GTK 4 under GNOME Shell | 0.50 | 0.93 | 2.50 | 0.62 | 3.8 / 3.3 |
| libadwaita under GNOME Shell | 0.72 | 0.90 | 2.58 | 0.57 | 4.8 / 3.7 |
| Qt Widgets under KWin | 0.39 | none | 0.51 | 0.95 | 0 / 0 |
| Qt Quick under KWin | 1.87 | 1.20 | 0.86 | 0.67 | 0 / 0 |

The order inside the apps matches the controlled set. Among the
compositors, GNOME Shell spends the most CPU per key, and Aizome's
compositor the most GPU time, which fits it uploading shared-memory
buffers. Aizome's compositor also used GPU time while idle in this
headless mode. These are 30 keys in one trial each, so none of these
differences are established.

## Real apps

*Smoke: one trial each, under the controlled sway.* Typing at the start of
the 10,000-line file (20 keys): Sumi, Aizome's text editor, 9.3 ms at the
median (14.4 at the 90th percentile); KWrite 11.5 (16.9); GNOME Text Editor
18.1 (27.4). Scrolling the 10,000-file folder by mouse wheel (15 notches):
Dolphin 9.8 ms, Files (Nautilus) 11.6. The wheel didn't reach Hako,
Aizome's file manager, through sway's virtual pointer, so it has no
number; the same problem is known for clicks into kit windows.

## Not measured

The run was planned in chunks (`tests/performance/why-toolkits/run.sh`);
only `typing-lat` finished. Not run:

- `typing-lat2`: 240 Hz, and sway idle between frames;
- `typing-trace`: per-key stages through the tracing proxy (key received,
  commit, damage, buffer, frame callback), and the clean cost per key
  (`cost-*`) with CPU, GPU and power;
- `scroll`: paced, fast and page-at-a-time scrolling, continuous resize
  and caret blink, for all five toolkits;
- `4k`: the same at 3840 × 2160, and the drawing ceiling;
- `terms`: Hotaru and foot, three trials, with the traces analysed;
- `apps`, `native-*`: real apps and the native set, measured;
- `perf`: per-stage attribution with `perf record` and `perf stat`, the
  main thing this page set out to explain.

## Versions

From [`environment.json`](data/why-toolkits/environment.json): Linux 7.3.0
(performance governor), Ryzen 7 9800X3D, Radeon at renderD128 with Mesa
26.2.6; sway 1.12; GTK 4.22.4; libadwaita 1.9.3; Qt 6.12.0 (qtbase,
qtdeclarative); GNOME Shell 50.4; KWin 6.8.2; foot 1.28.0; GNOME Text
Editor 50.1; Nautilus 50.2.2; Dolphin 26.08.2; Aizome `aizome-0.1.0` as in
[design.md](design.md) (`sg59lw6c…`); the kit's program is built with
Cargo against this tree's `desktop/aizome/crates/aizome-kit`.

## Reproducing

```sh
# The pins (Nix out-links), tools and fixtures live in TK_WORK
# (default /tmp/claude-1000/tk): roots/, bin/, fixtures/.
tests/performance/why-toolkits/build-tools.sh "$TK_WORK/bin"
# Each chunk, inside the measurement lock:
LOCK=tests/performance/lock-measure tests/performance/why-toolkits/run.sh typing-lat 3
# Summaries, tables and compressed raw runs into data/why-toolkits:
tests/performance/why-toolkits/pack.sh
```

The matched programs build from `tests/performance/why-toolkits/apps/`
(`default.nix`). The text fixtures are in the data directory; the folder of
10,000 files isn't scripted (each file holds `fixture NNNNN`).

## Data

In [data/why-toolkits](data/why-toolkits) (see its README):
`summary-sway.json` and `tables-sway.md` for the typing-latency set,
`runs-sway.tar.xz` with every trial's records and logs,
`runs-smoke.tar.xz` with the smoke runs cited here, `environment.json`,
and the fixtures.
