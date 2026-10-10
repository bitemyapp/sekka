# Resolve the catalog against maintained, locked application sources.
{ inputs }:
let
  system = "x86_64-linux";
  catalog = builtins.fromJSON (builtins.readFile "${inputs.calamares}/rust/src/applications.json");
  specialArgs = {
    applicationPkgs = import inputs.applications {
      inherit system;
      config.allowUnfree = true;
    };
    aiPackages = inputs.ai-apps.packages.${system};
    ompPackage = inputs.omp.packages.${system}.omp;
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
  packageList =
    app:
    if (app.source or "nixpkgs") == "omp" then
      [ specialArgs.ompPackage ]
    else
      map (
        name:
        (if (app.source or "nixpkgs") == "ai" then specialArgs.aiPackages else specialArgs.applicationPkgs)
        .${name}
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
