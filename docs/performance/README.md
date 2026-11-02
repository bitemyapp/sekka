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
| Aizome, GNOME and KDE Plasma in a small VM | [desktops.md](desktops.md) | [data/desktops](data/desktops) | [tests/performance/desktops](../../tests/performance/desktops) |
| Hotaru against Ghostty, GNOME's and KDE's terminals | [terminals.md](terminals.md) | [data/terminals](data/terminals) | [tests/performance/terminals](../../tests/performance/terminals) |
| Utsushi's energy against mpv and VLC | [media.md](media.md) | [data/media](data/media) | [tests/performance/media](../../tests/performance/media) |
| Shiori's renderer (hayro) against PDFium and others, and conformance | [pdf.md](pdf.md) | [data/pdf](data/pdf) | [tests/performance/pdf](../../tests/performance/pdf) |

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

## Rules every measurement followed

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
  processes' memory, in design.md); nothing was audible.
- The machine was unattended overnight; from about 10:00 its owner used
  the desktop again, which raised the socket's idle floor by up to 2 W.
  Every energy figure is a difference from a baseline taken beside it,
  and the write-ups say where that moved a result.
- What each number is: component evidence from headless compositors and
  disposable VMs, not physical-display latency (no photon was timed), and
  not battery life.
