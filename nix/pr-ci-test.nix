{ pkgs }:
pkgs.runCommand "trusted-pr-ci-tests"
  {
    nativeBuildInputs = [
      pkgs.python3
      pkgs.bash
      pkgs.actionlint
      pkgs.shellcheck
      pkgs.git
      pkgs.jq
      pkgs.util-linux
    ];
  }
  ''
    mkdir -p scripts tests
    cp ${../scripts/check-pr-trust.py} scripts/check-pr-trust.py
    cp ${../scripts/runner-admission.sh} scripts/runner-admission.sh
    cp ${../tests/test_pr_trust.py} tests/test_pr_trust.py
    cp ${../scripts/ci-retain.py} scripts/ci-retain.py
    cp ${../scripts/runner-git-cache.sh} scripts/runner-git-cache.sh
    cp ${../scripts/runner-supervisor.sh} scripts/runner-supervisor.sh
    cp ${../tests/test_ci_cache.py} tests/test_ci_cache.py
    cp ${../tests/test_runner_supervisor.py} tests/test_runner_supervisor.py
    python3 -m unittest discover -s tests -v
    shellcheck -s bash scripts/runner-admission.sh
    shellcheck scripts/runner-git-cache.sh scripts/runner-supervisor.sh ${../scripts/runner-host-setup.sh}
    actionlint -config-file ${../.github/actionlint.yaml} ${../.github/workflows/pr.yml} ${../.github/workflows/cache.yml} ${../.github/workflows/nightly.yml}
    touch $out
  ''
