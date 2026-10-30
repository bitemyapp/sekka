# Native Yukimi in the shipped Tatami session, on disposable software graphics.
import csv
import io
import json
import shlex
import subprocess
import time

# What each step waits for, at most. The checks share a host with other VM
# checks, where drawing the next page or starting a process can take far
# longer than it does alone.
WAIT = 180


def user(command):
    return tatami.succeed(
        "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
        "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus sh -c "
        + shlex.quote(command)
    )


def hypr(command):
    return user("export HYPRLAND_INSTANCE_SIGNATURE=" + signature + "; " + command)


def clients():
    return json.loads(hypr("hyprctl -j clients"))


def window():
    return next((c for c in clients() if c["class"] == "org.aizome.Yukimi"), None)


def until(check, describe, timeout=WAIT):
    # Poll until check() is true; describe() says what was seen instead.
    deadline = time.monotonic() + timeout
    while not check():
        assert time.monotonic() < deadline, describe()
        time.sleep(0.5)


def ocr(name, *options):
    return subprocess.run(
        [TESSERACT, str(tatami.out_dir / (name + ".png")), "-", *options],
        capture_output=True, text=True, check=True,
    ).stdout


def capture(name):
    tatami.send_key("shift")
    time.sleep(3)
    tatami.screenshot(name)
    return ocr(name)


def capture_until(name, check, timeout=WAIT):
    # Capture until the screen shows what the last action asked for. OCR can
    # miss a word in one capture and every capture after it of the same
    # screen, so a miss gets a second, sparse-text reading as well. The check
    # then sees both readings: text it rules out counts if either has it.
    deadline = time.monotonic() + timeout
    while True:
        text = capture(name)
        if check(text):
            return text
        text += "\n" + ocr(name, "--psm", "11")
        if check(text):
            return text
        assert time.monotonic() < deadline, text


def layers():
    return [layer["namespace"] for monitor in json.loads(hypr("hyprctl -j layers")).values()
            for level in monitor["levels"].values() for layer in level]


def monitor():
    return json.loads(hypr("hyprctl -j monitors"))[0]


def is_mode(width, height, scale):
    output = monitor()
    return (output["width"], output["height"]) == (width, height) and abs(output["scale"] - scale) < 0.01


tatami.start()
tatami.wait_until_succeeds("test -S /run/user/1000/hypr/*/.socket.sock", timeout=180)
signature = tatami.succeed("basename /run/user/1000/hypr/*").strip()
tatami.wait_until_succeeds(
    "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
    "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus "
    "systemctl --user is-active tatami-waybar.service", timeout=120,
)
user("systemctl --user stop tatami-hypridle.service")
hypr("printf %s '{\"displays\":[{\"name\":\"Virtual-1\",\"enabled\":true,\"scale\":2}],"
     "\"main\":\"Virtual-1\",\"apply\":[\"Virtual-1\"]}' | tatami displays apply")
until(lambda: abs(monitor()["scale"] - 2) < 0.01, monitor)
# Launch through the real applications menu and keyboard search.
tatami.send_key("meta_l-alt-spc")
until(lambda: "walker" in layers(), layers)
tatami.send_chars("yukimi")
print(capture_until("launcher-yukimi", lambda text: "Package Manager" in text))
tatami.send_key("ret")
until(lambda: window() is not None, clients)
print(capture_until("native-overview", lambda text: "freshness" in text.casefold()))
# Starting up is over before the idle overview is measured.
time.sleep(15)
assert not window()["xwayland"], window()
assert user("pgrep -x weston || true").strip() == ""
print(json.dumps(window()))

# Observe a real rendered label before clicking; coordinates come from OCR in
# the capture, rather than depending on one font's glyph widths.
SCALE = 2


def click_word(name, word, *, sidebar=False, exact_case=False, aliases=()):
    deadline = time.monotonic() + WAIT
    while True:
        capture(name)
        data = ocr(name, "--psm", "11", "tsv")
        # Tesseract doesn't quote its fields; a word with a quote mark in it
        # is just a word.
        rows = list(csv.DictReader(io.StringIO(data), delimiter="\t", quoting=csv.QUOTE_NONE))
        candidates = []
        for row in rows:
            label = row["text"].strip()
            matches = label == word if exact_case else label.casefold() in {
                candidate.casefold() for candidate in (word, *aliases)
            }
            x, y = int(row["left"]) / SCALE, int(row["top"]) / SCALE
            if matches and (not sidebar or (x < 210 and y > 100)):
                candidates.append((x + int(row["width"]) / SCALE / 2,
                                   y + int(row["height"]) / SCALE / 2))
        if candidates:
            break
        assert time.monotonic() < deadline, (word, [row["text"] for row in rows if row["text"].strip()])
    x, y = candidates[-1] if not sidebar else candidates[0]
    hypr("hyprctl dispatch " + shlex.quote(f"hl.dsp.cursor.move({{x={x}, y={y}}})"))
    tatami.succeed("YDOTOOL_SOCKET=/run/ydotoold/socket ydotool mousemove -x 1 -y 0")
    time.sleep(0.2)
    tatami.succeed("YDOTOOL_SOCKET=/run/ydotoold/socket ydotool click 0xC0")
    time.sleep(1)


def measure(label, pid):
    tatami.succeed(f"python3 /etc/yukimi-measure.py --seconds 15 --output /tmp/{label}.json {pid}")
    tatami.copy_from_machine(f"/tmp/{label}.json")


