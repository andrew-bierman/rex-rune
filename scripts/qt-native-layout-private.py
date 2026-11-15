#!/usr/bin/env python3
"""Check real French-layout Qt native key events on a newly owned headless Cage.

Requires built qt/build/native-tests/native-tests and installed or already
extracted Cage libraries. No personal display, keyboard, clipboard or Rex server
is used. Artifacts and processes belong exclusively to this invocation.
"""
import argparse
import os
import shlex
import signal
from pathlib import Path
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cage', required=True)
    parser.add_argument('--library-path')
    parser.add_argument('--tests', type=Path, default=ROOT/'qt/build/native-tests/native-tests')
    args = parser.parse_args()
    work = Path(tempfile.mkdtemp(prefix='native-layout-',dir=ROOT/'.build-tools'))
    flags=shlex.split(subprocess.check_output(['pkg-config','--cflags','--libs','wayland-client','xkbcommon'],text=True))
    subprocess.run(['cc','-std=c11',str(ROOT/'scripts/tools/qt-wayland-layout.c'),'-o',str(work/'layout-key'),*flags],check=True)
    with tempfile.TemporaryDirectory(prefix='rex-layout-') as runtime:
        env = {k:v for k,v in os.environ.items() if k not in (
            'DISPLAY','WAYLAND_DISPLAY','WAYLAND_SOCKET','DBUS_SESSION_BUS_ADDRESS',
            'HYPRLAND_INSTANCE_SIGNATURE','SWAYSOCK','SESSION_MANAGER') and not k.startswith(('WLR_','REX_','HERDR_'))}
        for name in ('home','config','cache','state'):
            (work/name).mkdir(mode=0o700)
        env.update(DBUS_SESSION_BUS_ADDRESS='unix:path='+str(work/'absent-bus'),HOME=str(work/'home'),XDG_CONFIG_HOME=str(work/'config'),
            XDG_CACHE_HOME=str(work/'cache'),XDG_STATE_HOME=str(work/'state'),XDG_RUNTIME_DIR=runtime,
            WLR_BACKENDS='headless',WLR_RENDERER='pixman',WLR_HEADLESS_OUTPUTS='1',
            XKB_DEFAULT_LAYOUT='fr',QT_QPA_PLATFORM='wayland',QT_QUICK_BACKEND='software',
            QT_QPA_PLATFORMTHEME='generic',REX_QT_NATIVE_LAYOUT_CHECK='1',REX_QT_LAYOUT_TYPER=str(work/'layout-key'))
        if args.library_path: env['LD_LIBRARY_PATH']=args.library_path
        with (work/'cage.log').open('w') as log:
            cage=subprocess.Popen([args.cage,'-D','--','sleep','40'],env=env,stdout=log,stderr=log,start_new_session=True)
            try:
                deadline=time.monotonic()+10
                while not any(p.is_socket() for p in Path(runtime).glob('wayland-*')):
                    assert cage.poll() is None, f'Cage failed: {work}'
                    assert time.monotonic()<deadline, f'Cage timeout: {work}'
                    time.sleep(.05)
                env['WAYLAND_DISPLAY']=next(p.name for p in Path(runtime).glob('wayland-*') if p.is_socket())
                result=subprocess.run([str(args.tests.resolve()),'nativeFrenchLayout','-o',str(work/'native.txt')+',txt'],env=env,timeout=20)
                assert result.returncode==0, f'Native layout proof failed: {work}'
                print(f'PASS real private Wayland French layout: {work}')
            finally:
                os.killpg(cage.pid,signal.SIGTERM)
                try: cage.wait(timeout=4)
                except subprocess.TimeoutExpired: os.killpg(cage.pid,signal.SIGKILL);cage.wait()

if __name__=='__main__': main()
