# Aizome's Cargo workspace as sources that let a package compile only what
# changed (DEVELOPERS.md, "Aizome's builds").
#
# A package's third-party crates compile in a derivation of their own, from
# `stubs`: the workspace with every first-party crate reduced to its
# manifest and empty targets. That copy changes with Cargo.lock and the
# manifests, never with Rust source, so the package's next build unpacks the
# compiled crates (`restoreTarget`) and compiles only the first-party ones.
#
# `withCrates` gives a package the real source of the crates it builds and
# their path dependencies, the rest as stubs: an edit to a crate it doesn't
# use leaves its derivation unchanged.
{
  lib,
  runCommand,
  rustPlatform,
}:
let
  sekka = ../../..;
  readManifest = dir: builtins.fromTOML (builtins.readFile (sekka + "/${dir}/Cargo.toml"));
  # The workspace's members, and the path dependencies it takes from
  # Sukumo, Yukimi's backend and Hotaru's core.
  parents = [
    "desktop/aizome/crates"
    "desktop/aizome/apps"
    "desktop/aizome/sukumo/crates"
    "apps/yukimi/crates"
    "apps/hotaru/crates"
  ];
  crates = lib.concatMap (
    parent:
    lib.mapAttrsToList (name: _: "${parent}/${name}") (
      lib.filterAttrs (
        name: type: type == "directory" && builtins.pathExists (sekka + "/${parent}/${name}/Cargo.toml")
      ) (builtins.readDir (sekka + "/${parent}"))
    )
  ) parents;
  # Each crate inherits from the nearest of these.
  roots = [
    "desktop/aizome/sukumo"
    "desktop/aizome"
    "apps/yukimi"
    "apps/hotaru"
  ];
  rootOf =
    dir: lib.findFirst (root: lib.hasPrefix "${root}/" dir) (throw "${dir}: no workspace") roots;
  normalize =
    path:
    lib.concatStringsSep "/" (
      lib.foldl' (
        parts: part:
        if part == ".." then
          lib.init parts
        else if part == "." || part == "" then
          parts
        else
          parts ++ [ part ]
      ) [ ] (lib.splitString "/" path)
    );

  # What Cargo expects of a crate on disk: its targets' files.
  targets =
    dir:
    let
      manifest = readManifest dir;
      package = manifest.package;
      exists = path: builtins.pathExists (sekka + "/${dir}/${path}");
      binDir = lib.optionalAttrs (exists "src/bin") (builtins.readDir (sekka + "/${dir}/src/bin"));
      discovered = lib.optionals (package.autobins or true) (
        lib.optional (exists "src/main.rs") "src/main.rs"
        ++ lib.mapAttrsToList (
          name: type: if type == "directory" then "src/bin/${name}/main.rs" else "src/bin/${name}"
        ) (lib.filterAttrs (name: type: type == "directory" || lib.hasSuffix ".rs" name) binDir)
      );
      declared =
        kind: default:
        map (target: {
          inherit kind;
          path = target.path or "${default}/${target.name}.rs";
        }) (manifest.${kind} or [ ]);
      build =
        if package ? build then
          lib.optional (builtins.isString package.build) package.build
        else
          lib.optional (exists "build.rs") "build.rs";
    in
    lib.optional (manifest ? lib || exists "src/lib.rs") {
      kind = "lib";
      path = manifest.lib.path or "src/lib.rs";
    }
    ++ declared "bin" "src/bin"
    ++ declared "test" "tests"
    ++ declared "bench" "benches"
    ++ declared "example" "examples"
    ++ map (path: {
      kind = "bin";
      inherit path;
    }) (discovered ++ build);

  # Every first-party crate's manifest, the lockfile, the rustc wrapper
  # and the vendored simdutf8 that [patch.crates-io] makes everyone's.
  manifests = lib.fileset.toSource {
    root = sekka;
    fileset = lib.fileset.unions (
      [
        (sekka + "/desktop/aizome/Cargo.lock")
        (sekka + "/desktop/aizome/scripts/rustc-wrapper")
        (sekka + "/apps/hotaru/vendor/simdutf8")
      ]
      ++ map (dir: sekka + "/${dir}/Cargo.toml") (roots ++ crates)
    );
  };
  stubs = runCommand "aizome-source" { } ''
    cp -r ${manifests} $out
    chmod -R u+w $out
    stub() {
      mkdir -p "$(dirname "$out/$2")"
      if [ "$1" = lib ]; then : > "$out/$2"; else echo 'fn main() {}' > "$out/$2"; fi
    }
    ${lib.concatMapStrings (
      dir: lib.concatMapStrings (target: "stub ${target.kind} ${dir}/${target.path}\n") (targets dir)
    ) crates}
  '';

  # First-party crates a crate needs: its dependencies and build
  # dependencies by path, its dev-dependencies too when it is one being
  # tested, and theirs.
  pathDependencies =
    dir: dev:
    let
      manifest = readManifest dir;
      inherited = (readManifest (rootOf dir)).workspace.dependencies or { };
      kinds = [
        "dependencies"
        "build-dependencies"
      ]
      ++ lib.optional dev "dev-dependencies";
      tables =
        map (kind: manifest.${kind} or { }) kinds
        ++ lib.concatMap (target: map (kind: target.${kind} or { }) kinds) (
          lib.attrValues (manifest.target or { })
        );
      resolve =
        name: spec:
        if builtins.isAttrs spec && spec ? path then
          normalize "${dir}/${spec.path}"
        else if builtins.isAttrs spec && spec.workspace or false && (inherited.${name} or { }) ? path then
          normalize "${rootOf dir}/${inherited.${name}.path}"
        else
          null;
    in
    lib.filter (dep: lib.elem dep crates) (lib.concatMap (lib.mapAttrsToList resolve) tables);
  closure =
    tested:
    map (item: item.key) (
      builtins.genericClosure {
        startSet = map (key: {
          inherit key;
          dev = true;
        }) tested;
        operator =
          item:
          map (key: {
            inherit key;
            dev = false;
          }) (pathDependencies item.key item.dev);
      }
    );

  # The first-party packages, whose compiled stubs a package's dependency
  # build must not hand on.
  names = map (dir: (readManifest dir).package.name) crates;
