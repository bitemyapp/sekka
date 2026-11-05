"""Which screen-coordinate convention a vision model uses, whatever it is
told: it is shown a real desktop screenshot, asked for the pixel positions
of three known elements, and each convention is scored by how far its
reading of the answers lands from the truth. Prints the best one (pixel,
norm1000 or norm1), for models.toml's `coords`.

    python3 grounding.py MODEL [--base-url http://127.0.0.1:8080/v1]
"""

import argparse
import base64
import json
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent / "harness"))
from llm import Client  # noqa: E402

SCREEN = (1440, 900)
# The plain Aizome desktop at 1440×900 (docs/agent-evals/screenshots/,
# sound-panel.png shows the same layout) and where these are on it.
SHOT = Path(__file__).resolve().parents[2] / "docs/agent-evals/screenshots/sound-panel.png"
TARGETS = {
    "clock": ("the clock at the right end of the top menu bar", (1411, 12)),
    "trash": ("the Trash icon on the desktop, at the left", (56, 245)),
    "toolchest": ("the word Toolchest at the left end of the top menu bar", (56, 12)),
}
CONVENTIONS = {
    "pixel": lambda x, y: (x, y),
    "norm1000": lambda x, y: (x * SCREEN[0] / 1000, y * SCREEN[1] / 1000),
    "norm1": lambda x, y: (x * SCREEN[0], y * SCREEN[1]),
}


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("model")
    ap.add_argument("--base-url", default="http://127.0.0.1:8080/v1")
    a = ap.parse_args()
    png = SHOT.read_bytes()
    ask = ("This screenshot is 1440 × 900 pixels. Give the pixel coordinates of the center of each of these, as "
           "JSON with integer pixel values, like {\"clock\": [x, y], ...}:\n"
           + "\n".join(f"- {k}: {d}" for k, (d, _) in TARGETS.items()))
    content = [{"type": "text", "text": ask},
               {"type": "image_url", "image_url": {"url": "data:image/png;base64," + base64.b64encode(png).decode()}}]
    r = Client(a.base_url, a.model).chat([{"role": "user", "content": content}], max_tokens=4096, temperature=0.0)
    text = r["choices"][0]["message"].get("content") or ""
    found = {}
    for k in TARGETS:
        m = re.search(rf'"?{k}"?\s*:\s*\[\s*([0-9.]+)\s*,\s*([0-9.]+)\s*\]', text)
        if m:
            found[k] = (float(m.group(1)), float(m.group(2)))
    if not found:
        sys.exit(f"no coordinates in the answer: {text[:500]!r}")
    scores = {}
    for name, f in CONVENTIONS.items():
        err = [((f(*found[k])[0] - t[0]) ** 2 + (f(*found[k])[1] - t[1]) ** 2) ** 0.5
               for k, (_, t) in TARGETS.items() if k in found]
        scores[name] = sum(err) / len(err)
    best = min(scores, key=scores.get)
    print(json.dumps({"answer": found, "mean_error_px": {k: round(v) for k, v in scores.items()}, "coords": best}))


if __name__ == "__main__":
    main()
