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
  # CachyOS gives these shells nice -4 ("Doc-View"), as it does terminals.
  shells = [
    "bash"
    "dash"
    "fish"
    "ksh"
    "nu"
    "tcsh"
    "zsh"
  ];
  # CachyOS's rules without its shell rules. ananicy loads rule files in
  # directory order and the last rule for a name wins, so an added rule could
  # not reliably replace them.
  rules = pkgs.runCommand "ananicy-rules-sekka" { } ''
    cp -r ${pkgs.ananicy-rules-cachyos} $out
    chmod -R u+w $out
    pattern='"name": *"(${lib.concatStringsSep "|" shells})"'
    grep -rlE "$pattern" $out/etc/ananicy.d | while read -r file; do
      sed -i -E "/$pattern/d" "$file"
    done
    if grep -rqE "$pattern" $out/etc/ananicy.d; then
      echo "shell rules remain" >&2
      exit 1
    fi
  '';
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
      boot.kernelParams = [
        "nowatchdog"
        # Ubuntu's lowlatency-kernel package: full preemption instead of the
        # kernel's default, lazy, and RCU callbacks run by kernel threads
        # rather than on each CPU at interrupt time.
        "preempt=full"
        "rcu_nocbs=all"
      ];
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
      # mq-deadline for NVMe, as SATA SSDs have by default: it serves I/O
      # priority classes in order, so the idle class nix-daemon's ananicy rule
      # gives Nix builds takes effect. Kyber ignores I/O priorities. BFQ for
      # rotating disks.
      hardware.block.defaultSchedulerRotational = "bfq";
      hardware.block.scheduler."nvme[0-9]*" = "mq-deadline";
      services.ananicy = {
        enable = true;
        package = pkgs.ananicy-cpp;
        rulesProvider = rules;
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
        # Compilers and linkers yield a little to the application that started
        # them, which otherwise passes on its own priority (CachyOS gives
        # terminals -4). Nice 3 is about half the CPU weight of a
        # normal task in the same cgroup and keeps the I/O priority the kernel
        # derives from nice at its default level. A rule without "sched" or
        # "ioclass" leaves both alone, so Nix builds keep the idle class
        # nix-daemon's rule gives them. Programs with generic names (Go's
        # "compile") are left out. ananicy names a process after its argv[0]
        # when it sees the exec and ignores a second exec right after, so a
        # linker behind a NixOS wrapper script is taken for bash and keeps its
        # inherited priority; linkers started directly (rust-lld) are matched.
        extraTypes = [
          {
            type = "Sekka_Build";
            nice = 3;
          }
        ];
        extraRules =
          map
            (name: {
              inherit name;
              type = "Sekka_Build";
            })
            [
              "cc1"
              "cc1plus"
              "lto1"
              "clang"
              "clang++"
              "rustc"
              "clippy-driver"
              "ld"
              "ld.bfd"
              "ld.gold"
              "ld.lld"
              "lld"
              "rust-lld"
              "mold"
              "ld.mold"
            ]
          # Shells run at nice 0: a terminal keeps CachyOS's -4 for itself,
          # and what runs in it starts at normal priority. ananicy sets
          # absolute values, so this also resets a shell started lower (nice,
          # a service's Nice=) to 0, as CachyOS's rule set it to -4.
          ++ map (name: {
            inherit name;
            nice = 0;
          }) shells;
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
