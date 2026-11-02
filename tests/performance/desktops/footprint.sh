#!/usr/bin/env bash
# Disk footprint of each system without the benchmark's own tools, and the
# versions record, into docs/performance/data/desktops (run under lock-build:
# it builds the uninstrumented toplevels, which share every package with
# the measured ones).
set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo=$(cd "$here/../../.." && pwd)
out=$repo/docs/performance/data/desktops
cd "$repo"
declare -A top ver
for v in baseline aizome gnome plasma; do
  top[$v]=$(nix build --impure -f "$here" --no-link --print-out-paths "$v.footprint" 2>/dev/null)
  ver[$v]=$(nix build --impure -f "$here" --no-link --print-out-paths "$v.versions" 2>/dev/null)
done
{
  echo "{"
  first=1
  for v in baseline aizome gnome plasma; do
    size=$(nix path-info -S "${top[$v]}" | awk '{print $2}')
    paths=$(nix path-info -r "${top[$v]}" | wc -l)
    [ $first = 1 ] || echo ","
    first=0
    printf '  "%s": {"toplevel": "%s", "closure_bytes": %s, "store_paths": %s}' "$v" "${top[$v]}" "$size" "$paths"
  done
  echo
  echo "}"
} > "$out/footprint-raw.json"
{
  echo "{"
  first=1
  for v in baseline aizome gnome plasma; do
    [ $first = 1 ] || echo ","
    first=0
    printf '  "%s": %s' "$v" "$(cat "${ver[$v]}")"
  done
  echo
  echo "}"
} > "$out/versions.json"
# The commit the systems were built from.
commit=$(git -C "$repo" rev-parse HEAD)
sed -i "s|\"sekka\":\"unknown\"|\"sekka\":\"$commit\"|g" "$out/versions.json"
# Paths each desktop adds over the baseline, largest first.
set +o pipefail
for v in aizome gnome plasma; do
  comm -13 <(nix path-info -r "${top[baseline]}" | sort) <(nix path-info -r "${top[$v]}" | sort) |
    xargs nix path-info -s | sort -k2 -nr | head -25 |
    awk '{printf "%s\t%.1f\n", $1, $2/1048576}' > "$out/footprint-top-$v.tsv"
done
cat "$out/footprint-raw.json"
# Split each desktop's addition into what all three desktops add (the
# graphics stack, speech, fonts and services every Sekka desktop brings) and
# what is the desktop's own.
tmp=$(mktemp -d)
for v in baseline aizome gnome plasma; do nix path-info -r "${top[$v]}" | sort > "$tmp/$v"; done
for v in aizome gnome plasma; do comm -13 "$tmp/baseline" "$tmp/$v" > "$tmp/$v.add"; done
comm -12 "$tmp/aizome.add" "$tmp/gnome.add" | comm -12 - "$tmp/plasma.add" > "$tmp/common"
bytes() { if [ -s "$1" ]; then xargs nix path-info -s < "$1" | awk '{s += $2} END {print s + 0}'; else echo 0; fi; }
{
  echo "{"
  echo "  \"baseline_bytes\": $(bytes "$tmp/baseline"),"
  echo "  \"common_to_all_desktops_bytes\": $(bytes "$tmp/common"), \"common_paths\": $(wc -l < "$tmp/common"),"
  for v in aizome gnome plasma; do
    comm -23 "$tmp/$v.add" "$tmp/common" > "$tmp/$v.own"
    sep=","; [ $v = plasma ] && sep=""
    echo "  \"$v\": {\"total_bytes\": $(bytes "$tmp/$v"), \"added_over_baseline_bytes\": $(bytes "$tmp/$v.add"), \"own_bytes\": $(bytes "$tmp/$v.own"), \"own_paths\": $(wc -l < "$tmp/$v.own")}$sep"
  done
  echo "}"
} > "$out/footprint.json"
rm -r "$tmp"
cat "$out/footprint.json"
