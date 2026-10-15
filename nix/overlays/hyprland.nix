# Sekka's fix for Hyprland's screencopy sessions, proposed upstream with the
# same patch. wlr-screencopy and toplevel-export sessions outlived their
# clients, and at monitor scales other than 1 a region share made a new one
# for every frame; every session costs every frame, so the compositor slowed
# until it was restarted. Remove this when a Hyprland release includes the
# fix, after the screenshare-sessions check passes with the new upstream
# package. A patch failure during an input update requires that review; do
# not just skip the patch.
final: prev: {
  hyprland = prev.hyprland.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ../patches/hyprland-screenshare-sessions.patch ];
  });
}
