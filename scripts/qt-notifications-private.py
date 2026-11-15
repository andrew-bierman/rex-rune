#!/usr/bin/env python3
"""Native Qt notification/service tests on a private bus and fake-only player PATH.

Requires Qt's notification-services-test binary, dbus-daemon and /usr/bin/python3
with dbus-python/GLib. No personal display, audio, server or service is accessed.
All artifacts remain in this checkout's ignored .build-tools directory.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


def fake_service(work):
    assert os.environ.get("DBUS_SESSION_BUS_ADDRESS", "").startswith("unix:abstract=rex-qt-") and work.name in os.environ["DBUS_SESSION_BUS_ADDRESS"], "explicit private bus required"
    import dbus
    import dbus.service
    from dbus.mainloop.glib import DBusGMainLoop
    from gi.repository import GLib
    DBusGMainLoop(set_as_default=True)
    bus = dbus.SessionBus()
    name = dbus.service.BusName('org.freedesktop.Notifications', bus)
    interface = 'org.freedesktop.Notifications'

    def record(**row):
        with (work / 'calls.jsonl').open('a') as stream:
            stream.write(json.dumps(row) + '\n')

    class Service(dbus.service.Object):
        def __init__(self):
            super().__init__(name, '/org/freedesktop/Notifications')
            self.next_id = 1
            self.reject = False

        @dbus.service.method(interface, in_signature='', out_signature='ssss')
        def GetServerInformation(self):
            return ('Private fake', 'Rex tests', '1', '1.2')

        @dbus.service.method(interface, in_signature='susssasa{sv}i', out_signature='u')
        def Notify(self, app, replaces, icon, summary, body, actions, hints, expire):
            if self.reject:
                self.reject = False
                record(method='Rejected')
                raise dbus.exceptions.DBusException('Private refusal', name='org.freedesktop.DBus.Error.Failed')
            ident = int(replaces) or self.next_id
            self.next_id = max(self.next_id, ident + 1)
            record(method='Notify', id=ident, replaces=int(replaces), summary=str(summary), body=str(body), urgency=int(hints['urgency']), actions=list(actions), pid=os.getpid())
            return dbus.UInt32(ident)

        @dbus.service.method(interface, in_signature='u', out_signature='')
        def CloseNotification(self, ident):
            record(method='CloseNotification', id=int(ident), pid=os.getpid())
            self.NotificationClosed(ident, 3)

        @dbus.service.signal(interface, signature='uu')
        def NotificationClosed(self, ident, reason):
            pass

        @dbus.service.signal(interface, signature='us')
        def ActionInvoked(self, ident, action):
            pass

        @dbus.service.signal(interface, signature='us')
        def ActivationToken(self, ident, token):
            pass

        @dbus.service.method('dev.shuv.NotificationTest', in_signature='', out_signature='')
        def RejectNext(self):
            self.reject = True

        @dbus.service.method('dev.shuv.NotificationTest', in_signature='u', out_signature='')
        def Click(self, ident):
            self.ActivationToken(ident, 'private-activation-token')
            self.ActionInvoked(ident, 'default')

    instance = Service()
    (work / 'service-ready').write_text(str(os.getpid()))
    GLib.MainLoop().run()
    return instance


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--test-bin', type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    work = Path(tempfile.mkdtemp(prefix='notifications-private-', dir=root / '.build-tools'))
    env = {k: v for k, v in os.environ.items() if not k.startswith(('REX_', 'HERDR_', 'HYPR')) and k not in ('DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'DBUS_SESSION_BUS_ADDRESS')}
    for folder in ('home', 'config', 'runtime', 'cache', 'data', 'tmp', 'bin'):
        (work / folder).mkdir(mode=0o700)
    env.update(HOME=str(work / 'home'), XDG_CONFIG_HOME=str(work / 'config'), XDG_RUNTIME_DIR=str(work / 'runtime'),
               XDG_CACHE_HOME=str(work / 'cache'), XDG_DATA_HOME=str(work / 'data'), XDG_DATA_DIRS=str(work / 'data'), TMPDIR=str(work / 'tmp'),
               PATH=str(work / 'bin'), REX_NOTIFICATION_FIXTURE=str(work), REX_NOTIFICATION_SCRIPT=str(Path(__file__).resolve()))
    player = work / 'bin/canberra-gtk-play'
    player.write_text('#!/usr/bin/python3\nimport os,json,time,pathlib,sys\nw=pathlib.Path(os.environ["REX_NOTIFICATION_FIXTURE"])\nwith (w/"audio.jsonl").open("a") as f:f.write(json.dumps({"args":sys.argv[1:],"pid":os.getpid()})+"\\n")\nif not (w/"player-mode").exists():time.sleep(20)\n')
    player.chmod(0o700)
    address = 'unix:abstract=rex-qt-' + work.name
    (work / 'bus.conf').write_text('<busconfig><listen>' + address + '</listen><type>session</type><auth>EXTERNAL</auth><policy context="default"><allow user="' + str(os.getuid()) + '"/><allow send_destination="*"/><allow receive_sender="*"/><allow own="*"/></policy></busconfig>')
    env['DBUS_SESSION_BUS_ADDRESS'] = address
    buslog = (work / 'bus.log').open('w')
    bus = subprocess.Popen(['/usr/bin/dbus-daemon', '--config-file=' + str(work / 'bus.conf'), '--nofork', '--nopidfile', '--print-address=1', '--address=' + address], env=env, stdout=buslog, stderr=buslog)
    env["REX_NOTIFICATION_BUS_PID"] = str(bus.pid)
    try:
        import time
        for _ in range(100):
            if (work / 'bus.log').stat().st_size: break
            time.sleep(.02)
        with (work / 'native-test.log').open('w') as log:
            result = subprocess.run([str(args.test_bin.resolve())], env=env, stdout=log, stderr=log, timeout=45)
        print(work)
        print((work / 'native-test.log').read_text())
        if result.returncode: raise SystemExit(result.returncode)
        for line in (work / 'audio.jsonl').read_text().splitlines():
            assert not Path('/proc', str(json.loads(line)['pid'])).exists(), 'fake player not reaped'
        (work / 'result.json').write_text(json.dumps({'passed':True,'private_bus':True,'fake_player_only':True,'players_reaped':True}, indent=2))
    finally:
        bus.terminate()
        bus.wait(timeout=5)
        buslog.close()


if __name__ == '__main__':
    if len(sys.argv) == 3 and sys.argv[1] == '--service': fake_service(Path(sys.argv[2]))
    else: main()
