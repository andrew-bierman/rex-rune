#!/usr/bin/env python3
"""Real Qt inventory proof: 128 empty sessions on two owned disposable Rex servers.

No PTYs are created: the first explicit initial_windows=[] session is checked before
large setup. Native offscreen/software Qt events search sidebar and switcher, then
assert exact session AND host endpoint identity despite colliding saved-host labels.
The elapsed bound includes driver startup and its deliberate 100 ms event cadence;
it is a regression ceiling, not a physical-display latency measurement.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--backend', required=True, type=Path)
    parser.add_argument('--driver', required=True, type=Path)
    args = parser.parse_args()
    (ROOT / '.build-tools').mkdir(exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix='qt-inventory-', dir=ROOT / '.build-tools'))
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(('REX_', 'HERDR_', 'HYPR', 'QT_', 'QML_', 'XDG_'))
           and k not in ('DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'DBUS_SESSION_BUS_ADDRESS')}
    for folder in ('home', 'config', 'cache', 'state', 'runtime', 'tmp'):
        (work / folder).mkdir(mode=0o700)
    env.update(HOME=str(work / 'home'), XDG_CONFIG_HOME=str(work / 'config'),
               XDG_CACHE_HOME=str(work / 'cache'), XDG_STATE_HOME=str(work / 'state'),
               XDG_RUNTIME_DIR=str(work / 'runtime'), TMPDIR=str(work / 'tmp'),
               DBUS_SESSION_BUS_ADDRESS='unix:path=' + str(work / 'absent-bus'),
               QT_QPA_PLATFORM='offscreen', QT_QUICK_BACKEND='software',
               QT_QPA_PLATFORMTHEME='generic', SHELL='/bin/sh')
    endpoints = ['unix://' + str(work / f'rex-{i}.sock') for i in range(2)]
    servers, logs, sessions = [], [], [[], []]
    evidence = {'artifacts': str(work), 'empty_sessions': 128, 'runs': []}

    def api(index, method, **payload):
        return json.loads(subprocess.check_output(
            ['rex', '--autostart=false', '--server', endpoints[index], 'api', 'call', method, json.dumps(payload)],
            env=env, text=True, timeout=20))

    try:
        for i, endpoint in enumerate(endpoints):
            log = (work / f'server-{i}.log').open('w')
            logs.append(log)
            servers.append(subprocess.Popen(['rex', 'server', 'run', '--listen', endpoint,
                '--restore=false', '--state-dir', str(work / f'server-{i}')], env=env, stdout=log, stderr=log))
            deadline = time.monotonic() + 10
            while not (work / f'rex-{i}.sock').exists():
                assert servers[-1].poll() is None and time.monotonic() < deadline, 'private server startup failed'
                time.sleep(.04)
        first = api(0, 'session.create', label='Inventory 000', initial_windows=[])['session_id']
        first_view = api(0, 'session.view', session_id=first)
        (work / 'empty-preflight.json').write_text(json.dumps(first_view, indent=2))
        assert first_view.get('windows', []) == [], 'ABORT: explicit empty session created windows; refusing large setup'
        sessions[0].append(first)
        for host in range(2):
            for index in range(len(sessions[host]), 64):
                label = 'Collision session' if index == 63 else f'Inventory {host * 64 + index:03}'
                sessions[host].append(api(host, 'session.create', label=label, initial_windows=[])['session_id'])
        for host in range(2):
            listed = api(host, 'session.list')['sessions']
            assert {s['session_id'] for s in listed} == set(sessions[host])
            for sid in sessions[host]:
                assert api(host, 'session.view', session_id=sid).get('windows', []) == [], 'inventory unexpectedly contains PTYs'
        evidence['empty_preflight'] = 'PASS; all 128 session views have zero windows'
        profile = work / 'config/rex-qt/workspace.json'
        profile.parent.mkdir()
        profile.write_text(json.dumps({'hosts': [{'label': 'Collision host', 'endpoint': e} for e in endpoints],
            'last_host': endpoints[0], 'sessions': dict(zip(endpoints, [s[0] for s in sessions])), 'sidebar_open': True}))

        steps = [{'op': 'waitState', 'field': 'session', 'value': first},
                 {'op': 'resize', 'width': 640, 'height': 480},
                 {'op': 'snapshot', 'path': str(work / 'initial.json')}]
        # Filter before selecting the late row: exercises the real scalable ListView
        # rather than relying on creation of all 128 delegates at once.
        for host, index in [(1, 62), (0, 62)]:
            steps += [{'op': 'field', 'control': 'sidebarSearch', 'text': f'Inventory {host * 64 + index:03}'},
                      {'op': 'click', 'control': 'sidebar-session-' + sessions[host][index]},
                      {'op': 'waitState', 'field': 'session', 'value': sessions[host][index], 'timeout': 5000},
                      {'op': 'snapshot', 'path': str(work / f'sidebar-{host}.json')}]
        steps += [{'op': 'field', 'control': 'sidebarSearch', 'text': ''}]
        for host in range(2):
            # Endpoint text disambiguates colliding host/session labels in the
            # native switcher; selecting a row must retain that exact host ID.
            steps += [{'op': 'controlKey', 'key': 'P', 'mods': 'ctrl+shift'},
                      {'op': 'waitControl', 'control': 'commandsPopup', 'property': 'opened', 'value': True, 'timeout': 5000},
                      {'op': 'field', 'control': 'commandSearch', 'text': 'Switch session'},
                      {'op': 'controlKey', 'key': 'Return'},
                      {'op': 'waitControl', 'control': 'hostSwitcher', 'property': 'opened', 'value': True, 'timeout': 5000},
                      {'op': 'field', 'control': 'switcherSearch', 'text': 'Collision session'},
                      {'op': 'snapshot', 'control': 'switcherResults', 'properties': ['count'], 'path': str(work / f'collisions-{host}.json')},
                      {'op': 'field', 'control': 'switcherSearch', 'text': 'Collision session Collision host · ' + endpoints[host]},
                      {'op': 'controlKey', 'key': 'Return'},
                      {'op': 'waitState', 'field': 'session', 'value': sessions[host][63], 'timeout': 5000},
                      {'op': 'snapshot', 'path': str(work / f'switcher-{host}.json')}]
        script = work / 'events.json'
        script.write_text(json.dumps(steps, indent=2))
        started = time.monotonic()
        with (work / 'ui.log').open('w') as log:
            run = subprocess.run([str(args.driver.resolve()), '--backend', str(args.backend.resolve()),
                '--config', str(profile), '--script', str(script), '--report', str(work / 'ui-report.json')],
                env=env, stdout=log, stderr=log, timeout=45)
        elapsed = time.monotonic() - started
        assert run.returncode == 0, f'native inventory proof failed; inspect {work}'
        for surface in ('sidebar', 'switcher'):
            for host in range(2):
                snap = json.loads((work / f'{surface}-{host}.json').read_text())
                row = next(h for h in snap['hosts']['rows'] if h['id'] == snap['host'])
                assert row['endpoint'] == endpoints[host], 'label collision retargeted a different host'
                assert snap['session'] == sessions[host][62 if surface == 'sidebar' else 63]
                assert sum(len(h['sessions']) for h in snap['hosts']['rows']) == 128
                assert not snap['windows'] and not snap['panes'], 'empty navigation created a terminal'
        for host in range(2):
            assert json.loads((work / f'collisions-{host}.json').read_text())['control']['count'] == 2
        evidence.update(success=True, elapsed_seconds=round(elapsed, 3), driver_timeout_seconds=45,
            checks=['128 real empty sessions', 'late sidebar identity on both hosts',
                    'colliding labels preserve both switcher identities', 'minimum 640x480 native controls'],
            limits=['Offscreen software Qt; no physical display latency claim',
                    'Filtered late sidebar rows; no full unfiltered list traversal benchmark'])
        print(json.dumps(evidence))
    finally:
        for server in servers:
            if server.poll() is None:
                server.terminate()
                try:
                    server.wait(timeout=8)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait(timeout=5)
        for log in logs:
            log.close()
        (work / 'result.json').write_text(json.dumps(evidence, indent=2))


if __name__ == '__main__':
    main()
