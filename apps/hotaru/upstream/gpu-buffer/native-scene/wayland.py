#!/usr/bin/env python3
"""Bounded private Intel Rust-worker + SCTK transport diagnostic."""
import fcntl, hashlib, importlib.util, json, os, pathlib, shutil, signal, subprocess, tempfile, time
base=pathlib.Path(__file__).resolve().parent
root=base.parent
helper=root/'tools/frame-matrix.py'
spec=importlib.util.spec_from_file_location('owned_cleanup',helper)
cleanup=importlib.util.module_from_spec(spec);spec.loader.exec_module(cleanup);cleanup.enable_subreaper()
mesa='/nix/store/457mqa9dsvw9cnmi6pr06zjy1ib0iic4-mesa-26.2.5'
egl='/nix/store/ww5pckflswny9whz04k06xh233jbg5kl-libglvnd-1.7.0/lib/libEGL.so.1'
libc='/nix/store/pqyiidz5mi73wv5f91d48s9plcqyj5bz-glibc-2.44-57/lib'
libgcc='/nix/store/3sj8msrsym648f35mk3gb3pp385hhk8r-gcc-16.2.0-libgcc/lib'
loader=[libc+'/ld-linux-x86-64.so.2','--library-path',libc+':'+libgcc]
result={'status':'running','scope':'actual complete native scene GPU composition and private headless GLES presentation; no live app event integration, physical scanout or performance claim','sources':{},'cleanup_helper_sha256':hashlib.sha256(helper.read_bytes()).hexdigest(),'loader':loader,'mesa':mesa,'egl':egl}
for p in sorted((base/'source').rglob('*')):
 if p.is_file():result['sources'][str(p.relative_to(base/'source'))]=hashlib.sha256(p.read_bytes()).hexdigest()
for name in ['scene-wayland-probe']:
 result[name+'_sha256']=hashlib.sha256((base/name).read_bytes()).hexdigest()
