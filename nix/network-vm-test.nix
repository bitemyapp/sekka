# The Network window against real NetworkManager and Wi-Fi, in a disposable
# VM: simulated radios (mac80211_hwsim), hostapd serving a WPA2 network, a
# hidden one, another for a wrong password and an open one, and a DHCP server
# handing out addresses, as a router does. The window is
# opened in a Tatami session the way the network panel opens it
# (`tatami-network --join NAME`) and answered from the keyboard; each outcome
# is checked in NetworkManager.
{
  pkgs,
  desktopsModule,
}:
pkgs.testers.runNixOSTest {
  name = "tatami-network";
  globalTimeout = 1500;
  node.pkgsReadOnly = false;
  nodes.machine =
    { lib, ... }:
    {
      imports = [ desktopsModule ];
      calamares.desktops = [ "tatami" ];
      calamares.defaultDesktop = "tatami";
      users.users.demo = {
        isNormalUser = true;
        uid = 1000;
        password = "demo";
        extraGroups = [
          "wheel"
          "networkmanager"
        ];
      };
      services.displayManager.autoLogin = {
        enable = true;
        user = "demo";
      };
      virtualisation = {
        memorySize = 4096;
        cores = 4;
        qemu.options = [
          "-vga none"
          "-device virtio-gpu-pci,xres=1280,yres=800"
        ];
      };
      hardware.graphics.enable = true;
      environment.sessionVariables.LIBGL_ALWAYS_SOFTWARE = "1";
      # Two radios: the access points on wlan0, the computer's Wi-Fi on wlan1.
      boot.kernelModules = [ "mac80211_hwsim" ];
      # NetworkManager's Wi-Fi backend, which test VMs otherwise turn off
      # (mkVMOverride).
      networking.wireless.enable = lib.mkOverride 9 true;
      networking.networkmanager = {
        enable = true;
        unmanaged = [ "interface-name:wlan0*" ];
      };
      # The access points: the first radio, moved into a network namespace of
      # its own with hostapd and a DHCP server, so the computer's Wi-Fi on
      # wlan1 reaches them over the simulated air as it would a router.
      environment.etc."access-points/hostapd.conf".text = ''
        interface=wlan0
        driver=nl80211
        hw_mode=g
        channel=6
        ssid=Home
        wpa=2
        wpa_key_mgmt=WPA-PSK
        rsn_pairwise=CCMP
        wpa_passphrase=correct horse

        bss=wlan0-1
        bssid=02:00:00:00:00:01
        ssid=Back Room
        ignore_broadcast_ssid=1
        wpa=2
        wpa_key_mgmt=WPA-PSK
        rsn_pairwise=CCMP
        wpa_passphrase=backroom-pass

        bss=wlan0-2
        bssid=02:00:00:00:00:02
        ssid=Office
        wpa=2
        wpa_key_mgmt=WPA-PSK
        rsn_pairwise=CCMP
        wpa_passphrase=office-pass

        bss=wlan0-3
        bssid=02:00:00:00:00:03
        ssid=Cafe
      '';
      environment.systemPackages = [
        pkgs.hostapd
        pkgs.dnsmasq
        pkgs.iw
      ];
    };
  testScript = ''
    import shlex, time

    def user(command):
        status, output = machine.execute(
            "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus sh -c "
            + shlex.quote("{ " + command + "; } 2>&1"))
        if status != 0:
            raise Exception(f"{command!r} failed with {status}:\n{output}")
        return output

    def window(args):
        # As the network panel, the menu and the launcher start it, with the
        # session's configuration directories (`tatami session`).
        user("XDG_CONFIG_DIRS=/etc/xdg/tatami:/etc/xdg tatami launch tatami-network " + args)

    def active():
        return machine.succeed("nmcli -t -f NAME connection show --active").splitlines()

    def saved():
        return machine.succeed("nmcli -t -f NAME connection show").splitlines()

    def wait_active(name, timeout=90):
        # Connected, not merely connecting: --active lists both.
        machine.wait_until_succeeds(
            "nmcli -t -f NAME,STATE connection show --active | grep -qx "
            + shlex.quote(name + ":activated"), timeout=timeout)

    machine.start()
    machine.wait_until_succeeds("test -e /sys/class/net/wlan0/phy80211")
    phy = machine.succeed("cat /sys/class/net/wlan0/phy80211/name").strip()
    machine.succeed(
        "ip netns add ap",
        f"iw phy {phy} set netns name ap",
        "ip netns exec ap hostapd -B -f /tmp/hostapd.log /etc/access-points/hostapd.conf",
    )
    machine.wait_until_succeeds("ip netns exec ap ip link show wlan0-3", timeout=30)
    ranges = []
    for i, device in enumerate(["wlan0", "wlan0-1", "wlan0-2", "wlan0-3"]):
        machine.succeed(f"ip netns exec ap ip addr add 192.168.1{i}.1/24 dev {device}")
        ranges += [f"--interface={device}", f"--dhcp-range=192.168.1{i}.10,192.168.1{i}.50,12h"]
    machine.succeed(
        "ip netns exec ap dnsmasq --port=0 --bind-interfaces --log-dhcp "
        "--log-facility=/tmp/dnsmasq.log --pid-file=/tmp/dnsmasq.pid " + " ".join(ranges))
    machine.wait_until_succeeds("test -S /run/user/1000/hypr/*/.socket.sock", timeout=180)
    signature = machine.succeed("basename /run/user/1000/hypr/*").strip()
    machine.wait_until_succeeds(
        "nmcli -t -f SSID device wifi list --rescan yes | grep -qx Home", timeout=120)
    print(machine.succeed("nmcli -t -f IN-USE,SSID,SECURITY,SIGNAL device wifi list"))

    with subtest("Joining with a password, as the panel hands a network over"):
        window("--join Home")
        machine.wait_until_succeeds(
            "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
            f"HYPRLAND_INSTANCE_SIGNATURE={signature} hyprctl clients | grep -q org.tatami.Network",
            timeout=60)
        time.sleep(3)
        machine.screenshot("password")
        machine.send_chars("correct horse\n")
        wait_active("Home")
        time.sleep(3)
        machine.screenshot("joined")

    with subtest("Joining a hidden network by name"):
        # A second start reaches the open window, which asks for the
        # network's security and password.
        window("--join 'Back Room'")
        time.sleep(3)
        machine.screenshot("hidden")
        machine.send_chars("backroom-pass\n")
        wait_active("Back Room")
        assert machine.succeed(
            "nmcli -g 802-11-wireless.hidden connection show 'Back Room'").strip() == "yes"

    with subtest("A wrong password leaves nothing behind"):
        window("--join Office")
        time.sleep(3)
        machine.send_chars("wrong-password\n")
        # The connection is made, joining gives up within its wait, and the
        # connection goes again.
        machine.wait_until_succeeds("nmcli -t -f NAME connection show | grep -qx Office", timeout=30)
        machine.wait_until_succeeds("! nmcli -t -f NAME connection show | grep -qx Office", timeout=90)
        time.sleep(2)
        machine.screenshot("refused")
        assert "Office" not in saved(), saved()

    with subtest("Passwords never stay on disk"):
        machine.fail("ls /run/user/1000/tatami-network-* 2>/dev/null | grep -q .")
        assert "Back Room" in active(), active()
  '';
}
