# Real packaged Tansu input, UDisks2 and ext4 on one disposable VM disk.
{ self, pkgs, package ? self.packages.${pkgs.stdenv.hostPlatform.system}.aizome, expectFailure ? false }:
pkgs.testers.runNixOSTest {
  name = "aizome-tansu-resize${if expectFailure then "-negative" else ""}";
  globalTimeout = 300;
  nodes.machine = { lib, ... }: {
    imports = [ self.nixosModules.default ];
    programs.aizome.enable = true;
    programs.aizome.package = lib.mkForce package;
    users.users.alice = { isNormalUser = true; uid = 1000; password = "test-only"; extraGroups = [ "wheel" ]; };
    services.greetd = {
      enable = true;
      settings.initial_session = { user = "alice"; command = "/run/current-system/sw/bin/aizome-session"; };
      settings.default_session.command = "${pkgs.greetd}/bin/agreety --cmd /run/current-system/sw/bin/aizome-session";
    };
    environment.systemPackages = [ pkgs.e2fsprogs ];
    environment.etc."aizome-tansu-resize-test".text = "disposable-resize-test\n";
    systemd.user.settings.Manager.DefaultEnvironment = "AIZOME_KIT_INSPECT=/run/user/1000/resize-inspect";
    systemd.user.tmpfiles.rules = [ "d %t/resize-inspect 0700 - - -" ];
    virtualisation = {
      memorySize = 4096;
      cores = 4;
      emptyDiskImages = [ 2048 ];
      resolution = { x = 1280; y = 800; };
      qemu.options = [ "-vga none" "-device virtio-gpu-pci,xres=1280,yres=800" ];
    };
  };
  testScript = "expect_failure = ${if expectFailure then "True" else "False"}\n" + builtins.readFile ./vm-tansu-resize-test.py;
}
