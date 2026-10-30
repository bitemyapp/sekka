#!/usr/bin/env bash
# Tests the M3 Pro session gate (session-env.sh, with its store paths) against
# fake T6030 roots: device tree, sysfs, render node, kernel log, state files.
# No GPU is needed. Usage: test-session-env.sh SESSION_ENV
set -u
gate=$1
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
uid=$(id -u)
fail=0

mkdir -p "$work/bin"
cat > "$work/bin/journalctl" <<'EOF'
#!/bin/sh
[ -n "${FAKE_JOURNAL_FAIL:-}" ] && exit 1
cat "$FAKE_JOURNAL"
EOF
printf '#!/bin/sh\n:\n' > "$work/bin/logger"
# shellcheck disable=SC2016 # expanded by the stub, not here
printf '#!/bin/sh\necho "profile=legacy chip=0x6030 G15S rev=0x11 match=${FAKE_ABI_MATCH:-1}"\n[ "${FAKE_ABI_MATCH:-1}" = 1 ]\n' > "$work/bin/abi-check"
chmod +x "$work/bin/"*

# A J514S as m1n1's handoff leaves it: the asahi GPU and the native display.
root() {
	r=$work/root
	rm -rf "${r:?}" "${work:?}/home"
	mkdir -p "$r/proc/device-tree" "$r/proc/sys/kernel/random" "$r/sys/class/drm" "$r/dev/dri" \
		"$r/etc" "$r/run/user/$uid" "$r/sys/bus/platform/drivers/asahi" "$work/home"
	echo boot-a > "$r/proc/sys/kernel/random/boot_id"
	echo run-1 > "$r/proc/sys/kernel/random/uuid"
	echo "init=/nix/store/x-nixos-system/init quiet" > "$r/proc/cmdline"
	printf '%s\0' apple,j514s apple,t6030 apple,arm-platform > "$r/proc/device-tree/compatible"
	d=$r/sys/devices/platform/gpu
	mkdir -p "$d/of_node" "$r/sys/class/drm/renderD128"
	printf '%s\0' apple,agx-t6030 apple,agx-g15s > "$d/of_node/compatible"
	ln -s "$r/sys/bus/platform/drivers/asahi" "$d/driver"
	ln -s "$d" "$r/sys/class/drm/renderD128/device"
	: > "$r/dev/dri/renderD128"
	chmod 0660 "$r/dev/dri/renderD128"
	d=$r/sys/devices/platform/display
	mkdir -p "$d/of_node" "$r/sys/class/drm/card2"
	printf '%s\0' apple,t6030-display-subsystem > "$d/of_node/compatible"
	ln -s "$d" "$r/sys/class/drm/card2/device"
	printf 'Booting Linux\nasahi gpu: M3 G15S ready\n' > "$work/journal"
}

# check NAME DECISION GPU(yes|no): source the gate as uwsm's preloader does,
# with the system's software default, and compare its decision and exports.
check() {
	name=$1 want=$2 gpu=$3
	# shellcheck disable=SC2016 # $1 is the inner shell's
	env=$(env -i HOME="$work/home" PATH="$work/bin:$PATH" LIBGL_ALWAYS_SOFTWARE=1 MESA_M3_FALLBACK=software \
		SEKKA_M3_GPU_ROOT="$work/root" SEKKA_M3_GPU_ABI_CHECK="$work/bin/abi-check" \
		FAKE_JOURNAL="$work/journal" FAKE_ABI_MATCH="${FAKE_ABI_MATCH:-1}" FAKE_JOURNAL_FAIL="${FAKE_JOURNAL_FAIL:-}" \
		sh -c '. "$1" && env' sh "$gate")
	got=$(sed -n 's/^decision=//p' "$work/root/run/user/$uid/sekka-m3-gpu.state")
	ok=1
	[ "$got" = "$want" ] || ok=0
	if [ "$gpu" = yes ]; then
		grep -qx 'LIBGL_ALWAYS_SOFTWARE=0' <<<"$env" || ok=0
		grep -q '^LD_LIBRARY_PATH=/nix/store/[^:]*-mesa-m3-native-pro-' <<<"$env" || ok=0
		grep -qx 'ASAHI_M3_EXPERIMENTAL=1' <<<"$env" || ok=0
		grep -q '^VK_DRIVER_FILES=/nix/store/.*-mesa-m3-pro-vulkan-.*/asahi_icd.aarch64.json$' <<<"$env" || ok=0
		grep -qx 'SEKKA_M3_GPU_RUN=run-1' <<<"$env" || ok=0
	else
		grep -qx 'LIBGL_ALWAYS_SOFTWARE=1' <<<"$env" || ok=0
		grep -q '^LD_LIBRARY_PATH=' <<<"$env" && ok=0
		grep -q '^SEKKA_M3_GPU_RUN=' <<<"$env" && ok=0
	fi
	# Nothing of the gate's own is left behind.
	grep -q '^_sm3_' <<<"$env" && ok=0
	if [ "$ok" = 1 ]; then
		echo "ok   $name: $got"
	else
		echo "FAIL $name: decision '$got', wanted '$want' (GPU $gpu)"
		sed -n 's/^detail=/     /p' "$work/root/run/user/$uid/sekka-m3-gpu.state"
		fail=1
	fi
}

root; check "all checks pass" active yes
[ "$(sed -n 's/^run=//p' "$work/home/.local/state/sekka-m3-gpu/attempt")" = run-1 ] ||
	{ echo "FAIL the attempt does not record its run"; fail=1; }
check "an unconfirmed login on the GPU this boot" previous-failed no
printf 'boot_id=boot-a\nrun=run-1\n' > "$work/home/.local/state/sekka-m3-gpu/session-ok"
check "the last login on the GPU confirmed" active yes
rm "$work/home/.local/state/sekka-m3-gpu/session-ok"
echo boot-b > "$work/root/proc/sys/kernel/random/boot_id"
check "an unconfirmed login of an earlier boot" active yes

root; echo "quiet mesa_m3=off" > "$work/root/proc/cmdline"; check "mesa_m3=off" opt-out no
root; mkdir -p "$work/root/etc/mesa-m3"; : > "$work/root/etc/mesa-m3/disable"; check "/etc/mesa-m3/disable" opt-out no
root; mkdir -p "$work/home/.config/mesa-m3"; : > "$work/home/.config/mesa-m3/disable"; check "the user's mesa-m3/disable" opt-out no
root; printf '%s\0' apple,j314s apple,t6000 > "$work/root/proc/device-tree/compatible"; check "an M1 Pro" not-supported no
root; rm -r "$work/root/sys/class/drm/renderD128"; check "no render node" no-gpu no
root; rm -r "$work/root/sys/class/drm/card2"; check "no native display (no handoff)" no-display no
root; chmod 0000 "$work/root/dev/dri/renderD128"; check "not in the render group" no-access no
root; FAKE_ABI_MATCH=0 check "another GPU identity" abi-mismatch no
root; echo 'asahi gpu: M3 firmware error: halted' >> "$work/journal"; check "a GPU fault this boot" gpu-fault no
root; FAKE_JOURNAL_FAIL=1 check "an unreadable kernel log" log-unreadable no

exit "$fail"
