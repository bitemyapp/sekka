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
  # Allow only the unfree packages whose licences let anyone redistribute
  # them, as the binary cache must (nix/push-cache.nix). Given to the
  # reference configuration itself: Nixpkgs merges `nixpkgs.config` its own
  # way, and a `mkForce` beside a plain `true` there overrides nothing.
  redistributable ? false,
  # Genkan's kernel choice: "latest", or "lts" for Nixpkgs' default.
  kernel ? "latest",
  # Genkan's NVIDIA choice: the driver, built for the kernel.
  nvidia ? false,
}:
let
  system = "x86_64-linux";
  inherit (inputs.nixpkgs) lib;
  pkgs = inputs.nixpkgs.legacyPackages.${system};
  redistributes = import ./redistributable.nix { inherit lib; };
  # The catalog's applications come from their own Nixpkgs, which allows
  # every unfree package. For the binary cache, one that allows only those
  # anyone may redistribute: an application whose own licence passes but
  # which pulls in one that doesn't fails to evaluate rather than being
  # published.
  specialArgs =
    applicationCatalog.specialArgs
    // lib.optionalAttrs redistributable {
      applicationPkgs = import inputs.applications {
        inherit system;
        config.allowUnfreePredicate = redistributes;
      };
    };
  evaluate =
    desktops:
    lib.nixosSystem {
      inherit system specialArgs;
      modules = [
        inputs.determinate.nixosModules.default
        # The same locked modules installed systems import.
        (import ../installer/genkan/rust/reference.nix {
          inherit desktops applications kernel;
          unfree = if redistributable then redistributes else true;
        })
        # templates/flake.nix.in adds FlakeHub's CLI to every installed
        # system beside Sekka's modules; without it here, the binary cache
        # never had it and each update that moved `fh` compiled it.
        (
          { pkgs, ... }:
          {
            environment.systemPackages = [ inputs.fh.packages.${pkgs.stdenv.hostPlatform.system}.default ];
          }
        )
        # Defined only when asked, so `extendModules` can still turn it on
        # (checks.binary-cache does).
        { calamares.nvidia.enable = lib.mkIf nvidia true; }
        inputs.tatami.nixosModules.default
        inputs.yukimi.nixosModules.default
        inputs.aizome.nixosModules.default
        # As nix/components.nix gives the catalog module Sekka's packages.
        { _module.args.sekkaPackages = inputs.catalog.packages.${system}; }
      ];
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
