import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import time

RECORD = "/run/user/1000/calamares-session-env"
CUSTOM = "before=graphical"
os.environ["OMP_THREAD_LIMIT"] = "1"


def wait_text(machine, expected, timeout=60, regex=False):
    # Preserve the full frame and bound OCR work. The driver's sparse OCR
    # preprocessing is excessively slow on GDM's mostly empty dark screen.
    deadline = time.monotonic() + timeout
    text = ""
    while time.monotonic() < deadline:
        filename = machine.name + "-ocr.png"
        machine.screenshot(filename)
        image = Path(machine.out_dir) / filename
        mode = "11"
        if expected == "Password":
            # Fixed 1280x800 fixture: read the actual GDM input placeholder,
            # whose low contrast makes full-screen sparse OCR omit it.
            prepared = Path(machine.out_dir) / (machine.name + "-password-ocr.png")
            subprocess.run(["@magick@", str(image), "-crop", "210x25+510+478", "+repage",
                            "-resize", "400%", "-colorspace", "Gray", "-negate", "-normalize",
                            str(prepared)], check=True, timeout=10)
            image, mode = prepared, "7"
        text = subprocess.check_output(["@tesseract@", str(image), "stdout", "--psm", mode],
                                       text=True, timeout=10)
        pattern = expected if regex else r"\b" + re.escape(expected) + r"\b"
        if re.search(pattern, text, re.IGNORECASE):
            print(machine.name + " OCR: " + repr(text))
            return
        time.sleep(0.5)
    raise AssertionError((expected, text))


def command(text):
    return "su demo -c " + shlex.quote(
        "export XDG_RUNTIME_DIR=/run/user/1000 "
        "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus; " + text
    )


def user(machine, text):
    return machine.succeed(command(text))


def entries(values):
    return dict(item.split("=", 1) for item in values if "=" in item)


def environment(machine):
    return entries(json.loads(user(machine,
        "busctl --user --json=short get-property org.freedesktop.systemd1 "
        "/org/freedesktop/systemd1 org.freedesktop.systemd1.Manager Environment"))["data"])


def baseline(machine):
    return machine.succeed("cat " + RECORD)


def manager_pid(machine):
    return machine.succeed("systemctl show user@1000.service -p MainPID --value").strip()


def process_environment(machine, name):
    code = r"""
import json, os, re, sys
result = []
for pid in os.listdir('/proc'):
    if not pid.isdigit():
        continue
    try:
        path = '/proc/' + pid
        if os.stat(path).st_uid != 1000:
            continue
        exe = os.readlink(path + '/exe')
        if re.search(r'/[.]?' + re.escape(sys.argv[1]) + r'(-wrapped)*$', exe):
            result.append(dict(item.split('=', 1) for item in open(path + '/environ', 'rb').read().decode().split('\0') if '=' in item))
    except (FileNotFoundError, ProcessLookupError):
        pass
assert len(result) == 1, (sys.argv[1], result)
print(json.dumps(result[0]))
"""
    return json.loads(machine.succeed("python3 -c " + shlex.quote(code) + " " + shlex.quote(name)))


def capture(machine, stage):
    name = machine.name + "-" + stage
    machine.screenshot(name)
    state = {"manager": environment(machine), "baseline": json.loads(baseline(machine)),
             "manager_pid": manager_pid(machine),
             "unit": user(machine, "systemctl --user show calamares-session-env.service")}
    (Path(machine.out_dir) / (name + ".json")).write_text(json.dumps(state, indent=2))


def click(machine, x, y):
    # X11 can choose a different resolution from GDM; use the actual frame.
    filename = machine.name + "-before-click.png"
    machine.screenshot(filename)
    width, height = map(int, subprocess.check_output([
        "@magick@", str(Path(machine.out_dir) / filename), "-format", "%w %h", "info:",
    ], text=True, timeout=10).split())
    machine.qmp_client.send("input-send-event", {"events": [
        {"type": "abs", "data": {"axis": "x", "value": round(x * 32767 / (width - 1))}},
        {"type": "abs", "data": {"axis": "y", "value": round(y * 32767 / (height - 1))}},
        {"type": "btn", "data": {"button": "left", "down": True}},
    ]})
    machine.qmp_client.send("input-send-event", {"events": [
        {"type": "btn", "data": {"button": "left", "down": False}},
    ]})


