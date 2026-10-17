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
  # As installer/genkan/rust/src/applications.nix resolves them; the module
  # gets Sekka's own packages from nix/components.nix.
  sources = {
    ai = specialArgs.aiPackages;
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
  # What the installation media carries: not applications marked
  # `"media": false`, whose licences forbid redistributing them. Preparing
  # an installation downloads those from their makers.
  onMedia = builtins.filter (app: app.media or true) catalog;
  offMedia = builtins.filter (app: !(app.media or true)) catalog;
  packageList =
    app:
    if (app.source or "nixpkgs") == "omp" then
      [ specialArgs.ompPackage ]
    else
      map (
        name: (sources.${app.source or "nixpkgs"} or specialArgs.applicationPkgs).${name}
      ) app.packages;
in
{
  inherit
    catalog
    onMedia
    specialArgs
    evaluate
    packageList
    ;
  cache = (evaluate (map (app: app.id) onMedia)).config.system.build.installerApplications;
  # The generated per-choice matrix runs in separate evaluator processes.
  # Retaining all those complete NixOS systems here exhausts small builders.
  check =
    let
      paths = apps: map (p: p.outPath) (builtins.concatMap packageList apps);
      onMediaPaths = paths onMedia;
    in
    # No application on the media brings the package of one kept off it.
    assert builtins.all (path: !(builtins.elem path onMediaPaths)) (paths offMedia);
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
