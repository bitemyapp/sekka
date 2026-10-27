"""Real GUI authorization across system activation and shell replacement."""
import json
import shlex
import time

INSPECT = "/run/user/1000/auth-inspect"
APP = "org.aizome.Yukimi"


def user(command):
    return machine.succeed("su - alice -c " + shlex.quote(
        "export XDG_RUNTIME_DIR=/run/user/1000; "
        "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
        "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; " + command))


def shell_pid():
    pids = machine.succeed("pgrep -u alice -x aizome-shell").split()
    return pids[0] if len(pids) == 1 else None


def until(description, condition):
    deadline = time.monotonic() + 40
    while time.monotonic() < deadline:
        result = condition()
        if result:
            return result
        time.sleep(0.2)
    machine.screenshot("failed")
    print(json.dumps(documents()))
    raise AssertionError(description)


def documents():
    return [json.loads(line) for line in machine.succeed(
        f'for f in {INSPECT}/*.json; do if test -f "$f"; then cat "$f"; printf "\\n"; fi; done'
    ).splitlines() if line.strip()]


def input_(command):
    user("aizomectl input " + command)
    time.sleep(0.2)


def find(label):
    windows = json.loads(user("aizomectl --json windows"))["windows"]
    matches = []
    for w in windows:
        if w["app_id"] != APP:
            continue
        for d in documents():
            if d.get("pid") != w["pid"] or d.get("kind") != "window":
                continue
            for e in d["elements"]:
                if e["visible"] and not e.get("disabled") and e.get("label") == label and e.get("role") == "button":
                    matches.append((w, e))
    return matches[-1] if matches else None


def click(label):
    w, e = until(label, lambda: find(label))
    x, y, width, height = e.get("shown", e["rect"])
    g = w["geometry"]
    input_(f"motion {g['x'] + x + width / 2} {g['y'] + y + height / 2}")
    input_("click")


def password_dialog():
    pids = {int(pid) for pid in machine.succeed("pgrep -u alice -x aizome-shell").split()}
    return next((d["window"] for d in documents() if d.get("pid") in pids
                 and any(e.get("name") == "auth-password" and e["visible"] for e in d["elements"])), None)


def password_visible():
    return password_dialog() is not None

def authenticate():
    until("administrator password dialog", password_visible)
    input_("type test-only")
    input_("key Return")
    until("password dialog closes", lambda: not password_visible())


def propose(target):
    until("previous job sheet closes", lambda: not find("Stop") and not find("Close"))
    user(f"aizomectl spawn aizome-yukimi --choose qa.install={target}")
    click("Apply…")
    click("Apply")


machine.start()
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
# The tour is unrelated to the test; dismiss it before driving Yukimi.
user("pkill -x aizome-annai || true")
shell = machine.succeed("pgrep -u alice -x aizome-shell").strip()
polkit = machine.succeed("systemctl show polkit -p MainPID --value").strip()
propose("first")
authenticate()
machine.wait_until_succeeds("test -x /run/current-system/sw/bin/qa-first-package")
until("polkit restarted by the system package change", lambda:
      machine.succeed("systemctl show polkit -p MainPID --value").strip() not in ["0", polkit])
assert machine.succeed("pgrep -u alice -x aizome-shell").strip() == shell
# A GUI launch must not have acquired a terminal for authentication.
yukimi_pid = machine.succeed("pgrep -u alice -x aizome-yukimi").strip()
assert machine.succeed(f"awk '{{print $7}}' /proc/{yukimi_pid}/stat").strip() == "0"
# The second install must work in this same shell and Yukimi session.
propose("second")
authenticate()
machine.wait_until_succeeds("test -x /run/current-system/sw/bin/qa-second-package")
assert machine.succeed("cat /var/lib/qa-helper-starts").splitlines() == ["first", "second"]
assert machine.succeed("pgrep -u alice -x aizome-shell").strip() == shell
machine.screenshot("two-installs-one-session")
# Shell handover overlaps agents: the replacement must retry registration.
user("aizomectl reload --shell")
until("replacement shell finishes handover", lambda: shell_pid() not in (None, shell))
# Ask again, then restart the daemon while the password dialog is open.
propose("first")
old_dialog = until("password before daemon restart", password_dialog)
machine.succeed("systemctl restart polkit")
until("obsolete password dialog closes", lambda: password_dialog() != old_dialog)
# A retry may already have opened a fresh dialog; dismiss it before the
# sustained-outage case. No request has reached the helper.
until("fresh prompt or completed authorization failure", lambda: password_visible() or find("Close"))
if password_visible():
    input_("key Escape")
assert machine.succeed("cat /var/lib/qa-helper-starts").splitlines() == ["first", "second"]
machine.screenshot("interrupted-authentication")

# Pause the agent's process while restarting polkit: the independent Yukimi
# and compositor remain usable, but no agent can register until resumed.
# This also avoids an /etc-vs-/run systemd masking precedence dependency.
click("Close")
paused_shell = until("one active shell", shell_pid)
machine.succeed(f"kill -STOP {paused_shell}")
machine.succeed("systemctl restart polkit")
propose("first")
until("recoverable authentication failure", lambda: find("Try Again"))
text = json.dumps(documents())
assert "Nothing was changed" in text
assert "was put back" not in text
assert "/dev/tty" not in text
assert machine.succeed("cat /var/lib/qa-helper-starts").splitlines() == ["first", "second"]
machine.screenshot("authentication-unavailable")
machine.succeed(f"kill -CONT {paused_shell}")
click("Try Again")
authenticate()
machine.wait_until_succeeds("test $(wc -l < /var/lib/qa-helper-starts) = 3")
until("retried operation succeeds and its sheet closes", lambda: not find("Stop") and not find("Close"))
machine.succeed("test ! -x /run/current-system/sw/bin/qa-second-package")
assert machine.succeed("cat /var/lib/qa-helper-starts").splitlines() == ["first", "second", "first"]
machine.screenshot("authentication-recovered")
