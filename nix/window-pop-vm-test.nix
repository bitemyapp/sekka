# Normal Pop Out shortcut in the shipped Tatami session. Component evidence,
# not qualification of an ISO. Also runnable with the predecessor module to
# demonstrate both the oversized geometry and already-floating regressions.
{ pkgs, desktopsModule }:
pkgs.testers.runNixOSTest {
  name = "tatami-window-pop";
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
  };
  testScript = ''
    import json, shlex, time

    machine.start()
    machine.wait_until_succeeds("test -S /run/user/1000/hypr/*/.socket.sock", timeout=180)
    signature = machine.succeed("basename /run/user/1000/hypr/*").strip()
    def user(command):
        return machine.succeed("runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus "
            f"HYPRLAND_INSTANCE_SIGNATURE={signature} sh -c " + shlex.quote(command))
    def clients():
        return json.loads(user("hyprctl -j clients"))
    def window():
        return next((c for c in clients() if c["class"] == "foot"), None)
    def monitor():
        return json.loads(user("hyprctl -j monitors"))[0]
    def wait(predicate, seconds=20):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if predicate():
                return
            time.sleep(0.2)
        machine.screenshot("timeout")
        raise AssertionError(json.dumps(dict(window=window(), monitor=monitor())))
    def fits(c, m):
        w, h = m["width"] / m["scale"], m["height"] / m["scale"]
        if m["transform"] % 2:
            w, h = h, w
        left, top, right, bottom = m["reserved"]
        x, y = c["at"]
        cw, ch = c["size"]
        # Include the configured two-pixel border as well as client content.
        return (m["x"] + left <= x - 2 and m["y"] + top <= y - 2
                and x + cw + 2 <= m["x"] + w - right
                and y + ch + 2 <= m["y"] + h - bottom)

    machine.wait_until_succeeds("runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
        "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus "
        "systemctl --user is-active tatami-waybar.service", timeout=120)
    wait(lambda: monitor()["reserved"][1] > 0)
    failures = []
    def check(condition, message):
        if not condition:
            failures.append(message)
    def snapshot(name):
        time.sleep(0.5)
        c, m = window(), monitor()
        print(json.dumps(dict(case=name, window=c, monitor=m)))
        machine.screenshot(name)
        return c, m
    def exercise(name, floating):
        machine.send_key("meta_l-ret")
        wait(lambda: window() is not None)
        if floating:
            machine.send_key("meta_l-t")
            wait(lambda: window()["floating"])
        snapshot(name + "-before")
        machine.send_key("meta_l-o")
        # A tag is added last by both versions, even when the old pin failed.
        wait(lambda: "pop" in window()["tags"])
        c, m = snapshot(name + "-popped")
        check(c["floating"] and c["pinned"], name + ": pop must float and pin")
        check(fits(c, m), name + ": pop exceeds usable logical workspace")
        if c["pinned"]:
            # Real pin behavior: it remains visible on another workspace.
            machine.send_key("meta_l-2")
            wait(lambda: window()["workspace"]["id"] == 2)
            c, m = snapshot(name + "-workspace2")
            check(c["visible"] and fits(c, m), name + ": pinned window not reachable")
            machine.send_key("meta_l-o")
            wait(lambda: not window()["pinned"] and not window()["floating"])
            c, m = snapshot(name + "-returned")
            check("pop" not in c["tags"] and fits(c, m), name + ": return to tiling failed")
        # The negative control can fail to pin; close that fixture normally
        # rather than changing its result by a corrective dispatcher call.
        machine.send_key("meta_l-w")
        wait(lambda: window() is None)
        machine.send_key("meta_l-1")

    for scale in [1, 2]:
        if scale == 2:
            for offered in [1.25, 1.6, 2]:
                machine.send_key("meta_l-slash")
                wait(lambda: abs(monitor()["scale"] - offered) < 0.001)
        exercise(f"scale{scale}-tiled", False)
        exercise(f"scale{scale}-floating", True)
    # A synthetic transformed/origin-shifted monitor fixture verifies that
    # the same shortcut uses logical coordinates, not the framebuffer size.
    user("hyprctl eval 'hl.monitor({ output = \"Virtual-1\", mode = \"1280x800@60\", "
         "position = \"1280x80\", scale = 2, transform = 1 })'")
    wait(lambda: monitor()["transform"] == 1 and monitor()["x"] == 1280
         and monitor()["y"] == 80 and monitor()["scale"] == 2)
    exercise("rotated-origin-floating", True)
    assert not failures, json.dumps(failures)
  '';
}
