#!/usr/bin/env python3
"""Unchanged Ghostty GTK OSC52 observations in an owned private Wayland session."""
import argparse
import base64
import contextlib
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import select
import signal
import socket
import subprocess
import sys
import tempfile
import time
import tty

GHOSTTY = Path('/nix/store/rvbnq9b2j16127m46g7ia1vv9zw5cy03-ghostty-1.4.1-dev+6ba144158577d033ccb6ed4b574e4013957c9161-nix/bin/ghostty')
OBSERVER = Path('/home/callen/work/hotaru-frame-harness-20261028/build/frame-observer')
CLEANUP = Path('/home/callen/work/hotaru-region-frames-20261028/tools/frame-matrix.py')


def sha(data):
    return hashlib.sha256(data).hexdigest()


def data_record(data):
    return {'length': len(data), 'sha256': sha(data), **({'hex': data.hex()} if len(data) <= 4096 else {})}


def fixture(address):
    tty.setraw(0)
    with socket.socket(socket.AF_UNIX) as conn:
        conn.connect(address)
        f = conn.makefile('rwb', buffering=0)
        conn.sendall(json.dumps({'ready': True, 'pid': os.getpid()}).encode()+b'\n')
        while True:
            line = f.readline(32*1024*1024)
            if not line:
                return
            request = json.loads(line)
            raw = base64.b64decode(request.get('data', ''))
            before = time.monotonic_ns()
            chunk=max(1,min(65536,request.get('write_chunk',65536)))
            for offset in range(0, len(raw), chunk):
                remaining = raw[offset:offset+chunk]
                while remaining:
                    written = os.write(1, remaining)
                    remaining = remaining[written:]
            reply = bytearray()
            deadline = time.monotonic()+request.get('wait', .25)
            while time.monotonic() < deadline:
                if select.select([0], [], [], max(0, deadline-time.monotonic()))[0]:
                    piece = os.read(0, 65536)
                    if not piece:
                        break
                    reply.extend(piece)
                    if len(reply) > 16*1024*1024:
                        raise RuntimeError('bounded reply exceeded')
            conn.sendall(json.dumps({'reply': base64.b64encode(reply).decode(),
                                'size': list(os.get_terminal_size(0)),
                                'elapsed_ns': time.monotonic_ns()-before}).encode()+b'\n')


