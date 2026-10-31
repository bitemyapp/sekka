# Resolver updates may signal Avahi as soon as its PID file appears, before
# the normal SIGHUP handler is installed. Patch only the service package;
# clients can keep the unchanged upstream libraries without being rebuilt.
final: prev: {
  sekka-avahi-daemon = prev.avahi.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ../patches/avahi-startup-sighup.patch ];
  });
}
