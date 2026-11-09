# Start a VM, and start it again if QEMU never connects to the test driver.
#
# The driver fails at once when QEMU exits before connecting, but waits for
# the test's whole timeout when it never connects and never exits: on loaded
# hosts "starting vm" was the last line until the timeout (network-dns-vm's
# 15 minutes and tuning's hour, under load past 150; network-vm's 25 minutes,
# one of 16 test VMs started within 13 seconds). Log what the processes were
# waiting on, end them, and try once more.
import os
import threading
from pathlib import Path


def vm_processes(pid: int, depth: int = 0) -> list[str]:
    lines = []
    try:
        proc = Path("/proc") / str(pid)
        state = next(line for line in (proc / "status").read_text().splitlines() if line.startswith("State:"))
        command = (proc / "cmdline").read_bytes().replace(b"\0", b" ").decode(errors="replace")[:160]
        lines.append(f"{'  ' * depth}{pid} {state.split(None, 1)[1]} {command}")
        for task in sorted((proc / "task").iterdir()):
            wchan = (task / "wchan").read_text() or "-"
            lines.append(f"{'  ' * depth}  task {task.name} {(task / 'comm').read_text().strip()} waits in {wchan}")
            for child in (task / "children").read_text().split():
                lines += vm_processes(int(child), depth + 1)
    except (OSError, StopIteration):
        pass
    return lines


def vm_pids(pid: int) -> list[int]:
    pids = [pid]
    try:
        for task in (Path("/proc") / str(pid) / "task").iterdir():
            for child in (task / "children").read_text().split():
                pids += vm_pids(int(child))
    except OSError:
        pass
    return pids


def start_vm(machine, seconds: int = 300, attempts: int = 2) -> None:
    for attempt in range(1, attempts + 1):
        connected = threading.Event()

        def watch(attempt: int = attempt) -> None:
            if connected.wait(seconds):
                return
            process = machine.process
            if process is None or process.poll() is not None:
                return
            machine.log(f"QEMU did not connect in {seconds} s (attempt {attempt}):\n" + "\n".join(vm_processes(process.pid)))
            # Every process that shares QEMU's output pipe, so that the
            # driver's read of it ends.
            for pid in reversed(vm_pids(process.pid)):
                try:
                    os.kill(pid, 9)
                except OSError:
                    pass

        threading.Thread(target=watch, daemon=True).start()
        try:
            machine.start()
            return
        except Exception:
            if attempt == attempts:
                raise
            machine.log(f"starting the VM again (attempt {attempt + 1})")
        finally:
            connected.set()
