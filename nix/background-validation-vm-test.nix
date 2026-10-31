# Normal Tatami menu, decoder, notifications and wallpaper renderer.
# Component GUI evidence, not qualification of an ISO.
{ pkgs, desktopsModule }:
let
  fixtures =
    pkgs.runCommand "wallpaper-validation-fixtures"
      {
        nativeBuildInputs = [ (pkgs.python3.withPackages (p: [ p.pillow ])) ];
      }
      ''
        mkdir -p $out
        python3 - "$out" <<'PY'
        from pathlib import Path
        from PIL import Image
        import sys
        out = Path(sys.argv[1])
        Image.new("RGB", (64, 48), (180, 40, 40)).save(out / "before.png")
        Image.new("RGB", (64, 48), (40, 180, 40)).save(out / "valid.jpg")
        (out / "text.png").write_text("This is readable text, not an image.")
        (out / "truncated.png").write_bytes((out / "before.png").read_bytes()[:40])
        PY
      '';
in
pkgs.testers.runNixOSTest {
  name = "tatami-background-validation";
  enableOCR = true;
  extraPythonPackages = p: [ p.pillow ];
  globalTimeout = 300;
  node.pkgsReadOnly = false;
  nodes.machine = {
    imports = [ desktopsModule ];
    calamares.desktops = [ "tatami" ];
    calamares.defaultDesktop = "tatami";
    users.users.demo = {
      isNormalUser = true;
      uid = 1000;
      password = "demo";
    };
    services.displayManager.autoLogin = {
      enable = true;
      user = "demo";
    };
    virtualisation = {
      memorySize = 4096;
      cores = 4;
      qemu.options = [ "-vga none -device virtio-gpu-pci,xres=1280,yres=800" ];
    };
    hardware.graphics.enable = true;
    environment.sessionVariables.LIBGL_ALWAYS_SOFTWARE = "1";
    environment.etc."background-fixtures".source = fixtures;
  };
  testScript = ''
    import json, shlex, time
    from datetime import timedelta
    from PIL import Image
    from typing import Any

    machine.start()
    machine.wait_for_unit("multi-user.target")
    machine.wait_until_succeeds("test -S /run/user/1000/hypr/*/.socket.sock", timeout=180)
    signature = machine.succeed("basename /run/user/1000/hypr/*").strip()
    def user(command):
        return machine.succeed("runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus "
            f"HYPRLAND_INSTANCE_SIGNATURE={signature} sh -c " + shlex.quote(command))
    def wait(predicate, seconds=20):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if predicate():
                return
            time.sleep(0.2)
        machine.screenshot("failure")
        print(user("journalctl --user -b -u tatami-swaybg --no-pager; makoctl list; hyprctl -j layers"))
        raise AssertionError("condition did not become true")
    def keys(*sequence):
        for key in sequence:
            machine.send_key(key)
            time.sleep(0.3)
    def park_pointer():
        # Real tablet input keeps the pointer below the menu and wallpaper
        # sample even when the compositor leaves it visible after typing.
        event: Any = {"events": [
            {"type": "abs", "data": {"axis": "x", "value": 0}},
            {"type": "abs", "data": {"axis": "y", "value": 32767}},
        ]}
        assert machine.qmp_client is not None
        machine.qmp_client.send("input-send-event", event)
        time.sleep(0.3)
    def menu_open():
        return '"namespace": "walker"' in json.dumps(json.loads(user("hyprctl -j layers")))
    def text(expected):
        try:
            machine.wait_for_text(expected, timeout=timedelta(seconds=20))
        except Exception:
            machine.screenshot("menu-failure")
            raise
    def panel(prompt):
        # The dmenu client waits for the real user's selection. Its prompt and
        # mapped layer provide readiness without OCR consuming a notice's 5s
        # lifetime after submission. No selections are supplied through IPC.
        def ready():
            commands = user("ps -ww -u demo -o args=").splitlines()
            return any("walker" in line and "--dmenu" in line
                       and "-p " + prompt + "…" in line for line in commands) and menu_open()
        wait(ready)
    def compositor():
        # Nix's wrapped compositor has this (15-byte) kernel comm name.
        pid = user("pgrep -u demo -x '[.]Hyprland-wrapp'").strip()
        assert pid.isdigit(), pid
        return pid, user("cat /proc/" + pid + "/stat").split()[21]
    def state():
        return user("tatami background status; readlink ~/.local/state/tatami/background; "
                    "systemctl --user show -p MainPID --value tatami-swaybg")
    def choose_path(path, output=False):
        keys("meta_l-spc")
        wait(menu_open)
        park_pointer()
        text("Style")
        keys("down", "down", "down", "ret")
        # This narrow menu ellipsizes "Background settings"; Hyprland is the
        # fully visible last entry in the same submenu.
        text("Hyprland")
        keys("down", "ret")
        panel("Wallpaper settings")
        if output:
            keys("down")
        keys("ret")
        panel("Virtual-1" if output else "All displays — defaults")
        keys("down", "down", "ret")
        panel("Wallpaper")
        keys("up", "ret")
        panel("Image file (absolute path)")
        machine.send_chars(path)
        keys("ret")
        panel("Virtual-1" if output else "All displays — defaults")
        keys("esc")
        wait(lambda: not menu_open())
    def screen(name):
        machine.screenshot(name)
        return Image.open(machine.out_dir / (name + ".png")).convert("RGB")

    wait(lambda: user("systemctl --user is-active tatami-waybar || true").strip() == "active")
    wait(lambda: user("systemctl --user is-active tatami-swaybg || true").strip() == "active")
    # Discovery metadata can lag the socket and services at startup. Preserve
    # its raw output, but use the real process identity for continuity.
    print("Startup instances discovery:", repr(user("hyprctl -j instances")))
    compositor_identity = compositor()
    user("tatami background set /etc/background-fixtures/before.png")
    time.sleep(1)
    before = state()
    park_pointer()
    # Keep the comparison away from the parked pointer, the bar and the
    # top-right notification.
    wallpaper_region = (100, 250, 600, 650)
    pixels = screen("before").crop(wallpaper_region)
    assert pixels.getpixel((100, 100)) == (180, 40, 40)
    for name, output in [("text.png", False), ("truncated.png", True)]:
        with subtest("normal menu rejects " + name + " without changing the wallpaper"):
            choose_path("/etc/background-fixtures/" + name, output=output)
            wait(lambda: "Wallpaper settings were not applied" in user("makoctl list"))
            shown = screen("rejected-" + name.replace(".", "-"))
            assert before == state(), "rejected input changed settings, symlink or renderer PID"
            assert pixels.tobytes() == shown.crop(wallpaper_region).tobytes()
            keys("meta_l-comma")
    with subtest("normal menu recovers by choosing a valid image"):
        choose_path("/etc/background-fixtures/valid.jpg")
        wait(lambda: "/etc/background-fixtures/valid.jpg" in user("readlink ~/.local/state/tatami/background"))
        time.sleep(1)
        shown = screen("valid-recovery")
        red, green, blue = shown.getpixel((300, 500))
        assert green > 160 and red < 60 and blue < 60
        assert "Wallpaper settings were not applied" not in user("makoctl list")
        assert before != state()
    assert compositor() == compositor_identity
    print(user("tatami background status; journalctl --user -b -u tatami-swaybg --no-pager"))
  '';
}
