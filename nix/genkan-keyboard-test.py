# The script of nix/genkan-keyboard-test.nix. PREVIEW (Genkan's debug build)
# and MAGICK come from the Nix file. Keys pressed with aizome.send_key come
# from QEMU's keyboard, through libinput, as a person's would.
import json
import shlex
import struct
import subprocess
import time

RUN = "/run/user/1000"
INSPECT = RUN + "/inspect"
GENKAN = "org.calamares.NixOSRust"
WIDTH, HEIGHT = 2880, 1800


def user(command):
    """Run a command as demo, in demo's session (its bus, its compositor)."""
    status, output = aizome.execute(
        "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
        "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus sh -c "
        + shlex.quote("export AIZOME_SOCKET=$(ls /run/user/1000/aizome.*.sock | head -1); { " + command + "; } 2>&1")
    )
    if status != 0:
        raise Exception(f"{command!r} failed with {status}:\n{output}")
    return output


def ctl(args):
    return user("aizomectl " + args)


def until(what, check, timeout=60, every=0.5):
    """Wait for check() to return something truthy, and return it."""
    deadline = time.monotonic() + timeout
    last = None
    while True:
        try:
            last = check()
        except Exception as e:  # noqa: BLE001 (a read racing a write)
            last = e
        if last and not isinstance(last, Exception):
            return last
        if time.monotonic() > deadline:
            aizome.screenshot("failed-" + "".join(c if c.isalnum() else "-" for c in what)[:60])
            raise AssertionError(f"timed out waiting for {what}: {last!r}")
        time.sleep(every)


def windows():
    return json.loads(ctl("--json windows")).get("windows", [])


def window(app_id, pid=None):
    for w in windows():
        if w.get("app_id") == app_id and (pid is None or w.get("pid") == pid) and w.get("parent") is None:
            return w
    return None


def launch(argv, app_id):
    """Spawn through the compositor, as the Toolchest does, and wait for the window."""
    before = {w["id"] for w in windows()}
    ctl("spawn -- " + argv)
    return until(
        f"a {app_id} window",
        lambda: next((w for w in windows() if w.get("app_id") == app_id and w["id"] not in before), None),
        timeout=120,
    )


def close(w):
    ctl(f"close {w['id']}")
    until(f"window {w['id']} to close", lambda: all(o["id"] != w["id"] for o in windows()), timeout=30)


def descendants(pid):
    """Processes under pid, by command name."""
    out = aizome.succeed("ps -e -o pid=,ppid=,comm=")
    rows = [line.split(None, 2) for line in out.splitlines() if line.strip()]
    children = {}
    for p, pp, comm in rows:
        children.setdefault(int(pp), []).append((int(p), comm.strip()))
    found, todo = [], [pid]
    while todo:
        for p, comm in children.get(todo.pop(), []):
            found.append(comm)
            todo.append(p)
    return found


