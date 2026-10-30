"""Verify and summarize the frozen core qualification; never edits product inputs."""
from pathlib import Path
import hashlib, importlib.util, json, re, datetime
root=Path(__file__).resolve().parents[2]
out=root/'artifacts/qualification'
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def read(name):return json.loads((out/name).read_text())
start=read('snapshot.json');build=read('release-build.json')
spec=importlib.util.spec_from_file_location('build',root/'tools/build-candidate.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
assert module.sources(root)==start['sources']==build['sources'],'compiled source changed'
assert start['source_sha256']==build['source_sha256']
assert sha(Path(build['binary']))==build['binary_sha256']
logs={}
for name in ['debug-clippy.log','release-tests.log']:
 text=(out/name).read_text();results=[tuple(map(int,m))for m in re.findall(r'test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;',text)]
 assert results and all(failed==0 for _,failed,_ in results),name
 assert 'error:'not in text,name
 logs[name]={'passed':sum(p for p,_,_ in results),'failed':0,'ignored':sum(i for _,_,i in results),'sha256':sha(out/name)}
 assert logs[name]['ignored']==2,name
references={}
for name,expected in [('previous-reference.json',444),('vt-controls-reference.json',3462)]:
 r=read(name);assert len(r['cases'])==expected,(name,len(r['cases']))
 assert all(c['status']=='matched'for c in r['cases']),name
 assert r['candidate_sha256']==build['binary_sha256']
 assert sha(Path(r['library']))==r['library_sha256']
 references[name]={'comparisons':expected,'matched':expected,'sha256':sha(out/name),'scope':r['compared'],'excluded':r['not_compared']}
cursor=read('cursor-reference.json');stored=json.loads((root/'upstream/cursor-fixtures.json').read_text())
assert cursor['cases']==stored['cases'],'fresh cursor library observations differ from bundled fixtures'
assert cursor['upstream_revision']==stored['upstream_revision']
assert cursor['library_sha256']==stored['library_sha256']
assert len(cursor['cases'])==756
references['cursor-reference.json']={'fresh_cases':756,'chunks':[1,7,65536],'comparisons':2268,'matched':2268,'method':'Fresh unchanged-library case objects equal bundled fixture case objects exactly; debug and release cursor integration tests replay all cases at each chunk size.','scope':cursor['scope'],'sha256':sha(out/'cursor-reference.json')}
ignored=[{'test':'hotaru-io::shell::actual_shell_startup','reason':'Requires explicit Bash/Zsh/Fish/ncurses disposable-home run; not executed in this core qualification.'},{'test':'hotaru-io::shell::shell_child','reason':'Isolated subprocess helper, intentionally not run directly.'}]
report={'schema':1,'recorded_at':datetime.datetime.now(datetime.timezone.utc).isoformat(),'status':'passed','source_sha256':build['source_sha256'],'source_scope':'Cargo files, crates, .cargo and vendor; all recorded files unchanged before/after the qualification.','binary_sha256':build['binary_sha256'],'base_revision':start['base_revision'],'workspace':str(root),'commands':['cargo +1.95.0 clean -p hotaru-core -p hotaru-io -p hotaru-replay','cargo +1.95.0 test --workspace --locked','cargo +1.95.0 clippy --workspace --all-targets --locked -- -D warnings','RUSTFLAGS="-C target-cpu=native" CARGO_PROFILE_RELEASE_OPT_LEVEL=3 CARGO_PROFILE_RELEASE_LTO=thin CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_PROFILE_RELEASE_DEBUG=0 CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=false CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=false cargo +1.95.0 test --workspace --locked --release','python3 tools/build-candidate.py --workspace "$PWD" --revision <recorded in release-build.json> --metadata artifacts/qualification/release-build.json','python3 tools/differential.py --library /home/callen/work/ghostty-reference-20261028/zig-out/lib/libghostty-vt.so --replay target/release/hotaru-replay --report artifacts/qualification/previous-reference.json','python3 tools/vt-controls-oracle.py --library /home/callen/work/ghostty-reference-20261028/zig-out/lib/libghostty-vt.so --replay target/release/hotaru-replay --report artifacts/qualification/vt-controls-reference.json','python3 tools/cursor-oracle.py --root /home/callen/work/ghostty-reference-20261028 --output artifacts/qualification/cursor-reference.json'], 'tests':logs,'ignored_tests':ignored,'references':references,'performance':'Not measured: unrelated external compiler activity. Native application, Kit, packaged tests and Wayland behavior are separate qualifications.','tools_sha256':{str(p.relative_to(root)):sha(p)for p in [root/'tools/build-candidate.py',root/'tools/differential.py',root/'tools/vt-controls-oracle.py',root/'tools/vt-controls-review.py',root/'tools/cursor-oracle.py',Path(__file__)]},'artifacts':{p.name:sha(p)for p in sorted(out.iterdir())if p.is_file()and p.name!='summary.json'}}
(out/'summary.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({'source_sha256':build['source_sha256'],'binary_sha256':build['binary_sha256'],'tests':logs,'reference_comparisons':444+3462+2268}))
