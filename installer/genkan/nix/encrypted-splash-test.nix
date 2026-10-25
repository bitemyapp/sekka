# Sekka's boot splash over Genkan's encrypted storage, in disposable VMs:
# Genkan's production storage code encrypts the disk, then the VM boots the
# storage configuration Genkan generates for it with the splash turned on.
# The splash must ask for the passphrase in the initrd, take it once for
# root and swap, ask again after a wrong one, carry on to the login prompt,
# and restart without leaving the encrypted root unclean. Like the
# encrypted-storage check, the harness supplies the bootloader and store.
# Screenshots of each state are in the result.
{ pkgs }:
let
  tests = import ./storage-tests.nix { inherit pkgs; };
  fixture = firmware: ../rust/src/test-data + "/${firmware}-ext4.nix";
  node =
    firmware:
    { lib, ... }:
    {
      imports = [ ../rust/system/boot-splash ];
      options.calamares.zswap.enable = lib.mkEnableOption "fixture zswap selection";
      config = {
        environment.etc."genkan-storage-test".text = "disposable VM\n";
        environment.systemPackages = [
          tests
          pkgs.cryptsetup
          pkgs.parted
        ];
        boot.supportedFilesystems = [
          "ext4"
          "btrfs"
          "xfs"
          "vfat"
        ];
        boot.kernelModules = [ "dm-crypt" ];
        boot.loader.systemd-boot.enable = firmware == "uefi";
        boot.loader.grub.enable = firmware == "bios";
        virtualisation = {
          memorySize = 4096;
          cores = 2;
          useBootLoader = true;
          useEFIBoot = firmware == "uefi";
          mountHostNixStore = true;
          # No GPU driver in the initrd, as on an installed system: the
          # passphrase is asked on the firmware's framebuffer, and the
          # splash moves to the GPU's display once its driver loads.
          qemu.options = [
            "-vga none"
            "-device VGA,edid=on,xres=1920,yres=1080,vgamem_mb=64"
          ];
          emptyDiskImages = [
            {
              size = 32768;
              driveConfig = {
                deviceExtraOpts.serial = "GENKAN_LUKS_TEST";
                driveExtraOpts.discard = "unmap";
              };
            }
          ];
        };
        specialisation.encrypted.configuration = {
          imports = [ (fixture firmware) ];
          calamares.bootSplash.enable = true;
          virtualisation.useDefaultFilesystems = false;
          virtualisation.fileSystems."/" = (import (fixture firmware) { inherit lib; }).fileSystems."/";
          virtualisation.rootDevice = "/dev/disk/by-uuid/11111111-2222-4333-8444-555555555555";
          # Test VMs log to a serial console, which would make Plymouth show
          # only text.
          boot.kernelParams = [ "plymouth.ignore-serial-consoles" ];
          # Hold the splash until the test has looked at it.
          systemd.services.splash-hold = {
            wantedBy = [ "multi-user.target" ];
            before = [
              "plymouth-quit.service"
              "plymouth-quit-wait.service"
            ];
            serviceConfig = {
              Type = "oneshot";
              TimeoutStartSec = "10min";
            };
            script = "until [ -e /run/splash-release ]; do sleep 0.2; done";
          };
          # Something to wait for while restarting, so the splash is seen.
          systemd.services.slow-stop = {
            wantedBy = [ "multi-user.target" ];
            serviceConfig = {
              Type = "oneshot";
              RemainAfterExit = true;
              ExecStart = "${lib.getExe' pkgs.coreutils "true"}";
              ExecStop = "${lib.getExe' pkgs.coreutils "sleep"} 30";
            };
          };
        };
      };
    };
