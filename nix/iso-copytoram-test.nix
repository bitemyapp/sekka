# Exercise the actual initrd helper against disposable loop-mounted filesystems.
{ pkgs }:
let
  helper = import ./iso-copytoram.nix { inherit pkgs; };
in
pkgs.testers.runNixOSTest {
  name = "sekka-iso-copytoram";
  nodes.machine = { ... }: {
    virtualisation.memorySize = 6144;
    virtualisation.cores = 2;
    environment.systemPackages = [
      helper
      pkgs.e2fsprogs
    ];
  };
  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")
    machine.succeed("mkdir -p /sysroot/iso")

    def media(size):
        machine.succeed(f"truncate -s {size} /tmp/media.img")
        machine.succeed("mkfs.ext4 -F /tmp/media.img")
        machine.succeed("mount -o loop /tmp/media.img /sysroot/iso")
        machine.succeed("echo intact-media > /sysroot/iso/marker")

    with subtest("insufficient memory preserves the original mount and succeeds with a warning"):
        media("8G")
        result = machine.succeed("sekka-copy-iso-to-ram 2>&1")
        assert "insufficient RAM" in result, result
        machine.succeed("findmnt -n -o FSTYPE /sysroot/iso | grep -x ext4")
        machine.succeed("grep -x intact-media /sysroot/iso/marker")
        machine.succeed("umount /sysroot/iso")
        machine.succeed("rm /tmp/media.img")

    with subtest("failed copy never replaces or unmounts the original media"):
        media("32M")
        # Reading process memory at offset zero gives EIO, a real copy failure.
        machine.succeed("touch /sysroot/iso/unreadable")
        machine.succeed("mount --bind /proc/1/mem /sysroot/iso/unreadable")
        result = machine.succeed("sekka-copy-iso-to-ram 2>&1")
        assert "copy failed" in result, result
        machine.succeed("findmnt -n -o FSTYPE /sysroot/iso | grep -x ext4")
        machine.succeed("grep -x intact-media /sysroot/iso/marker")
        machine.succeed("umount /sysroot/iso/unreadable")
        machine.succeed("rm /sysroot/iso/unreadable")
        machine.succeed("test -z \"$(find /run -maxdepth 1 -name 'sekka-copytoram.*' -print -quit)\"")

    with subtest("complete copy switches to RAM and releases the original media"):
        result = machine.succeed("sekka-copy-iso-to-ram 2>&1")
        assert "complete;" in result, result
        machine.succeed("findmnt -n -o FSTYPE /sysroot/iso | grep -x tmpfs")
        machine.succeed("grep -x intact-media /sysroot/iso/marker")
        machine.succeed("test -z \"$(losetup -j /tmp/media.img)\"")
        machine.succeed("rm /tmp/media.img")
        machine.succeed("test -z \"$(find /run -maxdepth 1 -name 'sekka-copytoram.*' -print -quit)\"")
  '';
}
