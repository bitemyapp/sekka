"""Paired immutable decoder comparison, with a heap-refactor causal control."""
import fcntl,hashlib,json,os,platform,statistics,subprocess,time
from pathlib import Path
root=Path('/home/callen/work/hotaru-page-decode-isolated')
owner=Path('/home/callen/work/hotaru-page-owner-isolated')
report_owner=json.loads((owner/'measure-hoisted/report.json').read_text())
builds={'owner':dict(report_owner['builds']['candidate'])}
builds['owner']['binary']=str(owner/'artifacts/owner-hoisted')
for kind in ['heap','direct']:builds[kind]=json.loads((root/(kind+'-build.json')).read_text())
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
for build in builds.values():assert sha(build['binary'])==build['binary_sha256']
result={'schema':1,'host':platform.node(),'date':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'cpu':8,'builds':builds,'driver_sha256':sha(__file__),'samples':5,'notes':['Frozen prior uniform-owner binary versus identical refactored decoder source selecting heap or direct page cells.','Fresh processes; CPU8; exclusive cooperative lock; alternating engine order; no allocator trim or host setting changes.','Heap feature isolates metadata construction cleanup from mapping effects. Archive capacities and complete content/retention must match all engines.'],'cases':{}}
output=root/'measure';output.mkdir(exist_ok=True)
os.sched_setaffinity(0,{8})
cases=[(x,10000,1,'none')for x in ['ascii','styled','unicode','unique-uri','byte-limit-uri']]+[('ascii',10000,0,'none'),('ascii',10000,1,'resident'),('ascii',10000,1,'cold'),('parse-ascii',0,0,'none'),('parse-unicode',0,0,'none')]
for name,limit,compress,hold in cases:
 key=f'{name}-c{compress}-{hold}'; data=owner/'measure-hoisted'/(name+'.vt'); groups={}
 with (Path.home()/'.cache/hotaru-workstation.lock').open('a')as lock:
  fcntl.flock(lock,fcntl.LOCK_EX)
  before={'load':os.getloadavg(),'processes':subprocess.check_output(['ps','-eo','comm,pcpu','--sort=-pcpu'],text=True).splitlines()[:12]}
  for iteration in range(5):
   order=list(builds)if iteration%2==0 else list(builds)[::-1]
   for engine in order:
    command=[builds[engine]['binary'],str(data),'120','24',str(limit),'50000000',str(compress),hold,str(int(hold!='none'))]
    value=json.loads(subprocess.check_output(command,text=True,timeout=120))
    groups.setdefault(engine,{'command':command,'samples':[]})['samples'].append(value)
 hashes={(v['history_rows'],v['content_fnv1a64'])for g in groups.values()for v in g['samples']};assert len(hashes)==1,(key,hashes)
 for group in groups.values():
  numeric=[k for k,v in group['samples'][0].items()if isinstance(v,(int,float))and not isinstance(v,bool)]
  group['median']={k:statistics.median(v[k]for v in group['samples'])for k in numeric}
 result['cases'][key]={'input_sha256':sha(data),'input_bytes':data.stat().st_size,'host_before':before,'equal_content':True,'groups':groups}
 (output/'report.json').write_text(json.dumps(result,indent=2)+'\n')
 print(key,{k:{f:v['median'][f]for f in ['rss_cold_kib','rss_read_kib','feed_ns','read_history_ns']}for k,v in groups.items()},flush=True)
for build in builds.values():assert sha(build['binary'])==build['binary_sha256']
