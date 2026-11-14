#!/usr/bin/env bash
# YUK-11: Clean Up deletes old generations, then collects garbage, and only
# then rebuilds the boot menu. When the collection is stopped, or fails,
# the generations are gone but the boot menu still lists them; a later
# collection removes their closures and those entries no longer boot. The
# boot menu must match the generations that exist whenever `clean` has
# deleted some.
#
# Runs the real yukimi-helper worker (`__worker DIR …`) inside an
# unprivileged user + mount namespace in which /run, /nix/var/nix,
# /var/lib/yukimi and /etc/yukimi are private tmpfs mounts and /etc/nixos is
# a bind of a temporary directory. The helper's fixed PATH
# (/run/current-system/sw/bin) then holds only stand-ins for nix, nix-env,
# nix-channel, nix-store, git, loginctl and systemctl, so nothing real runs
# and nothing outside the namespace's temporary directories changes (the
# namespace's "root" is the unprivileged user outside it).
#
# usage: run default yukimi bash repro_yuk_11.sh /path/to/yukimi-helper
# exit 0: correct behaviour; exit 1: the bug reproduced.
set -euo pipefail
if [[ ${REPRO_IN_NS:-} != 1 ]]; then
  helper=$(readlink -f "${1:?path to the yukimi-helper binary}")
  exec env REPRO_IN_NS=1 unshare --user --map-root-user --mount --propagation private \
    bash "$(readlink -f "$0")" "$helper"
fi

# ---- harness (identical in repro_yuk_02/09/10/11.sh) -----------------------
T=$(mktemp -d "${TMPDIR:-/tmp}/repro-yuk.XXXXXX")
trap 'rm -rf "$T"' EXIT
cp "$1" "$T/yukimi-helper"
[[ -x $(dirname "$1")/sekka-update ]] && cp "$(dirname "$1")/sekka-update" "$T/sekka-update"
SH=$(readlink -f "$(command -v bash)")
CORE=$(dirname "$(readlink -f "$(command -v mkdir)")")
mkdir -p "$T"/{sw/bin,sys,chan,ctl,state,etc-nixos} && : >"$T/calls"
mount -t tmpfs repro /run
mount -t tmpfs repro /nix/var/nix
mount -t tmpfs repro /var/lib/yukimi
mount -t tmpfs repro /etc/yukimi
mount --bind "$T/etc-nixos" /etc/nixos
mkdir -p /run/lock /nix/var/nix/profiles/per-user/root /var/lib/yukimi/operations
chmod 700 /var/lib/yukimi/operations

stub() { # name body…: a stand-in on the helper's PATH that logs its arguments
  local file=$1; shift
  { echo "#!$SH"; echo "export PATH=$CORE"; echo "T=$T"; echo 'echo "${0##*/} $*" >>"$T/calls"'; printf '%s\n' "$@"; } >"$file"
  chmod +x "$file"
}
stub "$T/sw/bin/nix" 'a=" $* "' 'case "$a" in' \
  '*" config show nix-path "*) echo "nixpkgs=/nix/var/nix/profiles/per-user/root/channels/nixos";;' \
  '*" build "*) echo "channels=$(readlink /nix/var/nix/profiles/per-user/root/channels) lock=$(cat /etc/nixos/flake.lock 2>/dev/null)" >>"$T/builds"' \
  '  m=$(cat "$T/ctl/build"); case "$m" in' \
  '    fail) echo "error: builder for system failed" >&2; exit 1;;' \
  '    hang) touch "$T/state/build-started"; exec sleep 600;;' \
  '    ok\ *) echo "$T/sys/${m#ok }";; esac;;' \
  '*attrNames*) echo "[\"fake\"]";;' \
  '*nix.settings*) echo "{\"substituters\":[],\"keys\":[]}";;' \
  '*" flake update "*) cat "$T/ctl/lock" >/etc/nixos/flake.lock;;' \
  '*) exit 1;; esac'
stub "$T/sw/bin/nix-channel" 'cd /nix/var/nix/profiles/per-user/root' \
  'l=$(readlink channels); n=${l#channels-}; n=$(( ${n%-link} + 1 ))' \
  'mkdir -p "$T/chan/$n" "$T/chan/env-$n"; ln -sfn "$T/chan/$n" "$T/chan/env-$n/nixos"' \
  'ln -s "$T/chan/env-$n" "channels-$n-link"; ln -sfn "channels-$n-link" channels'
stub "$T/sw/bin/nix-env" 'cd /nix/var/nix/profiles' 'case "$*" in' \
  '*--set*) max=0; for l in system-*-link; do n=${l#system-}; n=${n%-link}; (( n > max )) && max=$n; done' \
  '  ln -s "${@: -1}" "system-$((max+1))-link"; ln -sfn "system-$((max+1))-link" system;;' \
  '*--switch-generation*) ln -sfn "system-${@: -1}-link" system;;' \
  '*--delete-generations*) echo $$ >"$T/state/delete-pid"; cur=$(readlink system); for l in system-*-link; do [[ $l == "$cur" ]] && continue; rm "$l"' \
  '    if [[ -e $T/ctl/slow-delete ]]; then touch "$T/state/delete-started"; sleep 1; fi; done;;' \
  'esac'
