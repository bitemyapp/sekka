# Performance: the dataset behind aizome.computer/performance

Everything the [performance page](https://aizome.computer/performance/)
claims comes from the measurements here: each topic's write-up, its raw
data under `data/`, and the harness that produced it under
`tests/performance/`. Where a number on the page came from an earlier
record (`desktop/aizome/docs/performance.md`, `docs/test-results/`), the
write-up names it.

| Topic | Write-up | Data | Harness |
|---|---|---|---|
| How Aizome is built for speed, with targeted benchmarks | [design.md](design.md) | [data/design](data/design) | [tests/performance/design](../../tests/performance/design) |
| Idle-update follow-up, 6–7 November 2026 | [design.md §9](design.md#9-idle-updates-november-2026) | [data/design/idle-updates-2026-11-07](data/design/idle-updates-2026-11-07) | [supplemental fixtures](../../tests/performance/design/idle-updates) |
| Aizome, GNOME and KDE Plasma in a small VM | [desktops.md](desktops.md) | [data/desktops](data/desktops) | [tests/performance/desktops](../../tests/performance/desktops) |
| Hotaru against Ghostty, GNOME's and KDE's terminals | [terminals.md](terminals.md) | [data/terminals](data/terminals) | [tests/performance/terminals](../../tests/performance/terminals) |
| Utsushi's energy against mpv and VLC | [media.md](media.md) | [data/media](data/media) | [tests/performance/media](../../tests/performance/media) |
| Shiori's renderer (hayro) against PDFium and others, and conformance | [pdf.md](pdf.md), with [pdf-verdicts.md](pdf-verdicts.md) | [data/pdf](data/pdf) | [tests/performance/pdf](../../tests/performance/pdf) |

**Why the numbers come out as they do.** Three analyses take the
comparisons above apart, each with its own data and harness. They aren't on
the performance page yet; the third is incomplete.

| Question | Write-up | Data | Harness |
|---|---|---|---|
| Where apps' startup time and memory go, toolkit by toolkit | [why-apps.md](why-apps.md) | [data/why-apps](data/why-apps) | [tests/performance/why-apps](../../tests/performance/why-apps) |
| Why Aizome uses less memory, logs in faster and shows keys sooner than GNOME and Plasma in the VM, and where it loses | [why-desktop.md](why-desktop.md) | [data/why-desktop](data/why-desktop) | [tests/performance/why-desktop](../../tests/performance/why-desktop) |
| Toolkits at work: typing latency, cost per interaction, throughput (incomplete) | [why-toolkits.md](why-toolkits.md) | [data/why-toolkits](data/why-toolkits) | [tests/performance/why-toolkits](../../tests/performance/why-toolkits) |

## The machine

All of it was measured on one desktop, runnymede, on 2 November 2026,
unless a write-up says otherwise:

- AMD Ryzen 7 9800X3D (8 cores, 16 threads; SMT siblings N and N+8),
  62 GiB RAM, NVMe SSD.
- Two GPUs: the processor's Radeon graphics (radeonsi, Mesa 26.2.6,
  `/dev/dri/renderD128`) and an NVIDIA GeForce RTX 5090
  (`/dev/dri/renderD129`), which drives the display. Benchmarks that
  measure power render on the Radeon, so that its work is inside the
  processor's power sensor.
- NixOS (Sekka), Linux 7.3.0 with `preempt=full rcu_nocbs=all`,
  amd-pstate active with the `performance` governor and energy
  preference. That keeps the idle floor high (about 18 W for the socket)
  and makes deltas over an idle baseline the figures to compare.
- Sekka at `d7ecbac` (origin/main on 2 November 2026); every comparison
  program comes from Sekka's own Nixpkgs pin, so its versions are the ones
  a Sekka system installs.

## Power

There is no battery to drain on a desktop, so energy comes from the
processor's own sensors:

- **PPT**: the amdgpu driver's `power1_input` (label `PPT`) is the
  socket's power as the SMU reports it: the CPU cores, the integrated GPU
  and the I/O die, not the NVIDIA card, RAM or the power supply. Any user
  can read it; it updates about every 7 ms in whole watts (the value is in
  mW, though hwmon's ABI says µW). `tests/performance/powermeter.c` samples
  it every 10 ms while a command runs and integrates the samples
  (trapezoids) into joules. Calibration: 18.2 W idle, 68.8 W with four
  threads spinning, about 126 W with eight.
- **RAPL**: Aizome's root `aizome-power-meter` service answers on
  `/run/aizome/power.sock` with the package energy counter, refreshed once
  a second. Over the same 20 s window PPT gave 69.5 W and RAPL 70.6 W,
  1.5% apart.

## Measurement protocol

The original 2 November measurements followed the protocol below.
Follow-ups record their dates and exceptions in their write-ups: the
6 November latency rerun had unrelated background jobs; those jobs were
stopped for the 7 November energy rerun. Its idle measurements use RAPL
boundary reads, and report absolute package power.

- One measurement at a time: five tracks of work shared the machine, and
  every measured run held an exclusive lock that also held off builds,
  encodes and test runs (`tests/performance/lock-measure`, `lock-build`).
  Once, from 11:14 to 11:37, a headless sway left over from a failed run
  kept holding the lock, so nothing ran at all; the lock now closes its
  descriptor before running the command (`flock -o`), so a leftover
  process can't hold it.
- Subjects alternate (A B C, A B C …) rather than running all of one and
  then all of the next, with an unmeasured warm-up first, and each write-up
  reports medians with the spread.
- Comparison programs run as Nixpkgs builds them, with their defaults
  unless a write-up says what was changed and why (for matched settings,
  such as a terminal's font and scrollback, both the matched and the
  out-of-the-box figures are given).
- Private sessions: headless compositors with their own runtime
  directory, home and D-Bus; nothing ran on the display or in the user's
  session, which was only observed (its compositor's statistics and its
  processes' memory, in design.md); nothing was audible. One harness
  broke this once: why-desktop.md's first sandbox shared the host's
  `/tmp`, and its GNOME Shell removed the session's Xwayland lock file,
  taking it for stale. The lock was restored and the harness isolated;
  its Reproducing section says how.
- The machine was unattended overnight; from about 10:00 its owner used
  the desktop again, which raised the socket's idle floor by up to 2 W.
  Every energy figure is a difference from a baseline taken beside it,
  and the write-ups say where that moved a result.
- What each number is: component evidence from headless compositors and
  disposable VMs, not physical-display latency (no photon was timed), and
  not battery life.

## Follow-up work these records point to

- **A zero-copy buffer for Hotaru, in Sukumo's backend.** CPU-drawn frames
  reach the compositor as shared memory, which it has to copy into a
  texture: sway spends about twice the CPU per scroll update on Hotaru that
  it does on Ghostty, and at 4K a full kit frame costs the compositor
  3.1 ms of GPU time uploading 28.8 MB. Drawing into a linear buffer the GPU
  reads in place (a `udmabuf` or linear GBM buffer shared as a DMA-BUF)
  would remove the copy ([terminals.md](terminals.md#next-a-zero-copy-buffer-for-hotaru),
  [why-toolkits.md](why-toolkits.md#throughput-and-whole-window-redraws)).
- **Hotaru's extra frame of typing latency** is in its own PTY-to-frame
  path, not Sukumo's; the proxy traces recorded in why-toolkits.md's smoke
  runs are the place to start.
- **Aizome's remaining idle wake-ups:** settings polling was removed on
  6 November; the kit window went from 1 to 0 wakeups/s. Service watchers,
  watchdogs and caret timers remain ([idle-update follow-up](design.md#9-idle-updates-november-2026),
  [original breakdown](why-desktop.md)).
- **Aizome's wallpaper** costs its shell 24 MiB for an 8 MiB picture
  ([why-desktop.md](why-desktop.md)).
- **The kit's font discovery** took 12 of an app's 19 ms to its first
  frame ([why-apps.md](why-apps.md)); PR #263 makes it lazy.
- **The rest of why-toolkits.md:** the clean cost per key, scrolling,
  resizing, 4K, the native compositors measured, and the per-stage
  attribution.
