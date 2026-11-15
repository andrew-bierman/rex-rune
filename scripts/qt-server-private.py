#!/usr/bin/env python3
"""Verify Qt server controls on an owned Rex and fake SSH helpers, or decode pairing.

Every controlled server, socket, profile, helper and UI capture is created by this
fixture. It never uses the default server or contacts a real SSH host. Pairing
checks use zbarimg to decode both the matrix and the actual Qt Canvas capture.
"""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile


def decode(pair, folder):
    import types
    args = types.SimpleNamespace(pair_json=Path(folder) / "pair.json")
    args.pair_json.write_text(json.dumps(pair))
    decode_file(args)


def decode_file(args):
    decoder = shutil.which('zbarimg')
    if not decoder:
        raise SystemExit('zbarimg is required for independent QR verification')
    pair = json.loads(args.pair_json.read_text())
    matrix = pair['matrix']
    size = len(matrix)
    assert size and all(len(row) == size for row in matrix)
    scale, quiet = 8, 4
    width = (size + quiet * 2) * scale
    pixels = bytearray([255]) * (width * width)
    for y, row in enumerate(matrix):
        for x, dark in enumerate(row):
            if dark:
                for dy in range(scale):
                    start = ((y + quiet) * scale + dy) * width + (x + quiet) * scale
                    pixels[start:start + scale] = bytes(scale)
    with tempfile.TemporaryDirectory(prefix='qt-pair-', dir=args.pair_json.parent) as folder:
        image = Path(folder) / 'pair.pgm'
        image.write_bytes(f'P5\n{width} {width}\n255\n'.encode() + pixels)
        result = subprocess.run([decoder, '--quiet', '--raw', str(image)], capture_output=True, check=True)
        assert result.stdout.rstrip(b'\r\n') == pair['address'].encode(), 'QR decoded a different value'
    print('Private pairing QR decoded independently')


