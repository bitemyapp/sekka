# SPDX-License-Identifier: MIT OR Apache-2.0
# Yukimi's NixOS module. `self` is Yukimi's flake, or null when imported
# without one (nix/nixos.nix), in which case the package is built here.
self:
{
  config,
  options,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.yukimi;
  catalog = lib.types.submodule {
    options = {
      file = lib.mkOption {
        type = lib.types.path;
        description = "A JSON list of applications, in the format of Yukimi's own catalog.";
      };
      setting = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        example = "calamares.applications";
        description = ''
          A setting of this system, a list of application ids in its main
          configuration file, that installs the catalog's applications.
          Without one, Yukimi installs their packages as it installs any
          package.
        '';
      };
      title = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        example = "Apps chosen when installing";
        description = "What Yukimi calls the applications installed through `setting`.";
      };
    };
  };
in
{
  options.programs.yukimi = {
    enable = lib.mkEnableOption "Yukimi, the window onto what is installed and available";
    package = lib.mkOption {
      type = lib.types.package;
      default =
        if self == null then
          pkgs.callPackage ./package.nix { }
        else
          self.packages.${pkgs.stdenv.hostPlatform.system}.yukimi;
      defaultText = lib.literalExpression "yukimi.packages.\${system}.yukimi";
      description = "The Yukimi package. Its polkit policy names its own helper.";
    };
    configuration = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "/home/alice/nixos";
      description = ''
        The directory this system's configuration is in, when it isn't
        /etc/nixos. Like nixos-rebuild, Yukimi follows /etc/nixos/flake.nix
        when it links elsewhere, so this is only needed for a flake that
        /etc/nixos doesn't lead to.
      '';
    };
    catalogs = lib.mkOption {
      type = lib.types.listOf catalog;
      default = [ ];
      description = ''
        Catalogs of applications Discover offers besides Yukimi's own, such
        as the applications an installer offered. Their entries take the
        place of Yukimi's own entries for the same applications.
      '';
    };
  };
  config = lib.mkIf cfg.enable {
    # The package carries its desktop entry, icon and polkit action, which
    # the system profile links into place.
    environment.systemPackages = [ cfg.package ];
    # The helper runs through pkexec, which works only through NixOS's setuid
    # wrapper. Recent NixOS has that wrapper only when something asks for it
    # (Xfce does; Plasma, GNOME and Hyprland don't).
    security.polkit = {
      enable = true;
    }
    // lib.optionalAttrs (options.security.polkit ? enablePkexecWrapper) {
      enablePkexecWrapper = true;
    };
    # What Yukimi would otherwise have to evaluate the configuration to
    # learn: the Nixpkgs the system is built from (which Discover searches
    # and installs from), whether it accepts unfree packages, where its
    # configuration is, and its catalogs.
    environment.etc."yukimi/system.json".text = builtins.toJSON {
      # A flake system's Nixpkgs is a store path already, which the system
      # keeps; otherwise this is where the channel's Nixpkgs was found. Not
      # "${pkgs.path}", which would copy Nixpkgs into the store once more.
      nixpkgs =
        let
          source = (config.nixpkgs.flake or { }).source or null;
        in
        if source != null then toString source else toString pkgs.path;
      allowUnfree = pkgs.config.allowUnfree or false;
      inherit (cfg) configuration;
      catalogs = map (catalog: {
        file = "${catalog.file}";
        inherit (catalog) setting title;
      }) cfg.catalogs;
    };
  };
}
