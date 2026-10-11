{
  pkgs,
  hyprland,
  tatami,
  displays,
}:
let
  inherit (pkgs) lib;
  # Tatami's own lid bindings, so the test exercises the shipped lines.
  lidBindings = lib.concatStringsSep "\n" (
    lib.filter (line: lib.hasPrefix "local lid = " line || lib.hasPrefix "hl.bind(\"switch:" line) (
      lib.splitString "\n" (builtins.readFile ../desktop/tatami/config/hypr/hyprland.lua)
    )
  );
  # A lid switch both logind and Hyprland (libinput) see: a uinput device,
  # closed or opened through /run/fake-lid.
  fakeLid = pkgs.writers.writePython3 "fake-lid" { libraries = [ pkgs.python3Packages.evdev ]; } ''
    import os
    from evdev import UInput, ecodes

    lid = UInput({ecodes.EV_SW: [ecodes.SW_LID]}, name="Lid Switch")
    fifo = "/run/fake-lid"
    if not os.path.exists(fifo):
        os.mkfifo(fifo, 0o666)
    while True:
        with open(fifo) as requests:
            for line in requests:
                lid.write(ecodes.EV_SW, ecodes.SW_LID, line.strip() == "closed")
                lid.syn()
  '';
in
assert lib.hasInfix "switch:on:Lid Switch" lidBindings;
pkgs.testers.runNixOSTest {
  name = "tatami-display-recovery";
  globalTimeout = 480;
  nodes.machine = { ... }: {
    imports = [ (pkgs.path + "/nixos/tests/common/user-account.nix") ];
    virtualisation = {
      memorySize = 3072;
      cores = 2;
      qemu.options = [ "-vga none -device virtio-gpu-pci" ];
    };
    hardware.graphics.enable = true;
    hardware.uinput.enable = true;
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
      -- The VM's own output stays off, through reloads too: undocking must
      -- really go through Hyprland's no-output/FALLBACK transition.
      hl.monitor({ output = "Virtual-1", disabled = true })
      pcall(dofile, os.getenv("HOME") .. "/.local/state/tatami/displays.lua")
      pcall(dofile, os.getenv("XDG_RUNTIME_DIR") .. "/tatami/monitors.lua")
      hl.config({ misc = { focus_on_activate = true } })
      hl.window_rule({ match = { class = "org.tatami.Displays" }, float = true, center = true, size = { 880, 600 } })
      ${lidBindings}
    '';
    # The headless test outputs are not DRM connectors: logind would think a
    # closed lid undocked and suspend the machine. Report the lid only.
    services.logind.settings.Login = {
      HandleLidSwitch = "ignore";
      HandleLidSwitchDocked = "ignore";
      HandleLidSwitchExternalPower = "ignore";
    };
    systemd.services.fake-lid = {
      serviceConfig.ExecStart = fakeLid;
    };
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
    def wait_for(predicate, seconds=10):
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            if predicate():
                return
            time.sleep(0.1)
        raise AssertionError(json.dumps(monitors()) + "\n" + run("hyprctl -j clients"))
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
    wait_for(lambda: display("Virtual-1")["disabled"])
    compositor_pid = json.loads(run("hyprctl -j instances"))[0]["pid"]
    wayland = json.loads(run("hyprctl -j instances"))[0]["wl_socket"]
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
        "--setenv=PATH=/run/current-system/sw/bin "
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
        # Turned off with DP-1 connected: off again when it returns.
        wait_for(lambda: display("eDP-1")["disabled"] and not display("DP-1")["disabled"])
    apply(main="DP-1")
    wait_for(lambda: not display("eDP-1")["disabled"])

    # Closed-display mode, with a lid both logind and Hyprland see.
    machine.systemctl("start fake-lid.service")
    machine.wait_until_succeeds("test -p /run/fake-lid")
    wait_for(lambda: "Lid Switch" in run("hyprctl devices"))
    def lid(state):
        machine.succeed("echo " + state + " > /run/fake-lid")
    def logind_lid():
        return machine.succeed("busctl get-property org.freedesktop.login1 /org/freedesktop/login1 "
                               "org.freedesktop.login1.Manager LidClosed").strip()
    lid("closed")
    wait_for(lambda: display("eDP-1")["disabled"], 5)
    assert not display("DP-1")["disabled"]
    machine.wait_until_succeeds("busctl get-property org.freedesktop.login1 /org/freedesktop/login1 "
                                "org.freedesktop.login1.Manager LidClosed | grep -q true")
    assert run("tatami displays lid").strip() == "closed"
    # A reload keeps the panel off behind the closed lid.
    run("hyprctl reload")
    time.sleep(2)
    assert display("eDP-1")["disabled"]
    lid("open")
    wait_for(lambda: not display("eDP-1")["disabled"], 5)
    assert logind_lid() == "b false"
    # Undocked with the lid closed: the panel comes back, blanked behind the
    # lid, and wakes as the lid opens.
    lid("closed")
    wait_for(lambda: display("eDP-1")["disabled"], 5)
    run("hyprctl output remove DP-1")
    wait_for(lambda: not display("eDP-1")["disabled"] and display("eDP-1")["dpmsStatus"] is False)
    lid("open")
    wait_for(lambda: display("eDP-1")["dpmsStatus"] is True, 5)
    run("hyprctl output create headless DP-1")
    wait_for(lambda: any(m["name"] == "DP-1" and not m["disabled"] for m in monitors()))
    assert not display("eDP-1")["disabled"]

    # The Displays window, open on the external display when it is unplugged
    # with the panel off, stays reachable: Hyprland moves floating windows off
    # every screen in that transition, and the watcher brings them back.
    apply(False, main="DP-1")
    wait_for(lambda: display("eDP-1")["disabled"])
    run("hyprctl dispatch " + shlex.quote('hl.dsp.focus({ monitor = "DP-1" })'))
    def present():
        run("WAYLAND_DISPLAY=" + wayland + " GSK_RENDERER=cairo tatami display")
    def window():
        return next((c for c in json.loads(run("hyprctl -j clients")) if c["class"] == "org.tatami.Displays"), None)
    def on_own_screen():
        client = window()
        if client is None:
            return False
        screen = next((m for m in monitors() if m["id"] == client["monitor"] and not m["disabled"]), None)
        if screen is None:
            return False
        (x, y), (w, h) = client["at"], client["size"]
        return x < screen["x"] + screen["width"] and screen["x"] < x + w and y < screen["y"] + screen["height"] and screen["y"] < y + h
    present()
    wait_for(on_own_screen)
    run("hyprctl output remove DP-1")
    wait_for(lambda: not display("eDP-1")["disabled"])
    wait_for(on_own_screen)
    run("hyprctl output create headless DP-1")
    wait_for(lambda: display("eDP-1")["disabled"] and not display("DP-1")["disabled"])
    wait_for(on_own_screen)
    # Placed off screen by an application: left alone until the screens
    # change, then brought back.
    time.sleep(4)  # past the rescue that follows the redock above
    address = window()["address"]
    evaluate('hl.dispatch(hl.dsp.window.move({ window = "address:' + address + '", x = -3000, y = 60 }))')
    wait_for(lambda: not on_own_screen())
    time.sleep(3)
    assert not on_own_screen()
    run("hyprctl output create headless HEADLESS-9")
    wait_for(on_own_screen, 6)
    run("hyprctl output remove HEADLESS-9")
    wait_for(lambda: not any(m["name"] == "HEADLESS-9" for m in monitors()))
    # Asked for again from another workspace: brought here, in front.
    run("hyprctl dispatch " + shlex.quote('hl.dsp.focus({ workspace = "5" })'))
    present()
    def in_front():
        focused = next(m for m in monitors() if m["focused"])
        client = window()
        active = json.loads(run("hyprctl -j activewindow"))
        return (client is not None and on_own_screen()
                and client["workspace"]["id"] == focused["activeWorkspace"]["id"]
                and active.get("class") == "org.tatami.Displays")
    wait_for(in_front)
    # Exactly one window, from one responsive instance.
    assert len([c for c in json.loads(run("hyprctl -j clients")) if c["class"] == "org.tatami.Displays"]) == 1
    run("busctl --user --timeout=2 call org.tatami.Displays /org/tatami/Displays org.gtk.Actions List")
    time.sleep(1)
    run("WAYLAND_DISPLAY=" + wayland + " grim -o DP-1 /tmp/display-recovered.png")
    machine.copy_from_machine("/tmp/display-recovered.png")

    assert json.loads(run("hyprctl -j instances"))[0]["pid"] == compositor_pid
    assert run("hyprctl configerrors").strip() == ""
    print(run("journalctl --user -u display-recovery --no-pager"))
    run("systemctl --user stop display-recovery")
  '';
}
