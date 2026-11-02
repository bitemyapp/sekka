# How Aizome is built for speed, measured

Aizome's speed comes from a few decisions made once, at the bottom, and
kept everywhere above them. This page names each decision, says why it
helps, and gives a benchmark aimed at that decision alone, measured on
runnymede on 2 November 2026 ([the machine](README.md#the-machine)). It
ends with what the decisions cost.

The decisions, from [architecture.md](../../desktop/aizome/docs/architecture.md),
[rendering.md](../../desktop/aizome/docs/rendering.md),
[compositor.md](../../desktop/aizome/docs/compositor.md) and the
[kit's guide](../../desktop/aizome/docs/kit.md):

1. **One toolkit, drawing on the processor.** The shell and every app are
   programs on Sukumo (Sidestep's engine and native toolkit) and Aizome's
   kit. Each window is rasterised on the CPU (tiny-skia) by its render
   thread, into shared memory the compositor shows. An app opens no GPU
   context, loads no GL or Vulkan driver and links almost nothing: the
   toolkit, the kit, the fonts' engine and the app are one static Rust
   binary.
2. **Only what changed is drawn.** A message runs `update`, `view`,
   layout and a diff; only the rectangles of nodes that moved, resized or
   draw differently are invalidated, and painting skips everything outside
   them. Scrolling moves the pixels already drawn and draws only the strip
   scrolled into view. The compositor tracks damage too (smithay's damage
   tracker, GLES or pixman): moving a window composites without anything
   being drawn again, and an output with nothing new renders no frame.
3. **Every surface says what it covers.** Opaque regions let the
   compositor skip what is under them; integer scales (1×, 2×, 3×) map
   a buffer's pixels one to one onto the display's, so nothing is
   resampled.
4. **A budget of 4 ms of CPU from an event to its frame**, for every part
   of the desktop, measured in every app's tests and written up where it
   isn't met ([performance.md](../../desktop/aizome/docs/performance.md)).
5. **Nothing draws that can't be seen.** Frame callbacks fire after a
   frame that showed the surface, and about once a second for a surface
   that doesn't show (another Desk, minimised), so a hidden app that draws
   as fast as it's allowed slows to a frame a second.
6. **The GPU only where it pays.** The compositor composites with GLES,
   and falls back to pixman by itself; video goes from the decoder to the
   compositor as dmabufs, never through the player's memory or a GL context
   of its own.

## 1. The cost of a window

**What it shows:** how much a toolkit costs before an app does anything.
The same window, a label and a button in 480 × 200 points, written five
ways: in Aizome's kit, and in the toolkits GNOME's and KDE's apps use.

- `kit`: `aizome-kit` at Sekka `d7ecbac`, release build with thin LTO
  (`tests/performance/design/hello/kit`, its own workspace taking the kit
  by path, with Aizome's `Cargo.lock`).
- `gtk4`: GTK 4.22.4, `GtkApplicationWindow` with a `GtkBox`, label and
  button. `adw`: the same in libadwaita 1.9.3's `AdwApplicationWindow`
  with a header bar, as GNOME's apps are built.
- `qtwidgets`: Qt 6.12.0 Widgets, `QWidget` with `QLabel` and
  `QPushButton`. `qtquick`: Qt Quick, the same in QML (`QtQuick.Controls`),
  as Plasma's shell and newer KDE apps are built.
- All from Sekka's Nixpkgs, built by `tests/performance/design/hello/default.nix`.

**Method** (`tests/performance/design/window-cost.py`): a private headless
sway 1.12 (1920 × 1080, scale 1) rendering with GLES on the Radeon,
Mesa 26.2.6, a private D-Bus session started inside that environment, no
Xwayland. Each program's EGL picks the same GPU. The five programs ran in
turn, ten times each after an unmeasured warm-up, alternating the order.
For each run:

- **First frame**: from just before `exec` to sway's IPC `window` event
  `new` for the program's process (sway maps a toplevel on its first
  commit with a buffer), stamped with `CLOCK_MONOTONIC` as it arrives.
- **CPU to first frame**: the process's threads' run time from
  `schedstat` at that event.
- After 3 s: PSS and RSS (`smaps_rollup`, summed over the process and its
  children), threads, the shared objects mapped, and whether a GPU driver
  (Mesa's Gallium or a Vulkan driver, or NVIDIA's) is mapped.
- Then 15 s with nothing happening: context switches of every thread per
  second (wakeups), and CPU time.

**Results** ([hello-windows.json](data/design/hello-windows.json); median
of 10, range in brackets; load average 2.1 before and 0.1 after):

| | First frame, ms | CPU to first frame, ms | PSS, MiB | Private (anonymous), MiB | RSS, MiB | Threads | Shared objects | GPU driver loaded | Wakeups a second at rest |
|---|---:|---:|---:|---:|---:|---:|---:|---|---:|
| Aizome kit | **19.0** (18.3–19.7) | **18.8** | **12.4** | **4.7** | **19.0** | 5 | **13** | no | 1.0 |
| Qt 6 Widgets | 42.8 (42.4–46.8) | 45.2 | 37.7 | 11.6 | 64.9 | 8 | 98 | no | 0.07 |
| Qt 6 Quick | 86.5 (85.4–88.0) | 88.8 | 92.4 | 36.8 | 159 | 22 | 156 | GL (radeonsi) | 0.07 |
| GTK 4 | 92.7 (90.9–93.8) | 93.5 | 40.5 | 16.8 | 88.4 | 7–10 | 131 | Vulkan and GL | 0 |
| libadwaita | 115 (114–116) | 116 | 46.1 | 19.4 | 97.4 | 11 | 152 | Vulkan and GL | 0 |

What it shows:

- **A window opens in a fifth of GTK's time and holds a third of its
  memory.** The kit's window is on screen in 19 ms, 2.3× faster than Qt
  Widgets, 4.9× faster than GTK 4 and 6× faster than libadwaita. Starting
  is CPU-bound for every toolkit: the time to the first frame is the CPU
  the program spent (the two columns agree to a millisecond).
- **No GPU driver in the app.** GTK 4 opened its Vulkan renderer (the
  loader mapped radeonsi's, lavapipe's and dozen's Vulkan drivers, and
  Mesa's Gallium for GL); Qt Quick opened GL. The kit and Qt Widgets draw
  into shared memory and map neither.
- **Private memory, the part no other app shares,** is 4.7 MiB for the
  kit against 11.6–36.8 MiB. PSS counts a shared library in full when one
  process maps it, as here, so in a session with many GTK or Qt apps
  their libraries' share shrinks; the private column doesn't.
- **The one figure the kit loses: one wakeup a second at rest.** Every kit
  app checks `~/.config/aizome/settings.toml`'s modification time once a
  second on a thread of its own (aizome-config's `Watcher`), so theme and
  settings changes apply live without inotify. (A second watcher looks for
  the shell's socket once a second, but only while a session names a shell
  that isn't there yet; it didn't run here.) GTK and Qt woke 0–0.07 times
  a second. The CPU cost is 0.02 ms a second.

## 2. Whole apps, beside GNOME's and KDE's

**What it shows:** whether the toolkit's head start survives a real app.
Aizome's apps against the GNOME and KDE apps that do the same job, on the
same neutral compositor, measured the same way as the windows above
(`tests/performance/design/run-window-cost.sh`; six runs each after a
warm-up, alternating). Documents open the same file in each app: a
2,514-line Rust file (Aizome's `shell.rs`), a 6000 × 4000 JPEG, the
756-page ISO 32000-1 (Adobe's free copy, `PDF32000_2008.pdf`, SHA-256
`9de0ca9e…17ff`) and a folder of 2,000 files.

- Aizome: the `aizome` package of Sekka `d7ecbac` (cache.sekka.computer,
  `/nix/store/sg59lw6c…-aizome-0.1.0`).
- GNOME 50 apps (GNOME Calculator 50.0, Clocks 50.0, Text Editor 50.1,
  Loupe 50.0, Papers 50.2, Files 50.2.2, System Monitor 50.0, Disk Usage
  Analyzer 50.0) and KDE Gear 26.08.2 (KCalc, KWrite, Gwenview, Okular,
  Dolphin, Filelight) with Plasma System Monitor 6.8.2, all from Sekka's
  Nixpkgs.

Each app ran outside its own desktop, in sway: GNOME's apps had no GNOME
Shell and KDE's no Plasma platform integration (`QT_QPA_PLATFORMTHEME`
unset), as each would run on another desktop. A
portal and whatever the apps start over D-Bus were running after the
warm-up, as in a session. Aizome's apps had no Aizome shell either, so
their menus were in their windows. First frame is the window mapped:
document viewers in every toolkit draw the document after it.

**Results** ([small-apps.json](data/design/small-apps.json),
[document-apps.json](data/design/document-apps.json),
[monitors.json](data/design/monitors.json); medians of 6, range in
[summary.md](data/design/summary.md); load average under 1 throughout):

| Job | App | First frame, ms | PSS, MiB | Private, MiB | Shared objects | GPU driver | Wakeups a second at rest |
|---|---|---:|---:|---:|---:|---|---:|
| Calculator | **Soroban** | **19.0** | **14.3** | **5.0** | 13 | no | 1.0 |
| | GNOME Calculator | 153 | 61.0 | 25.1 | 156 | yes | 0 |
| | KCalc | 65.8 | 56.0 | 14.8 | 151 | no | 0.07 |
| Clock | **Tokei** | **19.2** | **16.2** | **5.5** | 13 | no | 5.0 (its seconds tick) |
| | GNOME Clocks | 145 | 59.3 | 24.2 | 162 | yes | 0 |
| Text editor (2,514 lines) | **Sumi** | **19.0** | **16.3** | **5.6** | 13 | no | 8.5 (its caret blinks) |
| | GNOME Text Editor | 158 | 67.5 | 26.5 | 171 | yes | 0 |
| | KWrite | 107 | 77.9 | 21.3 | 194 | no | 12.0 (its caret blinks) |
| Images (6000 × 4000 JPEG) | **Shashin** | **19.0** | **117** | 105 | 13 | no | 1.0 |
| | Loupe (4 processes) | 153 | 136 | 28.0 | 158 | yes | 0 |
| | Gwenview | 109 | 192 | 133 | 219 | no | 0.07 |
| Documents (756-page PDF) | **Shiori** (2 processes) | **19.2** | **69.5** | **55.0** | 13 | no | 5.6 |
| | Papers | 149 | 129 | 87.7 | 172 | yes | 0 |
| | Okular | 186 | 150 | 86.1 | 210 | no | 0.6 |
| Files (2,000 files) | **Hako** | **19.4** | **36.8** | **22.4** | 13 | no | 8.6 |
| | Files (Nautilus) | 171 | 97.5 | 60.9 | 179 | yes | 0 |
| | Dolphin | 114 | 70.7 | 24.2 | 213 | no | 0.07 |
| System monitor | **Myaku** | **18.9** | **43.7** | **30.1** | 17 | no | 12.9 (23.1 ms of CPU) |
| | GNOME System Monitor | 190 | 68.1 | 32.6 | 163 | yes | 47.7 (17.7 ms of CPU) |
| | Plasma System Monitor | 201 | 187 | 89.9 | 258 | yes | 105 (13.0 ms of CPU) |
| Disk usage | **Kura** | **19.2** | **15.5** | **5.3** | 13 | no | 0.98 |
| | Disk Usage Analyzer (Baobab) | 159 | 44.3 | 22.7 | 153 | yes | 5.7 |
| | Filelight | 164 | 129 | 50.5 | 226 | yes | 0 |

What it shows:

- **Every Aizome app is on screen in 18.9–19.4 ms, whatever it opens.**
  GNOME's apps took 145–190 ms and KDE's 66–201 ms, so the window is up
  3.5 to 10.6 times sooner. The kit maps the window with its first frame
  and does the slow work (decoding the picture, parsing the PDF, reading
  the folder) on other threads; the record's CPU column shows Aizome's
  apps still spent only 18–24 ms of processor time by then.
- **The head start in memory survives a real app.** The small apps hold
  5.0–5.6 MiB of private memory against 14.8–26.5 MiB for GNOME's and
  KDE's; Hako holds 22.4 MiB with 2,000 files listed, Dolphin 24.2 and
  Files 60.9.
- **Where the document lives decides the rest.** Shashin keeps the decoded
  picture in its own memory (6000 × 4000 × 4 bytes is 91.6 MiB of its
  105 MiB). Loupe decodes in sandboxed loader processes (glycin) and
  shares the picture with them, 68 MiB of its PSS being that shared
  memory, before handing it to the GPU; Gwenview holds 133 MiB privately.
  By PSS, the measure that counts shared memory once, Shashin uses the
  least: 117 MiB against 136 and 192. Shiori draws pages in a worker
  process of its own; the two together hold 69.5 MiB against Papers'
  129 and Okular's 150.
- **System monitors never rest, and Myaku spends the most processor time
  doing it.** All three read the system on a timer: Myaku every process,
  each second, for 23.1 ms of CPU a second on this machine's several
  hundred processes, against GNOME System Monitor's 17.7 and Plasma
  System Monitor's 13.0 at their own default intervals; Myaku wakes 13
  times a second to their 48 and 105.
- **Kura sizes the Nix store the first time it opens.** On its first
  launch, and after the store changes, it asks Nix for every store path
  (`nix path-info --all --json`) and keeps the total: the warm-up run,
  which did that, held 397 MiB three seconds after its first frame (373 MiB
  of it the parsed list), while the measured runs read the cached total
  and held 15.5 MiB. Neither Disk Usage Analyzer nor Filelight looks at
  anything until asked.
- **Wakeups at rest are where Aizome's apps lose.** Tokei's clock ticks
  and Sumi's caret blinks by design, but Hako's 8.6 and Shiori's 5.6
  wakeups a second are housekeeping: system-service watchers and an
  engine watchdog polling on timers
  ([thread-wakeups.txt](data/design/thread-wakeups.txt); see
  [what the design costs](#what-the-design-costs)). GNOME's apps woke
  none at all.

## 3. An idle desktop draws nothing

**What it shows:** decisions 2 and 5 at the scale of the whole desktop. A
desktop left alone should neither draw nor wake.

**Method** (`tests/performance/design/session-idle.py`): the `aizome`
package's compositor started headless, as a login starts it (it starts
the shell and xwayland-satellite), as a fresh user with Aizome's
defaults, a private runtime directory and session bus, GLES on the
Radeon. After 30 s to settle, the compositor's frame statistics were reset
(`aizomectl stats --reset`) and the session left alone for 60 s; then, per
process, CPU time (schedstat), context switches of every thread, and PSS.
Three runs at 1920 × 1080 (scale 1) and three at 3840 × 2160 (scale 2),
alternating; load average under 0.2.

**Results** ([session-idle.json](data/design/session-idle.json); medians
of three, per process):

| Output | Frames drawn in 60 s | Compositor CPU, ms/s | Compositor wakeups/s | Shell CPU, ms/s | Shell wakeups/s | Compositor PSS, MiB | Shell PSS (private), MiB | Xwayland PSS, MiB | Session PSS, MiB |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1920 × 1080 | **1** (1–4) | 0.19 | 6.9 | 1.00 | 52.8 | 66 | 60 (39) | 44 | 172 |
| 3840 × 2160 | **1** (1–1) | 0.19 | 6.9 | 1.03 | 52.8 | 67 | 80 (55) | 44 | 193 |

(xwayland-satellite: 2 MiB and no wakeups; Xwayland itself is started
with the session and then sleeps.)

What it shows:

- **One frame a minute.** In five of six runs the compositor drew one
  frame in the minute: the menu bar's clock turning over. Its processor
  time for that frame was 0.19–0.23 ms. Nothing else on the desktop
  changed, so nothing else was drawn; a compositor that redraws on a timer,
  or a shell that animates at rest, would show 60 or more frames a second
  here.
- **The 4K desktop costs 20 MiB more,** almost all of it the shell's
  private memory: its surfaces (bar, Deskbar, the desktop and its
  wallpaper) are pictures in RAM, four times the pixels at 3840 × 2160.
- **The shell wakes about 53 times a second.** It does 1 ms of work a
  second, but it isn't silent: most of its wakeups come from
  `aizome-sys`'s D-Bus name watchers, one thread per watched system
  service (UPower, NetworkManager, BlueZ and the rest), each of which
  checks for a stop request every 250 ms (`recv_timeout` in
  `crates/aizome-sys/src/bus.rs`). In the live session's shell, fourteen
  such threads made 29 of its 84 wakeups a second
  ([thread-wakeups.txt](data/design/thread-wakeups.txt)). See
  [what the design costs](#what-the-design-costs).

## 4. A working session at 240 Hz

**What it shows:** the same decisions in the session these
measurements were taken from: runnymede's own Aizome session, its
compositor on the RTX 5090 (GLES) driving one 3840 × 2160 display at
240 Hz and scale 2, with this work's agents writing into a Hotaru window
the whole time.

**Method:** two snapshots, read without disturbing the session:
`aizomectl --json stats` (the compositor's frame statistics since it
started: frames rendered, and processor time per frame over the last
4,096 frames) and `/proc` for Aizome's processes. Window titles and app
identities were left out, and the display's serial number removed.

**Results** ([live-session.json](data/design/live-session.json) at
02:31, [live-session-later.json](data/design/live-session-later.json) at
06:32):

| | 47 min after login | 4.8 h after login |
|---|---:|---:|
| Frames rendered | 53,159 | 146,990 |
| Frames a second, on average (of 240 possible) | 18.8 | 8.5 |
| Compositor CPU per frame, ms: median / p95 / p99 / max | 0.30 / 0.62 / 1.30 / 3.91 | 0.55 / 0.67 / 1.26 / 2.72 |
| Compositor CPU time, s (share of one core) | 31.8 (1.1%) | 91.9 (0.53%) |
| Shell CPU time, s | 7.9 | 45.8 |
| Compositor PSS (private), MiB | 277 (40) | 288 (52) |
| Shell PSS (private), MiB | 433 (372) | 754 (693) |

What it shows:

- **The display could have shown 240 frames a second; the compositor drew
  8.5 on average**, because frames are drawn when something changes (a
  terminal's output, the clock), not on every refresh. Each took about
  half a millisecond of processor time, and 99 in 100 within 1.3 ms.
- **The compositor's memory is mostly not its own.** Of its 288 MiB, 52 MiB
  is private; 146 MiB is file-backed (the NVIDIA driver's libraries alone
  map about 114 MiB) and 90 MiB is shared memory, the clients' window
  buffers, counted half to the compositor and half to the client.
- **The shell grew.** Its private memory went from 372 MiB to 693 MiB in
  four hours with nobody at the machine, almost all of it in nine
  anonymous regions of 54–68 MiB, the size of glibc's per-thread malloc
  arenas. A fresh shell at the same resolution holds 55 MiB (section 3).
  Whether this is pictures it keeps, fragmentation or a leak wasn't
  established here. Sampled every five minutes for the next eight and a
  half hours, with the machine in use again from the morning
  ([live-shell-memory.tsv](data/design/live-shell-memory.tsv)), it grew
  more slowly: private memory 694 → 767 MiB and PSS 754 → 828 MiB, in
  steps rather than steadily (the largest, 27 MiB, at about 10:25). See
  [what the design costs](#what-the-design-costs).

## 5. The compositor's frames at 4K

**What it shows:** what composing costs per frame, on the GPU and in
software, and that a still desktop costs no frames. The compositor's own
benchmark (`desktop/aizome/crates/aizome-comp/examples/frame_bench.rs`,
run by `tests/performance/design/run-frame-bench.sh`) starts a headless
compositor with one 3840 × 2160 output at scale 2, opens Sidestep test
windows and runs its scenarios, reading the compositor's own per-frame
statistics: the processor time from collecting the scene to submitting
the frame, and for GLES the wait on the frame's fence as GPU time. Frames
with nothing damaged are skipped and draw nothing. Three rounds, each
running GLES on the Radeon, GLES on the RTX 5090 and pixman (software) in
turn, from a release build of `d7ecbac` in Aizome's development shell;
load 0.2–2.1 at the rounds' starts. The two GLES runs took different GPUs:
their fence waits differ tenfold and more.

**Results** ([frame-bench/](data/design/frame-bench/)): milliseconds of
compositor processor time per drawn frame, the median over the three
rounds of each run's median and 99th percentile, with frames drawn:

| Scenario | Frames | GLES, Radeon | GLES, RTX 5090 | pixman (CPU) |
|---|---:|---|---|---|
| Idle, 2 s, three windows | 0 | – | – | – |
| Moving a window, 240 steps | 216–229 | 0.37 / 0.44 | 0.48 / 2.06 | 0.24 / 0.29 |
| A client drawing every frame, 3 s | 167–178 | 0.30 / 0.39 | 0.35 / 0.42 | 0.18 / 0.21 |
| Switching Desks (sliding), 12 | 26–27 | 0.33 / 1.46 | 0.44 / 0.62 | 0.18 / 0.37 |
| Overview open and close, 8 | 31 | 0.52 / 1.83 | 0.96 / 2.92 | 1.19 / 2.79 |
| Ten windows, one moving | 108–115 | 0.44 / 0.53 | 0.66 / 2.16 | 0.31 / 0.35 |
| Frosted menu, a window moving under it | 199–219 | 0.54 / 0.66 | 0.75 / 2.33 | 0.38 / 0.45 |
| Frosted menu, a client drawing under it | 157–175 | 0.35 / 0.59 | 0.61 / 2.19 | 0.30 / 0.36 |
| A full-screen client drawing every frame | 153–176 | 0.25 / 0.34 | 0.13 / 0.39 | 1.33 / 1.46 |
| Ctrl+Alt+Del's overlay over it | 2 | 0.40 / 0.64 | 0.45 / 0.60 | 0.22 / 0.32 |

GPU time (the fence) was about 1.0 ms a frame on the Radeon (2.0 under the
frosted menu, 2.6 for the full-screen client, 3.0 for the overview) and
0.01–0.6 ms on the RTX 5090. The largest single frame in any run was
3.2 ms of processor time (the overview on the RTX 5090).

What it shows:

- **A still desktop costs nothing.** Two seconds with three windows open
  drew no frames under any renderer; in five seconds of the recovery
  overlay's list refreshing, all five redraws found nothing damaged and
  were skipped.
- **Every scenario fits the 4 ms budget at 4K on every renderer**, at the
  99th percentile and at the maximum. Composing is cheap
  because only damage is drawn: moving a window repaints the strips it
  leaves and enters.
- **Software composing is the cheapest on the processor until the whole
  screen changes.** pixman spends less CPU than GLES in most scenarios,
  since GLES's CPU time includes building and submitting GPU work, but a
  client changing the whole 4K output every frame costs it 1.3 ms a frame
  against GLES's 0.1–0.3, and the overview's full-screen thumbnails 1.2 ms.
  That's why the compositor composites on the GPU and keeps pixman as the
  fallback that looks the same.
- **The NVIDIA driver's tails are longer.** On the RTX 5090 the 99th
  percentile reached 2–3 ms where the Radeon's stayed near 0.5, though its
  GPU finishes the work in a twentieth of the time.

The laptop's figures in Aizome's own record (2880 × 1800, Intel
graphics, `desktop/aizome/docs/performance.md#the-compositor`) are of the
same order: moving a window 0.46 ms (GLES) and 0.45 (pixman), a
full-screen client 0.44 and 2.31.

## 6. Scrolling moves pixels

**What it shows:** what the kit saves by moving what it already drew when
a view scrolls, and drawing only the strip that comes into view. The kit's
gallery (`crates/aizome-kit/examples/gallery`, a release build of
`d7ecbac`) has two scroll benchmarks: its 10,000-row list and table moving
two rows every 20 ms (`GALLERY_DEMO=bench-scroll`), and its editor moving
a line every 20 ms (`bench-scroll-editor`). Each ran 30 s in a private
headless sway at 2880 × 1800, scale 2, on the Radeon, with the pixels moved
(the default) and with `AIZOME_NO_PIXEL_SCROLL=1`, which makes the kit draw
the whole view again, alternating, three times each
(`tests/performance/design/scroll-ab.sh`). Sidestep's frame statistics
(`SIDESTEP_FRAME_STATS=1`) time each frame's rasterising and presenting on
the render thread; the gallery's CPU time is its utime and stime over the
run; the socket's power is from the PPT sensor.

**Results** ([scroll-ab/](data/design/scroll-ab/); medians of three runs,
range in brackets; frame times are the median of the five-second
summaries' medians, leaving out each run's first):

| | List and table, moved | drawn again | Editor, moved | drawn again |
|---|---:|---:|---:|---:|
| Frames a second | 50 | 50 | 50 | 50 |
| Gallery CPU, ms a second | **50.0** (42.3–50.7) | 86.3 (84.3–91.0) | **52.0** (49.0–57.3) | 94.0 (84.3–99.3) |
| Rasterising a frame, µs | **196** | 284 | **209** | 321 |
| Rasterising and presenting, µs | **224** | 323 | **270** | 391 |
| Socket power, W | 21.6 | 21.5 | 21.7 | 21.8 |

What it shows:

- **Moving the pixels cuts the app's processor time by 42–45 %** (50
  against 86 ms a second for the list, 52 against 94 for the editor) and
  each frame's drawing by about a third. Both modes kept up with every
  step, so the difference is all spare time: on a slower processor, or
  at a larger size, it is the difference between keeping up and not.
- **The socket's power barely tells them apart.** 36–42 ms of one core's
  time a second is a few percent of one core, and the sensor reports whole
  watts over a floor of about 21 W here. For the list the means were within
  0.1 W of each other; the editor drawn again read 0.2 W higher at the
  median (one of its runs read 26 W, the rest 21.8), which is the size of
  the sensor's wander. The saving is work not done, more than this sensor
  can weigh.

## 7. Hidden windows don't draw

**What it shows:** what a window that can't be seen costs. A GL client
that draws as fast as frame callbacks let it (`es2gears_wayland` from
mesa-demos, swap interval 1, 300 × 300) ran on Aizome's headless
compositor (the `aizome` package, GLES on the Radeon, 1920 × 1080) for
20 s visible, 20 s with Desk 2 shown, and 20 s minimised, three times,
with the client's and the compositor's CPU time over each window, the
client's own frame reports and the socket's power
(`tests/performance/design/offscreen.sh`). The compositor sends frame
callbacks after a frame that showed the surface; a surface that isn't
shown gets one at most every 995 ms, when the output draws a frame
(`send_frames` in `crates/aizome-comp-core/src/backend/mod.rs`).

**Results** ([offscreen/](data/design/offscreen/); the same in all three
rounds):

| | Visible | On another Desk | Minimised |
|---|---:|---:|---:|
| Client frames | about 56 a second | none | none |
| Client CPU, ms a second | 3.5 | 0 | 0 |
| Compositor CPU, ms a second | 9.0–9.5 | 0 | 0–0.5 |
| Socket power, W (median of three; 21.6 with no client) | 23.1 | 23.1 | 23.8 |

The client's log shows the pattern: 56 frames a second while visible,
then nothing for the 20 s on Desk 2, a frame the moment Desk 1 was shown
again, and nothing once minimised. (Its own reports cover five seconds
from its last frame, so the per-state rates in `runs.jsonl` mix states;
the log in the run's directory and the CPU counts are what to read.)

What it shows:

- **A hidden window on a still desktop costs nothing.** Its callbacks ride
  on frames the output draws anyway, at most one a second, and an idle
  output draws none, so the client was never asked for a frame: no CPU,
  no frames, for the client or the compositor. On a busy desktop it would
  be at most one frame a second.
- **The socket's power can't show it.** The windows read 21–27 W in no
  order of state (the empty desktop before the run 21.6 W, after it
  24.2), a wander larger than what a 300 × 300 client costs. CPU time is
  the measure here.

## 8. The 4 ms budget on this machine

**What it shows:** whether the desktop's own frame-budget tests, which
Aizome's docs record on an Intel Core Ultra 7 365 laptop, hold on this
desktop. Each app's tests drive it headlessly through the kit's offscreen
renderer at scale 2 and time a frame from the event through update, view,
layout, diffing and rasterising the damage; the 4 ms budget is the whole
of that (`desktop/aizome/docs/performance.md`). Release builds of
`d7ecbac` run by `tests/performance/design/budget.sh`, each test alone
under the measurement lock. The time and load in each output's header are
when the test was queued, not when it ran.

**Results** ([budget/](data/design/budget/); milliseconds of processor time
per frame as each test prints them; the laptop's median and 95th
percentile from Aizome's record where it has the same case):

| Interaction | p50 | p95 | max | Laptop p50 / p95 |
|---|---:|---:|---:|---|
| Hover a button (settings pane, ~200 widgets) | 0.01 | 0.01 | 0.08 | |
| Toggle a switch | 0.03 | 0.04 | 0.06 | |
| Type a character in a text field | 0.06 | 0.07 | 0.18 | |
| Type a character in a 2,000-line editor (the kit) | 0.22 | 0.37 | 1.75 | 0.29 / 0.51 |
| Scroll a 10,000-row list a notch | 0.12 | 0.16 | 0.30 | 0.15 / 0.20 |
| Move a 10,000-row list's selection by a row | 0.09 | 0.11 | 0.13 | |
| The shell: the clock's tick | 0.07 | 0.12 | 0.18 | 0.11 / 0.13 |
| The shell: a meter updating | 0.04 | 0.04 | 0.05 | |
| The shell: a window opening and closing | 0.24 | 0.38 | 1.38 | |
| The shell: a key typed in the palette | 0.40 | 0.81 | 0.83 | 0.74 / 1.80 |
| The shell: an arrow in the palette | 0.15 | 0.21 | 0.24 | |
| Sumi: typing | 0.38 | 0.47 | 0.81 | |
| Sumi: scrolling | 0.33 | 0.40 | 0.55 | |
| Hako: ↓ in a column of 10,000 files | 1.20 | 1.32 | 1.37 | 2.28 / 2.44 |
| Hako: scrolling a column of 10,000 | 0.28 | 0.29 | 0.41 | |
| Hako: ↓ in a list of 10,000 | 0.58 | 0.59 | 0.70 | |
| Myaku: a new reading of 600 processes | 2.26 | 2.58 | 2.69 | 2.61 / 2.99 |
| Myaku: the same, one GPU card shown | 1.12 | 1.43 | 1.47 | |
| Myaku: clicking a row | 0.19 | 0.25 | **4.36** | |
| Myaku: scrolling | 0.27 | 0.35 | 0.38 | |

The palette's first opening took 0.93 ms after the shell warmed it at
idle (3.0 ms of warming and 2.6 ms of drawing ahead, done once, before
anyone asks).

What it shows:

- **Every median and 95th percentile fits the budget with room to
  spare,** the slowest being Myaku's reading of 600 processes (2.3 ms,
  2.6 ms at the 95th percentile, 2.7 ms at worst).
- **One frame went over:** one of Myaku's 30 timed row clicks took
  4.36 ms, the rest within 0.25 ms at the 95th percentile. Aizome's record
  judges the budget at the 95th percentile, and its harness figures are
  upper bounds (a frame that covers the window also pays for the harness's
  offscreen buffer, about 1 ms); on the laptop the same click's worst was
  11 ms. Myaku's memory stayed put over 200 readings (234 MiB before and
  after, the test's own check).
- **This desktop core is faster than the laptop's** by 1.15–1.9 times on
  the cases both measured; the budget was set on the slower machine.
- These are headless and in-process: they time the app's own work for a
  frame, not the compositor's or the display's (sections 4 and 5 time
  those).

## What the design costs

The same measurements show where the decisions cost something, or where
Aizome hasn't yet done what its design allows:

- **Apps and the shell aren't silent at rest yet.** GNOME's apps woke
  0 times a second in these tests; Aizome's woke 1–8.6 times, and the
  shell about 53. Per thread
  ([thread-wakeups.txt](data/design/thread-wakeups.txt), from
  `tests/performance/design/thread-wakeups.sh` and the live shell):
  - every kit app checks its settings file's modification time once a
    second (the price of applying settings live without inotify; 0.02 ms
    of CPU a second);
  - `aizome-sys`'s D-Bus watchers, one thread per watched service, check
    for a stop request every 250 ms: four wakeups a second each, most of
    the shell's and two of Hako's threads;
  - Shiori's engine watchdog ticks four times a second even when no page
    is being drawn (it times out a worker stuck on one);
  - Sumi's caret blinks for as long as the window has the keyboard (main
    and render thread each wake about four times a second); GTK stops
    blinking after 10 s without typing, and KWrite doesn't stop either;
  - Tokei's world clock ticks every second, as it should while it shows
    seconds.

  Each is small (0.02–1 ms of CPU a second), but a wakeup can keep a core
  from its deepest idle state. A watcher that blocks until it's stopped,
  a watchdog that sleeps while nothing is in flight, a caret that stops
  blinking and an inotify watch for the settings would take most of them
  to zero.
- **Static binaries don't share code.** Every app is one Rust program
  with the toolkit inside (13–24 MB each, the shell 30 MB; the `aizome`
  package is 507 MiB for some 30 programs). Two Aizome apps open at once
  each map their own copy of the toolkit's code (10–13 MiB of file-backed
  PSS each), where two GTK apps share one `libgtk`. The private memory
  the apps start with is still a third or less of GTK's and Qt's.
- **Drawing on the CPU keeps pictures in RAM.** A surface the shell draws
  is held as pixels in memory: the shell's private memory was 39 MiB at
  1920 × 1080 and 55 MiB at 3840 × 2160 (section 3), and Shashin holds a
  photo's decoded pixels itself (section 2). A GPU-drawn shell would hold
  the same pictures as textures, in video memory instead.
- **The live shell's memory grew.** Over four unattended hours of the
  session in section 4, the shell's private memory went from 372 MiB to
  693 MiB, in glibc malloc arenas of its worker threads, and to 767 MiB
  over the next eight and a half hours. A fresh shell at the same
  resolution holds 55 MiB. The cause wasn't established here.
- **CPU-drawn buffers are copied to the GPU on every update.** The
  compositor uploads the changed part of a shared-memory buffer into a
  texture; a GL client's buffer is on the GPU already. Damage tracking
  keeps the copies to what changed, but a client that changes most of its
  window every frame pays for the copy: in [terminals.md](terminals.md),
  a one-row scroll at 60 Hz cost sway 91 ms of CPU per 120 updates with
  Hotaru and 43 ms with Ghostty, which draws with GL. The copy isn't the
  whole story: typing cost sway 4.3 ms of CPU a second with Hotaru and
  1.6 ms with Konsole, which also hands it CPU-drawn buffers, so Hotaru
  likely commits more often or damages more than a keystroke needs. That
  wasn't isolated.

## Reproducing

From the repository root on a NixOS machine with a GPU (the scripts
assume runnymede's render nodes: `/dev/dri/renderD128` for Mesa and
`renderD129` for NVIDIA; change them in the scripts for another
machine). `LOCK` and `LOCK_BUILD` are optional commands each step runs
under; here they were `tests/performance/lock-measure` and `lock-build`.

```sh
# What is measured: Aizome (from cache.sekka.computer), the hello programs
# and the kit's hello program, and the fixtures.
nix build .#aizome --out-link /tmp/aizome
nix build --impure --out-link /tmp/hello --expr 'let f = builtins.getFlake (toString ./.); pkgs = f.inputs.nixpkgs.legacyPackages.x86_64-linux; in pkgs.callPackage ./tests/performance/design/hello { }'
(cd tests/performance/design/hello/kit && nix develop ../../../../..#aizome -c env CARGO_TARGET_DIR=/tmp/kit cargo build --release)
F=/tmp/fixtures; mkdir -p $F/folder
curl -sSL -o $F/PDF32000_2008.pdf https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/PDF32000_2008.pdf
nix shell nixpkgs#imagemagick -c magick -seed 7 -size 6000x4000 plasma:fractal -blur 0x1 -quality 90 $F/photo.jpg
for i in $(seq -w 1 2000); do case $((10#$i % 5)) in 0) e=txt;; 1) e=png;; 2) e=pdf;; 3) e=rs;; 4) e=md;; esac; printf 'fixture %s\n' $i > $F/folder/file-$i.$e; done
cp desktop/aizome/crates/aizome-shell/src/shell.rs $F/editor-fixture.rs

# 1 and 2: windows and apps (four sets; ONLY=set,set runs some of them).
tests/performance/design/run-window-cost.sh docs/performance/data/design /tmp/aizome /tmp/hello /tmp/kit/release/hello-kit $F
nix shell nixpkgs#python3 -c python3 tests/performance/design/summarize.py docs/performance/data/design/{hello-windows,small-apps,document-apps,monitors}.json > docs/performance/data/design/summary.md

# 3: the idle session.
nix shell nixpkgs#python3 nixpkgs#dbus -c python3 tests/performance/design/session-idle.py \
  docs/performance/data/design/session-idle.json /tmp/aizome/bin 1920x1080@1 3840x2160@2 --reps 3 --settle 30 --idle 60

# Per-thread wakeups at rest.
PATH=/tmp/aizome/bin:$PATH nix shell nixpkgs#sway nixpkgs#dbus -c tests/performance/design/thread-wakeups.sh \
  "aizome-hako $F/folder" "aizome-shiori $F/PDF32000_2008.pdf" "aizome-sumi $F/editor-fixture.rs" aizome-tokei

# 5 to 8 need a release build of the compositor, its examples and the kit's gallery.
(cd desktop/aizome && nix develop ../..#aizome -c env CARGO_TARGET_DIR=/tmp/comp cargo build --release \
  -p aizome-comp -p aizome-comp-logic -p aizome-kit --bin aizome --examples)
tests/performance/design/run-frame-bench.sh docs/performance/data/design/frame-bench /tmp/comp 3
cc -O2 -o /tmp/powermeter tests/performance/powermeter.c
PATH=$(nix build --no-link --print-out-paths nixpkgs#sway)/bin:$PATH nix develop .#aizome -c tests/performance/design/scroll-ab.sh docs/performance/data/design/scroll-ab /tmp/comp/release/examples/gallery /tmp/powermeter 3 30
tests/performance/design/offscreen.sh docs/performance/data/design/offscreen /tmp/aizome/bin "$(nix build --no-link --print-out-paths nixpkgs#mesa-demos)/bin/es2gears_wayland" /tmp/powermeter 3 20
tests/performance/design/budget.sh docs/performance/data/design/budget /tmp/comp "$AIZOME_FONT_DIR"
```

Section 4 is a snapshot of a live session: `aizomectl --json stats`
and `/proc/PID/smaps_rollup` of the compositor and shell, read while the
session runs.
