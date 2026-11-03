#!/usr/bin/env python3
"""A program for a terminal's PTY (docs/performance/why-toolkits.md), in raw
mode as the terminal comparison's program was: for each byte typed, answer
with the character and a status line at the bottom, in one write (as that
program did), or in two (`responder.py two`), as many programs do."""
import os
import sys
import termios
import tty

two = sys.argv[1:] == ["two"]
fd = sys.stdin.fileno()
old = termios.tcgetattr(fd)
tty.setraw(fd)
try:
    os.write(1, b"\x1b[2J\x1b[H")
    n = 0
    while True:
        b = os.read(fd, 64)
        if not b or b"\x03" in b:
            break
        for ch in b:
            n += 1
            c = b"x" if ch == 0x7f else bytes([ch])
            first = b"\x1b[10;%dH" % (10 + n % 40) + c
            second = b"\x1b[30;1Hkeys %d   " % n
            if two:
                os.write(1, first)
                os.write(1, second)
            else:
                os.write(1, first + second)
finally:
    termios.tcsetattr(fd, termios.TCSADRAIN, old)
