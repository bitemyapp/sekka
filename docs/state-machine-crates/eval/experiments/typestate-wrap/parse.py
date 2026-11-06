# python3 -I parse.py VARIANT...: per-unit counts from $LOGS/VARIANT.log (default logs/ here).
import os, re, sys
from pathlib import Path
V = Path(os.environ.get("LOGS") or Path(__file__).resolve().parent / "logs")
units = {"wrap_80": 201227, "wrap_200": 208057, "replay_wrap": None}
over = {"default": {"wrap_80": 190, "wrap_200": 190, "replay_wrap": 984}, "lto": {"wrap_80": 173, "wrap_200": 173, "replay_wrap": 850}}
def parse(variant):
    text = (V / f"{variant}.log").read_text()
    res = {}
    for sec in re.split(r"^=== ", text, flags=re.M):
        m = re.match(r"\S+: instructions \((\w+)\)", sec)
        if not m: continue
        prof = m.group(1)
        for b in re.finditer(r"instructions::machines::(\w+) (\w+):\(.*?\)\n((?:  .*\n)+)", sec):
            bench, impl, body = b.groups()
            ir = int(re.search(r"Instructions:\s+(\d+)", body).group(1))
            rw = int(re.search(r"Total read\+write:\s+(\d+)", body).group(1))
            res[(prof, bench, impl)] = (ir, rw)
    return res
for variant in sys.argv[1:]:
    r = parse(variant)
    for prof in ["default", "lto"]:
        for bench in ["wrap_80", "wrap_200", "replay_wrap"]:
            row = []
            for impl in ["implicit", "enum_", "typestate"]:
                if (prof, bench, impl) not in r: row.append("-"); continue
                ir, rw = r[(prof, bench, impl)]
                n = units[bench] or 1
                row.append(f"{impl}={(ir-over[prof][bench])/n:.1f}" if units[bench] else f"{impl}={ir-over[prof][bench]}")
            e = r.get((prof, bench, "enum_")); t = r.get((prof, bench, "typestate"))
            ratio = (t[0]-over[prof][bench])/(e[0]-over[prof][bench]) if e and t else 0
            mem = f"rw/unit ts={(t[1])/(units[bench] or 1):.1f} enum={(e[1])/(units[bench] or 1):.1f}" if e and t else ""
            print(f"{variant:8} {prof:7} {bench:11} " + " ".join(row) + f"  ts/enum={ratio:.2f}  {mem}")
