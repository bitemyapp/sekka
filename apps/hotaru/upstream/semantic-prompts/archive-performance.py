#!/usr/bin/env python3
import fcntl,gzip,hashlib,json,shutil
from pathlib import Path
root=Path(__file__).resolve().parent.parent
workspace=root/'apps/hotaru'
first=Path('/home/callen/work/hotaru-semantic-prompts-20261028/apps/hotaru')
out=root/'artifacts/performance'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
with Path('/home/callen/.cache/hotaru-workstation.lock').open('a') as lock:
 fcntl.flock(lock,fcntl.LOCK_EX)
 out.mkdir(exist_ok=True)
 raw=workspace/'native-forty-scoped-a/summary.json'
 d=json.loads(raw.read_text())
 assert d['status']=='completed' and len(d['runs'])==40
 for name,expected in d['builds']['candidate']['sources'].items():
  assert sha(workspace/name)==expected,name
 assert sha(Path(d['builds']['candidate']['binary']))==d['builds']['candidate']['binary_sha256']
 sources={
 'native-summary.json.gz':raw,
 'mpstat.log.gz':workspace/'native-forty-scoped-a/per-core-mpstat.log',
 'first-attempt-summary.json.gz':first/'native-forty-a/summary.json',
 'first-attempt-mpstat.log.gz':first/'native-forty-a/per-core-mpstat.log',
 }
 compressed={}
 for dest,src in sources.items():
  data=src.read_bytes()
  (out/dest).write_bytes(gzip.compress(data,compresslevel=9,mtime=0))
  compressed[dest]={'source':str(src),'uncompressed_bytes':len(data),'uncompressed_sha256':hashlib.sha256(data).hexdigest()}
 copies={
 'native-build.json':workspace/'evidence/native-build.json',
 'baseline-build.json':workspace/'evidence/native-baseline-build.json',
 'native-build.log':workspace/'evidence/native-build-fastgate.log',
 'run.log':workspace/'evidence/native-forty-scoped-a.log',
 'runner.py':first/'tools/native-forty-scoped.py',
 'first-attempt-runner.py':first/'tools/native-forty.py',
 'first-attempt-build.json':first/'evidence/native-build.json',
 'first-attempt-build.log':first/'evidence/native-build.log',
 'first-attempt-run.log':first/'evidence/native-forty-a.log',
 'build-candidate.py':first/'tools/build-candidate.py',
 'differential.py':first/'tools/differential.py',
 'corpus.py':first/'tools/corpus.py',
 }
 for dest,src in copies.items():shutil.copy2(src,out/dest)
 rows=[]
 for r in d['runs']:
  keys=['mode','input','rows','cols','input_bytes','input_sha256','statistics','candidate_over_baseline_statistics','candidate_over_ghostty_statistics','baseline_over_ghostty_statistics','full_baseline_candidate_state_equal','pinned_reference_common_state_equal','load_flags','smt_sibling_active_percent','external_selected_core_observations']
  rows.append({k:r[k] for k in keys})
 ratio=[r['candidate_over_baseline_statistics']['median_feed_ns'] for r in rows]
 summary={'status':'completed','scope':d['scope'],'interpretation':'Feature-cost observation, not whole-app parity or a claimed optimization. All40 common state gates passed. Six workload windows recorded one10ms kernel tick each on CPU24; preserve flags rather than treating the host as perfectly idle. Unrelated compilers elsewhere were observed and permitted. Snapshot sampling cannot exclude unsampled migration.',
 'cpu':d['cpu'],'smt_siblings':d['observed_sibling_cpus'],'load_policy':d['load_policy'],'cases':len(rows),'samples_per_engine_case':60,'warmups_per_engine_case':3,'orders':'all six permutations of baseline/candidate/Ghostty','candidate_source_sha256':d['builds']['candidate']['source_sha256'],'candidate_binary_sha256':d['builds']['candidate']['binary_sha256'],'baseline_binary_sha256':d['builds']['baseline']['binary_sha256'],'reference_build':d['reference_build'],'reference_library_sha256':d['library_sha256'],'candidate_over_baseline_median_range':[min(ratio),max(ratio)],'first_attempt':'Aborted after15 cases by prior whole-host compiler gate; unguarded ED2 showed an indicative~2x cursor cost. It is not acceptance evidence. Final source adds a conservative marker-seen gate.','compressed_raw':compressed,'cases_with_load_flags':d['cases_with_load_flags'],'runs':rows}
 (out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
 manifest={p.name:{'sha256':sha(p),'bytes':p.stat().st_size} for p in sorted(out.iterdir()) if p.is_file() and p.name!='manifest.json'}
 (out/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
 print(json.dumps({'files':len(manifest),'bytes':sum(v['bytes'] for v in manifest.values()),'candidate':summary['candidate_binary_sha256']}))

