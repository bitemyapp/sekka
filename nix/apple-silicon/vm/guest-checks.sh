# Run inside the product image's test VM (boot-image-in-vm.sh), as root, from
# the test disk it attaches: bash guest-checks.sh PHASE. The test disk also
# holds expect.env, written by the harness: the GPT partition UUIDs it gave
# disk A (the installation), disk B (a byte copy with other GPT UUIDs) and
# disk D (a stand-in for a developer install), both images' root labels,
# UUIDs and ESP serials, the image's toplevel and lock digest, and the
# registration's path count.
#
# Each check prints `ok NAME: detail` or `FAIL NAME: detail`; the last line
# is `result PHASE pass|fail`. Nothing here repairs anything.
set -u
here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=/dev/null
. "$here/expect.env"
phase=$1
failures=0
export PAGER=cat SYSTEMD_PAGER=cat SYSTEMD_COLORS=0 TERM=dumb

ok() { echo "ok $1: $2"; }
bad() {
	echo "FAIL $1: $2"
	failures=$((failures + 1))
}
check() { # NAME DETAIL COMMAND...
	local name=$1 detail=$2
	shift 2
	if "$@"; then ok "$name" "$detail"; else bad "$name" "$detail"; fi
}
# The VM's console arguments in the entries NixOS wrote, after they were
# checked: a change to this VM's disk only, so the next boot has its consoles.
vm_entries() {
	local e
	for e in /boot/loader/entries/*.conf; do
		grep -q systemd.debug_shell "$e" || sed -i "s|^options .*|& $VM_ARGS|" "$e"
	done
	sync
}
partuuid_of() { blkid -p -s PART_ENTRY_UUID -o value "$1" 2>/dev/null; }
mounted_partuuid() { partuuid_of "$(findmnt -n -o SOURCE --target "$1")"; }
nixq() { nix --extra-experimental-features 'nix-command flakes' "$@"; }

running() {
	local state screen
	# The login screen, with the VM's GPU standing in for the Mac's
	# (login-screen.sh), before it waits out Plymouth and gives up.
	if screen=$(bash "$here/login-screen.sh" 2>&1); then
		ok login-screen "$(echo "$screen" | tr '\n' ' ')"
	else
		bad login-screen "$(echo "$screen" | tr '\n' ' ')"
	fi
	state=$(timeout 300 systemctl is-system-running --wait 2>&1)
	check system-running "systemd reports '$state'" test "$state" = running
	local failed
	failed=$(systemctl --failed --no-legend --plain | awk '{print $1}' | tr '\n' ' ')
	check no-failed-units "failed units: ${failed:-none}" test -z "$failed"
	check graphical "graphical.target and display-manager.service: $(systemctl is-active graphical.target display-manager.service | tr '\n' ' ')" \
		systemctl is-active -q graphical.target display-manager.service
}

identity() { # the partitions this boot mounted are disk A's, by GPT UUID
	local cmdline root esp
	cmdline=$(cat /proc/cmdline)
	echo "cmdline: $cmdline"
	check cmdline-names-a "sekka.root= and sekka.esp= name disk A" \
		grep -q "sekka.root=PARTUUID=$ROOT_A sekka.esp=PARTUUID=$ESP_A" /proc/cmdline
	root=$(mounted_partuuid /)
	esp=$(mounted_partuuid /boot)
	check root-is-a "/ is $(findmnt -n -o SOURCE /) with GPT UUID $root (disk A's: $ROOT_A)" test "$root" = "$ROOT_A"
	check esp-is-a "/boot is $(findmnt -n -o SOURCE /boot) with GPT UUID $esp (disk A's: $ESP_A)" test "$esp" = "$ESP_A"
	check esp-has-handoff "/boot/sekka/install.json names installation $INSTALLATION" \
		grep -q "\"installation\":\"$INSTALLATION\"" /boot/sekka/install.json
	echo "fstab: $(grep -E ' (/|/boot) ' /etc/fstab | awk '{print $1, $2}' | tr '\n' ';')"
	journalctl -b -o cat -u sekka-identity.service | sed 's/^/initrd identity check: /'
}

case $phase in
first | second | updated | rolled-back) running ;;
esac

case $phase in
first)
	identity
	check booted-image "the booted system is the image's: $(readlink -f /run/booted-system)" \
		test "$(readlink -f /run/booted-system)" = "$TOPLEVEL"
	# The by-sekka names come from the command line, not from labels.
	check by-sekka "/dev/disk/by-sekka/root and esp resolve to disk A's partitions" \
		test "$(readlink -f /dev/disk/by-sekka/root):$(readlink -f /dev/disk/by-sekka/esp)" = \
		"$(readlink -f "/dev/disk/by-partuuid/$ROOT_A"):$(readlink -f "/dev/disk/by-partuuid/$ESP_A")"
	# Disk B: the same image bytes, its own GPT UUIDs, present but unused.
	for u in "$ROOT_B" "$ESP_B"; do
		dev=$(readlink -f "/dev/disk/by-partuuid/$u")
		check "b-present-not-mounted-${u:0:8}" "disk B's $dev ($u) is present and not mounted" \
			bash -c "[ -b '$dev' ] && ! findmnt -n -S '$dev' >/dev/null"
	done
	# A and B share the product image's root label and filesystem UUID
	# (byte copies), and so their ESPs' serial, which no developer install
	# mounts by.
	labels=$(blkid -t "LABEL=$PRODUCT_LABEL" -o device | sort | tr '\n' ' ')
	fsuuids=$(blkid -t "UUID=$PRODUCT_UUID" -o device | sort | tr '\n' ' ')
	check product-names-shared "label $PRODUCT_LABEL is on: $labels; filesystem UUID $PRODUCT_UUID on: $fsuuids" \
		test "$(echo "$labels" | wc -w):$(echo "$fsuuids" | wc -w)" = "2:2"
	# Disk D, a stand-in for a developer install beside this one: its names
	# lead to its own partitions only, as its system mounts them, and this
	# system's partitions carry none of them.
	dev_root=$(readlink -f "/dev/disk/by-partuuid/$ROOT_D")
	dev_esp=$(readlink -f "/dev/disk/by-partuuid/$ESP_D")
	check developer-root-label "label $DEVELOPER_LABEL is only on disk D's root ($dev_root): $(blkid -t "LABEL=$DEVELOPER_LABEL" -o device | tr '\n' ' ')" \
		test "$(blkid -t "LABEL=$DEVELOPER_LABEL" -o device):$(readlink -f "/dev/disk/by-label/$DEVELOPER_LABEL")" = "$dev_root:$dev_root"
	check developer-esp-serial "volume ID $DEVELOPER_SERIAL is only on disk D's ESP ($dev_esp): $(blkid -t "UUID=$DEVELOPER_SERIAL" -o device | tr '\n' ' ')" \
		test "$(blkid -t "UUID=$DEVELOPER_SERIAL" -o device):$(readlink -f "/dev/disk/by-uuid/$DEVELOPER_SERIAL")" = "$dev_esp:$dev_esp"
	check developer-root-uuid "filesystem UUID $DEVELOPER_UUID is only on disk D's root" \
		test "$(blkid -t "UUID=$DEVELOPER_UUID" -o device)" = "$dev_root"
	check d-not-mounted "disk D's partitions are not mounted" \
		bash -c "! findmnt -n -S '$dev_root' >/dev/null && ! findmnt -n -S '$dev_esp' >/dev/null"
	loader=$(python3 -c 'import glob,sys
for f in glob.glob("/sys/firmware/efi/efivars/LoaderDevicePartUUID-*"):
    print(open(f,"rb").read()[4:].decode("utf-16-le").strip("\0").lower())' 2>/dev/null)
	echo "systemd-boot was loaded from the ESP with GPT UUID: ${loader:-unknown} (A: $ESP_A, B: $ESP_B)"
	# The store, registered once from /nix-path-registration.
	check registered "/nix-path-registration consumed; system profile $(readlink /nix/var/nix/profiles/system)" \
		bash -c "[ ! -e /nix-path-registration ] && [ \"\$(readlink /nix/var/nix/profiles/system)\" = system-1-link ]"
	n=$(nixq path-info --all | wc -l)
	check registered-count "$n valid store paths (the image registers $REGISTERED)" test "$n" = "$REGISTERED"
	journalctl -b -o cat -u sekka-image-first-boot.service | sed 's/^/registration: /'
	# The sources offline evaluation reads survive a garbage collection
	# (asked what it would delete; nothing is deleted).
	dead=$(nix-store --gc --print-dead 2>/dev/null)
	kept=0
	for s in $SEEDS; do grep -qx "$s" <<<"$dead" || kept=$((kept + 1)); done
	check seeds-rooted "a garbage collection would keep $kept of the $(echo "$SEEDS" | wc -w) seeded sources" \
		test "$kept" = "$(echo "$SEEDS" | wc -w)"
	# The root filesystem grew into its partition.
	part=$(blockdev --getsize64 "/dev/disk/by-partuuid/$ROOT_A")
	fs=$(df -B1 --output=size / | tail -1 | tr -d ' ')
	check root-grew "ext4 is $fs bytes in a $part-byte partition; root.img was $ROOT_IMAGE_BYTES" \
		python3 -c "import sys; fs,part,img=map(int,sys.argv[1:]); sys.exit(not (fs > img + 3 * 2**30 and fs > part * 0.9))" "$fs" "$part" "$ROOT_IMAGE_BYTES"
	# Apple's firmware, here the harness's stand-in, handed to the system.
	check firmware "$(findmnt -n -o FSTYPE /lib/firmware/vendor) at /lib/firmware/vendor: $(ls /lib/firmware/vendor | tr '\n' ' ')" \
		bash -c '[ "$(findmnt -n -o FSTYPE /lib/firmware/vendor)" = tmpfs ] && [ -e /lib/firmware/vendor/sekka-vm-test.txt ]'
	# identity.nix, written once from the handoff.
	journalctl -b -o cat -u sekka-install-identity.service | sed 's/^/identity record: /'
	check identity-nix "/etc/nixos/identity.nix names disk A's partitions" \
		bash -c "grep -q 'root = \"$ROOT_A\";' /etc/nixos/identity.nix && grep -q 'esp = \"$ESP_A\";' /etc/nixos/identity.nix"
	sed 's/^/identity.nix: /' /etc/nixos/identity.nix
	# /etc/nixos as shipped: its lock, then the image's own system, offline
	# (this VM has no network device).
	echo "/etc/nixos: $(ls /etc/nixos | tr '\n' ' ')"
	check shipped-lock "flake.lock is the image's ($LOCK_SHA256)" \
		test "$(sha256sum /etc/nixos/flake.lock | cut -d' ' -f1)" = "$LOCK_SHA256"
	rm -rf /tmp/shipped && mkdir /tmp/shipped
	cp /etc/nixos/flake.nix /etc/nixos/flake.lock /etc/nixos/configuration.nix /etc/nixos/imports.nix /tmp/shipped/
	export XDG_CACHE_HOME=/tmp/empty-cache
	rm -rf "$XDG_CACHE_HOME"
	start=$SECONDS
	evaluated=$(nixq path-info --offline --no-update-lock-file \
		"path:/tmp/shipped#nixosConfigurations.sekka.config.system.build.toplevel" 2>/tmp/eval.err)
	tail -n 5 /tmp/eval.err | sed 's/^/evaluation: /'
	check offline-evaluation "the shipped flake and lock evaluate offline, in $((SECONDS - start))s, to $evaluated" \
		test "$evaluated" = "$TOPLEVEL"
	start=$SECONDS
	(cd /tmp/shipped && nixos-rebuild build --flake "path:/tmp/shipped#sekka" --no-update-lock-file --offline) >/tmp/rebuild.log 2>&1
	tail -n 5 /tmp/rebuild.log | sed 's/^/rebuild: /'
	check no-change-rebuild "nixos-rebuild build offline, in $((SECONDS - start))s, gives $(readlink -f /tmp/shipped/result)" \
		test "$(readlink -f /tmp/shipped/result)" = "$TOPLEVEL"
	check lock-unchanged "the rebuild left flake.lock as shipped" \
		test "$(sha256sum /tmp/shipped/flake.lock | cut -d' ' -f1)" = "$LOCK_SHA256"
	# With identity.nix the configuration evaluates too (a new system).
	drv=$(nixq path-info --derivation --offline --no-update-lock-file \
		"path:/etc/nixos#nixosConfigurations.sekka.config.system.build.toplevel" 2>/tmp/eval2.err)
	tail -n 3 /tmp/eval2.err | sed 's/^/evaluation with identity.nix: /'
	check identity-evaluates "/etc/nixos with identity.nix evaluates offline to $drv" test -n "$drv"
	# No account and no usable password; first-boot setup waiting.
	# People's accounts as setup and the login screen count them: a login
	# shell and a uid from 1000 (Nix's build users have no login shell).
	people=$(getent passwd | awk -F: '$3 >= 1000 && $3 < 60000 && $7 !~ /nologin$/ {print $1}' | tr '\n' ' ')
	check no-accounts "accounts with a login shell and a uid from 1000: ${people:-none}" test -z "$people"
	check root-locked "root's password field: $(awk -F: '$1 == "root" {print $2}' /etc/shadow)" \
		bash -c "awk -F: '\$1 == \"root\" {exit !(\$2 ~ /^!/)}' /etc/shadow"
	if [ -e /etc/sekka-setup/helper.json ]; then
		check setup-waiting "first-boot setup's socket: $(systemctl is-active sekka-setup.socket)" \
			systemctl is-active -q sekka-setup.socket
		# What setup checks before it finishes: the flake, with the setup.nix
		# it writes, evaluates offline under the new host name.
		rm -rf /tmp/with-setup && mkdir /tmp/with-setup
		cp /etc/nixos/flake.nix /etc/nixos/flake.lock /etc/nixos/configuration.nix /etc/nixos/imports.nix /etc/nixos/identity.nix /tmp/with-setup/
		sed -e "s/63d89be1-72f7-4aff-9679-09cafa7e0c2d/$ROOT_A/" -e "s/910de5ca-7390-4716-9a54-8028916e4d78/$ESP_A/" \
			"$here/setup.nix" >/tmp/with-setup/setup.nix
		start=$SECONDS
		drv=$(nixq path-info --derivation --offline --no-update-lock-file \
			"path:/tmp/with-setup#nixosConfigurations.analytical.config.system.build.toplevel" 2>/tmp/eval3.err)
		tail -n 3 /tmp/eval3.err | sed 's/^/evaluation with setup.nix: /'
		check setup-evaluates "with setup's sample setup.nix (host analytical) the flake evaluates offline, in $((SECONDS - start))s, to $drv" test -n "$drv"
		# The first update after setup, offline, for a language and keyboard
		# other than the image's: everything it rebuilds must be buildable
		# from what the image holds.
		sed -i -e 's/"en_GB.UTF-8"/"de_DE.UTF-8"/' -e 's/layout = "gb"/layout = "de"/' /tmp/with-setup/setup.nix
		grep -E 'defaultLocale|layout' /tmp/with-setup/setup.nix | sed 's/^/setup.nix for the build: /'
		start=$SECONDS
		(cd /tmp/with-setup && nixos-rebuild build --flake "path:/tmp/with-setup#analytical" --no-update-lock-file --offline) >/tmp/setup-build.log 2>&1
		status=$?
		tail -n 15 /tmp/setup-build.log | sed 's/^/build with setup.nix: /'
		check setup-builds-offline "with setup.nix for de_DE and de, nixos-rebuild build offline exited $status in $((SECONDS - start))s" test "$status" = 0
	fi
	# boot.bin as the image installed it, and its record.
	check boot-bin-record "m1n1/boot.bin is the record's current copy" python3 -c '
import hashlib, json
r = json.load(open("/boot/sekka/boot-bin.json"))
d = "sha256:" + hashlib.sha256(open("/boot/m1n1/boot.bin", "rb").read()).hexdigest()
raise SystemExit(r["current"]["digest"] != d or r["pending"] is not None)'
	check boot-bin-booted "sekka-boot-bin.service recorded this start's boot.bin as booted" python3 -c '
import json
raise SystemExit(json.load(open("/boot/sekka/boot-bin.json"))["current"]["booted"] is not True)'
	;;

second)
	identity
	check booted-image "the booted system is still the image's" test "$(readlink -f /run/booted-system)" = "$TOPLEVEL"
	check registered-once "sekka-image-first-boot did not run again: ConditionResult=$(systemctl show -p ConditionResult --value sekka-image-first-boot.service)" \
		test "$(systemctl show -p ConditionResult --value sekka-image-first-boot.service)" = no
	n=$(nixq path-info --all | wc -l)
	echo "valid store paths now: $n (image: $REGISTERED; the first boot's evaluation and build add derivations)"
	journalctl -b -o cat -u sekka-install-identity.service | sed 's/^/identity record: /'
	check identity-kept "identity.nix was checked, not rewritten" \
		bash -c "journalctl -b -o cat -u sekka-install-identity.service | grep -q 'already names'"
	# The replacement procedure against this VM's real FAT filesystem, in a
	# directory of its own: the real m1n1/boot.bin isn't touched.
	tool=$here/boot_bin.py
	rm -rf /boot/vm-boot-bin && mkdir -p /boot/vm-boot-bin
	head -c 3000000 /dev/urandom >/tmp/one.bin
	head -c 2500000 /dev/urandom >/tmp/two.bin
	python3 "$tool" install --esp /boot/vm-boot-bin --source /tmp/one.bin >/tmp/bb.log 2>&1
	python3 "$tool" install --esp /boot/vm-boot-bin --source /tmp/two.bin >>/tmp/bb.log 2>&1
	python3 "$tool" install --esp /boot/vm-boot-bin --source /tmp/two.bin >>/tmp/bb.log 2>&1
	python3 "$tool" restore-previous --esp /boot/vm-boot-bin >>/tmp/bb.log 2>&1
	sed 's/^/boot.bin on FAT: /' /tmp/bb.log
	check boot-bin-on-fat "replace, no-op and restore on vfat leave one.bin active and two.bin previous" bash -c \
		"cmp -s /tmp/one.bin /boot/vm-boot-bin/m1n1/boot.bin && cmp -s /tmp/two.bin /boot/vm-boot-bin/m1n1/boot.bin.previous && ! ls /boot/vm-boot-bin/m1n1/*.new 2>/dev/null"
	rm -rf /boot/vm-boot-bin
	# A controlled update: the owner adds a module with one marker file to
	# configuration.nix, built offline with identity.nix as the next
	# generation. (Setup itself builds no generation.)
	cp "$here/vm-update.nix" /etc/nixos/vm-update.nix
	sed -i 's|^    ./imports.nix$|&\n    ./vm-update.nix|' /etc/nixos/configuration.nix
	check update-imported "configuration.nix imports vm-update.nix" grep -q '^    ./vm-update.nix$' /etc/nixos/configuration.nix
	start=$SECONDS
	nixos-rebuild boot --flake /etc/nixos#sekka --no-update-lock-file --offline >/tmp/update.log 2>&1
	status=$?
	tail -n 25 /tmp/update.log | sed 's/^/update: /'
	check update-built "nixos-rebuild boot offline exited $status in $((SECONDS - start))s" test "$status" = 0
	nix-env --list-generations -p /nix/var/nix/profiles/system | sed 's/^/generations: /'
	for f in /boot/loader/loader.conf /boot/loader/entries/*.conf; do
		sed "s|^|$f: |" "$f"
		echo
	done
	check entries-filled "no boot entry still has a placeholder" bash -c '! grep -l @SEKKA_ /boot/loader/entries/*.conf'
	check entries-name-a "every entry names disk A" bash -c \
		"for e in /boot/loader/entries/*.conf; do grep -q 'sekka.root=PARTUUID=$ROOT_A sekka.esp=PARTUUID=$ESP_A' \$e || exit 1; done"
	check entries-firmware "every entry loads the firmware archive first" bash -c \
		'for e in /boot/loader/entries/*.conf; do [ "$(grep -m1 ^initrd $e)" = "initrd /vendorfw/firmware.cpio" ] || exit 1; done'
	python3 "$here/boot_bin.py" status --esp /boot | sed 's/^/boot.bin status: /'
	vm_entries
	;;

updated)
	identity
	check booted-generation-2 "booted $(readlink -f /run/booted-system), system-2-link is $(readlink -f /nix/var/nix/profiles/system-2-link)" \
		test "$(readlink -f /run/booted-system)" = "$(readlink -f /nix/var/nix/profiles/system-2-link)"
	check update-applied "the update's marker: $(cat /etc/sekka-vm-update 2>&1)" test -e /etc/sekka-vm-update
	check partitions-by-partuuid "/ and /boot are mounted by partition UUID from identity.nix" \
		grep -q "^/dev/disk/by-partuuid/$ROOT_A / " /etc/fstab
	check no-identity-service "generation 2 has no first-start identity service" \
		bash -c '! systemctl cat sekka-install-identity.service >/dev/null 2>&1'
	# Roll back: the image's generation, whose own boot loader installation
	# runs (and puts its boot.bin back if it differs).
	nixos-rebuild boot --rollback >/tmp/rollback.log 2>&1
	status=$?
	tail -n 15 /tmp/rollback.log | sed 's/^/rollback: /'
	check rollback-staged "nixos-rebuild boot --rollback exited $status" test "$status" = 0
	sed 's/^/loader.conf: /' /boot/loader/loader.conf
	check entries-filled "no boot entry still has a placeholder" bash -c '! grep -l @SEKKA_ /boot/loader/entries/*.conf'
	python3 "$here/boot_bin.py" status --esp /boot | sed 's/^/boot.bin status: /'
	vm_entries
	;;

rolled-back)
	identity
	check booted-image "the image's generation is back: $(readlink -f /run/booted-system)" \
		test "$(readlink -f /run/booted-system)" = "$TOPLEVEL"
	check update-gone "the update's marker is gone" test ! -e /etc/sekka-vm-update
	check by-sekka-mounts "generation 1 mounts /dev/disk/by-sekka/root" grep -q '^/dev/disk/by-sekka/root / ' /etc/fstab
	journalctl -b -o cat -u sekka-install-identity.service | sed 's/^/identity record: /'
	nix-env --list-generations -p /nix/var/nix/profiles/system | sed 's/^/generations: /'
	;;

*)
	echo "unknown phase $phase"
	exit 2
	;;
esac

if [ "$failures" = 0 ]; then
	echo "result $phase pass"
else
	echo "result $phase fail ($failures)"
fi
