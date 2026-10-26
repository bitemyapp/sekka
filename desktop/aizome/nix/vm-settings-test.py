# The script of nix/vm-settings-test.nix: drive Settings the way a person
# would and check each change reached the system. WIFI_PASSPHRASE comes
# from the Nix file. AIZOME_TEST_ONLY=appearance,sound runs only the parts
# whose titles hold those words (when the driver runs outside the sandbox).
import json
import os
import re
import shlex
import time

RUN = "/run/user/1000"
INSPECT = RUN + "/inspect"
APP = "org.aizome.Settei"
HOME = "/home/alice"


def alice(command, check=True):
    """Run a command as alice in her session (its bus, its compositor)."""
    script = (
        "export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus DCONF_PROFILE=aizome; "
        "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
        "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; " + command
    )
    cmd = "su - alice -c " + shlex.quote(script)
    if check:
        return machine.succeed(cmd)
    return machine.execute(cmd)


def ipc(request):
    return json.loads(alice("aizomectl --json " + request))


def setting(path):
    """A value from alice's settings.toml, by its dotted path, or None."""
    script = (
        "import tomllib, json, sys\n"
        "try:\n    d = tomllib.load(open(sys.argv[1], 'rb'))\nexcept FileNotFoundError:\n    d = {}\n"
        "for k in sys.argv[2].split('.'):\n    d = d.get(k) if isinstance(d, dict) else None\n"
        "print(json.dumps(d))"
    )
    out = machine.succeed(f"python3 -c {shlex.quote(script)} {HOME}/.config/aizome/settings.toml {shlex.quote(path)}")
    return json.loads(out)


def settings_toml():
    return machine.succeed(f"cat {HOME}/.config/aizome/settings.toml 2>/dev/null || true")


def shot(name):
    machine.screenshot(name)


def until(what, check, timeout=20, every=0.5):
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
            shot("failed-" + "".join(c if c.isalnum() else "-" for c in what)[:60])
            raise AssertionError(f"timed out waiting for {what}: {last!r}")
        time.sleep(every)


# Input through the compositor, as a person's.


def move(x, y):
    alice(f"aizomectl input motion {x:.1f} {y:.1f}")


def click_at(x, y, button="left", times=1):
    clicks = " && ".join([f"aizomectl input click {button}"] * times)
    alice(f"aizomectl input motion {x:.1f} {y:.1f} && {clicks}")


def key(name):
    alice(f"aizomectl input key {name}")


def chord(keys):
    alice(f"aizomectl input chord {keys}")


def type_text(text):
    alice("aizomectl input type " + shlex.quote(text))


def select_all_and_type(text):
    chord("super+a")
    type_text(text)


class Settings:
    """Settings (Settei), as the kit's inspection files describe it."""

    CONTROLS = [
        "switch",
        "checkbox",
        "slider",
        "combobox",
        "button",
        "radiobutton",
        "textinput",
        "searchinput",
        "passwordinput",
        "spinbutton",
        "tab",
    ]

    def __init__(self):
        self.pid = None

    def running(self):
        return machine.execute("pgrep -u alice -x aizome-settei")[0] == 0

    def open(self, pane):
        """Open Settings at a pane, as the palette or a systray panel does."""
        assert not self.running(), "Settings is already open"
        alice(f"mkdir -p {INSPECT} && rm -f {INSPECT}/{APP}.*")
        alice(f"aizomectl spawn env AIZOME_KIT_INSPECT={INSPECT} aizome-settei --pane {pane}")
        self.pid = int(
            until("Settings to start", lambda: machine.succeed("pgrep -u alice -x aizome-settei || true").strip())
        )
        until("Settings' window", lambda: self.window() and self.doc())
        # Its first frames settle while the pane reads the system.
        time.sleep(1.5)
        return self

    def quit(self):
        """Quit with ❖Q, as a person would, and wait for it to go."""
        self.focus()
        chord("super+q")
        until("Settings to quit", lambda: not self.running(), timeout=15)
        self.pid = None

    def focus(self):
        w = self.window()
        assert w, "no Settings window"
        if not w["focused"]:
            ipc(f"focus {w['id']}")
            until("Settings to be focused", lambda: self.window()["focused"])

    def window(self):
        for w in ipc("windows")["windows"]:
            if w["app_id"] == APP and w["pid"] == self.pid and w.get("parent") is None:
                return w
        return None

    def docs(self):
        out = machine.succeed(
            f'for f in {INSPECT}/{APP}.{self.pid}.*.json; do [ -f "$f" ] && cat "$f" && echo; done; true'
        )
        return [json.loads(line) for line in out.splitlines() if line.strip()]

    def doc(self, kind="window"):
        for d in self.docs():
            if d["kind"] == kind:
                return d
        return None

    def elements(self):
        d = self.doc()
        return d["elements"] if d else []

    # Finding.

    @staticmethod
    def matches(e, text=None, name=None, role=None, label=None, contains=None, inside=None, visible=True):
        if visible and not e["visible"]:
            return False
        if text is not None and e.get("text") != text:
            return False
        if contains is not None and contains not in (e.get("text") or e.get("label") or e.get("value") or ""):
            return False
        if name is not None and e.get("name") != name:
            return False
        if role is not None and e.get("role") not in (role if isinstance(role, (list, tuple)) else [role]):
            return False
        if label is not None and e.get("label") != label:
            return False
        if inside is not None:
            x, y, w, h = e["rect"]
            X, Y, W, H = inside["rect"]
            if not (x >= X - 0.5 and y >= Y - 0.5 and x + w <= X + W + 0.5 and y + h <= Y + H + 0.5):
                return False
        return True

    def all(self, **kw):
        return [e for e in self.elements() if self.matches(e, **kw)]

    def find(self, **kw):
        found = self.all(**kw)
        return found[0] if found else None

    def wait(self, what=None, timeout=20, **kw):
        return until(what or f"an element {kw}", lambda: self.find(**kw), timeout=timeout)

    def gone(self, what=None, timeout=20, **kw):
        until(what or f"no element {kw}", lambda: self.doc() is not None and self.find(**kw) is None, timeout=timeout)

    def row(self, pane, entry):
        """An option's row (form rows are named entry:<pane>.<entry>)."""
        return self.find(name=f"entry:{pane}.{entry}", visible=False)

    def control(self, pane, entry, role=None, **kw):
        """The control in an option's row, scrolled into view."""
        self.reveal(name=f"entry:{pane}.{entry}")
        row = self.row(pane, entry)
        assert row, f"no row {pane}.{entry}"
        found = self.find(inside=row, role=role or self.CONTROLS, **kw)
        if not found:
            inside = [e for e in self.elements() if self.matches(e, inside=row, visible=False)]
            raise AssertionError(f"no control in row {pane}.{entry}: {inside}")
        return found

    def texts(self, inside=None):
        return [
            e["text"]
            for e in self.elements()
            if e.get("text") and (inside is None or self.matches(e, inside=inside, visible=False))
        ]

    # Looking around: wheel the pane until something shows whole.

    def reveal(self, tries=30, **kw):
        for _ in range(tries):
            e = until(f"something like {kw}", lambda: self.find(visible=False, **kw), timeout=10)
            x, y, w, h = e["rect"]
            sx, sy, sw, sh = e["shown"]
            if e["visible"] and sh >= min(h, 30) - 0.5 and sw > 0:
                return e
            d = self.doc()
            height = d["size"][1]
            win = self.window()["geometry"]
            # Over the pane (right of the 232pt sidebar), wheel towards it.
            px = win["x"] + 232 + (win["w"] - 232) / 2
            py = win["y"] + win["h"] / 2
            dy = 60 if y > height / 2 else -60
            alice(f"aizomectl input motion {px:.1f} {py:.1f} && aizomectl input scroll 0 {dy}")
            time.sleep(0.4)
        raise AssertionError(f"couldn't scroll {kw} into view: {e}, the window {self.doc()['size']}")

    # Acting.

    def point(self, e):
        win = self.window()["geometry"]
        x, y, w, h = e["shown"] if e["visible"] else e["rect"]
        return win["x"] + x + w / 2, win["y"] + y + h / 2

    def click(self, e=None, button="left", times=1, **kw):
        if e is None:
            self.reveal(**kw)
            e = self.wait(**kw)
        self.focus()
        x, y = self.point(e)
        click_at(x, y, button, times)
        time.sleep(0.5)
        return e

    def press(self, label, **kw):
        """Click the button (or card, or row) a person knows by this name."""
        return self.click(self.button(label, **kw))

    def button(self, label, timeout=30, **kw):
        """A button (or card, or row) by the name a person knows it by, in
        view (waiting for it to show)."""
        wants = ({"role": "button", "label": label}, {"label": label}, {"text": label})
        want = until(
            f"a button {label!r}",
            lambda: next((w for w in wants if self.find(visible=False, **w, **kw)), None),
            timeout=timeout,
        )
        return self.reveal(**want, **kw)

    def slide(self, e, fraction):
        """Drag a slider's knob to a fraction of its track."""
        self.focus()
        win = self.window()["geometry"]
        x, y, w, h = e["rect"]
        lo, hi, cur = e["range"]
        start = win["x"] + x + min(max(w * ((cur - lo) / (hi - lo) if hi > lo else 0), 1), w - 1)
        to = win["x"] + x + min(max(w * fraction, 1), w - 1)
        cy = win["y"] + y + h / 2
        alice(
            f"aizomectl input motion {start:.1f} {cy:.1f} && aizomectl input button left down && "
            f"aizomectl input motion {(start + to) / 2:.1f} {cy:.1f} && aizomectl input motion {to:.1f} {cy:.1f} && "
            "aizomectl input button left up"
        )
        time.sleep(0.6)

    def choose(self, popup, item):
        """Open a pop-up button and pick an item by typing its name."""
        for _ in range(3):
            self.click(popup)
            try:
                until("the menu to open", lambda: self.doc("menu"), timeout=4)
                break
            except AssertionError:
                # A click while a sheet went away; again.
                popup = self.find(role="combobox", visible=False, inside=None) and popup
        else:
            raise AssertionError(f"{popup} opened no menu")
        type_text(item)
        key("return")
        until("the menu to close", lambda: self.doc("menu") is None)
        time.sleep(0.5)

    def field(self, e, text, submit=True):
        """Replace a text field's text (and press Return)."""
        self.click(e)
        chord("super+a")
        type_text(text)
        if submit:
            key("return")
        time.sleep(0.6)

    def beside(self, near, role, **kw):
        """The control on the same line as some text (a row without an entry)."""
        t = self.reveal(text=near)
        cy = t["rect"][1] + t["rect"][3] / 2
        best = None
        for e in self.all(role=role, visible=False, **kw):
            ey = e["rect"][1] + e["rect"][3] / 2
            if e["rect"][0] > t["rect"][0] and abs(ey - cy) < 16:
                if best is None or abs(ey - cy) < abs(best["rect"][1] + best["rect"][3] / 2 - cy):
                    best = e
        assert best, f"no {role} beside {near!r}"
        return best

    def segment(self, control, name):
        """Click a segment of a segmented control by its name."""
        for p in control.get("parts", []):
            if p["name"] == name:
                win = self.window()["geometry"]
                x, y, w, h = p["rect"]
                self.focus()
                click_at(win["x"] + x + w / 2, win["y"] + y + h / 2)
                time.sleep(0.5)
                return
        raise AssertionError(f"no segment {name!r} in {control}")

    def checked(self, pane, entry, role="switch"):
        return self.control(pane, entry, role=role).get("checked")


