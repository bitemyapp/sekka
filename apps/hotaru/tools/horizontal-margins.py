#!/usr/bin/env python3
"""Actual pinned-library DECSLRM qualification; never supplies expected Rust output."""
from pathlib import Path
import argparse
import fcntl
import hashlib
import itertools
import json
import subprocess
import tempfile

from differential import Oracle, differences, digest


def cases():
    out = []
    def add(name, data, cols=8, rows=5):
        out.append(dict(name=name, data=data, cols=cols, rows=rows))
    setup = b'\x1b[?69h\x1b[3;6s\x1b[2;4r'
    query = b'\x1b[6n\x1b[?69$p\x1bP$qs\x1b\\'
    grid = b''.join(b'\x1b['+str(r+1).encode()+b';1H'+bytes([65+r])*8 for r in range(5))
    for params in [b'', b'0', b';', b'0;0', b'1;8', b'3;6', b'3', b'6;3', b'3;3', b'9;99', b'1;99', b'2;3;4', b'2:3;4']:
        for enabled in [False, True]:
            add(f'set-{params.hex()}-{enabled}', b'\x1b[3;4H'+(b'\x1b[?69h' if enabled else b'')+b'\x1b['+params+b's'+query+b'X')
    for origin, row, col in itertools.product([False,True],[1,2,4,5],[1,2,3,6,7,8]):
        for tail in [b'abcdefghi', '雪雪雪'.encode(), b'\rZ', b'\nZ', b'\x1bDZ', b'\x1bMZ', b'\bZ', b'\tZ', b'\x1b[2ZZ']:
            data=grid+setup+(b'\x1b[?6h' if origin else b'')+f'\x1b[{row};{col}H'.encode()+tail+query
            add(f'cursor-{origin}-{row}-{col}-{tail.hex()}',data)
    edits=[b'\x1b[2@',b'\x1b[2P',b'\x1b[2L',b'\x1b[2M',b'\x1b[2S',b'\x1b[2T',b'\x1b[2X',b'\x1b[K',b'\x1b[1K',b'\x1b[2K',b'\x1b[J',b'\x1b[1J',b'\x1b[2J']
    for col,edit in itertools.product([1,2,3,4,6,7,8],edits):
        add(f'edit-{col}-{edit.hex()}',grid+setup+f'\x1b[3;{col}H'.encode()+b'\x1b[44m'+edit+query)
    for col,movement in itertools.product([1,2,3,4,6,7,8],[b'\x1b[99C',b'\x1b[99D',b'\x1b[99a',b'\x1b[99A',b'\x1b[99B',b'\x1b[99e',b'\x1b[2E',b'\x1b[2F',b'\x1b[2G',b'\x1b[2d']):
        for origin in [False,True]:
            add(f'move-{col}-{movement.hex()}-{origin}',setup+(b'\x1b[?6h' if origin else b'')+f'\x1b[3;{col}H'.encode()+movement+query+b'X')
    for content,tail,wrap in itertools.product(['雪雪雪雪','☀\ufe0fX','😀\ufe0eX','e\u0301雪','👨\u200d👩\u200d👧','\u0600A#\ufe0f\u20e3'], [b'Z',b'\x1b[2@Z',b'\x1b[PZ',b'\nZ'], [False,True]):
        add(f'wide-{content}-{tail.hex()}-{wrap}',grid+setup+(b'' if wrap else b'\x1b[?7l')+b'\x1b[3;6H'+content.encode()+tail+query)
    for edit,side in itertools.product(edits[:8],[b'\x1b[2;2H',b'\x1b[2;6H']):
        add(f'split-{side.hex()}-{edit.hex()}',grid+side+b'\x1b[1;31;44m\x1b]8;;https://example.org/keep\x07'+"雪\u0301".encode()+b'\x1b]8;;\x07'+setup+b'\x1b[2;3H'+edit+query)
    for reset in [b'\x1b[?69l',b'\x1b[!p',b'\x1bc',b'\x1b#8',b'\x1b[?47hALT\x1b[?47l',b'\x1b[?1047hALT\x1b[?1047l',b'\x1b[?1049hALT\x1b[?1049l',b'\x1b7\x1b[4;7s\x1b8',b'\x1b[?69s\x1b[?69l\x1b[?69r']:
        add(f'reset-{reset.hex()}',grid+setup+b'\x1b[3;6H'+reset+query+b'xy')
    for left,right in [(1,8),(2,8),(1,7),(2,3),(3,6)]:
        for pending in [b'abcdef', '雪雪雪'.encode()]:
            add(f'pending-{left}-{right}-{pending.hex()}',b'\x1b[?69h'+f'\x1b[{left};{right}s\x1b[3;{left}H'.encode()+pending+b'\x1b[?69lZ'+query)
    for cols in [1,2,3]:
        add(f'tiny-{cols}',b'\x1b[?69h\x1b[1;2s'+('雪abc\r\n☀\ufe0fX'*5).encode()+query,cols,3)
    for column, tail in itertools.product([1,2,3,6,7,8],[b'\x1b[@Z',b'\x1b[PZ',b'\x1b[LZ',b'\x1b[MZ',b'\x1b[SZ',b'\x1b[TZ','雪X'.encode(),'☀\ufe0fX'.encode()]):
        add(f'pending-edit-{column}-{tail.hex()}',setup+f'\x1b[3;{column}H'.encode()+b'X'+tail+query)
    for left,right in [(2,3),(3,4),(6,7)]:
        for tail in ['雪X','☀\ufe0fX','😀\ufe0eX']:
            add(f'outside-pending-{left}-{right}-{tail}',b'\x1b[?69h'+f'\x1b[{left};{right}s\x1b[3;8H'.encode()+b'X'+tail.encode()+query)
            if tail != '☀\ufe0fX':
                out[-1]['reference_limitation']='stale-pre-wrap-right-limit'
    for movement in [b'\x1b[A',b'\x1b[B',b'\x1b[C',b'\x1b[D',b'\x1b[a',b'\x1b[e',b'\x1b[2G',b'\x1b[2d',b'\r',b'\n',b'\t']:
        add(f'saved-outside-origin-{movement.hex()}',b'\x1b[?6h\x1b7'+setup+b'\x1b8'+movement+b'X'+query)
    for invalid in [b'\x1b[3;3s',b'\x1b[7;2s',b'\x1b[2;3;4s']:
        add(f'invalid-pending-{invalid.hex()}',setup+b'\x1b[3;6HX'+invalid+b'Z'+query)
    for width,text,edit in itertools.product([3,4,5],['雪雪','☀\ufe0f雪','A雪'],[b'\x1b[@\x1b[P',b'\x1b[P\x1b[@']):
        add(f'compound-wide-shift-{width}-{text}-{edit.hex()}',grid+b'\x1b[?69h'+f'\x1b[1;{width}s\x1b[H'.encode()+text.encode()+b'\r'+edit+query)
    return out


