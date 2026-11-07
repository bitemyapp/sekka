#!/bin/bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Notarize and staple a release-signed Sekka Installer.app and its
# recovery.zip, with a notarytool keychain profile the maintainer created
# (xcrun notarytool store-credentials). Releases only; publishing is a
# separate, explicitly authorized step (packaging/publish-dry-run.py).
#
#   packaging/notarize.sh PROFILE DIR
set -euo pipefail
profile=${1:?notarytool keychain profile}
dir=${2:?the directory build-app.sh --release wrote}
app="$dir/Sekka Installer.app"
codesign --verify --deep --strict --check-notarization "$app" 2>/dev/null && { echo "already notarized"; exit 0; }
submit="$dir/Sekka Installer.zip"
ditto -c -k --keepParent "$app" "$submit"
xcrun notarytool submit "$submit" --keychain-profile "$profile" --wait
xcrun stapler staple "$app"
rm -f "$submit"
xcrun notarytool submit "$dir/recovery.zip" --keychain-profile "$profile" --wait
spctl --assess --type execute --verbose "$app"
