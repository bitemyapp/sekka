"""Log in on boot-image-in-vm.sh's virtio console and run commands.

Usage: vm-console.py SOCKET USER PASSWORD COMMAND...
Prints each command with its output. The console drops input typed faster
than a person types, so lines go in small pieces.
"""

import re
import socket
import sys
import time

sock_path, user, password = sys.argv[1], sys.argv[2], sys.argv[3]
commands = sys.argv[4:]
s = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
s.connect(sock_path)
s.settimeout(0.5)


def read_until(pattern, timeout=60):
    buf = b""
    end = time.time() + timeout
    while time.time() < end:
        try:
            chunk = s.recv(65536)
            if chunk:
                buf += chunk
                if re.search(pattern.encode(), buf):
                    break
        except socket.timeout:
            pass
    return buf.decode(errors="replace")


def type_line(text):
    for i in range(0, len(text), 16):
        s.sendall(text[i : i + 16].encode())
        time.sleep(0.05)
    s.sendall(b"\n")


def clean(text):
    text = re.sub(r"\x1b\[[0-9;?]*[A-Za-z]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)", "", text)
    return text.replace("\r", "")


type_line("")
read_until("login:", 30)
type_line(user)
read_until("assword:", 30)
type_line(password)
read_until(r"\$ ", 60)
type_line("export TERM=dumb PAGER=cat SYSTEMD_PAGER=cat PS1='$ '")
read_until(r"\$ ", 10)
for i, command in enumerate(commands):
    # The marker is split in the command, so only the output contains it whole.
    type_line(f"{{ {command} ; }} 2>&1; echo @@EN''D{i}@@")
    out = clean(read_until(f"@@END{i}@@", 180))
    body = out.split("\n", 1)[1] if "\n" in out else out
    print(f"$ {command}\n{body.split(f'@@END{i}@@')[0].strip()}\n", flush=True)
    read_until(r"\$ ", 5)
type_line("exit")
