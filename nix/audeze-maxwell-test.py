"""Check the Maxwell's two playback devices and their independent mixer controls."""

import configparser
import json
import sys
from pathlib import Path


def acp_lines(path):
    for line in path.read_text().splitlines(keepends=True):
        if line.startswith(".include "):
            yield from acp_lines(path.parent / line.split(maxsplit=1)[1].strip())
        else:
            yield line


def read_acp(path):
    # ACP permits a profile to include another file and override individual keys.
    config = configparser.ConfigParser(interpolation=None, strict=False, delimiters=("=",))
    config.read_file(acp_lines(path), source=str(path))
    return config


rule_path, mixer_paths = map(Path, sys.argv[1:])
rule, = json.loads(rule_path.read_text())["monitor.alsa.rules"]
assert rule["matches"] and all(
    match.get("device.vendor.id") == "0x3329" and match.get("device.product.id") == "0x4b29"
    for match in rule["matches"]
)
profile_path = Path(rule["actions"]["update-props"]["device.profile-set"])
assert profile_path.is_absolute()
profile = read_acp(profile_path)
duplex = profile["Profile output:stereo-game+output:stereo-chat+input:mono-chat"]
assert set(duplex["output-mappings"].split()) == {"stereo-game", "stereo-chat"}
assert duplex["input-mappings"] == "mono-chat"

for mapping, index, element in [("stereo-chat", 0, "Element PCM"), ("stereo-game", 1, "Element PCM,1")]:
    output = profile[f"Mapping {mapping}"]
    assert output["device-strings"] == f"hw:%f,{index},0", mapping
    assert output["channel-map"] == "left,right", mapping
    path = read_acp(mixer_paths / (output["paths-output"] + ".conf"))
    assert {key for key in path if key.startswith("Element PCM")} == {element}, mapping
    assert path[element]["volume"] == "merge" and path[element]["switch"] == "mute", mapping
    assert path[element]["override-map.2"] == "all-left,all-right", mapping
