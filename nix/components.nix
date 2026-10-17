# First-party source is always taken from this checkout; Aizome comes from
# its own repository (the flake's aizome input).
{ nixpkgs, aizome }:
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
      network = pkgs.callPackage ../desktop/tatami/network/package.nix { };
      activity = pkgs.callPackage ../desktop/tatami/activity/package.nix { };
      wallpapers = pkgs.callPackage ../desktop/tatami/wallpapers/package.nix { };
      elephant = pkgs.callPackage ../desktop/tatami/elephant/package.nix { };
    };
  };
  yukimi = {
    outPath = ../apps/yukimi;
    nixosModules.default = import ../apps/yukimi/nix/module.nix yukimi;
    packages.${system}.yukimi = pkgs.callPackage ../apps/yukimi/nix/package.nix { };
  };
  kakehashi = {
    outPath = ../apps/kakehashi;
    packages.${system}.kakehashi = (import ./kakehashi.nix { inherit pkgs; }).package;
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
        aizome.nixosModules.default
      ];
      # The catalog's applications from this flake (source "sekka"), built
      # with its Nixpkgs, which installed systems make their own: the same
      # store paths as `nix run .#push-cache` publishes.
      applications = {
        key = "sekka#applications";
        imports = [ ../installer/genkan/rust/src/applications.nix ];
        _module.args.sekkaPackages = kakehashi.packages.${system};
      };
    };
  };
}
