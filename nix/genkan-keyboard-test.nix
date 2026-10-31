# Genkan's keyboard step, and what the installed system makes of it, in a
# disposable Aizome VM.
#
# The VM is a Sekka system built from the modules installed systems import
# (desktops and applications), with Aizome chosen and what Genkan writes
# after the keyboard step: Left Alt and Super swapped, so ❖ is QEMU's Alt, just
# left of the space bar. The test checks that
#
# - Aizome's session starts from that system keyboard, so QEMU's Alt and A
#   select all in Genkan's text fields (Genkan takes ❖ as Aizome's apps do,
#   rather than as Ctrl), and so does QEMU's ❖ once the person's own
#   settings clear the options (an empty list must not bring the session's
#   XKB_DEFAULT_OPTIONS back), as in the live session;
# - Genkan's keyboard step (its debug build's sample pages) hears QEMU's
#   keys through the compositor: the space bar asks again, Alt suggests
#   swapping left Alt and Super, which can be declined, Esc stops waiting, and
#   Control is left as it is, saying Genkan swaps only Alt and Super;
# - Hotaru and foot open fish, which no application choice brought.
#
# Its display is 2880 × 1800 at scale 2, drawn in software (pixman); the
# captures crop Genkan's window from it pixel for pixel.
#
#     nix build .#checks.x86_64-linux.genkan-keyboard -L
{
  pkgs,
  genkan,
  sekkaModule,
}:
let
  # Preview pages exist only in debug builds.
  preview = genkan.overrideAttrs {
    cargoBuildType = "debug";
    doCheck = false;
  };
in
pkgs.testers.runNixOSTest {
  name = "genkan-keyboard";
  globalTimeout = 1800;
  node.pkgsReadOnly = false;
  nodes.aizome =
    { pkgs, ... }:
    {
      imports = [ sekkaModule ];
      # What the installed flake passes the application catalog.
      _module.args = {
        applicationPkgs = pkgs;
        aiPackages = { };
        ompPackage = null;
      };
      calamares.desktops = [ "aizome" ];
      calamares.defaultDesktop = "aizome";
      # No applications: fish has to come with Aizome.
      calamares.applications = [ ];
      calamares.installUser = "demo";
      # What Genkan writes when the key left of the space bar is Alt and
      # the person keeps its suggestion.
      services.xserver.xkb.layout = "us";
      services.xserver.xkb.options = "altwin:swap_lalt_lwin";
      console.useXkbConfig = true;
      networking.hostName = "sekka-keyboard";
      users.users.demo = {
        isNormalUser = true;
        uid = 1000;
        description = "Demo";
        password = "demo";
      };
      services.displayManager.autoLogin = {
        enable = true;
        user = "demo";
      };
      virtualisation = {
        memorySize = 6144;
        cores = 4;
        qemu.options = [
          "-vga none"
          "-device virtio-gpu-pci,xres=2880,yres=1800"
        ];
      };
      hardware.graphics.enable = true;
    };
  testScript = ''
    PREVIEW = "${preview}/bin/calamares-nixos"
    MAGICK = "${pkgs.imagemagick}/bin/magick"
    ${builtins.readFile ./genkan-keyboard-test.py}
  '';
}