S = Settings()


def shell_docs():
    pid = machine.succeed("pgrep -u alice -x aizome-shell").split()[0]
    out = machine.succeed(
        f'for f in {INSPECT}/org.aizome.Shell.{pid}.*.json; do [ -f "$f" ] && cat "$f" && echo; done; true'
    )
    return [json.loads(line) for line in out.splitlines() if line.strip()]


def shell_texts():
    return [e["text"] for d in shell_docs() for e in d["elements"] if e.get("text") and e["visible"]]


def shell_surface(holding):
    """The texts of the shell's surface that shows `holding` (the bar: Toolchest)."""
    for d in shell_docs():
        texts = [e["text"] for e in d["elements"] if e.get("text") and e["visible"]]
        if holding in texts:
            return texts
    return []


def locked():
    """The shell's lock screen shows (its password field)."""
    return any(e.get("role") == "passwordinput" for d in shell_docs() for e in d["elements"])


def shell_labels():
    return [e["label"] for d in shell_docs() for e in d["elements"] if e.get("label") and e["visible"]]


TESTS = []


def part(title):
    def add(f):
        TESTS.append((title, f))
        return f

    return add


def reopen(pane):
    S.quit()
    return S.open(pane)


# Appearance.


@part("Appearance: Day and the rose focus colour reach the config, dconf and GTK; Night comes back")
def appearance():
    S.open("appearance")
    S.press("Day")
    until("theme = day", lambda: setting("appearance.theme") == "day")
    S.press("Rose")
    until("focus = rose", lambda: setting("appearance.focus") == "rose")
    # GTK apps: libadwaita reads color-scheme (and the portal relays it),
    # GTK 3 the theme's name.
    until(
        "dconf to say light",
        lambda: "prefer-light" in alice("dconf read /org/gnome/desktop/interface/color-scheme"),
    )
    portal = alice(
        "gdbus call --session --dest org.freedesktop.portal.Desktop --object-path /org/freedesktop/portal/desktop "
        "--method org.freedesktop.portal.Settings.ReadOne org.freedesktop.appearance color-scheme"
    )
    assert "uint32 2" in portal, f"the portal doesn't say light: {portal}"
    gtk = alice("dconf read /org/gnome/desktop/interface/gtk-theme")
    assert "Adwaita" in gtk and "dark" not in gtk, f"GTK 3 stays dark: {gtk}"
    accent = alice("dconf read /org/gnome/desktop/interface/accent-color")
    assert "pink" in accent, f"no accent for GTK: {accent!r}"
    shot("appearance-day")
    reopen("appearance")
    day = S.button("Day")
    assert day.get("selected") or day.get("checked"), f"Day isn't shown chosen: {day}"
    rose = S.button("Rose")
    assert rose.get("selected") or rose.get("checked"), f"Rose isn't shown chosen: {rose}"
    S.press("Night")
    until("theme = night", lambda: setting("appearance.theme") == "night")
    until("dconf to say dark", lambda: "prefer-dark" in alice("dconf read /org/gnome/desktop/interface/color-scheme"))
    S.quit()


def sun_times(lat, lon, day_of_year, utc_offset):
    """Sunrise and sunset in local minutes (NOAA's approximation, as
    aizome-config reckons them), or None where the sun doesn't rise or set."""
    import math

    g = 2 * math.pi / 365 * (day_of_year - 1)
    eqtime = 229.18 * (
        0.000075 + 0.001868 * math.cos(g) - 0.032077 * math.sin(g) - 0.014615 * math.cos(2 * g) - 0.040849 * math.sin(2 * g)
    )
    decl = (
        0.006918
        - 0.399912 * math.cos(g)
        + 0.070257 * math.sin(g)
        - 0.006758 * math.cos(2 * g)
        + 0.000907 * math.sin(2 * g)
        - 0.002697 * math.cos(3 * g)
        + 0.00148 * math.sin(3 * g)
    )
    la = math.radians(lat)
    cos_ha = math.cos(math.radians(90.833)) / (math.cos(la) * math.cos(decl)) - math.tan(la) * math.tan(decl)
    if abs(cos_ha) > 1:
        return None
    ha = math.degrees(math.acos(cos_ha))

    def wrap(m):
        return int(math.floor(m + utc_offset + 0.5)) % 1440

    return wrap(720 - 4 * (lon + ha) - eqtime), wrap(720 - 4 * (lon - ha) - eqtime)


def where_and_when():
    """The VM's time zone, the place sunrise and sunset are reckoned for
    (GeoClue's, if the shell kept one, else the zone's city from tzdata),
    and the local day of the year, UTC offset and minutes past midnight."""
    zone = localtime_zone()
    doy, off, h, m = machine.succeed("date '+%j %z %H %M'").split()
    sign = -1 if off.startswith("-") else 1
    offset = sign * (int(off[1:3]) * 60 + int(off[3:5]))
    lat, lon = setting("location.latitude"), setting("location.longitude")
    # As aizome-config holds it: only while the zone is the one it was found in.
    if lat is not None and lon is not None and (zone == "" or setting("location.zone") in (None, zone)):
        return zone, (lat, lon), "where you are", int(doy), offset, int(h) * 60 + int(m)
    assert "/" in zone, f"no city's time zone here: {zone!r}"
    line = machine.succeed(f"grep -P '\\t{zone}(\\t|$)' /etc/zoneinfo/zone1970.tab | head -1").split("\t")
    coords = line[1]
    split = max(coords.rfind("+"), coords.rfind("-"))

    def angle(t, d):
        digits = t[1:]
        v = int(digits[:d]) + int(digits[d : d + 2]) / 60 + (int(digits[d + 2 : d + 4]) / 3600 if len(digits) > d + 2 else 0)
        return -v if t[0] == "-" else v

    city = zone.split("/")[-1].replace("_", " ")
    return zone, (angle(coords[:split], 2), angle(coords[split:], 3)), f"in {city}", int(doy), offset, int(h) * 60 + int(m)


def localtime_zone():
    """The zone /etc/localtime names, as Aizome reads it ("" for none or UTC)."""
    status, link = machine.execute("readlink /etc/localtime")
    link = link.strip() if status == 0 else ""
    return link.rsplit("zoneinfo/", 1)[-1] if "zoneinfo/" in link else ""


def near_minutes(a, b, slack=2):
    return min((a - b) % 1440, (b - a) % 1440) <= slack


def hm_minutes(t):
    h, m = t.split(":")
    return int(h) * 60 + int(m)


