#!/usr/bin/env python3
"""Build the same unchanged actual raster sources in ReleaseFast for all-family native probes."""
import hashlib,json,pathlib,subprocess
root=pathlib.Path('/home/callen/work/hotaru-sprites-geometric-20261028')
folder=root/'oracle-expanded'
command=json.loads((folder/'build-command.json').read_text())
command=['ReleaseFast' if x=='ReleaseSafe' else '-femit-bin=libsprites-fast.so' if x=='-femit-bin=libsprites.so' else x for x in command]
subprocess.run(command,cwd=folder,check=True)
report={'command':command,'library':str(folder/'libsprites-fast.so'),'sha256':hashlib.sha256((folder/'libsprites-fast.so').read_bytes()).hexdigest(),'adapter_sha256':hashlib.sha256((folder/'oracle.zig').read_bytes()).hexdigest(),'source_note':'Same unchanged pinned Ghostty/z2d sources as ReleaseSafe oracle; ReleaseFast is required for the documented upstream Braille residual assertion defect.'}
(root/'artifacts/fast-oracle.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report))
