# Displays at the offered larger-text scales in a real Tatami session.
# Component GUI proof; this is not qualification of a release ISO.
{ pkgs, desktopsModule }:
pkgs.testers.runNixOSTest {
  name = "tatami-displays-fit";
  globalTimeout = 600;
  node.pkgsReadOnly = false;
  nodes.machine =
    { ... }:
    {
      imports = [ desktopsModule ];
      calamares.desktops = [ "tatami" ];
      calamares.defaultDesktop = "tatami";
      users.users.demo = {
        isNormalUser = true;
        uid = 1000;
        password = "demo";
        extraGroups = [ "wheel" ];
      };
      services.displayManager.autoLogin = {
        enable = true;
        user = "demo";
      };
      virtualisation = {
        memorySize = 4096;
        cores = 4;
        qemu.options = [
          "-vga none"
          "-device virtio-gpu-pci,xres=1280,yres=800"
        ];
      };
      hardware.graphics.enable = true;
      programs.ydotool.enable = true;
      environment.sessionVariables.LIBGL_ALWAYS_SOFTWARE = "1";
    };
  testScript = ''
    import json, shlex, time

    def user(command):
        return machine.succeed("runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus "
            f"HYPRLAND_INSTANCE_SIGNATURE={signature} sh -c " + shlex.quote(command))

    def window():
        return next((c for c in json.loads(user("hyprctl -j clients"))
                     if c["class"] == "org.tatami.Displays"), None)

    def monitor():
        return next(m for m in json.loads(user("hyprctl -j monitors")) if m["focused"])

    def wait_for(predicate, seconds=20):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if predicate():
                return
            time.sleep(0.2)
        machine.screenshot("failure")
        raise AssertionError(json.dumps(dict(window=window(), monitor=monitor())))

    def fits():
        c, m = window(), monitor()
        if not c:
            return False
        x, y = c["at"]
        w, h = c["size"]
        left, top, right, bottom = m["reserved"]
        return (m["x"] + left <= x and m["y"] + top <= y
                and x + w <= m["x"] + m["width"] / m["scale"] - right
                and y + h <= m["y"] + m["height"] / m["scale"] - bottom)

    def click(x, y):
        user("hyprctl dispatch " + shlex.quote(f"hl.dsp.cursor.move({{ x = {x}, y = {y} }})"))
        machine.succeed("YDOTOOL_SOCKET=/run/ydotoold/socket ydotool mousemove -x 1 -y 0")
        time.sleep(0.3)
        machine.succeed("YDOTOOL_SOCKET=/run/ydotoold/socket ydotool click 0xC0")

    machine.start()
    machine.wait_for_unit("ydotoold.service")
    machine.wait_until_succeeds("test -S /run/user/1000/hypr/*/.socket.sock", timeout=180)
    signature = machine.succeed("basename /run/user/1000/hypr/*").strip()
    machine.wait_until_succeeds("runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
        "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus "
        "systemctl --user is-active tatami-waybar.service", timeout=120)
    machine.send_key("meta_l-ctrl-d")
    wait_for(lambda: window() is not None)
    time.sleep(3)
    machine.screenshot("scale1")
    print(json.dumps(dict(window=window(), monitor=monitor())))
    normal_fits = fits()
    # The second offered looks-like choice at 1280x800 is 640x400 (scale2).
    x, y = window()["at"]
    click(x + 304, y + 596)
    wait_for(lambda: monitor()["scale"] == 2)
    wait_for(fits)
    assert normal_fits, "scale1 exceeded the workspace"
    time.sleep(2)
    machine.screenshot("scale2")
    c = window()
    click(c["at"][0] + c["size"][0] - 24, c["at"][1] + 24)
    wait_for(lambda: window() is None)
    machine.send_key("meta_l-ctrl-d")
    wait_for(fits)
    time.sleep(2)
    machine.screenshot("scale2-reopened")

    def scroll(down, sideways=0):
        c = window()
        x, y = c["at"][0] + c["size"][0] - 50, c["at"][1] + c["size"][1] - 60
        user("hyprctl dispatch " + shlex.quote(f"hl.dsp.cursor.move({{ x = {x}, y = {y} }})"))
        machine.succeed("YDOTOOL_SOCKET=/run/ydotoold/socket ydotool mousemove -x 1 -y 0")
        machine.succeed(f"YDOTOOL_SOCKET=/run/ydotoold/socket ydotool mousemove --wheel -x {sideways} -y {down}")
        time.sleep(2)

    scroll(-40)
    machine.screenshot("scale2-bottom-controls")
    scroll(40)
    scroll(-6)
    machine.screenshot("scale2-scale-controls")
    # The rightmost visible scale choice, reached by scrolling, restores 1x.
    click(536, 360)
    wait_for(lambda: monitor()["scale"] == 1)
    wait_for(fits)
    time.sleep(2)
    machine.screenshot("scale1-restored")
    # The remaining offered scales, through the shipped scaling shortcut.
    for scale in [1.25, 1.6, 2, 3.2]:
        machine.send_key("meta_l-slash")
        wait_for(lambda: abs(monitor()["scale"] - scale) < 0.001)
        wait_for(fits)
    time.sleep(2)
    machine.screenshot("largest-scale")
    scroll(-40, -40)
    machine.screenshot("largest-scale-bottom-controls")
    assert fits()
    for scale in [2, 1.6, 1.25, 1]:
        machine.send_key("meta_l-alt-slash")
        wait_for(lambda: abs(monitor()["scale"] - scale) < 0.001)
        wait_for(fits)
    machine.send_key("esc")
    wait_for(lambda: window() is None)
    machine.send_key("meta_l-ctrl-d")
    wait_for(fits)
    time.sleep(2)
    machine.screenshot("scale1-reopened")
  '';
}
