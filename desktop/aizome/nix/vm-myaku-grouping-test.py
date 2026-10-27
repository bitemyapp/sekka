"""Launch real apps by palette, open Sumi from Hako, inspect/control Myaku groups.

Only pointer and keyboard input changes the applications. IPC, toolkit inspection
and /proc are read-only assertions. No fixture frames or app messages are injected.
"""
import json
import shlex
import time
from pathlib import Path

INSPECT = "/run/user/1000/myaku-inspect"
inputs = []
observations = []


def user(command):
    return machine.succeed("su - alice -c " + shlex.quote(
        "export XDG_RUNTIME_DIR=/run/user/1000; "
        "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
        "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; "
        "export AIZOME_SHELL_SOCKET=/run/user/1000/aizome-shell.$WAYLAND_DISPLAY.sock; "
        + command))


def until(description, check):
    deadline = time.monotonic() + 25
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.2)
    raise AssertionError(description)


def windows():
    return json.loads(user("aizomectl --json windows"))["windows"]


def window(app):
    return next((w for w in windows() if w["app_id"].lower() == app.lower()), None)


def elements(app="org.aizome.Myaku"):
    w = window(app)
    if not w:
        return []
    docs = [json.loads(line) for line in machine.succeed(
        f"for f in {INSPECT}/*.json; do if test -f \"$f\"; then cat \"$f\"; printf '\\n'; fi; done"
    ).splitlines() if line.strip()]
    return [e for d in docs if d.get("app_id") == w["app_id"]
            and d.get("pid") == w["pid"] and d.get("kind") == "window"
            for e in d["elements"] if e["visible"]]


def input_(command):
    inputs.append(command)
    user("aizomectl input " + command)
    time.sleep(0.25)


def click(x, y):
    input_(f"motion {x} {y}")
    input_("click left")


def click_element(e, app="org.aizome.Myaku"):
    g = window(app)["geometry"]
    x, y, width, height = e["rect"]
    click(g["x"] + x + width / 2, g["y"] + y + height / 2)


def focus(app):
    g = window(app)["geometry"]
    click(g["x"] + 35, g["y"] - 12)


def launch(name, app):
    input_("chord super+space")
    until("palette open", lambda: json.loads(user("aizome-shell msg state"))["palette"] is not None)
    input_("type " + shlex.quote(name))
    until("palette query entered", lambda: json.loads(user("aizome-shell msg state"))["palette"] == name)
    input_("key Return")
    until(name + " window", lambda: window(app))
    focus(app)


def text_element(text):
    return next((e for e in elements() if e.get("text") == text), None)


def button(label):
    return next((e for e in elements() if e.get("role") == "button"
                 and (e.get("label") == label or e.get("text") == label)), None)


def choose_group(name):
    input_("chord super+f")
    input_("chord super+a")
    input_("type " + shlex.quote(name))
    # The virtual table paints cells without exposing them as inspection nodes.
    # Filter to one group, then click its first row below the 24px header.
    e = until(name + " single group row", lambda: next(
        (e for e in elements() if e.get("name") == "processes"
         and e.get("value", "").startswith("1 rows,")), None))
    g = window("org.aizome.Myaku")["geometry"]
    click(g["x"] + 60, g["y"] + e["rect"][1] + 36)
    until(name + " inspector heading", lambda: text_element(name))
    until(name + " independent single window", lambda: any(
        e.get("text", "").startswith("Application · 1 window") for e in elements()))
    observations.append({"selected_group": name, "inspector": elements()})


def proc(pid):
    return machine.succeed(f"cat /proc/{pid}/status; cat /proc/{pid}/cgroup")


def alive(pids):
    for pid in pids:
        status = proc(pid)
        assert "State:\tT" not in status and "State:\tZ" not in status, status


