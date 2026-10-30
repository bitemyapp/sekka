# SPDX-License-Identifier: GPL-3.0-or-later
# Chromium and Electron keep the key that seals cookies, saved passwords and
# Electron's safeStorage in the keyring of a desktop they recognise: GNOME
# Keyring in GNOME, Xfce or Cinnamon, KWallet in Plasma. In one they don't,
# such as Tatami, Hyprland or Aizome, they fall back to a fixed key in the
# profile, which Electron's safeStorage treats as no encryption: Claude and
# ChatGPT then keep no sign-in between launches. Every Sekka desktop runs
# GNOME Keyring, unlocked at login, so outside Plasma the catalog's Chromium
# and Electron apps are pointed at it. What they sealed with the fixed key
# still opens.
#
# Signal records its backend and refuses to open its database if it changes.
# Keep that recorded backend across desktops while its service is on the
# session bus; new profiles use GNOME Keyring.
# Bitwarden reaches the keyring by itself.
#
# Used by applications.nix and by the ISO's per-application closures
# (nix/applications.nix), with the same package set, so an offline
# installation finds the wrapped packages on the medium.
{ lib, pkgs }:
app: package:
let
  apps = [
    "signal"
    "chromium"
    "google-chrome"
    "chatgpt"
    "claude-desktop"
    "vscode"
    "obsidian"
    "discord"
    "slack"
  ];
  signalKeyring = pkgs.writeText "signal-keyring.sh" (
    builtins.replaceStrings [ "@jq@" "@busctl@" ] [ "${pkgs.jq}/bin/jq" "${pkgs.systemd}/bin/busctl" ] (
      builtins.readFile ./signal-keyring.sh
    )
  );
  launchPolicy =
    if app.id == "signal" then
      ". ${signalKeyring}"
    else
      ''
        case ":''${XDG_CURRENT_DESKTOP-}:" in
          *:KDE:*) ;;
          *) set -- --password-store=gnome-libsecret "$@" ;;
        esac
      '';
in
if !builtins.elem app.id apps then
  package
else
  pkgs.symlinkJoin {
    inherit (package) name;
    paths = [ package ];
    nativeBuildInputs = [ pkgs.makeWrapper ];
    # A package may link a whole directory from another (ChatGPT's share
    # is its unwrapped package's): make those directories this package's
    # own before changing what is in them. Launchers that name the program
    # by its store path get this one.
    postBuild = ''
      own() {
        if [ -L "$1" ]; then
          target=$(readlink -f "$1")
          rm "$1"
          mkdir "$1"
          ${lib.getExe' (pkgs.lndir or pkgs.xorg.lndir) "lndir"} -silent "$target" "$1"
        fi
      }
      own $out/bin
      own $out/share
      own $out/share/applications
      for program in $out/bin/*; do
        rm "$program"
        makeWrapper ${package}/bin/"''${program##*/}" "$program" \
          --run ${lib.escapeShellArg launchPolicy}
      done
      for entry in $out/share/applications/*.desktop; do
        [ -e "$entry" ] || continue
        target=$(readlink -f "$entry")
        rm "$entry"
        substitute "$target" "$entry" --replace-quiet ${package} $out
      done
    '';
    passthru = lib.optionalAttrs (package ? version) { inherit (package) version; };
    meta = package.meta or { };
  }
