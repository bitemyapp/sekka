import argparse,fcntl,hashlib,importlib.util,json,os,platform,statistics,subprocess,time
from pathlib import Path

def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
parser=argparse.ArgumentParser()
parser.add_argument('--root',type=Path,required=True)
parser.add_argument('--memory-tools',type=Path,required=True)
parser.add_argument('--output',type=Path,required=True)
parser.add_argument('--samples',type=int,default=5)
args=parser.parse_args()
args.output.mkdir(parents=True,exist_ok=True)
spec=importlib.util.spec_from_file_location('corpora',args.memory_tools/'history-budget-benchmark.py')
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
builds={key:json.loads((args.root/(key+'-build.json')).read_text())for key in ['baseline','candidate']}
for build in builds.values():assert sha(build['binary'])==build['binary_sha256']
os.sched_setaffinity(0,{8})
report={'schema':1,'host':platform.node(),'cpu':8,'date':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'builds':builds,'driver_sha256':sha(__file__),'samples':args.samples,'cases':{},'notes':['Exact cb91125 source except direct recycled-row construction; fresh process per sample, input freed before RSS, full normalized content digest checked.', 'Alternating baseline/candidate order. Shared cooperative lock; no host settings or allocator trimming.', 'Both builds keep existing Vec-backed cells and the system allocator; all capacities and Row layout must remain identical.']}
cases=[('ascii','ascii',8000,0,10000,True),('styled','styled',8000,0,10000,True),('unicode','unicode',8000,0,10000,True),('unique-uri','uri',3000,600,10000,True),('byte-limit-uri','uri',14000,1000,10000,False),('parse-ascii','ascii',80000,0,0,True),('parse-unicode','unicode',80000,0,0,True)]
for name,kind,count,uri_bytes,row_limit,equal in cases:
 data=args.output/(name+'.vt');module.corpus(data,kind,count,uri_bytes)
 groups={}
 with (Path.home()/'.cache/hotaru-workstation.lock').open('a')as lock:
  fcntl.flock(lock,fcntl.LOCK_EX)
  before={'load':os.getloadavg(),'processes':subprocess.check_output(['ps','-eo','comm,pcpu','--sort=-pcpu'],text=True).splitlines()[:12]}
  configs=[(key,compress,'none')for key in builds for compress in ([0]if row_limit==0 else [0,1])]
  if name=='ascii':configs += [(key,1,hold)for key in builds for hold in ['resident','cold']]
  for iteration in range(args.samples):
   for key,compress,hold in (configs if iteration%2==0 else configs[::-1]):
    command=[builds[key]['binary'],str(data),'120','24',str(row_limit),'50000000',str(compress),hold,str(int(hold!='none'))]
    output=json.loads(subprocess.check_output(command,text=True,timeout=120))
    group=groups.setdefault(f'{key}-c{compress}-{hold}',{'command':command,'samples':[]});group['samples'].append(output)
 hashes={(x['history_rows'],x['content_fnv1a64'])for g in groups.values()for x in g['samples']}
 if equal:assert len(hashes)==1,(name,hashes)
 for group in groups.values():
  numeric=[key for key,value in group['samples'][0].items()if isinstance(value,(int,float))and not isinstance(value,bool)]
  group['median']={key:statistics.median(x[key]for x in group['samples'])for key in numeric}
 report['cases'][name]={'groups':groups,'data_sha256':sha(data),'input_bytes':data.stat().st_size,'equal_contents':len(hashes)==1,'host_before':before}
 (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
 print(name,{key:{x:g['median'][x]for x in ['rss_cold_kib','feed_ns','compression_ns','read_history_ns','history_rows']}for key,g in groups.items()},flush=True)
for build in builds.values():assert sha(build['binary'])==build['binary_sha256']
