#!/usr/bin/env python3
"""Live OSC -> Qt -> fake desktop and native searchable Settings on private fixtures.

Requires rex, dbus-daemon, the Qt test driver and child. Uses an owned server,
private bus, fake-only sound PATH, private HOME and offscreen software window.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import tempfile
import time


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--driver', type=Path, required=True)
    p.add_argument('--backend', type=Path, required=True)
    args = p.parse_args()
    root = Path(__file__).resolve().parents[1]
    work = Path(tempfile.mkdtemp(prefix='notification-window-', dir=root / '.build-tools'))
    rex = shutil.which('rex')
    assert rex
    env = {k: v for k, v in os.environ.items() if not k.startswith(('REX_', 'HERDR_', 'HYPR')) and k not in ('DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'DBUS_SESSION_BUS_ADDRESS')}
    for folder in ('home', 'config', 'runtime', 'cache', 'state', 'data', 'tmp', 'bin'):
        (work / folder).mkdir(mode=0o700)
    # Rex's socket too must fit sockaddr_un; abstract sockets are used only for
    # D-Bus. The owned server uses a short private directory in this fixture's TMP.
    short = Path(tempfile.mkdtemp(prefix='rex-qt-note-', dir='/tmp'))
    short.chmod(0o700)
    endpoint = 'unix://' + str(short / 'rex.sock')
    address = 'unix:abstract=rex-qt-' + work.name
    env.update(HOME=str(work / 'home'), XDG_CONFIG_HOME=str(work / 'config'), XDG_RUNTIME_DIR=str(work / 'runtime'),
               XDG_CACHE_HOME=str(work / 'cache'), XDG_STATE_HOME=str(work / 'state'), XDG_DATA_HOME=str(work / 'data'),
               XDG_DATA_DIRS=str(work / 'data'), TMPDIR=str(work / 'tmp'), DBUS_SESSION_BUS_ADDRESS=address,
               REX_SERVER=endpoint, QT_QPA_PLATFORM='offscreen', QT_QUICK_BACKEND='software', QT_QPA_PLATFORMTHEME='generic', SHELL='/bin/sh', PATH=str(work / 'bin'))
    (work / 'bus.conf').write_text('<busconfig><listen>' + address + '</listen><type>session</type><auth>EXTERNAL</auth><policy context="default"><allow user="' + str(os.getuid()) + '"/><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
    processes, streams = [], []

    def start(name, command):
        log = (work / (name + '.log')).open('w'); streams.append(log)
        child = subprocess.Popen(command, env=env, stdout=log, stderr=log)
        processes.append(child)
        return child

    def wait(check, why, timeout=20):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            if check(): return
            time.sleep(.04)
        raise AssertionError(why)

    def api(method, **fields):
        return json.loads(subprocess.check_output([rex, '--autostart=false', '--server', endpoint, 'api', 'call', method, json.dumps(fields)], env=env, text=True, timeout=15))

    def calls(method):
        path = work / 'calls.jsonl'
        return [r for line in path.read_text().splitlines() if (r := json.loads(line))['method'] == method] if path.exists() else []

    def control(method, ident=None):
        command = ['/usr/bin/gdbus', 'call', '--session', '--dest', 'org.freedesktop.Notifications', '--object-path', '/org/freedesktop/Notifications', '--method', 'dev.shuv.NotificationTest.' + method]
        if ident is not None: command.append(str(ident))
        subprocess.run(command, env=env, check=True, stdout=subprocess.DEVNULL, timeout=5)

    try:
        start('bus', ['/usr/bin/dbus-daemon', '--config-file=' + str(work / 'bus.conf'), '--nofork', '--nopidfile', '--print-address=1', '--address=' + address])
        wait(lambda: (work / 'bus.log').stat().st_size > 0, 'private bus not ready')
        start('service', ['/usr/bin/python3', str(root / 'scripts/qt-notifications-private.py'), '--service', str(work)])
        wait(lambda: (work / 'service-ready').exists(), 'fake service not ready')
        start('rex', [rex, 'server', 'run', '--listen', endpoint, '--restore=false', '--config', '/dev/null', '--state-dir', str(work / 'server')])
        wait(lambda: (short / 'rex.sock').exists(), 'private Rex not ready')
        sessions = [api('session.create', label=label, initial_windows=[{'label':label,'layout':{'block':{'flavor':'com.superlogical.terminal.shell'}}}])['session_id'] for label in ('Visible shell', 'Owned reporter')]
        report = work / 'report.py'
        report.write_text('import pathlib,time\np=pathlib.Path(__file__).with_suffix(".state")\nlast=None\nwhile True:\n s=p.read_text().strip()\n if s!=last:\n  print(("\\x1b]9;Owned program message\\x07" if s=="notice" else "\\x1b]7501;state="+s+":app=claude:msg=Owned live event\\x07"),end="",flush=True);last=s\n time.sleep(.03)\n')
        report.with_suffix('.state').write_text('working')
        target = api('session.view', session_id=sessions[1])['windows'][0]
        pane = target['focused_block_id']
        api('block.event', session_id=sessions[1], block_id=pane, event={'type':'paste','payload':{'text':'/usr/bin/python3 ' + shlex.quote(str(report))}})
        api('block.event', session_id=sessions[1], block_id=pane, event={'type':'key','payload':{'key':'enter','action':'press','mods':''}})
        wait(lambda: any(r['state']=='working' for r in api('com.superlogical.terminal.program_status',session_id=sessions[1],block_id=pane).get('records',[])), 'reporter not live')
        profile = work / 'workspace.json'
        profile.write_text(json.dumps({'hosts':[{'label':'Notification fixture','endpoint':endpoint}],'last_host':endpoint,'sessions':{endpoint:sessions[0]},'generated_titles':False}))
        steps = [
            {'op':'waitState','field':'connected','value':True},
            {'op':'pause','ms':2500},
            {'op':'snapshot','path':str(work/'ready.json')},
            {'op':'waitState','field':'session','value':sessions[1]},
            {'op':'snapshot','path':str(work/'navigated.json')},
            {'op':'resize','width':640,'height':480},
            {'op':'click','control':'settingsButton'},
            {'op':'field','control':'settingsSearch','text':'service'},
            {'op':'waitControl','control':'notificationHealth','property':'visible','value':True},
            {'op':'click','control':'notificationTest'},
            {'op':'waitControl','control':'notificationTestResult','property':'text','value':'Test failed: Service rejected the request'},
            {'op':'click','control':'notificationTest'},
            {'op':'waitControl','control':'notificationTestResult','property':'text','value':'Test notification sent'},
            {'op':'pause','ms':500},
            {'op':'screenshot','path':str(work/'notifications-minimum.png'),'notBlank':True,'width':640,'height':480},
            {'op':'snapshot','control':'settingsPopup','path':str(work/'settings.json')},
            {'op':'click','control':'cancelPreferences'}]
        script = work / 'driver.json'; script.write_text(json.dumps(steps))
        driver = start('qt', [str(args.driver.resolve()),'--backend',str(args.backend.resolve()),'--config',str(profile),'--script',str(script),'--report',str(work/'driver-report.json')])
        wait(lambda: (work/'ready.json').exists() or driver.poll() is not None, 'native window not ready')
        assert driver.poll() is None, (work/'qt.log').read_text()
        assert not calls('Notify'), 'read-back state notified'
        ready = json.loads((work/'ready.json').read_text())
        assert ready['preferences']['notifications']['play_sounds'] is False
        assert any(r['session']==sessions[1] for r in ready['activity']['rows'])
        report.with_suffix('.state').write_text('blocked')
        wait(lambda: len(calls('Notify'))==1, 'live blocked event did not notify')
        first = calls('Notify')[0]
        assert first['urgency']==2 and first['replaces']==0
        report.with_suffix('.state').write_text('error')
        wait(lambda: len(calls('Notify'))==2, 'live error event did not notify')
        assert calls('Notify')[1]['replaces']==first['id']
        report.with_suffix('.state').write_text('notice')
        wait(lambda: len(calls('Notify'))==3, 'live program OSC notice did not notify')
        assert 'Owned program message' in calls('Notify')[-1]['body']
        assert calls('Notify')[-1]['replaces']==first['id']
        control('Click', first['id'])
        control('RejectNext')
        driver.wait(timeout=25)
        outcome = json.loads((work/'driver-report.json').read_text())
        assert outcome['success'], outcome.get('message')
        navigated = json.loads((work/'navigated.json').read_text())
        assert navigated['session']==sessions[1] and navigated['window']==target['window_id']
        assert calls('CloseNotification'), 'seen tab not withdrawn'
        assert len(calls('Notify'))==4, 'unexpected duplicate notifications'
        assert json.loads(profile.read_text())['notifications'].get('play_sounds',False) is False
        (work/'result.json').write_text(json.dumps({'live_osc':True,'replace':True,'captured_click_navigation':True,'native_settings_test':True,'minimum_capture':True,'default_sounds_off':True,'native_rejection_and_recovery':True},indent=2))
        print(work)
    finally:
        for child in reversed(processes):
            if child.poll() is None:
                child.terminate()
                try: child.wait(timeout=5)
                except subprocess.TimeoutExpired: child.kill();child.wait(timeout=3)
        for stream in streams: stream.close()
        shutil.rmtree(short)


if __name__=='__main__': main()
