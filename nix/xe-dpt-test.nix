# Sekka's rebuilt xe module (installer/genkan/rust/system/xe-dpt) in QEMU:
# modprobe finds it before the kernel's own copy, and the Nixpkgs kernel it
# was built for loads it. QEMU has no Intel GPU, so this checks the module's
# build and fit, not page flips. The check fails once Nixpkgs' latest kernel
# is one the module isn't built for, so the patch is looked at again then.
{ pkgs }:
let
  kernel = pkgs.linuxPackages_latest.kernel;
  stock = "${kernel.modules}/lib/modules/${kernel.modDirVersion}/kernel/drivers/gpu/drm/xe/xe.ko.xz";
in
pkgs.testers.runNixOSTest {
  name = "xe-dpt";
  nodes.machine = {
    imports = [ ../installer/genkan/rust/system/xe-dpt ];
    boot.kernelPackages = pkgs.linuxPackages_latest;
    # Real Intel machines may load xe before the root filesystem is mounted.
    boot.initrd.kernelModules = [ "xe" ];
    virtualisation.memorySize = 1024;
  };
  testScript = ''
    machine.wait_for_unit("multi-user.target")
    machine.succeed("grep -q '^xe ' /proc/modules")
    taint = machine.succeed("cat /sys/module/xe/taint").strip()
    assert "O" in taint, f"initrd did not load the replacement xe: {taint!r}"

    path = machine.succeed("modinfo -n xe").strip()
    assert "/updates/xe.ko" in path, f"modprobe resolves xe to {path}"

    def field(name, module):
        return machine.succeed(f"modinfo -F {name} {module}").strip()

    ours, theirs = field("vermagic", path), field("vermagic", "${stock}")
    assert ours == theirs, f"vermagic {ours!r}, the kernel's own {theirs!r}"
    ours, theirs = (set(field("depends", m).split(",")) for m in [path, "${stock}"])
    assert ours == theirs, f"depends {ours}, the kernel's own {theirs}"

    machine.succeed("modprobe xe")
    machine.succeed("grep -q '^xe ' /proc/modules")
    # Out of tree and unsigned, as expected; nothing else.
    taint = machine.succeed("cat /sys/module/xe/taint").strip()
    assert set(taint) <= set("OE"), f"xe taints the kernel with {taint!r}"
    machine.fail("dmesg | grep -E 'xe: (disagrees|Unknown symbol|version magic|.*invalid)'")
  '';
}
