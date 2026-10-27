"""Manual live-service component proof; deterministic ordering is covered in Rust."""

import json
import shlex
import time

APP = "org.aizome.Tenki"
INSPECT = "/run/user/1000/tenki-search-inspect"


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


def window():
    return next(
        (
            w
            for w in json.loads(user("aizomectl --json windows"))["windows"]
            if w["app_id"] == APP
        ),
        None,
    )


def elements():
    w = window()
    if w is None:
        return []
    docs = user(
        f"for f in {INSPECT}/{APP}.{w['pid']}.*.json; do "
        'if test -f "$f"; then cat "$f"; printf "\\n"; fi; done'
    ).splitlines()
    return next(
        (json.loads(d)["elements"] for d in docs if json.loads(d)["kind"] == "window"),
        [],
    )


def find(**attrs):
    return next(
        (
            e
            for e in elements()
            if e["visible"] and all(e.get(k) == v for k, v in attrs.items())
        ),
        None,
    )


def until(what, check, timeout=35):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.2)
    machine.screenshot("failed")
    machine.succeed("journalctl _UID=1000 --no-pager > /tmp/tenki-search-failed.log")
    machine.copy_from_machine("/tmp/tenki-search-failed.log")
    raise AssertionError(what)


def click(**attrs):
    e = until(str(attrs), lambda: find(**attrs))
    g = window()["geometry"]
    x, y, width, height = e["shown"]
    user(f"aizomectl input motion {g['x'] + x + width / 2} {g['y'] + y + height / 2}")
    user("aizomectl input click")
    time.sleep(0.2)


def type_text(text):
    user("aizomectl input type " + shlex.quote(text))


def chord(keys):
    user("aizomectl input chord " + keys)
    time.sleep(0.2)


def places():
    return json.loads(user("cat ~/.local/share/aizome/tenki/places.json"))["places"]


machine.start()
machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=120)
# Fail preflight explicitly if the live service is unavailable; no product pass
# or deterministic negative is inferred from a service/network failure.
machine.succeed(
    "curl --fail --silent --show-error --max-time 20 "
    "'https://geocoding-api.open-meteo.com/v1/search?name=Kyoto&count=1' > /tmp/service-preflight.json"
)
machine.succeed(
    "python3 -c \"import json; assert json.load(open('/tmp/service-preflight.json'))['results'][0]['name'] == 'Kyoto'\""
)
machine.copy_from_machine("/tmp/service-preflight.json")
device = json.loads(machine.succeed("ip -j route show default"))[0]["dev"]
machine.succeed(f"tc qdisc replace dev {shlex.quote(device)} root netem delay 350ms")
machine.succeed(f"tc -s qdisc show dev {shlex.quote(device)} > /tmp/network-delay.txt")
machine.copy_from_machine("/tmp/network-delay.txt")
user("mkdir -p " + INSPECT)
user(f"aizomectl spawn env AIZOME_KIT_INSPECT={INSPECT} aizome-tenki")
until("Tenki search field", lambda: find(name="search"))

with subtest("query changes while its prefix request is running"):
    click(name="search")
    type_text("Kyo")
    time.sleep(0.7)  # Past debounce, before the deliberately delayed HTTPS reply.
    machine.screenshot("01-prefix-in-flight")
    type_text("to")
    # The text field itself also paints "Kyoto"; require a result row.
    until("latest Kyoto result row", lambda: find(name="result-0"))
    machine.screenshot("02-latest-query-results")
    click(name="result-0")
    until("Kyoto added", lambda: window()["title"] == "Kyoto")
    assert sum(p["name"] == "Kyoto" for p in places()) == 1
    machine.screenshot("03-kyoto-added")

with subtest("clear in-flight query and short input then recover with Return"):
    click(name="search")
    type_text("Tok")
    time.sleep(0.7)
    chord("super+a")
    user("aizomectl input key BackSpace")
    type_text("T")
    until("short query prompt", lambda: find(text="Type a city's name."))
    machine.screenshot("04-short-query")
    time.sleep(2)
    assert find(text="Type a city's name.") is not None
    type_text("okyo")
    user("aizomectl input key Return")
    until("Return adds Tokyo after reply", lambda: window()["title"] == "Tokyo")
    assert sum(p["name"] == "Tokyo" for p in places()) == 1
    machine.screenshot("05-return-added-tokyo")

with subtest("normal quit and reopen retain saved places"):
    saved = places()
    chord("super+q")
    until("Tenki exited", lambda: window() is None)
    user(f"aizomectl spawn env AIZOME_KIT_INSPECT={INSPECT} aizome-tenki")
    until(
        "Tokyo selected after relaunch",
        lambda: window() and window()["title"] == "Tokyo",
    )
    assert places() == saved
    machine.screenshot("06-reopened")
    chord("super+q")
    until("normal final quit", lambda: window() is None)

machine.succeed(f"tc qdisc del dev {shlex.quote(device)} root")
machine.succeed("journalctl _UID=1000 --no-pager > /tmp/tenki-live-search.log")
machine.copy_from_machine("/tmp/tenki-live-search.log")
machine.fail("coredumpctl --no-pager list aizome-tenki")