in
{
  inherit stubs;

  # The lockfile's crates, vendored once for every package that builds from
  # this workspace. The lockfile alone, not the source around it, keys them.
  vendor = rustPlatform.importCargoLock {
    lockFile = sekka + "/desktop/aizome/Cargo.lock";
    outputHashes = import ./cargo-output-hashes.nix;
  };

  # The real source of `crates` (directories under Sekka's root) and every
  # first-party crate they reach, plus `extras`; stubs for the rest.
  withCrates =
    {
      crates,
      extras ? [ ],
    }:
    let
      real = lib.fileset.toSource {
        root = sekka;
        fileset = lib.fileset.unions (map (dir: sekka + "/${dir}") (closure crates) ++ extras);
      };
    in
    runCommand "aizome-source" { } ''
      cp -r ${stubs} $out
      chmod -R u+w $out
      cp -r ${real}/. $out/
      chmod -R u+w $out
    '';

  # A dependency build's install phase: its target directory without the
  # first-party stubs, which the package must compile from its own source.
  saveTarget = ''
    runHook preInstall
    for profile in target/*/ target/*/*/; do
      [ -d "$profile/.fingerprint" ] || continue
      for name in ${lib.escapeShellArgs names}; do
        rm -rf "$profile".fingerprint/"$name"-* "$profile"build/"$name"-* \
          "$profile"deps/lib"''${name//-/_}"-* "$profile"deps/"''${name//-/_}"-*
      done
      # Binaries and libraries Cargo copied out of deps/.
      find "$profile" -maxdepth 1 -type f -delete
    done
    mkdir -p $out
    tar -I 'zstd -T0' -cf $out/target.tar.zst target
    runHook postInstall
  '';

  # Before both builds' Cargo commands. Stdenv seeds the C compilers with
  # the derivation's own hash, and bindgen's build script watches the flags
  # that carry it (BINDGEN_EXTRA_CLANG_ARGS): unchanged, every crate that
  # generates bindings would compile again in the package. A fixed seed
  # keeps C output as reproducible.
  sameEnvironment = ''
    if [[ ''${NIX_CFLAGS_COMPILE-} =~ -frandom-seed=([^ ]+) ]]; then
      seed=''${BASH_REMATCH[1]}
      NIX_CFLAGS_COMPILE=''${NIX_CFLAGS_COMPILE//-frandom-seed=$seed/-frandom-seed=aizome}
      if [[ -v BINDGEN_EXTRA_CLANG_ARGS ]]; then
        BINDGEN_EXTRA_CLANG_ARGS=''${BINDGEN_EXTRA_CLANG_ARGS//-frandom-seed=$seed/-frandom-seed=aizome}
      fi
    fi
  '';

  # Before a package's build: its dependency build's crates, with their
  # times kept. The vendored sources were copied into this build just now
  # (cargoSetupHook), so they are dated back to before those crates for
  # Cargo to find them fresh.
  restoreTarget = dependencies: ''
    tar -I zstd -xf ${dependencies}/target.tar.zst
    find "$NIX_BUILD_TOP/$(stripHash "$cargoDeps")" -exec touch -h -d @1 {} +
  '';
}
