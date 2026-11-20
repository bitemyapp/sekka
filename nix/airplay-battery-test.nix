{
  runCommand,
  python3,
  lib,
}:
runCommand "sekka-airplay-battery-check" { nativeBuildInputs = [ python3 ]; } ''
  cp -r ${
    lib.fileset.toSource {
      root = ../.;
      fileset = lib.fileset.unions [
        ../scripts/airplay_battery.py
        ../tests/check_airplay_battery.py
        ../tests/airplay-battery-cases.json
        ../services/airplay-radio/src/profiles.rs
        ../desktop/aizome/apps/kagami/src/battery.rs
      ];
    }
  } source
  chmod -R u+w source
  python source/tests/check_airplay_battery.py -v
  touch $out
''
