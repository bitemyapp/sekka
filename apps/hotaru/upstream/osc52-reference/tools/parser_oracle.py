#!/usr/bin/env python3
"""Direct unchanged-C-API OSC parser cases; full terminal C0 handling is separate."""
import ctypes as C
import fcntl
import hashlib
import json
from pathlib import Path

root=Path(__file__).resolve().parents[1]
library=Path('/home/callen/work/ghostty-reference-20261028/zig-out/lib/libghostty-vt.so.0.1.0')
adapter=root/'artifacts/libparser-oracle.so'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
lock=Path('/home/callen/.cache/hotaru-workstation.lock').open('a')
fcntl.flock(lock,fcntl.LOCK_EX)
api=C.CDLL(str(adapter));api.osc52_parser_type.argtypes=[C.c_char_p,C.c_size_t,C.c_uint8];api.osc52_parser_type.restype=C.c_int
cases=[]
def add(name,data,term=0x07):
    cases.append({'name':name,'input_length':len(data),'input_sha256':hashlib.sha256(data).hexdigest(),
                  **({'input_hex':data.hex()} if len(data)<2048 else {}),'terminator':term,
                  'actual_command_type':api.osc52_parser_type(data,len(data),term)})
for prefix in [b'52',b'052',b'0052',b'00000000000052',b'+52',b' 52',b'52 ',b'52\x00',b'52\x07',b'52\x1b']:
    add('numeric-'+prefix.hex(),prefix+b';c;?')
for kind in [b'',b'c',b'p',b's',b'x',b'0',b'cp',b'cc',b'\xff',b'\x00',b'\xc3\xa9']:
    for payload in [b'',b'?',b'??',b'YQ==',b'a\x00b',b'\xff',b'a;b']:
        add('kind-payload-'+kind.hex()+'-'+payload.hex(),b'52;'+kind+b';'+payload)
for control in [0,7,0x18,0x1a,0x1b,0x7f]:
    add('embedded-control-'+str(control),b'52;c;'+bytes([control])+b'?')
    add('terminator-'+str(control),b'52;c;?',control)
for kind in [b'',b'c']:
    for offset in [-4,-3,-2,-1,0,1]:
        n=8*1024*1024+offset
        add(f'capture-{kind.hex()}-{n}',b'52;'+kind+b';'+b'Y'*n)
report={'scope':'actual C API Parser.next/end; no stream C0 filter, GTK dispatch or base64 decoder',
        'library':str(library),'library_sha256':sha(library),'adapter_sha256':sha(adapter),
        'tool_sha256':sha(Path(__file__)),'source_sha256':sha(root/'tools/parser_oracle.c'),
        'clipboard_command_type':4,'cases':cases}
(root/'artifacts/parser-results.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({'cases':len(cases),'clipboard_commands':sum(r['actual_command_type']==4 for r in cases)}))