def exercise(args):
    import os
    import queue
    import shlex
    import threading
    import time
    root = Path(__file__).resolve().parents[1]
    work = Path(tempfile.mkdtemp(prefix='qt-server-private-', dir=root / '.build-tools'))
    env = {k: v for k, v in os.environ.items() if not k.startswith(('REX_', 'HERDR_')) and k not in ('DISPLAY', 'WAYLAND_DISPLAY', 'DBUS_SESSION_BUS_ADDRESS')}
    for name in ('home', 'config', 'state', 'cache', 'runtime', 'tools'):
        (work / name).mkdir(mode=0o700)
    env.update(HOME=str(work/'home'), XDG_CONFIG_HOME=str(work/'config'), XDG_STATE_HOME=str(work/'state'), XDG_CACHE_HOME=str(work/'cache'), XDG_RUNTIME_DIR=str(work/'runtime'), QT_QPA_PLATFORM='offscreen', QT_QUICK_BACKEND='software', QT_QPA_PLATFORMTHEME='generic', SHELL='/bin/sh')
    real_rex = shutil.which('rex')
    endpoint = 'unix://' + str(work / 'server.sock')
    env['REX_SERVER'] = endpoint
    log = (work/'server.log').open('w')
    server = subprocess.Popen([real_rex, 'server', 'run', '--listen', endpoint, '--restore=false', '--state-dir',str(work/'server-state')],env=env,stdout=log,stderr=log)
    children = []
    traces = []
    def wait_socket():
        until = time.monotonic()+15
        while not (work/'server.sock').exists():
            assert server.poll() is None
            assert time.monotonic()<until
            time.sleep(.03)
    def connect(target):
        profile = work / ('profile-'+str(len(children))+'.json')
        profile.write_text(json.dumps({'hosts':[{'label':'Private server fixture','endpoint':target}],'last_host':target}))
        p = subprocess.Popen([str(args.backend.resolve()),'--config',str(profile)],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=(work/('child-'+str(len(children))+'.log')).open('w'),text=True)
        q = queue.Queue()
        def read():
            for line in p.stdout:
                value=json.loads(line);q.put(value)
        threading.Thread(target=read,daemon=True).start()
        children.append(p)
        state={}
        def receive(test):
            nonlocal state
            if state and test(state): return state
            until=time.monotonic()+20
            while time.monotonic()<until:
                try:v=q.get(timeout=.2)
                except queue.Empty:continue
                if v.get('type')=='state':state=v
                traces.append(v)
                if test(v):return v
            raise AssertionError('private backend response timeout')
        receive(lambda v:v.get('type')=='state')
        def send(op, **fields):
            p.stdin.write(json.dumps(dict(version=3,op=op,host=state['host'],epoch=state['epoch'],**fields))+'\n');p.stdin.flush()
        return profile,p,receive,send,lambda:state
    def native(profile, steps, name):
        if not args.driver:return
        path=work/(name+'-steps.json'); path.write_text(json.dumps(steps))
        with (work/(name+'.log')).open('w') as output:
            subprocess.run([str(args.driver.resolve()),'--backend',str(args.backend.resolve()),'--config',str(profile),'--script',str(path),'--report',str(work/(name+'-report.json'))],env=env,stdout=output,stderr=output,check=True,timeout=75)
    def controls():
        return [{'op':'resize','width':640,'height':480},{'op':'click','control':'settingsButton'},{'op':'waitControl','control':'settingsPopup','property':'opened','value':True},{'op':'click','control':'serverSettingsButton'},{'op':'waitControl','control':'serverPopup','property':'opened','value':True}]
    try:
        wait_socket()
        # Fixture creation is explicit; only this server/socket is ever controlled.
        subprocess.check_output([real_rex,'--autostart=false','--server',endpoint,'api','call','session.create',json.dumps({'label':'Qt private server controls','initial_windows':[{'layout':{'block':{'flavor':'com.superlogical.terminal.shell'}}}]})],env=env)
        status_fixture=subprocess.check_output([real_rex,"--autostart=false","--server",endpoint,"api","call","server.status","{}"],env=env,text=True).strip()
        profile,p,receive,send,state=connect(endpoint)
        receive(lambda v:v.get('server',{}).get('can_stop') is True)
        p.stdin.write(json.dumps(dict(version=3,op='server_prepare_stop',host=state()['host'],epoch=state()['epoch']+1,request_id=99))+'\n');p.stdin.flush()
        receive(lambda v:v.get('type')=='error' and 'connection changed' in v.get('message',''))
        assert server.poll() is None
        send('server_prepare_stop',request_id=11)
        receive(lambda v:(v.get('server',{}).get('confirmation') or {}).get('request_id')==11)
        send('server_cancel')
        receive(lambda v:v.get('server',{}).get('confirmation','missing') is None)
        assert server.poll() is None
        send('server_confirm',request_id=11)
        receive(lambda v:v.get('type')=='error')
        assert server.poll() is None
        native(profile,[{'op':'waitState','field':'server.can_stop','value':True}]+controls()+[{'op':'click','control':'stopServerButton'},{'op':'waitControl','control':'keepServerButton','property':'visible','value':True},{'op':'controlKey','key':'Return'},{'op':'waitState','field':'server.confirmation','value':None},{'op':'waitControl','control':'keepServerButton','property':'visible','value':False},{'op':'screenshot','path':str(work/'server-640.png')}], 'native-stop-cancel')
        assert server.poll() is None
        if args.driver:
            native(profile,[{'op':'waitState','field':'server.can_stop','value':True}]+controls()+[{'op':'click','control':'stopServerButton'},{'op':'waitControl','control':'confirmServerButton','property':'visible','value':True},{'op':'click','control':'confirmServerButton'},{'op':'waitState','field':'connected','value':False},{'op':'serverConfirm','request_id':1},{'op':'waitProblem','text':'confirmation'},{'op':'screenshot','path':str(work/'server-disconnected-640.png')}], 'native-stop-confirm')
        else:
            send('server_prepare_stop',request_id=12)
            receive(lambda v:(v.get('server',{}).get('confirmation') or {}).get('request_id')==12)
            send('server_confirm',request_id=12)
        server.wait(timeout=15)
        send('server_confirm',request_id=12)
        receive(lambda v:v.get('type')=='error')
        # Fake command edge: no SSH network or personal service is contacted.
        tools=work/'tools'
        (tools/'ssh').write_text("#!/bin/sh\ncase \"$*\" in *\"rex version --json\"*) echo '{\"api_revision\":12}';; *) echo 'none fixture';; esac\n")
        (tools/'rex').write_text("#!/bin/sh\nif [ \"$1\" = version ]; then echo '{\"api_revision\":12}'; exit; fi\nprintf 'attempt\\n' >> "+shlex.quote(str(work/'attempts'))+"\nprintf '%s\\n' "+shlex.quote(status_fixture)+"\n")
        for helper in tools.iterdir():helper.chmod(0o700)
        env['PATH']=str(tools)+os.pathsep+env['PATH']
        profile,p,receive,send,state=connect('ssh://qt-owned-fake')
        receive(lambda v:v.get('server',{}).get('can_setup') is True)
        send('server_prepare_setup',request_id=20)
        preview=receive(lambda v:(v.get('server',{}).get('confirmation') or {}).get('request_id')==20)
        assert '--server ssh://qt-owned-fake' in preview['server']['confirmation']['command']
        send('server_cancel');receive(lambda v:v.get('server',{}).get('confirmation','missing') is None)
        assert not (work/'attempts').exists()
        native(profile,[{'op':'waitState','field':'server.can_setup','value':True}]+controls()+[{'op':'click','control':'setupServerButton'},{'op':'click','control':'keepServerButton'},{'op':'pause','ms':150}], 'native-setup-cancel')
        assert not (work/'attempts').exists()
        if args.driver:
            native(profile,[{'op':'waitState','field':'server.can_setup','value':True}]+controls()+[{'op':'click','control':'setupServerButton'},{'op':'waitControl','control':'confirmServerButton','property':'visible','value':True},{'op':'click','control':'confirmServerButton'},{'op':'waitState','field':'server.setup_result.message','value':'Personal server answered. Use Connect to attach without another start.'},{'op':'serverConfirm','request_id':1},{'op':'waitProblem','text':'confirmation'}], 'native-setup-confirm')
        else:
            send('server_prepare_setup',request_id=21)
            receive(lambda v:(v.get('server',{}).get('confirmation') or {}).get('request_id')==21)
            send('server_confirm',request_id=21)
            receive(lambda v:bool(v.get('server',{}).get('setup_result')))
            send('server_confirm',request_id=21);receive(lambda v:v.get('type')=='error')
        time.sleep(.3)
        assert (work/'attempts').read_text().splitlines()==['attempt']
        (work/'trace.json').write_text(json.dumps(traces,indent=2))
        (work/'result.json').write_text(json.dumps({'cancel_no_stop':True,'confirmed_stop_owned_fixture':True,'setup_cancel_no_attempt':True,'setup_confirm_single_attempt':True,'duplicate_confirmation_rejected':True,'native_events':bool(args.driver)}))
        print('Private server controls passed; artifacts: '+str(work.relative_to(root)))
    finally:
        for p in children:
            if p.poll() is None:
                p.stdin.close()
                try:p.wait(timeout=10)
                except subprocess.TimeoutExpired:p.kill();p.wait()
        if server.poll() is None:server.terminate();server.wait(timeout=10)
        log.close()


