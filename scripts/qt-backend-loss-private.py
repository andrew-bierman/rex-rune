#!/usr/bin/env python3
"""Exercise native Qt dialogs when their real private backend child dies.

Requires rex, a built Qt test driver and backend. Starts only a disposable Rex
server/session, uses offscreen software Qt and private HOME/runtime/services.
No existing server or desktop is accessed. Artifacts remain in .build-tools.
The active case reuses qt-files-private.py's throttled owned-socket proxy.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
CASES = {
    'directory': ('Insert path', 'files.directory', 'directoryPopup', 'directory.open'),
    'download': ('Download file', 'files.download', 'downloadPath', 'download.open'),
    'workspace': ('Close pane', 'workspace', 'workspaceConfirmation', 'workspace'),
    'active': ('Download file', 'files.download', 'downloadPath', 'download.start'),
    'maintenance-busy': ('', 'maintenance.preview', 'integrationsPopup', 'maintenance'),
    'maintenance-preview': ('', 'maintenance.preview', 'integrationPreview', 'maintenance'),
}

# This transparent wrapper forwards actual requests and responses. It never
# invents frontend state; only the timing of its owned child's death is changed.
CHILD = '''#!/usr/bin/env python3
import json, os, subprocess, sys, threading, time
p = subprocess.Popen([BACKEND] + sys.argv[1:], stdin=subprocess.PIPE, stdout=subprocess.PIPE)
def output():
    for line in p.stdout:
        sys.stdout.buffer.write(line)
        sys.stdout.buffer.flush()
def die():
    time.sleep(2)
    p.kill()
    p.wait()
    os._exit(0)
threading.Thread(target=output, daemon=True).start()
count = 0
try:
    for line in sys.stdin:
        request = json.loads(line)
        with open(__file__ + '.ops', 'a') as log:
            log.write(line)
        p.stdin.write(line.encode())
        p.stdin.flush()
        if request.get('op') == TRIGGER and (ACTION is None or request.get('action') == ACTION) and (TRIGGER != 'workspace' or request.get('action') not in ('confirm.cancel', 'confirm.accept')):
            count += 1
            if count == COUNT:
                threading.Thread(target=die, daemon=True).start()
finally:
    if p.poll() is None:
        p.terminate()
        try:
            p.wait(timeout=3)
        except subprocess.TimeoutExpired:
            p.kill()
            p.wait()
'''


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def run_case(args, kind, work, env, endpoint, session):
    title, field, control, trigger = CASES[kind]
    folder = work / kind
    folder.mkdir()
    proxy = None
    case_env = dict(env)
    maintenance = kind.startswith('maintenance-')
    try:
        if kind == 'active':
            spec = importlib.util.spec_from_file_location('private_files', ROOT / 'scripts/qt-files-private.py')
            module = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(module)
            proxy = module.SlowProxy(work / 'slow.sock', work / 'rex.sock')
            endpoint = 'unix://' + str(work / 'slow.sock')
        if maintenance:
            tools = folder / 'tools'
            tools.mkdir()
            fake = tools / 'rex'
            fake.write_text("#!/usr/bin/python3\nimport json,sys,time\n"
                + ("time.sleep(8)\n" if kind == 'maintenance-busy' else "")
                + "print(json.dumps({'harnesses':[{'id':'claude','detected':True,'items':[{'state':'current'}]}]} if 'status' in sys.argv else {'steps':[{'change':'write'}]}))\n")
            fake.chmod(0o700)
            case_env['PATH'] = str(tools) + os.pathsep + env['PATH']
        wrapper = folder / 'child.py'
        wrapper.write_text(CHILD.replace('BACKEND', repr(str(args.backend.resolve())))
                           .replace('TRIGGER', repr(trigger)).replace('COUNT', '1' if kind == 'active' or maintenance else '3')
                           .replace('ACTION', repr('status' if kind == 'maintenance-busy' else 'preview') if maintenance else 'None'))
        wrapper.chmod(0o700)
        command = [
            {'op': 'key', 'key': 'P', 'mods': 'ctrl+shift'},
            {'op': 'waitControl', 'control': 'commandsPopup', 'property': 'opened', 'value': True},
            {'op': 'field', 'control': 'commandSearch', 'text': title},
            {'op': 'controlKey', 'key': 'Return'},
            {'op': 'waitControl', 'control': control, 'property': 'visible', 'value': True},
        ]
        steps = [{'op': 'waitState', 'field': 'terminal.connected', 'value': True}]
        if maintenance:
            steps += [
                {'op':'click','control':'settingsButton'},
                {'op':'click','control':'integrationsSettingsButton'},
            ]
            if kind == 'maintenance-preview':
                steps += [
                    {'op':'waitState','field':'maintenance.status.harnesses.0.items.0.state','value':'current'},
                    {'op':'click','control':'integrationSetup_claude'},
                    {'op':'waitControl','control':'integrationPreview','property':'visible','value':True},
                ]
            else:
                steps += [{'op':'waitState','field':'maintenance.busy','value':True}]
            steps += [
                {'op':'waitProblem','text':'backend stopped'},
                {'op':'waitState','field':'maintenance.busy','value':False},
                {'op':'waitState','field':'maintenance.preview','value':None},
            ]
            if kind == 'maintenance-preview':
                steps += [{'op':'waitControl','control':'integrationPreview','property':'visible','value':False}]
            steps += [{'op':'click','control':'integrationClose'}]
        elif kind == 'active':
            source = folder / 'source.bin'
            with source.open('wb') as stream:
                stream.truncate(64 * 1024 * 1024)
            steps += command + [
                {'op': 'field', 'control': 'downloadPath', 'text': str(source)},
                {'op': 'field', 'control': 'downloadFolder', 'text': str(folder)},
                {'op': 'click', 'control': 'downloadStart'},
                {'op': 'waitState', 'field': 'downloads.0.running', 'value': True},
                {'op': 'waitProblem', 'text': 'backend stopped'},
                {'op': 'waitState', 'field': 'downloads.0.running', 'value': False},
                {'op': 'waitState', 'field': 'downloads.0.status', 'value': 'Backend unavailable; transfer outcome unknown'},
            ]
        else:
            for _ in range(2):
                steps += command + [
                    {'op': 'controlKey', 'key': 'Escape'},
                    {'op': 'waitState', 'field': field, 'value': None},
                ]
            steps += command + [{'op': 'waitProblem', 'text': 'backend stopped'}]
        steps += [
            {'op': 'waitState', 'field': field, 'value': None},
            {'op': 'waitState', 'field': 'hosts.rows.0.connected', 'value': False},
            {'op': 'waitControl', 'control': 'integrationsPopup' if maintenance else control, 'property': 'visible', 'value': False},
            {'op': 'click', 'control': 'settingsButton'},
            {'op': 'waitControl', 'control': 'settingsPopup', 'property': 'opened', 'value': True},
            {'op': 'controlKey', 'key': 'Escape'},
            {'op': 'waitControl', 'control': 'settingsPopup', 'property': 'visible', 'value': False},
            {'op': 'snapshot', 'path': str(folder / 'snapshot.json')},
        ]
        script = folder / 'steps.json'
        script.write_text(json.dumps(steps, indent=2))
        with (folder / 'driver.log').open('w') as log:
            driver = subprocess.Popen([
                str(args.driver.resolve()), '--backend', str(wrapper), '--endpoint', endpoint,
                '--rex-session', session, '--script', str(script), '--report', str(folder / 'report.json'),
            ], env=case_env, stdout=log, stderr=log, start_new_session=True)
            try:
                result = driver.wait(timeout=70)
            finally:
                # The group contains only this driver, transparent wrapper and
                # its child. Clean up even if a native assertion times out.
                try:
                    os.killpg(driver.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                stop(driver)
                try:
                    os.killpg(driver.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
        if result:
            raise AssertionError(f'{kind}: native test failed; see {folder}')
        snapshot = json.loads((folder / 'snapshot.json').read_text())
        assert not snapshot['connected'] and not snapshot['terminal']['connected']
        assert snapshot['panes'] and snapshot['frame']['cells'], 'Disconnected terminal frame was lost'
        print(f'{kind}: PASS', flush=True)
    finally:
        if proxy:
            proxy.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--driver', type=Path, default=ROOT / 'qt/build/test-driver/rex-qt-test-driver')
    parser.add_argument('--backend', type=Path, default=ROOT / 'target/release/rex-qt-backend')
    parser.add_argument('--case', choices=['all', *CASES], default='all')
    args = parser.parse_args()
    for executable in (args.driver, args.backend):
        if not executable.is_file():
            parser.error(f'build or specify executable: {executable}')
    work = Path(tempfile.mkdtemp(prefix='qt-backend-loss-', dir=ROOT / '.build-tools'))
    print(work, flush=True)
    env = {key: value for key, value in os.environ.items()
           if key not in ('DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'DBUS_SESSION_BUS_ADDRESS')
           and not key.startswith(('REX_', 'HYPR', 'HERDR_'))}
    for name in ('home', 'runtime', 'config', 'state', 'cache', 'tmp'):
        (work / name).mkdir(mode=0o700)
    env.update(HOME=str(work / 'home'), XDG_CONFIG_HOME=str(work / 'config'),
               XDG_STATE_HOME=str(work / 'state'), XDG_CACHE_HOME=str(work / 'cache'),
               XDG_RUNTIME_DIR=str(work / 'runtime'), TMPDIR=str(work / 'tmp'),
               DBUS_SESSION_BUS_ADDRESS='unix:path=' + str(work / 'absent-bus'),
               QT_QPA_PLATFORM='offscreen', QT_QUICK_BACKEND='software', QT_QPA_PLATFORMTHEME='generic', SHELL='/bin/sh')
    endpoint = 'unix://' + str(work / 'rex.sock')
    with (work / 'server.log').open('w') as log:
        server = subprocess.Popen([
            'rex', 'server', 'run', '--listen', endpoint, '--restore=false', '--config', '/dev/null',
            '--state-dir', str(work / 'server'),
        ], env=env, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 10
            while not (work / 'rex.sock').exists():
                if server.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError(f'Owned Rex did not start; see {work}')
                time.sleep(.05)
            session = json.loads(subprocess.check_output([
                'rex', '--autostart=false', '--server', endpoint, 'api', 'call', 'session.create',
                json.dumps({'label': 'Owned backend-loss regression', 'initial_windows': [
                    {'layout': {'block': {'flavor': 'com.superlogical.terminal.shell'}}}]}),
            ], env=env, timeout=20))['session_id']
            for kind in CASES if args.case == 'all' else [args.case]:
                run_case(args, kind, work, env, endpoint, session)
        finally:
            stop(server)


if __name__ == '__main__':
    main()
