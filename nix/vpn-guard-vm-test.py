import re
import shlex

WEB4 = "203.0.113.10"
WEB6 = "2001:db8:ffff::10"
ISP4 = "203.0.113.1"
VPN4 = "203.0.113.20"
VPN6 = "2001:db8:ffff::20"
ROUTER4 = "192.168.1.1"
ROUTER6 = "2001:db8:1::1"
# The table sekka-vpn-guard keeps its unreachable route in (guard.rs, TABLE).
TABLE = "5458759"
# A web address with both an AAAA and an A record, as most sites have.
BOTH = f"--resolve 'whoami.test:80:[{WEB6}],{WEB4}' http://whoami.test/"
ONLY4 = f"-4 http://{WEB4}/"
ONLY6 = f"-6 http://[{WEB6}]/"


def fetch(node, arguments: str) -> tuple[str, float]:
    """Who the web host saw, or how the request failed, and the seconds it took."""
    status, out = node.execute(f"curl -sS --max-time 8 -w ' after %{{time_total}} s' {arguments} 2>&1")
    text = " ".join(out.split())
    timed = re.search(r" ?after ([0-9.]+) s$", text)
    assert timed, text
    answer = text[: timed.start()]
    return (answer if status == 0 else "FAILED: " + answer), float(timed.group(1))


def arrives_from(node, arguments: str, wanted: list[str]) -> None:
    """The request is answered at once and came from one of the wanted addresses."""
    answer, seconds = fetch(node, arguments)
    assert answer in wanted and seconds < 2, (arguments, answer, seconds, wanted)


def refused_at_once(node, arguments: str) -> None:
    """The request fails without waiting for a timeout."""
    answer, seconds = fetch(node, arguments)
    assert answer.startswith("FAILED") and seconds < 2, (arguments, answer, seconds)


def own_addresses(node) -> list[str]:
    return node.succeed("ip -6 -o addr show dev eth1 scope global | awk '{print $4}' | cut -d/ -f1").split()


def home_network_answers(node) -> None:
    link_local = node.succeed("ip -6 route show default dev eth1 | awk 'NR==1 {print $3}'").strip()
    node.succeed(
        f"ping -4 -c 1 -W 2 {ROUTER4}",
        f"ping -6 -c 1 -W 2 {ROUTER6}",
        f"ping -6 -c 1 -W 2 {link_local}%eth1",
    )


def table(node, flag: str) -> str:
    """What the guard's table holds: nothing while the family is open."""
    _, out = node.execute(f"ip {flag} route show table {TABLE} 2>/dev/null")
    return out


def blocked(node, flag: str) -> bool:
    return "unreachable default" in table(node, flag)


def wait_blocked(node, flag: str) -> None:
    node.wait_until_succeeds(f"ip {flag} route show table {TABLE} | grep 'unreachable default'", timeout=60)


def wait_open(node, flag: str) -> None:
    node.wait_until_fails(f"ip {flag} route show table {TABLE} | grep 'unreachable default'", timeout=60)


def status(node) -> list[str]:
    return node.succeed("sekka-vpn-guard status").splitlines()


def reverse_path_drops(node) -> str:
    return node.succeed(
        "nft list chain ip mangle nixos-fw-rpfilter | awk '/drop/ {for (i = 1; i < NF; i++) if ($i == \"packets\") print $(i + 1)}'"
    ).strip()


def keys(node) -> tuple[str, str]:
    node.succeed("umask 077 && wg genkey > /root/client.key && wg pubkey < /root/client.key > /root/client.pub")
    return node.succeed("cat /root/client.key").strip(), node.succeed("cat /root/client.pub").strip()


def connect(node, key: str, name: str, address: str, dns: str, allowed: str, port: int) -> None:
    """Imports a wg-quick file as Settings and Network do, and brings it up."""
    conf = (
        "[Interface]\n"
        f"PrivateKey = {key}\n"
        f"Address = {address}\n"
        f"DNS = {dns}\n"
        "\n"
        "[Peer]\n"
        f"PublicKey = {server_pub}\n"
        f"AllowedIPs = {allowed}\n"
        f"Endpoint = {VPN4}:{port}\n"
    )
    node.succeed(f"umask 077 && printf %s {shlex.quote(conf)} > /root/{name}.conf")
    node.succeed(f"nmcli connection import type wireguard file /root/{name}.conf")
    node.succeed(f"nmcli connection up {name}")


for node in (isp, internet, vpn, guarded, unguarded):
    start_vm(node)