stub "$T/sw/bin/nix-store" 'case $(cat "$T/ctl/gc") in' \
  'fail) echo "error: collecting garbage failed" >&2; exit 1;;' \
  'hang) touch "$T/state/gc-started"; exec sleep 600;;' \
  '*) echo "0 store paths deleted, 0.00 MiB freed";; esac'
for name in git loginctl systemctl; do stub "$T/sw/bin/$name" 'exit 0'; done
bootmenu() { (cd /nix/var/nix/profiles && for l in system-*-link; do echo "$l"; done) >"$T/state/bootmenu"; }
mksys() { # a stand-in system whose switch-to-configuration logs and moves /run/current-system
  local s=$T/sys/$1
  mkdir -p "$s/bin"; ln -s "$T/sw" "$s/sw"
  stub "$s/bin/switch-to-configuration" "[[ -e \$T/ctl/switch-fail ]] && exit 1" \
    "[[ \$1 == switch ]] && ln -sfn $s /run/current-system" \
    '(cd /nix/var/nix/profiles && for l in system-*-link; do echo "$l"; done) >"$T/state/bootmenu"'
}
generation() { ln -sfn "$T/sys/$2" "/nix/var/nix/profiles/system-$1-link"; }
select_generation() { ln -sfn "system-$1-link" /nix/var/nix/profiles/system; }
running() { ln -sfn "$T/sys/$1" /run/current-system; [[ -n ${2:-} ]] && ln -sfn "$T/sys/$1" /run/booted-system; true; }
channels_at() { # n: root's channels profile at generation n, its generations links into the store as Nix makes them
  local d=/nix/var/nix/profiles/per-user/root
  rm -rf "$d"/channels*; mkdir -p "$T/chan/$1" "$T/chan/env-$1"
  ln -sfn "$T/chan/$1" "$T/chan/env-$1/nixos"; ln -s "$T/chan/env-$1" "$d/channels-$1-link"; ln -sfn "channels-$1-link" "$d/channels"
}
helper() { # args…: run the worker, print its result
  local dir; dir=$(mktemp -d /var/lib/yukimi/operations/op.XXXXXX)
  "$T/yukimi-helper" __worker "$dir" "$@" </dev/null 2>"$dir.stderr" || true
  cat "$dir/result.json"
}
helper_stopped() { # marker args…: run the worker, interrupt it once marker appears
  local marker=$1; shift
  local dir; dir=$(mktemp -d /var/lib/yukimi/operations/op.XXXXXX)
  rm -f "$marker"
  "$T/yukimi-helper" __worker "$dir" "$@" </dev/null 2>"$dir.stderr" &
  local pid=$! i=0
  until [[ -e $marker ]]; do sleep 0.02; i=$((i + 1)); ((i < 1500)) || { echo "never reached $marker"; exit 2; }; done
  kill -INT "$pid"; wait "$pid" || true
  cat "$dir/result.json"
}
helper_killed() { # marker args…: run the worker, SIGKILL it and the program it runs once marker appears (systemd's control-group kill)
  local marker=$1; shift
  local dir; dir=$(mktemp -d /var/lib/yukimi/operations/op.XXXXXX)
  rm -f "$marker" "$T/state/delete-pid"
  "$T/yukimi-helper" __worker "$dir" "$@" </dev/null 2>"$dir.stderr" &
  local pid=$! i=0
  until [[ -e $marker ]]; do sleep 0.02; i=$((i + 1)); ((i < 1500)) || { echo "never reached $marker"; exit 2; }; done
  kill -KILL "$pid"; [[ -e $T/state/delete-pid ]] && { kill -KILL "$(cat "$T/state/delete-pid")" 2>/dev/null || true; }; wait "$pid" || true
  echo "killed, result: $(cat "$dir/result.json" 2>/dev/null || echo none)"
}
check_isolated() { # nothing real is reachable through the helper's fixed paths
  [[ $(readlink -f /run/current-system/sw/bin/nix) == "$T/sw/bin/nix" ]]
  [[ ! -e /run/wrappers && ! -e /nix/var/nix/daemon-socket ]]
  grep -q REPRO-FAKE /etc/nixos/configuration.nix
  [[ $(readlink -f /nix/var/nix/profiles/system) == "$T/sys/"* ]]
}
FAILED=0
expect() { # description, actual, expected
  if [[ $2 == "$3" ]]; then echo "ok:   $1"; else echo "FAIL: $1"; echo "      expected: $3"; echo "      actual:   $2"; FAILED=1; fi
}
# ---- end of harness ---------------------------------------------------------