@part("Sunrise and sunset: Auto's theme turns at them where the computer is, and Night Light's schedule follows them")
def sun():
    S.open("appearance")
    S.press("Auto")
    until("theme = auto", lambda: setting("appearance.theme") == "auto")
    # The switch is usable with Auto chosen.
    follow = until(
        "the switch usable",
        lambda: (lambda c: c if not c.get("disabled") else None)(
            S.control("appearance", "follow-sunset", role="switch")
        ),
    )
    assert follow.get("checked") is False, follow
    S.click(follow)
    until("follow_sunset = true", lambda: setting("appearance.follow_sunset") is True)
    # A zone that names no place (UTC, as this computer starts, and GeoClue
    # has nothing to go on offline): no sunrise and sunset to follow, and
    # it says so; Night Light runs from 19:00 to 07:00.
    status, before = machine.execute("readlink /etc/localtime")
    zone_set = False
    if "/" not in localtime_zone() and setting("location.latitude") is None:
        until(
            "the pane to say where isn't known",
            lambda: any("isn't known" in t for t in S.texts(S.row("appearance", "follow-sunset"))),
        )
        night = ipc("night-light")["state"]
        assert night.get("place") is None and (night.get("from"), night.get("to")) == ("19:00", "07:00"), night
    # The shell asks GeoClue (offline, as in the sandbox, it gives up after
    # 30 s; with a network it may find where the host is): wait for its
    # answer, which decides the place.
    until(
        "the shell to hear from GeoClue",
        lambda: machine.succeed(
            "journalctl -b --no-pager -o cat | grep -c -E 'where the computer is: |the computer is near ' || true"
        ).strip()
        not in ("", "0"),
        timeout=60,
        every=2,
    )
    if "/" not in localtime_zone() and setting("location.latitude") is None:
        # A city's zone: its sunrise and sunset. Set as timedated sets one
        # (a relative link; an absolute one is NixOS's own, which Date &
        # Time reads as the configuration's), and put back after.
        machine.succeed("ln -sfn ../etc/zoneinfo/Europe/Paris /etc/localtime")
        zone_set = True
        reopen("appearance")
    zone, (lat, lon), where, doy, offset, now = where_and_when()
    rise, dusk = sun_times(lat, lon, doy, offset)
    print(f"{zone} ({lat:.2f}, {lon:.2f}): sunrise {rise // 60:02}:{rise % 60:02}, sunset {dusk // 60:02}:{dusk % 60:02}")
    # Settings says when, for where.
    words = until(
        "the hours under the switch",
        lambda: re.search(
            r"sunrise \((\d\d:\d\d)\) to sunset \((\d\d:\d\d)\) (.+?), Night after",
            " ".join(S.texts(S.row("appearance", "follow-sunset"))),
        ),
    )
    assert near_minutes(hm_minutes(words.group(1)), rise) and near_minutes(hm_minutes(words.group(2)), dusk), (
        words.group(0),
        rise,
        dusk,
    )
    assert words.group(3) == where, (words.group(3), where)
    # The shell's theme (which GTK apps follow) turns at them, not at
    # 07:00 and 19:00: checked away from the moments it turns. The shell
    # looks again each minute (nothing tells it the zone changed).
    zone, (lat, lon), where, doy, offset, now = where_and_when()
    if not any(near_minutes(now, t, 3) for t in (rise, dusk)):
        day = rise <= now < dusk if rise < dusk else not (dusk <= now < rise)
        want = "prefer-light" if day else "prefer-dark"
        until(
            f"dconf to say {want}",
            lambda: want in alice("dconf read /org/gnome/desktop/interface/color-scheme"),
            timeout=90,
            every=2,
        )

    # Night Light's sunset schedule, as the compositor runs it.
    night = ipc("night-light")["state"]
    print("night light:", night)
    assert night["schedule"] == "sunset", night
    assert night.get("place") in (zone, "located"), night
    assert near_minutes(hm_minutes(night["from"]), dusk) and near_minutes(hm_minutes(night["to"]), rise), (night, rise, dusk)

    reopen("appearance")
    assert S.checked("appearance", "follow-sunset") is True
    S.click(S.control("appearance", "follow-sunset", role="switch"))
    until("follow_sunset = false", lambda: setting("appearance.follow_sunset") is False)
    S.press("Night")
    until("theme = night", lambda: setting("appearance.theme") == "night")
    S.quit()
    if zone_set:
        if status == 0:
            machine.succeed(f"ln -sfn {shlex.quote(before.strip())} /etc/localtime")
        else:
            machine.succeed("rm -f /etc/localtime")


@part("Desktop & Deskbar: a Desk renamed and added, the Shelf and Trash hidden, tabs minimise on double-click")
def desktop():
    S.open("desktop")
    S.field(S.control("desktop", "desks", role="textinput"), "Mail")
    until("the compositor's first Desk to be Mail", lambda: ipc("desks")["desks"][0]["name"] == "Mail")
    assert setting("desks")[0]["name"] == "Mail", settings_toml()
    count = len(ipc("desks")["desks"])
    S.press("Add a Desk")
    until("a Desk more", lambda: len(ipc("desks")["desks"]) == count + 1)

    until("the Deskbar's Shelf", lambda: "Shelf" in shell_texts())
    S.click(S.control("desktop", "shelf", role="switch"))
    until("show_shelf = false", lambda: setting("deskbar.show_shelf") is False)
    until("the Deskbar to drop the Shelf", lambda: "Shelf" not in shell_texts())

    until("the desktop's Trash", lambda: "Trash" in shell_texts())
    S.click(S.beside("Trash", "switch"))
    until("show_trash = false", lambda: setting("desktop.show_trash") is False)
    until("the desktop to drop the Trash", lambda: "Trash" not in shell_texts())

    S.choose(S.control("desktop", "double-click", role="combobox"), "Minimise")
    until("tab_double_click = minimise", lambda: setting("windows.tab_double_click") == "minimise")
    # Double-click Settings' own tab, above its frame's top left.
    w = S.window()
    g = w["geometry"]
    click_at(g["x"] + 60, g["y"] - 12, times=2)
    until("Settings to minimise", lambda: S.window()["minimised"])
    ipc(f"unminimise {w['id']}")
    until("Settings back", lambda: not S.window()["minimised"])

    reopen("desktop")
    assert S.control("desktop", "desks", role="textinput").get("value") == "Mail"
    assert S.checked("desktop", "shelf") is False
    assert S.beside("Trash", "switch").get("checked") is False
    assert S.control("desktop", "double-click", role="combobox").get("value") == "Minimise"
    # Back as they were.
    S.click(S.control("desktop", "shelf", role="switch"))
    S.click(S.beside("Trash", "switch"))
    S.choose(S.control("desktop", "double-click", role="combobox"), "Fill the screen")
    until("the Shelf back", lambda: "Shelf" in shell_texts())
    S.quit()


@part("Menu Bar: seconds on the clock and the Bluetooth widget off reach the shell's bar")
def menubar():
    S.open("menubar")
    S.click(S.control("menubar", "clock-seconds", role="switch"))
    until("clock_seconds = true", lambda: setting("menubar.clock_seconds") is True)
    until(
        "the bar's clock to show seconds",
        lambda: any(re.fullmatch(r"\d{1,2}:\d{2}:\d{2}.*", t) for t in shell_texts()),
    )
    before = shell_labels()
    print("the bar's labels:", before)
    S.click(S.control("menubar", "widget-bluetooth", role="switch"))
    until("widgets without bluetooth", lambda: "bluetooth" not in (setting("menubar.widgets") or []))
    until("the bar without Bluetooth", lambda: "Bluetooth" not in shell_texts())
    reopen("menubar")
    assert S.checked("menubar", "clock-seconds") is True
    assert S.checked("menubar", "widget-bluetooth") is False
    S.click(S.control("menubar", "clock-seconds", role="switch"))
    S.click(S.control("menubar", "widget-bluetooth", role="switch"))
    until("widgets with bluetooth", lambda: "bluetooth" in (setting("menubar.widgets") or []))
    S.quit()


def shell_request(request):
    """A request over the shell's socket, as the compositor or an app sends
    one ({"request": "open", "what": "sound"}); its reply."""
    script = (
        "import socket, sys\n"
        "s = socket.socket(socket.AF_UNIX)\n"
        "s.connect(sys.argv[1])\n"
        "s.sendall((sys.argv[2] + '\\n').encode())\n"
        "print(s.makefile().readline())\n"
    )
    line = json.dumps({"type": "request", **request})
    out = alice(
        f"python3 -c {shlex.quote(script)} $XDG_RUNTIME_DIR/aizome-shell.$WAYLAND_DISPLAY.sock {shlex.quote(line)}"
    )
    return json.loads(out)


def shell_state():
    return json.loads(shell_request({"request": "state"})["message"])


@part("Shell: a systray panel asked for over the shell's socket opens under its widget")
def tray_panel():
    widgets = until(
        "the bar's sound widget",
        lambda: [e["rect"] for d in shell_docs() for e in d["elements"] if e.get("name") == "tray-sound"],
    )
    reply = shell_request({"request": "open", "what": "sound"})
    assert reply.get("ok"), reply
    state = until("the panel open", lambda: (lambda s: s if s.get("panel") == "sound" else None)(shell_state()))
    under = state["panel_under"]
    print("the panel is under", under, "; the widgets are at", widgets)
    assert any(abs(under[0] - x) < 1 and abs(under[2] - (x + w)) < 1 for x, y, w, h in widgets), (under, widgets)
    shot("tray-panel")
    shell_request({"request": "dismiss"})
    until("the panel closed", lambda: shell_state().get("panel") is None)


@part("Keep Awake: logind blocks sleep, idle and the lid until it's off, and the bar shows its cup meanwhile")
def keep_awake():
    def cup():
        return [e for d in shell_docs() for e in d["elements"] if e.get("name") == "tray-keep-awake" and e["visible"]]

    def held():
        return machine.execute("systemd-inhibit --list --no-legend | grep 'Aizome Keep Awake' | grep -q block")[0] == 0

    def holder():
        return alice("systemctl --user show -p MainPID --value aizome-keep-awake.service").strip()

    assert not cup() and not held()
    print(alice("aizome-awake on"))
    line = machine.succeed("systemd-inhibit --list --no-legend | grep 'Aizome Keep Awake'")
    assert "block" in line and all(w in line for w in ("sleep", "idle", "handle-lid-switch")), line
    assert json.loads(alice("aizome-awake status")) == {"enabled": True, "protected": True}
    until("the cup in the bar", cup)
    shot("keep-awake")
    # What logind goes by for the lid, its idle action and sleep asked for
    # by others (the login screen's own idle suspend). An explicit Sleep
    # from alice herself still goes through: logind lets a user past her
    # own inhibitors, as it should, so that isn't tried here.
    blocked = machine.succeed(
        "busctl get-property org.freedesktop.login1 /org/freedesktop/login1 org.freedesktop.login1.Manager BlockInhibited"
    )
    assert all(w in blocked for w in ("sleep", "idle", "handle-lid-switch")), blocked
    # A holder that dies takes the locks again in a second.
    first = holder()
    machine.succeed(f"kill -9 {first}")
    until("another holder", lambda: holder() not in ("", "0", first))
    until("the locks again", held)
    # It stays on for the next login: the service starts when it was left on.
    machine.succeed(f"test -e {HOME}/.local/state/aizome/keep-awake")
    print(alice("aizome-awake off"))
    assert not held()
    assert json.loads(alice("aizome-awake status")) == {"enabled": False, "protected": False}
    until("the cup gone", lambda: not cup())
    # Off, the unit doesn't start (its condition), so a login doesn't hold sleep off.
    alice("systemctl --user start aizome-keep-awake.service", check=False)
    assert not held()


