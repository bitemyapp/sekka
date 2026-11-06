# Keep Cargo's complete workspace metadata, but only real sources for the
# requested packages and their path dependencies. Unselected members have
# Crane's inert sources so explicit workspace members remain valid.
{
  lib,
  runCommand,
  craneLib,
  root ? ../../..,
}:
let
  path = relative: root + "/${relative}";
  directories = [
    "desktop/aizome/crates"
    "desktop/aizome/apps"
    "desktop/aizome/sukumo/crates"
    "apps/yukimi/crates"
    "apps/hotaru/crates"
  ];
  packageDirs =
    lib.concatMap (
      dir:
      map (name: "${dir}/${name}") (
        lib.filter (name: builtins.pathExists (path "${dir}/${name}/Cargo.toml")) (
          builtins.attrNames (builtins.readDir (path dir))
        )
      )
    ) directories
    ++ [ "apps/hotaru/vendor/simdutf8" ];
  packages = builtins.listToAttrs (
    map (dir: {
      name = (builtins.fromTOML (builtins.readFile (path "${dir}/Cargo.toml"))).package.name;
      value = dir;
    }) packageDirs
  );
  lock = builtins.fromTOML (builtins.readFile (path "desktop/aizome/Cargo.lock"));
  # Include every locked version of a dependency name. This is conservative
  # about feature/target-specific dependencies and does not run Cargo in eval.
  # Index once: every component walks this graph, and repeatedly scanning
  # the entire lockfile for each edge makes package evaluation expensive.
  dependencies = lib.mapAttrs (
    _: versions:
    lib.unique (
      lib.concatMap (
        p: map (dependency: builtins.head (lib.splitString " " dependency)) (p.dependencies or [ ])
      ) versions
    )
  ) (lib.groupBy (p: p.name) lock.package);
  closure =
    names:
    map (item: item.key) (
      builtins.genericClosure {
        startSet = map (key: { inherit key; }) names;
        operator = item: map (key: { inherit key; }) (dependencies.${item.key} or [ ]);
      }
    );
  localClosure =
    names: lib.sort builtins.lessThan (lib.filter (name: packages ? ${name}) (closure names));
  metadata = map path [
    "desktop/aizome/Cargo.toml"
    "desktop/aizome/Cargo.lock"
    "desktop/aizome/sukumo/Cargo.toml"
    "apps/yukimi/Cargo.toml"
    "apps/hotaru/Cargo.toml"
  ];
  allSource = lib.fileset.toSource {
    inherit root;
    fileset = lib.fileset.unions (metadata ++ map path packageDirs);
  };
  dummy = craneLib.mkDummySrc {
    src = allSource;
    extraDummyScript = ''
      cp ${path "desktop/aizome/Cargo.lock"} $out/desktop/aizome/Cargo.lock
    '';
  };
  forPackages =
    names:
    let
      dirs = map (name: packages.${name}) (localClosure names);
      actual = lib.fileset.toSource {
        inherit root;
        fileset = lib.fileset.unions (
          metadata
          ++ map path dirs
          ++ [
            (path "desktop/aizome/scripts/rustc-wrapper")
          ]
          ++ lib.optionals (builtins.elem "aizome-comp-core" names || builtins.elem "aizome-comp" names) [
            # The compositor embeds a small set of kit icons without linking it.
            (path "desktop/aizome/crates/aizome-kit/icons")
          ]
        );
      };
    in
    runCommand "source" { } ''
      mkdir -p $out
      cp -R ${dummy}/. $out/
      chmod -R u+w $out
      ${lib.concatMapStringsSep "\n" (dir: "rm -rf $out/${dir}") dirs}
      cp -R ${actual}/. $out/
    '';
in
{
  inherit
    packages
    localClosure
    forPackages
    dummy
    ;
}
