{ pkgs }:
pkgs.runCommand "hotaru-evidence-restoration-tests"
  {
    nativeBuildInputs = [
      pkgs.python3
      pkgs.git
    ];
  }
  ''
    mkdir -p scripts tests
    cp ${../scripts/restore-hotaru-evidence.py} scripts/restore-hotaru-evidence.py
    cp ${../tests/test_restore_hotaru_evidence.py} tests/test_restore_hotaru_evidence.py
    python3 -m unittest discover -s tests -p test_restore_hotaru_evidence.py -v
    touch $out
  ''
