# Evaluate both architectures on any Linux builder; no applications are built.
{ pkgs, inputs }:
let
  inherit (pkgs) lib;
  variants = [
    {
      system = "x86_64-linux";
      alternate = false;
    }
    {
      system = "aarch64-linux";
      alternate = false;
    }
    {
      system = "aarch64-linux";
      alternate = true;
    }
  ];
  catalogs = map (
    { system, alternate }:
    let
      config =
        (inputs.nixpkgs.lib.nixosSystem {
          inherit system;
          specialArgs = {
            applicationPkgs =
              let
                p = import inputs.applications {
                  inherit system;
                  config.allowUnfree = true;
                };
              in
              p // lib.optionalAttrs alternate { firefox = null; };
            aiPackages = if alternate then { } else inputs.ai-apps.packages.${system};
            ompPackage = if alternate then null else inputs.omp.packages.${system}.omp or null;
            sekkaPackages = {
              superluminal = inputs.superluminal.packages.${system}.default or null;
              tracy = inputs.tracy.packages.${system}.tracy or null;
              kakehashi =
                (import ./kakehashi.nix {
                  pkgs = inputs.nixpkgs.legacyPackages.${system};
                }).package;
            };
          };
          modules = [
            (import ../apps/yukimi/nix/module.nix null)
            ../installer/genkan/rust/src/applications.nix
            ({ pkgs, ... }: {
              programs.yukimi = {
                enable = true;
                package = pkgs.emptyDirectory;
              };
              calamares = {
                applications = lib.optional (!alternate) "firefox";
                installUser = "demo";
              };
              nixpkgs.config.allowUnfree = true;
              system.stateVersion = "26.11";
            })
          ];
        }).config;
    in
    {
      inherit system alternate;
      facts = config.environment.etc."yukimi/system.json".text;
    }
  ) variants;
  resolve = import ../apps/yukimi/nix/catalog.nix {
    inherit lib;
    inherit (pkgs.stdenv) hostPlatform;
    packagesFor = app: app.testPackages or [ ];
  };
  good = pkgs.hello;
  bad = good.overrideAttrs (_: {
    meta.platforms = [ ];
  });
  fixtures = resolve [
    {
      id = "bundle";
      testPackages = [
        good
        bad
      ];
    }
    {
      id = "missing";
      testPackages = [ null ];
    }
    {
      id = "broken";
      testPackages = [
        (good.overrideAttrs (_: {
          meta.broken = true;
        }))
      ];
    }
    {
      id = "throws";
      testPackages = [ (throw "package unavailable") ];
    }
    {
      id = "dependent";
      requires = [ "bundle" ];
    }
    {
      id = "transitive";
      requires = [ "dependent" ];
    }
    {
      id = "absent-dependency";
      requires = [ "not-in-catalog" ];
    }
    {
      id = "cycle-a";
      requires = [ "cycle-b" ];
    }
    {
      id = "cycle-b";
      requires = [ "cycle-a" ];
    }
  ];
in
assert builtins.all (app: (app.unavailable == null) == lib.hasPrefix "cycle-" app.id) fixtures;
pkgs.runCommand "yukimi-catalog-platforms" { nativeBuildInputs = [ pkgs.python3 ]; } ''
  python3 - ${pkgs.writeText "catalog-test-paths.json" (builtins.toJSON catalogs)} <<'PY'
  import json, sys
  for paths in json.load(open(sys.argv[1])):
      facts = json.loads(paths["facts"])
      files = {"own": facts["ownCatalog"], "installer": facts["catalogs"][0]["file"]}
      for source in ("own", "installer"):
          apps = {a["id"]: a for a in json.load(open(files[source]))}
          assert (apps["firefox"]["unavailable"] is None) == (source == "own" or not paths["alternate"]), (paths, source)
          assert (apps["discord"]["unavailable"] is None) == (paths["system"] == "x86_64-linux"), (paths, source)
          assert (apps["steam"]["unavailable"] is None) == (paths["system"] == "x86_64-linux"), (paths, source)
          if source == "installer":
              # Sources other than Nixpkgs, including an app with no package list.
              assert (apps["codex"]["unavailable"] is None) == (not paths["alternate"]), paths
              assert (apps["oh-my-pi"]["unavailable"] is None) == (not paths["alternate"]), paths
              if paths["system"] == "aarch64-linux":
                  assert apps["superluminal"]["unavailable"] is not None, paths
  PY
  touch $out
''
