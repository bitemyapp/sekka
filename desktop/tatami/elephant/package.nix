# Tatami's launcher backend. Only the providers the launcher and menu use
# (config/walker): not Arch package search, password managers or other
# compositors' windows. search.patch matches applications by name, generic
# name and keyword, and keeps the generic name, which the launcher shows
# under each name, as the subtext. Run the launcher-search check
# (tests/test_launcher_search.py) after an Elephant update; a patch failure
# requires that review, not a skipped patch.
{ elephant }:
(elephant.override {
  enabledProviders = [
    "desktopapplications"
    "menus"
    "providerlist"
    "calc"
    "clipboard"
    "files"
    "symbols"
    "websearch"
  ];
}).overrideAttrs
  (old: {
    patches = (old.patches or [ ]) ++ [
      ./search.patch
      ./files-watch.patch
    ];
  })
