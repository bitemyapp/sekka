# Sourced by Signal's launcher. Read only backend metadata, never the key.
# Electron otherwise guesses from XDG_CURRENT_DESKTOP, so a profile made in
# Plasma cannot open under Aizome (which Electron calls basic_text).
signal_password_store() {
  local profile="${XDG_CONFIG_HOME:-$HOME/.config}/Signal" backend

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
    gnome_libsecret) printf '%s\n' gnome-libsecret ;;
    kwallet|kwallet5|kwallet6) printf '%s\n' "$backend" ;;
    basic_text) printf '%s\n' basic ;;
  esac
}

signal_store=$(signal_password_store "$@")
if [ -n "$signal_store" ]; then
  set -- "--password-store=$signal_store" "$@"
fi
unset signal_store
unset -f signal_password_store
