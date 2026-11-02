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
    import shlex

    def media_command(command):
        return ("nsenter --mount=/proc/$(systemctl show live-media-fixture.service "
                "-p MainPID --value)/ns/mnt -- sh -c " + shlex.quote(command))

    def succeed(machine, command):
        return machine.succeed(media_command(command))

    def fail(machine, command):
        return machine.fail(media_command(command))

    def fixture(machine):
        machine.start()
        machine.wait_for_unit("multi-user.target")
        # Keep these real kernel mounts private to the fixture. Unmounting
        # the VM's original /nix/.ro-store in PID 1's namespace makes systemd
        # asynchronously stop nix-store.mount and nix-daemon.socket; a later
        # remount then races systemd's teardown. The probe reads its own mount
        # namespace, exactly as it does on installation media.
        machine.succeed("systemd-run --unit=live-media-fixture -p PrivateMounts=yes sleep infinity")
        machine.wait_for_unit("live-media-fixture.service")
        succeed(machine, "while mountpoint -q /nix/.ro-store; do umount /nix/.ro-store || exit 1; done")
        succeed(machine, "mkdir -p /run/store-fixture /run/media-fixture /iso")
        succeed(machine, "echo fixture > /run/store-fixture/marker")
        succeed(machine, "mksquashfs /run/store-fixture /run/media-fixture/nix-store.squashfs -noappend -processors 1")
        succeed(machine, "xorriso -as mkisofs -quiet -o /run/media.iso /run/media-fixture")

    def iso(machine):
        succeed(machine, "mount -t iso9660 -o loop,ro /run/media.iso /iso")

    def ram_media(machine):
        succeed(machine, "mount -t tmpfs tmpfs /iso")
        succeed(machine, "cp /run/media-fixture/nix-store.squashfs /iso/")
        # The VM's /nix/store remains a separate bind of the original store.
        succeed(machine, "mount -t squashfs -o loop,ro /iso/nix-store.squashfs /nix/.ro-store")

    with subtest("ordinary read-only ISO remains accepted"):
        fixture(ram)
        assert succeed(ram, "findmnt -n -o FSTYPE --mountpoint /").strip() == "tmpfs"
        iso(ram)
        succeed(ram, "live-guard-probe")
        error = fail(ram, "su - alice -c live-guard-probe 2>&1")
        assert "requires root" in error, error
        succeed(ram, "umount /iso")

    with subtest("genuine RAM media with its read-only squashfs is accepted"):
        ram_media(ram)
        print(succeed(ram, "findmnt -J --mountpoint /iso"))
        print(succeed(ram, "findmnt -J --mountpoint /nix/.ro-store"))
        succeed(ram, "live-guard-probe")

    with subtest("a tmpfs with an absent or unrelated store image is refused"):
        succeed(ram, "umount /nix/.ro-store")
        fail(ram, "live-guard-probe")
        succeed(ram, "mount -t squashfs -o loop,ro /run/media-fixture/nix-store.squashfs /nix/.ro-store")
        error = fail(ram, "live-guard-probe 2>&1")
        assert "not backed by /iso/nix-store.squashfs" in error, error
        succeed(ram, "umount /nix/.ro-store")
        succeed(ram, "mount -t squashfs -o loop,ro /iso/nix-store.squashfs /nix/.ro-store")
        succeed(ram, "live-guard-probe")
        succeed(ram, "mv /iso/nix-store.squashfs /iso/unrelated.squashfs")
        error = fail(ram, "live-guard-probe 2>&1")
        assert "not backed by /iso/nix-store.squashfs" in error, error
        succeed(ram, "mv /iso/unrelated.squashfs /iso/nix-store.squashfs")
        succeed(ram, "live-guard-probe")
        ram.succeed("mountpoint -q /nix/.ro-store && systemctl is-active nix-daemon.socket")
        ram.shutdown()

    with subtest("temporary-root hosts without copytoram cannot use the RAM exception"):
        fixture(ordinary)
        ram_media(ordinary)
        error = fail(ordinary, "live-guard-probe 2>&1")
        assert "only supported after a copytoram boot" in error, error
        ordinary.shutdown()

    with subtest("an installed root is refused with optical or RAM media attached"):
        fixture(installed)
        assert succeed(installed, "findmnt -n -o FSTYPE --mountpoint /").strip() == "ext4"
        iso(installed)
        error = fail(installed, "live-guard-probe 2>&1")
        assert "root is not temporary" in error, error
        succeed(installed, "umount /iso")
        ram_media(installed)
        error = fail(installed, "live-guard-probe 2>&1")
        assert "root is not temporary" in error, error
        installed.shutdown()
  '';
}
