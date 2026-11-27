import ipaddress
import json
import re
import shlex

WEB4 = "203.0.113.10"
WEB6 = "2001:db8:ffff::10"
ISP4 = "203.0.113.1"
VPN4 = "203.0.113.20"
ALTERNATE_VPN4 = "203.0.113.21"
VPN6 = "2001:db8:ffff::20"
ROUTER4 = "192.168.1.1"
ROUTER6 = "2001:db8:1::1"
NFT_TABLE = "nft list table inet sekka_vpn_guard"
NAT64 = "-6 http://[64:ff9b::cb00:710a]/"
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


def private_network_answers(node) -> None:
    link_local = node.succeed("ip -6 route show default dev eth1 | awk 'NR==1 {print $3}'").strip()
    node.succeed(
        f"ping -4 -c 1 -W 2 {ROUTER4}",
        "ping -6 -c 1 -W 2 fd42::1",
        f"ping -6 -c 1 -W 2 {link_local}%eth1",
    )


def wait_policy(ipv4: str, ipv6: str) -> None:
    checks = [
        f'printf "%s\\n" "$policy" | grep -Fxq {shlex.quote(line)}'
        for line in (f"IPv4: {ipv4}", f"IPv6: {ipv6}")
    ]
    guarded.wait_until_succeeds(
        "policy=$(su -s /bin/sh demo -c 'sekka-vpn-guard status') && " + " && ".join(checks),
        timeout=60,
    )
    if "blocked" in (ipv4, ipv6):
        guarded.succeed(NFT_TABLE)
    else:
        guarded.fail(NFT_TABLE)


def connect(name: str, address: str, dns: str, allowed: str, port: int, endpoint: str = VPN4) -> None:
    """Imports a wg-quick file as Settings and Network do, and brings it up."""
    conf = (
        f"Address = {address}\n"
        f"DNS = {dns}\n"
        "\n"
        "[Peer]\n"
        f"PublicKey = {server_pub}\n"
        f"AllowedIPs = {allowed}\n"
        f"Endpoint = {endpoint}:{port}\n"
    )
    guarded.succeed(
        "umask 077 && { printf '[Interface]\\nPrivateKey = '; cat /root/client.key; "
        f"printf %s {shlex.quote(conf)}; }} > /root/{name}.conf"
    )
    guarded.succeed(f"nmcli connection import type wireguard file /root/{name}.conf")
    guarded.succeed(f"nmcli connection up {name}")


def guard_table_handle() -> int | None:
    """The kernel's handle for the guard's table: a new one after it was deleted and made again."""
    listing = json.loads(guarded.succeed("nft -j list tables inet"))["nftables"]
    handles = [o["table"]["handle"] for o in listing if o.get("table", {}).get("name") == "sekka_vpn_guard"]
    return handles[0] if handles else None


for node in (isp, internet, vpn, guarded):
    start_vm(node)
isp.wait_for_unit("radvd.service")
isp.wait_for_unit("dnsmasq.service")
internet.wait_for_unit("nginx.service")
vpn.wait_for_unit("multi-user.target")
guarded.wait_for_unit("NetworkManager.service")
guarded.wait_until_succeeds("ip -4 route show default | grep 'dev eth1'", timeout=180)
guarded.wait_until_succeeds("ip -6 route show default | grep 'dev eth1'", timeout=180)
guarded.wait_until_succeeds(f"curl -sS --max-time 5 {ONLY4}", timeout=120)
guarded.wait_until_succeeds(f"curl -sS --max-time 5 {ONLY6}", timeout=120)

