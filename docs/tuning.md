# RAM-sized swap with zswap and CachyOS-inspired defaults

Installed systems get two independent, reviewed choices. Both are enabled by
default and both can be switched off on the installer's Disk page.

## Swap partition matched to RAM, with zswap

The installer reads `MemTotal` from the live system and rounds it up to whole
GiB, because the kernel reports slightly less than the installed RAM. A 16 GiB
laptop gets a 16 GiB swap partition. The GPT layout is ESP (or BIOS boot),
root, then swap at the end of the disk, with exact MiB boundaries that are
verified against the kernel's view before formatting. If the swap partition
would leave the root filesystem below 20 GiB, review fails before any disk write
and suggests disabling swap or choosing a larger disk.

The partition is formatted with `mkswap` using an identity chosen before
erasure and verified from its header. The installed configuration contains:

- `swapDevices` and `boot.resumeDevice` for that partition, so hibernation can
  resume from it.
- zswap kernel parameters: `zswap.enabled=1`, `zswap.compressor=zstd`,
  `zswap.max_pool_percent=25` and `zswap.shrinker_enabled=1`. zswap keeps
  compressed pages in RAM and writes cold ones to the partition. nixpkgs'
  `boot.zswap` module is deliberately not used: it also sets `zswap.zpool`,
  which Linux 6.18 removed.
