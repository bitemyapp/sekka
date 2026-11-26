# Linux's bluetooth module alone, with chanoma-bluetooth-keys.patch: a game
# controller's pairing key is kept even when the controller asks not to
# bond (8BitDo's in Switch mode do, then reconnect expecting the key, as a
# Switch keeps it). Without it such a controller works until it's switched
# off and never reconnects.
#
# Built from the kernel's own source, Nixpkgs patches, configuration and
# toolchain, and installed in lib/modules/<version>/updates, which depmod
# searches before the kernel's own directory: the kernel and every other
# module stay the cached Nixpkgs build (programs.aizome.chanoma.keepControllerKeys).
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
  modules = "$out/lib/modules/${kernel.modDirVersion}/updates";
in
kernel.stdenv.mkDerivation {
  pname = "linux-bluetooth-controller-keys";
  inherit (kernel) src version postPatch;
  # kernelPatches omits patches added by Nixpkgs' kernel builder (its
  # deterministic RANDSTRUCT seed patch among them): the actual ones.
  patches = kernel.patches ++ [ ./chanoma-bluetooth-keys.patch ];
  nativeBuildInputs = kernel.nativeBuildInputs ++ [ xz ];
  # Rust's library source, without which the configuration loses CONFIG_RUST.
  inherit (kernel.drvAttrs) env;

  configurePhase = ''
    runHook preConfigure
    cp ${kernel.configfile} .config
    chmod u+w .config
    # The configuration alone doesn't reproduce a randomized layout, and
    # this module has no kernel signing key.
    grep -qx 'CONFIG_BT=m' .config || { echo 'bluetooth is built in, not a module' >&2; exit 1; }
    grep -qx 'CONFIG_RANDSTRUCT_NONE=y' .config || { echo 'randomized structure layouts' >&2; exit 1; }
    if grep -qx 'CONFIG_MODULE_SIG_FORCE=y' .config; then echo 'signed modules required' >&2; exit 1; fi
    make ${flags} olddefconfig
    # Any other configuration would build a module this kernel can't use.
    cmp .config ${kernel.configfile}
    cmp .config ${build}/.config
    make ${flags} -j$NIX_BUILD_CORES modules_prepare
    test "$(cat include/config/kernel.release)" = ${kernel.modDirVersion}
    # The kernel's exported symbols, and its BTF as the module's base.
    cp ${build}/Module.symvers .
    ln -s ${build}/vmlinux vmlinux
    runHook postConfigure
  '';

  buildPhase = ''
    runHook preBuild
    make ${flags} -j$NIX_BUILD_CORES M=net/bluetooth modules
    runHook postBuild
  '';

  # bluetooth.ko alone: the rest of net/bluetooth (rfcomm, hidp, …) stays
  # the kernel's own, built against the same symbols.
  installPhase = ''
    runHook preInstall
    mkdir -p ${modules}
    strip --strip-debug net/bluetooth/bluetooth.ko
    xz --check=crc32 --lzma2=dict=1MiB -c net/bluetooth/bluetooth.ko > ${modules}/bluetooth.ko.xz
    runHook postInstall
  '';

  meta = {
    description = "Linux's bluetooth module, keeping the keys of game controllers that ask not to bond";
    license = lib.licenses.gpl2Only;
    platforms = lib.platforms.linux;
  };
}
