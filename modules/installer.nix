# Live media only; never imported by the installed machine.
# The live session is Aizome, logged in automatically, with Genkan open in it.
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
    fonts = inputs.aizome.packages.${pkgs.stdenv.hostPlatform.system}.fonts;
  };
  diagnostics = import ../nix/live-diagnostics.nix { inherit pkgs; };
  copytoram = import ../nix/iso-copytoram.nix { inherit pkgs; };
  # Installed systems' Aizome wallpapers (installer/genkan/rust/system/desktops.nix),
  # by store path: the media already carry them for the prebuilt systems.
  wallpapers = inputs.tatami.packages.${pkgs.stdenv.hostPlatform.system}.wallpapers;
  wallpaper = file: "${wallpapers}/share/backgrounds/tatami/${file}";
  applicationCatalog = import ../nix/applications.nix { inherit inputs; };
  references = import ../nix/reference-systems.nix {
    inherit inputs applicationCatalog;
    desktops = config.respin.referenceDesktops;
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
    ./portal-fixes.nix
    ./avahi-fixes.nix
    inputs.aizome.nixosModules.default
    # Sekka's boot splash, as on installed systems.
    ../installer/genkan/rust/system/boot-splash
    # Intel page flips use the same replacement module as installed systems.
    ../installer/genkan/rust/system/xe-dpt
    # Deferred installations build the system from the live session's store.
    ../installer/genkan/rust/system/binary-cache.nix
    # The live session joins Wi-Fi with installed systems' wpa_supplicant.
    ../installer/genkan/rust/system/wpa-supplicant
  ];
  options.respin = {
    enableInstaller = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Include and start Genkan. Disable for live media whose installed boot format Genkan does not support.";
    };
    referenceDesktops = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [
        "aizome"
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
      description = "Store optional applications marked for preload on the media. Development images may omit them.";
    };
  };
  config = {
    # Upstream copies after removing the original mount and has no RAM check.
    # A large Sekka ISO can otherwise OOM the initrd or leave .ro-store without
    # its backing image. Preserve media until a complete RAM copy is available.
    boot.initrd.systemd.storePaths = [
      "${copytoram}/bin/sekka-copy-iso-to-ram"
    ]
    ++ map (package: "${lib.getBin package}/bin") copytoram.runtimeInputs;
    boot.initrd.systemd.services.copytoram.script = lib.mkForce ''
      exec ${copytoram}/bin/sekka-copy-iso-to-ram
    '';
    # Aizome, logged in automatically: greetd starts the session at boot
    # (programs.aizome.greeter's initial session), and Genkan opens in it.
    # Logging out shows Aizome's login screen.
    programs.aizome = {
      enable = true;
      wallpapers.night = wallpaper "da-nang-at-night.jpg";
      wallpapers.day = wallpaper "tokyo-at-dusk.jpg";
      greeter.enable = true;
    };
    services.displayManager = {
      defaultSession = "aizome";
      autoLogin = {
        enable = true;
        user = "nixos";
      };
    };
    # Aizome's welcome tour is for an installed system; the live session
    # starts with the installer.
    environment.etc."xdg/autostart/org.aizome.Annai.Welcome.desktop".enable = lib.mkForce false;
    # The live session never locks or turns its displays off on its own, like
    # Ubuntu's: Aizome locks after 10 minutes and turns displays off after 15
    # by default, and an unattended Acer with an NVIDIA GPU that slept never
    # lit its screen again. Aizome doesn't sleep by itself. While installing,
    # the helper also holds a logind lock against sleep. Settings writes the
    # same file if the live user changes them.
    systemd.tmpfiles.settings."10-installer-desktop" = {
      "/home/nixos/.config".d = {
        user = "nixos";
        group = "users";
        mode = "0755";
      };
      "/home/nixos/.config/aizome".d = {
        user = "nixos";
        group = "users";
        mode = "0755";
      };
      "/home/nixos/.config/aizome/settings.toml".f = {
        user = "nixos";
        group = "users";
        mode = "0644";
        argument = "[idle]\nlock_after_minutes = 0\nmonitors_off_after_minutes = 0";
      };
    };
    environment.systemPackages = [
      pkgs.file
      inputs.fh.packages.${pkgs.stdenv.hostPlatform.system}.default
      diagnostics
      # Small interactive diagnostics, also used to prove real GUI event delivery
      # in our disposable VM tests (not merely successful QMP commands).
      pkgs.xev
      pkgs.xwininfo
    ]
    ++ lib.optionals config.respin.enableInstaller [
      calamares
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
        if (${lib.boolToString config.respin.enableInstaller} && action.id == "org.calamares.nixos.install" &&
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
    hardware.cpu.intel.updateMicrocode = true;
    hardware.cpu.amd.updateMicrocode = true;
    # NVIDIA's own driver (latest release, open kernel modules) instead of
    # nouveau, which can hang hybrid laptops when a program wakes the NVIDIA
    # GPU. Machines without one simply do not load it. The installed system's
    # calamares.nvidia module chooses the same package, so installations copy
    # it from the media: NVIDIA's packages are not in the public binary cache.
    services.xserver.videoDrivers = [
      "nvidia"
      "modesetting"
      "fbdev"
    ];
    hardware.nvidia = {
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
    # Sekka's boot splash, as installed systems show it (DEVELOPERS.md, "Boot
    # splash"); Esc shows the boot messages. The media's initrd has no GPU
    # driver, and may be copying the image to memory: draw on the firmware's
    # framebuffer at once, as encrypted installations do, rather than leave
    # the screen blank for DeviceTimeout's 8 seconds. The splash moves to the
    # GPU when its driver loads.
    calamares.bootSplash.enable = true;
    boot.plymouth.extraConfig = ''
      UseSimpledrm=2
    '';

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
    isoImage.edition = lib.mkForce "aizome-determinate-rust";
    isoImage.volumeID = "nixos-determinate-x86_64";
    image.baseName = lib.mkForce "sekka-${config.system.nixos.label}-x86_64-linux";
    # Put the distinguishing information FIRST. The upstream menu appends the
    # distro/revision; our old long branding hid kernel versions on small screens.
    isoImage.prependToMenuLabel = "${config.boot.kernelPackages.kernel.version} Aizome (default) | ";
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
        isoImage.prependToMenuLabel = lib.mkForce "${config.boot.kernelPackages.kernel.version} Aizome (LTS) | ";
        environment.etc."calamares-nixos/settings.json".text = lib.mkForce (settings "lts");
      };
    # The recovery entry: the same Aizome session, drawn in software. Aizome's
    # compositor composites with pixman instead of GLES, its apps draw on the
    # CPU as they always do, and other programs get Mesa's software GL. There
    # is no GPU driver for NVIDIA GPUs: neither NVIDIA's nor nouveau.
    specialisation.compatibility.configuration = { config, lib, ... }: {
      isoImage.prependToMenuLabel = lib.mkForce "${config.boot.kernelPackages.kernel.version} Aizome (software) | ";
      services.xserver.videoDrivers = lib.mkForce [
        "modesetting"
        "fbdev"
      ];
      boot.blacklistedKernelModules = [ "nouveau" ];
      # Through PAM, for the login screen's compositor and the session alike;
      # aizome-session also hands its environment file to the compositor's
      # service.
      environment.sessionVariables = {
        AIZOME_RENDERER = "pixman";
        LIBGL_ALWAYS_SOFTWARE = "1";
      };
      environment.etc."xdg/aizome/environment".text = lib.mkAfter ''
        AIZOME_RENDERER=pixman
        LIBGL_ALWAYS_SOFTWARE=1
      '';
    };
    isoImage.squashfsCompression = "zstd -Xcompression-level 6";
    # Keep preload-eligible packages and a prebuilt installed system for every desktop
    # in the read-only store, without adding them to the live session. An
    # installation then copies from the media instead of downloading into RAM.
    isoImage.storeContents =
      lib.optional config.respin.cacheApplications applicationCatalog.cache ++ references.toplevels;
    # The serial console keeps boot logs for VM tests and serial-attached
    # machines. Plymouth would show only text with one named; the splash
    # stays on the display.
    boot.kernelParams = [
      "console=ttyS0,115200n8"
      "console=tty0"
      "plymouth.ignore-serial-consoles"
    ];

  };
}
