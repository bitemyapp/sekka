#!/usr/bin/env python3
"""Exact native Alpha8 output against unchanged pinned Ghostty sprite raster.

Run under the shared workstation lock. Only a disposable private compositor
is used. This verifies software presentation, not GPU/frame-time equivalence.
Usage: sprites.py BINARY GHOSTTY_SPRITE_LIBRARY EVIDENCE_DIRECTORY
"""
import ctypes
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import time

from PIL import Image
from smoke import wait

CPS = [cp for cp in range(0x2500, 0x25a0) if not 0x256d <= cp <= 0x2573]


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def compare(png, metrics, raster):
    image = Image.open(png).convert('RGB')
    pixels = image.load()
    points = [(x, y) for y in range(image.height) for x in range(image.width)
              if pixels[x, y] == (255, 0, 255)]
    cw, ch, thickness = (metrics[key] for key in ('cell_width_px', 'cell_height_px', 'box_thickness_px'))
    assert len(points) == cw * ch, ('anchor area', len(points), cw * ch)
    ox, oy = min(x for x, y in points), min(y for x, y in points)
    assert max(x for x, y in points) == ox + cw - 1
    assert max(y for x, y in points) == oy + ch - 1
    width, height = cw + 2 * (cw // 4), ch + 2 * (ch // 4)
    data = (ctypes.c_ubyte * (width * height))()
    compared = 0
    for index, cp in enumerate(CPS):
        row, col = 2 + 3 * (index // 17), 2 + 3 * (index % 17)
        code = raster(cp, cw, ch, thickness, data, len(data))
        assert code == 0, (hex(cp), code)
        x0, y0 = ox + col * cw - cw // 4, oy + row * ch - ch // 4
        actual = image.crop((x0, y0, x0 + width, y0 + height)).tobytes()
        expected = bytes(channel for alpha in data for channel in (alpha, alpha, alpha))
        if actual != expected:
            mismatch = next(i for i, (a, e) in enumerate(zip(actual, expected)) if a != e)
            raise AssertionError((hex(cp), (cw, ch, thickness), 'byte', mismatch,
                                  'actual', actual[mismatch], 'expected', expected[mismatch]))
        compared += len(actual)
    return {'glyphs': len(CPS), 'compared_rgb_bytes': compared, 'origin_px': [ox, oy],
            'capture_sha256': sha(png), 'size_px': list(image.size)}


def run(binary, library, evidence):
    evidence.mkdir(parents=True, exist_ok=True)
    oracle = ctypes.CDLL(str(library))
    raster = oracle.sprite_raster
    raster.argtypes = [ctypes.c_uint32] * 4 + [ctypes.POINTER(ctypes.c_ubyte), ctypes.c_size_t]
    raster.restype = ctypes.c_int
    runtime = Path(tempfile.mkdtemp(prefix='hotaru-sprites-wayland-'))
    os.chmod(runtime, 0o700)
    env = dict(os.environ)
    for key in ['DISPLAY', 'WAYLAND_DISPLAY', 'SWAYSOCK', 'AIZOME_SHELL_SOCKET', 'AIZOME_SOCKET', 'SIDESTEP_BACKEND']:
        env.pop(key, None)
    env.update(XDG_RUNTIME_DIR=str(runtime), XDG_CONFIG_HOME=str(runtime / 'config'),
               WLR_BACKENDS='headless', WLR_RENDERER='pixman', WLR_LIBINPUT_NO_DEVICES='1',
               XDG_CURRENT_DESKTOP='sway', XDG_SESSION_TYPE='wayland',
               RUST_LOG='warn,aizome_kit::fonts=debug,aizome_hotaru=debug')
    config = runtime / 'sway.conf'
    config.write_text('output HEADLESS-1 mode 1920x1900\ndefault_border none\nseat seat0 fallback true\n')
    fixture = runtime / 'fixture.py'
    fixture.write_text('''import fcntl,json,os,pathlib,struct,sys,termios,time
path=pathlib.Path(sys.argv[1])
while True:
    rows,cols,_,_=struct.unpack('HHHH',fcntl.ioctl(0,termios.TIOCGWINSZ,bytes(8)))
    if rows>=31 and cols>=52:break
    time.sleep(.02)
cp=[c for c in range(0x2500,0x25a0) if not 0x256d<=c<=0x2573]
payload='\\x1b[?25l\\x1b]11;#000000\\x07\\x1b[2J\\x1b[H\\x1b[38;2;255;0;255m█'
payload+='\\x1b[38;2;255;255;255m\\x1b]8;id=sprite-fixture;https://example.test/sprites\\x1b\\\\'
for i,c in enumerate(cp):
    payload+=f'\\x1b[{3+3*(i//17)};{3+3*(i%17)}H'+chr(c)
payload+='\\x1b]8;;\\x1b\\\\'
payload+='\\x1b[31;1H\\x1b[1;3;4;9;38;2;0;255;0mAB┼CD╭─\\u0301\\x1b[0m'
os.write(1,payload.encode())
while True:
    size=struct.unpack('HHHH',fcntl.ioctl(0,termios.TIOCGWINSZ,bytes(8)))
    tmp=path.with_suffix('.tmp');tmp.write_text(json.dumps(size));tmp.replace(path)
    time.sleep(.02)
''')
    processes, files = [], []
    result = {'schema': 1, 'binary': str(binary), 'binary_sha256': sha(binary),
              'oracle_library': str(library), 'oracle_sha256': sha(library),
              'upstream_pin': '6ba144158577d033ccb6ed4b574e4013957c9161',
              'scope': 'exact actual private Wayland software capture of 153 qualified sprites; no GPU or timing claim',
              'fixture_sha256': sha(fixture), 'compositor_config': config.read_text(), 'scales': []}
    try:
        log = open(evidence / 'sway.log', 'wb'); files.append(log)
        processes.append(subprocess.Popen(['sway', '--unsupported-gpu', '-c', str(config)], env=env,
                                          stdout=log, stderr=subprocess.STDOUT))
        display = wait('private Wayland socket', lambda: next((p for p in runtime.glob('wayland-*') if p.is_socket()), None))
        socket = wait('private Sway socket', lambda: next(runtime.glob('sway-ipc.*.sock'), None))
        env.update(WAYLAND_DISPLAY=display.name, SWAYSOCK=str(socket))
        log = open(evidence / 'hotaru.log', 'wb'); files.append(log)
        processes.append(subprocess.Popen([str(binary), '--', sys.executable, str(fixture), str(evidence / 'pty.json')],
                                          env=env, stdout=log, stderr=subprocess.STDOUT))
        for index, scale in enumerate([1.0, 1.25, 2.0, 1.0]):
            subprocess.run(['swaymsg', '-s', str(socket), f'output HEADLESS-1 scale {scale}'], env=env,
                           check=True, stdout=subprocess.DEVNULL, timeout=3)
            def metrics():
                matches = re.findall(r'terminal font "([^"]+)", scale ([0-9.]+), grid ([0-9]+)x([0-9]+) px,.*sprite thickness ([0-9]+) px \(estimated (true|false)\)',
                                     (evidence / 'hotaru.log').read_text(errors='replace'))
                if not matches or float(matches[-1][1]) != scale:
                    return None
                family, _, cw, ch, thickness, estimated = matches[-1]
                try:
                    rows, cols, width, height = json.loads((evidence / 'pty.json').read_text())
                except (OSError, json.JSONDecodeError):
                    return None
                if rows < 31 or cols < 52 or width != cols * int(cw) or height != rows * int(ch):
                    return None
                return dict(scale=scale, family=family, cell_width_px=int(cw), cell_height_px=int(ch),
                            box_thickness_px=int(thickness), thickness_estimated=estimated == 'true',
                            rows=rows, cols=cols, pty_width_px=width, pty_height_px=height)
            value = wait(f'physical metrics at {scale}', metrics)
            tree=json.loads(subprocess.check_output(['swaymsg','-s',str(socket),'-t','get_tree'],env=env,text=True))
            def clients(node):
                if node.get('app_id')=='org.aizome.Hotaru':yield node
                for child in node.get('nodes',[])+node.get('floating_nodes',[]):yield from clients(child)
            value['client_geometry']=[{key:node.get(key) for key in ('app_id','rect','window_rect','deco_rect','border','current_border_width')} for node in clients(tree)]
            value['outputs']=json.loads(subprocess.check_output(['swaymsg','-s',str(socket),'-t','get_outputs'],env=env,text=True))
            # Readiness is the complete actual pixel comparison, never a PTY
            # acknowledgement or merely waiting a presumed frame duration.
            deadline = time.monotonic() + 10
            png = evidence / f'scale-{index}-{scale}.png'
            while True:
                subprocess.run(['grim', str(png)], env=env, check=True, timeout=3)
                try:
                    value.update(compare(png, value, raster))
                    break
                except AssertionError:
                    if time.monotonic() >= deadline:
                        raise
                    time.sleep(.05)
            result['scales'].append(value)
        result['status'] = 'passed'
    except Exception as error:
        result.update(status='failed', error=repr(error))
        raise
    finally:
        for process in reversed(processes):
            if process.poll() is None:
                process.terminate()
                try: process.wait(timeout=3)
                except subprocess.TimeoutExpired: process.kill(); process.wait(timeout=3)
        for file in files: file.close()
        shutil.copy2(fixture, evidence / 'fixture.py')
        (evidence / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
        shutil.rmtree(runtime)
    print(json.dumps(result, indent=2))


if __name__ == '__main__':
    run(*(Path(path).resolve() for path in sys.argv[1:4]))
