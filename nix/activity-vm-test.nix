# Activity in a Tatami session, in a disposable VM. Super+Ctrl+T opens the
# window, floating, on the application that had the focus. The processor
# power meter's socket starts the root service under its sandbox and it
# answers. A VM has no RAPL counters, so a stand-in meter then answers on
# the same socket as `tatami power-meter` does, its counter rising with the
# processor's use, and the Energy view gives a busy process processor power.
# Quit, from the window's action and its dialog, ends that process.
{
  pkgs,
  desktopsModule,
}:
let
  # 8 W idle plus 40 W with every processor busy, from /proc/stat.
  standIn = pkgs.writers.writePython3 "stand-in-power-meter" { } ''
    import os
    import socket
    import time

    PATH = "/run/tatami/power.sock"
    HZ = os.sysconf("SC_CLK_TCK")
    CPUS = os.cpu_count() or 1


    def busy():
        with open("/proc/stat") as f:
            fields = [int(x) for x in f.readline().split()[1:]]
        fields = (fields + [0] * 8)[:8]
        user, nice, system, idle, iowait, irq, softirq, steal = fields
        return (user + nice + system + irq + softirq + steal) / HZ


    start = time.monotonic()
    base = busy()
    os.makedirs("/run/tatami", exist_ok=True)
    if os.path.exists(PATH):
        os.unlink(PATH)
    server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    server.bind(PATH)
    os.chmod(PATH, 0o666)
    server.listen()
    while True:
        client, _ = server.accept()
        idle_joules = 8 * (time.monotonic() - start)
        busy_joules = 40 * (busy() - base) / CPUS
        energy = int((idle_joules + busy_joules) * 1e6)
        client.sendall(f"package-0 {energy} 262143328850\n".encode())
        client.close()
  '';
  socat = "${pkgs.socat}/bin/socat";
  gdbus = "${pkgs.glib.bin}/bin/gdbus";
