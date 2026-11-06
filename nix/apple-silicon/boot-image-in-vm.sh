#!/usr/bin/env bash
# Boot Sekka's Mac images in a disposable QEMU VM without touching this
# computer's disks: on an Apple Silicon Mac with Apple's hypervisor, or on
# aarch64 Linux with KVM. Every disk is a file in WORKDIR.
#
# Usage: boot-image-in-vm.sh RESULT [WORKDIR]
#   RESULT   packages.aarch64-linux.mac-release-j514s (the product image), or
#            packages.aarch64-linux.mac-installer-j514s (the developer image)
#   WORKDIR  scratch space, default a new temporary directory. The product
#            test needs about three times the root image (two disks and the
#            unpacked ESP); disk files are sparse where the filesystem allows.
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
# 2. the first boot, with disk B attached: a byte copy of disk A with other
#    GPT UUIDs (same labels, same filesystem UUIDs). It must mount disk A's
#    root and ESP only, register the store, grow the root, hand over the
#    firmware, write identity.nix, and evaluate and rebuild /etc/nixos
#    offline (there is no network device) to the image's own system;
# 3. the second boot: nothing registered again; identity.nix kept; boot.bin's
#    replacement on this FAT filesystem; a controlled update built offline;
# 4. the updated generation boots; it rolls back;
# 5. the image's generation boots again.
# The product image has no account: the harness turns on systemd's debug
# shell on the second virtio console (hvc1) in each boot entry it starts, a
# change to the VM's disk only. guest-checks.sh (vm/) runs the checks inside.
#
# The developer image is laid out as the unmodified Asahi installer lays it
# out (its labels and volume ID) and is checked as before, logging in as
# sekka/sekka on the console.
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
case $(uname -s) in
Darwin) accel=(-machine virt,accel=hvf,acpi=off -cpu host) ;;
Linux)
	[ -w /dev/kvm ] || { echo "/dev/kvm isn't usable here"; exit 1; }
	accel=(-machine virt,acpi=off,gic-version=max -accel kvm -cpu host)
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
		drives+=(-drive "if=none,id=d$i,format=raw,file=$d" -device "virtio-blk-device,drive=d$i")
		i=$((i + 1))
	done
	rm -f "$work/con.sock" "$work/shell.sock"
	: >"$work/console-$run.log"
	# Relative socket paths: macOS limits them to 104 bytes, Linux to 108.
	(cd "$work" && exec "$qemu/bin/qemu-system-aarch64" \
		"${accel[@]}" -smp 4 -m 4096 \
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
	log "booting, twice: the first boot and an ordinary one"
	checks=(
		"systemctl is-system-running --wait"
		"systemctl --failed --no-legend --plain"
		"systemctl is-active graphical.target display-manager.service"
		"test -e /nix-path-registration && echo unregistered || readlink /nix/var/nix/profiles/system"
		"findmnt -n -o FSTYPE /lib/firmware/vendor; ls /lib/firmware/vendor"
		"findmnt -n -o SOURCE,FSTYPE /boot; findmnt -n -o SIZE /"
	)
	for run in first second; do
		start "$run" disk.img
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
		stop
	done
	log "booted twice: running, no failed units, graphical, firmware handed over"
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
volume_id=$(field esp.volume_id)
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
mkdir -p "$work/fw/vendorfw"
echo "Sekka VM test firmware" >"$work/fw/vendorfw/sekka-vm-test.txt"
(cd "$work/fw" && find vendorfw | "$cpio" -o -H newc 2>/dev/null) >"$work/pkg/esp/firmware.cpio.tmp"
mkdir -p "$work/pkg/esp/vendorfw"
mv "$work/pkg/esp/firmware.cpio.tmp" "$work/pkg/esp/vendorfw/firmware.cpio"
esp=$work/esp.img
blank "$esp" "$esp_bytes"
"$mtools/bin/mformat" -i "$esp" -F -N "${volume_id#0x}" -v "EFI - SEKKA" ::
"$mtools/bin/mcopy" -i "$esp" -s "$work/pkg/esp/"* ::/

log "disk A: a stand-in for the stub, the ESP, root.img and $((extra / 1024 / mib)) GiB to grow into"
a=$work/disk-a.img
esp_start=$((mib + stub_bytes))
root_start=$((esp_start + esp_bytes))
blank "$a" $((root_start + root_bytes + extra + mib))
"$sgdisk" -Z "$a" >/dev/null
"$sgdisk" -n "1:$((mib / 512)):+$((stub_bytes / 512 - 1))" -t 1:AF0A -c 1:Sekka -u "1:$STUB_A" \
	-n "2:$((esp_start / 512)):+$((esp_bytes / 512 - 1))" -t 2:EF00 -c "2:EFI - SEKKA" -u "2:$ESP_A" \
	-n "3:$((root_start / 512)):+$(((root_bytes + extra) / 512 - 1))" -t 3:8300 -c 3:Root -u "3:$ROOT_A" \
	"$a" >/dev/null
put "$esp" "$a" "$esp_start"
"$unzip" -p "$payload" root.img | dd of="$a" bs=$mib seek=$((root_start / mib)) conv=notrunc,sparse 2>/dev/null
"$sgdisk" -v "$a" | tail -n1

log "disk B: disk A's bytes, with other GPT UUIDs"
b=$work/disk-b.img
cp "$a" "$b" 2>/dev/null
"$sgdisk" -G "$b" >/dev/null
partuuid() { "$sgdisk" -i "$2" "$1" | awk '/Partition unique GUID/ {print tolower($4)}'; }
ESP_B=$(partuuid "$b" 2)
ROOT_B=$(partuuid "$b" 3)
[ "$(partuuid "$a" 3)" = "$ROOT_A" ] && [ "$ROOT_B" != "$ROOT_A" ] && [ "$ESP_B" != "$ESP_A" ]
"$python" - "$a" "$b" "$root_start" "$esp_start" "$esp_bytes" <<'EOF'
# Byte-identical partitions: same labels, same filesystem UUIDs.
import hashlib, sys
a, b, root, esp, esp_bytes = sys.argv[1], sys.argv[2], *map(int, sys.argv[3:])
def h(path, offset, size):
    d = hashlib.sha256()
    with open(path, "rb") as f:
        f.seek(offset)
        while size > 0:
            chunk = f.read(min(size, 1 << 22))
            d.update(chunk)
            size -= len(chunk)
    return d.hexdigest()
assert h(a, esp, esp_bytes) == h(b, esp, esp_bytes)
assert h(a, root, 1 << 30) == h(b, root, 1 << 30)
print("disk B's ESP and the first GiB of its root are byte-identical to disk A's")
EOF

log "the test disk: the checks, their expectations and the update"
t=$work/test.img
blank "$t" $((64 * mib))
"$mtools/bin/mformat" -i "$t" -F -v SEKKATEST ::
cat >"$work/expect.env" <<EOF
ROOT_A=$ROOT_A
ESP_A=$ESP_A
ROOT_B=$ROOT_B
ESP_B=$ESP_B
INSTALLATION=$INSTALLATION
TOPLEVEL=$(field system.toplevel)
LOCK_SHA256=$(field source.lock | sed 's/^sha256://')
REGISTERED=$(field root.registered_paths)
ROOT_IMAGE_BYTES=$root_bytes
EOF
"$mtools/bin/mcopy" -i "$t" "$work/expect.env" "$here/vm/guest-checks.sh" "$here/vm/vm-update.nix" \
	"$here/boot-bin/boot_bin.py" ::/

# The VM's consoles: the getty on hvc0, systemd's debug shell on hvc1.
vm_args="console=hvc0 systemd.debug_shell=/dev/hvc1"
espi="$a@@$esp_start"
# Add the VM's console arguments to every boot entry on disk A that lacks
# them, including entries NixOS writes during the test.
patch_entries() {
	local e
	for e in $("$mtools/bin/mdir" -i "$espi" -b ::/loader/entries/ | sed 's|.*/||'); do
		"$mtools/bin/mtype" -i "$espi" "::/loader/entries/$e" >"$work/e.conf"
		if ! grep -q 'systemd.debug_shell' "$work/e.conf"; then
			sed -i.bak "s|^options .*|& $vm_args|" "$work/e.conf"
			"$mtools/bin/mcopy" -o -i "$espi" "$work/e.conf" "::/loader/entries/$e"
		fi
	done
}
set_default() {
	"$mtools/bin/mtype" -i "$espi" ::/loader/loader.conf | sed "s|^default .*|default $1|" >"$work/loader.conf"
	"$mtools/bin/mcopy" -o -i "$espi" "$work/loader.conf" ::/loader/loader.conf
}

log "boots that must stop in the initrd, with a message, not a 90-second wait"
negative() { # NAME ENTRYFILE PATTERN
	"$mtools/bin/mcopy" -o -i "$espi" "$2" "::/loader/entries/vm-$1.conf"
	set_default "vm-$1.conf"
	start "negative-$1" "$a"
	if wait_for "negative-$1" "$3" 180; then
		echo "negative-$1: stopped after $((SECONDS - started))s, from QEMU's start: $(grep -a -m1 "Sekka cannot start" "$work/console-negative-$1.log" | tr -d '\r')"
		echo "negative-$1 pass $((SECONDS - started))s" >>"$work/summary.txt"
	else
		echo "negative-$1: no message"
		echo "negative-$1 fail" >>"$work/summary.txt"
		tail -c 3000 "$work/console-negative-$1.log"
	fi
	stop
	"$mtools/bin/mdel" -i "$espi" "::/loader/entries/vm-$1.conf"
}
: >"$work/summary.txt"
sed "s|^options .*|& $vm_args|" "$work/entry-unfilled.conf" >"$work/vm-placeholders.conf"
negative placeholders "$work/vm-placeholders.conf" "still has the image's placeholder"
sed -e "s|@SEKKA_ROOT_PARTUUID@|$(new_uuid)|" -e "s|@SEKKA_ESP_PARTUUID@|$ESP_A|" -e "s|^options .*|& $vm_args|" \
	"$work/entry-unfilled.conf" >"$work/vm-missing.conf"
negative missing "$work/vm-missing.conf" "appeared within"
set_default nixos-generation-1.conf

guest() { # NAME: run guest-checks.sh PHASE on the debug shell, then power off
	(cd "$work" && "$python" -I "$here/vm-console.py" --shell --timeout 2400 shell.sock \
		"mkdir -p /mnt/t && mount /dev/disk/by-label/SEKKATEST /mnt/t" \
		"bash /mnt/t/guest-checks.sh $1 > /mnt/t/results-$1.txt 2>&1; tail -n 400 /mnt/t/results-$1.txt" \
		"sync; umount /mnt/t" \
		"!systemctl poweroff") | tee "$work/checks-$1.txt"
	powered_off 180
	grep -a "^result $1" "$work/checks-$1.txt" >>"$work/summary.txt" || echo "result $1 missing" >>"$work/summary.txt"
}
boot() { # NAME DISK...
	local run=$1
	shift
	patch_entries
	log "boot: $run"
	start "$run" "$@"
	if ! wait_for "$run" 'login:' 400; then
		echo "the $run boot never reached a login prompt"
		echo "result $run never-booted" >>"$work/summary.txt"
		tail -c 3000 "$work/console-$run.log"
		stop
		return 1
	fi
	echo "$run: login prompt after $((SECONDS - started))s"
	guest "$run"
}
boot first "$a" "$b" "$t"
boot second "$a" "$t"
boot updated "$a" "$t"
boot rolled-back "$a" "$t"
"$mtools/bin/mcopy" -n -i "$t" ::/results-first.txt ::/results-second.txt ::/results-updated.txt \
	::/results-rolled-back.txt "$work/" 2>/dev/null || true
log "summary"
cat "$work/summary.txt"
! grep -qv ' pass' "$work/summary.txt"
