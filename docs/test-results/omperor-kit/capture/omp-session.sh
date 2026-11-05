#!/usr/bin/env bash
# Start a private headless Aizome session (compositor + shell) with Omperor
# on its mock backend and the README demo project. Usage:
#   omp-session.sh THEME   (night or day)
# Writes $root/env.sh to talk to the session; stop with omp-stop.sh THEME.
set -euo pipefail
theme=$1
work=$HOME/sekka-work/omperor-rebase
bin=$work/target/release
omperor=$work/omperor-target/release/omperor
demo=$work/omperor/tests/readme-demo
root=/tmp/omperor-shots-$theme
rm -rf "$root"
mkdir -p "$root"/{home,config/aizome,data/applications,data/icons/hicolor/scalable/apps,state,cache,runtime,inspect,empty,frames,omperor-state}
chmod 700 "$root/runtime"
libs=$(grep -o "LD_LIBRARY_PATH='[^']*'" "$work/env" | head -1 | cut -d"'" -f2)
fonts=$(grep -o "AIZOME_FONT_DIR='[^']*'" "$work/env" | head -1 | cut -d"'" -f2)

printf '[appearance]\ntheme = "%s"\n' "$theme" >"$root/config/aizome/settings.toml"
# Omperor's own desktop entry and icon, under the local build's app id.
sed -e 's/^Icon=org.aizome.Omperor$/Icon=org.aizome.OmperorDev/' \
    -e 's/^StartupWMClass=org.aizome.Omperor$/StartupWMClass=org.aizome.OmperorDev/' \
    -e "s|^Exec=omperor |Exec=$omperor |" \
    "$work/omperor/data/org.aizome.Omperor.desktop" >"$root/data/applications/org.aizome.OmperorDev.desktop"
cp "$work/omperor/data/org.aizome.Omperor.svg" "$root/data/icons/hicolor/scalable/apps/org.aizome.OmperorDev.svg"

# The README's demo project: committed, then the work patch applied.
project=/tmp/demo/harbor
rm -rf "$project"; mkdir -p /tmp/demo
cp -r "$demo/base" "$project"
export GIT_AUTHOR_NAME=Demo GIT_AUTHOR_EMAIL=demo@example.test GIT_COMMITTER_NAME=Demo GIT_COMMITTER_EMAIL=demo@example.test
git -C "$project" init -q -b main
git -C "$project" add -A
git -C "$project" commit -qm harbor
git -C "$project" apply "$demo/work.patch"

# Developer mode with the mock backend, as Settings ▸ Developer sets it.
sqlite3 "$root/omperor-state/omperor.sqlite3" <<'SQL'
CREATE TABLE preferences (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL);
INSERT INTO preferences VALUES ('developer_mode', 'true'), ('inference_backend', 'mock');
SQL

cat >"$root/env.sh" <<ENV
unset DISPLAY WAYLAND_SOCKET XDG_ACTIVATION_TOKEN DESKTOP_STARTUP_ID
export HOME=$root/home XDG_CONFIG_HOME=$root/config XDG_DATA_HOME=$root/data
export XDG_STATE_HOME=$root/state XDG_CACHE_HOME=$root/cache XDG_RUNTIME_DIR=$root/runtime
export XDG_DATA_DIRS=$root/data:$root/empty XDG_CONFIG_DIRS=$root/empty
export AIZOME_KIT_INSPECT=$root/inspect AIZOME_FONT_DIR=$fonts AIZOME_SESSION=0
export WAYLAND_DISPLAY=omp AIZOME_SOCKET=$root/runtime/aizome.omp.sock
export AIZOME_SHELL_SOCKET=$root/runtime/aizome-shell.omp.sock
export DBUS_SESSION_BUS_ADDRESS=unix:path=$root/no-bus DBUS_SYSTEM_BUS_ADDRESS=unix:path=$root/no-bus
export OMPEROR_STATE_DIR=$root/omperor-state OMPEROR_PROJECT=$project
export PATH=$bin:\$PATH LD_LIBRARY_PATH=$libs
root=$root
ENV
# shellcheck disable=SC1091
. "$root/env.sh"
cd "$root"
setsid "$bin/aizome" --backend headless --renderer pixman --socket omp --headless-output 1440x900@1 \
  --no-session --no-shell --no-xwayland >"$root/compositor.log" 2>&1 < /dev/null &
echo $! >"$root/compositor.pid"
for _ in $(seq 100); do [ -S "$AIZOME_SOCKET" ] && break; sleep 0.1; done
[ -S "$AIZOME_SOCKET" ] || { cat "$root/compositor.log"; exit 1; }
setsid "$bin/aizome-shell" >"$root/shell.log" 2>&1 < /dev/null &
echo $! >"$root/shell.pid"
for _ in $(seq 100); do [ -S "$AIZOME_SHELL_SOCKET" ] && break; sleep 0.1; done
[ -S "$AIZOME_SHELL_SOCKET" ] || { cat "$root/shell.log"; exit 1; }
sleep 1
setsid "$omperor" >"$root/omperor.log" 2>&1 < /dev/null &
echo $! >"$root/omperor.pid"
echo "session up in $root"