def native_pair(args):
    import os
    work=Path(tempfile.mkdtemp(prefix='qt-pair-native-',dir=args.pair_json.resolve().parent))
    pair=json.loads(args.pair_json.read_text())
    fixture=work/'child.py'
    fixture.write_text("""#!/usr/bin/env python3
import json,sys
pair=PAIR
state={'version':3,'type':'state','host':'owned-qr-fixture','epoch':1,'connected':False,'panes':[],'sessions':[],'windows':[],'keymap':{'actions':[{'id':'client.settings','title':'Settings','enabled':True,'kind':'client'}]},'server':{'status':'Private pairing fixture','can_pair':True,'can_stop':False,'pair':None,'confirmation':None}}
def emit(v):print(json.dumps(v),flush=True)
emit({'version':3,'type':'hello'});emit(state)
for line in sys.stdin:
 v=json.loads(line)
 with open(LOG,'a') as log:log.write(v['op']+'\\n')
 if v['op']=='shutdown':break
 if v['op']=='server_pair':state['server']['pair']=pair
 if v['op']=='server_pair_close':state['server']['pair']=None
 emit(state)
""".replace('PAIR',repr(pair)).replace('LOG',repr(str(work/'operations'))))
    fixture.chmod(0o700)
    steps=[{'op':'waitState','field':'server.can_pair','value':True},{'op':'resize','width':640,'height':480},{'op':'click','control':'settingsButton'},{'op':'waitControl','control':'settingsPopup','property':'opened','value':True},{'op':'click','control':'serverSettingsButton'},{'op':'click','control':'pairDeviceButton'},{'op':'waitControl','control':'pairQr','property':'visible','value':True},{'op':'pause','ms':200},{'op':'screenshot','path':str(work/'pair-640.png')},{'op':'controlKey','key':'Escape'},{'op':'waitState','field':'server.pair','value':None}]
    script=work/'steps.json';script.write_text(json.dumps(steps))
    env=dict(os.environ,QT_QPA_PLATFORM='offscreen',QT_QUICK_BACKEND='software',QT_QPA_PLATFORMTHEME='generic',HOME=str(work),XDG_CONFIG_HOME=str(work),XDG_RUNTIME_DIR=str(work))
    with (work/'native.log').open('w') as log:
        subprocess.run([str(args.driver.resolve()),'--backend',str(fixture),'--script',str(script),'--report',str(work/'report.json')],env=env,stdout=log,stderr=log,check=True,timeout=40)
    decoded=subprocess.run(['zbarimg','--quiet','--raw',str(work/'pair-640.png')],capture_output=True,check=True).stdout.rstrip(b'\r\n')
    assert decoded==pair['address'].encode()
    operations=(work/'operations').read_text().splitlines()
    assert operations.count('server_pair')==1 and operations.count('server_pair_close')>=1
    # A child failure must clear a displayed code even when no final state arrives.
    fixture.write_text(fixture.read_text().replace('import json,sys', 'import json,sys,threading,os').replace("if v['op']=='server_pair':state['server']['pair']=pair", "if v['op']=='server_pair':state['server']['pair']=pair;threading.Timer(.7,lambda:os._exit(0)).start()"))
    crash_steps=steps[:8]+[{'op':'waitState','field':'server.pair','value':None},{'op':'waitProblem','text':'backend stopped'}]
    crash_script=work/'crash-steps.json';crash_script.write_text(json.dumps(crash_steps))
    with (work/'crash-native.log').open('w') as log:
        subprocess.run([str(args.driver.resolve()),'--backend',str(fixture),'--script',str(crash_script),'--report',str(work/'crash-report.json')],env=env,stdout=log,stderr=log,check=True,timeout=40)
    print('Native pairing Canvas independently decoded; private artifacts retained')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--pair-json', type=Path)
    parser.add_argument('--backend', type=Path)
    parser.add_argument('--driver', type=Path)
    args = parser.parse_args()
    if args.pair_json:
        decode_file(args)
        if args.driver: native_pair(args)
    elif args.backend:
        exercise(args)
    else:
        parser.error('provide --pair-json or --backend')


if __name__ == '__main__':
    main()
