#!/usr/bin/env python3
"""Exercise the production Qt window/item on a disposable Rex server.

Requires built rex-qt-test-driver, rex-qt-backend, Qt6Test and nvim. Qt's
offscreen software platform isolates input, rendering and clipboard from the
desktop. No personal Rex socket, session or config is used. Artifacts remain
under ignored .build-tools/qt-acceptance-* unless --artifacts is supplied.
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

ROOT = Path(__file__).resolve().parents[1]


def wait(predicate, description, timeout=25):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(.05)
    raise AssertionError(description)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--driver', type=Path, default=ROOT / 'qt/build/test-driver/rex-qt-test-driver')
    parser.add_argument('--backend', type=Path, default=ROOT / 'target/release/rex-qt-backend')
    parser.add_argument('--artifacts', type=Path)
    parser.add_argument('--native-wayland-smoke', action='store_true',
                        help='Also show only this private fixture in a native Wayland window; no clipboard operations')
    args = parser.parse_args()
    for tool in ('rex', 'nvim'):
        assert shutil.which(tool), f'{tool} is required'
    assert args.driver.is_file() and args.backend.is_file(), 'Build the Qt driver and Rust backend first'
    work = args.artifacts or Path(tempfile.mkdtemp(prefix='qt-acceptance-', dir=ROOT / '.build-tools'))
    work = work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    env = {k: v for k, v in os.environ.items() if not k.startswith(('REX_', 'HERDR_'))
           and k not in ('DISPLAY', 'WAYLAND_DISPLAY', 'DBUS_SESSION_BUS_ADDRESS')}
    for name in ('home', 'config', 'state', 'cache', 'runtime'):
        (work / name).mkdir(mode=0o700, exist_ok=True)
    endpoint = 'unix://' + str(work / 'rex.sock')
    env.update(HOME=str(work / 'home'), XDG_CONFIG_HOME=str(work / 'config'),
               XDG_STATE_HOME=str(work / 'state'), XDG_CACHE_HOME=str(work / 'cache'),
               XDG_RUNTIME_DIR=str(work / 'runtime'), SHELL='/bin/sh', PS1='qt-private$ ',
               REX_SERVER=endpoint, QT_QPA_PLATFORM='offscreen', QT_QUICK_BACKEND='software',
               QT_QPA_PLATFORMTHEME='generic')

    def api(method, **payload):
        return json.loads(subprocess.check_output(
            ['rex', '--autostart=false', '--server', endpoint, 'api', 'call', method, json.dumps(payload)],
            env=env, text=True, timeout=20))

    server_log = (work / 'server.log').open('w')
    server = subprocess.Popen(['rex', 'server', 'run', '--listen', endpoint, '--restore=false',
                               '--config', '/dev/null', '--state-dir', str(work / 'server')],
                              env=env, stdout=server_log, stderr=server_log)
    client = None
    server_paused = False
    try:
        wait(lambda: (work / 'rex.sock').exists() or server.poll() is not None, 'private server startup')
        assert server.poll() is None, 'private server exited; inspect server.log'
        shell = {'block': {'flavor': 'com.superlogical.terminal.shell'}}
        split = {'split': {'direction': 'horizontal', 'ratio': .5, 'before': shell, 'after': shell}}
        sid = api('session.create', label='Qt acceptance', initial_windows=[
            {'window_label': 'Editor', 'layout': split}, {'window_label': 'Other tab', 'layout': shell}])['session_id']
        other = api('session.create', label='Other session', initial_windows=[
            {'window_label': 'Other shell', 'layout': shell}])['session_id']
        view = api('session.view', session_id=sid)
        first, second = view['windows']
        blocks = [b['block_id'] for layer in first['layers'] for b in layer['blocks']
                  if b.get('creator_name') == 'com.superlogical.terminal']
        assert len(blocks) == 2, 'split fixture must have two terminal panes'
        api('session.focus_window', session_id=sid, window_id=first['window_id'])
        api('session.focus_block', session_id=sid, block_id=blocks[0])
        editor = work / 'editor.txt'
        copy_fixture = work / 'copy-fixture.py'
        copy_fixture.write_text("import shutil,sys\n"
                                "columns=shutil.get_terminal_size().columns\n"
                                "sys.stdout.write('\\nA\\x1b[5GB\\n'+'Q'*(columns-1)+' '+'WRAP_SENTINEL\\n')\n")
        keys_fixture = work / 'keys-fixture.py'
        keys_fixture.write_text("import os,select,sys,termios,time,tty\n"
                                "old=termios.tcgetattr(0); data=b''\n"
                                "try:\n"
                                " tty.setraw(0)\n"
                                " os.write(1,b'\\x1b[>11u\\x1b[2J\\x1b[HQT_KEYS_READY')\n"
                                " deadline=time.monotonic()+8\n"
                                " last=time.monotonic()\n"
                                " while time.monotonic()<deadline:\n"
                                "  if select.select([0],[],[],.1)[0]:\n"
                                "   data+=os.read(0,1024); last=time.monotonic()\n"
                                "  elif data and time.monotonic()-last>.5: break\n"
                                "finally:\n"
                                " os.write(1,b'\\x1b[<u')\n"
                                " termios.tcsetattr(0,termios.TCSANOW,old)\n"
                                f"open({str(work / 'chords.bin')!r},'wb').write(data)\n"
                                "print('\\r\\nQT_KEYS_DONE',flush=True)\n")
        cursor_fixture = work / 'cursor-fixture.py'
        cursor_fixture.write_text("import sys,time\n"
                                  "sys.stdout.write('\\x1b[2J\\x1b[H\\x1b[2 q\\x1b[?25h界\\x1b[1G');sys.stdout.flush()\n"
                                  "time.sleep(2)\n"
                                  "print('\\r\\nQT_CURSOR_DONE',flush=True)\n")
        steps = [
            {'op': 'waitState', 'field': 'terminal.connected', 'value': True},
            {'op': 'type', 'text': "printf '\\nQT_SHELL_%s\\n' ready"}, {'op': 'key', 'key': 'Return'},
            {'op': 'waitText', 'text': 'QT_SHELL_ready'},
            {'op': 'paste', 'text': "printf '\\nQt wide: 界 é ┌─┐\\n'"}, {'op': 'key', 'key': 'Return'},
            {'op': 'waitText', 'text': 'Qt wide: 界 é ┌─┐'},
            {'op': 'type', 'text': f'python3 {shlex.quote(str(copy_fixture))}'}, {'op': 'key', 'key': 'Return'},
            {'op': 'waitText', 'text': 'WRAP_SENTINEL'},
            {'op': 'selectAll'}, {'op': 'copy'},
            {'op': 'waitClipboard', 'text': 'QT_SHELL_ready'},
            {'op': 'waitClipboard', 'text': 'Qt wide: 界 é ┌─┐'},
            {'op': 'waitClipboard', 'text': 'A   B'},
            {'op': 'waitClipboard', 'text': ' WRAP_SENTINEL'},
            {'op': 'snapshot', 'path': str(work / 'clipboard.json')},
            {'op': 'type', 'text': "printf '\\nQT_KEY_%s\\n' '"},
            {'op': 'key', 'key': 'é', 'text': 'é', 'action': 'press'},
            {'op': 'key', 'key': 'é', 'text': 'é', 'action': 'repeat'},
            {'op': 'key', 'key': 'é', 'text': '', 'action': 'release'},
            {'op': 'ime', 'text': 'Ω'},
            {'op': 'type', 'text': "'"}, {'op': 'key', 'key': 'Return'},
            {'op': 'waitText', 'text': 'QT_KEY_ééΩ'},
            {'op': 'type', 'text': f'python3 {shlex.quote(str(keys_fixture))}'}, {'op': 'key', 'key': 'Return'},
            {'op': 'waitText', 'text': 'QT_KEYS_READY'},
            {'op': 'key', 'key': 'c', 'mods': 'ctrl+shift'},
            {'op': 'key', 'key': 'v', 'mods': 'ctrl+shift'},
            {'op': 'waitText', 'text': 'QT_KEYS_DONE'},
            {'op': 'type', 'text': f'nvim -u NONE -i NONE -n {shlex.quote(str(editor))}'}, {'op': 'key', 'key': 'Return'},
            {'op': 'waitText', 'text': '~'},
            {'op': 'key', 'key': 'i'}, {'op': 'type', 'text': 'qt-editor-save'},
            {'op': 'key', 'key': 'Escape'}, {'op': 'waitText', 'text': 'qt-editor-save'},
            {'op': 'screenshot', 'path': str(work / 'editor.png')},
            {'op': 'type', 'text': ':wq'}, {'op': 'key', 'key': 'Return'},
            {'op': 'type', 'text': "printf '\\nQT_EDITOR_%s\\n' done"}, {'op': 'key', 'key': 'Return'},
            {'op': 'waitText', 'text': 'QT_EDITOR_done'},
            {'op': 'type', 'text': f'python3 {shlex.quote(str(cursor_fixture))}'}, {'op': 'key', 'key': 'Return'},
            {'op': 'waitState', 'field': 'frame.cursor', 'value': [0, 0]},
            {'op': 'assertWideCursor'},
            {'op': 'screenshot', 'path': str(work / 'wide-cursor.png')},
            {'op': 'waitText', 'text': 'QT_CURSOR_DONE'},
            {'op': 'resize', 'width': 640, 'height': 480},
            {'op': 'pause', 'ms': 400},
            {'op': 'type', 'text': "printf '\\nQT_SIZE '; stty size"}, {'op': 'key', 'key': 'Return'},
            {'op': 'waitText', 'text': 'QT_SIZE '},
            {'op': 'snapshot', 'path': str(work / 'minimum.json')},
            {'op': 'screenshot', 'path': str(work / 'minimum.png')},
            {'op': 'select', 'control': 'panes', 'index': 1},
            {'op': 'waitState', 'field': 'pane', 'value': blocks[1]},
            {'op': 'type', 'text': "printf '\\nQT_PANE_%s\\n' second"}, {'op': 'key', 'key': 'Return'},
            {'op': 'waitText', 'text': 'QT_PANE_second'},
            {'op': 'select', 'control': 'tabs', 'index': 1},
            {'op': 'waitState', 'field': 'window', 'value': second['window_id']},
            {'op': 'type', 'text': "printf '\\nQT_TAB_%s\\n' second"}, {'op': 'key', 'key': 'Return'},
            {'op': 'waitText', 'text': 'QT_TAB_second'},
            {'op': 'snapshot', 'path': str(work / 'navigation.json')},
        ]

        def run(script, report, client_env=None):
            path = work / script
            log = (work / (script + '.log')).open('w')
            return subprocess.Popen([str(args.driver.resolve()), '--backend', str(args.backend.resolve()),
                                     '--endpoint', endpoint, '--session', sid, '--script', str(path),
                                     '--report', str(work / report)], env=client_env or env, stdout=log, stderr=log)

        (work / 'main.json').write_text(json.dumps(steps))
        client = run('main.json', 'main-report.json')
        assert client.wait(timeout=150) == 0, 'Qt event acceptance failed; inspect main-report.json and main.json.log'
        assert editor.read_text() == 'qt-editor-save\n', 'editor did not save exact expected bytes'
        assert (work / 'chords.bin').read_bytes() == b'\x1b[99;6u\x1b[99;6:3u\x1b[118;6u\x1b[118;6:3u', 'modified key presses/releases did not reach the raw PTY'
        clipboard = json.loads((work / 'clipboard.json').read_text())
        assert 'QT_SHELL_ready' in clipboard['clipboard'], 'visible selection did not reach Qt clipboard'
        assert 'Qt wide: 界 é ┌─┐' in clipboard['clipboard'], 'Unicode changed during copying'
        assert 'A   B' in clipboard['clipboard'], 'copy collapsed cursor-positioned blank cells'
        assert 'Q' * (clipboard['qt_columns'] - 1) + ' WRAP_SENTINEL' in clipboard['clipboard'], 'copy changed a soft-wrap boundary'
        minimum = json.loads((work / 'minimum.json').read_text())
        # Compare the program-visible PTY dimensions to the actual measured Qt item.
        size = api('com.superlogical.terminal.size', session_id=sid, block_id=blocks[0])
        assert size['columns'] == minimum['qt_columns'] and size['rows'] == minimum['qt_rows'], (size, minimum['qt_columns'], minimum['qt_rows'])
        navigation = json.loads((work / 'navigation.json').read_text())
        state = navigation.get('state', navigation)
        index = next(i for i, session in enumerate(state['sessions']) if session['id'] == other)
        reconnect = [
            {'op': 'waitState', 'field': 'terminal.connected', 'value': True},
            {'op': 'snapshot', 'path': str(work / 'reconnect-ready.json')},
            {'op': 'pause', 'ms': 500},
            {'op': 'key', 'key': 'Return'},
            {'op': 'select', 'control': 'sessions', 'index': index},
            {'op': 'waitState', 'field': 'connected', 'value': False, 'timeout': 25000},
            {'op': 'type', 'text': 'must-not-replay'},
            {'op': 'snapshot', 'path': str(work / 'reconnect-paused.json')},
            {'op': 'waitState', 'field': 'connected', 'value': True, 'timeout': 30000},
            {'op': 'waitState', 'field': 'terminal.connected', 'value': True},
            {'op': 'type', 'text': "printf '\\nQT_AFTER_%s\\n' reconnect"}, {'op': 'key', 'key': 'Return'},
            {'op': 'waitText', 'text': 'QT_AFTER_reconnect'},
            {'op': 'snapshot', 'path': str(work / 'reconnected.json')},
        ]
        (work / 'reconnect.json').write_text(json.dumps(reconnect))
        client = run('reconnect.json', 'reconnect-report.json')
        wait(lambda: (work / 'reconnect-ready.json').exists() or client.poll() is not None, 'reconnect fixture readiness')
        assert client.poll() is None, 'reconnect driver failed before readiness'
        server.send_signal(signal.SIGSTOP)
        server_paused = True
        wait(lambda: (work / 'reconnect-paused.json').exists() or client.poll() is not None, 'recoverable private disconnect', timeout=35)
        assert client.poll() is None, 'reconnect driver failed before pause observation'
        paused = json.loads((work / 'reconnect-paused.json').read_text())
        assert paused['problem'], 'asynchronous host failure did not reach the UI'
        server.send_signal(signal.SIGCONT)
        server_paused = False
        assert client.wait(timeout=50) == 0, 'reconnect acceptance failed'
        reconnected = json.loads((work / 'reconnected.json').read_text())
        text = api('com.superlogical.terminal.format', session_id=reconnected['session'], block_id=reconnected['pane'],
                   args={'format': 'text', 'trim': True, 'unwrap': True})['content']
        assert 'QT_AFTER_reconnect' in text and 'must-not-replay' not in text, 'failed input was replayed after reconnect'
        if args.native_wayland_smoke:
            assert os.environ.get('WAYLAND_DISPLAY') and os.environ.get('XDG_RUNTIME_DIR'), 'native Wayland environment required'
            native_env = dict(env, QT_QPA_PLATFORM='wayland', WAYLAND_DISPLAY=os.environ['WAYLAND_DISPLAY'],
                              XDG_RUNTIME_DIR=os.environ['XDG_RUNTIME_DIR'])
            native_steps = [
                {'op': 'waitState', 'field': 'terminal.connected', 'value': True},
                {'op': 'type', 'text': "printf '\\nQT_WAYLAND_%s\\n' ready"}, {'op': 'key', 'key': 'Return'},
                {'op': 'waitText', 'text': 'QT_WAYLAND_ready'},
                {'op': 'screenshot', 'path': str(work / 'wayland.png')},
            ]
            (work / 'wayland.json').write_text(json.dumps(native_steps))
            client = run('wayland.json', 'wayland-report.json', native_env)
            assert client.wait(timeout=45) == 0, 'native Wayland fixture failed'
        disconnect = [
            {'op': 'waitState', 'field': 'terminal.connected', 'value': True},
            {'op': 'select', 'control': 'sessions', 'index': index},
            {'op': 'waitState', 'field': 'session', 'value': other},
            {'op': 'waitState', 'field': 'terminal.connected', 'value': True},
            {'op': 'type', 'text': "printf '\\nQT_SESSION_%s\\n' other"}, {'op': 'key', 'key': 'Return'},
            {'op': 'waitText', 'text': 'QT_SESSION_other'},
            {'op': 'snapshot', 'path': str(work / 'before-disconnect.json')},
            {'op': 'waitState', 'field': 'connected', 'value': False},
            {'op': 'type', 'text': 'must-not-replay'},
            {'op': 'snapshot', 'path': str(work / 'disconnected.json')},
            {'op': 'screenshot', 'path': str(work / 'disconnected.png')},
        ]
        (work / 'disconnect.json').write_text(json.dumps(disconnect))
        client = run('disconnect.json', 'disconnect-report.json')
        wait(lambda: (work / 'before-disconnect.json').exists() or client.poll() is not None, 'session selection before disconnect')
        assert client.poll() is None, 'session selection failed before disconnect'
        server.terminate()
        server.wait(timeout=10)
        assert client.wait(timeout=30) == 0, 'disconnect acceptance failed'
        print(json.dumps({'success': True, 'artifacts': str(work),
                          'checks': ['shell keys', 'Unicode clipboard paste/copy', 'editor save',
                                     'owned resize', 'pane/tab/session selection', 'disconnect pause',
                                     'copy gaps and soft wraps', 'direct Unicode and IME commits',
                                     'clipboard chords reach PTY', 'reconnect without input replay']}))
    finally:
        if server_paused and server.poll() is None:
            server.send_signal(signal.SIGCONT)
        if client is not None and client.poll() is None:
            client.terminate()
            client.wait(timeout=10)
        if server.poll() is None:
            server.terminate()
            server.wait(timeout=10)
        server_log.close()


if __name__ == '__main__':
    main()
