"""Diagnostic clocks and syscall counts; never acceptance timing."""
import fcntl,hashlib,json,os,statistics,subprocess
from pathlib import Path
root=Path('/home/callen/work/hotaru-page-owner-isolated')
output=root/'profile';output.mkdir(exist_ok=True)
diagnostic=json.loads((root/'diagnostic-build.json').read_text())
reference=json.loads((root/'measure-hoisted/report.json').read_text())
def sha(path):return hashlib.sha256(Path(path).read_bytes()).hexdigest()
assert sha(diagnostic['binary'])==diagnostic['binary_sha256']
labels=['sealed_blocks','mapped_bytes','map_ns','cell_copy_ns','protect_ns','seal_total_ns','decoded_blocks','raw_bytes','lz4_ns','link_dictionary_ns','row_restore_ns','decode_total_ns','seal_failures']
report={'schema':1,'diagnostic_build':diagnostic,'driver_sha256':sha(__file__),'notes':['Feature-gated Instant/atomic instrumentation adds overhead; these are diagnostic phase measurements, not speed acceptance.','Decode counters are reset before timed iteration and sampled before the post-read content digest.','strace timings are intentionally not used; only syscall counts and full content are checked.'],'phases':{},'syscalls':{}}
os.sched_setaffinity(0,{8})
with (Path.home()/'.cache/hotaru-workstation.lock').open('a')as lock:
 fcntl.flock(lock,fcntl.LOCK_EX)
 for name in ['ascii','unicode','unique-uri']:
  data=root/'measure-hoisted'/(name+'.vt')
  command=[diagnostic['binary'],str(data),'120','24','10000','50000000','1','none','0']
  samples=[]
  expected=reference['cases'][name]['groups']['candidate-c1-none']['samples'][0]['content_fnv1a64']
  for _ in range(5):
   result=subprocess.run(command,text=True,capture_output=True,check=True)
   state=json.loads(result.stdout);counters=json.loads(result.stderr)
   assert state['content_fnv1a64']==expected
   samples.append({'output':state,'feed':dict(zip(labels,counters['feed_diagnostics'])),'read':dict(zip(labels,counters['read_diagnostics']))})
  report['phases'][name]={'command':command,'input_sha256':sha(data),'samples':samples,'median':{stage:{label:statistics.median(sample[stage][label]for sample in samples)for label in labels}for stage in ['feed','read']}}
 for key,binary in [('baseline',reference['builds']['baseline']['binary']),('owner',str(root/'artifacts/owner-hoisted'))]:
  expected_build=reference['builds']['baseline'if key=='baseline'else'candidate'];assert sha(binary)==expected_build['binary_sha256']
  data=root/'measure-hoisted/ascii.vt'
  command=['strace','-qq','-c','-e','trace=mmap,mprotect,munmap','-o',str(output/(key+'.strace')),binary,str(data),'120','24','10000','50000000','1','none','0']
  result=json.loads(subprocess.check_output(command,text=True));assert result['content_fnv1a64']==reference['cases']['ascii']['groups']['candidate-c1-none']['samples'][0]['content_fnv1a64']
  report['syscalls'][key]={'command':command,'binary_sha256':sha(binary),'raw_summary':(output/(key+'.strace')).read_text(),'output':result}
(output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
for name,case in report['phases'].items():print(name,case['median'],flush=True)
for name,case in report['syscalls'].items():print(name,case['raw_summary'],flush=True)
