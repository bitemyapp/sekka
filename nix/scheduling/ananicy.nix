{ pkgs }:
pkgs.ananicy-cpp.overrideAttrs (old: {
  patches = (old.patches or [ ]) ++ [
    ./ananicy-processes.patch
    ./ananicy-descendants.patch
  ];
  postPatch = (old.postPatch or "") + ''
    cp ${./descendant-rules.hpp} include/sekka-descendant-rules.hpp
  '';
})