@part(
    "Date & Time: repeated timezone changes stay editable and reach the running shell; NixOS's network time is explained"
)
def datetime_():
    S.open("general.datetime")
    assert S.checked("general", "clock-24h") is True
    S.click(S.control("general", "clock-24h", role="switch"))
    until("clock = 12h", lambda: setting("menubar.clock") == "12h")
    until("the bar's clock in 12 hours", lambda: any(re.search(r"\b(AM|PM)\b", t) for t in shell_texts()))

    # NixOS keeps systemd's units in the store, so timedated can't turn
    # network time on or off: the row says how it's set instead.
    ntp = machine.succeed("timedatectl show -p NTP --value").strip() == "yes"
    auto = " ".join(S.texts(S.row("general", "auto-time")))
    assert "services.timesyncd.enable" in auto and ("On" if ntp else "Off") in auto, auto

    shell_pid = machine.succeed("pgrep -u alice -x aizome-shell").strip()

    def clock_matches_system(twelve_hour):
        # Read a fresh date on every retry so a minute boundary is harmless.
        fmt = "+%-I:%M %p" if twelve_hour else "+%H:%M"
        expected = machine.succeed(f"date '{fmt}'").strip()
        # Each output has its own bar; a narrow one may hide its clock.
        # Check every visible bar clock, not just the first bar's surface.
        clocks = [
            e["text"]
            for d in shell_docs()
            if any(e.get("text") == "Toolchest" for e in d["elements"])
            for e in d["elements"]
            if e.get("visible") and re.fullmatch(r"\d{1,2}:\d{2}(?: [AP]M)?", e.get("text", ""))
        ]
        return bool(clocks) and all(clock == expected for clock in clocks)

    def choose_zone(city, zone):
        S.field(S.reveal(role=["searchinput", "textinput"], label="City, country or zone"), city, submit=False)
        S.click(S.button(city))
        until(
            f"the zone to be {zone}",
            lambda: machine.succeed("timedatectl show -p Timezone --value").strip() == zone,
        )
        until("the running bar's new local time", lambda: clock_matches_system(True), timeout=10)
        # SetTimezone must not turn its own writable /etc/zoneinfo link
        # into a managed setting. Change it again without restarting Settei.
        assert S.find(role=["searchinput", "textinput"], label="City, country or zone", visible=False) is not None
        assert "Set by your system configuration" not in " ".join(S.texts(S.row("general", "timezone")))
        assert machine.succeed("pgrep -u alice -x aizome-shell").strip() == shell_pid
        shot("datetime-" + city.lower())

    choose_zone("Tokyo", "Asia/Tokyo")
    choose_zone("Chicago", "America/Chicago")
    reopen("general.datetime")
    assert S.checked("general", "clock-24h") is False
    zone = " ".join(S.texts(S.row("general", "timezone")))
    assert "Chicago" in zone, zone
    choose_zone("Tokyo", "Asia/Tokyo")
    S.click(S.control("general", "clock-24h", role="switch"))
    until("clock = 24h", lambda: setting("menubar.clock") == "24h")
    until("the bar's new zone in 24 hours", lambda: clock_matches_system(False))
    S.quit()


@part("About: the computer's name reaches hostnamed; the host name NixOS sets is explained")
def about():
    S.open("general.about")
    S.field(S.control("general", "computer-name", role="textinput"), "Alice's Aizome")
    until("the pretty name", lambda: machine.succeed("hostnamectl --pretty").strip() == "Alice's Aizome")
    host = " ".join(S.texts(S.row("general", "hostname")))
    assert "networking.hostName" in host, host
    reopen("general.about")
    assert S.control("general", "computer-name", role="textinput").get("value") == "Alice's Aizome"
    S.quit()


@part("Default Apps: the text editor chosen opens text files; the terminal chosen opens on ❖Return")
def default_apps():
    S.open("general.default-apps")
    S.choose(S.control("general", "default-text-editor", role="combobox"), "Plain Editor")
    until(
        "mimeapps.list to name it",
        lambda: "text/plain=aizome-test-editor.desktop" in machine.succeed(f"cat {HOME}/.config/mimeapps.list || true"),
    )
    query = alice(
        "XDG_CURRENT_DESKTOP=Aizome XDG_CONFIG_DIRS=/etc/xdg/aizome:/etc/xdg xdg-mime query default text/plain"
    )
    assert query.strip() == "aizome-test-editor.desktop", query

    # The terminal chosen is the one ❖Return starts.
    S.choose(S.control("general", "default-terminal", role="combobox"), "Test Terminal")
    until("[keys] terminal", lambda: setting("keys.terminal") == ["foot", "--app-id=aizome-test-terminal"])
    chord("super+return")
    term = until(
        "the chosen terminal",
        lambda: next((w for w in ipc("windows")["windows"] if w["app_id"] == "aizome-test-terminal"), None),
    )
    ipc(f"close {term['id']}")

    reopen("general.default-apps")
    assert S.control("general", "default-text-editor", role="combobox").get("value") == "Plain Editor"
    assert S.control("general", "default-terminal", role="combobox").get("value") == "Test Terminal"
    S.quit()


@part("Login Items: the welcome tour stops opening at login")
def login_items():
    S.open("general.login-items")
    switch = S.beside("Welcome to Aizome", "switch")
    assert switch.get("checked") is True
    S.click(switch)
    until(
        "a hidden autostart entry",
        lambda: (
            "Hidden=true" in machine.succeed(f"cat {HOME}/.config/autostart/org.aizome.Annai.Welcome.desktop || true")
        ),
    )
    reopen("general.login-items")
    assert S.beside("Welcome to Aizome", "switch").get("checked") is False
    S.quit()


@part("Language & Region: NixOS sets the locale, and the pane says so")
def language():
    S.open("general.language")
    text = " ".join(S.texts(S.row("general", "system-language")))
    assert "Set by your system configuration" in text, text
    S.quit()


@part("Lock Screen: a minute's idleness locks, without the clock; the password unlocks; Never stays")
def lock():
    S.open("lock")
    S.choose(S.control("lock", "lock-after", role="combobox"), "1 minute")
    until("lock_after_minutes = 1", lambda: setting("idle.lock_after_minutes") == 1)
    S.click(S.control("lock", "show-clock", role="switch"))
    until("show_clock = false", lambda: setting("lock.show_clock") is False)
    S.quit()
    # Nobody touches anything for a minute.
    until("the screen to lock", locked, timeout=110, every=2)
    shot("lock-locked")
    lock_texts = [
        e["text"]
        for d in shell_docs()
        if any(x.get("role") == "passwordinput" for x in d["elements"])
        for e in d["elements"]
        if e.get("text")
    ]
    assert not any(re.fullmatch(r"\d{1,2}:\d{2}.*", t) for t in lock_texts), f"a clock on the lock screen: {lock_texts}"
    machine.send_chars("aizome\n")
    until("the screen to unlock", lambda: not locked(), timeout=20)
    S.open("lock")
    assert S.control("lock", "lock-after", role="combobox").get("value") == "1 minute"
    assert S.checked("lock", "show-clock") is False
    S.choose(S.control("lock", "lock-after", role="combobox"), "Never")
    until("lock_after_minutes = 0", lambda: setting("idle.lock_after_minutes") == 0)
    S.click(S.control("lock", "show-clock", role="switch"))
    S.quit()


@part("Notifications: Do Not Disturb holds banners back, and a quiet schedule is kept")
def notifications():
    S.open("notifications")
    S.click(S.control("notifications", "dnd", role="switch"))
    until("do_not_disturb = true", lambda: setting("notifications.do_not_disturb") is True)
    S.click(S.control("notifications", "schedule", role="switch"))
    until("quiet hours", lambda: setting("notifications.quiet_from") == "22:00")
    S.segment(S.control("notifications", "banner-time", role="tablist"), "12 s")
    until("banner_seconds = 12", lambda: setting("notifications.banner_seconds") == 12)
    alice("notify-send 'Quiet please' 'Held back by Do Not Disturb'")
    # Kept for the Deskbar, not shown in the bar.
    until("the Deskbar to keep it", lambda: any("Quiet please" in t for t in shell_texts()))
    time.sleep(1)
    assert not any("Quiet please" in t for t in shell_surface("Toolchest")), shell_surface("Toolchest")
    reopen("notifications")
    assert S.checked("notifications", "dnd") is True
    assert S.checked("notifications", "schedule") is True
    assert S.control("notifications", "banner-time", role="tablist").get("value") == "12 s"
    S.click(S.control("notifications", "dnd", role="switch"))
    until("do_not_disturb = false", lambda: setting("notifications.do_not_disturb") is False)
    # The quiet hours (22:00 to 07:00) hold banners back too, whatever the
    # VM's clock says: off.
    S.click(S.control("notifications", "schedule", role="switch"))
    until("no quiet hours", lambda: setting("notifications.quiet_from") is None)
    alice("notify-send 'Hello again' 'Shown now'")
    until("the bar to show it", lambda: any("Hello again" in t for t in shell_surface("Toolchest")))
    S.quit()


