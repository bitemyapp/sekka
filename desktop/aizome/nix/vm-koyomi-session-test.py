import shlex
import time


def command(text):
    return "su demo -c " + shlex.quote(
        "export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus; "
        + text
    )


def user(text):
    return machine.succeed(command(text))


def active(unit):
    machine.wait_until_succeeds(
        command("systemctl --user is-active " + unit), timeout=60
    )


def inactive(unit):
    machine.wait_until_succeeds(
        command(
            'test "$(systemctl --user show -p ActiveState --value '
            + unit
            + ')" = inactive'
        ),
        timeout=60,
    )


def no_session():
    inactive("graphical-session.target")
    inactive("aizome-koyomi-agent.service")
    assert (
        machine.succeed("systemctl show user@1000.service -p MainPID --value")
        == manager_pid
    )
    # A manual start outside a graphical session must be skipped, without
    # starting the target or leaving a calendar agent alive.
    user("systemctl --user start aizome-koyomi-agent.service")
    inactive("aizome-koyomi-agent.service")
    inactive("graphical-session.target")
    machine.fail("pgrep -u demo -f '[.]?aizome-koyomi(-wrapped)? --agent'")


def login(desktop, kind):
    machine.succeed("systemctl stop display-manager.service")
    for method, value in (
        ("SetSession", desktop),
        ("SetXSession", desktop),
        ("SetSessionType", kind),
    ):
        machine.succeed(
            "busctl call org.freedesktop.Accounts /org/freedesktop/Accounts/User1000 "
            "org.freedesktop.Accounts.User " + method + " s " + value
        )
    machine.succeed("systemctl start display-manager.service")


machine.start()
machine.wait_for_unit("display-manager.service")
machine.wait_until_succeeds(
    "pgrep -u demo -f '(^|/)[.]?xfce4-session(-wrapped)?($| )'", timeout=120
)
manager_pid = machine.succeed("systemctl show user@1000.service -p MainPID --value")

with subtest("Koyomi runs in Xfce and releases the session on normal logout"):
    active("aizome-koyomi-agent.service")
    active("graphical-session.target")
    machine.wait_until_succeeds(
        command(
            "busctl --user introspect org.xfce.SessionManager /org/xfce/SessionManager org.xfce.Session.Manager"
        ),
        timeout=60,
    )
    time.sleep(3)
    machine.screenshot("01-xfce-koyomi")
    # Xfce exports the bus name before startup finishes; request its normal
    # logout once its session manager is idle and accepts the request.
    machine.wait_until_succeeds(
        command(
            "busctl --user call org.xfce.SessionManager /org/xfce/SessionManager org.xfce.Session.Manager Logout bb false false"
        ),
        timeout=60,
    )
    machine.wait_until_fails(
        "pgrep -u demo -f '(^|/)[.]?xfce4-session(-wrapped)?($| )'", timeout=60
    )
    no_session()

with subtest("GNOME starts after Xfce using the same user manager"):
    login("gnome", "wayland")
    machine.wait_until_succeeds(
        "pgrep -u demo -f '(^|/)[.]?gnome-shell(-wrapped)?($| )'", timeout=120
    )
    active("gnome-session.target")
    machine.wait_until_succeeds(
        command(
            "test \"$(busctl --user call org.gnome.SessionManager /org/gnome/SessionManager org.gnome.SessionManager IsSessionRunning)\" = 'b true'"
        ),
        timeout=60,
    )
    active("graphical-session.target")
    active("aizome-koyomi-agent.service")
    time.sleep(3)
    machine.screenshot("02-gnome-after-xfce")
    user(
        "busctl --user call org.gnome.SessionManager /org/gnome/SessionManager org.gnome.SessionManager Logout u 1"
    )
    machine.wait_until_fails(
        "pgrep -u demo -f '(^|/)[.]?gnome-shell(-wrapped)?($| )'", timeout=60
    )
    no_session()

with subtest("Aizome starts and its normal logout also stops Koyomi"):
    login("aizome", "wayland")
    machine.wait_until_succeeds("pgrep -u demo -x aizome-shell", timeout=120)
    active("aizome-session.target")
    active("aizome-koyomi-agent.service")
    time.sleep(3)
    machine.screenshot("03-aizome-after-gnome")
    user("systemd-run --user --quiet --collect aizomectl quit")
    machine.wait_until_fails("pgrep -u demo -x aizome-shell", timeout=60)
    no_session()

machine.succeed("journalctl _UID=1000 --no-pager > /tmp/koyomi-session.log")
machine.copy_from_machine("/tmp/koyomi-session.log")
assert "A graphical session is already running" not in machine.succeed(
    "cat /tmp/koyomi-session.log"
)
