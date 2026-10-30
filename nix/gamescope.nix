# Keep the shader compiler alive only while its Vulkan driver is alive.
# Scoped package override: no global nixpkgs overlay or graphics overrides.
{ gamescope }:
gamescope.overrideAttrs (old: {
  separateDebugInfo = true;
  patches = (old.patches or [ ]) ++ [ ./patches/gamescope-vulkan-shutdown.patch ];
})