@part("Accessibility: sticky keys reach the compositor; reduce motion and a larger pointer are kept")
def accessibility():
    S.open("accessibility")
    S.click(S.control("accessibility", "reduced-motion", role="switch"))
    until("reduced_motion = true", lambda: setting("appearance.reduced_motion") is True)
    S.click(S.control("accessibility", "sticky-keys", role="switch"))
    until("sticky_keys = true", lambda: setting("input.keyboard.sticky_keys") is True)
    # Sticky keys in the compositor: Shift pressed and let go, then a, is A.
    S.click(S.wait(name="search"))
    key("shift")
    key("a")
    until("a sticky Shift", lambda: S.find(name="search").get("value") == "A")
    key("escape")
    S.slide(S.control("accessibility", "cursor-size", role="slider"), 0.75)
    until("a larger pointer", lambda: (setting("appearance.cursor_size") or 24) > 24)
    reopen("accessibility")
    assert S.checked("accessibility", "reduced-motion") is True
    assert S.checked("accessibility", "sticky-keys") is True
    S.click(S.control("accessibility", "reduced-motion", role="switch"))
    S.click(S.control("accessibility", "sticky-keys", role="switch"))
    until("sticky_keys = false", lambda: setting("input.keyboard.sticky_keys") is False)
    S.quit()


@part("Performance: the renderer for the next login is kept and the compositor knows")
def performance():
    S.open("performance")
    use = S.control("performance", "renderer", role="tablist")
    S.segment(use, "Software")
    until("renderer = software", lambda: setting("performance.renderer") == "software")
    stats = ipc("stats")
    print(stats)
    S.segment(S.control("performance", "renderer", role="tablist"), "GPU")
    until("renderer = gpu", lambda: setting("performance.renderer") == "gpu")
    stats = ipc("stats")["stats"]
    assert stats.get("renderer_setting") == "gpu", stats
    assert stats.get("renderer_after_restart") is True, stats
    reopen("performance")
    assert S.control("performance", "renderer", role="tablist").get("value") == "GPU"
    S.segment(S.control("performance", "renderer", role="tablist"), "Automatic")
    until("renderer = auto", lambda: setting("performance.renderer") == "auto")
    S.quit()


FRECENCY = f"{HOME}/.local/state/aizome/frecency.json"


def frecency():
    return machine.succeed(f"cat {FRECENCY} 2>/dev/null || true")


def launch_from_palette(name, app_id):
    """Open an app the way a person does: ❖Space, its name, Return."""
    chord("super+space")
    until("the palette", lambda: any(d.get("title") == "Palette" for d in shell_docs()), timeout=10)
    type_text(name)
    time.sleep(1)
    key("return")
    w = until(f"{name}'s window", lambda: next((w for w in ipc("windows")["windows"] if w["app_id"] == app_id), None))
    ipc(f"close {w['id']}")
    until(f"{name} gone", lambda: not any(w["app_id"] == app_id for w in ipc("windows")["windows"]))


@part("Search: a palette scope turned off is kept; forgetting what the palette learnt sticks")
def search():
    S.open("search")
    S.click(S.control("search", "scopes", role="switch"))
    until("a scope less", lambda: setting("palette.scopes") is not None and "commands" not in setting("palette.scopes"))
    reopen("search")
    assert S.checked("search", "scopes") is False
    S.click(S.control("search", "scopes", role="switch"))
    until("every scope", lambda: "commands" in (setting("palette.scopes") or ["commands"]))
    S.quit()

    # The palette learns a pick...
    launch_from_palette("Tokei", "org.aizome.Tokei")
    until("the pick learnt", lambda: "app:org.aizome.Tokei" in frecency())
    # ...Settings forgets it...
    S.open("search")
    S.press("Forget…")
    S.press("Forget")
    until("the history gone", lambda: machine.execute(f"test -e {FRECENCY}")[0] != 0)
    S.quit()
    # ...and the next pick doesn't bring it back.
    launch_from_palette("Soroban", "org.aizome.Soroban")
    until("the new pick learnt", lambda: "app:org.aizome.Soroban" in frecency())
    assert "Tokei" not in frecency(), frecency()


def write_png(path, rgb, side=96):
    """A picture of one colour, written as alice."""
    import base64
    import struct
    import zlib

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    raw = b"".join(b"\x00" + bytes(rgb) * side for _ in range(side))
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", side, side, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw))
        + chunk(b"IEND", b"")
    )
    data = base64.b64encode(png).decode()
    alice(f"mkdir -p $(dirname {path}) && echo {data} | base64 -d > {path}")


def icon_file():
    out = machine.succeed(
        "busctl get-property org.freedesktop.Accounts /org/freedesktop/Accounts/User1000 "
        "org.freedesktop.Accounts.User IconFile"
    )
    return out.split('"')[1] if '"' in out else ""


def choose_picture(name):
    S.press("Choose…")
    S.wait(text="Choose a Picture")
    S.click(S.wait(text=name, role="label"))
    S.press("Open")
    S.gone("the panel to close", text="Choose a Picture")


@part("Users & Groups: a new full name and picture reach AccountsService and the password file")
def users():
    write_png(f"{HOME}/Pictures/indigo.png", (61, 89, 161))
    write_png(f"{HOME}/Pictures/amber.png", (224, 175, 104))
    S.open("users")
    S.field(S.control("users", "name", role="textinput"), "Alice Pleasance Liddell")
    until(
        "AccountsService's name",
        lambda: "Alice Pleasance Liddell"
        in machine.succeed(
            "busctl get-property org.freedesktop.Accounts /org/freedesktop/Accounts/User1000 "
            "org.freedesktop.Accounts.User RealName"
        ),
    )
    assert "Alice Pleasance Liddell" in machine.succeed("getent passwd alice")

    choose_picture("indigo.png")
    until("a picture in AccountsService", lambda: icon_file() and machine.execute(f"test -s {icon_file()}")[0] == 0)
    first = machine.succeed(f"sha256sum {icon_file()}").split()[0]
    # Another, which AccountsService keeps at the same path.
    choose_picture("amber.png")
    until("the new picture", lambda: machine.succeed(f"sha256sum {icon_file()}").split()[0] != first)
    shot("users-picture")

    # Another account, whose home isn't under /home: Remove names it, and
    # its type can change (accounts aren't fixed by the configuration here).
    machine.succeed("useradd -m -d /srv/bob -s /run/current-system/sw/bin/bash -c 'Bob Example' bob")
    reopen("users")
    assert S.control("users", "name", role="textinput").get("value") == "Alice Pleasance Liddell"
    kind = until("Bob's account type", lambda: S.beside("Bob Example", "combobox"), timeout=30)
    assert not kind.get("disabled"), kind
    S.click(S.beside("Bob Example", "button", label="Remove"))
    S.wait(text="Also delete /srv/bob and everything in it")
    S.press("Cancel")
    S.gone("the sheet to go", text="Also delete /srv/bob and everything in it")
    S.quit()
    machine.succeed("userdel -r bob")


@part("Keyboard: quick repeat and a German layout reach the compositor; Caps Lock as Escape is kept")
def keyboard():
    S.open("keyboard")

    def held(ch="x", seconds=1.5):
        """Hold a key in the "Try it" field; how many it typed."""
        S.click(S.beside("Try it", "textinput"))
        chord("super+a")
        key("backspace")
        alice(f"aizomectl input key {ch} down && sleep {seconds} && aizomectl input key {ch} up")
        time.sleep(0.5)
        return (S.beside("Try it", "textinput").get("value") or "").count(ch)

    slow = held()
    S.slide(S.control("keyboard", "repeat-delay", role="slider"), 1.0)
    S.slide(S.control("keyboard", "repeat-rate", role="slider"), 1.0)
    until("a short delay", lambda: (setting("input.keyboard.repeat_delay") or 600) <= 200)
    until("a fast rate", lambda: (setting("input.keyboard.repeat_rate") or 25) >= 50)
    fast = held()
    print(f"a held key typed {slow} times, then {fast}")
    assert fast > max(2 * slow, 30), (slow, fast)

    S.choose(S.control("keyboard", "caps-lock", role="combobox"), "Escape")
    until("caps:escape", lambda: "caps:escape" in str(setting("input.keyboard.options")))

    S.press("Add a Layout…")
    S.wait(text="Add a Keyboard Layout")
    type_text("German")
    S.click(S.wait(text="German", role="label"))
    S.press("Add")
    until("German added", lambda: "de" in (setting("input.keyboard.layouts") or []))
    S.click(S.beside("German", "button", label="Move up"))
    until("German first", lambda: (setting("input.keyboard.layouts") or [""])[0] == "de")
    # The compositor's keymap is German now: the key labelled Y types z.
    S.click(S.beside("Try it", "textinput"))
    chord("super+a")
    key("backspace")
    key("y")
    key("z")
    until("German keys", lambda: S.beside("Try it", "textinput").get("value") == "zy")

    reopen("keyboard")
    first = " ".join(S.texts(S.row("keyboard", "layouts")))
    assert "German" in first, first
    assert S.control("keyboard", "caps-lock", role="combobox").get("value") == "Escape"
    assert "150 ms" in " ".join(S.texts(S.row("keyboard", "repeat-delay")))
    # Back to the US layout alone, before anything else is typed.
    S.click(S.beside("German", "button", label="Remove"))
    until("US alone", lambda: setting("input.keyboard.layouts") == ["us"])
    S.choose(S.control("keyboard", "caps-lock", role="combobox"), "Caps Lock")
    S.quit()


