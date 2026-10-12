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

# The release's flake check runs NixOS tests in VMs: builds that ask for
# KVM get /dev/kvm in their sandbox, and the build users may open it.
conf=/etc/nix/nix.custom.conf
touch "$conf"
if ! grep -qx 'extra-system-features = kvm' "$conf"; then
  echo 'extra-system-features = kvm' >>"$conf"
fi
for user in $(getent group nixbld | cut -d: -f4 | tr , ' '); do
  usermod --append --groups kvm "$user"
done

# Sekka's own binary cache, so the builder downloads what Sekka has built
# instead of building it again.
if ! grep -q 'cache.sekka.computer' "$conf"; then
  cat >>"$conf" <<'EOF'
extra-substituters = https://cache.sekka.computer
extra-trusted-public-keys = cache.sekka.computer-1:JJsOkmRt3iCwjvJrol8x2a7euU+kMV7iHaDSFzofA+Y=
EOF
fi

systemctl restart nix-daemon.service
echo "Done: sekka-runner ($id), KVM for builds, and Sekka's binary cache."
