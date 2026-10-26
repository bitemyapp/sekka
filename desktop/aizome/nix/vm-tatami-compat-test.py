"""Real Aizome and Tatami sessions sharing one user and NetworkManager."""

import json
import shlex
import time

AGENT = "aizome-network-agent.service"
PASSWORD = "wifi-test-password"
NM_APPLET = "pgrep -u alice -f '(^|/)[.]?nm-applet(-wrapped)?( |$)'"
AUTOSTART = "app-nm\\x2dapplet@autostart.service"


def alice(command, check=True):
    script = (
        "export XDG_RUNTIME_DIR=/run/user/1000 "
        "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus; "
        "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
        "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; "
        "export HYPRLAND_INSTANCE_SIGNATURE=$(ls /run/user/1000/hypr 2>/dev/null | head -n1); "
        + command
    )
    command = "su - alice -c " + shlex.quote(script)
    return machine.succeed(command) if check else machine.execute(command)


def until(description, check, timeout=60):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        value = check()
        if value:
            return value
        time.sleep(0.5)
    machine.screenshot("failure")
    raise AssertionError("Timed out: " + description)


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


def login():
    machine.wait_until_tty_matches("1", "login: ")
    machine.send_chars("alice\n")
    machine.wait_until_tty_matches("1", "Password: ")
    machine.send_chars("aizome\n")


def active(unit):
    return alice("systemctl --user is-active " + shlex.quote(unit), False)[0] == 0


def desktop_environment():
    return alice("systemctl --user show-environment")


def launch(command):
    alice(command + " </dev/null >/tmp/compat-launch.log 2>&1 &")


def wifi_active():
    return alice("nmcli -g GENERAL.STATE device show wlan1").startswith(
        "100"
    ) and "yes:Aizome WPA2" in alice(
        "nmcli -t -f ACTIVE,SSID device wifi list --rescan no"
    )


def network_panel(selection):
    launch("tatami network")
    # NetworkManager may scan before Walker opens the panel.
    until(
        "Tatami network panel opens",
        lambda: '"namespace": "walker"' in alice("hyprctl -j layers"),
    )
    time.sleep(0.5)
    machine.send_chars(selection)
    machine.send_key("ret")


def activate(name):
    machine.succeed("rm -f /tmp/compat-result /tmp/compat-auth.log")
    launch(
        "sh -c "
        + shlex.quote(
            "nmcli -w 60 connection up "
            + shlex.quote(name)
            + " >/tmp/compat-auth.log 2>&1; echo $? >/tmp/compat-result"
        )
    )


def connected():
    machine.wait_for_file("/tmp/compat-result", timeout=90)
    assert machine.succeed("cat /tmp/compat-result").strip() == "0", machine.succeed(
        "cat /tmp/compat-auth.log"
    )


start_all()
machine.wait_for_unit("greetd.service")
login()
until("Aizome agent starts", lambda: active(AGENT))
until(
    "Aizome agent registers",
    lambda: "agent registered" in alice("journalctl --user -u " + AGENT),
)
machine.wait_for_unit("network-test-ap.service")
machine.wait_for_unit("network-test-vpn.service")
manager_pid = machine.succeed(
    "systemctl show -p MainPID --value user@1000.service"
).strip()

with subtest("Aizome and Tatami retain their separate startup policy"):
    assert "XDG_CURRENT_DESKTOP=Aizome\n" in desktop_environment()
    machine.succeed(
        "grep -q '^NotShowIn=.*Tatami;' /etc/xdg/autostart/nm-applet.desktop"
    )
    # Tatami's NotShowIn doesn't name Aizome: Aizome's own ExecCondition
    # skips the autostart unit, and an explicit start too.
    until("the autostart unit is skipped", lambda: skips(AUTOSTART) > 0)
    until("Aizome's condition refuses it", lambda: refusals() >= 1)
    before = (skips("nm-applet.service"), refusals())
    alice("systemctl --user start nm-applet.service")
    until(
        "an explicit start is skipped",
        lambda: skips("nm-applet.service") > before[0] and refusals() > before[1],
    )
    machine.fail(NM_APPLET)
    alice(
        "printf %s shared-keyring-value | timeout 10 secret-tool store "
        "--label=Compatibility aizome-tatami shared"
    )

