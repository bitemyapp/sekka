# Whether anyone may redistribute a package: every licence it names says so
# (Nixpkgs' `redistributable`, true of free licences, of NVIDIA's driver and of
# unfreeRedistributable ones, false of Google Chrome's or VS Code's). As
# Nixpkgs' `allowUnfreePredicate` it is asked of unfree packages alone, which
# always name a licence. A package that names none passes, so a caller that
# must not take silence for permission checks for licences first.
{ lib }:
package:
lib.all (license: license.redistributable or false) (lib.toList (package.meta.license or [ ]))
