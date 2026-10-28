import fcntl, json, os, subprocess, hashlib
from pathlib import Path
root=Path(__file__).resolve().parent.parent
build=json.loads((root/'evidence/native-build.json').read_text())
env=dict(os.environ);env.update(build['environment'])
with (Path.home()/'.cache/hotaru-workstation.lock').open('a') as lock:
 fcntl.flock(lock,fcntl.LOCK_EX)
 subprocess.run(['cargo','+1.95.0','test','--locked','--release','--workspace'],cwd=root,env=env,check=True)
 common=['--library','/home/callen/work/ghostty-reference-20261028/zig-out/lib/libghostty-vt.so','--baseline-metadata','/home/callen/work/hotaru-unicode-mode-20261028/reference-build.json','--replay',str(root/'target/release/hotaru-replay')]
 for tool,name in [('differential.py','reference-previous'),('vt-controls-oracle.py','reference-vt'),('ascii-subrun-oracle.py','reference-ascii-subruns')]:
  subprocess.run(['python3',str(root/'tools'/tool),*common,'--report',str(root/'evidence'/(name+'.json'))],cwd=root,check=True)
 subprocess.run(['python3',str(root/'tools/cursor-oracle.py'),'--output',str(root/'evidence/reference-cursor.json')],cwd=root,check=True)
 assert json.loads((root/'evidence/reference-cursor.json').read_text())==json.loads((root/'upstream/cursor-fixtures.json').read_text())
 assert hashlib.sha256((root/'target/release/hotaru-replay').read_bytes()).hexdigest()==build['binary_sha256']
 print('native binary unchanged after full release and actual-library gates')
