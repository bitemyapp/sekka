# Instrumented component test, separate from exact ISO qualification.
{ pkgs, inputs }:
let
  gtkProbe = pkgs.stdenv.mkDerivation {
    name = "ibus-gtk-probe";
    dontUnpack = true;
    nativeBuildInputs = [
      pkgs.pkg-config
      pkgs.wrapGAppsHook3
    ];
    buildInputs = [ pkgs.gtk3 ];
    buildPhase = ''
      $CC -Wall -Wextra -Werror ${../tests/ibus-gtk.c} $(pkg-config --cflags --libs gtk+-3.0) -o ibus-gtk-probe
    '';
    installPhase = "install -Dm755 ibus-gtk-probe $out/bin/ibus-gtk-probe";
  };
in
pkgs.testers.runNixOSTest {
  name = "sekka-aizome-ibus";
  globalTimeout = 900;
  node.pkgsReadOnly = false;
  nodes.machine = { lib, ... }: {
    imports = [ inputs.calamares.nixosModules.desktops ];
    calamares.desktops = [
      "aizome"
      "gnome"
      "xfce"
    ];
    calamares.defaultDesktop = "aizome";
    users.users.alice = {
      isNormalUser = true;
      uid = 1000;
      password = "test-only";
      linger = true;
    };
    services.displayManager.autoLogin = {
      enable = true;
      user = "alice";
    };
    # Let AccountsService retain the test's selected session between real
    # GDM logins. Sekka normally resets the chooser to the installer default.
    services.displayManager.defaultSession = lib.mkForce null;
    system.activationScripts.ibusTestSession.text = ''
      mkdir -p /var/lib/AccountsService/users
      if [ ! -e /var/lib/AccountsService/users/alice ]; then
        cat > /var/lib/AccountsService/users/alice <<'EOF'
      [User]
      Session=aizome
      XSession=aizome
      SessionType=wayland
      SystemAccount=false
      EOF
      fi
    '';
    i18n.inputMethod.ibus.engines = [ pkgs.ibus-engines.hangul ];
    environment.systemPackages = [
      gtkProbe
      pkgs.wl-clipboard
      pkgs.xdotool
    ];
    virtualisation = {
      memorySize = 6144;
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
  testScript = builtins.readFile ../tests/aizome-ibus.py;
}
