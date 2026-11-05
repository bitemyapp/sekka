#!/usr/bin/env bash
# Why Aizome's apps start faster and hold less: every set of runs behind
# docs/performance/why-apps.md.
#
#   tests/performance/why-apps/run.sh OUT HELLO KIT AIZOME FIXTURES [SET...]
#
# OUT       a working directory for raw runs (large: perf data, smaps)
# HELLO     the GTK and Qt hello programs (design/hello)
# KIT       the kit's hello program (design/hello/kit, cargo build --release)
# AIZOME    the aizome package
# FIXTURES  design's fixtures (editor-fixture.rs)
# SET       ablate stamps perf mem heaptrack strace wldebug fonts lddebug
#           pstat memablate perffonts kitfonts (default: all)
#
# LOCK (optional) is a command each set runs under. Each set waits for a
# 1-minute load average under 1.5 (up to 10 minutes) before it starts.
set -euo pipefail
out=$1 hello=$2 kit=$3 aizome=$4 fx=$5
shift 5
sets=${*:-ablate stamps perf mem heaptrack strace wldebug fonts lddebug pstat memablate perffonts kitfonts}
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
mkdir -p "$out"
pkg() { nix build --inputs-from "$repo" --no-link --print-out-paths "nixpkgs#$1"; }
gte=$(pkg gnome-text-editor) kate=$(pkg kdePackages.kate)
bins() { for p in "$@"; do for o in $(pkg "$p"); do [ -d "$o/bin" ] && echo -n "$o/bin:"; done; done; }
PATH=$(bins sway dbus python3 perf strace heaptrack)$PATH
export PATH
stamps=$out/stamps.so
nix shell --inputs-from "$repo" nixpkgs#gcc -c gcc -O2 -Wall -shared -fPIC -o "$stamps" "$here/stamps.c"
# The fonts set's configuration: the system's rules (conf.d) without the
# file that lists its font directories, and one font package, DejaVu.
dejavu=$(grep -o '/nix/store/[a-z0-9]*-dejavu-fonts-2[^<]*' /etc/fonts/conf.d/00-nixos-cache.conf | head -1)
mkdir -p "$out/fonts-conf/conf.d" "$out/fonts-conf/cache"
for f in /etc/fonts/conf.d/*.conf; do
  [ "$(basename "$f")" = 00-nixos-cache.conf ] || ln -sf "$(readlink -f "$f")" "$out/fonts-conf/conf.d/"
done
printf '%s\n' '<?xml version="1.0"?>' '<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">' '<fontconfig>' \
  "  <dir>$dejavu</dir>" "  <cachedir>$out/fonts-conf/cache</cachedir>" \
  "  <include ignore_missing=\"yes\">$out/fonts-conf/conf.d</include>" '</fontconfig>' >"$out/fonts-conf/fonts.conf"
export FONTS_CONF=$out/fonts-conf/fonts.conf
quiet() {
  for _ in $(seq 120); do
    awk '{exit !($1 < 1.5)}' /proc/loadavg && return
    sleep 5
  done
}
for set in $sets; do
  # perffonts is the perf set's runs of the kit's programs with one font
  # package.
  vset=$set
  [ "$set" = perffonts ] && vset=perf
  PERF_FONTS=$([ "$set" = perffonts ] && echo 1 || true) \
    python3 "$here/variants.py" "$vset" "$hello" "$kit" "$aizome" "$gte" "$kate" "$fx" "$stamps" >"$out/$set.json"
  case $set in
  perffonts) args=(--reps 3 --linger 0.3) ;;
  ablate) args=(--reps 10) ;;
  stamps) args=(--reps 5) ;;
  perf) args=(--reps 3 --linger 0.3) ;;
  mem) args=(--reps 3 --settle 3 --smaps) ;;
  heaptrack) args=(--reps 1 --settle 3) ;;
  strace) args=(--reps 2) ;;
  wldebug) args=(--reps 2 --linger 0.5) ;;
  fonts) args=(--reps 10) ;;
  lddebug) args=(--reps 3) ;;
  pstat) args=(--reps 8) ;;
  memablate) args=(--reps 3 --settle 3 --smaps) ;;
  kitfonts) args=(--reps 8 --settle 3 --smaps) ;;
  esac
  ${LOCK:-} bash -c "$(declare -f quiet); quiet; exec python3 '$here/probe.py' '$out/$set' '$out/$set.json' ${args[*]}"
done
