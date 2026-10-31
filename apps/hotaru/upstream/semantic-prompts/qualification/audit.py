#!/usr/bin/env python3
from pathlib import Path
import fcntl,hashlib,json,itertools,shutil,struct,subprocess
root=Path(__file__).resolve().parent;base=Path('/home/callen/work/hotaru-semantic-fastgate-20261028');osc=lambda x:b'\x1b]133;'+x+b'\x07'
def feed(x):return b'F'+struct.pack('<I',len(x))+x
def resize(c,r):return b'R'+struct.pack('<HH',c,r)
cases=[]
def add(name,data,c=5,r=5,ops=False):cases.append({'name':name,'cols':c,'rows':r,'operations':(data if ops else feed(data)).hex()})
for c in [1,2,5,13]:
 for action in [b'A',b'N',b'L']:
  for initial in [b'P',b'B',b'I']:
   add(f'pending-{c}-{action}-{initial}',osc(initial)+b'x'*c+osc(action)+b'Y',c)
for before,after in itertools.product([b'P',b'B',b'I',b'C'],repeat=2):
 for basechar,tail in [('☀','\ufe0f'),('😀','\ufe0e'),('e','\u0301'),('#','\ufe0f\u20e3')]:
  for position in [0,3,4]:
   data=b'x'*position+osc(before)+basechar.encode()+osc(after)+tail.encode()
   add(f'edge-{before}-{after}-{basechar}-{position}',data)
   add(f'edge-resize-{before}-{after}-{basechar}-{position}',feed(data)+resize(3,5)+resize(7,5),ops=True)
for sem in [b'P',b'B',b'I']:
 for edit in [b'\x1b[?2J',b'\x1b[?2K',b'\x1b[2@',b'\x1b[2P',b'\x1b[3X',b'\x1b#8',b'\x1b[3b']:
  for start in [b'\r',b'\x1b[2G',b'\x1b[4G']:
   add(f'edit-{sem}-{edit.hex()}-{start.hex()}',osc(sem)+b'\x1b[1"q'+('雪雪'.encode())+b'\x1b[0"q'+start+osc(b'C')+edit+b'Z')
for sem in [b'P',b'B',b'I']:
 for ending in [b'',b'\r\n',b'\x1b[2K',b'\x1b[2;1H']:
  data=(osc(sem)+b'abcd'+osc(b'C')+b'e\r\n')*100+osc(sem)+b'xy'+ending
  add(f'cold-{sem}-{ending.hex()}',feed(data)+b'C'+resize(3,5)+resize(8,6),ops=True)
for prompt in [b'P',b'P;k=c',b'P;k=r',b'P;k=s']:
 for body in [b'',b'a',b'abcdefghi',b'abcd\r\n',b'abc\r\x1b[2K']:
  add(f'row-reflow-{prompt}-{body}',feed(osc(prompt)+body)+resize(2,5)+resize(9,5),ops=True)
for label,transition in [('soft',b'\x1b[!p'),('alternate',b'\x1b[?1049hALT\x1b[?1049l'),('plain',b'')]:
 add('gate-B-'+label,feed(osc(b'B')+b'abc\r\nxyz'+transition)+resize(16,3)+feed(b'\x1b[2J'),8,3,True)
for reset in [False,True]:
 data=osc(b'P')+b'prompt'+osc(b'I')+b'input\r\noutput\x1b[!p\x1b[2J'
 if reset:data+=b'\x1bcplain\r\nordinary\x1b[2J'
 add('gate-ended-input-'+str(reset),data,12,3)
with Path('/home/callen/.cache/hotaru-workstation.lock').open('a') as lock:
 fcntl.flock(lock,fcntl.LOCK_EX)
 expected={'ghostty-semantic':'f78b17b7c9c9c7b06dc3c781596dda6574d83b15e7b04ca76d1b93cbf747aa4b','hotaru-semantic':'ed0b72771b35ba894d2a97954085c1bc71104d13a7b6092c4df962c5eb59c816'}
 for name,h in expected.items():
  src=base/'artifacts'/name;assert hashlib.sha256(src.read_bytes()).hexdigest()==h;shutil.copy2(src,root/name)
 files={str(p.relative_to(base/'apps/hotaru')):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted((base/'apps/hotaru/crates/hotaru-core').rglob('*')) if p.is_file()}
 (root/'source.json').write_text(json.dumps({'binaries':expected,'core_files':files},indent=2)+'\n')
 results=[];failures=[]
 for case in cases:
  path=root/'input.bin';path.write_bytes(bytes.fromhex(case['operations']))
  for mode,chunk in itertools.product(['legacy','unicode'],[1,7,65536]):
   args=[str(path),str(case['cols']),str(case['rows']),str(chunk),mode]
   output={name:json.loads(subprocess.check_output([str(root/name),*args])) for name in expected}
   equal=output['ghostty-semantic']==output['hotaru-semantic'];item={'name':case['name'],'mode':mode,'chunk':chunk,'equal':equal};results.append(item)
   if not equal:failures.append(dict(item,output=output))
 report={'status':'passed' if not failures else 'failed','cases':cases,'results':results,'failures':failures,'comparisons':len(results)}
 (root/'result.json').write_text(json.dumps(report,indent=2)+'\n')
 print(json.dumps({'status':report['status'],'comparisons':len(results),'failures':len(failures),'failed_cases':sorted(set(x['name'] for x in failures))}))
