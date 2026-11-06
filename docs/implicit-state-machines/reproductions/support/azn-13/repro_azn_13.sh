#!/usr/bin/env bash
# AZN-13: with Aizome enabled (mutableTimeZone defaults to true), a declared
# `time.timeZone = "Europe/Paris"` is overridden at priority 90 and never
# used, not even as the starting zone. A fresh system installed without
# Genkan has neither a /etc/localtime link nor /var/lib/aizome/timezone, so
# activation leaves it in UTC.
#
# 1. Evaluates (no build) a minimal NixOS configuration importing the
#    worktree's desktop/aizome/nix/module.nix with Sekka's pinned Nixpkgs
#    (fetched --offline: already in the store).
# 2. Runs the module's own two activation snippets (aizomeTimeZoneSave and
#    aizomeTimeZoneRestore, text taken from the evaluation) in the order
#    their deps give (save, etc, restore) against an empty temporary root
#    standing for a fresh nixos-install target: /etc/localtime,
#    /etc/zoneinfo/ and /var/lib/aizome are rewritten to that root, whose
#    etc/zoneinfo is the tzdata link `etc` activation makes (and no
#    localtime, since the evaluation has none).
# Asserts the correct behaviour: the declared zone ends up in /etc/localtime.
set -euo pipefail
wt=${REPRO_WT:-$(git rev-parse --show-toplevel)}
work=$(mktemp -d "${TMPDIR:-/tmp}/azn13.XXXXXX")
trap 'rm -rf "$work"' EXIT
nixpkgs=$(nix eval --offline --raw --impure --expr 'builtins.fetchTree { type = "tarball"; url = "https://api.flakehub.com/f/pinned/NixOS/nixpkgs/0.1.1106360%2Brev-0bb2eedc20e4739f29e8045034f2828ad7054f8f/01a19d56-7554-7803-a091-6a18d4aafdff/source.tar.gz"; narHash = "sha256-fiPJ88izTVucjeo2EQ3jk4aWmLg03I1D4vxnX2QjEgE="; }')
cat > "$work/eval.nix" <<NIX
let
  eval = extra: (import $nixpkgs/nixos/lib/eval-config.nix {
    system = "x86_64-linux";
    modules = [
      (import $wt/desktop/aizome/nix/module.nix { self = { packages.x86_64-linux.aizome = throw "package not needed for this evaluation"; }; })
      {
        programs.aizome.enable = true;
        time.timeZone = "Europe/Paris";
        fileSystems."/" = { device = "/dev/disk/by-label/nixos"; fsType = "ext4"; };
        boot.loader.grub.enable = false;
        system.stateVersion = "26.05";
      }
      extra
    ];
  }).config;
  aizome = eval { };
  pinned = eval { programs.aizome.mutableTimeZone = false; };
in {
  effective = aizome.time.timeZone;
  etcHasLocaltime = aizome.environment.etc ? localtime;
  policy = aizome.systemd.services.systemd-timedated.environment.AIZOME_TIMEZONE_MANAGED;
  saveDeps = aizome.system.activationScripts.aizomeTimeZoneSave.deps;
  restoreDeps = aizome.system.activationScripts.aizomeTimeZoneRestore.deps;
  etcDeps = aizome.system.activationScripts.etc.deps;
  script = aizome.system.activationScripts.aizomeTimeZoneSave.text + "\n" + aizome.system.activationScripts.aizomeTimeZoneRestore.text;
  control = {
    effective = pinned.time.timeZone;
    localtime = pinned.environment.etc.localtime.source or null;
  };
}
NIX
get() { nix eval --impure "$@" --file "$work/eval.nix"; }
echo "evaluation (programs.aizome.enable, time.timeZone = \"Europe/Paris\"):"
echo "  time.timeZone = $(get --json effective)  environment.etc ? localtime = $(get --json etcHasLocaltime)  AIZOME_TIMEZONE_MANAGED = $(get --json policy)"
echo "  activation deps: save=$(get --json saveDeps) etc=$(get --json etcDeps) restore=$(get --json restoreDeps)"
echo "control (+ programs.aizome.mutableTimeZone = false): time.timeZone = $(get --json control.effective)  etc.localtime -> $(get --json control.localtime)"

root=$work/root
mkdir -p "$root/etc" "$root/var/lib"
ln -s /etc/zoneinfo "$root/etc/zoneinfo"
script=$(get --raw script)
script=${script//\/etc\/localtime/$root/etc/localtime}
script=${script//\/var\/lib\/aizome/$root/var/lib/aizome}
script=${script//\"\/etc\/zoneinfo\//\"$root/etc/zoneinfo/}
bash -c "$script"
if [ -L "$root/etc/localtime" ]; then
  result="/etc/localtime -> $(readlink "$root/etc/localtime")"
else
  result="no /etc/localtime (glibc and systemd use UTC)"
fi
echo "fresh install, after activation: $result; saved zone: $(cat "$root/var/lib/aizome/timezone" 2>/dev/null || echo none)"
case "$result" in
  *Europe/Paris*) echo PASS; exit 0 ;;
  *) echo "FAIL: the declared time.timeZone (Europe/Paris) was discarded on a fresh system"; exit 1 ;;
esac