- `vm.swappiness = 60`, the kernel default rather than CachyOS's 100. Under
  sustained pressure, zswap's shrinker writes most swapped pages out to the
  partition, so anonymous memory is no cheaper to reclaim than file cache.
  On a workstation with 60 GiB of usable RAM running heavy agent workloads,
  two days of swap wrote 211 GiB to the partition and read 113 GiB back,
  while the compressed pool held about 1 GiB. 65% of swapped pages were
  needed again, against 11% of evicted file cache, and at one point about
  16 GiB of anonymous memory sat only on disk beside 18 GiB of page cache.
  At 60, reclaim weighs file cache over anonymous memory (60 against 140)
  instead of treating them alike; it still swaps, just less. Much lower
  values strip programs' code from memory well before anything is swapped,
  and 0 limits reclaim to file pages. With
  the desktop's own services protected (see
  [Keeping the desktop resident](#keeping-the-desktop-resident)), swappiness
  decides only which applications' pages pay.
- `vm.page-cluster = 1`. CachyOS uses 0 for zram. Pages that zswap writes back
  live on an SSD, where a small readahead still helps.

## CachyOS-inspired tuning

Source: [CachyOS-Settings](https://github.com/CachyOS/CachyOS-Settings) at
commit `e27ea45` (GPL-3.0) and the CachyOS kernel's runtime defaults, plus
the boot options of Ubuntu's `lowlatency-kernel` package. Only settings that
work with stock NixOS kernels and nixpkgs packages are used. There is no BORE
scheduler, no custom kernel, and no x86-64-v3 package rebuild. Implemented in
the installer's `rust/system/tuning.nix`. Installed systems import it from
their flake's Sekka input, so updates bring changes to it.

| Area | Setting |
|---|---|
| Memory | `vm.vfs_cache_pressure=50`; `vm.dirty_bytes=256 MiB` and `vm.dirty_background_bytes=64 MiB`, absolute limits that avoid huge writeback stalls on large-RAM machines; `vm.dirty_writeback_centisecs=1500`; `vm.watermark_boost_factor=0` and `vm.compaction_proactiveness=0`, as in the CachyOS kernel |
| Transparent huge pages | `defrag=defer+madvise`, `khugepaged/max_ptes_none=409` |
| Kernel | `kernel.nmi_watchdog=0` plus the `nowatchdog` parameter; watchdog modules `iTCO_wdt`, `sp5100_tco` and `wdat_wdt` blacklisted; `kernel.kptr_restrict=2`; console log level 3; `ntsync` module loaded for Wine/Proton; `preempt=full rcu_nocbs=all` (below) |
| I/O | mq-deadline for NVMe and SATA SSDs, BFQ for rotating disks (below) |
| Network | `net.core.netdev_max_backlog=4096` |
| Scheduling | ananicy-cpp with the CachyOS rule set (`ananicy-rules-cachyos`), adjusting process priorities, with Sekka's rules for shells, compilers and interactive AI harnesses (below). Its cgroup features are off: moving processes out of their systemd units broke VT switching and session cleanup at logout |
| Memory pressure | systemd-oomd enabled for system and user slices |
| systemd | `DefaultTimeoutStopSec=10s` for system and user managers; `DefaultLimitNOFILE` 2048:2097152 (system) and 1024:1048576 (user); `Delegate=cpu cpuset io memory pids` for user managers; journal capped at 50 MiB; coredumps cleaned after 3 days; rtkit logging at info |
| Services already default in NixOS | weekly fstrim, dbus-broker, systemd initrd |

### Preemption and RCU

Ubuntu replaced its lowlatency kernel flavour with the `lowlatency-kernel`
package. Its only effect is to add `preempt=full rcu_nocbs=all` to the kernel
command line (`debian/99-lowlatency.cfg`, version 1.3). Sekka sets the same
two options.

- `preempt=full`. Since Linux 7.0, x86 kernels offer only full and lazy
  preemption, and lazy is the default. Under lazy preemption, a normal task
  that should preempt another waits for the next timer tick, up to 1 ms at
  1000 Hz. Full preemption switches at once. Real-time tasks, such as
  Hyprland's, preempt at once in both modes. The one published x86
  comparison found no throughput difference ([tbench](https://lkml.iu.edu/hypermail/linux/kernel/2410.1/00023.html):
  3476.5 MB/s full, 3474.0 lazy). Nobody has published a desktop latency
  comparison. Root can read the active mode from
  `/sys/kernel/debug/sched/preempt`.
- `rcu_nocbs=all`. RCU callbacks run in kernel threads rather than at
  interrupt time on the CPU that queued them, so they no longer interrupt the
  task running there. NixOS kernels have `RCU_LAZY`, which batches offloaded
  callbacks to save power.

### I/O scheduler

mq-deadline serves the realtime, best-effort and idle I/O priority classes in
that order (since Linux 5.14). It lets a waiting lower-class request through
after 10 s (`prio_aging_expire`). The Nix daemon starts with idle CPU and I/O scheduling, so its builders inherit
the idle class, which Kyber, the previous NVMe choice, ignores. A `nix`
command waiting on the daemon while other I/O keeps the disk saturated can
see that delay too.

mq-deadline needs about a third of BFQ's CPU time per request (0.7 µs against
1.9 µs in the [kernel documentation](https://docs.kernel.org/block/bfq-iosched.html)).
It reached 569K random-read IOPS on one SSD where BFQ reached 315K and Kyber
786K ([ICPE 2024](https://atlarge-research.com/pdfs/2024-io-schedulers.pdf),
Linux 6.3). BFQ's published start-up gains under heavy I/O date from 2018–2019
and conflict with the only independent laptop measurement. BFQ also has a
single lock per device and has been in "Odd Fixes" maintenance since 2024.
It stays on rotating disks.

Kernel flusher threads submit a build's buffered writes, so the build's I/O
class does not apply to them.

### Interactive programs and background jobs

Foot, Chrome/Chromium and the recognized AI harnesses (`codex`, `claude`,
`claude-code`, `gemini`, `omp`) get nice −4. Harnesses explicitly get normal CPU
scheduling, including every thread: typing, drawing and responding to tool results must
remain responsive. Shells run at nice 0, and the listed compilers/linkers at
nice 3. These are absolute values; `nice -n 19 bash` is reset to 0. Per-process
nice values do not replace CPU allocation between systemd groups.

`sekka-agent-workloads.service` runs as each user. While a harness is running,
every 250 ms it discovers harness process trees and asks that user's systemd
manager to create scopes for their child jobs. All work scopes share
`app-background.slice`, whose CPU
and I/O weights are 20; an ordinary application has CPU weight 100. These are
relative weights under contention, not quotas: jobs can use otherwise idle
CPUs. Shells, arbitrary test binaries and their descendants stay in the work
scope even when they change their own nice values. The harness remains in its
launcher's unit. A nested harness that execs inside a work scope is moved to
an interactive app scope, and its own jobs are separated again.

The service handles both packaged harnesses and recognized executables started
by other launchers. It only manages the current user's processes already
inside their user manager, leaves delegated subgroups alone, and uses
systemd's `StartTransientUnit`/`AttachProcessesToUnit` APIs. Scopes follow
explicit stops of their original unit through `PartOf`; the user manager
tracks and cleans them up. Ananicy's raw cgroup migration and realtime
workaround remain disabled. Discovery is periodic: short jobs may finish
before classification, and a process that daemonizes and loses its ancestry
before discovery cannot be attributed to an agent. Containers and privileged
helpers are not moved. This is resource scheduling, not sandbox enforcement.

`sekka-background COMMAND ...` explicitly runs other long jobs in the same
background slice, preserving arguments, environment, working directory,
terminal and exit status. It runs directly without a user manager or when
`calamares.tuning.enable` is off. No AI harness is wrapped in this command.

Nix builders belong to the daemon rather than the calling agent. Sekka sets
`nix.daemonCPUSchedPolicy = "idle"`, `nix.daemonIOSchedClass = "idle"` and the
daemon's CPU/IO weights to 20 as overridable defaults. This also covers
Determinate Nix. Nix evaluations within agent jobs stay in their work scopes.

### Keeping the desktop resident

CPU priority cannot help a compositor waiting for swapped-out pages. Aizome's
NixOS module gives each service the desktop waits on its own best-effort
reclaim reserve (`MemoryLow`), sized to what that service keeps resident:

| Service | MiB | What waits on it |
|---|---|---|
| `aizome.service` | 1024 | Drawing and every input event: the compositor (libinput runs in it), shell, Xwayland |
| `aizome-ibus.service` | 96 | Typing, where Sekka's IBus frontend is the input method |
| `pipewire`, `pipewire-pulse`, `wireplumber` | 32, 48, 32 | Audio |
| `aizome-portal`, `xdg-desktop-portal` | 32, 16 | File choosers; apps read the color scheme as they start |
| Session bus | 16 | The shell's and apps' calls to session services |
| `systemd-logind` | 16 | The compositor's seat: its input and DRM devices, VT switches, sleep |
| `systemd-udevd` | 64 | Hotplugged keyboards, mice and displays |
| System bus | 32 | logind's signals to the compositor |
| `rtkit-daemon` | 8 | Realtime scheduling for audio threads |
| `bluetooth` | 16 | Bluetooth LE keyboards and mice, whose every event passes through BlueZ |
| `systemd-oomd` | 64 | The last-resort killer itself |

The sizes come from a 4K workstation under heavy load: each service's peak and
its resident plus swapped memory, with headroom. `aizome.service`'s cgroup
also carries reclaimable kernel caches (directory entries, inodes and unmapped
page cache, close to 1 GiB there), which its processes don't need resident; its
budget covers the roughly 800 MiB of heap, mapped code and shared buffers.
`systemd-oomd`'s unit already asks for 64 MiB, which had no effect while
`system.slice` granted nothing. Services that aren't installed get no budget,
and with `services.pipewire.systemWide = true` PipeWire's budgets move to its
system services.

A service is protected only as far as every ancestor is, so `session.slice`,
each user manager (`user@UID.service`) and `user-UID.slice`, and `user.slice`
get the sum of the session budgets, and `system.slice` the sum of the system
ones. Protection a service doesn't use passes to its siblings in proportion to
their use (systemd mounts cgroup2 with `memory_recursiveprot`). Accounts busy
at the same time share `user.slice`'s reserve by usage; a machine where
another account runs heavy jobs beside the desktop can raise
`systemd.slices.user.sliceConfig.MemoryLow`.

This is best-effort reclaim protection, not preallocation, `mlock`, a hard
minimum or a limit on applications. Unused protection remains available to
other programs. A service using more than its budget still loses pages, but
reclaim scans it in proportion to the unprotected part, and the kernel
reclaims protected memory rather than run out. The budgets are NixOS defaults
in Aizome's module (the IBus one in Sekka's input-method module), independent
of `calamares.tuning.enable`. Change them through
`programs.aizome.memoryReserve.session` and `.system`, in MiB, so the slices
follow. 0 turns a service's protection off with a `MemoryLow=0` drop-in, which
also overrides what the unit asks for itself (systemd-oomd's 64 MiB). The
drop-ins are named `40-aizome-memory.conf`, after packaged drop-ins and before
an administrator's (`50-…`, `override.conf`). An explicit
`serviceConfig.MemoryLow` on one of these units keeps that setting, but the
slices still count the budget, so the module warns.

The reserve changes where reclaim happens: unprotected workloads can lose
pages and run more slowly before the desktop does. It introduces no new OOM
threshold, hard workload cap or proactive job termination. Sekka's existing
oomd fallback remains unchanged (80% sustained pressure for 30 seconds with
the pinned defaults), and genuine exhaustion can still trigger oomd or the
kernel OOM killer. The budgets are an overridable starting policy measured on
one machine, not a demonstrated latency bound; applications outside
`session.slice` receive none.

When diagnosing a freeze, inspect `oomctl`, `/proc/pressure/{memory,io,cpu}`
and the desktop's `memory.low`, `memory.swap.current` and `memory.pressure`
cgroup files. Large swap use alone does not prove current thrashing; combine
pressure samples, swap traffic and compositor stall logs.

### Launcher and ananicy repairs

Aizome's desktop-entry launcher already uses app scopes, but its keybinding
and IPC launcher directly spawned terminals in `aizome.service`. Their agents
and builds consequently shared the compositor's weight 1000, inside
`session.slice` at 400, ahead of `app.slice` at 100. Aizome's `spawn`
(`desktop/aizome/crates/aizome-comp-core/src/session.rs`) now starts those
launches in `app.slice` too, using `systemd-run --scope`. Sekka patched its
pinned Aizome package this way until Aizome moved into Sekka. Session
environment, arguments and working directory are retained. Without a user
manager, the launcher keeps its direct-spawn behavior.

`nix/scheduling/ananicy-processes.patch` repairs gaps in ananicy-cpp 1.2.0:

- Apply CPU scheduling and I/O priority to every existing thread, as nice
  already was. Previously a Nix main thread could display SCHED_IDLE while
  its garbage-collector workers used normal scheduling.
- Report the child PID on BPF fork events, covering new threads and browser
  zygote children that never exec.
- Process consecutive execs of the same PID and refresh its identity when
  applying a rule, so NixOS shell wrappers do not hide the real program.
- Recognize Chromium's rewritten argv[0], including switches containing paths.
  Use the executable name as a fallback when permission allows, without
  adding ptrace privileges to the daemon.
- Recognize known harness scripts under Node/Bun before applying their generic
  background-runtime rule, so interpreted harnesses stay interactive.

Review the ananicy patch whenever ananicy-cpp changes and remove it when the
upstream version contains equivalent fixes. Existing terminals and harnesses
should be restarted after installing the change.

### Opt-in continuous lag recording

The observational recorder is off by default. On an installed system importing
Sekka's tuning module, enable it explicitly:

```nix
services.sekka-lag-recorder.enable = true;
```

This option is independent of `calamares.tuning.enable`: it works with tuning
disabled and does not enable scheduling, memory reserves, or OOM policy.
Apply the configuration through the normal NixOS rebuild workflow. The module
installs `sekka-lag-recorder` and the user unit
`sekka-lag-recorder.service`, wanted by `default.target`. To start or inspect
the configured unit in the current login:

```sh
systemctl --user start sekka-lag-recorder.service
systemctl --user status sekka-lag-recorder.service
```

The service runs in `background.slice` with CPU and I/O weights of 20,
`MemoryMax=128M`, private umask `0077`, and no new privileges. It restarts on
failure after five seconds. There is deliberately no hard CPU quota: a quota
could manufacture sampling gaps. These limits apply only to the recorder;
it does not kill, reprioritize, or move workload processes.

Default sampling is once per second. The command also accepts
`--output-dir DIR`, `--interval SECONDS` (finite and at least 0.1),
`--samples N` for a finite capture, and `--once` for one sample. Use a separate
output directory for a manual capture while the service runs; a process lock
prevents duplicate writers to the same directory.

Evidence remains local in `$XDG_STATE_HOME/sekka/lag-recorder`, or
`~/.local/state/sekka/lag-recorder` when `XDG_STATE_HOME` is unset. The directory
is private (`0700`) and files are private (`0600`). There are two storage tiers:

- `history.sqlite3` retains **72 hours** of compressed evidence, leaving room
  to investigate an incident 24–48 hours later. A compact `summary` is saved
  for every sample, including quiet periods. Full samples are saved every
  30 seconds and at incident starts (at least one second apart); incident
  boundaries, process details, metadata, relevant journal messages, and
  collection failures are also retained. Empty successful journal polls and
  routine worker timings are omitted.
- `recorder.jsonl` still rotates at 8 MiB with four backups, approximately
  40 MiB total. This short-term tier preserves full one-second incident
  samples and 60-sample prehistory. Quiet full samples are written every ten
  seconds, and incidents end after 15 quiet seconds. Its duration depends on
  event frequency and record size, not the 72-hour history policy.

The history database has a **2 GiB safety budget**, including its WAL and
shared-memory files; write batches and reclamation can temporarily exceed
that budget. Time-based expiry runs about once a minute. If the byte budget
requires earlier eviction, the recorder warns in its service journal and
persists a `retention_notice` with the lost time range and record count.
`--history-stats` reports that notice, actual oldest/newest timestamps,
allocation, and any unresolved over-budget state. The window can therefore
be shorter with unusually large records or faster sampling; it is not a
guarantee of uninterrupted coverage. The raw 40 MiB ring is additional.

The compact timeline keeps global pressure averages/interval stalls,
CPU/load, memory/swap/zswap occupancy, selected fault/reclaim/swap rates,
disk latency/activity, desktop process identity/faults/memory/wait evidence,
IPC status/latency, and configured cgroup protection and occupancy. Detailed
cgroup counters and short-window pressure are kept for the session,
compositor, background, and Nix daemon, rather than every ancestor.
Summaries cap groups/devices/desktop processes at 16/8/8 with explicit
omission flags; full snapshots preserve the collector's broader view.
Values that are unavailable or have reset are not converted to zero.

History writes are batched for up to five seconds during normal sampling;
clean SIGTERM/SIGINT flushes them. Abrupt termination can lose the pending
batch, and power loss can also lose recent uncheckpointed transactions.
Existing committed history survives recorder restarts. Coverage begins
when recording starts; longer retention cannot recover already-rotated
evidence or periods when the recorder was stopped.

Query history without stopping the recorder or taking its writer lock:

```sh
sekka-lag-recorder --history-stats
umask 077
sekka-lag-recorder --history \
  --since '2026-11-05T05:30:00Z' --until '2026-11-05T05:45:00Z' \
  --kind summary --kind event_start --kind event_end --kind journal \
  > lag-window.jsonl
```

Use the incident's actual timestamps, including `Z` or an explicit UTC
offset. Both boundaries are inclusive; omit `--kind` to include full
snapshots and other evidence. Export is ordered JSONL and reads bounded
batches without pinning the writer's WAL. It is a live traversal, not an
atomic backup: concurrent expiry can remove records not yet read. Missing
or corrupt history is an error, not an empty successful result. Use the
export command rather than copying only the database file while recording.
Review exported workload metadata before sharing it.

Detailed process snapshots are rate-limited to one per 30 seconds;
periodic metadata is refreshed every five minutes.

Accessible `/proc` and `/sys` evidence includes:

- System CPU/load and CPU, memory, and I/O PSI totals and interval stall
  percentages; memory/swap/zswap usage; selected fault, reclaim, swap, and
  zswap counters and rates; block-device I/O counters, rates, await, and busy
  time (excluding loop and RAM devices).
- Known system/user/session/application/background, Aizome, and Nix-daemon
  cgroups: configured memory protection and limits, ancestor limit context,
  current memory/swap usage, memory events/statistics, pressure, and CPU/I/O
  counters. These are configured hierarchy values, not a calculation proving
  effective reclaim protection.
- Actual session-cgroup Aizome process identity (PID and start time),
  executable, CPU/faults/RSS/swap, and main-thread wait channel/scheduling
  statistics. Event details also rank processes by CPU delta and RSS and
  include desktop processes. One detail worker limits discovery to 2,048
  process entries and a quarter-second budget, with a 0.35-second total
  budget including enriched reads of selected processes; results may be
  incomplete.
- Kernel/version information, selected kernel configuration, zswap module
  parameters, swappiness, and the recorder's settings in metadata snapshots.

Journal collection is restricted to the current user's `aizome.service`
watchdog, libinput, and stall messages, not broad application logs. It runs
about every ten seconds, requests at most 20 matching messages, has a
one-second timeout and a 64 KiB raw-output cap, and truncates each message
to 2,048 characters. Inaccessible metrics are reported as unavailable, not
silently replaced with zero.
Timeouts and unavailable journal reads preserve the previous query window
for the next bounded retry. Successful reads, including no matches, advance
the window. Hitting the raw-output cap also advances it to avoid retrying
one oversized message forever, explicitly recording `window_advanced` and
`window_truncated`; some messages can therefore be omitted.
`possible_line_limit` flags a full 20-line response before deduplication,
not proof that the entire interval's messages were captured.

Automatic recording triggers include memory or I/O PSI `full avg10` at
10%, CPU PSI `some avg10` at 50%, a sampling delay of at least
`max(1 second, 2 × interval)`, and relevant watchdog/libinput/stall journal
messages. The command's `--memory-full-threshold`, `--io-full-threshold`,
and `--cpu-some-threshold` options accept finite percentages; these only
change incident capture and detail collection, not resource policy.

The recorder also probes the user's running Aizome with the read-only
newline-delimited IPC request `{"request":"version"}`. It verifies the
socket peer's credentials against discovered `aizome.service` processes,
revalidates PID start time and service-cgroup membership before sending
anything, excludes shell/headless sockets, bounds the reply to 16 KiB, and
gives the whole exchange at most 100 ms per sample.
A response taking at least 50 ms or timing out triggers recording.
Unavailable, refused, or unverifiable sockets are reported as unavailable,
not classified as compositor lag. Round-trip latency includes scheduling
delays in the recorder itself; it is not compositor-only execution time or
an input-to-screen latency measurement.

After noticing a stall, request an immediate manual event and pre-event
history by signalling **only the recorder's main process**:

```sh
systemctl --user kill --kill-whom=main --signal=SIGUSR1 sekka-lag-recorder.service
```

Manual events request a sample within about 0.1 seconds, scheduling
permitting; detailed snapshots use the same worker and 30-second cooldown
as automatic triggers, so a fresh detailed snapshot is not guaranteed.
Do not use broad process-name matching or signal the compositor. If the
recorder itself cannot run during a stall, the request is handled only when
it runs again; the resulting sample is not a reconstruction of the frozen
instant.

Stop recording without stopping any other work:

```sh
systemctl --user stop sekka-lag-recorder.service
```

For a temporary stop that also prevents another start during this user
manager's lifetime:

```sh
systemctl --user mask --runtime --now sekka-lag-recorder.service
# Remove that temporary block when recording is wanted again:
systemctl --user unmask --runtime sekka-lag-recorder.service
```

To disable it persistently, set `services.sekka-lag-recorder.enable = false;`,
apply the NixOS configuration, and stop the unit if it is still running.
Do not rely on `systemctl --user disable` to override NixOS's declarative
`default.target` dependency. Stopping or disabling does not delete retained
logs. SIGTERM and SIGINT also request a clean stop.

#### Interpreting a memory-protection comparison

Recording thresholds flag evidence to retain, not conditions for killing a
process or changing resource policy. Enabling the recorder does **not**
activate the separate memory-reservation fix. Before and after deploying
that fix, save the actual running configuration, not only the desired Nix
options or a PR revision:

```sh
systemctl --user show session.slice app-background.slice aizome.service \
  --property=ControlGroup,MemoryMin,MemoryLow,MemoryHigh,MemoryMax,ManagedOOMMemoryPressure
systemctl show "user-$(id -u).slice" "user@$(id -u).service" \
  --property=ControlGroup,MemoryMin,MemoryLow,MemoryHigh,MemoryMax,ManagedOOMMemoryPressure
```

Read the corresponding accessible cgroup `memory.min`, `memory.low`,
`memory.high`, `memory.max`, `memory.events`, and pressure files as well.
The user manager, its parent slices, and the session/compositor hierarchy
matter: a reservation requested in one child alone does not prove effective
protection through its ancestors. `MemoryMin`/`MemoryLow` reclaim protection,
`MemoryHigh` throttling, `MemoryMax` limits, and systemd-oomd's pressure
policy are different mechanisms. Note missing units, unavailable properties,
and permission failures rather than treating them as zero protection.
Compare similar workloads and record the deployed generation, relevant
overrides, and actual active values for both runs. A recorder event or the
absence of an OOM kill alone does not establish that protection worked.

These measurements are a baseline for correlating symptoms with memory,
reclaim, swap, I/O, and scheduler pressure; they are **not** an exact
decompression CPU profile. Aggregate swap/reclaim activity and CPU use cannot
attribute CPU time to zswap decompression. Sampling can be delayed or queued
under contention, and a gap says the recorder was late, not which component
caused it. Snapshots after a gap can miss short-lived processes and transient
peaks. Unprivileged access and journal retention also limit what can be
observed; missing data is not evidence that no stall happened.

There are no uploads, command-line or environment-variable captures,
screenshots, keystrokes, `smaps` scans, `perf`, root, or debugfs access.
Before sharing even a small event window, review it: process names and IDs,
unit/cgroup names and paths, device identifiers, timestamps, and relevant
journal excerpts can disclose metadata about workloads or the machine.
Private permissions and bounded retention are not anonymization.

### Checks

`nix build .#checks.x86_64-linux.tuning-policy` checks disabling the policy,
explicit overrides, process-tree classification, and opt-in recording without
enabling scheduling or OOM policy. It also runs the recorder's regressions for
PID reuse/counter resets, incident history and cooldown, bounded private
rotation and locking, journal retry windows, and authenticated IPC timeouts.
History regressions cover 24–48-hour survival, the 72-hour expiry boundary,
restart/replay deduplication, live time-window queries, explicit budget
eviction, checkpoint pressure, and loss-aware metric projection.
`nix build .#checks.x86_64-linux.tuning` boots a disposable VM and checks the
preemption and disk-scheduler settings, terminal/shell/compiler nice values,
all-thread Nix scheduling and I/O, late threads, Chrome retitling and fork-only
children, wrapper execs, interactive harnesses and isolated jobs, nested
harnesses, scope stop propagation, and Aizome's launch function with a real
user manager. The unpatched ananicy binary is the failing control for
pre-existing Nix worker threads. `tests/test_agent_workloads.py` checks process
tree classification and the boundaries on which units the service may manage.

`nix build .#checks.x86_64-linux.aizome-login` additionally logs into the full
Aizome desktop, opens actual Foot through its IPC launcher, starts a raw-mode
TUI stand-in named `codex` with eight CPU-bound child jobs, and sends keyboard
input through QEMU's emulated keyboard. It checks every received byte and the
harness/job group separation. This checks the compositor/terminal input path;
it does not run an authenticated Codex session or validate physical hardware.

### Not ported

- `kernel.unprivileged_userns_clone`: NixOS kernels lack that patch.
- `fs.file-max`: systemd already sets the maximum, so this would lower it.
- zram rules: they would disable zswap.
- SATA `max_performance` link power, NVIDIA module options, the audio power-save
  override, split-lock mitigation, and NTP/DNS policy. Each is situational or
  trades battery, security or stability for benchmarks.
- A sched-ext scheduler, and per-application time slices: held for later, see
  [interactive scheduling](interactive-scheduling.md).
