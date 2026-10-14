# Interactive scheduling: follow-ups

Two ways to keep applications responsive under heavy background work. Neither
is implemented yet; this records what is known about each, so the follow-up
can start from measurements rather than claims. What Sekka does today is in
[tuning](tuning.md): full preemption, mq-deadline, and ananicy rules that
lower compilers and keep shells at normal priority.

Numbers below were gathered in October 2026. Most interactive figures come
from games or microbenchmarks; nobody has published a controlled measurement
of desktop applications under a background build for either approach.

## Display priority through short time slices

BeOS and Haiku give each thread a priority. Window threads run at display
priority, above normal threads, so drawing and input win over background work
without the application asking for more CPU. Linux's EEVDF scheduler has a
close equivalent: a short time slice.

**How it works.** Since Linux 6.12, a normal task can request its own slice
with `sched_setattr()` (`sched_attr.sched_runtime`, 0.1 to 100 ms, no
privilege needed). A shorter slice gives the task an earlier deadline, so the
scheduler picks it sooner when it wakes, while its share of CPU time stays the
same. `chrt -o -T <ns> 0 <command>` sets it (util-linux 2.41 or later).

**On ganymede** (Linux 7.2.9, 8 CPUs), checked directly:

- The default slice is 2.8 ms: 0.70 ms × (1 + log2 of the CPU count).
- An unprivileged shell can set a 1 ms slice.
- Children inherit a custom slice. With reset-on-fork (`chrt -R`), children
  go back to the default 2.8 ms.

**Published effects:**

| Change | Effect | Conditions | Source |
|---|---|---|---|
| Shorter slices may preempt on wakeup (6.12) | cyclictest average delay 0.45–0.48 → 0.37 ms; maximum not improved | Author and tester, 2024 | [85e511df](https://github.com/torvalds/linux/commit/85e511df3cec) |
| Short-slice task among default-slice tasks | p90/p99 wakeup latency 63/76 µs against 1237/4575 µs; no noticeable hackbench change | Author, June 2026; series lands in 7.3, not 7.2 | [LKML](https://lkml.rescloud.iu.edu/hypermail/linux/kernel/2606.1/15109.html) |

**Measured on ganymede** ([responsiveness](responsiveness.md)). A frame probe
doing 2 ms of work per 120 Hz frame did worse with a 1 ms slice than with the
default:
- Before Sekka's tuning changes, it missed 7% of frames during a terminal
  build, against 4.1% with the default slice.
- After them, it missed 0.4% against 0.3%.

The work no longer fits in one slice, so the frame can be preempted halfway
through. A slice helps a thread that should be picked up quickly; it should
still cover the thread's longest burst. That favours slices sized per
application, or per thread, over one short slice for everything.

**What to prototype.**

1. Start UI applications with a shorter slice from Tatami's launcher, which
   runs every application through `systemd-run`. Size it to cover a frame's
   work (the 1 ms tried above was too short for 2 ms frames), and try it on
   Linux 7.3, where shorter slices preempt more readily.
2. Keep the slice from spreading to work the application starts. A terminal's
   builds would otherwise inherit it and switch tasks more often. systemd can
   set reset-on-fork for a unit, but only with an explicit policy:
   `CPUSchedulingPolicy=other` and `CPUSchedulingResetOnFork=yes` together.
   `CPUSchedulingResetOnFork=yes` alone did nothing on ganymede. systemd has
   no property for the slice itself, so the launcher has to set it before
   exec.
3. Decide whether a whole process should get the slice or only its UI
   threads. A launcher can only set the process's initial threads; threads
   created later inherit from their creator.
4. Leave the compositor alone: Hyprland already runs SCHED_RR.

ananicy cannot set slices. Its `latency_nice` setting targets an
out-of-tree kernel patch and is off on stock kernels.

**How to measure.** Compare the responsiveness harness's frame probe with
and without a short slice, under the same build load.

## sched-ext schedulers: held

The kernel can load a scheduler written in BPF (sched-ext) in place of
EEVDF; NixOS has `services.scx`. The candidates for desktops are scx_lavd
(Igalia, funded by Valve), scx_bpfland, and scx_flash. Sekka does not use any
of them yet.

| Scheduler | Interactive vs. EEVDF | Throughput / power | Conditions | Source |
|---|---|---|---|---|
| lavd | Forza Horizon 5: 59 vs 58 FPS; stutters 46 vs 87 | 15.4 vs 15.5 W | Steam Deck OLED, author, 2024 | [LPC 2024](https://lpc.events/event/18/contributions/1713/attachments/1425/3058/scx_lavd-lpc-mc-24.pdf) |
| lavd | Resident Evil 2 1% lows: 41.6–41.9 vs 36.7–38.8 FPS; frames over 25 ms: 0–1 vs 20–44 | Uncapped: EEVDF 4–6 FPS faster; lavd 12–15% more CPU per frame, 10.4 vs 8.9 W | Retroid Pocket 6, independent, Oct 2026 | [pocknix-os#118](https://github.com/shuuri-labs/pocknix-os/pull/118) |
| lavd, flash | Starfield 1% lows 33.9, 33.4 vs 30.7 FPS; Cyberpunk within noise | – | Intel hybrid Core Ultra, kernel 7.1, no background load, one user | [GamingOnLinux](https://www.gamingonlinux.com/forum/topic/6679/) |
| bpfland | Cyberpunk with 16 CPU hogs: 113 vs 28 FPS (EEVDF with `preempt=full`: 87) | Removed by its own watchdog in 4 of 4 hackbench runs | Ryzen 9800X3D, one user; EPYC, open bug | [bazzite#3313](https://github.com/ublue-os/bazzite/issues/3313), [scx#3878](https://github.com/sched-ext/scx/issues/3878) |
| rustland | WebGL demo during `make -j32`: 60 vs 28 fps | – | i7-1195G7, author, 2024 | [scx commit](https://github.com/sched-ext/scx/commit/d0fb29a0f74dd3cc2938f6f486f497812e421d11) |

**Why it is held.**

- **Per-application fairness would go.** lavd and bpfland ignore cgroup
  `cpu.weight`, so the equal share each application's cgroup gets today
  would no longer protect, say, a browser from a build started in a
  terminal. Of the candidates, only scx_flash honours cgroup weights.
- **Idle builds would lose their edge.** Under EEVDF, any normal task that
  wakes preempts a SCHED_IDLE task at once, which is what keeps Nix builds
  out of the way. These schedulers treat SCHED_IDLE as nice 19.
- **Failure is slow.** On an error or a stall, the kernel returns to EEVDF,
  but only after up to 30 s (lavd) or 5 s (bpfland).
- **lavd lags on CPUs like ganymede's.** Two fixes that matter on a CPU
  without SMT landed in September 2026, after release 1.1.3. nixpkgs ships
  1.1.2.
- **NixOS's default is the wrong one.** `services.scx` defaults to
  scx_rustland, so a scheduler has to be chosen explicitly.

**When to revisit.** Once a lavd release includes those fixes, A/B it
against EEVDF on ganymede with the responsiveness harness, under the same
build load. If per-application fairness matters in that test, include
scx_flash.
