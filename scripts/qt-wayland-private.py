#!/usr/bin/env python3
"""Prove Qt CLIPBOARD, PRIMARY and middle-paste on an owned headless Wayland seat.

Requires Linux, Cage with headless/pixman and PRIMARY/virtual-keyboard support,
rex, wtype, Python 3, a C++ compiler, pkg-config and Qt6Gui development files.
Build the Qt test driver and rex-qt-backend first. No packages are installed or
fetched. Optional --library-path supports an already extracted Cage dependency
bundle. Example (from the repository root):

  python3 scripts/qt-wayland-private.py --cage /path/to/cage \
    --driver qt/build/test-driver/rex-qt-test-driver \
    --backend target/release/rex-qt-backend

Only a newly launched private Rex server/session and headless Cage are used.
There is deliberately no existing endpoint option. Private HOME/XDG directories,
explicit headless-only backend and pixman prevent desktop clipboard/seat access.
Logs, scripts and exact byte evidence remain in .build-tools/qt-wayland-*; short
mode-0700 runtime directories under /tmp are removed after all owned processes
stop. No personal windows are displayed or captured.
"""
import argparse
import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import subprocess
import tempfile
import time
import uuid

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cage', default='cage')
    parser.add_argument('--library-path', type=Path)
    parser.add_argument('--driver', type=Path, default=ROOT / 'qt/build/test-driver/rex-qt-test-driver')
    parser.add_argument('--backend', type=Path, default=ROOT / 'target/release/rex-qt-backend')
    args = parser.parse_args()
    cage_path = shutil.which(args.cage)
    if not cage_path:
        parser.error('Cage not found; supply an installed or extracted --cage executable')
    for executable in ('rex', 'wtype', 'c++', 'pkg-config'):
        if not shutil.which(executable): parser.error(f'missing dependency: {executable}')
    for binary in (args.driver, args.backend):
        if not binary.is_file(): parser.error(f'build the executable first: {binary}')
    (ROOT / '.build-tools').mkdir(exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix='qt-wayland-', dir=ROOT / '.build-tools'))
    runtime = Path(tempfile.mkdtemp(prefix='rex-qt-wl-', dir='/tmp'))
    endpoint = 'unix://' + str(runtime / 'rex.sock')
    assert len(os.fsencode(runtime / 'wayland-0')) < 100
    assert len(os.fsencode(runtime / 'rex.sock')) < 100
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(('REX_', 'HERDR_', 'WLR_')) and k not in (
               'DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'DBUS_SESSION_BUS_ADDRESS',
               'HYPRLAND_INSTANCE_SIGNATURE', 'SWAYSOCK', 'SESSION_MANAGER')}
    for name in ('home', 'config', 'state', 'cache'):
        (work / name).mkdir(mode=0o700)
    env.update(HOME=str(work / 'home'), XDG_CONFIG_HOME=str(work / 'config'),
               XDG_STATE_HOME=str(work / 'state'), XDG_CACHE_HOME=str(work / 'cache'),
               XDG_RUNTIME_DIR=str(runtime), WLR_BACKENDS='headless', WLR_RENDERER='pixman',
               WLR_HEADLESS_OUTPUTS='1', QT_QPA_PLATFORM='wayland', QT_QUICK_BACKEND='software',
               QT_QPA_PLATFORMTHEME='generic', QT_WAYLAND_CLIENT_BUFFER_INTEGRATION='shm',
               WAYLAND_DEBUG='client', SHELL='/bin/sh', PS1='private$ ', REX_SERVER=endpoint)
    if args.library_path:
        env['LD_LIBRARY_PATH'] = str(args.library_path.resolve())
    processes = []

    def spawn(command, name):
        with (work / (name + '.log')).open('w') as log:
            process = subprocess.Popen(command, env=env, stdout=log, stderr=log, start_new_session=True)
        processes.append(process)
        return process

    def wait_for(predicate, process, description, timeout=15):
        deadline = time.monotonic() + timeout
        while not predicate():
            if process.poll() is not None:
                raise RuntimeError(f'{description}: process exited {process.returncode}; see {work}')
            if time.monotonic() >= deadline:
                raise RuntimeError(f'timed out waiting for {description}; see {work}')
            time.sleep(.05)

    def key_serial():
        subprocess.run(['wtype', '-k', 'Shift_L'], env=env, check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=3)

    try:
        flags = shlex.split(subprocess.check_output(
            ['pkg-config', '--cflags', '--libs', 'Qt6Gui'], text=True))
        reader_binary = work / 'clipboard-reader'
        subprocess.run(['c++', '-fPIC', '-std=c++17', str(ROOT / 'scripts/tools/qt-wayland-clipboard.cpp'),
                        '-o', str(reader_binary), *flags], check=True,
                       env={**os.environ, 'TMPDIR': str(work)})
        server = spawn(['rex', 'server', 'run', '--listen', endpoint, '--restore=false',
                        '--config', '/dev/null', '--state-dir', str(work / 'server')], 'server')
        wait_for(lambda: (runtime / 'rex.sock').is_socket(), server, 'private Rex socket')
        created = json.loads(subprocess.check_output(
            ['rex', '--autostart=false', '--server', endpoint, 'api', 'call', 'session.create',
             json.dumps({'label': 'Private native Wayland selection proof', 'initial_windows': [
                 {'layout': {'block': {'flavor': 'com.superlogical.terminal.shell'}}}]})],
            env=env, text=True, timeout=20))
        cage = spawn([cage_path, '-D', '--', 'sleep', '90'], 'cage')
        wait_for(lambda: any(runtime.glob('wayland-*')), cage, 'private Wayland socket')
        wait_for(lambda: any(path.is_socket() for path in runtime.glob('wayland-*')), cage,
                 'private Wayland listener')
        sockets = [path for path in runtime.glob('wayland-*') if path.is_socket()]
        env['WAYLAND_DISPLAY'] = sockets[0].name
        token = 'NATIVE_SELECTION_' + uuid.uuid4().hex[:12].upper()
        (work / 'token').write_text(token)
        fixture = work / 'fixture.py'
        fixture.write_text(f'''import os, select, termios, time, tty
old = termios.tcgetattr(0)
data = b''
try:
    tty.setraw(0)
    os.write(1, b'\\x1b[2J\\x1b[H{token}')
    deadline = time.monotonic() + 35
    while len(data) < {len(token)} and time.monotonic() < deadline:
        if select.select([0], [], [], .1)[0]:
            data += os.read(0, 4096)
finally:
    termios.tcsetattr(0, termios.TCSANOW, old)
open({str(work / 'primary-paste.bin')!r}, 'wb').write(data)
print('\\r\\nPRIVATE_PRIMARY_RECEIVED', flush=True)
time.sleep(2)
''')
        steps = [
            {'op': 'waitState', 'field': 'terminal.connected', 'value': True},
            {'op': 'pause', 'ms': 1500},
            {'op': 'type', 'text': 'python3 ' + shlex.quote(str(fixture))},
            {'op': 'key', 'key': 'Return'}, {'op': 'waitText', 'text': token},
            {'op': 'pause', 'ms': 800},
            {'op': 'clipboardPolicy', 'copyOnSelection': True, 'primarySelection': True},
            {'op': 'pointer', 'phase': 'down', 'x': 0, 'y': 0},
            {'op': 'pointer', 'phase': 'move', 'x': len(token), 'y': 0},
            {'op': 'pointer', 'phase': 'up', 'x': len(token), 'y': 0},
            {'op': 'waitClipboard', 'text': token, 'exact': True},
            {'op': 'waitPrimary', 'text': token, 'exact': True},
            {'op': 'snapshot', 'path': str(work / 'selected.json')},
            {'op': 'pause', 'ms': 8000},
            {'op': 'pointer', 'phase': 'down', 'button': 'middle', 'x': 0, 'y': 1},
            {'op': 'pointer', 'phase': 'up', 'button': 'middle', 'x': 0, 'y': 1},
            {'op': 'waitText', 'text': 'PRIVATE_PRIMARY_RECEIVED'},
            {'op': 'snapshot', 'path': str(work / 'pasted.json')},
        ]
        (work / 'events.json').write_text(json.dumps(steps))
        driver = spawn([str(args.driver.resolve()), '--backend', str(args.backend.resolve()),
                        '--endpoint', endpoint, '--session', created['session_id'],
                        '--script', str(work / 'events.json'), '--report', str(work / 'report.json')], 'driver')
        # Wait for a native surface before creating the private virtual keyboard.
        wait_for(lambda: '.attach(wl_buffer#' in (work / 'driver.log').read_text(), driver, 'mapped Qt Wayland surface')
        key_serial()
        wait_for(lambda: (work / 'selected.json').exists(), driver, 'exact selected token')
        selected = json.loads((work / 'selected.json').read_text())
        assert selected['supportsSelection'] and selected['selectionText'] == token
        assert selected['clipboard'] == token
        reader = spawn([str(reader_binary), token, str(work / 'reader.json')], 'reader')
        assert reader.wait(timeout=7) == 0, f'independent native read failed: {work}'
        assert driver.wait(timeout=15) == 0, f'middle-paste driver failed: {work}'
        assert (work / 'primary-paste.bin').read_bytes() == token.encode(), 'raw PTY bytes differ'
        report = json.loads((work / 'reader.json').read_text())
        assert report['clipboard'] == report['primary'] == token
        print(f'PASS native CLIPBOARD, PRIMARY and exact middle-paste PTY bytes: {work}')
    finally:
        for process in processes:
            try: os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError: pass
        for process in processes:
            try: process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
        shutil.rmtree(runtime)
        print(f'Artifacts: {work}')


if __name__ == '__main__':
    main()
