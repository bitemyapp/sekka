# First-party source is always taken from this checkout.
{ nixpkgs }:
let
  system = "x86_64-linux";
  pkgs = nixpkgs.legacyPackages.${system};
in
rec {
  tatami = {
    outPath = ../desktop/tatami;
    nixosModules.default = ../desktop/tatami/module.nix;
    packages.${system} = {
      tatami = pkgs.callPackage ../desktop/tatami/tool/package.nix { };
      displays = pkgs.callPackage ../desktop/tatami/displays/package.nix { };
      wallpapers = pkgs.callPackage ../desktop/tatami/wallpapers/package.nix { };
    };
  };
  yukimi = {
    outPath = ../apps/yukimi;
    nixosModules.default = import ../apps/yukimi/nix/module.nix yukimi;
    packages.${system}.yukimi = pkgs.callPackage ../apps/yukimi/nix/package.nix { };
  };
  calamares = {
    outPath = ../installer/genkan;
    nixosModules = {
      default.imports = [
        calamares.nixosModules.desktops
        calamares.nixosModules.applications
      ];
      desktops.imports = [
        ../installer/genkan/rust/system
        tatami.nixosModules.default
        yukimi.nixosModules.default
      ];
      applications = ../installer/genkan/rust/src/applications.nix;
    };
  };
}
