# Resolve the catalog against maintained, locked application sources.
{
  inputs,
  system ? "x86_64-linux",
}:
let
  # Genkan's source as a path: reading through it needs no store copy (see
  # nix/calamares.nix).
  genkan = inputs.calamares.outPath;
  catalog = import (genkan + "/rust/src/application-catalog.nix") { inherit system; };
  specialArgs = {
    applicationPkgs = import inputs.applications {
      inherit system;
      config.allowUnfree = true;
    };
    aiPackages = inputs.ai-apps.packages.${system};
    ompPackage = inputs.omp.packages.${system}.omp;
  };
  # As installer/genkan/rust/src/applications.nix resolves them; the module
  # gets Sekka's own packages from nix/components.nix.
  sources = {
    ai = import ./ai-packages.nix specialArgs.aiPackages;
    sekka = inputs.catalog.packages.${system};
  };
  evaluate =
    ids:
    inputs.nixpkgs.lib.nixosSystem {
      inherit system specialArgs;
      modules = [
        inputs.calamares.nixosModules.applications
        {
          calamares.applications = ids;
          calamares.installUser = "catalog-test";
          nixpkgs.config.allowUnfree = true;
          users.users.catalog-test.isNormalUser = true;
          fileSystems."/" = {
            device = "/dev/disk/by-label/test";
            fsType = "ext4";
          };
          boot.loader.grub.enable = false;
          system.stateVersion = "26.11";
        }
      ];
    };
  all = evaluate (map (app: app.id) catalog);
  withKeyring = import (genkan + "/rust/src/keyring.nix") {
    inherit (inputs.nixpkgs) lib;
    pkgs = specialArgs.applicationPkgs;
  };
  packageList =
    app:
    if (app.source or "nixpkgs") == "omp" then
      [ specialArgs.ompPackage ]
    else
      map (
        name: withKeyring app (sources.${app.source or "nixpkgs"} or specialArgs.applicationPkgs).${name}
      ) app.packages;
in
{
  inherit
    catalog
    specialArgs
    evaluate
    packageList
    ;
  cache = all.config.system.build.installerApplications;
  # The generated per-choice matrix runs in separate evaluator processes.
  # Retaining all those complete NixOS systems here exhausts small builders.
  check =
    builtins.deepSeq
      (map (c: c.config.system.build.toplevel.drvPath) ([
        all
        (evaluate [ ])
      ]))
      (
        all.pkgs.writeText "application-catalog.json" (
          builtins.toJSON (
            map (app: {
              inherit (app) id name;
              source = app.source or "nixpkgs";
              packages = map (p: {
                inherit (p) name;
                version = p.version or null;
                path = p.outPath;
              }) (packageList app);
            }) catalog
          )
        )
      );
}
