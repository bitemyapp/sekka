# First-party source is always taken from this checkout; Aizome comes from
# its own repository (the flake's aizome input), and the catalog's
# applications from other flakes from Sekka's own inputs of those names.
{
  nixpkgs,
  aizome,
  superluminal,
  tracy,
}:
let
  system = "x86_64-linux";
  pkgs = nixpkgs.legacyPackages.${system};
  upstreamAizome = aizome;
  scheduledAizome = upstreamAizome.packages.${system}.aizome.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ./scheduling/aizome-launch-scope.patch ];
  });
in
rec {
  aizome = upstreamAizome // {
    packages.${system} = upstreamAizome.packages.${system} // {
      aizome = scheduledAizome;
      default = scheduledAizome;
    };
    nixosModules = upstreamAizome.nixosModules // {
      default = { lib, ... }: {
        imports = [ upstreamAizome.nixosModules.default ];
        programs.aizome.package = lib.mkDefault scheduledAizome;
      };
    };
  };
  tatami = {
    outPath = ../desktop/tatami;
    nixosModules.default = ../desktop/tatami/module.nix;
    packages.${system} = {
      tatami = pkgs.callPackage ../desktop/tatami/tool/package.nix { };
      displays = pkgs.callPackage ../desktop/tatami/displays/package.nix { };
      network = pkgs.callPackage ../desktop/tatami/network/package.nix { };
      bluetooth = pkgs.callPackage ../desktop/tatami/bluetooth/package.nix { };
      sound = pkgs.callPackage ../desktop/tatami/sound/package.nix { };
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
  # The catalog's applications from this flake (source "sekka"): its own, and
  # those other flakes package, built with the applications' Nixpkgs.
  catalog.packages.${system} = kakehashi.packages.${system} // {
    superluminal = superluminal.packages.${system}.default;
    tracy = tracy.packages.${system}.tracy;
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
      # Installed systems make this flake's inputs their own, so these are the
      # same store paths as `nix run .#push-cache` publishes for the free ones.
      applications = {
        key = "sekka#applications";
        imports = [ ../installer/genkan/rust/src/applications.nix ];
        _module.args.sekkaPackages = catalog.packages.${system};
      };
    };
  };
}