isp.wait_for_unit("radvd.service")
isp.wait_for_unit("dnsmasq.service")
internet.wait_for_unit("nginx.service")
vpn.wait_for_unit("multi-user.target")
for node in (guarded, unguarded):
    # The home network: IPv4 by DHCP and a global IPv6 address by router
    # advertisement, both through eth1.
    node.wait_for_unit("NetworkManager.service")
    node.wait_until_succeeds("ip -4 route show default | grep 'dev eth1'", timeout=180)
    node.wait_until_succeeds("ip -6 route show default | grep 'dev eth1'", timeout=180)
    node.wait_until_succeeds(f"curl -sS --max-time 5 {ONLY4}", timeout=120)
    node.wait_until_succeeds(f"curl -sS --max-time 5 {ONLY6}", timeout=120)

# The server: an IPv4-only tunnel, an IPv6-only one and one for both, each
# with both clients as peers.
vpn.succeed("umask 077 && wg genkey > /root/server.key && wg pubkey < /root/server.key > /root/server.pub")
server_pub = vpn.succeed("cat /root/server.pub").strip()
guarded_key, guarded_pub = keys(guarded)
unguarded_key, unguarded_pub = keys(unguarded)
vpn.succeed(
    "ip link add wg4 type wireguard",
    "wg set wg4 listen-port 51820 private-key /root/server.key"
    f" peer {guarded_pub} allowed-ips 192.168.200.2/32 peer {unguarded_pub} allowed-ips 192.168.200.3/32",
    "ip addr add 192.168.200.1/24 dev wg4",
    "ip link set wg4 up",
    "ip link add wg6 type wireguard",
    f"wg set wg6 listen-port 51821 private-key /root/server.key peer {guarded_pub} allowed-ips fd00:200::2/128",
    "ip -6 addr add fd00:200::1/64 dev wg6",
    "ip link set wg6 up",
    "ip link add wg46 type wireguard",
    "wg set wg46 listen-port 51822 private-key /root/server.key"
    f" peer {guarded_pub} allowed-ips 192.168.201.2/32,fd00:201::2/128",
    "ip addr add 192.168.201.1/24 dev wg46",
    "ip -6 addr add fd00:201::1/64 dev wg46",
    "ip link set wg46 up",
)

guarded_own = own_addresses(guarded)
unguarded_own = own_addresses(unguarded)

with subtest("Without a tunnel the guard's rules are in place and both families leave directly"):
    guarded.wait_for_unit("sekka-vpn-guard.service")
    for flag in ("-4", "-6"):
        rules = " ".join(guarded.succeed(f"ip {flag} rule show").split())
        assert "32764: from all lookup main suppress_prefixlength 0 proto 83" in rules, rules
        assert f"32765: from all lookup {TABLE} proto 83" in rules, rules
        assert not blocked(guarded, flag)
    assert status(guarded) == ["IPv4 leaves by eth1.", "IPv6 leaves by eth1."], status(guarded)
    arrives_from(guarded, ONLY4, [ISP4])
    arrives_from(guarded, ONLY6, guarded_own)
    arrives_from(guarded, BOTH, guarded_own)

with subtest("Without the guard, an IPv4-only tunnel leaves IPv6 on the home network"):
    unguarded.fail("command -v sekka-vpn-guard")
    connect(unguarded, unguarded_key, "GLADOS", "192.168.200.3/32", "192.168.200.1", "0.0.0.0/0", 51820)
    unguarded.wait_until_succeeds(f"ip route get {WEB4} | grep 'dev GLADOS'", timeout=60)
    arrives_from(unguarded, ONLY4, [VPN4])
    # The leak: the web host sees the home address, and a site with both
    # kinds of address is reached that way without anyone asking for IPv6.
    arrives_from(unguarded, ONLY6, unguarded_own)
    arrives_from(unguarded, BOTH, unguarded_own)
    assert TABLE not in unguarded.succeed("ip -6 rule show")

with subtest("An IPv4-only tunnel: IPv6 is refused at once and the home network stays"):
    connect(guarded, guarded_key, "GLADOS", "192.168.200.2/32", "192.168.200.1", "0.0.0.0/0", 51820)
    wait_blocked(guarded, "-6")
    arrives_from(guarded, ONLY4, [VPN4])
    refused_at_once(guarded, ONLY6)
    arrives_from(guarded, BOTH, [VPN4])
    home_network_answers(guarded)
    assert status(guarded) == [
        "IPv4 goes through GLADOS.",
        "IPv6 is blocked: GLADOS carries only IPv4, so IPv6 would leave outside it.",
    ], status(guarded)
    # The tunnel's far end is an IPv4 address: IPv6 needs no way round for it.
    assert "throw" not in table(guarded, "-6"), table(guarded, "-6")
    assert not blocked(guarded, "-4")
    guarded.succeed("journalctl -b --no-pager | grep -F 'IPv6 is blocked while GLADOS is connected'")
    # Another run finds the guard as wanted, changes nothing and says nothing.
    assert guarded.succeed("sekka-vpn-guard reconcile") == ""

