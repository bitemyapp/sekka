"""Exercise the packaged Sound window against an isolated PipeWire graph."""
import json
from pathlib import Path
import subprocess
import sys
import time

import pyatspi

OUT = Path(sys.argv[1])
processes = []


def start(*args, name=None):
    log = open(OUT / ((name or args[0]) + '.log'), 'w')
    process = subprocess.Popen(args, stdout=log, stderr=log)
    processes.append(process)
    return process


def command(*args):
    return subprocess.check_output(args, text=True, timeout=10).strip()


def pactl(*args):
    return command('pactl', *args)


def wait(check, message, seconds=25):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        try:
            result = check()
            if result:
                return result
        except Exception:
            pass
        time.sleep(.15)
    raise AssertionError(message)


def tree(node):
    yield node
    for child in node:
        if child:
            yield from tree(child)


def widgets(root=None):
    return list(tree(root or pyatspi.Registry.getDesktop(0)))


def find(name, role=None, root=None):
    return next((w for w in widgets(root) if w.name == name and
                 (role is None or w.getRoleName() == role)), None)


def state(widget, which):
    return widget.getState().contains(which)


def window():
    return next(w for w in widgets() if w.getRoleName() == 'frame' and w.name == 'Sound')


def reveal(widget):
    """Scroll the page until the widget is in view. GTK 4 does not report
    desktop origins; Xvfb has no window manager, so the window is at (0, 0)
    and window coordinates reach its controls."""
    height = window().queryComponent().getExtents(pyatspi.WINDOW_COORDS).height
    for _ in range(40):
        rect = widget.queryComponent().getExtents(pyatspi.WINDOW_COORDS)
        if rect.y < 60:
            button = '4'
        elif rect.y + rect.height > height - 40:
            button = '5'
        else:
            return rect
        # Over the page's margin, where the wheel scrolls the page rather
        # than a slider.
        command('xdotool', 'mousemove', '12', str(height // 2), 'click', button)
        time.sleep(.1)
    raise AssertionError(f'Could not scroll to {widget.name}')


def click(widget, x=None):
    rect = reveal(widget)
    x = rect.x + (rect.width // 2 if x is None else x(rect))
    command('xdotool', 'mousemove', str(x), str(rect.y + rect.height // 2), 'click', '1')


def choose_device(group, name):
    """Choose a device from the Output or Input list."""
    section = wait(lambda: find(group, 'grouping'), f'Missing {group}')
    row = wait(lambda: find(name, 'list item', section), f'Missing {name} in {group}')
    click(row)
    wait(lambda: state(find(name, 'list item', find(group, 'grouping')), pyatspi.STATE_SELECTED),
         f'{name} was not checked')


def press(name):
    button = wait(lambda: find(name, 'toggle button'), f'Missing {name}')
    button.queryAction().doAction(0)


def pressed(name):
    button = find(name, 'toggle button')
    return state(button, pyatspi.STATE_PRESSED) or state(button, pyatspi.STATE_CHECKED)


def control(row, role):
    """The control of this role in the row named so."""
    return wait(lambda: next(w for w in widgets(find(row, 'list item'))
                             if w.getRoleName() == role), f'Missing {role} in {row}')


def choose(widget, choice):
    """Choose from a drop-down, with the keyboard: GTK reports a popover's
    items some way above where they are drawn."""
    # Its popover's choices, not the device lists' rows of the same name.
    def options():
        items = [w for w in widgets(widget) if w.getRoleName() == 'list item']
        names = [next((c.name for c in tree(w) if c.getRoleName() == 'label'), '')
                 for w in items]
        return (items, names) if choice in names else None
    def focused():
        found = options()
        return found and any(state(w, pyatspi.STATE_FOCUSED)
                             for item in found[0] for w in tree(item))
    for _ in range(3):
        if not options():
            # The arrow, at the right of the drop-down.
            click(widget, lambda rect: rect.width - 20)
        items, names = wait(options, f'Missing choice {choice}')
        # Keys reach the popover once it has opened and its list has the focus.
        try:
            wait(focused, '', seconds=3)
        except AssertionError:
            time.sleep(.5)
        # The list opens on the chosen item; a drop-down is named by it.
        steps = names.index(choice) - names.index(widget.name)
        for _ in range(abs(steps)):
            command('xdotool', 'key', 'Down' if steps > 0 else 'Up')
        command('xdotool', 'key', 'Return')
        try:
            return wait(lambda: widget.name == choice, '', seconds=3)
        except AssertionError:
            print(f'Choosing {choice} again', flush=True)
    raise AssertionError(f'Could not choose {choice}')


def slider(name):
    return wait(lambda: find(name, 'slider'), f'Missing {name}')


def value(name):
    return find(name, 'slider').queryValue().currentValue


def devices(kind='sinks'):
    return json.loads(pactl('--format=json', 'list', kind))


def device(name, kind='sinks'):
    return next(d for d in devices(kind) if d['name'] == name)


def channels(name, kind='sinks'):
    d = device(name, kind)
    return [d['volume'][c]['value'] / 65536 * 100 for c in d['channel_map'].split(',')]


def stream(client, kind='sink-inputs'):
    return next(s for s in devices(kind)
                if s.get('properties', {}).get('application.name') == client)


def capture(name, widget=None):
    if widget is not None:
        reveal(widget)
        # Let the scroll's animation and a redraw finish.
        time.sleep(.4)
    command('import', '-window', 'root', str(OUT / name))


try:
    start('pipewire')
    pulse = start('pipewire-pulse')
    start('wireplumber')
    wait(lambda: pactl('info'), 'PipeWire did not start')
    pactl('load-module', 'module-null-sink', 'sink_name=speakers',
          'sink_properties=device.description=Desk_Speakers')
    second = pactl('load-module', 'module-null-sink', 'sink_name=headphones',
                   'sink_properties=device.description=USB_Headphones')
    pactl('load-module', 'module-remap-source', 'master=speakers.monitor',
          'source_name=microphone', 'source_properties=device.description=USB_Microphone')
    pactl('load-module', 'module-remap-source', 'master=speakers.monitor',
          'source_name=headset_mic', 'source_properties=device.description=Headset_Microphone')
    # pipewire-pulse answers before WirePlumber's default-device metadata
    # exists, and until then setting a default is "Not supported".
    wait(lambda: pactl('set-default-sink', 'speakers') == '' and
         pactl('get-default-sink') == 'speakers', 'WirePlumber did not set the default output')
    wait(lambda: pactl('set-default-source', 'microphone') == '' and
         pactl('get-default-source') == 'microphone', 'WirePlumber did not set the default input')
    # What the service reports, to see that the window doesn't keep it busy.
    events = OUT / 'events.log'
    logger = start('pactl', 'subscribe', name='events')
    app = start('tatami-sound')
    slider('Output volume')
    command('xdotool', 'windowfocus', command('xdotool', 'search', '--name', '^Sound$').splitlines()[0])
    time.sleep(1)
    (OUT / 'tree.txt').write_text('\n'.join(f'{w.getRoleName()}: {w.name}' for w in widgets()))
    capture('output.png')

    print('Checking the output list, volume and mute', flush=True)
    assert state(find('Desk_Speakers', 'list item', find('Output', 'grouping')),
                 pyatspi.STATE_SELECTED), 'The default output is not checked'
    assert not any('Monitor of' in w.name for w in widgets()), 'Output monitor shown as microphone'
    slider('Output volume').queryValue().currentValue = 37
    wait(lambda: abs(channels('speakers')[0] - 37) < 1, 'Volume gesture did not reach PipeWire')
    press('Mute output')
    wait(lambda: device('speakers')['mute'], 'Mute did not reach PipeWire')
    wait(lambda: not state(find('Output volume', 'slider'), pyatspi.STATE_SENSITIVE),
         'A muted volume can still be dragged')
    pactl('set-sink-mute', 'speakers', '0')
    pactl('set-sink-volume', 'speakers', '63%')
    wait(lambda: abs(value('Output volume') - 63) < 1, 'External volume did not reach the window')
    wait(lambda: not pressed('Mute output'), 'External unmute did not reach the window')

    print('Checking the balance', flush=True)
    slider('Output balance').queryValue().currentValue = -0.5
    wait(lambda: all(abs(a - b) < 1 for a, b in zip(channels('speakers'), [63, 31.5])),
         'Balance did not reach PipeWire')
    # A volume change keeps the balance.
    wait(lambda: abs(value('Output balance') + 0.5) < .03, 'Balance did not come back')
    slider('Output volume').queryValue().currentValue = 40
    wait(lambda: all(abs(a - b) < 1 for a, b in zip(channels('speakers'), [40, 20])),
         'Volume change lost the balance')
    slider('Output balance').queryValue().currentValue = 0.02
    wait(lambda: all(abs(a - 40) < 1 for a in channels('speakers')),
         'Balance near the middle was not the middle')

    print('Checking volume above 100%', flush=True)
    pactl('set-sink-volume', 'speakers', '120%')
    wait(lambda: find('Output volume', 'slider').queryValue().maximumValue == 150
         and abs(value('Output volume') - 120) < 1, 'A louder volume was not shown')
    wait(lambda: find('120%', 'label'), 'A louder volume was not shown in words')
    slider('Output volume').queryValue().currentValue = 90
    wait(lambda: abs(channels('speakers')[0] - 90) < 1, 'Volume from above 100% did not apply')
    wait(lambda: find('Output volume', 'slider').queryValue().maximumValue == 100,
         'The slider did not return to 100%')

    print('Checking the default output', flush=True)
    choose_device('Output', 'USB_Headphones')
    wait(lambda: pactl('get-default-sink') == 'headphones', 'Default output was not changed')
    # The controls are now the headphones'.
    wait(lambda: abs(value('Output volume') - 100) < 1, 'Controls did not follow the default')
    slider('Output volume').queryValue().currentValue = 55
    wait(lambda: abs(channels('headphones')[0] - 55) < 1, 'Volume went to the wrong output')
    assert abs(channels('speakers')[0] - 90) < 1, 'The previous output changed'

    print('Checking microphone controls', flush=True)
    assert state(find('USB_Microphone', 'list item', find('Input', 'grouping')),
                 pyatspi.STATE_SELECTED), 'The default input is not checked'
    slider('Input volume').queryValue().currentValue = 46
    wait(lambda: '46%' in pactl('get-source-volume', 'microphone'), 'Input volume did not change')
    press('Mute input')
    wait(lambda: pactl('get-source-mute', 'microphone') == 'Mute: yes', 'Microphone was not muted')
    pactl('set-source-mute', 'microphone', '0')
    wait(lambda: not pressed('Mute input'), 'External unmute did not reach the window')

    print('Checking the input level', flush=True)
    meter = wait(lambda: find('Input level', 'level bar'), 'Missing input level')
    assert meter.queryValue().currentValue == 0, 'Silence moved the level'
    noise = start('pacat', '--playback', '--raw', '--device=speakers', '--client-name=Noise',
                  '--stream-name=Noise', '/dev/urandom', name='noise')
    wait(lambda: find('Input level', 'level bar').queryValue().currentValue > 4,
         'Sound did not move the level')
    # The meter's own stream is neither an application nor a recording.
    slider('Noise volume')
    assert not any(w.name == 'Sound volume' for w in widgets()), 'The meter is listed as an app'
    assert not find('In use', 'label'), 'The meter alone makes the microphone in use'
    # The window reads on each change it is told of, and each reading is six
    # pactl clients. Neither may keep the other going.
    count = len(events.read_text().splitlines())
    assert count, 'The event log records nothing'
    time.sleep(3)
    burst = events.read_text().splitlines()[count:]
    changes = [line for line in burst if ' on client ' not in line]
    reads = sum("'new' on client" in line for line in burst) / 6
    print(f'In 3 s with the level meter running: {len(changes)} changes, {reads:.1f} readings',
          flush=True)
    assert len(changes) < 10 and reads <= 3, \
        f'The window keeps the audio service busy: {changes}, {reads:.1f} readings'
    capture('input.png', find('Input level', 'level bar'))
    noise.terminate()
    noise.wait()
    wait(lambda: find('Input level', 'level bar').queryValue().currentValue == 0,
         'The level did not fall back')
    wait(lambda: not find('Noise volume', 'slider'), 'Ended stream was not removed')
    choose_device('Input', 'Headset_Microphone')
    wait(lambda: pactl('get-default-source') == 'headset_mic', 'Default input was not changed')

    print('Checking playback stream controls', flush=True)
    assert not find('Applications', 'grouping'), 'Applications shown with none playing'
    player = start('pacat', '--playback', '--raw', '--device=speakers',
                   '--client-name=Test_Player', '--stream-name=Music', '/dev/zero')
    slider('Test_Player volume').queryValue().currentValue = 29
    wait(lambda: abs(next(iter(stream('Test_Player')['volume'].values()))['value'] / 65536 * 100
                     - 29) < 1, 'Stream volume did not change')
    wait(lambda: find('Music', 'label'), 'The stream does not say what it plays')
    # To the default output (the stream then follows the default), and
    # back to the one it named.
    choose(control('Test_Player', 'combo box'), 'USB_Headphones')
    headphone_id = device('headphones')['index']
    wait(lambda: stream('Test_Player')['sink'] == headphone_id, 'Playback stream did not move')
    choose(control('Test_Player', 'combo box'), 'Desk_Speakers')
    speakers_id = device('speakers')['index']
    wait(lambda: stream('Test_Player')['sink'] == speakers_id, 'Playback stream did not move back')
    press('Mute Test_Player')
    wait(lambda: stream('Test_Player')['mute'], 'Stream mute did not change')
    press('Mute Test_Player')
    wait(lambda: not stream('Test_Player')['mute'], 'Stream unmute did not change')
    capture('applications.png', find('Test_Player volume', 'slider'))
    player.terminate()
    player.wait()
    wait(lambda: not find('Test_Player volume', 'slider'), 'Ended stream was not removed')

    print('Checking hotplug', flush=True)
    pactl('unload-module', second)
    wait(lambda: not find('USB_Headphones', 'list item', find('Output', 'grouping')),
         'Removed output is still displayed')
    wait(lambda: state(find('Desk_Speakers', 'list item', find('Output', 'grouping')),
                       pyatspi.STATE_SELECTED), 'The fallback output is not checked')
    wait(lambda: abs(value('Output volume') - 90) < 1, 'Controls did not follow the fallback')

    print('Checking recording stream controls', flush=True)
    recorder = subprocess.Popen(
        ['parec', '--raw', '--device=microphone', '--client-name=Test_Recorder'],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    processes.append(recorder)
    slider('Test_Recorder volume')
    wait(lambda: find('In use', 'label'), 'A recording does not make its microphone in use')
    press('Mute Test_Recorder')
    wait(lambda: stream('Test_Recorder', 'source-outputs')['mute'],
         'Recording stream mute did not change')
    choose(control('Test_Recorder', 'combo box'), 'Headset_Microphone')
    mic_id = device('headset_mic', 'sources')['index']
    wait(lambda: stream('Test_Recorder', 'source-outputs')['source'] == mic_id,
         'Recording stream did not move')
    choose(control('Test_Recorder', 'combo box'), 'USB_Microphone')
    usb_id = device('microphone', 'sources')['index']
    wait(lambda: stream('Test_Recorder', 'source-outputs')['source'] == usb_id,
         'Recording stream did not move back')
    choose(control('Test_Recorder', 'combo box'), 'Headset_Microphone')
    wait(lambda: stream('Test_Recorder', 'source-outputs')['source'] == mic_id,
         'Recording stream did not move again')
    capture('recording.png', find('Test_Recorder volume', 'slider'))
    recorder.terminate()
    recorder.wait()
    wait(lambda: not find('Test_Recorder volume', 'slider'), 'Ended recording was not removed')
    assert not find('Device profiles', 'grouping'), 'Profiles shown without sound cards'

    # A service failure disables old controls, and a new connection recovers.
    print('Checking audio service restart', flush=True)
    pulse.terminate()
    pulse.wait()
    wait(lambda: any('Audio service:' in w.name for w in widgets()), 'Missing service error')
    start('pipewire-pulse', name='pipewire-pulse-2')
    wait(lambda: pactl('info'), 'Audio service did not restart')
    pactl('load-module', 'module-null-sink', 'sink_name=reconnected',
          'sink_properties=device.description=Reconnected')
    wait(lambda: find('Reconnected', 'list item'), 'Window did not reconnect')
    # The remapped microphones were pipewire-pulse's own modules.
    wait(lambda: find('No input devices', 'list item'), 'Missing empty input list')

    # GApplication forwards the second launch to the existing window.
    print('Checking single instance', flush=True)
    command('tatami-sound')
    assert sum(w.getRoleName() == 'frame' and w.name == 'Sound' for w in widgets()) == 1

    print('Checking Escape', flush=True)
    command('xdotool', 'windowfocus', command('xdotool', 'search', '--name', '^Sound$').splitlines()[0])
    command('xdotool', 'key', 'Escape')
    app.wait(timeout=10)
    # The window's own subscription ends with it; the test's goes on.
    others = [p.parent.name for p in Path('/proc').glob('[0-9]*/comm')
              if p.read_text().strip() == 'pactl' and int(p.parent.name) != logger.pid]
    assert not others, f'pactl outlived the window: {others}'
    (OUT / 'passed').write_text(
        'Real PipeWire + GTK: device lists and defaults, volume, mute, balance, volume above '
        '100%, input level, stream volume, mute and routing, hotplug, reconnect, single '
        'instance, Escape.\n')
except Exception:
    command('import', '-window', 'root', str(OUT / 'failed.png'))
    for log in OUT.glob("*.log"):
        print(log.name, log.read_text()[-4000:], flush=True)
    try:
        print("\n".join(f"{w.getRoleName()}: {w.name}" for w in widgets()), flush=True)
    except Exception:
        pass
    raise
finally:
    for process in reversed(processes):
        process.terminate()
    for process in processes:
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
