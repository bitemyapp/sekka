# Free installed closures published to Sekka's binary cache, for one platform.
{
  inputs,
  system,
  applicationCatalog,
  desktops,
}:
let
  pkgs = inputs.nixpkgs.legacyPackages.${system};
  reference =
    desktops: applications:
    import ./reference-systems.nix {
      inherit
        inputs
        system
        applicationCatalog
        desktops
        applications
        ;
      free = true;
      cacheApplications = false;
    };
  desktopSystems = reference desktops [ "firefox" ];
  applications = reference [ "plasma" ] (
    map (app: app.id) (builtins.filter (app: !(app.unfree or false)) applicationCatalog.catalog)
  );
in
pkgs.linkFarm "sekka-installed-systems" (
  map (desktop: {
    name = desktop;
    path = desktopSystems.systems.${desktop}.config.system.build.toplevel;
  }) desktopSystems.desktops
  ++ [
    {
      name = "applications";
      path = applications.systems.plasma.config.system.build.toplevel;
    }
  ]
)
