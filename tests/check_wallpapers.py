#!/usr/bin/env python3
"""Check original wallpaper integrity and the built desktop catalog, offline."""

import hashlib
import json
from pathlib import Path
import sys
from urllib.parse import parse_qs, urlsplit
import xml.etree.ElementTree as ET

from PIL import Image


def check(source: Path, package: Path) -> None:
    entries = json.loads((source / "wallpapers.json").read_text())
    assert entries, "Empty wallpaper collection"
    filenames = [entry["file"] for entry in entries]
    assert len(filenames) == len(set(filenames)), "Duplicate wallpaper filename"
    assert set(filenames) == {path.name for path in source.glob("*.jpg")}
    assert json.loads((package / "share/tatami/wallpapers.json").read_text()) == entries
    catalog = ET.parse(package / "share/gnome-background-properties/tatami.xml")
    gnome = {entry.findtext("filename"): entry for entry in catalog.findall("wallpaper")}
    assert len(gnome) == len(entries)

    for entry in entries:
        filename = entry["file"]
        original = source / filename
        digest = hashlib.sha256(original.read_bytes()).hexdigest()
        assert digest == entry["sha256"], f"{filename}: original bytes changed"
        with Image.open(original) as image:
            image.load()  # Decode the whole image, including its last scanline.
            assert image.format == "JPEG", filename
            assert image.size == (entry["width"], entry["height"]), filename
            assert image.getexif().get(274, 1) == 1, f"{filename}: unexpected rotation"

        url = urlsplit(entry["download_url"])
        assert url.scheme == "https", filename
        if url.hostname == "images.unsplash.com":
            # Raw CDN URLs may carry attribution/client IDs, never transforms.
            assert set(parse_qs(url.query)) <= {"ixid", "ixlib"}, filename
        else:
            assert url.hostname == "cdn.esahubble.org", filename
            assert url.path.startswith("/archives/images/large/"), filename
            assert not url.query, filename

        installed = package / "share/backgrounds/tatami" / filename
        plasma = (package / "share/wallpapers" / f"tatami-{original.stem}"
                  / "contents/images" / f"{entry['width']}x{entry['height']}.jpg")
        # Every desktop gets the same original, never a resized derivative.
        assert installed.resolve() == plasma.resolve(), filename
        assert hashlib.sha256(installed.read_bytes()).hexdigest() == digest, filename
        thumbnail = package / "share/tatami/thumbnails" / filename
        assert thumbnail.resolve() != installed.resolve(), filename
        with Image.open(thumbnail) as preview:
            assert preview.size == (480, 300), filename
        background = gnome[f"/run/current-system/sw/share/backgrounds/tatami/{filename}"]
        assert background.findtext("options") == "zoom", filename
        print(f"PASS: {filename} ({entry['width']} × {entry['height']}), original SHA-256 and desktop links")


if __name__ == "__main__":
    check(Path(sys.argv[1]), Path(sys.argv[2]))
