#!/usr/bin/env python3
"""Activity proof using only an owned disposable Rex server, HOME and Qt window.

Needs rex on PATH. --baseline expects the immutable pre-activity child and records
missing activity after the public status sweep discovers a real unopened session.
For native proof supply --driver; its window uses the offscreen Qt software platform.
No personal endpoint, desktop services or existing session is accessed.
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
    parser.add_argument('--backend', type=Path, required=True)
    parser.add_argument('--driver', type=Path)
    parser.add_argument('--baseline', action='store_true')
    args = parser.parse_args()
    work = Path(tempfile.mkdtemp(prefix='activity-private-', dir=ROOT / '.build-tools'))
    env = {k: v for k, v in os.environ.items() if not k.startswith(('REX_', 'HERDR_', 'HYPR'))
           and k not in ('DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'DBUS_SESSION_BUS_ADDRESS')}
    for folder in ('home', 'config', 'cache', 'state', 'runtime', 'tmp'):
        (work / folder).mkdir(mode=0o700)
    endpoint = 'unix://' + str(work / 'rex.sock')
    env.update(HOME=str(work / 'home'), XDG_CONFIG_HOME=str(work / 'config'), XDG_CACHE_HOME=str(work / 'cache'),
               XDG_STATE_HOME=str(work / 'state'), XDG_RUNTIME_DIR=str(work / 'runtime'), TMPDIR=str(work / 'tmp'),
               REX_SERVER=endpoint, SHELL='/bin/sh', PS1='activity-private$ ', QT_QPA_PLATFORM='offscreen',
               QT_QUICK_BACKEND='software', QT_QPA_PLATFORMTHEME='generic')
    profile = work / 'config/rex-qt/workspace.json'
    profile.parent.mkdir()
    unrelated = work / 'config/rex-rune/workspace.json'
    unrelated.parent.mkdir()
    unrelated.write_text('UNRELATED RUNE CONFIG\n')
    records = queue.Queue()
    transcript = []
    processes, logs = [], []
    state, version = {}, None

    def api(method, **payload):
        return json.loads(subprocess.check_output(['rex', '--autostart=false', '--server', endpoint,
                                                  'api', 'call', method, json.dumps(payload)], env=env, text=True, timeout=15))

    def poll(check, description, seconds=20):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if check():
                return
            time.sleep(.05)
        raise AssertionError(description)

    def wait(check, description, seconds=25):
        nonlocal state, version
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if state and check(state):
                return state
            try:
                r = records.get(timeout=.1)
            except queue.Empty:
                assert child.poll() is None, 'backend exited'
                continue
            transcript.append(r)
            if r.get('type') == 'hello':
                version = r['version']
            if r.get('type') == 'error':
                raise AssertionError(r['message'])
            if r.get('type') == 'state':
                state = r
        raise AssertionError(description)

    def inventory(s):
        return s.get('activity', {}).get('rows', [])

    def row(s, sid):
        return next((r for r in inventory(s) if r['session'] == sid), {})

    def send(op, **fields):
        payload = {key: state.get(key) for key in ('host', 'epoch', 'session', 'window', 'pane')}
        payload.update(version=version, op=op)
        payload.update(fields)
        child.stdin.write(json.dumps(payload) + '\n')
        child.stdin.flush()

    def program(sid, path):
        view = api('session.view', session_id=sid)
        pane = view['windows'][0]['focused_block_id']
        api('block.event', session_id=sid, block_id=pane, event={'type': 'paste', 'payload': {'text': '/usr/bin/python3 ' + shlex.quote(str(path))}})
        api('block.event', session_id=sid, block_id=pane, event={'type': 'key', 'payload': {'key': 'enter', 'action': 'press', 'mods': ''}})
        return pane

    try:
        log = (work / 'rex.log').open('w'); logs.append(log)
        server = subprocess.Popen(['rex', 'server', 'run', '--listen', endpoint, '--restore=false', '--state-dir', str(work / 'server')], env=env, stdout=log, stderr=log)
        processes.append(server)
        poll(lambda: (work / 'rex.sock').exists(), 'private server socket missing')
        sessions = []
        for label in ('Visible shell', 'Waiting agent', 'Broken build'):
            sessions.append(api('session.create', label=label, initial_windows=[{'label': label, 'layout': {'block': {'flavor': 'com.superlogical.terminal.shell'}}}])['session_id'])
        # The reporter watches an owned file, permitting deterministic live state changes
        # while Qt retains focus. No frontend behavior is simulated by the control channel.
        reporter = work / 'report.py'
        reporter.write_text('import pathlib,time,sys\np=pathlib.Path(sys.argv[0]).with_suffix(".state")\nlast=None\nwhile True:\n s=p.read_text().strip()\n if s!=last:\n  print("\\x1b]7501;state="+s+":app=claude:msg=Owned activity fixture\\x07",end="",flush=True);last=s\n time.sleep(.05)\n')
        reporter.with_suffix('.state').write_text('blocked')
        broken = work / 'broken.py'
        broken.write_text('import time\nprint("\\x1b]7501;state=error:app=cargo:msg=Owned build failure\\x07",end="",flush=True)\ntime.sleep(300)\n')
        time.sleep(.3)
        waiting_pane = program(sessions[1], reporter)
        broken_pane = program(sessions[2], broken)
        poll(lambda: any(r.get('state') == 'blocked' for r in api('com.superlogical.terminal.program_status', session_id=sessions[1], block_id=waiting_pane).get('records', [])), 'owned program did not report blocked')
        poll(lambda: any(r.get('state') == 'error' for r in api('com.superlogical.terminal.program_status', session_id=sessions[2], block_id=broken_pane).get('records', [])), 'owned program did not report error')
        profile.write_text(json.dumps({'hosts': [{'label': 'Activity fixture', 'endpoint': endpoint}], 'last_host': endpoint,
                                       'sessions': {endpoint: sessions[0]}, 'generated_titles': False}))
        log = (work / 'backend.log').open('w'); logs.append(log)
        child = subprocess.Popen([str(args.backend.resolve()), '--config', str(profile)], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=log, text=True)
        processes.append(child)

        def reader():
            for line in child.stdout:
                records.put(json.loads(line))
        threading.Thread(target=reader, daemon=True).start()
        wait(lambda s: s.get('connected') and s.get('session') == sessions[0] and len(s.get('hosts', {}).get('rows', [])) == 1, 'initial visible session did not attach')
        if args.baseline:
            # Wait beyond initial status coverage; the status fixture independently proves
            # the server data exists. Baseline emits no activity presentation at all.
            end = time.monotonic() + 3
            while time.monotonic() < end:
                try:
                    r = records.get(timeout=.1); transcript.append(r)
                    if r.get('type') == 'state': state = r
                except queue.Empty:
                    pass
            assert 'activity' not in state, 'baseline unexpectedly has activity presentation'
            result = {'baseline_missing_activity': True, 'server_blocked': True, 'server_error': True, 'selected_session_unchanged': state['session'] == sessions[0]}
        else:
            wait(lambda s: row(s, sessions[1]).get('state') == 'blocked' and row(s, sessions[2]).get('state') == 'error' and not s['activity'].get('reading'), 'unopened sessions missing from Activity')
            assert state['session'] == sessions[0], 'activity sweep selected a background session'
            assert row(state, sessions[1])['background'], 'background read marked session opened'
            assert row(state, sessions[1])['icons'][0]['slug'] == 'claude-code'
            assert row(state, sessions[1])['age'] is None or row(state, sessions[1])['age']['reported'], 'read-back age presented as live observation'
            (work / 'before.json').write_text(json.dumps(state, indent=2))
            reporter.with_suffix('.state').write_text('working')
            wait(lambda s: row(s, sessions[1]).get('state') == 'working', 'live Working missing')
            reporter.with_suffix('.state').write_text('done')
            wait(lambda s: row(s, sessions[1]).get('unseen') is True, 'live completion did not latch unseen')
            assert row(state, sessions[1])['age']['reported'] is False
            (work / 'done.json').write_text(json.dumps(state, indent=2))
            reporter.with_suffix('.state').write_text('blocked')
            wait(lambda s: row(s, sessions[1]).get('state') == 'blocked' and not row(s, sessions[1]).get('unseen'), 'new activity did not clear completion')
            result = {'unopened_reports': True, 'live_completion': True, 'monotonic_age': True, 'selected_session_unchanged': state['session'] == sessions[0]}
        (work / 'state.json').write_text(json.dumps(state, indent=2))
        child.terminate(); child.wait(timeout=10)
        if args.driver and not args.baseline:
            # Native controls navigate by the selected captured identity. Model-level
            # ordering/duplicate-name cases also have independent QtTest coverage.
            waiting_index = next(i for i, r in enumerate(inventory(state)) if r['session'] == sessions[1])
            def live_wake():
                poll(lambda: (work / 'activity-native.json').exists(), 'native initial snapshot missing')
                reporter.with_suffix('.state').write_text('working')
            wake_thread = threading.Thread(target=live_wake, daemon=True)
            wake_thread.start()
            steps = [
                {'op': 'waitState', 'field': 'session', 'value': sessions[0]},
                {'op': 'waitState', 'field': 'terminal.connected', 'value': True},
                {'op': 'resize', 'width': 640, 'height': 480},
                {'op': 'waitState', 'field': 'activity.reading', 'value': False},
                {'op': 'click', 'control': 'activityButton'},
                {'op': 'waitControl', 'control': 'activityDialog', 'property': 'opened', 'value': True},
                {'op': 'field', 'control': 'activityFilter', 'text': 'Waiting agent'},
                {'op': 'waitControl', 'control': 'activityModel', 'property': 'filter', 'value': 'Waiting agent'},
                {'op': 'snapshot', 'control': 'activityModel', 'properties': ['rows', 'selectedKey'], 'path': str(work / 'activity-native.json')},
                {'op': 'screenshot', 'path': str(work / 'activity-640.png')},
                {'op': 'waitState', 'field': f'activity.rows.{waiting_index}.state', 'value': 'working'},
                {'op': 'snapshot', 'control': 'activityModel', 'properties': ['rows', 'selectedKey'], 'path': str(work / 'activity-wake.json')},
                {'op': 'searchKey', 'key': 'Tab'},
                {'op': 'waitState', 'field': 'activity.view', 'value': 'session'},
                {'op': 'searchKey', 'key': 'Return'},
                {'op': 'waitState', 'field': 'session', 'value': sessions[1]},
                {'op': 'waitControl', 'control': 'activityDialog', 'property': 'opened', 'value': False},
                {'op': 'snapshot', 'path': str(work / 'navigated.json')},
            ]
            script = work / 'native-events.json'; script.write_text(json.dumps(steps))
            log = (work / 'native.log').open('w'); logs.append(log)
            native = subprocess.run([str(args.driver.resolve()), '--backend', str(args.backend.resolve()), '--config', str(profile), '--script', str(script), '--report', str(work / 'native-report.json')], env=env, stdout=log, stderr=log, timeout=70)
            assert native.returncode == 0, 'native Activity control flow failed'
            wake_thread.join(timeout=5)
            before_control = json.loads((work / 'activity-native.json').read_text())['control']
            after_control = json.loads((work / 'activity-wake.json').read_text())['control']
            assert before_control['selectedKey'] == after_control['selectedKey'], 'live wake changed selected target'
            assert len(after_control['rows']) == 1 and after_control['rows'][0]['session'] == sessions[1], 'native filter did not retain exactly captured target'
            assert after_control['rows'][0]['state'] == 'working', 'native model did not receive live report'
            assert json.loads(profile.read_text())['activity_view'] == 'session', 'Activity view was not persisted'
            result['native_filter_view_navigation'] = True

            # A separate fixture starts only after the original completion/navigation
            # oracle has passed. Distinct real program reports identify current and
            # background rows in rendered surfaces without test-only presentation data.
            shell = {'block': {'flavor': 'com.superlogical.terminal.shell'}}
            split = {'split': {'direction': 'horizontal', 'ratio': .5, 'before': shell, 'after': shell}}
            icon_session = api('session.create', label='Icons fixture', initial_windows=[
                {'window_label': 'Icons current', 'layout': split},
                {'window_label': 'Icons background', 'layout': split}])['session_id']
            icon_view = api('session.view', session_id=icon_session)
            current_window = icon_view['windows'][0]['window_id']
            programs = [('claude', 'cargo'), ('python', 'git')]
            time.sleep(.3)
            for window, names in zip(icon_view['windows'], programs):
                panes = [b['block_id'] for layer in window['layers'] for b in layer['blocks']
                         if b.get('creator_name') == 'com.superlogical.terminal']
                focused = window['focused_block_id']
                panes = [focused] + [pane for pane in panes if pane != focused]
                assert len(panes) == 2
                for pane, app in zip(panes, names):
                    program_file = work / ('icon-' + app + '.py')
                    program_file.write_text('import time\nprint("\\x1b]7501;state=working:app=' + app + '\\x07",end="",flush=True)\ntime.sleep(300)\n')
                    api('block.event', session_id=icon_session, block_id=pane, event={'type': 'paste', 'payload': {'text': '/usr/bin/python3 ' + shlex.quote(str(program_file))}})
                    api('block.event', session_id=icon_session, block_id=pane, event={'type': 'key', 'payload': {'key': 'enter', 'action': 'press', 'mods': ''}})
                    poll(lambda pane=pane: any(r.get('state') == 'working' for r in api('com.superlogical.terminal.program_status', session_id=icon_session, block_id=pane).get('records', [])), 'icon program did not report')
            icon_profile = json.loads(profile.read_text())
            icon_profile.update(pane_header='always', vertical_tabs=True, sidebar_open=False)
            icon_profile.setdefault('interface', {}).update(tab_icon_count=1, tab_icon_color='never')
            profile.write_text(json.dumps(icon_profile))

            def run_icons(name, events):
                script_file = work / (name + '-events.json')
                script_file.write_text(json.dumps(events))
                with (work / (name + '.log')).open('w') as native_log:
                    client = subprocess.run([str(args.driver.resolve()), '--backend', str(args.backend.resolve()),
                                             '--config', str(profile), '--script', str(script_file),
                                             '--report', str(work / (name + '-report.json'))],
                                            env=env, stdout=native_log, stderr=native_log, timeout=80)
                assert client.returncode == 0, f'{name} native icon events failed: {work}'

            def setting_search(query):
                return [{'op': 'field', 'control': 'settingsSearch', 'text': query}, {'op': 'pause', 'ms': 180}]

            def apply_setting(path, query, event, expected):
                return ([{'op': 'click', 'control': 'settingsButton'}] + setting_search(query)
                        + event + [{'op': 'click', 'control': 'applyPreferences'},
                                   {'op': 'waitState', 'field': 'preferences.' + path, 'value': expected}])

            def count_events(value, before):
                return apply_setting('interface.tab_icon_count', 'program icons per tab',
                                     [{'op': 'click', 'control': 'setting-interface.tab_icon_count'}]
                                     + [{'op': 'controlKey', 'key': 'Up' if value > before else 'Down'} for _ in range(abs(value - before))], value)

            def color_events(mode):
                index = {'never': 0, 'always': 1, 'active_pane': 2, 'active_tab': 3}[mode]
                return apply_setting('interface.tab_icon_color', 'program icon colour',
                                     [{'op': 'select', 'control': 'setting-interface.tab_icon_color', 'index': index}], mode)

            def activity_snapshot(name):
                return [{'op': 'click', 'control': 'activityButton'},
                        {'op': 'waitControl', 'control': 'activityDialog', 'property': 'opened', 'value': True},
                        {'op': 'field', 'control': 'activityFilter', 'text': 'Icons fixture'},
                        {'op': 'snapshot', 'path': str(work / (name + '-activity.json'))},
                        {'op': 'searchKey', 'key': 'Escape'},
                        {'op': 'waitControl', 'control': 'activityDialog', 'property': 'opened', 'value': False}]

            # Navigate through Activity; then native Settings changes both icon count and
            # colour. Vertical tabs start enabled, and the actual toggle exposes toolbar
            # icons while the searched sidebar continues showing both tabs.
            setup = [{'op': 'waitState', 'field': 'terminal.connected', 'value': True},
                     {'op': 'resize', 'width': 1024, 'height': 700},
                     {'op': 'waitState', 'field': 'activity.reading', 'value': False},
                     {'op': 'click', 'control': 'activityButton'},
                     {'op': 'field', 'control': 'activityFilter', 'text': 'Icons current'},
                     {'op': 'searchKey', 'key': 'Return'},
                     {'op': 'waitState', 'field': 'session', 'value': icon_session},
                     {'op': 'waitState', 'field': 'window', 'value': current_window}]
            setup += count_events(2, 1) + color_events('always')
            setup += [{'op': 'click', 'control': 'sidebarButton'},
                      {'op': 'waitState', 'field': 'hosts.sidebar_open', 'value': True},
                      {'op': 'field', 'control': 'sidebarSearch', 'text': 'Icons fixture'},
                      {'op': 'snapshot', 'path': str(work / 'icons-vertical.json')}]
            setup += apply_setting('vertical_tabs', 'tabs in sidebar',
                                   [{'op': 'click', 'control': 'setting-vertical_tabs'}], False)
            setup += [{'op': 'snapshot', 'path': str(work / 'icons-always.json')},
                      {'op': 'screenshot', 'path': str(work / 'icons-colored-1024.png')}]
            setup += activity_snapshot('icons-always')
            run_icons('icons-setup', setup)

            for mode in ('never', 'active_tab', 'active_pane'):
                events = [{'op': 'waitState', 'field': 'window', 'value': current_window},
                          {'op': 'waitState', 'field': 'terminal.connected', 'value': True},
                          {'op': 'resize', 'width': 1024, 'height': 700},
                          {'op': 'field', 'control': 'sidebarSearch', 'text': 'Icons fixture'}]
                events += color_events(mode)
                events += [{'op': 'snapshot', 'path': str(work / ('icons-' + mode + '.json'))}]
                events += activity_snapshot('icons-' + mode)
                run_icons('icons-' + mode, events)
            for cap in (0, 1):
                events = [{'op': 'waitState', 'field': 'window', 'value': current_window},
                          {'op': 'waitState', 'field': 'terminal.connected', 'value': True},
                          {'op': 'resize', 'width': 1024, 'height': 700},
                          {'op': 'field', 'control': 'sidebarSearch', 'text': 'Icons fixture'}]
                events += count_events(cap, 2 if cap == 0 else 0)
                events += [{'op': 'snapshot', 'path': str(work / f'icons-cap{cap}.json')}]
                events += activity_snapshot(f'icons-cap{cap}')
                run_icons(f'icons-cap{cap}', events)

            current_slugs = {'claude-code', 'rust'}
            background_slugs = {'python', 'git'}
            def icon_records(name, surface):
                snapshot = json.loads((work / (name + '.json')).read_text())
                assert 'qt_program_icons' in snapshot, 'driver lacks actual ProgramIcon evidence'
                return [i for i in snapshot['qt_program_icons'] if i['surface'] == surface]

            def rows_of(items):
                groups = {}
                for item in items:
                    groups.setdefault(round(item['y'], 1), []).append(item)
                return list(groups.values())

            # The sidebar session has its own single monochrome glyph, separate from the
            # tab-icon preference. Identify it from the cap-two real rendered row shape.
            sidebar_rows = rows_of(icon_records('icons-always', 'sidebar'))
            session_rows = [r for r in sidebar_rows if len(r) == 1]
            assert len(session_rows) == 1, ('sidebar session glyph row missing', sidebar_rows)
            session_y = session_rows[0][0]['y']
            def tab_sidebar(items):
                return [i for i in items if abs(i['y'] - session_y) > .5]

            monochrome = None
            evidence = []
            for mode in ('always', 'never', 'active_tab', 'active_pane'):
                for surface in ('tabs', 'header', 'sidebar', 'activity'):
                    name = 'icons-' + mode + ('-activity' if surface == 'activity' else '')
                    items = icon_records(name, surface)
                    if surface == 'sidebar': items = tab_sidebar(items)
                    assert len(items) == (4 if surface in ('sidebar', 'activity') else 2), (mode, surface, items)
                    for group in rows_of(items):
                        assert len({i['icon']['slug'] for i in group}) == len(group), ('rendered duplicate glyph', group)
                    for item in items:
                        slug = item['icon']['slug']
                        assert slug in current_slugs | background_slugs
                        current = slug in current_slugs
                        expected = mode == 'always' or (mode == 'active_tab' and current) or (mode == 'active_pane' and slug == 'claude-code')
                        assert item['colored'] == expected, (mode, surface, slug, item)
                        if expected:
                            assert item['color'].lower() == item['icon']['tint'].lower(), ('actual glyph tint mismatch', item)
                        else:
                            assert item['color'].lower() != item['icon']['tint'].lower(), ('monochrome glyph retained tint', item)
                            monochrome = monochrome or item['color'].lower()
                            assert item['color'].lower() == monochrome, ('inconsistent monochrome surface', item)
                    evidence.append({'mode': mode, 'surface': surface, 'icons': len(items)})
            for cap in (0, 1):
                for surface in ('tabs', 'header', 'sidebar', 'activity'):
                    name = f'icons-cap{cap}' + ('-activity' if surface == 'activity' else '')
                    items = icon_records(name, surface)
                    if surface == 'sidebar': items = tab_sidebar(items)
                    expected_count = 0 if cap == 0 else (2 if surface in ('header', 'sidebar', 'activity') else 1)
                    assert len(items) == expected_count, (cap, surface, items)
                    if cap == 1:
                        expected_slugs = current_slugs if surface == 'header' else {'claude-code', 'python'} if surface in ('sidebar', 'activity') else {'claude-code'}
                        assert {i['icon']['slug'] for i in items} == expected_slugs, (cap, surface, items)
                    evidence.append({'cap': cap, 'surface': surface, 'icons': len(items)})
            (work / 'icons-evidence.json').write_text(json.dumps(evidence, indent=2))
            result['native_icon_counts_colors_surfaces'] = True
        assert unrelated.read_text() == 'UNRELATED RUNE CONFIG\n'
        (work / 'result.json').write_text(json.dumps(result, indent=2))
        print(json.dumps({'success': True, 'artifacts': str(work), **result}))
    finally:
        (work / 'transcript.json').write_text(json.dumps(transcript, indent=2))
        for process in reversed(processes):
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill(); process.wait(timeout=5)
        for log in logs:
            log.close()


if __name__ == '__main__':
    main()
