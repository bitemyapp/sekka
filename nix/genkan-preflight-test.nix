# Store changes and target disks are confined to a disposable VM. Preparation
# must skip store-wide content scans while retaining disk/configuration guards.
{
  pkgs,
  helper,
  nixPackage,
}:
pkgs.testers.runNixOSTest {
  name = "genkan-preflight";
  nodes.machine = {
    virtualisation.diskImage = null;
    # Preparation reaches the deliberately absent template without networking.
    virtualisation.vlans = [ ];
    virtualisation.memorySize = 2048;
    virtualisation.writableStore = true;
    virtualisation.emptyDiskImages = [
      {
        size = 40960;
        driveConfig.deviceExtraOpts.serial = "RESPIN_TEST_ONLY";
      }
    ];
    boot.nixStoreMountOpts = pkgs.lib.mkForce [
      "nosuid"
      "nodev"
    ];
    boot.kernelModules = [
      "loop"
      "isofs"
    ];
    environment.systemPackages = [
      helper
      pkgs.xorriso
    ];
    nix.package = nixPackage;
    nix.settings.experimental-features = [
      "nix-command"
      "flakes"
    ];
    environment.etc."calamares-nixos/settings.json".text = builtins.toJSON {
      template_dir = "/run/no-template";
      zoneinfo = "${pkgs.tzdata}/share/zoneinfo";
      state_version = "26.11";
      kernel = "latest";
      test_diagnostics = true;
    };
  };
  testScript = ''
    import json, shlex

    machine.start()
    machine.wait_for_unit("multi-user.target")
    machine.succeed("mkdir /run/media /iso; echo fixture > /run/media/marker")
    machine.succeed("xorriso -as mkisofs -quiet -o /run/media.iso /run/media")
    machine.succeed("mount -t iso9660 -o loop,ro /run/media.iso /iso")
    disks = json.loads(machine.succeed("calamares-nixos-helper discover"))
    disk = next(d["identity"] for d in disks if d["identity"]["serial"] == "RESPIN_TEST_ONLY")
    assert disk["path"] == "/dev/vda", disk
    before = machine.succeed("sha256sum /dev/vda", timeout=180)
    password = "Disposable-VM-Only-123!"
    request = dict(disk=disk, firmware="bios", filesystem="ext4",
                   hostname="preflight-test", username="installer", full_name="Installer Test",
                   password=password, locale="en_US.UTF-8", timezone="Etc/UTC",
                   keyboard="us", desktops=["aizome"], default_desktop="aizome", applications=[],
                   copy_wifi=False, wifi_profiles=[], allow_unfree=True, swap=False, tuning=True,
                   confirmation="ERASE /dev/vda")
    machine.succeed("printf %s " + shlex.quote(json.dumps(request)) + " > /run/request.json")
    machine.succeed("mkdir /run/payload; echo original > /run/payload/data")
    path = machine.succeed("nix-store --add /run/payload").strip()
    command = "calamares-nixos-helper install < /run/request.json"

    def configuration_refused():
        output = machine.fail(command, timeout=180)
        assert "Checking filesystem support" in output, output
        assert "No such file or directory" in output, output
        assert "Checking installation files" not in output, output
        assert "Verifying the prepared system" not in output, output
        assert '"stage":"verify-live-store"' not in output, output
        assert '"stage":"verify-system"' not in output, output
        assert '"kind":"progress"' not in output, output
        assert '"kind":"prepared"' not in output, output
        return output

    with subtest("good store reaches configuration without disk writes"):
        configuration_refused()
        machine.succeed(f"nix copy --no-check-sigs --to 'local?root=/run/good-copy' {path}")

    with subtest("Nix still detects corrupt contents when copying"):
        machine.succeed(f"chmod u+w {path}/data; echo modified > {path}/data; chmod u-w {path}/data")
        machine.succeed(f"nix build --offline --no-link {path}; cat {path}/data > /dev/null")
        error = machine.fail(f"nix copy --no-check-sigs --to 'local?root=/run/bad-copy' {path} 2>&1")
        assert "hash mismatch importing path" in error, error

    with subtest("preparation does not scan unrelated corrupt store contents"):
        output = configuration_refused()
        machine.succeed("printf %s " + shlex.quote(output) + " > /run/preflight.jsonl")

    with subtest("preparation does not scan unrelated missing registered paths"):
        machine.succeed(f"rm -rf {path}")
        configuration_refused()

    with subtest("changed target identity is still refused before configuration"):
        stale = dict(request, disk=dict(disk, serial="STALE-IDENTITY"))
        machine.succeed("printf %s " + shlex.quote(json.dumps(stale)) + " > /run/request.json")
        output = machine.fail(command, timeout=180)
        assert "Disk identity changed" in output, output
        assert "Checking filesystem support" not in output, output
        assert '"kind":"progress"' not in output, output

    assert machine.succeed("sha256sum /dev/vda", timeout=180) == before, "the target disk changed"
    machine.copy_from_machine("/run/preflight.jsonl")
    machine.shutdown()
  '';
}
