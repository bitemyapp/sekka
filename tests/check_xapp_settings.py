"""Real XApp Settings backend under a private X server and D-Bus session."""

import math
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time


def wait_for(check, description):
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline:
        if check():
            return
        time.sleep(0.05)
    raise AssertionError(description)


def call(method, *args):
    return subprocess.check_output(
        ["gdbus", "call", "--session", "--dest",
         "org.freedesktop.impl.portal.desktop.xapp", "--object-path",
         "/org/freedesktop/portal/desktop", "--method",
         "org.freedesktop.impl.portal.Settings." + method, *args],
        text=True, timeout=10,
    )


def read_accent(expected):
    answer = call("Read", "org.freedesktop.appearance", "accent-color")
    values = re.search(r"<\(([^)]+)\)>", answer)
    assert values, answer
    actual = tuple(float(x.strip()) for x in values.group(1).split(","))
    assert len(actual) == 3 and all(
        math.isclose(a, b, abs_tol=1e-6) for a, b in zip(actual, expected)
    ), answer
    print(answer.strip(), flush=True)


package, expected = sys.argv[1:]
children = []
try:
    # Both theme and font arrive together, as when xfsettingsd starts after
    # a desktop application has already activated the portal during login.
    for theme, color in (("SekkaBlue", "#336699"), ("SekkaRed", "#cc6633")):
        directory = Path.home() / ".themes" / theme / "gtk-3.0"
        directory.mkdir(parents=True, exist_ok=True)
        (directory / "gtk.css").write_text("@define-color accent_color " + color + ";\n")

    app = subprocess.Popen(
        [package + "/libexec/xdg-desktop-portal-xapp", "--verbose"],
        env=dict(os.environ, XDG_CURRENT_DESKTOP="XFCE"),
        stdout=open("xapp.log", "w"), stderr=subprocess.STDOUT,
    )
    children.append(app)

    def owns_bus_name():
        answer = subprocess.check_output(
            ["gdbus", "call", "--session", "--dest", "org.freedesktop.DBus",
             "--object-path", "/org/freedesktop/DBus", "--method",
             "org.freedesktop.DBus.NameHasOwner", "org.freedesktop.impl.portal.desktop.xapp"],
            text=True, timeout=10,
        )
        return "true" in answer

    wait_for(owns_bus_name, "XApp did not acquire its bus name")
    monitor = subprocess.Popen(
        ["gdbus", "monitor", "--session", "--dest",
         "org.freedesktop.impl.portal.desktop.xapp", "--object-path",
         "/org/freedesktop/portal/desktop"],
        stdout=open("signals.log", "w"), stderr=subprocess.STDOUT,
    )
    children.append(monitor)
    wait_for(lambda: "owned by" in Path("signals.log").read_text(), "monitor not ready")
    config = Path("xsettings.conf")
    config.write_text('Net/ThemeName "SekkaBlue"\nGtk/FontName "Sans 13"\n')
    settings = subprocess.Popen(
        ["xsettingsd", "-c", str(config)],
        stdout=open("xsettings.log", "w"), stderr=subprocess.STDOUT,
    )
    children.append(settings)
    if expected == "crash":
        wait_for(lambda: app.poll() is not None, "unpatched control did not reproduce")
        assert app.returncode == -signal.SIGSEGV, app.returncode
        print("unpatched Settings backend reproduced SIGSEGV", flush=True)
    else:
        def signal_count():
            assert app.poll() is None, (app.returncode, Path("xapp.log").read_text())
            return Path("signals.log").read_text().count(".SettingChanged")

        wait_for(lambda: signal_count() >= 1, "initial theme notification missing")
        read_accent((0.2, 0.4, 0.6))
        initial = signal_count()
        config.write_text('Net/ThemeName "SekkaRed"\nGtk/FontName "Sans 14"\n')
        settings.send_signal(signal.SIGHUP)
        wait_for(lambda: signal_count() > initial, "changed theme notification missing")
        read_accent((0.8, 0.4, 0.2))
        assert "accent-color" in call("ReadAll", "['org.freedesktop.appearance']")
        assert app.poll() is None
        print("patched backend reads both themes and emits Settings changes", flush=True)
finally:
    for child in reversed(children):
        if child.poll() is None:
            child.terminate()
        try:
            child.wait(timeout=5)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait()
