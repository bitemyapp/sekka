#!/usr/bin/env python3
"""Isolate a same-pin x86-64-v3 reference build for instruction attribution."""
from pathlib import Path
import datetime,fcntl,hashlib,json,shutil,subprocess
ROOT=Path(__file__).resolve().parent.parent
SOURCE=Path('/home/callen/work/ghostty-reference-20261028')
REFERENCE=ROOT/'reference'
ZIG=Path('/home/callen/.cache/hotaru-reference/zig-x86_64-linux-0.17.0/zig')
PIN='6ba144158577d033ccb6ed4b574e4013957c9161'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def output(args,**kw):return subprocess.check_output(args,text=True,**kw).strip()
with (Path.home()/'.cache/hotaru-workstation.lock').open('a') as lock:
 fcntl.flock(lock,fcntl.LOCK_EX)
 native_library=SOURCE/'zig-out/lib/libghostty-vt.so';native_hash=sha(native_library)
 assert native_hash=='70e1bdcf62801b735bb268c38c1e65fa727debff73d89ef1d3ed9fb5e23e7639'
 if not REFERENCE.exists():subprocess.run(['git','clone','--shared','--no-checkout',str(SOURCE),str(REFERENCE)],check=True)
 subprocess.run(['git','-C',str(REFERENCE),'checkout','--detach',PIN],check=True)
 assert not output(['git','-C',str(REFERENCE),'status','--porcelain','--untracked-files=no'])
 names=subprocess.check_output(['git','-C',str(REFERENCE),'ls-files','-z']).decode().split('\0')
 sources={name:sha(REFERENCE/name) for name in names if name and (REFERENCE/name).is_file()}
 command=[str(ZIG),'build','-Demit-bench=true','-Demit-lib-vt=true','-Demit-exe=false','-Dapp-runtime=none','-Doptimize=ReleaseFast','-Demit-docs=false','-Demit-terminfo=false','-Dcpu=x86_64_v3','-Dstrip=false','-j8']
 subprocess.run(command,cwd=REFERENCE,check=True)
 original_adapter=Path('/home/callen/work/hotaru-unicode-mode-20261028/tools/reference-replay.c')
 adapter=ROOT/'evidence/reference-replay.c';shutil.copy2(original_adapter,adapter)
 assert sha(adapter)=='737f7c40816cf58e5c8bd4e92989f810b7de3c0775ec705f5c70f983a27a7f77'
 library=REFERENCE/'zig-out/lib/libghostty-vt.so';binary=ROOT/'evidence/ghostty-reference-replay-v3'
 cc=Path(shutil.which('cc')).resolve()
 compile_command=[str(cc),'-std=c11','-O3','-g','-march=x86-64-v3','-I',str(REFERENCE/'include'),str(adapter),str(library),'-Wl,-rpath,'+str(library.parent),'-o',str(binary)]
 subprocess.run(compile_command,check=True)
 assert sha(native_library)==native_hash
 assert not output(['git','-C',str(REFERENCE),'status','--porcelain','--untracked-files=no'])
 assert sources=={name:sha(REFERENCE/name) for name in names if name and (REFERENCE/name).is_file()}
 report={'schema':1,'build_confirmed':True,'scope':'Diagnostic attribution only, x86-64-v3/ReleaseFast/debug retained. Not the native timed reference. Original pinned library untouched.','recorded_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'revision':PIN,'source_sha256':hashlib.sha256(json.dumps(sources,sort_keys=True,separators=(',',':')).encode()).hexdigest(),'sources':sources,'zig':output([str(ZIG),'version']),'zig_sha256':sha(ZIG),'command':command,'library':str(library),'library_sha256':sha(library),'binary':str(binary),'binary_sha256':sha(binary),'adapter_source_sha256':sha(adapter),'adapter_command':compile_command,'cc':output([str(cc),'--version']),'cc_sha256':sha(cc),'native_library_sha256_unchanged':native_hash}
 (ROOT/'evidence/reference-diagnostic-build.json').write_text(json.dumps(report,indent=2)+'\n')
 print(json.dumps({k:report[k] for k in ('source_sha256','library_sha256','binary_sha256','scope')}))
