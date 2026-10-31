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
    };
    environment.systemPackages = [ helper ];
    environment.etc."yukimi/system.json".text = builtins.toJSON { evaluationCores = 8; };
  };
  testScript = ''
    import json
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
  '';
}
