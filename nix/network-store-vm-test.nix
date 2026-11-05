# Saved network connections sealed to the TPM
# (installer/genkan/rust/system/network-store.nix), in disposable VMs with
# simulated Wi-Fi and a software TPM. `machine` goes through the store's
# life: profiles saved before it, a store made only once every generation
# can open it and recovering from a stop at each step, adoption at the next
# boot, a password joined afterwards found nowhere on the raw disk, Wi-Fi at
# boot with nobody logged in, an evicted SRK, a cleared TPM and the old one
# coming back, and turning it off. `blacklisted` has a TPM whose driver never
# loads: NetworkManager mustn't wait for it.
{ pkgs }:
let
  inherit (pkgs) lib;
  module = ../installer/genkan/rust/system/network-store.nix;
  store = pkgs.callPackage ../installer/genkan/rust/system/network-store/package.nix { };
  wifi = import ./simulated-wifi.nix {
    inherit lib pkgs;
    networks = [
      {
        ssid = "Home";
        password = "correct horse";
      }
      {
        ssid = "Office";
        password = "office-pass-7d1e";
      }
    ];
  };
  common = {
    imports = [
      wifi.module
      module
    ];
    virtualisation.tpm.enable = true;
    # The access points' dnsmasq can't reopen its log from the last boot.
    boot.tmp.cleanOnBoot = true;
    environment.systemPackages = [
      pkgs.cryptsetup
      pkgs.e2fsprogs
      pkgs.tpm2-tools
    ];
  };
