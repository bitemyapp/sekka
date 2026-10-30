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
  # Steam's rules follow programs.steam.enable; nothing else uses child_rule.
  steam = configuration true { programs.steam.enable = true; };
  steamRulesOff = configuration true {
    programs.steam.enable = true;
    calamares.tuning.steam.enable = false;
  };
  tracksAncestry = c: builtins.any (rule: rule ? child_rule) c.services.ananicy.extraRules;
  steamNames = [
    "steam"
    "steam.sh"
    "Steam.exe"
    "sekka-steam-children"
    "fossilize_replay"
  ];
  hasSteamRules =
    c: builtins.any (rule: builtins.elem (rule.name or "") steamNames) c.services.ananicy.extraRules;
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
assert !tracksAncestry enabled && !hasSteamRules enabled;
assert !tracksAncestry steamRulesOff && !hasSteamRules steamRulesOff;
assert tracksAncestry steam;
assert builtins.all
  (
    name:
    builtins.any (
      rule: rule.name == name && rule.nice == -5 && rule.child_rule == "sekka-steam-children"
    ) steam.services.ananicy.extraRules
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
) steam.services.ananicy.extraRules;
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
    # Without Steam's rules CachyOS's own rule for its launcher stays; with
    # them it is replaced.
    pattern='"name": *"steam"'
    grep -rqE "$pattern" ${enabled.services.ananicy.rulesProvider}/etc/ananicy.d
    if grep -rqE "$pattern" ${steam.services.ananicy.rulesProvider}/etc/ananicy.d; then
      exit 1
    fi
    touch $out
  ''
