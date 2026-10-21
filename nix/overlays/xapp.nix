# Local fix for XApp 1.1.3's synchronous GTK theme callback. Review/remove
# when upstream fixes it; the original-crash control must be revisited then.
final: prev: {
  xdg-desktop-portal-xapp = prev.xdg-desktop-portal-xapp.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ../patches/xapp-settings-theme-idle.patch ];
  });
}
