#!/usr/bin/env python3
"""Reconstruct verified generated inputs outside Git: generate-inputs.py DEST."""
from pathlib import Path
import hashlib,json,sys
root=Path(__file__).resolve().parent.parent
out=Path(sys.argv[1]);out.mkdir(parents=True,exist_ok=True)
for c in json.loads((root/'input-manifest.json').read_text())['cases']:
 pattern=bytes.fromhex(c['pattern_hex'])
 assert c['bytes']%len(pattern)==0
 raw=pattern*(c['bytes']//len(pattern))
 assert hashlib.sha256(raw).hexdigest()==c['sha256']
 (out/(c['name']+'.vt')).write_bytes(raw)
 print(c['name'],len(raw),c['sha256'])
