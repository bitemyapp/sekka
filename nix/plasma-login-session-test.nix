# Real Plasma Login password sessions. No ISO qualification or hardware claim.
{
  pkgs,
  desktopsModule,
}:
let
  node = original: { lib, ... }: {
    imports = [ desktopsModule ];
    calamares.desktops = [
      "xfce"
      "plasma"
    ];
    calamares.defaultDesktop = "xfce";
    # An ordinary custom package definition must still override Sekka's
    # default. The unmodified package is also the runtime negative control.
    services.displayManager.plasma-login-manager.package = lib.mkIf original pkgs.kdePackages.plasma-login-manager;
    users.users.demo = {
      isNormalUser = true;
      uid = 1000;
      description = "Demo";
      password = "demo";
      linger = true;
    };
    systemd.user.settings.Manager.DefaultEnvironment = "SEKKA_SESSION_CUSTOM=preserved";
    environment.sessionVariables.LIBGL_ALWAYS_SOFTWARE = "1";
    virtualisation = {
      memorySize = 6144;
      cores = 4;
      qemu.options = [
        "-vga none"
        "-device virtio-gpu-pci,xres=1280,yres=800"
      ];
    };
  };
in
pkgs.testers.runNixOSTest {
  name = "sekka-plasma-login-session";
  globalTimeout = 900;
  enableOCR = true;
  node.pkgsReadOnly = false;
  nodes.original = node true;
  nodes.fixed = node false;
  testScript =
    builtins.replaceStrings
      [ "@tesseract@" "@magick@" ]
      [ "${pkgs.tesseract}/bin/tesseract" "${pkgs.imagemagick}/bin/magick" ]
      (builtins.readFile ../tests/plasma-login-session.py);
}
