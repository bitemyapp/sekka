# shellcheck shell=sh
# The M3 Pro's session graphics. Sourced (not run) by uwsm's environment
# preloader through /etc/xdg/uwsm/env.d/50-sekka-m3-gpu at the start of each
# uwsm session, Tatami's among them. uwsm exports what this sets, and unsets
# what it unsets, to the compositor and the systemd user manager, and undoes it
# when the session ends, so each login decides again.
#
# Adapted from Omarchy's mesa-m3 hooks (omacom/linux-aurora
# tools/aurora-sep/recipes/mesa-m3/mesa-m3-session-env and
# tools/aurora-sep/pro/mesa-m3-pro-session-env, release aurora-2026.10.17.20).
# The kernel refuses Nixpkgs' Mesa on the M3 GPU, and a compositor that renders
# through that GPU's render node with it stops at start. So every session
# renders in software (LIBGL_ALWAYS_SOFTWARE=1, which the system sets for all
# sessions) unless every check below passes, in this order:
#   opt-out          /etc/mesa-m3/disable, ~/.config/mesa-m3/disable, or
#                    mesa_m3=off on the kernel command line
#   not-supported    the device tree does not say apple,t6030 (M3 Pro)
#   no-gpu           not exactly one render node, or it is not the asahi
#                    driver's apple,agx-t6030 GPU
#   no-display       no native display card (apple,t6030-display-subsystem):
#                    m1n1's handoff is off
#   no-access        this user cannot open the render node (render group)
#   abi-mismatch     the GPU does not report the identity the Mesa was made
#                    for (T6030 G15S, two clusters, legacy HAL)
#   log-unreadable   this boot's kernel log cannot be read, so GPU faults
#                    cannot be ruled out
#   gpu-fault        the kernel log shows an M3 GPU or display fault this boot
#   previous-failed  a login on the GPU earlier this boot was never confirmed
#                    alive (sekka-m3-gpu-confirm.service), so there is no login
#                    loop when the compositor cannot run on it
#   active           the session uses the M3 Pro's Mesa
# Each login writes its decision to /run/user/UID/sekka-m3-gpu.state and the
# journal (tag sekka-m3-gpu). Omarchy's library, load and user-setup checks are
# left out: Nix pins the libraries and the kernel together.

_sm3_gate() {
	if [ -e "$_sm3_root/etc/mesa-m3/disable" ]; then
		echo "opt-out /etc/mesa-m3/disable exists"; return 1
	fi
	if [ -e "${XDG_CONFIG_HOME:-$HOME/.config}/mesa-m3/disable" ]; then
		echo "opt-out ${XDG_CONFIG_HOME:-$HOME/.config}/mesa-m3/disable exists"; return 1
	fi
	case " $(cat "$_sm3_root/proc/cmdline" 2>/dev/null) " in
	*" mesa_m3=off "*) echo "opt-out mesa_m3=off is on the kernel command line"; return 1 ;;
	esac
	if ! tr '\0' '\n' 2>/dev/null < "$_sm3_root/proc/device-tree/compatible" | grep -qx 'apple,t6030'; then
		echo "not-supported the device tree does not say apple,t6030"; return 1
	fi

	_sm3_nodes=0
	_sm3_node=
	for _sm3_n in "$_sm3_root"/sys/class/drm/renderD*; do
		[ -e "$_sm3_n" ] || continue
		_sm3_nodes=$((_sm3_nodes + 1))
		_sm3_node=$_sm3_n
	done
	if [ "$_sm3_nodes" -ne 1 ]; then
		echo "no-gpu expected exactly one render node, found $_sm3_nodes"; return 1
	fi
	_sm3_drv=$(basename "$(readlink -f "$_sm3_node/device/driver" 2>/dev/null)" 2>/dev/null)
	if [ "$_sm3_drv" != asahi ]; then
		echo "no-gpu render node ${_sm3_node##*/} is driven by '$_sm3_drv', not asahi"; return 1
	fi
	if ! tr '\0' '\n' 2>/dev/null < "$_sm3_node/device/of_node/compatible" | grep -qx 'apple,agx-t6030'; then
		echo "no-gpu render node ${_sm3_node##*/} is not the T6030 GPU"; return 1
	fi

	_sm3_dcp=
	for _sm3_c in "$_sm3_root"/sys/class/drm/card*; do
		case "${_sm3_c##*/}" in *-*) continue ;; esac
		if tr '\0' '\n' 2>/dev/null < "$_sm3_c/device/of_node/compatible" | grep -qx 'apple,t6030-display-subsystem'; then
			_sm3_dcp=${_sm3_c##*/}
		fi
	done
	if [ -z "$_sm3_dcp" ]; then
		echo "no-display no native display card (apple,t6030-display-subsystem)"; return 1
	fi

	_sm3_dev=$_sm3_root/dev/dri/${_sm3_node##*/}
	if [ ! -r "$_sm3_dev" ] || [ ! -w "$_sm3_dev" ]; then
		echo "no-access this user cannot open $_sm3_dev (it is for the render group)"; return 1
	fi
	if ! _sm3_out=$("${SEKKA_M3_GPU_ABI_CHECK:-@abiCheck@}" legacy "$_sm3_dev" 2>&1); then
		echo "abi-mismatch $_sm3_out"; return 1
	fi

	# journalctl exits 1 both for "no match" and for errors, so the log is read
	# whole and searched: a failed read or an empty log refuses.
	if ! _sm3_out=$(journalctl -k -b -q --no-pager -o cat 2>/dev/null) || [ -z "$_sm3_out" ]; then
		echo "log-unreadable this user cannot read this boot's kernel log"; return 1
	fi
	_sm3_faults=$(printf '%s\n' "$_sm3_out" | grep -E \
		-e 'M3 scheduler: execution failed|M3 G15[SG]: firmware crashed|M3 firmware error|M3 bank [0-9]' \
		-e 'M3 compute rejected|native DCP flip failed|DCP firmware crashed')
	case $? in
	0)
		echo "gpu-fault GPU acceleration is off until reboot: $(printf '%s\n' "$_sm3_faults" | tail -n 1)"
		return 1 ;;
	1) ;;
	*) echo "log-unreadable this boot's kernel log could not be searched"; return 1 ;;
	esac

	_sm3_boot=$(cat "$_sm3_root/proc/sys/kernel/random/boot_id" 2>/dev/null)
	_sm3_att=$_sm3_state_dir/attempt
	if [ -f "$_sm3_att" ] && [ "$(sed -n 's/^boot_id=//p' "$_sm3_att")" = "$_sm3_boot" ]; then
		_sm3_ran=$(sed -n 's/^run=//p' "$_sm3_att")
		if [ "$(sed -n 's/^boot_id=//p' "$_sm3_state_dir/session-ok" 2>/dev/null)" != "$_sm3_boot" ] ||
			[ "$(sed -n 's/^run=//p' "$_sm3_state_dir/session-ok" 2>/dev/null)" != "$_sm3_ran" ]; then
			echo "previous-failed the last login on the GPU this boot was never confirmed alive; to try again: rm $_sm3_att, then log in again"
			return 1
		fi
	fi
	echo "active render node $_sm3_dev, display $_sm3_dcp"
}

