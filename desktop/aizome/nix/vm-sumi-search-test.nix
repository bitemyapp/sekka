# Packaged Sumi search highlight repaint; a component test, not an ISO gate.
{
  pkgs,
  self,
  package ? null,
}:
pkgs.testers.runNixOSTest {
  name = "aizome-sumi-search";
  globalTimeout = 400;
  extraPythonPackages = p: [ p.pillow ];
  nodes.machine = { lib, ... }: {
    imports = [ self.nixosModules.default ];
    programs.aizome.enable = true;
    programs.aizome.package = lib.mkIf (package != null) package;
    users.users.alice = {
      isNormalUser = true;
      uid = 1000;
      password = "test-only";
    };
    environment.etc."sumi-search.txt".text =
      "cat Cat caterpillar cat1 cat2\nnumber:12 number:34\nplain line\n";
    services.greetd = {
      enable = true;
      settings.initial_session = {
        user = "alice";
        command = "/run/current-system/sw/bin/aizome-session";
      };
      settings.default_session.command = "${pkgs.greetd}/bin/agreety --cmd /run/current-system/sw/bin/aizome-session";
    };
    systemd.user.settings.Manager.DefaultEnvironment = "AIZOME_KIT_INSPECT=/run/user/1000/sumi-inspect";
    systemd.user.tmpfiles.rules = [ "d %t/sumi-inspect 0700 - - -" ];
    virtualisation = {
      memorySize = 4096;
      cores = 4;
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
  testScript = builtins.readFile ./vm-sumi-search-test.py;
}
