{ pkgs }:
let
  inherit (pkgs) lib;
  patched = import ./libsecret-dh-padding.nix { inherit (pkgs) lib libsecret; };
  # Build the upstream backend itself, with only the GnuTLS return value
  # controlled by the test. The original package must reproduce the defect.
  probe =
    package: expectFailure:
    package.overrideAttrs (old: {
      pname = "libsecret-dh-${if expectFailure then "control" else "fixed"}";
      postBuild = (old.postBuild or "") + ''
        $CC -I.. -I. ${../tests/libsecret-dh-padding.c} egg/libegg.a \
          $(pkg-config --cflags --libs glib-2.0 gnutls) \
          -Wl,--wrap=gnutls_privkey_derive_secret -o dh-padding-test
        ${
          if expectFailure then
            ''
              if ./dh-padding-test > probe.log 2>&1; then
                echo "The unpatched backend no longer reproduces the bug; remove the backport."
                exit 1
              fi
              grep DH_PADDING_MISSING probe.log
            ''
          else
            ''
              ./dh-padding-test
            ''
        }
      '';
      # This test exercises the backend, while the production package retains
      # its ordinary upstream suite and aizome-login tests the real D-Bus path.
      doCheck = false;
      installPhase = "mkdir -p $out; touch $out/passed";
      dontFixup = true;
      outputs = [ "out" ];
      postFixup = "";
      separateDebugInfo = false;
    });
in
# Once Nixpkgs reaches 0.21.8, `patched` is its own package and this checks
# that the release really pads; the control and the backport can then go.
pkgs.linkFarm "libsecret-dh-padding-tests" (
  {
    fixed = probe patched false;
  }
  // lib.optionalAttrs (lib.versionOlder pkgs.libsecret.version "0.21.8") {
    control = probe pkgs.libsecret true;
  }
)
