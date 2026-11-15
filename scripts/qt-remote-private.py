#!/usr/bin/env python3
"""Check directed Qt client actions on two owned Rex servers and optional native Qt events.

Requires rex and built rex-qt-backend; --driver adds isolated offscreen Qt consent events
and a minimum-size capture. Never attaches to configured servers or existing sessions.
"""
import argparse
import json
import os
from pathlib import Path
import queue
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
    work = Path(tempfile.mkdtemp(prefix='qt-remote-', dir=ROOT / '.build-tools'))
    env = {k: v for k, v in os.environ.items() if not k.startswith(('REX_', 'HERDR_', 'HYPR'))
           and k not in ('DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'DBUS_SESSION_BUS_ADDRESS', 'SSH_AUTH_SOCK')}
    for folder in ('home', 'config', 'cache', 'state', 'runtime', 'tmp', 'bin'):
        (work / folder).mkdir(mode=0o700)
    (work / 'bin/tailscale').write_text('#!/bin/sh\nexit 1\n')
    (work / 'bin/tailscale').chmod(0o755)
    env.update(DBUS_SESSION_BUS_ADDRESS='unix:path='+str(work/'absent-bus'),HOME=str(work / 'home'), XDG_CONFIG_HOME=str(work / 'config'), XDG_CACHE_HOME=str(work / 'cache'),
               XDG_STATE_HOME=str(work / 'state'), XDG_RUNTIME_DIR=str(work / 'runtime'), TMPDIR=str(work / 'tmp'),
               SHELL='/bin/sh', QT_QPA_PLATFORM='offscreen', QT_QUICK_BACKEND='software', QT_QPA_PLATFORMTHEME='generic',
               PATH=str(work / 'bin') + os.pathsep + env['PATH'])
    endpoints = ['unix://' + str(work / f'{n}.sock') for n in range(2)]
    processes = []
    transcript = []
    write_lock = threading.Lock()
    def stop(process):
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
    def poll(test, description, seconds=12):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if test():
                return
            time.sleep(.04)
        raise AssertionError(description)
    def cli(host, method, **payload):
        return subprocess.run(['rex', '--autostart=false', '--server', endpoints[host], 'api', 'call', method, json.dumps(payload)],
                              env=env, text=True, capture_output=True, timeout=15)
    def api(host, method, **payload):
        reply = cli(host, method, **payload)
        assert reply.returncode == 0, (method, reply.stderr)
        return json.loads(reply.stdout)
    def clients(host):
        return [c for c in api(host, 'client.list')['clients'] if c.get('info', {}).get('label') == 'Rex Qt Quick']
    def client(host):
        rows = clients(host)
        return rows[-1].get('id', rows[-1].get('client_id')) if rows else None
    def perform(host, action, arguments=None, code=None, **target):
        reply = cli(host, 'client.perform', client_id=client(host), action=action, args=arguments or {}, timeout_ms=4000, **target)
        transcript.append({'host': host, 'action': action, 'args': arguments, 'status': reply.returncode, 'stdout': reply.stdout, 'stderr': reply.stderr})
        assert (reply.returncode != 0) if code else (reply.returncode == 0), (action, reply.stdout, reply.stderr)
        if code:
            assert code in reply.stderr.lower(), (action, reply.stderr)
    backend = None
    try:
        for n, endpoint in enumerate(endpoints):
            log = (work / f'server-{n}.log').open('w')
            p = subprocess.Popen(['rex', 'server', 'run', '--listen', endpoint, '--restore=false', '--state-dir', str(work / f'server-{n}')], env=env, stdout=log, stderr=log)
            processes.append(p)
            poll(lambda n=n: (work / f'{n}.sock').exists(), 'private server did not start')
        def window():
            return {'layout': {'block': {'flavor': 'com.superlogical.terminal.shell'}}}
        sessions = [api(n, 'session.create', label='Qt remote fixture', initial_windows=[window(), window()])['session_id'] for n in range(2)]
        config = work / 'workspace.json'
        config.write_text(json.dumps({'hosts': [{'label': f'Private {n}', 'endpoint': e} for n, e in enumerate(endpoints)],
                                      'last_host': endpoints[0], 'sessions': dict(zip(endpoints, sessions))}))
        def start_backend():
            nonlocal backend
            log = (work / 'backend.log').open('a')
            backend = subprocess.Popen([str(args.backend.resolve()), '--config', str(config)], env=env,
                                       text=True, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log)
            processes.append(backend)
            records = queue.Queue()
            process = backend
            def reader():
                for line in process.stdout:
                    record = json.loads(line)
                    records.put(record)
                    if record.get('type') == 'remote_check':
                        authorization = dict(record, op='remote_authorize', accepted=True)
                        with write_lock:
                            process.stdin.write(json.dumps(authorization) + '\n')
                            process.stdin.flush()
            threading.Thread(target=reader, daemon=True).start()
            return records
        records = start_backend()
        state = {}
        def wait(test, description, seconds=12):
            nonlocal state
            end = time.monotonic() + seconds
            while time.monotonic() < end:
                if state and test(state):
                    return
                try:
                    record = records.get(timeout=.05)
                except queue.Empty:
                    assert backend.poll() is None, 'backend exited'
                    continue
                transcript.append(record)
                assert record.get('type') != 'error', record
                if record.get('type') == 'state':
                    state = record
            raise AssertionError(description)
        def send(op, **fields):
            payload = {k: state.get(k) for k in ('host', 'epoch', 'session', 'window', 'pane')}
            payload.update(version=3, op=op, **fields)
            with write_lock:
                backend.stdin.write(json.dumps(payload) + '\n')
                backend.stdin.flush()
        wait(lambda s: s.get('connected') and all(r['connected'] for r in s['hosts']['rows']) and s.get('terminal', {}).get('connected'), 'hosts did not attach')
        poll(lambda: client(0) and client(1), 'Qt client declarations missing')
        actions = api(0, 'client.inspect', client_id=client(0))['actions']
        assert 'pane.scroll' in {a['name'] for a in actions}
        assert len([a for a in state['keymap']['actions'] if a['remote'] == 'view']) == 8
        assert not any(h.get('remote_control') for h in json.loads(config.read_text())['hosts'])
        send('visibility', visible=True)
        send('remote_context', modal=False, field=False, sequence=False, keypress=False)
        perform(0, 'client.sidebar.toggle', code='off')
        send('host', action='remote_control', args={'enabled': True})
        wait(lambda s: s['hosts']['rows'][0]['remote_control'], 'consent was not saved')
        assert json.loads(config.read_text())['hosts'][0]['remote_control']
        assert not json.loads(config.read_text())['hosts'][1].get('remote_control', False)
        original_window = state['window']
        send('workspace', action='pane.split', args={'direction': 'right'})
        wait(lambda s: len(s.get('panes', [])) == 2, 'fixture split did not attach')
        split_focus = state['pane']
        old = state['hosts']['sidebar_open']
        perform(0, 'client.sidebar.toggle')
        wait(lambda s: s['hosts']['sidebar_open'] != old, 'sidebar request did not run')
        perform(0, 'client.tab.next')
        perform(0, 'client.tab.previous')
        perform(0, 'client.tab.goto', {'index': -1})
        perform(0, 'session.select', {'session_id': sessions[0], 'window_id': original_window})
        perform(0, 'pane.focus', {'direction': 'left'})
        wait(lambda s: s['pane'] != split_focus and s['window'] == original_window, 'remote directional focus did not act on the captured tab')
        perform(0, 'pane.zoom')
        wait(lambda s: len(s.get('panes', [])) == 1, 'remote zoom did not change the visible split')
        perform(0, 'pane.scroll', {'lines': 3})
        for action in ('pane.copy', 'pane.paste', 'pane.send_key', 'session.new', 'pane.close', 'client.settings', 'pane.download', 'client.host.switch'):
            perform(0, action, code='not')
        perform(1, 'session.select', {'session_id': sessions[1]}, code='off')
        send('host', host=state['hosts']['rows'][1]['id'], epoch=state['hosts']['rows'][1]['epoch'], action='remote_control', args={'enabled': True})
        wait(lambda s: s['hosts']['rows'][1]['remote_control'], 'other host consent was not saved')
        perform(1, 'session.select', {'session_id': sessions[1]}, code='not available')
        for flag in ('modal', 'field', 'sequence'):
            values = dict(modal=False, field=False, sequence=False, keypress=False)
            values[flag] = True
            send('remote_context', **values)
            perform(0, 'client.tab.next', code='not available')
        send('remote_context', modal=False, field=False, sequence=False, keypress=True)
        perform(0, 'client.tab.next', code='typing')
        time.sleep(1.6)
        perform(0, 'client.tab.next')
        # A new worker sees saved consent; it never learns consent from a requester.
        stop(backend)
        records = start_backend()
        state = {}
        wait(lambda s: s.get('connected') and s.get('terminal', {}).get('connected'), 'restart did not attach')
        assert state['hosts']['rows'][0]['remote_control']
        stop(backend)
        if args.driver:
            saved = json.loads(config.read_text())
            saved['sidebar_open'] = False
            config.write_text(json.dumps(saved))
            # The native event toggles host consent and captures only this fixture window.
            script = work / 'native.json'
            steps = [{'op': 'waitState', 'field': 'terminal.connected', 'value': True},
                     {'op': 'resize', 'width': 640, 'height': 480},
                     {'op': 'click', 'control': 'moreButton'},
                     {'op': 'waitControl', 'control': 'actionsMenu', 'property': 'visible', 'value': True},
                     {'op': 'snapshot', 'path': str(work / 'native-more.json')},
                     {'op': 'pause', 'ms': 1200},
                     {'op': 'controlKey', 'key': 'Escape'},
                     {'op': 'click', 'control': 'settingsButton'},
                     {'op': 'waitControl', 'control': 'settingsPopup', 'property': 'visible', 'value': True},
                     {'op': 'field', 'control': 'settingsSearch', 'text': 'remote'},
                     {'op': 'pause', 'ms': 300},
                     {'op': 'screenshot', 'path': str(work / 'before-consent.png')},
                     {'op': 'snapshot', 'path': str(work / 'before-consent.json')},
                     {'op': 'pause', 'ms': 1200},
                     {'op': 'click', 'control': 'remoteConsent-host-1'},
                     {'op': 'waitState', 'field': 'hosts.rows.0.remote_control', 'value': False},
                     {'op': 'resize', 'width': 641, 'height': 480},
                     {'op': 'pause', 'ms': 200},
                     {'op': 'resize', 'width': 640, 'height': 480},
                     {'op': 'pause', 'ms': 200},
                     {'op': 'screenshot', 'path': str(work / 'remote-consent-minimum.png')},
                     {'op': 'click', 'control': 'remoteConsent-host-1'},
                     {'op': 'waitState', 'field': 'hosts.rows.0.remote_control', 'value': True},
                     {'op': 'click', 'control': 'cancelPreferences'},
                     {'op': 'pause', 'ms': 250},
                     {'op': 'key', 'key': 'X'},
                     {'op': 'snapshot', 'path': str(work / 'native-typing.json')},
                     {'op': 'pause', 'ms': 3500}]
            script.write_text(json.dumps(steps))
            log = (work / 'native.log').open('w')
            driver = subprocess.Popen([str(args.driver.resolve()), '--backend', str(args.backend.resolve()), '--config', str(config),
                                       '--script', str(script), '--report', str(work / 'native-report.json')], env=env, stdout=log, stderr=log)
            processes.append(driver)
            poll(lambda: (work / 'native-more.json').exists(), 'native More menu did not open')
            perform(0, 'client.sidebar.toggle', code='not available')
            poll(lambda: (work / 'before-consent.json').exists(), 'native settings did not open')
            perform(0, 'client.tab.next', code='not available')
            poll(lambda: (work / 'native-typing.json').exists(), 'native key event did not arrive')
            perform(0, 'client.tab.next', code='typing')
            time.sleep(1.6)
            perform(0, 'client.sidebar.toggle')
            assert driver.wait(timeout=45) == 0, (work / 'native.log').read_text()
            assert json.loads((work / 'native-report.json').read_text())['success']
        (work / 'transcript.json').write_text(json.dumps(transcript, indent=2))
        print(f'PASS private Qt remote actions; artifacts: {work}')
    finally:
        for process in reversed(processes):
            stop(process)
        (work / 'transcript.json').write_text(json.dumps(transcript, indent=2))


if __name__ == '__main__':
    main()
