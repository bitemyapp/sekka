#!/usr/bin/env python3
"""Actual private Wayland automatic links; fake xdg-open, no network/browser."""
import hashlib, json, os, re, select, shutil, subprocess, sys, tempfile, time
from pathlib import Path
from smoke import wait

def run(binary, evidence):
    evidence.mkdir(parents=True, exist_ok=True)
    build = evidence / "input"; build.mkdir(exist_ok=True)
    specs = [("virtual-keyboard", ".cargo/registry/src/*/wayland-protocols-misc-*/protocols/virtual-keyboard-unstable-v1.xml"),
             ("virtual-pointer", ".cargo/registry/src/*/wayland-protocols-wlr-*/wlr-protocols/unstable/wlr-virtual-pointer-unstable-v1.xml")]
    for name, pattern in specs:
        xml = next(Path.home().glob(pattern))
        for action, suffix in [("client-header", "-client.h"), ("private-code", ".c")]:
            subprocess.run(["wayland-scanner", action, str(xml), str(build / (name+suffix))], check=True)
    flags = subprocess.check_output(["pkg-config", "--cflags", "--libs", "wayland-client", "xkbcommon"], text=True).split()
    subprocess.run(["cc", "-O2", "-Wall", "-Wextra", "-Werror", "-I"+str(build), str(Path(__file__).with_name("auto-link-pointer.c")),
                    *[str(build/(name+".c")) for name,_ in specs], "-o", str(build/"pointer"), *flags], check=True)
    runtime = Path(tempfile.mkdtemp(prefix="hotaru-auto-links-")); runtime.chmod(0o700)
    opener = runtime/"bin"; opener.mkdir()
    calls = evidence/"opener.jsonl"; calls.unlink(missing_ok=True)
    fake = opener/"xdg-open"
    fake.write_text("#!/usr/bin/python3\nimport json,os,sys\nwith open(os.environ['HOTARU_TEST_OPEN_RECORD'],'a') as f: f.write(json.dumps(sys.argv[1:])+'\\n')\n")
    fake.chmod(0o700)
    env = dict(os.environ)
    for name in ["DISPLAY","WAYLAND_DISPLAY","SWAYSOCK","AIZOME_SHELL_SOCKET","AIZOME_SOCKET","SIDESTEP_BACKEND"]: env.pop(name,None)
    inspect = evidence/"inspect"; inspect.mkdir(exist_ok=True)
    env.update(XDG_RUNTIME_DIR=str(runtime), XDG_CONFIG_HOME=str(runtime/"config"), WLR_BACKENDS="headless", WLR_RENDERER="pixman",
               WLR_LIBINPUT_NO_DEVICES="1", XDG_CURRENT_DESKTOP="sway", XDG_SESSION_TYPE="wayland", AIZOME_KIT_INSPECT=str(inspect),
               PATH=str(opener)+":/usr/bin:/bin", HOTARU_TEST_OPEN_RECORD=str(calls), RUST_LOG="aizome_hotaru=debug")
    config = runtime/"sway.conf"; config.write_text("output HEADLESS-1 mode 1280x800\nseat seat0 fallback true\n")
    fixture = runtime/"fixture.py"
    fixture.write_text("""import fcntl,json,os,pathlib,struct,sys,termios,time
root=pathlib.Path(sys.argv[1]); previous=None
while True:
    try: command=json.loads((root/'stage').read_text())
    except (FileNotFoundError,json.JSONDecodeError): command=None
    if command and command!=previous:
        os.write(1,b'\\x1bc\\x1b[2J\\x1b[H\\x1b[?25l'+bytes.fromhex(command['data'])+b'\\x1b]2;'+command['name'].encode()+b'\\x07')
        previous=command
    size=struct.unpack('HHHH',fcntl.ioctl(0,termios.TIOCGWINSZ,bytes(8)))
    (root/'pty.tmp').write_text(json.dumps(size)); (root/'pty.tmp').replace(root/'pty.json')
    time.sleep(.01)
""")
    processes=[]; files=[]
    result={"binary":str(binary),"binary_sha256":hashlib.sha256(binary.read_bytes()).hexdigest(),"backend":"private Sway pixman","checks":[]}
    try:
        log=open(evidence/"sway.log","wb"); files.append(log)
        processes.append(subprocess.Popen(["sway","--unsupported-gpu","-c",str(config)],env=env,stdout=log,stderr=subprocess.STDOUT))
        display=wait("Wayland socket",lambda:next((p for p in runtime.glob('wayland-*') if p.is_socket()),None))
        socket=wait("Sway socket",lambda:next(runtime.glob('sway-ipc.*.sock'),None))
        env.update(WAYLAND_DISPLAY=display.name,SWAYSOCK=str(socket))
        log=open(evidence/"hotaru.log","wb"); files.append(log)
        app=subprocess.Popen([str(binary),'--',sys.executable,str(fixture),str(runtime)],env=env,stdout=log,stderr=subprocess.STDOUT); processes.append(app)
        def inspected():
            try:return json.loads(next(inspect.glob(f'org.aizome.Hotaru.{app.pid}.*.json')).read_text())
            except (StopIteration,FileNotFoundError,json.JSONDecodeError):return None
        def texts():return [e.get('text','') for e in (inspected() or {}).get('elements',[])]
        def opened():return [json.loads(s) for s in calls.read_text().splitlines()] if calls.exists() else []
        def stage(name,data):
            (runtime/'stage.tmp').write_text(json.dumps(dict(name=name,data=data.encode().hex()))); (runtime/'stage.tmp').replace(runtime/'stage')
            wait("stage "+name,lambda:(inspected() or {}).get('title')==name+' — Hotaru')
        def geometry():
            try:
                rows,cols,width,height=json.loads((runtime/'pty.json').read_text())
                return (rows,cols,width//cols,height//rows) if width and height else None
            except (FileNotFoundError,json.JSONDecodeError):return None
        wait('native window',inspected)
        last=[None,time.monotonic()]
        def settled():
            value=geometry()
            if value!=last[0]:last[:]=[value,time.monotonic()]
            return value if value and f"{value[1]} × {value[0]}" in texts() and time.monotonic()-last[1]>.2 else None
        rows,cols,cw,ch=wait('stable actual PTY pixels and chrome',settled)
        result['grid']={'rows':rows,'cols':cols,'cw':cw,'ch':ch}
        tree=json.loads(subprocess.check_output(['swaymsg','-t','get_tree'],env=env))
        def find(node):
            if node.get('pid')==app.pid:return node
            for child in node.get('nodes',[])+node.get('floating_nodes',[]):
                found=find(child)
                if found:return found
        node=find(tree); ox=node['rect']['x']+node['window_rect']['x']; oy=node['rect']['y']+node['window_rect']['y']
        terminal=next(e for e in inspected()['elements'] if e.get('name','').startswith('terminal-'))
        def pointer(row=0,col=2):
            terminal=next(e for e in inspected()['elements'] if e.get('name','').startswith('terminal-'))
            x=int(ox+terminal['rect'][0]+10+(col+.5)*cw); y=int(oy+terminal['rect'][1]+10+(row+.5)*ch)
            p=subprocess.Popen([str(build/'pointer'),str(x),str(y),'sequence'],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True); processes.append(p)
            assert select.select([p.stdout],[],[],3)[0] and p.stdout.readline().strip()=='ready'
            return p
        def command(p,c):
            p.stdin.write(c+'\n');p.stdin.flush()
            assert select.select([p.stdout],[],[],3)[0] and p.stdout.readline().strip()=='ack'
        def close(p):p.stdin.write('q\n');p.stdin.flush();assert p.wait(timeout=3)==0
        expected=[]
        def activate(name,text,target,row=0,col=2,hover_target=None):
            stage(name,text); p=pointer(row,col)
            wait('matched '+name,lambda:(hover_target or target) in texts())
            command(p,'c'); expected.append([target])
            wait('opened '+name,lambda:len(opened())==len(expected));assert opened()==expected,opened()
            close(p);result['checks'].append(name)
        target='https://example.test/日本?x=$literal&quote=literal'
        activate('unicode-url',target,target)
        path=runtime/'relative.txt';path.write_text('fixture')
        cwd='\x1b]7;file://localhost'+str(runtime)+'\x1b\\'
        activate('relative-existing',cwd+'./relative.txt',str(path),hover_target='./relative.txt')
        activate('relative-missing',cwd+'./missing.txt','./missing.txt')
        wrapped='https://example.test/'+'a'*(cols+13)
        activate('softwrap',wrapped,wrapped,row=1,col=2)
        activate('semantic-boundary','\x1b]133;A\x07PROMPT\x1b]133;B\x07https://example.test/input','https://example.test/input',col=10)
        activate('osc8-priority','\x1b]8;;https://explicit.test\x1b\\https://implicit.test\x1b]8;;\x1b\\','https://explicit.test')
        stage('cancel-old','https://cancel.test/old');p=pointer();wait('old hover',lambda:'https://cancel.test/old' in texts());command(p,'d')
        stage('cancel-new','https://cancel.test/new');wait('new hover',lambda:'https://cancel.test/new' in texts());command(p,'u');close(p)
        # A final completed valid activation orders earlier cancelled gestures.
        activate('barrier','https://barrier.test','https://barrier.test')
        assert opened()==expected
        result['checks'].append('changed-frame-held-gesture-cancelled')
        subprocess.run(['grim',str(evidence/'automatic-links.png')],env=env,check=True,timeout=3)
        result.update(status='passed',opened_argv=opened())
    except Exception as error:
        result.update(status='failed',error=repr(error));raise
    finally:
        for process in reversed(processes):
            if process.poll() is None:
                process.terminate()
                try:process.wait(timeout=3)
                except subprocess.TimeoutExpired:process.kill();process.wait(timeout=3)
        for file in files:file.close()
        (evidence/'result.json').write_text(json.dumps(result,indent=2)+'\n');shutil.rmtree(runtime)
    print(json.dumps(result,indent=2))
if __name__=='__main__':run(Path(sys.argv[1]).resolve(),Path(sys.argv[2]).resolve())
