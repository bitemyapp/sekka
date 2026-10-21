# Shared by the installed module and the media's application cache. Platform
# limits are explicit catalog data, never inferred by hiding evaluation errors.
{ system }:
builtins.filter (app: !(app ? systems) || builtins.elem system app.systems) (
  builtins.fromJSON (builtins.readFile ./applications.json)
)
