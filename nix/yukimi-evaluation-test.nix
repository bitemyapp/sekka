# Real Nix and the packaged privileged helper, confined to a disposable VM.
# The tiny system's activation records a marker; it changes no boot devices.
{
  pkgs,
  helper,
  nix,
}:
let
  # A cached system, like an unchanged retry after an interrupted activation.
  # The fixture's activation changes no boot devices or running services.
  system = pkgs.runCommand "yukimi-evaluation-system" { } ''
    mkdir -p "$out/bin"
    cat > "$out/bin/switch-to-configuration" <<'SCRIPT'
    #!${pkgs.bash}/bin/bash
    test "$1" = boot || exit 1
    echo prepared > /run/yukimi-evaluation-applied
    SCRIPT
    chmod +x "$out/bin/switch-to-configuration"
  '';
  fixture = pkgs.writeText "yukimi-evaluation-flake.nix" ''
    {
      outputs = _: {
        nixosConfigurations.machine.config = {
          nix.settings = {};
          system.build.toplevel = builtins.trace "YUKIMI_EVAL_CACHE_MISS" {
            type = "derivation";
            name = "yukimi-evaluation-system";
            system = "${pkgs.stdenv.hostPlatform.system}";
            drvPath = "${system.drvPath}";
            outPath = "${system}";
            outputName = "out";
            outputs = [ "out" ];
          };
        };
      };
    }
  '';
