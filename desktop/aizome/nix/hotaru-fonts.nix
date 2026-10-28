# Terminal-only fallback faces, registered process-locally by Hotaru's font
# worker. Keep Aizome's chrome bundle unchanged and avoid retaining all CJK
# weights in the application closure.
{
  lib,
  runCommand,
  fetchurl,
  noto-fonts-color-emoji,
}:
let
  cjkRev = "523d033d6cb47f4a80c58a35753646f5c3608a78"; # Sans2.004
  emojiRev = "8998f5dd683424a73e2314a8c1f1e359c19e8742"; # v2.051
  cjk = fetchurl {
    url = "https://raw.githubusercontent.com/notofonts/noto-cjk/${cjkRev}/Sans/OTC/NotoSansCJK-Regular.ttc";
    hash = "sha256-t2sEMyAwF8qAQBsu4N1pNQNJhxxLGdUEw0292AVBaQo=";
  };
  cjkLicense = fetchurl {
    url = "https://raw.githubusercontent.com/notofonts/noto-cjk/${cjkRev}/LICENSE";
    hash = "sha256-anP5VBwt50FYwOfPawpY73dPWngL8ZHy1+ycxT7+K/I=";
  };
  emojiLicense = fetchurl {
    url = "https://raw.githubusercontent.com/googlefonts/noto-emoji/${emojiRev}/fonts/LICENSE";
    hash = "sha256-anP5VBwt50FYwOfPawpY73dPWngL8ZHy1+ycxT7+K/I=";
  };
in
runCommand "hotaru-fonts"
  {
    meta = {
      description = "Hotaru's Noto CJK and color emoji fallback fonts";
      license = lib.licenses.ofl;
      platforms = lib.platforms.all;
    };
  }
  ''
  mkdir -p $out
  install -m444 ${cjk} $out/NotoSansCJK-Regular.ttc
  install -m444 ${noto-fonts-color-emoji}/share/fonts/noto/NotoColorEmoji.ttf $out/NotoColorEmoji.ttf
  install -m444 ${cjkLicense} $out/OFL-NotoSansCJK.txt
  install -m444 ${emojiLicense} $out/OFL-NotoColorEmoji.txt
''
