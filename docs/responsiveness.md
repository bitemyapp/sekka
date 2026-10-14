# Responsiveness under load

`tests/responsiveness/run.sh` measures how well the desktop keeps up while
builds run. It runs on the live session of a real machine, not in a VM: CPU,
disk and kernel behaviour are what is being measured. Use it to compare two
system configurations, one measurement each, under the same conditions (power
source, power profile, open applications).

```sh
tests/responsiveness/run.sh measure before   # about 15 minutes
# switch configuration, reboot
tests/responsiveness/run.sh measure after
tests/responsiveness/run.sh compare before after
```

`measure` starts a user service, `sekka-responsiveness`, and returns. The
service holds an idle, sleep and lid-switch inhibitor while it runs. Results go
to `.work/responsiveness/LABEL/`; `summary.json` collects them.

## Scenarios

Each scenario starts the probes, runs a load, and stops the probes when the
load ends. Thirty seconds of rest separate scenarios. An unmeasured warm-up of
both builds runs first, so downloads and the page cache don't favour either
measurement.

| Scenario | Load |
|---|---|
| idle | 60 s of the session as it is |
| nix-1, nix-2 | ripgrep, fd and bat built by nix-daemon from the pinned Nixpkgs, with a changed attribute so they rebuild |
| terminal-1, terminal-2 | release builds of Yukimi and Genkan with cargo in the repository's development shell, started from a shell 3 s after it opens, as a person would |

The terminal scenario runs as its own systemd unit, as a terminal application
would. Its shell is a process named `bash`, so ananicy's shell rule applies to
it, and cargo inherits the result. Beside it, in the same cgroup, a frame probe
named `foot` stands in for the terminal drawing its window.

## Probes

Every probe except the terminal's runs in its own unit at nice 0, as an
application of its own.

| Probe | Measures |
|---|---|
| wake | How late a thread wakes from a 1 ms sleep |
| frame | An application drawing at 120 Hz: each frame wakes at its deadline and does 2 ms of work (calibrated once on an idle CPU and reused). Frame time runs from the deadline to the end of the work; a frame over 8.3 ms misses the next refresh |
| frame, 1 ms slice | The same with a 1 ms EEVDF time slice (`chrt -T`) instead of the default 2.8 ms, for [display priority](interactive-scheduling.md) |
| terminal frame | The frame probe inside the terminal build's cgroup, named `foot` |
| exec | Time to start and finish `foot --version`, four times a second |
| 4K read | fio: 200 random 4 KiB direct reads per second from a 1 GiB file |
| fsync | fio: 20 4 KiB writes per second, each followed by fsync |

The run also records PSI stall time (`/proc/pressure`), the build's duration,
and every 5 s the scheduling class and nice value of the build processes
(`priorities.log`). `meta.txt` records the kernel, its command line, the NVMe
scheduler, power state, and the nice value ananicy gives a new shell.

## Caveats

- The frame probe runs on whichever core the scheduler picks. On a hybrid CPU
  the same work takes longer on an efficiency core, so idle frame times
  already vary (2.6 ms median and 5.5 ms p99 on ganymede for 2 ms of work
  calibrated on a performance core).
- One run per configuration covers only the conditions of that run. Battery
  versus mains power, a thermal limit, or other work on the machine can
  change the numbers more than the configuration does.
- A reboot between measurements also empties the page cache. The warm-up
  limits that effect for the builds and the test files, not for the rest of
  the session.

## Results

### ganymede, October 13, 2026

ThinkPad X1 Carbon Gen 14 (Core Ultra 7 365: 4 performance and 4 efficiency
cores, no SMT), 30 GiB RAM, ext4 on NVMe. Mains power, platform profile
"performance", Linux 7.2.9 in both runs. One measurement each, each scenario
run twice and averaged:

- **baseline**: Sekka nightly (system generation 24): Kyber, lazy preemption,
  CachyOS's ananicy rules. Measured after 7 hours of uptime.
- **after**: branch `gentle-build-rules` at `c90e9a2`, booted as generation
  25: mq-deadline, `preempt=full rcu_nocbs=all`, shells at nice 0, compilers
  at nice 3. Measured from 5 minutes after the reboot.

Summaries: [baseline](test-results/responsiveness-ganymede-baseline.json),
[after](test-results/responsiveness-ganymede-after.json).

