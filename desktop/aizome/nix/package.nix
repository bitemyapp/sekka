# Aizome: the compositor, the shell, aizomectl and the apps, from one
# checkout of the workspace.
#
# The workspace depends on the shared Yukimi backend by path, so the
# source here is a directory retaining Sekka's desktop/aizome and
# apps/yukimi paths. Sukumo, the toolkit, is inside desktop/aizome. Only
# Aizome, Sukumo and the shared Yukimi backend are copied; unrelated Sekka
# components stay outside this source.
#
# Two builds: the compositor with `release` (its core and logic are Rust
# dylibs, which can't take part in LTO; docs/reload.md), everything else
# with `release-lto`. The compositor build is one a developer can repeat
# from their own checkout (`aizome-dev`): same packages built together,
# same profile, target, flags and toolchain, recorded in
# share/aizome/build-env.json, so the logic library they build loads
# into the session this package runs.
{
  lib,
  stdenv,
  rustPlatform,
  runCommand,
  pkg-config,
  makeWrapper,
  jq,
  wayland,
  libxkbcommon,
  libinput,
  udev,
  seatd,
  libdrm,
  libgbm,
  libglvnd,
  mesa,
  pixman,
  fontconfig,
  freetype,
  dbus,
  libpulseaudio,
  libsecret,
  ffmpeg,
  pipewire,
  pam,
  systemd,
  util-linux,
  cmake,
  shaderc,
  openblas,
  vulkan-headers,
  vulkan-loader,
  src,
  yukimiSrc,
  fonts,
  bash,
  weston,
  tzdata,
  makeFontsConf,
  noto-fonts-cjk-sans,
  version ? "0.1.0",
  rev ? null,
}:
let
  source = runCommand "aizome-source" { } ''
    mkdir -p $out/desktop $out/apps
    cp -r ${src} $out/desktop/aizome
    cp -r ${yukimiSrc} $out/apps/yukimi
  '';
  # Libraries opened at run time with dlopen rather than linked: EGL and
  # GLES through libglvnd (the drivers come from /run/opengl-driver),
  # fontconfig for the text engine, and xkbcommon for smithay's keymaps.
  runtimeLibs = [
    libglvnd
    libpulseaudio
    libsecret
    fontconfig
    libxkbcommon
    wayland
  ];
  target = stdenv.hostPlatform.rust.rustcTarget;
  inherit (rustPlatform.rust) rustc cargo;
  # A store path written so the package doesn't keep it alive (Nix finds
  # references by their hash): its two halves, for aizome-dev to join.
  split =
    path:
    let
      p = toString path;
      at = builtins.stringLength builtins.storeDir + 17;
    in
    [
      (builtins.substring 0 at p)
      (builtins.substring at (builtins.stringLength p) p)
    ];
  buildEnv = builtins.toJSON {
    inherit target version;
    profile = "release";
    # Built together, so Cargo resolves the core's dependencies' features
    # the same way (another set changes the core's symbols).
    packages = [
      "aizome-comp"
      "aizome-comp-logic"
    ];
    rustc_store_path = split rustc;
    cargo_store_path = split cargo;
    rustc_version = rustc.version;
    source_rev = rev;
  };
