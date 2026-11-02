# GC must preserve both the check marker and the compile-only dependency.
# This uses the real runner client and an untrusted user of a disposable daemon.
{ pkgs, nixPackage }:
let
  fixture = pkgs.writeText "ci-cache-flake.nix" ''
    {
      outputs = { self }: let
        make = name: script: builtins.derivation {
          inherit name;
          system = "x86_64-linux";
          builder = "${pkgs.bash}/bin/bash";
          args = [ "-c" script ];
        };
        dependency = make "ci-compile-input" "echo compiled > $out";
        check = make "ci-success" "test -e ''${dependency}; echo passed > $out";
      in {
        checks.x86_64-linux.example = check;
        packages.x86_64-linux = {
          inherit dependency;
          fail = make "ci-failure" "echo expected-failure >&2; exit 23";
          unrelated = make "ci-unrelated" "echo unrelated > $out";
        };
      };
    }
  '';
in
pkgs.testers.runNixOSTest {
  name = "sekka-ci-cache";
  nodes.machine = { lib, ... }: {
    virtualisation.memorySize = 2048;
    nix.package = nixPackage;
    nix.settings = {
      experimental-features = [
        "nix-command"
        "flakes"
      ];
      sandbox = false;
      substituters = lib.mkForce [ ];
      trusted-users = [ "root" ];
      allowed-users = [ "runner" ];
      keep-outputs = false;
    };
    users.users.runner = {
      isNormalUser = true;
      uid = 30500;
    };
    environment.systemPackages = [
      pkgs.python3
      pkgs.bash
    ];
    virtualisation.additionalPaths = [ fixture ];
    system.extraDependencies = [
      fixture
      ../scripts/ci-retain.py
    ];
  };
  testScript = ''
    import shlex
    machine.start()
    machine.wait_for_unit("multi-user.target")
    root = "/nix/var/nix/gcroots/sekka-ci"
    machine.succeed(f"mkdir -p {root} /home/runner/fixture; chown runner:users {root} /home/runner/fixture")
    machine.succeed("cp ${fixture} /home/runner/fixture/flake.nix")

    def runner(command):
        return machine.succeed("su -s /bin/sh runner -c " + shlex.quote(
            "cd /home/runner/fixture; export NIX_REMOTE=daemon SEKKA_CI_ROOTS=" + root + "; " + command))

    retain = "python3 ${../scripts/ci-retain.py}"
    with subtest("flake check roots its result and build-only dependencies"):
        runner(retain + " main checks flake check --no-update-lock-file -L")
        check = runner("nix eval --raw .#checks.x86_64-linux.example.outPath")
        dependency = runner("nix eval --raw .#packages.x86_64-linux.dependency.outPath")
        unrelated = runner("nix build --no-link --print-out-paths .#unrelated").strip()
        machine.succeed("nix-store --gc")
        machine.succeed(f"test -e {check} && test -e {dependency}")
        machine.fail(f"test -e {unrelated}")
        # Force an error if a cached result attempts to build: GC reuse, not a
        # second successful execution, must account for this success.
        runner(retain + " main checks flake check --max-jobs 0 --no-update-lock-file")

    with subtest("failures remain failures and never become a successful generation"):
        runner(f"if {retain} pr uta build .#fail > /tmp/ci-failure.log 2>&1; then exit 1; fi")
        machine.succeed("grep expected-failure /tmp/ci-failure.log")
        machine.succeed(f"test -z \"$(find {root}/pr-uta -name success)\"")

    with subtest("expired roots release store paths"):
        machine.succeed(f"rm -rf {root}/main-checks")
        machine.succeed("nix-store --gc")
        machine.fail(f"test -e {check}")
        machine.fail(f"test -e {dependency}")
    machine.shutdown()
  '';
}
