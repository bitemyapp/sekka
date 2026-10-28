"""Chrome/IBus regression probe for a disposable Aizome test session.

CDP observes the local page and focuses its editors. All tested keystrokes go
through the compositor, IBus and Wayland; CDP never synthesizes text or keys.
See DEVELOPERS.md#aizome-and-ibus for the upstream context and removal gates.
"""

import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import time
import urllib.error
import urllib.request

from websockets.sync.client import connect

SETTING = "/desktop/ibus/general/use-system-keyboard-layout"
HTML = """<!doctype html><meta charset="utf-8"><title>IBus Chrome regression</title>
<textarea id="plain" style="width:600px;height:160px"></textarea>
<div id="rich" contenteditable="true" style="width:600px;height:160px"></div>
<script>
window.events = [];
for (const type of ['keydown', 'keyup', 'beforeinput', 'input',
                    'compositionstart', 'compositionupdate', 'compositionend']) {
  document.addEventListener(type, e => events.push({type, key: e.key,
    data: e.data, ctrl: e.ctrlKey, shift: e.shiftKey, target: e.target.id}), true);
}
</script>
"""


def until(check, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            result = check()
            if result:
                return result
        except (OSError, ValueError, urllib.error.URLError):
            pass
        time.sleep(0.05)
    raise AssertionError("condition did not become true")


class CDP:
    def __init__(self, url):
        self.ws = connect(url, open_timeout=10)
        self.sequence = 0

    def call(self, method, **params):
        self.sequence += 1
        self.ws.send(json.dumps(dict(id=self.sequence, method=method, params=params)))
        while True:
            reply = json.loads(self.ws.recv(timeout=20))
            if reply.get("id") == self.sequence:
                assert "error" not in reply, reply
                return reply.get("result", {})

    def js(self, expression):
        reply = self.call("Runtime.evaluate", expression=expression, returnByValue=True)
        assert "exceptionDetails" not in reply, reply
        return reply["result"].get("value")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--chrome", required=True)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    # This probe temporarily changes the session's IBus preference and engine.
    assert (
        os.environ.get("SEKKA_IBUS_TEST") == "1"
    ), "requires a disposable test session"
    socket = os.environ["AIZOME_SOCKET"]
    args.output.mkdir(parents=True)
    page = args.output / "page.html"
    page.write_text(HTML)
    profile = args.output / "profile"
    records = []
    browser = None
    client = None

    def command(*argv):
        return subprocess.check_output(argv, text=True, timeout=20).strip()

    def ipc(*argv):
        return json.loads(command("aizomectl", "--json", "--socket", socket, *argv))

    def key(name, action=None):
        ipc("input", "key", name, *([action] if action else []))

    def type_text(value):
        ipc("input", "type", value)

    def record(name, **extra):
        state = client.js("({plain:plain.value,rich:rich.textContent,events:events})")
        result = dict(name=name, **extra, **state)
        records.append(result)
        (args.output / "results.json").write_text(json.dumps(records, indent=2))
        print(
            json.dumps({k: v for k, v in result.items() if k != "events"}), flush=True
        )
        return result

    def clear(editor="plain"):
        key("shift")
        client.js(f'plain.value="";rich.textContent="";events=[];{editor}.focus()')
        until(
            lambda: client.js(
                f'document.hasFocus() && document.activeElement.id === "{editor}"'
            )
        )
        time.sleep(0.2)

    def expect(editor, value):
        expression = "plain.value" if editor == "plain" else "rich.textContent"
        until(lambda: client.js(expression) == value)

    def tab_cycle(hold_control=False):
        key("ctrl", "down")
        key("t")
        until(lambda: len(pages()) == 2)
        if not hold_control:
            key("ctrl", "up")
            key("ctrl", "down")
        key("w")
        until(lambda: len(pages()) == 1 and client.js("document.hasFocus()"))
        key("ctrl", "up")
        # No additional keydown after returning: that would hide the defect.
        time.sleep(0.2)

    assert command("dconf", "read", SETTING) == "true", "Aizome profile default missing"
    try:
        env = os.environ.copy()
        env.pop("NIXOS_OZONE_WL", None)
        env["WAYLAND_DEBUG"] = "client"
        with (args.output / "chrome.log").open("w") as log:
            browser = subprocess.Popen(
                [
                    args.chrome,
                    "--user-data-dir=" + str(profile),
                    "--ozone-platform=wayland",
                    "--enable-wayland-ime",
                    "--remote-debugging-port=0",
                    "--no-first-run",
                    "--no-default-browser-check",
                    "--password-store=basic",
                    "--disable-extensions",
                    "--disable-background-networking",
                    "--disable-component-update",
                    "--disable-sync",
                    "--disable-gpu",
                    page.as_uri(),
                ],
                env=env,
                stdout=log,
                stderr=log,
                start_new_session=True,
            )
        port = until(
            lambda: (profile / "DevToolsActivePort").read_text().splitlines()[0]
        )

        def pages():
            with urllib.request.urlopen(
                "http://127.0.0.1:" + port + "/json", timeout=5
            ) as response:
                return [p for p in json.load(response) if p.get("type") == "page"]

        target = until(
            lambda: next((p for p in pages() if p["url"] == page.as_uri()), None)
        )
        client = CDP(target["webSocketDebuggerUrl"])
        window = until(
            lambda: next(
                (
                    w
                    for w in ipc("windows")["windows"]
                    if "chrome" in w["app_id"].lower()
                ),
                None,
            )
        )
        ipc("focus", str(window["id"]))
        command("ibus", "engine", "xkb:us::eng")
        clear()
        type_text("baseline")
        expect("plain", "baseline")

        # Demonstrate the original failure with the same browser/frontend.
        # A future Chrome fix may make committed text work here; preserve that
        # observation without requiring future versions to retain the bug.
        command("dconf", "write", SETTING, "false")
        time.sleep(0.3)
        clear()
        tab_cycle()
        type_text("abc")
        time.sleep(0.3)
        control = record("committed-text-control")
        assert control["plain"] in ("", "abc"), control
        if not control["plain"]:
            key("shift")
            type_text("z")
            expect("plain", "z")
            record("control-shift-recovery")

        # Reset, rather than write true: exercise the actual session default
        # and demonstrate that a user's false preference is not locked out.
        command("dconf", "reset", SETTING)
        assert command("dconf", "read", SETTING) == "true"
        time.sleep(0.3)
        for editor in ("plain", "rich"):
            for hold_control in (False, True):
                for trial in range(5):
                    clear(editor)
                    tab_cycle(hold_control)
                    type_text("abc")
                    expect(editor, "abc")
                    result = record(
                        "tab-return",
                        editor=editor,
                        hold_control=hold_control,
                        trial=trial,
                    )
                    keydowns = [
                        e["key"]
                        for e in result["events"]
                        if e["type"] == "keydown" and e.get("key") in ("a", "b", "c")
                    ]
                    assert keydowns == ["a", "b", "c"], result

        clear()
        type_text("AbC 42!?")
        expect("plain", "AbC 42!?")
        record("shift-punctuation")
        command("ibus", "engine", "hangul")
        time.sleep(0.3)
        for after_tab in (False, True):
            clear()
            if after_tab:
                tab_cycle()
            for letter in "gksrmf":
                key(letter)
                time.sleep(0.1)
            expect("plain", "한글")
            preedit = record("hangul-preedit", after_tab=after_tab)
            assert any(
                e["type"] == "compositionstart" for e in preedit["events"]
            ), preedit
            for name in ("backspace", "f", "space", "1", "enter"):
                key(name)
                time.sleep(0.1)
            expect("plain", "한글 1\n")
            record("hangul-forwarded-keys", after_tab=after_tab)
    except BaseException:
        if client:
            record("failure")
        raise
    finally:
        if client:
            client.ws.close()
        if browser:
            try:
                os.killpg(browser.pid, signal.SIGTERM)
                browser.wait(timeout=10)
            except ProcessLookupError:
                pass
            except subprocess.TimeoutExpired:
                os.killpg(browser.pid, signal.SIGKILL)
                browser.wait(timeout=10)
        command("dconf", "reset", SETTING)
        command("ibus", "engine", "xkb:us::eng")


if __name__ == "__main__":
    main()
