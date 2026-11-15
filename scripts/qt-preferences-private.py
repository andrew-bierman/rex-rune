#!/usr/bin/env python3
"""Settings via real Qt events and PTY coordinates, using only an owned Rex/socket/HOME.

Requires rex, Python and built Qt test driver/backend. Runs offscreen with software rendering;
this checks Qt events and a real PTY, not native desktop clipboard or compositor integration.
"""
import argparse
import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import time
import threading

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--driver', type=Path, required=True)
    parser.add_argument('--backend', type=Path, required=True)
    args = parser.parse_args()
    (ROOT / '.build-tools').mkdir(exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix='preferences-private-', dir=ROOT / '.build-tools'))
    env = {key: value for key, value in os.environ.items() if not key.startswith(('REX_', 'HERDR_')) and key not in ('DISPLAY', 'WAYLAND_DISPLAY', 'DBUS_SESSION_BUS_ADDRESS')}
    for name in ('home', 'config', 'state', 'cache', 'runtime'):
        (work / name).mkdir(mode=0o700)
    endpoint = 'unix://' + str(work / 'rex.sock')
    env["DBUS_SESSION_BUS_ADDRESS"] = "unix:path=" + str(work / "absent-notification-bus")
    env.update(HOME=str(work / 'home'), XDG_CONFIG_HOME=str(work / 'config'), XDG_STATE_HOME=str(work / 'state'),
               XDG_CACHE_HOME=str(work / 'cache'), XDG_RUNTIME_DIR=str(work / 'runtime'), SHELL='/bin/sh',
               REX_SERVER=endpoint, QT_QPA_PLATFORM='offscreen', QT_QUICK_BACKEND='software', QT_QPA_PLATFORMTHEME='generic', QT_FORCE_STDERR_LOGGING='1')
    config = work / 'workspace.json'
    config.write_text(json.dumps({'hosts': [{'label': 'Preferences fixture', 'endpoint': endpoint}],
                                  'font_family': 'IBM Plex Mono', 'font_fallbacks': ['Symbols Nerd Font']}))
    override = work / 'workspace.shortcuts.json'
    override.write_text('{"bindings":[]}\n')
    rune = work / 'config' / 'rex-rune' / 'workspace.json'
    rune.parent.mkdir()
    rune.write_text('{"font_size":31,"sentinel":"unchanged"}\n')
    rune_before = rune.read_bytes()

    def api(method, **payload):
        return json.loads(subprocess.check_output(['rex', '--autostart=false', '--server', endpoint, 'api', 'call', method, json.dumps(payload)], env=env, text=True, timeout=20))

    def run(name, steps):
        events = work / (name + '.json')
        events.write_text(json.dumps(steps))
        with (work / (name + '.log')).open('w') as log:
            result = subprocess.run([str(args.driver.resolve()), '--backend', str(args.backend.resolve()), '--endpoint', endpoint,
                                     '--rex-session', sid, '--config', str(config), '--script', str(events), '--report', str(work / (name + '-report.json'))],
                                    env=env, stdout=log, stderr=log, timeout=65)
        assert result.returncode == 0, f'{name} events failed ({result.returncode}): {work}'

    def search(text):
        return [{'op': 'field', 'control': 'settingsSearch', 'text': text}, {'op': 'pause', 'ms': 180}]

    with (work / 'server.log').open('w') as log:
        server = subprocess.Popen(['rex', 'server', 'run', '--listen', endpoint, '--restore=false', '--config', '/dev/null', '--state-dir', str(work / 'server')], env=env, stdout=log, stderr=log)
        try:
            for _ in range(200):
                if (work / 'rex.sock').exists():
                    break
                time.sleep(.05)
            sid = api('session.create', label='Preferences fixture', initial_windows=[{'layout': {'block': {'flavor': 'com.superlogical.terminal.shell'}}}])['session_id']
            mouse = work / 'mouse.py'
            mouse.write_text("import os,select,termios,time,tty\nold=termios.tcgetattr(0);data=b''\ntry:\n tty.setraw(0)\n os.write(1,b'\\x1b[2J\\x1b[H\\x1b[?1000h\\x1b[?1006hPREF_MOUSE_READY')\n deadline=time.monotonic()+4\n while time.monotonic()<deadline:\n  if select.select([0],[],[],.1)[0]:data+=os.read(0,1024)\nfinally:\n os.write(1,b'\\x1b[?1000l\\x1b[?1006l')\n termios.tcsetattr(0,termios.TCSANOW,old)\n" + f"open({str(work / 'mouse.bin')!r},'wb').write(data)\nprint('\\r\\nPREF_MOUSE_DONE',flush=True)\n")
            steps = [{'op': 'waitState', 'field': 'terminal.connected', 'value': True},
                     {'op': 'type', 'text': ':'}, {'op': 'key', 'key': 'Return'},
                     {'op': 'waitState', 'field': 'terminal.owner', 'value': True},
                     {'op': 'snapshot', 'path': str(work / 'before.json')}, {'op': 'click', 'control': 'settingsButton'}]
            steps += search('font size')
            steps += [{'op': 'click', 'control': 'setting-font_size'}, {'op': 'controlKey', 'key': 'Up'},
                      {'op': 'click', 'control': 'applyPreferences'}, {'op': 'waitState', 'field': 'preferences.font_size', 'value': 16},
                      {'op': 'click', 'control': 'settingsButton'}]
            steps += search('line height')
            steps += [{'op': 'click', 'control': 'setting-terminal.font.line_height'}, {'op': 'controlKey', 'key': 'Up'},
                      {'op': 'screenshot', 'path': str(work / 'typography-preview.png')},
                      {'op': 'click', 'control': 'applyPreferences'},
                      {'op': 'waitState', 'field': 'preferences.terminal.font.line_height', 'value': 5},
                      {'op': 'click', 'control': 'settingsButton'}]
            steps += search('cursor style')
            steps += [{'op': 'select', 'control': 'setting-terminal.cursor_style', 'index': 1},
                      {'op': 'screenshot', 'path': str(work / 'cursor-preview.png')},
                      {'op': 'click', 'control': 'applyPreferences'},
                      {'op': 'waitState', 'field': 'preferences.terminal.cursor_style', 'value': 'bar'},
                      {'op': 'waitState', 'field': 'frame.cursor_shape', 'value': 'bar'},
                      {'op': 'click', 'control': 'settingsButton'}]
            steps += search('selection')
            steps += [{'op': 'screenshot', 'path': str(work / 'selection-preview.png')},
                      {'op': 'click', 'control': 'cancelPreferences'},
                      {'op': 'click', 'control': 'settingsButton'}]
            steps += search('horizontal pane padding')
            steps += [{'op': 'click', 'control': 'setting-pane_padding'}, {'op': 'controlKey', 'key': 'Up'},
                      {'op': 'click', 'control': 'applyPreferences'}, {'op': 'waitState', 'field': 'preferences.pane_padding', 'value': 9},
                      {'op': 'click', 'control': 'settingsButton'}]
            steps += search('vertical pane padding')
            steps += [{'op': 'click', 'control': 'setting-interface.pane_padding_vertical'}, {'op': 'controlKey', 'key': 'Down'},
                      {'op': 'click', 'control': 'applyPreferences'}, {'op': 'waitState', 'field': 'preferences.interface.pane_padding_vertical', 'value': 7},
                      {'op': 'snapshot', 'path': str(work / 'metrics.json')},
                      {'op': 'type', 'text': 'stty size > ' + shlex.quote(str(work / 'size.txt'))}, {'op': 'key', 'key': 'Return'},
                      {'op': 'type', 'text': 'python3 ' + shlex.quote(str(mouse))}, {'op': 'key', 'key': 'Return'},
                      {'op': 'waitText', 'text': 'PREF_MOUSE_READY'}, {'op': 'pointer', 'phase': 'down', 'x': 4, 'y': 3},
                      {'op': 'pointer', 'phase': 'up', 'x': 4, 'y': 3}, {'op': 'waitText', 'text': 'PREF_MOUSE_DONE'},
                      {'op': 'click', 'control': 'settingsButton'}]
            steps += search('terminal theme')
            steps += [{'op': 'select', 'control': 'setting-terminal_theme', 'index': 4}, {'op': 'click', 'control': 'applyPreferences'},
                      {'op': 'waitState', 'field': 'frame.bg', 'value': 0x191724}, {'op': 'snapshot', 'path': str(work / 'theme.json')},
                      {'op': 'click', 'control': 'settingsButton'}]
            steps += search('terminal theme')
            steps += [{'op': 'select', 'control': 'setting-terminal_theme', 'index': 0}, {'op': 'click', 'control': 'applyPreferences'},
                      {'op': 'waitState', 'field': 'preferences.terminal_theme', 'value': 'follow_server'},
                      {'op': 'pause', 'ms': 800}, {'op': 'snapshot', 'path': str(work / 'cleared-theme.json')},
                      {'op': 'resize', 'width': 640, 'height': 480}, {'op': 'click', 'control': 'settingsButton'},
                      {'op': 'field', 'control': 'settingsSearch', 'text': ''}, {'op': 'controlKey', 'key': 'Backspace'}, {'op': 'pause', 'ms': 250},
                      {'op': 'screenshot', 'path': str(work / 'settings-minimum.png')},
                      {'op': 'snapshot', 'control': 'settingsPopup', 'path': str(work / 'minimum.json')},
                      {'op': 'click', 'control': 'resetPreferences'}, {'op': 'controlKey', 'key': 'Escape'},
                      {'op': 'click', 'control': 'cancelPreferences'}, {'op': 'click', 'control': 'settingsButton'},
                      {'op': 'click', 'control': 'resetPreferences'}, {'op': 'click', 'control': 'confirmResetPreferences'},
                      {'op': 'click', 'control': 'cancelPreferences'},
                      {'op': 'waitState', 'field': 'preferences.font_size', 'value': 16},
                      {'op': 'screenshot', 'path': str(work / 'terminal-minimum.png')}]
            run('settings', steps)
            before = json.loads((work / 'before.json').read_text())
            metrics = json.loads((work / 'metrics.json').read_text())
            assert metrics['qt_grid_left'] == 9 and metrics['qt_grid_top'] == 7
            assert metrics['qt_cell_height'] > before['qt_cell_height']
            assert tuple(map(int, (work / 'size.txt').read_text().split())) == (metrics['qt_rows'], metrics['qt_columns'])
            assert (work / 'mouse.bin').read_bytes() == b'\x1b[<0;5;4M\x1b[<0;5;4m'
            assert rune.read_bytes() == rune_before
            assert override.read_text() == '{"bindings":[]}\n'
            minimum = json.loads((work / 'minimum.json').read_text())['control']
            assert minimum['horizontalOverflow'] <= 1, minimum
            run('reload', [{'op': 'waitState', 'field': 'terminal.connected', 'value': True},
                           {'op': 'waitState', 'field': 'preferences.font_size', 'value': 16},
                           {'op': 'waitState', 'field': 'preferences.pane_padding', 'value': 9},
                           {'op': 'waitState', 'field': 'preferences.interface.pane_padding_vertical', 'value': 7},
                           {'op': 'snapshot', 'path': str(work / 'reloaded.json')}])
            # A second core client owns the same disposable terminal while the Qt settings change.
            owner_processes = []
            owner_errors = []
            stop_owner = threading.Event()
            def own_terminal():
                try:
                    ready = work / 'ownership-ready.json'
                    deadline = time.monotonic() + 25
                    while not ready.exists():
                        if stop_owner.wait(.05) or time.monotonic() > deadline:
                            raise RuntimeError('ownership fixture did not become ready')
                    owner_config = work / 'other-owner.json'
                    owner_config.write_text(json.dumps({'hosts': [{'label':'Other fixture client','endpoint':endpoint}], 'client_label':'Preferences ownership fixture'}))
                    process = subprocess.Popen([str(args.backend.resolve()), '--config', str(owner_config), '--session', sid],
                                               stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)
                    owner_processes.append(process)
                    claimed = False
                    resized = False
                    for line in process.stdout:
                        state = json.loads(line)
                        if state.get('type') != 'state' or not (state.get('terminal') or {}).get('connected'):
                            continue
                        target = {key:state[key] for key in ('host','epoch','session','window','pane')}
                        if not claimed:
                            process.stdin.write(json.dumps(dict(target,version=3,op='claim'))+'\n')
                            process.stdin.flush()
                            claimed = True
                        elif state['terminal'].get('owner') and not resized:
                            process.stdin.write(json.dumps(dict(target,version=3,op='resize',cols=81,rows=24,cell_width=11,cell_height=21))+'\n')
                            process.stdin.flush()
                            resized = True
                        if stop_owner.is_set():
                            break
                except Exception as error:
                    owner_errors.append(str(error))
            actor = threading.Thread(target=own_terminal,daemon=True)
            actor.start()
            try:
                ownership_steps = [{'op':'waitState','field':'terminal.connected','value':True},
                                   {'op':'click','control':'settingsButton'}] + search('font size') + [
                    {'op':'snapshot','path':str(work / 'ownership-ready.json')},
                    {'op':'waitState','field':'terminal.owner','value':False},
                    {'op':'waitState','field':'frame.cols','value':81},
                    {'op':'waitState','field':'frame.rows','value':24},
                    {'op':'click','control':'setting-font_size'},{'op':'controlKey','key':'Up'},
                    {'op':'click','control':'applyPreferences'},
                    {'op':'waitState','field':'preferences.font_size','value':17},
                    {'op':'pause','ms':500},{'op':'snapshot','path':str(work / 'unowned.json')}]
                run('ownership',ownership_steps)
                unowned=json.loads((work / 'unowned.json').read_text())
                assert unowned['terminal']['owner'] is False
                assert (unowned['frame']['cols'],unowned['frame']['rows']) == (81,24)
                assert unowned['qt_columns'] != 81 or unowned['qt_rows'] != 24
                assert not owner_errors,owner_errors
            finally:
                stop_owner.set()
                for process in owner_processes:
                    process.terminate()
                    try:
                        process.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        process.kill();process.wait(timeout=5)
                actor.join(timeout=5)
            cleared = json.loads((work / 'cleared-theme.json').read_text())
            reloaded = json.loads((work / 'reloaded.json').read_text())
            assert cleared['frame']['bg'] == reloaded['frame']['bg'], f"Live theme reset diverges from reconnect: {cleared['frame']['bg']:#x} vs {reloaded['frame']['bg']:#x}: {work}"
            print(json.dumps({'success': True, 'artifacts': str(work)}))
        finally:
            server.terminate()
            try:
                server.wait(timeout=5)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=5)


if __name__ == '__main__':
    main()
