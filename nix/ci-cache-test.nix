# GC must preserve both the check marker and the compile-only dependency.
# This uses the real runner client and an untrusted user of a disposable daemon.
{ pkgs, nixPackage }:
let
  flake = checks: ''
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
        partialInput = make "ci-partial-input" "echo compiled > $out";
      in {
        checks.x86_64-linux = ${checks};
        packages.x86_64-linux = {
          inherit dependency partialInput;
          fail = make "ci-failure" "echo expected-failure >&2; exit 23";
          unrelated = make "ci-unrelated" "echo unrelated > $out";
        };
      };
    }
  '';
  fixture = pkgs.writeText "ci-cache-flake.nix" (flake "{ example = check; }");
  # One check passes while another fails: --keep-going still links the
  # passing one, and its build input must survive for the rerun.
  partial = pkgs.writeText "ci-cache-partial-flake.nix" (flake ''
    {
      passing = make "ci-partial-pass" "test -e ''${partialInput}; echo passed > $out";
      failing = make "ci-partial-fail" "exit 1";
    }
  '');
in
pkgs.testers.runNixOSTest {
  name = "sekka-ci-cache";
  nodes.machine =
    { lib, ... }:
    {
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
      virtualisation.additionalPaths = [
        fixture
        partial
      ];
      system.extraDependencies = [
        fixture
        partial
        ../scripts/ci-retain.py
      ];
    };
  testScript = ''
    import shlex
    machine.start()
    machine.wait_for_unit("multi-user.target")
    root = "/nix/var/nix/gcroots/sekka-ci"
    machine.succeed(f"mkdir -p {root} /home/runner/fixture /home/runner/partial; chown runner:users {root} /home/runner/fixture /home/runner/partial")
    machine.succeed("cp ${fixture} /home/runner/fixture/flake.nix")
    machine.succeed("cp ${partial} /home/runner/partial/flake.nix")

    # The VM's small writable store would always count as low on space.
    def runner(command, directory="fixture"):
        return machine.succeed("su -s /bin/sh runner -c " + shlex.quote(
            f"cd /home/runner/{directory}; export NIX_REMOTE=daemon SEKKA_CI_ROOTS={root} SEKKA_CI_MIN_FREE_BYTES=0; " + command))

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

    with subtest("a failed --keep-going check keeps the inputs of the checks that passed"):
        runner(f"if {retain} pr checks flake check --keep-going --no-update-lock-file > /tmp/ci-partial.log 2>&1; then exit 1; fi", "partial")
        passing = runner("nix eval --raw .#checks.x86_64-linux.passing.outPath", "partial")
        partial_input = runner("nix eval --raw .#packages.x86_64-linux.partialInput.outPath", "partial")
        machine.succeed("nix-store --gc")
        machine.succeed(f"test -e {passing} && test -e {partial_input}")
        machine.succeed(f"test -z \"$(find {root}/pr-checks -name success)\"")

    with subtest("without the roots mount, it runs plain Nix and roots nothing"):
        before = machine.succeed(f"find {root} | sort")
        runner("env -u SEKKA_CI_ROOTS " + retain + " pr uta build .#unrelated 2> /tmp/ci-plain.log")
        machine.succeed("grep 'No persistent CI roots' /tmp/ci-plain.log")
        assert machine.succeed(f"find {root} | sort") == before

    with subtest("low free space drops pull request roots first"):
        runner(f"SEKKA_CI_MIN_FREE_BYTES={2**62} " + retain + " prune")
        machine.succeed(f"test -z \"$(find {root}/pr-uta {root}/pr-checks -mindepth 1)\"")
        machine.succeed(f"test -n \"$(find {root}/main-checks -name success)\"")
        machine.succeed("nix-store --gc")
        machine.fail(f"test -e {partial_input}")
        machine.succeed(f"test -e {check} && test -e {dependency}")

    with subtest("expired roots release store paths"):
        machine.succeed(f"rm -rf {root}/main-checks")
        machine.succeed("nix-store --gc")
        machine.fail(f"test -e {check}")
        machine.fail(f"test -e {dependency}")
    machine.shutdown()
  '';
}
