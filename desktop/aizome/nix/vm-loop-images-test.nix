# Real UDisks2, kernel loop devices and mounts, only on disposable VM files.
{ self, pkgs }:
let
  fixture = import ./loop-images-fixture.nix { inherit self pkgs; };
in
pkgs.testers.runNixOSTest {
  name = "aizome-loop-images";
  globalTimeout = 240;
  nodes.machine = {
    services.udisks2.enable = true;
    environment.etc."aizome-loop-test-vm".text = "disposable-loop-test\n";
    environment.systemPackages = [ fixture pkgs.e2fsprogs ];
    virtualisation.memorySize = 2048;
  };
  testScript = ''
    machine.start()
    machine.succeed("systemctl start udisks2.service")
    machine.wait_for_unit("udisks2.service")
    try:
        machine.succeed("aizome-loop-images-fixture --ignored --test-threads=1 --nocapture > /tmp/loop-images.log 2>&1")
        assert machine.succeed("grep -c 'PASS ' /tmp/loop-images.log").strip() == "4"
        machine.succeed("test -z \"$(losetup --list --noheadings)\"")
        machine.succeed("test -z \"$(systemctl --failed --no-legend)\"")
    finally:
        machine.succeed("journalctl -u udisks2 --no-pager > /tmp/udisks-journal.log")
        machine.succeed("lsblk -J -o NAME,TYPE,RO,MOUNTPOINTS > /tmp/loop-lsblk.json")
        machine.copy_from_machine("/tmp/loop-images.log")
        machine.copy_from_machine("/tmp/udisks-journal.log")
        machine.copy_from_machine("/tmp/loop-lsblk.json")
  '';
}
