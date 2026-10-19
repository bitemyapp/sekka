{
  pkgs,
  hyprland,
  tatami,
  displays,
}:
# The bar's display icon opens the Displays window without moving the
# cursor: focusing a window in Hyprland warps the cursor to its middle.
pkgs.testers.runNixOSTest {
  name = "tatami-displays-cursor";
  globalTimeout = 900;
  nodes.machine = { ... }: {
    imports = [ (pkgs.path + "/nixos/tests/common/user-account.nix") ];
    virtualisation = {
      memorySize = 3072;
      cores = 2;
      qemu.options = [ "-vga none -device virtio-gpu-pci" ];
    };
    hardware.graphics.enable = true;
    programs.hyprland = {
      enable = true;
      package = hyprland;
    };
    environment.systemPackages = [
      tatami
      displays
    ];
    environment.etc."hypr-test.lua".text = ''
      hl.monitor({ output = "", mode = "1280x720@60", position = "0x0", scale = 1 })
      hl.window_rule({ match = { class = "org.tatami.Displays" }, float = true, center = true, size = { 880, 600 } })
    '';
    services.getty.autologinUser = "alice";
    programs.bash.loginShellInit = ''
      if [ "$(tty)" = /dev/tty1 ]; then
        export LIBGL_ALWAYS_SOFTWARE=1
        exec Hyprland -c /etc/hypr-test.lua > /tmp/hyprland.log 2>&1
      fi
    '';
  };
  testScript = ''
    import json, shlex, time
    from datetime import timedelta
    machine.start()
    machine.wait_for_unit("multi-user.target")
    # Hyprland can fail its first start while the GPU comes up; the getty
    # starts it again, so use the instance that answers.
    def as_alice(command, signature="$(ls -t /run/user/1000/hypr | head -1)"):
        return "su - alice -c " + shlex.quote(
            "export XDG_RUNTIME_DIR=/run/user/1000 HYPRLAND_INSTANCE_SIGNATURE=" + signature + "; " + command)
    try:
        machine.wait_until_succeeds(as_alice("hyprctl -j activewindow"), timeout=timedelta(seconds=300))
    except Exception:
        print(machine.execute("cat /tmp/hyprland.log")[1])
        raise
    signature = machine.succeed("ls -t /run/user/1000/hypr | head -1").strip()
    def run(command):
        return machine.succeed(as_alice(command, signature))
    def hypr(what):
        reply = run("hyprctl -j " + what)
        try:
            return json.loads(reply)
        except ValueError:
            raise AssertionError("hyprctl -j " + what + ": " + repr(reply))
    def displays_focused():
        # Software rendering can keep the compositor from answering for a while.
        status, reply = machine.execute(as_alice("hyprctl -j activewindow", signature))
        return status == 0 and '"class": "org.tatami.Displays"' in reply
    def wait_for(predicate, seconds=180):
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            if predicate():
                return
            time.sleep(0.2)
        raise AssertionError(run("hyprctl -j clients") + run("hyprctl cursorpos") + run("cat /tmp/hyprland.log | tail -20"))
    def cursor():
        return run("hyprctl cursorpos").strip()
    wayland = machine.succeed("basename $(ls /run/user/1000/wayland-? | head -1)").strip()
    def display_icon():
        run("WAYLAND_DISPLAY=" + wayland + " GSK_RENDERER=cairo tatami display")
    def at_corner():
        # Where the display icon is: the bar's right end.
        run("hyprctl dispatch " + shlex.quote("hl.dsp.cursor.move({ x = 1250, y = 12 })"))
        wait_for(lambda: cursor() == "1250, 12")
    def check():
        wait_for(displays_focused)
        window = hypr("activewindow")
        print("Displays window at", window["at"], "cursor at", cursor())
        assert cursor() == "1250, 12", "the cursor moved to " + cursor()
        assert '"bool": false' in run("hyprctl -j getoption cursor:no_warps")
        return window
    at_corner()
    display_icon()
    window = check()
    # Again with the window open, moved aside: the helper must center it,
    # which shows its request reached the compositor.
    run("hyprctl dispatch " + shlex.quote('hl.dsp.window.move({ window = "address:' + window["address"] + '", x = 20, y = 40 })'))
    wait_for(lambda: hypr("activewindow")["at"] == [20, 40])
    at_corner()
    display_icon()
    wait_for(lambda: hypr("activewindow")["at"] == [200, 60])
    check()
  '';
}
