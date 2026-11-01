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
        return next((c for c in clients() if c["class"] == "org.aizome.Hotaru"), None)
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

    with subtest("Hotaru is the default, and terminal shortcuts keep separate windows and cwd"):
        machine.fail("command -v foot")
        machine.succeed("test -f /run/current-system/sw/share/applications/org.aizome.Hotaru.desktop")
        machine.send_key("meta_l-ret")
        wait(lambda: window() is not None)
        first = window()
        user("mkdir -p '/home/demo/terminal space'")
        machine.send_chars("cd '/home/demo/terminal space'\n")
        def shell_cwd(pid):
            children = machine.succeed(f"pgrep -P {pid} || true").split()
            return [machine.succeed(f"readlink /proc/{p}/cwd").strip() for p in children]
        wait(lambda: "/home/demo/terminal space" in shell_cwd(first["pid"]))
        # A newer background tab must not supply the focused tab's cwd.
        machine.send_key("ctrl-shift-t")
        wait(lambda: len(shell_cwd(first["pid"])) == 2)
        machine.send_chars("cd /tmp\n")
        wait(lambda: "/tmp" in shell_cwd(first["pid"]))
        machine.send_key("ctrl-shift-tab")
        machine.wait_until_succeeds(
            "runuser -u demo -- sh -c " + shlex.quote(
                f"test \"$(aizome-hotaru --print-working-directory {first['pid']})\" = '/home/demo/terminal space'"))
        machine.send_key("meta_l-ret")
        wait(lambda: len([c for c in clients() if c["class"] == "org.aizome.Hotaru"]) == 2)
        second = next(c for c in clients() if c["class"] == "org.aizome.Hotaru" and c["pid"] != first["pid"])
        wait(lambda: "/home/demo/terminal space" in shell_cwd(second["pid"]))
        machine.screenshot("hotaru-separate-windows")
        machine.send_key("meta_l-w")
        wait(lambda: len(clients()) == 1)
        machine.send_key("meta_l-w")
        wait(lambda: window() is None)

    with subtest("TUI focus and About hold preserve their independent window identity"):
        user("tatami tui sleep 120")
        wait(lambda: any(c["class"] == "org.tatami.sleep" for c in clients()))
        first = next(c for c in clients() if c["class"] == "org.tatami.sleep")
        user("tatami tui sleep 120")
        time.sleep(1)
        assert [c["pid"] for c in clients() if c["class"] == "org.tatami.sleep"] == [first["pid"]]
        machine.send_key("meta_l-w")
        wait(lambda: not clients())
        user("tatami about")
        wait(lambda: any(c["class"] == "org.tatami.fastfetch" for c in clients()))
        about = next(c for c in clients() if c["class"] == "org.tatami.fastfetch")
        assert about["floating"], about
        machine.wait_until_fails("pgrep -u demo -x fastfetch", timeout=30)
        time.sleep(1)
        assert any(c["pid"] == about["pid"] for c in clients())
        machine.screenshot("hotaru-about-held")
        machine.send_key("q")
        wait(lambda: not clients())

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
