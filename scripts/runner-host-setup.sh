#!/usr/bin/env bash
# One-time root setup of the Ubuntu builder for the nightly/release runner
# (nix/runner.nix, DEVELOPERS.md), on a host with Determinate Nix and
# Docker. Run it there as root: `sudo bash scripts/runner-host-setup.sh`.
# It is safe to run again.
set -euo pipefail

if [[ $EUID -ne 0 ]]; then
  echo "Run this as root: sudo bash $0" >&2
  exit 1
fi

# Build the exact native ARM derivations through QEMU user emulation, retaining
# upstream binary-cache reuse. A cross-compiled package set has different store
# paths and a much smaller cache. Registration belongs on the host, never in a
# privileged workflow container. Ubuntu's static interpreter and the F flag
# keep it available inside Nix's mount namespaces without host filesystem binds.
apt-get update
apt-get install --yes qemu-user-static binfmt-support
update-binfmts --enable qemu-aarch64
if ! grep -q '^flags: .*F' /proc/sys/fs/binfmt_misc/qemu-aarch64; then
  echo "qemu-aarch64 must be registered with the fix-binary (F) flag for Nix sandboxes." >&2
  exit 1
fi

# Reserve the id the runner's jobs run as, so that no account of the host
# ever shares it: no home, no login, and no group but its own. The Nix
# daemon treats it as untrusted.
id=30500
if ! getent group sekka-runner >/dev/null; then
  groupadd --system --gid "$id" sekka-runner
fi
if ! getent passwd sekka-runner >/dev/null; then
  useradd --system --uid "$id" --gid "$id" --no-create-home \
    --home-dir /nonexistent --shell /usr/sbin/nologin sekka-runner
fi

# The flake check of releases and nightlies runs NixOS tests in VMs: builds
# that ask for KVM get /dev/kvm in their sandbox. There a build has no group but
# nixbld (the sandbox drops the build users' other groups), so that group
# may open it, now and whenever the device appears.
conf=/etc/nix/nix.custom.conf
touch "$conf"
# Nix's extra- prefix appends to the extra-platforms setting rather than
# replacing the daemon's existing i686/CPU compatibility platforms.
if ! grep -qx 'extra-extra-platforms = aarch64-linux' "$conf"; then
  echo 'extra-extra-platforms = aarch64-linux' >>"$conf"
fi
if ! grep -qx 'extra-system-features = kvm' "$conf"; then
  echo 'extra-system-features = kvm' >>"$conf"
fi
cat >/etc/udev/rules.d/90-nix-build-kvm.rules <<'EOF'
# Nix's sandboxed builds (group nixbld) may use KVM (scripts/runner-host-setup.sh).
KERNEL=="kvm", RUN+="/usr/bin/setfacl -m g:nixbld:rw /dev/kvm"
EOF
udevadm control --reload
setfacl -m g:nixbld:rw /dev/kvm
# The kvm group, which the first version of this script gave the build
# users, never reaches the sandbox.
for user in $(getent group kvm | cut -d: -f4 | tr , ' '); do
  case $user in
    nixbld*) gpasswd --delete "$user" kvm >/dev/null ;;
  esac
done

# Nightly builds leave their ISOs in the store with nothing keeping them:
# when free space falls under 100 GiB during a build, the daemon collects
# garbage until 200 GiB are free.
if ! grep -q '^min-free' "$conf"; then
  printf 'min-free = %d\nmax-free = %d\n' $((100 * 1024 ** 3)) $((200 * 1024 ** 3)) >>"$conf"
fi

# Sekka's own binary cache, so the builder downloads what Sekka has built
# instead of building it again.
if ! grep -q 'cache.sekka.computer' "$conf"; then
  cat >>"$conf" <<'EOF'
extra-substituters = https://cache.sekka.computer
extra-trusted-public-keys = cache.sekka.computer-1:JJsOkmRt3iCwjvJrol8x2a7euU+kMV7iHaDSFzofA+Y=
EOF
fi

# The AI applications come from numtide/llm-agents.nix, newer than Nixpkgs'
# and not on cache.nixos.org; its own cache has them, so the builder need
# not compile Codex and the rest from source.
if ! grep -q 'cache.numtide.com' "$conf"; then
  cat >>"$conf" <<'EOF'
extra-substituters = https://cache.numtide.com
extra-trusted-public-keys = niks3.numtide.com-1:DTx8wZduET09hRmMtKdQDxNNthLQETkc/yaX7M4qK0g=
EOF
fi

# The root flake and installed systems use this signed public cache for the
# pinned ARM Nix bootstrap binaries. The job user cannot add daemon caches.
if ! grep -q 'install.determinate.systems' "$conf"; then
  cat >>"$conf" <<'EOF'
extra-substituters = https://install.determinate.systems
extra-trusted-public-keys = cache.flakehub.com-3:hJuILl5sVK4iKm86JzgdXW12Y2Hwd5G07qKtHTOcDCM=
EOF
fi

systemctl restart nix-daemon.service
echo "Done: sekka-runner ($id), native ARM emulation, KVM for x86 tests, garbage collection, and signed builder caches."
