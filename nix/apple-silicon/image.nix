# Sekka for the Asahi installer, starting with the 14-inch MacBook Pro with an
# M3 Pro (J514S). The Asahi installer, run from macOS, shrinks macOS, creates
# the EFI system partition and the root partition, and writes them from an OS
# package: a zip of the partition's files (esp/) and a raw filesystem image
# (root.img). installer_data.json describes the package; Sekka's bootstrap
# (install-sekka.sh) runs the unmodified installer with it, from this
# directory, so no USB stick or download server is needed.
#
# The installer's m1n1 first stage chainloads m1n1/boot.bin from the new EFI
# system partition and gives it that partition's UUID; it also copies Apple's
# firmware for this Mac to vendorfw/ there (copy_firmware). The ESP here is
# what NixOS's systemd-boot would write for the first generation, with the
# firmware archive as the entry's first initrd; NixOS takes it over at the
# first rebuild. The root image holds the system's closure, registered in the
# Nix database on first boot, and grows to fill its partition.
{
  inputs,
  nixpkgs,
  revision,
}:
let
  system = "aarch64-linux";
  pkgs = nixpkgs.legacyPackages.${system};
  inherit (nixpkgs) lib;

  name = "sekka-j514s-${revision}";
  # The FAT serial the installer gives the EFI system partition
  # (newfs_msdos -I), and its /dev/disk/by-uuid name.
  espVolumeId = "0x5ecca5e1";
  espUuid = "5ECC-A5E1";
  rootLabel = "sekka-root";
  # The installer's ESP for UEFI-only installs is 500 MB; NixOS keeps a
  # kernel and initrd per generation there, so this one is larger.
  espSize = 1024 * 1024 * 1024;

  nixos = nixpkgs.lib.nixosSystem {
    inherit system;
    specialArgs = {
      applicationPkgs = import inputs.applications {
        inherit system;
        config.allowUnfree = true;
      };
      aiPackages = inputs.ai-apps.packages.${system};
      ompPackage = inputs.omp.packages.${system}.omp;
    };
    modules = [
      inputs.determinate.nixosModules.default
      inputs.calamares.nixosModules.default
      (
        { config, pkgs, ... }:
        {
          calamares = {
            installUser = "sekka";
            applications = [ "firefox" ];
            desktops = [ "tatami" ];
            defaultDesktop = "tatami";
            tuning.enable = true;
            appleSilicon = {
              enable = true;
              t6030Handoff = true;
            };
          };
          boot.loader.systemd-boot.enable = true;
          # QEMU's virt machine reaches its disks through virtio-mmio, so this
          # same image boots in a test VM; on a Mac the module never loads.
          boot.initrd.availableKernelModules = [ "virtio_mmio" ];
          boot.loader.systemd-boot.configurationLimit = 5;
          fileSystems."/" = {
            device = "/dev/disk/by-label/${rootLabel}";
            fsType = "ext4";
            options = [ "noatime" ];
            # The installer grows the partition (expand); this grows ext4.
            autoResize = true;
          };
          fileSystems."/boot" = {
            device = "/dev/disk/by-uuid/${espUuid}";
            fsType = "vfat";
            options = [
              "fmask=0077"
              "dmask=0077"
            ];
          };
          networking.hostName = "sekka";
          networking.networkmanager.enable = true;
          hardware.enableRedistributableFirmware = true;
          time.timeZone = "UTC";
          i18n.defaultLocale = "en_US.UTF-8";
          services.xserver.xkb.layout = "us";
          console.useXkbConfig = true;
          services.printing.enable = true;
          services.pipewire = {
            enable = true;
            alsa.enable = true;
            pulse.enable = true;
          };
          users.mutableUsers = true;
          users.users.root.initialHashedPassword = "!";
          # A developer image: Genkan has no first-boot setup for Macs yet.
          # Change the password after the first login (passwd).
          users.users.sekka = {
            isNormalUser = true;
            description = "Sekka";
            extraGroups = [
              "networkmanager"
              "wheel"
            ];
            initialPassword = "sekka";
          };
          nixpkgs.config.allowUnfree = true;
          system.stateVersion = "26.11";

          # The root image's store paths become the Nix database, and the
          # booted system the system profile, once, before anything uses Nix.
          systemd.services.sekka-image-first-boot = {
            description = "Register the image's Nix store";
            wantedBy = [ "sysinit.target" ];
            before = [
              "sysinit.target"
              "nix-daemon.socket"
              "nix-daemon.service"
            ];
            after = [ "local-fs.target" ];
            unitConfig = {
              DefaultDependencies = false;
              ConditionPathExists = "/nix-path-registration";
            };
            serviceConfig.Type = "oneshot";
            script = ''
              ${config.nix.package.out}/bin/nix-store --load-db < /nix-path-registration
              touch /etc/NIXOS
              ${config.nix.package.out}/bin/nix-env -p /nix/var/nix/profiles/system --set /run/current-system
              rm -f /nix-path-registration
            '';
          };
        }
      )
    ];
  };

  inherit (nixos.config.system.build) toplevel;

  rootImage = pkgs.callPackage "${nixpkgs}/nixos/lib/make-ext4-fs.nix" {
    storePaths = [ toplevel ];
    volumeLabel = rootLabel;
    uuid = "5ecca000-0000-4000-8000-000000000001";
  };

  # What NixOS's systemd-boot writes for generation 1, plus the firmware.
  esp = pkgs.runCommand "${name}-esp" { } ''
    mkdir -p $out/m1n1 $out/EFI/BOOT $out/EFI/systemd $out/EFI/nixos $out/loader/entries
    cp ${nixos.config.system.build.appleSiliconBootBin} $out/m1n1/boot.bin
    boot=${nixos.config.systemd.package}/lib/systemd/boot/efi/systemd-bootaa64.efi
    cp $boot $out/EFI/BOOT/BOOTAA64.EFI
    cp $boot $out/EFI/systemd/systemd-bootaa64.efi
    cp ${toplevel}/kernel $out/EFI/nixos/kernel.efi
    cp ${toplevel}/initrd $out/EFI/nixos/initrd.efi
    printf 'timeout 3\ndefault nixos-generation-1.conf\nconsole-mode keep\neditor no\n' \
      > $out/loader/loader.conf
    {
      echo "title Sekka"
      echo "version Generation 1 (image ${revision})"
      echo "linux /EFI/nixos/kernel.efi"
      echo "initrd /vendorfw/firmware.cpio"
      echo "initrd /EFI/nixos/initrd.efi"
      echo "options init=${toplevel}/init $(cat ${toplevel}/kernel-params)"
    } > $out/loader/entries/nixos-generation-1.conf
  '';

  package = pkgs.runCommand "${name}.zip" { nativeBuildInputs = [ pkgs.zip ]; } ''
    size=$(stat -c %s ${rootImage})
    if (( size % 4096 )); then
      echo "root.img is $size bytes, not a multiple of 4 KiB" >&2
      exit 1
    fi
    mkdir work && cd work
    cp -r ${esp} esp
    chmod -R u+w esp
    ln -s ${rootImage} root.img
    # Stored, not compressed: the package is read from this disk.
    zip -0 -r -X -q $out esp root.img
  '';

  installerData = pkgs.writeText "installer_data.json" (
    builtins.toJSON {
      os_list = [
        {
          name = "Sekka (14-inch MacBook Pro M3 Pro, developer image ${revision})";
          default_os_name = "Sekka";
          boot_object = "m1n1.bin";
          next_object = "m1n1/boot.bin";
          package = "${name}.zip";
          # The macOS 14.8.3 OS firmware Asahi gives M3 Macs, which the GPU and
          # display drivers were tested with.
          supported_fw = [ "14.8.3" ];
          partitions = [
            {
              name = "EFI";
              type = "EFI";
              size = "${toString espSize}B";
              format = "fat";
              volume_id = espVolumeId;
              copy_firmware = true;
              copy_installer_data = true;
              source = "esp";
            }
            {
              name = "Root";
              type = "Linux";
              size = "@ROOT_SIZE@B";
              expand = true;
              image = "root.img";
            }
          ];
        }
      ];
    }
  );
in
{
  inherit nixos toplevel esp;
  # The directory to run install-sekka.sh from: installer_data.json, the OS
  # package under os/ (the installer's REPO_BASE layout) and the bootstrap.
  bundle = pkgs.runCommand "${name}-mac-installer" { } ''
    mkdir -p $out/os
    ln -s ${package} $out/os/${name}.zip
    substitute ${installerData} $out/installer_data.json \
      --replace-fail @ROOT_SIZE@ "$(stat -c %s ${rootImage})"
    install -m755 ${./install-sekka.sh} $out/install-sekka.sh
  '';
}
