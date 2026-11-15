#!/usr/bin/env python3
"""Verify Qt host isolation on two owned private Rex servers; optional real native UI flow.

Never uses the caller's Rex server/display/config. All endpoints, process IDs, input,
clipboard content and captured windows belong to this disposable fixture.
"""
import argparse
import json
import os
from pathlib import Path
import queue
import shlex
import signal
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--backend', type=Path, default=ROOT / 'target/debug/rex-qt-backend')
    parser.add_argument('--driver', type=Path)
    args = parser.parse_args()
    (ROOT / '.build-tools').mkdir(exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix='qt-hosts-', dir=ROOT / '.build-tools'))
    env = {k: v for k, v in os.environ.items() if not k.startswith(('REX_', 'HERDR_', 'HYPR'))
           and k not in ('DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'DBUS_SESSION_BUS_ADDRESS')}
    for folder in ('home', 'config', 'cache', 'state', 'runtime', 'tmp'):
        (work / folder).mkdir(mode=0o700)
    endpoints = ['unix://' + str(work / f'rex-{i}.sock') for i in range(2)]
    env.update(HOME=str(work / 'home'), XDG_CONFIG_HOME=str(work / 'config'), XDG_CACHE_HOME=str(work / 'cache'),
               XDG_STATE_HOME=str(work / 'state'), XDG_RUNTIME_DIR=str(work / 'runtime'), TMPDIR=str(work / 'tmp'),
               REX_SERVER=endpoints[0], SHELL='/bin/sh', PS1='hosts-private$ ', QT_QPA_PLATFORM='offscreen',
               QT_QUICK_BACKEND='software', QT_QPA_PLATFORMTHEME='generic')
    # Replace only the external command edge: never enumerate or probe the owner's real tailnet.
    # This exercises core error translation and Qt discovery state, not reachability on a real tailnet.
    tools = work / 'tools'
    tools.mkdir()
    tailscale = tools / 'tailscale'
    tailscale.write_text('#!/bin/sh\n[ "$#" -eq 2 ] && [ "$1" = status ] && [ "$2" = --json ] || exit 64\necho "discovery fixture unavailable" >&2\nexit 1\n')
    tailscale.chmod(0o755)
    env['PATH'] = str(tools) + os.pathsep + env.get('PATH', '')
    profile = work / 'config/rex-qt/workspace.json'
    profile.parent.mkdir()
    override = profile.with_name('workspace.shortcuts.json')
    override.write_text('{"shortcuts":[]}\n')
    override_before = override.read_bytes()
    rune = work / 'config/rex-rune/workspace.json'
    rune.parent.mkdir()
    rune.write_text('DO NOT READ OR CHANGE THIS UNRELATED RUNE PROFILE\n')
    rune_before = rune.read_bytes()
    servers, logs = [], []
    backend = None
    version = None
    state = {}
    records = queue.Queue()
    transcript = []

    def api(index, method, **payload):
        return json.loads(subprocess.check_output(['rex', '--autostart=false', '--server', endpoints[index], 'api', 'call',
                                                  method, json.dumps(payload)], env=env, text=True, timeout=20))

    def poll(test, description, seconds=10):
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            if test():
                return
            time.sleep(.04)
        raise AssertionError(description)

    def wait(test, description, error=False, seconds=20):
        nonlocal state, version
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            if not error and state and test(state):
                return state
            try:
                record = records.get(timeout=.1)
            except queue.Empty:
                assert backend.poll() is None, 'backend exited unexpectedly'
                continue
            transcript.append(record)
            if record.get('type') == 'hello':
                version = record['version']
            if record.get('type') == 'error':
                if error and test(record):
                    return record
                raise AssertionError(record['message'])
            if record.get('type') == 'state':
                state = record
            if error and test(record):
                return record
        raise AssertionError(description)

    def send(op, **fields):
        payload = {key: state.get(key) for key in ('host', 'epoch', 'session', 'window', 'pane')}
        payload.update(version=version, op=op)
        payload.update(fields)
        backend.stdin.write(json.dumps(payload) + '\n')
        backend.stdin.flush()

    def host(action, arguments=None, **captured):
        send('host', action=action, args=arguments or {}, **captured)

    def shell_line(text):
        send('paste', text=text)
        send('key', payload={'key': 'enter', 'action': 'press', 'mods': ''})
        send('key', payload={'key': 'enter', 'action': 'release', 'mods': ''})

    def row(id):
        return next(h for h in state['hosts']['rows'] if h['id'] == id)

    try:
        for i, endpoint in enumerate(endpoints):
            log = (work / f'server-{i}.log').open('w')
            logs.append(log)
            server = subprocess.Popen(['rex', 'server', 'run', '--listen', endpoint, '--restore=false',
                                       '--state-dir', str(work / f'server-{i}')], env=env, stdout=log, stderr=log)
            servers.append(server)
            poll(lambda i=i: (work / f'rex-{i}.sock').exists(), 'private server did not start')
        sessions = []
        for i in range(2):
            sid = api(i, 'session.create', label='Same workspace', initial_windows=[{'label': 'Same tab', 'layout': {'block': {'flavor': 'com.superlogical.terminal.shell'}}}])['session_id']
            sessions.append(sid)
        other = api(0, 'session.create', label='History target', initial_windows=[{'layout': {'block': {'flavor': 'com.superlogical.terminal.shell'}}}])['session_id']
        for i in range(12):
            last_navigation = api(0, 'session.create', label=f'Navigation row {i:02}', initial_windows=[{'layout': {'block': {'flavor': 'com.superlogical.terminal.shell'}}}])['session_id']
        profile.write_text(json.dumps({'hosts': [{'label': 'Same host', 'endpoint': e} for e in endpoints],
                                       'last_host': endpoints[1], 'sessions': dict(zip(endpoints, sessions)), 'sidebar_open': True,
                                       'vertical_tabs': True}))
        log = (work / 'backend.log').open('w')
        logs.append(log)
        backend = subprocess.Popen([str(args.backend.resolve()), '--config', str(profile)], env=env, text=True,
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log)
        def reader():
            for line in backend.stdout:
                records.put(json.loads(line))
        threading.Thread(target=reader, daemon=True).start()
        wait(lambda s: len(s.get('hosts', {}).get('rows', [])) == 2 and all(h['connected'] for h in s['hosts']['rows'])
             and (s.get('terminal') or {}).get('connected'), 'two hosts did not connect')
        h0, h1 = [h['id'] for h in state['hosts']['rows']]
        original_h0 = h0
        assert state['host'] == h1 and state['session'] == sessions[1], 'saved last_host/preferred session ignored'
        assert row(h0)['epoch'] == row(h1)['epoch'], 'fixture needs equal per-host epochs'
        assert row(h0)['id'] != row(h1)['id'] and row(h0)['label'] == row(h1)['label'], 'duplicate host labels were not isolated'
        second_marker = work / 'second-host.txt'
        shell_line('printf host-two > ' + shlex.quote(str(second_marker)))
        poll(second_marker.exists, 'input did not reach selected second host')
        bad = work / 'wrong-host.txt'
        send('paste', host=h0, text='printf WRONG > ' + shlex.quote(str(bad)) + '\n')
        wait(lambda e: 'host' in e.get('message', '').lower(), 'equal-epoch stale host accepted', error=True)
        assert not bad.exists()
        host('tab_step', {'direction': 1}, host=h0, epoch=row(h0)['epoch'])
        wait(lambda e: 'active host' in e.get('message', '').lower(), 'stale contextual host action retargeted active host', error=True)
        # Settle private prefetch before pausing; selecting an already-selected session alone
        # can acknowledge locally, so include a real focus-window RPC to hold navigation.
        wait(lambda s: len(row(h0)['sessions']) == 14 and all(session['tabs'] for session in row(h0)['sessions']), 'private sidebar prefetch did not settle')
        held_window = api(0, 'session.view', session_id=sessions[0])['windows'][0]['window_id']
        # No user server is signalled; only this fixture's own process is paused.
        os.kill(servers[0].pid, signal.SIGSTOP)
        host('navigate', {'session': sessions[0], 'window': held_window}, host=h0, epoch=row(h0)['epoch'])
        wait(lambda s: s['hosts']['pending'], 'navigation was not held pending')
        send('paste', text='printf WRONG > ' + shlex.quote(str(bad)) + '\n')
        wait(lambda e: 'pending' in e.get('message', '').lower(), 'pending navigation accepted old input', error=True)
        # Supersede the held request. Old completion must not make the first target current.
        host('navigate', {'session': other}, host=h0, epoch=row(h0)['epoch'])
        os.kill(servers[0].pid, signal.SIGCONT)
        wait(lambda s: s['host'] == h0 and s['session'] == other and not s['hosts']['pending']
             and (s.get('terminal') or {}).get('connected'), 'superseding navigation did not settle on exact target')
        assert not bad.exists()
        first_marker = work / 'first-host.txt'
        shell_line('printf host-one > ' + shlex.quote(str(first_marker)))
        poll(first_marker.exists, 'input did not reach selected first host')
        assert second_marker.read_text() == 'host-two'
        host('history', {'direction': -1})
        wait(lambda s: s['host'] == h1 and s['session'] == sessions[1] and not s['hosts']['pending'], 'back did not restore exact host/session')
        host('history', {'direction': 1})
        wait(lambda s: s['host'] == h0 and s['session'] == other and not s['hosts']['pending'], 'forward did not restore exact host/session')
        # Legacy selectors and later local workspace saves must share the supervisor's newest profile.
        send('select_session', session=last_navigation)
        wait(lambda s: s['host'] == h0 and s['session'] == last_navigation and not s['hosts']['pending']
             and (s.get('terminal') or {}).get('connected'), 'legacy selector did not reach the exact session')
        send('workspace', action='client.tab.mute', args={}, width=640, height=480)
        wait(lambda s: any(tab['session'] == last_navigation and tab['endpoint'] == endpoints[0]
                          for tab in json.loads(profile.read_text()).get('muted_tabs', [])), 'workspace mute was not saved')
        persisted = json.loads(profile.read_text())
        assert persisted['sessions'][endpoints[0]] == last_navigation, 'workspace save overwrote the latest selected session'
        assert persisted['last_host'] == endpoints[0], 'workspace save overwrote the latest active host'
        host('navigate', {'session': other}, host=h0, epoch=row(h0)['epoch'])
        wait(lambda s: s['session'] == other and not s['hosts']['pending']
             and (s.get('terminal') or {}).get('connected'), 'navigation after local settings save failed')
        # Editing only a label keeps the worker identity; changing endpoint creates a new identity.
        host('edit', {'label': 'Renamed host', 'endpoint': endpoints[0], 'shell': '/bin/sh'}, host=h0, epoch=row(h0)['epoch'])
        wait(lambda e: e.get('type') == 'host_saved', 'host edit was not acknowledged', error=True)
        wait(lambda s: row(h0)['label'] == 'Renamed host', 'host edit projection not published')
        assert json.loads(profile.read_text())['hosts'][0]['label'] == 'Renamed host'
        host('disconnect', host=h0)
        wait(lambda s: not row(h0)['enabled'] and not s['connected'], 'explicit client disconnect did not pause input')
        assert api(0, 'session.view', session_id=other)['windows'], 'client disconnect stopped server programs'
        old_h0 = h0
        old_epoch = row(h0)['epoch']
        host('connect', host=h0)
        wait(lambda s: old_h0 not in [h['id'] for h in s['hosts']['rows']], 'explicitly recreated worker retained stale identity')
        h0 = next(h['id'] for h in state['hosts']['rows'] if h['endpoint'] == endpoints[0])
        wait(lambda s: row(h0)['connected'] and (s.get('terminal') or {}).get('connected'), 'explicit client reconnect failed')
        send('paste', host=old_h0, epoch=old_epoch, text='printf WRONG > ' + shlex.quote(str(bad)) + '\n')
        wait(lambda e: 'host' in e.get('message', '').lower(), 'recreated worker accepted old captured host', error=True)
        assert not bad.exists()
        # Explicit Add/Edit/Remove persists only Qt's catalog; unreachable endpoints never start servers.
        unreachable = 'unix://' + str(work / 'unreachable.sock')
        host('add', {'label': 'Unreachable fixture', 'endpoint': unreachable, 'shell': ''})
        wait(lambda e: e.get('type') == 'host_saved', 'host Add was not acknowledged', error=True)
        wait(lambda s: len(s['hosts']['rows']) == 3, 'added host inventory missing')
        added = state['hosts']['rows'][-1]['id']
        wait(lambda s: not row(added)['connected'] and 'Connecting' not in row(added)['status'], 'unreachable host state not visible')
        replaced_endpoint = 'unix://' + str(work / 'replacement.sock')
        host('edit', {'label': 'Replacement fixture', 'endpoint': replaced_endpoint, 'shell': ''}, host=added, epoch=row(added)['epoch'])
        wait(lambda e: e.get('type') == 'host_saved', 'endpoint edit was not acknowledged', error=True)
        wait(lambda s: added not in [h['id'] for h in s['hosts']['rows']], 'endpoint edit retained stale host identity')
        replacement = state['hosts']['rows'][-1]['id']
        assert replacement != added
        host('activate', host=added)
        wait(lambda e: 'host' in e.get('message', '').lower(), 'removed host ID retargeted replacement', error=True)
        host('remove', host=replacement, epoch=row(replacement)['epoch'])
        wait(lambda s: len(s['hosts']['rows']) == 2, 'host remove did not persist')
        assert len(json.loads(profile.read_text())['hosts']) == 2
        assert not (work / 'unreachable.sock').exists() and not (work / 'replacement.sock').exists()
        host('discover')
        wait(lambda s: 'discovery fixture unavailable' in (s['hosts'].get('discovery', {}).get('problem') or ''), 'discovery failure not visible')
        assert len(state['hosts']['rows']) == 2, 'discovery saved a host without explicit Add'
        host('sidebar', {'open': False})
        wait(lambda s: not s['hosts']['sidebar_open'], 'sidebar preference not applied')
        host('sidebar', {'open': True})
        wait(lambda s: s['hosts']['sidebar_open'], 'sidebar preference not restored')
        (work / 'state.json').write_text(json.dumps(state, indent=2))
        send('shutdown')
        backend.wait(timeout=8)
        backend = None
        if args.driver:
            # Actual native events: searchable switcher disambiguates duplicate labels; sidebar scrolls at minimum size.
            steps = [
                {'op': 'waitState', 'field': 'terminal.connected', 'value': True},
                {'op': 'resize', 'width': 640, 'height': 480},
                {'op': 'key', 'key': 'P', 'mods': 'ctrl+shift'},
                {'op': 'waitControl', 'control': 'commandsPopup', 'property': 'opened', 'value': True},
                {'op': 'field', 'control': 'commandSearch', 'text': 'Switch host'},
                {'op': 'controlKey', 'key': 'Return'},
                {'op': 'waitControl', 'control': 'hostSwitcher', 'property': 'opened', 'value': True},
                {'op': 'field', 'control': 'switcherSearch', 'text': 'Same host'},
                {'op': 'controlKey', 'key': 'Return'},
                {'op': 'waitState', 'field': 'host', 'value': h1},
                {'op': 'waitState', 'field': 'session', 'value': sessions[1]},
                {'op': 'waitState', 'field': 'terminal.connected', 'value': True},
                {'op': 'key', 'key': 'P', 'mods': 'ctrl+shift'},
                {'op': 'waitControl', 'control': 'commandsPopup', 'property': 'opened', 'value': True},
                {'op': 'field', 'control': 'commandSearch', 'text': 'Switch session'},
                {'op': 'controlKey', 'key': 'Return'},
                {'op': 'waitControl', 'control': 'hostSwitcher', 'property': 'opened', 'value': True},
                {'op': 'field', 'control': 'switcherSearch', 'text': 'Navigation row 11'},
                {'op': 'controlKey', 'key': 'Return'},
                {'op': 'waitState', 'field': 'host', 'value': original_h0},
                {'op': 'waitState', 'field': 'session', 'value': last_navigation},
                {'op': 'waitState', 'field': 'terminal.connected', 'value': True},
                {'op': 'click', 'control': 'sidebarButton'},
                {'op': 'waitState', 'field': 'hosts.sidebar_open', 'value': False},
                {'op': 'waitControl', 'control': 'hostDrawer', 'property': 'visible', 'value': False},
                {'op': 'type', 'text': "printf 'QT_NATIVE_HOST_%s\\n' INPUT"},
                {'op': 'key', 'key': 'Return'},
                {'op': 'waitText', 'text': 'QT_NATIVE_HOST_INPUT'},
                {'op': 'pointer', 'button': 'back', 'phase': 'click', 'x': 2, 'y': 2},
                {'op': 'waitState', 'field': 'host', 'value': h1},
                {'op': 'waitState', 'field': 'session', 'value': sessions[1]},
                {'op': 'waitState', 'field': 'terminal.connected', 'value': True},
                {'op': 'pointer', 'button': 'forward', 'phase': 'click', 'x': 2, 'y': 2},
                {'op': 'waitState', 'field': 'host', 'value': original_h0},
                {'op': 'waitState', 'field': 'session', 'value': last_navigation},
                {'op': 'waitState', 'field': 'terminal.connected', 'value': True},
                {'op': 'waitText', 'text': 'QT_NATIVE_HOST_INPUT'},
                {'op': 'screenshot', 'path': str(work / 'native-terminal-640.png')},
                {'op': 'click', 'control': 'sidebarButton'},
                {'op': 'waitState', 'field': 'hosts.sidebar_open', 'value': True},
                {'op': 'click', 'control': 'addHostButton'},
                {'op': 'waitControl', 'control': 'hostForm', 'property': 'opened', 'value': True},
                {'op': 'field', 'control': 'hostName', 'text': 'Not saved'},
                {'op': 'field', 'control': 'hostEndpoint', 'text': 'invalid-endpoint'},
                {'op': 'click', 'control': 'saveHost'},
                {'op': 'waitProblem', 'text': 'Use an'},
                {'op': 'waitControl', 'control': 'hostForm', 'property': 'opened', 'value': True},
                {'op': 'field', 'control': 'hostEndpoint', 'text': 'unix:///does-not-exist-qt-fixture'},
                {'op': 'click', 'control': 'discoverHosts'},
                {'op': 'waitState', 'field': 'hosts.discovery.problem', 'value': 'Tailscale: discovery fixture unavailable'},
                {'op': 'screenshot', 'path': str(work / 'host-form-640.png')},
                {'op': 'click', 'control': 'cancelHost'},
                {'op': 'waitControl', 'control': 'hostForm', 'property': 'opened', 'value': False},
                {'op': 'snapshot', 'path': str(work / 'native.json')},
                {'op': 'screenshot', 'path': str(work / 'sidebar-640.png')},
            ]
            script = work / 'native-events.json'
            script.write_text(json.dumps(steps))
            with (work / 'native.log').open('w') as native_log:
                client = subprocess.run([str(args.driver.resolve()), '--backend', str(args.backend.resolve()), '--config', str(profile),
                                         '--script', str(script), '--report', str(work / 'native-report.json')],
                                        env=env, stdout=native_log, stderr=native_log, timeout=60)
            assert client.returncode == 0, f'native host flow failed: {work}'
            assert len(json.loads(profile.read_text())['hosts']) == 2, 'canceling Add saved a host'
            native = json.loads((work / 'native.json').read_text())
            active = next(host for host in native['hosts']['rows'] if host['id'] == native['host'])
            assert json.loads(profile.read_text())['last_host'] == active['endpoint'], 'cross-host navigation did not persist the actual active host'

        assert override.read_bytes() == override_before, 'host edits changed the read-only shortcuts file'
        assert rune.read_bytes() == rune_before, 'Qt touched the unrelated Rune profile'
        (work / 'transcript.json').write_text(json.dumps(transcript, indent=2))
        print(json.dumps({'success': True, 'artifacts': str(work), 'checks': ['duplicate-label host isolation', 'same-epoch stale-host rejection',
              'pending input refusal', 'superseded navigation', 'exact history', 'settings round-trip', 'client-only disconnect/reconnect',
              'separate Rune profile', 'native minimum-size navigation' if args.driver else 'native UI not requested']}))
    finally:
        (work / 'transcript.json').write_text(json.dumps(transcript, indent=2))
        (work / 'last-state.json').write_text(json.dumps(state, indent=2))
        if backend and backend.poll() is None:
            backend.terminate()
            backend.wait(timeout=8)
        for server in servers:
            if server.poll() is None:
                os.kill(server.pid, signal.SIGCONT)
                server.terminate()
                server.wait(timeout=8)
        for log in logs:
            log.close()


if __name__ == '__main__':
    main()
