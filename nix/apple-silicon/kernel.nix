# linux-aurora, Omarchy's Apple Silicon kernel, as its release
# aurora-2026.10.17.20 built it: the same source archive and configuration,
# compiled with Clang 22 and LLD (LLVM=1) as its recipe does. A 14-inch M3 Pro
# (J514S) owner reported this release's kernel, m1n1 and Mesa working
# (https://github.com/omacom/linux-aurora/issues/138); the commits below are
# that release's mesa-matched-stack.json.
#
# linux-aurora.config is the recipe's `config`, byte for byte (sha256
# c811698c5242d42850bef6669f29cd7b485e997d3ac1a423e2d3cbbb3fdc19b8). Only its
# LOCALVERSION, which names Arch's builder, is replaced here; `make oldconfig`
# then adapts the compiler-specific symbols as the recipe's olddefconfig does.
{
  lib,
  stdenv,
  fetchurl,
  linuxKernel,
  linuxPackagesFor,
  overrideCC,
  llvmPackages_22,
  writeText,
}:
let
  release = "aurora-2026.10.17.20";
  commit = "c46d7dcdb8712ea9aeb0ce47b439ffae76371e55";
  version = "7.1.12";
  localVersion = "-aurora20";

  original = builtins.readFile ./linux-aurora.config;
  from = ''CONFIG_LOCALVERSION="-ARCH"'';
  text =
    assert lib.assertMsg (lib.hasInfix from original) "linux-aurora.config no longer sets ${from}";
    builtins.replaceStrings [ from ] [ ''CONFIG_LOCALVERSION="${localVersion}"'' ] original;

  # As the kernel builder's own readConfig does for a configuration file in the
  # source tree, without importing from a derivation.
  config = lib.listToAttrs (
    lib.concatMap (
      line:
      let
        m = builtins.match "(CONFIG_[A-Za-z0-9_]+)=(.*)" line;
      in
      lib.optional (m != null) (lib.nameValuePair (builtins.elemAt m 0) (builtins.elemAt m 1))
    ) (lib.splitString "\n" text)
  );

  # The recipe sets LLVM=1 and checks for clang and ld.lld 22.1.8.
  llvmStdenv = overrideCC stdenv (
    llvmPackages_22.clang.override { inherit (llvmPackages_22) bintools; }
  );

  kernel = linuxKernel.manualConfig {
    inherit version config;
    pname = "linux-aurora";
    modDirVersion = version + localVersion;
    src = fetchurl {
      # The archive the recipe builds (its sha256 493ed9fb…).
      url = "https://github.com/omacom/linux-aurora/releases/download/${release}/aurora-linux-${commit}.tar.gz";
      hash = "sha256-ST7Z+0qeJF/VmpKiOW08loUWvpMnSbgFl1eik4BlJQk=";
    };
    configfile = writeText "linux-aurora-${version}${localVersion}.config" text;
    stdenv = llvmStdenv;
    extraMakeFlags = [ "LLVM=1" ];
    # What NixOS asks of a kernel (systemd-boot needs the EFI stub); the
    # configuration has CONFIG_EFI_STUB and CONFIG_RUST.
    features = {
      efiBootStub = true;
      rust = true;
    };
    extraMeta = {
      description = "Omarchy's Apple Silicon kernel (linux-aurora ${release})";
      homepage = "https://github.com/omacom/linux-aurora";
      platforms = [ "aarch64-linux" ];
      branch = lib.versions.majorMinor version;
    };
  };
in
{
  inherit kernel;
  packages = linuxPackagesFor kernel;
  passthru = { inherit release commit; };
}
