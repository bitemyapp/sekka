"""Print the screen centre of an Omperor element: by .id name, or by text.

    omp-at.py [--name N | --text T] [--nth K] [--window-at X,Y]
The window's position on screen comes from `aizomectl windows`.
"""
import glob, json, os, subprocess, sys, argparse

p = argparse.ArgumentParser()
p.add_argument("--name"); p.add_argument("--text"); p.add_argument("--nth", type=int, default=0)
p.add_argument("--list", action="store_true"); p.add_argument("--exact", action="store_true"); p.add_argument("--rect", action="store_true"); p.add_argument("--minx", type=float, default=-1e9); p.add_argument("--all", action="store_true")
a = p.parse_args()
root = os.environ["root"]
files = sorted(glob.glob(f"{root}/inspect/org.aizome.OmperorDev.*.json"), key=os.path.getmtime)
doc = json.load(open(files[-1]))
line = [l for l in subprocess.check_output(["aizomectl", "windows"], text=True).splitlines() if "org.aizome.OmperorDev" in l][0]
pos = [w for w in line.split() if "," in w and w.replace(",", "").lstrip("-").isdigit()][0]
wx, wy = (int(v) for v in pos.split(","))
hits = []
for e in doc["elements"]:
    if a.list:
        print(json.dumps({k: e.get(k) for k in ("name", "text", "label", "role", "rect", "visible")}))
        continue
    t = e.get("text") or e.get("label") or ""
    if e["rect"][0] < a.minx:
        continue
    if (a.name and e.get("name") == a.name) or (a.text and ((a.text == t) if a.exact else (a.text in t))):
        if a.all or e.get("visible", True):
            hits.append(e)
if a.list:
    sys.exit(0)
if len(hits) <= a.nth:
    sys.exit(f"not found: {a.name or a.text}")
if a.rect:
    print(*hits[a.nth]["rect"]); sys.exit(0)
x, y, w, h = hits[a.nth]["shown"] if "shown" in hits[a.nth] else hits[a.nth]["rect"]
print(round(wx + x + w / 2), round(wy + y + h / 2))
