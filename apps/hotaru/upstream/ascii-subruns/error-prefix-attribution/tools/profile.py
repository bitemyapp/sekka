"""Bounded legacy-mode instruction attribution, not native wall-clock timing."""
from pathlib import Path
import fcntl,hashlib,json,re,subprocess

ROOT=Path(__file__).resolve().parent.parent
BASE=Path('/home/callen/work/hotaru-malformed-profile-20261028')
NATIVE=Path('/home/callen/work/hotaru-error-ascii-prefix-20261028')
NATIVE_BASE=Path('/home/callen/work/hotaru-ascii-cell-store-20261028')
VG=Path('/nix/store/4298593522xgrr28874w20cvvs742r51-valgrind-3.27.1/bin')
OUT=ROOT/'evidence/profiles'

def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def verify(root,build):
    for name,expected in build['sources'].items():assert sha(root/name)==expected,name
    assert sha(Path(build['binary']))==build['binary_sha256']
def dump_functions(name,binary):
    symbols=subprocess.check_output(['nm','-S','-C',str(binary)],text=True)
    result={}
    for line in symbols.splitlines():
        parts=line.split(None,3)
        if len(parts)!=4:continue
        address,size,kind,symbol=parts
        if symbol not in {'hotaru_core::Terminal::feed_inner','hotaru_core::screen::State::print_utf8','hotaru_core::screen::State::print_ascii','hotaru_core::print_prefix_before_error'}:continue
        path=OUT/(name+'-'+symbol.rsplit('::',1)[1]+'.asm')
        command=['objdump','-d','-C','--no-show-raw-insn',f'--start-address=0x{address}',f'--stop-address=0x{int(address,16)+int(size,16):x}',str(binary)]
        with path.open('w') as output:subprocess.run(command,stdout=output,check=True)
        result[symbol]={'address':address,'bytes':int(size,16),'assembly':str(path),'sha256':sha(path),'command':command}
    return result

with (Path.home()/'.cache/hotaru-workstation.lock').open('a') as lock:
    fcntl.flock(lock,fcntl.LOCK_EX)
    OUT.mkdir(parents=True,exist_ok=True)
    roots={'baseline':BASE,'candidate':ROOT}
    builds={'baseline':json.loads((BASE/'evidence/diagnostic-build.json').read_text()),'candidate':json.loads((ROOT/'evidence/diagnostic-build.json').read_text())}
    for name,root in roots.items():verify(root,builds[name])
    for key in ['rustc_sha256','toolchain']:
        assert builds['baseline'][key]==builds['candidate'][key]
    for key,value in builds['baseline']['environment'].items():
        if key!='CARGO_TARGET_DIR':assert builds['candidate']['environment'][key]==value,key
    report={'schema':1,'scope':'Legacy mode only; whole-process Callgrind instruction counts, including setup/teardown; no wall-time or native-ISA parity claim. Input sizes bounded for attribution, not identical to every native timing case.','builds':builds,'cases':[],'native_disassembly':{}}
    for name,root in [('baseline',NATIVE_BASE),('candidate',NATIVE)]:
        binary=root/'target/release/hotaru-replay'
        report['native_disassembly'][name]={'binary_sha256':sha(binary),'functions':dump_functions(name,binary)}
    manifest=json.loads((NATIVE/'native-thirty-two/summary.json').read_text())
    specs={'short-invalid-leads':262185,'unicode':262144,'long-dense-error-prefix':49156}
    for label,limit in specs.items():
        pattern=bytes.fromhex(manifest['input_patterns_hex'][label])
        data=pattern*max(1,limit//len(pattern))
        path=OUT/(label+'.vt');path.write_bytes(data)
        entry={'input':label,'pattern_hex':pattern.hex(),'bytes':len(data),'sha256':sha(path),'engines':{}}
        states={}
        for name,build in builds.items():
            binary=Path(build['binary'])
            args=[str(binary),'--data',str(path),'--rows','12','--cols','120','--chunk','65536','--grapheme-width','legacy']
            states[name]=json.loads(subprocess.check_output([*args,'--format','json'],text=True))
            raw=OUT/(name+'-'+label+'.callgrind')
            command=['taskset','-c','8',str(VG/'valgrind'),'--tool=callgrind',f'--callgrind-out-file={raw}',*args,'--bench-json','--iterations','1']
            with (OUT/(name+'-'+label+'.log')).open('w') as err,(OUT/(name+'-'+label+'.json')).open('w') as out:
                subprocess.run(command,stderr=err,stdout=out,check=True)
            annotations={}
            for inclusive in ['no','yes']:
                annotated=OUT/(name+'-'+label+'.inclusive-'+inclusive+'.txt')
                cmd=[str(VG/'callgrind_annotate'),'--auto=no','--threshold=100','--show=Ir','--inclusive='+inclusive,str(raw)]
                with annotated.open('w') as out:subprocess.run(cmd,stdout=out,check=True)
                annotations[inclusive]={'path':str(annotated),'sha256':sha(annotated),'command':cmd}
            total=int(re.search(r'^summary: (\d+)',raw.read_text(),re.M).group(1))
            entry['engines'][name]={'command':command,'instructions_whole_process':total,'raw_sha256':sha(raw),'annotations':annotations}
        assert states['baseline']==states['candidate'],label
        entry['full_state_equal']=True
        report['cases'].append(entry)
    for name,root in roots.items():verify(root,builds[name])
    report['status']='completed'
    (ROOT/'evidence/summary.json').write_text(json.dumps(report,indent=2)+'\n')
    print([(c['input'],{n:e['instructions_whole_process'] for n,e in c['engines'].items()}) for c in report['cases']])
