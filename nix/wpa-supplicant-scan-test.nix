# wpa_supplicant still joins a network after a scan it was asked for is
# dropped (installer/genkan/rust/system/wpa-supplicant), in a disposable VM
# with simulated Wi-Fi. NetworkManager drops one when it takes the interface
# down to change its scanning address; here wpa_supplicant is paused while a
# D-Bus Scan with AllowRoam=false and wlan1 going down both wait for it, so
# the scan is queued and then removed before it starts. SelectNetwork must
# then join Home. Without the fix, wpa_supplicant treats that join's scan as
# scan-only and joins nothing.
{
  pkgs,
  # Configurations that must run the fixed wpa_supplicant: the live session's
  # and installed systems'.
  systems,
}:
let
  inherit (pkgs) lib;
  fix = ../installer/genkan/rust/system/wpa-supplicant;
  patch = fix + "/clear-scan-res-handler.patch";
  # The wpa_supplicant NetworkManager starts over D-Bus.
  supplicant = c: lib.findFirst (p: (p.pname or "") == "wpa_supplicant") { } c.services.dbus.packages;
  fixed = c: lib.elem patch ((supplicant c).patches or [ ]);
  wifi = import ./simulated-wifi.nix {
    inherit lib pkgs;
    networks = [
      {
        ssid = "Home";
        password = "correct horse";
      }
    ];
  };
in
assert lib.all fixed systems;
pkgs.testers.runNixOSTest {
  name = "wpa-supplicant-scan";
  node.pkgsReadOnly = false;
  nodes.machine = {
    imports = [
      wifi.module
      fix
    ];
    # The test drives wpa_supplicant on wlan1 itself, over D-Bus.
    networking.networkmanager.unmanaged = [ "interface-name:wlan1" ];
  };
  testScript = ''
    import shlex

    ${wifi.script}

    service = "fi.w1.wpa_supplicant1"

    def busctl(*args):
        return machine.succeed("busctl " + " ".join(shlex.quote(str(a)) for a in args)).strip()

    def value(reply):
        # `o "/fi/w1/..."` or `s "completed"`
        return reply.split(" ", 1)[1].strip('"')

    def logged(pattern):
        # Succeeds once wpa_supplicant has logged a line matching pattern.
        return ("journalctl -b -u wpa_supplicant.service -q --no-pager --grep "
                + shlex.quote(pattern) + " | grep -q .")

    machine.start()
    start_access_points(machine)
    machine.wait_for_unit("NetworkManager.service")
    machine.succeed("ip link set wlan1 up")
    status, reply = machine.execute(f"busctl call {service} /fi/w1/wpa_supplicant1 {service} GetInterface s wlan1")
    if status != 0:
        reply = busctl("call", service, "/fi/w1/wpa_supplicant1", service, "CreateInterface",
                       "a{sv}", 2, "Ifname", "s", "wlan1", "Driver", "s", "nl80211")
    interface = value(reply.strip())
    busctl("set-property", service, "/fi/w1/wpa_supplicant1", service, "DebugLevel", "s", "debug")
    # Added networks start disabled: nothing joins Home before SelectNetwork.
    network = value(busctl("call", service, interface, service + ".Interface", "AddNetwork", "a{sv}", 3,
                           "ssid", "s", "Home", "psk", "s", "correct horse", "key_mgmt", "s", "WPA-PSK"))
    machine.wait_until_succeeds(
        f"busctl get-property {service} {interface} {service}.Interface Scanning | grep -qx 'b false'")

    with subtest("A scan is removed before it starts"):
        pid = machine.succeed("pidof wpa_supplicant").strip()
        machine.succeed(f"kill -STOP {pid}")
        machine.succeed(
            f"busctl call --expect-reply=no {service} {interface} {service}.Interface Scan "
            "'a{sv}' 2 Type s active AllowRoam b false")
        machine.succeed("ip link set wlan1 down", "sleep 0.5")
        machine.succeed(f"kill -CONT {pid}")
        # Removed work that had started would say "(started)".
        machine.wait_until_succeeds(logged(r"Remove radio work 'scan'@0x[0-9a-f]+$"), timeout=10)
        machine.succeed("ip link set wlan1 up")

    with subtest("The next join still connects"):
        busctl("call", service, interface, service + ".Interface", "SelectNetwork", "o", network)
        machine.wait_until_succeeds(
            f"busctl get-property {service} {interface} {service}.Interface State | grep -qx 's \"completed\"'",
            timeout=60)
        machine.fail(logged("Scan-only results received"))
  '';
}
