# First-party source is always taken from this checkout. The catalog's
# applications from other flakes come from Sekka's own inputs of those names.
{
  nixpkgs,
  superluminal,
  tracy,
}:
let
  systems = [
    "x86_64-linux"
    "aarch64-linux"
  ];
  forSystems = nixpkgs.lib.genAttrs systems;
in
rec {
  tatami = {
    outPath = ../desktop/tatami;
    nixosModules.default = ../desktop/tatami/module.nix;
    packages = forSystems (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        tatami = pkgs.callPackage ../desktop/tatami/tool/package.nix { };
        displays = pkgs.callPackage ../desktop/tatami/displays/package.nix { };
        network = pkgs.callPackage ../desktop/tatami/network/package.nix { };
        activity = pkgs.callPackage ../desktop/tatami/activity/package.nix { };
        wallpapers = pkgs.callPackage ../desktop/tatami/wallpapers/package.nix { };
        elephant = pkgs.callPackage ../desktop/tatami/elephant/package.nix { };
      }
    );
  };
  yukimi = {
    outPath = ../apps/yukimi;
    nixosModules.default = import ../apps/yukimi/nix/module.nix yukimi;
    packages = forSystems (system: {
      yukimi = nixpkgs.legacyPackages.${system}.callPackage ../apps/yukimi/nix/package.nix { };
    });
  };
  kakehashi = {
    outPath = ../apps/kakehashi;
    packages = forSystems (system: {
      kakehashi = (import ./kakehashi.nix { pkgs = nixpkgs.legacyPackages.${system}; }).package;
    });
  };
  # The catalog's applications from this flake (source "sekka"): its own, and
  # those other flakes package, built with the applications' Nixpkgs.
  catalog.packages = forSystems (
    system:
    kakehashi.packages.${system}
    // {
      tracy = tracy.packages.${system}.tracy;
    }
    // nixpkgs.lib.optionalAttrs (builtins.hasAttr system superluminal.packages) {
      superluminal = superluminal.packages.${system}.default;
    }
  );
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
      # Installed systems make this flake's inputs their own, so these are the
      # same store paths as `nix run .#push-cache` publishes for the free ones.
      applications = { pkgs, ... }: {
        key = "sekka#applications";
        imports = [ ../installer/genkan/rust/src/applications.nix ];
        _module.args.sekkaPackages = catalog.packages.${pkgs.stdenv.hostPlatform.system};
      };
    };
  };
}
