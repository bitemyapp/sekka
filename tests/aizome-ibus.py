# Test actual engine composition through QEMU keys and real app input contexts.
import json
import shlex
import time
from datetime import timedelta


def user(command):
    return machine.succeed(
        "su alice -c "
        + shlex.quote(
            "export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus; "
            + command
        )
    )


def session(command):
    # systemd-run uses the running desktop's imported environment.
    return user("systemd-run --user --quiet --wait --pipe --collect " + command)


def ipc(command):
    return json.loads(session("aizomectl --json " + command))


def until(check, timeout=30):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if check():
            return
        time.sleep(0.2)
    raise AssertionError("condition did not become true")


def key(name):
    machine.send_key(name, delay=timedelta(milliseconds=300))


def type_keys(text):
    machine.send_chars(text, delay=timedelta(milliseconds=300))


def focus(app):
    until(lambda: any(w["app_id"] == app for w in ipc("windows")["windows"]))
    window = next(w for w in ipc("windows")["windows"] if w["app_id"] == app)
    ipc("focus " + str(window["id"]))
    time.sleep(0.5)


machine.start()
machine.wait_for_unit("display-manager.service")
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
user("systemctl --user is-active aizome-ibus.service")
machine.wait_until_succeeds("pgrep -u alice -f ibus-engine-simple")
assert "hangul - Hangul" in session("ibus list-engine")
print(user("systemctl --user status aizome-ibus.service"))
session(
    "dconf write /org/freedesktop/ibus/engine/hangul/initial-input-mode \"'hangul'\""
)

with subtest("frontend failures remain failures while Aizome is running"):
    # Inject exit 1 into a temporary copy of the installed wrapper, retaining
    # its actual state guard. A live-session error must remain an error.
    user("systemctl --user is-active aizome.service aizome-session.target")
    start = (
        user("systemctl --user show -p ExecStart --value aizome-ibus.service")
        .split("path=", 1)[1]
        .split(" ;", 1)[0]
    )
    script = machine.succeed("cat " + start).splitlines()
    assert "/libexec/ibus-ui-gtk3 --enable-wayland-im" in script[1]
    script[1] = "false # injected frontend exit 1"
    machine.succeed(
        "printf %s " + shlex.quote("\n".join(script) + "\n") + " > /tmp/ibus-exit-one"
    )
    code, output = machine.execute(
        "su alice -c "
        + shlex.quote(
            "export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus; "
            "bash /tmp/ibus-exit-one"
        )
    )
    assert code == 1, output
    user("systemctl --user is-active aizome.service aizome-session.target")
    # Kill the actual frontend. Its wrapper must fail, the unit must restart,
    # and composition below must work through the replacement daemon.
    old_main = user("systemctl --user show -p MainPID --value aizome-ibus.service")
    shell_pid = machine.succeed("pgrep -u alice -x aizome-shell")
    old_daemon = machine.succeed(
        "pgrep -u alice -f '^/nix/store/[^ ]+/bin/ibus-daemon '"
    )
    machine.succeed("pkill -KILL -u alice -f '^/nix/store/[^ ]+/libexec/ibus-ui-gtk3 '")
    until(
        lambda: (
            user("systemctl --user show -p MainPID --value aizome-ibus.service")
            not in (old_main, "0\n")
        )
    )
    machine.wait_until_succeeds(
        "pgrep -u alice -f '^/nix/store/[^ ]+/bin/ibus-daemon '"
    )
    assert (
        machine.succeed("pgrep -u alice -f '^/nix/store/[^ ]+/bin/ibus-daemon '")
        != old_daemon
    )
    user(
        "systemctl --user is-active aizome-ibus.service aizome.service aizome-session.target"
    )
    assert (
        int(user("systemctl --user show -p NRestarts --value aizome-ibus.service")) > 0
    )
    assert machine.succeed("pgrep -u alice -x aizome-shell") == shell_pid

with subtest("native Sumi composes with the real Hangul engine"):
    user("touch /home/alice/native.txt")
    session("aizomectl spawn aizome-sumi /home/alice/native.txt")
    focus("org.aizome.Sumi")
    session("ibus engine xkb:us::eng")
    type_keys("Hello 42")
    key("ret")
    key("meta_l-s")
    until(lambda: machine.succeed("cat /home/alice/native.txt") == "Hello 42\n")
    machine.screenshot("00-native-xkb")
    key("meta_l-a")
    session("ibus engine hangul")
    type_keys("gksrmf")
    key("backspace")
    key("f")
    machine.screenshot("01-native-preedit")
    key("spc")
    key("1")
    key("ret")
    key("meta_l-s")
    print("native bytes:", repr(machine.succeed("cat /home/alice/native.txt")))
    until(lambda: machine.succeed("cat /home/alice/native.txt") == "한글 1\n")
    machine.screenshot("02-native-committed")
    key("meta_l-q")

