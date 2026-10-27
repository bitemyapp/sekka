# Manual component regression against the real HTTPS service. Deliberately not
# part of flake checks: it needs external network access and a working Open-Meteo.
# Build .driver and run it outside the Nix sandbox, with private XDG_RUNTIME_DIR
# and TMPDIR directories. All input is normal keyboard/pointer input.
{
  pkgs,
  self,
  package ? null,
}:
pkgs.testers.runNixOSTest {
  name = "aizome-tenki-live-search";
  globalTimeout = 300;
  nodes.machine = { lib, ... }: {
    imports = [ self.nixosModules.default ];
    programs.aizome.enable = true;
    programs.aizome.package = lib.mkIf (package != null) package;
    time.timeZone = "America/Chicago";
    boot.kernelModules = [ "sch_netem" ];
    users.users.alice = {
      isNormalUser = true;
      uid = 1000;
      password = "test-only";
    };
    services.greetd = {
      enable = true;
      settings.initial_session = {
        user = "alice";
        command = "/run/current-system/sw/bin/aizome-session";
      };
      settings.default_session.command = "${pkgs.greetd}/bin/agreety --cmd /run/current-system/sw/bin/aizome-session";
    };
    environment.systemPackages = [
      pkgs.curl
      pkgs.python3
      pkgs.iproute2
    ];
    virtualisation = {
      memorySize = 4096;
      cores = 4;
      restrictNetwork = false;
      resolution = {
        x = 1280;
        y = 800;
      };
      qemu.options = [
        "-vga none"
        "-device virtio-gpu-pci,xres=1280,yres=800"
      ];
    };
  };
  testScript = builtins.readFile ./vm-tenki-live-search-test.py;
}