| Metric | idle, baseline | idle, after | Nix build, baseline | Nix build, after | terminal build, baseline | terminal build, after |
|---|---|---|---|---|---|---|
| Load duration (s) | 60 | 60 | 89.1 | 87.5 | 137 | 132 |
| CPU stall, some (s) | 0.5 | 0.1 | 28.1 | 25.6 | 16.8 | 13.1 |
| I/O stall, some (s) | 0.3 | 0.6 | 3.7 | 3.7 | 1.7 | 2.5 |
| I/O stall, full (s) | 0.3 | 0.6 | 1 | 2.2 | 0.9 | 1.1 |
| Memory stall, full (s) | 0 | 0 | 0 | 0 | 0 | 0 |
| Wakeup late p99 (µs) | 105 | 95 | 825 | 145 | 2147 | 1477 |
| Wakeup late max (µs) | 9795 | 1797 | 4797 | 4721 | 20827 | 9398 |
| Frame time p99 (µs) | 2761 | 2776 | 4337 | 3497 | 11904 | 6750 |
| Frames missed at 120 Hz (%) | 0 | 0 | 0 | 0 | 4.1 | 0.3 |
| Frames over 16.7 ms (%) | 0 | 0 | 0 | 0 | 0.1 | 0 |
| 1 ms slice: frame time p99 (µs) | 3746 | 2783 | 4683 | 4277 | 12569 | 7511 |
| 1 ms slice: frames missed (%) | 0 | 0 | 0 | 0 | 7 | 0.4 |
| Terminal frames missed (%) | – | – | – | – | 0.6 | 0.1 |
| Terminal frame time p99 (µs) | – | – | – | – | 7577 | 5392 |
| Program start p50 (µs) | 2698 | 3111 | 2608 | 2191 | 2430 | 2181 |
| Program start p99 (µs) | 4112 | 3554 | 7745 | 5568 | 14704 | 8394 |
| 4K read p50 (µs) | 84.5 | 88.6 | 106 | 101 | 106 | 97.8 |
| 4K read p99 (µs) | 194 | 913 | 3858 | 4284 | 1995 | 3162 |
| 4K read p99.9 (µs) | 659 | 4751 | 18055 | 24904 | 6291 | 9847 |
| fsync p50 (µs) | 1303 | 1434 | 2408 | 1425 | 1855 | 1434 |
| fsync p99 (µs) | 2023 | 11469 | 169083 | 120062 | 12321 | 21856 |

**CPU: clearly better, and builds were no slower.**

- During terminal builds, another application missed 0.3% of its 120 Hz
  frames instead of 4.1% (35 and 71 frames in the two runs, against 648 and
  672). Its p99 frame time fell from 11.9 to 6.8 ms. Its worst wakeup fell
  from 20.8 to 9.4 ms.
- Inside the terminal's own cgroup, missed frames fell from 0.6% to 0.1%.
- Program start p99 fell from 14.7 to 8.4 ms during terminal builds, and from
  7.7 to 5.6 ms during Nix builds.
- During Nix builds, p99 wakeup lateness fell from 825 to 145 µs (821 and
  828 against 141 and 149). Nix builds ran at SCHED_IDLE in both runs, and
  the ananicy changes don't apply to them, so this difference comes from
  full preemption (with RCU offloading).
- Build times: Nix 89.1 → 87.5 s, terminal 137 → 132 s.
- The sampled priorities confirm the rules. Baseline: shell and `rustc` at −4.
  After: shell at 0 and `rustc` at 3, except samples from a compiler's first
  half second, which were still at 0.

**I/O: worse at the tail.**

- 4K read p99 during terminal builds rose from about 2.0 to 3.2 ms, in both
  runs. p99.9 rose in every scenario.
- Full I/O stall during Nix builds doubled, from 1.0 to 2.2 s.
- fsync p99 varied too much between runs to compare (Nix builds: 68 and
  271 ms before, 131 and 110 ms after).
- Most of a build's disk traffic is buffered writeback, submitted by kernel
  threads at normal priority, so mq-deadline's priority classes don't
  deprioritize it. Kyber throttles writes to keep reads near its 2 ms
  target.
- The idle scenario also got worse (read p99 0.19 → 0.91 ms, fsync p99
  2.0 → 11.5 ms), where the scheduler has little to arbitrate. That points
  to activity in the first minutes after the reboot as a second cause. These
  two runs cannot separate the causes. Switching the scheduler at runtime
  within one boot (`/sys/block/nvme0n1/queue/scheduler`) and alternating
  would.

**A 1 ms slice did not help.** It was worse than the default 2.8 ms in the
baseline (7% of frames missed against 4.1%) and about equal after (0.4%
against 0.3%). The frame's 2 ms of work no longer fits one slice, so it can
be preempted halfway through; see [interactive
scheduling](interactive-scheduling.md).

**Also changed:** most Chrome processes ran at nice 0 after the change,
against −4 before. Chrome had evidently inherited −4 from a bash wrapper at
launch. It runs in its own cgroup, so this does not change how it shares CPU
with other applications.
