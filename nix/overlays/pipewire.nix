# Fix the audio service without rebuilding applications against new client
# libraries. Both packages retain PipeWire's protocol and ABI version.
final: prev: {
  sekka-pipewire-daemon = prev.pipewire.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ../patches/pipewire-x11-bell-disconnect.patch ];
  });
}
