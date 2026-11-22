#!/usr/bin/env python3
"""Verify pane ANSI inheritance in packaged rex-tui on private Foot and Rex.

Requires rex (or REX_BIN), Foot, labwc, wtype, grim, and ImageMagick. Each
palette mode gets a new startup profile, Rex socket, and headless compositor.
"""

import argparse
from collections import Counter
from contextlib import ExitStack
import json
import os
from pathlib import Path
import shlex
import signal
import subprocess
import tempfile
import time

SERVER_RED = 'cc6666'
SERVER_BLUE = '81a2be'
FOOT_RED = '3ef1a4'
FOOT_BLUE = 'dd78f2'
FOOT_BG = '38475a'
OVERRIDE_RED = '12b4a8'
ROWS = (
    ('indexed foreground', 'e2af43', r'\033[31m' + '█' * 8),
    ('indexed background', '2edaeb', r'\033[44m' + ' ' * 8),
    ('explicit same-RGB foreground', 'a357d7', r'\033[38;2;204;102;102m' + '█' * 8),
    ('default background', 'efa41d', ' ' * 8),
)


def stop(process):
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=4)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=4)


def wait_for(check, description):
    until = time.monotonic() + 10
    while time.monotonic() < until:
        if result := check():
            return result
        time.sleep(.07)
    raise AssertionError(f'timed out waiting for {description}')


def frame_pixels(path):
    width, height = map(int, subprocess.check_output(
        ['magick', 'identify', '-format', '%w %h', str(path)], text=True, timeout=5).split())
    raw = subprocess.check_output(['magick', str(path), '-depth', '8', 'rgb:-'], timeout=5)
    assert len(raw) == width * height * 3
    return width, raw