# Generations 1 and 2 are older than a week; 3 is current. The boot menu
# lists all three, as the last switch-to-configuration boot left it.
for s in S0 S1 S2; do mksys $s; done
printf '# REPRO-FAKE\n{ ... }: { }\n' >/etc/nixos/configuration.nix
setup() {
  generation 1 S0; generation 2 S1; generation 3 S2; select_generation 3; running S2 booted; bootmenu
}
links() { (cd /nix/var/nix/profiles && for l in system-*-link; do echo "$l"; done) | tr '\n' ' '; }
menu() { tr '\n' ' ' <"$T/state/bootmenu"; }
setup; check_isolated

echo "== control: Clean Up \"older than a week\" whose collection succeeds"
echo ok >"$T/ctl/gc"
echo "result: $(helper clean --older-than-days 7)"
expect "generations left" "$(links)" "system-3-link "
expect "boot menu lists only what exists" "$(menu)" "$(links)"

echo "== A: Clean Up \"older than a week\", Stop during nix-store --gc"
setup; echo hang >"$T/ctl/gc"; : >"$T/calls"
echo "result: $(helper_stopped "$T/state/gc-started" clean --older-than-days 7)"
expect "generations left" "$(links)" "system-3-link "
expect "boot menu lists only what exists after Stop" "$(menu)" "$(links)"

echo "== B: Clean Up \"older than a week\", nix-store --gc fails"
setup; echo fail >"$T/ctl/gc"
echo "result: $(helper clean --older-than-days 7)"
expect "boot menu lists only what exists after a failed collection" "$(menu)" "$(links)"

echo "== C: Clean Up \"older than a week\", Stop while the generations are being deleted"
setup; echo ok >"$T/ctl/gc"; touch "$T/ctl/slow-delete"
echo "result: $(helper_stopped "$T/state/delete-started" clean --older-than-days 7)"
rm "$T/ctl/slow-delete"
expect "boot menu lists only what exists after Stop during the deletion" "$(menu)" "$(links)"

# D-F: a clean-up killed (SIGKILL, as at a power cut, nothing of it runs
# after) between deleting generations and rewriting the boot menu.
record=/var/lib/yukimi/clean-up.json
recorded() { [[ -e $record ]] && echo recorded || echo gone; }
stale() { [[ $(menu) != "$(links)" ]] && echo stale || echo current; }

echo "== D: Clean Up killed while deleting; the unit's ExecStopPost runs the recovery"
setup; echo ok >"$T/ctl/gc"; touch "$T/ctl/slow-delete"
helper_killed "$T/state/delete-started" clean --older-than-days 7
rm "$T/ctl/slow-delete"
expect "the killed clean-up left its record" "$(recorded)" "recorded"
expect "the record names the deletion" "$(grep -o '"older_than_days":7' $record)" '"older_than_days":7'
expect "the boot menu is stale after the kill" "$(stale)" "stale"
"$T/yukimi-helper" __recover </dev/null 2>"$T/recover.log" || true
expect "boot menu lists only what exists after the ExecStopPost recovery" "$(menu)" "$(links)"
expect "the record is gone after the recovery" "$(recorded)" "gone"
"$T/yukimi-helper" __recover </dev/null 2>>"$T/recover.log" || true
expect "running the recovery again changes nothing" "$(menu)" "$(links)"

echo "== E: Clean Up killed while deleting; the next operation's start recovers (after a power cut)"
setup; touch "$T/ctl/slow-delete"
helper_killed "$T/state/delete-started" clean --older-than-days 7
rm "$T/ctl/slow-delete"
expect "the boot menu is stale after the kill" "$(stale)" "stale"
# The next operation's build fails, so only the recovery writes the menu.
echo fail >"$T/ctl/build"
echo "result: $(helper change --packages htop --mode auto)"
expect "boot menu lists only what exists after the next start" "$(menu)" "$(links)"
expect "the record is gone after the next start" "$(recorded)" "gone"

echo "== F: the recovery doesn't run while a live Clean Up holds the lock"
setup; echo ok >"$T/ctl/gc"; touch "$T/ctl/slow-delete"; rm -f "$T/state/delete-started"
dir=$(mktemp -d /var/lib/yukimi/operations/op.XXXXXX)
"$T/yukimi-helper" __worker "$dir" clean --older-than-days 7 </dev/null 2>"$dir.stderr" &
live=$! i=0
until [[ -e $T/state/delete-started ]]; do sleep 0.02; i=$((i + 1)); ((i < 1500)) || { echo "never started"; exit 2; }; done
: >"$T/calls"
"$T/yukimi-helper" __recover </dev/null 2>"$T/recover-busy.log" || true
expect "the recovery stood aside" "$(grep -c 'Not recovering now' "$T/recover-busy.log")" "1"
expect "the recovery didn't touch the boot menu" "$(grep -c switch-to-configuration "$T/calls")" "0"
expect "the live clean-up's record is still there" "$(recorded)" "recorded"
wait "$live" || true
rm "$T/ctl/slow-delete"
echo "result: $(cat "$dir/result.json")"
expect "boot menu lists only what exists after the live clean-up" "$(menu)" "$(links)"
expect "the live clean-up removed its record" "$(recorded)" "gone"

echo "== helper calls (A, B and C)"; cat "$T/calls"
exit $FAILED
