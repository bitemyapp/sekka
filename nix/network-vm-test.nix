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
let
  wifi = import ./simulated-wifi.nix {
    inherit (pkgs) lib;
    inherit pkgs;
    networks = [
      {
        ssid = "Home";
        password = "correct horse";
      }
      {
        ssid = "Back Room";
        password = "backroom-pass";
        hidden = true;
      }
      {
        ssid = "Office";
        password = "office-pass";
      }
      { ssid = "Cafe"; }
    ];
  };
in
pkgs.testers.runNixOSTest {
  name = "tatami-network";
  globalTimeout = 1500;
  node.pkgsReadOnly = false;
  nodes.machine =
    { lib, ... }:
    {
      imports = [
        desktopsModule
        wifi.module
      ];
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
    };
  testScript = ''
    import shlex, time

    ${wifi.script}

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
    start_access_points(machine)
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
        # Removing Office precedes autoconnect; wait for Back Room to finish
        # activating, rather than assuming it has reconnected after a delay.
        wait_active("Back Room")
        machine.screenshot("refused")
        assert "Office" not in saved(), saved()

    with subtest("Passwords never stay on disk"):
        machine.fail("ls /run/user/1000/tatami-network-* 2>/dev/null | grep -q .")
        assert "Back Room" in active(), active()
  '';
}
