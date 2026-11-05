#!/usr/bin/env bash
# Drive Omperor in the private session through Aizome's own input path
# (aizomectl input), finding controls from the kit's inspect files, and
# capture the whole output with grim. Usage: omp-drive.sh THEME
set -euo pipefail
theme=$1
here=$HOME/sekka-work/omperor-rebase
export PATH=$(cat "$here/tools-path")$PATH
"$here/omp-stop.sh" night
"$here/omp-stop.sh" day
"$here/omp-session.sh" "$theme"
# shellcheck disable=SC1090
. "/tmp/omperor-shots-$theme/env.sh"
export root
mkdir -p "$root/shots" "$root/frames"
at() { python3 -I "$here/omp-at.py" "$@"; }
until_at() { for _ in $(seq 300); do at "$@" 2>/dev/null && return 0; sleep 0.2; done; echo "never: $*" >&2; return 1; }
click() { aizomectl input motion "$1" "$2"; aizomectl input click left; }
park() { aizomectl input motion 1325 600; }
shot() { grim "$root/shots/$1.png"; echo "shot $1"; }
frames() { for i in $(seq -w 1 "$2"); do grim "$root/frames/$1-$i.png"; sleep "$3"; done; }
drag() { # y x0 x1: press, move along the line, release
  aizomectl input motion "$2" "$1"; aizomectl input button left down
  for x in $(seq "$2" 60 "$3") "$3"; do aizomectl input motion "$x" "$1"; sleep 0.03; done
  aizomectl input button left up
}

# The window, its composer, ready.
until_at --name composer >/dev/null
sleep 2
read -r x y < <(at --name composer)
click "$x" "$y"
sleep 0.3

# 1. The tour: a change with two scouts. The Agents panel (❖2) while they
# work, and the call that started them opened with a click.
aizomectl input type "/mock scenario=tour"
aizomectl input key enter
sleep 0.5
aizomectl input chord super+2
until_at --text scout-tests --exact --minx 790 >/dev/null
read -r x y < <(until_at --text "Delegated scout-callers, scout-tests" --exact)
click "$x" "$y"
park
frames agents 12 0.5

# 2. The change made: the Changes panel (❖1) on chat.rs's diff, and a
# draft three lines tall.
until_at --text "Ready for another prompt" >/dev/null
sleep 1
aizomectl input chord super+1
read -r x y < <(until_at --text chat.rs --exact --minx 790)
click "$x" "$y"
sleep 1
read -r x y < <(at --name composer)
click "$x" "$y"
aizomectl input type "Thanks. Two follow-ups:"
aizomectl input chord shift+enter
aizomectl input type "- hyphenate a word longer than the width"
aizomectl input chord shift+enter
aizomectl input type "- a test for an emoji with a skin tone"
park
sleep 1
shot "change-$theme"

# 3. A new session (❖N), the showcase answer, read back to its Math; a
# sentence with formulas in it selected and copied (❖C), pasted into the
# composer (❖V), and selected again.
aizomectl input chord super+n
sleep 1.5
read -r x y < <(until_at --name composer)
click "$x" "$y"
aizomectl input type "Show me the showcase: every kind of answer you can draw"
aizomectl input key enter
sleep 1
until_at --text "Ready for another prompt" >/dev/null
sleep 2
aizomectl input motion 735 400
for _ in 1 2 3 4; do
  y=$(at --text Math --exact --all --rect | cut -d' ' -f2)
  d=$(python3 -c "print(round(($y - 44) / 2.67))")
  [ "$d" = 0 ] && break
  aizomectl input scroll 0 "$d"
  sleep 1
done
read -r _ ly _ < <(at --text Math --exact --rect)
line=$((54 + ${ly%.*} + 52))
drag "$line" 295 760
aizomectl input chord super+c
sleep 0.3
read -r x y < <(at --name composer)
click "$x" "$y"
aizomectl input chord super+v
sleep 0.5
drag "$line" 295 760
park
sleep 0.8
shot "math-$theme"
echo done
