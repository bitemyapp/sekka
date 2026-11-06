#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Put Sekka Installer.app in a disk image with a link to /Applications, and
# write its SHA-256 beside it.
#
#   packaging/make-dmg.sh DIR VERSION
set -euo pipefail
dir=${1:?the directory build-app.sh wrote}
version=${2:?the version, for the file name}
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
ditto "$dir/Sekka Installer.app" "$stage/Sekka Installer.app"
ln -s /Applications "$stage/Applications"
dmg="$dir/Sekka-Installer-$version.dmg"
rm -f "$dmg"
hdiutil create -volname "Sekka Installer" -srcfolder "$stage" -format ULFO -fs APFS "$dmg"
(cd "$dir" && shasum -a 256 "$(basename "$dmg")" > "$(basename "$dmg").sha256")
echo "$dmg"
