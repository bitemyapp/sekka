#!/usr/bin/env python3
"""Actual pinned-library comparisons for interior ASCII handoff boundaries."""
import differential

def fixtures():
    bodies = [
        "雪{ascii}\u0301\x1b[3b", "\u0600{ascii}\x1b[2b",
        "雪{ascii}1\ufe0f\u20e3\x1b[2b", "雪{ascii}\ufe0f日",
        "雪{ascii}\u0080\x1b[2b", "雪{ascii}👩🏽‍💻end",
        "\x1b(0雪{ascii}x\x1b(B", "\x1b(A雪{ascii}#\x1b(B",
        "\x1b*0\x1bN雪{ascii}x", "\x1b[4h雪{ascii}\x1b[4l",
        "\x1b[1\"q雪{ascii}\x1b[0\"q",
    ]
    for unicode in (False, True):
        for cols in (1, 2, 7, 16, 80):
            for nowrap in (False, True):
                for count in (15, 16, 17, 48):
                    for index, body in enumerate(bodies):
                        data = b'\x1b[?2027h' if unicode else b'\x1b[?2027l'
                        data += 'old 雪雪\r\nline\r'.encode()
                        if nowrap: data += b'\x1b[?7l'
                        before,after=body.split('{ascii}')
                        data += before.encode() + b'a'*count + b'\xf0\x9f' + after.encode() + b'\x1b[6n'
                        yield {'name': f'ascii-{int(unicode)}-{cols}-{int(nowrap)}-{count}-{index}',
                               'cols': cols, 'rows': 3, 'data': data}
    for unicode in (False,True):
        for tail in (b'',b'abcdefghijklmnopabcdefghijklmnop'):
            data=(b'\x1b[?2027h' if unicode else b'\x1b[?2027l') + '雪'.encode()*16384 + tail + b'\xf0\x9f\r\n\x1b[6n'
            yield {'name':f'long-dense-prefix-{int(unicode)}-{len(tail)}','cols':120,'rows':12,'data':data}

differential.fixtures = fixtures
if __name__ == '__main__': differential.main()
