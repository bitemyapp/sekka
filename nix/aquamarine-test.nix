{
  pkgs,
  aquamarine,
  hyprland,
}:
let
  # The same library without the null-connector fix, whether it came from
  # upstream (0.16.0 and newer) or from Sekka's backport (older releases).
  unfixedAquamarine = aquamarine.overrideAttrs (old: {
    postPatch = (old.postPatch or "") + ''
      patch -R -p1 < ${./patches/aquamarine-null-connector.patch}
    '';
  });
in
pkgs.gcc16Stdenv.mkDerivation {
  name = "aquamarine-null-connector-check";
  dontUnpack = true;
  nativeBuildInputs = [ pkgs.pkg-config ];
  buildInputs = aquamarine.buildInputs ++ [ aquamarine ];
  buildPhase = ''
    $CXX -std=c++23 -fno-access-control ${../tests/aquamarine-null-connector.cpp} \
      $(pkg-config --cflags --libs aquamarine hyprutils libdrm pixman-1) -o null-connector
  '';
  checkPhase = ''
    runHook preCheck
    ulimit -c 0
    ./null-connector
    echo 'Aquamarine handles null connectors'
    # Prove this test detects the original bug, using the same executable.
    echo 'Negative control: expecting SIGSEGV from Aquamarine without the fix'
    set +e
    LD_LIBRARY_PATH=${unfixedAquamarine}/lib ./null-connector
    status=$?
    set -e
    test "$status" -eq 139
    # Check the compositor's runtime binding, not merely its build inputs.
    ldd ${hyprland}/bin/.Hyprland-wrapped > hyprland-libraries
    grep -F '${aquamarine}/lib/libaquamarine.so' hyprland-libraries
    runHook postCheck
  '';
  doCheck = true;
  installPhase = ''
    mkdir -p $out
    cp hyprland-libraries $out/
  '';
}
