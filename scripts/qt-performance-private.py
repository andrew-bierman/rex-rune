#!/usr/bin/env python3
"""Bounded output/echo measurements on an owned Rex server and offscreen Qt.

Requires rex, built Qt test driver/backend. Five seconds of 100 VT row updates/s per
pane; this is a reproducible workload, not a maximum throughput claim. The
proxy measures actual child JSON bytes/arrival times. Echo timing measures Qt
key dispatch to the production item's visible text, with driver polling
quantization; it does not measure physical scanout. No clipboard/service use.
"""
import argparse
import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import subprocess
import tempfile
import time
import threading
import textwrap
import statistics

ROOT = Path(__file__).resolve().parents[1]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--backend', type=Path, default=ROOT / 'target/release/rex-qt-backend')
    p.add_argument('--driver', type=Path, default=ROOT / 'qt/build/test-driver/rex-qt-test-driver')
    p.add_argument('--artifacts', type=Path)
    p.add_argument('--backend-only', action='store_true', help='Drain identical snapshots without Qt to attribute frontend cost')
    p.add_argument('--baseline', action='store_true', help='Use existing keys/waits without timed driver op')
    a = p.parse_args()
    assert shutil.which('rex') and a.backend.is_file() and a.driver.is_file()
    work = (a.artifacts or Path(tempfile.mkdtemp(prefix='qt-performance-', dir=ROOT / '.build-tools'))).resolve()
    work.mkdir(parents=True, exist_ok=True)
    env = {k: v for k, v in os.environ.items() if not k.startswith(('REX_', 'HERDR_')) and k not in ('DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'DBUS_SESSION_BUS_ADDRESS')}
    for name in ('home', 'config', 'state', 'cache', 'runtime'):
        (work / name).mkdir(mode=0o700, exist_ok=True)
    endpoint = 'unix://' + str(work / 'rex.sock')
    env.update(DBUS_SESSION_BUS_ADDRESS='unix:path='+str(work/'absent-bus'),HOME=str(work / 'home'), XDG_CONFIG_HOME=str(work / 'config'), XDG_STATE_HOME=str(work / 'state'), XDG_CACHE_HOME=str(work / 'cache'), XDG_RUNTIME_DIR=str(work / 'runtime'), REX_SERVER=endpoint, SHELL='/bin/sh', QT_QPA_PLATFORM='offscreen', QT_QUICK_BACKEND='software', QT_QPA_PLATFORMTHEME='generic', QT_SCALE_FACTOR='1')
    def api(method, **payload):
        return json.loads(subprocess.check_output(['rex', '--autostart=false', '--server', endpoint, 'api', 'call', method, json.dumps(payload)], env=env, text=True, timeout=20))
    log = (work / 'server.log').open('w')
    server = subprocess.Popen(['rex', 'server', 'run', '--listen', endpoint, '--restore=false', '--config', '/dev/null', '--state-dir', str(work / 'server')], env=env, stdout=log, stderr=log)
    client = None
    results = []
    try:
        deadline = time.monotonic() + 20
        while not (work / 'rex.sock').exists():
            assert server.poll() is None and time.monotonic() < deadline, 'server startup'
            time.sleep(.05)
        for count in (1, 4):
            load_start = os.getloadavg()
            case = work / str(count)
            case.mkdir()
            shell = {'block': {'flavor': 'com.superlogical.terminal.shell'}}
            layout = shell if count == 1 else {'split': {'direction': 'horizontal', 'ratio': .5, 'before': {'split': {'direction': 'vertical', 'ratio': .5, 'before': shell, 'after': shell}}, 'after': {'split': {'direction': 'vertical', 'ratio': .5, 'before': shell, 'after': shell}}}}
            sid = api('session.create', label=f'Private Qt performance {count}', initial_windows=[{'window_label': 'Output', 'layout': layout}])['session_id']
            view = api('session.view', session_id=sid)
            blocks = [b['block_id'] for layer in view['windows'][0]['layers'] for b in layer['blocks'] if b.get('creator_name') == 'com.superlogical.terminal']
            assert len(blocks) == count
            fixture = case / 'output.py'
            fixture.write_text(textwrap.dedent(rf"""
                import os
                import select
                import time
                import tty

                tty.setraw(0)
                os.write(1, b'READY')
                while not os.path.exists({str(case / 'start')!r}):
                    time.sleep(.01)
                begin = time.monotonic()
                updates = 0
                data = b''
                done = False
                while True:
                    elapsed = time.monotonic() - begin
                    if elapsed < 5 and updates < int(elapsed * 100):
                        line = '\x1b[%d;1HOUTPUT %06d ' % (3 + updates % 5, updates)
                        os.write(1, (line + 'x' * 28).encode())
                        updates += 1
                    if elapsed >= 5 and not done:
                        os.write(1, b'\x1b[10;1HDONE')
                        done = True
                    if select.select([0], [], [], .005)[0]:
                        chunk = os.read(0, 1024)
                        data += chunk
                        for character in chunk:
                            os.write(1, b'\x1b7\x1b[1;1HECHO_' + bytes([character]) + b'\x1b8')
                        with open({str(case / 'bytes')!r} + '.' + str(os.getpid()), 'wb') as output:
                            output.write(data)
            """))
            for block in blocks:
                api('block.event', session_id=sid, block_id=block, event={'type': 'paste', 'payload': {'text': 'python3 '+shlex.quote(str(fixture))}})
                api('block.event', session_id=sid, block_id=block, event={'type': 'key', 'payload': {'key': 'enter', 'action': 'press', 'mods': ''}})
            if a.backend_only:
                client = subprocess.Popen([str(a.backend.resolve()), '--endpoint', endpoint, '--session', sid], env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE, start_new_session=True)
                records = []
                latest = [{}]
                errors = []
                def drain():
                    for line in client.stdout:
                        item = json.loads(line)
                        if item.get('type') == 'error': errors.append(item)
                        if item.get('type') == 'state':
                            latest[0] = item
                            records.append({'t': time.monotonic(), 'bytes': len(line)})
                reader = threading.Thread(target=drain, daemon=True)
                reader.start()
                deadline = time.monotonic()+20
                while not (latest[0].get('terminal') or {}).get('connected'):
                    assert time.monotonic()<deadline, 'backend attachment'
                    time.sleep(.02)
                state = latest[0]
                for pane in blocks:
                    command = {'version': 3, 'op': 'resize', 'host': state['host'], 'epoch': state['epoch'], 'session': sid, 'window': state['window'], 'pane': pane, 'cols': 108 if count==1 else 52, 'rows': 30 if count==1 else 13, 'cell_width': 9, 'cell_height': 20}
                    claim = dict(command, op='claim')
                    client.stdin.write((json.dumps(claim)+'\n').encode());client.stdin.flush()
                    time.sleep(.3)
                    client.stdin.write((json.dumps(command)+'\n').encode());client.stdin.flush()
                time.sleep(.3)
                assert not errors, errors
                for pane in blocks:
                    size = api('com.superlogical.terminal.size', session_id=sid, block_id=pane)
                    assert (size['columns'], size['rows']) == (108 if count == 1 else 52, 30 if count == 1 else 13), size
                start = time.monotonic(); (case / 'start').touch()
                time.sleep(5.3)
                selected = [r for r in records if start <= r['t'] <= start+5]
                intervals = [1000*(b['t']-a['t']) for a,b in zip(selected, selected[1:])]
                results.append({'load_average_start': load_start, 'load_average_end': os.getloadavg(), 'panes': count, 'backend_only': True, 'snapshot_count': len(selected), 'snapshot_bytes': sum(r['bytes'] for r in selected), 'max_snapshot_interval_ms': max(intervals, default=0), 'median_snapshot_interval_ms': statistics.median(intervals) if intervals else None})
                client.terminate();client.wait(timeout=10);reader.join(timeout=5)
                continue
            proxy = case / 'proxy.py'
            proxy.write_text('#!/usr/bin/env python3\n' + textwrap.dedent(f"""
                import json
                import subprocess
                import sys
                import threading
                import time

                child = subprocess.Popen([{str(a.backend.resolve())!r}, *sys.argv[1:]],
                                         stdin=subprocess.PIPE, stdout=subprocess.PIPE)
                def forward_input():
                    for line in sys.stdin.buffer:
                        child.stdin.write(line)
                        child.stdin.flush()
                threading.Thread(target=forward_input, daemon=True).start()
                try:
                    with open({str(case / 'traffic.jsonl')!r}, 'w') as traffic:
                        for line in child.stdout:
                            item = json.loads(line)
                            traffic.write(json.dumps(dict(t=time.monotonic(), bytes=len(line),
                                                          type=item.get('type'))) + '\\n')
                            traffic.flush()
                            sys.stdout.buffer.write(line)
                            sys.stdout.buffer.flush()
                finally:
                    child.terminate()
                    child.wait()
            """))
            proxy.chmod(0o700)
            steps = [{'op': 'waitState', 'field': 'terminal.connected', 'value': True}, {'op': 'waitText', 'text': 'READY'}]
            for index in range(count):
                steps += [{'op': 'clickPane', 'index': index}, {'op': 'waitState', 'field': 'pane', 'value': blocks[index]}, {'op': 'key', 'key': 'z'}, {'op': 'pause', 'ms': 200}]
            steps += [{'op': 'clickPane', 'index': 0}, {'op': 'waitState', 'field': 'pane', 'value': blocks[0]}]
            steps += [{'op': 'snapshot', 'path': str(case / 'ready.json')}, {'op': 'pause', 'ms': 300}]
            for letter in 'abc':
                if a.baseline:
                    steps += [{'op': 'key', 'key': letter}, {'op': 'waitText', 'text': 'ECHO_'+letter}]
                else:
                    steps += [{'op': 'measureEcho', 'key': letter, 'text': 'ECHO_'+letter, 'timeout': 20000}]
                steps += [{'op': 'pause', 'ms': 300}]
            steps += [{'op': 'waitText', 'text': 'DONE'}, {'op': 'snapshot', 'path': str(case / 'final.json')}, {'op': 'pause', 'ms': 2000}]
            (case / 'steps.json').write_text(json.dumps(steps))
            with (case / 'client.log').open('w') as out:
                launched = time.monotonic()
                client = subprocess.Popen([str(a.driver.resolve()), '--backend', str(proxy), '--endpoint', endpoint, '--rex-session', sid, '--script', str(case / 'steps.json'), '--report', str(case / 'report.json')], env=env, stdout=out, stderr=out, start_new_session=True)
                deadline = time.monotonic()+90
                while not (case / 'ready.json').exists():
                    assert client.poll() is None and time.monotonic()<deadline, 'Qt readiness'
                    time.sleep(.02)
                start = time.monotonic()
                ready_ms = 1000 * (start - launched)
                (case / 'start').touch()
                deadline = time.monotonic()+90
                while not (case / 'final.json').exists():
                    assert client.poll() is None and time.monotonic()<deadline, f'Qt output/echo failed: {case}'
                    time.sleep(.02)
                completion_ms = 1000 * (time.monotonic() - start)
                final = json.loads((case / 'final.json').read_text())
                for pane in final['qt_panes']:
                    size = api('com.superlogical.terminal.size', session_id=sid, block_id=pane['pane'])
                    assert (size['columns'], size['rows']) == (pane['columns'], pane['rows']), (size, pane)
                assert client.wait(timeout=15) == 0, f'Qt output/echo failed: {case}'
            end = start+5
            records = [json.loads(line) for line in (case / 'traffic.jsonl').read_text().splitlines()]
            states = [r for r in records if r['type'] == 'state' and start <= r['t'] <= end]
            intervals = [1000*(b['t']-a['t']) for a,b in zip(states,states[1:])]
            final = json.loads((case / 'final.json').read_text())
            try:
                os.killpg(client.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            assert len(final['qt_panes']) == count
            for pane in final['qt_panes']:
                assert 'DONE' in pane['text'], 'background output missing'
            received = [p.read_bytes() for p in case.glob('bytes.*')]
            assert sorted(received) == sorted([b'zabc'] + [b'z'] * (count - 1)), received
            report = json.loads((case / 'report.json').read_text())
            results.append({'load_average_start': load_start, 'load_average_end': os.getloadavg(), 'panes': count, 'ready_ms': ready_ms, 'workload_to_done_ms': completion_ms, 'workload_seconds': 5, 'target_updates_per_second_per_pane': 100, 'snapshot_count': len(states), 'snapshot_bytes': sum(r['bytes'] for r in states), 'max_snapshot_interval_ms': max(intervals, default=0), 'median_snapshot_interval_ms': statistics.median(intervals) if intervals else None, 'echo_measurements': [r for r in report['evidence'] if 'elapsed_ms' in r]})
        (work / 'results.json').write_text(json.dumps(results, indent=2))
        print(json.dumps({'artifacts': str(work), 'results': results}))
    finally:
        if client:
            try:
                os.killpg(client.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass
            client.wait(timeout=10)
        server.terminate(); server.wait(timeout=10)
        log.close()


if __name__ == '__main__':
    main()
