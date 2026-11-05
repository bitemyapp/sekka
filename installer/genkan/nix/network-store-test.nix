# Genkan makes the installed system's store of saved networks at
# installation, sealed to the computer's TPM, with the live session's Wi-Fi
# already inside (rust/src/install.rs, `save_networks`). The VM, with a
# software TPM, runs Genkan's storage and network steps on a marked
# disposable disk and then boots that disk on the same TPM: its first boot
# opens the store and joins the transferred network with nobody logged in,
# and the password is nowhere on the disk in plaintext, before or after.
# Like encryption-test.nix, a component test: the harness supplies the
# bootloader and the Nix store, and places the installed system as
# nixos-install would.
{ pkgs }:
let
  inherit (pkgs) lib;
  tests = import ./storage-tests.nix { inherit pkgs; };
  # The network in rust/src/install.rs's `network_store_tests::PROFILE`.
  wifi = import ../../../nix/simulated-wifi.nix {
    inherit lib pkgs;
    networks = [
      {
        ssid = "Installer";
        password = "installer-pass-2c9a";
      }
    ];
  };
  root = "/dev/disk/by-uuid/11111111-2222-4333-8444-555555555555";
in
pkgs.testers.runNixOSTest {
  name = "genkan-network-store";
  nodes.machine = {
    environment.etc."genkan-storage-test".text = "disposable VM\n";
    environment.systemPackages = [
      tests
      pkgs.e2fsprogs
    ];
    boot.loader.systemd-boot.enable = true;
    virtualisation = {
      memorySize = 2048;
      cores = 2;
      useBootLoader = true;
      useEFIBoot = true;
      mountHostNixStore = true;
      tpm.enable = true;
      emptyDiskImages = [
        {
          size = 32768;
          driveConfig.deviceExtraOpts.serial = "GENKAN_NETSTORE_TEST";
        }
      ];
    };
    # The installed system, on the root Genkan made: NetworkManager and the
    # store's module, as installed systems import it.
    specialisation.installed.configuration = {
      imports = [
        wifi.module
        ../rust/system/network-store.nix
      ];
      virtualisation.useDefaultFilesystems = false;
      virtualisation.fileSystems."/" = {
        device = root;
        fsType = "ext4";
      };
      virtualisation.rootDevice = root;
    };
  };
  testScript =
    { nodes, ... }:
    let
      installed = nodes.machine.specialisation.installed.configuration;
    in
    ''
      import json

      ${wifi.script}

      keyfiles = "/etc/NetworkManager/system-connections"
      password = "installer-pass-2c9a"

      def on_disk(text):
          machine.succeed("sync", "echo 3 > /proc/sys/vm/drop_caches")
          # Split at NULs: the mostly empty disk has no newlines, and grep
          # would hold all of it as one line.
          return machine.execute(f"grep -a -z -q -F {text} /dev/vdb")[0] == 0

      with subtest("Genkan seals the live session's Wi-Fi into a new store"):
          machine.start()
          machine.wait_for_unit("multi-user.target")
          print(machine.succeed(
              "GENKAN_TEST_SYSTEM=${installed.system.build.toplevel} storage-tests"
              " install::network_store_tests::wifi_is_sealed_at_installation --ignored --nocapture",
              timeout=600,
          ))
          # The scan reads what's on the disk: the secret-less copy is there.
          assert on_disk("ssid=Installer"), "the raw-disk scan found nothing at all"
          assert not on_disk(password), "the password reached the disk in plaintext"
          # Directory entries outlive their files: root has never had an
          # /etc, let alone NetworkManager's directory or a profile in it.
          listing = machine.succeed("debugfs -R 'ls -d /' /dev/vdb2")
          print(listing)
          assert "etc" not in [name.strip("<>") for name in listing.split()], listing

      with subtest("The installed system's first boot opens it"):
          # A real installation's activation writes fstab (encryption-test.nix).
          machine.succeed(
              "mkdir -p /mnt && mount /dev/vdb2 /mnt && mkdir /mnt/etc",
              "cp ${installed.environment.etc.fstab.source} /mnt/etc/fstab",
              "umount /mnt",
          )
          machine.succeed("${installed.system.build.toplevel}/bin/switch-to-configuration boot")
          machine.succeed("sync")
          machine.shutdown()
          machine.start()
          machine.wait_for_unit("NetworkManager.service")
          machine.succeed("test $(findmnt -n -o UUID /) = 11111111-2222-4333-8444-555555555555")
          status = json.loads(machine.succeed("cat /run/sekka/network-store.json"))
          assert status["state"] == "sealed", status
          bound = machine.succeed(f"findmnt -n -o SOURCE {keyfiles}").strip()
          assert bound.startswith("/dev/mapper/sekka-network-store"), bound
          # Nothing came from plaintext: the profile was in the store already.
          journal = machine.succeed("journalctl -b -o cat -u sekka-network-store.service")
          print(journal)
          assert "adopted" not in journal, journal
          machine.succeed("mkdir -p /mnt/root && mount --bind / /mnt/root")
          underneath = machine.succeed(f"ls -A /mnt/root{keyfiles}").split()
          machine.succeed("umount /mnt/root")
          assert underneath == [], underneath
          profile = f"{keyfiles}/installer-wifi-0.nmconnection"
          machine.succeed(f"test $(stat -c %u:%a {profile}) = 0:600")
          machine.succeed(f"grep -qx psk={password} {profile}")

      with subtest("It joins the network at boot, with nobody logged in"):
          start_access_points(machine)
          machine.wait_until_succeeds(
              "nmcli -t -f NAME,STATE connection show --active | grep -qx Installer:activated",
              timeout=180,
          )
          machine.fail("loginctl list-sessions --no-legend | grep -q .")
          assert not on_disk(password), "the password reached the disk in plaintext"
    '';
}