# SEKKA_M3_GPU_ROOT and SEKKA_M3_GPU_ABI_CHECK are for m3-gpu/test-session-env.sh.
_sm3_root=${SEKKA_M3_GPU_ROOT:-}
_sm3_state_dir=${XDG_STATE_HOME:-$HOME/.local/state}/sekka-m3-gpu
_sm3_run=$(cat "$_sm3_root/proc/sys/kernel/random/uuid" 2>/dev/null)
_sm3_decision=$(_sm3_gate)
_sm3_on=$?

if [ "$_sm3_on" -eq 0 ]; then
	unset LIBGL_ALWAYS_SOFTWARE MESA_M3_FALLBACK GALLIUM_DRIVER MESA_LOADER_DRIVER_OVERRIDE GBM_ALWAYS_SOFTWARE VK_ADD_DRIVER_FILES
	@nativeExports@
	export SEKKA_M3_GPU_RUN="$_sm3_run"
	# A login on the GPU counts as failed until sekka-m3-gpu-confirm.service
	# writes session-ok with this run beside it.
	mkdir -p "${_sm3_state_dir%/*}" 2>/dev/null &&
		{ [ -d "$_sm3_state_dir" ] || mkdir -m 0700 "$_sm3_state_dir" 2>/dev/null; } &&
		printf 'boot_id=%s\nrun=%s\n' "$(cat "$_sm3_root/proc/sys/kernel/random/boot_id")" "$_sm3_run" > "$_sm3_state_dir/attempt"
	_sm3_fallback=none
else
	export LIBGL_ALWAYS_SOFTWARE=1 MESA_M3_FALLBACK=software
	_sm3_fallback=software
fi

logger -t sekka-m3-gpu "decision=${_sm3_decision%% *} fallback=$_sm3_fallback run=$_sm3_run: ${_sm3_decision#* }" 2>/dev/null
if [ -d "$_sm3_root/run/user/$(id -u)" ]; then
	printf 'decision=%s\ndetail=%s\nfallback=%s\nrun=%s\n' "${_sm3_decision%% *}" "${_sm3_decision#* }" \
		"$_sm3_fallback" "$_sm3_run" > "$_sm3_root/run/user/$(id -u)/sekka-m3-gpu.state" 2>/dev/null
fi
unset -f _sm3_gate
unset _sm3_root _sm3_state_dir _sm3_run _sm3_decision _sm3_on _sm3_fallback _sm3_nodes _sm3_node _sm3_n _sm3_drv \
	_sm3_dcp _sm3_c _sm3_dev _sm3_out _sm3_faults _sm3_boot _sm3_att _sm3_ran
