# Component regression: real GDM authentication and desktop sessions around an
# activation, with one lingering user manager. This does not qualify an ISO.
{ pkgs, desktopsModule }:
let
  node = original: { lib, ... }: {
    imports = [ desktopsModule ];
    calamares.desktops = [
      "tatami"
      "xfce"
      "gnome"
    ];
    calamares.defaultDesktop = "tatami";
    services.displayManager.defaultSession = lib.mkForce null;
    # This direct-kernel test VM has an unpartitioned scratch root. Keep real
    # switch activation, but only generate GRUB's menu on its synthetic disk.
    boot.loader.grub.devices = lib.mkForce [ "nodev" ];
    users.users.demo = {
      isNormalUser = true;
      uid = 1000;
      description = "Demo";
      password = "demo";
      linger = true;
    };
    # A manager-level customization must survive the reset; it is not a
    # session export and is absent from the display manager's own environment.
    systemd.user.settings.Manager.DefaultEnvironment = "SEKKA_BASELINE_CUSTOM=before=graphical";
    environment.sessionVariables.LIBGL_ALWAYS_SOFTWARE = "1";
    environment.systemPackages = [ pkgs.python3 ];
    system.activationScripts.sessionTest.text = ''
      mkdir -p /var/lib/AccountsService/users
      if [ ! -e /var/lib/AccountsService/users/demo ]; then
        cat > /var/lib/AccountsService/users/demo <<'USER'
      [User]
      Session=tatami
      XSession=tatami
      SessionType=wayland
      SystemAccount=false
      USER
      fi
    '';
    # Exercise a changed unit definition, as well as an unchanged activation
    # and rollback. The baseline still belongs to the same manager lifetime.
    specialisation.updated.configuration.systemd.user.services.calamares-session-env.description =
      lib.mkForce "Updated environment snapshot service description";
    systemd.user.services.calamares-session-env = lib.mkIf original {
      restartIfChanged = lib.mkForce true;
      serviceConfig.RemainAfterExit = lib.mkForce false;
    };
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
  name = "sekka-session-environment";
  globalTimeout = 1200;
  enableOCR = true;
  node.pkgsReadOnly = false;
  nodes.original = node true;
  nodes.fixed = node false;
  testScript =
    builtins.replaceStrings
      [ "@tesseract@" "@magick@" ]
      [ "${pkgs.tesseract}/bin/tesseract" "${pkgs.imagemagick}/bin/magick" ]
      (builtins.readFile ../tests/session-environment.py);
}
