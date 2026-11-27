import ipaddress
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
NAT64 = "-6 http://[64:ff9b::cb00:710a]/"
# A web address with both an AAAA and an A record, as most sites have.
BOTH = f"--resolve 'whoami.test:80:[{WEB6}],{WEB4}' http://whoami.test/"
ONLY4 = f"-4 http://{WEB4}/"
ONLY6 = f"-6 http://[{WEB6}]/"
# The priorities of the patch's rule pair.
PASS_RULE = 31800
BLOCK_RULE = 31801

# Behaviour this prototype has to establish rather than assume. Each is
# recorded and the run carries on, so that one run answers every question;
# the last line fails the test if any of them went wrong.
findings: list[str] = []


def expect(ok: bool, finding: str) -> bool:
    if not ok:
        findings.append(finding)
        print(f"FINDING: {finding}")
    return ok


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


def blocking(family: str) -> str:
    return f"ip {family} rule show priority {BLOCK_RULE} | grep unreachable >/dev/null"


def policy_is(ipv4: str, ipv6: str, timeout: int = 30) -> bool:
    for family, state in (("-4", ipv4), ("-6", ipv6)):
        command = blocking(family) if state == "blocked" else f"! ( {blocking(family)} )"
        try:
            guarded.wait_until_succeeds(command, timeout=timeout)
        except Exception:
            return False
    return True


def wait_policy(ipv4: str, ipv6: str) -> None:
    assert policy_is(ipv4, ipv6), guarded.succeed("ip -4 rule; ip -6 rule")


def clear_stale_block() -> None:
    """Removes the pair by hand after a finding, so later scenarios start clean."""
    guarded.succeed(
        f"for f in -4 -6; do for p in {PASS_RULE} {BLOCK_RULE}; do"
        ' while ip "$f" rule del priority "$p" 2>/dev/null; do :; done; done; done'
    )


def first_address(name: str) -> str:
    """The address getaddrinfo puts first, after RFC 6724 selection."""
    return guarded.succeed(f"getent ahosts {name} | awk 'NR==1 {{print $1}}'").strip()


def connect(name: str, address: str, dns: str, allowed: str, port: int, endpoint: str = VPN4, up: bool = True) -> None:
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
    if up:
        guarded.succeed(f"nmcli connection up {name}")


def switch(generation: str) -> None:
    """A live switch. Records it if Yukimi's pre-switch check refuses one, then skips the check."""
    status, out = guarded.execute(f"{generation}/bin/switch-to-configuration test 2>&1")
    if status != 0:
        expect(False, f"switch to {generation} refused without NIXOS_NO_CHECK: {out[-600:]}")
        guarded.succeed(f"NIXOS_NO_CHECK=1 {generation}/bin/switch-to-configuration test")


def networkmanager_settled(*active: str) -> bool:
    """NetworkManager is running again and has taken back the connected tunnels."""
    guarded.wait_for_unit("NetworkManager.service")
    for name in active:
        try:
            guarded.wait_until_succeeds(
                f"nmcli -g GENERAL.STATE connection show {name} | grep -Fx activated", timeout=60
            )
        except Exception:
            return expect(False, f"NetworkManager did not take {name} back after restarting")
    return True


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
base = guarded.succeed("readlink -f /run/current-system").strip()

with subtest("Direct internet, public LAN, private LAN and NAT64-prefix service before connecting"):
    wait_policy("open", "open")
    guarded.succeed(f"! ip -6 rule show priority {PASS_RULE} | grep . >/dev/null")
    # No trusted prefixes in this generation, so no unit for them.
    guarded.fail("systemctl cat sekka-vpn-guard-trusted.service")
    private_network_answers(guarded)
    guarded.succeed(f"ping -6 -c 1 -W 2 {ROUTER6}")
    arrives_from(guarded, ONLY4, [ISP4])
    arrives_from(guarded, ONLY6, guarded_own)
    arrives_from(guarded, BOTH, guarded_own)
    arrives_from(guarded, NAT64, guarded_own)
    assert first_address("whoami.test") == WEB6

