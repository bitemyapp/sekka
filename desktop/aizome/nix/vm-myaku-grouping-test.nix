# Real multi-app grouping and process controls; component evidence, not ISO qualification.
{
  pkgs,
  self,
  package ? null,
}:
pkgs.testers.runNixOSTest {
  name = "aizome-myaku-grouping";
  globalTimeout = 400;
  nodes.machine = { lib, ... }: {
    imports = [ self.nixosModules.default ];
    programs.aizome.enable = true;
    programs.aizome.package = lib.mkIf (package != null) package;
    environment.systemPackages = [ pkgs.firefox ];
    users.users.alice = {
      isNormalUser = true;
      uid = 1000;
      password = "test-only";
    };
    systemd.tmpfiles.rules = [
      "f /home/alice/Grouping.txt 0644 alice users - Independent editor document"
    ];
    services.greetd = {
      enable = true;
      settings.initial_session = {
        user = "alice";
        command = "/run/current-system/sw/bin/aizome-session";
      };
      settings.default_session.command = "${pkgs.greetd}/bin/agreety --cmd /run/current-system/sw/bin/aizome-session";
    };
    systemd.user.settings.Manager.DefaultEnvironment =
      "AIZOME_KIT_INSPECT=/run/user/1000/myaku-inspect";
    systemd.user.tmpfiles.rules = [ "d %t/myaku-inspect 0700 - - -" ];
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
  testScript = builtins.readFile ./vm-myaku-grouping-test.py;
}
