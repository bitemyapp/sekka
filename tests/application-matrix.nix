# Evaluate real configuration.nix output from `application-configurations`.
{
  caseName,
  configurations ? null,
}:
let
  repo = toString ../.;
  flake = builtins.getFlake ("git+file://" + repo);
  library = import ../nix/applications.nix { inputs = flake.inputs // flake.lib.components; };
  lib = flake.inputs.nixpkgs.lib;
  requested =
    if caseName == "all" then
      map (app: app.id) library.catalog
    else if caseName == "none" then
      [ ]
    else if caseName == "free-only" then
      map (app: app.id) (builtins.filter (app: !(app.unfree or false)) library.catalog)
    else
      [ caseName ] ++ ((lib.findFirst (app: app.id == caseName) null library.catalog).requires or [ ]);
  expected = map (app: app.id) (
    builtins.filter (app: builtins.elem app.id requested) library.catalog
  );
  system = flake.inputs.nixpkgs.lib.nixosSystem {
    system = "x86_64-linux";
    inherit (library) specialArgs;
    modules = [
      flake.inputs.determinate.nixosModules.default
      # The modules installed systems import from their `sekka` input.
      flake.nixosModules.default
      (builtins.toPath (repo + "/.work/application-modules/" + caseName + ".nix"))
      {
        fileSystems."/" = {
          device = "/dev/disk/by-uuid/old-root";
          fsType = "ext4";
        };
        fileSystems."/boot" = {
          device = "/dev/disk/by-uuid/old-boot";
          fsType = "vfat";
        };
      }
    ];
  };
  c = system.config;
  has = id: builtins.elem id expected;
  # Inspect the manifest without realizing its referenced packages here.
  manifest = builtins.fromJSON (
    builtins.unsafeDiscardStringContext c.environment.etc."installer-applications.json".text
  );
in
assert c.calamares.applications == expected;
assert manifest.selected == expected;
assert c.programs.firefox.enable == has "firefox";
assert c.programs.steam.enable == has "steam";
assert c.virtualisation.docker.rootless.enable == has "docker";
# Rootless Docker starts only for regular users, not the login screen's.
assert !has "docker" || c.systemd.user.services.docker.unitConfig.ConditionUser == "!@system";
# Links open in the first selected browser and documents in LibreOffice, not
# in whatever else claims them (the ChatGPT app claims both).
assert
  (c.xdg.mime.defaultApplications."x-scheme-handler/https" or null) == (
    if has "firefox" then
      "firefox.desktop"
    else if has "chromium" then
      "chromium-browser.desktop"
    else if has "google-chrome" then
      "google-chrome.desktop"
    else
      null
  );
assert
  (c.xdg.mime.defaultApplications."text/csv" or null)
  == (if has "libreoffice" then "calc.desktop" else null);
assert !(builtins.elem "docker" c.users.users.alice.extraGroups);
assert !has "rustup" || has "build-tools";
assert c.fileSystems."/".device == "/dev/disk/by-uuid/11111111-2222-4333-8444-555555555555";
assert c.fileSystems."/boot".device == "/dev/disk/by-uuid/A1B2-C3D4";
# The all-inclusive case also takes the GitHub step: a key from GitHub, the
# SSH server for keys only, and a Git identity. Other cases take none of it.
assert
  c.services.openssh.enable == (caseName == "all")
  && (c.users.users.alice.openssh.authorizedKeys.keys != [ ]) == (caseName == "all");
assert
  caseName != "all"
  || (
    !c.services.openssh.settings.PasswordAuthentication
    && !c.services.openssh.settings.KbdInteractiveAuthentication
    && map (lib.hasPrefix "ssh-ed25519 ") c.users.users.alice.openssh.authorizedKeys.keys == [ true ]
    && c.environment.etc ? "ssh/authorized_keys.d/alice"
    && lib.hasInfix ''name = "Alice"'' c.environment.etc.gitconfig.text
    && lib.hasInfix ''email = "alice@example.com"'' c.environment.etc.gitconfig.text
  );
{
  derivation = c.system.build.toplevel.drvPath;
  inherit manifest;
  allowUnfree = c.nixpkgs.config.allowUnfree;
  rootlessDocker = c.virtualisation.docker.rootless.enable;
  dockerLinger = c.users.users.alice.linger;
  ssh = {
    server = c.services.openssh.enable;
    authorizedKeys = c.users.users.alice.openssh.authorizedKeys.keys;
  };
  git = if c.programs.git.enable then c.environment.etc.gitconfig.text else null;
}