@part("Keyboard shortcuts: ❖T recorded for the terminal opens one; back to the default")
def shortcuts():
    S.open("keyboard.shortcuts")
    S.press("Change Open a terminal")
    S.wait(name="recorder")
    chord("super+t")
    until("❖T bound", lambda: (setting("keys.bindings") or {}).get("Super+T") == "terminal")
    before = {w["id"] for w in ipc("windows")["windows"]}

    def new_window():
        return next((w for w in ipc("windows")["windows"] if w["id"] not in before and w["app_id"] != APP), None)

    # The file is written before the compositor has read it: press again
    # until it has, as a person would.
    term = None
    for _ in range(5):
        chord("super+t")
        try:
            term = until("a terminal from ❖T", new_window, timeout=4)
            break
        except AssertionError:
            pass
    assert term, "❖T opened no terminal"
    ipc(f"close {term['id']}")
    reopen("keyboard.shortcuts")
    row = S.reveal(name="entry:keyboard.shortcut-terminal")
    assert "T" in S.texts(row), S.texts(row)
    S.click(S.find(inside=row, role="button", label="Back to the default"))
    until("❖T unbound", lambda: "Super+T" not in (setting("keys.bindings") or {}))
    S.quit()


def mouse_index(kind):
    """QEMU's number for a mouse ("PS/2", "Tablet")."""
    for line in machine.send_monitor_command("info mice").splitlines():
        m = re.search(r"Mouse #(\d+): (.*)", line)
        if m and kind in m.group(2):
            return int(m.group(1))
    raise AssertionError(f"no {kind} mouse")


def sidebar_top():
    """Where the sidebar's first pane is (it scrolls)."""
    return S.find(text="Network", role="label", visible=False)["rect"][1]


def scrolled_by_wheel(dz, kind="PS/2"):
    """Turn a real mouse's wheel (QEMU's PS/2 mouse, or another plugged in)
    over Settings' sidebar; how far its list moved up."""
    y0 = sidebar_top()
    win = S.window()["geometry"]
    move(win["x"] + 116, win["y"] + win["h"] / 2)
    machine.send_monitor_command(f"mouse_set {mouse_index(kind)}")
    for _ in range(2):
        machine.send_monitor_command(f"mouse_move 0 0 {dz}")
        time.sleep(0.2)
    time.sleep(0.8)
    return y0 - sidebar_top()


def pointer_travel(dx):
    """Move the real relative mouse (PS/2) right by dx; how far the pointer went."""
    win = S.window()["geometry"]
    move(win["x"] + 300, win["y"] + win["h"] / 2)
    time.sleep(0.3)
    start = S.doc()["pointer"][0]
    machine.send_monitor_command(f"mouse_set {mouse_index('PS/2')}")
    for _ in range(4):
        machine.send_monitor_command(f"mouse_move {dx} 0")
        time.sleep(0.15)
    time.sleep(0.6)
    return S.doc()["pointer"][0] - start


@part("Mouse: natural scrolling turns the wheel around, and pointer speed changes how far the mouse goes")
def mouse():
    S.open("mouse")
    # Flat, so speed alone sets how far a movement goes.
    S.click(S.control("mouse", "acceleration", role="switch"))
    until("a flat profile", lambda: setting("input.pointer.accel_profile") == "flat")
    S.slide(S.control("mouse", "speed", role="slider"), 0.1)
    until("a slow pointer", lambda: (setting("input.pointer.speed") or 0) < -0.5)
    slow = pointer_travel(10)
    S.slide(S.control("mouse", "speed", role="slider"), 0.95)
    until("a fast pointer", lambda: (setting("input.pointer.speed") or 0) > 0.5)
    fast = pointer_travel(10)
    print(f"the same movement took the pointer {slow} then {fast} points")
    assert fast > 1.5 * max(slow, 1), (slow, fast)

    # The same turn of the wheel, before and after: the list goes one way,
    # then back (libinput offers this PS/2 wheel natural scrolling of its
    # own, so turning it round there too would turn it back: no change).
    dz = 1
    traditional = scrolled_by_wheel(dz)
    if traditional == 0:
        # Already at that end of the list.
        dz = -1
        traditional = scrolled_by_wheel(dz)
    S.click(S.control("mouse", "natural-scroll", role="switch"))
    until("natural_scroll = true", lambda: setting("input.pointer.natural_scroll") is True)
    time.sleep(1)
    natural = scrolled_by_wheel(dz)
    print(f"the wheel moved the pane {traditional}, then {natural} with natural scrolling")
    assert traditional * natural < 0, (traditional, natural)
    # A mouse plugged in with natural scrolling already on (libinput can
    # turn a USB wheel round itself): turned round once, as the other is.
    added = machine.send_monitor_command("device_add usb-mouse,bus=usb-bus.0,id=natmouse")
    if "rror" in added or "not found" in added:
        print("device_add on usb-bus.0:", added)
        machine.send_monitor_command("device_add usb-mouse,id=natmouse")
    until("the USB mouse", lambda: mouse_index("HID Mouse") is not None)
    time.sleep(2)
    # The other way, which, turned round, goes as the first turn did.
    usb = scrolled_by_wheel(-dz, kind="HID Mouse")
    print(f"the USB mouse's wheel moved the pane {usb}")
    machine.send_monitor_command("device_del natmouse")
    assert usb * traditional > 0, f"turned round twice: {usb} against {traditional}"

    reopen("mouse")
    assert S.checked("mouse", "natural-scroll") is True
    assert S.checked("mouse", "acceleration") is False
    S.click(S.control("mouse", "natural-scroll", role="switch"))
    S.click(S.control("mouse", "acceleration", role="switch"))
    S.slide(S.control("mouse", "speed", role="slider"), 0.5)
    until("adaptive again", lambda: setting("input.pointer.accel_profile") == "adaptive")
    S.quit()


@part("Trackpad: with none attached, the pane says so, and its settings are kept for when one is")
def trackpad():
    S.open("trackpad")
    assert any("No trackpad is connected" in t for t in S.texts()), S.texts()
    S.click(S.control("trackpad", "tap", role="switch"))
    until("tap changed", lambda: setting("input.touchpad.tap") is not None)
    tap = setting("input.touchpad.tap")
    reopen("trackpad")
    assert S.checked("trackpad", "tap") == tap
    S.click(S.control("trackpad", "tap", role="switch"))
    S.quit()


@part("Battery & Power: Power Saver reaches power-profiles-daemon; screen timings are kept; no battery is said")
def battery():
    S.open("battery")
    texts = " ".join(S.texts())
    assert "there's no battery to show" in texts, texts
    mode = S.control("battery", "mode", role="tablist")
    names = [p["name"] for p in mode.get("parts", [])]
    assert names == ["Power Saver", "Balanced"], names
    S.segment(mode, "Power Saver")
    until("power-saver", lambda: machine.succeed("powerprofilesctl get").strip() == "power-saver")
    S.choose(S.control("battery", "screen-off", role="combobox"), "5 minutes")
    until("monitors off after 5", lambda: setting("idle.monitors_off_after_minutes") == 5)
    reopen("battery")
    assert S.control("battery", "mode", role="tablist").get("value") == "Power Saver"
    assert S.control("battery", "screen-off", role="combobox").get("value") == "5 minutes"
    assert not S.find(name="entry:battery.lid", visible=False), "a lid row on a computer without a lid"
    S.segment(S.control("battery", "mode", role="tablist"), "Balanced")
    until("balanced", lambda: machine.succeed("powerprofilesctl get").strip() == "balanced")
    S.quit()


def wpctl(args):
    return alice(f"wpctl {args}")


@part("Sound: the default output, its volume and mute reach PipeWire")
def sound():
    S.open("sound")
    S.press("Desk Headphones")
    until(
        "the headphones as the default",
        lambda: "aizome-headphones" in wpctl("inspect @DEFAULT_AUDIO_SINK@"),
    )
    S.slide(S.control("sound", "output-volume", role="slider"), 0.3)
    until("about 30%", lambda: abs(float(wpctl("get-volume @DEFAULT_AUDIO_SINK@").split()[1]) - 0.3) < 0.06)
    S.click(S.control("sound", "output-volume", role="button", label="Mute"))
    until("muted", lambda: "MUTED" in wpctl("get-volume @DEFAULT_AUDIO_SINK@"))
    reopen("sound")
    chosen = S.reveal(label="Desk Headphones")
    assert chosen.get("selected"), chosen
    assert S.find(inside=S.row("sound", "output-volume"), role="button", label="Unmute"), "not shown muted"
    S.click(S.control("sound", "output-volume", role="button", label="Unmute"))
    until("unmuted", lambda: "MUTED" not in wpctl("get-volume @DEFAULT_AUDIO_SINK@"))
    vol = S.control("sound", "output-volume", role="slider")
    assert abs(vol["range"][2] - 0.3) < 0.06 or abs(vol["range"][2] - 30) < 6, vol

    # Alerts, heard on the output (its monitor recorded, the output at full
    # volume: 30% is about 3% of the signal): the bell plays the alert
    # sound at the alert volume, silent at none...
    wpctl("set-volume @DEFAULT_AUDIO_SINK@ 1.0")
    bell = lambda: alice("aizomectl action bell")  # noqa: E731
    loud = loudness_during(bell)
    print(f"the bell peaked at {loud}")
    assert loud > 300, f"the bell made no sound ({loud})"
    S.slide(S.control("sound", "alert-volume", role="slider"), 0.0)
    until("alert_volume = 0", lambda: setting("sound.alert_volume") == 0.0)
    quiet = loudness_during(bell)
    assert quiet < 50, f"the bell sounded at no volume ({quiet})"
    S.slide(S.control("sound", "alert-volume", role="slider"), 0.7)
    until("alert_volume back", lambda: (setting("sound.alert_volume") or 0) > 0.5)
    # (Letting go of the slider plays the alert, to hear it: let it end.)
    time.sleep(3)
    # ...and with feedback on, the volume keys are heard at the new volume.
    silent = loudness_during(lambda: alice("aizomectl action volume-up"))
    assert silent < 50, f"feedback without asking ({silent})"
    S.click(S.control("sound", "feedback", role="switch"))
    until("volume_feedback = true", lambda: setting("sound.volume_feedback") is True)
    heard = loudness_during(lambda: alice("aizomectl action volume-down"))
    print(f"the volume key's feedback peaked at {heard}")
    assert heard > 300, f"no feedback from the volume key ({heard})"
    S.click(S.control("sound", "feedback", role="switch"))
    until("volume_feedback = false", lambda: setting("sound.volume_feedback") is False)
    wpctl("set-volume @DEFAULT_AUDIO_SINK@ 0.3")
    S.quit()