in
pkgs.testers.runNixOSTest {
  name = "network-store";
  nodes.machine = {
    imports = [ common ];
    # The test makes the store itself, stopping it partway first.
    systemd.services.sekka-network-store-create.wantedBy = lib.mkForce [ ];
  };
  nodes.blacklisted = {
    imports = [ common ];
    # The firmware still reports the TPM.
    boot.kernelParams = [ "module_blacklist=tpm_tis,tpm_tis_core,tpm_crb" ];
  };
  testScript = ''
    import json, shlex, shutil

    ${wifi.script}

    image = "/var/lib/sekka/network-store/store.luks"
    keyfiles = "/etc/NetworkManager/system-connections"
    store = "${lib.getExe store}"
    options = "--state /var/lib/sekka/network-store --keyfiles " + keyfiles

    def join(ssid, password):
        # The password through a pipe, as Tatami gives it, never on a command line.
        machine.succeed(
            f"nmcli connection add type wifi con-name {ssid} ifname wlan1 ssid {ssid} wifi-sec.key-mgmt wpa-psk",
        )
        connect(ssid, password)

    def connect(ssid, password):
        machine.succeed(
            f"printf '802-11-wireless-security.psk:%s\\n' {shlex.quote(password)}"
            f" | nmcli --wait 45 connection up id {ssid} passwd-file /dev/stdin"
        )
        active(ssid)

    def active(ssid):
        machine.wait_until_succeeds(
            f"nmcli -t -f NAME,STATE connection show --active | grep -qx {ssid}:activated", timeout=90)

    def listed(ssid):
        machine.succeed(f"nmcli -t -f NAME connection show | grep -qx {ssid}")

    def bound():
        return machine.succeed(f"findmnt -n -o SOURCE {keyfiles} || true").strip()

    def underneath():
        # The root filesystem's own directory, under the store.
        machine.succeed("mkdir -p /mnt/root && (mountpoint -q /mnt/root || mount --bind / /mnt/root)")
        names = machine.succeed(f"ls -A /mnt/root{keyfiles}").split()
        machine.succeed("umount /mnt/root")
        return names

    def state():
        return json.loads(machine.succeed("cat /run/sekka/network-store.json"))["state"]

    def stop_at(unit, step):
        machine.succeed(
            f"mkdir -p /run/systemd/system/{unit}.d",
            f"printf '[Service]\\nEnvironment=SEKKA_NETWORK_STORE_CRASH_AT={step}\\n' > /run/systemd/system/{unit}.d/crash.conf",
            "systemctl daemon-reload",
        )
        machine.fail(f"systemctl start {unit}")
        machine.succeed(f"rm -r /run/systemd/system/{unit}.d", "systemctl daemon-reload", f"systemctl reset-failed {unit}")

    def boot():
        machine.start()
        machine.wait_for_unit("NetworkManager.service")
        # systemd breaks an ordering cycle by dropping a start job: maybe the store's.
        machine.fail("journalctl -b -o cat | grep -q 'ordering cycle'")
        start_access_points(machine)
        machine.wait_until_succeeds(
            "nmcli -t -f SSID device wifi list --rescan yes | grep -qx Office", timeout=120)

    def reboot():
        machine.shutdown()
        boot()

    tpm = machine.state_dir / "machine-swtpm"
    first_tpm = machine.state_dir / "first-swtpm"

    with subtest("Saved before there's a store, in plaintext as before"):
        boot()
        support = machine.succeed("systemd-analyze has-tpm2 || true")
        print(support)
        assert "+driver" in support and "+subsystem" in support, support
        machine.fail(f"test -e {image}")
        join("Home", "correct horse")
        machine.succeed(f"grep -q 'psk=correct horse' {keyfiles}/Home.nmconnection")
        # As Genkan writes the live session's networks into the installed system.
        machine.succeed(
            f"printf '[connection]\\nid=Installer\\nuuid=135ea3d9-d456-44b1-ae42-1e7081f66666\\ntype=wifi\\n"
            f"[wifi]\\nssid=Installer\\n[wifi-security]\\nkey-mgmt=wpa-psk\\npsk=installer-pass-2c9a\\n'"
            f" > {keyfiles}/installer-wifi-0.nmconnection",
            f"chmod 600 {keyfiles}/installer-wifi-0.nmconnection",
        )

    with subtest("No store while a bootable generation couldn't open it"):
        machine.succeed(
            "mkdir -p /nix/var/nix/profiles /tmp/old-system",
            "ln -sfn /tmp/old-system /nix/var/nix/profiles/system-1-link",
            "systemctl start sekka-network-store-create",
        )
        machine.fail(f"test -e {image}")
        assert state() == "waiting-for-generations", state()
        machine.succeed("rm /nix/var/nix/profiles/system-1-link")

    with subtest("Making the store recovers from a stop at any step"):
        for step in ["allocated", "formatted", "enrolled", "made", "before-commit"]:
            stop_at("sekka-network-store-create.service", step)
            machine.fail(f"test -e {image}")
        machine.succeed("systemctl start sekka-network-store-create")
        dump = machine.succeed(f"cryptsetup luksDump {image}")
        print(dump)
        assert "sekka-network-store-v1" in dump, dump
        keyslots = dump.split("Keyslots:")[1].split("Tokens:")[0]
        assert keyslots.count(": luks2") == 1, f"one keyslot, the TPM's:\n{keyslots}"
        assert "systemd-tpm2" in dump.split("Tokens:")[1], dump
        assert state() == "created", state()
        # Opened from the next boot: nothing changed for NetworkManager yet.
        assert bound() == "", bound()

    with subtest("The next boot adopts what was saved in plaintext"):
        reboot()
        assert bound().startswith("/dev/mapper/sekka-network-store"), bound()
        assert state() == "sealed", state()
        assert underneath() == [], underneath()
        for name in ["Home.nmconnection", "installer-wifi-0.nmconnection"]:
            machine.succeed(f"test $(stat -c %u:%a {keyfiles}/{name}) = 0:600")
        machine.succeed(f"grep -q 'psk=correct horse' {keyfiles}/Home.nmconnection")
        listed("Installer")
        # Nobody has logged in: no agent, the system's own secrets.
        active("Home")
        machine.succeed(f"nsenter -t $(pidof NetworkManager) -m findmnt {keyfiles}")
        machine.succeed("test -e /var/lib/sekka/network-store/skeleton/Home.nmconnection")
        machine.fail("grep -q psk= /var/lib/sekka/network-store/skeleton/Home.nmconnection")

    with subtest("A password saved now never reaches the disk unencrypted"):
        join("Office", "office-pass-7d1e")
        machine.succeed(f"grep -q psk=office-pass-7d1e {keyfiles}/Office.nmconnection")
        assert "Office.nmconnection" not in underneath()
        disk = machine.succeed("findmnt -n -o SOURCE /").strip()
        machine.succeed("sync", "echo 3 > /proc/sys/vm/drop_caches")
        machine.fail(f"grep -a -q office-pass-7d1e {disk}")

    with subtest("Stopping the unit under NetworkManager leaves the store open"):
        machine.succeed("systemctl stop sekka-network-store")
        assert bound().startswith("/dev/mapper/sekka-network-store"), bound()
        machine.succeed("systemctl start sekka-network-store")

    with subtest("Adoption recovers from a stop partway"):
        machine.succeed("systemctl stop NetworkManager", "systemctl stop sekka-network-store")
        assert bound() == "", bound()
        machine.succeed(
            f"printf '[connection]\\nid=Spare\\ntype=wifi\\n[wifi]\\nssid=Spare\\n' > {keyfiles}/Spare.nmconnection",
            f"chmod 600 {keyfiles}/Spare.nmconnection",
        )
        stop_at("sekka-network-store.service", "merge-placed")
        machine.succeed("systemctl start sekka-network-store")
        assert bound().startswith("/dev/mapper/sekka-network-store"), bound()
        assert underneath() == [], underneath()
        machine.succeed(f"grep -q id=Spare {keyfiles}/Spare.nmconnection")
        machine.fail(f"test -e {keyfiles}/.sekka-conflicts")
        machine.succeed("systemctl start NetworkManager")
        active("Office")

    with subtest("An evicted SRK doesn't lock the store"):
        machine.succeed("tpm2_evictcontrol -C o -c 0x81000001")
        reboot()
        assert bound().startswith("/dev/mapper/sekka-network-store"), bound()
        assert state() == "sealed", state()

    with subtest("A cleared TPM: saved networks stay listed, and a new store is made"):
        machine.shutdown()
        shutil.copytree(tpm, first_tpm)
        shutil.rmtree(tpm)
        boot()
        assert bound() == "", bound()
        assert state() == "unsealable", state()
        listed("Home")
        listed("Office")
        machine.fail(f"grep -q psk= {keyfiles}/Home.nmconnection")
        machine.succeed("systemctl start sekka-network-store-create")
        assert state() == "rotated", state()
        machine.succeed("test $(ls /var/lib/sekka/network-store/rotated/*.luks | wc -l) = 1")
        # Typed once more, kept as before until the next boot seals it.
        connect("Office", "office-pass-7d1e")
        reboot()
        assert bound().startswith("/dev/mapper/sekka-network-store"), bound()
        machine.succeed(f"grep -q psk=office-pass-7d1e {keyfiles}/Office.nmconnection")
        machine.fail(f"grep -q psk= {keyfiles}/Home.nmconnection")
        active("Office")

    with subtest("The first TPM back: its store is merged in again"):
        machine.shutdown()
        shutil.rmtree(tpm)
        shutil.copytree(first_tpm, tpm)
        boot()
        # The newest store was sealed to the other TPM: set aside in turn.
        assert state() == "unsealable", state()
        machine.succeed("systemctl start sekka-network-store-create")
        reboot()
        assert bound().startswith("/dev/mapper/sekka-network-store"), bound()
        machine.succeed(f"grep -q 'psk=correct horse' {keyfiles}/Home.nmconnection")
        machine.succeed(f"grep -q psk=office-pass-7d1e {keyfiles}/Office.nmconnection")
        machine.succeed(f"grep -q psk=installer-pass-2c9a {keyfiles}/installer-wifi-0.nmconnection")
        # The other TPM's store stays set aside; the first one's was merged.
        machine.succeed("test $(ls /var/lib/sekka/network-store/rotated/*.luks | wc -l) = 1")
        # Home joins with the password merged back, no agent asked.
        machine.succeed("nmcli --wait 45 connection up id Home")
        active("Home")

    with subtest("Turned off, the profiles move back out"):
        machine.succeed("systemctl stop NetworkManager", "systemctl stop sekka-network-store")
        machine.succeed(f"{store} export {options}")
        assert state() == "exported", state()
        machine.fail(f"test -e {image}")
        assert bound() == "", bound()
        machine.succeed(f"grep -q 'psk=correct horse' {keyfiles}/Home.nmconnection")
        machine.succeed("systemctl start NetworkManager")
        active("Home")

    with subtest("A TPM without its driver holds nothing up"):
        blacklisted.start()
        blacklisted.wait_for_unit("NetworkManager.service")
        up = int(blacklisted.succeed(
            "systemctl show -p ActiveEnterTimestampMonotonic --value NetworkManager.service")) / 1e6
        assert up < 30, f"NetworkManager started {up:.1f} s into the boot"
        blacklisted.fail("systemd-analyze has-tpm2")
        blacklisted.fail(f"test -e {image}")
  '';
}
