# Real activation and boot selection, with no host configuration or devices.
{
  pkgs,
  yukimi,
  yukimiModule,
}:
pkgs.testers.runNixOSTest {
  name = "yukimi-update-completion";
  globalTimeout = 300;
  nodes.machine = { ... }: {
    imports = [
      yukimiModule
      (pkgs.path + "/nixos/tests/common/x11.nix")
    ];
    virtualisation = {
      memorySize = 3072;
      useBootLoader = true;
      useEFIBoot = true;
      resolution = {
        x = 1600;
        y = 1000;
      };
    };
    boot.loader.systemd-boot.enable = true;
    boot.loader.efi.canTouchEfiVariables = true;
    boot.loader.timeout = 1;
    system.switch.enable = true;
    nix.enable = true;
    # Follow the upstream systemd-boot fixture: keep a valid machine ID
    # through bootloader reboots, including for dbus-broker.
    environment.etc."machine-id".text = "1234567890abcdef1234567890abcdef\n";
    programs.yukimi = {
      enable = true;
      package = yukimi;
    };
    test-support.displayManager.auto.user = "demo";
    users.users.demo = {
      isNormalUser = true;
      uid = 1000;
    };
    environment.systemPackages = [ pkgs.xdotool ];
    environment.etc."icewm/winoptions".text = ".startMaximized: 1\n";
    specialisation.live.configuration.environment.etc."update-test".text = "live";
    specialisation.restart.configuration.boot.kernelParams = [ "sekka-update-test=1" ];
    specialisation.loginChange.configuration.systemd.services.display-manager.environment.YUKIMI_TEST = "changed";
    # Give the test a deterministic point at which activation is committed,
    # then keep writing output after its original client has been killed.
    system.activationScripts.yukimiTest = ''
      if test -e /run/test-slow-activation; then
        touch /run/test-activation-started
        sleep 3
        echo "activation survived its client"
      fi
    '';
  };
  testScript = ''
    import json, time
    machine.start()
    machine.wait_for_unit("multi-user.target")
    base = machine.succeed("readlink -f /run/current-system").strip()
    live = machine.succeed(f"readlink -f {base}/specialisation/live").strip()
    restart = machine.succeed(f"readlink -f {base}/specialisation/restart").strip()
    login_change = machine.succeed(f"readlink -f {base}/specialisation/loginChange").strip()
    def status():
        return json.loads(machine.succeed("sekka-update --status --json"))
    assert status()["action"] == "none", status()
    machine.wait_for_x()
    desktop_pid = machine.succeed("pgrep -u demo -x icewm").strip()
    # A target-side check protects even an older frontend which blindly
    # calls switch. It must fail before the display manager is stopped.
    machine.fail(f"{login_change}/bin/switch-to-configuration switch")
    assert machine.succeed("pgrep -u demo -x icewm").strip() == desktop_pid
    assert machine.succeed("readlink -f /run/current-system").strip() == base
    # Real helper, real systemd service, real activation; no build mocks.
    machine.succeed(f"ln -s {live} /nix/var/nix/profiles/system-100-link")
    machine.succeed("touch /run/test-slow-activation")
    machine.succeed("systemd-run --unit=test-updater-client --property=StandardOutput=journal --property=StandardError=journal yukimi-helper rollback 100")
    machine.wait_for_file("/run/test-activation-started")
    machine.succeed("systemctl kill --signal=SIGKILL test-updater-client.service")
    machine.wait_until_succeeds("grep -q '\"state\":\"activated\"' /var/lib/yukimi/update.json")
    machine.wait_until_succeeds("grep -l '\"ok\":true' /var/lib/yukimi/operations/*/result.json")
    machine.succeed("grep -l 'activation survived its client' /var/lib/yukimi/operations/*/progress.log")
    machine.succeed("rm /run/test-slow-activation")
    assert machine.succeed("readlink -f /run/current-system").strip() == live
    assert machine.succeed("pgrep -u demo -x icewm").strip() == desktop_pid
    assert status()["action"] == "none", status()
    machine.succeed(f"ln -s {restart} /nix/var/nix/profiles/system-101-link")
    report = json.loads(machine.succeed("yukimi-helper rollback 101"))
    assert report["ok"] and report["mode"] == "boot", report
    assert machine.succeed("readlink -f /run/current-system").strip() == live
    assert machine.succeed("pgrep -u demo -x icewm").strip() == desktop_pid
    assert status()["action"] == "reboot", status()
    # The status survives closing and reopening Yukimi; no background GUI.
    assert status()["action"] == "reboot", status()
    machine.wait_for_x()
    machine.succeed("su - demo -c 'DISPLAY=:0 GSK_RENDERER=cairo yukimi > /tmp/yukimi.log 2>&1 &' ")
    try:
        machine.wait_until_succeeds("pgrep -u demo -f yukimi", timeout=20)
    except Exception:
        print(machine.succeed("cat /tmp/yukimi.log"))
        raise
    time.sleep(4)
    machine.screenshot("prepared-update")
    machine.shutdown()
    machine.start()
    machine.wait_for_unit("multi-user.target")
    assert machine.succeed("readlink -f /run/booted-system").strip() == restart
    assert "sekka-update-test=1" in machine.succeed("cat /proc/cmdline")
    assert status()["action"] == "none", status()
    assert status()["action"] == "none", status()
    machine.screenshot("update-booted")
  '';
}