def loudness_during(act, seconds=4):
    """The loudest sample on the default output (its monitor) while `act`
    runs, 0–32767."""
    raw = "/tmp/aizome-monitor.raw"
    alice(
        f"rm -f {raw}; setsid timeout {seconds} parecord --device=@DEFAULT_MONITOR@ --raw --format=s16le "
        f"--channels=1 --rate=16000 {raw} </dev/null >/dev/null 2>&1 &"
    )
    time.sleep(1)
    act()
    time.sleep(seconds)
    script = (
        "import array, sys\n"
        "d = open(sys.argv[1], 'rb').read()\n"
        "a = array.array('h')\n"
        "a.frombytes(d[: len(d) // 2 * 2])\n"
        "print(max(map(abs, a)) if a else 0)\n"
    )
    return int(machine.succeed(f"python3 -c {shlex.quote(script)} {raw}").strip())


def search_settings(words):
    """Search Settings and open what it finds first, as a person does."""
    S.click(S.wait(name="search"))
    chord("super+a")
    type_text(words)
    key("return")
    time.sleep(1)


def revealed(entry):
    """The option search opened, shown in the pane."""
    return until(f"{entry} shown", lambda: S.find(name=entry))


def clear_search():
    """Empty Settings' search field (Escape there could go Back instead)."""
    S.click(S.wait(name="search"))
    chord("super+a")
    key("backspace")
    time.sleep(0.5)


@part("Network: search reveals Add VPN and Other Network…; a wrong passphrase can be corrected; the hotspot")
def network():
    S.open("network")
    S.reveal(label="Aizome Test AP", role="button") if S.find(label="Aizome Test AP", visible=False) else None
    ap = until("the access point listed", lambda: S.find(label="Aizome Test AP", role="button", visible=False), timeout=60)
    # No VPNs here: Add VPN is the VPN section's only row, and found.
    search_settings("Add VPN")
    revealed("entry:network.add-vpn")
    clear_search()
    # A wrong passphrase: the sheet comes back with it, to be put right.
    S.click(S.reveal(label=ap["label"], role="button"))
    field = S.wait(name="network-password")
    assert field.get("focused"), field
    type_text("not-the-passphrase")
    key("return")
    until("the sheet back", lambda: S.find(contains="Couldn't join: "), timeout=120)
    field = S.wait(name="network-password")
    assert field.get("text") is None and field.get("value") is None, field
    assert "Aizome Test AP" not in machine.succeed("nmcli -t -f NAME connection show"), "a refused network left behind"
    S.click(field)
    chord("super+a")
    type_text(WIFI_PASSPHRASE)
    key("return")
    until(
        "NetworkManager connected",
        lambda: "yes:Aizome Test AP" in machine.succeed("nmcli -t -f ACTIVE,SSID dev wifi list --rescan no || true"),
        timeout=90,
    )
    until("the pane to say so", lambda: any("Connected to Aizome Test AP" in t for t in S.texts()), timeout=30)
    shot("network-joined")

    # Airplane mode through rfkill (the seat's user may write /dev/rfkill).
    S.click(S.control("network", "airplane", role="switch"))
    until("radios off", lambda: "disabled" in machine.succeed("nmcli radio wifi") or "blocked" in machine.succeed("rfkill -n -o SOFT"))
    S.click(S.control("network", "airplane", role="switch"))
    until("radios on", lambda: "enabled" in machine.succeed("nmcli radio wifi"))
    until("Wi-Fi back", lambda: any("Connected to Aizome Test AP" in t for t in S.texts()), timeout=90)

    # Without rfkill (no uaccess on /dev/rfkill): NetworkManager's Wi-Fi and
    # BlueZ's adapters go off instead, and those that were on come back.
    before = until("Bluetooth on", lambda: (lambda p: p if any(p.values()) else None)(bluetooth_powered()))
    machine.succeed("setfacl -b /dev/rfkill")
    assert machine.execute("su alice -c 'test -w /dev/rfkill'")[0] != 0, "alice may still write /dev/rfkill"
    S.click(S.control("network", "airplane", role="switch"))
    until("Wi-Fi off", lambda: "disabled" in machine.succeed("nmcli radio wifi"))
    until("Bluetooth off", lambda: not any(bluetooth_powered().values()))
    # Bluetooth went off through BlueZ, not rfkill (NetworkManager blocks
    # the Wi-Fi radios itself when its Wi-Fi goes off).
    radios = machine.succeed("rfkill -n -o TYPE,SOFT")
    print("radios:", radios)
    assert not any(
        line.split()[0] == "bluetooth" and line.split()[1] == "blocked" for line in radios.splitlines() if line.strip()
    ), radios
    time.sleep(1)
    assert S.checked("network", "airplane") is True, "the switch sprang back"
    S.click(S.control("network", "airplane", role="switch"))
    until("Wi-Fi on", lambda: "enabled" in machine.succeed("nmcli radio wifi"))
    until("Bluetooth back as it was", lambda: bluetooth_powered() == before)
    machine.succeed("setfacl -m u:alice:rw /dev/rfkill")

    addr = machine.succeed("nmcli -g IP4.ADDRESS dev show eth0").strip().split("/")[0]
    wired = " ".join(S.texts(S.row("network", "wired")))
    assert addr in wired, (addr, wired)

    # Wi-Fi off: search for Other Network… shows the switch that brings it.
    S.click(S.control("network", "wifi", role="switch"))
    until("Wi-Fi off", lambda: "disabled" in machine.succeed("nmcli radio wifi"))
    search_settings("Other Network")
    revealed("entry:network.join-other")
    clear_search()
    S.click(S.control("network", "wifi", role="switch"))
    until("Wi-Fi on", lambda: "enabled" in machine.succeed("nmcli radio wifi"))
    until("Wi-Fi back", lambda: any("Connected to Aizome Test AP" in t for t in S.texts()), timeout=90)

    # The hotspot says the network's name, not its profile's ("Hotspot").
    S.click(S.control("network", "hotspot", role="switch"))
    name = S.wait(name="network-hotspot-name").get("value")
    assert name and name != "Hotspot", name
    S.click(S.wait(name="network-hotspot-password"))
    type_text("hotspot-pass-1")
    key("return")
    until(
        "the hotspot named",
        lambda: f"Sharing as “{name}”" in " ".join(S.texts(S.row("network", "hotspot"))),
        timeout=60,
    )
    shot("network-hotspot")
    S.click(S.control("network", "hotspot", role="switch"))
    until("the hotspot off", lambda: "Sharing as" not in " ".join(S.texts(S.row("network", "hotspot"))), timeout=30)

    reopen("network")
    until("Wi-Fi back", lambda: any("Connected to Aizome Test AP" in t for t in S.texts()), timeout=90)
    assert S.find(text="Aizome Test AP", visible=False), "the network in use isn't shown"
    S.quit()


def bluetooth_powered():
    """Each BlueZ controller's address, and whether it's on."""
    macs = re.findall(r"Controller (\S+)", machine.succeed("bluetoothctl list"))
    return {m: "Powered: yes" in machine.succeed(f"bluetoothctl show {m}") for m in macs}


def controller_named(alias):
    """The address of the BlueZ controller with this name (btvirt makes two)."""
    for mac in re.findall(r"Controller (\S+)", machine.succeed("bluetoothctl list")):
        if f"Alias: {alias}\n" in machine.succeed(f"bluetoothctl show {mac}"):
            return mac
    return None


@part("Bluetooth: turning it off and on reaches BlueZ; the controller's name too; discovery runs while open")
def bluetooth():
    S.open("bluetooth")
    # The controller the pane shows (after airplane mode, not always BlueZ's
    # first: the default is the first that's on).
    mac = until("the pane's controller", lambda: controller_named(S.wait(name="bluetooth-name").get("value")))
    show = lambda: machine.succeed(f"bluetoothctl show {mac}")  # noqa: E731
    until("discovery while the pane is open", lambda: "Discovering: yes" in show(), timeout=30)
    switch = S.control("bluetooth", "bluetooth", role="switch")
    assert switch.get("checked") is ("Powered: yes" in show())
    S.click(switch)
    until("powered off", lambda: "Powered: no" in show())
    # The switch is back to be used once the change is done, and stays off
    # though the other controller is on.
    off = until(
        "the switch off and usable",
        lambda: (lambda c: c if c.get("checked") is False and not c.get("disabled") else None)(
            S.control("bluetooth", "bluetooth", role="switch")
        ),
    )
    S.click(off)
    until("powered on", lambda: "Powered: yes" in show())
    S.field(S.wait(name="bluetooth-name"), "Alice's Aizome")
    until("the new alias", lambda: "Alias: Alice's Aizome" in show())
    reopen("bluetooth")
    assert S.wait(name="bluetooth-name").get("value") == "Alice's Aizome"
    S.quit()
    until("discovery stopped with the pane", lambda: "Discovering: no" in show(), timeout=20)


