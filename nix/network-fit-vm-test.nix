# Network's header and settings stay reachable at larger text scales.
# A component VM, not qualification of an installed ISO.
{ pkgs, desktopsModule }:
pkgs.testers.runNixOSTest {
  name = "tatami-network-fit";
  enableOCR = true;
  globalTimeout = 600;
  node.pkgsReadOnly = false;
  nodes.machine = { ... }: {
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
    networking.networkmanager.enable = true;
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
        return next((c for c in json.loads(user("hyprctl -j clients")) if c["class"] == "org.tatami.Network"), None)
    def monitor():
        return next(m for m in json.loads(user("hyprctl -j monitors")) if m["focused"])
    def wait_for(predicate, seconds=15):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if predicate(): return
            time.sleep(0.2)
        machine.screenshot("failure")
        raise AssertionError(json.dumps(dict(window=window(), monitor=monitor())))
    def fits():
        c, m = window(), monitor()
        if not c: return False
        x, y = c["at"]; w, h = c["size"]
        left, top, right, bottom = m["reserved"]
        return (m["x"] + left <= x and m["y"] + top <= y
                and x + w <= m["x"] + m["width"] / m["scale"] - right
                and y + h <= m["y"] + m["height"] / m["scale"] - bottom)
    machine.start()
    machine.wait_for_unit("ydotoold.service")
    machine.wait_until_succeeds("test -S /run/user/1000/hypr/*/.socket.sock", timeout=180)
    signature = machine.succeed("basename /run/user/1000/hypr/*").strip()
    machine.wait_until_succeeds("runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus systemctl --user is-active tatami-waybar.service", timeout=120)
    user("XDG_CONFIG_DIRS=/etc/xdg/tatami:/etc/xdg tatami launch tatami-network")
    wait_for(lambda: window() is not None)
    time.sleep(3)
    machine.screenshot("scale1")
    print("SCALE1", json.dumps(dict(window=window(), monitor=monitor())))
    machine.send_key("meta_l-slash")
    wait_for(lambda: monitor()["scale"] == 1.25)
    time.sleep(4)
    machine.screenshot("scale125")
    print("SCALE125", json.dumps(dict(window=window(), monitor=monitor())))
    changed_scale_fits = fits()
    machine.send_key("meta_l-w")
    wait_for(lambda: window() is None)
    user("XDG_CONFIG_DIRS=/etc/xdg/tatami:/etc/xdg tatami launch tatami-network")
    wait_for(lambda: window() is not None)
    time.sleep(4)
    machine.screenshot("scale125-reopened")
    print("REOPENED", json.dumps(dict(window=window(), monitor=monitor())))
    assert fits(), "Network reopened beyond usable workspace at scale1.25"
    assert changed_scale_fits, "Network exceeded usable workspace after changing to scale1.25"

    from datetime import timedelta
    def text(expected):
        try:
            machine.wait_for_text(expected, timeout=timedelta(seconds=15))
        except Exception:
            machine.screenshot("text-failure")
            raise
    def click(x, y):
        user("hyprctl dispatch " + shlex.quote(f"hl.dsp.cursor.move({{ x = {x}, y = {y} }})"))
        machine.succeed("YDOTOOL_SOCKET=/run/ydotoold/socket ydotool mousemove -x 1 -y 0")
        time.sleep(0.3)
        machine.succeed("YDOTOOL_SOCKET=/run/ydotoold/socket ydotool click 0xC0")
    def close_button():
        c = window()
        click(c["at"][0] + c["size"][0] - 24, c["at"][1] + 24)
        wait_for(lambda: window() is None)
    def open_panel():
        machine.send_key("meta_l-ctrl-w")
        wait_for(lambda: '"namespace": "walker"' in json.dumps(json.loads(user("hyprctl -j layers"))))
        # At the largest scale, search for the row below the initial viewport.
        machine.send_chars("Network settings")
        text("Network settings")
        machine.send_key("ret")
        wait_for(fits)
        time.sleep(2)
    def scroll_bottom():
        c = window()
        x, y = c["at"][0] + c["size"][0] - 35, c["at"][1] + c["size"][1] - 45
        user("hyprctl dispatch " + shlex.quote(f"hl.dsp.cursor.move({{ x = {x}, y = {y} }})"))
        machine.succeed("YDOTOOL_SOCKET=/run/ydotoold/socket ydotool mousemove -x 1 -y 0")
        machine.succeed("YDOTOOL_SOCKET=/run/ydotoold/socket ydotool mousemove --wheel -x 0 -y -40")
        time.sleep(2)

    # Use the node's private NAT connection; no host connection is touched.
    connection = machine.succeed("nmcli --get-values GENERAL.CONNECTION device show eth0").strip()
    assert connection and connection != "--", connection
    text("Ethernet")
    close_button()
    open_panel()
    machine.screenshot("scale125-panel-open")
    for scale in [1.6, 2]:
        machine.send_key("meta_l-slash")
        wait_for(lambda: abs(monitor()["scale"] - scale) < 0.001)
        wait_for(fits)
    machine.screenshot("scale2")
    for scale in [2, 3.2]:
        if scale == 3.2:
            machine.send_key("meta_l-slash")
            wait_for(lambda: abs(monitor()["scale"] - scale) < 0.001)
            wait_for(fits)
        text("Ethernet")
        c = window()
        click(c["at"][0] + c["size"][0] / 2, c["at"][1] + 134)
        text(connection)
        scroll_bottom()
        # Ethernet ends with IPv6; Forget is only offered for Wi-Fi/VPN.
        text("Configure IPv[6é]")
        machine.screenshot("details-bottom-" + str(scale) + ".png")
        assert fits()
        # Header navigation stays reachable even with the page scrolled down.
        c = window()
        click(c["at"][0] + 24, c["at"][1] + 24)
        text("Ethernet")
        machine.screenshot("back-" + str(scale) + ".png")
        close_button()
        open_panel()
        machine.screenshot("reopened-" + str(scale) + ".png")
    for scale in [2, 1.6, 1.25, 1]:
        machine.send_key("meta_l-alt-slash")
        wait_for(lambda: abs(monitor()["scale"] - scale) < 0.001)
        wait_for(fits)
    close_button()
    open_panel()
    machine.screenshot("scale1-restored")
    assert fits()
    close_button()
  '';
}
