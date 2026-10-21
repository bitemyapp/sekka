# Production storage code + generated NixOS storage configuration in disposable VMs.
# This is a component test: the harness supplies the bootloader and Nix store.
{ pkgs }:
let
  tests = import ./storage-tests.nix { inherit pkgs; };
  node = firmware: { lib, ... }: {
    options.calamares.zswap.enable = lib.mkEnableOption "fixture zswap selection";
    config = {
      environment.etc."genkan-storage-test".text = "disposable VM\n";
      environment.systemPackages = [
        tests
        pkgs.cryptsetup
        pkgs.parted
      ];
      boot.supportedFilesystems = [
        "ext4"
        "btrfs"
        "xfs"
        "vfat"
      ];
      boot.kernelModules = [ "dm-crypt" ];
      boot.kernelParams = lib.mkOverride 5 [ "console=tty1" ];
      boot.loader.systemd-boot.enable = firmware == "uefi";
      boot.loader.grub.enable = firmware == "bios";
      virtualisation = {
        memorySize = 4096;
        cores = 2;
        useBootLoader = true;
        useEFIBoot = firmware == "uefi";
        mountHostNixStore = true;
        emptyDiskImages = [
          {
            size = 32768;
            driveConfig = {
              deviceExtraOpts.serial = "GENKAN_LUKS_TEST";
              driveExtraOpts.discard = "unmap";
            };
          }
        ];
      };
      specialisation.encrypted.configuration = {
        imports = [ (../rust/src/test-data + "/${firmware}-ext4.nix") ];
        virtualisation.useDefaultFilesystems = false;
        virtualisation.fileSystems."/" =
          (import (../rust/src/test-data + "/${firmware}-ext4.nix") { inherit lib; }).fileSystems."/";
        # /boot and the store are supplied by the harness; root and swap use
        # the production-generated UUIDs and initrd unlock configuration.
        virtualisation.rootDevice = "/dev/disk/by-uuid/11111111-2222-4333-8444-555555555555";
      };
    };
  };
in
pkgs.testers.runNixOSTest {
  name = "genkan-encrypted-storage";
  nodes = {
    uefi = node "uefi";
    bios = node "bios";
  };
  enableOCR = true;
  testScript = { nodes, ... }: ''
    for machine, firmware, system, fstab in [
        (bios, "bios", "${nodes.bios.specialisation.encrypted.configuration.system.build.toplevel}", "${nodes.bios.specialisation.encrypted.configuration.environment.etc.fstab.source}"),
        (uefi, "uefi", "${nodes.uefi.specialisation.encrypted.configuration.system.build.toplevel}", "${nodes.uefi.specialisation.encrypted.configuration.environment.etc.fstab.source}"),
    ]:
        machine.start()
        machine.wait_for_unit("multi-user.target")
        assert int(machine.succeed("lsblk -bdno DISC-GRAN /dev/vdb").strip()) > 0
        print(machine.succeed(f"GENKAN_TEST_FIRMWARE={firmware} storage-tests install::encryption_tests::encrypted_storage_matrix --ignored --nocapture", timeout=1200))
        # A real installation's activation writes fstab. The storage-only
        # fixture needs it too: systemd reloads root's fstab before switch-root.
        machine.succeed("printf %s Genkan-test-only-123 | cryptsetup open --key-file - /dev/vdb2 fixture-root")
        machine.succeed("mkdir -p /mnt; mount /dev/mapper/fixture-root /mnt; mkdir -p /mnt/etc")
        machine.succeed(f"cp {fstab} /mnt/etc/fstab")
        machine.succeed("umount /mnt; cryptsetup close fixture-root")
        machine.succeed(f"{system}/bin/switch-to-configuration boot")
        machine.succeed("sync")
        machine.crash()
        machine.start()
        machine.wait_for_text("[Pp]assphrase for", timeout=180)
        machine.send_chars("Genkan-test-only-123\n")
        machine.wait_for_unit("multi-user.target", timeout=180)
        machine.succeed("test $(findmnt -n -o UUID /) = 11111111-2222-4333-8444-555555555555")
        assert machine.succeed("cat /encrypted-sentinel").strip() == "written by Genkan"
        machine.succeed("cryptsetup status cryptroot", "cryptsetup status cryptswap")
        assert int(machine.succeed("lsblk -bdno DISC-GRAN /dev/vdb").strip()) > 0
        assert int(machine.succeed("lsblk -bdno DISC-GRAN /dev/mapper/cryptroot").strip()) > 0
        print(machine.succeed("fstrim -v /"))
        machine.succeed("swapon --show=NAME --noheadings | grep -q /dev/dm-")
        machine.shutdown()
  '';
}