with subtest("Chrome keeps typing after tab shortcuts and preserves composition"):
    session("env SEKKA_IBUS_TEST=1 ibus-chrome-probe /tmp/ibus-chrome")
    machine.copy_from_machine("/tmp/ibus-chrome/results.json")
    machine.copy_from_machine("/tmp/ibus-chrome/chrome.log")

with subtest("GTK composes through the Wayland frontend"):
    session("aizomectl spawn ibus-gtk-probe /tmp/gtk-text")
    focus("ibus-gtk-probe")
    session("ibus engine xkb:us::eng")
    type_keys("Hello 42")
    until(lambda: machine.succeed("cat /tmp/gtk-text") == "Hello 42")
    machine.screenshot("03-gtk-xkb")
    key("ctrl-a")
    session("ibus engine hangul")
    type_keys("gksrmf")
    machine.screenshot("03-gtk-preedit")
    key("spc")
    key("1")
    print("GTK bytes:", repr(machine.succeed("cat /tmp/gtk-text")))
    until(lambda: machine.succeed("cat /tmp/gtk-text") == "한글 1")
    assert machine.succeed("cat /tmp/gtk-preedit").strip(), "no GTK preedit"
    machine.screenshot("04-gtk-committed")
    key("ret")
    until(lambda: machine.succeed("cat /tmp/gtk-activated") == "한글 1")

machine.copy_from_machine("/home/alice/native.txt")
machine.copy_from_machine("/tmp/gtk-text")
machine.copy_from_machine("/tmp/gtk-preedit")


def daemon_count():
    return int(
        machine.succeed(
            "pgrep -u alice -f '^/nix/store/[^ ]+/bin/ibus-daemon ' | wc -l"
        )
    )


def logged_out():
    machine.wait_until_fails("pgrep -u alice -x aizome-shell", timeout=60)
    until(
        lambda: (
            user(
                "systemctl --user show -p ActiveState --value aizome-ibus.service"
            ).strip()
            == "inactive"
        )
    )
    machine.wait_until_fails(
        "pgrep -u alice -f '^/nix/store/[^ ]+/bin/ibus-daemon '", timeout=60
    )
    assert (
        machine.succeed("systemctl show user@1000.service -p MainPID --value")
        == manager_pid
    )
    # The same injected exit is normal only once its Aizome session is gone.
    user("bash /tmp/ibus-exit-one")


def login(desktop, kind):
    machine.succeed("systemctl stop display-manager.service")
    path = "/org/freedesktop/Accounts/User1000"
    for method, value in (
        ("SetSession", desktop),
        ("SetXSession", desktop),
        ("SetSessionType", kind),
    ):
        machine.succeed(
            "busctl call org.freedesktop.Accounts "
            + path
            + " org.freedesktop.Accounts.User "
            + method
            + " s "
            + value
        )
    machine.succeed("systemctl start display-manager.service")


def gtk_composition(desktop):
    target = "/tmp/gtk-" + desktop
    machine.succeed("rm -f /tmp/gtk-preedit /tmp/gtk-activated")
    session(
        "dconf write /org/freedesktop/ibus/engine/hangul/initial-input-mode \"'hangul'\""
    )
    user(
        "systemd-run --user --quiet --collect --unit=gtk-"
        + desktop
        + " ibus-gtk-probe "
        + target
    )
    user("systemctl --user is-active gtk-" + desktop + ".service")
    time.sleep(2)
    if desktop == "xfce":
        session("xdotool search --name 'IBus GTK composition' windowactivate --sync")
    else:
        # A background user-unit launch has no GNOME activation token. Focus
        # its only application window through the normal window switcher.
        key("alt-tab")
        key("esc")
        time.sleep(1)
    machine.screenshot("composition-ready-" + desktop)
    session("ibus engine hangul")
    type_keys("gksrmf")
    key("spc")
    key("1")
    key("ret")
    until(lambda: machine.succeed("cat " + target) == "한글 1")
    until(lambda: machine.succeed("cat /tmp/gtk-activated") == "한글 1")
    assert machine.succeed("cat /tmp/gtk-preedit").strip()
    machine.screenshot("composition-" + desktop)
    machine.copy_from_machine(target)
    key("alt-f4")


