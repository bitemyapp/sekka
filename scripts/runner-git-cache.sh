#!/usr/bin/env bash
# Called only from the immutable runner image, after admission. The supervisor
# alone updates the cache; jobs see it read-only and have private refs/indexes.
set -euo pipefail

cache=${SEKKA_CI_GIT_CACHE:-/var/cache/sekka-ci-git}/repository.git
url=https://github.com/bitemyapp/sekka
case ${1:?refresh or seed} in
  refresh)
    mkdir -p "$(dirname "$cache")"
    if [[ ! -d $cache ]]; then
      git -c core.hooksPath=/dev/null clone --bare --single-branch --branch main --no-tags "$url" "$cache"
    fi
    git -c core.hooksPath=/dev/null -c gc.auto=0 -c maintenance.auto=false \
      --git-dir="$cache" fetch --prune --no-tags "$url" '+refs/heads/main:refs/heads/main'
    # Detached maintenance could otherwise repack the alternates after a job
    # starts. Any needed GC must finish before this command returns.
    git -c gc.autoDetach=false --git-dir="$cache" gc --auto
    ;;
  seed)
    # A cache outage must not prevent checkout's normal authenticated fetch.
    [[ -d $cache && -n ${GITHUB_WORKSPACE:-} ]] || exit 0
    mkdir -p "$GITHUB_WORKSPACE"
    [[ -z $(ls -A "$GITHUB_WORKSPACE") ]] || exit 0
    if ! git -c safe.directory="$cache" -c core.hooksPath=/dev/null clone --shared --no-checkout "$cache" "$GITHUB_WORKSPACE"; then
      echo '::warning::Git object cache unavailable; actions/checkout will initialize the repository.'
      exit 0
    fi
    git -C "$GITHUB_WORKSPACE" remote set-url origin "$url"
    # Keep the initial checkout sparse even when actions/checkout resets HEAD
    # while preparing an existing repository. The action later expands this
    # to the exact admitted merge, after the trust recheck.
    git -C "$GITHUB_WORKSPACE" sparse-checkout set --no-cone /scripts/check-pr-trust.py
    ;;
  *) echo 'expected refresh or seed' >&2; exit 2 ;;
esac
