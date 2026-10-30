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
- `vm.swappiness = 100`, the CachyOS value. Swapping first lands in
  compressed memory, so anonymous memory is reclaimed as readily as file
  cache.
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

### Steam process trees

With system tuning enabled on a system with Steam (`programs.steam.enable`,
which `calamares.tuning.steam.enable` follows), `steam`, `steam.sh` and
`Steam.exe` receive nice −5 with normal CPU scheduling and best-effort I/O.
Their same-user descendants receive a nice −10 baseline, including unlisted
native games, Proton wrappers, secondary launchers and Steam's browser
helpers. Generic shell, runtime and game rules cannot lower that baseline. A
stronger named rule, such as `wineserver` at −12, still applies, and threads
already using a stronger nice value retain it. Existing FIFO, RR and deadline
CPU policies are preserved; this policy does not grant real-time CPU
scheduling.

`fossilize_replay` and its descendants are the deliberate exception: background
shader-cache processing remains at nice 16 with idle CPU and I/O scheduling,
so it yields to a running game. Steam and its other descendants receive
best-effort I/O instead of inheriting the upstream launcher's idle I/O class.
GameMode does not renice processes, leaving Ananicy as the policy owner.

`nix/scheduling/ananicy-descendants.patch` adds `child_rule`, naming a rule to
apply to descendants. `descendant-rules.hpp` walks host `/proc` ancestry at
fork/exec events and on the daemon's startup scan, normalizing thread IDs to
their process leader. It retains observed lineage across exec and reparenting,
keyed by PID, start time and UID; dead entries are pruned. A process with its
own `child_rule` starts a new lineage, which lets Fossilize override Steam's
foreground policy. The daemon's `nice_only_raise` flag keeps stronger thread
priorities, and `sched_keep_realtime` preserves existing real-time CPU policies.
Upstream rules for Steam and Fossilize are removed before adding Sekka's rules,
so filesystem iteration order cannot choose the old launcher policy. Without
Steam, none of these rules is added and CachyOS's own rules for those names
stay. The daemon then has no rule with a `child_rule` and skips the ancestry
walk altogether: each event costs what it did without this patch. At info
level, its log says whether it follows ancestry. ananicy-cpp reads its rules
only when it starts, so a rebuild that changes them, adding or removing Steam
among other things, restarts it.

Like every Ananicy rule, these match a process by name: the basename of its
`argv[0]`, or of its executable when no rule has that name. Any local user can
choose either, for example with `exec -a steam` or a copy named `steam`, and
so give a process tree of their own nice −10, which an unprivileged user
cannot otherwise set. The same holds for single processes under CachyOS's
named rules; Steam's rule extends it to a whole tree. On a machine shared with
users who should not have that, set `calamares.tuning.steam.enable = false`.

Ancestry stops at UID changes and setuid helpers. No process is moved between
cgroups. A process that loses all recognizable ancestry before its first
observation cannot be attributed; restarting the daemon also forgets already
orphaned lineage. Games started independently of Steam continue to use their
ordinary named rules. These are CPU contention priorities, not quotas or a
promise of higher FPS in a GPU-bound game.

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

Review both Ananicy patches whenever ananicy-cpp changes and remove them when
the upstream version contains equivalent fixes. Existing terminals and harnesses
should be restarted after installing the change.

### Checks

`nix build .#checks.x86_64-linux.tuning-policy` checks disabling the policy,
explicit overrides and process-tree classification, including Steam ancestry,
PID reuse, credential boundaries and reparenting, and that only systems with
Steam get its rules.
`nix build .#checks.x86_64-linux.tuning` boots a disposable VM and checks the
preemption and disk-scheduler settings, terminal/shell/compiler nice values,
all-thread Nix scheduling and I/O, late threads, Chrome retitling and fork-only
children, wrapper execs, interactive harnesses and isolated jobs, nested
harnesses, scope stop propagation, and Aizome's launch function with a real
user manager. The unpatched ananicy binary is the failing control for
pre-existing Nix worker threads. `tests/test_agent_workloads.py` checks process
tree classification and the boundaries on which units the service may manage.
The same VM checks Steam's −5 launcher and −10 descendants as an ordinary
user, unknown and known games, generic runtimes, shell wrappers, Wine's
stronger rule, preservation of real-time threads, background shader helpers,
fork-only children, reparenting, and classification after daemon restart.
It then switches to a configuration without Steam, as most installations
are, and back: each switch restarts the daemon. Without Steam the daemon
logs that it follows no ancestry, terminals, shells and compilers keep their
rules, and a child of a process named `steam` keeps the nice 16 it inherits
under CachyOS's launcher rule. With Steam again, a new game gets −10 at once.

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