manager_pid = machine.succeed("systemctl show user@1000.service -p MainPID --value")
with subtest("Aizome logout ends its frontend with a persistent user manager"):
    assert daemon_count() == 1
    user("systemd-run --user --quiet --collect aizomectl quit")
    logged_out()

with subtest("Xfce keeps the legacy IBus frontend and composition"):
    login("xfce", "x11")
    machine.wait_until_succeeds(
        "pgrep -u alice -f '(^|/)[.]?xfce4-session(-wrapped)?($| )'",
        timeout=120,
    )
    machine.wait_until_succeeds(
        "pgrep -u alice -f '^/nix/store/[^ ]+/bin/ibus-daemon '", timeout=60
    )
    assert daemon_count() == 1
    assert (
        user("systemctl --user show -p ActiveState --value aizome-ibus.service").strip()
        == "inactive"
    )
    env = user("systemctl --user show-environment")
    assert "GTK_IM_MODULE=wayland" not in env and "QT_IM_MODULE=wayland" not in env, env
    assert (
        session("dconf read /desktop/ibus/general/use-system-keyboard-layout").strip()
        != "true"
    )
    gtk_composition("xfce")
    user(
        "busctl --user call org.xfce.SessionManager /org/xfce/SessionManager org.xfce.Session.Manager Logout bb false false"
    )
    machine.wait_until_fails(
        "pgrep -u alice -f '(^|/)[.]?xfce4-session(-wrapped)?($| )'", timeout=60
    )
    logged_out()

with subtest("GNOME keeps its own IBus service and composition"):
    login("gnome", "wayland")
    machine.wait_until_succeeds(
        "pgrep -u alice -f '(^|/)[.]?gnome-shell(-wrapped)?($| )'", timeout=120
    )
    machine.wait_until_succeeds(
        "pgrep -u alice -f '^/nix/store/[^ ]+/bin/ibus-daemon '", timeout=60
    )
    user("systemctl --user is-active org.freedesktop.IBus.session.GNOME.service")
    machine.wait_until_succeeds(
        "su alice -c 'XDG_RUNTIME_DIR=/run/user/1000 busctl --user call org.gnome.SessionManager /org/gnome/SessionManager org.gnome.SessionManager IsSessionRunning' | grep -Fx 'b true'",
        timeout=60,
    )
    assert daemon_count() == 1
    assert (
        user("systemctl --user show -p ActiveState --value aizome-ibus.service").strip()
        == "inactive"
    )
    assert (
        session("dconf read /desktop/ibus/general/use-system-keyboard-layout").strip()
        != "true"
    )
    session(
        "gsettings set org.gnome.desktop.input-sources sources \"[('ibus', 'hangul')]\""
    )
    time.sleep(3)
    # Close the first-login welcome dialog, then the initial overview.
    key("esc")
    time.sleep(0.5)
    key("esc")
    gtk_composition("gnome")
    user(
        "busctl --user call org.gnome.SessionManager /org/gnome/SessionManager org.gnome.SessionManager Logout u 1"
    )
    machine.wait_until_fails(
        "pgrep -u alice -f '(^|/)[.]?gnome-shell(-wrapped)?($| )'", timeout=90
    )
    logged_out()

with subtest("returning to Aizome starts one fresh Wayland frontend"):
    login("aizome", "wayland")
    machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
    user("systemctl --user is-active aizome-ibus.service")
    machine.wait_until_succeeds(
        "pgrep -u alice -f '^/nix/store/[^ ]+/bin/ibus-daemon '", timeout=60
    )
    assert daemon_count() == 1
    env = user("systemctl --user show-environment")
    assert "GTK_IM_MODULE=wayland" in env and "QT_IM_MODULE=wayland" in env, env
    assert (
        session("dconf read /desktop/ibus/general/use-system-keyboard-layout").strip()
        == "true"
    )
    assert (
        machine.succeed("systemctl show user@1000.service -p MainPID --value")
        == manager_pid
    )
    session("aizomectl spawn aizome-sumi /home/alice/native.txt")
    focus("org.aizome.Sumi")
    key("meta_l-a")
    session("ibus engine hangul")
    type_keys("gksrmf")
    key("spc")
    key("2")
    key("ret")
    key("meta_l-s")
    until(lambda: machine.succeed("cat /home/alice/native.txt") == "한글 2\n")
    machine.screenshot("native-after-session-switches")
    user("systemd-run --user --quiet --collect aizomectl quit")
    logged_out()

machine.succeed("journalctl _UID=1000 --no-pager > /tmp/ibus-session.log")
machine.copy_from_machine("/tmp/ibus-session.log")
