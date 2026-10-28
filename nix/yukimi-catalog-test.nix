# Evaluate both architectures on any Linux builder; no applications are built.
# Systems read catalog availability from metadata alone; the instantiated
# variants are the truth that metadata (with catalog `platforms` and `unfree`
# flags) must not contradict by offering something that cannot install.
{ pkgs, inputs }:
let
  inherit (pkgs) lib;
  variants = [
    { system = "x86_64-linux"; }
    { system = "aarch64-linux"; }
    {
      system = "aarch64-linux";
      alternate = true;
    }
    {
      system = "x86_64-linux";
      instantiate = true;
    }
    {
      system = "aarch64-linux";
      instantiate = true;
    }
    # A system refusing unfree packages, as Genkan installs when asked to.
    {
      system = "x86_64-linux";
      instantiate = true;
      free = true;
    }
  ];
  catalogs = map (
    {
      system,
      alternate ? false,
      instantiate ? false,
      free ? false,
    }:
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
                instantiateCatalogs = instantiate;
              };
              calamares = {
                applications = lib.optional (!alternate) "firefox";
                installUser = "demo";
              };
              nixpkgs.config.allowUnfree = !free;
              system.stateVersion = "26.11";
            })
          ];
        }).config;
    in
    {
      inherit
        system
        alternate
        instantiate
        free
        ;
      facts = config.environment.etc."yukimi/system.json".text;
    }
  ) variants;
  resolve =
    instantiate:
    import ../apps/yukimi/nix/catalog.nix {
      inherit lib instantiate;
      inherit (pkgs.stdenv) hostPlatform;
      packagesFor = app: app.testPackages or [ ];
    };
  good = pkgs.hello;
  bad = good.overrideAttrs (_: {
    meta.platforms = [ ];
  });
  fixtures = resolve false [
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
      id = "platforms";
      platforms = [ "riscv64-linux" ];
      testPackages = [ good ];
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
  # Metadata that says nothing of an unsupported dependency, as a wrapper's.
  wrapper = [
    {
      id = "wrapper";
      testPackages = [
        (good.overrideAttrs (_: {
          buildInputs = [ (throw "unsupported dependency") ];
        }))
      ];
    }
  ];
in
assert builtins.all (app: (app.unavailable == null) == lib.hasPrefix "cycle-" app.id) fixtures;
assert (builtins.head (resolve false wrapper)).unavailable == null;
assert (builtins.head (resolve true wrapper)).unavailable != null;
pkgs.runCommand "yukimi-catalog-platforms" { nativeBuildInputs = [ pkgs.python3 ]; } ''
  python3 - ${pkgs.writeText "catalog-test-paths.json" (builtins.toJSON catalogs)} <<'PY'
  import json, sys
  resolved = {}
  for paths in json.load(open(sys.argv[1])):
      facts = json.loads(paths["facts"])
      files = {"own": facts["ownCatalog"], "installer": facts["catalogs"][0]["file"]}
      key = (paths["system"], paths["alternate"], paths["instantiate"], paths["free"])
      resolved[key] = {source: {a["id"]: a for a in json.load(open(file))} for source, file in files.items()}

  problems = []
  # Nothing offered may fail to instantiate.
  for system in ("x86_64-linux", "aarch64-linux"):
      offered = resolved[(system, False, False, False)]
      checked = resolved[(system, False, True, False)]
      for source, apps in offered.items():
          for id, app in apps.items():
              truth = checked[source][id]["unavailable"]
              if app["unavailable"] is None and truth is not None:
                  problems.append(f'{source} catalog, {id} on {system}: offered, but "{truth}" '
                                  'List the systems it supports as its "platforms".')
  # Yukimi refuses unfree applications on a free system by their flag, before
  # Nix is asked; an application that needs an unfree package must say so.
  free = resolved[("x86_64-linux", False, True, True)]["own"]
  allowed = resolved[("x86_64-linux", False, True, False)]["own"]
  for id, app in free.items():
      if not app.get("unfree") and allowed[id]["unavailable"] is None and app["unavailable"] is not None:
          problems.append(f'own catalog, {id}: needs an unfree package without being marked "unfree".')
  print("\n".join(problems))
  assert not problems

  # What systems offer.
  for (system, alternate, instantiate, _), catalogs in resolved.items():
      if instantiate:
          continue
      for source, apps in catalogs.items():
          where = (system, alternate, source)
          assert (apps["firefox"]["unavailable"] is None) == (source == "own" or not alternate), where
          assert (apps["discord"]["unavailable"] is None) == (system == "x86_64-linux"), where
          assert (apps["steam"]["unavailable"] is None) == (system == "x86_64-linux"), where
          if source == "installer":
              # Sources other than Nixpkgs, including an app with no package list.
              assert (apps["codex"]["unavailable"] is None) == (not alternate), where
              assert (apps["oh-my-pi"]["unavailable"] is None) == (not alternate), where
              if system == "aarch64-linux":
                  assert apps["superluminal"]["unavailable"] is not None, where
  PY
  touch $out
''
