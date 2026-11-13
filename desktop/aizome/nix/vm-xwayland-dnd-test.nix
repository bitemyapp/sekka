# Drag and drop between X11 and Wayland windows through the session's
# xwayland-satellite: a file dragged from an X11 window to another X11
# window, from a Wayland window to an X11 one and from an X11 window to a
# Wayland one reaches the target, with the windows placed where the X
# screen's positions don't match Aizome's (vm-xwayland-dnd-test.py).
#
#     nix build .#aizome-checks.xwayland-dnd -L
{
  pkgs,
  self,
  package ? null,
}:
pkgs.testers.runNixOSTest {
  name = "aizome-xwayland-dnd";
  globalTimeout = 600;
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
    # A GTK 3 drag source and drop target, run as an X11 or a Wayland
    # client by GDK_BACKEND.
    environment.systemPackages = [ pkgs.dragon-drop ];
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
  testScript = builtins.readFile ./vm-xwayland-dnd-test.py;
}
