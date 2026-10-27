# Real Fusen save failures, protected close and retry; component evidence only.
{
  pkgs,
  self,
  package ? null,
}:
pkgs.testers.runNixOSTest {
  name = "aizome-fusen-save";
  globalTimeout = 400;
  nodes.machine = { lib, ... }: {
    imports = [ self.nixosModules.default ];
    programs.aizome.enable = true;
    programs.aizome.package = lib.mkIf (package != null) package;
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
    systemd.user.settings.Manager.DefaultEnvironment =
      "AIZOME_KIT_INSPECT=/run/user/1000/fusen-inspect";
    systemd.user.tmpfiles.rules = [ "d %t/fusen-inspect 0700 - - -" ];
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
  testScript = builtins.readFile ./vm-fusen-save-test.py;
}
