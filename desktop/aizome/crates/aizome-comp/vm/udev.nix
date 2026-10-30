# The udev backend in a throwaway VM: virtio-gpu without 3D (so pixman,
# through the fallback), logind and libseat, libinput, KMS scan-out. It
# runs a development build shared in from the host, so it needs no Nix
# build of Aizome:
#
#   nix develop .#aizome --command cargo build -p aizome-comp -p aizome-ipc
#   export AIZOME_BIN=$CARGO_TARGET_DIR/debug        # holds aizome, aizomectl
#   export AIZOME_LD_LIBRARY_PATH=$(nix develop .#aizome --command sh -c 'echo $LD_LIBRARY_PATH')
#   nix build --impure -f crates/aizome-comp/vm/udev.nix driver -o vm-driver
#   env -u WAYLAND_DISPLAY -u DISPLAY ./vm-driver/bin/nixos-test-driver -o vm-out
#
# (The driver runs outside the build sandbox so the VM can see the build.)
# It logs alice in on tty1, starts the compositor there, opens foot with
# ❖Return through libinput, types into it, and keeps screenshots of the
# real scan-out and through IPC in vm-out/.
let
  flake = builtins.getFlake (toString ../../..);
  pkgs = flake.inputs.nixpkgs.legacyPackages.x86_64-linux;
  bin = builtins.getEnv "AIZOME_BIN";
  ldp = builtins.getEnv "AIZOME_LD_LIBRARY_PATH";
  run = pkgs.writeShellScript "run-aizome" ''
    export LD_LIBRARY_PATH=${ldp}
    export AIZOME_LOG=info,smithay=warn XDG_CONFIG_HOME=/tmp/azcfg
    # Only foot is installed here, so it's the default terminal (❖Return's).
    exec /mnt/aizome/aizome --backend udev --no-shell --no-xwayland > /tmp/az.log 2>&1
  '';
in
assert bin != "" && ldp != "";
pkgs.testers.runNixOSTest {
  name = "aizome-udev";
  skipLint = true;
  skipTypeCheck = true;
  nodes.machine =
    { pkgs, ... }:
    {
      users.users.alice = {
        isNormalUser = true;
        uid = 1000;
        extraGroups = [ "video" "input" ];
      };
      services.getty.autologinUser = "alice";
      hardware.graphics.enable = true;
      environment.systemPackages = [ pkgs.foot ];
      virtualisation = {
        memorySize = 2048;
        cores = 2;
        resolution = { x = 1280; y = 800; };
        qemu.options = [ "-vga none" "-device virtio-gpu-pci,xres=1280,yres=800" ];
        sharedDirectories.aizome = { source = bin; target = "/mnt/aizome"; };
      };
    };
  testScript = ''
    machine.wait_for_unit("multi-user.target")
    machine.wait_until_succeeds("pgrep -u alice -x bash", timeout=60)
    machine.sleep(2)
    # In alice's tty1 session, so libseat gets the seat through logind.
    machine.send_chars("${run} &\n")
    machine.wait_until_succeeds("ls /run/user/1000/ | grep -q '^aizome\\..*\\.sock$'", timeout=60)
    sock = machine.succeed("ls /run/user/1000/aizome.*.sock").strip()
    ctl = f"/mnt/aizome/aizomectl --socket {sock}"
    machine.log(machine.succeed(f"{ctl} outputs"))
    # A terminal from ❖Return, through libinput and the binding table.
    machine.send_key("meta_l-ret")
    machine.wait_until_succeeds(f"{ctl} windows | grep -q foot", timeout=30)
    machine.sleep(2)
    machine.screenshot("scanout")
    machine.succeed(f"{ctl} screenshot /tmp/ipc.png")
    machine.copy_from_machine("/tmp/ipc.png", "")
    # Typing reaches it.
    machine.send_chars("echo hello-from-aizome > /tmp/typed\n")
    machine.wait_until_succeeds("grep -q hello-from-aizome /tmp/typed", timeout=20)
    machine.log(machine.succeed(f"{ctl} stats"))
    machine.copy_from_machine("/tmp/az.log", "")
  '';
}
