#!/usr/bin/env bash
# check.sh and role.sh against fake roots: kernel command lines, a device tree
# property, /dev/disk/by-partuuid and by-sekka links and lsblk's list.
# Usage: test-check.sh DIR (the directory with check.sh and role.sh)
set -euo pipefail
here=$(cd "${1:-$(dirname "$0")}" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

root=63d89be1-72f7-4aff-9679-09cafa7e0c2d
esp=910de5ca-7390-4716-9a54-8028916e4d78
other=0b8a6d1e-2f61-4c55-8f7e-3a3c2c1d9e01
failures=0

# A fake disk: by-partuuid links to fake devices, by-sekka links as udev's
# rule would make them, and lsblk printing the GPT UUIDs present.
disk() {
	rm -rf "$work/dev" && mkdir -p "$work/dev/by-partuuid" "$work/dev/by-sekka"
	: >"$work/partuuids"
	local i=0 u
	for u in "$@"; do
		i=$((i + 1))
		: >"$work/dev/vda$i"
		ln -sf "$work/dev/vda$i" "$work/dev/by-partuuid/$u"
		echo "$u" >>"$work/partuuids"
	done
	printf '#!/bin/sh\ncat %s\n' "$work/partuuids" >"$work/lsblk"
	chmod +x "$work/lsblk"
}
sekka_link() { ln -sf "$work/dev/by-partuuid/$2" "$work/dev/by-sekka/$1"; }

# expect pass|fail PATTERN CMDLINE [DT]
expect() {
	local want=$1 pattern=$2 cmdline=$3 dt=${4:-}
	printf '%s\n' "$cmdline" >"$work/cmdline"
	rm -f "$work/dt"
	[ -z "$dt" ] || printf '%s\0' "$dt" >"$work/dt"
	local status=0 started=$SECONDS
	SEKKA_CMDLINE=$work/cmdline SEKKA_DT_ESP=$work/dt SEKKA_BY_PARTUUID=$work/dev/by-partuuid \
		SEKKA_BY_SEKKA=$work/dev/by-sekka SEKKA_TIMEOUT=2 SEKKA_CONSOLE=/dev/null SEKKA_KMSG=/dev/null \
		SEKKA_LSBLK=$work/lsblk bash "$here/check.sh" >"$work/out" 2>&1 || status=$?
	local got=pass
	[ "$status" = 0 ] || got=fail
	if [ "$got" != "$want" ] || ! grep -q -- "$pattern" "$work/out"; then
		echo "FAIL: expected $want matching '$pattern' for: $cmdline"
		sed 's/^/    /' "$work/out"
		failures=$((failures + 1))
	else
		echo "ok: $want in $((SECONDS - started))s: $(tail -n1 "$work/out")"
	fi
}

good="init=/x sekka.root=PARTUUID=$root sekka.esp=PARTUUID=$esp quiet"

disk "$root" "$esp" "$other"
sekka_link root "$root"
sekka_link esp "$esp"
expect pass "esp partition: $esp" "$good"
# The J514S's own property (2026-11-06, /proc/device-tree/chosen).
expect pass "m1n1 started from this installation's" "$good" "$esp"
expect pass "m1n1 started from this installation's" "$good" "${esp^^}"
expect fail "m1n1 started this system from the EFI system partition $other" "$good" "$other"
expect fail "still has the image's placeholder" "init=/x sekka.root=PARTUUID=@SEKKA_ROOT_PARTUUID@ sekka.esp=PARTUUID=@SEKKA_ESP_PARTUUID@"
expect fail "no sekka.root= argument" "init=/x sekka.esp=PARTUUID=$esp"
expect fail "sekka.esp= 2 times" "$good sekka.esp=PARTUUID=$esp"
expect fail "is not PARTUUID=" "init=/x sekka.root=$root sekka.esp=PARTUUID=$esp"
expect fail "is not PARTUUID=" "init=/x sekka.root=PARTUUID=${root^^} sekka.esp=PARTUUID=$esp"
expect fail "name the same partition" "init=/x sekka.root=PARTUUID=$esp sekka.esp=PARTUUID=$esp"

# Missing: a clear message after the timeout, not a 90-second device wait.
disk "$esp" "$other"
sekka_link esp "$esp"
expect fail "no partition with the GPT UUID $root (sekka.root=) appeared within 2 seconds. Partitions present: $esp $other" "$good"

# Two partitions claiming the root's UUID (a copied disk).
disk "$root" "$esp" "$root"
sekka_link root "$root"
sekka_link esp "$esp"
expect fail "2 partitions carry the GPT UUID $root" "$good"

# udev's link missing or pointing elsewhere.
disk "$root" "$esp"
sekka_link esp "$esp"
expect fail "by-sekka/root does not name the partition $root" "$good"
sekka_link root "$esp"
expect fail "by-sekka/root does not name the partition $root" "$good"

# role.sh: udev's IMPORT{program}.
role() {
	printf '%s\n' "$2" >"$work/cmdline"
	SEKKA_CMDLINE=$work/cmdline bash "$here/role.sh" "$1"
}
check_role() {
	local got
	got=$(role "$1" "$2")
	if [ "$got" != "$3" ]; then
		echo "FAIL: role.sh $1 printed '$got', expected '$3'"
		failures=$((failures + 1))
	else
		echo "ok: role.sh ${1:0:8} -> '${got}'"
	fi
}
check_role "$root" "$good" "SEKKA_PARTITION=root"
check_role "$esp" "$good" "SEKKA_PARTITION=esp"
check_role "$other" "$good" ""
check_role "$root" "quiet" ""
check_role '*' "$good" ""
check_role "" "$good" ""

if [ "$failures" != 0 ]; then
	echo "$failures failures"
	exit 1
fi
echo "all identity checks passed"
