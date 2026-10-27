"""Authenticate real virtual networks without touching any host interface."""

import json
import shlex
import time

APP = "org.aizome.NetworkAuthentication"
AGENT = "aizome-network-agent.service"
WIFI_PASSWORD = "wifi-test-password"
NM_APPLET = "pgrep -u alice -f '(^|/)[.]?nm-applet(-wrapped)?( |$)'"
APPLET_UNITS = ["nm-applet.service", "app-nm\\x2dapplet@autostart.service"]


def alice(command, check=True):
    script = (
        "export XDG_RUNTIME_DIR=/run/user/1000 "
        "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus; "
        "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
        "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; " + command
    )
    command = "su - alice -c " + shlex.quote(script)
    return machine.succeed(command) if check else machine.execute(command)


def skips(unit):
    """How often systemd has skipped this unit for its ExecCondition. (Not
    its Result: an inactive unit nothing refers to is unloaded, and that
    resets it.)"""
    journal = alice("journalctl --user --no-pager -o cat -u " + shlex.quote(unit))
    return journal.count("Skipped due to 'exec-condition'.")


def refusals():
    """How often Aizome's condition has refused nm-applet. (Not by unit:
    journald may not tell which unit a process that ended at once was in.)"""
    journal = alice("journalctl --user --no-pager -o cat")
    return journal.count("nm-applet is not started in Aizome")


def until(description, check, timeout=60):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.5)
    machine.screenshot("failure")
    raise AssertionError("Timed out: " + description)


def windows():
    return json.loads(alice("aizomectl --json windows"))["windows"]


def prompt():
    return next((w for w in windows() if w["app_id"] == APP), None)


def document(w):
    files = machine.succeed(
        f"cat /run/user/1000/inspect/{APP}.{w['pid']}.*.json 2>/dev/null || true"
    )
    return next((json.loads(line) for line in files.splitlines() if line.strip()), None)


def click(w, element):
    x, y, width, height = element["rect"]
    g = w["geometry"]
    alice(
        f"aizomectl focus {w['id']} && aizomectl input motion "
        f"{g['x'] + x + width / 2} {g['y'] + y + height / 2} && aizomectl input click left"
    )


def answer(secret, screenshot=None):
    w = until("credential dialog", prompt)
    doc = until("credential controls", lambda: document(w))
    fields = [e for e in doc["elements"] if e.get("role") == "passwordinput"]
    assert len(fields) == 1, doc
    click(w, fields[0])
    alice(
        "aizomectl input chord super+a && aizomectl input type " + shlex.quote(secret)
    )
    time.sleep(0.4)
    doc = document(w)
    assert secret not in json.dumps(doc), "Password leaked into inspection"
    if screenshot:
        machine.screenshot(screenshot)
    click(w, next(e for e in doc["elements"] if e.get("text") == "Connect"))
    until("dialog closes", lambda: not any(x["pid"] == w["pid"] for x in windows()))


def cancel_prompt():
    w = until("credential dialog", prompt)
    doc = until("credential controls", lambda: document(w))
    click(w, next(e for e in doc["elements"] if e.get("text") == "Cancel"))
    until("cancelled dialog closes", lambda: not prompt())


def activate(name):
    machine.succeed("rm -f /tmp/auth-result /tmp/auth-log")
    # No --ask: only the Aizome agent can satisfy these requests.
    alice(
        "sh -c "
        + shlex.quote(
            "nmcli -w 100 connection up "
            + shlex.quote(name)
            + " >/tmp/auth-log 2>&1; echo $? >/tmp/auth-result"
        )
        + " </dev/null >/dev/null 2>&1 &"
    )


def finished(success=True):
    until(
        "NetworkManager activation result",
        lambda: machine.execute("test -f /tmp/auth-result")[0] == 0,
        110,
    )
    result = machine.succeed("cat /tmp/auth-result").strip()
    assert (result == "0") == success, machine.succeed("cat /tmp/auth-log")


def down(name):
    alice("nmcli connection down " + shlex.quote(name))


def wifi(name, flags, ssid="Aizome WPA2", key="wpa-psk"):
    alice(
        f"nmcli connection add type wifi ifname wlan1 con-name {name} ssid {shlex.quote(ssid)} "
        f"connection.autoconnect no ipv4.never-default yes ipv6.method disabled "
        f"wifi-sec.key-mgmt {key} wifi-sec.psk-flags {flags}"
    )


def uuid(name):
    return alice("nmcli -g connection.uuid connection show " + name).strip()


def saved(name, setting="802-11-wireless-security", key="psk"):
    return alice(
        f"secret-tool lookup connection-uuid {uuid(name)} setting-name {setting} setting-key {key}",
        False,
    )


def seed(name, value):
    alice(
        f"printf %s {shlex.quote(value)} | secret-tool store --label='Other desktop Wi-Fi' "
        f"connection-uuid {uuid(name)} setting-name 802-11-wireless-security setting-key psk"
    )


