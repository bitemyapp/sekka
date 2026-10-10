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
commit `e27ea45` (GPL-3.0) and the CachyOS kernel's runtime defaults. Only
settings that work with stock NixOS kernels and nixpkgs packages are used. There
is no BORE scheduler, no custom kernel, and no x86-64-v3 package rebuild.
Implemented in the installer's `rust/system/tuning.nix`, which is copied to
`/etc/nixos/calamares/tuning.nix` on the installed system.

| Area | Setting |
|---|---|
| Memory | `vm.vfs_cache_pressure=50`; `vm.dirty_bytes=256 MiB` and `vm.dirty_background_bytes=64 MiB`, absolute limits that avoid huge writeback stalls on large-RAM machines; `vm.dirty_writeback_centisecs=1500`; `vm.watermark_boost_factor=0` and `vm.compaction_proactiveness=0`, as in the CachyOS kernel |
| Transparent huge pages | `defrag=defer+madvise`, `khugepaged/max_ptes_none=409` |
| Kernel | `kernel.nmi_watchdog=0` plus the `nowatchdog` parameter; watchdog modules `iTCO_wdt`, `sp5100_tco` and `wdat_wdt` blacklisted; `kernel.kptr_restrict=2`; console log level 3; `ntsync` module loaded for Wine/Proton |
| I/O | BFQ for rotating disks, Kyber for NVMe; SATA SSDs keep the kernel default, mq-deadline |
| Network | `net.core.netdev_max_backlog=4096` |
| Scheduling | ananicy-cpp with the CachyOS rule set (`ananicy-rules-cachyos`), adjusting nice, I/O and OOM priorities only. Its cgroup features are off: moving processes out of their systemd units broke VT switching and session cleanup at logout |
| Memory pressure | systemd-oomd enabled for system and user slices |
| systemd | `DefaultTimeoutStopSec=10s` for system and user managers; `DefaultLimitNOFILE` 2048:2097152 (system) and 1024:1048576 (user); `Delegate=cpu cpuset io memory pids` for user managers; journal capped at 50 MiB; coredumps cleaned after 3 days; rtkit logging at info |
| Services already default in NixOS | weekly fstrim, dbus-broker, systemd initrd |

Deliberately not ported:

- `kernel.unprivileged_userns_clone`: NixOS kernels lack that patch.
- `fs.file-max`: systemd already sets the maximum, so this would lower it.
- zram rules: they would disable zswap.
- SATA `max_performance` link power, NVIDIA module options, the audio power-save
  override, split-lock mitigation, `preempt=full`, a sched-ext scheduler, and
  NTP/DNS policy. Each is situational or trades battery, security or stability
  for benchmarks.
