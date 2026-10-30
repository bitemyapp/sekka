#!/usr/bin/env python3
"""Bounded Intel/Mesa offscreen diagnostic; never contacts a compositor."""
import fcntl, hashlib, json, os, pathlib, signal, stat, subprocess, time
base=pathlib.Path(__file__).resolve().parent
root=base.parent
mesa='/nix/store/457mqa9dsvw9cnmi6pr06zjy1ib0iic4-mesa-26.2.5'
loader='/nix/store/ww5pckflswny9whz04k06xh233jbg5kl-libglvnd-1.7.0'
dev='/nix/store/risg12y850mmilsg2ikrx417mg1dj6dm-libglvnd-1.7.0-dev'
source=base/'gpu-buffer-probe.c'
binary=base/'gpu-buffer-probe'
env=dict(os.environ)
for key in list(env):
 if key.startswith(('MESA_', 'EGL_', 'GL_', '__EGL_', 'LIBGL_', 'GALLIUM_', 'DRI_')) or key in ('DISPLAY','WAYLAND_DISPLAY','SWAYSOCK','LD_LIBRARY_PATH'):
  env.pop(key,None)
env.update(__EGL_VENDOR_LIBRARY_FILENAMES=mesa+'/share/glvnd/egl_vendor.d/50_mesa.json',LIBGL_DRIVERS_PATH=mesa+'/lib/dri',MESA_SHADER_CACHE_DISABLE='true')
result={'scope':'hardware offscreen two-unshared-context DMA-BUF correctness; no Wayland, presentation, renderer integration or timing claim','source_sha256':hashlib.sha256(source.read_bytes()).hexdigest(),'host':os.uname().nodename,'kernel':os.uname().release,'mesa':mesa,'loader':loader,'device':{'path':'/dev/dri/renderD128','mode':oct(stat.S_IMODE(os.stat('/dev/dri/renderD128').st_mode)),'vendor':pathlib.Path('/sys/class/drm/renderD128/device/vendor').read_text().strip(),'device':pathlib.Path('/sys/class/drm/renderD128/device/device').read_text().strip(),'driver':str(pathlib.Path('/sys/class/drm/renderD128/device/driver').resolve())},'environment':{k:env[k] for k in ['__EGL_VENDOR_LIBRARY_FILENAMES','LIBGL_DRIVERS_PATH','MESA_SHADER_CACHE_DISABLE']}}
with open('/home/callen/.cache/hotaru-local-benchmark.lock','a') as lock:
 fcntl.flock(lock,fcntl.LOCK_EX)
 command=['cc','-std=c11','-O2','-Wall','-Wextra','-Werror','-I'+dev+'/include',str(source),'-L'+loader+'/lib','-Wl,-rpath,'+loader+'/lib','-lEGL','-o',str(binary)]
 result['build_command']=command
 with (base/'build.log').open('w') as log:
  built=subprocess.run(command,env=env,stdout=log,stderr=subprocess.STDOUT,timeout=60)
 result['build_returncode']=built.returncode
 if built.returncode:raise RuntimeError('probe build failed')
 result['binary_sha256']=hashlib.sha256(binary.read_bytes()).hexdigest()
 command=['/nix/store/78jn0lxqjgz4ahjzvvh4rvan7p607xph-strace-7.2/bin/strace','-f','-yy','-e','trace=openat,ioctl,close','-o',str(base/'probe.strace'),str(binary),'0']
 result['command']=command
 with (base/'probe.jsonl').open('w') as out,(base/'probe.stderr').open('w') as err:
  child=subprocess.Popen(command,env=env,stdout=out,stderr=err,start_new_session=True)
  try: code=child.wait(timeout=20)
  except subprocess.TimeoutExpired:
   os.killpg(child.pid,signal.SIGKILL);child.wait(timeout=5);raise
 result['returncode']=code
 records=[json.loads(l) for l in (base/'probe.jsonl').read_text().splitlines()]
 result['events']=[r for r in records if r.get('event')!='mapping']
 result['mapped_files']=sorted({r['path'] for r in records if r.get('event')=='mapping'})
 result['status']='passed' if code==0 and any(r.get('event')=='passed' for r in records) else 'failed'
 (base/'result.json').write_text(json.dumps(result,indent=2)+'\n')
 print(json.dumps({'status':result['status'],'events':result['events']}))
 if result['status']!='passed':raise SystemExit(2)
