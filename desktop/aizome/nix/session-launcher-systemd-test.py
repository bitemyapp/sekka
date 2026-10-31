"""Runs as the disposable VM's alice, never against a workstation manager."""

import os
from pathlib import Path
import signal
import subprocess
import time


ROOT = Path('/home/alice/launcher-test')
ROOT.mkdir(exist_ok=True)
CONFIG = ROOT / 'config/aizome'
CONFIG.mkdir(parents=True, exist_ok=True)
WRAPPER = Path('/etc/launcher-test/wrapper-path').read_text().strip()
(CONFIG / 'environment').write_text(f'PATH={WRAPPER}:{os.environ["PATH"]}\n')
ENV = dict(os.environ, HOME='/home/alice', XDG_CONFIG_HOME=str(CONFIG.parent),
           DBUS_SESSION_BUS_ADDRESS='unix:path=/run/user/1000/bus', TEST_DIR=str(ROOT))
processes = []


def ctl(*args, check=True):
    return subprocess.run(['/run/current-system/sw/bin/systemctl', '--user', *args],
                          env=ENV, check=check, text=True, capture_output=True)


def state():
    return ctl('show', '-p', 'ActiveState', '--value', 'aizome.service').stdout.strip()


def file_manager_state():
    return ctl('show', '-p', 'ActiveState', '--value', 'aizome-file-manager.service').stdout.strip()


def until(predicate):
    deadline = time.monotonic() + 10
    while not predicate():
        assert time.monotonic() < deadline, ('timed out', state())
        time.sleep(.02)


def start(name):
    with (ROOT / (name + '.log')).open('w') as log:
        process = subprocess.Popen(['sh', '/etc/launcher-test/launcher', '--no-login-shell'],
            env=dict(ENV, TEST_LOGIN=name), start_new_session=True,
            stdout=log, stderr=subprocess.STDOUT)
    processes.append(process)
    return process


def terminated(process, sig=signal.SIGTERM, group=False):
    (os.killpg if group else os.kill)(process.pid, sig)
    assert process.wait(timeout=5) == 128 + sig
    until(lambda: state() == 'inactive')
    until(lambda: file_manager_state() == 'inactive')
    environment = ctl('show-environment').stdout
    assert 'XDG_SESSION_DESKTOP=' not in environment, environment


def relogin(name):
    process = start(name)
    until(lambda: state() == 'active')
    until(lambda: file_manager_state() == 'active')
    until(lambda: ctl('is-active', 'xdg-desktop-autostart.target', check=False).returncode == 0)
    # The bus owner must be ready before autostarted browsers run.
    ready = int(ctl('show', '-p', 'ActiveEnterTimestampMonotonic', '--value',
                    'aizome-file-manager.service').stdout)
    autostart = int(ctl('show', '-p', 'ActiveEnterTimestampMonotonic', '--value',
                       'xdg-desktop-autostart.target').stdout)
    assert 0 < ready <= autostart, (ready, autostart)
    terminated(process, group=True)


try:
    # Another desktop's graphical session does not start Hako's provider.
    subprocess.run(['systemd-run', '--user', '--unit=other-desktop',
                    '--property=BindsTo=graphical-session.target', 'sleep', 'infinity'],
                   env=ENV, check=True)
    assert file_manager_state() == 'inactive'
    ctl('start', 'aizome-shutdown.target')
    # Before StartUnit has even been sent: a completed shutdown target
    # cannot stop a start submitted later. The client must already be gone.
    for name, sig, group in [('pending-term', signal.SIGTERM, False),
                             ('pending-hup', signal.SIGHUP, False),
                             ('pending-group-int', signal.SIGINT, True)]:
        (ROOT / 'hold-start').touch()
        process = start(name)
        until(lambda: (ROOT / ('entered-' + name)).exists())
        child = int((ROOT / ('client-' + name)).read_text())
        terminated(process, sig, group)
        assert not Path(f'/proc/{child}').exists(), 'start client survived cleanup'
        (ROOT / 'hold-start').unlink()
        assert state() == 'inactive'
        relogin(name + '-next')
        print(name + ': stopped and relogin succeeded', flush=True)

    # The opposite ordering: a real start transaction is queued and the
    # notify service has not become ready. Cleanup must cancel that job too.
    for group in (False, True):
        (ROOT / 'hold-ready').touch()
        process = start('submitted-' + str(group))
        until(lambda: state() == 'activating')
        terminated(process, group=group)
        (ROOT / 'hold-ready').unlink()
        relogin('submitted-next-' + str(group))
        print(f'submitted start, group={group}: stopped and relogin succeeded', flush=True)

    # The owner holds the lock both before submission and before READY=1.
    # Refused duplicates must not stop it, clear its environment, or start
    # a second service client. The duplicate exits without running cleanup.
    for gate in ('hold-start', 'hold-ready'):
        (ROOT / gate).touch()
        owner = start(gate + '-owner')
        if gate == 'hold-start':
            until(lambda: (ROOT / ('entered-' + gate + '-owner')).exists())
        else:
            until(lambda: state() == 'activating')
        environment = ctl('show-environment').stdout
        duplicate = start(gate + '-duplicate')
        assert duplicate.wait(timeout=5) == 1
        assert 'already running' in (ROOT / (gate + '-duplicate.log')).read_text()
        assert not (ROOT / ('entered-' + gate + '-duplicate')).exists()
        assert owner.poll() is None
        assert ctl('show-environment').stdout == environment
        (ROOT / gate).unlink()
        until(lambda: state() == 'active')
        terminated(owner, group=True)
        relogin(gate + '-next')
        print(gate + ': duplicate refused, owner survived, relogin succeeded', flush=True)

    # No lock is held by a service started manually or by an old launcher.
    (ROOT / 'hold-ready').touch()
    ctl('--no-block', 'start', 'aizome.service')
    until(lambda: state() == 'activating')
    duplicate = start('unowned-activating')
    assert duplicate.wait(timeout=5) == 1
    assert state() == 'activating'
    (ROOT / 'hold-ready').unlink()
    until(lambda: state() == 'active')
    duplicate = start('unowned-active')
    assert duplicate.wait(timeout=5) == 1
    assert state() == 'active'
    print('unowned services: activating and active both refused', flush=True)
finally:
    for process in processes:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait()
    ctl('start', '--job-mode=replace-irreversibly', 'aizome-shutdown.target')
    until(lambda: state() in ('inactive', 'failed'))