pid = window()["pid"]
tatami.wait_for_unit("ydotoold.service")
measure("native-overview-resources", pid)
click_word("before-installed", "Installed", sidebar=True)
capture_until("native-installed", lambda text: "Installed" in text)
measure("native-installed-resources", pid)
# The command alias hands a page request to the same process and window.
# The History page's own heading shows it opened; the sidebar always names
# History, and OCR has missed both of those where the page was showing.
user("systemd-run --user --collect --quiet yukimi --page history")
capture_until("native-history", lambda text: "Every version" in text)
assert window()["pid"] == pid
assert len(clients()) == 1, clients()
for page in ["Storage", "Desktops", "Updates", "Overview"]:
    click_word("before-" + page.lower(), page, sidebar=True)
    capture_until("native-" + page.lower(), lambda text: page in text)

# Ctrl shortcuts work without changing Tatami's Super workspace bindings.
tatami.send_key("ctrl-3")
capture_until("native-discover", lambda text: "Find software" in text)
# The real catalog and package index; this is not a seeded screenshot fixture.
tatami.wait_until_succeeds(
    "test -s /home/demo/.cache/aizome/yukimi/packages-" + NIXPKGS_NAME + ".json", timeout=900,
)
tatami.send_chars("steam")
capture_until("discover-steam", lambda text: "steam" in text.casefold() and "packages" in text.casefold())
measure("native-discover-resources", pid)
# Editing shortcuts, including Tatami's normal Super+C/V forwarding.
tatami.send_key("ctrl-a")
tatami.send_key("meta_l-c")
wayland = json.loads(user("hyprctl instances -j"))[0]["wl_socket"]
until(lambda: user("WAYLAND_DISPLAY=" + shlex.quote(wayland) + " wl-paste --no-newline || true") == "steam",
      lambda: "Tatami's Copy command did not publish the selected search")
tatami.send_chars("no-such-package-yukimi-test")
tatami.send_key("ctrl-a")
tatami.send_key("meta_l-v")
capture_until("discover-pasted", lambda text: "Steam" in text and "no-such-package" not in text)
click_word("before-steam-details", "steam", exact_case=True)
capture_until("steam-details", lambda text: "32-bit graphics drivers" in text and "for everyone" in text)
before = len(clients())
click_word("before-disabled-try", "Try", aliases=("TryIt",))
assert len(clients()) == before
click_word("before-disabled-personal", "Me")
assert len(clients()) == before
assert not tatami.succeed("pgrep -f '[n]ix (shell|profile)' || true").strip()
configuration = tatami.succeed("sha256sum /etc/nixos/configuration.nix")
click_word("before-install-everyone", "Everyone")
capture_until("install-steam", lambda text: "install steam" in text.casefold())
tatami.send_key("esc")
time.sleep(1)
assert tatami.succeed("sha256sum /etc/nixos/configuration.nix") == configuration
tatami.fail("test -e /etc/nixos/yukimi.nix")
tatami.fail("pgrep -f '[y]ukimi-helper|[p]kexec'")
# Reach Tatami's real authentication agent, then dismiss its password dialog.
# This checks the installed policy/helper path without authorizing a change.
click_word("before-reopen-steam", "steam", exact_case=True)
click_word("before-authentication", "Everyone")
capture_until("confirm-authentication", lambda text: "install steam" in text.casefold())
tatami.send_key("ret")
tatami.wait_until_succeeds("pgrep -f '[p]kexec'", timeout=WAIT)
capture_until("tatami-authentication",
              lambda text: "password" in text.casefold() or "authenticate" in text.casefold())
tatami.send_key("esc")
# Agents differ: Tatami's agent reports dismissal as Not authorized, while
# others return Request dismissed. Both must leave a finished, harmless job.
capture_until("authentication-cancelled",
              lambda text: "Nothing was changed" in text and ("Cancelled" in text or "permission was denied" in text))
assert tatami.succeed("sha256sum /etc/nixos/configuration.nix") == configuration
tatami.fail("test -e /etc/nixos/yukimi.nix")
tatami.wait_until_fails("pgrep -f '[y]ukimi-helper|[p]kexec'", timeout=WAIT)
# The finished operation sheet has Close as its default button.
tatami.send_key("ret")
capture_until("after-authentication", lambda text: "Nothing was changed" not in text)

# Fractional scaling and a smaller 1280×800 monitor, with the app still open.
for scale, mode, label in [(1.25, "1920x1080@60", "fractional"), (1, "1280x800@60", "small")]:
    hypr("hyprctl eval " + shlex.quote(
        f'hl.monitor({{output="Virtual-1", mode="{mode}", scale={scale}}})'))
    SCALE = scale
    width, height = map(int, mode.split("@")[0].split("x"))
    until(lambda: is_mode(width, height, scale), monitor)
    tatami.send_key("ctrl-1")
    capture_until("native-" + label, lambda text: "freshness" in text.casefold())

    def fits():
        output = monitor()
        client = window()
        return (client["at"][0] >= 0 and client["at"][1] >= 0
                and client["at"][0] + client["size"][0] <= output["width"] / scale
                and client["at"][1] + client["size"][1] <= output["height"] / scale)

    # Yukimi's window is laid out again for the new monitor; it must fit.
    until(fits, lambda: (window(), monitor()))

tatami.send_key("ctrl-q")
tatami.wait_until_succeeds(f"test ! -e /proc/{pid}", timeout=WAIT)
assert not window(), clients()
tatami.succeed("test ! -S /run/user/1000/aizome-app.yukimi.sock")
tatami.fail("coredumpctl --no-legend list | grep -i yukimi")
