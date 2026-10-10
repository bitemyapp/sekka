# SPDX-License-Identifier: GPL-3.0-or-later
# Portable performance and reliability defaults adapted from CachyOS-Settings
# (https://github.com/CachyOS/CachyOS-Settings, GPL-3.0, commit e27ea45) and
# the CachyOS kernel's runtime defaults. Only settings that work with stock
# NixOS kernels and packages are included: no custom kernel, no rebuilt
# packages. CachyOS uses zram; the installer instead creates a swap partition
# matched to RAM and puts zswap, a compressed in-memory cache, in front of it.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.calamares;
in
{
  options.calamares = {
    tuning.enable = lib.mkEnableOption "CachyOS-inspired kernel, memory, I/O and service defaults";
    zswap.enable = lib.mkEnableOption "zswap in front of the RAM-sized swap partition";
  };
  config = lib.mkMerge [
    (lib.mkIf cfg.zswap.enable {
      # Written directly: nixpkgs' boot.zswap module also sets zswap.zpool,
      # which Linux 6.18 removed. zstd and zsmalloc are the kernel defaults.
      boot.kernelParams = [
        "zswap.enabled=1"
        "zswap.compressor=zstd"
        "zswap.max_pool_percent=25"
        # Write cold compressed pages back to disk before the pool fills.
        "zswap.shrinker_enabled=1"
      ];
      boot.kernel.sysctl = {
        # Swapping first lands in compressed RAM, so reclaim anonymous memory
        # as readily as file cache (CachyOS: 100).
        "vm.swappiness" = 100;
        # CachyOS uses 0 for zram. Pages written back by zswap live on an SSD,
        # where small readahead still helps.
        "vm.page-cluster" = 1;
      };
    })
    (lib.mkIf cfg.tuning.enable {
      boot.kernel.sysctl = {
        "vm.vfs_cache_pressure" = 50;
        # Absolute dirty limits avoid multi-gigabyte writeback stalls on
        # machines with lots of RAM.
        "vm.dirty_bytes" = 268435456;
        "vm.dirty_background_bytes" = 67108864;
        "vm.dirty_writeback_centisecs" = 1500;
        # CachyOS kernel defaults: no watermark boosting or proactive compaction.
        "vm.watermark_boost_factor" = 0;
        "vm.compaction_proactiveness" = 0;
        "kernel.nmi_watchdog" = 0;
        "kernel.kptr_restrict" = 2;
        "net.core.netdev_max_backlog" = 4096;
      };
      boot.consoleLogLevel = 3;
      boot.kernelParams = [ "nowatchdog" ];
      # Hardware watchdogs only cost wakeups on a desktop.
      boot.blacklistedKernelModules = [
        "iTCO_wdt"
        "sp5100_tco"
        "wdat_wdt"
      ];
      # Fast Windows synchronization primitives for Wine and Proton.
      boot.kernelModules = [ "ntsync" ];
      boot.kernel.sysfs.kernel.mm.transparent_hugepage = {
        defrag = "defer+madvise";
        khugepaged.max_ptes_none = 409;
      };
      # CachyOS I/O schedulers: BFQ for rotating disks, Kyber for NVMe;
      # SATA SSDs keep the kernel default, mq-deadline.
      hardware.block.defaultSchedulerRotational = "bfq";
      hardware.block.scheduler."nvme[0-9]*" = "kyber";
      services.ananicy = {
        enable = true;
        package = pkgs.ananicy-cpp;
        rulesProvider = pkgs.ananicy-rules-cachyos;
        # Adjust priorities only; never move processes between cgroups. A
        # process outside its systemd unit escapes logind and polkit session
        # tracking (a compositor can then no longer switch VTs) and outlives
        # its unit at logout, still holding the GPU for the next session.
        # nixpkgs forces the realtime workaround, which moves compositors to
        # the root cgroup.
        settings = {
          cgroup_load = false;
          apply_cgroup = false;
          cgroup_realtime_workaround = lib.mkForce false;
        };
      };
      # Kill the largest offender under sustained memory pressure instead of
      # letting the desktop thrash.
      systemd.oomd = {
        enable = true;
        enableSystemSlice = true;
        enableUserSlices = true;
      };
      systemd.services."user@" = {
        overrideStrategy = "asDropin";
        serviceConfig.Delegate = "cpu cpuset io memory pids";
      };
      systemd.settings.Manager = {
        DefaultTimeoutStopSec = "10s";
        DefaultLimitNOFILE = "2048:2097152";
      };
      systemd.user.settings.Manager = {
        DefaultTimeoutStopSec = "10s";
        DefaultLimitNOFILE = "1024:1048576";
      };
      services.journald.settings.Journal.SystemMaxUse = "50M";
      systemd.services.rtkit-daemon.serviceConfig.LogLevelMax = "info";
      systemd.tmpfiles.settings."10-calamares-tuning"."/var/lib/systemd/coredump".e.age = "3d";
      # Already NixOS defaults; stated because CachyOS depends on them.
      services.fstrim.enable = true;
      services.dbus.implementation = "broker";
    })
  ];
}
