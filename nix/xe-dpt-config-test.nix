# Reject configurations whose structure layout or signature requirements
# cannot be reproduced by the separately built module, before compiling it.
{ pkgs }:
let
  kernel = pkgs.linuxPackages_latest.kernel;
  replacement = pkgs.callPackage ../installer/genkan/rust/system/xe-dpt/package.nix {
    inherit kernel;
  };
  checkConfig = ../installer/genkan/rust/system/xe-dpt/check-config.sh;
in
assert
  replacement.patches
  == kernel.patches ++ [ ../installer/genkan/rust/system/xe-dpt/dpt-cursor.patch ];
pkgs.runCommand "xe-dpt-config" { } ''
  cp ${kernel.configfile} stock.config
  bash ${checkConfig} stock.config

  reject() {
    if bash ${checkConfig} "$1" >diagnostic 2>&1; then
      echo "unexpectedly accepted $1" >&2
      exit 1
    fi
    grep -qF 'calamares.xeDpt.enable = false' diagnostic
  }

  for mode in FULL PERFORMANCE; do
    sed '/^CONFIG_RANDSTRUCT_NONE=y$/d' stock.config > randomized.config
    echo "CONFIG_RANDSTRUCT_$mode=y" >> randomized.config
    reject randomized.config
    grep -qF 'randomized structure layouts' diagnostic
  done

  cat stock.config > signed.config
  echo 'CONFIG_MODULE_SIG_FORCE=y' >> signed.config
  reject signed.config
  grep -qF 'unsigned' diagnostic

  sed 's/^CONFIG_DRM_XE=m$/CONFIG_DRM_XE=y/' stock.config > builtin.config
  reject builtin.config
  grep -qF 'CONFIG_DRM_XE=m' diagnostic

  touch $out
''
