#!/usr/bin/env bash
# run-codex.sh BRIDGE MODEL EFFORT CONFIG: Codex on one bridge episode
# (harness/bridge.py), its JSON events on stdout.
#
# It runs in a bubblewrap sandbox: the host read-only, no D-Bus, display or
# sudo (/run is empty but for the system's programs and the resolver), and
# $HOME hidden but for Codex's own state, so its account and logins work
# while its user configuration (plugins, MCP servers, projects) is replaced
# by CONFIG, the model and its effort alone. The bridge directory is the
# only other place it can write. Codex's own sandbox is off: this one is
# the boundary, and its commands need to reach ./vm's files.
set -eu
bridge=$1 model=$2 effort=$3 config=$4
codex=${SEKKA_EVAL_CODEX:?the codex binary to run}
bwrap=${SEKKA_EVAL_BWRAP:-bwrap}
{
  printf 'model = "%s"\n' "$model"
  [ -n "$effort" ] && printf 'model_reasoning_effort = "%s"\n' "$effort"
} > "$config"
exec "$bwrap" --die-with-parent --new-session --unshare-pid --unshare-ipc --unshare-uts \
  --ro-bind / / --dev /dev --proc /proc \
  --tmpfs /run --ro-bind /run/current-system /run/current-system \
  --ro-bind-try /run/systemd/resolve /run/systemd/resolve \
  --tmpfs /tmp \
  --tmpfs "$HOME" --bind "$HOME/.codex" "$HOME/.codex" --ro-bind "$config" "$HOME/.codex/config.toml" \
  --bind "$bridge" "$bridge" --chdir "$bridge" \
  "$codex" exec --ephemeral --json --skip-git-repo-check --sandbox danger-full-access \
  -C "$bridge" -o "$bridge/last-message.txt" "$(cat "$bridge/brief.md")"
