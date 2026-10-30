#!/usr/bin/env python3
"""Private headless software compositor; exact complete-worker-SHM presentation."""
import hashlib,json,os,signal,subprocess,tempfile,time,sys,shutil
from pathlib import Path
root=Path(__file__).resolve().parent
evidence=root/'artifacts'/'wayland-admission';evidence.mkdir(parents=True,exist_ok=True)
for name in ['ready.json','released.json','detach']:(evidence/name).unlink(missing_ok=True)
exe=None
for line in (root/'artifacts'/'native-build.jsonl').read_text().splitlines():
    try:r=json.loads(line)
    except ValueError:continue
    if r.get('reason')=='compiler-artifact' and r.get('target',{}).get('name')=='scene_inventory' and r.get('executable'):exe=Path(r['executable'])
assert exe and exe.is_file()
runtime=Path(tempfile.mkdtemp(prefix='hotaru-admission-wayland-'));os.chmod(runtime,0o700)
env=dict(os.environ)
for key in ['DISPLAY','WAYLAND_DISPLAY','SWAYSOCK','AIZOME_SOCKET','AIZOME_SHELL_SOCKET','SIDESTEP_BACKEND']:env.pop(key,None)
env.update(XDG_RUNTIME_DIR=str(runtime),XDG_CONFIG_HOME=str(runtime/'config'),WLR_BACKENDS='headless',WLR_RENDERER='pixman',WLR_LIBINPUT_NO_DEVICES='1',XDG_CURRENT_DESKTOP='sway',XDG_SESSION_TYPE='wayland',AIZOME_FONT_DIR='/nix/store/b9vp4qd3gz9vmsl20grvd80hniirn5n5-aizome-fonts',HOTARU_FONT_DIR='/nix/store/1rv68ixfzh8ki0jmanx7y846h2qdvzbg-hotaru-fonts',HOTARU_SCENE_INVENTORY=str(evidence/'inventory.json'),HOTARU_ADMISSION_WAYLAND=str(evidence),HOTARU_SCENE_EXPECTED_DIR='/home/callen/work/hotaru-gpu-scene-isolated/artifacts/gpu-scene/fixtures')
config=runtime/'sway.conf';config.write_text('output HEADLESS-1 mode 1280x960\nseat seat0 fallback true\nxwayland disable\n')
processes=[];logs=[]
def start(name,args):
    log=(evidence/(name+'.log')).open('wb');logs.append(log)
    p=subprocess.Popen(args,env=env,stdout=log,stderr=subprocess.STDOUT,start_new_session=True);processes.append((name,p));return p
def wait(pred,seconds=20):
    end=time.monotonic()+seconds
    while time.monotonic()<end:
        if value:=pred():return value
        if any(p.poll() is not None for _,p in processes):raise RuntimeError('owned process exited early')
        time.sleep(.01)
    raise RuntimeError('bounded phase timeout')
result={'scope':'isolated full native CPU-worker SHM fallback; simulated GPU unavailability; private headless Sway pixman','binary':str(exe),'binary_sha256':hashlib.sha256(exe.read_bytes()).hexdigest(),'runner_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
try:
    sway=start('sway',['/usr/bin/sway','--unsupported-gpu','-c',str(config)])
    display=wait(lambda:next((p for p in runtime.glob('wayland-*') if p.is_socket()),None))
    env['WAYLAND_DISPLAY']=display.name
    test=start('native',[str(exe),'--exact','complete_native_terminal_scene_inventory','--nocapture'])
    wait(lambda:(evidence/'ready.json').is_file())
    geometry=json.loads((evidence/'ready.json').read_text());w,h=geometry['width'],geometry['height']
    subprocess.run(['/usr/bin/grim','-t','ppm',str(evidence/'capture.ppm')],env=env,check=True,timeout=3)
    data=(evidence/'capture.ppm').read_bytes();parts=data.split(b'\n',3);assert parts[:3]==[b'P6',b'1280 960',b'255'];rgb=parts[3];assert len(rgb)==1280*960*3
    x,y=(1280-w)//2,(960-h)//2;actual=b''.join(rgb[((y+r)*1280+x)*3:((y+r)*1280+x+w)*3]for r in range(h))
    expected=(evidence/'expected.rgba').read_bytes();assert len(expected)==w*h*4 and all(a==255 for a in expected[3::4])
    expected_rgb=b''.join(expected[i:i+3]for i in range(0,len(expected),4))
    assert actual==expected_rgb,('RGB mismatch',next((i for i,(a,b)in enumerate(zip(actual,expected_rgb))if a!=b),None))
    result.update(exact_rgb_bytes=len(actual),extent=[w,h],origin=[x,y],expected_rgba_sha256=hashlib.sha256(expected).hexdigest(),capture_sha256=hashlib.sha256(data).hexdigest())
    (evidence/'detach').write_text('detach\n');test.wait(timeout=10);assert test.returncode==0
    result['release']=json.loads((evidence/'released.json').read_text());assert all(result['release'].values())
    result['native_scenes']=len(json.loads((evidence/'inventory.json').read_text()));assert result['native_scenes']==18
    result['status']='passed'
except BaseException as e:result.update(status='failed',error=repr(e));raise
finally:
    forced=[]
    for name,p in reversed(processes):
        if p.poll()is None:
            os.killpg(p.pid,signal.SIGTERM)
            try:p.wait(timeout=3)
            except subprocess.TimeoutExpired:forced.append(name);os.killpg(p.pid,signal.SIGKILL);p.wait(timeout=3)
    result['forced_kill']=forced;result['all_owned_leaders_reaped']=all(p.poll()is not None for _,p in processes)
    group_gone=[]
    for _,p in processes:
        try:os.killpg(p.pid,0);group_gone.append(False)
        except ProcessLookupError:group_gone.append(True)
    result['all_owned_process_groups_gone']=all(group_gone)
    if not result['all_owned_process_groups_gone']:result.update(status='failed',cleanup_error='owned process group remains')
    result['tools']={name:subprocess.run([path,'--version'],capture_output=True,text=True,timeout=3).stdout.strip()for name,path in [('sway','/usr/bin/sway'),('grim','/usr/bin/grim')]}
    for log in logs:log.close()
    (evidence/'result.json').write_text(json.dumps(result,indent=2)+'\n');shutil.rmtree(runtime)
if result["status"]!="passed":raise RuntimeError("owned cleanup qualification failed")
print(json.dumps(result,indent=2))