in
pkgs.testers.runNixOSTest {
  name = "tatami-activity";
  enableOCR = true;
  globalTimeout = 1500;
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
    import json, re, shlex, time
    from datetime import timedelta

    def user(command, check=True):
        status, output = machine.execute(
            "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus "
            f"HYPRLAND_INSTANCE_SIGNATURE={signature} sh -c "
            + shlex.quote("{ " + command + "; } 2>&1"))
        if check and status != 0:
            raise Exception(f"{command!r} failed with {status}:\n{output}")
        return output

    def clients():
        return json.loads(user("hyprctl -j clients"))

    def activity():
        found = [c for c in clients() if c["class"] == "org.tatami.Activity"]
        return found[0] if found else None

    def action(name, value=None):
        target = f"[<'{value}'>]" if value else "[]"
        user(f"${gdbus} call --session --dest org.tatami.Activity "
             "--object-path /org/tatami/Activity/window/1 "
             f"--method org.gtk.Actions.Activate {name} \"{target}\" '{{}}'")

    def inspector_pause_button():
        # The application inspector's third button. Exercise the widget,
        # not win.resume directly: the regression was its stale action name.
        window = activity()
        x = window["at"][0] + window["size"][0] - 78
        y = window["at"][1] + 198
        user("hyprctl dispatch " + shlex.quote(
            f"hl.dsp.cursor.move({{ x = {x}, y = {y} }})"))
        machine.succeed("YDOTOOL_SOCKET=/run/ydotoold/socket ydotool mousemove -x 1 -y 0")
        time.sleep(0.5)
        machine.succeed("YDOTOOL_SOCKET=/run/ydotoold/socket ydotool click 0xC0")

    def wait_action(name):
        # Wait for the sampler's next reading without depending on OCR or
        # a fixed delay. The click below still exercises the inspector widget.
        deadline = time.monotonic() + 20
        while True:
            state = user(f"${gdbus} call --session --dest org.tatami.Activity "
                         "--object-path /org/tatami/Activity/window/1 "
                         f"--method org.gtk.Actions.Describe {name}")
            if "true" in state:
                return
            if time.monotonic() >= deadline:
                raise Exception(f"{name} did not become enabled: {state}")
            time.sleep(0.25)

    def wait_paused(pids, paused):
        deadline = time.monotonic() + 20
        while True:
            states = machine.succeed("ps -o stat= -p " + ",".join(pids)).split()
            if len(states) == len(pids) and all(state.startswith("T") == paused for state in states):
                return
            if time.monotonic() >= deadline:
                raise Exception(f"Expected paused={paused} for {pids}, got {states}")
            time.sleep(0.5)

    def row_focus():
        # The single filtered row. Clicking it gives the table real keyboard
        # focus; changing selection over D-Bus would not reproduce forwarding.
        window = activity()
        x, y = window["at"][0] + 110, window["at"][1] + 126
        user("hyprctl dispatch " + shlex.quote(
            f"hl.dsp.cursor.move({{ x = {x}, y = {y} }})"))
        machine.succeed("YDOTOOL_SOCKET=/run/ydotoold/socket ydotool mousemove -x 1 -y 0")
        time.sleep(0.3)
        machine.succeed("YDOTOOL_SOCKET=/run/ydotoold/socket ydotool click 0xC0")

    def keys(*sequence):
        for key in sequence:
            machine.send_key(key)
            time.sleep(0.3)

    def clipboard():
        return user("WAYLAND_DISPLAY=$(systemctl --user show-environment | "
                    "sed -n 's/^WAYLAND_DISPLAY=//p') wl-paste --no-newline").strip()

    def text(expected):
        try:
            machine.wait_for_text(expected, timeout=timedelta(seconds=15))
        except Exception:
            machine.screenshot("keyboard-failure")
            raise

    machine.start()
    machine.wait_for_unit("ydotoold.service")
    machine.wait_until_succeeds("test -S /run/user/1000/hypr/*/.socket.sock", timeout=180)
    signature = machine.succeed("basename /run/user/1000/hypr/*").strip()
    machine.wait_until_succeeds(
        "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
        "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus "
        "systemctl --user is-active tatami-waybar.service", timeout=120)

    with subtest("The power meter starts from its socket, as root, under its sandbox"):
        machine.succeed("systemctl is-active tatami-power-meter.socket")
        machine.succeed("test -S /run/tatami/power.sock")
        # Anyone may ask; the VM has no RAPL counters, so the answer is empty.
        answer = user("${socat} -t 2 - UNIX-CONNECT:/run/tatami/power.sock < /dev/null")
        print(f"power meter answered {answer!r}")
        machine.wait_until_succeeds("systemctl is-active tatami-power-meter.service", timeout=10)
        print(machine.succeed("systemctl status --no-pager tatami-power-meter.service"))

    with subtest("Super+Ctrl+T opens Activity, floating, on the focused application"):
        machine.send_key("meta_l-ret")  # Tatami's terminal
        machine.wait_until_succeeds(
            "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
            f"HYPRLAND_INSTANCE_SIGNATURE={signature} hyprctl clients | grep -q org.aizome.Hotaru",
            timeout=60)
        time.sleep(3)
        # Something for the terminal to be busy with.
        machine.send_chars("yes > /dev/null\n")
        machine.wait_until_succeeds("pgrep -x yes", timeout=20)
        machine.send_key("meta_l-ctrl-t")
        machine.wait_until_succeeds(
            "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
            f"HYPRLAND_INSTANCE_SIGNATURE={signature} hyprctl clients "
            "| grep -q org.tatami.Activity", timeout=60)
        time.sleep(12)
        window = activity()
        print(json.dumps(window, indent=1))
        print("Activity runtime executable and SHA-256:")
        print(machine.succeed(
            f"readlink -f /proc/{window['pid']}/exe; "
            f"sha256sum /proc/{window['pid']}/exe"))
        assert window["floating"], window
        machine.screenshot("activity")

    with subtest("The application inspector resumes every process it paused"):
        terminal = str(next(c["pid"] for c in clients() if c["class"] == "org.aizome.Hotaru"))
        busy = machine.succeed("pgrep -x yes").strip()
        # Include the terminal's shell without assuming the fixture user's shell.
        pids = sorted({terminal, busy, *machine.succeed("pgrep -P " + terminal).split()})
        inspector_pause_button()
        wait_paused(pids, True)
        wait_action("resume")
        machine.screenshot("application-paused")
        inspector_pause_button()
        wait_paused(pids, False)
        wait_action("pause")
        machine.screenshot("application-resumed")

    with subtest("Processor power, shared out by use, from the meter's socket"):
        machine.succeed("systemctl stop tatami-power-meter.socket tatami-power-meter.service")
        machine.succeed("systemd-run --unit stand-in-power-meter ${standIn}")
        machine.wait_until_succeeds("test -S /run/tatami/power.sock", timeout=20)
        user("tatami-activity --lens energy")
        reading = user("tatami-activity --print")
        print(reading)
        assert re.search(r"Power: processor [0-9.]+ m?W \(Service\)", reading), reading
        # The terminal running `yes` uses a whole processor: it is given
        # most of the processor's power above its idle floor.
        terminal = [line for line in reading.splitlines() if line.startswith("Hotaru")]
        assert terminal, reading
        assert re.search(r"\b[0-9.]+ W\b", terminal[0]), terminal[0]
        time.sleep(10)
        machine.screenshot("energy")

    with subtest("Process keys precede search forwarding only when the table has focus"):
        pid = machine.succeed("pgrep -x yes").strip()
        user(f"tatami-activity --pid {pid} --search {pid} --lens cpu")
        action("mode", "all")
        time.sleep(3)
        row_focus()
        machine.screenshot("keyboard-row-focused")
        keys("shift-f10")
        text("Copy PID")
        text("Delete")  # the context menu retains its shortcut hints
        machine.screenshot("keyboard-process-menu")
        keys("esc")
        row_focus()
        # XKB Menu is evdev KEY_COMPOSE (127); KEY_MENU (139) instead maps
        # to XF86MenuKB. Deliver the advertised key through the input device.
        machine.succeed("YDOTOOL_SOCKET=/run/ydotoold/socket ydotool key 127:1 127:0")
        text("Copy PID")
        machine.screenshot("keyboard-menu-key")
        keys("esc")
        row_focus()
        keys("ctrl-c")
        assert clipboard() == "yes", clipboard()

        # Search retains normal text editing, cutting, copying and context
        # menu behavior. None of these should act on the selected process.
        keys("ctrl-f", "home", "delete", "ctrl-a", "ctrl-c")
        assert clipboard() == pid[1:], (clipboard(), pid)
        machine.send_chars(pid)
        keys("ctrl-a", "shift-delete")
        assert clipboard() == pid, (clipboard(), pid)
        machine.succeed("test -e /proc/" + pid)
        keys("ctrl-v", "shift-f10")
        text("Select All")
        machine.screenshot("keyboard-search-menu")
        keys("esc")

        # Type-to-search still works from the table after handling the reserved
        # process shortcuts. Read the resulting search text through normal Copy.
        row_focus()
        machine.send_chars("x")
        time.sleep(1)
        machine.screenshot("keyboard-type-search")
        # Type-anywhere updates the query without moving keyboard focus from
        # the table. Focus Search before selecting/copying its contents.
        keys("ctrl-f", "ctrl-a", "ctrl-c")
        assert clipboard() == pid + "x", (clipboard(), pid)
        machine.send_chars(pid)
        time.sleep(1)
        row_focus()
        keys("delete")
        text("asked to quit")
        machine.screenshot("keyboard-quit-cancel")
        keys("esc")
        machine.succeed("test -e /proc/" + pid)
        row_focus()
        keys("shift-delete")
        text("Force quitting")
        machine.screenshot("keyboard-force-cancel")
        keys("esc")
        machine.succeed("test -e /proc/" + pid)

    with subtest("Keyboard Quit asks, then ends the selected process"):
        row_focus()
        keys("delete")
        text("asked to quit")
        machine.screenshot("quit")
        machine.send_key("ret")  # Quit, the dialog's default
        machine.wait_until_fails("pgrep -x yes", timeout=20)
        # Removing the last selected row must also clear its inspector. GTK
        # can change selected-item without a nonempty selection-changed range.
        text("Nothing Selected")
        machine.screenshot("quitted")
  '';
}
