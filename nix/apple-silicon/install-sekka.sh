#!/bin/sh
# SPDX-License-Identifier: MIT
# Install Sekka from this directory on an Apple Silicon Mac, from macOS.
# Adapted from the Asahi Linux installer's bootstrap (AsahiLinux/asahi-installer
# scripts/bootstrap.sh, MIT): it runs the unmodified Asahi installer, pinned to
# the version Sekka was tested with, with this directory's installer_data.json
# and OS package instead of Asahi's. Nothing is downloaded but the installer
# itself and Apple's firmware.
#
# The installer shrinks macOS, adds Sekka's partitions in the free space, and
# asks you to finish in recoveryOS: shut down, hold the power button, choose
# Sekka, and enter your password. It needs administrator rights (sudo).

if true; then
    set -e

    if [ ! -e /System ]; then
        echo "Run this from macOS on the Mac Sekka is for."
        exit 1
    fi

    export LC_ALL=en_US.UTF-8
    export LANG=en_US.UTF-8
    export PATH="/usr/bin:/bin:/usr/sbin:/sbin:$PATH"

    here=$(cd "$(dirname "$0")" && pwd)
    version=v0.9.2
    sha256=484d999e53b4aef27d17c4478e49567e96842f8a6cb35c073a0df5bf0bbe8276

    export INSTALLER_BASE=https://cdn.asahilinux.org/installer
    export INSTALLER_DATA="file://$here/installer_data.json"
    export REPO_BASE="$here"

    echo
    echo "Sekka for Apple Silicon: developer image"
    echo
    echo "  Back up this Mac first. The installer resizes the macOS partition."
    echo "  On macOS 27.0.1, Apple's resize has failed for others with error -69606"
    echo "  (AsahiLinux/asahi-installer#448); if it does, the installer stops and"
    echo "  macOS is left as it was. This Mac's limits:"
    diskutil apfs resizeContainer disk0s2 limits 2>/dev/null | sed 's/^/    /' || true
    echo
    printf "  Type 'install' to continue: "
    read -r answer
    [ "$answer" = install ] || { echo "Nothing was changed."; exit 1; }

    tmp=/tmp/sekka-install
    if [ -e "$tmp" ]; then
        mv "$tmp" "$tmp-$(date +%Y%m%d-%H%M%S)"
    fi
    mkdir -p "$tmp"
    cd "$tmp"

    echo "  Downloading the Asahi installer $version..."
    curl --no-progress-meter -L -o installer.tar.gz "$INSTALLER_BASE/installer-$version.tar.gz"
    if [ "$(shasum -a 256 installer.tar.gz | cut -d' ' -f1)" != "$sha256" ]; then
        echo "The installer download does not match the version Sekka was tested with."
        exit 1
    fi
    curl --no-progress-meter -L -O "$INSTALLER_DATA"
    tar xf installer.tar.gz

    echo "  Starting the installer..."
    echo
    if [ "$USER" != "root" ]; then
        echo "The installer needs to run as root."
        echo "Please enter your password if prompted."
        exec caffeinate -dis sudo -E ./install.sh "$@"
    else
        exec caffeinate -dis ./install.sh "$@"
    fi
fi
