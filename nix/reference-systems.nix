# Prebuilt installed systems, one per desktop choice, from the installer's own
# reference configuration. Their closures go on the installation media, so an
# installation copies packages instead of downloading them, and their store
# path lists let the installer warm the page cache for the user's selection
# while the user is still choosing.
{
  inputs,
  applicationCatalog,
  desktops,
  # The catalog's applications each system has.
  applications ? [ "firefox" ],
  # Refuse unfree packages, as the binary cache must (nix/push-cache.nix).
  free ? false,
}:
let
  system = "x86_64-linux";
  inherit (inputs.nixpkgs) lib;
  pkgs = inputs.nixpkgs.legacyPackages.${system};
  evaluate =
    desktop:
    lib.nixosSystem {
      inherit system;
      inherit (applicationCatalog) specialArgs;
      modules = [
        inputs.determinate.nixosModules.default
        # The same locked modules installed systems import.
        (import ../installer/genkan/rust/reference.nix {
          desktops = [ desktop ];
          inherit applications;
        })
        inputs.tatami.nixosModules.default
        inputs.yukimi.nixosModules.default
        inputs.aizome.nixosModules.default
        # As nix/components.nix gives the catalog module Sekka's packages.
        { _module.args.sekkaPackages = inputs.catalog.packages.${system}; }
      ]
      ++ lib.optional free { nixpkgs.config.allowUnfree = lib.mkForce false; };
    };
  systems = lib.genAttrs desktops evaluate;
  paths = roots: "${pkgs.closureInfo { rootPaths = roots; }}/store-paths";
in
{
  inherit desktops systems;
  toplevels = map (desktop: systems.${desktop}.config.system.build.toplevel) desktops;
  # Plain files on the ISO 9660 filesystem, /iso/calamares/closures/
  # {desktop,application}-<id>.paths. Lists inside the Nix store would make
  # every listed path a runtime reference of the live system.
  lists =
    map (desktop: {
      source = paths [ systems.${desktop}.config.system.build.toplevel ];
      target = "/calamares/closures/desktop-${desktop}.paths";
    }) desktops
    ++ map (app: {
      source = paths (applicationCatalog.packageList app);
      target = "/calamares/closures/application-${app.id}.paths";
    }) applicationCatalog.catalog;
}
