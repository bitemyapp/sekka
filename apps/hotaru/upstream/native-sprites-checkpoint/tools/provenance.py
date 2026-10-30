#!/usr/bin/env python3
"""Capture exact packaged-source/runtime evidence; no product edits."""
import hashlib
import json
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

mirror, package, prepared, evidence = map(Path, sys.argv[1:])
digest = lambda file: hashlib.sha256(file.read_bytes()).hexdigest()
manifest = json.loads((evidence / "smoke/source-manifest.json").read_text())
missing, differences, matched = [], [], 0
for name, expected in manifest["sources"].items():
    source = prepared / name
    if not source.is_file(): missing.append(name)
    elif digest(source) != expected: differences.append(name)
    else: matched += 1
expected_missing = {"apps/hotaru/Cargo.lock", *[f"apps/hotaru/upstream/sidestep-{name}.patch" for name in ["clipboard", "input", "mime", "grid-font", "sprites"]]}
assert not differences, differences
assert set(missing) == expected_missing, missing
wrapper, elf = package / "bin/hotaru", package / "bin/.hotaru-wrapped"
header = subprocess.check_output(["readelf", "-l", str(elf)], text=True)
interpreter = re.search(r"Requesting program interpreter: ([^\]]+)", header).group(1)
(evidence / "elf-dependencies.txt").write_text(subprocess.check_output([interpreter, "--list", str(elf)], text=True))
(evidence / "package-wrapper.txt").write_bytes(wrapper.read_bytes())
fonts = Path(re.search(r"HOTARU_FONT_DIR=.*?'([^']+)'", wrapper.read_text()).group(1))
resources = package / "share/hotaru"
resource_hashes = {str(file.relative_to(resources)): digest(file) for file in resources.rglob("*") if file.is_file()}
resource_differences = [name for name, expected in resource_hashes.items() if digest(mirror / "apps/hotaru/resources" / name) != expected]
assert not resource_differences
notices = {
    "Ghostty-MIT.txt": "upstream/LICENSE.ghostty",
    "uucode-MIT.txt": "upstream/LICENSE.uucode",
    "Unicode-3.0.txt": "upstream/LICENSE.unicode",
    "Shell-integration-MIT.txt": "resources/LICENSE-MIT",
    "simdutf8-MIT.txt": "vendor/simdutf8/LICENSE-MIT",
    "simdutf8-Apache.txt": "vendor/simdutf8/LICENSE-Apache",
}
license_hashes = {file.name: digest(file) for file in (package / "share/licenses/hotaru").iterdir() if file.is_file()}
assert set(license_hashes) == set(notices)
for name, relative in notices.items():
    assert license_hashes[name] == digest(mirror / "apps/hotaru" / relative), name
font_hashes = {file.name: digest(file) for file in fonts.iterdir() if file.is_file()}
assert font_hashes["NotoColorEmoji.ttf"] == "3a44421dc3ce672f80fa0d4fdf7da5b583494ab4517178747f5fa6f287db5c1d"
assert font_hashes["NotoSansCJK-Regular.ttc"] == "b76b0433203017ca80401b2ee0dd69350349871c4b19d504c34dbdd80541690a"
for name in ["OFL-NotoColorEmoji.txt", "OFL-NotoSansCJK.txt"]:
    assert font_hashes[name] == "6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2"
report = {
    "recorded_utc": datetime.now(timezone.utc).isoformat(), "package": str(package), "prepared_source": str(prepared),
    "wrapper_sha256": digest(wrapper), "elf_sha256": digest(elf), "elf_interpreter": interpreter,
    "font_files": font_hashes, "application_licenses": license_hashes, "resource_files": resource_hashes,
    "installed_resource_source_differences": resource_differences,
    "prepared_source_comparison": {"matches": matched, "missing": missing, "differences": differences},
    "runtime_environment": {"PATH": "/usr/bin:/bin", "unset": ["AIZOME_FONT_DIR", "HOTARU_FONT_DIR", "HOTARU_RESOURCES_DIR", "HOTARU_TERMINFO_DIR", "FONTCONFIG_FILE", "FONTCONFIG_PATH", "LD_LIBRARY_PATH", "SIDESTEP_FONT", "SIDESTEP_MONO_FONT"], "private_compositor": "headless Sway/pixman", "runtime_configuration": "package wrapper", "links_opener": "disposable fake xdg-open prepended by private fixture"},
    "suites": {name: json.loads((evidence / name / "result.json").read_text())["status"] for name in ["smoke", "fonts", "search", "shell", "links", "cursor", "pixel_scroll", "sprites"]},
}
assert all(status == "passed" for status in report["suites"].values())
(evidence / "package-runtime.json").write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps({"suites": report["suites"], "source_matches": matched, "licenses": len(license_hashes)}, indent=2))
