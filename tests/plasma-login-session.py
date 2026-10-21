import csv
import io
import json
import os
from pathlib import Path
import re
import shlex
import subprocess
import time

os.environ["OMP_THREAD_LIMIT"] = "1"
PORTAL = "busctl --user --timeout=10 call org.freedesktop.portal.Desktop /org/freedesktop/portal/desktop org.freedesktop.portal.Settings Read ss org.freedesktop.appearance color-scheme 2>&1"


def command(text):
    return "su demo -c " + shlex.quote(
        "export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus; " + text
    )


def user(machine, text):
    return machine.succeed(command(text))


def state(machine, unit):
    return user(machine, "systemctl --user show " + unit + " -p ActiveState --value").strip()


def manager(machine):
    return machine.succeed("systemctl show user@1000.service -p MainPID --value").strip()


def environment(machine):
    return dict(line.split("=", 1) for line in user(machine, "systemctl --user show-environment").splitlines() if "=" in line)


frame_number = 0


def frame(machine, name):
    global frame_number
    frame_number += 1
    stem = f"{frame_number:03d}-{machine.name}-{name}"
    machine.screenshot(stem)
    return Path(machine.out_dir) / (stem + ".png")


def words(machine, name="ocr", psm="11"):
    path = frame(machine, name)
    output = subprocess.check_output(["@tesseract@", str(path), "stdout", "--psm", psm, "tsv"], text=True, timeout=10)
    return [row for row in csv.DictReader(io.StringIO(output), delimiter="\t") if row["text"].strip()]


def wait_word(machine, text, timeout=60, psm="11"):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        rows = words(machine, psm=psm)
        found = [row for row in rows if re.fullmatch(text, row["text"], re.IGNORECASE)]
        if found:
            return found[0]
        time.sleep(0.5)
    raise AssertionError((text, [row["text"] for row in rows]))


def click(machine, x, y):
    path = frame(machine, "before-click")
    width, height = map(int, subprocess.check_output(["@magick@", str(path), "-format", "%w %h", "info:"], text=True).split())
    machine.qmp_client.send("input-send-event", {"events": [
        {"type": "abs", "data": {"axis": "x", "value": round(x * 32767 / (width - 1))}},
        {"type": "abs", "data": {"axis": "y", "value": round(y * 32767 / (height - 1))}},
        {"type": "btn", "data": {"button": "left", "down": True}},
    ]})
    machine.qmp_client.send("input-send-event", {"events": [{"type": "btn", "data": {"button": "left", "down": False}}]})