class Genkan:
    """Genkan's preview, as the kit's inspection files describe its window."""

    def __init__(self, page):
        user(f"mkdir -p {INSPECT} && rm -f {INSPECT}/{GENKAN}.*")
        self.w = launch(f"env AIZOME_KIT_INSPECT={INSPECT} CALAMARES_UI_PREVIEW={page} {PREVIEW}", GENKAN)
        self.pid = self.w["pid"]
        until("Genkan's window to be inspected", self.doc)
        time.sleep(2)

    def docs(self):
        out = aizome.succeed(
            f'for f in {INSPECT}/{GENKAN}.{self.pid}.*.json; do [ -f "$f" ] && cat "$f" && echo; done; true'
        )
        return [json.loads(line) for line in out.splitlines() if line.strip()]

    def doc(self, kind="window"):
        return next((d for d in self.docs() if d["kind"] == kind), None)

    def find(self, text=None, name=None, contains=None, label=None):
        d = self.doc()
        for e in d["elements"] if d else []:
            if not e["visible"]:
                continue
            if text is not None and e.get("text") != text:
                continue
            if name is not None and e.get("name") != name:
                continue
            if label is not None and e.get("label") != label:
                continue
            if contains is not None and contains not in (e.get("text") or e.get("label") or e.get("value") or ""):
                continue
            return e
        return None

    def wait(self, what, **kw):
        return until(what, lambda: self.find(**kw))

    def focus(self):
        w = window(GENKAN, self.pid)
        if not w["focused"]:
            ctl(f"focus {w['id']}")
            until("Genkan to be focused", lambda: window(GENKAN, self.pid)["focused"])

    def click(self, e):
        self.focus()
        g = window(GENKAN, self.pid)["geometry"]
        x, y, w, h = e["shown"] if e["visible"] else e["rect"]
        ctl(f"input motion {g['x'] + x + w / 2:.1f} {g['y'] + y + h / 2:.1f} && aizomectl input click left")
        time.sleep(0.5)

    def choose(self, name, item):
        """Open a pop-up button and pick an item by typing its name."""
        self.click(self.wait(f"the {name} pop-up", name=name))
        until("the menu to open", lambda: self.doc("menu"), timeout=10)
        ctl("input type " + shlex.quote(item))
        ctl("input key return")
        until("the menu to close", lambda: self.doc("menu") is None)
        time.sleep(0.5)

    def capture(self, name):
        """The window with its tab and frame, cropped from the display at its own pixels."""
        time.sleep(2)
        ctl("input motion 20 860")
        time.sleep(1)
        aizome.screenshot(name + "-display")
        path = aizome.out_dir / (name + "-display.png")
        with open(path, "rb") as f:
            size = struct.unpack(">II", f.read(24)[16:24])
        assert size == (WIDTH, HEIGHT), f"{name} is {size}"
        g = window(GENKAN, self.pid)["geometry"]
        x, y, w, h = (round(v * 2) for v in (g["x"] - 2, g["y"] - 24, g["w"] + 8, g["h"] + 30))
        subprocess.run(
            [MAGICK, str(path), "-crop", f"{w}x{h}+{x}+{y}", "+repage", "-strip",
             "-define", "png:exclude-chunks=date,time", "PNG24:" + str(aizome.out_dir / (name + ".png"))],
            check=True,
        )
        print(f"{name}.png: {w} × {h} at {x},{y} of {name}-display.png")

    def quit(self):
        close(window(GENKAN, self.pid))


aizome.start()
aizome.wait_until_succeeds("ls /run/user/1000/aizome.*.sock", timeout=240)
aizome.wait_until_succeeds("pgrep -u demo -x aizome-shell", timeout=60)
ctl("output Virtual-1 --scale 2")
time.sleep(3)
# Annai's welcome greets a new user; close it as they would.
time.sleep(6)
for w in windows():
    if w.get("app_id") == "org.aizome.Annai":
        close(w)

with subtest("Aizome's session starts from the system's keyboard"):
    env = user("systemctl --user show-environment")
    assert "XKB_DEFAULT_OPTIONS=altwin:swap_alt_win" in env, env
    assert "XKB_DEFAULT_LAYOUT=us" in env, env
    # The compositor lists the keyboards that type as libinput adds them.
    aizome.send_key("shift")
    keyboards = until("a keyboard that types", lambda: json.loads(ctl("--json keyboards"))["keyboards"])
    print("keyboards:", keyboards)

with subtest("Hotaru and foot open fish, which came with Aizome"):
    assert "fish" not in aizome.succeed("cat /etc/installer-applications.json"), "fish isn't an application"
    for argv, app_id in [("aizome-hotaru", "org.aizome.Hotaru"), ("foot", "foot")]:
        w = launch(argv, app_id)
        shells = until(f"{app_id}'s shell", lambda: [c for c in descendants(w["pid"]) if c in ("fish", "bash")])
        print(app_id, "runs", shells)
        assert shells == ["fish"], f"{app_id} runs {shells}"
        close(w)

