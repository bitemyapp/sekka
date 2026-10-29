#!/usr/bin/env python3
"""Actual libghostty-vt common style/cursor/mode state for DECALN."""
import fcntl,hashlib,json,os,subprocess,sys
from pathlib import Path
root=Path(__file__).resolve().parent
sys.path.insert(0,str(root.parent/'apps/hotaru/tools'))
from differential import Oracle,differences
binary=root.parent/'apps/hotaru/target/release/hotaru-replay'
library=Path('/home/callen/work/ghostty-reference-20261028/zig-out/lib/libghostty-vt.so')
cases=[
 ('plain',b'abc\x1b#8'),
 ('rgb-attrs',b'\x1b[1;3;4:3;7;9;38;2;1;2;3;48;2;4;5;6;58;2;7;8;9mabc\x1b#8Z'),
 ('palette-attrs',b'\x1b[1;2;3;5;8;38;5;231;48;5;19;58;5;42mabc\x1b#8\x1b[3b'),
 ('protected',b'\x1b[1"qABCDE\x1b#8X\x1b[?0K'),
 ('grapheme', '雪👨\u200d👩\u200d👧e\u0301\x1b#8Z'.encode()),
 ('margins-origin', b'\x1b[2;4r\x1b[?6h\x1b[?25l\x1b[3;4Hxxx\x1b#8\x1b[?6$p\x1b[T'),
 ('alt-return', b'main\x1b[?1049h\x1b[41;32malt\x1b#8\x1b[?1049l'),
 ('alt-pattern', b'main\x1b[?1049h\x1b[41;32malt\x1b#8'),
 ('not-exact', b'ab\x1b##8Z')]
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
with Path('/home/callen/.cache/hotaru-workstation.lock').open('a') as lock:
 fcntl.flock(lock,fcntl.LOCK_EX)
 oracle=Oracle(library); results=[]
 for name,raw in cases:
  path=root/'decaln-style-input.vt';path.write_bytes(raw)
  for mode in ['legacy','unicode']:
   for chunk in [1,7,65536]:
    expected=oracle.snapshot(raw,5,13,chunk,{2027:mode=='unicode'})
    actual=json.loads(subprocess.check_output([str(binary),'--data',str(path),'--rows','5','--cols','13','--chunk',str(chunk),'--grapheme-width',mode,'--format','json']))
    delta=differences(expected,actual)
    row={'case':name,'mode':mode,'chunk':chunk,'equal':not delta}
    if delta: row.update(differences=delta,expected=expected,actual=actual)
    results.append(row)
 report={'status':'passed' if all(r['equal'] for r in results) else 'failed','scope':'DECALN actual-library common cell/style/cursor/mode state; hyperlink ownership separately tested','library':str(library),'library_sha256':sha(library),'candidate':str(binary),'candidate_sha256':sha(binary),'runner_sha256':sha(Path(__file__)),'helper_sha256':sha(root.parent/'apps/hotaru/tools/differential.py'),'cases':[{'name':n,'hex':r.hex()} for n,r in cases],'comparisons':len(results),'results':results}
 (root/'decaln-style-reference.json').write_text(json.dumps(report,indent=2)+'\n')
 print(report['status'],len(results))
 if report['status']!='passed':sys.exit(1)

