# The shell of a terminal that runs a build. ananicy names this process
# "bash", so it gets the shell rule, and cargo inherits its nice value. A frame
# probe named "foot" stands in for the terminal drawing beside it, in the same
# cgroup, with the priority ananicy gives foot.
# Usage: bash terminal.sh STATE DIR PROBE WORK ROOT
set -eo pipefail
state=$1 dir=$2 probe=$3 work=$4 root=$5

(exec -a foot "$probe" frame --period-us 8333 --work "$work" \
  --stop "$dir/terminal.stop" --out "$dir/terminal-frame.json") &
trap 'touch "$dir/terminal.stop"; wait' EXIT

# shellcheck disable=SC1091
source "$state/devenv.sh"
# Like a person typing the command: ananicy reaches a new process in about
# half a second, and the build should inherit the shell's settled priority.
sleep 3
build() {
  (cd "$root/$1" && CARGO_TARGET_DIR="$state/target/$2" cargo build --release --locked --offline --quiet)
}
build apps/yukimi yukimi
build installer/genkan/rust genkan
