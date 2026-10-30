#!/usr/bin/env python3
"""Kitty clipboard protocol (OSC 5522): actual pinned-library differential.

Each scenario runs through the pinned libghostty-vt, using its clipboard read
and write effects, ghostty_terminal_paste() and a fixed entropy source. It
also runs through Hotaru's `kitty-clipboard-replay` with the same embedder
model. The tool compares what the program would see and what the embedder
would be asked:
- every byte written back to the program, step by step;
- every read, write and paste request the embedder is asked to answer.
Expected output comes only from the library, never from Rust.

The embedder model is the same on both sides:
- A clipboard holds typed contents for the standard and primary locations.
- Each read and write consumes the scenario's next decision (success when
  none are left).
- A successful read serves the requested types the clipboard has.
- A successful write replaces that location's contents (none clears it).
- `remember` records a grant where the request carried a password.
"""
from pathlib import Path
import argparse
import base64
import ctypes as C
import fcntl
import json
import subprocess
import tempfile

from differential import Oracle, checked, differences, digest

LIBRARY_SHA256 = '70e1bdcf62801b735bb268c38c1e65fa727debff73d89ef1d3ed9fb5e23e7639'
OTP_ALPHABET = b'23456789abcdefghijkmnopqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ'
OTP_LEN = 22
CHUNKS = [1, 3, 65536]


class String(C.Structure):
    _fields_ = [('ptr', C.c_void_p), ('len', C.c_size_t)]


class Content(C.Structure):
    _fields_ = [('mime', String), ('data', String)]


class Read(C.Structure):
    pass


class ReadReply(C.Structure):
    _fields_ = [('size', C.c_size_t), ('result', C.c_int), ('contents', C.POINTER(Content)),
                ('contents_len', C.c_size_t), ('available', C.POINTER(String)), ('available_len', C.c_size_t),
                ('remember', C.c_bool)]


READ_REPLY = C.CFUNCTYPE(None, C.POINTER(Read), C.POINTER(ReadReply))
Read._fields_ = [('size', C.c_size_t), ('location', C.c_int), ('mimes', C.POINTER(String)), ('mimes_len', C.c_size_t),
                 ('list', C.c_bool), ('name', String), ('granted', C.c_bool), ('can_remember', C.c_bool),
                 ('ctx', C.c_void_p), ('reply', READ_REPLY)]


class Write(C.Structure):
    pass


class WriteReply(C.Structure):
    _fields_ = [('size', C.c_size_t), ('result', C.c_int), ('remember', C.c_bool)]


WRITE_REPLY = C.CFUNCTYPE(None, C.POINTER(Write), C.POINTER(WriteReply))
Write._fields_ = [('size', C.c_size_t), ('location', C.c_int), ('contents', C.POINTER(Content)),
                  ('contents_len', C.c_size_t), ('name', String), ('granted', C.c_bool), ('can_remember', C.c_bool),
                  ('ctx', C.c_void_p), ('reply', WRITE_REPLY)]


class Writer(C.Structure):
    _fields_ = [('write', C.c_void_p), ('userdata', C.c_void_p)]


MIME_READ = C.CFUNCTYPE(C.c_bool, C.c_void_p, String, Writer)


class MimeReader(C.Structure):
    _fields_ = [('read', MIME_READ), ('userdata', C.c_void_p)]


class Paste(C.Structure):
    _fields_ = [('size', C.c_size_t), ('location', C.c_int), ('source', C.c_int), ('mimes', C.POINTER(String)),
                ('mimes_len', C.c_size_t), ('reader', MimeReader), ('allow_unsafe', C.c_bool)]


LAYOUTS = {'GhosttyString': String, 'GhosttyClipboardContent': Content, 'GhosttyClipboardRead': Read,
           'GhosttyClipboardReadReply': ReadReply, 'GhosttyClipboardWrite': Write,
           'GhosttyClipboardWriteReply': WriteReply, 'GhosttyWriter': Writer, 'GhosttyMimeReader': MimeReader,
           'GhosttyPaste': Paste}

