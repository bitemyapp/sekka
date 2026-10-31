#!/usr/bin/env python3
"""Exact native screenshots with existing pixel scrolling enabled/disabled.

This is a correctness test in disposable Sway, not a timing benchmark.
"""
import hashlib, io, json, os, shutil, subprocess, sys, tempfile, time
from pathlib import Path
from PIL import Image
from smoke import wait


def run(binary, evidence):
    evidence.mkdir(parents=True, exist_ok=True)
    fixture_source = r'''import os,pathlib,sys,time
root=pathlib.Path(sys.argv[1]); previous=-1; serial=0
time.sleep(.4)
os.write(1,b'\x1b[2 q\x1b[?25h')
def row():
    global serial
    serial+=1
    text=f'{serial:04} 日本語 😀 e\u0301 != -> \x1b[3{serial%8};4mcolor\x1b[0m\r\n'
    os.write(1,text.encode())
for _ in range(90): row()
while True:
    try:stage=int((root/'stage').read_text())
    except (FileNotFoundError,ValueError):time.sleep(.005);continue
    if stage!=previous:
        if 0 < stage < 28:
            for _ in range(1+stage%5):row()
        if stage==8:os.write(1,b'\x1b]4;1;#ff8844\x07')
        if stage==12:os.write(1,b'\x1b[4 q')
        if stage==16:os.write(1,b'\x1b[6 q')
        if stage==20:os.write(1,b'\x1b[?25l')
        if stage==24:os.write(1,b'\x1b[?25h\x1b[2 q')
        if stage==28:
            rows=os.get_terminal_size().lines
            os.write(1,b'\x1b[r\x1b[2J\x1b[H')
            for y in range(1,rows+1):
                os.write(1,f'\x1b[{y};1Hregion {y:03} 日本語 😀 e\u0301 ─┼│ \x1b[3{y%8};4mcolor\x1b[0m'.encode())
            os.write(1,f'\x1b[2;{rows-1}r\x1b[{rows-1};1H'.encode())
        actions={29:b'\n',30:b'\x1b[2S',31:b'\x1b[2;1H\x1bM',32:b'\x1b[2T',
                 33:b'\x1b[6;1H\x1b[2L',34:b'\x1b[6;1H\x1b[2M',
                 36:b'\x1b[8;1Hchanged retained row\x1b[S',37:b'\x1b[6 q\x1b[T',
                 38:b'\x1b[4 q\x1b[S',39:b'\x1b[?25l\x1b[T',
                 40:b'\x1b[?25h\x1b[2 q\x1b[S',41:b'\x1b[r\x1b[2T'}
        if stage==35:
            mid=rows//2
            os.write(1,f'\x1b[2;{mid}r\x1b[S\x1b[{mid+2};{rows-1}r\x1b[T'.encode())
        if stage in actions:os.write(1,actions[stage])
        os.write(1,f'\x1b]2;scroll-stage-{stage}\x07'.encode())
        previous=stage
    time.sleep(.005)
'''
    screenshots = {}
    for name, disabled in [('copy', '0'), ('redraw', '1')]:
        mode = evidence / name; mode.mkdir(exist_ok=True)
        runtime = Path(tempfile.mkdtemp(prefix='hotaru-pixel-scroll-')); os.chmod(runtime, 0o700)
        env = dict(os.environ)
        for key in ['DISPLAY','WAYLAND_DISPLAY','SWAYSOCK','AIZOME_SHELL_SOCKET','AIZOME_SOCKET','SIDESTEP_BACKEND']:
            env.pop(key, None)
        inspect = mode/'inspect'; inspect.mkdir(exist_ok=True)
        env.update(XDG_RUNTIME_DIR=str(runtime), XDG_CONFIG_HOME=str(runtime/'config'),
                   WLR_BACKENDS='headless', WLR_RENDERER='pixman', WLR_LIBINPUT_NO_DEVICES='1',
                   XDG_CURRENT_DESKTOP='sway', XDG_SESSION_TYPE='wayland', AIZOME_NO_PIXEL_SCROLL=disabled,
                   AIZOME_KIT_INSPECT=str(inspect))
        config=runtime/'sway.conf';config.write_text('output HEADLESS-1 mode 1280x800\nseat seat0 fallback true\n')
        fixture=runtime/'fixture.py';fixture.write_text(fixture_source)
        files=[]; processes=[]; screenshots[name]=[]
        try:
            def launch(label, command):
                stream=open(mode/(label+'.log'),'wb');files.append(stream)
                process=subprocess.Popen(command,env={**env, **({"SIDESTEP_TRACE_FRAMES":"tiles", "WAYLAND_DEBUG":"client"} if label=="hotaru" else {})},stdout=stream,stderr=subprocess.STDOUT)
                processes.append(process);return process
            launch('sway',['sway','--unsupported-gpu','-c',str(config)])
            display=wait('private Wayland',lambda:next((p for p in runtime.glob('wayland-*') if p.is_socket()),None))
            env['WAYLAND_DISPLAY']=display.name
            app=launch('hotaru',[str(binary),'--',sys.executable,str(fixture),str(runtime)])
            def title():
                try:return json.loads(next(inspect.glob(f'org.aizome.Hotaru.{app.pid}.*.json')).read_text())['title']
                except (StopIteration,FileNotFoundError,json.JSONDecodeError):return None
            wait('native window',title)
            for stage in range(42):
                (runtime/'stage').write_text(str(stage))
                wait('stage title',lambda:title()==f'scroll-stage-{stage} — Hotaru')
                # Require repeated unchanged actual compositor frames. The title
                # acknowledgment alone can precede worker-shaped terminal text.
                previous=None; stable=0; deadline=time.monotonic()+5
                time.sleep(.15)
                while time.monotonic()<deadline:
                    png=subprocess.check_output(['grim','-'],env=env,timeout=3)
                    pixels=Image.open(io.BytesIO(png)).convert('RGBA').tobytes()
                    digest=hashlib.sha256(pixels).hexdigest()
                    stable=stable+1 if digest==previous else 0;previous=digest
                    if stable>=2:break
                    time.sleep(.05)
                else:raise AssertionError(f'unstable stage {stage}')
                (mode/f'{stage:02}.png').write_bytes(png)
                screenshots[name].append(digest)
        finally:
            for process in reversed(processes):
                if process.poll() is None:
                    process.terminate()
                    try:process.wait(timeout=3)
                    except subprocess.TimeoutExpired:process.kill();process.wait(timeout=3)
            for stream in files:stream.close()
            shutil.rmtree(runtime)
    differing=[n for n,(a,b) in enumerate(zip(screenshots['copy'],screenshots['redraw'])) if a!=b]
    report={'binary':str(binary),'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
            'scope':'private headless Sway/pixman actual screenshots; exact RGBA, no timing claim',
            'frames_per_mode':42,'screenshots_sha256':screenshots,'differing_frames':differing,
            'status':'passed' if not differing else 'failed'}
    (evidence/'result.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({'status':report['status'],'differing_frames':differing,'frames_per_mode':42}))
    assert not differing, differing


if __name__=='__main__':run(Path(sys.argv[1]).resolve(),Path(sys.argv[2]).resolve())
