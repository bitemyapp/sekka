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


def unit_state(unit):
    return ctl('show', '-p', 'ActiveState', '--value', unit).stdout.strip()


def state():
    return unit_state('aizome.service')


def file_manager_state():
    return unit_state('aizome-file-manager.service')


def watch_state():
    return unit_state('aizome-session-watch.service')


def environment():
    return set(ctl('show-environment').stdout.splitlines())


# The user manager's environment before any session: each one must leave
# it as it found it, its variables and the compositor's removed.
BASELINE = environment()


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


def ended():
    until(lambda: state() == 'inactive')
    until(lambda: file_manager_state() == 'inactive')
    until(lambda: watch_state() == 'inactive')
    after = environment()
    assert after == BASELINE, {'left': sorted(after - BASELINE), 'lost': sorted(BASELINE - after)}


def terminated(process, sig=signal.SIGTERM, group=False):
    (os.killpg if group else os.kill)(process.pid, sig)
    assert process.wait(timeout=5) == 128 + sig
    ended()


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
    # Another desktop's graphical session, once started, has not started
    # Hako's provider, and Aizome's activation files leave FileManager1 to
    # other file managers. The portal backend's own file shows they're read.
    subprocess.run(['systemd-run', '--user', '--unit=other-desktop',
                    '--property=BindsTo=graphical-session.target', 'sleep', 'infinity'],
                   env=ENV, check=True)
    until(lambda: ctl('is-active', 'graphical-session.target', check=False).returncode == 0)
    until(lambda: 'No jobs running.' in ctl('list-jobs').stdout)
    names = subprocess.run(['busctl', '--user', 'call', 'org.freedesktop.DBus', '/org/freedesktop/DBus',
                            'org.freedesktop.DBus', 'ListActivatableNames'],
                           env=ENV, check=True, text=True, capture_output=True).stdout
    assert '"org.freedesktop.impl.portal.desktop.aizome"' in names, names
    assert '"org.freedesktop.FileManager1"' not in names, names
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
        owned = environment()
        duplicate = start(gate + '-duplicate')
        assert duplicate.wait(timeout=5) == 1
        assert 'already running' in (ROOT / (gate + '-duplicate.log')).read_text()
        assert not (ROOT / ('entered-' + gate + '-duplicate')).exists()
        assert owner.poll() is None
        assert environment() == owned
        (ROOT / gate).unlink()
        until(lambda: state() == 'active')
        terminated(owner, group=True)
        relogin(gate + '-next')
        print(gate + ': duplicate refused, owner survived, relogin succeeded', flush=True)

    # Killed outright, the launcher runs no trap. The watch takes the lock
    # the kernel released with it and ends the session in its place.
    process = start('killed')
    until(lambda: state() == 'active')
    assert watch_state() == 'active'
    process.kill()
    assert process.wait(timeout=5) == -signal.SIGKILL
    ended()
    relogin('killed-next')
    print('killed launcher: the watch ended its session, relogin succeeded', flush=True)

    # A stop timeout kills what is left of the login scope while cleanup
    # still waits for a compositor slow to stop. The watch finishes ending
    # the session; a login meanwhile is refused, not started into it.
    (ROOT / 'hold-stop').touch()
    process = start('killed-in-cleanup')
    until(lambda: state() == 'active')
    os.kill(process.pid, signal.SIGTERM)
    until(lambda: state() == 'deactivating')
    os.killpg(process.pid, signal.SIGKILL)
    assert process.wait(timeout=5) == -signal.SIGKILL
    duplicate = start('during-watch')
    assert duplicate.wait(timeout=5) == 1
    assert 'already running' in (ROOT / 'during-watch.log').read_text()
    assert state() == 'deactivating'
    (ROOT / 'hold-stop').unlink()
    ended()
    relogin('killed-in-cleanup-next')
    print('killed during cleanup: the watch finished it, a login meanwhile was refused', flush=True)

    # A login resets the failures of the session's own units, which may
    # need to start again, and leaves other user services' failures alone.
    for unit, bound in (('unrelated-failure', False), ('session-failure', True)):
        subprocess.run(['systemd-run', '--user', '--quiet', '--unit=' + unit,
                        *(['--property=PartOf=graphical-session.target'] if bound else []), 'false'],
                       env=ENV, check=True)
        until(lambda: unit_state(unit + '.service') == 'failed')
    process = start('reset-failed')
    until(lambda: state() == 'active')
    assert unit_state('session-failure.service') == 'inactive'
    assert unit_state('unrelated-failure.service') == 'failed'
    terminated(process, group=True)
    ctl('reset-failed', 'unrelated-failure.service')
    print('reset-failed: the session unit reset, the unrelated failure kept', flush=True)

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