READ_CALLBACK = C.CFUNCTYPE(None, C.c_void_p, C.c_void_p, C.POINTER(Read))
WRITE_CALLBACK = C.CFUNCTYPE(None, C.c_void_p, C.c_void_p, C.POINTER(Write))
PTY_CALLBACK = C.CFUNCTYPE(None, C.c_void_p, C.c_void_p, C.c_void_p, C.c_size_t)
RANDOM = C.CFUNCTYPE(C.c_bool, C.c_void_p, C.POINTER(C.c_uint8), C.c_size_t)


def b64(value):
    return base64.b64encode(value if isinstance(value, bytes) else value.encode()).decode()


def packet(metadata, payload=None, bel=False):
    body = b'\x1b]5522;' + metadata.encode()
    if payload is not None:
        body += b';' + (payload if isinstance(payload, bytes) else payload.encode())
    return body + (b'\x07' if bel else b'\x1b\\')


def encoded(data):
    return base64.b64encode(data)


def otp(entropy, at=0):
    """The password the pinned algorithm makes from `entropy` read in blocks
    of 44 from `at`: only used to write follow-up reads into the corpus."""
    password = b''
    while len(password) < OTP_LEN:
        block = entropy[at:at + 2 * OTP_LEN]
        if len(block) < 2 * OTP_LEN:
            return None, at
        at += len(block)
        for byte in block:
            if byte < 256 // len(OTP_ALPHABET) * len(OTP_ALPHABET):
                password += OTP_ALPHABET[byte % len(OTP_ALPHABET)].to_bytes(1, 'big')
                if len(password) == OTP_LEN:
                    break
    return password.decode(), at


