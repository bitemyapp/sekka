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
    environment.systemPackages = [ tests ];
  };
  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")
    print(machine.succeed("storage-tests filesystem::tests::real_reformat_mount_matrix --ignored --nocapture"))
  '';
}
