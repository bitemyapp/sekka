#!/usr/bin/env python3
"""Compare isolated width-cache/write-batch variants against an exact checkpoint.

This is a controlled microbenchmark experiment, not Ghostty parity acceptance.
Each printed-only workload first matches the real reference grid/effects. No
OSC8 or other unobserved protocol occurs in these deliberately narrow streams.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import statistics
import subprocess

from corpus import PATTERNS
from differential import Oracle, differences, digest


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--metadata',type=Path,nargs='+',required=True)
    p.add_argument('--library',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--lock',type=Path,default=Path.home()/'.cache/hotaru-workstation.lock')
    a=p.parse_args();a.output.parent.mkdir(parents=True,exist_ok=True)
    os.sched_setaffinity(0,{min(os.sched_getaffinity(0))})
    builds=[json.loads(path.read_text()) for path in a.metadata]
    for b in builds:
        assert digest(Path(b['binary']))==b['binary_sha256']
        for name,want in b['sources'].items():assert digest(Path(b['workspace'])/name)==want
    results=[]
    with a.lock.open('a') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        oracle=Oracle(a.library)
        for name,pattern in [('ascii',PATTERNS['ascii']),('unicode',PATTERNS['unicode']),('dense-wide',('雪'*36+'\r\n').encode())]:
            data=pattern*(2*1024*1024//len(pattern));path=a.output.parent/(name+'.vt');path.write_bytes(data)
            common=['--data',str(path),'--rows','12','--cols','120','--chunk','65536']
            expected=oracle.snapshot(data,12,120,65536)
            for b in builds:
                actual=json.loads(subprocess.check_output([b['binary'],*common,'--format','json']))
                assert not differences(expected,actual),(b['revision'],differences(expected,actual))
            samples={b['revision']:[] for b in builds}
            def measure(b,count):
                value=json.loads(subprocess.check_output([b['binary'],*common,'--bench-json','--iterations',str(count)]))
                assert value['timed_scope']=='feed-only' and value['response_bytes']==[0]*count
                assert value['input_bytes']==len(data) and value['scrollback']==0 and len(value['samples_ns'])==count
                return value['samples_ns']
            for b in builds:measure(b,3)
            order=[]
            for round in range(4):
                for b in (builds if round%2==0 else list(reversed(builds))):
                    order.append(b['revision']);samples[b['revision']].extend(measure(b,5))
            medians={name:statistics.median(values) for name,values in samples.items()}
            results.append({'name':name,'input_bytes':len(data),'sha256':hashlib.sha256(data).hexdigest(),'rows':12,'cols':120,'chunk_bytes':65536,'history':0,'samples_ns':samples,'median_feed_ns':medians,'process_order':order})
        for b in builds:
            assert digest(Path(b['binary']))==b['binary_sha256']
            for name,want in b['sources'].items():assert digest(Path(b['workspace'])/name)==want
        report={'schema':1,'scope':__doc__,'cpu_affinity':sorted(os.sched_getaffinity(0)),'library_sha256':digest(a.library),'tool_sha256':digest(Path(__file__)),'builds':[{'metadata':str(path),'sha256':digest(path),'source_sha256':b['source_sha256'],'binary_sha256':b['binary_sha256'],'revision':b['revision']} for path,b in zip(a.metadata,builds,strict=True)],'results':results}
        a.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({r['name']:r['median_feed_ns'] for r in results}))


if __name__=='__main__':main()