machine.start()
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
try:
    # The first-run tour starts asynchronously; let it paint and close it
    # normally before opening a palette, so it cannot take the palette's focus.
    until("first-run tour window", lambda: window("org.aizome.Annai"))
    focus("org.aizome.Annai")
    input_("chord super+q")
    until("tour closed", lambda: window("org.aizome.Annai") is None)
    with subtest("independent shell and Hako launches remain separate applications"):
        launch("Hako", "org.aizome.Hako")
        hako = window("org.aizome.Hako")["pid"]
        input_("chord super+l")
        input_("chord super+a")
        input_("type /home/alice/Grouping.txt")
        input_("key Return")
        until("Hako selects fixture document", lambda: any(
            e.get("text") == "Grouping.txt" for e in elements("org.aizome.Hako")))
        input_("key Return")
        until("Sumi opened by Hako", lambda: window("org.aizome.Sumi"))
        sumi = window("org.aizome.Sumi")["pid"]
        assert f"PPid:\t{hako}\n" in proc(sumi), "Sumi must be Hako's actual child"
        launch("Firefox", "firefox")
        firefox = window("firefox")["pid"]
        launch("Myaku", "org.aizome.Myaku")
        myaku = window("org.aizome.Myaku")["pid"]
        before = {name: proc(pid) for name, pid in
                  [("Hako", hako), ("Sumi", sumi), ("Firefox", firefox), ("Myaku", myaku)]}
        observations.append({"before": before, "windows": windows()})
        # Name sorting makes application rows stable while CPU usage changes.
        g = window("org.aizome.Myaku")["geometry"]
        table = until("process table", lambda: next(
            (e for e in elements() if e.get("name") == "processes"), None))
        click(g["x"] + 35, g["y"] + table["rect"][1] + 12)
        machine.screenshot("01-four-independent-applications")
        input_("chord super+i")
        for name in ["Hako", "Sumi", "Firefox", "Myaku"]:
            choose_group(name)
        assert button("Quit").get("disabled"), "Myaku still protects itself"
        choose_group("Hako")
        assert not button("Quit").get("disabled"), "Hako must not include Myaku"
        machine.screenshot("02-hako-independent-inspector")

    with subtest("browser helpers remain grouped and Sumi alone pauses and resumes"):
        choose_group("Firefox")
        # The original browser and its content processes share one app scope.
        browser_scope = machine.succeed(f"cat /proc/{firefox}/cgroup").strip()
        members = machine.succeed(
            "for p in /proc/[0-9]*; do "
            f"if test \"$(cat $p/cgroup 2>/dev/null)\" = {shlex.quote(browser_scope)}; "
            "then basename $p; fi; done").splitlines()
        assert len(members) > 1, members
        assert not button("Quit").get("disabled")
        table = next(e for e in elements() if e.get("name") == "processes")
        g = window("org.aizome.Myaku")["geometry"]
        click(g["x"] + 14, g["y"] + table["rect"][1] + 36)
        until("browser helper process rows", lambda: any(
            e.get("name") == "processes" and int(e["value"].split()[0]) >= 3
            for e in elements()))
        machine.screenshot("03-firefox-helper-group")
        observations.append({"firefox_scope_members": members})
        choose_group("Sumi")
        assert not button("Pause").get("disabled")
        click_element(button("Pause"))
        until("Sumi stopped", lambda: "State:\tT" in proc(sumi))
        alive([hako, firefox, myaku])
        until("Sumi Resume control", lambda: button("Resume"))
        machine.screenshot("04-sumi-only-paused")
        click_element(button("Resume"))
        until("Sumi resumed", lambda: "State:\tT" not in proc(sumi))
        alive([hako, firefox, myaku])
        machine.screenshot("05-sumi-resumed")

    with subtest("Hako Quit cancel and confirm leave independent apps alive"):
        choose_group("Hako")
        click_element(button("Quit"))
        until("Quit confirmation", lambda: button("Cancel"))
        machine.screenshot("06-hako-quit-confirmation")
        click_element(button("Cancel"))
        alive([hako, sumi, firefox, myaku])
        click_element(button("Quit"))
        until("Quit confirmation again", lambda: button("Cancel"))
        input_("key Return")
        until("only Hako exits", lambda: window("org.aizome.Hako") is None)
        machine.wait_until_fails(f"test -d /proc/{hako}")
        alive([sumi, firefox, myaku])
        machine.screenshot("07-hako-exited-others-survive")
        observations.append({"after": windows()})
    machine.fail("coredumpctl --no-pager list aizome-myaku")
finally:
    machine.screenshot("final")
    Path(machine.out_dir, "inputs.json").write_text(json.dumps(inputs, indent=2))
    Path(machine.out_dir, "observations.json").write_text(json.dumps(observations, indent=2))
    machine.succeed("journalctl _UID=1000 --no-pager > /tmp/myaku-grouping.log")
    machine.copy_from_machine("/tmp/myaku-grouping.log")
    machine.copy_from_machine(INSPECT)
    machine.shutdown()