def click_word(machine, text):
    row = wait_word(machine, text)
    click(machine, int(row["left"]) + int(row["width"]) // 2, int(row["top"]) + int(row["height"]) // 2)


def key(machine, value):
    # Ordered press/release events avoid QEMU sendkey's virtual-time hold
    # overlapping the next chord when the guest runs slower than wall time.
    machine.log("QMP key " + value)
    keys = value.split("-")
    for down, sequence in [(True, keys), (False, list(reversed(keys)))]:
        machine.qmp_client.send("input-send-event", {"events": [
            {"type": "key", "data": {"down": down, "key": {"type": "qcode", "data": key}}}
            for key in sequence
        ]})
        # Give the guest keyboard a polling interval in each state. Sending
        # both states in one batch can leave an observed text field empty.
        time.sleep(0.2)


def verify_test_password(machine):
    # Reveal only this disposable fixture's public password, then verify the
    # actual field pixels before submitting it. Keep the full frame too.
    click(machine, 732, 479)
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        path = frame(machine, "test-password")
        crop = path.with_name(path.stem + "-field.png")
        subprocess.run(["@magick@", str(path), "-crop", "200x20+500+468", "+repage", "-bordercolor", "white", "-border", "10", str(crop)], check=True)
        text = subprocess.check_output(["@tesseract@", str(crop), "stdout", "--psm", "7"], text=True, timeout=10).strip()
        if text == "demo":
            return
        time.sleep(0.5)
    raise AssertionError(("test password field", text))


def wait_plasma_panel(machine):
    # Read the clock region of the observed 1280x800 default panel. Full-frame
    # sparse OCR confuses the wallpaper with text and duplicates clock digits.
    deadline = time.monotonic() + 60
    while time.monotonic() < deadline:
        path = frame(machine, "plasma-panel")
        crop = path.with_name(path.stem + "-clock.png")
        # Include both floating and edge-attached panel positions.
        subprocess.run(["@magick@", str(path), "-crop", "80x56+1144+744", "+repage", "-bordercolor", "white", "-border", "10", str(crop)], check=True)
        text = subprocess.check_output(["@tesseract@", str(crop), "stdout", "--psm", "6"], text=True, timeout=10).strip()
        if any(re.fullmatch(r"[0-2]?[0-9]:[0-5][0-9]\s*[AP]M", line) for line in text.splitlines()):
            return
        time.sleep(0.5)
    raise AssertionError(("Plasma panel clock", text))


def login(machine, session="Xfce", expected_preselection=None):
    machine.wait_for_unit("display-manager.service")
    (Path(machine.out_dir) / (machine.name + "-login-configuration.txt")).write_text(
        machine.succeed("cat /etc/plasmalogin.conf.d/*.conf; systemctl cat plasmalogin.service")
    )
    # A running display-manager unit can precede the first greeter frame.
    wait_word(machine, "Keyboard", timeout=90)
    # Wake the greeter's normally hidden idle controls.
    key(machine, "spc")
    wait_word(machine, "Demo", timeout=90)
    if expected_preselection:
        # Assert the first visible selection before touching the combo. Use
        # its observed text region, retaining the full screenshot as evidence.
        path = frame(machine, "fresh-preselection")
        crop = path.with_name(path.stem + "-selection.png")
        subprocess.run(["@magick@", str(path), "-crop", "285x24+495+502", "+repage", "-bordercolor", "white", "-border", "10", str(crop)], check=True)
        text = subprocess.check_output(["@tesseract@", str(crop), "stdout", "--psm", "7"], text=True, timeout=10).strip()
        assert expected_preselection in text, (expected_preselection, text)
    if session:
        click_word(machine, "Desktop")
        frame(machine, "session-menu-" + session)
        # This fixture offers exactly Plasma then Xfce (the greeter sorts
        # Plasma first). Use its real combo-box keys; tiny popup text is
        # unreliable under full-frame OCR. Assert the launched process and
        # desktop environment below, and retain the actual selection pixels.
        assert session in ["Xfce", "Plasma"]
        key(machine, "end" if session == "Xfce" else "home")
        key(machine, "ret")
        frame(machine, "session-selected-" + session)
    click(machine, 620, 473)
    key(machine, "ctrl-a")
    key(machine, "backspace")
    for letter in "demo":
        key(machine, letter)
    verify_test_password(machine)
    click(machine, 768, 479)


def diagnostics(machine, name):
    frame(machine, name)
    data = {"processes": machine.succeed("ps -U demo -o pid,comm,args"), "manager_pid": manager(machine), "environment": environment(machine),
            "units": user(machine, "systemctl --user show graphical-session.target nixos-fake-graphical-session.target xdg-desktop-portal.service xdg-desktop-portal-xapp.service xdg-desktop-portal-gtk.service xdg-desktop-portal-kde.service"),
            "journal": machine.succeed("journalctl -b --no-pager -u plasmalogin; journalctl -b --no-pager _UID=1000")}
    (Path(machine.out_dir) / (machine.name + "-" + name + ".json")).write_text(json.dumps(data, indent=2))


def xfce(machine, name):
    machine.wait_until_succeeds("pgrep -u demo -f '^xfce4-session$'", timeout=90)
    wait_word(machine, "Applications")
    for unit in ["graphical-session.target", "nixos-fake-graphical-session.target"]:
        assert state(machine, unit) == "active", unit
    result = user(machine, PORTAL)
    (Path(machine.out_dir) / (name + "-portal.txt")).write_text(result)
    assert re.fullmatch(r"v v u [012]\s*", result), result
    for unit in ["xdg-desktop-portal.service", "xdg-desktop-portal-xapp.service", "xdg-desktop-portal-gtk.service"]:
        assert state(machine, unit) == "active", unit
    env = environment(machine)
    assert env["XDG_CURRENT_DESKTOP"] == "XFCE", env
    assert env["XDG_SESSION_TYPE"] == "x11", env
    assert env["SEKKA_SESSION_CUSTOM"] == "preserved", env
    diagnostics(machine, name)


def logout_xfce(machine, name):
    click(machine, 45, 12)
    click_word(machine, r"(?:Out|LogOut)")
    wait_word(machine, "Restart", psm="6")
    frame(machine, name + "-dialog")
    click(machine, 406, 348)
    machine.wait_until_fails("pgrep -u demo -f '^xfce4-session$'", timeout=60)
    machine.wait_until_succeeds(command("test $(systemctl --user show graphical-session.target -p ActiveState --value) = inactive"), timeout=60)
    for unit in ["graphical-session.target", "nixos-fake-graphical-session.target", "xdg-desktop-portal.service", "xdg-desktop-portal-xapp.service", "xdg-desktop-portal-gtk.service"]:
        assert state(machine, unit) == "inactive", unit
    env = environment(machine)
    for key in ["DISPLAY", "XAUTHORITY", "XDG_SESSION_ID", "XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP", "XDG_SESSION_TYPE"]:
        assert key not in env, env
    assert env["SEKKA_SESSION_CUSTOM"] == "preserved"
    diagnostics(machine, name + "-stopped")


with subtest("current unwrapped Plasma Login leaves Xfce portals unavailable"):
    original.start()
    try:
        login(original, expected_preselection="Plasma (Wayland)")
        original.wait_until_succeeds("pgrep -u demo -f '^xfce4-session$'", timeout=90)
        wait_word(original, "Applications")
        assert state(original, "graphical-session.target") == "inactive"
        assert state(original, "nixos-fake-graphical-session.target") == "inactive"
        status, output = original.execute(command(PORTAL))
        (Path(original.out_dir) / "negative-portal.txt").write_text(f"status={status}\n{output}")
        assert status != 0 and "startup job failed" in output, output
        diagnostics(original, "negative-xfce-portals")
    except Exception:
        diagnostics(original, "failure")
        raise
    finally:
        original.shutdown()

with subtest("normal X11 login starts portals and logout tears them down with a lingering manager"):
    fixed.start()
    try:
        # The configured default must work without manually selecting Xfce.
        login(fixed, session=None, expected_preselection="Xfce")
        xfce(fixed, "xfce-first")
        manager_pid = manager(fixed)
        logout_xfce(fixed, "xfce-first")
        assert manager(fixed) == manager_pid
        login(fixed)
        xfce(fixed, "xfce-second")
        assert manager(fixed) == manager_pid
        logout_xfce(fixed, "xfce-second")
        assert manager(fixed) == manager_pid
        login(fixed, "Plasma")
        fixed.wait_until_succeeds("pgrep -u demo -f '(^|/)plasmashell( |$)'", timeout=90)
        fixed.wait_until_succeeds(command("systemctl --user is-active plasma-workspace.target"), timeout=90)
        result = user(fixed, PORTAL)
        (Path(fixed.out_dir) / "plasma-wayland-portal.txt").write_text(result)
        assert re.fullmatch(r"v v u [012]\s*", result), result
        assert state(fixed, "graphical-session.target") == "active"
        assert state(fixed, "nixos-fake-graphical-session.target") == "inactive"
        env = environment(fixed)
        assert env["XDG_SESSION_TYPE"] == "wayland" and env["XDG_CURRENT_DESKTOP"] == "KDE", env
        assert env["SEKKA_SESSION_CUSTOM"] == "preserved"
        assert manager(fixed) == manager_pid
        fixed.wait_until_fails("pgrep -u demo -f '(^|/)ksplashqml( |$)'", timeout=60)
        # The splash can exit before the panel paints. Require its clock,
        # then open the real launcher so the final frame proves interaction.
        wait_plasma_panel(fixed)
        key(fixed, "meta_l")
        wait_word(fixed, r"(?:Favorites|Favourites|Applications)", timeout=60)
        frame(fixed, "plasma-wayland-launcher")
        key(fixed, "esc")
        diagnostics(fixed, "plasma-wayland")
    except Exception:
        diagnostics(fixed, "failure")
        raise
    finally:
        fixed.shutdown()