with subtest("Aizome saves a VPN credential for use by Tatami's optional applet"):
    machine.succeed(
        "cat > /tmp/compat.ovpn <<'EOF'\nclient\ndev tun\nproto udp\nremote 127.0.0.1 1194\n"
        "ca /run/network-test/server.crt\nremote-cert-tls server\nauth-user-pass\n"
        "route-nopull\ndata-ciphers AES-256-GCM\nEOF"
    )
    alice("nmcli connection import type openvpn file /tmp/compat.ovpn")
    alice(
        "nmcli connection modify compat connection.id compat-saved connection.autoconnect no "
        "ipv4.never-default yes +vpn.data 'username=vpnuser,password-flags=1'"
    )
    activate("compat-saved")
    until(
        "Aizome VPN authentication dialog",
        lambda: any(
            w["app_id"] == "org.aizome.NetworkAuthentication"
            for w in json.loads(alice("aizomectl --json windows"))["windows"]
        ),
    )
    machine.send_chars("vpn-test-password")
    machine.send_key("ret")
    connected()
    vpn_uuid = alice("nmcli -g connection.uuid connection show compat-saved").strip()
    assert (
        alice(
            f"secret-tool lookup connection-uuid {vpn_uuid} setting-name vpn setting-key password"
        ).strip()
        == "vpn-test-password"
    )
    alice("nmcli connection down compat-saved")

with subtest("Logging out of Aizome stops its agent and clears its environment"):
    machine.succeed("echo tatami > /run/compatibility-session")
    alice("systemctl --user stop aizome.service")
    until("agent stops at logout", lambda: not active(AGENT))
    until(
        "Aizome environment is removed",
        lambda: (
            "/etc/xdg/aizome" not in desktop_environment()
            and "XDG_CURRENT_DESKTOP=Aizome" not in desktop_environment()
            and "XDG_SESSION_DESKTOP=aizome" not in desktop_environment()
        ),
    )
    assert (
        machine.succeed("systemctl show -p MainPID --value user@1000.service").strip()
        == manager_pid
    )

login()
until("Tatami compositor", lambda: active("wayland-wm@tatami.service"), 120)
until("Tatami desktop", lambda: active("tatami-waybar.service"), 120)

with subtest("Tatami services, theme, portals and keyring work with Aizome installed"):
    env = desktop_environment()
    assert "XDG_CURRENT_DESKTOP=Hyprland:Tatami\n" in env, env
    assert "/etc/xdg/aizome" not in env, env
    assert "DCONF_PROFILE=tatami\n" in env, env
    assert not active(AGENT)
    assert not active("aizome-session.target")
    machine.fail(NM_APPLET)
    for unit in [
        "tatami-waybar",
        "tatami-mako",
        "tatami-swaybg",
        "tatami-swayosd",
        "tatami-hypridle",
        "tatami-polkit",
        "tatami-elephant",
        "tatami-walker",
        "tatami-udiskie",
        "xdg-desktop-portal",
    ]:
        until(unit + " runs", lambda unit=unit: active(unit + ".service"))
    assert (
        alice("secret-tool lookup aizome-tatami shared").strip()
        == "shared-keyring-value"
    )
    assert json.loads(alice("hyprctl -j monitors")), "Tatami has no output"
    assert alice("hyprctl configerrors").strip() in ["", "ok"], alice(
        "hyprctl configerrors"
    )
    machine.screenshot("tatami-desktop")

with subtest("Tatami's own Wi-Fi panel joins and reconnects without an applet"):
    network_panel("Aizome WPA2")
    machine.wait_until_succeeds("pgrep -u alice -f '[w]alker.*--password'")
    machine.send_chars(PASSWORD)
    machine.screenshot("tatami-wifi-password")
    machine.send_key("ret")
    until("Tatami Wi-Fi connects", wifi_active, 60)
    assert not active(AGENT)
    machine.fail(NM_APPLET)
    machine.succeed(
        "test -z \"$(find /run/user/1000 -maxdepth 1 -name 'tatami-wifi-*')\""
    )
    alice("nmcli connection down 'Aizome WPA2'")
    network_panel("Aizome WPA2")
    until("saved Tatami Wi-Fi reconnects", wifi_active)
    machine.screenshot("tatami-wifi-connected")

