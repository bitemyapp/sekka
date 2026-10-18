# Sekka on Apple Silicon Macs (calamares.appleSilicon), starting with the
# 14-inch MacBook Pro with an M3 Pro (J514S). Every installed system imports it
# (installer/genkan/rust/system); it does nothing unless enabled.
#
# Booting: the Asahi installer's m1n1 first stage chainloads m1n1/boot.bin from
# the EFI system partition. This module writes that file (m1n1-aurora, the
# kernel's device trees, U-Boot and m1n1's settings) with the bootloader, and
# U-Boot starts systemd-boot. The kernel boot steps follow
# nix-community/nixos-apple-silicon (MIT).
#
# Apple's peripheral firmware (Wi-Fi, Bluetooth, keyboard, trackpad, camera)
# is not redistributable: the Asahi installer extracts it on the Mac into
# vendorfw/firmware.cpio on the EFI system partition. As Asahi's interop notes
# ask, it is loaded at boot rather than built into the system: systemd-boot
# loads the archive as a second initrd, the initrd finds it at /vendorfw, and
# hands it to the booted system on a tmpfs at /lib/firmware/vendor, which the
# kernel searches.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.calamares.appleSilicon;
  kernel = pkgs.callPackage ./kernel.nix { };
  m1n1 = pkgs.callPackage ./m1n1.nix { };
  uboot = pkgs.callPackage ./uboot.nix { };
  mesa = pkgs.callPackage ./mesa.nix { };
  graphics = pkgs.callPackage ./graphics.nix { inherit mesa; };
  esp = config.boot.loader.efi.efiSysMountPoint;
  switches = lib.optionals cfg.t6030Handoff m1n1.t6030Switches;

  # m1n1's second stage: m1n1, every Apple device tree (m1n1 picks the Mac's),
  # U-Boot, then m1n1's settings, one per line.
  bootBin = pkgs.runCommand "boot.bin" { } ''
    cat ${m1n1}/lib/m1n1/m1n1.bin \
      ${config.boot.kernelPackages.kernel}/dtbs/apple/*.dtb \
      ${uboot}/u-boot-nodtb.bin.gz > $out
    ${lib.optionalString (switches != [ ]) "printf '%s\\n' ${lib.escapeShellArgs switches} >> $out"}
  '';
in
{
  options.calamares.appleSilicon = {
    enable = lib.mkEnableOption "Sekka's Apple Silicon platform: Omarchy's linux-aurora kernel, m1n1-aurora and U-Boot, and Apple's peripheral firmware from the EFI system partition";
    t6030Handoff = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Turn on m1n1-aurora's display and GPU handoff for an M3 Pro (T6030):
        the native display driver, external displays and the GPU. Omarchy's
        release was reported working with it on the 14-inch (J514S) and booted
        on the 16-inch (J516S) MacBook Pro.
      '';
    };
    m3ProGraphics = lib.mkOption {
      type = lib.types.bool;
      default = cfg.t6030Handoff;
      defaultText = lib.literalExpression "config.calamares.appleSilicon.t6030Handoff";
      description = ''
        Let uwsm sessions (Tatami) use the M3 Pro's native OpenGL and Vulkan
        when the GPU passes the session's checks. Every other session, and
        any login that fails them, renders in software.
      '';
    };
    graphicsUsers = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = lib.optional (config.calamares ? installUser) config.calamares.installUser;
      defaultText = lib.literalExpression "[ config.calamares.installUser ]";
      description = "Users who may open the M3 Pro's render node (the render group).";
    };
    initrdModules = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [
        "apple-mailbox"
        "apple_nvmem_spmi"
        "nvme_apple"
        "pinctrl-apple-gpio"
        "macsmc"
        "macsmc-power"
        "macsmc-input"
        "macsmc-hwmon"
        "macsmc-reboot"
        "i2c-pasemi-platform"
        "tps6598x"
        "sn201202x"
        "apple-dart"
        "dwc3"
        "dwc3-of-simple"
        "xhci-pci"
        "pcie-apple"
        "gpio_macsmc"
        "phy-apple-atc"
        "nvmem_apple_efuses"
        "spi-apple"
        "spi-hid-apple"
        "spi-hid-apple-of"
        "rtc-macsmc"
        "spmi-apple-controller"
        "apple-dockchannel"
        "dockchannel-hid"
        "apple-rtkit-helper"
        "usb-storage"
        "xhci-plat-hcd"
        "usbhid"
        "hid_generic"
        # An external Apple keyboard, for a disk password.
        "hid_apple"
        # The native display, from the start where m1n1 hands it over.
        "appledrm"
      ];
      description = "Kernel modules the initrd may need on these Macs, as nixos-apple-silicon lists them.";
    };
    sound = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Asahi's speaker DSP and speakersafetyd, which keep the built-in
        speakers within safe limits; the kernel keeps them quiet without it.
      '';
    };
  };

  config = lib.mkIf cfg.enable (
    lib.mkMerge [
      {
        assertions = [
          {
            assertion = pkgs.stdenv.hostPlatform.system == "aarch64-linux";
            message = "calamares.appleSilicon is for Apple Silicon Macs (aarch64-linux).";
          }
        ];

        # No other kernel boots these Macs.
        boot.kernelPackages = lib.mkForce kernel.packages;
        boot.kernelParams = [
          "earlycon"
          "console=tty0"
          # Apple's SSDs are slow to flush; see nixos-apple-silicon.
          "nvme_apple.flush_interval=0"
          # The initrd's vendor firmware, before /lib/firmware/vendor exists.
          "firmware_class.path=/vendorfw"
        ];
        # As the linux-aurora package's modprobe.d: m1n1's device tree copy
        # and log stay unbound unless a tool asks for them.
        boot.blacklistedKernelModules = [ "phram" ];
        boot.initrd.availableKernelModules = cfg.initrdModules;
        # NixOS adds a PC's storage and USB drivers to every initrd (ahci,
        # SATA, older USB controllers), which this kernel lacks or these Macs
        # do not have.
        boot.initrd.includeDefaultModules = false;
        # These Macs have no TPM, and the kernel has no TPM drivers.
        boot.initrd.systemd.tpm2.enable = false;
        systemd.tpm2.enable = false;

        # U-Boot has no EFI variables to write.
        boot.loader.efi.canTouchEfiVariables = lib.mkForce false;
        boot.loader.systemd-boot.extraFiles."m1n1/boot.bin" = bootBin;
        boot.loader.systemd-boot.extraInstallCommands = ''
          if [ -f ${esp}/vendorfw/firmware.cpio ]; then
            for entry in ${esp}/loader/entries/nixos-*.conf; do
              [ -f "$entry" ] || continue
              if ! ${pkgs.gnugrep}/bin/grep -qx 'initrd /vendorfw/firmware.cpio' "$entry"; then
                ${pkgs.gnused}/bin/sed -i '0,/^initrd /s//initrd \/vendorfw\/firmware.cpio\ninitrd /' "$entry"
              fi
            done
          fi
        '';
        system.build.appleSiliconBootBin = bootBin;

        boot.initrd.systemd.initrdBin = [
          pkgs.coreutils
          pkgs.util-linux
        ];
        boot.initrd.systemd.services.sekka-vendor-firmware = {
          description = "Apple's peripheral firmware for the booted system";
          wantedBy = [ "initrd.target" ];
          after = [ "initrd-root-fs.target" ];
          before = [ "initrd.target" ];
          unitConfig = {
            DefaultDependencies = false;
            ConditionPathIsDirectory = "/vendorfw";
          };
          serviceConfig.Type = "oneshot";
          script = ''
            mkdir -p /sysroot/lib/firmware/vendor
            mount -t tmpfs -o mode=0755,nosuid,nodev,noexec vendorfw /sysroot/lib/firmware/vendor
            cp -a /vendorfw/. /sysroot/lib/firmware/vendor/
          '';
        };

        # The Apple Video Decoder's firmware.
        hardware.firmware = [ pkgs.avd-fw ];

        # What installed systems set for PCs, which has no meaning here.
        hardware.cpu.intel.updateMicrocode = lib.mkForce false;
        hardware.cpu.amd.updateMicrocode = lib.mkForce false;
        services.pipewire.alsa.support32Bit = lib.mkForce false;
      }

      (lib.mkIf cfg.sound {
        services.pulseaudio.enable = false;
        security.rtkit.enable = true;
        services.pipewire = {
          enable = true;
          alsa.enable = true;
          pulse.enable = true;
          configPackages = [ pkgs.asahi-audio ];
          wireplumber = {
            enable = true;
            configPackages = [ pkgs.asahi-audio ];
          };
        };
        environment.variables.ALSA_CONFIG_UCM2 = "${pkgs.alsa-ucm-conf-asahi}/share/alsa/ucm2";
        systemd.user.services.pipewire.environment.ALSA_CONFIG_UCM2 =
          config.environment.variables.ALSA_CONFIG_UCM2;
        systemd.user.services.wireplumber.environment.ALSA_CONFIG_UCM2 =
          config.environment.variables.ALSA_CONFIG_UCM2;
        systemd.packages = [ pkgs.speakersafetyd ];
        services.udev.packages = [ pkgs.speakersafetyd ];
      })

      (lib.mkIf cfg.m3ProGraphics {
        # Software unless the session's checks pass (graphics.nix): the
        # kernel refuses Nixpkgs' Mesa on the M3 GPU.
        environment.sessionVariables = {
          LIBGL_ALWAYS_SOFTWARE = "1";
          MESA_M3_FALLBACK = "software";
        };
        environment.etc."xdg/uwsm/env.d/50-sekka-m3-gpu".text = ''
          # The M3 Pro's GPU for this session if its checks pass (calamares.appleSilicon).
          . ${graphics.sessionEnv}
        '';
        # Only the render group opens the GPU's render node, so a greeter or
        # other session never reaches it with Nixpkgs' Mesa.
        services.udev.extraRules = ''
          SUBSYSTEM=="drm", KERNEL=="renderD*", DRIVERS=="asahi", ATTR{device/of_node/compatible}=="apple,agx-t6030", GROUP="render", MODE="0660"
        '';
        users.users = lib.genAttrs cfg.graphicsUsers (_: {
          extraGroups = [ "render" ];
        });
        systemd.user.services.sekka-m3-gpu-confirm = {
          description = "Confirm that this login on the M3 Pro's GPU came up";
          wantedBy = [ "graphical-session.target" ];
          after = [ "graphical-session.target" ];
          partOf = [ "graphical-session.target" ];
          unitConfig.ConditionEnvironment = "SEKKA_M3_GPU_RUN";
          serviceConfig = {
            Type = "oneshot";
            ExecStart = graphics.confirm;
          };
        };
        systemd.services.sekka-m3-gpu-performance = {
          description = "The M3 Pro GPU's tested performance profile";
          wantedBy = [ "multi-user.target" ];
          after = [
            "systemd-modules-load.service"
            "sys-kernel-debug.mount"
          ];
          before = [ "display-manager.service" ];
          serviceConfig = {
            Type = "oneshot";
            RemainAfterExit = true;
            ExecStart = graphics.performance;
          };
        };
        environment.systemPackages = [ graphics.abiCheck ];
      })
    ]
  );
}
