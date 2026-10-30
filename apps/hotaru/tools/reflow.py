#!/usr/bin/env python3
"""Actual Ghostty resize oracle; compares every intermediate screen and history.

Run with --library /path/libghostty-vt.so --candidate /path/examples/reflow.
The candidate accepts the same JSON operations, independent of this implementation.
"""
import argparse
import ctypes as C
import hashlib
import json
import random
import subprocess
from pathlib import Path
from differential import Oracle, Rgb, Point, Coordinate, GridRef, Style, checked, fixture_palette

GHOSTTY_COMMIT = '6ba144158577d033ccb6ed4b574e4013957c9161'


def sha256(path):
    with Path(path).open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def compact_snapshot(snapshot):
    result = dict(snapshot)
    lines = []
    for row in snapshot['lines']:
        cells = []
        for cell in row['cells']:
            fields = [cell['text'], cell['width']]
            if any(cell[key] != {'default': True} for key in ('foreground', 'background', 'underline_color')) or cell['attrs'] or cell['protected']:
                fields += [cell['foreground'], cell['background'], cell['underline_color'], cell['attrs'], cell['protected']]
            cells.append(fields)
        lines.append([row['wrapped'], cells])
    result['lines'] = lines
    return result


class ReflowOracle(Oracle):
    def __init__(self, library):
        super().__init__(library)
        self.lib.ghostty_terminal_resize.argtypes = [C.c_void_p, C.c_uint16, C.c_uint16, C.c_uint32, C.c_uint32]
        self.lib.ghostty_terminal_resize.restype = C.c_int

    def run(self, case):
        terminal = C.c_void_p()
        cols, rows = case['cols'], case['rows']
        checked(self.lib.ghostty_terminal_new(None, C.byref(terminal), cols, rows))
        try:
            max_lines = C.c_size_t(case.get('history', 10000))
            checked(self.lib.ghostty_terminal_set(terminal, self.enum('GhosttyTerminalOption', 'SCROLLBACK_MAX_LINES'), C.byref(max_lines)))
            max_bytes = C.c_size_t(64 * 1024 * 1024 if max_lines.value else 0)
            checked(self.lib.ghostty_terminal_set(terminal, self.enum('GhosttyTerminalOption', 'SCROLLBACK_MAX_BYTES'), C.byref(max_bytes)))
            palette = (Rgb * 256)(*(Rgb(*rgb) for rgb in fixture_palette()))
            checked(self.lib.ghostty_terminal_set(terminal, self.enum('GhosttyTerminalOption', 'COLOR_PALETTE'), C.byref(palette)))
            replies = bytearray()
            callback_type = C.CFUNCTYPE(None, C.c_void_p, C.c_void_p, C.c_void_p, C.c_size_t)
            callback = callback_type(lambda terminal, userdata, pointer, length: replies.extend(C.string_at(pointer, length)))
            checked(self.lib.ghostty_terminal_set(terminal, self.enum('GhosttyTerminalOption', 'WRITE_PTY'), C.cast(callback, C.c_void_p)))
            steps = []
            for op in case['ops']:
                if 'write' in op:
                    data = op['write'].encode()
                    self.lib.ghostty_terminal_vt_write(terminal, data, len(data))
                if 'resize' in op:
                    cols, rows = op['resize']
                    checked(self.lib.ghostty_terminal_resize(terminal, cols, rows, 8, 16))
                q = lambda name, typ: self.query('ghostty_terminal_get', terminal, 'GhosttyTerminalData', name, typ)
                history = q('SCROLLBACK_ROWS', C.c_size_t)
                result = {'cols':cols, 'rows':rows, 'history':history,
                    'cursor':[q('CURSOR_X', C.c_uint16), q('CURSOR_Y', C.c_uint16)+history],
                    'responses':list(replies), 'lines':[]}
                replies.clear()
                for row in range(rows + history):
                    cells = []
                    for col in range(cols):
                        point = Point(tag=self.enum('GhosttyPointTag', 'SCREEN'))
                        point.value.coordinate = Coordinate(col, row)
                        ref = GridRef(size=C.sizeof(GridRef))
                        checked(self.lib.ghostty_terminal_grid_ref(terminal, point, C.byref(ref)))
                        cell, raw_row, style = C.c_uint64(), C.c_uint64(), Style(size=C.sizeof(Style))
                        checked(self.lib.ghostty_grid_ref_cell(C.byref(ref), C.byref(cell)))
                        checked(self.lib.ghostty_grid_ref_row(C.byref(ref), C.byref(raw_row)))
                        checked(self.lib.ghostty_grid_ref_style(C.byref(ref), C.byref(style)))
                        wrapped = self.query('ghostty_row_get', raw_row, 'GhosttyRowData', 'WRAP', C.c_bool)
                        wide = self.query('ghostty_cell_get', cell, 'GhosttyCellData', 'WIDE', C.c_int)
                        protected = self.query('ghostty_cell_get', cell, 'GhosttyCellData', 'PROTECTED', C.c_bool)
                        count = C.c_size_t()
                        codepoints = (C.c_uint32 * 128)()
                        checked(self.lib.ghostty_grid_ref_graphemes(C.byref(ref), codepoints, len(codepoints), C.byref(count)))
                        text = ''.join(chr(cp) for cp in codepoints[:count.value]) or ' '
                        background = self.color(style.bg_color)
                        tag = self.query('ghostty_cell_get', cell, 'GhosttyCellData', 'CONTENT_TAG', C.c_int)
                        if tag == self.enum('GhosttyCellContentTag', 'BG_COLOR_PALETTE'):
                            background = {'index':self.query('ghostty_cell_get', cell, 'GhosttyCellData', 'COLOR_PALETTE', C.c_uint8)}
                        elif tag == self.enum('GhosttyCellContentTag', 'BG_COLOR_RGB'):
                            rgb = self.query('ghostty_cell_get', cell, 'GhosttyCellData', 'COLOR_RGB', Rgb)
                            background = {'rgb':[rgb.r, rgb.g, rgb.b]}
                        attrs = sum(1 << bit for bit, enabled in enumerate((style.bold, style.faint, style.italic, style.underline != 0, style.blink, style.inverse, style.invisible, style.strikethrough)) if enabled)
                        cells.append({'text':text,'width':{0:1,1:2,2:0,3:1}[wide], 'foreground':self.color(style.fg_color),
                            'background':background,'underline_color':self.color(style.underline_color),'attrs':attrs,'protected':protected})
                    result['lines'].append({'wrapped':wrapped,'cells':cells})
                steps.append(result)
            return {'name':case['name'],'steps':steps}
        finally:
            self.lib.ghostty_terminal_free(terminal)


