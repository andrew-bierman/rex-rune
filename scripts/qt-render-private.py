#!/usr/bin/env python3
"""Render actual PTY Kitty graphics, sprites and IME through public core and Qt.

Requires built Qt test driver, Rust backend and rex. All server/session, HOME,
config, clipboard and display state is private. This offscreen pixel proof does
not replace scripts/qt-native-layout-private.py or qt-wayland-private.py.
"""
import argparse
import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import time

ROOT=Path(__file__).resolve().parents[1]

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--driver',type=Path,required=True)
    parser.add_argument('--backend',type=Path,required=True)
    parser.add_argument('--scale',default='1',choices=['1','2'])
    args=parser.parse_args()
    work=Path(tempfile.mkdtemp(prefix='qt-render-',dir=ROOT/'.build-tools'))
    with tempfile.TemporaryDirectory(prefix='rex-render-') as runtime:
        env={k:v for k,v in os.environ.items() if not k.startswith(('REX_','HERDR_')) and k not in ('DISPLAY','WAYLAND_DISPLAY','WAYLAND_SOCKET','DBUS_SESSION_BUS_ADDRESS')}
        for name in ('home','config','state','cache'): (work/name).mkdir(mode=0o700)
        endpoint='unix://'+runtime+'/rex.sock'
        env.update(HOME=str(work/'home'),XDG_CONFIG_HOME=str(work/'config'),XDG_STATE_HOME=str(work/'state'),
            XDG_CACHE_HOME=str(work/'cache'),XDG_RUNTIME_DIR=runtime,SHELL='/bin/sh',PS1='private$ ',REX_SERVER=endpoint,
            QT_QPA_PLATFORM='offscreen',QT_QUICK_BACKEND='software',QT_QPA_PLATFORMTHEME='generic',QT_SCALE_FACTOR=args.scale)
        def api(method,**payload):
            return json.loads(subprocess.check_output(['rex','--autostart=false','--server',endpoint,'api','call',method,json.dumps(payload)],env=env,text=True,timeout=15))
        fixture=work/'fixture.py'
        fixture.write_text("import os,select,termios,time,tty\nold=termios.tcgetattr(0);data=b''\ntry:\n tty.setraw(0)\n"
            " os.write(1,b'\\x1b[2J\\x1b[H\\x1b_Ga=T,f=32,s=1,v=1,c=4,r=2,i=42,q=2;/wAA/w==\\x1b\\\\')\n"
            " os.write(1,'\\x1b[4;1H\\x1b[38;2;0;255;0m████\\x1b[0m\\x1b[6;1HRENDER_READY'.encode())\n"
            " deadline=time.monotonic()+30\n while len(data)<2 and time.monotonic()<deadline:\n"
            "  if select.select([0],[],[],.1)[0]: data+=os.read(0,1024)\n"
            "finally:\n termios.tcsetattr(0,termios.TCSANOW,old)\n"
            f"open({str(work/'ime.bin')!r},'wb').write(data)\nos.write(1,b'\\x1b_Ga=d,d=A,q=2\\x1b\\\\')\nprint('\\r\\nRENDER_DONE',flush=True)\n")
        steps=[{'op':'waitState','field':'terminal.connected','value':True},
            {'op':'paste','text':'python3 '+shlex.quote(str(fixture))},{'op':'key','key':'Return'},
            {'op':'waitText','text':'RENDER_READY'}, {'op':'pause','ms':350},
            {'op':'snapshot','path':str(work/'frame.json')}, {'op':'screenshot','path':str(work/'before.png')},
            {'op':'assertCellPixel','x':.5,'y':.5,'color':'#ff0000'},
            {'op':'assertCellPixel','x':3.5,'y':1.5,'color':'#ff0000'},
            {'op':'assertCellPixel','x':1.99,'y':3.5,'color':'#00ff00'},
            {'op':'assertCellPixel','x':2.01,'y':3.5,'color':'#00ff00'},
            {'op':'key','key':'DeadAcute','action':'press','scan':48},
            {'op':'key','key':'DeadAcute','action':'release','scan':48},
            {'op':'ime','preedit':'unfinished'}, {'op':'screenshot','path':str(work/'preedit.png')},
            {'op':'ime','preedit':''}, {'op':'ime','text':'é'},
            {'op':'waitText','text':'RENDER_DONE'}, {'op':'assertCellPixel','x':.5,'y':.5,'color':'#000000'}, {'op':'screenshot','path':str(work/'render.png')}]
        (work/'events.json').write_text(json.dumps(steps))
        with (work/'server.log').open('w') as log:
            server=subprocess.Popen(['rex','server','run','--listen',endpoint,'--restore=false','--config','/dev/null','--state-dir',str(work/'server')],env=env,stdout=log,stderr=log)
            try:
                deadline=time.monotonic()+15
                while not Path(runtime+'/rex.sock').exists():
                    assert server.poll() is None and time.monotonic()<deadline,f'private server failed: {work}'
                    time.sleep(.05)
                sid=api('session.create',label='Private terminal rendering proof',initial_windows=[{'layout':{'block':{'flavor':'com.superlogical.terminal.shell'}}}])['session_id']
                with (work/'client.log').open('w') as client_log:
                    result=subprocess.run([str(args.driver.resolve()),'--backend',str(args.backend.resolve()),'--endpoint',endpoint,'--rex-session',sid,'--script',str(work/'events.json'),'--report',str(work/'report.json')],env=env,stdout=client_log,stderr=client_log,timeout=60)
                assert result.returncode==0,f'Qt render proof failed: {work}'
                snapshot=json.loads((work/'frame.json').read_text())
                assert snapshot['frame']['images'][0]['rgba']=='/wAA/w=='
                assert (work/'ime.bin').read_bytes()=='é'.encode(), 'preedit leaked, commit duplicated or wrong bytes'
                print(f'PASS actual PTY/core/Qt image, sprite and IME at {args.scale}x: {work}')
            finally:
                server.terminate()
                try:server.wait(timeout=4)
                except subprocess.TimeoutExpired:server.kill();server.wait()

if __name__=='__main__':main()