in
pkgs.testers.runNixOSTest {
  name = "yukimi-evaluation";
  globalTimeout = 180;
  nodes.machine = {
    virtualisation.memorySize = 2048;
    # Exercise the real helper's 10 GB free-space preflight without faking it.
    virtualisation.diskSize = 20480;
    virtualisation.writableStoreUseTmpfs = false;
    boot.tmp.useTmpfs = false;
    virtualisation.additionalPaths = [
      system
      system.drvPath
    ];
    nix.package = nix;
    nix.settings = {
      experimental-features = [
        "nix-command"
        "flakes"
      ];
      lazy-trees = true;
      substituters = pkgs.lib.mkForce [ ];
      # Cache regressions must fail instead of silently compiling locally.
      max-jobs = 0;
    };
    environment.systemPackages = [ helper ];
    environment.etc."yukimi/system.json".text = builtins.toJSON { evaluationCores = 8; };
  };
  testScript = ''
    import json
    import shlex
    machine.start()
    machine.wait_for_unit("multi-user.target")
    machine.succeed("mkdir -p /etc/nixos; cp ${fixture} /etc/nixos/flake.nix; chmod u+w /etc/nixos/flake.nix")
    def update():
        status, output = machine.execute("yukimi-helper change --mode boot")
        if status != 0:
            print(machine.succeed("journalctl -u nix-daemon.service --no-pager -n 80"))
        assert status == 0, output
        result = json.loads(output)
        assert result["ok"], result
        operation = machine.succeed("ls -1 /var/lib/yukimi/operations | sort | tail -1").strip()
        log = machine.succeed(f"cat /var/lib/yukimi/operations/{operation}/progress.log")
        machine.succeed("test -f /run/yukimi-evaluation-applied")
        assert machine.succeed("find /var/lib/yukimi/operations -name '*-root' -print").strip() == ""
        return result, log
    first, first_log = update()
    second, second_log = update()
    assert first["system"] == second["system"]
    assert "YUKIMI_EVAL_CACHE_MISS" in first_log, first_log
    assert "YUKIMI_EVAL_CACHE_MISS" not in second_log, second_log
    for phase in ["Evaluating your configuration", "Checking for cached packages", "Downloading and building your system"]:
        assert f"Yukimi phase: {phase}; elapsed_ms=" in second_log, second_log
    # New source bytes invalidate the snapshot/cache even though the builder
    # and system output are unchanged. The new configuration must be checked.
    machine.succeed("echo '# changed' >> /etc/nixos/flake.nix")
    third, third_log = update()
    assert "YUKIMI_EVAL_CACHE_MISS" in third_log, third_log
    assert third["system"] == first["system"]

    # Import fixture bytes as fixed-output paths, publish them, then remove
    # the unrooted outputs. Their derivations have no usable builder: these
    # tests require substitution and never depend on in-guest compilation.
    machine.succeed("nix-store --generate-binary-cache-key yukimi-test /root/cache.sec /root/cache.pub")
    key = machine.succeed("cat /root/cache.pub").strip()
    cache = "file:///var/cache/yukimi-test"
    def cached_derivation(name, script, recursive=False):
        source = "/tmp/" + name
        machine.succeed("out=" + shlex.quote(source) + "; " + script)
        target = machine.succeed("nix-store --add-fixed sha256 "
                                 + ("--recursive " if recursive else "") + source).strip()
        digest = machine.succeed("nix hash " + ("path" if recursive else "file")
                                 + " --type sha256 --base16 " + source).strip()
        expression = (
            "builtins.derivation { name = " + json.dumps(name) + "; "
            'system = "${pkgs.stdenv.hostPlatform.system}"; '
            'builder = "/no-yukimi-fixture-builder"; '
            'outputHashAlgo = "sha256"; outputHashMode = '
            + json.dumps("recursive" if recursive else "flat") + "; outputHash = " + json.dumps(digest) + "; "
            "preferLocalBuild = false; allowSubstitutes = true; }"
        )
        machine.succeed("nix store sign --key-file /root/cache.sec " + target)
        machine.succeed("nix copy --to " + cache + " " + target)
        machine.succeed("nix-store --delete " + target)
        machine.succeed("test ! -e " + target)
        return expression, target

    original = machine.succeed("cat /etc/nixos/flake.nix")
    def write_flake(text):
        machine.succeed("printf %s " + shlex.quote(text) + " > /etc/nixos/flake.nix")

    with subtest("new system caches are available to import-from-derivation"):
        expression, target = cached_derivation("yukimi-ifd", "echo cached > $out")
        text = original.replace("nix.settings = {};", "nix.settings = { substituters = [ "
                                + json.dumps(cache) + " ]; trusted-public-keys = [ " + json.dumps(key) + " ]; };")
        text = text.replace('builtins.trace "YUKIMI_EVAL_CACHE_MISS"',
                            "assert builtins.readFile (" + expression + ') == "cached\\n"; builtins.trace "YUKIMI_EVAL_CACHE_MISS"')
        write_flake(text)
        result, log = update()
        assert result["system"] == first["system"]
        machine.succeed("test -e " + target)
        assert "YUKIMI_EVAL_CACHE_MISS" in log, log

    script = '${pkgs.coreutils}/bin/mkdir -p "$out/bin"; ${pkgs.coreutils}/bin/cp ${system}/bin/switch-to-configuration "$out/bin/"'
    def flake_cache_fixture(name):
        expression, target = cached_derivation(name, script, recursive=True)
        text = '{ nixConfig = { extra-substituters = [ ' + json.dumps(cache)
        text += ' ]; extra-trusted-public-keys = [ ' + json.dumps(key) + ' ]; }; '
        text += 'outputs = _: { nixosConfigurations.machine.config = { nix.settings = {}; '
        text += 'system.build.toplevel = builtins.trace "YUKIMI_EVAL_CACHE_MISS" (' + expression + '); }; }; }'
        write_flake(text)
        return target

    with subtest("unaccepted flake settings remain untrusted"):
        machine.succeed("mkdir -p /root/.config/nix; echo 'accept-flake-config = false' > /root/.config/nix/nix.conf")
        target = flake_cache_fixture("yukimi-flake-cache")
        status, output = machine.execute("yukimi-helper change --mode boot")
        assert status != 0, output
        machine.succeed("test ! -e " + target)
        assert machine.succeed("find /var/lib/yukimi/operations -name '*-root' -print").strip() == ""

    with subtest("accepted flake settings reach planning and building"):
        machine.succeed("echo 'accept-flake-config = true' > /root/.config/nix/nix.conf")
        result, log = update()
        assert result["system"] == target, result
        # The rejected attempt already evaluated this unchanged source.
        assert "YUKIMI_EVAL_CACHE_MISS" not in log, log
  '';
}
