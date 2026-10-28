#!/usr/bin/env python3
"""Extract only the pinned scalar-to-line mapping, never raster expectations."""
import argparse
import hashlib
import pathlib
import re
import subprocess

ROOT = pathlib.Path(__file__).resolve().parent.parent
PIN = "6ba144158577d033ccb6ed4b574e4013957c9161"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=pathlib.Path, required=True)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    provenance = __import__("json").loads((ROOT / "evidence/provenance.json").read_text())
    raw = args.source.read_bytes()
    expected = provenance["unchanged_raster_sources"]["src/font/sprite/draw/box.zig"]
    if hashlib.sha256(raw).hexdigest() != expected or provenance["ghostty_revision"] != PIN:
        raise SystemExit("upstream source does not match the reviewed pin")
    rows = []
    for cp, body in re.findall(r"(0x[0-9a-f]+) => linesChar\(metrics, canvas, \.\{(.*?)\}\)", raw.decode()):
        fields = dict(re.findall(r"\.(up|right|down|left) = \.(none|light|heavy|double)", body))
        values = ", ".join(fields.get(x, "none").title() for x in ("up", "right", "down", "left"))
        rows.append(f"        {cp} => [{values}],")
    assert len(rows) == 109
    generated = f"// Generated scalar mapping from Ghostty {PIN}; MIT license.\n"
    generated += "// Raster algorithms are hand-ported separately; see tools/generate-lines.py.\n"
    generated += "pub(super) fn definition(cp: u32) -> Option<[Style; 4]> {\n    use Style::*;\n    Some(match cp {\n"
    generated += "\n".join(rows) + "\n        _ => return Option::None,\n    })\n}\n"
    formatted = subprocess.run(["rustfmt", "--edition", "2024", "--emit", "stdout"],
                               input=generated, text=True, capture_output=True, check=True).stdout
    output = ROOT / "src/lines_map.rs"
    if args.check:
        if output.read_text() != formatted:
            raise SystemExit("generated mapping differs")
    else:
        output.write_text(formatted)


if __name__ == "__main__":
    main()
