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
      # A password nmcli's passwd-file rewrites unless it is escaped: a
      # backslash, and a space at either end.
      {
        ssid = "Annex";
        password = " back\\slash ";
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
      # A password nmcli's passwd-file rewrites unless it is escaped: a
      # backslash, and a space at its end.
      {
        ssid = "Attic";
        password = "attic\\key ";
      }
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
  testScript = builtins.readFile ../tests/vm_start.py + ''
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

    start_vm(machine)
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

    with subtest("A password with a backslash and spaces at its ends joins as typed"):
        window("--join Annex")
        time.sleep(3)
        machine.send_chars(" back\\slash \n")
        wait_active("Annex")

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

    with subtest("Closing the window while joining leaves nothing behind"):
        window("--join Office")
        time.sleep(3)
        machine.send_chars("wrong-password\n")
        machine.wait_until_succeeds("nmcli -t -f NAME connection show | grep -qx Office", timeout=30)

        def secrets():
            return machine.execute("ls /run/user/1000/tatami-network-* 2>/dev/null")[1].split()

        # nmcli is waiting for the network: its password is in no file.
        waiting = secrets()
        # Escape closes the window from its first page; the join goes on.
        machine.send_key("esc")
        machine.wait_until_fails(
            "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
            f"HYPRLAND_INSTANCE_SIGNATURE={signature} hyprctl clients | grep -q org.tatami.Network",
            timeout=30)
        deadline = time.time() + 90
        while "Office" in saved() and time.time() < deadline:
            time.sleep(1)
        assert not waiting and not secrets() and "Office" not in saved(), (
            "closing the window while joining Office with a wrong password: "
            f"passwd-files while nmcli waited {waiting}, afterwards {secrets()}; "
            f"saved connections {saved()}")
        # The program ends with the join.
        machine.wait_until_fails("pgrep -u demo -f bin/tatami-network", timeout=30)
        wait_active("Back Room")

    with subtest("Passwords never stay on disk"):
        machine.fail("ls /run/user/1000/tatami-network-* 2>/dev/null | grep -q .")
        assert "Back Room" in active(), active()

    with subtest("The Wi-Fi list hands its password to nmcli on a pipe, as typed"):
        # Super+Ctrl+W opens the Walker list of networks (`tatami network`),
        # once Super+W has closed the Network window if it is still open.
        # While the password is typed and the network joined, a loop in the VM
        # notes ten times a second whether nmcli is joining with its
        # passwd-file on standard input, any process with the password in its
        # arguments or its environment, and any password file of the helper's.
        machine.send_key("meta_l-w")
        machine.wait_until_fails(
            "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
            f"HYPRLAND_INSTANCE_SIGNATURE={signature} hyprctl clients | grep -q org.tatami.Network",
            timeout=30)
        machine.succeed(
            "install -m 600 /dev/null /run/wifi-key",
            "cat > /run/wifi-key <<'END'\nattic\\key\nEND",
            "cat > /run/wifi-watch.sh <<'END'\n"
            "while [ ! -e /run/wifi-watch.stop ]; do\n"
            "  pgrep -f 'nmcli .*connection up .*passwd-file /dev/stdin' > /dev/null && echo nmcli\n"
            "  grep -laF -f /run/wifi-key /proc/[0-9]*/cmdline /proc/[0-9]*/environ 2> /dev/null\n"
            "  ls /run/user/1000/tatami-wifi-* 2> /dev/null\n"
            "  sleep 0.1\n"
            "done > /run/wifi-watch.log\n"
            "END")
        machine.send_key("meta_l-ctrl-w")
        machine.wait_until_succeeds(
            "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
            f"HYPRLAND_INSTANCE_SIGNATURE={signature} hyprctl layers | grep -q walker",
            timeout=30)
        time.sleep(1)
        machine.send_chars("Attic")
        time.sleep(1)
        machine.send_key("ret")
        machine.wait_until_succeeds("pgrep -u demo -f 'walker.*--passwor[d]'", timeout=30)
        time.sleep(1)
        machine.succeed("setsid -f sh /run/wifi-watch.sh > /dev/null 2>&1 < /dev/null")
        machine.send_chars("attic\\key ")
        machine.send_key("ret")
        wait_active("Attic")
        machine.succeed("touch /run/wifi-watch.stop")
        seen = set(machine.succeed("cat /run/wifi-watch.log").split())
        assert "nmcli" in seen, "nmcli was not seen joining Attic"
        assert seen == {"nmcli"}, f"while nmcli joined Attic, the password was in {sorted(seen - {'nmcli'})}"
  '';
}
