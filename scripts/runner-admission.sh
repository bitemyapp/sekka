# Sourced by the immutable job-started hook, before any workflow steps.
# The only PR workflow admitted is main's trusted gate; ordinary PR-owned
# workflows remain forbidden even if they remove their YAML conditions.
sekka_runner_admitted() {
  [[ ${GITHUB_REPOSITORY:-} == bitemyapp/sekka ]] || return 1
  [[ ${GITHUB_REF:-} == refs/heads/main ]] || return 1
  # This pool executes candidate build tools, so it has no daemon, shared roots
  # or publishing jobs. Only main's gated PR workflow may schedule it.
  if [[ ${SEKKA_RUNNER_ROLE:-nix} == build-client ]]; then
    [[ ${GITHUB_WORKFLOW_REF:-} == bitemyapp/sekka/.github/workflows/pr.yml@refs/heads/main ]] || return 1
    [[ ${GITHUB_EVENT_NAME:-} == pull_request_target || ${GITHUB_EVENT_NAME:-} == workflow_dispatch ]]
    return
  fi
  case ${GITHUB_EVENT_NAME:-} in
    workflow_dispatch | schedule | push) return 0 ;;
    pull_request_target)
      [[ ${GITHUB_WORKFLOW_REF:-} == bitemyapp/sekka/.github/workflows/pr.yml@refs/heads/main ]] ;;
    *) return 1 ;;
  esac
}