def password_login(machine):
    machine.wait_until_succeeds("pgrep -G gdm gnome-shell", timeout=90)
    wait_text(machine, "Demo", timeout=90)
    machine.send_key("ret")
    wait_text(machine, "Password", timeout=30)
    machine.send_chars("demo")
    machine.send_key("ret")


def select(machine, desktop, kind):
    # Select through GDM's normal AccountsService state. Authenticate through
    # its actual password UI; no autologin or shell-started session.
    machine.succeed("systemctl stop display-manager.service")
    for method, value in (("SetSession", desktop), ("SetXSession", desktop), ("SetSessionType", kind)):
        machine.succeed("busctl call org.freedesktop.Accounts /org/freedesktop/Accounts/User1000 "
                        "org.freedesktop.Accounts.User " + method + " s " + value)
    machine.succeed("systemctl start display-manager.service")
    password_login(machine)


def wait_tatami(machine):
    machine.wait_until_succeeds(command("systemctl --user is-active wayland-session@tatami.target"), timeout=120)
    machine.wait_until_succeeds(command("systemctl --user is-active tatami-waybar.service"), timeout=90)
    assert environment(machine)["XDG_MENU_PREFIX"] == "hyprland-"
    wait_text(machine, r"\b\d{1,2}:\d{2}\b", timeout=90, regex=True)


def switch(machine, path, label):
    status, _ = machine.execute(path + "/bin/switch-to-configuration switch > /tmp/switch-" + label + ".log 2>&1", timeout=180)
    machine.copy_from_machine("/tmp/switch-" + label + ".log", machine.name)
    assert status == 0, machine.succeed("cat /tmp/switch-" + label + ".log")
    capture(machine, label)


def wait_greeter_retired(machine):
    # GDM retires its previous Wayland greeter after a timeout. A rendered
    # user desktop can therefore overlap the greeter's stop transaction.
    # This check covers activation in a settled session; observe natural
    # retirement rather than stopping the greeter or retrying activation.
    uid = machine.succeed("id -u gdm-greeter").strip()
    state = {}
    for stage in ("before", "after"):
        state[stage] = {
            "uptime": machine.succeed("cat /proc/uptime").strip(),
            "runtime_exists": machine.execute("test -d /run/user/" + uid)[0] == 0,
            "unit": machine.execute("systemctl show user@" + uid + ".service -p ActiveState -p SubState -p MainPID"),
            "login": machine.execute("loginctl show-user " + uid + " -p State -p Sessions -p Linger"),
        }
        if stage == "before":
            machine.wait_until_succeeds("test ! -d /run/user/" + uid, timeout=90)
    (Path(machine.out_dir) / (machine.name + "-greeter-retirement.json")).write_text(json.dumps(state, indent=2))


