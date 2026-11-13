# AirPlay sender. Keep the protocol implementation pinned; its in-process
# receiver tests are component evidence, not certification of a real TV.
{
  lib,
  buildGoModule,
  fetchFromGitHub,
  pkg-config,
  makeWrapper,
  fdk_aac,
  gst_all_1,
  pipewire,
  pulseaudio,
  xrandr,
  dbus,
  xorg-server,
  makeFontsConf,
  dejavu_fonts,
}:
let
  plugins = with gst_all_1; [
    gstreamer
    gst-plugins-base
    gst-plugins-good
    gst-plugins-bad
    gst-plugins-ugly
    gst-libav
    pipewire
  ];
  pluginPath = lib.makeSearchPath "lib/gstreamer-1.0" (map lib.getLib plugins);
  testFonts = makeFontsConf { fontDirectories = [ dejavu_fonts ]; };
  runtimePath = lib.makeBinPath [
    gst_all_1.gstreamer
    pulseaudio
    xrandr
  ];
in
buildGoModule {
  pname = "doubletake";
  version = "0-unstable-2026-10-04";
  src = fetchFromGitHub {
    owner = "omarroth";
    repo = "doubletake";
    rev = "5e2f1cbe3e1070bcefec8e549c7baf884727bc00";
    hash = "sha256-ctD5KWrB9O3yA9UrrkQJ10ue2upjDh5gRbl7osAWGPA=";
  };
  vendorHash = "sha256-cgvY9MVGe8I3g3Ni2sGucTY6YCyPJ2YnoxxUaYfl1E4=";
  patches = [ ./patches/doubletake-daemon-shutdown.patch ];
  postPatch = ''
    substituteInPlace internal/airplay/capture_portal_test.go \
      --replace-fail '"--session", "--nofork"' '"--config-file=${dbus}/share/dbus-1/session.conf", "--nofork"'
  '';
  subPackages = [
    "cmd/doubletake"
    "cmd/doubletake-ctl"
    "cmd/doubletake-test-receiver"
  ];
  tags = [ "fdk_aac" ];
  env.CGO_ENABLED = 1;
  env.GST_PLUGIN_SYSTEM_PATH_1_0 = pluginPath;
  nativeBuildInputs = [
    pkg-config
    makeWrapper
  ];
  buildInputs = [ fdk_aac ];
  nativeCheckInputs = [
    dbus
    xorg-server
    gst_all_1.gstreamer
  ];
  preCheck = ''
    export GST_PLUGIN_SYSTEM_PATH_1_0=${pluginPath}
    export XDG_RUNTIME_DIR=$TMPDIR/runtime
    export XDG_CACHE_HOME=$TMPDIR/cache
    export FONTCONFIG_FILE=${testFonts}
    mkdir -m 700 -p "$XDG_RUNTIME_DIR"
    mkdir -p "$XDG_CACHE_HOME"
  '';
  # Include protocol, cancellation, pairing and encoder tests, not only cmd/.
  checkPhase = ''
    runHook preCheck
    go test -tags fdk_aac -timeout 240s ./...
    runHook postCheck
  '';
  postInstall = ''
    install -Dm644 man/man1/*.1 -t $out/share/man/man1
    install -Dm644 LICENSE COPYING.GPL -t $out/share/licenses/doubletake
    for exe in doubletake doubletake-test-receiver; do
      wrapProgram "$out/bin/$exe" \
        --prefix PATH : ${runtimePath} \
        --prefix GST_PLUGIN_SYSTEM_PATH_1_0 : ${pluginPath}
    done
  '';
  passthru = { inherit pluginPath; };
  meta = {
    description = "AirPlay screen mirroring sender with audio and PIN pairing";
    homepage = "https://github.com/omarroth/doubletake";
    license = lib.licenses.lgpl3Plus;
    platforms = lib.platforms.linux;
    mainProgram = "doubletake";
  };
}
