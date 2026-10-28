# Native Yukimi in a disposable Tatami VM: navigation, search, clipboard,
# scaling, and cancelling both an install confirmation and authentication.
# Nothing is installed; no host desktop or disks are used.
#
# The VM is a Tatami system built like an installed Sekka system: Genkan's
# desktop and application modules (so Discover offers Genkan's catalog,
# installed through calamares.applications), unfree software allowed, and its
# Nixpkgs recorded for Yukimi, which makes its own package list from it as it
# would on a new installation. The display is the screenshots' 2560 × 1600 at
# scale 2, followed by fractional and smaller-display checks.
{
  pkgs,
  nixpkgs,
  desktopsModule,
  applicationsModule,
}:
let
  width = 2560;
  height = 1600;
in
pkgs.testers.runNixOSTest {
  name = "yukimi-discover";
  globalTimeout = 2400;
  # Sekka's desktop module adds its Aquamarine fix as a Nixpkgs overlay.
  node.pkgsReadOnly = false;
  nodes.tatami =
    { pkgs, ... }:
    {
      imports = [
        desktopsModule
        applicationsModule
      ];
      # What the installed flake passes; only Firefox is chosen here.
      _module.args = {
        applicationPkgs = pkgs;
        aiPackages = { };
        ompPackage = null;
      };
      calamares.desktops = [ "tatami" ];
      calamares.defaultDesktop = "tatami";
      calamares.applications = [ "firefox" ];
      calamares.installUser = "demo";
      networking.hostName = "sekka-demo";
      # As on installed systems: a flake's Nixpkgs, which Yukimi searches.
      nixpkgs.flake.source = nixpkgs.outPath;
      nixpkgs.config.allowUnfree = true;
      users.users.demo = {
        isNormalUser = true;
        uid = 1000;
        description = "Demo";
        password = "demo";
        extraGroups = [ "wheel" ];
      };
      services.displayManager.autoLogin = {
        enable = true;
        user = "demo";
      };
      # The pointer: Hyprland moves it, ydotool clicks.
      programs.ydotool.enable = true;
      virtualisation = {
        # Making the package list evaluates all of Nixpkgs.
        memorySize = 12288;
        cores = 4;
        qemu.options = [
          "-vga none"
          "-device virtio-gpu-pci,xres=${toString width},yres=${toString height}"
        ];
      };
      environment.systemPackages = [ pkgs.python3 ];
      environment.etc."yukimi-measure.py".source = ../tests/measure_yukimi.py;
      hardware.graphics.enable = true;
      environment.sessionVariables.LIBGL_ALWAYS_SOFTWARE = "1";
      # What Yukimi reads: the installer's applications and unfree software.
      environment.etc."nixos/configuration.nix".text = ''
        { pkgs, ... }: {
          networking.hostName = "sekka-demo";
          nixpkgs.config.allowUnfree = true;
          calamares.applications = [ "firefox" ];
        }
      '';
    };
  testScript = ''
    NIXPKGS_NAME = "${builtins.baseNameOf nixpkgs.outPath}"
    TESSERACT = "${pkgs.tesseract}/bin/tesseract"
    ${builtins.readFile ../tests/yukimi-tatami.py}
  '';
}
