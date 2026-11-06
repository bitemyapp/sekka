"""What the window's VM tests share (vm-desktop.nix's machine): commands as
alice in her session, and finding and pressing the window's controls from
the files kit apps write under INSPECT, through the compositor's input as
a person would."""
import json
import shlex
import time

INSPECT = "/run/user/1000/inspect"
APP = "org.aizome.Tasuke"


def user(command):
    return machine.succeed("su - alice -c " + shlex.quote(
        "export XDG_RUNTIME_DIR=/run/user/1000; "
        "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
        "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; " + command))


def documents():
    return [json.loads(line) for line in machine.succeed(
        f'for f in {INSPECT}/*.json; do if test -f "$f"; then cat "$f"; printf "\\n"; fi; done'
    ).splitlines() if line.strip()]


def until(description, condition, timeout=60):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = condition()
        if result:
            return result
        time.sleep(0.5)
    machine.screenshot("failed")
    print(json.dumps(documents()))
    raise AssertionError(description)


def input_(command):
    user("aizomectl input " + command)
    time.sleep(0.2)


def tasuke_docs():
    return [d for d in documents() if d.get("app_id") == APP and d.get("kind") == "window"]


def find(match):
    """The last visible, enabled element of the window that `match` takes."""
    windows = {w["pid"]: w for w in json.loads(user("aizomectl --json windows"))["windows"] if w["app_id"] == APP}
    found = []
    for d in tasuke_docs():
        w = windows.get(d.get("pid"))
        if not w:
            continue
        for e in d["elements"]:
            if e["visible"] and not e.get("disabled") and match(e):
                found.append((w, e))
    return found[-1] if found else None


def click(description, match, timeout=60):
    w, e = until(description, lambda: find(match), timeout)
    x, y, width, height = e.get("shown", e["rect"])
    g = w["geometry"]
    input_(f"motion {g['x'] + x + width / 2} {g['y'] + y + height / 2}")
    input_("click")
    return e


def button(label):
    return lambda e: e.get("role") == "button" and e.get("label") == label


def shows(text):
    return lambda: any(text in json.dumps(d) for d in tasuke_docs())


def password_visible():
    pids = {int(p) for p in machine.succeed("pgrep -u alice -x aizome-shell").split()}
    return any(d.get("pid") in pids and any(e.get("name") == "auth-password" and e["visible"] for e in d["elements"])
               for d in documents())


def start_session():
    machine.start()
    machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
    # The tour is unrelated to the tests.
    user("pkill -x aizome-annai || true")
    machine.wait_until_succeeds(
        "test \"$(systemctl --machine=alice@.host --user show -p ActiveState --value tasuke-llm.service)\" = active",
        timeout=60,
    )
    machine.wait_for_open_port(8737)
