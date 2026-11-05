{ nixpkgs, system }:
let
  pkgs = nixpkgs.legacyPackages.${system};
  configuration =
    enabled: extra:
    (nixpkgs.lib.nixosSystem {
      inherit system;
      modules = [
        ../../installer/genkan/rust/system/tuning.nix
        { calamares.tuning.enable = enabled; }
        extra
      ];
    }).config;
  enabled = configuration true { };
  disabled = configuration false { };
  custom = configuration true {
    nix.daemonCPUSchedPolicy = "batch";
    systemd.services.nix-daemon.serviceConfig.CPUWeight = 50;
    systemd.user.slices.app-background.sliceConfig.CPUWeight = 30;
  };
  recorderOnly = configuration false { services.sekka-lag-recorder.enable = true; };
in
assert enabled.systemd.user.slices.app-background.sliceConfig.CPUWeight == 20;
assert enabled.nix.daemonCPUSchedPolicy == "idle";
assert enabled.nix.daemonIOSchedClass == "idle";
assert builtins.any (
  rule: rule.name == "codex" && rule.nice == -4 && rule.sched == "normal"
) enabled.services.ananicy.extraRules;
assert !(disabled.systemd.user.services ? sekka-agent-workloads);
assert !(disabled.environment.etc ? "sekka/background-scheduling");
assert !disabled.services.ananicy.enable;
# Recording is opt-in and must not turn on scheduling or OOM policy.
assert !(enabled.systemd.user.services ? sekka-lag-recorder);
assert !(disabled.systemd.user.services ? sekka-lag-recorder);
assert recorderOnly.systemd.user.services ? sekka-lag-recorder;
assert !recorderOnly.services.ananicy.enable;
assert recorderOnly.systemd.oomd.enable == disabled.systemd.oomd.enable;
assert custom.nix.daemonCPUSchedPolicy == "batch";
assert custom.systemd.services.nix-daemon.serviceConfig.CPUWeight == 50;
assert custom.systemd.user.slices.app-background.sliceConfig.CPUWeight == 30;
pkgs.runCommand "sekka-scheduling-policy" { nativeBuildInputs = [ pkgs.python3 ]; } ''
  mkdir -p source/nix/scheduling source/tests
  cp ${./agent-workloads.py} source/nix/scheduling/agent-workloads.py
  cp ${../../tests/test_agent_workloads.py} source/tests/test_agent_workloads.py
  cp ${./lag-recorder.py} source/nix/scheduling/lag-recorder.py
  cp ${./lag_history.py} source/nix/scheduling/lag_history.py
  cp ${./lag_summary.py} source/nix/scheduling/lag_summary.py
  cp ${../../tests/test_lag_recorder.py} source/tests/test_lag_recorder.py
  cp ${../../tests/test_lag_history.py} source/tests/test_lag_history.py
  cp ${../../tests/test_lag_summary.py} source/tests/test_lag_summary.py
  python3 -m unittest discover -s source/tests -p 'test_*.py' -v
  touch $out
''
