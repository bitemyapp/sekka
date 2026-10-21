# Retain the normal DMA-BUF path and retry failed allocations using ARGB8888
# and the matching primary DRM node for software GBM. Revisit this patch when
# updating hyprtoolkit; the launcher software-rendering VM check must pass.
final: prev: {
  hyprtoolkit = prev.hyprtoolkit.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ../patches/hyprtoolkit-buffer-allocation.patch ];
  });
}