def cases():
    streams = {
        'empty':'', 'short':'abc', 'full':'abcdefgh', 'wrapped':'abcdefghijklmnopqr',
        'hard-lines':'first\r\nsecond\r\nthird', 'blank-lines':'a\r\n\r\nb\r\n',
        'trailing-space':'abc    ', 'erased-space':'abc\x1b[K', 'blank-cursor':'\x1b[2;8H',
        'wide':'ab雪花xyz', 'wide-edge':'abcdefg雪z', 'wide-tail-cursor':'abc雪\x1b[1;5H',
        'combining':'e\u0301 ab雪\u0301cdef', 'colors':'\x1b[31;44mabcdefghi\x1b[0mXYZ',
        'background':'\x1b[44m\x1b[2J\x1b[0mabc', 'protected':'\x1b[1"qabcdefghijk\x1b[0"qX',
        'saved':'abcdef\x1b7ghijklmn', 'saved-blank':'\x1b[3;8H\x1b7\x1b[Habcd',
        'history':'0first\r\n1second\r\n2third\r\n3fourth\r\n4fifth\r\n5sixth',
        'clear-with-history':'one\r\ntwo\r\nthree\r\nfour\r\nfive\x1b[2J\x1b[H',
        'alternate':'primaryabcdefgh\x1b[?1049h012345678901234567',
        'no-wrap':'abcdefghijklmn\x1b[?7l', 'custom-tabs':'\x1b[3g\x1b[4G\x1bH\rA',
    }
    result = []
    for name, text in streams.items():
        for sizes in [[(4,4),(12,4)],[(12,4),(4,4)],[(1,4),(8,4)],[(8,2),(8,6)],[(4,2),(12,6)],[(12,2),(4,6)],[(8,4)]]:
            ops = [{'write':text}]+[{'resize':size} for size in sizes]+[{'write':'\x1b[6nX'}]
            if name.startswith('saved'): ops.append({'write':'\x1b8\x1b[6nY'})
            if name=='alternate': ops.append({'write':'\x1b[?1049l\x1b[6nZ'})
            if name=='custom-tabs': ops.append({'write':'\r\tT'})
            result.append({'name':name+'-'+str(sizes),'cols':8,'rows':4,'ops':ops})
    rng=random.Random(0x484F54415255)
    for index in range(100):
        ops=[]
        for _ in range(12):
            if rng.randrange(3)==0: ops.append({'resize':[rng.randrange(1,14),rng.randrange(1,8)]})
            else: ops.append({'write':rng.choice(['abcde','雪花','e\u0301','\r\n','\x1b7','\x1b8','  ','\x1b[K','\x1b[2J\x1b[H','\x1b[2;3H','\x1b[31;44m','\x1b[0m'])})
        result.append({'name':f'random-{index}','cols':8,'rows':4,'ops':ops})
    return result


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--library',required=True)
    parser.add_argument('--candidate')
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--cases',type=Path)
    parser.add_argument('--record-fixtures',type=Path)
    args=parser.parse_args()
    inputs=json.loads(args.cases.read_text()) if args.cases else cases()
    if isinstance(inputs, dict): inputs=inputs['cases']
    oracle=ReflowOracle(args.library)
    expected=[oracle.run(case) for case in inputs]
    args.output.mkdir(parents=True,exist_ok=True)
    (args.output/'cases.json').write_text(json.dumps(inputs,ensure_ascii=False,indent=2)+'\n')
    (args.output/'expected.json').write_text(json.dumps(expected,ensure_ascii=False,separators=(',',':'))+'\n')
    metadata={'schema':1,'ghostty_commit':GHOSTTY_COMMIT,'library_sha256':sha256(args.library),
        'generator_sha256':sha256(__file__),'case_count':len(inputs),'snapshot_count':sum(len(v['steps']) for v in expected)}
    if args.record_fixtures:
        fixture=dict(metadata,cases=[{**{k:v for k,v in case.items() if k!='expected'},
            'expected':[compact_snapshot(step) for step in reference['steps']]}
            for case,reference in zip(inputs,expected,strict=True)])
        args.record_fixtures.write_text(json.dumps(fixture,ensure_ascii=False,separators=(',',':'))+'\n')
    if not args.candidate: return
    actual=json.loads(subprocess.check_output([args.candidate],input=json.dumps(inputs).encode()))
    (args.output/'actual.json').write_text(json.dumps(actual,ensure_ascii=False,separators=(',',':'))+'\n')
    failures=[]
    for case, want, got in zip(inputs,expected,actual,strict=True):
        for step, (w,g) in enumerate(zip(want['steps'],got['steps'],strict=True)):
            if w!=g:
                failures.append((case['name'],step))
                if len(failures)<=15: print('FAIL',case['name'],'step',step,'operation',case['ops'][step])
                break
    print(f'{len(inputs)-len(failures)}/{len(inputs)} resize sequences match actual Ghostty; {sum(len(v["steps"]) for v in expected)} snapshots')
    metadata.update(candidate_sha256=sha256(args.candidate),failures=failures,
        expected_sha256=sha256(args.output/'expected.json'),actual_sha256=sha256(args.output/'actual.json'))
    (args.output/'report.json').write_text(json.dumps(metadata,indent=2)+'\n')
    if failures: raise SystemExit(1)

if __name__=='__main__': main()