def compact(snapshot):
    lines=[]
    for row in snapshot['lines']:
        cells=[]
        for cell in row['cells']:
            item=[cell['text'],cell['width']]
            colors=[cell[key] for key in ['foreground','background','underline_color']]
            if colors != [{'default':True}]*3 or cell['attrs']:
                item.extend([*colors,cell['attrs']])
            cells.append(item)
        lines.append([row['wrapped'],cells])
    cursor=snapshot['cursor']
    return {'cursor':[cursor['col'],cursor['row'],cursor['visible']],
            'lines':lines,'responses':snapshot['responses'],'alternate_screen':snapshot['modes']['alternate_screen']}


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--replay',type=Path,required=True)
    parser.add_argument('--library',type=Path,required=True)
    parser.add_argument('--report',type=Path,required=True)
    parser.add_argument('--fixtures',type=Path)
    args=parser.parse_args()
    with Path('/home/callen/.cache/hotaru-workstation.lock').open('a') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        assert digest(args.library)=='70e1bdcf62801b735bb268c38c1e65fa727debff73d89ef1d3ed9fb5e23e7639'
        oracle=Oracle(args.library)
        records=[];fixtures=[]
        with tempfile.TemporaryDirectory() as temporary:
            path=Path(temporary)/'input.vt'
            for case in cases():
                data=case['data'];path.write_bytes(data)
                for mode in ['legacy','unicode']:
                    expected=oracle.snapshot(data,case['rows'],case['cols'],65536,{2027:mode=='unicode'})
                    limitation=case.get('reference_limitation')
                    if not limitation:
                        fixtures.append({k:v for k,v in case.items() if k!='data'}|{'mode':mode,'data_hex':data.hex(),'expected':compact(expected)})
                    for chunk in [1,7,65536]:
                        reference=oracle.snapshot(data,case['rows'],case['cols'],chunk,{2027:mode=='unicode'}) if chunk != 65536 else expected
                        reference_delta=differences(expected,reference)
                        command=[str(args.replay),'--data',str(path),'--rows',str(case['rows']),'--cols',str(case['cols']),'--chunk',str(chunk),'--grapheme-width',mode,'--format','json']
                        result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                        actual=json.loads(result.stdout) if result.returncode==0 else None
                        delta=differences(reference,actual) if actual is not None else [result.stderr[-2000:]]
                        record={'name':case['name'],'mode':mode,'chunk':chunk,'differences':delta,'reference_chunk_differences':reference_delta,'reference_limitation':limitation,'process_failed':result.returncode!=0}
                        if limitation:
                            record.update(reference=reference,actual=actual,data_hex=data.hex())
                        records.append(record)
        source=Path(__file__).resolve().parents[1]
        manifest={str(p.relative_to(source)):digest(p) for p in sorted((source/'crates/hotaru-core').rglob('*')) if p.is_file()}
        report={'library_sha256':digest(args.library),'replay_sha256':digest(args.replay),'core_sources':manifest,'tool_sha256':digest(__file__),'comparisons':len(records),'failures':sum(r['process_failed'] or (bool(r['differences'] or r['reference_chunk_differences']) and not r['reference_limitation']) for r in records),'named_reference_difference_count':sum(bool(r['differences']) and bool(r['reference_limitation']) for r in records),'records':records}
        args.report.write_text(json.dumps(report,indent=2)+'\n')
        if args.fixtures:args.fixtures.write_text(json.dumps({'cases':fixtures,'library_sha256':report['library_sha256']},ensure_ascii=False,separators=(',',':'))+'\n')
        print(json.dumps({k:v for k,v in report.items() if k not in ['records','core_sources']}))


if __name__=='__main__':main()
