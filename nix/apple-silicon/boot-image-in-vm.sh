#!/usr/bin/env bash
# Boot Sekka's Mac images in a disposable QEMU VM without touching this
# computer's disks: on an Apple Silicon Mac with Apple's hypervisor, or on
# aarch64 Linux with KVM. Every disk is a file in WORKDIR.
#
# Usage: boot-image-in-vm.sh RESULT [WORKDIR]
#   RESULT   packages.aarch64-linux.mac-release-j514s (the product image), or
#            packages.aarch64-linux.mac-installer-j514s (the developer image)
#   WORKDIR  scratch space, default a new temporary directory. The product
#            test needs about the root image's size once more: one raw base
#            disk (sparse where the filesystem allows) and two qcow2 overlays.
#
# QEMU's virt machine has none of the Mac's hardware. linux-aurora has no
# ACPI, PCI host or serial driver for it, so the firmware is given QEMU's
# device tree (acpi=off), and the disks and consoles are virtio-mmio. This
# checks the NixOS side of the exact packaged image: systemd-boot and its
# entry, both initrds, the root and EFI partitions, the firmware hand-off
# (with a stand-in archive), root growth, first-boot registration and the
# boot reaching its targets. m1n1, U-Boot, the T6030 display/GPU handoff,
# Apple's firmware and drivers, the Asahi installer and macOS's own steps
# need the Mac.
#
# The product image (manifest.json present) is laid out as Sekka's Mac
# installer would lay it out (installer/genkan/macos, handoff.rs): random GPT
# partition UUIDs, the ESP from the payload with the boot entry's
# placeholders filled and sekka/install.json written, and root.img. Then:
# 1. two boots that must stop in the initrd with a clear message: an entry
#    whose placeholders were never filled, and one naming a missing partition;
# 2. the first boot, with disk B attached: disk A's bytes with other GPT
#    UUIDs (same labels, same filesystem UUIDs; both are overlays of one base
#    disk, and B's own data is only its two GPT copies), and disk D, a
#    stand-in for a developer install (a FAT ESP and an ext4 root with the
#    developer image's serial, label and UUID). It must mount disk A's root
#    and ESP only, leave the developer names to disk D's partitions alone,
#    register the store, grow the root, hand over the firmware, write
#    identity.nix, and evaluate and rebuild /etc/nixos offline (there is no
#    network device) to the image's own system;
# 3. the second boot: nothing registered again; identity.nix kept; boot.bin's
#    replacement on this FAT filesystem; a controlled update built offline;
# 4. the updated generation boots; it rolls back;
# 5. the image's generation boots again.
# The product image has no account: the harness turns on systemd's debug
# shell on the second virtio console (hvc1) in each boot entry it starts, a
# change to the VM's disk only. guest-checks.sh (vm/) runs the checks inside.
#
# The developer image is laid out as the unmodified Asahi installer lays it
# out (its labels and volume ID) and checked as before, logging in as
# sekka/sekka on the console; its first boot has a stand-in for a product
# install attached (the product image's label, UUID and some ESP serial), and
# its names must lead to its own partitions only.
set -euo pipefail

result=$(cd "$1" && pwd)
work=${2:-$(mktemp -d)}
mkdir -p "$work"
work=$(cd "$work" && pwd)
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
nix=$(command -v nix || echo /nix/var/nix/profiles/default/bin/nix)
tool() { "$nix" build --no-link --print-out-paths --inputs-from "$repo" "nixpkgs#$1" | tail -n1; }
qemu=$(tool qemu)
mtools=$(tool mtools)
sgdisk=$(tool gptfdisk)/bin/sgdisk
unzip=$(tool unzip)/bin/unzip
python=$(tool python3)/bin/python3
cpio=$(tool cpio)/bin/cpio
mkfs_ext4=$(tool e2fsprogs.bin)/bin/mkfs.ext4
memory=4096
case $(uname -s) in
Darwin) accel=(-machine virt,accel=hvf,acpi=off -cpu host) ;;
Linux)
	[ -w /dev/kvm ] || { echo "/dev/kvm isn't usable here"; exit 1; }
	# A 32-bit guest address space (highmem=off, so at most 3 GiB of
	# memory) keeps KVM's stage-2 table to one 16 KiB page. With the
	# default 40-bit space it needs 256 KiB contiguous, which a host whose
	# memory is fragmented by its page cache refuses (KVM_CREATE_VM: Cannot
	# allocate memory), as this J514S did on 2026-11-06.
	accel=(-machine virt,acpi=off,gic-version=max,highmem=off -accel kvm -cpu host)
	memory=3072
	;;
