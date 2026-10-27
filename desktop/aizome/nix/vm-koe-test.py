"""Koe in a real session: nobody runs it or downloads a model until they ask;
its window offers to set it up, and a download that can't happen leaves
nothing; its switch starts the service now and keeps it starting; ❖⌥D works
it through the compositor; and off stops it, for later starts too.

The VM has no network, so recognition is sent to a server koe.toml names
that isn't there. Only pointer and keyboard input changes the window; the
toolkit's inspection, systemctl and files are read-only assertions.
"""
import json
import shlex
import time

INSPECT = "/run/user/1000/koe-inspect"
APP = "org.aizome.Koe"


def user(command):
    return machine.succeed("su - alice -c " + shlex.quote(
        "export XDG_RUNTIME_DIR=/run/user/1000; "
        "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
        "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; "
        "export AIZOME_SHELL_SOCKET=/run/user/1000/aizome-shell.$WAYLAND_DISPLAY.sock; "
        + command))


def until(description, check, timeout=30):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.2)
    raise AssertionError(description)


def unit(prop):
    return user(f"systemctl --user show -p {prop} --value aizome-koe.service").strip()


def phase():
    out = user("aizome-koe status 2>/dev/null || true")
    return json.loads(out)["status"] if out.strip() else None


def windows():
    return json.loads(user("aizomectl --json windows"))["windows"]


def window(app):
    return next((w for w in windows() if w["app_id"].lower() == app.lower()), None)


def elements(app=APP):
    w = window(app)
    if not w:
        return []
    docs = [json.loads(line) for line in machine.succeed(
        f"for f in {INSPECT}/*.json; do if test -f \"$f\"; then cat \"$f\"; printf '\\n'; fi; done"
    ).splitlines() if line.strip()]
    return [e for d in docs if d.get("app_id") == w["app_id"]
            and d.get("pid") == w["pid"] and d.get("kind") == "window"
            for e in d["elements"] if e["visible"]]


def named(name):
    return next((e for e in elements() if e.get("name") == name), None)


def says(text):
    return any(text in e.get("text", "") for e in elements())


def input_(command):
    user("aizomectl input " + command)
    time.sleep(0.25)


def click_element(e, app=APP):
    g = window(app)["geometry"]
    x, y, width, height = e["rect"]
    input_(f"motion {g['x'] + x + width / 2} {g['y'] + y + height / 2}")
    input_("click left")


def focus(app):
    g = window(app)["geometry"]
    input_(f"motion {g['x'] + 35} {g['y'] - 12}")
    input_("click left")


def launch(name, app):
    input_("chord super+space")
    until("palette open", lambda: json.loads(user("aizome-shell msg state"))["palette"] is not None)
    input_("type " + shlex.quote(name))
    until("palette query entered", lambda: json.loads(user("aizome-shell msg state"))["palette"] == name)
    input_("key Return")
    until(name + " window", lambda: window(app))
    focus(app)


machine.start()
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
# The first-run tour starts by itself: close it, so it can't take the focus.
until("first-run tour window", lambda: window("org.aizome.Annai"))
focus("org.aizome.Annai")
input_("chord super+q")
until("tour closed", lambda: window("org.aizome.Annai") is None)

with subtest("nobody runs Koe or downloads a model until they ask"):
    # The session wanted it, and its condition said no.
    until("condition checked", lambda: unit("ConditionResult") == "no")
    assert unit("ActiveState") == "inactive"
    machine.fail("pgrep -u alice -f 'aizome-koe daemon'")
    machine.fail("test -e /home/alice/.local/share/aizome/koe")

with subtest("the window offers to set up; a download that can't happen leaves nothing"):
    launch("Koe", APP)
    until("set-up offered", lambda: named("set-up"))
    machine.screenshot("01-koe-setting-up")
    click_element(named("set-up"))
    until("the failure said", lambda: says("didn't download"))
    files = machine.succeed("find /home/alice/.local/share/aizome/koe -type f 2>/dev/null | wc -l").strip()
    assert files == "0", files
    assert unit("ActiveState") == "inactive"
    machine.fail("test -e /home/alice/.config/aizome/koe.toml")

with subtest("the switch starts the service now, adds the shortcuts, and keeps it starting"):
    user("mkdir -p ~/.config/aizome && printf '%s\\n' '[engine]' 'backend = \"http\"' "
         "'[engine.http]' 'endpoint = \"http://127.0.0.1:9/inference\"' > ~/.config/aizome/koe.toml")
    until("switch enabled", lambda: (e := named("dictation-switch")) and not e.get("disabled"))
    click_element(named("dictation-switch"))
    until("service active", lambda: unit("ActiveState") == "active")
    until("ready", lambda: says("Press ⌥❖D in any app"))
    assert "enabled = true" in user("cat ~/.config/aizome/koe.toml")
    settings = user("cat ~/.config/aizome/settings.toml")
    assert '"spawn aizome-koe record toggle"' in settings, settings
    machine.screenshot("02-koe-ready")
    # As at the next login: the condition passes.
    user("systemctl --user restart aizome-koe.service")
    until("started again", lambda: unit("ActiveState") == "active" and unit("ConditionResult") == "yes")
    until("window follows the restart", lambda: says("Press ⌥❖D in any app"))

with subtest("❖⌥D works dictation through the compositor"):
    focus(APP)
    input_("chord super+alt+d")
    status = until("dictation started", lambda: (s := phase()) and s["phase"] != "idle" and s)
    # Whatever the VM's audio, the microphone program was found: the unit
    # has PipeWire's tools.
    assert "cannot start pw-record" not in (status.get("error") or ""), status
    if status["phase"] == "recording":
        input_("chord super+alt+d")
    until("dictation finished", lambda: (s := phase()) and s["phase"] in ("idle", "error"), timeout=20)
    machine.fail("pgrep -u alice -x pw-record")

with subtest("off from the window stops it, and it isn't started again"):
    input_("chord super+alt+escape")
    until("switch on", lambda: named("dictation-switch"))
    click_element(named("dictation-switch"))
    until("service stopped", lambda: unit("ActiveState") == "inactive")
    assert "enabled = false" in user("cat ~/.config/aizome/koe.toml")
    user("systemctl --user start aizome-koe.service")
    until("condition said no", lambda: unit("ConditionResult") == "no")
    assert unit("ActiveState") == "inactive"
    until("window says off", lambda: says("Dictation is off"))
    machine.screenshot("03-koe-off")
