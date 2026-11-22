#!/usr/bin/env python3
"""Capture the real rex-tui in private Foot with a fictional Rex workspace."""

import argparse
import json
import os
from pathlib import Path
import shlex
import shutil
import signal
import subprocess
import sys
import tempfile
import time


ROOT = Path(__file__).resolve().parents[1]
FOREGROUND = 0xC4D5E2
BACKGROUND = 0x101922
CYAN = 0x83D2CD
GREEN = 0xA8D88F
GOLD = 0xF1C77A
MUTED = 0x8298AD


def terminal_frame(lines, cursor=None):
    cells = []
    roles = []
    for foreground, content in lines:
        cells.append([
            {"text": char, "fg": foreground if foreground is not None else FOREGROUND,
             "bg": BACKGROUND}
            for char in content
        ])
        roles.append(("3" if foreground is None else "2") * len(content))
    return {
        "cols": 60,
        "rows": 30,
        "fg": FOREGROUND,
        "bg": BACKGROUND,
        "cursor": cursor,
        "cells": cells,
        "color_roles": {"inherit": [True, True], "rows": roles},
    }


def scene():
    connected = {"connected": True, "owner": True}
    tabs = [
        {"id": "build", "label": "Build", "selected": True},
        {"id": "review", "label": "Review"},
        {"id": "notes", "label": "Notes"},
    ]
    sessions = [
        {"id": "orchard", "label": "Orchard", "selected": True, "opened": True, "tabs": tabs},
        {"id": "drift", "label": "Drift"},
    ]
    panes = [
        {
            "pane": "build-shell",
            "label": "Build shell",
            "rect": [0, 0, 0.57, 1],
            "terminal": connected,
            "frame": terminal_frame([
                (CYAN, "~/orchard  main"),
                (None, "$ just check"),
                (None, ""),
                (MUTED, "  FORMAT"),
                (GREEN, "  ok   cargo fmt"),
                (None, ""),
                (MUTED, "  TESTS"),
                (GREEN, "  ok   24 unit tests"),
                (GREEN, "  ok    8 integration tests"),
                (None, ""),
                (GREEN, "Checks passed in 1.8s"),
                (None, ""),
                (None, "$ "),
            ], cursor=[2, 12]),
        },
        {
            "pane": "service-log",
            "label": "Service log",
            "rect": [0.57, 0, 0.43, 0.54],
            "terminal": connected,
            "frame": terminal_frame([
                (CYAN, "09:24:02  starting preview"),
                (GREEN, "09:24:02  service ready"),
                (None, "09:24:07  GET /healthz  200"),
                (None, "09:24:11  GET /status   200"),
                (MUTED, "Watching for changes"),
            ]),
        },
        {
            "pane": "change-notes",
            "label": "Change notes",
            "rect": [0.57, 0.54, 0.43, 0.46],
            "terminal": connected,
            "frame": terminal_frame([
                (GOLD, "Orchard / next pass"),
                (None, ""),
                (GREEN, "  [x] Build and test"),
                (None, "  [ ] Review cache behavior"),
                (None, "  [ ] Update release notes"),
            ]),
        },
    ]
    for pane in panes:
        pane.update(id=pane["pane"], session="orchard", window="build")
    return {
        "type": "state", "version": 3, "connected": True, "status": "Connected",
        "host": "workshop", "epoch": 1, "session": "orchard", "window": "build",
        "pane": "build-shell", "terminal": connected, "sessions": sessions,
        "windows": tabs, "panes": panes,
        "hosts": {
            "active": "workshop", "pending": False,
            "rows": [
                {"id": "workshop", "label": "Workshop", "connected": True, "active": True,
                 "sessions": sessions},
                {"id": "relay", "label": "Build relay", "connected": True,
                 "sessions": [{"id": "builds", "label": "Nightly builds"}]},
                {"id": "bench", "label": "Test bench", "connected": False,
                 "sessions": [{"id": "trials", "label": "Trials"}]},
            ],
        },
    }