in
rustPlatform.buildRustPackage {
  pname = "aizome";
  inherit version;
  # Share the native app without retaining the compositor and other apps.
  outputs = [
    "out"
    "yukimi"
  ];
  src = source;
  sourceRoot = "aizome-source/desktop/aizome";
  cargoLock.lockFile = "${src}/Cargo.lock";

  nativeBuildInputs = [
    pkg-config
    makeWrapper
    jq
    rustPlatform.bindgenHook
    # Koe's whisper.cpp (built by whisper-rs-sys through CMake) and its
    # Vulkan shaders.
    cmake
    shaderc
  ];
  buildInputs = [
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
    libsecret
    # aizome-media: FFmpeg (mpv's) and PipeWire.
    ffmpeg
    pipewire
    openblas
    vulkan-headers
    vulkan-loader
  ];

  # Koe's recognition: whisper.cpp with OpenBLAS, which picks kernels for
  # the processor at run time while the rest stays portable (no -march),
  # and Vulkan for any vendor's GPU (whisper.cpp uses the processor where
  # none starts). whisper-rs-sys passes GGML_* to CMake, which Cargo runs:
  # this isn't a CMake project.
  dontUseCmakeConfigure = true;
  BLAS_INCLUDE_DIRS = "${lib.getDev openblas}/include";
  OPENBLAS_PATH = "${lib.getLib openblas}";
  GGML_NATIVE = "OFF";
  GGML_AVX = "OFF";
  GGML_AVX2 = "OFF";
  GGML_FMA = "OFF";
  GGML_F16C = "OFF";

  postPatch = ''
    patchShebangs scripts
  '';

  buildPhase = ''
    runHook preBuild
    # -C prefer-dynamic for the compositor's core alone (scripts/rustc-wrapper).
    export RUSTC_WRAPPER=$PWD/scripts/rustc-wrapper
    flags=(-j "$NIX_BUILD_CORES" --target ${target} --offline)
    # Plain cargo for the compositor: nixpkgs' cargo-auditable wrapper is a
    # RUSTC_WORKSPACE_WRAPPER, whose path Cargo hashes into every workspace
    # crate's symbols, and a logic built from a checkout couldn't match.
    ${cargo}/bin/cargo build "''${flags[@]}" --release -p aizome-comp -p aizome-comp-logic
    # ggml's shader generator starts a thread per shader variant (over a
    # thousand), each with a stack the size of the soft limit: under
    # nix-daemon's 64 MiB its glslc forks fail on a loaded machine, and the
    # missing shaders only show as undefined symbols when Koe links.
    ulimit -Ss 8192
    cargo build "''${flags[@]}" --profile release-lto --workspace \
      --exclude aizome-comp --exclude aizome-comp-core --exclude aizome-comp-logic \
      --features aizome-koe/openblas,aizome-koe/vulkan
    runHook postBuild
  '';

  # Yukimi's headless interactions and rendering run without a desktop.
  # Session, D-Bus and GPU coverage remain in the component VMs.
  doCheck = true;
  checkPhase = ''
    runHook preCheck
    export AIZOME_FONT_DIR=${fonts}
    # Tests run before postFixup adds the libraries opened with dlopen.
    export LD_LIBRARY_PATH=${lib.makeLibraryPath runtimeLibs}
    # The brand's Japanese glyphs use the same fallback as the desktops.
    # A sandbox has no system font directories of its own.
    export FONTCONFIG_FILE=${
      makeFontsConf {
        fontDirectories = [
          fonts
          noto-fonts-cjk-sans
        ];
      }
    }
    export TZDIR=${tzdata}/share/zoneinfo
    cargo test --offline --target ${target} --profile release-lto -p aizome-yukimi -p aizome-kit --lib --test headless --test look --test golden
    # A successful spawn is not a successful application launch. Gate the
    # shipped shell on process outcomes and its failure/details interface.
    cargo test --offline --target ${target} --profile release-lto -p aizome-sys -p aizome-shell --lib
    runHook postCheck
  '';

  installPhase = ''
    runHook preInstall
    rel=target/${target}/release
    lto=target/${target}/release-lto
    mkdir -p $out/bin $out/lib/aizome $out/share/aizome
    install -Dm644 etc/org.aizome.DisplayScales.xml $out/share/dbus-1/interfaces/org.aizome.DisplayScales.xml
    mkdir -p $out/share/accountsservice/interfaces
    ln -s ../../dbus-1/interfaces/org.aizome.DisplayScales.xml $out/share/accountsservice/interfaces/org.aizome.DisplayScales.xml
    install -m755 $rel/aizome $out/bin/aizome
    # The core, the logic it loads, and the std they share.
    install -m644 $rel/libaizome_comp_core.so $rel/libaizome_comp_logic.so $out/lib/aizome/
    install -m644 "$(rustc --print sysroot)"/lib/rustlib/${target}/lib/libstd-*.so $out/lib/aizome/
    find $lto -maxdepth 1 -type f -executable ! -name '*.so' -exec install -m755 -t $out/bin {} +

    # How the compositor was built, for aizome-dev (and the ABI it checks).
    echo '${buildEnv}' | jq --argjson core "$(LD_LIBRARY_PATH=$out/lib/aizome $out/bin/aizome --build-info)" \
      '. + {core: $core}' > $out/share/aizome/build-env.json

    # Session: the login entry, the systemd user units and the portals.
    install -Dm755 session/aizome-session $out/bin/aizome-session
    substituteInPlace $out/bin/aizome-session \
      --replace-fail 'flock -n 9' '${util-linux}/bin/flock -n 9'
    install -Dm644 session/aizome.desktop $out/share/wayland-sessions/aizome.desktop
    shopt -s nullglob
    for unit in session/*.service session/*.target session/*.path apps/*/data/*.service; do
      install -Dm644 "$unit" "$out/lib/systemd/user/$(basename "$unit")"
      substituteInPlace "$out/lib/systemd/user/$(basename "$unit")" \
        --replace-quiet @bindir@ "$out/bin" \
        --replace-quiet @systemctl@ "${systemd}/bin/systemctl"
    done
    install -Dm644 session/aizome-portals.conf $out/share/xdg-desktop-portal/aizome-portals.conf
    # Aizome's own backend (file choosers), started by D-Bus as its unit.
    install -Dm644 session/aizome.portal $out/share/xdg-desktop-portal/portals/aizome.portal
    for service in session/dbus/*.service; do
      install -Dm644 "$service" "$out/share/dbus-1/services/$(basename "$service")"
      substituteInPlace "$out/share/dbus-1/services/$(basename "$service")" --replace-fail @bindir@ "$out/bin"
    done

    # Apps: desktop entries and icons.
    for entry in apps/*/data/*.desktop; do
      [ -e "$entry" ] && install -Dm644 "$entry" "$out/share/applications/$(basename "$entry")"
    done
    for icon in icons/apps/*.svg; do
      install -Dm644 "$icon" "$out/share/icons/hicolor/scalable/apps/$(basename "$icon")"
    done
    # Nisshi's reader, run as root only through pkexec under its own
    # polkit action, which names it by this path.
    mkdir -p $out/libexec $out/share/polkit-1/actions
    mv $out/bin/aizome-nisshi-helper $out/libexec/
    substitute apps/nisshi/data/org.aizome.Nisshi.policy.in $out/share/polkit-1/actions/org.aizome.Nisshi.policy \
      --subst-var-by helper $out/libexec/aizome-nisshi-helper
    # The Toolchest's mark, for the shell and the login screen.
    install -Dm644 icons/apps/org.aizome.Toolchest.svg $out/share/aizome/mark.svg
    mkdir -p $yukimi/bin $yukimi/libexec $yukimi/share/applications $yukimi/share/icons/hicolor/scalable/apps
    mv $out/bin/aizome-yukimi $yukimi/libexec/
    substitute apps/yukimi/data/yukimi-launch $yukimi/bin/aizome-yukimi \
      --subst-var-by bash ${bash}/bin/bash \
      --subst-var-by app "$yukimi/libexec/aizome-yukimi" \
      --subst-var-by weston ${weston}/bin/weston
    chmod +x $yukimi/bin/aizome-yukimi
    ln -s aizome-yukimi $yukimi/bin/yukimi
    mv $out/share/applications/org.aizome.Yukimi.desktop $yukimi/share/applications/
    mv $out/share/icons/hicolor/scalable/apps/org.aizome.Yukimi.svg $yukimi/share/icons/hicolor/scalable/apps/
    ln -s $yukimi/bin/aizome-yukimi $out/bin/aizome-yukimi
    ln -s $yukimi/share/applications/org.aizome.Yukimi.desktop $out/share/applications/
    ln -s $yukimi/share/icons/hicolor/scalable/apps/org.aizome.Yukimi.svg $out/share/icons/hicolor/scalable/apps/
    runHook postInstall
  '';

  postFixup = ''
    for bin in $out/bin/* $yukimi/libexec/*; do
      if [ ! -L "$bin" ] && isELF "$bin"; then
        patchelf --add-rpath ${lib.makeLibraryPath runtimeLibs} "$bin"
      fi
    done
    wrapProgram $yukimi/libexec/aizome-yukimi --set-default AIZOME_FONT_DIR ${fonts}
    # The compositor finds its core and std beside it (its own rpath says
    # $ORIGIN/../lib/aizome); they and the logic find each other there.
    for so in $out/lib/aizome/*.so; do
      patchelf --add-rpath '$ORIGIN' "$so"
    done
    # Not in the toolchain's sysroot, where the linker found std: that would
    # keep the compiler (a gigabyte) in the system's closure.
    for f in $out/bin/aizome $out/lib/aizome/*.so; do
      patchelf --set-rpath "$(patchelf --print-rpath "$f" | tr ':' '\n' | grep -v -- '-rustc-' | paste -sd:)" "$f"
    done
    patchelf --add-rpath ${lib.makeLibraryPath runtimeLibs} $out/lib/aizome/libaizome_comp_core.so
  '';

  passthru = {
    providedSessions = [ "aizome" ];
    inherit buildEnv;
  };

  meta = {
    description = "Aizome, Sekka's desktop environment, on Sukumo, its native Rust toolkit";
    homepage = "https://github.com/bitemyapp/sekka/tree/main/desktop/aizome";
    license = with lib.licenses; [
      mit
      asl20
    ];
    mainProgram = "aizome";
    platforms = lib.platforms.linux;
  };
}
