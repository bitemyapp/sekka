# Evaluate both architectures on any Linux builder; no applications are built.
# Systems read catalog availability from Nixpkgs' metadata alone, and that of
# entries with a `source` (mostly other flakes' packages) from their
# `platforms` lists; the instantiated variants read every package and are the
# truth that systems (with catalog `platforms` and `unfree` flags) must not
# contradict by offering something that cannot install.
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
    # Where the Nixpkgs a catalog's packages come from installs unsupported
    # packages, the catalog offers them, except where its own lists say
    # otherwise. Yukimi's own catalog follows the system's Nixpkgs; Genkan's
    # follows applicationPkgs, which installed systems configure apart from
    # nixpkgs.config. Each variant allows them in only one of the two.
    {
      system = "aarch64-linux";
      unsupported = "system";
    }
    {
      system = "aarch64-linux";
      unsupported = "applications";
    }
  ];
  catalogs = map (
    {
      system,
      alternate ? false,
      instantiate ? false,
      free ? false,
      unsupported ? null,
    }:
    let
      # Sekka's catalog packages for this system, from the per-system
      # definition that also gives systems theirs (x86_64-linux's today,
      # whatever the system).
      sekkaPackages = inputs.catalog.packagesFor system;
      # A system that doesn't instantiate the catalogs must not read the
      # packages of entries with a `source`: those of other flakes would be
      # fetched whatever was picked. An abort, unlike a throw, escapes the
      # resolver's tryEval and fails here.
      unread =
        source: value:
        if instantiate then value else abort "A system evaluation read the catalog's ${source} packages.";
      config =
        (inputs.nixpkgs.lib.nixosSystem {
          inherit system;
          specialArgs = {
            applicationPkgs =
              let
                p = import inputs.applications {
                  inherit system;
                  config = {
                    allowUnfree = true;
                    allowUnsupportedSystem = unsupported == "applications";
                  };
                };
              in
              p // lib.optionalAttrs alternate { firefox = null; };
            aiPackages = unread "ai-apps" inputs.ai-apps.packages.${system};
            ompPackage = unread "omp" (inputs.omp.packages.${system}.omp or null);
            sekkaPackages = lib.mapAttrs (name: unread name) sekkaPackages;
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
              nixpkgs.config = {
                allowUnfree = !free;
                allowUnsupportedSystem = unsupported == "system";
              };
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
        unsupported
        ;
      facts = config.environment.etc."yukimi/system.json".text;
    }
  ) variants;
  genkanCatalog = builtins.fromJSON (
    builtins.readFile ../installer/genkan/rust/src/applications.json
  );
  # The catalog's Sekka packages are exactly those nix/components.nix gives
  # systems, for every system.
  sekkaNames = lib.unique (
    lib.concatMap (app: app.packages) (
      builtins.filter (app: app.source or null == "sekka") genkanCatalog
    )
  );
  resolve =
    args:
    import ../apps/yukimi/nix/catalog.nix (
      {
        inherit lib;
        inherit (pkgs.stdenv) hostPlatform;
        packagesFor = app: app.testPackages or [ ];
        listed = app: app.listed or false;
      }
      // args
    );
  available = apps: map (app: app.id) (builtins.filter (app: app.unavailable == null) apps);
  host = pkgs.stdenv.hostPlatform.system;
  good = pkgs.hello;
  bad = good.overrideAttrs (_: {
    meta.platforms = [ ];
  });
  broken = good.overrideAttrs (_: {
    meta.broken = true;
  });
  fixtures = [
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
      testPackages = [ broken ];
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
    # Decided by their lists; their packages are never read.
    {
      id = "listed";
      listed = true;
      platforms = [ host ];
      testPackages = abort "A listed entry's packages were read.";
    }
    {
      id = "listed-elsewhere";
      listed = true;
      platforms = [ "riscv64-linux" ];
      testPackages = abort "A listed entry's packages were read.";
    }
    {
      id = "unlisted";
      listed = true;
      testPackages = abort "A listed entry's packages were read.";
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
  # A list says nothing of a missing package: instantiating reads it anyway.
  listedMissing = [
    {
      id = "listed-missing";
      listed = true;
      platforms = [ host ];
      testPackages = [ null ];
    }
  ];
  policyFixtures = builtins.filter (
    app:
    builtins.elem app.id [
      "bundle"
      "broken"
      "platforms"
    ]
  ) fixtures;
in
assert
  available (resolve { } fixtures) == [
    "cycle-a"
    "cycle-b"
    "listed"
  ];
assert available (resolve { } wrapper) == [ "wrapper" ];
assert available (resolve { instantiate = true; } wrapper) == [ ];
assert available (resolve { } listedMissing) == [ "listed-missing" ];
assert available (resolve { instantiate = true; } listedMissing) == [ ];
# The source's Nixpkgs policy decides, as it would when installing; the
# catalog's own `platforms` lists still do.
assert available (resolve { config.allowBroken = true; } policyFixtures) == [ "broken" ];
assert available (resolve { config.allowUnsupportedSystem = true; } policyFixtures) == [ "bundle" ];
assert lib.sort lib.lessThan sekkaNames == lib.attrNames inputs.catalog.packages.x86_64-linux;
assert
  lib.attrNames (inputs.catalog.packagesFor "aarch64-linux")
  == lib.attrNames inputs.catalog.packages.x86_64-linux;
pkgs.runCommand "yukimi-catalog-platforms" { nativeBuildInputs = [ pkgs.python3 ]; } ''
  python3 - ${pkgs.writeText "catalog-test-paths.json" (builtins.toJSON catalogs)} <<'PY'
  import json, sys
  resolved = {}
  for paths in json.load(open(sys.argv[1])):
      facts = json.loads(paths["facts"])
      files = {"own": facts["ownCatalog"], "installer": facts["catalogs"][0]["file"]}
      key = (paths["system"], paths["alternate"], paths["instantiate"], paths["free"], paths["unsupported"])
      resolved[key] = {source: {a["id"]: a for a in json.load(open(file))} for source, file in files.items()}

  problems = []
  # Nothing offered may fail to instantiate.
  for system in ("x86_64-linux", "aarch64-linux"):
      offered = resolved[(system, False, False, False, None)]
      checked = resolved[(system, False, True, False, None)]
      for source, apps in offered.items():
          for id, app in apps.items():
              truth = checked[source][id]["unavailable"]
              if app["unavailable"] is None and truth is not None:
                  problems.append(f'{source} catalog, {id} on {system}: offered, but "{truth}" '
                                  'List the systems it supports as its "platforms".')
  # Yukimi refuses unfree applications on a free system by their flag, before
  # Nix is asked; an application that needs an unfree package must say so.
  free = resolved[("x86_64-linux", False, True, True, None)]["own"]
  allowed = resolved[("x86_64-linux", False, True, False, None)]["own"]
  for id, app in free.items():
      if not app.get("unfree") and allowed[id]["unavailable"] is None and app["unavailable"] is not None:
          problems.append(f'own catalog, {id}: needs an unfree package without being marked "unfree".')
  print("\n".join(problems))
  assert not problems

  # What systems offer.
  for (system, alternate, instantiate, _, unsupported), catalogs in resolved.items():
      if instantiate or unsupported:
          continue
      for source, apps in catalogs.items():
          where = (system, alternate, source)
          assert (apps["firefox"]["unavailable"] is None) == (source == "own" or not alternate), where
          assert (apps["discord"]["unavailable"] is None) == (system == "x86_64-linux"), where
          assert (apps["steam"]["unavailable"] is None) == (system == "x86_64-linux"), where
          if source == "installer":
              # Entries with a `source`, offered by their lists without their
              # packages being read, including one with no package list.
              assert apps["codex"]["unavailable"] is None, where
              assert apps["oh-my-pi"]["unavailable"] is None, where
              assert (apps["superluminal"]["unavailable"] is None) == (system == "x86_64-linux"), where
  # Where the Nixpkgs a catalog's packages come from installs unsupported
  # packages, an ARM system is offered Discord from that catalog alone, though
  # not Steam, whose own list says its runtime is x86 only.
  follows = {"own": "system", "installer": "applications"}
  for allowing in ("system", "applications"):
      for source, apps in resolved[("aarch64-linux", False, False, False, allowing)].items():
          where = (allowing, source)
          assert (apps["discord"]["unavailable"] is None) == (follows[source] == allowing), where
          assert apps["steam"]["unavailable"] is not None, where
  PY
  touch $out
''
