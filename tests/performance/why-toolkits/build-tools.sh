#!/usr/bin/env bash
# Build the harness's C tools into OUT: the observer (sway only), the tracing
# proxy and the socket power sampler (from the terminal harness).
#
#   tests/performance/why-toolkits/build-tools.sh OUT
#
# Needs gcc, pkg-config, wayland-scanner, wayland and xkbcommon headers on
# PATH/PKG_CONFIG_PATH, and in the environment WLR_PROTOCOLS (wlr-protocols'
# unstable/ folder), VIRTUAL_KEYBOARD_XML (from wlroots' source) and
# XDG_SHELL_XML (wayland-protocols' stable/xdg-shell/xdg-shell.xml) and
# FAKE_INPUT_XML (plasma-wayland-protocols' fake-input.xml).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
out=${1:?OUT}
mkdir -p "$out"
cd "$out"
wayland-scanner client-header "$WLR_PROTOCOLS/wlr-screencopy-unstable-v1.xml" screencopy-client.h
wayland-scanner private-code "$WLR_PROTOCOLS/wlr-screencopy-unstable-v1.xml" screencopy-protocol.c
wayland-scanner client-header "$WLR_PROTOCOLS/wlr-virtual-pointer-unstable-v1.xml" virtual-pointer-client.h
wayland-scanner private-code "$WLR_PROTOCOLS/wlr-virtual-pointer-unstable-v1.xml" virtual-pointer-protocol.c
wayland-scanner client-header "$VIRTUAL_KEYBOARD_XML" virtual-keyboard-client.h
wayland-scanner private-code "$VIRTUAL_KEYBOARD_XML" virtual-keyboard-protocol.c
wayland-scanner client-header "$WLR_PROTOCOLS/wlr-layer-shell-unstable-v1.xml" layer-shell-client.h
wayland-scanner private-code "$WLR_PROTOCOLS/wlr-layer-shell-unstable-v1.xml" layer-shell-protocol.c
wayland-scanner private-code "$XDG_SHELL_XML" xdg-shell-protocol.c
cc -O2 -std=c11 -Wall -Wextra -Werror -I. -o observer "$here/observer.c" screencopy-protocol.c \
  virtual-pointer-protocol.c virtual-keyboard-protocol.c layer-shell-protocol.c xdg-shell-protocol.c $(pkg-config --cflags --libs wayland-client xkbcommon)
cc -O2 -std=c11 -Wall -Wextra -Werror -Wno-vla -o wltrace "$here/wltrace.c"
# KWin's injector (fake-input; FAKE_INPUT_XML from plasma-wayland-protocols).
wayland-scanner client-header "$FAKE_INPUT_XML" fake-input-client.h
wayland-scanner private-code "$FAKE_INPUT_XML" fake-input-protocol.c
cc -O2 -std=c11 -Wall -Wextra -Werror -I. -o kwin-input "$here/kwin-input.c" fake-input-protocol.c \
  $(pkg-config --cflags --libs wayland-client)
cc -O2 -std=c11 -Wall -Wextra -o ppt-sampler "$here/../terminals/ppt-sampler.c"
echo "built $out/observer $out/wltrace $out/kwin-input $out/ppt-sampler"
