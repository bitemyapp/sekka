# OpenBIOS, PCSX-Redux's free replacement for the PlayStation's BIOS (MIT,
# from its `nugget` repository), for Chanoma's PlayStation games: the
# console's own BIOS is Sony's and can't be shipped. PCSX-ReARMed takes any
# 512 KiB image that looks like a BIOS from the cores' system folder, so
# Chanoma links this one in (module.nix's programs.aizome.chanoma.bios).
#
# Built with Nixpkgs' MIPS cross compiler, unwrapped: the wrapper adds
# -march=mips32r2, and the PlayStation's R3000 is MIPS I. Linux's binutils
# call little-endian MIPS ELF elf32-tradlittlemips, where OpenBIOS's
# Makefile expects a bare-metal toolchain's elf32-littlemips.
{
  lib,
  stdenvNoCC,
  fetchFromGitHub,
  pkgsCross,
}:
let
  cross = pkgsCross.mipsel-linux-gnu.buildPackages;
  cc = cross.gcc.cc;
  binutils = cross.binutils-unwrapped;
  target = "mipsel-unknown-linux-gnu";
in
stdenvNoCC.mkDerivation {
  pname = "psx-openbios";
  version = "0-unstable-2026-11-01";

  src = fetchFromGitHub {
    owner = "pcsx-redux";
    repo = "nugget";
    rev = "fd7dae22cbf3ca03ca85cc22ba9f772a961fdda5";
    hash = "sha256-mTj6IqPQ/iiiMFNGRws72ArfUYtzNy3ot5PWsnB7YZw=";
  };

  # OpenBIOS's Makefile names its tools mipsel-none-elf-*: these are the
  # cross compiler's, told where its assembler and linker are.
  preBuild = ''
    mkdir -p "$TMPDIR/toolchain"
    for t in gcc g++; do
      printf '#!%s\nexec %s/bin/%s-%s -B%s/bin/%s- "$@"\n' \
        ${stdenvNoCC.shell} ${cc} ${target} "$t" ${binutils} ${target} > "$TMPDIR/toolchain/mipsel-none-elf-$t"
      chmod +x "$TMPDIR/toolchain/mipsel-none-elf-$t"
    done
    ln -s ${cc}/bin/${target}-gcc-ar "$TMPDIR/toolchain/mipsel-none-elf-gcc-ar"
    for t in objcopy objdump ld as ar nm ranlib size readelf; do
      ln -s ${binutils}/bin/${target}-$t "$TMPDIR/toolchain/mipsel-none-elf-$t"
    done
    export PATH="$TMPDIR/toolchain:${binutils}/bin:$PATH"
  '';

  makeFlags = [
    "-C"
    "openbios"
    "PREFIX=mipsel-none-elf"
    "FORMAT=elf32-tradlittlemips"
  ];
  enableParallelBuilding = true;

  installPhase = ''
    runHook preInstall
    install -Dm444 openbios/openbios.bin $out/share/chanoma/bios/openbios.bin
    install -Dm444 LICENSE $out/share/licenses/psx-openbios/LICENSE
    runHook postInstall
  '';

  meta = {
    description = "OpenBIOS, a free replacement for the PlayStation's BIOS, from PCSX-Redux";
    homepage = "https://github.com/pcsx-redux/nugget/tree/main/openbios";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
  };
}
