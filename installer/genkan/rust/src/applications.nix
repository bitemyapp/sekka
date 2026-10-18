# SPDX-License-Identifier: GPL-3.0-or-later
# Exported as nixosModules.applications by this repository's flake; installed
# systems import it from their `calamares` input. Package sources come from the
# installed flake.lock.
{
  config,
  options,
  lib,
  pkgs,
  applicationPkgs ? pkgs,
  aiPackages ? { },
  ompPackage ? null,
  sekkaPackages,
  ...
}:
let
  catalog = builtins.fromJSON (builtins.readFile ./applications.json);
  ids = config.calamares.applications;
  has = id: builtins.elem id ids;
  selected = builtins.filter (app: has app.id) catalog;
  # Where each source's packages come from; Nixpkgs' otherwise.
  sources = {
    ai = aiPackages;
    sekka = sekkaPackages;
  };
  # Chromium and Electron apps use GNOME Keyring in every desktop but Plasma.
  withKeyring = import ./keyring.nix {
    inherit lib;
    pkgs = applicationPkgs;
  };
  packagesFor =
    app:
    if (app.source or "nixpkgs") == "omp" then
      [ ompPackage ]
    else
      map (
        name: withKeyring app (sources.${app.source or "nixpkgs"} or applicationPkgs).${name}
      ) app.packages;
  packages = lib.unique (lib.concatMap packagesFor selected);
  # Development build tools, Lazygit and the GitHub CLI each bring Git.
  withGit = builtins.any (app: builtins.elem "git" app.packages) selected;
  # nixpkgs names Zed's Linux CLI `zeditor`, while Zed's docs and most user
  # workflows invoke it as `zed`. Expose the expected command whenever the
  # editor is selected in the installer.
  zedCli = lib.optional (has "zed") (pkgs.writeShellScriptBin "zed" ''
    exec ${applicationPkgs.zed-editor}/bin/zeditor "$@"
  '');
  launchers = map (
    app:
    pkgs.makeDesktopItem {
      name = "calamares-${app.id}";
      desktopName = app.name;
      comment = app.description;
      exec = app.terminal;
      terminal = true;
      icon = "utilities-terminal";
      categories = [
        "Development"
        "ConsoleOnly"
      ];
    }
  ) (builtins.filter (app: app ? terminal && app.id != "neovim") selected);
  # Without defaults, whichever installed program claims a type opens it:
  # the ChatGPT app claims web links and Office documents.
  browsers = {
    firefox = "firefox.desktop";
    chromium = "chromium-browser.desktop";
    google-chrome = "google-chrome.desktop";
  };
  # In the catalog's order.
  browser = lib.findFirst has null [
    "firefox"
    "chromium"
    "google-chrome"
  ];
  office = {
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document" = "writer.desktop";
    "application/msword" = "writer.desktop";
    "application/vnd.oasis.opendocument.text" = "writer.desktop";
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" = "calc.desktop";
    "application/vnd.ms-excel" = "calc.desktop";
    "application/vnd.ms-excel.sheet.macroEnabled.12" = "calc.desktop";
    "application/vnd.oasis.opendocument.spreadsheet" = "calc.desktop";
    "text/csv" = "calc.desktop";
    "text/tab-separated-values" = "calc.desktop";
    "application/vnd.openxmlformats-officedocument.presentationml.presentation" = "impress.desktop";
    "application/vnd.ms-powerpoint" = "impress.desktop";
    "application/vnd.oasis.opendocument.presentation" = "impress.desktop";
  };
in
{
  # Yukimi's Discover offers this catalog, installed through
  # calamares.applications, when Yukimi's module is imported (and is new
  # enough to take catalogs). Not even an empty programs.yukimi otherwise:
  # without the module that option doesn't exist.
  imports = [
    {
      config = lib.optionalAttrs (options ? programs.yukimi.catalogs) {
        programs.yukimi.catalogs = [
          {
            file = ./applications.json;
            setting = "calamares.applications";
            title = "Apps chosen when installing";
          }
        ];
      };
    }
  ];
  options.calamares = {
    applications = lib.mkOption {
      type = lib.types.listOf (lib.types.enum (map (app: app.id) catalog));
      default = [ "firefox" ];
      description = "Applications selected in the graphical installer.";
    };
    installUser = lib.mkOption {
      type = lib.types.str;
      description = "User created by the graphical installer.";
    };
  };
  config = {
    assertions = [
      {
        assertion = lib.length ids == lib.length (lib.unique ids);
        message = "Duplicate installer applications.";
      }
      {
        assertion = !has "rustup" || has "build-tools";
        message = "Rustup requires Development build tools.";
      }
      {
        assertion =
          (config.nixpkgs.config.allowUnfree or false) || builtins.all (app: !(app.unfree or false)) selected;
        message = "Selected applications require allowing proprietary software.";
      }
    ];
    environment.systemPackages = packages ++ launchers ++ zedCli;
    environment.etc."installer-applications.json".text = builtins.toJSON {
      selected = ids;
      packages = map (package: {
        inherit (package) name;
        path = package.outPath;
        version = package.version or null;
      }) packages;
    };
    # A GC root for the exact selected packages is built BEFORE any disk write.
    system.build.installerApplications = pkgs.linkFarm "installer-applications" (
      lib.imap0 (i: package: {
        name = "${toString i}-${lib.getName package}";
        path = package;
      }) (packages ++ launchers)
    );
    # The first selected browser opens links, LibreOffice opens documents;
    # each user can still choose others.
    xdg.mime.defaultApplications =
      lib.optionalAttrs (browser != null) (
        lib.genAttrs [
          "text/html"
          "application/xhtml+xml"
          "x-scheme-handler/http"
          "x-scheme-handler/https"
        ] (_: browsers.${browser})
      )
      // lib.optionalAttrs (has "libreoffice") office;
    programs.firefox = lib.mkIf (has "firefox") {
      enable = true;
      package = applicationPkgs.firefox;
    };
    programs.steam = lib.mkIf (has "steam") {
      enable = true;
      package = applicationPkgs.steam;
    };
    # Git LFS comes with Git. Its filter in /etc/gitconfig is what
    # `git lfs install` would set per account: without it, LFS files clone as
    # pointer files. Git LFS adds each repository's hooks itself.
    programs.git = lib.mkIf withGit {
      enable = true;
      package = lib.mkDefault applicationPkgs.git;
      lfs = {
        enable = true;
        package = lib.mkDefault applicationPkgs.git-lfs;
      };
    };
    virtualisation.docker = lib.mkIf (has "docker") {
      package = applicationPkgs.docker;
      rootless = {
        enable = true;
        setSocketVariable = true;
      };
    };
    # For people only: nixpkgs excludes just root, so the login screen's
    # user manager (and other system users') started Docker at boot, where it
    # cannot run and retried until rate-limited.
    systemd.user.services.docker.unitConfig.ConditionUser = lib.mkIf (has "docker") (
      lib.mkForce "!@system"
    );
    users.users.${config.calamares.installUser}.linger = lib.mkIf (has "docker") true;
  };
}
