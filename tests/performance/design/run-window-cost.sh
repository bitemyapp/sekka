#!/usr/bin/env bash
# The cost of a window, and of whole apps, on one neutral compositor:
# window-cost.py over four sets, each its own measurement.
#
#   tests/performance/design/run-window-cost.sh OUTDIR AIZOME HELLO KIT FIXTURES
#
# AIZOME   the aizome package (nix build .#aizome)
# HELLO    the GTK and Qt hello programs (tests/performance/design/hello)
# KIT      the kit's hello program (hello/kit, cargo build --release)
# FIXTURES photo.jpg, PDF32000_2008.pdf, folder/, editor-fixture.rs
#
# SMOKE=1 makes one short run of each program, to check they all open;
# ONLY=document-apps,monitors runs only the sets named.
#
# The GNOME and KDE apps come from Sekka's Nixpkgs. LOCK (optional) is a
# command each set runs under, e.g. a lock that keeps other benchmarks off
# the machine.
set -euo pipefail
out=$1 aizome=$2 hello=$3 kit=$4 fx=$5
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../../.." && pwd)
mkdir -p "$out"
pkg() { nix build --inputs-from "$repo" --no-link --print-out-paths "nixpkgs#$1"; }
gc=$(pkg gnome-calculator) gte=$(pkg gnome-text-editor) loupe=$(pkg loupe) papers=$(pkg papers)
naut=$(pkg nautilus) gsm=$(pkg gnome-system-monitor) clocks=$(pkg gnome-clocks) baobab=$(pkg baobab)
kcalc=$(pkg kdePackages.kcalc) kate=$(pkg kdePackages.kate) gwen=$(pkg kdePackages.gwenview)
okular=$(pkg kdePackages.okular) dolphin=$(pkg kdePackages.dolphin)
psm=$(pkg kdePackages.plasma-systemmonitor) filelight=$(pkg kdePackages.filelight)
A=$aizome/bin
run() {
  local name=$1
  shift
  # ONLY=name1,name2 runs just those sets.
  if [ -n "${ONLY:-}" ] && [[ ",$ONLY," != *",$name,"* ]]; then return; fi
  ${LOCK:-} nix shell --inputs-from "$repo" nixpkgs#sway nixpkgs#python3 nixpkgs#dbus -c \
    python3 "$here/window-cost.py" "$out/$name.json" "$@" ${SMOKE:+--reps 1 --idle 2}
}
run hello-windows --reps 10 --idle 15 \
  kit="$kit" gtk4="$hello/bin/hello-gtk4" adw="$hello/bin/hello-adw" \
  qtwidgets="$hello/bin/hello-qtwidgets" qtquick="$hello/bin/hello-qtquick"
run small-apps --reps 6 --idle 15 \
  soroban="$A/aizome-soroban" gnome-calculator="$gc/bin/gnome-calculator" kcalc="$kcalc/bin/kcalc" \
  tokei="$A/aizome-tokei" gnome-clocks="$clocks/bin/gnome-clocks" \
  sumi="$A/aizome-sumi $fx/editor-fixture.rs" gnome-text-editor="$gte/bin/gnome-text-editor $fx/editor-fixture.rs" \
  kwrite="$kate/bin/kwrite $fx/editor-fixture.rs"
run document-apps --reps 6 --idle 15 \
  shashin="$A/aizome-shashin $fx/photo.jpg" loupe="$loupe/bin/loupe $fx/photo.jpg" gwenview="$gwen/bin/gwenview $fx/photo.jpg" \
  shiori="$A/aizome-shiori $fx/PDF32000_2008.pdf" papers="$papers/bin/papers $fx/PDF32000_2008.pdf" \
  okular="$okular/bin/okular $fx/PDF32000_2008.pdf" \
  hako="$A/aizome-hako $fx/folder" nautilus="$naut/bin/nautilus --new-window $fx/folder" dolphin="$dolphin/bin/dolphin $fx/folder"
run monitors --reps 6 --idle 30 \
  myaku="$A/aizome-myaku" gnome-system-monitor="$gsm/bin/gnome-system-monitor" \
  plasma-systemmonitor="$psm/bin/plasma-systemmonitor" \
  kura="$A/aizome-kura" baobab="$baobab/bin/baobab" filelight="$filelight/bin/filelight"
