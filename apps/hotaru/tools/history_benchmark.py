#!/usr/bin/env python3
"""Same-host RSS and full-history row-read comparisons, with explicit phases.

RSS is measured from /proc/self/status and reported separately from library
storage accounting. Reading history includes row-handle acquisition and the
first cell of each row; this is an application-access measurement, not a pure
codec microbenchmark. Candidate and oracle run as separate fresh processes.
"""
import argparse
import hashlib
import json
from pathlib import Path
import statistics
import subprocess


def digest(path):
    with Path(path).open('rb') as source:
        return hashlib.file_digest(source,'sha256').hexdigest()


def main():
    p=argparse.ArgumentParser()
    p.add_argument('--candidate',required=True)
    p.add_argument('--reference',required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--samples',type=int,default=7)
    a=p.parse_args()
    a.output.mkdir(parents=True,exist_ok=True)
    report={'schema':1,'candidate_sha256':digest(a.candidate),'reference_sha256':digest(a.reference),
        'read_scope':'acquire history row handles and read first codepoint in every retained row',
        'rss_source':'/proc/self/status VmRSS; fresh process per sample; no allocator trimming', 'corpora':{}}
    for name in ['ascii','styled','unicode','mixed']:
        lines=[]
        for index in range(8000):
            if name=='ascii': text=f'{index:08x} '+('terminal scrollback log data '*4)[:101]
            elif name=='styled': text=f'{index:08x} '+''.join(f'\x1b[38;2;{(index+j)%256};{j*3%256};{index*7%256}m{j:03x}' for j in range(33))+'\x1b[0m'
            elif name=='unicode': text=f'{index:08x} '+('雪花 e\u0301 日本語 '*6)
            else: text=f'{index:08x} '+''.join(chr(33+((index*1597+j*947)%94)) for j in range(101))
            lines.append((text+'\r\n').encode())
        data=a.output/f'{name}.vt';data.write_bytes(b''.join(lines))
        groups={}
        for engine,command in [('hotaru-uncompressed',[a.candidate]),('hotaru-compressed',[a.candidate]),('ghostty-uncompressed',[a.reference]),('ghostty-compressed',[a.reference])]:
            command += [str(data), '120','24','10000']
            command += ['1' if engine.endswith('-compressed') else '0']
            results=[json.loads(subprocess.check_output(command)) for _ in range(a.samples)]
            keys=['history_rows','feed_ns','read_history_ns','rss_before_kib','rss_populated_kib','rss_cold_kib','rss_read_kib','storage_populated_bytes','storage_cold_bytes','compression_ns','compressed_pages','compressed_rows','compressed_bytes','compressed_blocks','resident_size_bound','compression_steps']
            medians={key:statistics.median(v[key] for v in results) for key in keys if key in results[0]}
            groups[engine]={'samples':results,'median':medians}
        histories={group['samples'][0]['history_rows'] for group in groups.values()}
        checksums={group['samples'][0]['checksum'] for group in groups.values()}
        if len(histories)!=1 or len(checksums)!=1: raise RuntimeError(f'{name}: unequal retained contents invalidate comparison: rows={histories},checksum={checksums}')
        report['corpora'][name]={'data_sha256':digest(data),'input_bytes':data.stat().st_size,'results':groups}
        print(name,{key:value['median'] for key,value in groups.items()})
    (a.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')

if __name__=='__main__':main()
