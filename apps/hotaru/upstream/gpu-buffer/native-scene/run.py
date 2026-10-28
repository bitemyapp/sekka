#!/usr/bin/env python3
"""Isolated hardware correctness/diagnostic-cost probe; never touches desktop."""
import fcntl,hashlib,importlib.util,json,os,pathlib,shutil,signal,subprocess,tempfile,time
base=pathlib.Path(__file__).resolve().parent;root=base.parent
helper=root/'tools/frame-matrix.py';spec=importlib.util.spec_from_file_location('owned_cleanup',helper);cleanup=importlib.util.module_from_spec(spec);spec.loader.exec_module(cleanup);cleanup.enable_subreaper()
mesa='/nix/store/457mqa9dsvw9cnmi6pr06zjy1ib0iic4-mesa-26.2.5'
egl='/nix/store/ww5pckflswny9whz04k06xh233jbg5kl-libglvnd-1.7.0/lib/libEGL.so.1'
libc='/nix/store/pqyiidz5mi73wv5f91d48s9plcqyj5bz-glibc-2.44-57/lib';libgcc='/nix/store/3sj8msrsym648f35mk3gb3pp385hhk8r-gcc-16.2.0-libgcc/lib'
loader=[libc+'/ld-linux-x86-64.so.2','--library-path',libc+':'+libgcc]
fixtures=pathlib.Path('/home/callen/work/hotaru-gpu-scene-isolated/artifacts/gpu-scene/fixtures')
sha=lambda p:hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
result={'status':'running','scope':'actual complete native scene GPU composition and DMA-BUF export, offscreen only; diagnostic costs are not application benchmark results','loader':loader,'egl':egl,'mesa':mesa,'binary_sha256':sha(base/'scene-worker-probe'),'source':{str(p.relative_to(base/'source')):sha(p)for p in sorted((base/'source').rglob('*'))if p.is_file()},'fixtures':{p.name:sha(p)for p in sorted(fixtures.iterdir())if p.is_file()},'cleanup_helper_sha256':sha(helper)}
with open('/home/callen/.cache/hotaru-local-benchmark.lock','a')as lock:
 fcntl.flock(lock,fcntl.LOCK_EX)
 runtime=pathlib.Path(tempfile.mkdtemp(prefix='hotaru-compute-'));os.chmod(runtime,0o700);env=dict(os.environ);probe=None
 for key in list(env):
  if key.startswith(('MESA_','EGL_','GL_','__EGL_','LIBGL_','GALLIUM_','DRI_','WLR_','GBM_'))or key in ['DISPLAY','WAYLAND_DISPLAY','SWAYSOCK','LD_LIBRARY_PATH','DBUS_SESSION_BUS_ADDRESS','XDG_RUNTIME_DIR']:env.pop(key,None)
 for d in ['home','cache','config']:(runtime/d).mkdir()
 env.update(HOME=str(runtime/'home'),XDG_RUNTIME_DIR=str(runtime),XDG_CONFIG_HOME=str(runtime/'config'),XDG_CACHE_HOME=str(runtime/'cache'),GBM_BACKENDS_PATH=mesa+'/lib/gbm',__EGL_VENDOR_LIBRARY_FILENAMES=mesa+'/share/glvnd/egl_vendor.d/50_mesa.json',LIBGL_DRIVERS_PATH=mesa+'/lib/dri',MESA_SHADER_CACHE_DISABLE='true')
 try:
  result['elf_dependencies']=subprocess.check_output([*loader,'--list',str(base/'scene-worker-probe')],text=True,env=env,timeout=10)
  with (base/'probe.log').open('w')as log:
   probe=subprocess.Popen([*loader,str(base/'scene-worker-probe'),egl,str(fixtures)],env=env,stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT,start_new_session=True)
   end=time.monotonic()+60;maps=set()
   while probe.poll()is None:
    if time.monotonic()>end:raise TimeoutError('scene probe deadline')
    try:maps.update(l[l.index('/'):]for l in pathlib.Path(f'/proc/{probe.pid}/maps').read_text().splitlines()if '/'in l)
    except FileNotFoundError:pass
    time.sleep(.02)
  result['exit']=probe.returncode;result['mapped_file_sha256']={p:sha(p)for p in sorted(maps)if pathlib.Path(p).is_file()and(p.startswith('/nix/store/')or p==str(base/'scene-worker-probe'))}
  events=[json.loads(l)for l in (base/'probe.log').read_text().splitlines()if l.startswith('{')];result['events']=events
  assert result['exit']==0, 'probe failed'
  scenes=[e for e in events if e.get('event')=='scene_verified'];assert len(scenes)==36 and all(e['different_bytes']==0 for e in scenes)
  assert all(scenes[i]['asset_upload_bytes']==0 for i in range(1,36,2)), 'warm cache unexpectedly uploads'
  for name,count in [('fence_destroyed',36),('resource_destroyed',36),('scene_gpu_cache_destroyed',1),('context_destroyed',1)]:
   checks=[e for e in events if e.get('event')==name];assert len(checks)==count and all(e.get('ok')is True for e in checks),(name,checks)
  assert any(e.get('event')=='scene_probe_complete'for e in events)
  assert any(e.get('event')=='rust_context'and'Mesa Intel'in e.get('renderer','')for e in events)
  result['status']='passed'
 except Exception as e:result.update(status='failed',error=repr(e))
 finally:
  if probe is not None and probe.poll()is None:
   os.killpg(probe.pid,signal.SIGTERM)
   try:probe.wait(timeout=3)
   except subprocess.TimeoutExpired:os.killpg(probe.pid,signal.SIGKILL);probe.wait(timeout=3)
  result['forced_descendant_cleanup']=cleanup.cleanup_descendants();result['all_owned_processes_exited']=probe is None or probe.poll()is not None
  if result['forced_descendant_cleanup']:result['status']='failed_unexpected_descendant'
  shutil.rmtree(runtime);result['runner_sha256']=sha(__file__);(base/'result.json').write_text(json.dumps(result,indent=2)+'\n')
 print(json.dumps({k:result.get(k)for k in ['status','error','exit','all_owned_processes_exited']}))
 if result['status']!='passed':raise SystemExit(2)
