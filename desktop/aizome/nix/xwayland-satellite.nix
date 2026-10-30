# Preserve X11 fullscreen intent at startup and across minimize/restore.
# Scoped to Aizome; other desktops retain their own Xwayland integration.
{
  lib,
  xwayland-satellite,
  # The root flake's checks.xwayland-satellite-unit: the unit tests, the
  # patches' own included, with a time limit.
  unitTests ? false,
}:
# The patches change this release's internals (its ECS queries, event.rs and
# xstate) and need rebasing for any other.
assert lib.assertMsg (xwayland-satellite.version == "0.8.3") ''
  Nixpkgs moved xwayland-satellite to ${xwayland-satellite.version}. Rebase
  desktop/aizome/nix/xwayland-satellite-*.patch onto it, run
  checks.x86_64-linux.xwayland-satellite-unit and xwayland-fullscreen, then
  update the version in desktop/aizome/nix/xwayland-satellite.nix.
'';
xwayland-satellite.overrideAttrs (
  old:
  {
    patches = (old.patches or [ ]) ++ [
      ./xwayland-satellite-fullscreen.patch
      ./xwayland-satellite-resolution.patch
    ];
    # The shipped package skips the tests, as nixpkgs does: the integration
    # tests need a running Xwayland and fail or hang under load, and every
    # Aizome system and the ISO would build them.
    doCheck = false;
  }
  // lib.optionalAttrs unitTests {
    pname = old.pname + "-unit-tests";
    doCheck = true;
    cargoTestFlags = [ "--lib" ];
    # cargoCheckHook runs cargo through env, so a time limit goes in PATH.
    # A hung test then fails the check instead of holding its builder.
    preCheck = (old.preCheck or "") + ''
      export XDG_RUNTIME_DIR=$(mktemp -d)
      mkdir -p "$TMPDIR/time-limit"
      printf '#!%s\nexec timeout --kill-after=30s 20m %s "$@"\n' \
        "$BASH" "$(command -v cargo)" > "$TMPDIR/time-limit/cargo"
      chmod +x "$TMPDIR/time-limit/cargo"
      export PATH="$TMPDIR/time-limit:$PATH"
    '';
  }
)
