import hashlib,json,pathlib
root=pathlib.Path(__file__).resolve().parents[2]
old=root/'artifacts/geometric/baseline/source-manifest.json'
paths={root/p for p in json.loads(old.read_text())}
for directory in ['desktop/aizome/apps/hotaru','desktop/aizome/crates/aizome-kit','apps/hotaru/crates/hotaru-core','apps/hotaru/crates/hotaru-io']:
 paths.update(p for p in (root/directory).rglob('*') if p.is_file() and p.suffix in {'.rs','.py','.c','.toml'})
paths.update(p for p in (root/'apps/hotaru/crates/hotaru-sprites').rglob('*') if p.is_file())
missing=[str(p.relative_to(root)) for p in sorted(paths) if not p.is_file()]
assert not missing,missing
report={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(paths)}
(root/'artifacts/geometric/source-manifest.json').write_text(json.dumps(report,indent=2)+'\n')
