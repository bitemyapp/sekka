# Fault injection is confined to a disposable VM's writable store. The actual
# packaged helper must refuse even a confirmed request before touching its disk.
{
  pkgs,
  genkan,
  nixPackage,
}:
pkgs.testers.runNixOSTest {
  name = "genkan-store-integrity";
  nodes.machine = {
    virtualisation.diskImage = null;
    # Content verification must work offline; no inter-VM network is needed.
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
      genkan
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
                   hostname="integrity-test", username="installer", full_name="Installer Test",
                   password=password, locale="en_US.UTF-8", timezone="Etc/UTC",
                   keyboard="us", desktops=["aizome"], default_desktop="aizome", applications=[],
                   copy_wifi=False, wifi_profiles=[], allow_unfree=True, swap=False, tuning=True,
                   confirmation="ERASE /dev/vda")
    machine.succeed("printf %s " + shlex.quote(json.dumps(request)) + " > /run/request.json")
    machine.succeed("mkdir /run/payload; echo original > /run/payload/data")
    path = machine.succeed("nix-store --add /run/payload").strip()
    command = "calamares-nixos-helper install < /run/request.json"

    with subtest("good store reaches configuration without disk writes"):
        output = machine.fail(command, timeout=180)
        assert "Checking filesystem support" in output, output
        assert "No such file or directory" in output, output
        assert "Installation files could not be verified" not in output, output
        machine.succeed(f"nix copy --no-check-sigs --to 'local?root=/run/good-copy' {path}")

    with subtest("old validity and read checks accept corrupted bytes; Nix copy rejects them"):
        machine.succeed(f"chmod u+w {path}/data; echo modified > {path}/data; chmod u-w {path}/data")
        machine.succeed(f"nix build --offline --no-link {path}; cat {path}/data > /dev/null")
        error = machine.fail(f"nix copy --no-check-sigs --to 'local?root=/run/bad-copy' {path} 2>&1")
        assert "hash mismatch importing path" in error, error

    with subtest("confirmed install refuses corrupt bytes before evaluation or formatting"):
        output = machine.fail(command, timeout=180)
        assert "Installation files could not be verified" in output, output
        assert "target disk has not been changed" in output, output
        assert path in output, output
        assert "Checking filesystem support" not in output, output
        assert '"kind":"progress"' not in output, output
        assert '"kind":"prepared"' not in output, output
        machine.succeed("printf %s " + shlex.quote(output) + " > /run/corrupt-refused.jsonl")

    with subtest("missing registered files also fail closed"):
        machine.succeed(f"rm -rf {path}")
        output = machine.fail(command, timeout=180)
        assert "Installation files could not be verified" in output, output
        assert '"kind":"progress"' not in output, output

    with subtest("diagnostics retain the failure without account secrets"):
        logs = machine.succeed("cat /run/sekka-integrity-*.log")
        assert path in logs and "was modified" in logs, logs
        assert password not in logs
        assert set(machine.succeed("stat -c '%a:%u' /run/sekka-integrity-*.log").split()) == {"600:0"}

    assert machine.succeed("sha256sum /dev/vda", timeout=180) == before, "the target disk changed"
    machine.copy_from_machine("/run/corrupt-refused.jsonl")
    machine.shutdown()
  '';
}
