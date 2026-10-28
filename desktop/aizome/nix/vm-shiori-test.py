"""Packaged Shiori in the Aizome session: a component regression, not an
ISO qualification. Real keys through the compositor; inspection only reads
what the window painted.
"""

import json
import shlex
import time

APP = "org.aizome.Shiori"
INSPECT = "/run/user/1000/shiori-inspect"
DOC = "/home/alice/Documents/sample.pdf"


def user(command):
    return machine.succeed(
        "su - alice -c "
        + shlex.quote(
            "export XDG_RUNTIME_DIR=/run/user/1000; "
            "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
            "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; "
            + command
        )
    )


def ipc(command):
    return json.loads(user("aizomectl --json " + command))


def until(description, check):
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(0.3)
    machine.screenshot("failed")
    raise AssertionError(description)


def window():
    return next((w for w in ipc("windows")["windows"] if w["app_id"] == APP), None)


def document():
    current = window()
    if current is None:
        return {"elements": []}
    docs = user(
        f"for file in {INSPECT}/{APP}.{current['pid']}.*.json; do "
        'if test -f "$file"; then cat "$file"; printf "\\n"; fi; done'
    ).splitlines()
    return next(
        (json.loads(d) for d in docs if json.loads(d)["kind"] == "window"),
        {"elements": []},
    )


def texts():
    return [e.get("text") or "" for e in document()["elements"] if e["visible"]]


def find(**attributes):
    return next(
        (
            e
            for e in document()["elements"]
            if e["visible"] and all(e.get(k) == v for k, v in attributes.items())
        ),
        None,
    )


def click(**attributes):
    element = until(str(attributes), lambda: find(**attributes))
    x, y, width, height = element["rect"]
    geometry = window()["geometry"]
    user(
        f"aizomectl input motion {geometry['x'] + x + width / 2} "
        f"{geometry['y'] + y + height / 2}"
    )
    user("aizomectl input click")
    time.sleep(0.3)


def key(name):
    user("aizomectl input key " + name)
    time.sleep(0.3)


def chord(keys):
    user("aizomectl input chord " + keys)
    time.sleep(0.3)


def type_text(text):
    user("aizomectl input type " + shlex.quote(text))
    time.sleep(0.3)


def workers():
    return machine.succeed("pgrep -u alice -f -- '--pdfium-worker' || true").split()


machine.start()
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
machine.succeed("install -d -o alice -g users /home/alice/Documents")
machine.succeed("install -o alice -g users -m 644 /etc/shiori-sample.pdf " + DOC)
user("mkdir -p " + INSPECT)
user(f"aizomectl spawn env AIZOME_KIT_INSPECT={INSPECT} aizome-shiori {DOC}")
until("Shiori window", window)
until("the document open", lambda: any(t.startswith("Page 1 of 3") for t in texts()))
first = until("its worker", workers)
machine.screenshot("01-document")

with subtest("the worker draws with the packaged PDFium"):
    machine.succeed(f"grep -q 'pdfium.*/lib/libpdfium.so' /proc/{first[0]}/maps")

with subtest("find goes through the worker"):
    click(name="pages")
    chord("super+f")
    type_text("needle")
    until("two hits", lambda: find(text="1 of 2"))
    machine.screenshot("02-found")
    key("Return")
    until("the second hit", lambda: find(text="2 of 2"))

with subtest("a killed worker is replaced and the window stays"):
    machine.succeed(f"kill -KILL {first[0]}")
    until("another worker", lambda: [p for p in workers() if p != first[0]])
    until("Shiori says so", lambda: any("started it again" in t for t in texts()))
    assert window() is not None
    machine.screenshot("03-worker-replaced")

with subtest("Shiori is the session's PDF reader"):
    machine.succeed(
        "grep -qx 'application/pdf=org.aizome.Shiori.desktop;' /etc/xdg/aizome-mimeapps.list"
    )
    machine.succeed("test -f /run/current-system/sw/share/applications/org.aizome.Shiori.desktop")

machine.succeed("journalctl _UID=1000 --no-pager > /tmp/shiori.log")
machine.copy_from_machine("/tmp/shiori.log")
machine.fail("coredumpctl --no-pager list aizome-shiori")
