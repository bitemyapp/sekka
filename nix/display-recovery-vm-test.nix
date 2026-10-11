{
  pkgs,
  hyprland,
  tatami,
  displays,
}:
pkgs.testers.runNixOSTest {
  name = "tatami-display-recovery";
  globalTimeout = 180;
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
      pkgs.grim
      pkgs.jq
    ];
    environment.etc."hypr-test.lua".text = ''
      hl.monitor({ output = "", mode = "1280x720@60", position = "auto", scale = 1 })
      pcall(dofile, os.getenv("HOME") .. "/.local/state/tatami/displays.lua")
      pcall(dofile, os.getenv("XDG_RUNTIME_DIR") .. "/tatami/monitors.lua")
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
    try:
        machine.wait_until_succeeds("test -S /run/user/1000/hypr/*/.socket.sock", timeout=timedelta(seconds=45))
    except Exception:
        print(machine.succeed("cat /tmp/hyprland.log"))
        raise
    signature = machine.succeed("basename /run/user/1000/hypr/*").strip()
    def run(command):
        return machine.succeed("su - alice -c " + shlex.quote(
            "export XDG_RUNTIME_DIR=/run/user/1000 HYPRLAND_INSTANCE_SIGNATURE=" + signature + "; " + command))
    def monitors():
        return json.loads(run("hyprctl -j monitors all"))
    def display(name):
        return next(m for m in monitors() if m["name"] == name)
    def wait_for(predicate):
        until = time.monotonic() + 10
        while time.monotonic() < until:
            if predicate():
                return
            time.sleep(0.1)
        raise AssertionError(monitors())
    def evaluate(lua):
        assert run("hyprctl eval " + shlex.quote(lua)).strip() == "ok"
    def apply(enabled=True, mirror=None, main="eDP-1"):
        panel = dict(name="eDP-1", enabled=enabled, mode="1280x720@60", scale=1)
        if mirror:
            panel["mirror"] = mirror
        change = dict(displays=[panel, dict(name="DP-1", enabled=True)],
                      main=main, apply=["eDP-1"])
        run("printf %s " + shlex.quote(json.dumps(change)) + " | tatami displays apply")
    run("hyprctl output create headless eDP-1")
    run("hyprctl output create headless DP-1")
    wait_for(lambda: len(monitors()) == 3)
    # Leave only the simulated laptop/dock outputs enabled, so undocking
    # really goes through Hyprland's no-output/FALLBACK transition.
    evaluate('hl.monitor({ output="Virtual-1", disabled=true })')
    wait_for(lambda: display("Virtual-1")["disabled"])
    compositor_pid = json.loads(run("hyprctl -j instances"))[0]["pid"]
    # Reproduce the exact missing-field bug against the actual Lua API.
    evaluate('hl.monitor({ output="eDP-1", disabled=true })')
    wait_for(lambda: display("eDP-1")["disabled"])
    evaluate('hl.monitor({ output="eDP-1", mode="1280x720@60" })')
    assert display("eDP-1")["disabled"]
    # All three actions from Displays: main, extended, mirrored.
    apply()
    wait_for(lambda: not display("eDP-1")["disabled"])
    assert json.loads(run("cat ~/.local/state/tatami/displays.json"))["main"] == "eDP-1"
    apply(False, main="DP-1")
    wait_for(lambda: display("eDP-1")["disabled"])
    apply(mirror="DP-1", main="DP-1")
    wait_for(lambda: not display("eDP-1")["disabled"] and display("eDP-1")["mirrorOf"] == str(display("DP-1")["id"]))
    apply(main="DP-1")
    wait_for(lambda: display("eDP-1")["mirrorOf"] == "none")
    # The watcher must recover without the settings app, then keep the panel
    # on after a compositor configuration reload. Repeat the undock cycle.
    run("systemd-run --user --unit=display-recovery --setenv=XDG_RUNTIME_DIR=/run/user/1000 "
        "--setenv=HYPRLAND_INSTANCE_SIGNATURE=" + signature + " tatami displays watch")
    for _ in range(2):
        apply(False, main="DP-1")
        wait_for(lambda: display("eDP-1")["disabled"])
        run("hyprctl output remove DP-1")
        wait_for(lambda: not display("eDP-1")["disabled"])
        assert display("eDP-1")["mirrorOf"] == "none"
        run("hyprctl reload")
        wait_for(lambda: not display("eDP-1")["disabled"])
        run("hyprctl output create headless DP-1")
        wait_for(lambda: any(m["name"] == "DP-1" for m in monitors()))
    assert json.loads(run("hyprctl -j instances"))[0]["pid"] == compositor_pid
    assert run("hyprctl configerrors").strip() == ""
    socket = json.loads(run("hyprctl -j instances"))[0]["wl_socket"]
    run("systemd-run --user --unit=displays --setenv=XDG_RUNTIME_DIR=/run/user/1000 "
        "--setenv=HYPRLAND_INSTANCE_SIGNATURE=" + signature + " --setenv=WAYLAND_DISPLAY=" + socket
        + " --setenv=GSK_RENDERER=cairo tatami-displays")
    wait_for(lambda: any(c["class"] == "org.tatami.Displays" for c in json.loads(run("hyprctl -j clients"))))
    time.sleep(1)
    run("WAYLAND_DISPLAY=" + socket + " grim -o eDP-1 /tmp/display-recovered.png")
    machine.copy_from_machine("/tmp/display-recovered.png")
    run("systemctl --user stop displays")
    run("systemctl --user stop display-recovery")
  '';
}
