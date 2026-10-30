# Sourced by Signal's launcher. Read only backend metadata, never the key.
# Electron otherwise guesses from XDG_CURRENT_DESKTOP, so a profile made in
# Plasma cannot open under Aizome (which Electron calls basic_text).

# Whether the session bus has the keyring's service, running or activatable.
signal_bus_has() {
  local reply
  reply=$(@busctl@ --user call org.freedesktop.DBus /org/freedesktop/DBus \
    org.freedesktop.DBus NameHasOwner s "$1" 2>/dev/null) || return 1
  if [ "$reply" = "b true" ]; then
    return 0
  fi
  reply=$(@busctl@ --user call org.freedesktop.DBus /org/freedesktop/DBus \
    org.freedesktop.DBus ListActivatableNames 2>/dev/null) || return 1
  case " $reply " in
    *" \"$1\" "*) return 0 ;;
  esac
  return 1
}

signal_password_store() {
  local profile="${XDG_CONFIG_HOME:-$HOME/.config}/Signal" backend store service

  # Signal's node-config storagePath/storageProfile can override Electron's
  # userData path. Leave custom node-config launches to Signal rather than
  # accidentally selecting the backend of the default profile.
  [ -z "${NODE_CONFIG:-}" ] || return 0
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --password-store|--password-store=*) return 0 ;;
      --user-data-dir=*) profile=${1#*=} ;;
      --user-data-dir)
        [ "$#" -ge 2 ] || return 0
        shift
        profile=$1
        ;;
      --) break ;;
    esac
    shift
  done
  [ -n "$profile" ] || return 0

  backend=gnome_libsecret
  if [ -e "$profile/config.json" ]; then
    # An older encrypted profile without backend metadata, an unknown
    # backend, or a malformed config must keep Signal's own error/recovery
    # path. Do not guess, rewrite the profile, or downgrade encryption.
    backend=$(@jq@ -er '
      if type != "object" then empty
      elif has("safeStorageBackend") then .safeStorageBackend
      elif has("encryptedKey") then empty
      else "gnome_libsecret" end
      | select(type == "string")
    ' "$profile/config.json" 2>/dev/null) || return 0
  fi
  case "$backend" in
    gnome_libsecret) store=gnome-libsecret service=org.freedesktop.secrets ;;
    kwallet) store=kwallet service=org.kde.kwalletd ;;
    kwallet5) store=kwallet5 service=org.kde.kwalletd5 ;;
    kwallet6) store=kwallet6 service=org.kde.kwalletd6 ;;
    basic_text)
      printf '%s\n' basic
      return 0
      ;;
    *) return 0 ;;
  esac
  # Electron reports a keyring it was told to use as selected even when it
  # cannot reach it, and Signal then shows a generic error that offers to
  # delete its data. Without the flag, Signal names the keyring it needs.
  signal_bus_has "$service" || return 0
  printf '%s\n' "$store"
}

signal_store=$(signal_password_store "$@")
if [ -n "$signal_store" ]; then
  set -- "--password-store=$signal_store" "$@"
fi
unset signal_store
unset -f signal_password_store signal_bus_has