with subtest("IPv4-only full tunnel permits IPv4, the LAN and routes more specific than a default"):
    connect("GLADOS", "192.168.200.2/32", "192.168.200.1", "0.0.0.0/0", 51820)
    # NetworkManager adds the pair before the tunnel's pre-up, and so before
    # it reports the tunnel up.
    guarded.succeed(blocking("-6"))
    print(guarded.succeed("ip -6 rule"))
    guarded.succeed(f"ip -6 rule show priority {PASS_RULE} | grep -F 'lookup main suppress_prefixlength 0'")
    wait_policy("open", "blocked")
    arrives_from(guarded, ONLY4, [VPN4])
    refused_at_once(guarded, ONLY6)
    refused_at_once(guarded, NAT64)
    arrives_from(guarded, BOTH, [VPN4])
    assert "Network is unreachable" in guarded.fail(f"ip -6 route get {WEB6} 2>&1")
    # Address selection sees the unreachable family and puts it last.
    assert first_address("whoami.test") == WEB4
    private_network_answers(guarded)
    # The LAN's own global prefix is a route more specific than the default.
    guarded.succeed(f"ping -6 -c 1 -W 2 {ROUTER6}")
    guarded.fail(f"ping -6 -c 1 -W 2 {VPN6}")
    # So is a route added by hand, or by the network: it is followed, as
    # NetworkManager follows one for the family the tunnel carries.
    guarded.succeed(f"ip -6 route add {WEB6}/128 via {ROUTER6} dev eth1")
    arrives_from(guarded, ONLY6, guarded_own)
    guarded.succeed(f"ip -6 route del {WEB6}/128")
    refused_at_once(guarded, ONLY6)

with subtest("NetworkManager pausing leaves the block in place"):
    guarded.succeed("systemctl kill --signal=SIGSTOP NetworkManager.service")
    refused_at_once(guarded, ONLY6)
    arrives_from(guarded, ONLY4, [VPN4])
    guarded.succeed("systemctl kill --signal=SIGCONT NetworkManager.service")

with subtest("An active tunnel keeps its policy during a switch, and reaches a trusted prefix"):
    guarded.succeed("touch /run/observe-vpn-switch")
    switch(f"{base}/specialisation/trusted")
    assert guarded.succeed("cat /run/vpn-during-switch").strip() == "blocked"
    guarded.succeed("rm /run/observe-vpn-switch")
    wait_policy("open", "blocked")
    guarded.succeed("systemctl is-active sekka-vpn-guard-trusted.service")
    guarded.succeed(f"ping -6 -c 1 -W 2 {VPN6}")
    refused_at_once(guarded, ONLY6)
    arrives_from(guarded, ONLY4, [VPN4])
    switch(base)
    guarded.fail(f"ping -6 -c 1 -W 2 {VPN6}")
    guarded.succeed("! ip -6 rule show priority 31799 | grep . >/dev/null")
    wait_policy("open", "blocked")

with subtest("A NetworkManager restart with a tunnel up keeps the block, which lifts with the tunnel"):
    guarded.succeed("systemctl restart NetworkManager.service")
    if networkmanager_settled("GLADOS"):
        if expect(policy_is("open", "blocked"), "after a NetworkManager restart, the tunnel has no block"):
            refused_at_once(guarded, ONLY6)
    guarded.execute("nmcli connection down GLADOS")
    if not expect(
        policy_is("open", "open"),
        "a tunnel connected across a NetworkManager restart leaves its block when it goes down",
    ):
        clear_stale_block()
    arrives_from(guarded, ONLY6, guarded_own)
    arrives_from(guarded, ONLY4, [ISP4])

