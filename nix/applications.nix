# Resolve the catalog against maintained, locked application sources.
{ inputs }:
let
  inherit (inputs.nixpkgs) lib;
  system = "x86_64-linux";
  # Genkan's source as a path: reading through it needs no store copy (see
  # nix/calamares.nix).
  genkan = inputs.calamares.outPath;
  catalog = builtins.fromJSON (builtins.readFile (genkan + "/rust/src/applications.json"));
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
  # No application chosen; uta-installation checks the same system.
  empty = evaluate [ ];
  # Catalog visibility and offline media contents are separate policies.
  # Download-only apps remain selectable and eligible for the binary cache.
  preloaded = builtins.filter (app: app.preload or true) catalog;
  # Every preloaded app installed: the ISO's offline application cache.
  preloadedSystem = evaluate (map (app: app.id) preloaded);
  withKeyring = import (genkan + "/rust/src/keyring.nix") {
    inherit lib;
    pkgs = specialArgs.applicationPkgs;
  };
  # Steam is the one `programs.steam` builds, as the module installs it
  # (`fromPrograms` there). It follows the system it is built for: the
  # media carry the preloaded system's (`cache` below), so that one is
  # listed.
  fromPrograms = lib.optionalAttrs (builtins.any (app: app.id == "steam") preloaded) {
    steam = preloadedSystem.config.programs.steam.package;
  };
  packageList =
    app:
    if (app.source or "nixpkgs") == "omp" then
      [ specialArgs.ompPackage ]
    else
      map (
        name:
        withKeyring app (
          if (app.source or "nixpkgs") == "nixpkgs" && fromPrograms ? ${name} then
            fromPrograms.${name}
          else
            (sources.${app.source or "nixpkgs"} or specialArgs.applicationPkgs).${name}
        )
      ) app.packages;
  # What the system the media carry installs.
  carried = map toString (
    builtins.filter lib.isDerivation preloadedSystem.config.environment.systemPackages
  );
  # The Steams among a system's packages.
  steams =
    config:
    lib.unique (
      map toString (
        builtins.filter (
          package: lib.isDerivation package && lib.getName package == "steam"
        ) config.environment.systemPackages
      )
    );
in
{
  inherit
    catalog
    specialArgs
    evaluate
    packageList
    preloaded
    preloadedSystem
    empty
    ;
  cache = preloadedSystem.config.system.build.installerApplications;
  # The generated per-choice matrix runs in separate evaluator processes.
  # Retaining all those complete NixOS systems here exhausts small builders.
  check =
    # A system has one Steam, the one `programs.steam` builds. The catalog's
    # plain package beside it wins every file: the module's is then built on
    # each machine and linked nowhere, so the binary cache never has it.
    assert steams all.config == [ (toString all.config.programs.steam.package) ];
    # The media list each preloaded application's packages as the system
    # they carry installs them.
    assert lib.all (package: builtins.elem (toString package) carried) (
      lib.concatMap packageList preloaded
    );
    builtins.deepSeq
      (map (c: c.config.system.build.toplevel.drvPath) ([
        all
        empty
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
