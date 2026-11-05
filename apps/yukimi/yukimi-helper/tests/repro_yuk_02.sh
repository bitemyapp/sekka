#!/usr/bin/env bash
# YUK-02 (end to end): a Yukimi update whose activation fails after commit
# leaves the receipt 'activating' for S1. The person repairs the system with
# nixos-rebuild switch to S2 and restarts into it. sekka-update --status (the
# same completion::read the GUI uses) must not keep reporting the old failed
# activation. The restart is simulated by moving /run/booted-system and
# giving the receipt an earlier boot id (the real boot id can't change here).
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
# usage: run default yukimi bash repro_yuk_02.sh /path/to/yukimi-helper
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
  '*--delete-generations*) cur=$(readlink system); for l in system-*-link; do [[ $l == "$cur" ]] && continue; rm "$l"' \
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

for s in S0 S1 S2; do mksys $s; done
generation 1 S0; select_generation 1; running S0 booted; bootmenu
printf '# REPRO-FAKE\n{ ... }: { }\n' >/etc/nixos/configuration.nix
check_isolated
[[ -x $T/sekka-update ]]

echo "== Yukimi installs htop; switch-to-configuration fails after commit"
echo "ok S1" >"$T/ctl/build"; touch "$T/ctl/switch-fail"
echo "result: $(helper change --packages htop --mode auto)"
echo "receipt: $(cat /var/lib/yukimi/update.json)"
rm "$T/ctl/switch-fail"

echo "== outside Yukimi: nixos-rebuild switch to S2, then restart into it"
generation 3 S2; select_generation 3; running S2 booted
r=$(cat /var/lib/yukimi/update.json); b=$(cat /proc/sys/kernel/random/boot_id)
printf '%s' "${r//$b/boot-before-restart}" >/var/lib/yukimi/update.json
echo "receipt: $(cat /var/lib/yukimi/update.json)"
status=$("$T/sekka-update" --status --json)
echo "sekka-update --status --json: $status"
expect "advice after a healthy restart into S2" "$([[ $status == *'"action":"unknown"'* ]] && echo unknown || echo not-unknown)" "not-unknown"
exit $FAILED
