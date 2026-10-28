# SPDX-License-Identifier: MIT OR Apache-2.0
# Describe availability from the packages the catalog actually installs. Reading
# metadata does not build a package or relax Nixpkgs' installation policy.
{
  lib,
  hostPlatform,
  packagesFor,
}:
catalog:
let
  describe =
    app:
    let
      problems = lib.concatMap (
        package:
        if package == null || !lib.isDerivation package then
          [ "A required package is missing from this system's package source." ]
        else if !lib.meta.availableOn hostPlatform package then
          [ "Not available for ${hostPlatform.system}." ]
        else if package.meta.broken or false then
          [ "A required package is marked broken for ${hostPlatform.system}." ]
        else
          [ ]
      ) (packagesFor app);
      # Wrappers may have broad (or absent) meta.platforms while an input is
      # unsupported. Instantiate too, without building or retaining its closure.
      evaluated = map (package: package.drvPath) (packagesFor app);
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
          "A required package cannot be evaluated for ${hostPlatform.system}."
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
