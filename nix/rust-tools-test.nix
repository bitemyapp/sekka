{ pkgs, applicationCatalog }:
let
  enabled =
    (applicationCatalog.evaluate [
      "build-tools"
      "rustup"
    ]).config;
  disabled = (applicationCatalog.evaluate [ ]).config;
  service = enabled.systemd.user.services.sekka-rust-tools;
in
assert !(disabled.systemd.user.services ? sekka-rust-tools);
assert service.wantedBy == [ "default.target" ];
assert service.unitConfig.ConditionUser == "!@system";
assert service.serviceConfig.Restart == "on-failure";
assert service.serviceConfig.Type == "exec";
assert service.serviceConfig.RemainAfterExit;
pkgs.runCommand "sekka-rust-tools-tests"
  {
    nativeBuildInputs = [
      pkgs.python3
      pkgs.bash
    ];
    # Build the real helper too, including writeShellApplication's ShellCheck.
    helper = service.serviceConfig.ExecStart;
  }
  ''
    test -x "$helper"
    python3 ${../tests/test_rust_tools.py} \
      ${../installer/genkan/rust/system/rust-tools/prepare.sh} -v
    touch "$out"
  ''
