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
  };
  testScript = ''
    import json, shlex, time
    machine.start()
    machine.wait_for_unit("multi-user.target")
    base = machine.succeed("readlink -f /run/current-system").strip()
    live = machine.succeed(f"readlink -f {base}/specialisation/live").strip()
    restart = machine.succeed(f"readlink -f {base}/specialisation/restart").strip()
    def status():
        return json.loads(machine.succeed("sekka-update --status --json"))
    assert status()["action"] == "none", status()
    machine.succeed(f"nix-env -p /nix/var/nix/profiles/system --set {live}")
    machine.succeed(f"{live}/bin/switch-to-configuration switch")
    assert status()["action"] == "none", status()
    machine.succeed(f"nix-env -p /nix/var/nix/profiles/system --set {restart}")
    machine.succeed(f"{restart}/bin/switch-to-configuration boot")
    receipt = {"system": restart, "boot_id": machine.succeed("cat /proc/sys/kernel/random/boot_id").strip(), "state": "prepared", "reasons": ["Kernel boot settings changed."]}
    machine.succeed("mkdir -p /var/lib/yukimi")
    machine.succeed("printf %s " + shlex.quote(json.dumps(receipt)) + " > /var/lib/yukimi/update.json")
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
