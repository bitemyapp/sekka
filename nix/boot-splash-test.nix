# Sekka's boot splash in QEMU, at 1080p and at 4K: boot holds the splash
# until the test has looked at it, then the test checks its states as
# Plymouth's own commands drive them, types a passphrase into it, lets the
# login prompt take over, and powers off to see the shutdown splash.
# Screenshots of each state are in the result.
{ pkgs }:
let
  machine =
    xres: yres:
    { lib, ... }:
    {
      imports = [ ../installer/genkan/rust/system/boot-splash ];
      calamares.bootSplash.enable = true;
      virtualisation.memorySize = 1024;
      virtualisation.qemu.options = [
        "-vga none"
        "-device VGA,edid=on,xres=${toString xres},yres=${toString yres},vgamem_mb=64"
      ];
      # The display from the initrd on, as a GPU driver in the initrd gives it.
      boot.initrd.kernelModules = [ "bochs" ];
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
      # Something to wait for while shutting down, so the splash is seen.
      systemd.services.slow-stop = {
        wantedBy = [ "multi-user.target" ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
          ExecStart = "${lib.getExe' pkgs.coreutils "true"}";
          ExecStop = "${lib.getExe' pkgs.coreutils "sleep"} 20";
        };
      };
    };
in
pkgs.testers.runNixOSTest {
  name = "boot-splash";
  enableOCR = true;
  nodes.machine = machine 1920 1080;
  nodes.hidpi = machine 3840 2160;
  testScript = ''
    import shlex, time
    from datetime import timedelta

    WAIT = timedelta(minutes=2)

    def splash_up(m):
        m.wait_until_succeeds("plymouth --ping", timeout=WAIT)
        m.wait_until_succeeds('test "$(systemctl is-active splash-hold.service)" = activating')

    start_all()
    for m in (machine, hidpi):
        splash_up(m)
        # Still under the splash: nothing has quit Plymouth.
        m.fail("systemctl is-active plymouth-quit.service")
        assert "sekka" in m.succeed("cat /etc/plymouth/plymouthd.conf")

    with subtest("Boot: the lockup and the rail"):
        machine.screenshot("boot-1080p")
        hidpi.screenshot("boot-4k")
        machine.wait_for_text("sekka", timeout=WAIT)

    with subtest("Messages appear under the rail and go again"):
        machine.succeed("plymouth display-message --text='Checking the file system on root'")
        machine.wait_for_text("Checking the file system", timeout=WAIT)
        machine.screenshot("message-1080p")
        machine.succeed("plymouth hide-message --text='Checking the file system on root'")

    with subtest("A passphrase typed into the splash reaches whoever asked"):
        for m in (machine, hidpi):
            m.succeed(
                "systemd-run --unit=ask --property=StandardOutput=file:/run/answer "
                + "plymouth ask-for-password --prompt="
                + shlex.quote("Please enter passphrase for disk root (luks-1234):")
            )
            m.wait_until_succeeds("systemctl is-active ask.service")
        machine.wait_for_text("passphrase for disk root", timeout=WAIT)
        for m in (machine, hidpi):
            m.send_chars("snow on the")
        time.sleep(1)
        machine.screenshot("password-1080p")
        hidpi.screenshot("password-4k")
        machine.send_key("caps_lock")
        machine.wait_for_text("Caps Lock is on", timeout=WAIT)
        machine.screenshot("password-caps-1080p")
        machine.send_key("caps_lock")
        for m in (machine, hidpi):
            m.send_chars(" roof\n")
            m.wait_until_fails("systemctl is-active ask.service")
            answer = m.succeed("cat /run/answer").strip()
            assert answer == "snow on the roof", answer

    with subtest("A slow boot says how to see what it's doing, and Esc does show it"):
        # The hint shows 15 seconds after Plymouth started, in the initrd.
        machine.wait_until_succeeds("test $(cut -d. -f1 /proc/uptime) -ge 20")
        machine.wait_for_text("boot messages", timeout=WAIT)
        machine.screenshot("hint-1080p")
        hidpi.screenshot("hint-4k")
        machine.send_key("esc")
        machine.wait_for_text("splash-hold", timeout=WAIT)
        machine.screenshot("details-1080p")
        machine.send_key("esc")
        machine.wait_for_text("boot messages", timeout=WAIT)

    with subtest("The login prompt takes over when boot finishes"):
        for m in (machine, hidpi):
            m.succeed("touch /run/splash-release")
            m.wait_for_unit("multi-user.target")
            m.wait_until_fails("plymouth --ping")
            m.wait_for_unit("getty@tty1.service")
        machine.wait_until_tty_matches("1", "login: ")
        machine.screenshot("login-1080p")

    with subtest("Restarting and shutting down say so"):
        # Plymouth's other modes, shown by hand over the login prompt.
        for mode, text in (("reboot", "Restarting"), ("shutdown", "Shutting down")):
            machine.succeed(f"plymouthd --mode={mode} --tty=/dev/tty1 && plymouth show-splash")
            machine.wait_for_text(text, timeout=WAIT)
            machine.screenshot(f"{mode}-1080p")
            machine.succeed("plymouth quit --wait")
        hidpi.succeed("plymouthd --mode=reboot --tty=/dev/tty1 && plymouth show-splash")
        time.sleep(2)
        hidpi.screenshot("reboot-4k")
        hidpi.succeed("plymouth quit --wait")

    with subtest("Shutting down shows the splash"):
        hidpi.shutdown()
        machine.execute("systemctl poweroff --no-block")
        machine.wait_for_text("Shutting down", timeout=WAIT)
        machine.screenshot("poweroff-1080p")
        machine.wait_for_shutdown()
  '';
}
