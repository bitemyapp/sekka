# Direct QEMU: unmodified GUI installation and disk-only boot

This is the primary qualification path while the old harness modifies installer
settings. Run on a workstation from the pinned Sekka checkout. Pick the verified
ISO and an evidence directory first. Resolve packages through that checkout's
lock; do not use an unrelated latest QEMU/OVMF silently.

```sh
set -euo pipefail
qualification_iso=$(realpath /absolute/path/to/verified.iso)
qualification_evidence=$(realpath /absolute/path/to/evidence)
qualification_qemu=$(nix build --no-update-lock-file --no-link --print-out-paths --inputs-from . nixpkgs#qemu)
qualification_ovmf=$(nix build --no-update-lock-file --no-link --print-out-paths --inputs-from . nixpkgs#OVMF.fd)
qualification_run=$(mktemp -d "$qualification_evidence/uefi-usb-XXXXXXXX")
qualification_sockets=$(mktemp -d /tmp/sekka-qual-XXXXXXXX)
ln -s "$qualification_sockets" "$qualification_run/sockets"
printf '%s\n' "$qualification_iso" > "$qualification_run/iso-path"
sha256sum "$qualification_iso" > "$qualification_run/iso.sha256"
cp "$qualification_ovmf/FV/OVMF_VARS.fd" "$qualification_run/OVMF_VARS.fd"
chmod u+w "$qualification_run/OVMF_VARS.fd"
truncate -s 100G "$qualification_run/target.raw"
qualification_args=(
  -name sekka-qualification -machine q35,vmport=off -accel kvm -cpu host
  -smp 4 -m 8192 -display none -vga none
  -device virtio-gpu-pci,xres=1280,yres=800
  -serial "file:$qualification_run/serial.log"
  -qmp "unix:$qualification_sockets/qmp.sock,server=on,wait=off"
  -device virtio-serial-pci
  -chardev "socket,path=$qualification_sockets/qga.sock,server=on,wait=off,id=qga"
  -device virtserialport,chardev=qga,name=org.qemu.guest_agent.0
  -netdev user,id=net0 -device virtio-net-pci,netdev=net0
  -drive "if=pflash,format=raw,readonly=on,file=$qualification_ovmf/FV/OVMF_CODE.fd"
  -drive "if=pflash,format=raw,file=$qualification_run/OVMF_VARS.fd"
  -device qemu-xhci,id=xhci -device usb-kbd,bus=xhci.0
  -device usb-tablet,bus=xhci.0
  -drive "if=none,id=target,format=raw,file=$qualification_run/target.raw"
  -device nvme,drive=target,serial=SEKKA_TEST_ONLY,bootindex=2
)
qualification_media=(
  -drive "if=none,id=iso,format=raw,readonly=on,file=$qualification_iso"
  -device usb-storage,drive=iso,bootindex=1
)
printf '%q ' "$qualification_qemu/bin/qemu-system-x86_64" "${qualification_args[@]}" "${qualification_media[@]}" > "$qualification_run/command.sh"
printf '\n' >> "$qualification_run/command.sh"
"$qualification_qemu/bin/qemu-system-x86_64" "${qualification_args[@]}" "${qualification_media[@]}" \
  -pidfile "$qualification_run/qemu.pid" -daemonize
```

Record host free memory before launching. Adjust RAM, CPUs, disk size and GPU
only as intentional matrix dimensions. A 100 GiB sparse file is still capable
of consuming 100 GiB on the host. Paths used in QEMU drive options must not
contain commas; use the fresh generated directories and a suitable ISO path.
If the installed QEMU/OVMF packages use different names, inspect their output
and record the resolved paths. BIOS omits both pflash drives. Optical testing
replaces USB ISO options with a read-only CD-ROM drive, leaving the disk intact.

Use a QMP client to connect to `sockets/qmp.sock`, read its greeting and issue
`qmp_capabilities`. Then send paired absolute pointer/button events, qcode keys
and `screendump` as described in the parent controls leaf. Observe before
acting. Use the actual guest layout for non-ASCII typing, including its compose
or Unicode input UI; the old helper's ASCII typing is not Unicode coverage.

Use QGA only when the guest actually provides it. Do not inject the fixture or
enable `test_diagnostics` to obtain it. The live ISO normally has QGA, whereas a
normal installed system may not. For installed diagnostics, use the terminal
through UI/serial console, or explicitly selected SSH with a disposable test
key. Keep keys on the host and public keys in the guest; no personal credentials.

After installation use its normal shutdown control. Confirm the saved PID has
exited and the QMP connection closes. Keep firmware variables and disk, replace
serial/QMP output names for the next boot, and launch the same base argument
array with **no** `qualification_media` arguments. Never attach a repository
share. Save this second command separately and prove the root mounts came from
the installed disk. Do not fabricate a harness `result.json` to run
`boot-installed` against this manually created VM.

Keep the VM running for desktop testing. In a later turn, inspect the saved PID,
process command and socket response before continuing. Do not truncate the disk
or copy fresh firmware variables when resuming. After collecting evidence,
request guest shutdown and verify exit; use QMP `quit` only for an irrecoverable
test hang, marking that case failed and retaining diagnostics.
