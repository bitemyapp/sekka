# Live media only; never imported by the installed machine.
# Plasma setup follows the official NixOS graphical installer module (MIT).
{
  config,
  lib,
  pkgs,
  inputs,
  installedLock,
  sekkaSource,
  ...
}:
let
  calamares = import ../nix/calamares.nix {
    inherit pkgs;
    source = inputs.calamares;
  };
  diagnostics = import ../nix/live-diagnostics.nix { inherit pkgs; };
  applicationCatalog = import ../nix/applications.nix {
    inherit inputs;
    system = pkgs.stdenv.hostPlatform.system;
  };
  references = import ../nix/reference-systems.nix {
    inherit inputs applicationCatalog;
    system = pkgs.stdenv.hostPlatform.system;
    desktops = config.respin.referenceDesktops;
    cacheApplications = config.respin.cacheApplications;
  };
  settings =
    kernel:
    builtins.toJSON {
      template_dir = "/etc/calamares-nixos";
      zoneinfo = "${pkgs.tzdata}/share/zoneinfo";
      state_version = config.system.nixos.release;
      inherit kernel;
      test_diagnostics = false;
    };
in
{
  imports = [
    # Deferred installations build the system from the live session's store.
    ../installer/genkan/rust/system/binary-cache.nix
    # The live session joins Wi-Fi with installed systems' wpa_supplicant.
    ../installer/genkan/rust/system/wpa-supplicant
  ];
  options.respin = {
    referenceDesktops = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [
        "plasma"
        "gnome"
        "xfce"
        "hyprland"
        "tatami"
      ];
      description = "Desktops whose complete installed systems are prebuilt on the media.";
    };
    cacheApplications = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Store every optional application on the media. Development images may omit it.";
    };
  };
  config = {
    services.desktopManager.plasma6 = {
      enable = true;
      enableQt5Integration = false;
    };
    services.displayManager = {
      plasma-login-manager.enable = true;
      autoLogin = {
        enable = true;
        user = "nixos";
      };
    };
    environment.plasma6.excludePackages = [ pkgs.kdePackages.plasma-workspace-wallpapers ];
    programs.kde-pim.enable = false;
    # The live session never dims, blanks, locks or sleeps on its own, like
    # Ubuntu's. Plasma's laptop defaults lock after 5 minutes, turn the screen
    # off after 10 and sleep after 15 on AC (5 on battery): an unattended Acer
    # with an NVIDIA GPU slept and never lit its screen again. While
    # installing, the helper also holds a logind lock against sleep.
    environment.etc."xdg/powerdevilrc".text =
      lib.concatMapStrings
        (profile: ''
          [${profile}][Display]
          DimDisplayWhenIdle=false
          TurnOffDisplayWhenIdle=false
          LockBeforeTurnOffDisplay=false

          [${profile}][SuspendAndShutdown]
          AutoSuspendAction=0

        '')
        [
          "AC"
          "Battery"
          "LowBattery"
        ];
    environment.etc."xdg/kscreenlockerrc".text = ''
      [Daemon]
      Autolock=false
      LockOnResume=false
    '';
    environment.systemPackages = [
      pkgs.file
      inputs.fh.packages.${pkgs.stdenv.hostPlatform.system}.default
      calamares
      diagnostics
      # Small interactive diagnostics, also used to prove real GUI event delivery
      # in our disposable VM tests (not merely successful QMP commands).
      pkgs.xev
      pkgs.xwininfo
      (pkgs.makeAutostartItem {
        name = "org.calamares.NixOSRust";
        package = calamares;
      })
    ];
    # No blanket wheel grant from the graphical base. Only the local active live
    # user may start this exact helper without a password. Keep live networking
    # usable too: mkForce replaces NetworkManager's normal group-based rule.
    security.polkit.enable = true;
    security.polkit.enablePkexecWrapper = true;
    security.polkit.extraConfig = lib.mkForce ''
      polkit.addRule(function(action, subject) {
        if (action.id == "org.calamares.nixos.install" &&
            subject.user == "nixos" && subject.local && subject.active) {
          return polkit.Result.YES;
        }
        if (action.id.indexOf("org.freedesktop.NetworkManager.") == 0 &&
            subject.user == "nixos" && subject.local && subject.active &&
            subject.isInGroup("networkmanager")) {
          return polkit.Result.YES;
        }
      });
    '';
    # NetworkManager enables wpa_supplicant in D-Bus-controlled mode. Forcing
    # wireless.enable off removes that backend, leaving even working radios
    # unavailable. Do not run a separate, competing wireless manager.
    networking.networkmanager.enable = true;
    networking.networkmanager.wifi.backend = "wpa_supplicant";
    hardware.enableRedistributableFirmware = true;
    # Universal media can carry both bundles; early boot applies only the update
    # matching the CPU. Installed systems use upstream vendor detection.
    hardware.cpu.intel.updateMicrocode = pkgs.stdenv.hostPlatform.isx86_64;
    hardware.cpu.amd.updateMicrocode = pkgs.stdenv.hostPlatform.isx86_64;
    # NVIDIA's own driver (latest release, open kernel modules) instead of
    # nouveau, which can hang hybrid laptops when a program wakes the NVIDIA
    # GPU. Machines without one simply do not load it. The installed system's
    # calamares.nvidia module chooses the same package, so installations copy
    # it from the media: NVIDIA's packages are not in the public binary cache.
    services.xserver.videoDrivers = lib.optional pkgs.stdenv.hostPlatform.isx86_64 "nvidia" ++ [
      "modesetting"
      "fbdev"
    ];
    hardware.nvidia = lib.mkIf pkgs.stdenv.hostPlatform.isx86_64 {
      package = config.boot.kernelPackages.nvidiaPackages.latest;
      open = true;
      modesetting.enable = true;
      # Suspend and resume through NVIDIA's services with video memory
      # preserved, as installed systems do (calamares.nvidia).
      powerManagement.enable = true;
    };
    nixpkgs.config.allowUnfree = true;
    services.libinput.enable = true;
    # Compressed RAM swap for the live session, with CachyOS's zram values.
    # Evaluating a desktop system needs a few GiB, and the live store itself
    # is RAM (tmpfs pages are swappable): this keeps machines with 6-8 GiB from
    # running out of memory before installation. Installed systems instead get
    # a RAM-sized swap partition with zswap.
    zramSwap = {
      enable = true;
      algorithm = "zstd";
      memoryPercent = 100;
    };
    boot.kernel.sysctl = {
      "vm.swappiness" = 150;
      "vm.page-cluster" = 0;
    };

    # Recent laptops need recent DRM, I2C and HID support. Keep the version pinned
    # by our existing lock; the older kernel is an explicitly named fallback.
    boot.kernelPackages = pkgs.linuxPackages_latest;
    boot.supportedFilesystems = {
      ext4 = true;
      btrfs = true;
      xfs = true;
      vfat = true;
      zfs = lib.mkForce false;
    };
    # Keep boot progress and the text-console recovery route visible.
    boot.plymouth.enable = lib.mkForce false;

    # The native backend writes the target flake and explicitly selects it.
    # No Python patch, global-storage bridge or nixos-generate-config override.
    environment.etc."calamares-nixos/flake.nix.in".source = ../templates/flake.nix.in;
    environment.etc."calamares-nixos/flake.lock".text = builtins.toJSON installedLock;
    # The installed flake's `sekka` input is locked to this source: with
    # it in the live store, preparing an installation need not download it.
    system.extraDependencies = [
      sekkaSource
    ];
    environment.etc."calamares-nixos/settings.json".text = settings "latest";
    # Store path lists of the prebuilt reference systems and applications, read
    # by the installer to warm the page cache for the current selection.
    isoImage.contents = references.lists;
    systemd.tmpfiles.settings."10-installer-desktop" = {
      "/home/nixos/Desktop".d = {
        user = "nixos";
        group = "users";
        mode = "0755";
      };
      "/home/nixos/Desktop/nixos-manual.desktop"."L+".argument =
        "/run/current-system/sw/share/applications/nixos-manual.desktop";
      "/home/nixos/Desktop/gparted.desktop"."L+".argument =
        "${pkgs.gparted}/share/applications/gparted.desktop";
      "/home/nixos/Desktop/org.calamares.NixOSRust.desktop"."L+".argument =
        "${calamares}/share/applications/org.calamares.NixOSRust.desktop";
    };

    isoImage.edition = lib.mkForce "plasma-determinate-rust";
    isoImage.volumeID = "nixos-determinate-${pkgs.stdenv.hostPlatform.parsed.cpu.name}";
    image.baseName = lib.mkForce "sekka-${config.system.nixos.label}-${pkgs.stdenv.hostPlatform.system}";
    # Put the distinguishing information FIRST. The upstream menu appends the
    # distro/revision; our old long branding hid kernel versions on small screens.
    isoImage.prependToMenuLabel = "${config.boot.kernelPackages.kernel.version} Plasma (default) | ";
    isoImage.appendToMenuLabel = lib.mkForce "";
    isoImage.configurationName = lib.mkForce null;
    specialisation.lts_kernel.configuration =
      {
        config,
        lib,
        pkgs,
        ...
      }:
      {
        boot.kernelPackages = lib.mkForce pkgs.linuxPackages;
        isoImage.prependToMenuLabel = lib.mkForce "${config.boot.kernelPackages.kernel.version} Plasma (LTS) | ";
        environment.etc."calamares-nixos/settings.json".text = lib.mkForce (settings "lts");
      };
    specialisation.compatibility.configuration = { config, lib, ... }: {
      isoImage.prependToMenuLabel = lib.mkForce "${config.boot.kernelPackages.kernel.version} Xfce/X11 (software) | ";
      services.desktopManager.plasma6.enable = lib.mkForce false;
      services.displayManager.plasma-login-manager.enable = lib.mkForce false;
      services.xserver.displayManager.lightdm.enable = true;
      services.xserver.desktopManager.xfce = {
        enable = true;
        enableScreensaver = false;
      };
      services.displayManager.defaultSession = lib.mkForce "xfce";
      # The fallback uses no GPU driver for NVIDIA GPUs: neither NVIDIA's
      # nor nouveau.
      services.xserver.videoDrivers = lib.mkForce [
        "modesetting"
        "fbdev"
      ];
      boot.blacklistedKernelModules = [ "nouveau" ];
      environment.sessionVariables = {
        LIBGL_ALWAYS_SOFTWARE = "1";
        GSK_RENDERER = "cairo";
      };
      # Retain native kernel modesetting: nomodeset is a separate upstream boot
      # menu option, not a blanket workaround for all graphics hardware.
      services.xserver.deviceSection = ''
        Option "AccelMethod" "none"
        Option "SWcursor" "true"
      '';
      environment.etc."xdg/xfce4/xfconf/xfce-perchannel-xml/xfwm4.xml".text = ''
        <?xml version="1.0" encoding="UTF-8"?>
        <channel name="xfwm4" version="1.0">
          <property name="general" type="empty">
            <property name="use_compositing" type="bool" value="false"/>
          </property>
        </channel>
      '';
    };
    isoImage.squashfsCompression = "zstd -Xcompression-level 6";
    # Keep optional packages and a prebuilt installed system for every desktop
    # in the read-only store, without adding them to the live session. An
    # installation then copies from the media instead of downloading into RAM.
    isoImage.storeContents =
      lib.optional config.respin.cacheApplications applicationCatalog.cache ++ references.toplevels;
    boot.kernelParams = [
      "console=${if pkgs.stdenv.hostPlatform.isAarch64 then "ttyAMA0" else "ttyS0"},115200n8"
      "console=tty0"
    ];

  };
}