with subtest("❖A selects all in Genkan's fields: QEMU's Alt, beside the space bar, is ❖"):
    g = Genkan("account")
    g.click(g.wait("the computer name field", name="hostname"))
    aizome.send_key("alt-a")
    ctl("input type rust-test")
    until("❖A to select the sample name", lambda: (g.find(name="hostname") or {}).get("value") == "rust-test")
    # Settings that clear the options undo the system's swap (the compositor
    # must not let libxkbcommon put XKB_DEFAULT_OPTIONS back), as in the live
    # session, which has none: ❖ is the key marked with it.
    user(
        "mkdir -p ~/.config/aizome && "
        "printf '[input.keyboard]\\nlayouts = [\"us\"]\\noptions = \"\"\\n' > ~/.config/aizome/settings.toml"
    )
    ctl("reload-config")
    time.sleep(2)
    g.click(g.find(name="hostname"))
    aizome.send_key("meta_l-a")
    ctl("input type live-session")
    until("QEMU's ❖ and A to select all", lambda: (g.find(name="hostname") or {}).get("value") == "live-session")
    user("rm ~/.config/aizome/settings.toml")
    ctl("reload-config")
    time.sleep(2)
    g.quit()

with subtest("The keyboard step hears the key beside the space bar"):
    g = Genkan("keyboard")
    # The preview starts with its sample ThinkPad; press again, as a person would.
    assert g.find(text="AT Translated Set 2 keyboard")
    g.click(g.wait("Press Again", name="listen"))
    g.wait("the step to wait for the key", text="Waiting for the key… Esc stops waiting.")
    g.capture("genkan-keyboard-waiting")
    aizome.send_key("spc")
    g.wait("the space bar to be refused", text="That's the space bar: press the key just to its left.")
    aizome.send_key("alt")
    g.wait("Alt beside the space bar", text="Beside the space bar: ⌥ Alt (Option)")
    names = [k["name"] for k in keyboards]
    assert any(g.find(text=n) for n in names), f"none of {names} is shown"
    assert not g.find(text="AT Translated Set 2 keyboard"), "still the sample's keyboard"
    swap = lambda: g.find(name="swap-alt-super") or {}
    assert swap().get("checked") is True, swap()
    g.wait("the swap recommended", text="✓ Recommended: Genkan swaps Alt and Super on the installed system.")
    g.capture("genkan-keyboard")
    # Declined, the keys stay as they are.
    g.click(swap())
    until("the swap declined", lambda: swap().get("checked") is False)
    g.wait("the keys kept", text="Kept as they are: ❖ stays where it is now, not beside the space bar.")
    g.capture("genkan-keyboard-kept")
    # Esc stops waiting and leaves the question open.
    g.click(g.find(name="listen"))
    g.wait("waiting again", text="Waiting for the key… Esc stops waiting.")
    aizome.send_key("esc")
    g.wait("Esc to stop waiting", text="Genkan listens for it and suggests where ❖ should go.")
    # Control there: Genkan swaps only Alt and Super, says so, and leaves the
    # keys as they are, even after the swap was taken.
    g.click(swap())
    until("the swap taken again", lambda: swap().get("checked") is True)
    g.click(g.find(name="listen"))
    g.wait("waiting for Control", text="Waiting for the key… Esc stops waiting.")
    aizome.send_key("ctrl")
    g.wait("Control beside the space bar", text="Beside the space bar: ⌃ Control")
    g.wait("the keys left as they are", contains="Genkan swaps only Alt and Super, so your keys stay as they are.")
    until("no swap for Control", lambda: swap().get("checked") is False)
    assert not g.find(contains="Control and Super"), "Genkan offered a Control swap"
    g.capture("genkan-keyboard-control")
    g.quit()

aizome.shutdown()
