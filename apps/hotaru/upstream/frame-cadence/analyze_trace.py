#!/usr/bin/env python3
import re,json,statistics,collections,sys,hashlib
from pathlib import Path
root=Path(sys.argv[1]); reports=[]
def stats(v):
 if not v:return None
 v=sorted(v)
 return {'n':len(v),'min':v[0],'median':statistics.median(v),'p95':v[min(len(v)-1,int(.95*len(v)))],'max':v[-1],'mean':statistics.mean(v)}
for d in sorted(root.glob('[01][01]-*')):
 run=d/'run'
 if not (run/'result.json').exists():continue
 result=json.loads((run/'result.json').read_text()); text=(run/'application.log').read_text()
 pending={};records=[];commits=[];surfcount=collections.Counter();raster=[]
 for line in text.splitlines():
  m=re.match(r'\[\s*([0-9:.]+)\]',line)
  if not m:continue
  raw=m.group(1)
  if ':' in raw:
   h,mn,s=map(float,raw.split(':'));t=((h*60+mn)*60+s)*1000
  else:t=float(raw)
  m=re.search(r'wl_surface[@#](\d+)\.frame\((?:new id )?wl_callback[@#](\d+)\)',line)
  if m:
   surf,cb=map(int,m.groups());pending[cb]={'surface':surf,'request_ms':t};surfcount[surf]+=1
  m=re.search(r'wl_surface[@#](\d+)\.commit\(',line)
  if m:commits.append({'surface':int(m.group(1)),'time_ms':t})
  m=re.search(r'wl_callback[@#](\d+)\.done,?\s*\((\d+)\)',line)
  if m:
   cb,mono=map(int,m.groups())
   if cb in pending:
    r=pending.pop(cb);r.update(done_ms=t,monotonic_ms=mono);records.append(r)
 if not records:continue
 surface=surfcount.most_common(1)[0][0]
 records=[r for r in records if r['surface']==surface]
 # Callback payload is compositor CLOCK_MONOTONIC truncated to integer milliseconds.
 # Offset is for coarse phase assignment only; all interval metrics below use client log deltas.
 offset=statistics.median(r['monotonic_ms']-r['done_ms'] for r in records)
 phase=[]
 for stream in result['streams']:
  a,b=stream['first_write_ns']/1e6,stream['last_capture_ns']/1e6
  rec=[r for r in records if a+30<=r['done_ms']+offset<=b-30]
  com=[r for r in commits if r['surface']==surface and a+30<=r['time_ms']+offset<=b-30]
  prev={id(r):records[i-1] for i,r in enumerate(records) if i}
  phase.append({'kind':stream['kind'],'observed_unique':stream['observed_unique'],'produced':stream['produced'],
    'callback_intervals_ms':stats([y['done_ms']-x['done_ms'] for x,y in zip(rec,rec[1:])]),
    'commit_intervals_ms':stats([y['time_ms']-x['time_ms'] for x,y in zip(com,com[1:])]),
    'request_to_done_ms':stats([r['done_ms']-r['request_ms'] for r in rec]),
    'previous_done_to_request_ms':stats([r['request_ms']-prev[id(r)]['done_ms'] for r in rec if id(r) in prev]),
    'negative_request_gaps':sum(r['request_ms']<prev[id(r)]['done_ms'] for r in rec if id(r) in prev),
    'records':rec,'commits':com})
 reports.append({'trial':d.name,'surface':surface,'surface_frame_requests':dict(surfcount),'alignment_offset_ms':offset,
  'offset_spread_ms':stats([r['monotonic_ms']-r['done_ms']-offset for r in records]),'phases':phase})
output={'scope':'Instrumented diagnostic. Phase selection excludes first/last30ms, approximate mono alignment from integer compositor callback times; interval metrics use exact same client trace clock. No native throughput claim.','trials':reports}
(root/'trace-analysis.json').write_text(json.dumps(output,indent=2)+'\n')
for r in reports:
 print(r['trial'],r['surface_frame_requests'])
 for phase in r['phases']:
  print(phase['kind'],phase['observed_unique'],'callbacks',phase['callback_intervals_ms'],'commits',phase['commit_intervals_ms'],'prevDoneToRequest',phase['previous_done_to_request_ms'],'negative',phase['negative_request_gaps'])