*) echo "unsupported host $(uname -s)"; exit 1 ;;
esac
mib=$((1024 * 1024))
log() { echo "== $*"; }
# A sparse file of a size, on macOS and Linux alike.
blank() { rm -f "$1"; dd if=/dev/zero of="$1" bs=1 count=0 seek="$2" 2>/dev/null; }
size_of() { "$python" -c 'import os, sys; print(os.path.getsize(sys.argv[1]))' "$1"; }
# Write a file into a disk image at a byte offset (a multiple of 1 MiB).
put() { dd if="$1" of="$2" bs=$mib seek=$(($3 / mib)) conv=notrunc,sparse 2>/dev/null; }
# Both images' names (identities.json): ident developer|product FIELD.
ident() { "$python" -c 'import json, sys
v = json.load(open(sys.argv[1]))[sys.argv[2]][sys.argv[3]]
print("" if v is None else v)' "$here/identities.json" "$1" "$2"; }
# A FAT serial as blkid and /dev/disk/by-uuid write it: 0x5ecca5e1 -> 5ECC-A5E1.
serial_name() { local h; h=$(printf '%08X' "$(($1))"); echo "${h:0:4}-${h:4:4}"; }
random_serial() { "$python" -c 'import secrets; print("0x" + secrets.token_hex(4))'; }
# standin DISK SERIAL LABEL UUID: a small disk with another installation's
# names, a FAT ESP with that serial and an empty ext4 root with that label
# and UUID: the partitions that installation's system mounts by name.
standin() {
	local part=$((64 * mib))
	blank "$work/standin-esp.img" "$part"
	"$mtools/bin/mformat" -i "$work/standin-esp.img" -N "${2#0x}" -v EFI ::
	blank "$work/standin-root.img" "$part"
	"$mkfs_ext4" -q -F -L "$3" -U "$4" "$work/standin-root.img"
	blank "$1" $((2 * part + 2 * mib))
	"$sgdisk" -Z "$1" >/dev/null
	"$sgdisk" -n "1:$((mib / 512)):+$((part / 512 - 1))" -t 1:EF00 -c 1:EFI \
		-n "2:$(((mib + part) / 512)):+$((part / 512 - 1))" -t 2:8300 -c 2:Root "$1" >/dev/null
	put "$work/standin-esp.img" "$1" "$mib"
	put "$work/standin-root.img" "$1" $((mib + part))
	rm -f "$work/standin-esp.img" "$work/standin-root.img"
}

vm=
stop() {
	[ -n "$vm" ] || return 0
	kill "$vm" 2>/dev/null || true
	wait "$vm" 2>/dev/null || true
	vm=
}
trap stop EXIT
# start NAME DISK...: boot with these disks, console log console-NAME.log.
start() {
	local run=$1 drives=() i=0 d
	shift
	for d in "$@"; do
		local format=raw
		[ "${d%.qcow2}" = "$d" ] || format=qcow2
		drives+=(-drive "if=none,id=d$i,format=$format,file=$d" -device "virtio-blk-device,drive=d$i")
		i=$((i + 1))
	done
	rm -f "$work/con.sock" "$work/shell.sock"
	: >"$work/console-$run.log"
	# Relative socket paths: macOS limits them to 104 bytes, Linux to 108.
	(cd "$work" && exec "$qemu/bin/qemu-system-aarch64" \
		"${accel[@]}" -smp 4 -m "$memory" \
		-drive if=pflash,format=raw,readonly=on,file="$qemu/share/qemu/edk2-aarch64-code.fd" \
		"${drives[@]}" \
		-device virtio-serial-device \
		-chardev "socket,id=console,path=con.sock,server=on,wait=off,logfile=console-$run.log" \
		-device virtconsole,chardev=console \
		-chardev "socket,id=shell,path=shell.sock,server=on,wait=off,logfile=shell-$run.log" \
		-device virtconsole,chardev=shell \
		-device virtio-rng-device -device virtio-gpu-device \
		-display none -serial "file:firmware-$run.log" -monitor none) &
	vm=$!
	started=$SECONDS
}
# wait_for NAME PATTERN SECONDS: until the console log shows it.
wait_for() {
	local i
	for i in $(seq 1 "$3"); do
		grep -aq "$2" "$work/console-$1.log" && return 0
		kill -0 "$vm" 2>/dev/null || return 1
		sleep 1
	done
	return 1
}
# Wait for the VM to power itself off; stop it after a while.
powered_off() {
	local i
	for i in $(seq 1 "$1"); do
		kill -0 "$vm" 2>/dev/null || { wait "$vm" 2>/dev/null || true; vm=; return 0; }
		sleep 1
	done
	echo "the VM didn't power off within $1 seconds; stopping it"
	stop
}

if [ ! -e "$result/manifest.json" ]; then
	# The developer image, as the unmodified Asahi installer lays it out.
	jq=$(tool jq.bin)/bin/jq
	data=$result/installer_data.json
	package=$result/os/$("$jq" -r '.os_list[0].package' "$data")
	esp_bytes=$("$jq" -r '.os_list[0].partitions[0].size | rtrimstr("B")' "$data")
	volume_id=$("$jq" -r '.os_list[0].partitions[0].volume_id' "$data")
	root_bytes=$("$jq" -r '.os_list[0].partitions[1].size | rtrimstr("B")' "$data")
	extra=$((4 * 1024 * mib))
	log "unpacking $(basename "$package")"
	rm -rf "$work/pkg" && mkdir -p "$work/pkg"
	"$unzip" -q "$package" -d "$work/pkg"
	[ "$(size_of "$work/pkg/root.img")" = "$root_bytes" ] || { echo "root.img size differs from installer_data"; exit 1; }
	log "a stand-in for Apple's firmware archive"
	mkdir -p "$work/fw/vendorfw"
	echo "Sekka VM test firmware" >"$work/fw/vendorfw/sekka-vm-test.txt"
	(cd "$work/fw" && find vendorfw | "$cpio" -o -H newc 2>/dev/null) >"$work/firmware.cpio"
	log "the EFI system partition (FAT32, volume ID $volume_id)"
	esp=$work/esp.img
	blank "$esp" "$esp_bytes"
	"$mtools/bin/mformat" -i "$esp" -F -N "${volume_id#0x}" -v EFI ::
	"$mtools/bin/mcopy" -i "$esp" -s "$work/pkg/esp/"* ::/
	"$mtools/bin/mmd" -i "$esp" ::/vendorfw
	"$mtools/bin/mcopy" -i "$esp" "$work/firmware.cpio" ::/vendorfw/firmware.cpio
	# The VM's console is virtio (hvc0); the Mac's is the display.
	"$mtools/bin/mtype" -i "$esp" ::/loader/entries/nixos-generation-1.conf |
		sed 's/^options .*/& console=hvc0/' >"$work/entry.conf"
	"$mtools/bin/mcopy" -o -i "$esp" "$work/entry.conf" ::/loader/entries/nixos-generation-1.conf
	log "the disk: GPT, ESP, then root.img and $((extra / 1024 / mib)) GiB to grow into"
	disk=$work/disk.img
	esp_start=$mib
	root_start=$((esp_start + esp_bytes))
	blank "$disk" $((root_start + root_bytes + extra + mib))
	"$sgdisk" -Z "$disk" >/dev/null
	"$sgdisk" -n "1:$((esp_start / 512)):+$((esp_bytes / 512 - 1))" -t 1:EF00 -c 1:EFI \
		-n "2:$((root_start / 512)):+$(((root_bytes + extra) / 512 - 1))" -t 2:8300 -c 2:Root "$disk" >/dev/null
	put "$esp" "$disk" "$esp_start"
	put "$work/pkg/root.img" "$disk" "$root_start"
	log "a stand-in for a product install beside it, with the product image's names"
	standin "$work/product.img" "$(random_serial)" "$(ident product root_label)" "$(ident product root_uuid)"
	label=$(ident developer root_label)
	serial=$(serial_name "$volume_id")
	log "booting, twice: the first boot (the stand-in attached) and an ordinary one"
	checks=(
		"systemctl is-system-running --wait"
		"systemctl --failed --no-legend --plain"
		"systemctl is-active graphical.target display-manager.service"
		"test -e /nix-path-registration && echo unregistered || readlink /nix/var/nix/profiles/system"
		"findmnt -n -o FSTYPE /lib/firmware/vendor; ls /lib/firmware/vendor"
		"findmnt -n -o SOURCE,FSTYPE /boot; findmnt -n -o SIZE /"
		# Its names lead to its own partitions only (udev's database, as
		# lsblk reads it without root).
		"echo \"names: label \$(lsblk -rno LABEL | grep -cx $label) \$(readlink -f /dev/disk/by-label/$label) serial \$(lsblk -rno UUID | grep -cx $serial) \$(readlink -f /dev/disk/by-uuid/$serial) root \$(findmnt -n -o SOURCE /)\""
	)
	for run in first second; do
		disks=(disk.img)
		[ "$run" != first ] || disks+=(product.img)
		start "$run" "${disks[@]}"
		if ! wait_for "$run" 'sekka login:' 300; then
			echo "== the $run boot never reached a login prompt"
			tail -c 2000 "$work/console-$run.log"
			exit 1
		fi
		# The image's developer account (developer/developer.nix).
		(cd "$work" && "$python" -I "$here/vm-console.py" con.sock sekka sekka "${checks[@]}") | tee "$work/checks-$run.txt"
		grep -qx 'running' "$work/checks-$run.txt"
		[ "$(grep -cx active "$work/checks-$run.txt")" = 2 ]
		grep -qx 'system-1-link' "$work/checks-$run.txt"
		grep -qx 'tmpfs' "$work/checks-$run.txt"
		grep -qx 'sekka-vm-test.txt' "$work/checks-$run.txt"
		grep -qE '^/dev/vda1 +vfat$' "$work/checks-$run.txt"
		grep -qx 'names: label 1 /dev/vda2 serial 1 /dev/vda1 root /dev/vda2' "$work/checks-$run.txt"
		stop
	done
	log "booted twice: running, no failed units, graphical, firmware handed over, its own partitions by name"
	exit 0
fi

# The product image.
manifest=$result/manifest.json
field() { "$python" -c 'import json, sys; v = json.load(open(sys.argv[1]))
for k in sys.argv[2].split("."): v = v[k]
print(v)' "$manifest" "$1"; }
payload=$result/os/$(field payload.name)
esp_bytes=$(field image.esp_bytes)
root_bytes=$(field image.root_image.size)
# The product image fixes no ESP serial: newfs_msdos picks one per
# installation, as this does.
volume_id=$(field esp_layout.volume_id)
[ "$volume_id" != None ] || volume_id=$(random_serial)
entry=$(field esp.boot_entry)
extra=$((4 * 1024 * mib))
stub_bytes=$((16 * mib))

log "the payload against its manifest ($(field release))"
"$python" - "$manifest" "$payload" <<'EOF'
import hashlib, json, sys, zipfile
m = json.load(open(sys.argv[1]))
def digest(stream):
    h = hashlib.sha256()
    while chunk := stream.read(1 << 22):
        h.update(chunk)
    return "sha256:" + h.hexdigest()
assert digest(open(sys.argv[2], "rb")) == m["payload"]["digest"], "the payload's digest differs"
with zipfile.ZipFile(sys.argv[2]) as z:
    assert digest(z.open("root.img")) == m["image"]["root_image"]["digest"], "root.img differs"
print("payload and root.img match the manifest")
EOF


# The VM's consoles: the getty on hvc0, systemd's debug shell on hvc1. Every
# entry the VM starts gets these; NixOS's later entries get them from
# guest-checks.sh.
vm_args="console=hvc0 systemd.debug_shell=/dev/hvc1"

log "the ESP as Sekka's installer writes it: placeholders filled, the handoff, firmware"
rm -rf "$work/pkg" && mkdir -p "$work/pkg"
"$unzip" -q "$payload" 'esp/*' -d "$work/pkg"
new_uuid() { "$python" -c 'import uuid; print(uuid.uuid4())'; }
ROOT_A=$(new_uuid)
ESP_A=$(new_uuid)
STUB_A=$(new_uuid)
INSTALLATION=$(new_uuid)
cp "$work/pkg/esp/$entry" "$work/entry-unfilled.conf"
"$python" - "$work/pkg/esp" "$entry" "$ROOT_A" "$ESP_A" "$INSTALLATION" "$(field source.revision)" "$(field release)" <<'EOF'
# What the installer does (handoff.rs): fill each placeholder, exactly once,
# and write sekka/install.json in canonical form.
import datetime, json, pathlib, sys
esp, entry, root, esp_uuid, installation, revision, release = sys.argv[1:]
path = pathlib.Path(esp, entry)
text = path.read_text()
for placeholder, value in [("@SEKKA_ROOT_PARTUUID@", root), ("@SEKKA_ESP_PARTUUID@", esp_uuid)]:
    assert text.count(placeholder) == 1, placeholder
    text = text.replace(placeholder, value)
path.write_text(text)
handoff = {
    "schema": "sekka.mac.handoff/1",
    "installation": installation,
    "release": release,
    "source_revision": revision,
    "root": root,
    "esp": esp_uuid,
    "created": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "hints": {"locale": "en_US.UTF-8", "keyboard": "us", "timezone": "UTC",
              "computer_name": "Sekka VM test", "full_name": None},
}
out = pathlib.Path(esp, "sekka", "install.json")
out.parent.mkdir(exist_ok=True)
out.write_bytes(json.dumps(handoff, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode())
EOF
cp "$work/pkg/esp/sekka/install.json" "$work/install.json"
cp "$work/pkg/esp/$entry" "$work/entry-filled.conf"
# The VM-only change: its consoles.
sed -i.bak "s|^options .*|& $vm_args|" "$work/pkg/esp/$entry"
rm -f "$work/pkg/esp/$entry.bak"
mkdir -p "$work/fw/vendorfw"
echo "Sekka VM test firmware" >"$work/fw/vendorfw/sekka-vm-test.txt"
(cd "$work/fw" && find vendorfw | "$cpio" -o -H newc 2>/dev/null) >"$work/firmware.cpio"
mkdir -p "$work/pkg/esp/vendorfw"
cp "$work/firmware.cpio" "$work/pkg/esp/vendorfw/firmware.cpio"
esp=$work/esp.img
blank "$esp" "$esp_bytes"
"$mtools/bin/mformat" -i "$esp" -F -N "${volume_id#0x}" -v "EFI - SEKKA" ::
"$mtools/bin/mcopy" -i "$esp" -s "$work/pkg/esp/"* ::/

log "disk A: a stand-in for the stub, the ESP, root.img and $((extra / 1024 / mib)) GiB to grow into"
base=$work/base.img
esp_start=$((mib + stub_bytes))
root_start=$((esp_start + esp_bytes))
# Whole MiBs, so the backup GPT is in the last one.
disk_bytes=$(((root_start + root_bytes + extra + 2 * mib - 1) / mib * mib))
blank "$base" "$disk_bytes"
"$sgdisk" -Z "$base" >/dev/null
"$sgdisk" -n "1:$((mib / 512)):+$((stub_bytes / 512 - 1))" -t 1:AF0A -c 1:Sekka -u "1:$STUB_A" \
	-n "2:$((esp_start / 512)):+$((esp_bytes / 512 - 1))" -t 2:EF00 -c "2:EFI - SEKKA" -u "2:$ESP_A" \
	-n "3:$((root_start / 512)):+$(((root_bytes + extra) / 512 - 1))" -t 3:8300 -c 3:Root -u "3:$ROOT_A" \
	"$base" >/dev/null
put "$esp" "$base" "$esp_start"
rm -f "$esp"
"$unzip" -p "$payload" root.img | dd of="$base" bs=$mib seek=$((root_start / mib)) conv=notrunc,sparse 2>/dev/null
"$sgdisk" -v "$base" | tail -n1

# Two boots that must stop in the initrd, on the base disk before it is
# shared: an entry the installer never filled and one naming a partition
# that isn't there.
espi="$base@@$esp_start"
set_default() {
	"$mtools/bin/mtype" -i "$espi" ::/loader/loader.conf | sed "s|^default .*|default $1|" >"$work/loader.conf"
	"$mtools/bin/mcopy" -o -i "$espi" "$work/loader.conf" ::/loader/loader.conf
}
: >"$work/summary.txt"
log "boots that must stop in the initrd, with a message, not a 90-second wait"
negative() { # NAME ENTRYFILE PATTERN
	"$mtools/bin/mcopy" -o -i "$espi" "$2" "::/loader/entries/vm-$1.conf"
	set_default "vm-$1.conf"
	start "negative-$1" "$base"
	if wait_for "negative-$1" "$3" 180; then
		local after=$((SECONDS - started))
		echo "negative-$1: stopped ${after}s after QEMU started: $(grep -a -m1 "Sekka cannot start" "$work/console-negative-$1.log" | tr -d '\r')"
		echo "negative-$1 pass ${after}s" >>"$work/summary.txt"
	else
		echo "negative-$1: no message"
		echo "negative-$1 fail" >>"$work/summary.txt"
		tail -c 3000 "$work/console-negative-$1.log"
	fi
	stop
	"$mtools/bin/mdel" -i "$espi" "::/loader/entries/vm-$1.conf"
}
sed "s|^options .*|& $vm_args|" "$work/entry-unfilled.conf" >"$work/vm-placeholders.conf"
negative placeholders "$work/vm-placeholders.conf" "still has the image's placeholder"
sed -e "s|@SEKKA_ROOT_PARTUUID@|$(new_uuid)|" -e "s|@SEKKA_ESP_PARTUUID@|$ESP_A|" -e "s|^options .*|& $vm_args|" \
	"$work/entry-unfilled.conf" >"$work/vm-missing.conf"
negative missing "$work/vm-missing.conf" "appeared within"
set_default nixos-generation-1.conf

# Disks A and B share the base's bytes as qcow2 overlays: A's writes stay in
# its own overlay, and B's differs from the base only in its two GPT copies.
log "disk B: disk A's bytes, with other GPT UUIDs"
a=$work/disk-a.qcow2
b=$work/disk-b.qcow2
rm -f "$a" "$b"
"$qemu/bin/qemu-img" create -q -f qcow2 -F raw -b "$base" "$a"
"$qemu/bin/qemu-img" create -q -f qcow2 -F raw -b "$base" "$b"
gpt=$work/b-gpt.img
blank "$gpt" "$disk_bytes"
dd if="$base" of="$gpt" bs=$mib count=1 conv=notrunc 2>/dev/null
dd if="$base" of="$gpt" bs=$mib skip=$((disk_bytes / mib - 1)) seek=$((disk_bytes / mib - 1)) count=1 conv=notrunc 2>/dev/null
"$sgdisk" -G "$gpt" >/dev/null
dd if="$gpt" of="$work/b-head.bin" bs=$mib count=1 2>/dev/null
dd if="$gpt" of="$work/b-tail.bin" bs=$mib skip=$((disk_bytes / mib - 1)) count=1 2>/dev/null
"$qemu/bin/qemu-io" -f qcow2 -c "write -q -s $work/b-head.bin 0 $mib" \
	-c "write -q -s $work/b-tail.bin $((disk_bytes - mib)) $mib" "$b"
partuuid() { "$sgdisk" -i "$2" "$1" | awk '/Partition unique GUID/ {print tolower($4)}'; }
ESP_B=$(partuuid "$gpt" 2)
ROOT_B=$(partuuid "$gpt" 3)
rm -f "$gpt"
[ "$(partuuid "$base" 3)" = "$ROOT_A" ] && [ "$ROOT_B" != "$ROOT_A" ] && [ "$ESP_B" != "$ESP_A" ]
echo "disk A: ESP $ESP_A, root $ROOT_A; disk B: ESP $ESP_B, root $ROOT_B"
# Everything of B's but its GPT copies is the base's.
"$qemu/bin/qemu-img" map --output=json "$b" | "$python" -c '
import json, sys
size, mib = int(sys.argv[1]), 1 << 20
own = [(e["start"], e["length"]) for e in json.load(sys.stdin) if e.get("depth") == 0 and e["data"]]
assert own == [(0, mib), (size - mib, mib)], own
print("disk B differs from disk A only in its first and last MiB (the GPT copies); its partitions are byte-identical")
' "$disk_bytes"

log "disk D: a stand-in for a developer install, with the developer image's names"
d=$work/developer.img
standin "$d" "$(ident developer esp_volume_id)" "$(ident developer root_label)" "$(ident developer root_uuid)"
ESP_D=$(partuuid "$d" 1)
ROOT_D=$(partuuid "$d" 2)
[ "$(ident product root_label)" = "$(field root.label)" ] && [ "$(ident product root_uuid)" = "$(field root.filesystem_uuid)" ]

log "the test disk: the checks, their expectations and the update"
t=$work/test.img
blank "$t" $((64 * mib))
"$mtools/bin/mformat" -i "$t" -F -v SEKKATEST ::
cat >"$work/expect.env" <<EOF
ROOT_A=$ROOT_A
ESP_A=$ESP_A
ROOT_B=$ROOT_B
ESP_B=$ESP_B
ROOT_D=$ROOT_D
ESP_D=$ESP_D
PRODUCT_LABEL=$(ident product root_label)
PRODUCT_UUID=$(ident product root_uuid)
DEVELOPER_LABEL=$(ident developer root_label)
DEVELOPER_UUID=$(ident developer root_uuid)
DEVELOPER_SERIAL=$(serial_name "$(ident developer esp_volume_id)")
INSTALLATION=$INSTALLATION
TOPLEVEL=$(field system.toplevel)
LOCK_SHA256=$(field source.lock | sed 's/^sha256://')
REGISTERED=$(field root.registered_paths)
ROOT_IMAGE_BYTES=$root_bytes
VM_ARGS="$vm_args"
EOF
"$mtools/bin/mcopy" -i "$t" "$work/expect.env" "$here/vm/guest-checks.sh" "$here/vm/vm-update.nix" \
	"$here/boot-bin/boot_bin.py" ::/
# First-boot setup's own sample of the setup.nix it writes, if this source has it.
sample=$repo/installer/genkan/first-boot/test-data/setup.nix
[ ! -e "$sample" ] || "$mtools/bin/mcopy" -i "$t" "$sample" ::/setup.nix

guest() { # NAME: run guest-checks.sh PHASE on the debug shell, then power off
	(cd "$work" && "$python" -I "$here/vm-console.py" --shell --timeout 2400 shell.sock \
		"mkdir -p /mnt/t && mount /dev/disk/by-label/SEKKATEST /mnt/t" \
		"bash /mnt/t/guest-checks.sh $1 > /mnt/t/results-$1.txt 2>&1; tail -n 400 /mnt/t/results-$1.txt" \
		"sync; umount /mnt/t" \
		"!systemctl poweroff") | tee "$work/checks-$1.txt"
	powered_off 240
	grep -a "^result $1" "$work/checks-$1.txt" >>"$work/summary.txt" || echo "result $1 missing" >>"$work/summary.txt"
}
boot() { # NAME DISK...
	local run=$1
	shift
	log "boot: $run"
	start "$run" "$@"
	if ! wait_for "$run" 'login:' 400; then
		echo "the $run boot never reached a login prompt"
		echo "result $run never-booted" >>"$work/summary.txt"
		tail -c 3000 "$work/console-$run.log"
		stop
		return 1
	fi
	echo "$run: login prompt $((SECONDS - started))s after QEMU started"
	guest "$run"
}
boot first "$a" "$b" "$d" "$t" || true
boot second "$a" "$t" || true
boot updated "$a" "$t" || true
boot rolled-back "$a" "$t" || true
"$mtools/bin/mcopy" -n -i "$t" ::/results-first.txt ::/results-second.txt ::/results-updated.txt \
	::/results-rolled-back.txt "$work/" 2>/dev/null || true
log "summary"
cat "$work/summary.txt"
! grep -qv ' pass' "$work/summary.txt"
