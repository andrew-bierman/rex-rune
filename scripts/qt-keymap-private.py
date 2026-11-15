#!/usr/bin/env python3
"""Exercise real Qt Commands/recording/keys against an owned Rex server only."""
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
    parser.add_argument('--driver', type=Path, default=ROOT / 'qt/build/test-driver/rex-qt-test-driver')
    parser.add_argument('--backend', type=Path, default=ROOT / 'target/release/rex-qt-backend')
    args = parser.parse_args()
    (ROOT / '.build-tools').mkdir(exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix='qt-keys-', dir=ROOT / '.build-tools'))
    env = {k: v for k, v in os.environ.items() if not k.startswith(('REX_', 'HERDR_'))
           and k not in ('DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'DBUS_SESSION_BUS_ADDRESS')}
    for folder in ('home', 'config', 'cache', 'state', 'runtime'):
        (work / folder).mkdir(mode=0o700)
    endpoint = 'unix://' + str(work / 'rex.sock')
    env.update(HOME=str(work / 'home'), XDG_CONFIG_HOME=str(work / 'config'), XDG_CACHE_HOME=str(work / 'cache'),
               XDG_STATE_HOME=str(work / 'state'), XDG_RUNTIME_DIR=str(work / 'runtime'), REX_SERVER=endpoint,
               SHELL='/bin/sh', PS1='keys-private$ ', QT_QPA_PLATFORM='offscreen', QT_QUICK_BACKEND='software', QT_QPA_PLATFORMTHEME='generic')
    config = work / 'init.lua'
    config.write_text('''
local count=0
rex.action({name="qt_once",title="Qt once action",run=function(ctx)
 count=count+1
 rex.call("session.set_label",{session_id=ctx.session_id,label="Qt action "..count.." "..ctx.origin})
end})
rex.bind("ctrl+shift+y","qt_once")
rex.mode("qt-mode",{exclusive=true})
rex.bind("ctrl+alt+r","client.mode.enter",{name="qt-mode",once=true})
rex.bind("qt-mode/x","qt_once")
rex.action({name="qt_queue",title="Qt queue action",run=function(ctx)
 rex.call("session.set_label",{session_id=ctx.session_id,label="Qt queued"})
 rex.client.queue("client.search")
end})
rex.bind("ctrl+alt+q","qt_queue")
''')
    subprocess.run(['rex', '--autostart=false', 'config', 'check', str(config)], env=env, check=True, stdout=subprocess.DEVNULL)
    qt_config = work / 'config' / 'rex-qt'
    qt_config.mkdir()
    user_file = qt_config / 'workspace.shortcuts.json'
    original = json.dumps({'shortcuts': [{'keys': 'ctrl+b>c', 'action': 'pane.copy'}]})
    user_file.write_text(original)

    def api(method, **payload):
        return json.loads(subprocess.check_output(['rex', '--autostart=false', '--server', endpoint, 'api', 'call', method,
                                                   json.dumps(payload)], env=env, text=True, timeout=20))

    with (work / 'server.log').open('w') as log:
        server = subprocess.Popen(['rex', 'server', 'run', '--listen', endpoint, '--restore=false', '--config', str(config),
                                   '--state-dir', str(work / 'server')], env=env, stdout=log, stderr=log)
        try:
            for _ in range(100):
                if (work / 'rex.sock').exists():
                    break
                assert server.poll() is None, 'private keymap server exited'
                time.sleep(.05)
            sid = api('session.create', label='Qt keys', initial_windows=[{'layout': {'block': {'flavor': 'com.superlogical.terminal.shell'}}}])['session_id']
            raw = work / 'raw.py'
            raw.write_text("import os,select,sys,termios,time,tty\n"
                           "old=termios.tcgetattr(0);data=b''\n"
                           "try:\n tty.setraw(0)\n os.write(1,b'\\x1b[2J\\x1b[HRAW_READY')\n deadline=time.monotonic()+7\n"
                           " while time.monotonic()<deadline:\n  if select.select([0],[],[],.1)[0]:data+=os.read(0,1024)\n"
                           "finally:termios.tcsetattr(0,termios.TCSANOW,old)\n"
                           f"open({str(work / 'raw.bin')!r},'wb').write(data)\nprint('\\r\\nRAW_DONE',flush=True)\n")
            enhanced = work / 'enhanced.py'
            enhanced.write_text("import os,select,termios,time,tty\n"
                                "old=termios.tcgetattr(0);data=b''\n"
                                "try:\n tty.setraw(0)\n os.write(1,b'\\x1b[>11u\\x1b[2J\\x1b[HENHANCED_READY')\n deadline=time.monotonic()+4\n"
                                " while time.monotonic()<deadline:\n  if select.select([0],[],[],.1)[0]:data+=os.read(0,1024)\n"
                                "finally:\n os.write(1,b'\\x1b[<u')\n termios.tcsetattr(0,termios.TCSANOW,old)\n"
                                f"open({str(work / 'enhanced.bin')!r},'wb').write(data)\nprint('\\r\\nENHANCED_DONE',flush=True)\n")
            steps = [
                {'op': 'waitState', 'field': 'terminal.connected', 'value': True},
                {'op': 'waitState', 'field': 'keymap.revision', 'value': 1},
                {'op': 'type', 'text': 'python3 ' + shlex.quote(str(raw))}, {'op': 'key', 'key': 'Return'},
                {'op': 'waitText', 'text': 'RAW_READY'},
                {'op': 'selectAll'}, {'op': 'key', 'key': 'C', 'mods': 'ctrl+shift'},
                {'op': 'waitClipboard', 'text': 'RAW_READY'},
                {'op': 'key', 'key': 'B', 'mods': 'ctrl'}, {'op': 'snapshot', 'path': str(work / 'prefix.json')},
                {'op': 'key', 'key': 'Escape'}, {'op': 'snapshot', 'path': str(work / 'canceled.json')},
                {'op': 'key', 'key': 'B', 'mods': 'ctrl'}, {'op': 'key', 'key': 'C'},
                {'op': 'key', 'key': 'Y', 'mods': 'ctrl+shift'},
                {'op': 'waitState', 'field': 'sessions.0.label', 'value': 'Qt action 1 key'},
                {'op': 'key', 'key': 'R', 'mods': 'ctrl+alt'}, {'op': 'key', 'key': 'Escape'},
                {'op': 'snapshot', 'path': str(work / 'mode-exited.json')},
                {'op': 'key', 'key': 'R', 'mods': 'ctrl+alt'}, {'op': 'key', 'key': 'Z'}, {'op': 'key', 'key': 'X'},
                {'op': 'waitState', 'field': 'sessions.0.label', 'value': 'Qt action 2 key'},
                {'op': 'type', 'text': 'z'}, {'op': 'waitText', 'text': 'RAW_DONE'},
                {'op': 'key', 'key': 'P', 'mods': 'ctrl+shift'},
                {'op': 'waitControl', 'control': 'commandsPopup', 'property': 'opened', 'value': True},
                {'op': 'field', 'control': 'commandSearch', 'text': 'Qt once action'}, {'op': 'controlKey', 'key': 'Return'},
                {'op': 'waitState', 'field': 'sessions.0.label', 'value': 'Qt action 3 palette'},
                {'op': 'resize', 'width': 640, 'height': 480}, {'op': 'pause', 'ms': 150},
                {'op': 'click', 'control': 'settingsButton'},
                {'op': 'waitControl', 'control': 'shortcutPopup', 'property': 'opened', 'value': True},
                {'op': 'snapshot', 'control': 'shortcutPopup', 'path': str(work / 'shortcut-layout.json')},
                {'op': 'screenshot', 'path': str(work / 'before-remove-minimum.png')},
                {'op': 'click', 'control': 'removeShortcut-ctrl+shift+c'},
                {'op': 'waitState', 'field': 'keymap.generation', 'value': 2},
                {'op': 'click', 'control': 'recordShortcut'}, {'op': 'controlKey', 'key': 'J', 'mods': 'ctrl+alt'},
                {'op': 'click', 'control': 'recordShortcut'}, {'op': 'click', 'control': 'saveShortcut'},
                {'op': 'waitState', 'field': 'keymap.generation', 'value': 3},
                {'op': 'waitState', 'field': 'keymap.revision', 'value': 3},
                {'op': 'screenshot', 'path': str(work / 'shortcuts.png')}, {'op': 'controlKey', 'key': 'Escape'},
                {'op': 'type', 'text': 'python3 ' + shlex.quote(str(enhanced))}, {'op': 'key', 'key': 'Return'},
                {'op': 'waitText', 'text': 'ENHANCED_READY'},
                {'op': 'key', 'key': 'C', 'mods': 'ctrl+shift'}, {'op': 'key', 'key': 'J', 'mods': 'ctrl+alt'},
                {'op': 'waitText', 'text': 'ENHANCED_DONE'},
                {'op': 'key', 'key': 'Q', 'mods': 'ctrl+alt'},
                {'op': 'waitState', 'field': 'sessions.0.label', 'value': 'Qt queued'},
                {'op': 'waitControl', 'control': 'terminalSearch', 'property': 'visible', 'value': True},
                {'op': 'searchKey', 'key': 'Escape'}, {'op': 'snapshot', 'path': str(work / 'final.json')},
            ]
            (work / 'events.json').write_text(json.dumps(steps))
            with (work / 'client.log').open('w') as client_log:
                result = subprocess.run([str(args.driver.resolve()), '--backend', str(args.backend.resolve()), '--endpoint', endpoint,
                                         '--session', sid, '--script', str(work / 'events.json'), '--report', str(work / 'report.json')],
                                        env=env, stdout=client_log, stderr=client_log, timeout=90)
            assert result.returncode == 0, f'Qt keymap fixture failed: {work}'
            reopened_helper = work / 'reopened.py'
            reopened_helper.write_text(enhanced.read_text().replace('ENHANCED_READY','REOPEN_READY').replace('ENHANCED_DONE','REOPEN_DONE').replace('enhanced.bin','reopen.bin'))
            reopened_steps = [
                {'op': 'waitState', 'field': 'terminal.connected', 'value': True},
                {'op': 'waitState', 'field': 'keymap.revision', 'value': 1},
                {'op': 'type', 'text': 'python3 ' + shlex.quote(str(reopened_helper))}, {'op': 'key', 'key': 'Return'},
                {'op': 'waitText', 'text': 'REOPEN_READY'},
                {'op': 'key', 'key': 'C', 'mods': 'ctrl+shift'}, {'op': 'key', 'key': 'J', 'mods': 'ctrl+alt'},
                {'op': 'waitText', 'text': 'REOPEN_DONE'}, {'op': 'snapshot', 'path': str(work / 'reopened.json')},
            ]
            (work / 'reopen-events.json').write_text(json.dumps(reopened_steps))
            with (work / 'reopened.log').open('w') as client_log:
                reopened = subprocess.run([str(args.driver.resolve()), '--backend', str(args.backend.resolve()), '--endpoint', endpoint,
                                          '--session', sid, '--script', str(work / 'reopen-events.json'), '--report', str(work / 'reopen-report.json')],
                                         env=env, stdout=client_log, stderr=client_log, timeout=40)
            assert reopened.returncode == 0, f'persisted shortcut restart failed: {work}'
            assert (work / 'reopen.bin').read_bytes().removeprefix(b'\x1b[13;1:3u') == b'\x1b[99;6u\x1b[99;6:3u', 'saved bindings changed after restart'
            copy_action = next(a for a in json.loads((work / 'reopened.json').read_text())['keymap']['actions'] if a['id'] == 'pane.copy')
            assert copy_action['hint'] == 'Ctrl Alt J', 'recorded hint did not survive restart'
            layout = json.loads((work / 'shortcut-layout.json').read_text())['control']
            assert layout['actualContentWidth'] <= layout['width'] - 2 * layout['padding'] + 1, 'minimum shortcut content exceeds its popup'
            assert layout['horizontalOverflow'] <= 1, 'minimum shortcut form overflows its layout'
            assert (work / 'raw.bin').read_bytes() == b'z', 'consumed prefix/action/mode keys leaked to the PTY'
            enhanced_bytes = (work / 'enhanced.bin').read_bytes().removeprefix(b'\x1b[13;1:3u')
            assert enhanced_bytes == b'\x1b[99;6u\x1b[99;6:3u', f'removal/new recorded binding broke key edges: {enhanced_bytes!r}'
            assert json.loads((work / 'prefix.json').read_text())['shortcut_pending']
            assert not json.loads((work / 'canceled.json').read_text())['shortcut_pending']
            assert not json.loads((work / 'mode-exited.json').read_text())['shortcut_modes'], 'Escape did not leave the active mode'
            assert user_file.read_text() == original, 'recording changed the user-owned overrides'
            saved = json.loads((qt_config / 'workspace.json').read_text())
            assert any(c.get('remove') and c['keys'] == ['ctrl+shift+c'] for c in saved['recorded_shortcuts'])
            assert any(c['keys'] == ['ctrl+alt+j'] and c['action'] == 'pane.copy' for c in saved['recorded_shortcuts'])
            print(json.dumps({'success': True, 'artifacts': str(work), 'checks': ['Commands', 'default copy', 'prefix cancel/no leak',
                             'exclusive one-shot mode', 'server action once and origin', 'queued client action', 'record/remove/persist without user-file writes']}))
        finally:
            if server.poll() is None:
                server.terminate()
                server.wait(timeout=10)


if __name__ == '__main__':
    main()
