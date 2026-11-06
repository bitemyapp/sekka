#!/usr/bin/env bash
# Stop the private session omp-session.sh THEME started (only its own PIDs).
root=/tmp/omperor-shots-$1
for p in omperor shell compositor; do
  [ -f "$root/$p.pid" ] && kill "$(cat "$root/$p.pid")" 2>/dev/null
done
sleep 1
for p in omperor shell compositor; do
  [ -f "$root/$p.pid" ] && kill -9 "$(cat "$root/$p.pid")" 2>/dev/null
  rm -f "$root/$p.pid"
done
true
