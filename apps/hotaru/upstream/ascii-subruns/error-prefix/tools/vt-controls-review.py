#!/usr/bin/env python3
"""Independent adversarial cases for REP and modal screen transitions."""
import random
import differential


def fixtures():
    cases=[]
    def add(name,data,cols=12,rows=5):
        cases.append(dict(name=name,data=data,cols=cols,rows=rows))
    q=b'\x1b[6n\x1b[?47$p\x1b[?1047$p\x1b[?1049$p\x1b[?1048$p\x1b[?25$p'
    for mode in (False,True):
        setup=b'\x1b[?2027h' if mode else b''
        for cp in range(0x80,0xa0):
            add(f'c1-{mode}-{cp:x}',setup+b'A'+chr(cp).encode()+b'\x1b[3bZ'+q)
        for cp in range(0x80,0xa0):
            encoded=chr(cp).encode()
            add(f'raw-c1-{mode}-{cp:x}',setup+b'A'+bytes([cp])+b'\x1b[3bZ'+q)
            add(f'c1-single-shift-{mode}-{cp:x}',setup+b'A\x1b*0\x1bN'+encoded+b'q\x1b[2b'+q)
            add(f'c1-metadata-{mode}-{cp:x}',setup+b'\x1b[31m\x1b]8;id=x;https://example.test\x07A'+encoded+'\u0301'.encode()+b'Z\x1b[3b'+q)
        for cp in range(0xa0,0x100):
            add(f'latin1-{mode}-{cp:x}',setup+b'A'+chr(cp).encode()+b'\x1b[3bZ'+q)
        for count in (65536,99999999999999999,100000000):
            for text in ('A','雪'):
                add(f'rep-overflow-{mode}-{count}-{ord(text):x}',setup+text.encode()+f'\x1b[{count}b'.encode()+q)
        clusters=['e\u0301','☀︎','❤️','👩🏽\u200d💻','🇯🇵','\u0600a','\u1100\u1161','a\u0301'+('\u0301'*64)]
        for i,value in enumerate(clusters):
            for cols in (1,2,3,12):
                for nowrap in (False,True):
                    add(f'cluster-{mode}-{i}-cols{cols}-nowrap{nowrap}',setup+(b'\x1b[?7l' if nowrap else b'')+value.encode()+b'\x1b[9b'+q,cols)
    for mode in (47,1047,1049):
        enter=f'\x1b[?{mode}h'.encode();leave=f'\x1b[?{mode}l'.encode()
        for style in (b'\x1b[31;44;1;7m',b'\x1b[38;2;20;30;40;48;2;50;60;70m',b'\x1b[4:3;58:5:2;53m'):
            for cycle in (enter,enter+leave+b'\x1b[?47h',enter+enter,enter+leave+enter):
                add(f'alt-style-{len(cases)}',b'base'+style+b'\x1b[1"q'+cycle+b'Z'+q)
        for rows in (1,3):
            add(f'alt-wrap-metadata-{mode}-{rows}',b'\x1b[31;44mabcdefghijk\x1b]8;id=a;https://a.test\x07L'+enter+b'M\x1b[?1047l\x1b[?47hN\x1b[?1049hO'+leave+b'P'+q,12,rows)
        add(f'alt-cursor-saved-style-{mode}',b'\x1b[31mA\x1b7'+enter+b'\x1b[32mB\x1b7'+leave+b'\x1b8C\x1b[?47h\x1b8D'+q)
        add(f'alt-origin-margin-{mode}',b'\x1b[2;4r\x1b[?6h\x1b[3;12H'+enter+b'A'+leave+b'B'+q)
        add(f'alt-kitty-{mode}',b'\x1b[>31u'+enter+b'\x1b[>5u'+enter+b'\x1b[?u'+leave+b'\x1b[?u\x1b[?47h\x1b[?u'+q)
    rng=random.Random(0xA17DCAFE)
    choices=[b'\x1b[?47h',b'\x1b[?47l',b'\x1b[?1047h',b'\x1b[?1047l',b'\x1b[?1049h',b'\x1b[?1049l',b'\x1b[?1048h',b'\x1b[?1048l',b'\x1b7',b'\x1b8',b'\x1b[?25h',b'\x1b[?25l',b'\x1b[?1049s',b'\x1b[?1049r',b'\x1b[1;4r',b'\x1b[2;4r',b'\x1b[?6h',b'\x1b[?6l',b'\x1b[31m',b'\x1b[44m',b'\x1b[2b',b'\x1b[8G',b'\r\n',b'\x1b[!p',b'\x1bc',b'abc','雪'.encode(),'e\u0301'.encode()]
    for i in range(100):
        add(f'seeded-transition-{i}',b''.join(rng.choice(choices) for _ in range(64))+q,8,4)
    return cases


if __name__=='__main__':
    differential.fixtures=fixtures
    differential.main()
