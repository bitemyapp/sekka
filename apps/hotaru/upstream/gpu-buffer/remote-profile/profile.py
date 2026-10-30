#!/usr/bin/env python3
import os,sys,json,subprocess,time,hashlib,pathlib,signal
root=pathlib.Path('/home/callen/work/hotaru-gpu-profile-isolated')
loader='/nix/store/pqyiidz5mi73wv5f91d48s9plcqyj5bz-glibc-2.44-57/lib/ld-linux-x86-64.so.2'
libc=str(pathlib.Path(loader).parent)
glvnd='/nix/store/ww5pckflswny9whz04k06xh233jbg5kl-libglvnd-1.7.0/lib'
drm='/nix/store/f5bymphw4ci3j34w3iw63v32pqpkxkjf-libdrm-2.4.134/lib'
gcc='/nix/store/3sj8msrsym648f35mk3gb3pp385hhk8r-gcc-16.2.0-libgcc/lib'
old=pathlib.Path('/home/callen/work/hotaru-gpu-buffer-20261028/check-5')
bin=pathlib.Path('/home/callen/.cache/hotaru-gpu-transport-target/release/scene-phase-probe')
name,fixture,repeats,policy=sys.argv[1:]
out=root/name;out.mkdir()
(out/'home').mkdir();(out/'cache').mkdir()
env={'HOME':str(out/'home'),'XDG_CACHE_HOME':str(out/'cache'),'__EGL_VENDOR_LIBRARY_FILENAMES':str(old/'nvidia.json'),'__EGL_EXTERNAL_PLATFORM_CONFIG_DIRS':str(old/'external-platforms')}
command=[loader,'--library-path',':'.join([libc,glvnd,drm,gcc,str(old/'driver-libraries')]),str(bin),glvnd+'/libEGL.so.1',fixture,repeats,policy]
report={'scope':'remote offscreen actual native-scene compute correctness and diagnostic phases; no presentation or controlled CPU/GPU benchmark','command':command,'environment':env,'binary_sha256':hashlib.sha256(bin.read_bytes()).hexdigest(),'runner_sha256':hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),'source':{str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [root/'Cargo.toml',root/'Cargo.lock',*sorted((root/'src').glob('*'))] if p.is_file()},'load_start':os.getloadavg(),'export_policy':policy}
start=time.monotonic();maps=set();timed_out=False
with (out/'probe.log').open('w') as log:
 p=subprocess.Popen(command,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
 while p.poll() is None:
  try:
   for line in pathlib.Path(f'/proc/{p.pid}/maps').read_text().splitlines():
    parts=line.split(None,5)
    if len(parts)==6 and parts[5].startswith('/'):maps.add(parts[5])
  except FileNotFoundError:pass
  if time.monotonic()-start>120:
   timed_out=True;os.killpg(p.pid,signal.SIGKILL);break
  time.sleep(.01)
 report['exit']=p.wait()
events=[]
for line in (out/'probe.log').read_text().splitlines():
 try:events.append(json.loads(line))
 except json.JSONDecodeError:pass
verify=[e for e in events if e.get('event')=='scene_verified']; expected=(18 if pathlib.Path(fixture).is_dir() else 1)*int(repeats)
report.update(events=events,mapped_files=sorted(maps),mapped_file_sha256={s:hashlib.sha256(pathlib.Path(s).read_bytes()).hexdigest() for s in maps if pathlib.Path(s).is_file() and not s.startswith('/dev/')},elapsed_s=time.monotonic()-start,load_end=os.getloadavg(),timed_out=timed_out,expected_renders=expected)
cleanup_counts={'resource_destroyed':expected,'fence_destroyed':expected,'scene_gpu_cache_destroyed':1,'context_destroyed':1}
report['cleanup_verified']=all(sum(e.get('event')==kind and e.get('ok') is True for e in events)==count and not any(e.get('event')==kind and e.get('ok') is not True for e in events) for kind,count in cleanup_counts.items())
report['process_reaped']=p.returncode is not None
report['status']='passed' if report['exit']==0 and len(verify)==expected and all(e['different_bytes']==0 for e in verify) and any(e.get('event')=='profile_complete' for e in events) and report['cleanup_verified'] else 'failed'
(out/'result.json').write_text(json.dumps(report,indent=2)+'\n'); print(json.dumps({k:report[k] for k in ['status','exit','expected_renders','elapsed_s']}));sys.exit(0 if report['status']=='passed' else 1)
