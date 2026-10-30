# SPDX-License-Identifier: GPL-3.0-or-later
# Linux's xe module alone, with dpt-cursor.patch, built from the kernel's
# own source, Nixpkgs patches, configuration and toolchain. It is installed
# in lib/modules/<version>/updates, which depmod searches before the
# kernel's own directory, so the kernel and every other module stay the
# cached Nixpkgs build.
{
  lib,
  kernel,
  xz,
}:
let
  build = "${kernel.dev}/lib/modules/${kernel.modDirVersion}/build";
  # Nixpkgs' flags for the kernel's toolchain, without its separate build
  # directory and its module signing. The kernel doesn't require signed
  # modules; an unsigned one taints it, as any module built apart does.
  flags = lib.escapeShellArgs (
    builtins.filter (f: !(lib.hasPrefix "O=" f || lib.hasPrefix "cmd_sign=" f)) kernel.makeFlags
  );
in
kernel.stdenv.mkDerivation {
  pname = "linux-xe-dpt-cursor";
  inherit (kernel) src version postPatch;
  patches = map (p: p.patch) kernel.kernelPatches ++ [ ./dpt-cursor.patch ];
  nativeBuildInputs = kernel.nativeBuildInputs ++ [ xz ];
  # Rust's library source, without which the configuration loses CONFIG_RUST.
  inherit (kernel.drvAttrs) env;

  configurePhase = ''
    runHook preConfigure
    cp ${kernel.configfile} .config
    chmod u+w .config
    make ${flags} olddefconfig
    # Any other configuration would build a module this kernel can't use.
    cmp .config ${kernel.configfile}
    make ${flags} -j$NIX_BUILD_CORES modules_prepare
    test "$(cat include/config/kernel.release)" = ${kernel.modDirVersion}
    # The kernel's exported symbols, and its BTF as the module's base.
    cp ${build}/Module.symvers .
    ln -s ${build}/vmlinux vmlinux
    runHook postConfigure
  '';

  buildPhase = ''
    runHook preBuild
    make ${flags} -j$NIX_BUILD_CORES M=drivers/gpu/drm/xe modules
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    mkdir -p $out/lib/modules/${kernel.modDirVersion}/updates
    # As the kernel's own modules are installed: no debug sections (BTF stays).
    strip --strip-debug drivers/gpu/drm/xe/xe.ko
    xz --check=crc32 --lzma2=dict=1MiB -c drivers/gpu/drm/xe/xe.ko \
      > $out/lib/modules/${kernel.modDirVersion}/updates/xe.ko.xz
    runHook postInstall
  '';

  meta = {
    description = "Linux's xe module with Sekka's fix for slow page flips to tiled framebuffers";
    license = lib.licenses.gpl2Only;
    platforms = [ "x86_64-linux" ];
  };
}
