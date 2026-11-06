"""Calibration text for an importance matrix: round 2's training-set Tasuke
conversations (data/sekka-tasuke-v2.jsonl, guarded by tasuke_sft.py to the
training set) rendered with Gemma 4's own chat template, then Tasuke's
skills as plain text. Nothing from the held-out scenarios.

    python imatrix_text.py MODEL_DIR DATA.jsonl SKILLS_DIR OUT.txt

then, with llama.cpp:

    llama-imatrix -m BF16.gguf -f OUT.txt -o sekka.imatrix.gguf -ngl 99 -c 512 -b 512
    llama-quantize --imatrix sekka.imatrix.gguf BF16.gguf OUT.gguf IQ3_XXS
"""
import json
import sys
from pathlib import Path

from transformers import AutoTokenizer

model, data, skills, out = sys.argv[1:5]
tok = AutoTokenizer.from_pretrained(model)
parts = []
for line in Path(data).read_text().splitlines():
    ex = json.loads(line)
    msgs = []
    for m in ex["messages"]:
        m = dict(m)
        for c in m.get("tool_calls") or []:
            f = c["function"]
            if isinstance(f.get("arguments"), str):
                try:
                    f["arguments"] = json.loads(f["arguments"])
                except ValueError:
                    pass
        msgs.append(m)
    parts.append(tok.apply_chat_template(msgs, tools=ex.get("tools"), tokenize=False))
for p in sorted(Path(skills).rglob("*.md")):
    parts.append(p.read_text())
text = "\n\n".join(parts)
Path(out).write_text(text)
print(f"{len(parts)} parts, {len(text)} chars, {len(tok(text).input_ids)} tokens")