def scenarios():
    out = []
    TEXT, HTML, PNG = 'text/plain', 'text/html', 'image/png'
    clipboard = {'standard': [[TEXT, b'Ghostty'.hex()], [HTML, b'<b>hi</b>'.hex()], [PNG, bytes(range(256)).hex()]],
                 'primary': [[TEXT, b'picked'.hex()]]}

    def add(name, steps, **extra):
        case = dict(name=name, clipboards=extra.pop('clipboards', clipboard), steps=[], reads=[], writes=[],
                    entropy=extra.pop('entropy', bytes(range(256)) * 4).hex(), write_limit=extra.pop('write_limit', None))
        for step in steps:
            if isinstance(step, bytes):
                case['steps'].append({'feed': step.hex()})
            else:
                case['steps'].append(step)
        case.update(extra)
        out.append(case)

    def paste(location='standard', mimes=(TEXT, HTML, PNG)):
        return {'paste': {'location': location, 'mimes': list(mimes)}}

    # Metadata: records, keys, ids, locations, names and passwords, each on a
    # read of text/plain (and once on a write).
    read_text = encoded(b'text/plain')
    for metadata in ['type=read', 'type=read:', ':type=read', 'type=read:bare', 'bare:type=read', 'type=', 'type=bobr',
                     'type=READ', 'type=read:type=bobr', 'type=bobr:type=read', 'type=write:type=read',
                     'type=read:bobr=kurwa', 'type=read:=x', 'type=read:id=', 'type=read:id=a1-_+.b',
                     'type=read:id=*4 2*', 'type=read:id=' + 'x' * 600, 'type=read:id=' + 'ab*' * 300,
                     'type=read:id=a:id=b', 'type=read:id==x', 'type=read:loc=primary', 'type=read:loc=PRIMARY',
                     'type=read:loc=bobr', 'type=read:loc=', 'type=read:loc=primary:loc=standard',
                     'type=read:name=' + b64('app'), 'type=read:pw=' + b64('pw'),
                     'type=read:pw=' + b64('pw') + ':name=' + b64('app'),
                     'type=read:pw=' + b64('p' * 128) + ':name=' + b64('app'),
                     'type=read:pw=' + b64('p' * 129) + ':name=' + b64('app'),
                     'type=read:pw=!!!!:name=' + b64('app'), 'type=read:pw=' + b64('pw') + ':name=/w==',
                     'type=read:name=' + b64('n' * 256), 'type=read:name=' + b64('n' * 257),
                     'type=read:name=' + b64('app')[:-1], 'type=read:mime=' + b64('text/plain'),
                     'type=read:mime=!!!', 'type=read:password=' + b64('pw') + ':name=' + b64('app'),
                     'type=read:status=OK', '', 'loc=primary']:
        for payload in [read_text, None]:
            add(f'metadata-{metadata.encode().hex()}-{payload is not None}', [packet(metadata, payload)])
    add('metadata-empty-payload', [packet('type=read', b'')])
    add('metadata-bel', [packet('type=read:id=b', read_text, bel=True)])

    # Read lists: listings, order, absent types, the four-type cap, long
    # names, whitespace, and lists that aren't strict base64 or UTF-8.
    for name, mimes in [('listing', b'.'), ('order', b'image/png text/plain'), ('absent', b'text/x-none text/plain'),
                        ('cap', b'a/0 a/1 a/2 text/plain image/png .'), ('dup', b'text/plain text/plain'),
                        ('spaces', b'  text/plain\ttext/html\n.\r\n'), ('only-space', b'   '),
                        ('long', b'x/' + b'y' * 300 + b' text/plain'), ('dots', b'. . text/plain'),
                        ('utf8', 'text/plaîn text/plain'.encode())]:
        add(f'read-list-{name}', [packet('type=read:id=r', encoded(mimes))])
    for name, payload in [('unpadded', b'dGV4dC9wbGFpbg'), ('whitespace', b'dGV4dC9w bGFpbg=='), ('bad', b'!!!!'),
                          ('not-utf8', b'/w=='), ('padding-mid', b'Lg==Lg=='), ('trailing-bits', b'dGV4dC9wbGFpbh==')]:
        add(f'read-list-{name}', [packet('type=read:id=r', payload)])
    add('read-primary', [packet('type=read:loc=primary', encoded(b'text/plain .'))])
    add('read-empty-clipboard', [packet('type=read:id=e', encoded(b'text/plain .'))],
        clipboards={'standard': [], 'primary': []})
    add('read-big', [packet('type=read', encoded(b'image/png'))],
        clipboards={'standard': [[PNG, bytes(range(256)).hex() * 33]], 'primary': []})
    many = [[f'type/{i:02}', b'x'.hex()] for i in range(20)]
    add('read-listing-many', [packet('type=read', encoded(b'.'))], clipboards={'standard': many, 'primary': []})
    wide = [[f'type/{i:03}-' + 'z' * 200, b'x'.hex()] for i in range(25)]
    add('read-listing-truncated', [packet('type=read', encoded(b'.'))], clipboards={'standard': wide, 'primary': []})
    for result in ['denied', 'unsupported', 'busy', 'io_error']:
        add(f'read-result-{result}', [packet('type=read:id=x', read_text)], reads=[{'result': result}])

    # Read grants: remembered per password and direction, never from a
    # nameless password, never spent by a listing, the oldest of 32 evicted.
    secret = 'type=read:pw=' + b64('secret') + ':name=' + b64('app')
    add('read-grant', [packet(secret, read_text), packet(secret, read_text), packet(secret, encoded(b'.')),
                       packet('type=read:pw=' + b64('other') + ':name=' + b64('app'), read_text),
                       packet('type=read:pw=' + b64('secret'), read_text),
                       packet('type=write:pw=' + b64('secret') + ':name=' + b64('app')), packet('type=wdata')],
        reads=[{'result': 'success', 'remember': True}])
    add('read-grant-only-on-success', [packet(secret, read_text), packet(secret, read_text)],
        reads=[{'result': 'denied', 'remember': True}])
    add('read-grant-nameless', [packet('type=read:pw=' + b64('secret'), read_text),
                                packet('type=read:pw=' + b64('secret'), read_text)],
        reads=[{'result': 'success', 'remember': True}])
    eviction = [packet(f'type=read:pw={b64(f"p{i}")}:name={b64("app")}', read_text) for i in range(33)]
    add('read-grant-eviction', eviction + [packet(f'type=read:pw={b64("p0")}:name={b64("app")}', read_text),
                                           packet(f'type=read:pw={b64("p32")}:name={b64("app")}', read_text)],
        reads=[{'result': 'success', 'remember': True}] * 33)

    # Writes: types, chunks, reuse, aliases and their resolution.
    def data(mime, payload, bel=False):
        return packet('type=wdata:mime=' + b64(mime), payload, bel)

    add('write-basic', [packet('type=write:id=42'), data(TEXT, encoded(b'Ghost')), data(TEXT, encoded(b'ty')),
                        data(HTML, encoded(b'<b>hi</b>')), packet('type=walias:mime=' + b64(TEXT), encoded(b'TEXT UTF8_STRING')),
                        packet('type=wdata')])
    add('write-reuse', [packet('type=write'), data(TEXT, encoded(b'old')), data(HTML, encoded(b'h')),
                        data(TEXT, encoded(b'new')), packet('type=wdata')])
    add('write-empty-commit', [packet('type=write:loc=primary', bel=True), packet('type=wdata', bel=True)])
    add('write-empty-type', [packet('type=write'), data(TEXT, b''), data(HTML, encoded(b'')), packet('type=wdata')])
    add('write-commit-twice', [packet('type=write:id=w'), packet('type=wdata'), packet('type=wdata')])
    add('write-orphans', [data(TEXT, encoded(b'x')), packet('type=walias:mime=' + b64(TEXT), encoded(b'TEXT')),
                          packet('type=wdata')])
    add('write-replaced', [packet('type=write:id=old'), data(TEXT, encoded(b'old')), packet('type=write:id=new'),
                           data(TEXT, encoded(b'new')), packet('type=wdata')])
    add('write-terminators', [packet('type=write:id=t'), data(TEXT, encoded(b'x'), bel=True), packet('type=wdata', bel=True)])
    add('write-many-types', [packet('type=write')] + [data(f'x/{i}', encoded(b'%d' % i)) for i in range(66)] +
        [data('x/0', encoded(b'again')), packet('type=wdata')])
    add('write-aliases', [packet('type=write'), data(TEXT, encoded(b'hi')),
                          packet('type=walias:mime=' + b64(TEXT), encoded(b'a/1  a/2\nTEXT')),
                          packet('type=walias:mime=' + b64('a/1'), encoded(b'a/3')),
                          packet('type=walias:mime=' + b64(PNG), encoded(b'a/4')),
                          packet('type=walias:mime=' + b64(HTML), encoded(b'a/2')),
                          data(HTML, encoded(b'<i>')),
                          packet('type=walias:mime=' + b64(TEXT), encoded(b'text/html')),
                          packet('type=walias:mime=' + b64(TEXT), encoded(b'x/' + b'y' * 300 + b' a/5')),
                          packet('type=wdata')])
    add('write-many-aliases', [packet('type=write'), data(TEXT, encoded(b'hi'))] +
        [packet('type=walias:mime=' + b64(TEXT), encoded(b' '.join(b'al/%d' % (j * 10 + i) for i in range(10))))
         for j in range(7)] + [packet('type=wdata')])
    add('write-alias-before-data', [packet('type=write'), packet('type=walias:mime=' + b64(TEXT), encoded(b'TEXT')),
                                    data(TEXT, encoded(b'later')), packet('type=wdata')])

    # Base64 streams: split at every boundary, independently padded packets,
    # and every way a stream can be corrupt.
    stream = encoded(b'some data, then more')
    for cut in range(1, len(stream)):
        add(f'write-split-{cut}', [packet('type=write'), data(TEXT, stream[:cut]), data(TEXT, stream[cut:]),
                                   packet('type=wdata')])
    for name, parts in [('padded-each', [encoded(b'good'), encoded(b'more')]), ('padded-then-split', [encoded(b'x'), b'bW9', b'yZQ==']),
                        ('pad-then-more', [encoded(b'good') + encoded(b'more')]), ('bad-char', [b'SGVs!!!bG8=']),
                        ('newline', [b'\nZGF0YQ==']), ('unpadded-commit', [b'SGVsbG8']),
                        ('lone-pad', [b'=']), ('pad-third', [b'SG=v']), ('empty-chunks', [b'', b'SGk=', b'']),
                        ('trailing-bits', [b'SGl=']), ('split-pad', [b'SGk', b'=']), ('three-pad', [b'S===']),
                        ('two-then-pad', [b'SG', b'==']), ('carry-bad', [b'SG', b'!='])]:
        add(f'write-stream-{name}', [packet('type=write:id=s')] + [data(TEXT, part) for part in parts] + [packet('type=wdata')])
    add('write-unpadded-switch', [packet('type=write:id=s'), data(TEXT, b'SGVsbG8'), data(HTML, encoded(b'x')),
                                  packet('type=wdata')])
    for name, bad in [('alias-utf8', packet('type=walias:mime=' + b64(TEXT), b'/w==')),
                      ('alias-no-target', packet('type=walias', encoded(b'TEXT'))),
                      ('alias-bad-target', packet('type=walias:mime=//4=', encoded(b'TEXT'))),
                      ('alias-bad-list', packet('type=walias:mime=' + b64(TEXT), b'!!!')),
                      ('data-bad-mime', packet('type=wdata:mime=//4=', encoded(b'x'))),
                      ('data-unpadded-mime', packet('type=wdata:mime=dGV4dC9wbGFpbg', encoded(b'x'))),
                      ('data-long-mime', packet('type=wdata:mime=' + b64('m' * 257), encoded(b'x'))),
                      ('data-bad-pw', packet('type=wdata:pw=!!!', encoded(b'x'))),
                      ('data-long-pw', packet('type=wdata:mime=' + b64(TEXT) + ':pw=' + b64('p' * 129), encoded(b'x'))),
                      ('read-bad-mime', packet('type=read:mime=!!!', read_text)),
                      ('write-bad-mime', packet('type=write:mime=!!!')),
                      ('malformed-record', packet('type=wdata:bare', encoded(b'x'))),
                      ('unknown-type', packet('type=wbobr:mime=!!!', encoded(b'x')))]:
        add(f'write-invalid-{name}', [packet('type=write:id=w'), data(TEXT, encoded(b'Ghost')), bad,
                                      data(TEXT, encoded(b'World')), packet('type=wdata')])

    # Limits: across types, exactly at the limit, and one past it.
    for limit in [0, 4, 5, 9, 10]:
        add(f'write-limit-{limit}', [packet('type=write:id=w'), data(TEXT, encoded(b'Hell')), data(HTML, encoded(b'o')),
                                     data(PNG, encoded(b'worl')), data(TEXT, encoded(b'd')), packet('type=wdata')],
            write_limit=limit)

    # Decisions: every result, and grants.
    for result in ['denied', 'unsupported', 'busy', 'invalid_data', 'io_error']:
        add(f'write-result-{result}', [packet('type=write:id=r'), data(TEXT, encoded(b'x')), packet('type=wdata')],
            writes=[{'result': result}])
    named = 'type=write:pw=' + b64('secret') + ':name=' + b64('app')
    add('write-grant', [packet(named), packet('type=wdata'), packet(named), packet('type=wdata'),
                        packet('type=write:pw=' + b64('other') + ':name=' + b64('app')), packet('type=wdata'),
                        packet(secret, read_text)],
        writes=[{'result': 'success', 'remember': True}])
    add('write-grant-only-on-success', [packet(named), packet('type=wdata'), packet(named), packet('type=wdata')],
        writes=[{'result': 'busy', 'remember': True}])

    # Mode 5522 and paste events.
    query = b'\x1b[?5522$p'
    add('mode-5522', [query, b'\x1b[?5522h' + query, b'\x1b[?5522s\x1b[?5522l' + query, b'\x1b[?5522r' + query,
                      b'\x1b[!p' + query, b'\x1b[?5522h\x1bc' + query, b'\x1b[?5522;2004h\x1b[?5522;2004$p'])
    entropy = bytes(range(256)) * 4
    first, at = otp(entropy)
    second, _ = otp(entropy, at)
    as_paste = ':name=' + b64('Paste event')
    add('paste-event', [b'\x1b[?5522h', paste(),
                        packet(f'type=read:pw={b64(first)}{as_paste}', encoded(b'.')),
                        packet(f'type=read:pw={b64(first)}{as_paste}', encoded(b'image/png text/plain')),
                        packet(f'type=read:pw={b64(first)}{as_paste}', encoded(b'image/png')),
                        paste('primary', [TEXT]), packet(f'type=read:loc=primary:pw={b64(second)}{as_paste}', read_text)],
        entropy=entropy)
    add('paste-event-wrong-direction', [b'\x1b[?5522h', paste(),
                                        packet(f'type=write:pw={b64(first)}{as_paste}'), packet('type=wdata'),
                                        packet(f'type=read:pw={b64(first)}{as_paste}', read_text)], entropy=entropy)
    add('paste-event-nameless', [b'\x1b[?5522h', paste(), packet(f'type=read:pw={b64(first)}', read_text),
                                 packet(f'type=read:pw={b64(first)}{as_paste}', read_text)], entropy=entropy)
    add('paste-event-reset', [b'\x1b[?5522h', paste(), b'\x1bc\x1b[?5522h',
                              packet(f'type=read:pw={b64(first)}{as_paste}', read_text)], entropy=entropy)
    add('paste-event-listing', [b'\x1b[?5522h', paste(mimes=[f'type/{i:02}' for i in range(20)]), paste(mimes=[])],
        entropy=entropy)
    rejecting = bytes([255, 250, 228, 0, 229, 56, 57, 113, 114, 227] * 30)
    add('paste-event-rejection-sampling', [b'\x1b[?5522h', paste(), paste()], entropy=rejecting)
    add('paste-event-no-entropy', [b'\x1b[?5522h', paste()], entropy=bytes([255]) * 100)
    add('paste-event-off', [paste(mimes=[PNG]), b'\x1b[?5522h\x1b[?5522l', paste(mimes=[PNG])], entropy=entropy)

    # A reset keeps a write in flight, and ends grants.
    add('reset-keeps-write', [packet('type=write:id=w'), data(TEXT, encoded(b'kept')), b'\x1bc', packet('type=wdata')])
    add('reset-ends-grants', [packet(secret, read_text), b'\x1bc', packet(secret, read_text)],
        reads=[{'result': 'success', 'remember': True}])

    # The OSC capture around its 8 MiB, fed only in large chunks. The last
    # payload byte isn't base64: a packet taken whole is answered EINVAL at
    # once, and one dropped leaves the commit to answer DONE.
    limit = 8 * 1024 * 1024
    prefix = b'type=wdata:mime=' + b64(TEXT).encode() + b';'
    for name, size in [('below', limit - 1), ('at', limit), ('past', limit + 1)]:
        body = prefix + b'A' * (size - len(prefix) - 1) + b'!'
        add(f'capture-{name}', [packet('type=write:id=c'), b'\x1b]5522;' + body + b'\x1b\\', packet('type=wdata')],
            chunks=[65536])

    # Hotaru's own bound, named: a feed's clipboard requests past 64.
    add('bounded-prefix', [packet('type=read:id=b', read_text) * 70], hotaru_difference='bounded-event-prefix')
    return out