with subtest("Tatami handles Wi-Fi cancellation, WPA3 and the radio switch"):
    # Keep the previous AP from reconnecting while the virtual radio scans
    # for SAE. Each authentication scenario gets an idle station.
    alice("nmcli connection modify 'Aizome WPA2' connection.autoconnect no")
    alice("nmcli connection down 'Aizome WPA2'")
    network_panel("Aizome WPA3")
    machine.wait_until_succeeds("pgrep -u alice -f '[w]alker.*--password'")
    machine.send_key("esc")
    machine.wait_until_fails("pgrep -u alice -f '[w]alker.*--password'")
    assert alice("nmcli connection show 'Aizome WPA3'", False)[0] != 0
    network_panel("Aizome WPA3")
    machine.wait_until_succeeds("pgrep -u alice -f '[w]alker.*--password'")
    machine.send_chars(PASSWORD)
    machine.send_key("ret")
    until(
        "Tatami WPA3 connects",
        lambda: (
            alice("nmcli -g GENERAL.STATE device show wlan1").startswith("100")
            and "yes:Aizome WPA3"
            in alice("nmcli -t -f ACTIVE,SSID device wifi list --rescan no")
        ),
    )
    network_panel("Turn Wi-Fi off")
    until("Wi-Fi radio off", lambda: alice("nmcli radio wifi").strip() == "disabled")
    network_panel("Turn Wi-Fi on")
    until("Wi-Fi radio on", lambda: alice("nmcli radio wifi").strip() == "enabled")
    machine.fail(NM_APPLET)
    assert not active(AGENT)

with subtest("Network settings, DNS, notifications, terminal and displays still open"):
    launch("nm-connection-editor")
    until(
        "network editor opens",
        lambda: any(
            "nm-connection-editor" in (w.get("class", "") + w.get("initialClass", ""))
            for w in json.loads(alice("hyprctl -j clients"))
        ),
    )
    # Tatami changes the first active wired/Wi-Fi connection; the VM also
    # has a wired interface, which can precede its virtual Wi-Fi.
    primary_uuid = next(
        line.split(":")[0]
        for line in alice("nmcli -t -f UUID,TYPE connection show --active").splitlines()
        if line.split(":")[1] in ["802-3-ethernet", "802-11-wireless"]
    )
    alice("tatami dns cloudflare")
    assert "1.1.1.1" in alice("nmcli -g ipv4.dns connection show uuid " + primary_uuid)
    alice("tatami dns dhcp")
    launch("tatami terminal")
    until(
        "terminal opens",
        lambda: any(
            "foot" in w.get("class", "")
            for w in json.loads(alice("hyprctl -j clients"))
        ),
    )
    launch("tatami display")
    until(
        "Displays opens",
        lambda: any(
            "tatami" in w.get("class", "").lower()
            for w in json.loads(alice("hyprctl -j clients"))
        ),
    )
    alice("notify-send 'Tatami compatibility' 'Notifications still work'")
    assert "Tatami compatibility" in alice("makoctl list")
    machine.screenshot("tatami-applications")

with subtest(
    "Tatami's optional nm-applet authenticates VPN with Aizome's saved credential"
):
    # Clear NetworkManager's in-memory secrets; the applet must use the
    # login keyring rather than a credential left in the daemon's cache.
    machine.succeed("systemctl restart NetworkManager")
    alice("systemctl --user start nm-applet.service")
    machine.wait_until_succeeds(NM_APPLET)
    assert not active(AGENT)
    activate("compat-saved")
    connected()
    alice("nmcli connection down compat-saved")
    # Exercise an interactive plugin prompt too, under the Tatami session.
    alice("nmcli connection import type openvpn file /tmp/compat.ovpn")
    alice(
        "nmcli connection modify compat connection.id compat-ask "
        "connection.autoconnect no ipv4.never-default yes "
        "+vpn.data 'username=vpnuser,password-flags=2'"
    )
    activate("compat-ask")
    until(
        "Tatami applet VPN dialog",
        lambda: any(
            "nm-applet" in w.get("class", "").lower()
            for w in json.loads(alice("hyprctl -j clients"))
        ),
    )
    machine.send_chars("vpn-test-password")
    machine.screenshot("tatami-vpn-password")
    machine.send_key("ret")
    connected()
    alice("nmcli connection down compat-ask")
    alice("systemctl --user stop nm-applet.service")
    machine.fail(NM_APPLET)

with subtest("Tatami logout and the return to Aizome do not leak session services"):
    machine.succeed("echo aizome > /run/compatibility-session")
    alice("uwsm stop", False)
    until("Tatami stops", lambda: not active("wayland-wm@tatami.service"))
    login()
    until("Aizome agent returns", lambda: active(AGENT), 120)
    env = desktop_environment()
    assert "XDG_CURRENT_DESKTOP=Aizome\n" in env, env
    assert "DCONF_PROFILE=aizome\n" in env, env
    assert "/etc/xdg/tatami" not in env, env
    assert not active("tatami-waybar.service")
    assert not active("tatami-polkit.service")
    machine.fail(NM_APPLET)
    assert (
        alice("secret-tool lookup aizome-tatami shared").strip()
        == "shared-keyring-value"
    )
    alice("nmcli connection down 'Aizome WPA2'", False)
    alice("nmcli -w 30 connection up 'Aizome WPA2'")
    assert wifi_active()
    machine.screenshot("returned-to-aizome")
