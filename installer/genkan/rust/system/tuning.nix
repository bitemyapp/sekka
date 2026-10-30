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
  steamLaunchers = [
    "steam"
    "steam.sh"
    "Steam.exe"
  ];
  # Steam's rules apply only where Steam is installed. They are the only
  # child_rule rules, and without one the daemon tracks no ancestry.
  steamRules = cfg.tuning.steam.enable;
  replacedRules = shells ++ lib.optionals steamRules (steamLaunchers ++ [ "fossilize_replay" ]);
  # CachyOS's rules without names replaced below. ananicy loads rule files in
  # directory order and the last rule for a name wins, so an added rule could
  # not reliably replace them.
  rules = pkgs.runCommand "ananicy-rules-sekka" { } ''
    cp -r ${pkgs.ananicy-rules-cachyos} $out
    chmod -R u+w $out
    pattern='"name": *"(${lib.concatStringsSep "|" (map lib.escapeRegex replacedRules)})"'
    grep -rlE "$pattern" $out/etc/ananicy.d | while read -r file; do
      sed -i -E "/$pattern/d" "$file"
    done
    if grep -rqE "$pattern" $out/etc/ananicy.d; then
      echo "replaced upstream rules remain" >&2
      exit 1
    fi
  '';
in
{
  options.calamares = {
    tuning.enable = lib.mkEnableOption "CachyOS-inspired kernel, memory, I/O and service defaults";
    tuning.steam.enable = lib.mkOption {
      type = lib.types.bool;
      default = config.programs.steam.enable;
      defaultText = lib.literalExpression "config.programs.steam.enable";
      description = ''
        With tuning, raise Steam's launcher and its same-user process tree,
        and keep its shader-cache processing in the background
        (docs/tuning.md, "Steam process trees").
      '';
    };
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
        package = import ../../../../nix/scheduling/ananicy.nix { inherit pkgs; };
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
        # "compile") are left out. The packaged daemon handles wrapper execs,
        # fork-only children and all threads (nix/scheduling).
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
          }) shells
          # Steam's launcher must not pass the upstream Launcher type's nice
          # 16 and idle I/O to unknown games. The packaged daemon follows
          # same-user ancestry across wrappers, execs, forks and threads.
          # Like every Ananicy rule these match process names, which any
          # user can choose (docs/tuning.md).
          ++ lib.optionals steamRules (
            map (name: {
              inherit name;
              nice = -5;
              nice_only_raise = true;
              sched = "normal";
              sched_keep_realtime = true;
              ioclass = "best-effort";
              child_rule = "sekka-steam-children";
            }) steamLaunchers
            ++ [
              {
                name = "sekka-steam-children";
                nice = -10;
                nice_only_raise = true;
                sched = "normal";
                sched_keep_realtime = true;
                ioclass = "best-effort";
              }
              {
                # Steam's background shader cache processing must not compete
                # with a running game, even through compiler/helper children.
                name = "fossilize_replay";
                type = "BG_CPUIO";
                child_rule = "sekka-steam-shader-children";
              }
              {
                name = "sekka-steam-shader-children";
                type = "BG_CPUIO";
                nice_only_raise = false;
                sched_keep_realtime = false;
              }
            ]
          )
          # Harnesses stay interactive, like Foot and Chrome. Only their child
          # workloads enter the background slice, through the user service.
          ++
            map
              (name: {
                inherit name;
                nice = -4;
                sched = "normal";
              })
              [
                "codex"
                "claude"
                "claude-code"
                "gemini"
                "omp"
              ]
          ++ [
            {
              name = "determinate-nixd";
              type = "BG_CPUIO";
            }
          ];
      };
      # ananicy-cpp reads its rules only when it starts, so restart it when
      # they change, as when a rebuild adds or removes Steam. Otherwise, with
      # Steam newly added, CachyOS's launcher rule (nice 16, idle I/O) would
      # still reach its games until the next boot.
      systemd.services.ananicy-cpp.restartTriggers = [ config.environment.etc."ananicy.d".source ];
      # One shared budget for all job scopes, compared with each ordinary
      # app's weight 100. A shell/compiler rule cannot undo this group weight.
      systemd.user.slices.app-background.sliceConfig = {
        CPUWeight = lib.mkDefault 20;
        IOWeight = lib.mkDefault 20;
      };
      environment.etc."sekka/background-scheduling".text = "enabled\n";
      environment.systemPackages = [
        (import ../../../../nix/scheduling/background.nix { inherit pkgs; })
      ];
      systemd.user.services.sekka-agent-workloads = {
        description = "Separate interactive AI harnesses from their background jobs";
        wantedBy = [ "default.target" ];
        serviceConfig = {
          ExecStart = "${pkgs.python3}/bin/python3 ${../../../../nix/scheduling/agent-workloads.py} ${pkgs.systemd}/bin/busctl";
          Restart = "on-failure";
          RestartSec = 2;
          Slice = "background.slice";
          NoNewPrivileges = true;
        };
      };
      # Builders are children of the daemon, not of the agent's CLI scope.
      # Set policy before any worker threads are created, for both Nix daemons.
      systemd.services.nix-daemon.serviceConfig = {
        CPUWeight = lib.mkDefault 20;
        IOWeight = lib.mkDefault 20;
      };
      nix.daemonCPUSchedPolicy = lib.mkDefault "idle";
      nix.daemonIOSchedClass = lib.mkDefault "idle";
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
