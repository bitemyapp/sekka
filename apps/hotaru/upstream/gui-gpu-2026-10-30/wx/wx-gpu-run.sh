#!/usr/bin/env bash
# The GPU comparison on wx's NVIDIA GPU: qualify both applications, build the
# calibration, then four startup rounds and two sustained rounds.
set -uo pipefail
src=/home/callen/sekka-work/gpu-cmp/tools
W=/home/callen/sekka-work/gpu-cmp/wx
ghostty=/nix/store/rvbnq9b2j16127m46g7ia1vv9zw5cy03-ghostty-1.4.1-dev+6ba144158577d033ccb6ed4b574e4013957c9161-nix/bin/ghostty
hotaru=/nix/store/pyy32rwyrpgnnpwlc5fmw5r88px0jrs6-hotaru-0.1.0/bin/hotaru
fonts=/nix/store/b9vp4qd3gz9vmsl20grvd80hniirn5n5-aizome-fonts
fallback=/nix/store/1rv68ixfzh8ki0jmanx7y846h2qdvzbg-hotaru-fonts
cpus=(--app-cpus 8-11 --compositor-cpus 12-13 --observer-cpus 14 --controller-cpus 15 --fixture-cpus 16)
mkdir -p $W
phase=${1:-all}
if [ "$phase" = all ] || [ "$phase" = qualify ]; then
  rm -rf $W/qual-hotaru $W/qual-ghostty $W/calibration
  python3 $src/frame-benchmark.py hotaru $hotaru --renderer gpu --purpose qualification --output $W/qual-hotaru \
    --font-dir $fonts --fallback-dir $fallback --window-width 1349 --window-height 1077 --cell-width 11 --cell-height 24 "${cpus[@]}" | tail -1
  python3 $src/frame-benchmark.py ghostty $ghostty --renderer gpu --purpose qualification --output $W/qual-ghostty \
    --app-env $src/wx-ghostty-env.json --font-dir $fonts --fallback-dir $fallback --window-width 1349 --window-height 1077 \
    --cell-width 11 --cell-height 24 --ghostty-cell-height=-1 "${cpus[@]}" | tail -1
  python3 $src/frame-calibration.py $W/qual-hotaru $W/qual-ghostty $W/calibration || exit 1
fi
if [ "$phase" = all ] || [ "$phase" = measure ]; then
  for p in startup:4 steady:2; do
    name=${p%%:*}; rounds=${p##*:}
    python3 $src/frame-matrix.py --output $W/$name --calibration $W/calibration --font-dir $fonts --fallback-dir $fallback \
      --renderer gpu --ghostty-env $src/wx-ghostty-env.json --phase $name --rounds $rounds --quiet-wait 120 "${cpus[@]}" | tail -1
  done
fi
