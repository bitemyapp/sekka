# SPDX-License-Identifier: GPL-3.0-or-later
# Programs built for generic Linux run here as they do on Ubuntu.
#
# Applications such as Zed, VS Code, mise and the AI agents download and run
# programs that were not built by Nix: language servers, Node.js, debuggers,
# extensions' tools, and their own updates. Those programs ask for the loader
# at /lib64/ld-linux-x86-64.so.2 and for libraries in the usual places, and
# their scripts start with #!/bin/bash or #!/usr/bin/python3. Plain NixOS has
# none of these, so every such download fails to start ("No such file or
# directory", or "Could not start dynamically linked executable"), and each
# application then needs its own workaround. Telegram and Signal never run
# anything they did not ship, which is why they were never affected.
#
# nix-ld puts a loader at that path which finds libraries in the set below;
# envfs makes /bin and /usr/bin show the programs on the caller's PATH. Both
# are switched on for every Sekka system, whatever applications were chosen,
# because a person can install any such program later themselves.
{
  lib,
  options,
  pkgs,
  ...
}:
{
  programs.nix-ld = {
    enable = true;
    # nix-ld's own set (the C++ runtime, zlib, OpenSSL, curl, systemd,
    # util-linux, …) and what a desktop Ubuntu installation has besides, for
    # the graphical programs and the native parts of extensions. Zed's own
    # build ships its other libraries; it loads Vulkan, Wayland, xkbcommon
    # and X11 from the system when it starts.
    libraries =
      options.programs.nix-ld.libraries.default
      ++ (with pkgs; [
        # Core
        glib
        icu
        krb5
        libffi
        libcap
        libgcrypt
        gmp
        libunwind
        libuuid
        libelf
        libbsd
        ncurses
        pcre2
        sqlite
        lz4
        brotli
        libidn2
        libpsl
        nghttp2
        gnutls
        libxslt
        e2fsprogs
        fuse
        fuse3
        libusb1
        dbus
        expat
        nss
        nspr
        # Graphics
        libglvnd
        libgbm
        libdrm
        libepoxy
        libva
        vulkan-loader
        wayland
        libxkbcommon
        libx11
        libxcb
        libxau
        libxdmcp
        libxext
        libxrender
        libxfixes
        libxi
        libxrandr
        libxcursor
        libxcomposite
        libxdamage
        libxinerama
        libxtst
        libxscrnsaver
        libxshmfence
        libxkbfile
        libxxf86vm
        libxt
        libxmu
        libxv
        libxft
        libsm
        libice
        libxcb-util
        libxcb-image
        libxcb-keysyms
        libxcb-render-util
        libxcb-wm
        libxcb-cursor
        # Toolkits, fonts and images
        gtk3
        gtk4
        pango
        cairo
        harfbuzz
        gdk-pixbuf
        at-spi2-core
        fontconfig
        freetype
        libpng
        libjpeg
        libtiff
        libwebp
        SDL2
        # Sound, desktop services and printing
        alsa-lib
        libpulseaudio
        pipewire
        openal
        libsecret
        libnotify
        cups
      ]);
  };

  # #!/bin/bash, #!/usr/bin/python3 and the like.
  services.envfs.enable = lib.mkDefault true;
}
