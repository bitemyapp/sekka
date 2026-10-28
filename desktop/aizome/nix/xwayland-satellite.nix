# Keep initial X11 fullscreen state until its Wayland toplevel exists.
# Scoped to Aizome; other desktops retain their own Xwayland integration.
{ xwayland-satellite, xwayland }:
xwayland-satellite.overrideAttrs (old: {
  patches = (old.patches or [ ]) ++ [ ./xwayland-satellite-fullscreen.patch ];
  doCheck = true;
  nativeCheckInputs = (old.nativeCheckInputs or [ ]) ++ [ xwayland ];
  preCheck = (old.preCheck or "") + ''
    export XDG_RUNTIME_DIR=$(mktemp -d)
  '';
})