def outputs():
    return {o["name"]: o for o in ipc("outputs")["outputs"]}


def output_entry(name):
    for o in setting("outputs") or []:
        if o.get("name") == name:
            return o
    return None


def keep_sheet(button):
    S.wait(text="Keep these display settings?", timeout=30)
    S.press(button)
    S.gone("the sheet to go", text="Keep these display settings?", timeout=30)


@part("Displays: scale, arrangement and mirroring reach the compositor and are kept; a change not kept goes back; Night Light is explained")
def displays():
    S.open("displays")
    two = "RHT QEMU Monitor (Virtual-2)"
    picker = S.wait(role="tablist", contains="RHT QEMU Monitor")
    S.segment(picker, two)
    S.wait(contains="Virtual-2 ·")

    # Night Light: virtio's outputs take no gamma table, so no switch.
    assert not any(o.get("gamma") for o in outputs().values()), outputs()
    night = " ".join(S.texts(S.reveal(name="entry:displays.night-light")))
    assert "Not on these displays" in night, night
    assert not S.find(role="switch", inside=S.row("displays", "night-light"), visible=False)

    # Scale: not a risky change, applied and kept at once.
    S.click(S.reveal(role="button", contains="(1×)"))
    until("Virtual-2 at 1×", lambda: outputs()["Virtual-2"]["scale"] == 1.0)
    until("kept at 1×", lambda: (output_entry("Virtual-2") or {}).get("scale") == 1.0)

    # Arrangement: drag Virtual-2's tile under Virtual-1's.
    arr = S.reveal(name="entry:displays.arrangement")
    tiles = {p["name"]: p["rect"] for p in arr["parts"]}
    win = S.window()["geometry"]
    x1, y1, w1, h1 = tiles["Virtual-1"]
    x2, y2, w2, h2 = tiles["Virtual-2"]
    sx, sy = win["x"] + x2 + w2 / 2, win["y"] + y2 + h2 / 2
    tx, ty = win["x"] + x1 + w2 / 2, win["y"] + y1 + h1 + h2 / 2 + 2
    alice(
        f"aizomectl input motion {sx:.1f} {sy:.1f} && aizomectl input button left down && "
        + " && ".join(
            f"aizomectl input motion {sx + (tx - sx) * i / 6:.1f} {sy + (ty - sy) * i / 6:.1f}" for i in range(1, 7)
        )
        + " && aizomectl input button left up"
    )
    until("Virtual-2 below Virtual-1", lambda: outputs()["Virtual-2"]["logical"]["y"] == outputs()["Virtual-1"]["logical"]["h"])
    until("kept below", lambda: ((output_entry("Virtual-2") or {}).get("position") or [0, 0])[1] > 0)
    shot("displays-arranged")

    # Rotation, not kept: it goes back.
    S.choose(S.control("displays", "rotation", role="combobox"), "90")
    until("Virtual-2 turned", lambda: outputs()["Virtual-2"]["transform"] == "90")
    keep_sheet("Revert")
    until("Virtual-2 straight again", lambda: outputs()["Virtual-2"]["transform"] == "normal")
    assert (output_entry("Virtual-2") or {}).get("transform") in (None, "normal")

    # Mirroring: seen at once, before it's kept; Revert takes it back.
    S.choose(S.control("displays", "use-as", role="combobox"), "Mirror for")
    until("Virtual-2 mirroring before Keep", lambda: outputs()["Virtual-2"].get("mirror") == "Virtual-1", timeout=30)
    assert S.find(text="Keep these display settings?"), "mirrored without asking"
    keep_sheet("Revert")
    until("Virtual-2 its own after Revert", lambda: outputs()["Virtual-2"].get("mirror") is None, timeout=30)
    assert (output_entry("Virtual-2") or {}).get("mirror") is None, setting("outputs")
    # Mirroring, kept.
    S.choose(S.control("displays", "use-as", role="combobox"), "Mirror for")
    until("Virtual-2 mirroring", lambda: outputs()["Virtual-2"].get("mirror") == "Virtual-1", timeout=30)
    keep_sheet("Keep Changes")
    until("kept mirroring", lambda: (output_entry("Virtual-2") or {}).get("mirror") == "Virtual-1")

    reopen("displays")
    S.segment(S.wait(role="tablist", contains="RHT QEMU Monitor"), two)
    use = S.control("displays", "use-as", role="combobox")
    assert use.get("value", "").startswith("Mirror for"), use
    # Back to extending the desktop.
    S.choose(use, "Extended display")
    keep_sheet("Keep Changes")
    until("Virtual-2 its own again", lambda: outputs()["Virtual-2"].get("mirror") is None, timeout=30)

    # `[[outputs]]` changed elsewhere (Virtual-2 made the main display by
    # hand) and applied: the open pane follows, without opening it again.
    assert S.control("displays", "use-as", role="combobox").get("value") == "Extended display"
    edit = (
        "import sys\n"
        "p = sys.argv[1]\n"
        "t = open(p).read().replace('primary = true\\n', '')\n"
        "t = t.replace('name = \"Virtual-2\"\\n', 'name = \"Virtual-2\"\\nprimary = true\\n', 1)\n"
        "open(p, 'w').write(t)\n"
    )
    alice(f"python3 -c {shlex.quote(edit)} {HOME}/.config/aizome/settings.toml && aizomectl reload-config")
    until("the compositor's main display", lambda: outputs()["Virtual-2"].get("primary") is True)
    until(
        "the pane to say Virtual-2 is the main display",
        lambda: S.control("displays", "use-as", role="combobox").get("value") == "Main display",
    )
    S.quit()


@part("Printers: the network's printer is set up and not offered twice; one added by its address is the default and prints")
def printers():
    started = time.monotonic()
    S.open("printers")
    S.button("Add Printer…")
    print(f"the printers showed after {time.monotonic() - started:.1f} s")
    # cups-browsed sets up the printer announced on the network by itself.
    until("the network's printer listed", lambda: S.find(contains="Aizome_Test_Printer", visible=False), timeout=90)
    S.press("Add Printer…")
    S.wait(text="Add a Printer")
    S.gone("the search to end", contains="Looking for printers", timeout=60)
    assert not S.find(contains="cups-filters", role="button", visible=False), "the printer set up already is offered again"
    S.click(S.wait(label="Add one by its address", role="checkbox"))
    S.field(S.wait(name="printers-address"), "localhost:8631", submit=False)
    S.press("Add")
    added = until(
        "CUPS to have it",
        lambda: next((ln for ln in machine.succeed("lpstat -v").splitlines() if "ipp://localhost:8631/" in ln), None),
        timeout=120,
    )
    name = added.split()[2].rstrip(":")
    S.choose(S.control("printers", "default-printer", role="combobox"), name)
    until("alice's default", lambda: name in alice("lpstat -d"))
    # The new printer's row is open: print a test page on it.
    S.press("Print Test Page")
    until("the page printed", lambda: name in machine.succeed("lpstat -W completed -o 2>&1 || true"), timeout=90)
    # Sharing it with the network.
    S.click(S.beside("Share on the network", "switch"))
    until("shared", lambda: "printer-is-shared=true" in machine.succeed(f"lpoptions -p {name}"))
    reopen("printers")
    assert S.control("printers", "default-printer", role="combobox").get("value") == name
    S.quit()



# The session, then each part.

machine.wait_for_unit("multi-user.target")
machine.wait_until_succeeds("ls /run/user/1000/ | grep -q '^aizome\\..*\\.sock$'", timeout=120)
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=60)
# The welcome tour stays out of the way.
machine.sleep(3)
for w in ipc("windows")["windows"]:
    if w["app_id"].startswith("org.aizome.Annai"):
        ipc(f"close {w['id']}")

with subtest("the session is up"):
    print(json.dumps(ipc("outputs")["outputs"], indent=1))
    shot("00-desktop")

ONLY = [w.strip().lower() for w in os.environ.get("AIZOME_TEST_ONLY", "").split(",") if w.strip()]
# Commands to look around with first (in development: AIZOME_TEST_PROBE,
# separated by ;;).
for probe in [p for p in os.environ.get("AIZOME_TEST_PROBE", "").split(";;") if p.strip()]:
    print("probe:", probe)
    if probe.strip().startswith("monitor:"):
        print(machine.send_monitor_command(probe.strip()[len("monitor:") :]))
    else:
        print(machine.execute(probe)[1])
for title, test in TESTS:
    if title.startswith("zprobe") and not ONLY:
        continue
    if ONLY and not any(w in title.split(":")[0].lower() for w in ONLY):
        continue
    with subtest(title):
        try:
            test()
        except Exception:
            shot("failed-" + test.__name__)
            print(machine.execute(f"ls -la {INSPECT}")[1])
            try:
                print("the shell shows:", shell_texts())
            except Exception as e:  # noqa: BLE001
                print("no shell:", e)
            if S.running() and S.pid:
                print(json.dumps(S.doc(), indent=None)[:20000])
            raise

