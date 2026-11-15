#!/usr/bin/env python3
"""Path/upload evidence on an owned Rex, private HOME and offscreen Qt clipboard.

--baseline checks the immutable backend's actual version-3 unsupported operation.
--driver exercises native Commands, path field/list clicks and file MIME events.
--downloads exercises the native safe-download workflow, a fake folder opener,
private throttled streams and low-space confirmation in an unprivileged mount
namespace (requires unshare and mount). All destinations belong to the fixture.
Never connects to an existing Rex or display. Artifacts remain under .build-tools.
"""
import argparse
import asyncio
import json
import os
from pathlib import Path
import queue
import shlex
import socket
import struct
import select
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[1]


def revision_proxy(path, upstream):
    """Negotiate API 10 on owned control sockets; other bytes pass unchanged."""
    listener = socket.socket(socket.AF_UNIX)
    listener.bind(str(path)); listener.listen()
    def exact(sock, count):
        data = b''
        while len(data) < count:
            part = sock.recv(count - len(data))
            if not part: raise EOFError()
            data += part
        return data
    def header(sock):
        data = b''
        while not data.endswith(b'\r\n\r\n'): data += exact(sock, 1)
        return data
    def forward(client):
        server = socket.socket(socket.AF_UNIX)
        try:
            server.connect(str(upstream))
            server.sendall(header(client)); client.sendall(header(server))
            head = exact(client, 2); length = head[1] & 127
            extra = b''
            if length == 126: extra = exact(client, 2); length = struct.unpack('!H', extra)[0]
            elif length == 127: extra = exact(client, 8); length = struct.unpack('!Q', extra)[0]
            mask = exact(client, 4) if head[1] & 128 else b''
            body = exact(client, length)
            plain = bytes(v ^ mask[i % 4] for i, v in enumerate(body)) if mask else body
            try: message = json.loads(plain)
            except (ValueError, UnicodeError): message = {}
            if message.get('type') == 'hello':
                message['api_revision'] = 10
                message['api_min_revision'] = min(message.get('api_min_revision', 10), 10)
                plain = json.dumps(message).encode()
                body = bytes(v ^ mask[i % 4] for i, v in enumerate(plain)) if mask else plain
                length = len(body)
                extra = b'' if length < 126 else struct.pack('!H', length) if length < 65536 else struct.pack('!Q', length)
                head = bytes([head[0], (128 if mask else 0) | (length if length < 126 else 126 if length < 65536 else 127)])
            server.sendall(head + extra + mask + body)
            while True:
                ready, _, _ = select.select([client, server], [], [], 1)
                for source in ready:
                    data = source.recv(65536)
                    if not data: return
                    (server if source is client else client).sendall(data)
        except (OSError, EOFError): pass
        finally: client.close(); server.close()
    def accept():
        while True:
            try: client, _ = listener.accept()
            except OSError: return
            threading.Thread(target=forward, args=(client,), daemon=True).start()
    threading.Thread(target=accept, daemon=True).start()
    return listener


