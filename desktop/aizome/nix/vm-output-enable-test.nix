# Turns a display off and on in a real session on the udev backend, then
# makes turning it on fail and checks it is still off and listed, and comes
# on when asked again.
#
# No VM can make a modeset itself fail. Setting an output up while another
# VT has the seat fails earlier (the session's DRM device is paused), and
# leaves the backend's lists in the same state, which is what this checks.
# It says nothing about a modeset a driver or a monitor refuses.
#
#     nix build .#aizome-checks.output-enable -L
#     nix build .#aizome-checks.output-enable.driverInteractive && ./result/bin/nixos-test-driver
{ pkgs, self }:
pkgs.testers.runNixOSTest {
  name = "aizome-output-enable";

  nodes.machine =
    { pkgs, ... }:
    {
      imports = [ self.nixosModules.default ];
      programs.aizome.enable = true;

      users.users.alice = {
        isNormalUser = true;
        password = "aizome";
        extraGroups = [
          "wheel"
          "video"
        ];
        uid = 1000;
      };

      services.greetd = {
        enable = true;
        settings.initial_session = {
          user = "alice";
          command = "/run/current-system/sw/bin/aizome-session";
        };
        settings.default_session.command = "${pkgs.greetd}/bin/agreety --cmd /run/current-system/sw/bin/aizome-session";
      };

      virtualisation = {
        memorySize = 4096;
        cores = 4;
        resolution = {
          x = 1440;
          y = 900;
        };
        # Two outputs on one virtio GPU, the second forced on as in
        # vm-settings-test.nix.
        qemu.options = [
          "-vga none"
          "-device virtio-gpu-pci,max_outputs=2,xres=1440,yres=900"
        ];
      };
      boot.kernelParams = [ "video=Virtual-2:1920x1080e" ];
    };

  enableOCR = false;

  testScript = ''
    import json
    import time

    def until(what, f, timeout=60):
        deadline = time.monotonic() + timeout
        while not f():
            if time.monotonic() > deadline:
                raise Exception(f"timed out waiting for {what}")
            time.sleep(0.5)

    def alice(command):
        return (
            "su - alice -c 'export XDG_RUNTIME_DIR=/run/user/1000; "
            "export AIZOME_SOCKET=$(ls /run/user/1000/aizome.wayland-*.sock); "
            + command + "'"
        )

    def outputs():
        listed = json.loads(machine.succeed(alice("aizomectl --json outputs")))["outputs"]
        return {o["name"]: o for o in listed}

    def on(name):
        o = outputs().get(name)
        return o is not None and o["enabled"] and o["powered"]

    def logged(line):
        return int(machine.succeed(f"journalctl -b --no-pager _SYSTEMD_USER_UNIT=aizome.service | grep -c '{line}' || true"))

    machine.wait_for_unit("multi-user.target")
    machine.wait_until_succeeds("ls /run/user/1000/ | grep -q '^aizome\\..*\\.sock$'", timeout=120)
    machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=60)
    until("both displays on", lambda: on("Virtual-1") and on("Virtual-2"))

    with subtest("a display turned off stays listed and comes back on"):
        machine.succeed(alice("aizomectl output Virtual-2 --off"))
        assert outputs()["Virtual-2"]["enabled"] is False, outputs()
        machine.succeed(alice("aizomectl output Virtual-2 --on"))
        until("Virtual-2 on again", lambda: on("Virtual-2"))
        machine.screenshot("01-on-again")

    with subtest("one that fails to come on is still off and listed, and comes on when asked again"):
        machine.succeed(alice("aizomectl output Virtual-2 --off"))
        vt = machine.succeed("cat /sys/class/tty/tty0/active").strip().removeprefix("tty")
        paused, resumed = logged("session paused"), logged("session resumed")
        machine.succeed("chvt 3")
        until("the session to be paused", lambda: logged("session paused") > paused)
        said = machine.fail(alice("aizomectl output Virtual-2 --on 2>&1"))
        assert "didn't come on" in said, said
        machine.succeed(f"chvt {vt}")
        until("the session to be resumed", lambda: logged("session resumed") > resumed)

        listed = outputs()
        assert "Virtual-2" in listed, f"Virtual-2 is in no list after failing to come on: {sorted(listed)}"
        assert listed["Virtual-2"]["enabled"] is False, listed["Virtual-2"]
        machine.succeed(alice("aizomectl output Virtual-2 --on"))
        until("Virtual-2 on after all", lambda: on("Virtual-2"))
        assert on("Virtual-1"), outputs()["Virtual-1"]
        machine.screenshot("02-on-after-a-failed-attempt")
  '';
}
