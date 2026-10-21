# Local fixes for XApp 1.1.3's GTK theme callback and session lifetime.
# Review/remove as upstream fixes them; revisit the corresponding controls.
final: prev: {
  xdg-desktop-portal-xapp = prev.xdg-desktop-portal-xapp.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [
      ../patches/xapp-settings-theme-idle.patch
      # Match GTK's portal lifecycle: stop before the graphical session's
      # display disappears instead of retaining a stale backend on the bus.
      ../patches/xapp-session-lifecycle.patch
    ];
  });
}