class Ghostty:
    def __init__(self, library):
        self.oracle = Oracle(library)
        self.lib = self.oracle.lib
        manifest = self.oracle.manifest['types']
        for name, layout in LAYOUTS.items():
            expected = manifest[name]
            if C.sizeof(layout) != expected['size'] or C.alignment(layout) != expected['align']:
                raise RuntimeError(f'ABI size/alignment mismatch for {name}')
            for field, typ in layout._fields_:
                actual = expected['fields'][field]
                if getattr(layout, field).offset != actual['offset'] or C.sizeof(typ) != actual['size']:
                    raise RuntimeError(f'ABI field mismatch for {name}.{field}')
        self.lib.ghostty_terminal_paste.argtypes = [C.c_void_p, C.POINTER(Paste), C.POINTER(C.c_bool)]
        self.lib.ghostty_terminal_paste.restype = C.c_int
        self.lib.ghostty_sys_set.argtypes = [C.c_int, C.c_void_p]
        self.lib.ghostty_sys_set.restype = C.c_int
        self.entropy, self.entropy_at, self.entropy_requests = b'', 0, []

        def random(userdata, buffer, length):
            block = self.entropy[self.entropy_at:self.entropy_at + length]
            self.entropy_requests.append(length)
            if len(block) < length:
                return False
            C.memmove(buffer, block, length)
            self.entropy_at += length
            return True

        self.random = RANDOM(random)
        checked(self.lib.ghostty_sys_set(self.oracle.enum('GhosttySysOption', 'RANDOM_SECURE'), C.cast(self.random, C.c_void_p)))
        self.locations = {self.oracle.enum('GhosttyClipboardLocation', 'STANDARD'): 'standard',
                          self.oracle.enum('GhosttyClipboardLocation', 'PRIMARY'): 'primary',
                          self.oracle.enum('GhosttyClipboardLocation', 'SELECTION'): 'selection'}

    def run(self, case, chunk):
        option = lambda name: self.oracle.enum('GhosttyTerminalOption', name)
        terminal = C.c_void_p()
        checked(self.lib.ghostty_terminal_new(None, C.byref(terminal), 80, 24))
        keep = []
        clipboards = {location: [(mime, bytes.fromhex(data)) for mime, data in contents]
                      for location, contents in case['clipboards'].items()}
        reads, writes = list(case['reads']), list(case['writes'])
        step = None

        def string(value):
            buffer = C.create_string_buffer(value, len(value))
            keep.append(buffer)
            return String(C.cast(buffer, C.c_void_p), len(value))

        def text(value):
            return C.string_at(value.ptr, value.len) if value.len else b''

        def pty(_terminal, _userdata, pointer, length):
            step['pty'] += C.string_at(pointer, length)

        def read(_terminal, _userdata, pointer):
            request = pointer.contents
            mimes = [text(request.mimes[i]).decode('utf-8', 'surrogateescape') for i in range(request.mimes_len)]
            location = self.locations[request.location]
            step['requests'].append({'kind': 'read', 'location': location, 'mimes': mimes, 'list': request.list,
                                     'name': text(request.name).decode('utf-8', 'surrogateescape'),
                                     'granted': request.granted, 'can_remember': request.can_remember})
            decision = reads.pop(0) if reads else {'result': 'success'}
            held = clipboards.get(location, [])
            contents = (Content * max(1, len(held)))(*(Content(string(mime.encode()), string(data)) for mime, data in held))
            available = (String * max(1, len(held)))(*(string(mime.encode()) for mime, _ in held))
            keep.extend([contents, available])
            reply = ReadReply(C.sizeof(ReadReply), self.oracle.enum('GhosttyClipboardReadResult', decision['result'].upper()),
                              contents, len(held), available, len(held), decision.get('remember', False))
            request.reply(pointer, C.byref(reply))

        def write(_terminal, _userdata, pointer):
            request = pointer.contents
            contents = [(text(request.contents[i].mime).decode('utf-8', 'surrogateescape'), text(request.contents[i].data))
                        for i in range(request.contents_len)]
            location = self.locations[request.location]
            step['requests'].append({'kind': 'write', 'location': location,
                                     'contents': [[mime, data.hex()] for mime, data in contents],
                                     'name': text(request.name).decode('utf-8', 'surrogateescape'),
                                     'granted': request.granted, 'can_remember': request.can_remember})
            decision = writes.pop(0) if writes else {'result': 'success'}
            if decision['result'] == 'success':
                clipboards[location] = contents
            reply = WriteReply(C.sizeof(WriteReply), self.oracle.enum('GhosttyClipboardWriteResult', decision['result'].upper()),
                               decision.get('remember', False))
            request.reply(pointer, C.byref(reply))

        def mime_reader(_userdata, _mime, _writer):
            # Only a text paste reads data; the corpus pastes events.
            step['requests'].append({'kind': 'paste-read'})
            return False

        callbacks = [PTY_CALLBACK(pty), READ_CALLBACK(read), WRITE_CALLBACK(write), MIME_READ(mime_reader)]
        try:
            for name, callback in zip(['WRITE_PTY', 'CLIPBOARD_READ', 'CLIPBOARD_WRITE'], callbacks):
                checked(self.lib.ghostty_terminal_set(terminal, option(name), C.cast(callback, C.c_void_p)))
            if case['write_limit'] is not None:
                limit = C.c_size_t(case['write_limit'])
                checked(self.lib.ghostty_terminal_set(terminal, option('CLIPBOARD_WRITE_MAX_BYTES'), C.byref(limit)))
            self.entropy, self.entropy_at, self.entropy_requests = bytes.fromhex(case['entropy']), 0, []
            observed = []
            for item in case['steps']:
                step = {'pty': b'', 'requests': []}
                if 'feed' in item:
                    data = bytes.fromhex(item['feed'])
                    for start in range(0, len(data), chunk):
                        part = data[start:start + chunk]
                        self.lib.ghostty_terminal_vt_write(terminal, part, len(part))
                else:
                    location = self.oracle.enum('GhosttyClipboardLocation', item['paste']['location'].upper())
                    mimes = [string(mime.encode()) for mime in item['paste']['mimes']]
                    array = (String * max(1, len(mimes)))(*mimes)
                    paste = Paste(C.sizeof(Paste), location, self.oracle.enum('GhosttyPasteSource', 'CLIPBOARD'),
                                  array, len(mimes), MimeReader(callbacks[3], None), False)
                    written = C.c_bool(False)
                    result = self.lib.ghostty_terminal_paste(terminal, C.byref(paste), C.byref(written))
                    step['requests'].append({'kind': 'paste', 'written': bool(result == 0 and written.value)})
                observed.append({'pty': step['pty'].hex(), 'requests': step['requests']})
            processing_error = self.oracle.query('ghostty_terminal_get', terminal, 'GhosttyTerminalData',
                                                 'VT_PROCESSING_ERROR', C.c_bool)
            if processing_error:
                raise RuntimeError(f'reference terminal processing failed in {case["name"]}')
            return {'steps': observed, 'entropy_requests': self.entropy_requests}
        finally:
            self.lib.ghostty_terminal_free(terminal)