start_all()
machine.wait_for_unit("greetd.service")
# Exercise the real PAM login and keyring unlock, without a second daemon
# racing the desktop's Secret Service activation.
machine.wait_until_tty_matches("1", "login: ")
machine.send_chars("alice\n")
machine.wait_until_tty_matches("1", "Password: ")
machine.send_chars("aizome\n")
machine.wait_until_succeeds(
    "test -S /run/user/1000/wayland-1 || test -S /run/user/1000/wayland-0"
)
until(
    "Aizome session",
    lambda: alice("systemctl --user is-active " + AGENT, False)[0] == 0,
)
until(
    "agent registration",
    lambda: (
        "Network credential agent registered"
        in alice("journalctl --user -u " + AGENT + " --no-pager")
    ),
)
machine.wait_for_unit("network-test-ap.service")
machine.wait_for_unit("network-test-vpn.service")
for w in windows():
    if w["app_id"] == "org.aizome.Annai":
        alice(f"aizomectl close {w['id']}")
# The login keyring must already be unlocked by PAM.
alice(
    "printf %s keyring-probe | timeout 10 secret-tool store --label=Probe aizome-test ready"
)
assert alice("secret-tool lookup aizome-test ready").strip() == "keyring-probe"
agent_pid = alice("systemctl --user show -p MainPID --value " + AGENT).strip()

with subtest("Both nm-applet startup paths are skipped by their condition in Aizome"):
    # The session starts programs.nm-applet's unit and the one generated
    # from the XDG autostart entry, and Aizome's ExecCondition skips both.
    for unit in APPLET_UNITS:
        until(unit + " is skipped", lambda unit=unit: skips(unit) > 0)
    until("Aizome's condition refuses both", lambda: refusals() >= 2)
    before = (skips("nm-applet.service"), refusals())
    alice("systemctl --user start nm-applet.service")
    until(
        "an explicit start is skipped too",
        lambda: skips("nm-applet.service") > before[0] and refusals() > before[1],
    )
    machine.fail(NM_APPLET)
    # The shared entry stays as it is, for other desktops.
    machine.succeed("grep -q '^Exec=nm-applet' /etc/xdg/autostart/nm-applet.desktop")

with subtest("WPA2 ask every time, user cancellation, and NM cancellation"):
    wifi("ask", 2)
    activate("ask")
    answer(WIFI_PASSWORD, "wifi-authentication")
    finished()
    assert saved("ask")[0] != 0
    down("ask")
    activate("ask")
    cancel_prompt()
    finished(False)
    activate("ask")
    until("pending Wi-Fi dialog", prompt)
    alice("nmcli device disconnect wlan1")
    until("NM cancellation closes the dialog", lambda: not prompt())
    finished(False)
    assert saved("ask")[0] != 0

with subtest("Agent-owned secrets are saved, reused and interoperable"):
    wifi("remember", 1)
    activate("remember")
    answer(WIFI_PASSWORD)
    finished()
    assert saved("remember")[1].strip() == WIFI_PASSWORD
    down("remember")
    activate("remember")
    finished()
    assert not prompt()
    down("remember")
    wifi("existing", 1)
    seed("existing", WIFI_PASSWORD)
    activate("existing")
    finished()
    assert not prompt()
    down("existing")

with subtest("Wrong saved Wi-Fi password requests a replacement"):
    seed("remember", "wrong-password")
    activate("remember")
    answer(WIFI_PASSWORD)
    finished()
    assert saved("remember")[1].strip() == WIFI_PASSWORD
    down("remember")

with subtest("System-owned Wi-Fi credentials do not prompt or migrate"):
    wifi("system", 0)
    alice("nmcli connection modify system wifi-sec.psk " + WIFI_PASSWORD)
    activate("system")
    finished()
    assert not prompt()
    assert saved("system")[0] != 0
    assert (
        alice("nmcli -g 802-11-wireless-security.psk-flags connection show system")
        .strip()
        .startswith("0")
    )
    down("system")

with subtest("WPA3 SAE and enterprise PEAP authenticate through native UI"):
    wifi("sae", 2, "Aizome WPA3", "sae")
    activate("sae")
    answer(WIFI_PASSWORD)
    finished()
    down("sae")
    alice(
        "nmcli connection add type wifi ifname wlan1 con-name enterprise ssid 'Aizome Enterprise' "
        "connection.autoconnect no ipv4.never-default yes ipv6.method disabled "
        "wifi-sec.key-mgmt wpa-eap 802-1x.eap peap 802-1x.identity enterprise-user "
        "802-1x.phase2-auth mschapv2 802-1x.ca-cert /etc/network-test-ca.crt "
        "802-1x.domain-suffix-match auth.test 802-1x.password-flags 2"
    )
    activate("enterprise")
    answer("enterprise-password", "enterprise-authentication")
    finished()
    assert saved("enterprise", "802-1x", "password")[0] != 0
    down("enterprise")