with subtest("The block lifts when the tunnel goes down"):
    guarded.succeed("nmcli connection down GLADOS")
    wait_open(guarded, "-6")
    arrives_from(guarded, ONLY4, [ISP4])
    arrives_from(guarded, ONLY6, guarded_own)
    guarded.succeed("journalctl -b --no-pager | grep -F 'IPv6 is no longer blocked'")

with subtest("An IPv6-only tunnel with an IPv4 far end: IPv4 is refused and the tunnel keeps working"):
    connect(guarded, guarded_key, "WHEATLEY", "fd00:200::2/128", "fd00:200::1", "::/0", 51821)
    wait_blocked(guarded, "-4")
    routes = " ".join(table(guarded, "-4").split())
    assert f"throw {VPN4} proto 83" in routes and "unreachable default proto 83" in routes, routes
    assert not blocked(guarded, "-6")
    print("reverse-path filter drops before:", reverse_path_drops(guarded))
    arrives_from(guarded, ONLY6, [VPN6])
    refused_at_once(guarded, ONLY4)
    arrives_from(guarded, BOTH, [VPN6])
    home_network_answers(guarded)
    assert status(guarded) == [
        "IPv4 is blocked: WHEATLEY carries only IPv6, so IPv4 would leave outside it.",
        "IPv6 goes through WHEATLEY.",
    ], status(guarded)
    # The server forgets the session, so the tunnel has to shake hands again
    # with the block in force. The replies come from an address the block
    # covers, and NixOS's reverse-path filter drops them unless that address
    # is still reached the ordinary way.
    vpn.succeed(
        f"wg set wg6 peer {guarded_pub} remove",
        f"wg set wg6 peer {guarded_pub} allowed-ips fd00:200::2/128",
    )
    guarded.wait_until_succeeds(f"curl -sS --max-time 5 {ONLY6} | grep -F {VPN6}", timeout=90)
    print("reverse-path filter drops after:", reverse_path_drops(guarded))
    guarded.succeed("nmcli connection down WHEATLEY")
    wait_open(guarded, "-4")
    arrives_from(guarded, ONLY4, [ISP4])

with subtest("A tunnel that carries both families is not guarded"):
    connect(guarded, guarded_key, "CHELL", "192.168.201.2/32, fd00:201::2/128", "192.168.201.1", "0.0.0.0/0, ::/0", 51822)
    guarded.wait_until_succeeds(f"ip route get {WEB4} | grep 'dev CHELL'", timeout=60)
    guarded.wait_until_succeeds(f"ip -6 route get {WEB6} | grep 'dev CHELL'", timeout=60)
    # What the dispatcher runs, so that its turn is known to be over.
    guarded.succeed("sekka-vpn-guard reconcile")
    assert status(guarded) == ["IPv4 goes through CHELL.", "IPv6 goes through CHELL."], status(guarded)
    assert not blocked(guarded, "-4") and not blocked(guarded, "-6")
    arrives_from(guarded, ONLY4, [VPN4])
    arrives_from(guarded, ONLY6, [VPN6])
    guarded.succeed("nmcli connection down CHELL")
    guarded.wait_until_succeeds(f"ip route get {WEB4} | grep 'dev eth1'", timeout=60)

with subtest("The service takes the guard out when stopped and puts it back when started"):
    guarded.succeed("nmcli connection up GLADOS")
    wait_blocked(guarded, "-6")
    guarded.succeed("systemctl stop sekka-vpn-guard")
    # Rules and routes alike, in both families.
    left = guarded.succeed("ip -4 rule show; ip -6 rule show; ip -4 route show table all; ip -6 route show table all")
    assert TABLE not in left and "proto 83" not in left, left
    arrives_from(guarded, ONLY6, guarded_own)
    guarded.succeed("systemctl start sekka-vpn-guard")
    assert blocked(guarded, "-6") and not blocked(guarded, "-4")
    refused_at_once(guarded, ONLY6)
    guarded.succeed("nmcli connection down GLADOS")
    wait_open(guarded, "-6")