def summarize(records):
    return {'comparisons': len(records),
            'failures': sum(bool(r['differences'] or r['reference_chunk_differences'] or r['process_failed'])
                            and not r['hotaru_difference'] for r in records),
            'named_hotaru_differences': sum(bool(r['differences']) and bool(r['hotaru_difference']) for r in records)}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--replay', type=Path, required=True)
    parser.add_argument('--library', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    parser.add_argument('--fixtures', type=Path)
    args = parser.parse_args()
    with Path('/home/callen/.cache/hotaru-workstation.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        assert digest(args.library) == LIBRARY_SHA256
        ghostty = Ghostty(args.library)
        cases = scenarios()
        records, fixtures = [], []
        with tempfile.TemporaryDirectory() as temporary:
            expected_by_chunk = {}
            for chunk in CHUNKS:
                runnable = [case for case in cases if chunk in case.get('chunks', CHUNKS)]
                path = Path(temporary) / f'scenarios-{chunk}.json'
                path.write_text(json.dumps(runnable))
                result = subprocess.run([str(args.replay), '--scenarios', str(path), '--chunk', str(chunk)],
                                        capture_output=True, text=True, timeout=600)
                actual = json.loads(result.stdout) if result.returncode == 0 else None
                for index, case in enumerate(runnable):
                    reference = ghostty.run(case, chunk)
                    baseline = expected_by_chunk.setdefault(case['name'], reference)
                    hotaru = actual[index] if actual is not None else None
                    delta = differences(reference['steps'], hotaru['steps']) if hotaru is not None else [result.stderr[-2000:]]
                    record = {'name': case['name'], 'chunk': chunk, 'differences': delta,
                              'reference_chunk_differences': differences(baseline['steps'], reference['steps']),
                              'hotaru_difference': case.get('hotaru_difference'), 'process_failed': result.returncode != 0}
                    if delta and len(json.dumps(reference)) < 200_000:
                        record.update(reference=reference['steps'], actual=hotaru and hotaru['steps'])
                    records.append(record)
                    if chunk == max(case.get('chunks', CHUNKS)) and not case.get('hotaru_difference') \
                            and len(json.dumps(case)) < 200_000:
                        fixtures.append({**case, 'expected': reference['steps']})
        source = Path(__file__).resolve().parents[1]
        sources = sorted([*(source / 'crates/hotaru-core/src').rglob('*.rs'),
                          *(source / 'crates/hotaru-replay/src').rglob('*.rs')])
        report = {'upstream_revision': '6ba144158577d033ccb6ed4b574e4013957c9161', 'library_sha256': digest(args.library),
                  'replay_sha256': digest(args.replay), 'tool_sha256': digest(__file__),
                  'sources': {str(p.relative_to(source)): digest(p) for p in sources}, 'scenarios': len(cases),
                  **summarize(records), 'records': records}
        args.report.write_text(json.dumps(report, indent=2) + '\n')
        if args.fixtures:
            args.fixtures.write_text(json.dumps({'library_sha256': report['library_sha256'], 'cases': fixtures},
                                                separators=(',', ':')) + '\n')
        print(json.dumps({k: v for k, v in report.items() if k not in ['records', 'sources']}))


if __name__ == '__main__':
    main()
