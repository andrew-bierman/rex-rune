#!/usr/bin/env python3
"""Exercise workspace mutations on a disposable private Rex server, never the user's socket.

Uses the real backend IPC and optional Qt test driver. Pass --driver for native Commands,
forms and default-cancel proof as well. All sessions, configs and process IDs are owned here.
"""
import argparse
import json
import os
from pathlib import Path
import queue
import shlex
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--backend', type=Path, default=ROOT / 'target/release/rex-qt-backend')
    parser.add_argument('--driver', type=Path)
    args = parser.parse_args()
    work = Path(tempfile.mkdtemp(prefix='qt-workspace-', dir=ROOT / '.build-tools'))
    env = {k: v for k, v in os.environ.items() if not k.startswith(('REX_', 'HERDR_', 'HYPR'))
           and k not in ('DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'DBUS_SESSION_BUS_ADDRESS')}
    for folder in ('home', 'config', 'cache', 'state', 'runtime', 'tmp', 'cwd'):
        (work / folder).mkdir(mode=0o700)
    endpoint = 'unix://' + str(work / 'rex.sock')
    env.update(HOME=str(work / 'home'), XDG_CONFIG_HOME=str(work / 'config'), XDG_CACHE_HOME=str(work / 'cache'),
               XDG_STATE_HOME=str(work / 'state'), XDG_RUNTIME_DIR=str(work / 'runtime'), TMPDIR=str(work / 'tmp'),
               REX_SERVER=endpoint, SHELL='/bin/sh', PS1='workspace-private$ ', QT_QPA_PLATFORM='offscreen',
               QT_QUICK_BACKEND='software', QT_QPA_PLATFORMTHEME='generic')
    def api(method, **payload):
        return json.loads(subprocess.check_output(['rex', '--autostart=false', '--server', endpoint, 'api', 'call',
                                                   method, json.dumps(payload)], env=env, text=True, timeout=20))
    def poll(test, description, seconds=10):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if test():
                return
            time.sleep(.04)
        raise AssertionError(description)
    def alive(pid):
        try:
            os.kill(pid, 0)
            return True
        except ProcessLookupError:
            return False

    backend = None
    with (work / 'server.log').open('w') as log:
        server = subprocess.Popen(['rex', 'server', 'run', '--listen', endpoint, '--restore=false',
                                   '--state-dir', str(work / 'server')], env=env, stdout=log, stderr=log)
        try:
            poll(lambda: (work / 'rex.sock').exists(), 'private server did not start')
            sid = api('session.create', label='Qt workspace IPC', initial_windows=[{'layout': {'block': {'flavor': 'com.superlogical.terminal.shell'}}}])['session_id']
            backend_log = (work / 'backend.log').open('w')
            backend = subprocess.Popen([str(args.backend.resolve()), '--endpoint', endpoint, '--session', sid], env=env,
                                       text=True, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=backend_log)
            records = queue.Queue()
            def reader():
                for line in backend.stdout:
                    records.put(json.loads(line))
            threading.Thread(target=reader, daemon=True).start()
            state = {}
            transcript = []
            def wait(test, description, expect_error=False, seconds=15):
                nonlocal state
                end = time.monotonic() + seconds
                while time.monotonic() < end:
                    if not expect_error and state and test(state):
                        return state
                    try:
                        record = records.get(timeout=.1)
                    except queue.Empty:
                        assert backend.poll() is None, 'backend exited'
                        continue
                    transcript.append(record)
                    if record.get('type') == 'error':
                        if expect_error and test(record):
                            return record
                        raise AssertionError(record['message'])
                    if record.get('type') == 'state':
                        state = record
                raise AssertionError(description)
            def send(op, **fields):
                payload = {key: state.get(key) for key in ('host', 'epoch', 'session', 'window', 'pane')}
                payload.update(version=2, op=op)
                payload.update(fields)
                backend.stdin.write(json.dumps(payload) + '\n')
                backend.stdin.flush()
            def shell_line(text):
                send('paste', text=text)
                send('key', payload={'key': 'enter', 'action': 'press', 'mods': ''})
                send('key', payload={'key': 'enter', 'action': 'release', 'mods': ''})
            def action(name, arguments=None, **target):
                send('workspace', action=name, args=arguments or {}, width=900, height=600, **target)
            def question(name):
                old = (state.get('workspace') or {}).get('id')
                action(name)
                return dict(wait(lambda s: bool(s.get('workspace')) and s['workspace']['id'] != old,
                                 'confirmation did not appear')['workspace'])
            def answer(captured, yes):
                action('confirm.accept' if yes else 'confirm.cancel', {'id': captured['id']},
                       **{key: captured.get(key, state.get(key)) for key in ('host', 'epoch', 'session', 'window', 'pane')})

            wait(lambda s: (s.get('terminal') or {}).get('connected'), 'terminal did not attach')
            first = state['pane']
            original_window = state['window']
            shell_line('cd ' + shlex.quote(str(work / 'cwd')) + '; pwd > ' + shlex.quote(str(work / 'source-cwd')))
            poll(lambda: (work / 'source-cwd').exists(), 'source shell did not execute input')
            action('pane.split', {'direction': 'right'})
            wait(lambda s: len(s.get('panes', [])) == 2, 'split did not appear')
            second = state['pane']
            assert first != second
            action('pane.focus', {'direction': 'left'}, pane=second)
            wait(lambda s: s.get('pane') == first, 'directional focus did not follow server')
            action('pane.resize', {'direction': 'left', 'amount': 5})
            wait(lambda s: s['dividers'][0]['ratio'] < .5, 'keyboard resize did not reach server')
            action('pane.balance')
            wait(lambda s: s['dividers'][0]['ratio'] == .5, 'balance did not reach server')
            action('pane.move', {'direction': 'right'})
            wait(lambda s: s['dividers'][0]['before'] == second, 'swap did not reach server')
            action('pane.zoom')
            wait(lambda s: len(s.get('panes', [])) == 1, 'zoom did not reach server')
            action('pane.zoom')
            wait(lambda s: len(s.get('panes', [])) == 2, 'restore did not reach server')
            action('client.tab.new')
            wait(lambda s: len(s.get('windows', [])) == 2 and s['window'] != original_window and (s.get('terminal') or {}).get('connected'), 'new tab did not appear')
            destination = state['window']
            shell_line('pwd > ' + shlex.quote(str(work / 'inherited-cwd')))
            poll(lambda: (work / 'inherited-cwd').exists(), 'new tab did not execute shell input')
            assert (work / 'inherited-cwd').read_text().strip() == str(work / 'cwd'), 'core NewTerminal lost cwd inheritance'
            action('client.tab.move', {'direction': 'left'})
            wait(lambda s: s['windows'][0]['id'] == destination, 'tab order did not update')
            action('client.tab.mute')
            poll(lambda: (work / 'config/rex-qt/workspace.json').exists(), 'local mute was not saved')
            saved = json.loads((work / 'config/rex-qt/workspace.json').read_text())
            assert any(m['window'] == destination for m in saved['muted_tabs']), 'mute persistence missing'
            action('client.tab.rename', {'label': 'Named destination'})
            wait(lambda s: any(w['label'] == 'Named destination' for w in s['windows']), 'tab rename missing')
            action('client.tab.rename', {'label': ''})
            wait(lambda s: all(w['label'] != 'Named destination' for w in s['windows']), 'default title not restored')
            send('select_window', window=original_window)
            wait(lambda s: s['window'] == original_window, 'original tab navigation missing')
            action('pane.move_to_tab', {'window_id': destination}, pane=first)
            wait(lambda s: s['window'] == destination and len(s['panes']) == 2, 'move-to-tab did not follow server focus')

            # A real long-running program proves cancel keeps the process alive.
            helper = work / 'alive.py'
            helper.write_text('import os,time\nopen(' + repr(str(work / 'program.pid')) + ',"w").write(str(os.getpid()))\nprint("LIFE_READY",flush=True)\nwhile True: time.sleep(.1)\n')
            shell_line('python3 ' + shlex.quote(str(helper)))
            poll(lambda: (work / 'program.pid').exists(), 'test program did not start')
            pid = int((work / 'program.pid').read_text())
            captured = question('pane.close')
            answer(captured, False)
            wait(lambda s: not s.get('workspace'), 'cancel did not dismiss question')
            assert alive(pid), 'cancel killed a running program'

            # Navigate away after capturing a destructive target; an old confirm must fail.
            captured = question('pane.close')
            send('select_window', window=original_window)
            wait(lambda s: s['window'] == original_window and not s.get('workspace'), 'view change did not cancel stale question')
            answer(captured, True)
            wait(lambda r: 'question' in r['message'].lower() or 'changed' in r['message'].lower(), 'stale confirmation was accepted', expect_error=True)
            assert alive(pid), 'stale confirmation killed the captured program'
            send('select_window', window=destination)
            wait(lambda s: s['window'] == destination and s['pane'] == first, 'return navigation missing')
            captured = question('pane.close')
            answer(captured, True)
            poll(lambda: not alive(pid), 'confirmed close did not end owned test program')
            wait(lambda s: first not in [p['id'] for p in s['panes']], 'closed pane stayed in scene')

            # Other tabs are the IDs named when the question opens, including race proof.
            captured = question('client.tab.close_others')
            later = api('session.new_window', session_id=sid, layout={'block': {'flavor': 'com.superlogical.terminal.shell'}}, focus=False)['window_id']
            answer(captured, True)
            poll(lambda: [w['window_id'] for w in api('session.view', session_id=sid)['windows']] == [destination, later], 'close-others closed a later-created tab or retained an old one')
            captured = question('session.close')
            answer(captured, False)
            wait(lambda s: not s.get('workspace'), 'session cancel did not clear question')
            assert any(s['session_id'] == sid for s in api('session.list')['sessions']), 'session cancel destroyed session'
            captured = question('session.close')
            answer(captured, True)
            wait(lambda s: all(row['id'] != sid for row in s['sessions']), 'session close did not remove owned session')
            empty = api('session.create', label='Qt empty workspace')['session_id']
            send('select_session', session=empty)
            wait(lambda s: s['session'] == empty and not s['windows'], 'empty session did not attach')
            action('client.tab.new')
            wait(lambda s: len(s['windows']) == 1 and (s.get('terminal') or {}).get('connected'), 'explicit new tab in empty session failed')
            captured = question('session.close')
            answer(captured, True)
            wait(lambda s: all(row['id'] != empty for row in s['sessions']), 'empty-session test cleanup failed')
            send('shutdown')
            backend.wait(timeout=8)
            backend = None
            backend_log.close()
            (work / 'ipc-transcript.json').write_text(json.dumps(transcript, indent=2))

            if args.driver:
                ui_sid = api('session.create', label='Qt workspace controls', initial_windows=[{'layout': {'block': {'flavor': 'com.superlogical.terminal.shell'}}}])['session_id']
                def command(title):
                    return [{'op': 'key', 'key': 'P', 'mods': 'ctrl+shift'},
                            {'op': 'waitControl', 'control': 'commandsPopup', 'property': 'opened', 'value': True},
                            {'op': 'field', 'control': 'commandSearch', 'text': title}, {'op': 'controlKey', 'key': 'Return'}]
                steps = [{'op': 'waitState', 'field': 'terminal.connected', 'value': True}]
                steps += command('New tab')
                steps += [{'op': 'waitState', 'field': 'windows.1.label', 'value': 'Shell'}]
                steps += command('Rename tab')
                steps += [{'op': 'waitControl', 'control': 'workspaceForm', 'property': 'opened', 'value': True},
                          {'op': 'field', 'control': 'workspaceName', 'text': 'Qt renamed'}, {'op': 'click', 'control': 'workspaceFormApply'},
                          {'op': 'waitState', 'field': 'windows.1.label', 'value': 'Qt renamed'}]
                steps += [{'op': 'resize', 'width': 640, 'height': 480}]
                steps += command('Split pane')
                steps += [{'op': 'waitControl', 'control': 'workspaceForm', 'property': 'opened', 'value': True},
                          {'op': 'click', 'control': 'workspaceFormApply'},
                          {'op': 'waitState', 'field': 'panes.1.terminal.connected', 'value': True},
                          {'op': 'dragDivider', 'index': 0, 'ratio': .65},
                          {'op': 'waitState', 'field': 'dividers.0.ratio', 'value': .65, 'tolerance': .02},
                          {'op': 'snapshot', 'path': str(work / 'divider-drag.json')}]
                steps += command('Balance splits')
                steps += [{'op': 'waitState', 'field': 'dividers.0.ratio', 'value': .5},
                          {'op': 'key', 'key': 'W', 'mods': 'ctrl+shift'},
                          {'op': 'waitControl', 'control': 'workspaceConfirmation', 'property': 'opened', 'value': True},
                          {'op': 'screenshot', 'path': str(work / 'confirm.png')}, {'op': 'controlKey', 'key': 'Return'},
                          {'op': 'waitControl', 'control': 'workspaceConfirmation', 'property': 'visible', 'value': False},
                          {'op': 'snapshot', 'path': str(work / 'cancelled.json')}]
                steps += [{'op': 'key', 'key': 'W', 'mods': 'ctrl+shift'},
                          {'op': 'waitControl', 'control': 'workspaceConfirmation', 'property': 'opened', 'value': True},
                          {'op': 'click', 'control': 'workspaceConfirmClose'}, {'op': 'pause', 'ms': 300},
                          {'op': 'snapshot', 'path': str(work / 'pane-confirmed.json')}]
                steps += command('Close tab')
                steps += [{'op': 'waitControl', 'control': 'workspaceConfirmation', 'property': 'opened', 'value': True},
                          {'op': 'controlKey', 'key': 'Escape'},
                          {'op': 'waitControl', 'control': 'workspaceConfirmation', 'property': 'visible', 'value': False}]
                steps += command('Close tab')
                steps += [{'op': 'waitControl', 'control': 'workspaceConfirmation', 'property': 'opened', 'value': True},
                          {'op': 'click', 'control': 'workspaceConfirmClose'}, {'op': 'pause', 'ms': 300},
                          {'op': 'snapshot', 'path': str(work / 'confirmed.json')}]
                (work / 'events.json').write_text(json.dumps(steps))
                with (work / 'ui.log').open('w') as ui_log:
                    result = subprocess.run([str(args.driver.resolve()), '--backend', str(args.backend.resolve()), '--endpoint', endpoint,
                                             '--session', ui_sid, '--script', str(work / 'events.json'), '--report', str(work / 'ui-report.json')],
                                            env=env, stdout=ui_log, stderr=ui_log, timeout=75)
                assert result.returncode == 0, f'Qt controls failed; inspect {work}'
                assert len(json.loads((work / 'cancelled.json').read_text())['panes']) == 2, 'default Enter closed a pane'
                assert len(json.loads((work / 'pane-confirmed.json').read_text())['panes']) == 1, 'explicit confirmation did not close pane'
                assert len(json.loads((work / 'confirmed.json').read_text())['windows']) == 1, 'explicit confirmation did not close tab'
                api('session.destroy', session_id=ui_sid)
            (work / 'result.json').write_text(json.dumps({'ipc': 'PASS', 'qt_controls': 'PASS' if args.driver else 'NOT_RUN'}, indent=2))
            print(f'PASS workspace lifecycle, layout, process survival, stale target and captured close-others: {work}')
        finally:
            if 'transcript' in locals():
                (work / 'ipc-transcript.json').write_text(json.dumps(transcript, indent=2))
            if backend is not None:
                backend.terminate()
                try:
                    backend.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    backend.kill()
                    backend.wait(timeout=5)
            server.terminate()
            try:
                server.wait(timeout=8)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait(timeout=5)


if __name__ == '__main__':
    main()