# The server supports three distinct tunnel configurations.
vpn.succeed("umask 077 && wg genkey > /root/server.key && wg pubkey < /root/server.key > /root/server.pub")
server_pub = vpn.succeed("cat /root/server.pub").strip()
guarded.succeed("umask 077 && wg genkey > /root/client.key")
guarded_pub = guarded.succeed("wg pubkey < /root/client.key").strip()
vpn.succeed(
    "ip link add wg4 type wireguard",
    "wg set wg4 listen-port 51820 private-key /root/server.key"
    f" peer {guarded_pub} allowed-ips 192.168.200.2/32",
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

with subtest("Direct internet, public LAN, private LAN and NAT64-prefix service before connecting"):
    guarded.wait_for_unit("sekka-vpn-guard.service")
    wait_policy("open", "open")
    assert guarded.succeed("stat -c '%a %u' /run/sekka-vpn-guard/lock").strip() == "600 0"
    # Without nftables, the guard's additions to its service add no unit.
    guarded.fail("systemctl cat nftables.service")
    private_network_answers(guarded)
    guarded.succeed(f"ping -6 -c 1 -W 2 {ROUTER6}")
    arrives_from(guarded, ONLY4, [ISP4])
    arrives_from(guarded, ONLY6, guarded_own)
    arrives_from(guarded, BOTH, guarded_own)
    arrives_from(guarded, NAT64, guarded_own)

with subtest("IPv4-only full tunnel permits IPv4, private LAN and scoped IPv6"):
    connect("GLADOS", "192.168.200.2/32", "192.168.200.1", "0.0.0.0/0", 51820)
    # Set at the tunnel's pre-up, which NetworkManager waits for before it
    # reports the tunnel up, and not at the up event that follows.
    guarded.succeed(NFT_TABLE)
    wait_policy("open", "blocked")
    arrives_from(guarded, ONLY4, [VPN4])
    refused_at_once(guarded, ONLY6)
    refused_at_once(guarded, NAT64)
    arrives_from(guarded, BOTH, [VPN4])
    private_network_answers(guarded)
    guarded.fail(f"ping -6 -c 1 -W 2 {ROUTER6}")
    # The public destination has an ordinary route that is more specific
    # than the default. Host egress must still apply the same policy.
    guarded.succeed(f"ip -6 route add {WEB6}/128 via {ROUTER6} dev eth1")
    guarded.succeed(f"ip -6 route get {WEB6} | grep -F 'via {ROUTER6}'")
    refused_at_once(guarded, ONLY6)
    guarded.succeed(f"ip -6 route del {WEB6}/128")

with subtest("A read NetworkManager does not answer keeps the installed block"):
    guarded.succeed("systemctl kill --signal=SIGSTOP NetworkManager.service")
    guarded.fail("sekka-vpn-guard reconcile")
    code, observed = guarded.execute("su -s /bin/sh demo -c 'sekka-vpn-guard status'")
    guarded.succeed("systemctl kill --signal=SIGCONT NetworkManager.service")
    assert code != 0 and "Degraded: IPv6 egress block installed" in observed, observed
    guarded.succeed(NFT_TABLE)
    refused_at_once(guarded, ONLY6)
    guarded.succeed("sekka-vpn-guard reconcile")
    wait_policy("open", "blocked")
    arrives_from(guarded, ONLY4, [VPN4])

with subtest("An active tunnel keeps its policy during a generation switch"):
    base = guarded.succeed("readlink -f /run/current-system").strip()
    old_guard = guarded.succeed("readlink -f \"$(command -v sekka-vpn-guard)\"").strip()
    guarded.succeed("touch /run/observe-vpn-switch")
    guarded.succeed(f"{base}/specialisation/trusted/bin/switch-to-configuration test")
    assert guarded.succeed("cat /run/vpn-during-switch").strip() == "blocked"
    wait_policy("open", "blocked")
    guarded.succeed(f"ping -6 -c 1 -W 2 {ROUTER6}")
    refused_at_once(guarded, ONLY6)
    arrives_from(guarded, ONLY4, [VPN4])

with subtest("Disabling the module clears policy and rolling back restores it"):
    guarded.succeed(f"{base}/specialisation/disabled/bin/switch-to-configuration test")
    # An event queued before the switch can still have the old executable.
    guarded.succeed(f"{old_guard} eth1 up")
    guarded.fail(NFT_TABLE)
    arrives_from(guarded, ONLY4, [VPN4])
    arrives_from(guarded, ONLY6, guarded_own)
    guarded.succeed(f"{base}/bin/switch-to-configuration test")
    wait_policy("open", "blocked")
    guarded.fail(f"ping -6 -c 1 -W 2 {ROUTER6}")
    refused_at_once(guarded, ONLY6)
    guarded.succeed("nmcli connection down GLADOS")
    wait_policy("open", "open")
    arrives_from(guarded, ONLY4, [ISP4])
    arrives_from(guarded, ONLY6, guarded_own)

with subtest("A split tunnel reaches its peer and keeps ordinary internet access"):
    connect("SPLIT", "192.168.200.2/32", "192.168.200.1", "192.168.200.0/24, 192.0.2.0/24", 51820)
    guarded.succeed("ping -4 -c 1 -W 2 192.168.200.1")
    wait_policy("open", "open")
    arrives_from(guarded, ONLY4, [ISP4])
    arrives_from(guarded, ONLY6, guarded_own)
    guarded.succeed("nmcli connection down SPLIT")

with subtest("A split profile listing an address without a prefix length leaves the guard working"):
    # wg-quick reads an address alone as one host, and NetworkManager keeps
    # the entry as it was written.
    connect("SINGLE", "192.168.201.2/32", "192.168.201.1", "192.168.201.1", 51822)
    device = json.loads(
        guarded.succeed(
            "busctl --json=short call org.freedesktop.NetworkManager /org/freedesktop/NetworkManager"
            " org.freedesktop.NetworkManager GetDeviceByIpIface s SINGLE"
        )
    )["data"][0]
    guarded.succeed(
        f"busctl --json=short call org.freedesktop.NetworkManager {device}"
        " org.freedesktop.NetworkManager.Device GetAppliedConnection u 0 | grep -F '\"192.168.201.1\"'"
    )
    wait_policy("open", "open")
    # A full tunnel beside it is still counted, and its block lifts with it.
    guarded.succeed("nmcli connection up GLADOS")
    guarded.succeed(NFT_TABLE)
    wait_policy("open", "blocked")
    refused_at_once(guarded, ONLY6)
    guarded.succeed("nmcli connection down GLADOS")
    wait_policy("open", "open")
    arrives_from(guarded, ONLY6, guarded_own)
    guarded.succeed("nmcli connection down SINGLE")

with subtest("A block flushed with an administrator's nftables ruleset is put back"):
    guarded.succeed("nmcli connection up GLADOS")
    wait_policy("open", "blocked")
    before = guard_table_handle()
    # The switch starts nftables, which flushes the ruleset to load its own.
    # Enabling nftables changes the initrd (it blacklists ip_tables), so
    # Yukimi's pre-switch check would have the change wait for a boot. A
    # later edit of the ruleset switches live; the reload below is that.
    no_check = "NIXOS_NO_CHECK=1"
    guarded.succeed(f"{no_check} {base}/specialisation/nftables/bin/switch-to-configuration test")
    guarded.succeed("nft list table inet admin")
    for step in ("switch", "reload", "restart"):
        if step != "switch":
            guarded.succeed(f"systemctl {step} nftables.service")
        after = guard_table_handle()
        assert after is not None and after != before, (step, before, after)
        before = after
        wait_policy("open", "blocked")
        refused_at_once(guarded, ONLY6)
        arrives_from(guarded, ONLY4, [VPN4])
    # Back to the generation without nftables: it flushes the ruleset as it stops.
    guarded.succeed(f"{no_check} {base}/bin/switch-to-configuration test")
    guarded.fail("nft list table inet admin")
    assert guard_table_handle() not in (None, before)
    wait_policy("open", "blocked")
    refused_at_once(guarded, ONLY6)
    guarded.succeed("nmcli connection down GLADOS")
    wait_policy("open", "open")
    arrives_from(guarded, ONLY6, guarded_own)


def fresh_handshake(endpoint: str) -> None:
    # Recreate both peers to discard the negotiated session keys on each end.
    # The NetworkManager profile and the connected interface stay active.
    vpn.succeed(
        f"wg set wg6 peer {guarded_pub} remove",
        f"wg set wg6 peer {guarded_pub} allowed-ips fd00:200::2/128",
    )
    guarded.succeed(
        f"wg set WHEATLEY peer {server_pub} remove",
        f"wg set WHEATLEY peer {server_pub} allowed-ips ::/0 endpoint {endpoint}:51821",
    )
    guarded.wait_until_succeeds(f"curl -fsS --max-time 3 {ONLY6} | grep -Fx {VPN6}", timeout=30)
    for node, dev, peer in ((vpn, "wg6", guarded_pub), (guarded, "WHEATLEY", server_pub)):
        fields = node.succeed(f"wg show {dev} latest-handshakes").split()
        assert len(fields) == 2 and fields[0] == peer and int(fields[1]) > 0, fields
    assert guarded.succeed("wg show WHEATLEY endpoints").split() == [server_pub, f"{endpoint}:51821"]


with subtest("IPv6-only full tunnel negotiates fresh sessions at two IPv4 endpoints"):
    connect("WHEATLEY", "fd00:200::2/128", "fd00:200::1", "::/0", 51821)
    guarded.succeed(NFT_TABLE)
    wait_policy("blocked", "open")
    arrives_from(guarded, ONLY6, [VPN6])
    refused_at_once(guarded, ONLY4)
    arrives_from(guarded, BOTH, [VPN6])
    private_network_answers(guarded)
    fresh_handshake(VPN4)
    fresh_handshake(ALTERNATE_VPN4)
    wait_policy("blocked", "open")
    refused_at_once(guarded, ONLY4)
    guarded.succeed("nmcli connection down WHEATLEY")
    wait_policy("open", "open")
    arrives_from(guarded, ONLY4, [ISP4])

with subtest("A profile edit counts once NetworkManager applies it"):
    connect("CAROLINE", "192.168.201.2/32, fd00:201::2/128", "192.168.201.1", "0.0.0.0/0", 51822)
    wait_policy("open", "blocked")
    refused_at_once(guarded, ONLY6)
    # Saved but not applied: the connected tunnel still takes all of IPv4.
    guarded.succeed("nmcli connection modify CAROLINE ipv4.never-default yes", "sekka-vpn-guard reconcile")
    wait_policy("open", "blocked")
    arrives_from(guarded, ONLY4, [VPN4])
    refused_at_once(guarded, ONLY6)
    # Applied: the profile no longer asks for all of IPv4, so it is left alone.
    guarded.succeed("nmcli connection up CAROLINE")
    wait_policy("open", "open")
    arrives_from(guarded, ONLY4, [ISP4])
    arrives_from(guarded, ONLY6, guarded_own)
    guarded.succeed("nmcli connection down CAROLINE")


def everything_but(*left_out: str) -> str:
    """AllowedIPs for all of IPv4 except the given networks, as a calculator writes them."""
    nets = [ipaddress.IPv4Network("0.0.0.0/0")]
    for hole in map(ipaddress.IPv4Network, left_out):
        nets = [part for net in nets for part in (net.address_exclude(hole) if hole.subnet_of(net) else [net])]
    return ", ".join(str(net) for net in sorted(nets))


with subtest("A list that leaves out private ranges and its own server is still a full tunnel"):
    public = everything_but("10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", f"{VPN4}/32")
    connect("ATLAS", "192.168.200.2/32", "192.168.200.1", public, 51820)
    # Without a default route NetworkManager puts no mark on the tunnel's own packets.
    assert guarded.succeed("wg show ATLAS fwmark").strip() == "off"
    wait_policy("open", "blocked")
    arrives_from(guarded, ONLY4, [VPN4])
    refused_at_once(guarded, ONLY6)
    arrives_from(guarded, BOTH, [VPN4])
    private_network_answers(guarded)
    guarded.succeed("nmcli connection down ATLAS")
    wait_policy("open", "open")
    arrives_from(guarded, ONLY6, guarded_own)

with subtest("An unmarked full tunnel keeps reaching its server over the blocked family"):
    connect("PBODY", "192.168.200.2/32", "192.168.200.1", "0.0.0.0/1, 128.0.0.0/1", 51820, endpoint=f"[{VPN6}]")
    assert guarded.succeed("wg show PBODY fwmark").strip() == "off"
    wait_policy("open", "blocked")
    guarded.succeed(f"{NFT_TABLE} | grep -F 'ip6 daddr {VPN6} udp dport 51820 accept'")
    arrives_from(guarded, ONLY4, [VPN4])
    refused_at_once(guarded, ONLY6)
    arrives_from(guarded, BOTH, [VPN4])
    fields = guarded.succeed("wg show PBODY latest-handshakes").split()
    assert len(fields) == 2 and fields[0] == server_pub and int(fields[1]) > 0, fields
    assert guarded.succeed("wg show PBODY endpoints").split() == [server_pub, f"[{VPN6}]:51820"]
    guarded.succeed("nmcli connection down PBODY")
    wait_policy("open", "open")
    arrives_from(guarded, ONLY4, [ISP4])

with subtest("A full tunnel carrying both families remains usable in both"):
    connect("CHELL", "192.168.201.2/32, fd00:201::2/128", "192.168.201.1", "0.0.0.0/0, ::/0", 51822)
    guarded.wait_until_succeeds(f"ip route get {WEB4} | grep 'dev CHELL'", timeout=60)
    guarded.wait_until_succeeds(f"ip -6 route get {WEB6} | grep 'dev CHELL'", timeout=60)
    wait_policy("open", "open")
    arrives_from(guarded, ONLY4, [VPN4])
    arrives_from(guarded, ONLY6, [VPN6])
    guarded.succeed("nmcli connection down CHELL")
    guarded.wait_until_succeeds(f"ip route get {WEB4} | grep 'dev eth1'", timeout=60)

with subtest("Every run NetworkManager's events started had an answer"):
    failed = guarded.succeed("journalctl -b -o cat -t nm-dispatcher | grep -F 'sekka-vpn-guard:' || true").strip()
    assert not failed, failed