def swatch_colors(width, raw, anchor):
    marker = bytes.fromhex(anchor)
    positions = []
    start = 0
    while (offset := raw.find(marker, start)) != -1:
        start = offset + 1
        if offset % 3 == 0:
            positions.append(divmod(offset // 3, width))
    assert len(positions) > 300, f'missing row marker #{anchor}'
    top, bottom = min(y for y, _ in positions), max(y for y, _ in positions)
    left, right = min(x for _, x in positions), max(x for _, x in positions)
    span = right - left + 1
    assert 20 < span < 140 and 6 < bottom - top < 45, (anchor, span, top, bottom)
    counts = Counter()
    # Sample the interior of the adjacent eight-cell swatch and skip edges.
    for y in range(top + 2, bottom - 1):
        line = raw[3 * (y * width + right + 3):3 * (y * width + right + span - 2)]
        counts.update(line[i:i + 3].hex() for i in range(0, len(line), 3))
    return counts


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('launcher', type=Path, help='Path to the packaged rex-tui launcher')
    parser.add_argument('--labwc', default='labwc')
    parser.add_argument('--library-path', type=Path)
    parser.add_argument('--control', action='store_true', help='Run an older client with server ANSI colors')
    parser.add_argument('--artifacts', type=Path, help='Keep captures in this new private directory')
    args = parser.parse_args()
    launcher = args.launcher.resolve(strict=True)
    if args.artifacts:
        work = args.artifacts.resolve()
        work.mkdir(mode=0o700)
        exercise_modes(args, launcher, work)
        print(f'Captures: {work}')
    else:
        with tempfile.TemporaryDirectory(prefix='rex-ansi-') as directory:
            exercise_modes(args, launcher, Path(directory))
    print(f'PASS: {"legacy control" if args.control else "ANSI inheritance"}')


def exercise_modes(args, launcher, work):
    for mode in ('terminal', 'server'):
        run = work / mode
        run.mkdir(mode=0o700)
        try:
            exercise(args, launcher, run, mode)
        except Exception as error:
            for filename in ('foot.log', 'server.log'):
                log = run / filename
                if log.is_file():
                    error.add_note(f'{filename}:\n{log.read_text(errors="replace")[-3000:]}')
            raise


def exercise(args, launcher, work, mode):
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(('REX_', 'HERDR_', 'WLR_', 'AQ_'))
           and k not in ('DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'SWAYSOCK',
                         'HYPRLAND_INSTANCE_SIGNATURE', 'DBUS_SESSION_BUS_ADDRESS', 'NO_COLOR')}
    for name in ('home', 'config', 'state', 'cache', 'runtime'):
        (work / name).mkdir(mode=0o700)
    endpoint = f'unix://{work / "rex.sock"}'
    env.update(HOME=str(work / 'home'), XDG_CONFIG_HOME=str(work / 'config'),
               XDG_STATE_HOME=str(work / 'state'), XDG_CACHE_HOME=str(work / 'cache'),
               XDG_RUNTIME_DIR=str(work / 'runtime'), SHELL='/bin/sh', LC_ALL='C.UTF-8',
               DBUS_SESSION_BUS_ADDRESS=f'unix:path={work / "absent"}',
               WLR_BACKENDS='headless', WLR_RENDERER='pixman', WLR_HEADLESS_OUTPUTS='1')
    if args.library_path:
        env['LD_LIBRARY_PATH'] = str(args.library_path.resolve(strict=True))
    rex = os.environ.get('REX_BIN', 'rex')
    shell = work / 'fixture-shell'
    shell.write_text("#!/bin/sh\nPS1='fixture$ '\nexport PS1\nexec /bin/sh -i\n")
    shell.chmod(0o700)
    config = work / 'config' / 'rex-tui'
    config.mkdir()
    (config / 'workspace.json').write_text(json.dumps({
        'hosts': [{'label': 'Private ANSI fixture', 'endpoint': endpoint, 'shell': str(shell)}],
    }))
    settings = config / 'settings.toml'

    ansi = '' if args.control else f'ansi_palette = "{mode}"\n'
    settings.write_text(f'[sidebar]\nwidth = 20\n[panes]\ndefault_colors = "{mode}"\n{ansi}')
    default_bg = FOOT_BG if mode == 'terminal' else '000000'

    def api(method):
        return json.loads(subprocess.check_output(
            [rex, '--autostart=false', '--server', endpoint, 'api', 'call', method, '{}'],
            env=env, timeout=5))

    def send(*parts):
        subprocess.run(['wtype', '-s', '125', *parts, '-s', '125'], check=True, env=env, timeout=4)

    def key(name):
        send('-k', name)

    def render(phase, name, osc, expected_fg, expected_bg):
        file = work / f'{name}.sh'
        done = work / f'{name}.ready'
        body = r'\033]0;ANSI palette fixture\007\033[0m\033[2J\033[H' + osc
        anchors = [f'{int(anchor, 16) + phase:06x}' for _, anchor, _ in ROWS]
        for anchor, (_, _, swatch) in zip(anchors, ROWS):
            r, g, b = bytes.fromhex(anchor)
            body += fr'\033[48;2;{r};{g};{b}m' + ' ' * 8 + r'\033[0m' + swatch + r'\033[0m\n'
        file.write_text(f'printf {shlex.quote(body)}\n: > {shlex.quote(str(done))}\n')
        send(f'sh {shlex.quote(str(file))}', '-k', 'Return')
        wait_for(done.is_file, f'{name} shell output')
        image = work / f'{name}.png'

        def capture_phase():
            subprocess.run(['grim', str(image)], env=env, check=True, timeout=4)
            width, raw = frame_pixels(image)
            # A unique truecolor marker in each row distinguishes this redraw
            # from the previous phase, even when the ANSI colors are identical.
            if all(bytes.fromhex(anchor) in raw for anchor in anchors):
                return width, raw
            return None

        width, raw = wait_for(capture_phase, f'{mode}/{name} redraw')
        expected_colors = (expected_fg, expected_bg, SERVER_RED, default_bg)
        for (label, _, _), anchor, expected in zip(ROWS, anchors, expected_colors):
            counts = swatch_colors(width, raw, anchor)
            assert counts[expected] > 400, (name, label, expected, counts.most_common(5))
        print(f'{mode}/{name}: ANSI fg #{expected_fg}, bg #{expected_bg}, explicit RGB #{SERVER_RED}, default bg #{default_bg}', flush=True)

    with ExitStack() as stack:
        def spawn(command, filename):
            log = stack.enter_context((work / filename).open('wb'))
            process = subprocess.Popen(command, env=env, start_new_session=True, stdout=log, stderr=log)
            stack.callback(stop, process)
            return process

        spawn([rex, 'server', 'run', '--listen', endpoint, '--restore=false', '--config',
               '/dev/null', '--state-dir', str(work / 'server')], 'server.log')
        wait_for((work / 'rex.sock').is_socket, 'private Rex socket')
        assert not api('session.list')['sessions']
        command = shlex.join(['foot', '--fullscreen', '-o', 'font=monospace:size=12',
                              '-o', f'colors-dark.regular1={FOOT_RED}',
                              '-o', f'colors-dark.regular4={FOOT_BLUE}',
                              '-o', f'colors-dark.background={FOOT_BG}',
                              '-o', 'cursor.blink=no', '--', str(launcher)])
        compositor = spawn([args.labwc, '-C', str(work / 'config'), '-S', command], 'foot.log')
        socket = wait_for(lambda: next((p for p in (work / 'runtime').glob('wayland-*')
                                       if p.is_socket()), None), 'private Wayland socket')
        env['WAYLAND_DISPLAY'] = socket.name
        time.sleep(.8)
        key('F2')
        wait_for(lambda: api('session.list')['sessions'], 'session creation')
        key('F6')

        local_red, local_blue = (
            (SERVER_RED, SERVER_BLUE) if args.control or mode == 'server' else (FOOT_RED, FOOT_BLUE)
        )
        render(0, 'baseline', '', local_red, local_blue)
        render(1, 'osc4-different-rgb', fr'\033]4;1;#{OVERRIDE_RED}\007', OVERRIDE_RED, local_blue)
        render(2, 'osc4-same-rgb', fr'\033]4;1;#{SERVER_RED}\007', SERVER_RED, local_blue)
        render(3, 'osc104-reset', r'\033]104;1\007', local_red, local_blue)
        key('F10')
        assert compositor.wait(timeout=5) == 0


if __name__ == '__main__':
    main()