with subtest("Disabling the module clears policy and rolling back restores it"):
    guarded.succeed("nmcli connection up GLADOS")
    wait_policy("open", "blocked")
    switch(f"{base}/specialisation/disabled")
    up = networkmanager_settled("GLADOS")
    print(guarded.succeed("systemctl show -p ActiveEnterTimestamp NetworkManager.service; ip -6 rule"))
    if not expect(policy_is("open", "open"), "a switch to the disabled generation leaves the block"):
        clear_stale_block()
    if up:
        arrives_from(guarded, ONLY4, [VPN4])
    arrives_from(guarded, ONLY6, guarded_own)
    switch(base)
    if networkmanager_settled("GLADOS"):
        if expect(policy_is("open", "blocked"), "rolling back to the enabled generation does not restore the block"):
            refused_at_once(guarded, ONLY6)
    guarded.execute("nmcli connection down GLADOS")
    if not expect(policy_is("open", "open"), "after the rollback, the block outlives its tunnel"):
        clear_stale_block()
    arrives_from(guarded, ONLY4, [ISP4])
    arrives_from(guarded, ONLY6, guarded_own)

with subtest("A tunnel stopped while it waits after its pre-up, and one that fails, leave no block"):
    # NetworkManager sends no down event for either: the dispatcher's down
    # comes only after a device was ACTIVATED.
    #
    # A secondary whose VPN service is missing holds the tunnel after its
    # pre-up, routed and so blocking, until it is stopped.
    secondary = "6b1ec3f2-4f0a-4a3e-9a37-9d2f8f0c1e55"
    guarded.succeed(
        "umask 077 && cat > /etc/NetworkManager/system-connections/MISSING.nmconnection <<'EOF'\n"
        f"[connection]\nid=MISSING\nuuid={secondary}\ntype=vpn\nautoconnect=false\n\n"
        "[vpn]\nservice-type=org.freedesktop.NetworkManager.sekka-test-missing\n\n"
        "[ipv4]\nmethod=auto\n\n[ipv6]\nmethod=auto\n"
        "EOF",
        "nmcli connection reload",
    )
    connect("STUCK", "192.168.200.2/32", "192.168.200.1", "0.0.0.0/0", 51820, up=False)
    guarded.succeed(f"nmcli connection modify STUCK connection.autoconnect no connection.secondaries {secondary}")
    status, out = guarded.execute("nmcli --wait 10 connection up STUCK 2>&1")
    state = guarded.succeed("nmcli -g GENERAL.STATE device show STUCK").strip()
    print(status, out, state)
    if expect(status != 0 and "secondar" in state, f"the tunnel did not wait for its secondary: {state}"):
        expect(policy_is("open", "blocked"), "a tunnel routed while it waits for its secondary has no block")
    guarded.succeed("nmcli connection down STUCK")
    if not expect(policy_is("open", "open"), "a tunnel stopped while it waited after its pre-up left its block"):
        clear_stale_block()
    arrives_from(guarded, ONLY6, guarded_own)
    arrives_from(guarded, ONLY4, [ISP4])
    # An OpenVPN secondary with no server fails as its service starts, and
    # fails the tunnel with it, after its pre-up.
    broken = "0d6f52a1-93c4-4e59-8f3b-5a8a3c1d2e77"
    guarded.succeed(
        "umask 077 && cat > /etc/NetworkManager/system-connections/BROKEN.nmconnection <<'EOF'\n"
        f"[connection]\nid=BROKEN\nuuid={broken}\ntype=vpn\nautoconnect=false\n\n"
        "[vpn]\nservice-type=org.freedesktop.NetworkManager.openvpn\nconnection-type=static-key\n\n"
        "[ipv4]\nmethod=auto\n\n[ipv6]\nmethod=auto\n"
        "EOF",
        "nmcli connection reload",
    )
    connect("FAILS", "192.168.200.2/32", "192.168.200.1", "0.0.0.0/0", 51820, up=False)
    guarded.succeed(f"nmcli connection modify FAILS connection.autoconnect no connection.secondaries {broken}")
    status, out = guarded.execute("nmcli --wait 30 connection up FAILS 2>&1")
    print(status, out)
    print(guarded.succeed("journalctl -u NetworkManager -o cat | grep -E 'FAILS|BROKEN' | tail -20 || true"))
    expect(status != 0 and "Timeout" not in out, f"the tunnel with a broken secondary did not fail: {out}")
    if not expect(policy_is("open", "open"), "a tunnel that failed after its pre-up left its block"):
        clear_stale_block()
    arrives_from(guarded, ONLY6, guarded_own)
    arrives_from(guarded, ONLY4, [ISP4])
    guarded.execute(
        "nmcli connection delete STUCK FAILS; nmcli connection delete MISSING BROKEN;"
        " rm -f /etc/NetworkManager/system-connections/MISSING.nmconnection"
        " /etc/NetworkManager/system-connections/BROKEN.nmconnection"
    )

