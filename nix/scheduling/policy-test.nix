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
assert custom.nix.daemonCPUSchedPolicy == "batch";
assert custom.systemd.services.nix-daemon.serviceConfig.CPUWeight == 50;
assert custom.systemd.user.slices.app-background.sliceConfig.CPUWeight == 30;
assert builtins.all
  (
    name:
    builtins.any (
      rule: rule.name == name && rule.nice == -5 && rule.child_rule == "sekka-steam-children"
    ) enabled.services.ananicy.extraRules
  )
  [
    "steam"
    "steam.sh"
    "Steam.exe"
  ];
assert builtins.any (
  rule:
  rule.name == "sekka-steam-children"
  && rule.nice == -10
  && rule.nice_only_raise
  && rule.sched_keep_realtime
) enabled.services.ananicy.extraRules;
pkgs.runCommand "sekka-scheduling-policy"
  {
    nativeBuildInputs = [
      pkgs.python3
      pkgs.stdenv.cc
    ];
  }
  ''
    mkdir -p source/nix/scheduling source/tests
    cp ${./agent-workloads.py} source/nix/scheduling/agent-workloads.py
    cp ${../../tests/test_agent_workloads.py} source/tests/test_agent_workloads.py
    python3 -m unittest discover -s source/tests -p test_agent_workloads.py -v
    cp ${./descendant-rules.hpp} descendant-rules.hpp
    c++ -std=c++20 -Wall -Wextra -Werror -pthread -I . \
      ${../../tests/test_descendant_rules.cpp} -o test-descendants
    ./test-descendants
    touch $out
  ''
