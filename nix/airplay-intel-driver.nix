{
  lib,
  stdenv,
  kernel,
  python3,
  trace ? false,
  ibssMonitor ? false,
}:
# An opt-in module for diagnosing BE211 AWDL with the exact running kernel.
# It does not replace the system kernel or alter regulatory policy.
stdenv.mkDerivation {
  pname = "sekka-airplay-iwlmld";
  version = kernel.version;
  src = kernel.src;
  unpackPhase = ''
    runHook preUnpack
    mkdir source
    tar -xf "$src" -C source --strip-components=1 \
      --wildcards '*/drivers/net/wireless/intel/iwlwifi'
    sourceRoot=source
    runHook postUnpack
  '';
  patches = [
    ./patches/iwlmld-injected-legacy-rate.patch
  ]
  ++ lib.optional trace ./patches/iwlmld-airplay-trace.patch
  ++ lib.optional ibssMonitor ./patches/iwlmld-ibss-monitor-trial.patch;
  nativeBuildInputs = kernel.moduleBuildDependencies ++ [ python3 ];
  dontPatchELF = true;
  hardeningDisable = [ "pic" ];
  enableParallelBuilding = true;
  buildPhase = ''
    runHook preBuild
    make -C ${kernel.dev}/lib/modules/${kernel.modDirVersion}/build \
      M="$PWD/drivers/net/wireless/intel/iwlwifi/mld" \
      -j"$NIX_BUILD_CORES" modules
    runHook postBuild
  '';
  doCheck = true;
  checkPhase = ''
    runHook preCheck
    python ${../tests/check_iwlmld_rates.py} drivers/net/wireless/intel/iwlwifi
    runHook postCheck
  '';
  installPhase = ''
    runHook preInstall
    install -Dm444 drivers/net/wireless/intel/iwlwifi/mld/iwlmld.ko \
      "$out/lib/modules/${kernel.modDirVersion}/updates/iwlmld.ko"
    runHook postInstall
  '';
  passthru.modDirVersion = kernel.modDirVersion;
  passthru.loadParameters = lib.optional ibssMonitor "airplay_ibss_monitor=1";
  meta = {
    description = "Intel MLD driver with corrected 5-GHz monitor injection rates";
    license = lib.licenses.gpl2Only;
    platforms = lib.platforms.linux;
  };
}
