#!/usr/bin/env python3
"""Assemble unchanged pinned source for the braille/box/Powerline oracle."""
import argparse, hashlib, json, pathlib, shutil, subprocess, sys
root=pathlib.Path(__file__).resolve().parent.parent
p=argparse.ArgumentParser();p.add_argument('--reference',type=pathlib.Path,required=True);p.add_argument('--zig',type=pathlib.Path,required=True);p.add_argument('--output',type=pathlib.Path,required=True);a=p.parse_args()
checks={'powerline.zig':'91f897300bb8324c7f05da713d453d93624c4de89a9149e386bc7ace46c2a072','braille.zig':'8093913ed854b6d46a8b627c465d7f1ac7b2deedd35c208603f06c05bf9a6a8c'}
for name,sha in checks.items():
    assert hashlib.sha256((a.reference/'src/font/sprite/draw'/name).read_bytes()).hexdigest()==sha, name
r=subprocess.run([sys.executable,str(root/'tools/prepare-oracle.py'),'--reference',str(a.reference),'--zig',str(a.zig),'--output',str(a.output)],check=True,capture_output=True,text=True)
report=json.loads(r.stdout)
for name in checks:shutil.copyfile(a.reference/'src/font/sprite/draw'/name,a.output/'src/font/sprite/draw'/name)
shutil.copyfile(root/'tools/oracle-braille.zig',a.output/'oracle.zig')
report['additional_sources']=checks
report['adapter_sha256']=hashlib.sha256((a.output/'oracle.zig').read_bytes()).hexdigest()
command=json.loads((a.output/'build-command.json').read_text())
fast=[x.replace('ReleaseSafe','ReleaseFast').replace('libsprites.so','libsprites-fast.so') for x in command]
(a.output/'build-command-fast.json').write_text(json.dumps(fast,indent=2)+'\n')
report['build_command_fast']=fast
(a.output/'preparation.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
