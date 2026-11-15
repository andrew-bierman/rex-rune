#!/usr/bin/env python3
"""Exercise maintenance through native Qt events with fake Rex and a disposable HOME.

Needs built Qt test driver/backend. Uses offscreen software Qt (not a compositor
or a desktop clipboard); never starts Rex or invokes installers/installed apps.
Artifacts remain under .build-tools for inspection.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--driver', type=Path, required=True)
    parser.add_argument('--backend', type=Path, required=True)
    args = parser.parse_args()
    work = Path(tempfile.mkdtemp(prefix='maintenance-private-', dir=ROOT / '.build-tools'))
    for name in ('home', 'config', 'state', 'cache', 'runtime', 'bin'):
        (work / name).mkdir(mode=0o700)
    fake = work / 'bin' / 'rex'
    fake.write_text('''#!/usr/bin/python3
import json,sys,os,pathlib
home=pathlib.Path(os.environ['HOME'])
a=sys.argv[1:]
with (home/'calls.jsonl').open('a') as f:f.write(json.dumps(a)+'\\n')
assert a[:2]==['--autostart=false','skill']
assert a[2] in ['status','setup','remove']
assert a[-1]=='--json'
if a[2]=='status':
 print(json.dumps({'harnesses':[{'id':'claude','detected':True,'items':[{'kind':'skill','state':'current'}]},{'id':'codex','detected':True,'items':[{'kind':'skill','state':'foreign','path':str(home/'foreign')} ]},{'id':'pi','detected':True,'items':[{'kind':'skill','state':'missing'}]}]}))
elif '--dry-run' in a:
 print(json.dumps({'steps':[{'kind':'skill','path':str(home/'managed'),'change':'remove' if a[2]=='remove' else 'write','harnesses':['claude']}]}))
elif (home/'fail').exists():
 print('fixture exact failure',file=sys.stderr);sys.exit(7)
else:
 if a[2]=='remove': (home/'managed').unlink(missing_ok=True)
 else: (home/'managed').write_text('fixture managed integration')
 print(json.dumps({'steps':[{'kind':'skill','change':'done'}]}))
''')
    fake.chmod(0o700)
    env = {key: value for key, value in os.environ.items() if not key.startswith(('REX_', 'HERDR_')) and key not in ('DISPLAY', 'WAYLAND_DISPLAY', 'DBUS_SESSION_BUS_ADDRESS')}
    env.update(DBUS_SESSION_BUS_ADDRESS='unix:path='+str(work/'absent-bus'),HOME=str(work/'home'), XDG_CONFIG_HOME=str(work/'config'), XDG_STATE_HOME=str(work/'state'),
               XDG_CACHE_HOME=str(work/'cache'), XDG_RUNTIME_DIR=str(work/'runtime'), PATH=str(work/'bin')+':'+os.environ['PATH'],
               QT_QPA_PLATFORM='offscreen', QT_QUICK_BACKEND='software', QT_QPA_PLATFORMTHEME='generic')
    config = work / 'workspace.json'
    socket = Path('/tmp') / (work.name + '.sock')
    assert not socket.exists()
    endpoint = 'unix://' + str(socket)
    config.write_text(json.dumps({'hosts':[{'label':'PRIVATE_HOST_SENTINEL','endpoint':endpoint}], 'font_family':'IBM Plex Mono'}))
    (work/'home'/'foreign').write_text('foreign-owned')
    def run(name, steps):
        script = work / (name+'.json')
        script.write_text(json.dumps(steps))
        with (work/(name+'.log')).open('w') as log:
            result = subprocess.run([str(args.driver.resolve()), '--backend', str(args.backend.resolve()), '--config', str(config), '--script', str(script), '--report', str(work/(name+'-report.json'))], env=env, stdout=log, stderr=log, timeout=45)
        assert result.returncode == 0, f'{name} Qt events failed: {work}'
    def page(button):
        return [{'op':'pause','ms':300},{'op':'click','control':'settingsButton'},{'op':'click','control':button}]
    def wait(field, value):
        return {'op':'waitState','field':'maintenance.'+field,'value':value}
    preview = [{'op':'click','control':'integrationSetup_claude'}, wait('preview.steps.0.change','write')]
    run('preview-cancel', page('integrationsSettingsButton')+[wait('status.harnesses.1.items.0.state','foreign')]+preview+[
        {'op':'screenshot','path':str(work/'preview.png')},{'op':'click','control':'integrationCancel'},wait('preview',None),{'op':'click','control':'integrationClose'}])
    assert not (work/'home'/'managed').exists()
    run('confirmed',page('integrationsSettingsButton')+[wait('status.harnesses.0.items.0.state','current')]+preview+[
        {'op':'click','control':'integrationConfirm'},wait('apply.steps.0.change','done'),{'op':'click','control':'integrationClose'}])
    assert (work/'home'/'managed').read_text()=='fixture managed integration'
    run('remove',page('integrationsSettingsButton')+[wait('status.harnesses.2.items.0.state','missing'),
        {'op':'click','control':'integrationRemove_claude'},wait('preview.steps.0.change','remove'),
        {'op':'click','control':'integrationConfirm'},wait('apply.steps.0.change','done'),{'op':'click','control':'integrationClose'}])
    assert not (work/'home'/'managed').exists()
    (work/'home'/'fail').touch()
    run('failure',page('integrationsSettingsButton')+[wait('status.harnesses.0.items.0.state','current')]+preview+[
        {'op':'click','control':'integrationConfirm'},wait('error','rex skill failed: fixture exact failure'),{'op':'snapshot','path':str(work/'failure-state.json')}])
    (work/'home'/'fail').unlink()
    run('about',page('aboutSettingsButton')+[
        {'op':'click','control':'copyMaintenanceReport'},{'op':'snapshot','path':str(work/'about-state.json')},
        {'op':'screenshot','path':str(work/'about.png')},{'op':'click','control':'aboutClose'}])
    report=json.loads((work/'about-state.json').read_text())['clipboard']
    for secret in ('PRIVATE_HOST_SENTINEL',endpoint,str(work),'never-created.sock'):
        assert secret not in report
    assert 'Qt runtime 6.' in report and 'core 0.' in report and 'separate source build' in report
    # Refuse the private state lock before the checker can fetch any network data.
    (work/'verified-updates.lock').mkdir()
    run('updates',page('updatesSettingsButton')+[
        {'op':'click','control':'checkSignedUpdates'},
        wait('error','update state is busy or not writable; try again shortly'),
        {'op':'screenshot','path':str(work/'updates.png')},{'op':'click','control':'updatesClose'}])
    calls=[json.loads(line) for line in (work/'home'/'calls.jsonl').read_text().splitlines()]
    assert sum(a[2]=='setup' and '--dry-run' not in a for a in calls)==2
    assert (work/'home'/'foreign').read_text()=='foreign-owned'
    assert not socket.exists()
    (work/'result.json').write_text(json.dumps({'passed':True,'calls':len(calls),'report':report},indent=2))
    print(work)


if __name__ == '__main__':
    main()
