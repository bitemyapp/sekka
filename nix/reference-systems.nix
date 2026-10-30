# Prebuilt installed systems, one per desktop choice and one with all of them
# together, from the installer's own reference configuration. Their closures
# go on the installation media, so an installation copies packages instead of
# downloading them, and the single desktops' store path lists let the
# installer warm the page cache for the user's selection while the user is
# still choosing.
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
    desktops:
    lib.nixosSystem {
      inherit system;
      inherit (applicationCatalog) specialArgs;
      modules = [
        inputs.determinate.nixosModules.default
        # The same locked modules installed systems import.
        (import ../installer/genkan/rust/reference.nix {
          inherit desktops applications;
        })
        inputs.tatami.nixosModules.default
        inputs.yukimi.nixosModules.default
        inputs.aizome.nixosModules.default
        # As nix/components.nix gives the catalog module Sekka's packages.
        { _module.args.sekkaPackages = inputs.catalog.packages.${system}; }
      ]
      ++ lib.optional free { nixpkgs.config.allowUnfree = lib.mkForce false; };
    };
  systems = lib.genAttrs desktops (desktop: evaluate [ desktop ]);
  # Choosing several desktops brings packages no single desktop has, which an
  # installation would otherwise compile: calamares-session-env, and with
  # Aizome and GNOME, Aizome's patched IBus. One system with every desktop
  # carries them.
  together = if lib.length desktops > 1 then evaluate desktops else null;
  paths = roots: "${pkgs.closureInfo { rootPaths = roots; }}/store-paths";
in
{
  inherit desktops systems together;
  toplevels =
    map (desktop: systems.${desktop}.config.system.build.toplevel) desktops
    ++ lib.optional (together != null) together.config.system.build.toplevel;
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
    }) applicationCatalog.preloaded;
}
