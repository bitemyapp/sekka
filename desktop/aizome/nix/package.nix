# Independent runtime identities, with reusable Cargo dependency artifacts.
# The aggregate below contains symlinks and session metadata, never copied apps.
{
  pkgs,
  craneLib,
  fonts,
  hotaruFonts,
  version ? "0.1.0",
  rev ? null,
  root ? ../../..,
}:
let
  inherit (pkgs)
    lib
    stdenv
    rustPlatform
    runCommand
    ;
  path = relative: root + "/${relative}";
  sources = import ./package-sources.nix {
    inherit
      lib
      runCommand
      craneLib
      root
      ;
  };
  target = stdenv.hostPlatform.rust.rustcTarget;
  secretService = import ../../../nix/libsecret-dh-padding.nix { inherit (pkgs) lib libsecret; };
  select = names: lib.concatMapStringsSep " " (name: "-p ${lib.escapeShellArg name}") names;
  runtimeLibs = with pkgs; [
    libglvnd
    libpulseaudio
    secretService
    fontconfig
    libxkbcommon
    wayland
  ];
  vendor = craneLib.vendorCargoDeps {
    cargoLock = path "desktop/aizome/Cargo.lock";
    # Crane indexes Git hashes by locked source URL rather than name/version.
    outputHashes =
      let
        hashes = import ./cargo-output-hashes.nix;
        locked = (builtins.fromTOML (builtins.readFile (path "desktop/aizome/Cargo.lock"))).package;
      in
      builtins.listToAttrs (
        lib.concatMap (
          package:
          lib.optional (hashes ? "${package.name}-${package.version}") {
            name = package.source;
            value = hashes."${package.name}-${package.version}";
          }
        ) locked
      );
  };
  common = {
    inherit version;
    sourceRoot = "source/desktop/aizome";
    cargoVendorDir = vendor;
    nativeBuildInputs = with pkgs; [
      pkg-config
      makeWrapper
      ncurses
      jq
      rustPlatform.bindgenHook
      cmake
      shaderc
    ];
    buildInputs = with pkgs; [
      wayland
      libxkbcommon
      libinput
      udev
      seatd
      libdrm
      libgbm
      libglvnd
      mesa
      pixman
      fontconfig
      freetype
      dbus
      pam
      libpulseaudio
      secretService
      ffmpeg
      pipewire
      openblas
      vulkan-headers
      vulkan-loader
    ];
    dontUseCmakeConfigure = true;
    BLAS_INCLUDE_DIRS = "${lib.getDev pkgs.openblas}/include";
    OPENBLAS_PATH = "${lib.getLib pkgs.openblas}";
    GGML_NATIVE = "OFF";
    GGML_AVX = "OFF";
    GGML_AVX2 = "OFF";
    GGML_FMA = "OFF";
    GGML_F16C = "OFF";
    preBuild = ''
      ulimit -Ss 8192
      # Match Yukimi's bundled SQLite feature in dependencies and final builds.
      unset LIBSQLITE3_SYS_USE_PKG_CONFIG
      export RUSTC_WRAPPER=$PWD/scripts/rustc-wrapper
    '';
    # Incremental compressed archives share the base cache without retaining
    # copied target trees. Cargo may rewrite metadata even for unchanged
    # dependency filenames, so inherited artifacts must remain writable.
    installCargoArtifactsMode = "use-zstd";
    doCheck = false;
  };
  # Fonts are runtime assets/check inputs, never compiler-cache inputs.
  checkEnvironment = ''
    export XDG_CACHE_HOME=$TMPDIR/aizome-test-cache
    mkdir -p "$XDG_CACHE_HOME"
    export AIZOME_FONT_DIR=${fonts}
    export HOTARU_FONT_DIR=${hotaruFonts}
    export LD_LIBRARY_PATH=${lib.makeLibraryPath runtimeLibs}
    export TZDIR=${pkgs.tzdata}/share/zoneinfo
    export FONTCONFIG_FILE=${
      pkgs.makeFontsConf {
        fontDirectories = [
          fonts
          pkgs.noto-fonts-cjk-sans
        ];
      }
    }
  '';
  commandNames = [
    "shell"
    "portal"
    "greeter"
    "power-meter"
    "awake"
    "network-agent"
    "ipc"
  ];
  libraryNames = names: lib.subtractLists names (sources.localClosure names);
  packageFlags =
    names:
    "${select names} ${lib.optionalString (builtins.elem "aizome-koe" names) "--features aizome-koe/openblas,aizome-koe/vulkan"}";
  runtimePackages = map (name: "aizome-${name}") (appNames ++ commandNames);
  # Warm actual app selections, whose feature unification differs from a
  # workspace-wide build, in one shared target directory.
  appFlags = map (name: packageFlags [ name ]) runtimePackages;
  dependencies =
    profile: flags:
    craneLib.buildDepsOnly (
      common
      // {
        pname = "aizome-${profile}";
        dummySrc = sources.dummy;
        preBuild = ''
          ulimit -Ss 8192
          unset LIBSQLITE3_SYS_USE_PKG_CONFIG
        '';
        buildPhaseCargoCommand = lib.concatMapStringsSep "\n" (
          selection: "cargo build --locked --offline --target ${target} --profile ${profile} ${selection}"
        ) flags;
      }
    );
  appDependencies = dependencies "release-lto" appFlags;
  # The compositor's Rust dylibs must keep their own non-LTO feature graph.
  compositorDependencies = dependencies "release" [ "-p aizome-comp -p aizome-comp-logic" ];
  libraryArtifacts =
    names:
    let
      libraries = libraryNames names;
    in
    if libraries == [ ] then
      appDependencies
    else
      craneLib.mkCargoDerivation (
        common
        // {
          pname = "aizome-libraries";
          src = sources.forPackages libraries;
          cargoArtifacts = appDependencies;
          # Keep the app itself inert, but compile its real local dependencies
          # in the same LTO/feature context as the final app. Selecting the
          # libraries themselves as Cargo roots changes that context.
          buildPhaseCargoCommand = "cargo build --locked --offline --target ${target} --profile release-lto ${packageFlags names}";
        }
      );
  meta = {
    homepage = "https://github.com/bitemyapp/sekka/tree/main/desktop/aizome";
    license = with lib.licenses; [
      mpl20
      bsd2
      mit
      asl20
    ];
    platforms = lib.platforms.linux;
  };
  mkComponent =
    {
      name,
      packages ? [ "aizome-${name}" ],
      profile ? "release-lto",
      flags ? "",
      install ? "",
      fixup ? "",
      tests ? "",
      checkPackages ? [ ],
      checkSetup ? "",
      postPatch ? "",
      extraInputs ? [ ],
      artifacts ? libraryArtifacts packages,
    }:
    craneLib.mkCargoDerivation (
      common
      // {
        pname = "aizome-${name}";
        inherit postPatch;
        src = sources.forPackages (packages ++ checkPackages);
        cargoArtifacts = artifacts;
        doInstallCargoArtifacts = false;
        # mkCargoDerivation leaves these to its caller. Debug/panic paths
        # must not keep the vendored sources or compiler in runtime closures.
        nativeBuildInputs = common.nativeBuildInputs ++ [
          craneLib.removeReferencesToRustToolchainHook
          craneLib.removeReferencesToVendoredSourcesHook
        ];
        disallowedReferences = [
          vendor
          artifacts
          rustPlatform.rust.rustc
        ];
        doCheck = tests != "";
        preCheck = lib.optionalString (tests != "") (checkEnvironment + checkSetup);
        nativeCheckInputs = [ pkgs.dbus ];
        buildInputs = common.buildInputs ++ extraInputs;
        buildPhaseCargoCommand = "cargo build --locked --offline --target ${target} --profile ${profile} ${select packages} ${flags}";
        checkPhaseCargoCommand = "cargo test --locked --offline --target ${target} --profile ${profile} ${tests}";
        installPhaseCommand = ''
          rel=target/${target}/${profile}
          mkdir -p $out/bin
          ${install}
        '';
        postFixup = ''
          for bin in $out/bin/* $out/libexec/*; do
            if [ -f "$bin" ] && [ ! -L "$bin" ] && isELF "$bin"; then
              patchelf --add-rpath ${lib.makeLibraryPath runtimeLibs} "$bin"
            fi
          done
          ${fixup}
        '';
        meta = meta // {
          description = "Aizome ${name}";
          mainProgram =
            if name == "hotaru" then
              "hotaru"
            else if name == "compositor" then
              "aizome"
            else if name == "ipc" then
              "aizomectl"
            else
              "aizome-${name}";
        };
      }
    );
  appNames = map (name: lib.removePrefix "aizome-" name) (
    lib.filter (name: lib.hasPrefix "desktop/aizome/apps/" sources.packages.${name}) (
      builtins.attrNames sources.packages
    )
  );
  app =
    name:
    mkComponent {
      inherit name;
      flags = lib.optionalString (name == "koe") "--features aizome-koe/openblas,aizome-koe/vulkan";
      tests =
        if name == "yukimi" then
          "-p aizome-yukimi --lib --test headless --test look --test golden"
        else if name == "hako" then
          "-p aizome-hako --lib --test file_manager"
        else if name == "hotaru" then
          "-p aizome-hotaru"
        else
          "";
      postPatch = lib.optionalString (name == "hotaru") ''
        substituteInPlace apps/hotaru/tests/native.rs --replace-fail /bin/sh ${stdenv.shell}
      '';
      install = ''
        install -m755 $rel/aizome-${name} $out/bin/
        for entry in apps/${name}/data/*.desktop; do
          [ ! -e "$entry" ] || install -Dm644 "$entry" "$out/share/applications/$(basename "$entry")"
        done
        for unit in apps/${name}/data/*.service; do
          [ ! -e "$unit" ] || {
            install -Dm644 "$unit" "$out/lib/systemd/user/$(basename "$unit")"
            substituteInPlace "$out/lib/systemd/user/$(basename "$unit")" --replace-quiet @bindir@ "$out/bin"
          }
        done
        ${lib.optionalString (builtins.pathExists (path "desktop/aizome/icons/apps/org.aizome.${lib.toUpper (builtins.substring 0 1 name)}${builtins.substring 1 (-1) name}.svg")) ''
          install -Dm644 ${path "desktop/aizome/icons/apps/org.aizome.${lib.toUpper (builtins.substring 0 1 name)}${builtins.substring 1 (-1) name}.svg"} $out/share/icons/hicolor/scalable/apps/org.aizome.${
            lib.toUpper (builtins.substring 0 1 name)
          }${builtins.substring 1 (-1) name}.svg
        ''}
        ${lib.optionalString (name == "nisshi") ''
          install -Dm755 $rel/aizome-nisshi-helper $out/libexec/aizome-nisshi-helper
          mkdir -p $out/share/polkit-1/actions
          substitute apps/nisshi/data/org.aizome.Nisshi.policy.in $out/share/polkit-1/actions/org.aizome.Nisshi.policy \
            --subst-var-by helper $out/libexec/aizome-nisshi-helper
        ''}
        ${lib.optionalString (name == "yukimi") ''
          mkdir -p $out/libexec
          mv $out/bin/aizome-yukimi $out/libexec/
          substitute apps/yukimi/data/yukimi-launch $out/bin/aizome-yukimi \
            --subst-var-by bash ${pkgs.bash}/bin/bash \
            --subst-var-by app $out/libexec/aizome-yukimi \
            --subst-var-by weston ${pkgs.weston}/bin/weston
          chmod +x $out/bin/aizome-yukimi
          ln -s aizome-yukimi $out/bin/yukimi
        ''}
        ${lib.optionalString (name == "hotaru") ''
          ln -s aizome-hotaru $out/bin/hotaru
          install -Dm644 apps/hotaru/data/org.aizome.Hotaru.svg $out/share/icons/hicolor/scalable/apps/org.aizome.Hotaru.svg
          install -Dm644 ${path "apps/hotaru/upstream/LICENSE.ghostty"} $out/share/licenses/hotaru/Ghostty-MIT.txt
          install -m644 crates/hotaru-link-regex/vendor/oniguruma/LICENSE $out/share/licenses/hotaru/Oniguruma-BSD-2-Clause.txt
          install -Dm644 ${path "apps/hotaru/upstream/LICENSE.uucode"} $out/share/licenses/hotaru/uucode-MIT.txt
          install -Dm644 ${path "apps/hotaru/upstream/LICENSE.unicode"} $out/share/licenses/hotaru/Unicode-3.0.txt
          install -m644 ../../apps/hotaru/crates/hotaru-sprites/LICENSE.MPL-2.0 $out/share/licenses/hotaru/z2d-MPL-2.0.txt
          install -m644 ../../apps/hotaru/crates/hotaru-sprites/LICENSE.z2d $out/share/licenses/hotaru/z2d-NOTICES.txt
          install -m644 ../../apps/hotaru/crates/hotaru-sprites/LICENSE.zig $out/share/licenses/hotaru/Zig-MIT.txt
          install -m644 ../../apps/hotaru/vendor/simdutf8/LICENSE-MIT $out/share/licenses/hotaru/simdutf8-MIT.txt
          install -m644 ../../apps/hotaru/vendor/simdutf8/LICENSE-Apache $out/share/licenses/hotaru/simdutf8-Apache.txt
          mkdir -p $out/share/hotaru $out/share/terminfo
          cp -r ${path "apps/hotaru/resources"}/shell-integration $out/share/hotaru/
          chmod -R u+w $out/share/hotaru/shell-integration
          install -m644 ${path "apps/hotaru/resources/LICENSE-MIT"} $out/share/hotaru/LICENSE-MIT
          install -m644 ${path "apps/hotaru/resources/LICENSE-MIT"} $out/share/licenses/hotaru/Shell-integration-MIT.txt
          install -m644 ${path "apps/hotaru/resources/hotaru.terminfo"} $out/share/hotaru/hotaru.terminfo
          tic -x -o $out/share/terminfo $out/share/hotaru/hotaru.terminfo
        ''}
      '';
      fixup =
        lib.optionalString (name == "yukimi") ''
          wrapProgram $out/libexec/aizome-yukimi --set-default AIZOME_FONT_DIR ${fonts}
        ''
        + lib.optionalString (name == "hotaru") ''
          wrapProgram $out/bin/aizome-hotaru \
            --set-default AIZOME_FONT_DIR ${fonts} \
            --set-default HOTARU_FONT_DIR ${hotaruFonts} \
            --set-default HOTARU_RESOURCES_DIR $out/share/hotaru \
            --set-default HOTARU_TERMINFO_DIR $out/share/terminfo
        '';
    };
  apps = lib.genAttrs appNames app;
  split =
    value:
    let
      p = toString value;
      at = builtins.stringLength builtins.storeDir + 17;
    in
    [
      (builtins.substring 0 at p)
      (builtins.substring at (-1) p)
    ];
  buildEnv = builtins.toJSON {
    inherit target version;
    profile = "release";
    packages = [
      "aizome-comp"
      "aizome-comp-logic"
    ];
    rustc_store_path = split rustPlatform.rust.rustc;
    cargo_store_path = split rustPlatform.rust.cargo;
    rustc_version = rustPlatform.rust.rustc.version;
    source_rev = rev;
  };
  compositor = mkComponent {
    name = "compositor";
    checkPackages = [ "aizome-comp-process-tests" ];
    tests = "-p aizome-comp-process-tests -- --include-ignored";
    checkSetup = ''
      # Test the shipped compositor without rebuilding it with test features.
      # Long TMPDIR also exercises the runtime socket's short-directory guard.
      sessionTestTmp="$TMPDIR/session-environment-long-temporary-directory/session-environment-long-temporary-directory/session-environment-long-temporary-directory"
      mkdir -p "$sessionTestTmp"
      export TMPDIR="$sessionTestTmp"
      export AIZOME_TEST_COMPOSITOR="$PWD/target/${target}/release/aizome"
    '';
    packages = [
      "aizome-comp"
      "aizome-comp-logic"
    ];
    profile = "release";
    artifacts = compositorDependencies;
    install = ''
      mkdir -p $out/lib/aizome $out/share/aizome
      install -m755 $rel/aizome $out/bin/aizome
      install -m644 $rel/libaizome_comp_core.so $rel/libaizome_comp_logic.so $out/lib/aizome/
      install -m644 "$(rustc --print sysroot)"/lib/rustlib/${target}/lib/libstd-*.so $out/lib/aizome/
      echo '${buildEnv}' | jq --argjson core "$(LD_LIBRARY_PATH=$out/lib/aizome $out/bin/aizome --build-info)" \
        '. + {core: $core}' > $out/share/aizome/build-env.json
    '';
    fixup = ''
      for so in $out/lib/aizome/*.so; do patchelf --add-rpath '$ORIGIN' "$so"; done
      for f in $out/bin/aizome $out/lib/aizome/*.so; do
        patchelf --set-rpath "$(patchelf --print-rpath "$f" | tr ':' '\n' | grep -v -- '-rustc-' | paste -sd:)" "$f"
      done
      patchelf --add-rpath ${lib.makeLibraryPath runtimeLibs} $out/lib/aizome/libaizome_comp_core.so
    '';
  };
  commands = lib.genAttrs commandNames (
    name:
    mkComponent {
      inherit name;
      tests = lib.optionalString (name == "shell") "-p aizome-shell --lib";
      install =
        if name == "ipc" then
          ''
            install -m755 $rel/aizomectl $rel/aizome-dev $rel/aizome-share-picker $out/bin/
          ''
        else
          "install -m755 $rel/aizome-${name} $out/bin/";
    }
  );
  libraryChecks = craneLib.mkCargoDerivation (
    common
    // {
      pname = "aizome-library-checks";
      src = sources.forPackages [
        "aizome-kit"
        "aizome-sys"
      ];
      cargoArtifacts = libraryArtifacts [
        "aizome-kit"
        "aizome-sys"
      ];
      doInstallCargoArtifacts = false;
      doCheck = true;
      preCheck = checkEnvironment;
      nativeCheckInputs = [ pkgs.dbus ];
      buildPhaseCargoCommand = "true";
      checkPhaseCargoCommand = "cargo test --locked --offline --target ${target} --profile release-lto -p aizome-kit -p aizome-sys --lib";
      installPhaseCommand = "touch $out";
    }
  );
  session = runCommand "aizome-session-${version}" { } ''
    mkdir -p $out/bin $out/lib/systemd/user
    install -m755 ${path "desktop/aizome/session/aizome-session"} $out/bin/aizome-session
    substituteInPlace $out/bin/aizome-session --replace-fail 'flock -n 9' '${pkgs.util-linux}/bin/flock -n 9'
    install -Dm644 ${path "desktop/aizome/session/aizome.desktop"} $out/share/wayland-sessions/aizome.desktop
    for unit in ${path "desktop/aizome/session"}/*.service ${path "desktop/aizome/session"}/*.target ${path "desktop/aizome/session"}/*.path; do
      install -m644 "$unit" "$out/lib/systemd/user/$(basename "$unit")"
    done
    install -Dm644 ${path "desktop/aizome/session/aizome-portals.conf"} $out/share/xdg-desktop-portal/aizome-portals.conf
    install -Dm644 ${path "desktop/aizome/session/aizome.portal"} $out/share/xdg-desktop-portal/portals/aizome.portal
    mkdir -p $out/share/dbus-1/services
    cp ${path "desktop/aizome/session/dbus"}/*.service $out/share/dbus-1/services/
    for unit in $out/lib/systemd/user/* $out/share/dbus-1/services/*; do
      ${lib.concatMapStringsSep "\n"
        (name: ''
          substituteInPlace "$unit" --replace-quiet @bindir@/aizome-${name} ${commands.${name}}/bin/aizome-${name}
        '')
        [
          "portal"
          "network-agent"
          "awake"
          "shell"
        ]
      }
      substituteInPlace "$unit" --replace-quiet @bindir@/aizome-hako ${apps.hako}/bin/aizome-hako \
        --replace-quiet @systemctl@ ${pkgs.systemd}/bin/systemctl
      if grep -q @bindir@ "$unit"; then echo "unresolved executable in $unit" >&2; exit 1; fi
    done
    install -Dm644 ${path "desktop/aizome/etc/org.aizome.DisplayScales.xml"} $out/share/dbus-1/interfaces/org.aizome.DisplayScales.xml
    mkdir -p $out/share/accountsservice/interfaces
    ln -s ../../dbus-1/interfaces/org.aizome.DisplayScales.xml $out/share/accountsservice/interfaces/org.aizome.DisplayScales.xml
    install -Dm644 ${path "desktop/aizome/icons/apps/org.aizome.Toolchest.svg"} $out/share/aizome/mark.svg
    install -Dm644 ${path "desktop/aizome/icons/apps/org.aizome.Toolchest.svg"} $out/share/icons/hicolor/scalable/apps/org.aizome.Toolchest.svg
  '';
  aggregate = pkgs.symlinkJoin {
    name = "aizome-${version}";
    paths = [
      compositor
      session
    ]
    ++ builtins.attrValues commands
    ++ builtins.attrValues apps;
    postBuild = "test -e ${libraryChecks}";
    passthru = {
      providedSessions = [ "aizome" ];
      inherit buildEnv;
      yukimi = apps.yukimi;
      components = apps // commands // { inherit compositor session; };
      tests.libraries = libraryChecks;
    };
    meta = meta // {
      description = "Aizome, Sekka's desktop environment, on Sukumo, its native Rust toolkit";
      mainProgram = "aizome";
    };
  };
in
apps
// commands
// {
  inherit
    compositor
    session
    aggregate
    sources
    ;
}