with subtest("A split tunnel reaches its peer and keeps ordinary internet access"):
    connect("SPLIT", "192.168.200.2/32", "192.168.200.1", "192.168.200.0/24, 192.0.2.0/24", 51820)
    guarded.succeed("ping -4 -c 1 -W 2 192.168.200.1")
    wait_policy("open", "open")
    arrives_from(guarded, ONLY4, [ISP4])
    arrives_from(guarded, ONLY6, guarded_own)
    guarded.succeed("nmcli connection down SPLIT")

with subtest("A split profile listing an address alone beside a full tunnel leaves the block to the full one"):
    connect("SINGLE", "192.168.201.2/32", "192.168.201.1", "192.168.201.1", 51822)
    wait_policy("open", "open")
    guarded.succeed("nmcli connection up GLADOS")
    wait_policy("open", "blocked")
    refused_at_once(guarded, ONLY6)
    guarded.succeed("nmcli connection down GLADOS")
    wait_policy("open", "open")
    arrives_from(guarded, ONLY6, guarded_own)
    guarded.succeed("nmcli connection down SINGLE")

with subtest("nftables flushing the whole ruleset leaves the block alone"):
    guarded.succeed("nmcli connection up GLADOS")
    wait_policy("open", "blocked")
    # The switch starts nftables, which flushes the ruleset to load its own.
    # Enabling nftables changes the initrd (it blacklists ip_tables), so
    # Yukimi's pre-switch check would have the change wait for a boot.
    guarded.succeed(f"NIXOS_NO_CHECK=1 {base}/specialisation/nftables/bin/switch-to-configuration test")
    guarded.succeed("nft list table inet admin")
    for step in ("switch", "systemctl reload nftables.service", "systemctl restart nftables.service", "nft flush ruleset"):
        if step != "switch":
            guarded.succeed(step)
        wait_policy("open", "blocked")
        refused_at_once(guarded, ONLY6)
        arrives_from(guarded, ONLY4, [VPN4])
    guarded.succeed(f"NIXOS_NO_CHECK=1 {base}/bin/switch-to-configuration test")
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
    guarded.succeed(blocking("-4"))
    # Replies to the tunnel's own IPv4 packets get their mark back, as the
    # carried family's do, so NixOS's reverse-path filter lets them in.
    print(guarded.succeed("nft list table ip nm-wg-WHEATLEY"))
    # What the reverse-path checks see, for a failure below.
    guarded.execute(f"curl -sS --max-time 3 {ONLY6} >/dev/null")
    print(
        guarded.succeed(
            "sysctl net.ipv4.conf.all.rp_filter net.ipv4.conf.eth1.rp_filter net.ipv4.conf.all.src_valid_mark;"
            " wg show WHEATLEY; ip -4 route get 203.0.113.20 mark 0 2>&1 || true;"
            " journalctl -k -o cat | grep -F 'rpfilter drop' | tail -5 || true"
        )
    )
    print(vpn.succeed("wg show wg6"))
    wait_policy("blocked", "open")
    arrives_from(guarded, ONLY6, [VPN6])
    refused_at_once(guarded, ONLY4)
    arrives_from(guarded, BOTH, [VPN6])
    assert first_address("whoami.test") == WEB6
    private_network_answers(guarded)
    fresh_handshake(VPN4)
    fresh_handshake(ALTERNATE_VPN4)
    wait_policy("blocked", "open")
    refused_at_once(guarded, ONLY4)

