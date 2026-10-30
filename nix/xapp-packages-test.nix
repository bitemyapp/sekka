# XApp's patched Settings portal (nix/overlays/xapp.nix) is the one selected
# wherever Sekka runs or ships Xfce, and no unpatched one is selected beside it.
{
  pkgs,
  inputs,
  applicationCatalog,
  patched,
  installer,
}:
let
  inherit (pkgs) lib;
  # The prebuilt systems on the media: Xfce's own, and every desktop together
  # for a choice of several.
  references = import ./reference-systems.nix {
    inherit inputs applicationCatalog;
    desktops = installer.config.respin.referenceDesktops;
  };
  installed = inputs.nixpkgs.lib.nixosSystem {
    system = pkgs.stdenv.hostPlatform.system;
    inherit (applicationCatalog) specialArgs;
    modules = [
      inputs.calamares.nixosModules.default
      { calamares.desktops = [ "xfce" ]; }
    ];
  };
  xfce = {
    installed = installed.config;
    iso-reference = references.systems.xfce.config;
    iso-together = references.together.config;
  };
  # Every boot entry of the live media, the software one included, runs Aizome
  # rather than Xfce, so none selects XApp's portal today; one that did would
  # still have to select the patched package.
  live = {
    default = installer.config;
  }
  // lib.mapAttrs (_: s: s.configuration) installer.config.specialisation;
  xappPortals =
    config:
    map (p: p.outPath) (
      builtins.filter (p: lib.getName p == "xdg-desktop-portal-xapp") config.xdg.portal.extraPortals
    );
  patchedOnly = config: lib.all (path: path == patched.outPath) (xappPortals config);
in
pkgs.writeText "sekka-xapp-packages.json" (
  builtins.toJSON {
    xfce = lib.mapAttrs (
      name: config:
      assert config.services.xserver.desktopManager.xfce.enable;
      assert builtins.elem patched.outPath (xappPortals config);
      assert patchedOnly config;
      patched.outPath
    ) xfce;
    live = lib.mapAttrs (
      name: config:
      assert patchedOnly config;
      xappPortals config
    ) live;
  }
)
