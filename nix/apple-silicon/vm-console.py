"""Run commands on boot-image-in-vm.sh's virtio consoles.

Usage:
  vm-console.py SOCKET USER PASSWORD COMMAND...
      log in on the getty (the developer image's account) and run commands;
  vm-console.py --shell [--timeout SECONDS] SOCKET COMMAND...
      use the root shell already on the socket (systemd's debug shell, which
      the harness enables on hvc1 for the product image, which has no
      account) and run commands, each allowed SECONDS (default 180).

A COMMAND starting with `!` is typed without waiting for it to finish
(`!systemctl poweroff`).

Prints each command with its output. The console drops input typed faster
than a person types, so lines go in small pieces.
"""

import re
import socket
import sys
import time

args = sys.argv[1:]
shell = args[:1] == ["--shell"]
timeout = 180
if shell:
    args = args[1:]
    if args[:1] == ["--timeout"]:
        timeout = int(args[1])
        args = args[2:]
    sock_path, commands = args[0], args[1:]
else:
    sock_path, user, password = args[0], args[1], args[2]
    commands = args[3:]
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


if shell:
    type_line("")
    read_until(r"# ", 30)
else:
    type_line("")
    read_until("login:", 30)
    type_line(user)
    read_until("assword:", 30)
    type_line(password)
    read_until(r"\$ ", 60)
if shell:
    # systemd's debug shell starts with a minimal PATH.
    type_line("export PATH=/run/wrappers/bin:/run/current-system/sw/bin:$PATH")
    read_until(r"# ", 10)
type_line("export TERM=dumb PAGER=cat SYSTEMD_PAGER=cat PS1='$ '")
read_until(r"\$ ", 10)
for i, command in enumerate(commands):
    if command.startswith("!"):
        # Sent without waiting for its end (systemctl poweroff).
        type_line(command[1:])
        print(f"$ {command[1:]}", flush=True)
        time.sleep(2)
        continue
    # The marker is split in the command, so only the output contains it whole.
    type_line(f"{{ {command} ; }} 2>&1; echo @@EN''D{i}@@")
    out = clean(read_until(f"@@END{i}@@", timeout))
    body = out.split("\n", 1)[1] if "\n" in out else out
    print(f"$ {command}\n{body.split(f'@@END{i}@@')[0].strip()}\n", flush=True)
    read_until(r"\$ ", 5)
if not shell:
    type_line("exit")
