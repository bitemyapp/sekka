from pathlib import Path
import json,hashlib,shutil,subprocess,tempfile
r=Path('/home/callen/work/hotaru-gpu-admission-isolated')
archive=r/'archive'
manifest=json.loads((archive/'source-manifest.json').read_text())
expected={x['path']:x['sha256'] for x in manifest['files']}
checks=[]
with tempfile.TemporaryDirectory(prefix='hotaru-admission-replay-') as td:
 t=Path(td)
 for mode,base,patch in [('worker',Path('/home/callen/work/hotaru-gpu-profile-isolated'),'worker.patch'),('native',Path('/home/callen/work/hotaru-gpu-preparation-profile'),'native.patch')]:
  dst=t/mode;dst.mkdir()
  names=set()
  for line in (archive/patch).read_text().splitlines():
   if line.startswith('+++ b/'):names.add(line[6:])
  for name in names:
   src=base/name
   if src.exists():
    out=dst/name;out.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(src,out)
  subprocess.run(['git','-C',str(dst),'apply',str(archive/patch)],check=True)
  for name in sorted(names):
   p=dst/name;actual=hashlib.sha256(p.read_bytes()).hexdigest();key=('worker/'if mode=='worker'else'')+name
   assert actual==expected[key],key
   checks.append({'path':key,'sha256':actual})
(archive/'patch-replay.json').write_text(json.dumps({'status':'passed','files':checks},indent=2)+'\n')
print(len(checks),'reconstructed files match exact source hashes')
