#!/usr/bin/env python3
"""Real Qt events against an owned server; offscreen clipboard and history/search."""
import argparse
import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--driver', type=Path, required=True)
    parser.add_argument('--backend', type=Path, required=True)
    args = parser.parse_args()
    work = Path(tempfile.mkdtemp(prefix='terminal-parity-', dir=ROOT / '.build-tools'))
    env = {k: v for k, v in os.environ.items() if not k.startswith(('REX_', 'HERDR_')) and k not in ('DISPLAY', 'WAYLAND_DISPLAY', 'DBUS_SESSION_BUS_ADDRESS')}
    for name in ('home', 'config', 'state', 'cache', 'runtime'):
        (work / name).mkdir(mode=0o700)
    endpoint = 'unix://' + str(work / 'rex.sock')
    env.update(HOME=str(work / 'home'), XDG_CONFIG_HOME=str(work / 'config'), XDG_STATE_HOME=str(work / 'state'),
               XDG_CACHE_HOME=str(work / 'cache'), XDG_RUNTIME_DIR=str(work / 'runtime'), SHELL='/bin/sh', PS1='private$ ',
               REX_SERVER=endpoint, QT_QPA_PLATFORM='offscreen', QT_QUICK_BACKEND='software', QT_QPA_PLATFORMTHEME='generic')
    def api(method, **payload):
        return json.loads(subprocess.check_output(['rex', '--autostart=false', '--server', endpoint, 'api', 'call', method, json.dumps(payload)], env=env, text=True, timeout=20))
    with (work / 'server.log').open('w') as log:
        server = subprocess.Popen(['rex', 'server', 'run', '--listen', endpoint, '--restore=false', '--config', '/dev/null', '--state-dir', str(work / 'server')], env=env, stdout=log, stderr=log)
        try:
            for _ in range(200):
                if (work / 'rex.sock').exists(): break
                time.sleep(.05)
            sid = api('session.create', label='Terminal parity', initial_windows=[{'layout': {'block': {'flavor': 'com.superlogical.terminal.shell'}}}])['session_id']
            mouse = work / 'mouse.py'
            mouse.write_text("import os,select,termios,time,tty\n"
                "old=termios.tcgetattr(0);data=b''\n"
                "try:\n tty.setraw(0)\n os.write(1,b'\\x1b[2J\\x1b[H\\x1b[?1000h\\x1b[?1006hMOUSE_READY')\n"
                " deadline=time.monotonic()+7\n while time.monotonic()<deadline:\n"
                "  if select.select([0],[],[],.1)[0]: data+=os.read(0,1024)\n"
                "finally:\n os.write(1,b'\\x1b[?1000l\\x1b[?1006l')\n termios.tcsetattr(0,termios.TCSANOW,old)\n"
                f"open({str(work / 'mouse.bin')!r},'wb').write(data)\nprint('\\r\\nMOUSE_DONE',flush=True)\n")
            steps = [
                {'op': 'waitState', 'field': 'terminal.connected', 'value': True},
                {'op': 'paste', 'text': "printf '\\nWIDE_界 é \\n'; printf '%0200d' 0; printf 'WRAPPED_END\\n'"}, {'op': 'key', 'key': 'Return'},
                {'op': 'waitText', 'text': 'WRAPPED_END'},
                {'op': 'type', 'text': "printf '\\nHISTORY_OLD_needle\\n'; i=0; while [ $i -lt 120 ]; do printf 'line %s\\n' $i; i=$((i+1)); done; printf 'HISTORY_END\\n'"},
                {'op': 'key', 'key': 'Return'}, {'op': 'waitText', 'text': 'HISTORY_END'},
                {'op': 'wheel', 'delta': 1200}, {'op': 'waitText', 'text': 'line 75'},
                {'op': 'wheel', 'delta': -1200}, {'op': 'waitText', 'text': 'HISTORY_END'},
                {'op': 'selectAll'}, {'op': 'copy'}, {'op': 'waitClipboard', 'text': 'HISTORY_OLD_needle'},
                {'op': 'find'}, {'op': 'searchText', 'text': 'HISTORY_OLD_needle'},
                {'op': 'waitState', 'field': 'frame.search.total', 'value': 2},
                {'op': 'waitState', 'field': 'frame.search.index', 'value': 0},
                {'op': 'searchKey', 'key': 'Return'}, {'op': 'waitText', 'text': 'HISTORY_OLD_needle'},
                {'op': 'waitState', 'field': 'frame.search.index', 'value': 1},
                {'op': 'snapshot', 'path': str(work / 'history.json')},
                {'op': 'screenshot', 'path': str(work / 'search.png')},
                {'op': 'searchPrevious'}, {'op': 'waitState', 'field': 'frame.search.index', 'value': 0}, {'op': 'snapshot', 'path': str(work / 'previous.json')},
                {'op': 'searchNext'}, {'op': 'waitState', 'field': 'frame.search.index', 'value': 1}, {'op': 'snapshot', 'path': str(work / 'next.json')},
                {'op': 'searchKey', 'key': 'Escape'}, {'op': 'waitState', 'field': 'frame.search', 'value': None},
                {'op': 'wheel', 'delta': -1200}, {'op': 'pause', 'ms': 300},
                {'op': 'type', 'text': 'python3 ' + shlex.quote(str(mouse))}, {'op': 'key', 'key': 'Return'},
                {'op': 'waitText', 'text': 'MOUSE_READY'},
                {'op': 'pointer', 'phase': 'down', 'x': 4, 'y': 3},
                {'op': 'pointer', 'phase': 'up', 'x': 4, 'y': 3},
                {'op': 'wheel', 'x': 4, 'y': 3, 'delta': 120},
                {'op': 'pointer', 'phase': 'down', 'button': 'right', 'x': 4, 'y': 3},
                {'op': 'pointer', 'phase': 'up', 'button': 'right', 'x': 4, 'y': 3},
                {'op': 'waitControl', 'control': 'workspaceMenu', 'property': 'visible', 'value': True},
                {'op': 'controlKey', 'key': 'Escape'},
                {'op': 'waitControl', 'control': 'workspaceMenu', 'property': 'visible', 'value': False},
                {'op': 'pointer', 'phase': 'down', 'button': 'right', 'x': 4, 'y': 3, 'mods': 'shift'},
                {'op': 'pointer', 'phase': 'up', 'button': 'right', 'x': 4, 'y': 3, 'mods': 'shift'},
                {'op': 'pointer', 'phase': 'down', 'x': 0, 'y': 0, 'mods': 'shift'},
                {'op': 'pointer', 'phase': 'move', 'x': 5, 'y': 0, 'mods': 'shift'},
                {'op': 'pointer', 'phase': 'up', 'x': 5, 'y': 0, 'mods': 'shift'},
                {'op': 'snapshot', 'path': str(work / 'before-copy.json')}, {'op': 'copy'}, {'op': 'pause', 'ms': 500}, {'op': 'snapshot', 'path': str(work / 'debug-selection.json')}, {'op': 'waitClipboard', 'text': 'MOUSE'},
                {'op': 'snapshot', 'path': str(work / 'selection.json')},
                {'op': 'pointer', 'phase': 'double', 'x': 1, 'y': 0, 'mods': 'shift'}, {'op': 'copy'},
                {'op': 'waitClipboard', 'text': 'MOUSE_READY'},
                {'op': 'snapshot', 'path': str(work / 'word.json')},
                {'op': 'pointer', 'phase': 'triple', 'x': 1, 'y': 0, 'mods': 'shift'}, {'op': 'copy'},
                {'op': 'snapshot', 'path': str(work / 'line.json')},
                {'op': 'clipboardPolicy', 'copyOnSelection': True},
                {'op': 'pointer', 'phase': 'down', 'x': 6, 'y': 0, 'mods': 'shift'},
                {'op': 'pointer', 'phase': 'move', 'x': 11, 'y': 0, 'mods': 'shift'},
                {'op': 'pointer', 'phase': 'up', 'x': 11, 'y': 0, 'mods': 'shift'},
                {'op': 'waitClipboard', 'text': 'READY', 'exact': True},
                {'op': 'snapshot', 'path': str(work / 'auto-copy.json')},
                {'op': 'clipboardPolicy'},
                {'op': 'waitText', 'text': 'MOUSE_DONE'},
                {'op': 'type', 'text': "printf '\\033]52;c;T1NDNTJfT0s=\\007'"}, {'op': 'key', 'key': 'Return'},
                {'op': 'waitClipboard', 'text': 'OSC52_OK'},
                {'op': 'type', 'text': "sleep 1; printf '\\033]52;c;SElEREVO\\007'; printf 'HIDDEN_DONE\\n'"}, {'op': 'key', 'key': 'Return'},
                {'op': 'windowVisible', 'visible': False}, {'op': 'pause', 'ms': 1800},
                {'op': 'snapshot', 'path': str(work / 'hidden.json')},
                {'op': 'windowVisible', 'visible': True},
                {'op': 'waitText', 'text': 'HIDDEN_DONE'},
                {'op': 'pointer', 'phase': 'down', 'x': 0, 'y': 10},
                {'op': 'pointer', 'phase': 'move', 'x': 0, 'y': -5},
                {'op': 'pause', 'ms': 1800},
                {'op': 'pointer', 'phase': 'up', 'x': 0, 'y': -5},
                {'op': 'copy'}, {'op': 'pause', 'ms': 300},
                {'op': 'snapshot', 'path': str(work / 'autoscroll.json')},
                {'op': 'type', 'text': "printf '\\033[2J\\033[Hhttps://example.com/qt-fixture\\n'"}, {'op': 'key', 'key': 'Return'},
                {'op': 'waitText', 'text': 'https://example.com/qt-fixture'},
                {'op': 'pointer', 'phase': 'down', 'x': 3, 'y': 0}, {'op': 'pointer', 'phase': 'up', 'x': 3, 'y': 0},
                {'op': 'pointer', 'phase': 'down', 'x': 3, 'y': 0, 'mods': 'ctrl'},
                {'op': 'pointer', 'phase': 'up', 'x': 3, 'y': 2, 'mods': 'ctrl'},
                {'op': 'snapshot', 'path': str(work / 'link-canceled.json')},
                {'op': 'pointer', 'phase': 'down', 'x': 3, 'y': 0, 'mods': 'ctrl'},
                {'op': 'pointer', 'phase': 'up', 'x': 7, 'y': 0, 'mods': 'ctrl'},
                {'op': 'waitOpenedUrl', 'text': 'https://example.com/qt-fixture'},
                {'op': 'snapshot', 'path': str(work / 'link-opened.json')},
                {'op': 'type', 'text': "printf '\\033]133;A\\007PROMPT_BEFORE\\n'; i=0; while [ $i -lt 90 ]; do printf 'prompt filler %s\\n' $i; i=$((i+1)); done; printf '\\033]133;A\\007PROMPT_AFTER\\n'"}, {'op': 'key', 'key': 'Return'},
                {'op': 'waitText', 'text': 'PROMPT_AFTER'},
                {'op': 'promptPrevious'}, {'op': 'promptPrevious'},
                {'op': 'waitText', 'text': 'PROMPT_BEFORE'},
                {'op': 'snapshot', 'path': str(work / 'prompt-before.json')},
                {'op': 'promptNext'}, {'op': 'waitText', 'text': 'PROMPT_AFTER'},
                {'op': 'snapshot', 'path': str(work / 'prompt-after.json')},
            ]
            (work / 'events.json').write_text(json.dumps(steps))
            with (work / 'client.log').open('w') as client_log:
                result = subprocess.run([str(args.driver.resolve()), '--backend', str(args.backend.resolve()), '--endpoint', endpoint, '--rex-session', sid,
                                         '--script', str(work / 'events.json'), '--report', str(work / 'report.json')], env=env, stdout=client_log, stderr=client_log, timeout=70)
            assert result.returncode == 0, f'Qt terminal events failed ({result.returncode}): {work}'
            before_prompt = json.loads((work / 'prompt-before.json').read_text())['frame']['cells']
            after_prompt = json.loads((work / 'prompt-after.json').read_text())['frame']['cells']
            assert ''.join(cell['text'] for cell in before_prompt[0]).rstrip() == 'PROMPT_BEFORE', 'previous prompt did not anchor the old marker'
            after_text = '\n'.join(''.join(cell['text'] for cell in row) for row in after_prompt)
            assert 'PROMPT_AFTER' in after_text and 'PROMPT_BEFORE' not in after_text, 'next prompt did not restore the newer viewport'

            assert json.loads((work / 'link-canceled.json').read_text())['openedUrls'] == [], 'plain click or canceled Ctrl-click opened a link'
            assert json.loads((work / 'link-opened.json').read_text())['openedUrls'] == ['https://example.com/qt-fixture'], 'same-link release did not open exactly once'

            snapshot = json.loads((work / 'history.json').read_text())
            previous = json.loads((work / 'previous.json').read_text())['frame']['search']['index']
            following = json.loads((work / 'next.json').read_text())['frame']['search']['index']
            assert previous != following and following == snapshot['frame']['search']['index'], 'search next/previous did not restore selected match'
            assert 'HISTORY_OLD_needle' in snapshot['clipboard'] and 'line 119' in snapshot['clipboard']
            assert 'WIDE_界 é ' in snapshot['clipboard']
            assert '0' * 200 + 'WRAPPED_END' in snapshot['clipboard'], 'soft wraps changed copied text'
            assert json.loads((work / 'before-copy.json').read_text())['clipboard'] == snapshot['clipboard'], 'copy-on-selection was enabled by default'
            assert (work / 'mouse.bin').read_bytes() == b'\x1b[<0;5;4M\x1b[<0;5;4m' + b'\x1b[<64;5;4M' * 3 + b'\x1b[<2;5;4M\x1b[<2;5;4m', 'mouse/wheel cell bytes, local menu or Shift override differed'
            assert json.loads((work / 'hidden.json').read_text())['clipboard'] == 'OSC52_OK', 'hidden program changed clipboard'
            selection = json.loads((work / 'selection.json').read_text())
            assert json.loads((work / 'auto-copy.json').read_text())['clipboard'] == 'READY'
            assert selection['clipboard'] == 'MOUSE', repr(selection['clipboard'])
            assert json.loads((work / 'word.json').read_text())['clipboard'] == 'MOUSE_READY'
            assert json.loads((work / 'line.json').read_text())['clipboard'].rstrip() == 'MOUSE_READY'
            autoscroll = json.loads((work / 'autoscroll.json').read_text())
            assert autoscroll['clipboard'].count('\n') > autoscroll['frame']['rows'], 'drag did not retain selection through autoscroll'
            print(f'PASS history selection and search: {work}')
        finally:
            server.terminate()
            try: server.wait(timeout=5)
            except subprocess.TimeoutExpired: server.kill(); server.wait()


if __name__ == '__main__':
    main()
