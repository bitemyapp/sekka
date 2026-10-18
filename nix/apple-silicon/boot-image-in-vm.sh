#!/usr/bin/env bash
# Boot the Asahi installer package in a disposable QEMU VM on an Apple
# Silicon Mac (Apple's hypervisor), without touching this Mac's disk.
#
# The disk is laid out as the Asahi installer would lay it out: an EFI system
# partition with the package's esp/ tree, the installer_data's FAT volume ID
# and a stand-in vendorfw/firmware.cpio, then root.img in a larger partition.
# QEMU's virt machine has none of the Mac's hardware, so this checks the
# NixOS side of the image only: systemd-boot and its entry, both initrds, the
# root and EFI partitions found by label and volume ID, the firmware hand-off,
# the root filesystem growing, first-boot registration and the boot reaching
# its targets. m1n1, U-Boot, the handoff and Apple's drivers need the Mac.
#
# Usage: boot-image-in-vm.sh BUNDLE [WORKDIR]
#   BUNDLE   the built packages.aarch64-linux.mac-installer-j514s
#   WORKDIR  scratch space (about twice the image), default a new temp dir
set -euo pipefail

bundle=$(cd "$1" && pwd)
work=${2:-$(mktemp -d)}
mkdir -p "$work"
nix=/nix/var/nix/profiles/default/bin/nix
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
tool() { "$nix" build --no-link --print-out-paths --inputs-from "$repo" "nixpkgs#$1" | tail -n1; }
qemu=$(tool qemu)
mtools=$(tool mtools)
sgdisk=$(tool gptfdisk)/bin/sgdisk
unzip=$(tool unzip)/bin/unzip
jq=$(tool jq.bin)/bin/jq

data=$bundle/installer_data.json
package=$bundle/os/$("$jq" -r '.os_list[0].package' "$data")
esp_bytes=$("$jq" -r '.os_list[0].partitions[0].size | rtrimstr("B")' "$data")
volume_id=$("$jq" -r '.os_list[0].partitions[0].volume_id' "$data")
root_bytes=$("$jq" -r '.os_list[0].partitions[1].size | rtrimstr("B")' "$data")
extra=$((4 * 1024 * 1024 * 1024)) # the installer's expand, here 4 GiB

echo "== unpacking $(basename "$package")"
rm -rf "$work/pkg"
mkdir -p "$work/pkg"
"$unzip" -q "$package" -d "$work/pkg"
[ "$(stat -f %z "$work/pkg/root.img")" = "$root_bytes" ] || { echo "root.img size differs from installer_data"; exit 1; }

echo "== a stand-in for Apple's firmware archive"
mkdir -p "$work/fw/vendorfw"
echo "Sekka VM test firmware" > "$work/fw/vendorfw/sekka-vm-test.txt"
(cd "$work/fw" && find vendorfw | cpio -o -H newc 2>/dev/null) > "$work/firmware.cpio"

echo "== the EFI system partition (FAT32, volume ID $volume_id)"
esp=$work/esp.img
rm -f "$esp"
mkfile -n "$esp_bytes" "$esp"
"$mtools/bin/mformat" -i "$esp" -F -N "${volume_id#0x}" -v EFI ::
"$mtools/bin/mcopy" -i "$esp" -s "$work/pkg/esp/"* ::/
"$mtools/bin/mmd" -i "$esp" ::/vendorfw
"$mtools/bin/mcopy" -i "$esp" "$work/firmware.cpio" ::/vendorfw/firmware.cpio
# The VM's console is virtio (hvc0); the Mac's is the display.
"$mtools/bin/mtype" -i "$esp" ::/loader/entries/nixos-generation-1.conf |
	sed 's/^options .*/& console=hvc0/' > "$work/entry.conf"
"$mtools/bin/mcopy" -o -i "$esp" "$work/entry.conf" ::/loader/entries/nixos-generation-1.conf

echo "== the disk: GPT, ESP, then root.img and $((extra / 1024 / 1024 / 1024)) GiB to grow into"
disk=$work/disk.img
rm -f "$disk"
mib=$((1024 * 1024))
esp_start=$mib
root_start=$((esp_start + esp_bytes))
total=$((root_start + root_bytes + extra + mib))
mkfile -n "$total" "$disk"
"$sgdisk" -Z "$disk" >/dev/null
"$sgdisk" -n "1:$((esp_start / 512)):+$((esp_bytes / 512 - 1))" -t 1:EF00 -c 1:EFI \
	-n "2:$((root_start / 512)):+$(((root_bytes + extra) / 512 - 1))" -t 2:8300 -c 2:Root "$disk" >/dev/null
dd if="$esp" of="$disk" bs=1m seek=$((esp_start / mib)) conv=notrunc 2>/dev/null
dd if="$work/pkg/root.img" of="$disk" bs=1m seek=$((root_start / mib)) conv=notrunc 2>/dev/null

echo "== booting, twice: the first boot and an ordinary one"
# This kernel has no ACPI, so the firmware must hand over QEMU's device tree.
boot() {
	rm -f "$work/con.sock"
	: > "$work/console-$1.log"
	(cd "$work" && exec "$qemu/bin/qemu-system-aarch64" \
		-machine virt,accel=hvf,acpi=off -cpu host -smp 4 -m 4096 \
		-drive if=pflash,format=raw,readonly=on,file="$qemu/share/qemu/edk2-aarch64-code.fd" \
		-drive if=none,id=disk,format=raw,file=disk.img -device virtio-blk-device,drive=disk \
		-device virtio-serial-device \
		-chardev "socket,id=console,path=con.sock,server=on,wait=off,logfile=console-$1.log" \
		-device virtconsole,chardev=console -device virtio-rng-device \
		-display none -serial "file:firmware-$1.log" -monitor none) &
	vm=$!
	for _ in $(seq 1 300); do
		grep -aq 'sekka login:' "$work/console-$1.log" && return 0
		kill -0 $vm 2>/dev/null || return 1
		sleep 1
	done
	return 1
}
stop() { kill "$vm" 2>/dev/null || true; wait "$vm" 2>/dev/null || true; }
trap stop EXIT

checks=(
	"systemctl is-system-running --wait"
	"systemctl --failed --no-legend --plain"
	"systemctl is-active graphical.target display-manager.service"
	"test -e /nix-path-registration && echo unregistered || readlink /nix/var/nix/profiles/system"
	"findmnt -n -o FSTYPE /lib/firmware/vendor; ls /lib/firmware/vendor"
	"findmnt -n -o SOURCE,FSTYPE /boot; findmnt -n -o SIZE /"
)
for run in first second; do
	if ! boot "$run"; then
		echo "== the $run boot never reached a login prompt"
		tail -c 2000 "$work/console-$run.log"
		exit 1
	fi
	# The image's developer account (image.nix).
	# Relative: macOS limits socket paths to 104 bytes.
	(cd "$work" && python3 -I "$here/vm-console.py" con.sock sekka sekka "${checks[@]}") | tee "$work/checks-$run.txt"
	grep -qx 'running' "$work/checks-$run.txt"
	[ "$(grep -cx active "$work/checks-$run.txt")" = 2 ]
	grep -qx 'system-1-link' "$work/checks-$run.txt"
	grep -qx 'tmpfs' "$work/checks-$run.txt"
	grep -qx 'sekka-vm-test.txt' "$work/checks-$run.txt"
	grep -qE '^/dev/vda1 +vfat$' "$work/checks-$run.txt"
	stop
done
echo "== booted twice: running, no failed units, graphical, firmware handed over"
