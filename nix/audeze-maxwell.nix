{
  pipewire,
  writeText,
  writeTextDir,
}:
let
  # On USB 3329:4b29 both playback endpoints are stereo. The stock profile
  # therefore assigns its PCM,1 mixer path to both. Chat has its own PCM,0.
  profile = writeText "audeze-maxwell.conf" ''
    .include ${pipewire}/share/alsa-card-profile/mixer/profile-sets/usb-gaming-headset.conf

    [Mapping stereo-chat]
    paths-output = analog-output
  '';
in
writeTextDir "share/wireplumber/wireplumber.conf.d/50-sekka-audeze-maxwell.conf" (
  builtins.toJSON {
    "monitor.alsa.rules" = [
      {
        matches = [
          {
            "device.vendor.id" = "0x3329";
            "device.product.id" = "0x4b29";
          }
        ];
        actions.update-props."device.profile-set" = "${profile}";
      }
    ];
  }
)
