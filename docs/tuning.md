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
  values strip programs' code from memory before anything is swapped; only
  0 limits reclaim to file pages, and 1 merely tries them first. With
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
`system.slice` granted nothing. Services that aren't installed (BlueZ,
RealtimeKit, PipeWire's) get no budget.
PipeWire's budgets follow its services: by default they belong to
`memoryReserve.session`; with `services.pipewire.systemWide = true`, they
belong to `memoryReserve.system` and count toward `system.slice` instead.

A service is protected only as far as every ancestor is. The module sets
`session.slice` and `user.slice` to the sum of the session budgets, and
`system.slice` to the sum of the system ones. The user manager
(`user@UID.service`) and `user-UID.slice` of each person signed in at the
machine, on the console or a login screen, get the session sum while they
are: a `login` PAM session rule, which the login screens' PAM services
include, sets it at runtime after `pam_systemd` and starts
`aizome-memory-reserve-<session>.service`, bound to the session's scope, which
resets it to 0 when that person's last such session ends. The PAM rule also
runs at normal logout: background jobs can keep a logged-out session's scope
alive, so waiting for the scope alone would retain the reserve. Both paths
keep the reserve while another local session remains open.
Greeters, SSH logins, lingering user managers
and other accounts get none, so their jobs can't claim the reserve that
`user.slice` holds for the person at the desktop. Protection a service doesn't
use passes to its siblings in proportion to their use (systemd mounts cgroup2
with `memory_recursiveprot`). People signed in at the same time share
`user.slice`'s reserve; a shared machine can raise
`systemd.slices.user.sliceConfig.MemoryLow`.

This is best-effort reclaim protection, not preallocation, `mlock`, a hard
minimum or a limit on applications. Unused protection remains available to
other programs. A service using more than its budget still loses pages, but
reclaim scans it in proportion to the unprotected part. The kernel reclaims
protected memory rather than run out. The budgets are NixOS defaults in Aizome's
module (the IBus one in Sekka's input-method module), independent of
`calamares.tuning.enable`. Change them through
`programs.aizome.memoryReserve.session` and `.system`, in MiB, so the
ancestors follow. 0 turns a service's protection off with a `MemoryLow=0`
drop-in, which also overrides what the unit asks for itself (systemd-oomd's
64 MiB); `lib.mkForce { }` leaves an empty drop-in package. The drop-ins are
named `40-aizome-memory.conf`, after packaged drop-ins and before an
administrator's (`50-…`, `override.conf`), which can still change them. An
explicit `serviceConfig.MemoryLow` on one of these units, including 0, takes
precedence over its drop-in, for packaged and NixOS-defined services alike,
and the sums follow it when it's in bytes or K, M, G or T. Other forms, such
as a share of RAM, leave the service's budget in the sums, with a warning.

The reserve changes where reclaim happens: unprotected workloads can lose
pages and run more slowly before the desktop does. It introduces no new OOM
threshold, hard workload cap or proactive job termination. Sekka's existing
oomd fallback remains unchanged (80% sustained pressure for 30 seconds with
the pinned defaults). Genuine exhaustion can still trigger oomd or the kernel
OOM killer; keeping the desktop resident does not guarantee that every job
survives, and concentrating reclaim on workloads can increase their pressure.

`checks.aizome-memory-policy` evaluates the budgets, sums, drop-ins and
overrides for Aizome alone, with IBus, customized, without tuning, and without
Aizome. It also builds empty and zeroed reserve packages, checks explicit
service overrides and the sums that follow them, the login session rule, and
covers user and system-wide audio with optional audio services disabled. The
`aizome-login` VM test reads every budgeted unit's `MemoryLow` and the cgroup
files of the ancestors after a real login, checks that a lingering second
account and the greeter get no reserve and that the user manager keeps its
memory delegation, and that the reserve returns to 0 after forced termination
and normal logout with surviving background jobs, while another open local
session keeps it. A bounded
cgroup reclaim experiment
showed that protected pages remained resident while an unprotected control was
evicted. These are mechanism checks, not a before/after measurement of the
full desktop under the original workload. The budgets are an overridable
starting policy, not a demonstrated latency bound; applications outside
`session.slice` receive none.

Inspect `oomctl`, `/proc/pressure/{memory,io,cpu}` and the desktop's
`memory.low`, `memory.swap.current` and `memory.pressure` cgroup files when
diagnosing a freeze. Large swap use alone does not prove current thrashing;
combine pressure samples, swap traffic and compositor stall logs. A full
`nix flake check --no-build` still evaluates every check and can consume tens
of GiB: use focused component evaluations during concurrent development.
Neither memory protection nor oomd establishes a frame-latency guarantee or
fixes unrelated GPU stalls and blocking compositor operations.

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

### Checks

`nix build .#checks.x86_64-linux.tuning-policy` checks disabling the policy,
explicit overrides and process-tree classification.
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
