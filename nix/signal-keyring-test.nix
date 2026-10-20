# Exercise the packaged launchers, including an absolute desktop Exec path.
{ pkgs }:
let
  package = pkgs.runCommand "signal-launcher-fixture" { } ''
    mkdir -p $out/bin $out/share/applications
    cat > $out/bin/signal-desktop <<'EOF'
    #!${pkgs.python3}/bin/python3
    import json, sys
    print(json.dumps(sys.argv[1:]))
    EOF
    chmod +x $out/bin/signal-desktop
    cat > $out/share/applications/signal.desktop <<EOF
    [Desktop Entry]
    Name=Signal
    Type=Application
    Exec=$out/bin/signal-desktop %U
    EOF
  '';
  withKeyring = import ../installer/genkan/rust/src/keyring.nix {
    inherit pkgs;
    inherit (pkgs) lib;
  };
  wrapped = withKeyring { id = "signal"; } package;
in
pkgs.runCommand "signal-keyring-check" { nativeBuildInputs = [ pkgs.python3 ]; } ''
  python3 ${../tests/test_signal_keyring.py} ${wrapped} ${package}
  touch $out
''
