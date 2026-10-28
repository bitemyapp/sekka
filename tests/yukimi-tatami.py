# Native Yukimi in the shipped Tatami session, on disposable software graphics.
import json
import shlex
import subprocess
import time


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


def capture(name):
    tatami.send_key("shift")
    time.sleep(3)
    tatami.screenshot(name)
    return subprocess.run(
        [TESSERACT, str(tatami.out_dir / (name + ".png")), "-"],
        capture_output=True, text=True, check=True,
    ).stdout


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
time.sleep(3)
# Launch through the real applications menu and keyboard search.
tatami.send_key("meta_l-alt-spc")
time.sleep(2)
tatami.send_chars("yukimi")
print(capture("launcher-yukimi"))
tatami.send_key("ret")
until = time.monotonic() + 60
while window() is None:
    assert time.monotonic() < until, clients()
    time.sleep(1)
time.sleep(15)
print(capture("native-overview"))
assert not window()["xwayland"], window()
assert user("pgrep -x weston || true").strip() == ""
print(json.dumps(window()))

# Observe a real rendered label before clicking; coordinates come from OCR in
# the capture, rather than depending on one font's glyph widths.
import csv
import io

SCALE = 2


def click_word(name, word, *, sidebar=False, exact_case=False, aliases=()):
    capture(name)
    data = subprocess.run(
        [TESSERACT, str(tatami.out_dir / (name + ".png")), "-", "--psm", "11", "tsv"],
        capture_output=True, text=True, check=True,
    ).stdout
    candidates = []
    for row in csv.DictReader(io.StringIO(data), delimiter="\t"):
        label = row["text"].strip()
        matches = label == word if exact_case else label.casefold() in {
            candidate.casefold() for candidate in (word, *aliases)
        }
        x, y = int(row["left"]) / SCALE, int(row["top"]) / SCALE
        if matches and (not sidebar or (x < 210 and y > 100)):
            candidates.append((x + int(row["width"]) / SCALE / 2,
                               y + int(row["height"]) / SCALE / 2))
    assert candidates, (word, [row["text"] for row in csv.DictReader(io.StringIO(data), delimiter="\t")
                              if row["text"].strip()])
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
assert "Installed" in capture("native-installed")
measure("native-installed-resources", pid)
# The command alias hands a page request to the same process and window.
user("systemd-run --user --collect --quiet yukimi --page history")
time.sleep(2)
assert window()["pid"] == pid
assert len(clients()) == 1, clients()
assert "History" in capture("native-history")
for page in ["Storage", "Desktops", "Updates", "Overview"]:
    click_word("before-" + page.lower(), page, sidebar=True)
    assert page in capture("native-" + page.lower())

# Ctrl shortcuts work without changing Tatami's Super workspace bindings.
tatami.send_key("ctrl-3")
text = capture("native-discover")
assert "Find software" in text, text
# The real catalog and package index; this is not a seeded screenshot fixture.
tatami.wait_until_succeeds(
    "test -s /home/demo/.cache/aizome/yukimi/packages-" + NIXPKGS_NAME + ".json", timeout=900,
)
tatami.send_chars("steam")
text = capture("discover-steam")
assert "steam" in text.casefold() and "packages" in text.casefold(), text
measure("native-discover-resources", pid)
# Editing shortcuts, including Tatami's normal Super+C/V forwarding.
tatami.send_key("ctrl-a")
tatami.send_key("meta_l-c")
wayland = json.loads(user("hyprctl instances -j"))[0]["wl_socket"]
until = time.monotonic() + 10
while user("WAYLAND_DISPLAY=" + shlex.quote(wayland) + " wl-paste --no-newline || true") != "steam":
    assert time.monotonic() < until, "Tatami's Copy command did not publish the selected search"
    time.sleep(0.2)
tatami.send_chars("no-such-package-yukimi-test")
tatami.send_key("ctrl-a")
tatami.send_key("meta_l-v")
text = capture("discover-pasted")
assert "Steam" in text and "no-such-package" not in text, text
click_word("before-steam-details", "steam", exact_case=True)
text = capture("steam-details")
assert "32-bit graphics drivers" in text and "for everyone" in text, text
before = len(clients())
click_word("before-disabled-try", "Try", aliases=("TryIt",))
assert len(clients()) == before
click_word("before-disabled-personal", "Me")
assert len(clients()) == before
assert not tatami.succeed("pgrep -f '[n]ix (shell|profile)' || true").strip()
configuration = tatami.succeed("sha256sum /etc/nixos/configuration.nix")
click_word("before-install-everyone", "Everyone")
text = capture("install-steam")
assert "install steam" in text.casefold(), text
tatami.send_key("esc")
time.sleep(1)
assert tatami.succeed("sha256sum /etc/nixos/configuration.nix") == configuration
tatami.fail("test -e /etc/nixos/yukimi.nix")
tatami.fail("pgrep -f '[y]ukimi-helper|[p]kexec'")
# Reach Tatami's real authentication agent, then dismiss its password dialog.
# This checks the installed policy/helper path without authorizing a change.
click_word("before-reopen-steam", "steam", exact_case=True)
click_word("before-authentication", "Everyone")
assert "install steam" in capture("confirm-authentication").casefold()
tatami.send_key("ret")
tatami.wait_until_succeeds("pgrep -f '[p]kexec'", timeout=30)
text = capture("tatami-authentication")
assert "password" in text.casefold() or "authenticate" in text.casefold(), text
tatami.send_key("esc")
text = capture("authentication-cancelled")
# Agents differ: Tatami's agent reports dismissal as Not authorized, while
# others return Request dismissed. Both must leave a finished, harmless job.
assert "Nothing was changed" in text, text
assert "Cancelled" in text or "permission was denied" in text, text
assert tatami.succeed("sha256sum /etc/nixos/configuration.nix") == configuration
tatami.fail("test -e /etc/nixos/yukimi.nix")
tatami.fail("pgrep -f '[y]ukimi-helper|[p]kexec'")
# The finished operation sheet has Close as its default button.
tatami.send_key("ret")
assert "Nothing was changed" not in capture("after-authentication")

# Fractional scaling and a smaller 1280×800 monitor, with the app still open.
for scale, mode, label in [(1.25, "1920x1080@60", "fractional"), (1, "1280x800@60", "small")]:
    hypr("hyprctl eval " + shlex.quote(
        f'hl.monitor({{output="Virtual-1", mode="{mode}", scale={scale}}})'))
    SCALE = scale
    time.sleep(3)
    tatami.send_key("ctrl-1")
    text = capture("native-" + label)
    assert "freshness" in text.casefold(), text
    monitor = json.loads(hypr("hyprctl -j monitors"))[0]
    client = window()
    assert client["at"][0] >= 0 and client["at"][1] >= 0, client
    assert client["at"][0] + client["size"][0] <= monitor["width"] / scale, client
    assert client["at"][1] + client["size"][1] <= monitor["height"] / scale, client

tatami.send_key("ctrl-q")
tatami.wait_until_succeeds(f"test ! -e /proc/{pid}", timeout=30)
assert not window(), clients()
tatami.succeed("test ! -S /run/user/1000/aizome-app.yukimi.sock")
tatami.fail("coredumpctl --no-legend list | grep -i yukimi")
