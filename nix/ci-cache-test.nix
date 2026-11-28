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
  # The nightly's second attempt (`--retry-failed`). Beside a check that
  # passes: one that fails the first time it is built and passes the second,
  # as a VM test that lost a timing race does; one that never passes; or one
  # that does not evaluate. The test's daemon builds without a sandbox, so
  # each builder can count its runs in /tmp. A failing one writes its output
  # first, which leaves the path in the store without a valid result.
  retryFixture =
    name:
    let
      counted =
        check: rest:
        ''make "ci-${name}-${check}" "echo ${check} >> /tmp/${name}-builds; echo written > $out${rest}"'';
      cases = {
        flaky = counted "flaky" "; test -e /tmp/${name}-second || { : > /tmp/${name}-second; exit 1; }";
        broken = counted "broken" "; exit 1";
        unevaluable = ''throw "this check does not evaluate"'';
      };
      case = builtins.head (pkgs.lib.splitString "-" name);
    in
    pkgs.writeText "ci-cache-${name}-flake.nix" (flake ''
      {
        steady = ${counted "steady" ""};
        ${case} = ${cases.${case}};
      }
    '');
  # Each case with nix-fast-build and with the `nix flake check` that stands
  # in for it on a runner image without one.
  retryFixtures = pkgs.lib.genAttrs [
    "flaky-fast"
    "flaky-plain"
    "broken-fast"
    "broken-plain"
    "unevaluable-fast"
    "unevaluable-plain"
  ] retryFixture;
in
pkgs.testers.runNixOSTest {
  name = "sekka-ci-cache";
  nodes.machine =
    { lib, ... }:
    {
      virtualisation.memorySize = 4096;
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
        pkgs.nix-fast-build
      ];
      virtualisation.additionalPaths = [
        fixture
        partial
      ]
      ++ builtins.attrValues retryFixtures;
      system.extraDependencies = [
        fixture
        partial
        ../scripts/ci-retain.py
      ]
      ++ builtins.attrValues retryFixtures;
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
        # second successful execution, must account for this success. The
        # check's recorded build inputs serve again without a query.
        log = runner(retain + " main checks flake check --max-jobs 0 --no-update-lock-file 2>&1")
        assert "(queried 0 of 1 derivations)" in log, log

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

    with subtest("fast-build links each passing check and keeps its inputs"):
        runner(f"if {retain} pr packages fast-build --flake .#checks.x86_64-linux --eval-workers 1 > /tmp/ci-fast.log 2>&1; then exit 1; fi", "partial")
        machine.succeed(f"test -L \"$(find {root}/pr-packages -name result-passing)\"")
        machine.succeed(f"test -z \"$(find {root}/pr-packages -name 'result-failing*')\"")
        machine.succeed(f"test -z \"$(find {root}/pr-packages -name success)\"")
        # Only this generation keeps them now.
        machine.succeed(f"rm -rf {root}/pr-checks")
        machine.succeed("nix-store --gc")
        machine.succeed(f"test -e {passing} && test -e {partial_input}")

    with subtest("without the roots mount, it runs plain Nix and roots nothing"):
        before = machine.succeed(f"find {root} | sort")
        runner("env -u SEKKA_CI_ROOTS " + retain + " pr uta build .#unrelated 2> /tmp/ci-plain.log")
        machine.succeed("grep 'No persistent CI roots' /tmp/ci-plain.log")
        assert machine.succeed(f"find {root} | sort") == before

    with subtest("low free space drops pull request roots first"):
        runner(f"SEKKA_CI_MIN_FREE_BYTES={2**62} " + retain + " prune")
        machine.succeed(f"test -z \"$(find {root}/pr-uta {root}/pr-packages -mindepth 1)\"")
        machine.succeed(f"test -n \"$(find {root}/main-checks -name success)\"")
        machine.succeed("nix-store --gc")
        machine.fail(f"test -e {partial_input}")
        machine.succeed(f"test -e {check} && test -e {dependency}")

    with subtest("expired roots release store paths"):
        machine.succeed(f"rm -rf {root}/main-checks")
        machine.succeed("nix-store --gc")
        machine.fail(f"test -e {check}")
        machine.fail(f"test -e {dependency}")

    retry_fixtures = ${builtins.toJSON (builtins.mapAttrs (_: toString) retryFixtures)}
    # A runner image from before nix-fast-build has no such program on PATH.
    plain = "PATH=${
      pkgs.lib.makeBinPath [
        pkgs.python3
        nixPackage
        pkgs.coreutils
      ]
    } "

    def retried(name):
        """One fixture's checks with a second attempt: the status, the log and every build so far."""
        machine.succeed(
            f"mkdir -p /home/runner/{name} && cp {retry_fixtures[name]} /home/runner/{name}/flake.nix"
            f" && chown -R runner:users /home/runner/{name}"
            f" && {{ test -e /tmp/{name}-builds || install -m 666 /dev/null /tmp/{name}-builds; }}")
        prefix = plain if name.endswith("-plain") else ""
        status = runner(f"{prefix}{retain} --retry-failed pr checks fast-build --flake .#checks.x86_64-linux"
                        f" --eval-workers 1 > /tmp/{name}.log 2>&1; echo $?", name)
        log = machine.succeed(f"cat /tmp/{name}.log")
        assert ("nix-fast-build is missing" in log) == bool(prefix), log
        return int(status), log, sorted(machine.succeed(f"cat /tmp/{name}-builds").split())

    for kind in ("fast", "plain"):
        with subtest(f"a check that fails once is built once more, alone, and the run passes ({kind})"):
            status, log, built = retried(f"flaky-{kind}")
            assert status == 0, log
            assert "Running failed checks once more: flaky." in log, log
            assert built == ["flaky", "flaky", "steady"], built
            # With every check built, a pass that asks for no second attempt.
            status, log, built = retried(f"flaky-{kind}")
            assert status == 0 and "once more" not in log, log
            assert built == ["flaky", "flaky", "steady"], built

        with subtest(f"a check that fails twice fails the run ({kind})"):
            status, log, built = retried(f"broken-{kind}")
            assert status != 0, log
            assert "Running failed checks once more: broken." in log, log
            assert built == ["broken", "broken", "steady"], built
            # Its output's path exists all the same: the store has to be asked.
            broken = runner("nix eval --raw .#checks.x86_64-linux.broken.outPath", f"broken-{kind}")
            machine.succeed(f"test -e {broken}")

        with subtest(f"a check that does not evaluate is not run again ({kind})"):
            status, log, built = retried(f"unevaluable-{kind}")
            assert status != 0, log
            assert "A check failed to evaluate; running it again cannot help." in log, log
            assert "once more" not in log, log
            assert built == ["steady"], built
    machine.shutdown()
  '';
}
