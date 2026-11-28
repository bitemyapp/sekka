{ pkgs, nixPackage }:
pkgs.runCommand "cache-publication-tests"
  {
    nativeBuildInputs = [
      nixPackage
      pkgs.python3
      pkgs.zstd
    ];
    # The fixtures' builder, as in the coverage tests.
    TEST_SHELL = "${pkgs.pkgsStatic.busybox}/bin/busybox";
  }
  ''
    mkdir -p scripts tests
    cp ${../scripts/prepare_cache.py} scripts/prepare_cache.py
    cp ${../scripts/check_cache_coverage.py} scripts/check_cache_coverage.py
    cp ${../tests/test_prepare_cache.py} tests/test_prepare_cache.py
    python3 -m unittest discover -s tests -p test_prepare_cache.py -v
    touch $out
  ''
