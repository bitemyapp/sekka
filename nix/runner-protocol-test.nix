# A real untrusted daemon request must preserve the original build failure when
# another requested derivation is cancelled. The old upstream client is a control.
{ pkgs, runner }:
let
  inherit (runner) tools nixPackage;
  request = pkgs.writeText "runner-failing-builds.nix" ''
    { label }:
    let
      marker = "/tmp/runner-cancel-" + label;
      build = name: script: builtins.derivation {
        inherit name;
        system = "x86_64-linux";
        builder = "/bin/sh";
        args = [ "-c" script ];
      };
    in [
      (build ("runner-slow-" + label) '''
        echo started > "''${marker}"
        exec ${pkgs.coreutils}/bin/sleep 120
      ''')
      (build ("runner-fail-" + label) '''
        for attempt in $(${pkgs.coreutils}/bin/seq 1 100); do
          if test -s "''${marker}"; then
            echo RUNNER-EXPECTED-FAILURE >&2
            exit 23
          fi
          ${pkgs.coreutils}/bin/sleep 0.1
        done
        echo RUNNER-SLOW-BUILD-NEVER-STARTED >&2
        exit 24
      ''')
    ]
  '';
in
pkgs.testers.runNixOSTest {
  name = "sekka-runner-protocol";
  nodes.machine = { lib, ... }: {
    virtualisation.memorySize = 2048;
    virtualisation.cores = 2;
    virtualisation.additionalPaths = [
      pkgs.nix
      request
    ];
    nix.package = nixPackage;
    nix.settings = {
      experimental-features = [ "nix-command" ];
      sandbox = false;
      max-jobs = 2;
      trusted-users = [ "root" ];
      allowed-users = [ "runner" ];
      substituters = lib.mkForce [ ];
    };
    users.users.runner = {
      isNormalUser = true;
      uid = 30500;
    };
    environment.systemPackages = [
      tools
      pkgs.coreutils
    ];
  };
  testScript = ''
    import json
    import shlex
    machine.start()
    machine.wait_for_unit("multi-user.target")
    machine.succeed("test $(readlink -f ${tools}/bin/nix) = $(readlink -f ${nixPackage}/bin/nix)")

    def as_runner(command):
        return "su -s /bin/sh runner -c " + shlex.quote(command)

    with subtest("actual runner client and untrusted daemon identity"):
        version = machine.succeed(as_runner("${tools}/bin/nix --version"))
        assert "Determinate Nix" in version, version
        info = json.loads(machine.succeed(as_runner("${tools}/bin/nix store info --store daemon --json")))
        assert info["url"] == "daemon" and info["trusted"] is False, info
        machine.succeed("mkdir -p /tmp/runner-evidence")
        machine.succeed(as_runner("${tools}/bin/nix --version") + " > /tmp/runner-evidence/client.txt")
        machine.succeed(as_runner("${tools}/bin/nix store info --store daemon --json") + " > /tmp/runner-evidence/daemon.json")

    def build(client, label):
        command = client + " build --store daemon --no-link --max-jobs 2 --cores 1 -L --file ${request} --argstr label " + label
        code, output = machine.execute(as_runner(command) + " > /tmp/runner-evidence/" + label + ".log 2>&1", timeout=60)
        log = machine.succeed("cat /tmp/runner-evidence/" + label + ".log")
        print(log)
        assert code != 0, (code, log)
        assert "RUNNER-EXPECTED-FAILURE" in log, log
        assert "RUNNER-SLOW-BUILD-NEVER-STARTED" not in log, log
        machine.wait_until_succeeds("! pgrep -f '^${pkgs.coreutils}/bin/sleep 120$'", timeout=30)
        return log

    with subtest("upstream client reproduces masked cancellation status"):
        original = build("${pkgs.nix}/bin/nix", "upstream")
        assert "Invalid BuildResult status" in original, original

    with subtest("pinned runner client reports failed derivation without protocol error"):
        fixed = build("${tools}/bin/nix", "determinate")
        assert "Invalid BuildResult status" not in fixed, fixed
        assert "runner-fail-determinate.drv" in fixed and "exit code 23" in fixed, fixed
        assert "(cancelled)" in fixed, fixed

    machine.succeed("journalctl -b -u nix-daemon --no-pager > /tmp/runner-evidence/daemon-journal.txt")
    machine.copy_from_machine("/tmp/runner-evidence", "evidence")
    machine.shutdown()
  '';
}
