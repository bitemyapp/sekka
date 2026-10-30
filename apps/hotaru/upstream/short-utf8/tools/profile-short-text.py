#!/usr/bin/env python3
"""Compare diagnostic instruction counts; these are never native timings."""
import datetime, fcntl, hashlib, json, re, subprocess
from pathlib import Path
base=Path('/home/callen/work/hotaru-short-text-baseline-20261028')
candidate=Path('/home/callen/work/hotaru-short-text-candidate-20261028')
output=candidate/'profiles'
output.mkdir(exist_ok=True)
corpus=Path('/home/callen/work/hotaru-rep-last-cell-20261028/measurements')
valgrind=Path('/nix/store/4298593522xgrr28874w20cvvs742r51-valgrind-3.27.1/bin/valgrind')
def digest(path): return hashlib.sha256(path.read_bytes()).hexdigest()
def sources(root):
    files=[root/'Cargo.toml',root/'Cargo.lock']
    for dirname in ['crates','.cargo','vendor']:
        if (root/dirname).exists(): files.extend(p for p in (root/dirname).rglob('*') if p.is_file() and not {'target','.git'}.intersection(p.parts))
    return {str(p.relative_to(root)):digest(p) for p in sorted(files)}
inputs={name:corpus/f'{name}.vt' for name in ['ascii','unicode','styled','cursor']}
for name,pattern in [('short-invalid-leads',b'\x80'*53+b'\r\n'),('short-invalid-suffix',b'\xe9\x9b\xaa'+b'a'*48+b'\xf0\x9f'+b'\r\n')]:
    p=output/f'{name}.vt'
    p.write_bytes(pattern*((262144+len(pattern)-1)//len(pattern)))
    inputs[name]=p
report={'schema':1,'scope':'Callgrind Ir diagnostic, x86-64-v3/debug2; no elapsed-time claim','recorded_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'valgrind':str(valgrind),'valgrind_sha256':digest(valgrind),'builds':{},'runs':[]}
with (Path.home()/'.cache/hotaru-workstation.lock').open('a') as lock:
    fcntl.flock(lock,fcntl.LOCK_EX)
    for label,root in [('baseline',base),('candidate',candidate)]:
        build=json.loads((root/'diagnostic-build.json').read_text())
        assert sources(root)==build['sources']
        binary=root/'target/release/hotaru-replay'
        assert digest(binary)==build['binary_sha256']
        report['builds'][label]={'source_sha256':build['source_sha256'],'binary_sha256':build['binary_sha256'],'metadata':str(root/'diagnostic-build.json'),'metadata_sha256':digest(root/'diagnostic-build.json')}
        for name,input_path in inputs.items():
            prefix=output/f'{label}-{name}'
            command=[str(valgrind),'--tool=callgrind',f'--callgrind-out-file={prefix}.callgrind',str(binary),'--data',str(input_path),'--rows','12','--cols','120','--chunk','65536','--grapheme-width','unicode','--bench-json','--iterations','1']
            with Path(f'{prefix}.json').open('w') as stdout,Path(f'{prefix}.log').open('w') as stderr:
                subprocess.run(command,stdout=stdout,stderr=stderr,check=True)
            text=Path(f'{prefix}.log').read_text()
            count=int(re.search(r'Collected : ([0-9]+)',text).group(1))
            report['runs'].append({'build':label,'input':name,'input_path':str(input_path),'input_sha256':digest(input_path),'input_bytes':input_path.stat().st_size,'command':command,'instructions':count,'profile':str(prefix)+'.callgrind','profile_sha256':digest(Path(str(prefix)+'.callgrind'))})
        assert sources(root)==build['sources']
        assert digest(binary)==build['binary_sha256']
for name in inputs:
    rows=[r for r in report['runs'] if r['input']==name]
    a,b=rows
    report.setdefault('instruction_changes',{})[name]={'baseline':a['instructions'],'candidate':b['instructions'],'fraction':b['instructions']/a['instructions']-1}
(output/'summary.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(report['instruction_changes'],indent=2))
