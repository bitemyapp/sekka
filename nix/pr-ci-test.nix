{ pkgs }:
pkgs.runCommand "trusted-pr-ci-tests"
  {
    nativeBuildInputs = [
      pkgs.python3
      pkgs.bash
      pkgs.actionlint
      pkgs.shellcheck
    ];
  }
  ''
    mkdir -p scripts tests
    cp ${../scripts/check-pr-trust.py} scripts/check-pr-trust.py
    cp ${../scripts/runner-admission.sh} scripts/runner-admission.sh
    cp ${../tests/test_pr_trust.py} tests/test_pr_trust.py
    python3 -m unittest discover -s tests -p test_pr_trust.py -v
    shellcheck -s bash scripts/runner-admission.sh
    actionlint -config-file ${../.github/actionlint.yaml} ${../.github/workflows/pr.yml}
    touch $out
  ''