processes=[];logs=[]
with open('/home/callen/.cache/hotaru-local-benchmark.lock','a') as lock:
 fcntl.flock(lock,fcntl.LOCK_EX)
 runtime=pathlib.Path(tempfile.mkdtemp(prefix='hotaru-rust-gpu-'));os.chmod(runtime,0o700)
 env=dict(os.environ)
 for key in list(env):
  if key.startswith(('MESA_','EGL_','GL_','__EGL_','LIBGL_','GALLIUM_','DRI_','WLR_','GBM_')) or key in ['DISPLAY','WAYLAND_DISPLAY','SWAYSOCK','LD_LIBRARY_PATH','DBUS_SESSION_BUS_ADDRESS','XDG_RUNTIME_DIR']:env.pop(key,None)
 for d in ['home','cache','config']:(runtime/d).mkdir()
 env.update(HOME=str(runtime/'home'),XDG_RUNTIME_DIR=str(runtime),XDG_CONFIG_HOME=str(runtime/'config'),XDG_CACHE_HOME=str(runtime/'cache'),WLR_BACKENDS='headless',WLR_RENDERER='gles2',WLR_RENDER_DRM_DEVICE='/dev/dri/renderD128',WLR_LIBINPUT_NO_DEVICES='1',GBM_BACKENDS_PATH=mesa+'/lib/gbm',__EGL_VENDOR_LIBRARY_FILENAMES=mesa+'/share/glvnd/egl_vendor.d/50_mesa.json',LIBGL_DRIVERS_PATH=mesa+'/lib/dri',MESA_SHADER_CACHE_DISABLE='true')
 result['environment']={k:env[k] for k in ['WLR_BACKENDS','WLR_RENDERER','WLR_RENDER_DRM_DEVICE','GBM_BACKENDS_PATH','__EGL_VENDOR_LIBRARY_FILENAMES','LIBGL_DRIVERS_PATH']}
 try:
  result['elf_dependencies']=subprocess.check_output([*loader,'--list',str(base/'scene-wayland-probe')],text=True,env=env,timeout=10)
  def launch(name,cmd,stdin=False):
   log=(base/(name+'.log')).open('w');logs.append(log)
   p=subprocess.Popen(cmd,env=env,stdin=subprocess.PIPE if stdin else subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
   processes.append(p);return p
  def events(name):
   return [json.loads(l) for l in (base/(name+'.log')).read_text().splitlines() if l.startswith('{')]
  config=runtime/'sway.conf' ;config.write_text('output HEADLESS-1 mode 1280x960\nseat seat0 fallback true\ndefault_border none\n')
  sway=launch('sway',['sway','--unsupported-gpu','-d','-c',str(config)])
  end=time.monotonic()+10
  while True:
   display=next((p for p in runtime.glob('wayland-*') if p.is_socket()),None)
   if display:break
   if sway.poll() is not None:raise RuntimeError('private compositor exited')
   if time.monotonic()>end:raise TimeoutError('private compositor socket')
   time.sleep(.02)
  env['WAYLAND_DISPLAY']=display.name
  result['sway_mapped_files']=sorted({l[l.index('/'):].splitlines()[0] for l in pathlib.Path(f'/proc/{sway.pid}/maps').read_text().splitlines() if '/' in l})
  probe=launch('wayland',[*loader,str(base/'scene-wayland-probe'),egl,'/home/callen/work/hotaru-gpu-scene-isolated/artifacts/gpu-scene/fixtures/04.scene'],stdin=True)
  end=time.monotonic()+12
  while not any(e.get('event')=='rust_surface_frame' for e in events('wayland')):
   if probe.poll() is not None:raise RuntimeError('Rust probe exited before frame')
   if time.monotonic()>end:raise TimeoutError('Rust frame')
   time.sleep(.01)
  result['rust_mapped_files']=sorted({l[l.index('/'):].splitlines()[0] for l in pathlib.Path(f'/proc/{probe.pid}/maps').read_text().splitlines() if '/' in l})
  result['rust_mapped_file_sha256']={name:hashlib.sha256(pathlib.Path(name).read_bytes()).hexdigest() for name in result['rust_mapped_files'] if pathlib.Path(name).is_file() and (name.startswith('/nix/store/') or name==str(base/'scene-wayland-probe'))}
  capture=base/'capture.ppm' 
  subprocess.run(['grim','-t','ppm',str(capture)],check=True,env=env,timeout=5)
  raw=capture.read_bytes();magic,dimensions,maximum,pixels=raw.split(b'\n',3)
  assert magic==b'P6' and dimensions==b'1280 960' and maximum==b'255' and len(pixels)==1280*960*3
  reference=pathlib.Path('/home/callen/work/hotaru-gpu-scene-isolated/artifacts/gpu-scene/fixtures/04.rgba').read_bytes()
  assert len(reference)==1200*830*4 and all(v==255 for v in reference[3::4])
  expected=b''.join(reference[i:i+3]for i in range(0,len(reference),4))
  actual=b''.join(pixels[((y+65)*1280+40)*3:((y+65)*1280+1240)*3]for y in range(830))
  assert actual==expected, ('scene capture differs',sum(a!=b for a,b in zip(actual,expected)))
  result['capture']={'sha256':hashlib.sha256(raw).hexdigest(),'format':'P6 RGB8','width':1280,'height':960,'region':[40,65,1200,830],'checked_rgb_bytes':len(actual),'exact':True,'source_rgba_sha256':hashlib.sha256(reference).hexdigest()}
  probe.stdin.write(b'detach\n');probe.stdin.flush();probe.stdin.close()
  result['wayland_exit']=probe.wait(timeout=10)
  result['events']=events('wayland')
  assert result['wayland_exit']==0
  assert any(e.get('event')=='rust_buffer_release' and e['after_detach'] for e in result['events'])
  assert any(e.get('event')=='rust_wayland_probe_complete' for e in result['events'])
  for cleanup_event in ['fence_destroyed','resource_destroyed','scene_gpu_cache_destroyed','context_destroyed']:
   matches=[e for e in result['events'] if e.get('event')==cleanup_event]
   assert len(matches)==1 and matches[0].get('ok') is True, ('Wayland cleanup',cleanup_event,matches)
  result['hardware_verified']='GL renderer: Mesa Intel(R) Graphics (PTL)' in (base/'sway.log').read_text()
  assert result['hardware_verified']
  result['status']='passed'
 except Exception as error:
  result.update(status='failed',error=repr(error))
 finally:
  for p in reversed(processes):
   if p.poll() is None:
    os.killpg(p.pid,signal.SIGTERM)
    try:p.wait(timeout=3)
    except subprocess.TimeoutExpired:os.killpg(p.pid,signal.SIGKILL);p.wait(timeout=3)
  result['forced_descendant_cleanup']=cleanup.cleanup_descendants()
  result['all_owned_processes_exited']=all(p.poll() is not None for p in processes)
  if result['forced_descendant_cleanup']:result['status']='failed_unexpected_descendant'
  for log in logs:log.close()
  shutil.rmtree(runtime)
  result['runner_sha256']=hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest()
  (base/'wayland-result.json').write_text(json.dumps(result,indent=2)+'\n')
 print(json.dumps({k:result.get(k) for k in ['status','error','hardware_verified','capture','all_owned_processes_exited']}))
 if result['status']!='passed':raise SystemExit(2)
