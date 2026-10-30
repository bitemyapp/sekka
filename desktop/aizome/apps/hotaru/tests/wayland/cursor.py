#!/usr/bin/env python3
"""DECSCUSR shapes/blink in a private headless Wayland compositor.

Usage: cursor.py BINARY EVIDENCE_DIRECTORY. Never connects to the live desktop.
Uses repeated actual screenshots of an empty cursor cell, rather than assuming
which blink phase a wall-clock sleep will reach.
"""
import hashlib, io, json, os, re, shutil, subprocess, sys, tempfile, time
from pathlib import Path
from PIL import Image
from smoke import wait


def run(binary,evidence):
    evidence.mkdir(parents=True,exist_ok=True)
    runtime=Path(tempfile.mkdtemp(prefix="hotaru-cursor-wayland-"));os.chmod(runtime,0o700)
    env=dict(os.environ)
    for key in ["DISPLAY","WAYLAND_DISPLAY","SWAYSOCK","AIZOME_SHELL_SOCKET","AIZOME_SOCKET","SIDESTEP_BACKEND"]:env.pop(key,None)
    inspect=evidence/"inspect";inspect.mkdir(exist_ok=True)
    env.update(XDG_RUNTIME_DIR=str(runtime),XDG_CONFIG_HOME=str(runtime/"config"),WLR_BACKENDS="headless",WLR_RENDERER="pixman",
               WLR_LIBINPUT_NO_DEVICES="1",XDG_CURRENT_DESKTOP="sway",XDG_SESSION_TYPE="wayland",AIZOME_KIT_INSPECT=str(inspect),
               RUST_LOG="warn,aizome_hotaru=debug")
    config=runtime/"sway.conf";config.write_text("output HEADLESS-1 mode 1280x800\nseat seat0 fallback true\n")
    fixture=runtime/"fixture.py"
    fixture.write_text("""import os,pathlib,sys,time
root=pathlib.Path(sys.argv[1]);previous=None
while True:
    stage=(root/'stage').read_text() if (root/'stage').exists() else ''
    if stage and stage!=previous:
        code=b'\\x1b[?25l' if stage=='hidden' else f'\\x1b[{int(stage)} q\\x1b[?25h'.encode()
        os.write(1,b'\\x1b[2J\\x1b[3;1HCursor style '+stage.encode()+b'\\x1b[H'+code+b'\\x1b]2;cursor-'+stage.encode()+b'\\x07')
        previous=stage
    time.sleep(.01)
""")
    processes=[];files=[];result={"binary":str(binary),"binary_sha256":hashlib.sha256(binary.read_bytes()).hexdigest(),"backend":"private headless Sway/pixman", "checks":{}}
    try:
        def launch(name,argv,extra=None):
            log=open(evidence/(name+".log"),"wb");files.append(log)
            proc=subprocess.Popen(argv,env={**env,**(extra or {})},stdout=log,stderr=subprocess.STDOUT);processes.append(proc);return proc
        launch("sway",["sway","--unsupported-gpu","-c",str(config)])
        display=wait("private Wayland",lambda:next((p for p in runtime.glob("wayland-*") if p.is_socket()),None))
        socket=wait("private Sway",lambda:next(runtime.glob("sway-ipc.*.sock"),None))
        env.update(WAYLAND_DISPLAY=display.name,SWAYSOCK=str(socket))
        app=launch("hotaru",[str(binary),"--",sys.executable,str(fixture),str(runtime)])
        def inspected():
            try:return json.loads(next(inspect.glob(f"org.aizome.Hotaru.{app.pid}.*.json")).read_text())
            except (StopIteration,FileNotFoundError,json.JSONDecodeError):return None
        def stage(name):
            (runtime/"stage").write_text(str(name))
            wait("cursor title",lambda:(inspected() or {}).get("title")==f"cursor-{name} — Hotaru")
        def crop(save=None):
            tree=json.loads(subprocess.check_output(["swaymsg","-t","get_tree"],env=env))
            def find(node):
                if node.get("pid")==app.pid:return node
                for c in node.get("nodes",[])+node.get("floating_nodes",[]):
                    v=find(c)
                    if v:return v
            node=find(tree);assert node
            terminal=next(v for v in inspected()["elements"] if v.get("name","").startswith("terminal-"))
            metrics=re.findall(r'terminal font "([^"]+)", scale ([0-9.]+), grid ([0-9]+)x([0-9]+) px',(evidence/"hotaru.log").read_text(errors="replace"))
            assert metrics;_,scale,cw,ch=metrics[-1];assert float(scale)==1.
            x=int(node["rect"]["x"]+node["window_rect"]["x"]+terminal["rect"][0]+10)
            y=int(node["rect"]["y"]+node["window_rect"]["y"]+terminal["rect"][1]+10)
            data=subprocess.check_output(["grim","-"],env=env,timeout=3)
            if save:(evidence/save).write_bytes(data)
            image=Image.open(io.BytesIO(data)).convert("RGBA").crop((x,y,x+int(cw),y+int(ch)))
            return hashlib.sha256(image.tobytes()).hexdigest()
        wait("native window",inspected)
        stage("hidden");time.sleep(.2);blank=crop("hidden.png")
        steady={}
        for n in [2,4,6]:
            stage(n);time.sleep(.2);a=crop(f"steady-{n}.png");time.sleep(.7);assert crop()==a
            assert a!=blank;steady[n]=a
        assert len(set(steady.values()))==3
        result["checks"]["steady_distinct_and_stable"]=steady
        for n in [1,3,5,0]:
            stage(n);seen=set();deadline=time.monotonic()+3
            while time.monotonic()<deadline:
                seen.add(crop());time.sleep(.12)
            expected={blank,steady[{1:2,3:4,5:6,0:2}[n]]}
            assert seen==expected,(n,seen,expected)
            result["checks"][f"blink-{n}"]=sorted(seen)
        # Another private native window takes focus while both remain visible.
        stage(2);second_runtime=runtime/"second";second_runtime.mkdir(mode=0o700)
        second=launch("other",[str(binary),"--","/bin/sh","-c","sleep 60"],
                      {"XDG_RUNTIME_DIR":str(second_runtime),"WAYLAND_DISPLAY":str(display)})
        wait("second window",lambda:any(inspect.glob(f"org.aizome.Hotaru.{second.pid}.*.json")))
        time.sleep(.3);hollow=crop("unfocused.png");time.sleep(.8);assert crop()==hollow
        assert hollow not in {blank,*steady.values()}
        second.terminate();second.wait(timeout=3)
        subprocess.run(["swaymsg",f"[pid={app.pid}] focus"],env=env,check=True,stdout=subprocess.DEVNULL)
        time.sleep(.2);assert crop("refocused.png")==steady[2]
        result["checks"]["inactive_hollow_and_refocus"]=True
        result["status"]="passed"
    except Exception as error:
        result.update(status="failed",error=repr(error));raise
    finally:
        for p in reversed(processes):
            if p.poll() is None:
                p.terminate()
                try:p.wait(timeout=3)
                except subprocess.TimeoutExpired:p.kill();p.wait(timeout=3)
        for f in files:f.close()
        (evidence/"result.json").write_text(json.dumps(result,indent=2)+"\n")
        shutil.rmtree(runtime)
    print(json.dumps(result,indent=2))

if __name__=="__main__":run(Path(sys.argv[1]).resolve(),Path(sys.argv[2]).resolve())
