{ pkgs }:
let
  # Release archive is independently pinned, not a moving Docker tag. Toolchain
  # identity remains the root flake's native/Rust closure, not this source tree.
  nativelink = pkgs.stdenvNoCC.mkDerivation {
    pname = "nativelink";
    version = "1.8.3";
    src = pkgs.fetchurl {
      url = "https://github.com/TraceMachina/nativelink/releases/download/v1.8.3/nativelink-1.8.3-x86_64-unknown-linux-musl.tar.gz";
      hash = "sha256-GmWWyW8FnU6Z9zUhZDIeHxzf9DUnVmethInoP34pd8U=";
    };
    sourceRoot = ".";
    installPhase = ''
      runHook preInstall
      install -Dm755 $(find . -type f -name nativelink) $out/bin/nativelink
      runHook postInstall
    '';
    meta.platforms = [ "x86_64-linux" ];
  };
  # Match nix/bazel/toolchains.nix:testEnv's root-locked pkgs.dbus. Only private
  # session-bus policy and service-owned identities enter the worker, never host
  # NSS, bus sockets, or homes. This identity also covers the /bin interpreter
  # and /usr/bin/env mounts, so changing either invalidates remote action keys.
  workerRuntime = pkgs.runCommand "sekka-build-worker-runtime" { } ''
    mkdir -p $out/bin $out/etc/dbus-1/session.d
    ln -s ${pkgs.dbus}/bin/* $out/bin/
    ln -s ${pkgs.bashInteractive}/bin/bash $out/bin/bash
    ln -s ${pkgs.coreutils}/bin/env $out/bin/env
    cp ${pkgs.dbus}/share/dbus-1/session.conf $out/etc/dbus-1/session.conf
    # Nixpkgs installs the primary policy in share/, but its daemon loads /etc/.
    # Remove the legacy include that would recurse after this relocation.
    substituteInPlace $out/etc/dbus-1/session.conf \
      --replace-fail '<include ignore_missing="yes">/etc/dbus-1/session.conf</include>' ""
    cat > $out/etc/passwd <<'EOF'
    root:x:0:0:root:/nonexistent:/bin/sh
    sekka-builder:x:30501:30501:Sekka build worker:/scratch:/bin/sh
    EOF
    cat > $out/etc/group <<'EOF'
    root:x:0:
    sekka-builder:x:30501:
    EOF
    cat > $out/etc/nsswitch.conf <<'EOF'
    passwd: files
    group: files
    hosts: files dns
    EOF
  '';
  controller = pkgs.writeShellApplication {
    name = "sekka-build-service";
    # Use the deployment host's installed nix-store, matching its daemon/config.
    runtimeInputs = [ pkgs.python3 pkgs.openssh pkgs.coreutils pkgs.docker-client ];
    text = ''
      export SEKKA_BUILD_SERVICE_INVENTORY=${./inventory.py}
      # Physical inventory closes these immutable tool inputs through the host's
      # nix-store. compsize stays an explicit CLI input, never a hidden default;
      # immutable payload measurements exclude all observer/tool overhead.
      export SEKKA_BUILD_SERVICE_INVENTORY_INPUTS="${pkgs.python3} ${pkgs.openssh} ${pkgs.coreutils} ${pkgs.docker-client} ''${BASH_SOURCE[0]}"
      export SEKKA_BUILD_SERVICE_WORKER_RUNTIME=${workerRuntime}
      export SEKKA_CAS_DEDUPE=${../../build/buck2/cas_dedupe.py}
      exec python3 ${../../scripts/build-service} "$@"
    '';
  };
  inventoryTest = pkgs.runCommand "sekka-build-service-inventory-test" { } ''
    cp ${./inventory.py} inventory.py
    cp ${./test_inventory.py} test_inventory.py
    ${pkgs.python3}/bin/python3 -m unittest -v test_inventory
    touch $out
  '';
  qualificationPython = pkgs.python3.withPackages (p: [ p.grpcio ]);
  qualifier = pkgs.writeShellApplication {
    name = "sekka-build-service-qualify";
    text = ''exec ${qualificationPython}/bin/python3 ${../../scripts/build-service-qualify} "$@"'';
  };
in
{
  inherit nativelink controller inventoryTest qualifier qualificationPython workerRuntime;
}
