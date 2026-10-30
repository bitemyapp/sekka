#!/usr/bin/env python3
"""Attribute malformed workload instructions, never native execution time."""
from pathlib import Path
import datetime, fcntl, hashlib, json, re, subprocess
from differential import Oracle, differences
ROOT=Path(__file__).resolve().parent.parent
OUT=ROOT/'evidence/profiles'
REFERENCE=Path('/home/callen/work/hotaru-unicode-mode-20261028')
LIBRARY=Path('/home/callen/work/ghostty-reference-20261028/zig-out/lib/libghostty-vt.so')
SOURCE=Path('/home/callen/work/hotaru-short-utf8-complete-tail-20261028')
VALGRIND=Path('/nix/store/4298593522xgrr28874w20cvvs742r51-valgrind-3.27.1/bin/valgrind')
ANNOTATE=VALGRIND.with_name('callgrind_annotate')
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def source_check(m):
 for name,h in m['sources'].items():assert sha(ROOT/name)==h,name
OUT.mkdir(exist_ok=True)
report={'schema':1,'status':'preparing','scope':'Callgrind instruction attribution including startup/setup/teardown. Rust diagnostic uses x86-64-v3/debug2; pinned Ghostty remains its existing ReleaseFast/native library. No elapsed-time, native ratio, or cross-ISA instruction-parity claim.','recorded_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'runs':[],'valgrind_sha256':sha(VALGRIND),'valgrind_version':subprocess.check_output([str(VALGRIND),'--version'],text=True).strip(),'runner_sha256':sha(Path(__file__))}
try:
 with (Path.home()/'.cache/hotaru-workstation.lock').open('a') as lock:
  fcntl.flock(lock,fcntl.LOCK_EX)
  build=json.loads((ROOT/'evidence/diagnostic-build.json').read_text());source_check(build)
  report['hotaru_build']=build
  report['reference_build']=json.loads((REFERENCE/'reference-build.json').read_text())
  binaries={'hotaru':ROOT/'target/release/hotaru-replay','ghostty':REFERENCE/'ghostty-reference-replay'}
  expected_hashes={'hotaru':build['binary_sha256'],'ghostty':report['reference_build']['binaries']['ghostty-reference-replay']['sha256']}
  assert report['reference_build']['revision']=='6ba144158577d033ccb6ed4b574e4013957c9161'
  assert sha(LIBRARY)==report['reference_build']['binaries']['libghostty-vt']['sha256']=='70e1bdcf62801b735bb268c38c1e65fa727debff73d89ef1d3ed9fb5e23e7639'
  for engine,binary in binaries.items():assert sha(binary)==expected_hashes[engine]
  prior=json.loads((SOURCE/'direct-expanded/compact.json').read_text())
  oracle=Oracle(LIBRARY)
  for mode in ('legacy','unicode'):
   for label in ('short-invalid-suffix','short-interior-error'):
    data=SOURCE/'evidence/profiles'/f'{label}.vt';raw=data.read_bytes()
    old=next(r for r in prior['cases'] if r['mode']==mode and r['input']==label)
    assert sha(data)==old['input_sha256']
    args=['--data',str(data),'--rows','12','--cols','120','--chunk','65536','--grapheme-width',mode]
    expected=oracle.snapshot(raw,12,120,65536,{2027:mode=='unicode'})
    actual=json.loads(subprocess.check_output([str(binaries['hotaru']),*args,'--format','json'],text=True))
    assert not differences(expected,actual)
    for engine,binary in binaries.items():
     prefix=OUT/f'{engine}-{mode}-{label}'
     command=[str(VALGRIND),'--tool=callgrind',f'--callgrind-out-file={prefix}.callgrind',str(binary),*args,'--bench-json','--iterations','1']
     with Path(str(prefix)+'.json').open('w') as out,Path(str(prefix)+'.log').open('w') as err:
      status=subprocess.run(command,stdout=out,stderr=err).returncode
     run={'engine':engine,'mode':mode,'input':label,'input_sha256':sha(data),'input_bytes':len(raw),'command':command,'exit_code':status,'state_gate_passed':True,'profile_sha256':sha(Path(str(prefix)+'.callgrind')) if Path(str(prefix)+'.callgrind').exists() else None}
     if status==0:
      value=json.loads(Path(str(prefix)+'.json').read_text())
      for key,want in [('engine',engine),('rows',12),('cols',120),('scrollback',0),('chunk_bytes',65536),('grapheme_width_method',mode)]:assert value[key]==want
      assert value['response_bytes']==[len(expected['responses'])]
      run['instructions']=int(re.search(r'Collected : ([0-9]+)',Path(str(prefix)+'.log').read_text()).group(1))
      for inclusive in ('no','yes'):
       with Path(str(prefix)+f'.inclusive-{inclusive}.txt').open('w') as output:
        subprocess.run([str(ANNOTATE),'--auto=no',f'--inclusive={inclusive}','--threshold=99',str(prefix)+'.callgrind'],stdout=output,stderr=subprocess.STDOUT,check=True)
     else:run['failure_log_tail']=Path(str(prefix)+'.log').read_text()[-3000:]
     report['runs'].append(run)
     (OUT/'summary.json').write_text(json.dumps(report,indent=2)+'\n')
  source_check(build)
  for engine,binary in binaries.items():assert sha(binary)==expected_hashes[engine]
  report['status']='completed' if all(r['exit_code']==0 for r in report['runs']) else 'partial_with_preserved_failures'
except BaseException as e:
 report['status']='failed';report['error']=repr(e);raise
finally:
 (OUT/'summary.json').write_text(json.dumps(report,indent=2)+'\n')
 print(json.dumps({'status':report['status'],'runs':[(r['engine'],r['mode'],r['input'],r['exit_code'],r.get('instructions')) for r in report['runs']]}))