def wait_for(label, fn, seconds=10):
    end = time.monotonic()+seconds
    while time.monotonic() < end:
        value = fn()
        if value:
            return value
        time.sleep(.02)
    raise RuntimeError('timeout: '+label)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--fixture')
    parser.add_argument('--output', type=Path)
    parser.add_argument('--suite', choices=['allow','deny','ask','large','lifecycle','parser','order','raw'], default='allow')
    args = parser.parse_args()
    if args.fixture:
        fixture(args.fixture)
        return
    args.output.mkdir(parents=True, exist_ok=False)
    spec = importlib.util.spec_from_file_location('owned_cleanup', CLEANUP)
    cleanup = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(cleanup)
    cleanup.enable_subreaper()
    lock = Path('/home/callen/.cache/hotaru-workstation.lock').open('a')
    fcntl.flock(lock, fcntl.LOCK_EX)
    processes, logs = [], []
    report = {'suite':args.suite, 'status':'running', 'scope':'unchanged GTK app, private software Sway; behavioral observations, no performance claim',
              'sources':{str(p):sha(p.read_bytes()) for p in [Path(__file__), CLEANUP, GHOSTTY, OBSERVER]}, 'cases':[]}
    temp = tempfile.TemporaryDirectory(prefix='osc52-')
    runtime = Path(temp.name)
    env = {k:v for k,v in os.environ.items() if not k.startswith(('SIDESTEP_','GDK_','GTK_','AIZOME_'))}
    for key in ['DISPLAY','WAYLAND_DISPLAY','SWAYSOCK','DBUS_SESSION_BUS_ADDRESS','FONTCONFIG_FILE','FONTCONFIG_PATH','LD_LIBRARY_PATH']:
        env.pop(key, None)
    for name in ['home','config','cache','data']:
        (runtime/name).mkdir()
    env.update(HOME=str(runtime/'home'), XDG_RUNTIME_DIR=str(runtime), XDG_CONFIG_HOME=str(runtime/'config'),
               XDG_CACHE_HOME=str(runtime/'cache'), XDG_DATA_HOME=str(runtime/'data'), XDG_CURRENT_DESKTOP='sway',
               XDG_SESSION_TYPE='wayland', WLR_BACKENDS='headless', WLR_RENDERER='pixman', WLR_LIBINPUT_NO_DEVICES='1',
               LIBGL_ALWAYS_SOFTWARE='1', GALLIUM_DRIVER='llvmpipe', LP_NUM_THREADS='4')
    mesa = Path('/nix/store/457mqa9dsvw9cnmi6pr06zjy1ib0iic4-mesa-26.2.5')
    env.update(__EGL_VENDOR_LIBRARY_FILENAMES=str(mesa/'share/glvnd/egl_vendor.d/50_mesa.json'),
               LIBGL_DRIVERS_PATH=str(mesa/'lib/dri'))

    def launch(name, command, stdin=None, pipes=False):
        log = (args.output/(name+'.log')).open('wb'); logs.append(log)
        p = subprocess.Popen(command, env=env, stdin=stdin if stdin is not None else subprocess.DEVNULL,
                             stdout=subprocess.PIPE if pipes else log, stderr=log, start_new_session=True, bufsize=0)
        processes.append(p)
        return p

    def paste(primary=False):
        command = ['wl-paste','--no-newline','--type','text/plain;charset=utf-8']
        if primary: command.append('--primary')
        try:
            result = subprocess.run(command, env=env, capture_output=True, timeout=3)
            return result.stdout, {'code':result.returncode,'stderr':result.stderr.decode(errors='replace')}
        except subprocess.TimeoutExpired:
            return b'', {'timeout':True}

    def seed(primary=False, data=b'STANDARD-SEED'):
        command = ['wl-copy','--foreground','--type','text/plain;charset=utf-8']
        if primary: command.append('--primary')
        p = launch('seed-'+str(len(processes)), command, stdin=subprocess.PIPE)
        p.stdin.write(data); p.stdin.close()
        wait_for('clipboard seed', lambda: paste(primary)[0] == data, 4)
        return p

    try:
        address = 'unix:path='+str(runtime/'dbus')
        (runtime/'dbus.conf').write_text('<busconfig><type>session</type><listen>'+address+'</listen><auth>EXTERNAL</auth><policy context="default"><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
        launch('dbus',['dbus-daemon','--nofork','--nopidfile','--config-file='+str(runtime/'dbus.conf')])
        wait_for('dbus',lambda:(runtime/'dbus').is_socket())
        env['DBUS_SESSION_BUS_ADDRESS']=address
        (runtime/'sway.conf').write_text('output HEADLESS-1 mode 1024x768@60Hz\nseat seat0 fallback true\nfocus_follows_mouse no\ndefault_border none\n')
        launch('sway',['sway','--unsupported-gpu','-c',str(runtime/'sway.conf')])
        display=wait_for('display',lambda:next((p for p in runtime.glob('wayland-*') if p.is_socket()),None))
        env['WAYLAND_DISPLAY']=display.name
        env['SWAYSOCK']=str(wait_for('sway',lambda:next(runtime.glob('sway-ipc.*.sock'),None)))
        report['private_environment']={k:env[k] for k in ['WAYLAND_DISPLAY','XDG_RUNTIME_DIR','WLR_BACKENDS','WLR_RENDERER']}
        observer=launch('keyboard',[str(OBSERVER)],stdin=subprocess.PIPE,pipes=True)
        report['keyboard_ready']=observer.stdout.readline().decode()
        def key(code):
            observer.stdin.write(f'key 1 {code}\n'.encode());observer.stdin.flush();time.sleep(.12)
        listener=socket.socket(socket.AF_UNIX); listener.bind(str(runtime/'fixture'));listener.listen(1);listener.settimeout(30)
        policy=args.suite if args.suite in ['ask','deny'] else 'allow'
        config='gtk-single-instance = false\nshell-integration = none\ncursor-style-blink = false\nconfirm-close-surface = false\nclipboard-read = '+policy+'\nclipboard-write = '+policy+'\n'
        (args.output/'ghostty.conf').write_text(config)
        app=launch('ghostty',[str(GHOSTTY),'--config-default-files=false','--config-file='+str(args.output/'ghostty.conf'),'-e',sys.executable,str(Path(__file__).resolve()),'--fixture',str(runtime/'fixture')])
        conn,_=listener.accept();conn.settimeout(25);listener.close();stream=conn.makefile('rwb',buffering=0)
        report['fixture']=json.loads(stream.readline())
        def focused():
            tree=json.loads(subprocess.check_output(['swaymsg','-t','get_tree','-r'],env=env,timeout=3))
            def walk(node):
                if node.get('app_id') == 'com.mitchellh.ghostty' and node.get('focused'):
                    return node
                return next((found for child in node.get('nodes',[])+node.get('floating_nodes',[]) if (found:=walk(child))),None)
            return walk(tree)
        report['focused_window']=wait_for('focused mapped Ghostty',focused)
        key(42)  # Shift creates a real focused-seat serial without PTY bytes.
        seed(); seed(True,b'PRIMARY-SEED'); report['primary_selection_available']=True

        def rpc(data=b'', seconds=.25, write_chunk=65536):
            conn.sendall(json.dumps({'data':base64.b64encode(data).decode(),'wait':seconds,'write_chunk':write_chunk}).encode()+b'\n')
            response=json.loads(stream.readline(24*1024*1024));raw=base64.b64decode(response.pop('reply'))
            return raw,response

        def observe(name,data,seconds=.25,check_clipboards=True,write_chunk=65536):
            reply,detail=rpc(data,seconds,write_chunk)
            row={'name':name,'input':data_record(data),'reply':data_record(reply),'transport':detail,'producer_write_chunk':write_chunk}
            if check_clipboards:
                standard,s=paste();primary,p=paste(True)
                row.update(standard=data_record(standard),primary=data_record(primary),standard_status=s,primary_status=p)
            report['cases'].append(row)
            (args.output/'result.json').write_text(json.dumps(report,indent=2)+'\n')
            return row

        def reset():
            # A distinct value proves each new owner has actually published.
            # Reusing the previous sentinel can pass the wait before wl-copy
            # has acquired ownership and race the subsequent OSC write.
            serial=len(processes)
            seed(data=f'STANDARD-SEED-{serial}'.encode())
            seed(True,f'PRIMARY-SEED-{serial}'.encode())
            time.sleep(.05)
            key(42)  # Fresh serial after the external owner changes selection.

        key(30)
        keyboard_reply,_=rpc()
        report['keyboard_probe']=data_record(keyboard_reply)
        if keyboard_reply != b'a':
            raise RuntimeError('focused keyboard serial qualification failed')
        if policy == 'allow':
            initial_read,_=rpc(b'\x1b]52;c;?\x1b\\')
            report['initial_external_read']=data_record(initial_read)
            if initial_read != b'\x1b]52;c;'+base64.b64encode(b'STANDARD-SEED')+b'\x1b\\':
                raise RuntimeError('external clipboard visibility qualification failed')

        if args.suite in ['allow','deny']:
            for kind in [b'',b'c',b'p',b's',b'x',b'0',b'cp',b'pc',b'cc',b'\xff',b'\xc3\xa9']:
                reset(); observe('write-kind-'+kind.hex(),b'\x1b]52;'+kind+b';YWJj\x1b\\')
                reset(); observe('read-kind-'+kind.hex(),b'\x1b]52;'+kind+b';?\x1b\\')
            for name,data in [('empty',b''),('one-byte',b'x'),('nul',b'a\x00b'),('invalid-utf8',b'a\xffb'),('utf8','雪α'.encode()),('embedded-control',b'a\x1bb\nb')]:
                reset();observe('write-'+name,b'\x1b]52;c;'+base64.b64encode(data)+b'\x1b\\')
                observe('read-own-'+name,b'\x1b]52;c;?\x1b\\')
            for payload in [b'?',b'',b'Zg=',b'Z g==',b'Zg===',b'AB==',b'_w==']:
                reset(); observe('payload-'+payload.hex(),b'\x1b]52;c;'+payload+b'\x07')
            for ending in [b'\x07',b'\x1b\\']:
                reset();observe('read-terminator-'+ending.hex(),b'\x1b]52;p;?'+ending)
            for cancel in [b'\x18',b'\x1a']:
                reset();observe('cancel-capture-'+cancel.hex(),b'\x1b]52;c;YWJj'+cancel+b'\x1b\\')
        elif args.suite == 'large':
            for kind,length in [(b'c',300*1024),(b'c',2*1024*1024),(b'c',8*1024*1024-4),(b'c',8*1024*1024-2),(b'',8*1024*1024-2),(b'c',8*1024*1024+4)]:
                reset();observe(f'capture-{kind.hex()}-{length}',b'\x1b]52;'+kind+b';'+b'Y'*length+b'\x1b\\',.5)
        elif args.suite == 'lifecycle':
            pending=b'HELD-EXTERNAL-READ'
            owner=seed(data=pending)
            key(42)
            time.sleep(.05)
            os.kill(owner.pid,signal.SIGSTOP)
            observe('stopped-source-pending',b'\x1b]52;c;?\x1b\\',check_clipboards=False)
            observe('stopped-source-dsr',b'\x1b[6n',check_clipboards=False)
            os.kill(owner.pid,signal.SIGCONT)
            observe('resumed-source-completion',seconds=.6,check_clipboards=False,data=b'')
            payload=b'L'*(2*1024*1024)
            key(42)
            large=observe('large-write-before-stalled-reader',b'\x1b]52;c;'+base64.b64encode(payload)+b'\x1b\\',seconds=.5)
            if large['standard']['sha256'] != sha(payload):
                raise RuntimeError('large global clipboard ownership qualification failed')
            slow=launch('stalled-reader',['wl-paste','--no-newline'],pipes=True)
            time.sleep(.15)
            if slow.poll() is not None:
                raise RuntimeError('reader did not remain stalled')
            _,before=rpc()
            command=['swaymsg','[app_id="com.mitchellh.ghostty"]','floating enable, resize set 800 px 500 px']
            resize=subprocess.run(command,env=env,capture_output=True,timeout=3)
            time.sleep(.15)
            _,after=rpc()
            report['cases'].append({'name':'stalled-reader-resize','before':before,'after':after,'resize_return':resize.returncode,
                                    'resize_stdout':resize.stdout.decode(errors='replace'),'reader_alive':slow.poll() is None})
            slow.stdout.close()
            with contextlib.suppress(subprocess.TimeoutExpired):slow.wait(timeout=2)
            observe('after-reader-pipe-close',b'\x1b[6n')
            owner=seed(data=b'CLOSE-WITH-READ-PENDING')
            key(42)
            time.sleep(.05)
            os.kill(owner.pid,signal.SIGSTOP)
            pending=observe('close-window-source-pending',b'\x1b]52;c;?\x1b\\',check_clipboards=False)
            if pending['reply']['length'] != 0:
                raise RuntimeError('close case did not hold a pending external read')
            subprocess.run(['swaymsg','[app_id="com.mitchellh.ghostty"]','kill'],env=env,capture_output=True,timeout=3)
            with contextlib.suppress(subprocess.TimeoutExpired):app.wait(timeout=3)
            report['cases'].append({'name':'closed-window-with-read-pending','app_return':app.poll()})
            os.kill(owner.pid,signal.SIGCONT)
        elif args.suite == 'parser':
            for prefix in [b'52',b'052',b'0052',b'+52',b' 52',b'52 ',b'52\x00']:
                reset();observe('stream-numeric-'+prefix.hex(),b'\x1b]'+prefix+b';c;YWJj\x1b\\')
            for body in [b'52;c;Y\x00WJj',b'52;c\x00;YWJj',b'52;\x00;YWJj',b'52;c;Y\x7fWJj',b'52;c;Y\x18WJj',b'52;c;Y\x1aWJj']:
                reset();observe('stream-control-'+body.hex(),b'\x1b]'+body+b'\x1b\\')
        elif args.suite == 'order':
            for chunk in [1,7,65536]:
                for count in [2,8]:
                    reset()
                    data=b''.join(b'\x1b]52;c;'+base64.b64encode(f'WRITE-{i}'.encode())+b'\x1b\\\x1b]52;c;?\x1b\\' for i in range(count))
                    observe(f'write-read-burst-{count}-{chunk}',data,seconds=.6,write_chunk=chunk)
            owner=seed(data=b'OLD-HELD-SOURCE');key(42)
            os.kill(owner.pid,signal.SIGSTOP)
            observe('replacement-read-pending',b'\x1b]52;c;?\x1b\\',check_clipboards=False)
            seed(data=b'NEW-SOURCE');key(42)
            observe('replacement-before-old-completion',b'',check_clipboards=False)
            os.kill(owner.pid,signal.SIGCONT)
            observe('replacement-after-old-completion',b'',seconds=.6,check_clipboards=False)
            owner=seed(data=b'DISCONNECTED-SOURCE');key(42)
            os.kill(owner.pid,signal.SIGSTOP)
            observe('disconnect-read-pending',b'\x1b]52;c;?\x1b\\',check_clipboards=False)
            os.kill(owner.pid,signal.SIGKILL);owner.wait(timeout=2)
            observe('disconnected-source-completion',b'',seconds=.6,check_clipboards=False)
        elif args.suite == 'raw':
            for raw in [b'\x00',b'\x80',b'\xff',b'\xc0\xaf',b'\xed\xa0\x80',b'\xf4\x90\x80\x80',b'\xe2\x82',b'\xe2\x82\xac',
                        b'a\x00b',b'\\\xff',b'\x7f',b'\x1b',b'\n',b'\r',b'\r\n',b'\x00valid']:
                reset();observe('raw-write-'+raw.hex(),b'\x1b]52;c;'+base64.b64encode(raw)+b'\x1b\\')
                observe('raw-own-read-'+raw.hex(),b'\x1b]52;c;?\x1b\\')
                seed(data=raw);key(42)
                observe('raw-foreign-read-'+raw.hex(),b'\x1b]52;c;?\x1b\\')
        else:
            for name,data in [('write-ask',b'\x1b]52;c;YWJj\x1b\\'),('read-ask',b'\x1b]52;c;?\x07')]:
                reset();observe(name+'-pending',data)
                time.sleep(.4)
                subprocess.run(['grim',str(args.output/(name+'-pending.png'))],env=env,check=True,timeout=4)
                key(1);observe(name+'-escape',b'')
            for name,data in [('write-allow',b'\x1b]52;c;YWJj\x1b\\'),('read-allow',b'\x1b]52;c;?\x07'),
                              ('write-nul-allow',b'\x1b]52;c;YQBi\x1b\\'),('write-invalid-allow',b'\x1b]52;c;Yf9i\x1b\\')]:
                reset();observe(name+'-pending',data)
                key(15);key(28)
                observe(name+'-tab-enter',b'')
            key(1) # Close any residual invalid-preview dialog before next case.
            drained,_=rpc();report['ask_cleanup_drain']=data_record(drained)
            reset();observe('read-preview-replacement-pending',b'\x1b]52;c;?\x1b\\')
            seed(data=b'NEW-AFTER-PREVIEW');key(42)
            key(15);key(28)
            observe('read-preview-replacement-approved',b'')
        observer.stdin.write(b'quit\n');observer.stdin.flush()
        report['app_alive']=app.poll() is None
        report['status']='observed'
        conn.close()
    except Exception as error:
        report.update(status='failed',error=repr(error))
        raise
    finally:
        for process in reversed(processes):
            if process.poll() is None:
                with contextlib.suppress(ProcessLookupError):os.killpg(process.pid,signal.SIGTERM)
        for process in reversed(processes):
            with contextlib.suppress(subprocess.TimeoutExpired):process.wait(timeout=2)
            if process.poll() is None:
                with contextlib.suppress(ProcessLookupError):os.killpg(process.pid,signal.SIGKILL)
                process.wait(timeout=2)
        report['forced_descendant_cleanup']=cleanup.cleanup_descendants()
        report['all_owned_processes_exited']=True
        report['process_returns']=[p.returncode for p in processes]
        (args.output/'result.json').write_text(json.dumps(report,indent=2)+'\n')
        for log in logs:log.close()
        temp.cleanup()


if __name__ == '__main__':
    main()
