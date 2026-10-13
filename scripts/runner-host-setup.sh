#!/usr/bin/env bash
# One-time root setup of the builder for the release workflow's runner
# (nix/runner.nix, DEVELOPERS.md), on a host with Determinate Nix and
# Docker. Run it there as root: `sudo bash scripts/runner-host-setup.sh`.
# It is safe to run again.
set -euo pipefail

if [[ $EUID -ne 0 ]]; then
  echo "Run this as root: sudo bash $0" >&2
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

systemctl restart nix-daemon.service
echo "Done: sekka-runner ($id), KVM for builds, garbage collection when space runs low, and Sekka's binary cache."
