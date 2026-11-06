# Sekka's per-install identity, checked in the initrd before the root is
# mounted (sekka-identity.service, ../identity.nix).
#
# Every Sekka Mac image is the same bytes: its root filesystem and EFI system
# partition (ESP) carry the same label and filesystem UUIDs on every Mac, so
# only the GPT partition UUIDs the installer created tell this installation
# apart from another one on the same computer. The installer writes them into
# the first generation's boot entry (sekka.root=PARTUUID=… sekka.esp=PARTUUID=…);
# identity.nix carries them for every later generation. A udev rule names the
# two partitions /dev/disk/by-sekka/{root,esp} (role.sh), and the root is
# mounted from there or from /dev/disk/by-partuuid.
#
# This check stops the boot with a message saying what is wrong, rather than
# leaving systemd to wait 90 seconds for a device that cannot appear:
# - the arguments are missing, repeated, malformed, or still the image's
#   placeholders (the installer never wrote this installation's identity);
# - on a Mac, m1n1 says it started from another ESP
#   (/chosen/asahi,efi-system-partition);
# - a partition doesn't appear in time, or two partitions claim its UUID.
#
# It needs bash, coreutils and util-linux (lsblk). The environment variables
# below exist for tests (test-check.sh); the initrd uses the defaults.
set -u

cmdline_file=${SEKKA_CMDLINE:-/proc/cmdline}
dt_esp=${SEKKA_DT_ESP:-/proc/device-tree/chosen/asahi,efi-system-partition}
by_partuuid=${SEKKA_BY_PARTUUID:-/dev/disk/by-partuuid}
by_sekka=${SEKKA_BY_SEKKA:-/dev/disk/by-sekka}
timeout=${SEKKA_TIMEOUT:-20}
console=${SEKKA_CONSOLE:-/dev/console}
kmsg=${SEKKA_KMSG:-/dev/kmsg}
lsblk=${SEKKA_LSBLK:-lsblk}

fail() {
	local message="Sekka cannot start: $*"
	echo "<2>sekka-identity: $message" >"$kmsg" 2>/dev/null || true
	if [ -z "${SEKKA_CMDLINE:-}" ]; then
		# A console whose driver is a module (a VM's virtio console) is
		# only there once udev has loaded it; then the boot splash, which
		# would hide the message, goes. Emergency mode follows.
		udevadm settle --timeout=10 2>/dev/null || true
		if command -v plymouth >/dev/null 2>&1; then
			plymouth quit 2>/dev/null || true
		fi
	fi
	{
		echo
		echo "$message"
		echo "(sekka-identity.service in the initrd)"
		echo
	} >"$console" 2>/dev/null || true
	echo "$message" >&2
	exit 1
}

uuid_re='^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$'
words=()
read -r -a words <"$cmdline_file" || true

# The partition UUID in sekka.NAME=PARTUUID=…, which must appear exactly once.
argument() {
	local name=$1 count=0 value="" word
	for word in "${words[@]}"; do
		case $word in
		"sekka.$name="*)
			count=$((count + 1))
			value=${word#"sekka.$name="}
			;;
		esac
	done
	if [ "$count" = 0 ]; then
		fail "the boot entry has no sekka.$name= argument. This system mounts only the partitions the installer named for it; start the entry the installer wrote, or a later generation."
	fi
	if [ "$count" != 1 ]; then
		fail "the boot entry has sekka.$name= $count times; it must name one partition."
	fi
	case $value in
	*@SEKKA_*)
		fail "the boot entry still has the image's placeholder (sekka.$name=$value): the installer never wrote this installation's partition UUIDs into it. Install with Sekka's Mac installer, or put the GPT partition UUIDs of this installation's root and EFI system partition in the entry's sekka.root= and sekka.esp=."
		;;
	esac
	local uuid=${value#PARTUUID=}
	if [ "$uuid" = "$value" ] || ! [[ $uuid =~ $uuid_re ]]; then
		fail "sekka.$name=$value is not PARTUUID= and a lowercase GPT partition UUID."
	fi
	echo "$uuid"
}

# How many partitions udev knows with this GPT UUID, and which there are.
partitions() { "$lsblk" -rno PARTUUID 2>/dev/null; }
count_of() {
	local uuid=$1 n=0 p
	while read -r p; do
		[ "$p" = "$uuid" ] && n=$((n + 1))
	done < <(partitions)
	echo "$n"
}

root=$(argument root) || exit 1
esp=$(argument esp) || exit 1
[ "$root" != "$esp" ] || fail "sekka.root= and sekka.esp= name the same partition, $root."

# m1n1 names the ESP it was started from (from the stub's boot
# configuration), and U-Boot passes its device tree on. QEMU's has none.
if [ -r "$dt_esp" ]; then
	started=$(tr -d '\0' <"$dt_esp" | tr 'A-F' 'a-f')
	if [ "$started" != "$esp" ]; then
		fail "m1n1 started this system from the EFI system partition $started, but the boot entry names $esp (sekka.esp=). The entry belongs to another installation; start Sekka from its own startup volume."
	fi
	echo "m1n1 started from this installation's EFI system partition, $esp."
fi

wait_for() {
	local name=$1 uuid=$2 waited=0
	while [ ! -e "$by_partuuid/$uuid" ]; do
		if [ "$waited" -ge "$timeout" ]; then
			local seen
			seen=$(partitions | tr '\n' ' ')
			fail "no partition with the GPT UUID $uuid (sekka.$name=) appeared within $timeout seconds. Partitions present: ${seen:-none}. Is this installation's disk attached?"
		fi
		if [ "$waited" = 5 ]; then
			echo "Waiting for this installation's $name partition, $uuid..." >"$console" 2>/dev/null || true
		fi
		sleep 1
		waited=$((waited + 1))
	done
	local count
	count=$(count_of "$uuid")
	if [ "$count" -gt 1 ]; then
		fail "$count partitions carry the GPT UUID $uuid (sekka.$name=), so which one is this installation's is ambiguous. Detach the copied disk."
	fi
	# The udev rule's name for it (role.sh) must be the same partition.
	local alias=$by_sekka/$name n=0
	while [ ! -e "$alias" ] && [ "$n" -lt 5 ]; do
		sleep 1
		n=$((n + 1))
	done
	if [ ! -e "$alias" ] || [ "$(readlink -f "$alias")" != "$(readlink -f "$by_partuuid/$uuid")" ]; then
		fail "$alias does not name the partition $uuid: Sekka's udev rule (61-sekka-identity.rules) did not run."
	fi
	echo "This installation's $name partition: $uuid, $(readlink -f "$by_partuuid/$uuid")."
}

wait_for root "$root"
wait_for esp "$esp"
