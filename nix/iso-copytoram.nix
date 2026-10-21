# Initrd-only helper. The original media stays mounted until a complete copy
# exists, so insufficient RAM or a copy error cannot strand the store mount.
{ pkgs }:
pkgs.writeShellApplication {
  name = "sekka-copy-iso-to-ram";
  runtimeInputs = [
    pkgs.coreutils
    pkgs.util-linux
  ];
  text = ''
    source=/sysroot/iso
    notice() {
      echo "Sekka copy-to-RAM: $*" >&2
      echo "Sekka copy-to-RAM: $*" > /dev/console || true
    }
    if ! device=$(findmnt -n -o SOURCE --mountpoint "$source") ||
       ! bytes=$(blockdev --getsize64 "$device" 2>/dev/null || stat -Lc '%s' "$device"); then
      notice "cannot determine media size; continuing from installation media."
      exit 0
    fi
    available=0
    while read -r key value _; do
      if [[ "$key" == MemAvailable: ]]; then
        available=$((value * 1024))
        break
      fi
    done < /proc/meminfo
    # Leave room for the live desktop, writable store and installer. tmpfs's
    # size limit alone does not reserve memory and cannot prevent initrd OOM.
    reserve=$((4 * 1024 * 1024 * 1024))
    if [[ ! "$bytes" =~ ^[1-9][0-9]*$ ]] || (( bytes + reserve > available )); then
      notice "insufficient RAM (media $bytes bytes, available $available, reserve $reserve); continuing from installation media."
      exit 0
    fi

    stage=$(mktemp -d /run/sekka-copytoram.XXXXXXXX)
    mkdir "$stage/ram" "$stage/media"
    cleanup() {
      if mountpoint -q "$stage/ram"; then umount "$stage/ram"; fi
      if mountpoint -q "$stage/media"; then umount "$stage/media"; fi
      if mountpoint -q "$stage"; then umount "$stage"; fi
      rmdir "$stage/ram" "$stage/media" "$stage"
    }
    trap cleanup EXIT
    # A mount cannot be moved out of a shared parent. Keep both staging
    # mounts under a private mount even when systemd made /run shared.
    if ! mount --bind "$stage" "$stage" || ! mount --make-private "$stage"; then
      notice "cannot prepare RAM staging; continuing from installation media."
      exit 0
    fi
    if ! mount -t tmpfs -o "size=$bytes" tmpfs "$stage/ram"; then
      notice "cannot allocate RAM filesystem; continuing from installation media."
      exit 0
    fi
    if ! cp -a "$source/." "$stage/ram/"; then
      notice "copy failed; continuing from the intact installation media."
      exit 0
    fi

    # Keep a rollback mount across the final switch. Never expose an incomplete
    # copy at /sysroot/iso, which the read-only Nix store mount is about to use.
    mount --bind --make-private "$source" "$stage/media"
    if ! umount "$source"; then
      notice "media is busy; continuing from installation media."
      exit 0
    fi
    if ! mount --move "$stage/ram" "$source"; then
      if ! mount --move "$stage/media" "$source"; then
        # Do not discard the last usable media mount if restoration itself
        # fails. A failed unit enters initrd recovery with it still available.
        trap - EXIT
        notice "cannot restore installation media; preserved at $stage/media for recovery."
        exit 1
      fi
      notice "could not activate RAM copy; restored installation media."
      exit 0
    fi
    notice "complete; installation media can be removed."
  '';
}
