{
  pkgs,
  aquamarine,
  hyprland,
  unpatchedAquamarine,
}:
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
    echo 'Patched Aquamarine handles null connectors'
    # Prove this test detects the original bug, using the same executable.
    echo 'Negative control: expecting SIGSEGV from unpatched Aquamarine'
    set +e
    LD_LIBRARY_PATH=${unpatchedAquamarine}/lib ./null-connector
    status=$?
    set -e
    test "$status" -eq 139
    # Check the compositor's runtime binding, not merely its build inputs.
    ldd ${hyprland}/bin/.Hyprland-wrapped > hyprland-libraries
    grep -F '${aquamarine}/lib/libaquamarine.so' hyprland-libraries
    if grep -F '${unpatchedAquamarine}/' hyprland-libraries; then
      echo 'Hyprland still loads unpatched Aquamarine' >&2
      exit 1
    fi
    runHook postCheck
  '';
  doCheck = true;
  installPhase = ''
    mkdir -p $out
    cp hyprland-libraries $out/
  '';
}
