#!/usr/bin/env bash
# One disposable runner at a time per slot. Run this supervisor once for
# shared work and once for PR-only work; each owns its Git object cache.
set -euo pipefail

slot=${SEKKA_RUNNER_SLOT:-shared}
case $slot in
  shared)
    job=sekka-runner-job
    labels='["self-hosted","linux","x64","sekka-builder","sekka-pr"]'
    ;;
  pr)
    job=sekka-runner-pr-job
    labels='["self-hosted","linux","x64","sekka-pr"]'
    ;;
  *) echo "Unknown runner slot: $slot (expected shared or pr)" >&2; exit 2 ;;
esac

cache=${SEKKA_CI_GIT_CACHE_BASE:-/var/cache/sekka-ci-git}/$slot
mkdir -p "$cache"
# A second supervisor for this slot must not refresh objects while the first
# one's job uses them. Different slots hold different locks and repositories.
exec 9>"$cache/supervisor.lock"
flock -n 9 || { echo "The $slot slot already has a supervisor." >&2; exit 1; }

token=${SEKKA_RUNNER_TOKEN_FILE:-/run/secrets/token}
git_cache_script=${SEKKA_RUNNER_GIT_CACHE_SCRIPT:?The runner package supplies its immutable Git cache script}
uid=${SEKKA_RUNNER_UID:?The runner package supplies its untrusted job uid}
secrets=$(mktemp -d)
trap 'rm -rf "$secrets"' EXIT
while true; do
  # On supervisor restart, its surviving job finishes before this slot's
  # cache is refreshed. The other slot's job and cache are independent.
  docker wait "$job" >/dev/null 2>&1 || true
  if ! SEKKA_CI_GIT_CACHE="$cache" timeout 180 bash "$git_cache_script" refresh; then
    echo "Git object cache refresh failed; checkout will fetch missing objects."
  fi
  if [[ ! -r $token ]]; then
    echo "No token at $token yet; checking again in a minute."
    sleep 60
    continue
  fi
  # Neither token nor registration goes on the command line or into logs.
  (umask 077; printf 'Authorization: Bearer %s\n' "$(cat "$token")" > "$secrets/headers")
  name="wx-workstation-$slot-$(date -u +%Y%m%dT%H%M%S)"
  request=$(jq -cn --arg name "$name" --argjson labels "$labels" \
    '{name: $name, runner_group_id: 1, labels: $labels, work_folder: "_work"}')
  if ! curl -fsS -X POST \
    -H @"$secrets/headers" \
    -H "Accept: application/vnd.github+json" \
    -H "X-GitHub-Api-Version: 2022-11-28" \
    --data "$request" \
    https://api.github.com/repos/bitemyapp/sekka/actions/runners/generate-jitconfig \
    | jq -er '"RUNNER_JITCONFIG=" + .encoded_jit_config' > "$secrets/job.env"; then
    echo "GitHub did not register a runner; trying again in a minute."
    sleep 60
    continue
  fi
  echo "$name is waiting for a job."
  docker run --rm --name "$job" \
    --env-file "$secrets/job.env" \
    --env "SEKKA_RUNNER_SLOT=$slot" \
    --user "$uid:$uid" \
    --cap-drop ALL \
    --security-opt no-new-privileges \
    --read-only \
    --tmpfs /tmp:rw,exec,nosuid,nodev,size=8g \
    --mount type=volume,dst=/home/runner \
    --volume /nix/store:/nix/store:ro \
    --volume /nix/var/nix/daemon-socket:/nix/var/nix/daemon-socket:ro \
    --mount type=bind,src=/nix/var/nix/gcroots/sekka-ci,dst=/nix/var/nix/gcroots/sekka-ci \
    --env SEKKA_CI_ROOTS=/nix/var/nix/gcroots/sekka-ci \
    --mount "type=bind,src=$cache,dst=/var/cache/sekka-ci-git,readonly" \
    --pids-limit 16384 \
    --memory 64g \
    --cpus 32 \
    sekka-runner:latest \
    || echo "$name ended with status $?."
  rm -f "$secrets/job.env"
  sleep 5
done
