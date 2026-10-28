#!/usr/bin/env python3
"""Private headless GLES/Wayland import diagnostic. No desktop connection."""
import fcntl,hashlib,importlib.util,json,os,pathlib,shutil,signal,socket,subprocess,tempfile,time
base=pathlib.Path(__file__).resolve().parent
root=base.parent
cleanup_source=root/'tools/frame-matrix.py'
cleanup_spec=importlib.util.spec_from_file_location('owned_process_cleanup',cleanup_source)
cleanup_module=importlib.util.module_from_spec(cleanup_spec)
cleanup_spec.loader.exec_module(cleanup_module)
cleanup_module.enable_subreaper()
registry=pathlib.Path.home()/'.cargo/registry/src'
mesa='/nix/store/457mqa9dsvw9cnmi6pr06zjy1ib0iic4-mesa-26.2.5'
loader='/nix/store/ww5pckflswny9whz04k06xh233jbg5kl-libglvnd-1.7.0'
dev='/nix/store/risg12y850mmilsg2ikrx417mg1dj6dm-libglvnd-1.7.0-dev'
original=(root/'gpu-capability/gpu-buffer-probe.c').read_text()
assert hashlib.sha256(original.encode()).hexdigest()=='9ae5753f7a6d316607009c127e0e991f6c0919772328e912c523c2c2abcf4102'
source=original.replace('int main(int argc, char **argv) {','extern void hotaru_probe_wayland_present(int, uint32_t, uint32_t, uint32_t, uint64_t);\nint main(int argc, char **argv) {')
needle='    if (!p_eglDestroySyncKHR(display, fence)) failure("destroy fence");'
assert source.count(needle)==1
source=source.replace(needle,needle+'\n    hotaru_probe_wayland_present(fd, (uint32_t)fourcc, (uint32_t)stride, (uint32_t)offset, (uint64_t)modifiers[0]);')
(base/'producer.c').write_text(source)
patterns={'linux-dmabuf':'*/wayland-protocols-*/protocols/stable/linux-dmabuf/linux-dmabuf-v1.xml','layer-shell':'*/wayland-protocols-wlr-*/wlr-protocols/unstable/wlr-layer-shell-unstable-v1.xml','screencopy':'*/wayland-protocols-wlr-*/wlr-protocols/unstable/wlr-screencopy-unstable-v1.xml','presentation':'*/wayland-protocols-*/protocols/stable/presentation-time/presentation-time.xml','xdg-shell':'*/wayland-protocols-*/protocols/stable/xdg-shell/xdg-shell.xml'}
result={'status':'running','scope':'private headless hardware-GLES compositor import and captured pixels, no physical scanout or performance claim','host':os.uname().nodename,'original_probe_sha256':hashlib.sha256(original.encode()).hexdigest(),'protocols':{},'sources':{},'cleanup_helper_sha256':hashlib.sha256(cleanup_source.read_bytes()).hexdigest(),'mesa':mesa,'loader':loader}
processes=[];logs=[]
with open('/home/callen/.cache/hotaru-local-benchmark.lock','a') as lock:
 fcntl.flock(lock,fcntl.LOCK_EX)
 codes=[]
 for name,pattern in patterns.items():
  xml=sorted(registry.glob(pattern))[-1];result['protocols'][name]={'path':str(xml),'sha256':hashlib.sha256(xml.read_bytes()).hexdigest()}
  header=base/(name+'-client.h');code=base/(name+'-protocol.c');codes.append(str(code))
  subprocess.run(['wayland-scanner','client-header',str(xml),str(header)],check=True,timeout=10)
  subprocess.run(['wayland-scanner','private-code',str(xml),str(code)],check=True,timeout=10)
 flags=subprocess.check_output(['pkg-config','--cflags','--libs','wayland-client'],text=True).split()
 binary=base/'gpu-wayland-probe'
 command=['cc','-std=c11','-O2','-Wall','-Wextra','-Werror','-I'+dev+'/include','-I'+str(base),str(base/'producer.c'),str(base/'wayland-transport.c'),*codes,*flags,'-L'+loader+'/lib','-Wl,-rpath,'+loader+'/lib','-lEGL','-o',str(binary)]
 result['build_command']=command
 with (base/'build.log').open('w') as log:
  built=subprocess.run(command,stdout=log,stderr=subprocess.STDOUT,timeout=60)
 result['build_returncode']=built.returncode
 for name in ['producer.c','wayland-transport.c','run.py']:
  result['sources'][name]=hashlib.sha256((base/name).read_bytes()).hexdigest()
 if built.returncode:
  result['status']='build_failed';(base/'result.json').write_text(json.dumps(result,indent=2)+'\n');raise SystemExit(2)
 result['binary_sha256']=hashlib.sha256(binary.read_bytes()).hexdigest()
 runtime=pathlib.Path(tempfile.mkdtemp(prefix='hotaru-gpu-wayland-'));os.chmod(runtime,0o700)
 env=dict(os.environ)
 for key in list(env):
  if key.startswith(('MESA_','EGL_','GL_','__EGL_','LIBGL_','GALLIUM_','DRI_','WLR_','GBM_')) or key in ['DISPLAY','WAYLAND_DISPLAY','SWAYSOCK','LD_LIBRARY_PATH','DBUS_SESSION_BUS_ADDRESS','XDG_RUNTIME_DIR']:
   env.pop(key,None)
 for name in ['home','config','cache']: (runtime/name).mkdir()
 env.update(HOME=str(runtime/'home'),XDG_RUNTIME_DIR=str(runtime),XDG_CONFIG_HOME=str(runtime/'config'),XDG_CACHE_HOME=str(runtime/'cache'),WLR_BACKENDS='headless',WLR_RENDERER='gles2',GBM_BACKENDS_PATH=mesa+'/lib/gbm',WLR_RENDER_DRM_DEVICE='/dev/dri/renderD128',WLR_LIBINPUT_NO_DEVICES='1',__EGL_VENDOR_LIBRARY_FILENAMES=mesa+'/share/glvnd/egl_vendor.d/50_mesa.json',LIBGL_DRIVERS_PATH=mesa+'/lib/dri',MESA_SHADER_CACHE_DISABLE='true',HOTARU_GPU_CAPTURE_PATH=str(base/'capture.raw'))
 config=runtime/'sway.conf';config.write_text('output HEADLESS-1 mode 128x128\nseat seat0 fallback true\ndefault_border none\n')
 result['environment']={key:env[key] for key in ['WLR_BACKENDS','WLR_RENDERER','WLR_RENDER_DRM_DEVICE','GBM_BACKENDS_PATH','__EGL_VENDOR_LIBRARY_FILENAMES','LIBGL_DRIVERS_PATH','MESA_SHADER_CACHE_DISABLE']}
 try:
  def launch(name,command):
   log=(base/(name+'.log')).open('w');logs.append(log)
   p=subprocess.Popen(command,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True);processes.append(p);return p
  sway=launch('sway',['sway','--unsupported-gpu','-d','-c',str(config)])
  deadline=time.monotonic()+10
  while True:
   display=next((p for p in runtime.glob('wayland-*') if p.is_socket()),None)
   if display:break
   if sway.poll() is not None:raise RuntimeError('private compositor exited')
   if time.monotonic()>=deadline:raise TimeoutError('private compositor socket')
   time.sleep(.02)
  env['WAYLAND_DISPLAY']=display.name
  result['sway_pid']=sway.pid
  result['sway_mapped_files']=sorted({line[line.index('/'):].splitlines()[0] for line in pathlib.Path(f'/proc/{sway.pid}/maps').read_text().splitlines() if '/' in line})
  probe=launch('probe',['/nix/store/78jn0lxqjgz4ahjzvvh4rvan7p607xph-strace-7.2/bin/strace','-f','-yy','-e','trace=openat,ioctl,close,sendmsg,recvmsg','-o',str(base/'probe.strace'),str(binary),'0'])
  result['returncode']=probe.wait(timeout=30)
  records=[json.loads(line) for line in (base/'probe.log').read_text().splitlines() if line.startswith('{')]
  result['events']=[r for r in records if r.get('event')!='mapping']
  result['probe_mapped_files']=sorted({r['path'] for r in records if r.get('event')=='mapping'})
  hardware='GL renderer: Mesa Intel(R) Graphics (PTL)' in (base/'sway.log').read_text()
  result['compositor_hardware_renderer_verified']=hardware
  passed=result['returncode']==0 and hardware and any(r.get('event')=='wayland_transport_passed' for r in records) and any(r.get('event')=='passed' for r in records)
  result['status']='passed' if passed else 'failed'
  if (base/'capture.raw').exists():result['capture_sha256']=hashlib.sha256((base/'capture.raw').read_bytes()).hexdigest()
 except Exception as error:
  result.update(status='failed',error=repr(error));raise
 finally:
  for p in reversed(processes):
   if p.poll() is None:
    os.killpg(p.pid,signal.SIGTERM)
    try:p.wait(timeout=3)
    except subprocess.TimeoutExpired:os.killpg(p.pid,signal.SIGKILL);p.wait(timeout=3)
  result['forced_descendant_cleanup']=cleanup_module.cleanup_descendants()
  result['all_owned_processes_exited']=all(p.poll() is not None for p in processes)
  result['owned_descendant_check']='process-local subreaper and pidfd ownership recheck'
  if result['forced_descendant_cleanup']:
   result['status']='failed_unexpected_owned_descendant'
  for log in logs:log.close()
  shutil.rmtree(runtime)
  (base/'result.json').write_text(json.dumps(result,indent=2)+'\n')
 print(json.dumps({'status':result['status'],'returncode':result.get('returncode'),'hardware':result.get('compositor_hardware_renderer_verified')}))
 if result['status']!='passed':raise SystemExit(2)
