#!/usr/bin/env python3
"""Attribute same-pin reference instructions using a Valgrind-compatible ISA."""
from pathlib import Path
import datetime,fcntl,hashlib,json,re,subprocess
from differential import Oracle
ROOT=Path(__file__).resolve().parent.parent
OUT=ROOT/'evidence/reference-v3'
INPUT=Path('/home/callen/work/hotaru-short-utf8-complete-tail-20261028/evidence/profiles')
NATIVE=Path('/home/callen/work/ghostty-reference-20261028/zig-out/lib/libghostty-vt.so')
V=Path('/nix/store/4298593522xgrr28874w20cvvs742r51-valgrind-3.27.1/bin/valgrind')
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
OUT.mkdir(exist_ok=True)
build=json.loads((ROOT/'evidence/reference-diagnostic-build.json').read_text())
report={'schema':1,'scope':'Callgrind attribution only, whole process including setup/teardown; same pinned source with explicit x86-64-v3/ReleaseFast/debug. Not the native timed reference.','recorded_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'status':'preparing','build':build,'runner_sha256':sha(Path(__file__)),'runs':[]}
try:
 with (Path.home()/'.cache/hotaru-workstation.lock').open('a') as lock:
  fcntl.flock(lock,fcntl.LOCK_EX)
  binary=Path(build['binary']);library=Path(build['library'])
  assert sha(binary)==build['binary_sha256'] and sha(library)==build['library_sha256']
  assert sha(NATIVE)==build['native_library_sha256_unchanged']
  oracle=Oracle(library);native=Oracle(NATIVE)
  for mode in ('legacy','unicode'):
   for label in ('short-invalid-suffix','short-interior-error'):
    data=INPUT/f'{label}.vt';raw=data.read_bytes()
    state=oracle.snapshot(raw,12,120,65536,{2027:mode=='unicode'})
    assert state==native.snapshot(raw,12,120,65536,{2027:mode=='unicode'})
    prefix=OUT/f'ghostty-{mode}-{label}'
    command=[str(V),'--tool=callgrind',f'--callgrind-out-file={prefix}.callgrind',str(binary),'--data',str(data),'--rows','12','--cols','120','--chunk','65536','--grapheme-width',mode,'--bench-json','--iterations','1']
    with Path(str(prefix)+'.json').open('w') as output,Path(str(prefix)+'.log').open('w') as log:
     subprocess.run(command,stdout=output,stderr=log,check=True)
    value=json.loads(Path(str(prefix)+'.json').read_text());assert value['response_bytes']==[len(state['responses'])]
    for inclusive in ('no','yes'):
     with Path(str(prefix)+f'.inclusive-{inclusive}.txt').open('w') as output:
      subprocess.run([str(V.with_name('callgrind_annotate')),'--auto=no',f'--inclusive={inclusive}','--threshold=99',str(prefix)+'.callgrind'],stdout=output,stderr=subprocess.STDOUT,check=True)
    report['runs'].append({'mode':mode,'input':label,'input_bytes':len(raw),'input_sha256':sha(data),'native_reference_state_equal':True,'command':command,'profile_sha256':sha(Path(str(prefix)+'.callgrind')),'instructions':int(re.search(r'Collected : ([0-9]+)',Path(str(prefix)+'.log').read_text()).group(1))})
  assert sha(binary)==build['binary_sha256'] and sha(library)==build['library_sha256']
  assert sha(NATIVE)==build['native_library_sha256_unchanged']
  report['status']='completed'
except BaseException as e:
 report['status']='failed';report['error']=repr(e);raise
finally:
 (OUT/'summary.json').write_text(json.dumps(report,indent=2)+'\n')
 print(json.dumps({'status':report['status'],'runs':[(r['mode'],r['input'],r['instructions']) for r in report['runs']]}))
