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
  # A package's name without its version, as held packages are named
  # (and as Yukimi reads them from store paths).
  key = p: (builtins.parseDrvName p.name).name;
  # The packages of that name a saved configuration installed: exactly the
  # builds this system had, so keeping them builds nothing.
  heldFrom =
    name: hold:
    let
      saved = (builtins.getFlake hold.flake).nixosConfigurations.${hold.host}.config;
      found = builtins.filter (p: key p == name) saved.environment.systemPackages;
    in
    if found == [ ] then
      throw "programs.yukimi.held.${name}: the configuration it is kept from doesn't install ${name}"
    else
      found;
  # The system's packages with each held name's in place of the current
  # ones, each where it stood: the Nth of a name takes the saved
  # configuration's Nth, and the last of it any the saved one had beyond.
  withHeld =
    packages:
    let
      count = name: builtins.length (builtins.filter (q: key q == name) packages);
      step =
        acc: p:
        let
          name = key p;
          held = cfg.heldPackages.${name};
          i = acc.seen.${name} or 0;
          taken =
            if i + 1 == count name then
              lib.drop i held
            else
              lib.optional (i < builtins.length held) (builtins.elemAt held i);
        in
        if !(cfg.heldPackages ? ${name}) then
          acc // { out = acc.out ++ [ p ]; }
        else
          {
            seen = acc.seen // {
              ${name} = i + 1;
            };
            out = acc.out ++ taken;
          };
    in
    (builtins.foldl' step {
      seen = { };
      out = [ ];
    } packages).out;
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
    held = lib.mkOption {
      type = lib.types.attrsOf (
        lib.types.submodule {
          options = {
            flake = lib.mkOption {
              type = lib.types.str;
              example = "path:/var/lib/yukimi/held/generation-23?narHash=sha256-…";
              description = "A saved configuration of this system, pinned by its hash, to take the package from.";
            };
            host = lib.mkOption {
              type = lib.types.str;
              description = "The system's name among that flake's nixosConfigurations.";
            };
          };
        }
      );
      default = { };
      description = ''
        Packages kept at the version this system had, by name without the
        version (`firefox`). Each is taken from environment.systemPackages
        of a saved configuration instead of the current one, so it is
        exactly the build installed then and nothing is built. Only the
        system profile changes: anything else that uses the package gets
        the current one. Yukimi's Updates page writes these in yukimi.nix.
      '';
    };
    heldPackages = lib.mkOption {
      type = lib.types.attrsOf (lib.types.listOf lib.types.package);
      internal = true;
      default = lib.mapAttrs heldFrom cfg.held;
      defaultText = lib.literalMD "the packages of each held name in its saved configuration";
      description = "What each held name is replaced with in the system profile.";
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
  config = lib.mkMerge [
    (lib.mkIf (cfg.heldPackages != { }) {
      # NixOS's own system profile (nixos/modules/config/system-path.nix),
      # with held packages in place of the current ones. checks.held-packages
      # holds this to NixOS's: keeping a package at the version it is at must
      # build the same profile.
      system.path = lib.mkForce (
        pkgs.buildEnv {
          name = "system-path";
          paths = withHeld config.environment.systemPackages;
          inherit (config.environment) pathsToLink extraOutputsToInstall;
          ignoreCollisions = true;
          # !!! Hacky, should modularise.
          # outputs TODO: note that the tools will often not be linked by default
          postBuild = ''
            # Remove wrapped binaries, they shouldn't be accessible via PATH.
            find $out/bin -maxdepth 1 -name ".*-wrapped" -type l -delete
            find $out/bin -maxdepth 1 -name ".*-wrapped_*" -type l -delete

            if [ -x $out/bin/glib-compile-schemas -a -w $out/share/glib-2.0/schemas ]; then
                $out/bin/glib-compile-schemas $out/share/glib-2.0/schemas
            fi

            ${config.environment.extraSetup}
          '';
        }
      );
    })
    (lib.mkIf cfg.enable {
      systemd.tmpfiles.rules = [ "d /var/lib/yukimi/operations 0700 root root 30d" ];
      # The built target also protects users arriving through an old updater or
      # another rebuild frontend. Nix runs this before stopping any services.
      system.preSwitchChecks.yukimiSessionSafety = ''
        ${cfg.package}/bin/yukimi-helper check-activation "$1" "$2"
      '';
      # Changes which could tear down a live graphical session are prepared
      # for boot. Unrelated package/source changes leave this identity alone.
      environment.etc."yukimi/activation.json".text = builtins.toJSON {
        version = 1;
        displayManager = config.systemd.services.display-manager.serviceConfig or { };
        # Restarting logind drops its inhibitor file descriptors. A power-manager
        # transition must not open a sleep window during an overnight workload.
        sleepManager = {
          package = toString config.systemd.package;
          settings = config.services.logind.settings;
        };
        graphics = {
          package = toString config.hardware.graphics.package;
          extraPackages = map toString config.hardware.graphics.extraPackages;
          package32 = toString config.hardware.graphics.package32;
          extraPackages32 = map toString config.hardware.graphics.extraPackages32;
        };
      };
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
    })
  ];
}
