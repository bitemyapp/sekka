{ pkgs }:
let
  tests = pkgs.rustPlatform.buildRustPackage {
    pname = "calamares-storage-tests";
    version = "0.1.0";
    src = pkgs.lib.fileset.toSource {
      root = ../rust;
      fileset = pkgs.lib.fileset.unions [
        ../rust/Cargo.toml
        ../rust/Cargo.lock
        ../rust/reference.nix
        ../rust/src
      ];
    };
    cargoLock.lockFile = ../rust/Cargo.lock;
    CALAMARES_TOOL_PATH = pkgs.lib.makeBinPath [
      pkgs.util-linux
      pkgs.e2fsprogs
      pkgs.btrfs-progs
      pkgs.xfsprogs
      pkgs.dosfstools
      pkgs.coreutils
    ];
    buildPhase = ''
      runHook preBuild
      cargo test --release --offline --locked --no-default-features --lib --no-run --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget}
      runHook postBuild
    '';
    doCheck = false;
    installPhase = ''
      mkdir -p $out/bin
      for test in target/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release/deps/calamares_nixos-*; do
        if [ -x "$test" ] && [ -f "$test" ]; then
          cp "$test" $out/bin/storage-tests
        fi
      done
      test -x $out/bin/storage-tests
    '';
  };
in
pkgs.testers.runNixOSTest {
  name = "calamares-filesystem-reformat";
  # Also runs inside the rootless ISO builder without nested KVM.
  requiredFeatures.kvm = false;
  nodes.machine = {
    boot.supportedFilesystems = [
      "ext4"
      "btrfs"
      "xfs"
      "vfat"
    ];
    boot.kernelModules = [ "loop" ];
    virtualisation.memorySize = 2048;
    environment.systemPackages = [ tests ];
  };
  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")
    print(machine.succeed("storage-tests filesystem::tests::real_reformat_mount_matrix --ignored --nocapture"))
  '';
}
