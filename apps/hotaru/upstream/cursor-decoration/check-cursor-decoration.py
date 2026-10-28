import hashlib,json,os,pathlib,subprocess
root=pathlib.Path(__file__).resolve().parent.parent
source=root/'desktop/aizome/apps/hotaru/src/terminal_view.rs'
green=source.read_text();baseline=(root/'artifacts/baseline-terminal_view.rs').read_text()
a=green.index('    #[test]\n    fn block_cursor_keeps_and_tints_decorations');b=green.index('    #[test]\n    fn block_background_precedes_neighbor_ink',a)
red=baseline.replace('    #[test]\n    fn block_background_precedes_neighbor_ink',green[a:b]+'    #[test]\n    fn block_background_precedes_neighbor_ink')
env=os.environ.copy();env.update(AIZOME_FONT_DIR='/nix/store/b9vp4qd3gz9vmsl20grvd80hniirn5n5-aizome-fonts',HOTARU_FONT_DIR='/nix/store/1rv68ixfzh8ki0jmanx7y846h2qdvzbg-hotaru-fonts',CARGO_TARGET_DIR='/home/callen/.cache/hotaru-aizome-target')
base=['/home/callen/.cargo/bin/cargo','+1.95.0'];manifest=['--locked','--manifest-path',str(root/'desktop/aizome/Cargo.toml'),'-p','aizome-hotaru']
report={'baseline_sha256':hashlib.sha256(baseline.encode()).hexdigest(),'candidate_sha256':hashlib.sha256(green.encode()).hexdigest(),'checks':[]}
def run(name,args):
 with (root/'artifacts'/f'{name}.log').open('w') as out:r=subprocess.run(base+args,env=env,stdout=out,stderr=subprocess.STDOUT)
 report['checks'].append({'name':name,'command':base+args,'returncode':r.returncode});return r.returncode
try:
 source.write_text(red)
 assert run('red',['test',*manifest,'block_cursor_keeps_and_tints_decorations','--','--nocapture'])!=0,'red unexpectedly passed'
 log=(root/'artifacts/red.log').read_text();assert 'block_cursor_keeps_and_tints_decorations_in_both_paint_paths' in log and 'assertion `left == right` failed' in log,log
finally:source.write_text(green)
assert run('green',['test',*manifest])==0
assert run('clippy',['clippy',*manifest,'--all-targets','--no-deps','--','-D','warnings'])==0
report['status']='passed';(root/'artifacts/qualification.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
