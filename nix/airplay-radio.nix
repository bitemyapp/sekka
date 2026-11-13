{
  lib,
  rustPlatform,
  hostapd,
  iw,
  makeWrapper,
  peerClock ? true,
  pacedInjection ? false,
  pacingUs ? 2000,
  qosNoAck ? false,
  p2pGo ? false,
}:
assert builtins.elem pacingUs [
  1000
  2000
];
rustPlatform.buildRustPackage {
  pname = "sekka-airplay-radio";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ../services/airplay-radio;
    fileset = lib.fileset.unions [
      ../services/airplay-radio/Cargo.toml
      ../services/airplay-radio/Cargo.lock
      ../services/airplay-radio/src
    ];
  };
  cargoLock = {
    lockFile = ../services/airplay-radio/Cargo.lock;
    outputHashes."owly-radio-0.4.2" = "sha256-srNyP/FzZYNFuSECMfIvVssPVF84bIQBedxGP1yOTeY=";
  };
  env.OWLY_HOSTAPD = "${hostapd}/bin/hostapd";
  env.SEKKA_AIRPLAY_IW = "${iw}/bin/iw";
  env.SEKKA_AIRPLAY_GO_TRIAL = if p2pGo then "1" else "0";
  nativeBuildInputs = [ makeWrapper ];
  postPatch = ''
    # Patch the pinned dependency copy, never the Nix store or Cargo cache.
    awdlSource="$cargoDepsCopy/owly-awdl-0.4.2"
    radioSource="$cargoDepsCopy/owly-radio-0.4.2"
    ${lib.optionalString p2pGo ''patch -d "$radioSource" -p1 < ${./patches/owly-p2p-go-trial.patch}''}
    ${lib.optionalString peerClock ''patch -d "$awdlSource" -p1 < ${./patches/owly-peer-clock.patch}''}
    ${lib.optionalString qosNoAck ''patch -d "$awdlSource" -p1 < ${./patches/owly-qos-noack-trial.patch}''}
    ${lib.optionalString pacedInjection ''
      radioSource="$cargoDepsCopy/owly-radio-0.4.2"
      patch -d "$radioSource" -p1 < ${./patches/owly-injection-pacing-trial.patch}
      ${lib.optionalString (pacingUs != 2000) ''
        substituteInPlace "$radioSource/src/native.rs" --replace-fail \
          'from_micros(2000)' 'from_micros(${toString pacingUs})'
      ''}
    ''}
  '';
  postCheck = ''
    # Exercise the patched protocol itself, including independent clocks.
    cargo test --offline --manifest-path "$awdlSource/Cargo.toml" --features driver
    ${lib.optionalString (pacedInjection || p2pGo) ''
      # importCargoLock vendors sibling crates with versioned directory names.
      # A direct library test must use that name for its local path dependency.
      substituteInPlace "$radioSource/Cargo.toml" --replace-fail \
        'path = "../owly-awdl"' 'path = "../owly-awdl-0.4.2"'
      cargo test --offline --manifest-path "$radioSource/Cargo.toml" --lib
      ${lib.optionalString p2pGo ''cargo test --offline --manifest-path "$radioSource/Cargo.toml" --test injection connected_go''}
    ''}
  '';
  postFixup = ''
    # Use the host's nmcli with its NetworkManager daemon. Only root-owned
    # system paths, never a caller's PATH, reach the privileged radio code.
    wrapProgram $out/bin/sekka-airplay-radio \
      --set PATH /run/current-system/sw/bin:/run/wrappers/bin
  '';
  meta = {
    description = "Bounded AWDL radio sessions for Sekka AirPlay";
    license = [
      lib.licenses.mit
      lib.licenses.asl20
    ];
    platforms = lib.platforms.linux;
    mainProgram = "sekka-airplay-radio";
  };
}
