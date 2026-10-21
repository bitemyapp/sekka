# Real guest mounts exercise the production guard, without invoking disk writes.
{ pkgs }:
let
  probe = pkgs.rustPlatform.buildRustPackage {
    pname = "genkan-live-guard-probe";
    version = "0.1.0";
    src = pkgs.lib.fileset.toSource {
      root = ../installer/genkan/rust;
      fileset = pkgs.lib.fileset.unions [
        ../installer/genkan/rust/Cargo.toml
        ../installer/genkan/rust/Cargo.lock
        ../installer/genkan/rust/reference.nix
        ../installer/genkan/rust/src
      ];
    };
    cargoLock.lockFile = ../installer/genkan/rust/Cargo.lock;
    CALAMARES_TOOL_PATH = pkgs.lib.makeBinPath [
      pkgs.util-linux
      pkgs.coreutils
    ];
    postPatch = ''
      mkdir -p src/bin
      cp ${../installer/genkan/nix/live-guard-probe.rs} src/bin/live-guard-probe.rs
    '';
    buildPhase = ''
      runHook preBuild
      cargo build --release --offline --locked --no-default-features --bin live-guard-probe --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget}
      runHook postBuild
    '';
    doCheck = false;
    installPhase = ''
      mkdir -p $out/bin
      cp target/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release/live-guard-probe $out/bin/
    '';
  };
  common = {
    boot.kernelModules = [
      "loop"
      "squashfs"
      "isofs"
    ];
    virtualisation.memorySize = 1024;
    virtualisation.writableStore = false;
    environment.systemPackages = [
      probe
      pkgs.squashfsTools
      pkgs.xorriso
    ];
    users.users.alice.isNormalUser = true;
  };
in
pkgs.testers.runNixOSTest {
  name = "genkan-live-media-guard";
  nodes = {
    ram = {
      imports = [ common ];
      virtualisation.diskImage = null;
      boot.kernelParams = [ "copytoram" ];
    };
    ordinary = {
      imports = [ common ];
      virtualisation.diskImage = null;
    };
    installed = {
      imports = [ common ];
      boot.kernelParams = [ "copytoram" ];
    };
  };
  testScript = ''
    def fixture(machine):
        machine.start()
        machine.wait_for_unit("multi-user.target")
        # /nix/store has its own bind; removing this name leaves the VM store
        # usable and avoids hiding a second mount beneath the fixture mount.
        machine.succeed("while mountpoint -q /nix/.ro-store; do umount /nix/.ro-store || exit 1; done")
        machine.succeed("mkdir -p /run/store-fixture /run/media-fixture /iso")
        machine.succeed("echo fixture > /run/store-fixture/marker")
        machine.succeed("mksquashfs /run/store-fixture /run/media-fixture/nix-store.squashfs -noappend -processors 1")
        machine.succeed("xorriso -as mkisofs -quiet -o /run/media.iso /run/media-fixture")

    def iso(machine):
        machine.succeed("mount -t iso9660 -o loop,ro /run/media.iso /iso")

    def ram_media(machine):
        machine.succeed("mount -t tmpfs tmpfs /iso")
        machine.succeed("cp /run/media-fixture/nix-store.squashfs /iso/")
        # The VM's /nix/store remains a separate bind of the original store.
        machine.succeed("mount -t squashfs -o loop,ro /iso/nix-store.squashfs /nix/.ro-store")

    with subtest("ordinary read-only ISO remains accepted"):
        fixture(ram)
        assert ram.succeed("findmnt -n -o FSTYPE --mountpoint /").strip() == "tmpfs"
        iso(ram)
        ram.succeed("live-guard-probe")
        error = ram.fail("su - alice -c live-guard-probe 2>&1")
        assert "requires root" in error, error
        ram.succeed("umount /iso")

    with subtest("genuine RAM media with its read-only squashfs is accepted"):
        ram_media(ram)
        print(ram.succeed("findmnt -J --mountpoint /iso"))
        print(ram.succeed("findmnt -J --mountpoint /nix/.ro-store"))
        ram.succeed("live-guard-probe")

    with subtest("a tmpfs with an absent or unrelated store image is refused"):
        ram.succeed("umount /nix/.ro-store")
        ram.fail("live-guard-probe")
        ram.succeed("mount -t squashfs -o loop,ro /run/media-fixture/nix-store.squashfs /nix/.ro-store")
        error = ram.fail("live-guard-probe 2>&1")
        assert "not backed by /iso/nix-store.squashfs" in error, error
        ram.succeed("umount /nix/.ro-store")
        ram.succeed("mount -t squashfs -o loop,ro /iso/nix-store.squashfs /nix/.ro-store")
        ram.succeed("live-guard-probe")
        ram.succeed("mv /iso/nix-store.squashfs /iso/unrelated.squashfs")
        error = ram.fail("live-guard-probe 2>&1")
        assert "not backed by /iso/nix-store.squashfs" in error, error
        ram.succeed("mv /iso/unrelated.squashfs /iso/nix-store.squashfs")
        ram.succeed("live-guard-probe")
        ram.shutdown()

    with subtest("temporary-root hosts without copytoram cannot use the RAM exception"):
        fixture(ordinary)
        ram_media(ordinary)
        error = ordinary.fail("live-guard-probe 2>&1")
        assert "only supported after a copytoram boot" in error, error
        ordinary.shutdown()

    with subtest("an installed root is refused with optical or RAM media attached"):
        fixture(installed)
        assert installed.succeed("findmnt -n -o FSTYPE --mountpoint /").strip() == "ext4"
        iso(installed)
        error = installed.fail("live-guard-probe 2>&1")
        assert "root is not temporary" in error, error
        installed.succeed("umount /iso")
        ram_media(installed)
        error = installed.fail("live-guard-probe 2>&1")
        assert "root is not temporary" in error, error
        installed.shutdown()
  '';
}