with subtest(
    "Credential dialog survives shell reload, agent re-registers after NM restart"
):
    activate("ask")
    w = until("pending dialog", prompt)
    alice("aizomectl reload --shell")
    assert (
        alice("systemctl --user show -p MainPID --value " + AGENT).strip() == agent_pid
    )
    assert prompt()["pid"] == w["pid"]
    answer(WIFI_PASSWORD)
    finished()
    down("ask")
    registrations = alice("journalctl --user -u " + AGENT + " --no-pager").count(
        "agent registered"
    )
    machine.succeed("systemctl restart NetworkManager")
    until(
        "agent registers again",
        lambda: (
            alice("journalctl --user -u " + AGENT + " --no-pager").count(
                "agent registered"
            )
            > registrations
        ),
    )
    activate("ask")
    answer(WIFI_PASSWORD)
    finished()
    down("ask")


def saves():
    return alice("journalctl --user -u " + AGENT + " --no-pager").count(
        "Saved network credentials follow a profile update"
    )


def modify(name, change):
    before = saves()
    alice(f"nmcli connection modify {name} {change}")
    # NetworkManager asks agents to save after every update.
    until("the agent saves after the update", lambda: saves() > before)


with subtest("Updating a profile without its password keeps the saved one"):
    # NetworkManager has restarted and not activated "existing" since, so
    # an update that changes no secret has none to send the agent.
    modify("existing", "connection.autoconnect-priority 1")
    assert saved("existing")[1].strip() == WIFI_PASSWORD
    activate("existing")
    finished()
    assert not prompt()
    down("existing")
    # Asking every time forgets it.
    modify("existing", "wifi-sec.psk-flags 2")
    assert saved("existing")[0] != 0

with subtest("OpenVPN plugin authenticates, retries, saves and reuses passwords"):
    machine.succeed(
        "cat > /tmp/test.ovpn <<'EOF'\nclient\ndev tun\nproto udp\nremote 127.0.0.1 1194\n"
        "ca /run/network-test/server.crt\nremote-cert-tls server\nauth-user-pass\n"
        "route-nopull\ndata-ciphers AES-256-GCM\nEOF"
    )
    alice("nmcli connection import type openvpn file /tmp/test.ovpn")
    alice(
        "nmcli connection modify test connection.id vpn connection.autoconnect no "
        "ipv4.never-default yes +vpn.data 'username=vpnuser,password-flags=2'"
    )
    activate("vpn")
    answer("vpn-test-password", "vpn-authentication")
    finished()
    assert saved("vpn", "vpn", "password")[0] != 0
    down("vpn")
    activate("vpn")
    cancel_prompt()
    finished(False)
    activate("vpn")
    answer("wrong-vpn-password")
    # The OpenVPN plugin asks again after AUTH_FAILED.
    answer("vpn-test-password")
    finished()
    down("vpn")
    # A fresh UUID keeps this test independent of NM's in-memory secrets
    # from the ask-every-time connection.
    alice("nmcli connection import type openvpn file /tmp/test.ovpn")
    alice(
        "nmcli connection modify test connection.id vpn-remember connection.autoconnect no "
        "ipv4.never-default yes +vpn.data 'username=vpnuser,password-flags=1'"
    )
    activate("vpn-remember")
    answer("vpn-test-password")
    finished()
    assert saved("vpn-remember", "vpn", "password")[1].strip() == "vpn-test-password"
    down("vpn-remember")
    activate("vpn-remember")
    finished()
    assert not prompt()
    down("vpn-remember")

with subtest("Deletion clears keyring credentials and logs contain no passwords"):
    old_uuid = uuid("remember")
    alice("nmcli connection delete remember")
    until(
        "deleted secret removed",
        lambda: (
            alice(
                f"secret-tool lookup connection-uuid {old_uuid} setting-name 802-11-wireless-security setting-key psk",
                False,
            )[0]
            != 0
        ),
    )
    journal = alice("journalctl --user -u " + AGENT + " --no-pager")
    for secret in [WIFI_PASSWORD, "enterprise-password", "vpn-test-password"]:
        assert secret not in journal
    machine.fail(NM_APPLET)
    assert (
        alice("systemctl --user show -p MainPID --value " + AGENT).strip() == agent_pid
    )

with subtest("Other desktops can still start nm-applet"):
    # Only the VM's user manager changes, after authentication tests.
    alice("systemctl --user stop " + AGENT)
    alice(
        "systemctl --user set-environment XDG_SESSION_DESKTOP=xfce XDG_CURRENT_DESKTOP=XFCE"
    )
    for unit in APPLET_UNITS:
        alice("systemctl --user start " + shlex.quote(unit))
        machine.wait_until_succeeds(NM_APPLET)
        alice("systemctl --user stop " + shlex.quote(unit))
        machine.fail(NM_APPLET)

machine.screenshot("network-authentication-complete")