with subtest("Two tunnels that carry one family each are both usable at once"):
    # Each blocks the family the other carries. Every tunnel's own packets
    # are marked, and the block refuses only unmarked ones.
    guarded.succeed("nmcli connection up GLADOS")
    wait_policy("blocked", "blocked")
    arrives_from(guarded, ONLY4, [VPN4])
    arrives_from(guarded, ONLY6, [VPN6])
    for dev in ("GLADOS", "WHEATLEY"):
        fields = guarded.succeed(f"wg show {dev} latest-handshakes").split()
        assert len(fields) == 2 and int(fields[1]) > 0, (dev, fields)
    guarded.succeed("nmcli connection down GLADOS")
    wait_policy("blocked", "open")
    arrives_from(guarded, ONLY6, [VPN6])
    guarded.succeed("nmcli connection down WHEATLEY")
    wait_policy("open", "open")
    arrives_from(guarded, ONLY4, [ISP4])

with subtest("A profile edit counts once NetworkManager applies it"):
    connect("CAROLINE", "192.168.201.2/32, fd00:201::2/128", "192.168.201.1", "0.0.0.0/0", 51822)
    wait_policy("open", "blocked")
    refused_at_once(guarded, ONLY6)
    # Saved but not applied: the connected tunnel still takes all of IPv4.
    guarded.succeed("nmcli connection modify CAROLINE ipv4.never-default yes")
    wait_policy("open", "blocked")
    arrives_from(guarded, ONLY4, [VPN4])
    refused_at_once(guarded, ONLY6)
    # Applied: the profile no longer asks for all of IPv4, so it is left alone.
    guarded.succeed("nmcli connection up CAROLINE")
    wait_policy("open", "open")
    arrives_from(guarded, ONLY4, [ISP4])
    arrives_from(guarded, ONLY6, guarded_own)
    # Reapplied in place, without reconnecting.
    guarded.succeed("nmcli connection modify CAROLINE ipv4.never-default no")
    status, out = guarded.execute("nmcli device reapply CAROLINE 2>&1")
    if expect(status == 0, f"NetworkManager refused to reapply never-default: {out}"):
        expect(policy_is("open", "blocked"), "a reapplied profile edit does not add the block")
    guarded.succeed("nmcli connection down CAROLINE")
    wait_policy("open", "open")


def everything_but(*left_out: str) -> str:
    """AllowedIPs for all of IPv4 except the given networks, as a calculator writes them."""
    nets = [ipaddress.IPv4Network("0.0.0.0/0")]
    for hole in map(ipaddress.IPv4Network, left_out):
        nets = [part for net in nets for part in (net.address_exclude(hole) if hole.subnet_of(net) else [net])]
    return ", ".join(str(net) for net in sorted(nets))


with subtest("A list that leaves out private ranges is a split tunnel, as NetworkManager routes it"):
    public = everything_but("10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", f"{VPN4}/32")
    connect("ATLAS", "192.168.200.2/32", "192.168.200.1", public, 51820)
    # Without a /0 NetworkManager sets up no default route and no mark.
    assert guarded.succeed("wg show ATLAS fwmark").strip() == "off"
    wait_policy("open", "open")
    arrives_from(guarded, ONLY4, [VPN4])
    arrives_from(guarded, ONLY6, guarded_own)
    guarded.succeed("nmcli connection down ATLAS")

with subtest("Halves of IPv4 are a split tunnel too, and reach their server over IPv6"):
    connect("PBODY", "192.168.200.2/32", "192.168.200.1", "0.0.0.0/1, 128.0.0.0/1", 51820, endpoint=f"[{VPN6}]")
    assert guarded.succeed("wg show PBODY fwmark").strip() == "off"
    wait_policy("open", "open")
    arrives_from(guarded, ONLY4, [VPN4])
    arrives_from(guarded, ONLY6, guarded_own)
    fields = guarded.succeed("wg show PBODY latest-handshakes").split()
    assert len(fields) == 2 and fields[0] == server_pub and int(fields[1]) > 0, fields
    guarded.succeed("nmcli connection down PBODY")
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

with subtest("Nothing this prototype had to establish went wrong"):
    assert not findings, "\n".join(findings)
