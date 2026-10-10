# SPDX-License-Identifier: MIT OR Apache-2.0
# Tatami's wallpapers, laid out for every desktop from one copy of each
# image: share/backgrounds/tatami (Tatami, Xfce, GNOME), share/wallpapers
# (Plasma's wallpaper packages) and share/gnome-background-properties
# (GNOME's list). Everything but the images and their metadata is a symlink
# to the image's own store path. The images keep their own licenses (see
# README.md): the Unsplash License, which allows redistribution but is not an
# open-source license, and CC BY 4.0.
{
  lib,
  linkFarm,
  writeText,
}:
let
  wallpapers = lib.importJSON ./wallpapers.json;
  # Where the images are once the package is in environment.systemPackages:
  # the paths desktops store in their settings, which survive updates.
  installed = file: "/run/current-system/sw/share/backgrounds/tatami/${file}";
  slug = wallpaper: lib.removeSuffix ".jpg" wallpaper.file;
  image = wallpaper: ./. + "/${wallpaper.file}";
  plasma =
    wallpaper:
    let
      dir = "share/wallpapers/tatami-${slug wallpaper}";
    in
    [
      {
        name = "${dir}/metadata.json";
        path = writeText "tatami-${slug wallpaper}-metadata.json" (
          builtins.toJSON {
            KPackageStructure = "Wallpaper/Images";
            KPlugin = {
              Id = "tatami-${slug wallpaper}";
              Name = wallpaper.name;
              Authors = [ { Name = wallpaper.author; } ];
              License = wallpaper.license;
              Website = wallpaper.source;
            };
          }
        );
      }
      {
        # Plasma picks an image by the size in its name.
        name = "${dir}/contents/images/${toString wallpaper.width}x${toString wallpaper.height}.jpg";
        path = image wallpaper;
      }
    ];
  gnome = writeText "tatami-gnome-backgrounds.xml" ''
    <?xml version="1.0" encoding="UTF-8"?>
    <!DOCTYPE wallpapers SYSTEM "gnome-wp-list.dtd">
    <wallpapers>
    ${lib.concatMapStrings (wallpaper: ''
      <wallpaper deleted="false">
        <name>${lib.escapeXML wallpaper.name}</name>
        <filename>${installed wallpaper.file}</filename>
        <options>zoom</options>
        <shade_type>solid</shade_type>
        <pcolor>#1a1b26</pcolor>
        <scolor>#1a1b26</scolor>
      </wallpaper>
    '') wallpapers}
    </wallpapers>
  '';
in
(linkFarm "tatami-wallpapers" (
  lib.concatMap (wallpaper: [
    {
      name = "share/backgrounds/tatami/${wallpaper.file}";
      path = image wallpaper;
    }
    {
      name = "share/tatami/thumbnails/${wallpaper.file}";
      path = ./thumbnails + "/${wallpaper.file}";
    }
  ]) wallpapers
  ++ lib.concatMap plasma wallpapers
  ++ [
    {
      name = "share/gnome-background-properties/tatami.xml";
      path = gnome;
    }
    {
      name = "share/tatami/wallpapers.json";
      path = ./wallpapers.json;
    }
  ]
)).overrideAttrs
  {
    passthru = {
      inherit wallpapers installed;
    };
    meta.description = "Wallpapers from Tatami, for every desktop";
  }
