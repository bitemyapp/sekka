# SPDX-License-Identifier: MIT OR Apache-2.0
# Describe availability from the packages the catalog actually installs. Reading
# metadata does not build a package or relax Nixpkgs' installation policy.
#
# Systems read metadata only. Instantiating every application's packages
# nearly doubled a reference system's evaluation, and it meets Nixpkgs' policy
# too, so a system refusing unfree packages would have reported those as
# unsupported. Wrappers whose metadata claims more than their dependencies
# support, as Steam's 32-bit runtime does on ARM, list their `platforms` in
# the catalog. So do entries from other flakes, whose packages systems never
# read: reading them would fetch those flakes for every system. Checks of the
# catalogs instantiate everything to keep those lists and the `unfree` flags
# honest.
{
  lib,
  hostPlatform,
  packagesFor,
  # The configuration of the Nixpkgs the packages come from. Packages it
  # would install although unsupported or broken are offered too.
  config ? { },
  # Entries whose `platforms` list alone decides, without reading their
  # packages: those another flake packages, which reading would fetch and
  # evaluate on every system, picked or not. One without a list is not
  # offered.
  listed ? _: false,
  # Also instantiate each package, without building or retaining its closure.
  # Every entry's packages are read then, listed or not.
  instantiate ? false,
}:
catalog:
let
  inherit (hostPlatform) system;
  allowUnsupported = config.allowUnsupportedSystem or false;
  allowBroken = config.allowBroken or false;
  describe =
    app:
    let
      problems =
        if app ? platforms && !builtins.elem system app.platforms then
          [ "Not available for ${system}." ]
        else if listed app && !instantiate then
          lib.optional (!(app ? platforms)) "Not listed as available for ${system}."
        else
          lib.concatMap (
            package:
            if package == null || !lib.isDerivation package then
              [ "A required package is missing from this system's package source." ]
            else if !allowUnsupported && !lib.meta.availableOn hostPlatform package then
              [ "Not available for ${system}." ]
            else if !allowBroken && package.meta.broken or false then
              [ "A required package is marked broken for ${system}." ]
            else
              [ ]
          ) (packagesFor app);
      evaluated = lib.optionals instantiate (map (package: package.drvPath) (packagesFor app));
      checked = builtins.tryEval (
        builtins.deepSeq problems (
          if problems == [ ] then builtins.deepSeq evaluated problems else problems
        )
      );
    in
    app
    // {
      unavailable =
        if !checked.success then
          "A required package cannot be evaluated for ${system}."
        else if checked.value != [ ] then
          builtins.head checked.value
        else
          null;
    };
  described = map describe catalog;
  byId = lib.listToAttrs (map (app: lib.nameValuePair app.id app) described);
  # Include every dependency, even when it has dependencies of its own. A
  # generic closure also terminates if a third-party catalog contains a cycle.
  dependencies =
    app:
    builtins.genericClosure {
      startSet = [ { key = app.id; } ];
      operator = entry: map (id: { key = id; }) (byId.${entry.key}.requires or [ ]);
    };
in
map (
  app:
  let
    blocked = lib.findFirst (
      entry: !(byId ? ${entry.key}) || byId.${entry.key}.unavailable != null
    ) null (dependencies app);
  in
  app
  // lib.optionalAttrs (app.unavailable == null && blocked != null) {
    unavailable = "Requires ${blocked.key}, which is unavailable on this system.";
  }
) described