for machine in (original, fixed):
    machine.start()
    machine.wait_for_unit("display-manager.service")
    try:
        password_login(machine)
        wait_tatami(machine)
        pid = manager_pid(machine)
        before = baseline(machine)
        initial = entries(json.loads(before))
        assert initial["SEKKA_BASELINE_CUSTOM"] == CUSTOM
        for name in ("XDG_MENU_PREFIX", "WAYLAND_DISPLAY", "XDG_BACKEND", "DCONF_PROFILE"):
            assert name not in initial, (name, initial)
        capture(machine, "01-tatami")
        wait_greeter_retired(machine)
        toplevel = machine.succeed("readlink -f /run/current-system").strip()
        invocation = user(machine, "systemctl --user show calamares-session-env.service -p InvocationID --value")

        with subtest(machine.name + ": activation preserves the baseline, or reproduces its corruption"):
            switch(machine, toplevel, "02-unchanged-switch")
            if machine == original:
                assert entries(json.loads(baseline(machine)))["XDG_MENU_PREFIX"] == "hyprland-"
                assert baseline(machine) != before
            else:
                assert baseline(machine) == before
                switch(machine, toplevel + "/specialisation/updated", "03-updated-switch")
                assert baseline(machine) == before
                switch(machine, toplevel, "04-rollback")
                assert baseline(machine) == before
                assert user(machine, "systemctl --user show calamares-session-env.service -p InvocationID --value") == invocation
            assert manager_pid(machine) == pid

        user(machine, "tatami logout")
        machine.wait_until_fails("pgrep -u demo Hyprland", timeout=90)
        select(machine, "xfce", "x11")
        machine.wait_until_succeeds("pgrep -u demo xfce4-panel", timeout=120)
        machine.wait_until_succeeds(command("busctl --user introspect org.xfce.SessionManager /org/xfce/SessionManager org.xfce.Session.Manager"), timeout=60)
        time.sleep(2)
        capture(machine, "05-xfce")
        for name in ("xfce4-session", "xfce4-panel"):
            env = process_environment(machine, name)
            (Path(machine.out_dir) / (machine.name + "-" + name + ".json")).write_text(json.dumps(env, indent=2))
            assert env["SEKKA_BASELINE_CUSTOM"] == CUSTOM
            assert env["XDG_CURRENT_DESKTOP"] == "XFCE" and env["XDG_SESSION_TYPE"] == "x11"
            assert env.get("XDG_MENU_PREFIX") == ("hyprland-" if machine == original else "xfce-"), env
            if machine == fixed:
                assert not env.get("XDG_BACKEND") and not env.get("WAYLAND_DISPLAY"), env
        if machine == original:
            wait_text(machine, "Failed to load the applications menu", timeout=30)
            machine.screenshot("original-menu-failure")
        else:
            assert "WAYLAND_DISPLAY" not in environment(machine)
            assert "DCONF_PROFILE" not in environment(machine)
            click(machine, 50, 12)
            wait_text(machine, "Settings", timeout=30)
            machine.screenshot("fixed-xfce-menu")
            machine.send_key("esc")
            machine.wait_until_succeeds(command("busctl --user call org.xfce.SessionManager /org/xfce/SessionManager org.xfce.Session.Manager Logout bb false false"), timeout=60)
            machine.wait_until_fails("pgrep -u demo xfce4-session", timeout=60)
            select(machine, "gnome", "wayland")
            machine.wait_until_succeeds(command("systemctl --user is-active gnome-session.target"), timeout=120)
            machine.wait_until_succeeds(command("busctl --user call org.gnome.SessionManager /org/gnome/SessionManager org.gnome.SessionManager IsSessionRunning | grep -q 'b true'"), timeout=120)
            # Session readiness can precede the VT's first user frame; a
            # greeter clock alone does not prove that GNOME has rendered.
            user(machine, "busctl --user call org.gnome.Nautilus /org/gnome/Nautilus org.freedesktop.Application Activate 'a{sv}' 0")
            wait_text(machine, "Home", timeout=90)
            assert environment(machine)["XDG_CURRENT_DESKTOP"] == "GNOME"
            assert environment(machine)["SEKKA_BASELINE_CUSTOM"] == CUSTOM
            capture(machine, "06-gnome")
            user(machine, "busctl --user call org.gnome.SessionManager /org/gnome/SessionManager org.gnome.SessionManager Logout u 1")
            machine.wait_until_fails("pgrep -u demo gnome-shell", timeout=90)
            select(machine, "tatami", "wayland")
            wait_tatami(machine)
            assert baseline(machine) == before and manager_pid(machine) == pid
            capture(machine, "07-tatami-return")
            user(machine, "tatami logout")
            machine.wait_until_fails("pgrep -u demo Hyprland", timeout=90)
            machine.succeed("systemctl restart user@1000.service")
            machine.wait_until_succeeds(command("systemctl --user is-active calamares-session-env.service"), timeout=60)
            assert manager_pid(machine) != pid
            fresh = entries(json.loads(baseline(machine)))
            assert fresh["SEKKA_BASELINE_CUSTOM"] == CUSTOM
            for name in ("XDG_MENU_PREFIX", "WAYLAND_DISPLAY", "XDG_BACKEND", "DCONF_PROFILE"):
                assert name not in fresh, (name, fresh)
            capture(machine, "08-new-manager")
    finally:
        machine.screenshot(machine.name + "-final")
        machine.succeed("journalctl -b --no-pager > /tmp/session-environment-journal.txt")
        machine.copy_from_machine("/tmp/session-environment-journal.txt", machine.name)
        machine.shutdown()
