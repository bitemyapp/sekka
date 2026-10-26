{ pkgs }:
pkgs.ananicy-cpp.overrideAttrs (old: {
  patches = (old.patches or [ ]) ++ [ ./ananicy-processes.patch ];
})