def serve():
    # Only the capture runner can select this backend mode. The fixture has
    # no Rex endpoint, never opens a workspace profile, and makes no network calls.
    ready = Path(os.environ["REX_TUI_DEMO_READY"])
    print(json.dumps({"type": "hello", "version": 3}), flush=True)
    print(json.dumps(scene()), flush=True)
    for line in sys.stdin:
        request = json.loads(line)
        if request.get("op") == "resize":
            ready.touch()
        if request.get("op") == "shutdown":
            return


def wait_for(predicate, compositor, description):
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline and compositor.poll() is None:
        result = predicate()
        if result:
            return result
        time.sleep(0.05)
    raise RuntimeError(f"Timed out waiting for {description}")


def capture(args):
    binary = args.binary.resolve(strict=True)
    output = args.output.resolve()
    labwc = shutil.which(args.compositor)
    if not labwc or not all(shutil.which(command) for command in ("foot", "grim")):
        raise RuntimeError("Install labwc, Foot and grim or supply --compositor")
    with tempfile.TemporaryDirectory(prefix="rex-tui-demo-") as directory:
        work = Path(directory)
        for folder in ("home", "config", "cache", "state", "runtime"):
            (work / folder).mkdir(mode=0o700)
        env = {
            "PATH": os.environ["PATH"],
            "LC_ALL": "C.UTF-8",
            "HOME": str(work / "home"),
            "XDG_CONFIG_HOME": str(work / "config"),
            "XDG_CACHE_HOME": str(work / "cache"),
            "XDG_STATE_HOME": str(work / "state"),
            "XDG_RUNTIME_DIR": str(work / "runtime"),
            "DBUS_SESSION_BUS_ADDRESS": f"unix:path={work / 'absent-bus'}",
            "WLR_BACKENDS": "headless",
            "WLR_RENDERER": "pixman",
            "WLR_HEADLESS_OUTPUTS": "1",
            "REX_TUI_DEMO_READY": str(work / "ready"),
        }
        if args.library_path:
            env["LD_LIBRARY_PATH"] = str(args.library_path.resolve(strict=True))
        config = work / "config"
        (config / "rc.xml").write_text("<labwc_config/>\n")
        settings = work / "settings.toml"
        settings.write_text('startup_focus = "terminal"\n[sidebar]\nwidth = 27\n')
        launch = shlex.join(map(str, [
            "foot", "--fullscreen", "-o", f"font={args.font}",
            "-o", "colors-dark.background=101922", "-o", "colors-dark.foreground=c4d5e2",
            "-o", "colors-dark.regular6=83d2cd", "-o", "colors-dark.bright0=8298ad",
            "-o", "cursor.blink=no", binary, Path(__file__).resolve(),
            work / "unused-workspace.json", "--settings", settings,
        ]))
        log = work / "compositor.log"
        with log.open("w") as stream:
            compositor = subprocess.Popen(
                [labwc, "-C", config, "-S", launch], env=env, start_new_session=True,
                stdout=stream, stderr=subprocess.STDOUT,
            )
        try:
            socket = wait_for(
                lambda: next((path for path in (work / "runtime").glob("wayland-*")
                              if path.is_socket()), None),
                compositor, "the private Wayland socket",
            )
            env["WAYLAND_DISPLAY"] = socket.name
            wait_for(lambda: (work / "ready").is_file(), compositor, "the first rendered panes")
            time.sleep(0.2)
            draft = work / "workspace.png"
            subprocess.run(["grim", str(draft)], env=env, check=True, timeout=5)
            output.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(draft, output)
        except (RuntimeError, subprocess.SubprocessError):
            print(log.read_text(), file=sys.stderr)
            raise
        finally:
            if compositor.poll() is None:
                os.killpg(compositor.pid, signal.SIGTERM)
                compositor.wait(timeout=5)
    print(output)


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--config" and os.environ.get("REX_TUI_DEMO_READY"):
        serve()
        return
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument("--binary", type=Path, required=True, help="the actual frontend executable")
    cli.add_argument("--compositor", default="labwc", help="labwc command or path")
    cli.add_argument("--library-path", type=Path, help="runtime libraries for a private labwc build")
    cli.add_argument("--font", default="monospace:size=12", help="Foot font descriptor")
    cli.add_argument("--output", type=Path, default=ROOT / "docs" / "images" / "rex-tui-demo.png")
    capture(cli.parse_args())


if __name__ == "__main__":
    main()
