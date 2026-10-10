# Real GTK captures in a disposable VM; no host desktop or disks are used.
{
  pkgs,
  genkan,
  yukimi,
  yukimiModule,
}:
let
  preview = genkan.overrideAttrs {
    cargoBuildType = "debug";
    doCheck = false;
  };
in
pkgs.testers.runNixOSTest {
  name = "sekka-app-screenshots";
  nodes.machine = { ... }: {
    imports = [
      (pkgs.path + "/nixos/tests/common/x11.nix")
      yukimiModule
    ];
    networking.hostName = "sekka-demo";
    virtualisation.memorySize = 4096;
    virtualisation.resolution = {
      x = 1920;
      y = 1080;
    };
    test-support.displayManager.auto.user = "demo";
    users.users.demo = {
      isNormalUser = true;
      uid = 1000;
    };
    programs.yukimi = {
      enable = true;
      package = yukimi;
    };
    environment.systemPackages = [
      preview
      pkgs.file
      pkgs.firefox
    ];
    environment.etc."icewm/winoptions".text = ".startMaximized: 1\n";
    environment.etc."nixos/configuration.nix".text = ''
      { pkgs, ... }: {
        networking.hostName = "sekka-demo";
        environment.systemPackages = [ pkgs.file pkgs.firefox ];
        programs.yukimi.enable = true;
      }
    '';
    fonts.packages = [ pkgs.noto-fonts ];
  };
  testScript = ''
    import time
    machine.start()
    try:
        machine.wait_for_x(timeout=60)
    except Exception:
        print(machine.succeed("journalctl -b --no-pager -n 120"))
        raise
    machine.succeed("su - demo -c 'DISPLAY=:0 CALAMARES_UI_PREVIEW=welcome CALAMARES_UI_SIZE=1600x960 GSK_RENDERER=cairo calamares-nixos > /tmp/genkan.log 2>&1 &' ")
    try:
        machine.wait_until_succeeds("pgrep -u demo -f calamares-nixos", timeout=20)
    except Exception:
        print(machine.succeed("cat /tmp/genkan.log"))
        raise
    time.sleep(8)
    machine.screenshot("genkan")
    machine.succeed("pkill -u demo -f calamares-nixos")
    machine.succeed("su - demo -c 'DISPLAY=:0 GSK_RENDERER=cairo yukimi > /tmp/yukimi.log 2>&1 &' ")
    try:
        machine.wait_until_succeeds("pgrep -u demo -f yukimi", timeout=20)
    except Exception:
        print(machine.succeed("cat /tmp/yukimi.log"))
        raise
    time.sleep(25)
    machine.screenshot("yukimi")
    print(machine.succeed("cat /tmp/genkan.log /tmp/yukimi.log"))
  '';
}
