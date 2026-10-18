# The M3 Pro's Mesa from Omarchy's release aurora-2026.10.17.20, built as its
# recipes (release20/native-pro, release20/pro-vulkan) build it: native OpenGL
# for the T6030's G15S GPU and Honeykrisp Vulkan, two trees of
# iconidentify/mesa-omacom (26.1.4). Neither replaces the system's Mesa. As in
# Omarchy, a session points at them only once the kernel and the GPU are the
# ones they were made for; until then the system's Mesa and llvmpipe draw.
#
# Nixpkgs' Mesa (26.2) supplies the dependencies. The recipes'
# choices replace its driver selection and options: no OpenCL, OpenGL only
# through asahi and softpipe, debug assertions kept, MESA_DYNAMIC_TLS.
{
  lib,
  mesa,
  mesa-libclc,
  fetchFromGitHub,
  runCommand,
}:
let
  release = "aurora-2026.10.17.20";

  # The recipes build LLVM 22.1.8's libclc for Mesa. Nixpkgs has Mesa's own
  # build of it (its `spirv*-mesa3d-.spv`) as mesa-libclc.pc, and Mesa 26.1
  # asks pkg-config for libclc.pc, of which it reads only libexecdir.
  libclc = runCommand "libclc-pc-${mesa-libclc.version}" { } ''
    mkdir -p $out/share/pkgconfig
    cat > $out/share/pkgconfig/libclc.pc <<EOF
    libexecdir=${mesa-libclc}/share/mesa-clc
    Name: libclc
    Description: ${mesa-libclc.name}, under the name Mesa 26.1 looks for
    Version: 0.2.0
    EOF
  '';

  # Options of Nixpkgs' Mesa that a recipe sets otherwise, or that 26.1 lacks.
  replaced = [
    "b_ndebug"
    "platforms"
    "gallium-drivers"
    "vulkan-drivers"
    "vulkan-layers"
    "glvnd"
    "gbm"
    "libgbm-external"
    "teflon"
    "clang-libdir"
    "gallium-rusticl"
    "gallium-rusticl-enable-drivers"
    "intel-rt"
    "tools"
    "video-codecs"
    # Build-time compilers, which link LLVM and Clang.
    "install-mesa-clc"
    "install-precomp-compiler"
  ];
  keep = flag: !lib.any (name: lib.hasPrefix "-D${name}=" flag) replaced;

  build =
    {
      pname,
      branch,
      rev,
      hash,
      profile,
      capabilities,
      mesonFlags,
      postFixup,
    }:
    mesa.overrideAttrs (old: {
      inherit pname postFixup;
      version = "26.1.4-${builtins.substring 0 12 rev}";
      src = fetchFromGitHub {
        owner = "iconidentify";
        repo = "mesa-omacom";
        inherit rev hash;
      };
      # Nixpkgs' OpenCL patch and its header check are for 26.2.
      patches = [ ];
      # 26.1 looks for Clang's libraries in LLVM's directory only; Nixpkgs
      # keeps them apart (26.2's clang-libdir option).
      postPatch = ''
        patchShebangs .
        substituteInPlace meson.build --replace-fail "dirs : llvm_libdir" \
          "dirs : [llvm_libdir, '${lib.getLib mesa.llvmPackages.clang-unwrapped}/lib']"
      '';
      # Nixpkgs enables every automatic feature; the recipes leave them to
      # Meson, which turns off what the chosen drivers cannot use (VA-API).
      mesonAutoFeatures = "auto";
      buildInputs = old.buildInputs ++ [ libclc ];
      mesonFlags =
        builtins.filter keep old.mesonFlags
        ++ [
          (lib.mesonOption "platforms" "x11,wayland")
          (lib.mesonBool "gallium-rusticl" false)
          (lib.mesonOption "video-codecs" "")
          (lib.mesonOption "tools" "")
          (lib.mesonEnable "llvm" true)
          (lib.mesonBool "draw-use-llvm" false)
          (lib.mesonEnable "mesa-clc" true)
          (lib.mesonEnable "precomp-compiler" true)
        ]
        ++ mesonFlags;
      env = (old.env or { }) // {
        NIX_CFLAGS_COMPILE = "-DMESA_DYNAMIC_TLS";
      };
      # Without OpenCL or Nixpkgs' tools for cross builds, everything goes in
      # $out (and debug symbols in $debug), as the recipes package it.
      outputs = [ "out" ];
      # The recipes refuse a package that needs LLVM or Clang at run time.
      disallowedRequisites = [
        (lib.getLib mesa.llvmPackages.libllvm)
        (lib.getLib mesa.llvmPackages.clang-unwrapped)
      ];
      postInstall = ''
        mkdir -p $out/share/mesa-m3
        printf '%s\n' ${lib.escapeShellArg profile} > $out/share/mesa-m3/profile
        printf '%s\n' ${lib.escapeShellArg "source=${rev}"} ${lib.escapeShellArgs capabilities} \
          > $out/share/mesa-m3/capabilities
      '';
      passthru = old.passthru // {
        inherit release branch rev;
      };
      meta = old.meta // {
        description = "Omarchy's ${pname} for the M3 Pro (T6030 G15S), from ${release}";
        homepage = "https://github.com/iconidentify/mesa-omacom";
        platforms = [ "aarch64-linux" ];
      };
    });
in
{
  native = build {
    pname = "mesa-m3-native-pro";
    branch = "fix/native-index-offset-integrated-20261013";
    rev = "d616d0650d6fa80f72a750a0121442ab917ed6cf";
    hash = "sha256-ReYS0AgaVAzZfgnWDe6geCnvJ5/bg28ZpcYJLIys3mU=";
    profile = "t6030-native-gl-legacy-hal";
    capabilities = [
      "opengl=native-experimental"
      "vulkan=legacy-honeykrisp-retained"
    ];
    mesonFlags = [
      (lib.mesonOption "gallium-drivers" "asahi,softpipe")
      (lib.mesonOption "vulkan-drivers" "")
      (lib.mesonOption "glx" "dri")
      (lib.mesonEnable "glvnd" true)
      (lib.mesonEnable "egl" true)
      (lib.mesonEnable "gbm" true)
      (lib.mesonEnable "gles1" true)
      (lib.mesonEnable "gles2" true)
    ];
    postFixup = ''
      for js in $out/share/glvnd/egl_vendor.d/*.json; do
        substituteInPlace "$js" --replace-fail '"libEGL_' '"'"$out/lib/libEGL_"
      done
      rm -f $out/lib/pkgconfig/dri.pc
      find $out -type d -empty -delete
    '';
  };

  vulkan = build {
    pname = "mesa-m3-pro-vulkan";
    branch = "review/aurora-mesa-foundation-20261012";
    rev = "b0f56952b7f06955b143fbfa90c07de4c5864964";
    hash = "sha256-0XdF85SmG6FzT6a4ULo2J2PMYrqmJPIbfmX22VtTs0w=";
    profile = "t6030-pro-vulkan-legacy-hal";
    capabilities = [
      "opengl=unavailable"
      "vulkan=experimental-g15s"
    ];
    mesonFlags = [
      (lib.mesonOption "gallium-drivers" "")
      (lib.mesonOption "vulkan-drivers" "asahi")
      (lib.mesonOption "glx" "disabled")
      (lib.mesonEnable "glvnd" false)
      (lib.mesonEnable "egl" false)
      (lib.mesonEnable "gbm" false)
      (lib.mesonEnable "gles1" false)
      (lib.mesonEnable "gles2" false)
      (lib.mesonBool "opengl" false)
    ];
    postFixup = ''
      find $out -type d -empty -delete
    '';
  };
}
