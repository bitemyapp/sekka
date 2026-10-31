#!/usr/bin/env python3
"""Linux50MB history audit: actual RSS, logical ownership and cold row reads.

Run against the separately built history-budget adapters. Input is freed before
populated RSS. Full normalized cell/grapheme/style/URI digests are compared before
compression; the timed operation reads the first codepoint in every retained row.
"""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import time


def digest(path):
    with Path(path).open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


def corpus(path, kind, count, uri_bytes=0, links=4):
    with path.open('wb') as file:
        for index in range(count):
            prefix = f'{index:08x} '
            if kind == 'ascii':
                body = ('terminal scrollback log data ' * 4)[:101]
            elif kind == 'styled':
                body = ''.join(f'\x1b[38;2;{(index+j)%256};{j*3%256};{index*7%256}m{j:03x}' for j in range(33)) + '\x1b[0m'
            elif kind == 'unicode':
                body = '雪花 e\u0301 日本語 ' * 6
            elif kind == 'mixed':
                body = ''.join(chr(33 + ((index*1597+j*947) % 94)) for j in range(101))
            else:
                body = ''.join(f'\x1b]8;;https://example.test/{index}/{link}/' + 'u'*uri_bytes + f'\x1b\\{chr(65+link)}\x1b]8;;\x1b\\' for link in range(links))
            file.write((prefix + body + '\r\n').encode())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--build', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--samples', type=int, default=5)
    parser.add_argument('--cpu', type=int, default=8)
    parser.add_argument('--case', action='append', help='Run only this named case; repeat for several cases')
    parser.add_argument('--lock', type=Path, default=Path.home()/'.cache/hotaru-workstation.lock')
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    metadata = json.loads(args.build.read_text())
    for name in ['candidate', 'reference']:
        assert digest(metadata[name]) == metadata[name+'_sha256']
    os.sched_setaffinity(0, {args.cpu})
    cases = [('ascii','ascii',8000,0,True), ('styled','styled',8000,0,True), ('unicode','unicode',8000,0,True), ('mixed','mixed',8000,0,True), ('unique-uri','uri',3000,600,True), ('byte-limit-uri','uri',14000,1000,False)]
    if args.case:
        unknown = set(args.case) - {case[0] for case in cases}
        if unknown:
            parser.error(f'unknown case: {sorted(unknown)}')
        cases = [case for case in cases if case[0] in args.case]
    report = {'schema':1,'date':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'host':platform.node(),'cpu':args.cpu,'samples':args.samples,'byte_limit':50_000_000,'row_limit':10_000,'cols':120,'active_rows':24,'build':metadata,'build_metadata_sha256':digest(args.build),'driver_sha256':digest(__file__),'notes':[
        'Fresh process per sample; system allocator; no malloc_trim or forced collection. Input buffer freed before populated RSS.',
        'Logical charge, allocator storage estimates, Ghostty resident page bytes and process RSS are distinct quantities.',
        'Common-content cases require equal retained rows and FNV1a64 digest of all normalized cells/graphemes/styles/protection/URI bytes. Internal style IDs are not compared; zero scalars map to spaces and spacer-head width maps to narrow.',
        'Digest validation runs before compression and after timed read, outside all feed/compression/read timers.',
        'Read latency includes row/reference acquisition and first codepoint access. Hotaru restores temporary bounded blocks through owned ContentSnapshot; Ghostty restores live pages. This is not pure codec latency.',
        'Immutable resident/cold captures surviving terminal destruction are Hotaru-only cases: the Ghostty grid-reference API used here requires a live terminal.',
        'Byte-limit stress reports retention differences explicitly; no RSS/latency ratio is valid when retained content differs.',
        'URI workloads use four unique links per physical row. The original eight-link workload exceeded the pinned Ghostty page capacity of1,663 unique live links and is separately preserved as a reference limitation.',
        'The shared workstation lock covers each case; CPU affinity and cooperative lock cannot exclude unrelated host activity.'
    ],'cases':{},'held_captures':{}}
    for name,kind,count,uri_bytes,require_equal in cases:
        path=args.output/(name+'.vt')
        with args.lock.open('a') as lock:
            fcntl.flock(lock,fcntl.LOCK_EX)
            corpus(path,kind,count,uri_bytes)
        groups={key:[] for key in ['hotaru-resident','ghostty-resident','hotaru-cold','ghostty-cold']}
        commands={}
        with args.lock.open('a') as lock:
            fcntl.flock(lock,fcntl.LOCK_EX)
            for iteration in range(args.samples):
                order=list(groups) if iteration%2==0 else list(reversed(groups))
                for key in order:
                    engine=key.split('-')[0]; compress=key.endswith('-cold')
                    command=[metadata['candidate' if engine=='hotaru' else 'reference'],str(path),'120','24','10000','50000000',str(int(compress))]
                    if engine=='hotaru':command+=['none','0']
                    commands[key]=command
                    groups[key].append(json.loads(subprocess.check_output(command,text=True,timeout=120)))
        retained={(row['history_rows'],row['content_fnv1a64']) for values in groups.values() for row in values}
        if require_equal and len(retained)!=1:
            raise RuntimeError(f'{name}: unequal content invalidates comparison: {retained}')
        for engine in ['hotaru','ghostty']:
            assert len({(row['history_rows'],row['content_fnv1a64']) for key,values in groups.items() if key.startswith(engine) for row in values})==1, (name,engine,'compression changed retained content')
        result={'data_sha256':digest(path),'input_bytes':path.stat().st_size,'unique_links_per_row':4 if kind=='uri' else 0,'equal_retained_content':len(retained)==1,'commands':commands,'groups':{}}
        for key,values in groups.items():
            numeric=[field for field,value in values[0].items() if isinstance(value,(int,float)) and not isinstance(value,bool)]
            result['groups'][key]={'samples':values,'median':{field:statistics.median(v[field] for v in values) for field in numeric}}
        report['cases'][name]=result
        (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
        print(name,{key:{field:group['median'][field] for field in ['history_rows','rss_cold_kib','read_history_ns']} for key,group in result['groups'].items()},flush=True)
        if name in ['ascii','unique-uri']:
            held={}
            with args.lock.open('a') as lock:
                fcntl.flock(lock,fcntl.LOCK_EX)
                for stage in ['resident','cold']:
                    command=[metadata['candidate'],str(path),'120','24','10000','50000000','1',stage,'1']
                    values=[json.loads(subprocess.check_output(command,text=True,timeout=120)) for _ in range(args.samples)]
                    assert {(r['history_rows'],r['content_fnv1a64']) for r in values}==retained
                    fields=[field for field,value in values[0].items() if isinstance(value,(int,float)) and not isinstance(value,bool)]
                    held[stage]={'command':command,'samples':values,'median':{field:statistics.median(v[field] for v in values) for field in fields}}
            report['held_captures'][name]=held
            (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    for name in ['candidate','reference']:
        assert digest(metadata[name])==metadata[name+'_sha256']


if __name__=='__main__':
    main()
