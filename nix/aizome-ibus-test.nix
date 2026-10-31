# Instrumented component test, separate from exact ISO qualification.
{ pkgs, inputs }:
let
  chromeProbe =
    browserPkgs:
    browserPkgs.writeShellScriptBin "ibus-chrome-probe" ''
      exec ${browserPkgs.python3.withPackages (ps: [ ps.websockets ])}/bin/python3 \
        ${../tests/ibus-chrome.py} --chrome ${browserPkgs.google-chrome}/bin/google-chrome-stable "$@"
    '';
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
  nodes.machine = { lib, pkgs, ... }: {
    imports = [ inputs.calamares.nixosModules.desktops ];
    # Exercise the affected browser itself, only in this disposable VM.
    nixpkgs.config.allowUnfreePredicate = pkg: lib.getName pkg == "google-chrome";
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
    # A daemon PID can appear while its unit is still activating. Exercise
    # that readiness window deterministically, including under a quiet runner.
    systemd.user.services."org.freedesktop.IBus.session.GNOME" = {
      overrideStrategy = "asDropin";
      serviceConfig.ExecStartPost = "${pkgs.coreutils}/bin/sleep 3";
    };
    environment.systemPackages = [
      (chromeProbe pkgs)
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
