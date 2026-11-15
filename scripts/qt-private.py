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
import shutil
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
        steps = [
            {'op': 'waitState', 'field': 'terminal.connected', 'value': True},
            {'op': 'type', 'text': "printf '\\nQT_SHELL_%s\\n' ready"}, {'op': 'key', 'key': 'Return'},
            {'op': 'waitText', 'text': 'QT_SHELL_ready'},
            {'op': 'paste', 'text': "printf '\\nQt wide: 界 é ┌─┐\\n'"}, {'op': 'key', 'key': 'Return'},
            {'op': 'waitText', 'text': 'Qt wide: 界 é ┌─┐'},
            {'op': 'selectAll'}, {'op': 'copy'},
            {'op': 'waitClipboard', 'text': 'QT_SHELL_ready'},
            {'op': 'snapshot', 'path': str(work / 'clipboard.json')},
            {'op': 'type', 'text': f'nvim -u NONE -i NONE -n {editor}'}, {'op': 'key', 'key': 'Return'},
            {'op': 'waitText', 'text': '~'},
            {'op': 'key', 'key': 'i'}, {'op': 'type', 'text': 'qt-editor-save'},
            {'op': 'key', 'key': 'Escape'}, {'op': 'waitText', 'text': 'qt-editor-save'},
            {'op': 'screenshot', 'path': str(work / 'editor.png')},
            {'op': 'type', 'text': ':wq'}, {'op': 'key', 'key': 'Return'},
            {'op': 'type', 'text': "printf '\\nQT_EDITOR_%s\\n' done"}, {'op': 'key', 'key': 'Return'},
            {'op': 'waitText', 'text': 'QT_EDITOR_done'},
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
        clipboard = json.loads((work / 'clipboard.json').read_text())
        assert 'QT_SHELL_ready' in clipboard['clipboard'], 'visible selection did not reach Qt clipboard'
        minimum = json.loads((work / 'minimum.json').read_text())
        # Compare the program-visible PTY dimensions to the actual measured Qt item.
        size = api('com.superlogical.terminal.size', session_id=sid, block_id=blocks[0])
        assert size['columns'] == minimum['qt_columns'] and size['rows'] == minimum['qt_rows'], (size, minimum['qt_columns'], minimum['qt_rows'])
        navigation = json.loads((work / 'navigation.json').read_text())
        state = navigation.get('state', navigation)
        index = next(i for i, session in enumerate(state['sessions']) if session['id'] == other)
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
                                     'owned resize', 'pane/tab/session selection', 'disconnect pause']}))
    finally:
        if client is not None and client.poll() is None:
            client.terminate()
            client.wait(timeout=10)
        if server.poll() is None:
            server.terminate()
            server.wait(timeout=10)
        server_log.close()


if __name__ == '__main__':
    main()