class SlowProxy:
    """Throttle only an owned server's large writes; closing cuts owned streams."""
    def __init__(self, path, upstream):
        self.loop = asyncio.new_event_loop()
        self.ready = threading.Event()
        self.path, self.upstream = path, upstream
        self.thread = threading.Thread(target=self.start, daemon=True)
        self.thread.start()
        assert self.ready.wait(10)
    def start(self):
        asyncio.set_event_loop(self.loop)
        self.server = self.loop.run_until_complete(asyncio.start_unix_server(self.handle, str(self.path)))
        self.ready.set()
        self.loop.run_forever()
    async def handle(self, reader, writer):
        upstream, output = await asyncio.open_unix_connection(str(self.upstream))
        async def pump(source, destination, slow):
            try:
                while data := await source.read(32768):
                    destination.write(data)
                    await destination.drain()
                    if slow and len(data) > 4096: await asyncio.sleep(.5)
            except (ConnectionError, asyncio.CancelledError): pass
            finally: destination.close()
        await asyncio.gather(pump(reader, output, False), pump(upstream, writer, True))
    def close(self):
        async def finish():
            self.server.close()
            tasks=[t for t in asyncio.all_tasks(self.loop) if t is not asyncio.current_task()]
            for task in tasks: task.cancel()
            await asyncio.gather(*tasks, return_exceptions=True)
            self.loop.stop()
        asyncio.run_coroutine_threadsafe(finish(), self.loop)
        self.thread.join(10)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--backend', type=Path, required=True)
    parser.add_argument('--driver', type=Path)
    parser.add_argument('--baseline', action='store_true')
    parser.add_argument('--downloads', action='store_true')
    args = parser.parse_args()
    assert args.baseline or args.driver, 'native driver required for behavior proof'
    work = Path(tempfile.mkdtemp(prefix='qt-files-', dir=ROOT / '.build-tools'))
    env = {k: v for k, v in os.environ.items() if not k.startswith(('REX_', 'HERDR_', 'HYPR'))
           and k not in ('DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'DBUS_SESSION_BUS_ADDRESS')}
    for name in ('home', 'config', 'state', 'cache', 'runtime', 'tmp'):
        (work / name).mkdir(mode=0o700)
    endpoint = 'unix://' + str(work / 'rex.sock')
    env.update(DBUS_SESSION_BUS_ADDRESS='unix:path='+str(work/'absent-bus'),HOME=str(work / 'home'), XDG_CONFIG_HOME=str(work / 'config'),
               XDG_STATE_HOME=str(work / 'state'), XDG_CACHE_HOME=str(work / 'cache'),
               XDG_RUNTIME_DIR=str(work / 'runtime'), TMPDIR=str(work / 'tmp'),
               REX_SERVER=endpoint, SHELL='/bin/sh', QT_QPA_PLATFORM='offscreen',
               QT_QUICK_BACKEND='software', QT_QPA_PLATFORMTHEME='generic')
    def api(method, **payload):
        return json.loads(subprocess.check_output(['rex', '--autostart=false', '--server', endpoint,
                         'api', 'call', method, json.dumps(payload)], env=env, text=True, timeout=20))
    with (work / 'server.log').open('w') as log:
        server = subprocess.Popen(['rex', 'server', 'run', '--listen', endpoint, '--restore=false',
                                   '--config', '/dev/null', '--state-dir', str(work / 'server')],
                                  env=env, stdout=log, stderr=log)
        try:
            for _ in range(200):
                if (work / 'rex.sock').exists(): break
                time.sleep(.05)
            sid = api('session.create', label='Qt files private', initial_windows=[{'layout': {'block': {'flavor': 'com.superlogical.terminal.shell'}}}])['session_id']
            if args.baseline:
                with (work / 'backend.log').open('w') as backend_log:
                    child = subprocess.Popen([str(args.backend.resolve()), '--endpoint', endpoint, '--session', sid],
                                             env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                             stderr=backend_log, text=True)
                    try:
                        records = queue.Queue()
                        def read():
                            for line in child.stdout: records.put(json.loads(line))
                        threading.Thread(target=read, daemon=True).start()
                        transcript = []
                        deadline = time.monotonic() + 15
                        while time.monotonic() < deadline:
                            record = records.get(timeout=10); transcript.append(record)
                            if record.get('type') == 'state' and record.get('pane'):
                                request = {k: record.get(k) for k in ('host', 'epoch', 'session', 'window', 'pane')}
                                request.update(version=3, op='directory.open')
                                child.stdin.write(json.dumps(request) + '\n'); child.stdin.flush()
                                break
                        while time.monotonic() < deadline:
                            record = records.get(timeout=10); transcript.append(record)
                            if record.get('type') == 'error':
                                assert 'unknown' in record.get('message', '').lower(), record
                                break
                        else: raise AssertionError('baseline did not reject operation')
                        (work / 'baseline.json').write_text(json.dumps(transcript, indent=2))
                    finally:
                        child.terminate(); child.wait(timeout=10)
            elif args.downloads:
                destination = work / 'saved'
                destination.mkdir()
                source = work / "-$(touch NEVER_EXECUTE) \u202eunsafe ' file.bin"
                content = bytes(range(256)) * 191
                source.write_bytes(content)
                large = work / 'large.bin'
                with large.open('wb') as sparse: sparse.truncate(1 << 30)
                def command():
                    return [{'op':'key','key':'P','mods':'ctrl+shift'},
                            {'op':'waitControl','control':'commandsPopup','property':'opened','value':True},
                            {'op':'field','control':'commandSearch','text':'Download file'}, {'op':'controlKey','key':'Return'},
                            {'op':'waitControl','control':'downloadPath','property':'visible','value':True}]
                def download(path):
                    return command() + [{'op':'field','control':'downloadPath','text':str(path)},
                         {'op':'field','control':'downloadFolder','text':str(destination)},
                         {'op':'click','control':'downloadStart'}]
                steps = [{'op':'waitState','field':'terminal.connected','value':True}]
                for index in range(2):
                    transfer_steps=download(source)
                    if index==0: transfer_steps[-1]={'op':'controlKey','key':'Return'}
                    steps += transfer_steps + [{'op':'waitState','field':'downloads.0.running','value':False},
                                               {'op':'waitState','field':'files.download','value':None}]
                steps += [{'op':'click','control':'downloadsButton'},
                          {'op':'waitControl','control':'downloadRowAction0','property':'visible','value':True},
                          {'op':'click','control':'downloadRowAction0'},
                          {'op':'waitOpenedUrl','text':destination.as_uri()},
                          {'op':'screenshot','path':str(work/'downloads.png')},
                          {'op':'controlKey','key':'Escape'}]
                steps += [{'op':'resize','width':640,'height':480}] + download(large) + [{'op':'waitState','field':'downloads.0.confirmation','value':True},
                          {'op':'waitControl','control':'downloadCancel','property':'activeFocus','value':True},
                          {'op':'screenshot','path':str(work/'confirmation.png')},
                          {'op':'controlKey','key':'Return'},
                          {'op':'waitState','field':'downloads.0.running','value':False},
                          {'op':'snapshot','path':str(work/'downloads.json')}]
                alternate=work/'configured-downloads'
                alternate.mkdir()
                steps += [{'op':'click','control':'settingsButton'},
                          {'op':'waitControl','control':'settingsSearch','property':'visible','value':True},
                          {'op':'field','control':'settingsSearch','text':'download folder'},
                          {'op':'field','control':'downloadFolderSetting','text':str(alternate)},
                          {'op':'click','control':'applyPreferences'},
                          {'op':'waitControl','control':'settingsPopup','property':'visible','value':False},
                          {'op':'snapshot','path':str(work/'destination-setting.json')}]
                def run(steps, name, target=endpoint, prefix=None):
                    (work/(name+'-events.json')).write_text(json.dumps(steps))
                    with (work/(name+'.log')).open('w') as log:
                        result = subprocess.run((prefix or []) + [str(args.driver.resolve()),'--backend',str(args.backend.resolve()),
                            '--endpoint',target,'--rex-session',sid,'--script',str(work/(name+'-events.json')),
                            '--report',str(work/(name+'-report.json'))],env=env,stdout=log,stderr=log,timeout=90)
                    assert result.returncode == 0, f'{name} native events failed: {work}'
                run(steps, 'native-downloads')
                assert json.loads((work/'config/rex-qt/workspace.json').read_text())['downloads']['directory']==str(alternate)
                saved=list(destination.iterdir())
                assert len(saved)==2 and all(p.read_bytes()==content for p in saved), saved
                assert saved[0].name != saved[1].name
                assert all('\u202e' not in p.name for p in saved), 'hostile formatting survived name sanitization'
                assert not (work/'NEVER_EXECUTE').exists()
                proxy_path=work/'api10.sock'
                proxy=revision_proxy(proxy_path,work/'rex.sock')
                try:
                    run([{'op':'waitState','field':'terminal.connected','value':True}] + command() +
                        [{'op':'waitState','field':'files.download.unsupported','value':True},
                         {'op':'waitControl','control':'downloadStart','property':'enabled','value':False}],
                        'download-api10', 'unix://'+str(proxy_path))
                finally: proxy.close()
                assert len(list(destination.iterdir()))==2
                profile_path=work/'config/rex-qt/workspace.json'
                profile=json.loads(profile_path.read_text())
                for host in profile['hosts']: host['remote_control']=True
                profile_path.write_text(json.dumps(profile))
                remote_result=[]
                def remote_attempt():
                    for _ in range(400):
                        for client in api('client.list')['clients']:
                            if client.get('info',{}).get('kind')!='app': continue
                            cid=client.get('id',client.get('client_id'))
                            result=subprocess.run(['rex','--autostart=false','--server',endpoint,'api','call','client.perform',
                                json.dumps({'client_id':cid,'action':'pane.download','args':{},'timeout_ms':1000})],
                                env=env,text=True,capture_output=True,timeout=10)
                            remote_result.append({'code':result.returncode,'stdout':result.stdout,'stderr':result.stderr})
                            return
                        time.sleep(.05)
                remote=threading.Thread(target=remote_attempt,daemon=True); remote.start()
                run([{'op':'waitState','field':'terminal.connected','value':True},{'op':'pause','ms':1500},
                     {'op':'waitState','field':'files.download','value':None}], 'download-remote-denial')
                remote.join(10)
                assert remote_result and remote_result[0]['code']!=0, remote_result
                (work/'remote-denial.json').write_text(json.dumps(remote_result,indent=2))
                low=work/'low-space'
                low.mkdir()
                lowsteps=[{'op':'waitState','field':'terminal.connected','value':True}] + command() + [
                    {'op':'field','control':'downloadPath','text':str(source)},
                    {'op':'field','control':'downloadFolder','text':str(low)},
                    {'op':'click','control':'downloadStart'},
                    {'op':'waitState','field':'downloads.0.confirmation','value':True},
                    {'op':'waitControl','control':'downloadCancel','property':'activeFocus','value':True},
                    {'op':'snapshot','path':str(work/'low-space.json')},
                    {'op':'controlKey','key':'Return'},
                    {'op':'waitState','field':'downloads.0.running','value':False}]
                run(lowsteps,'download-low-space',prefix=['unshare','-Urnm','--','sh','-c',
                    'set -e; mount -t tmpfs -o size=64m tmpfs "$1"; shift; exec "$@"','sh',str(low)])
                lowstate=json.loads((work/'low-space.json').read_text())
                assert 'Low space' in json.dumps(lowstate)
                slowfile=work/'slow.bin'
                with slowfile.open('wb') as sparse: sparse.truncate(64 << 20)
                for mode in ('cancel', 'confirm', 'exit', 'disconnect'):
                    proxy_path=work/(mode+'.sock')
                    proxy=SlowProxy(proxy_path,work/'rex.sock')
                    marker=work/(mode+'-receiving.json')
                    sequence=[{'op':'waitState','field':'terminal.connected','value':True}] + download(large if mode=='confirm' else slowfile)
                    if mode=='confirm': sequence += [
                        {'op':'waitState','field':'downloads.0.confirmation','value':True},
                        {'op':'controlKey','key':'Tab'},
                        {'op':'waitControl','control':'downloadStart','property':'activeFocus','value':True},
                        {'op':'controlKey','key':'Return'}]
                    sequence += [
                        {'op':'waitState','field':'files.download','value':None},
                        {'op':'click','control':'downloadsButton'},
                        {'op':'waitControl','control':'downloadRowAction0','property':'visible','value':True},
                        {'op':'snapshot','path':str(marker)}]
                    if mode in ('cancel','confirm'): sequence += [{'op':'click','control':'downloadRowAction0'}, {'op':'waitState','field':'downloads.0.running','value':False}]
                    if mode=='disconnect':
                        def cut():
                            for _ in range(600):
                                if marker.exists(): proxy.close(); return
                                time.sleep(.05)
                        cutter=threading.Thread(target=cut,daemon=True); cutter.start()
                        sequence += [{'op':'waitState','field':'downloads.0.running','value':False}]
                    try: run(sequence, 'download-'+mode, 'unix://'+str(proxy_path))
                    finally:
                        if proxy.thread.is_alive(): proxy.close()
                    receiving=json.loads(marker.read_text())['downloads'][0]
                    assert receiving['running'] and 0 < receiving.get('progress',0) < 1, receiving
                    for _ in range(100):
                        if len(list(destination.iterdir()))==2: break
                        time.sleep(.05)
                    assert len(list(destination.iterdir()))==2, (mode,list(destination.iterdir()))
                (work/'result.json').write_text(json.dumps({'native_download_exact_bytes':'PASS','no_overwrite':'PASS',
                    'confirmation_cancel_default':'PASS','fake_folder_opener':'PASS','api13_refusal':'PASS','cancel_disconnect_exit_partial_cleanup':'PASS','low_space_cancel_default':'PASS','remote_perform_denied':'PASS'}))
            else:
                folder = work / "Alice's $(touch NEVER_EXECUTE)"
                folder.mkdir()
                upload = work / "uploaded bytes ' safe.bin"
                upload.write_bytes(bytes(range(256)) * 19)
                capture = work / 'input.bin'
                raw = work / 'read.py'
                raw.write_text("import os,select,time,tty\ntty.setraw(0)\nos.write(1,b'\\x1b[2J\\x1b[HFILES_READY')\ndata=b''\nend=time.monotonic()+180\nwhile time.monotonic()<end:\n if select.select([0],[],[],.1)[0]:\n  data+=os.read(0,65536)\n  open(" + repr(str(capture)) + ",'wb').write(data)\n")
                def command(title):
                    return [{'op':'key','key':'P','mods':'ctrl+shift'},
                            {'op':'waitControl','control':'commandsPopup','property':'opened','value':True},
                            {'op':'field','control':'commandSearch','text':title}, {'op':'controlKey','key':'Return'}]
                steps = [{'op':'waitState','field':'terminal.connected','value':True},
                         {'op':'paste','text':'python3 '+shlex.quote(str(raw))}, {'op':'key','key':'Return'},
                         {'op':'waitText','text':'FILES_READY'}]
                steps += command('Insert path')
                steps += [{'op':'waitControl','control':'directoryPath','property':'visible','value':True},
                          {'op':'field','control':'directoryPath','text':str(folder)+'/'},
                          {'op':'controlKey','key':'Escape'},
                          {'op':'waitState','field':'files.directory','value':None}]
                steps += command('Go to directory')
                steps += [{'op':'waitControl','control':'directoryPath','property':'visible','value':True},
                          {'op':'field','control':'directoryPath','text':str(folder)+'/'},
                          {'op':'waitState','field':'files.directory.text','value':str(folder)+'/'},
                          {'op':'waitState','field':'files.directory.pending','value':False},
                          {'op':'screenshot','path':str(work/'directory.png')},
                          {'op':'click','control':'directoryChoice0'},
                          {'op':'pause','ms':300}, {'op':'snapshot','path':str(work/'path.json')},
                          ]
                steps += command('Insert path')
                steps += [{'op':'waitState','field':'files.directory.rows.0.recent','value':True},
                          {'op':'snapshot','path':str(work/'recent.json')},
                          {'op':'controlKey','key':'Escape'},
                          {'op':'waitState','field':'files.directory','value':None},
                          {'op':'setMimeClipboard','paths':[str(upload)]}, {'op':'key','key':'V','mods':'ctrl+shift'},
                          {'op':'pause','ms':1500}, {'op':'fileDrop','paths':[str(upload)]},
                          {'op':'pause','ms':1500}, {'op':'snapshot','path':str(work/'upload.json')}]
                (work/'events.json').write_text(json.dumps(steps))
                with (work/'ui.log').open('w') as ui_log:
                    result = subprocess.run([str(args.driver.resolve()),'--backend',str(args.backend.resolve()),
                               '--endpoint',endpoint,'--rex-session',sid,'--script',str(work/'events.json'),
                               '--report',str(work/'report.json')],env=env,stdout=ui_log,stderr=ui_log,timeout=180)
                assert result.returncode == 0, f'native files events failed: {work}'
                profile=json.loads((work/'config/rex-qt/workspace.json').read_text())
                history=profile.get('recent_paths',{}).get(endpoint,[])
                assert history and history[0]=={'path':str(folder),'directory':True}, 'authoritative profile lost path history'
                assert len(history)<=10
                data=capture.read_bytes()
                expected=("cd '" + str(folder).replace("'", "'\\''") + "'").encode()
                assert data.startswith(expected), (data,expected)
                assert b'\r' not in data and b'\n' not in data, 'file operation sent Enter'
                # The server owns escaping and upload destination; decode its returned shell words.
                returned=shlex.split(data[len(expected):].decode())
                assert len(returned)==2, ('paths must be inserted once per explicit MIME event',returned)
                for path in returned: assert Path(path).read_bytes()==upload.read_bytes(), 'upload bytes changed'
                assert not (work/'NEVER_EXECUTE').exists()
                proxy_path = work / 'api10.sock'
                proxy = revision_proxy(proxy_path, work / 'rex.sock')
                try:
                    refused = [{'op':'waitState','field':'terminal.connected','value':True},
                               {'op':'setMimeClipboard','paths':[str(upload)]},
                               {'op':'key','key':'V','mods':'ctrl+shift'},
                               {'op':'waitProblem','text':'API 11 required'},
                               {'op':'snapshot','path':str(work/'api10.json')}]
                    (work/'api10-events.json').write_text(json.dumps(refused))
                    with (work/'api10.log').open('w') as ui_log:
                        result = subprocess.run([str(args.driver.resolve()),'--backend',str(args.backend.resolve()),
                                   '--endpoint','unix://'+str(proxy_path),'--rex-session',sid,
                                   '--script',str(work/'api10-events.json'),'--report',str(work/'api10-report.json')],
                                   env=env,stdout=ui_log,stderr=ui_log,timeout=35)
                    assert result.returncode == 0, f'API 10 native refusal failed: {work}'
                    assert capture.read_bytes() == data, 'unsupported upload inserted paths'
                finally: proxy.close()
                (work/'result.json').write_text(json.dumps({'native_path_paste':'PASS','native_mime_upload':'PASS','api10_refusal':'PASS'}))
            print(work)
        finally:
            server.terminate(); server.wait(timeout=10)


if __name__ == '__main__':
    main()