in
pkgs.testers.runNixOSTest {
  name = "genkan-encrypted-boot-splash";
  nodes = {
    uefi = node "uefi";
    bios = node "bios";
  };
  enableOCR = true;
  testScript =
    { nodes, ... }:
    ''
      import shlex, time
      from datetime import timedelta

      WAIT = timedelta(minutes=3)
      PASSPHRASE = "Genkan-test-only-123"
      REJECTED = "Failed to activate with specified passphrase"
      UNLOCKS = "-u systemd-cryptsetup@cryptroot.service -u systemd-cryptsetup@cryptswap.service"

      def asked(m):
          m.wait_for_text("[Pp]assphrase for", timeout=WAIT)

      def splash_up(m):
          m.wait_until_succeeds("plymouth --ping", timeout=WAIT)
          m.wait_until_succeeds('test "$(systemctl is-active splash-hold.service)" = activating', timeout=WAIT)

      def uptime(m):
          return float(m.succeed("cut -d' ' -f1 /proc/uptime"))

      def gone(m, text):
          # OCR until text is no longer on the screen.
          deadline = time.monotonic() + WAIT.total_seconds()
          while text in m.get_screen_text():
              assert time.monotonic() < deadline, f"{text} is still showing"
              time.sleep(1)

      def journal_time(m, text):
          # Seconds since the kernel started when this boot first logged text.
          line = m.succeed(f"journalctl -b -o short-monotonic --no-pager --grep={shlex.quote(text)}").splitlines()[0]
          return float(line.split("]")[0].strip("[ "))

      for machine, firmware, system, fstab in [
          (uefi, "uefi", "${nodes.uefi.specialisation.encrypted.configuration.system.build.toplevel}", "${nodes.uefi.specialisation.encrypted.configuration.environment.etc.fstab.source}"),
          (bios, "bios", "${nodes.bios.specialisation.encrypted.configuration.system.build.toplevel}", "${nodes.bios.specialisation.encrypted.configuration.environment.etc.fstab.source}"),
      ]:
          with subtest(f"{firmware}: Genkan encrypts the disk, and the splash is turned on"):
              machine.start()
              machine.wait_for_unit("multi-user.target")
              print(machine.succeed(f"GENKAN_TEST_FIRMWARE={firmware} storage-tests install::encryption_tests::encrypted_storage_matrix --ignored --nocapture", timeout=timedelta(minutes=20)))
              # A real installation's activation writes fstab; systemd reloads
              # root's fstab before switch-root.
              machine.succeed(f"printf %s {PASSPHRASE} | cryptsetup open --key-file - /dev/vdb2 fixture-root")
              machine.succeed("mkdir -p /mnt; mount /dev/mapper/fixture-root /mnt; mkdir -p /mnt/etc")
              machine.succeed(f"cp {fstab} /mnt/etc/fstab")
              machine.succeed("umount /mnt; cryptsetup close fixture-root")
              machine.succeed(f"{system}/bin/switch-to-configuration boot")
              machine.succeed("sync")
              machine.crash()
              machine.start(allow_reboot=True)

          with subtest(f"{firmware}: the splash asks for the passphrase in the initrd"):
              asked(machine)
              first_asked = time.monotonic()
              machine.wait_for_text("sekka", timeout=WAIT)
              machine.screenshot(f"{firmware}-1-asked")

          with subtest(f"{firmware}: one passphrase opens root and swap"):
              machine.send_chars(PASSPHRASE)
              # Someone takes their time over it: that isn't a slow start, so
              # the hint doesn't come though Plymouth's clock is past 15 seconds.
              time.sleep(max(0.0, 25 - (time.monotonic() - first_asked)))
              machine.screenshot(f"{firmware}-2-typed")
              assert "boot messages" not in machine.get_screen_text()
              machine.send_key("ret")
              splash_up(machine)
              unlocked_at = uptime(machine)
              machine.screenshot(f"{firmware}-3-unlocked")
              unlocked = machine.get_screen_text()
              machine.succeed("cryptsetup status cryptroot", "cryptsetup status cryptswap")
              machine.succeed("swapon --show=NAME --noheadings | grep -q /dev/dm-")
              assert machine.succeed("cat /encrypted-sentinel").strip() == "written by Genkan"
              unlocks = machine.succeed(f"journalctl -b -o cat {UNLOCKS}")
              print(unlocks)
              assert REJECTED not in unlocks, unlocks
              # The splash that asked in the initrd is still the one showing.
              assert machine.succeed("tr '\\0' ' ' < /proc/$(cat /run/plymouth/pid)/cmdline").startswith("@")

          with subtest(f"{firmware}: time spent at the prompt doesn't count as a slow start"):
              # The start itself, not counting the prompt: from Plymouth to
              # the prompt (at once), and from the passphrase (when cryptsetup
              # sets the cipher) to the screenshot. Unlocking and starting take
              # about 10 seconds here; a busy host can make it more.
              started = journal_time(machine, "Started Show Plymouth Boot Screen")
              asked_at = journal_time(machine, "Started Forward Password Requests to Plymouth[.]$")
              entered = journal_time(machine, "Set cipher")
              starting = (asked_at - started) + (unlocked_at - entered)
              print(f"{starting:.1f} seconds of starting, not counting the prompt")
              if starting < 13:
                  assert "boot messages" not in unlocked, unlocked
              machine.wait_for_text("boot messages", timeout=WAIT)
              machine.screenshot(f"{firmware}-4-hint")

          with subtest(f"{firmware}: after the GPU takes over, the splash still hears the keyboard"):
              # bochs took over from simpledrm after the unlock. Plymouth lost
              # the keyboard then, in about half the boots, and the splash read
              # the one Plymouth had freed (rust/system/boot-splash/plymouth).
              drivers = machine.succeed("for c in /sys/class/drm/card[0-9]; do basename $(readlink $c/device/driver); done").split()
              assert drivers == ["bochs-drm"], drivers
              machine.succeed(
                  "systemd-run --unit=ask --property=StandardOutput=file:/run/answer "
                  + "plymouth ask-for-password --prompt='Please enter passphrase for disk data (cryptdata):'"
              )
              machine.wait_for_text("cryptdata", timeout=WAIT)
              machine.send_key("caps_lock")
              machine.wait_for_text("Caps Lock is on", timeout=WAIT)
              machine.screenshot(f"{firmware}-5-asked-after-takeover")
              machine.send_key("caps_lock")
              machine.send_chars("snow\n")
              machine.wait_until_fails("systemctl is-active ask.service", timeout=WAIT)
              assert machine.succeed("cat /run/answer").strip() == "snow"
              machine.send_key("esc")
              machine.wait_for_text("splash-hold", timeout=WAIT)
              machine.send_key("esc")
              machine.wait_for_text("sekka", timeout=WAIT)

          with subtest(f"{firmware}: a question goes when whoever asked it does"):
              machine.succeed(
                  "systemd-run --unit=ask-and-go "
                  + "plymouth ask-for-password --prompt='Please enter passphrase for disk data (cryptdata):'"
              )
              machine.wait_for_text("cryptdata", timeout=WAIT)
              machine.succeed("systemctl stop ask-and-go.service")
              gone(machine, "cryptdata")
              machine.succeed("plymouth --ping")
              assert "No coredumps" in machine.execute("coredumpctl list --no-pager plymouthd 2>&1")[1]

          with subtest(f"{firmware}: the login prompt takes over"):
              machine.succeed("touch /run/splash-release")
              machine.wait_for_unit("multi-user.target")
              machine.wait_until_fails("plymouth --ping")
              machine.wait_until_tty_matches("1", "login: ")
              machine.screenshot(f"{firmware}-6-login")

          with subtest(f"{firmware}: restarting shows the splash"):
              machine.reboot()
              machine.wait_for_text("Restarting", timeout=WAIT)
              machine.screenshot(f"{firmware}-7-restarting")

          with subtest(f"{firmware}: a wrong passphrase is asked for again"):
              asked(machine)
              machine.send_chars("not the passphrase\n")
              # Argon2id takes a moment to say no.
              time.sleep(10)
              asked(machine)
              # Root alone: swap waits for root.
              assert "cryptroot" in machine.get_screen_text()
              machine.screenshot(f"{firmware}-8-asked-again")

          with subtest(f"{firmware}: Caps Lock is pointed out while typing"):
              machine.send_key("caps_lock")
              machine.wait_for_text("Caps Lock is on", timeout=WAIT)
              machine.screenshot(f"{firmware}-9-caps-lock")
              machine.send_key("caps_lock")

          with subtest(f"{firmware}: Esc shows the question as text and comes back"):
              machine.send_key("esc")
              # Plymouth's text view: systemd's lines, which the splash never shows.
              machine.wait_for_text("Found device|[Cc]ryptography", timeout=WAIT)
              machine.screenshot(f"{firmware}-10-details")
              machine.send_key("esc")
              machine.wait_for_text("sekka", timeout=WAIT)

          with subtest(f"{firmware}: the right one opens both, nothing goes on asking, and the restart left root clean"):
              machine.send_chars(PASSPHRASE + "\n")
              splash_up(machine)
              machine.screenshot(f"{firmware}-11-unlocked-again")
              # Swap opened after root with root's passphrase, without asking or
              # failing, and no question is left up for either disk.
              assert "assphrase" not in machine.get_screen_text()
              root = machine.succeed("journalctl -b -o cat -u systemd-cryptsetup@cryptroot.service")
              swap = machine.succeed("journalctl -b -o cat -u systemd-cryptsetup@cryptswap.service")
              print(root, swap)
              assert root.count(REJECTED) == 1, root
              assert REJECTED not in swap, swap
              assert journal_time(machine, "Finished Cryptography Setup for cryptroot") <= journal_time(machine, "Starting Cryptography Setup for cryptswap")
              machine.succeed("cryptsetup status cryptroot", "cryptsetup status cryptswap")
              kernel = machine.succeed("journalctl -k -b -o cat")
              assert "recovery complete" not in kernel and "orphan" not in kernel, kernel
              machine.succeed("touch /run/splash-release")
              machine.wait_until_tty_matches("1", "login: ")
              machine.shutdown()
    '';
}
