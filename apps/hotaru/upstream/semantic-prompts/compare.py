#!/usr/bin/env python3
"""Independent semantic-cell observations from unchanged libghostty-vt."""
import fcntl
import hashlib
import itertools
import json
from pathlib import Path
import struct
import subprocess

root = Path(__file__).resolve().parent
osc = lambda body: b'\x1b]133;' + body + b'\x07'
def feed(data):
    return b'F' + struct.pack('<I', len(data)) + data
def resize(cols, rows):
    return b'R' + struct.pack('<HH', cols, rows)
cases = []
def add(name, data, cols=13, rows=5, operations=False):
    cases.append({'name':name, 'cols':cols, 'rows':rows, 'operations':(data if operations else feed(data)).hex()})
add('basic', b'out'+osc(b'A')+b'prompt>'+osc(b'B')+b'input'+osc(b'C')+b'out'+osc(b'D')+b'end')
for action in [b'A',b'B',b'C',b'D',b'I',b'L',b'N',b'P',b'',b'X',b'AB',b'A;',b'A;k=s',b'A;k=bad;k=i',b'A;unknown=\xff',b'L;',b'L;x=1',b'P;k=r',b'P;k=c',b'P;k=i',b'N;aid=a']:
    for initial in [b'',osc(b'P'),osc(b'B'),osc(b'I')]:
        add(f'action-{action.hex()}-after-{initial.hex()}', initial+b'abc'+osc(action)+b'XYZ')
for kind in [b'P',b'B',b'I']:
    for cols in [1,2,5,13]:
        for body in [b'abcdefghijklmnop',b'abc\r\nDEF',b'abc\x1bDDEF',b'abc\x1bEDEF',b'abc\x1bMDEF', 'ab雪e\u0301☀\ufe0fZ'.encode()]:
            add(f'content-{kind.decode()}-{cols}-{body.hex()}',osc(kind)+body,cols=cols)
    for edit in [b'\r\x1b[3@',b'\r\x1b[2P',b'\r\x1b[3X',b'\r\x1b[2K',b'\x1b[2J',b'\x1b[2;4H\x1b[2L',b'\x1b[2;4H\x1b[2M',b'\x1b[2S',b'\x1b[T',b'\x1b[3b',b'\x1b[!p',b'\x1bc']:
        add(f'edit-{kind.decode()}-{edit.hex()}',osc(kind)+b'abcdefghij\r\nKLMNOP'+edit+b'Z')
    for mode in [47,1047,1049]:
        add(f'alt-{kind.decode()}-{mode}',osc(kind)+b'abc'+f'\x1b[?{mode}h'.encode()+b'ALT'+osc(b'C')+b'O'+f'\x1b[?{mode}l'.encode()+b'back')
    add(f'saved-{kind.decode()}',osc(kind)+b'ab\x1b7'+osc(b'C')+b'out\x1b8Z')
    add(f'cold-{kind.decode()}',feed((osc(kind)+b'abcde\r\n')*80)+b'C',operations=True)
for cols, rows in [(5,5),(20,5),(1,5),(13,3),(13,8)]:
    add(f'resize-{cols}-{rows}',feed(osc(b'A;redraw=0')+b'PROMPT>'+osc(b'B')+b'abcd'+osc(b'C')+b'OUTPUT\r\nrest')+resize(cols,rows),operations=True)
for before, after in itertools.product([b'P',b'B',b'C'],repeat=2):
    for text in ['e\u0301','☀\ufe0f','😀\ufe0e','🇺🇸']:
        chars=list(text)
        add(f'grapheme-{before.decode()}-{after.decode()}-{text}',osc(before)+chars[0].encode()+osc(after)+''.join(chars[1:]).encode()+b'Z',cols=5)

for body in [b'P',b'P;k=r',b'P;k=c',b'P;k=s',b'P;k=x;k=s',b'A;k=c',b'B',b'I']:
    add(f'empty-{body.hex()}',osc(body))
    add(f'empty-reflow-{body.hex()}',feed(osc(body))+resize(7,6),operations=True)
    for suffix in [b'\\x1b[2K',b'\\x1b[2J',b'\\x1b[H\\x1b[2J',b'\\x1b[3;4H\\x1b[2J',b'\\x1b[2;4r\\x1b[2J']:
        suffix=suffix.decode('unicode_escape').encode('latin1')
        add(f'prompt-clear-{body.hex()}-{suffix.hex()}',osc(body)+b'abc'+suffix+b'Z')

with Path('/home/callen/.cache/hotaru-workstation.lock').open('a') as lock:
    fcntl.flock(lock,fcntl.LOCK_EX)
    identities={name:hashlib.sha256((root/name).read_bytes()).hexdigest() for name in ['ghostty-semantic','hotaru-semantic','oracle.c','compare.py']}
    results=[]; failures=[]; fixtures=[]
    for case in cases:
        path=root/'input.bin';path.write_bytes(bytes.fromhex(case['operations']))
        for mode in ['legacy','unicode']:
            for chunk in [1,7,65536]:
                common=[str(path),str(case['cols']),str(case['rows']),str(chunk),mode]
                actual={name:json.loads(subprocess.check_output([str(root/name),*common])) for name in ['ghostty-semantic','hotaru-semantic']}
                if chunk == 65536: fixtures.append({**case,'mode':mode,'expected':actual['ghostty-semantic']})
                equal=actual['ghostty-semantic']==actual['hotaru-semantic']
                record={'case':case['name'],'mode':mode,'chunk':chunk,'equal':equal}
                if not equal:record.update(actual);failures.append(record)
                results.append(record)
        if len(failures)>100:break
    report={'status':'passed' if not failures else 'failed','identities':identities,'cases':cases,'comparisons':len(results),'matches':sum(r['equal'] for r in results),'results':results}
    (root/'comparison.json').write_text(json.dumps(report,indent=2)+'\n')
    if not failures:
        (root.parent/'apps/hotaru/upstream/semantic-fixtures.json').write_text(json.dumps({'ghostty_commit':'6ba144158577d033ccb6ed4b574e4013957c9161','identities':identities,'cases':fixtures},separators=(',',':'))+'\n')
    print(json.dumps({'status':report['status'],'comparisons':len(results),'failures':len(failures),'failed_cases':sorted(set(r['case'] for r in failures))}))
