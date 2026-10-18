# Simulated Wi-Fi for test VMs: two radios (mac80211_hwsim). The first is
# moved into a network namespace of its own, where hostapd serves `networks`
# and dnsmasq hands out addresses, as a router does; the computer's Wi-Fi is
# the second, wlan1, managed by NetworkManager, which reaches them over the
# simulated air. Sharing a namespace, the two would never exchange
# addresses.
#
# `module` configures the VM; `script` defines `start_access_points(machine)`
# for the test script, which the test calls once the VM has started.
{
  lib,
  pkgs,
  # [{ ssid; password ? none (open); hidden ? false; }]
  networks,
}:
let
  device = i: if i == 0 then "wlan0" else "wlan0-${toString i}";
  bss =
    i: network:
    lib.concatLines (
      (
        if i == 0 then
          [
            "interface=wlan0"
            "driver=nl80211"
            "hw_mode=g"
            "channel=6"
          ]
        else
          [
            "bss=${device i}"
            "bssid=02:00:00:00:00:0${toString i}"
          ]
      )
      ++ [ "ssid=${network.ssid}" ]
      ++ lib.optional (network.hidden or false) "ignore_broadcast_ssid=1"
      ++ lib.optionals (network ? password) [
        "wpa=2"
        "wpa_key_mgmt=WPA-PSK"
        "rsn_pairwise=CCMP"
        "wpa_passphrase=${network.password}"
      ]
    );
  devices = lib.genList device (lib.length networks);
in
assert lib.length networks <= 9;
{
  module =
    { lib, ... }:
    {
      boot.kernelModules = [ "mac80211_hwsim" ];
      # Under the world regulatory domain wlan1 offers 89 channels, 78 of
      # them passive, and takes 9.5 s for a scan; joining a network can take
      # two. regtest=6 gives the first radio, the access points', a domain of
      # hwsim's own, and the second, wlan1, one with only 32 channels (2.4 GHz
      # 1-11 and 5.7-5.9 GHz), six of them passive: 2.3 s. Both include
      # hostapd's channel 6.
      boot.extraModprobeConfig = "options mac80211_hwsim regtest=6";
      # NetworkManager's Wi-Fi backend, which test VMs otherwise turn off
      # (mkVMOverride).
      networking.wireless.enable = lib.mkOverride 9 true;
      networking.networkmanager = {
        enable = true;
        unmanaged = [ "interface-name:wlan0*" ];
        # Scan with the radio's own address. To change it, NetworkManager
        # takes wlan1 down as it leaves a network; a scan it has just asked
        # wpa_supplicant for can be dropped then, leaving wpa_supplicant to
        # treat its next scan's results as scan-only and join nothing until
        # NetworkManager gives up (DEVELOPERS.md, "Simulated Wi-Fi").
        wifi.scanRandMacAddress = false;
      };
      environment.etc."access-points/hostapd.conf".text = lib.concatStringsSep "\n" (
        lib.imap0 bss networks
      );
      environment.systemPackages = [
        pkgs.hostapd
        pkgs.dnsmasq
        pkgs.iw
      ];
    };
  script = ''
    def start_access_points(machine):
        machine.wait_until_succeeds("test -e /sys/class/net/wlan0/phy80211")
        phy = machine.succeed("cat /sys/class/net/wlan0/phy80211/name").strip()
        machine.succeed(
            "ip netns add ap",
            f"iw phy {phy} set netns name ap",
            "ip netns exec ap hostapd -B -f /tmp/hostapd.log /etc/access-points/hostapd.conf",
        )
        devices = ${builtins.toJSON devices}
        machine.wait_until_succeeds(f"ip netns exec ap ip link show {devices[-1]}", timeout=30)
        ranges = []
        for i, device in enumerate(devices):
            machine.succeed(f"ip netns exec ap ip addr add 192.168.1{i}.1/24 dev {device}")
            ranges += [f"--interface={device}", f"--dhcp-range=192.168.1{i}.10,192.168.1{i}.50,12h"]
        machine.succeed(
            "ip netns exec ap dnsmasq --port=0 --bind-interfaces --log-dhcp "
            "--log-facility=/tmp/dnsmasq.log --pid-file=/tmp/dnsmasq.pid " + " ".join(ranges))
  '';
}
