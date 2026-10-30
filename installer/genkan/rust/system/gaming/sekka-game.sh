# Steam launch options: sekka-game -- %command%
# Options are local to this game; nothing exports GPU overrides globally.
hud=false
nested=false
hdr=false
tearing=false
use_gamemode=true
mode=
while [[ $# -gt 0 ]]; do
  case "$1" in
    --hud) hud=true ;;
    --gamescope) nested=true ;;
    --mode=*) mode=${1#--mode=}; nested=true ;;
    --mode)
      if [[ $# -lt 2 ]]; then
        printf 'sekka-game: --mode needs WxH or WxH@Hz\n' >&2
        exit 2
      fi
      mode=$2; nested=true; shift ;;
    --hdr) hdr=true; nested=true ;;
    --tearing) tearing=true; nested=true ;;
    --no-gamemode) use_gamemode=false ;;
    --help|-h)
      cat <<'HELP'
Usage: sekka-game [--hud] [--gamescope] [--mode WxH[@Hz]] [--hdr] [--tearing] [--no-gamemode] -- COMMAND [ARG...]

Steam launch options: sekka-game -- %command%
Add --hud for frame times and GPU usage, or --gamescope for a fullscreen game container.
--mode sets the game's resolution and refresh rate in Gamescope, usually the display's own,
for example --mode 2560x1440@144. Without it, nested Gamescope renders at 1280x720.
--hdr needs an HDR-capable host compositor (Tatami with HDR enabled, or the Steam login session).
--tearing allows immediate flips where the host compositor and driver support them.
Inside the Steam login session, Gamescope is already running and is not nested again.
GameMode applies only while the game runs. --no-gamemode leaves CPU policy alone.
HELP
      exit 0 ;;
    --) shift; break ;;
    *) printf 'sekka-game: unknown option: %s (use -- before the game command)\n' "$1" >&2; exit 2 ;;
  esac
  shift
done
if [[ $# -eq 0 ]]; then
  printf 'sekka-game: expected a game command after --\n' >&2
  exit 2
fi
size=()
if [[ -n "$mode" ]]; then
  if [[ ! "$mode" =~ ^([1-9][0-9]*)x([1-9][0-9]*)(@([1-9][0-9]*))?$ ]]; then
    printf 'sekka-game: --mode needs WxH or WxH@Hz, not %s\n' "$mode" >&2
    exit 2
  fi
  width=${BASH_REMATCH[1]} height=${BASH_REMATCH[2]} refresh=${BASH_REMATCH[4]}
  # The output and the game's own resolution; the window still fills the display.
  size=(-W "$width" -H "$height" -w "$width" -h "$height")
  if [[ -n "$refresh" ]]; then size+=(-r "$refresh"); fi
fi

in_gamescope=false
if [[ -n "${GAMESCOPE_WAYLAND_DISPLAY:-}" || ":${XDG_CURRENT_DESKTOP:-}:" == *:gamescope:* ]]; then
  in_gamescope=true
fi
if $hdr && ! $in_gamescope && [[ ":${XDG_CURRENT_DESKTOP:-}:" == *:Aizome:* ]]; then
  printf 'sekka-game: Aizome currently displays SDR. For HDR, choose Steam at login or enable HDR in Tatami Displays.\n' >&2
  exit 2
fi
if $tearing && ! $in_gamescope && [[ ":${XDG_CURRENT_DESKTOP:-}:" == *:Aizome:* ]]; then
  printf 'sekka-game: Aizome does not support tearing. Choose the Steam login session with calamares.gaming.gamescopeSession.allowTearing enabled.\n' >&2
  exit 2
fi

command=()
if $nested && ! $in_gamescope; then
  command+=(gamescope --fullscreen "${size[@]}")
  if [[ -z "$mode" ]]; then
    printf 'sekka-game: Gamescope renders at 1280x720; add --mode WxH[@Hz] for your display.\n' >&2
  fi
  if $hdr; then command+=(--hdr-enabled); fi
  if $tearing; then command+=(--immediate-flips); fi
  command+=(--)
elif $tearing; then
  printf 'sekka-game: configure immediate flips in the Steam session; a per-game wrapper cannot change its running compositor.\n' >&2
fi
if $use_gamemode; then command+=(gamemoderun); fi
if $hud; then command+=(mangohud); fi
exec "${command[@]}" "$@"
