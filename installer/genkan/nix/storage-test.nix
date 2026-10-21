{ pkgs }:
let
  tests = import ./storage-tests.nix { inherit pkgs; };
in
pkgs.testers.runNixOSTest {
  name = "calamares-filesystem-reformat";
  # Also runs inside the rootless ISO builder without nested KVM.
  requiredFeatures.kvm = false;
  nodes.machine = {
    boot.supportedFilesystems = [
      "ext4"
      "btrfs"
      "xfs"
      "vfat"
    ];
    boot.kernelModules = [ "loop" ];
    virtualisation.memorySize = 2048;
    virtualisation.emptyDiskImages =
      map
        (logical: {
          size = 40960;
          driveConfig.deviceExtraOpts = {
            logical_block_size = toString logical;
            physical_block_size = "4096";
            serial = if logical == 4096 then "SEKKA_BIOS4KN_TEST" else "SEKKA_BIOS512E_TEST";
          };
        })
        [
          4096
          512
        ];
    environment.systemPackages = [ tests ];
  };
  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")
    print(machine.succeed("storage-tests disk::tests --nocapture"))
    print(machine.succeed("storage-tests disk::tests::real_bios_sector_discovery_and_revalidation --ignored --nocapture"))
    print(machine.succeed("storage-tests filesystem::tests::real_reformat_mount_matrix --ignored --nocapture"))
  '';
}
