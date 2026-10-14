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
| Scheduling | ananicy-cpp with the CachyOS rule set (`ananicy-rules-cachyos`), adjusting nice, I/O and OOM priorities only, with Sekka's rules for shells and compilers (below). Its cgroup features are off: moving processes out of their systemd units broke VT switching and session cleanup at logout |
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
after 10 s (`prio_aging_expire`). nix-daemon's ananicy rule gives Nix builds
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

### Process priorities

CachyOS's ananicy rules give terminals and shells −4 ("Doc-View"). Everything
started from a shell inherits its nice value, including builds, and on NixOS
every wrapper script is a bash script. Sekka changes two things.

- **Shells** (`bash`, `dash`, `fish`, `ksh`, `nu`, `tcsh`, `zsh`) run at
  nice 0. A terminal keeps −4, and what runs in it starts at normal priority.
  ananicy loads rule files in directory order and the last rule for a name
  wins, so Sekka removes the shell rules from its copy of the CachyOS set
  rather than adding rules after them. Because ananicy sets absolute values, a
  shell started lower (`nice -n 19`, a service's `Nice=`) is also set to 0.
  CachyOS's rule set it to −4.
- **Compilers and linkers** (`cc1`, `cc1plus`, `lto1`, `clang`, `rustc`,
  `clippy-driver`, and the `ld`, `lld` and `mold` variants) run at nice 3,
  about half the CPU weight of a normal task. Each application has its own
  cgroup with equal CPU weight, so this only affects competition inside the
  application that started the build. A build loses CPU only when that
  application is busy too. The rule sets nothing but nice, so Nix builds keep
  the SCHED_IDLE policy and idle I/O class they inherit from `nix-daemon`
  (SCHED_IDLE ignores nice). Being absolute, it also raises the compilers of
  `nice -n 19 make` to 3; `chrt --idle` keeps a build lower.

ananicy identifies a process by its argv[0] when it is executed, and ignores a
second exec of the same process straight after. NixOS's `ld` is a bash wrapper
script that execs the real linker, so ananicy takes it for a shell and the
linker keeps the shell's nice value. In a VM, `cc1` (10 of 10 compiles) and
`rustc` behind nixpkgs' wrapper (6 of 6) were reniced; the wrapped `ld` was not
(0 of 10). Linkers started directly, such as rustup's `rust-lld`, are matched.
ananicy reaches a new process about half a second after it starts (measured on
ganymede), so a compiler process that finishes sooner keeps its inherited
priority.

### Checks

`nix build .#checks.x86_64-linux.tuning` boots a VM with the installed kernel
and the tuning module, and checks:

- the boot options and the active preemption mode;
- mq-deadline on an emulated NVMe disk and BFQ on a rotating one;
- a terminal at −4 whose shell runs at 0 and whose compiler runs at 3;
- a compiler at SCHED_IDLE with the idle I/O class, which keeps both.

### Not ported

- `kernel.unprivileged_userns_clone`: NixOS kernels lack that patch.
- `fs.file-max`: systemd already sets the maximum, so this would lower it.
- zram rules: they would disable zswap.
- SATA `max_performance` link power, NVIDIA module options, the audio power-save
  override, split-lock mitigation, and NTP/DNS policy. Each is situational or
  trades battery, security or stability for benchmarks.
- A sched-ext scheduler, and per-application time slices: held for later, see
  [interactive scheduling](interactive-scheduling.md).
