#!/usr/bin/env python3
"""Exercise bracketed paste through a packaged launcher and disposable Rex."""

import argparse
import json
import os
from pathlib import Path
import pty
import select
import shlex
import subprocess
import tempfile
import termios
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("launcher", type=Path, help="Path to the packaged rex-tui launcher")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="rex-tui-paste-") as temp:
        root = Path(temp)
        profile = root / "config" / "rex-tui" / "workspace.json"
        profile.parent.mkdir(parents=True)
        endpoint = f"unix://{root / 'rex.sock'}"
        profile.write_text(json.dumps({"hosts": [{"label": "Private Rex", "endpoint": endpoint}]}))
        env = {key: value for key, value in os.environ.items()
               if not key.startswith(("REX_", "HERDR_"))}
        env.update(HOME=temp, XDG_CONFIG_HOME=str(root / "config"),
                   XDG_STATE_HOME=str(root / "state"), XDG_RUNTIME_DIR=temp,
                   TERM="xterm-256color", SHELL="/bin/sh", PS1="rex-paste-private> ")
        with (root / "server.log").open("wb") as log:
            server = subprocess.Popen(
                [os.environ.get("REX_BIN", "rex"), "server", "run", "--listen", endpoint,
                 "--restore=false", "--config", "/dev/null", "--state-dir", str(root / "server")],
                env=env, stdout=log, stderr=log,
            )
            try:
                exercise(args.launcher.resolve(), root, env)
            finally:
                server.terminate()
                server.wait(timeout=5)


def exercise(launcher, root, env):
    master, slave = pty.openpty()
    try:
        termios.tcsetwinsize(slave, (35, 140))
        app = subprocess.Popen([launcher], env=env, stdin=slave, stdout=slave, stderr=slave)
    finally:
        os.close(slave)
    output = bytearray()

    def wait_for(predicate, description):
        deadline = time.monotonic() + 8
        while not predicate():
            if app.poll() is not None or time.monotonic() >= deadline:
                raise AssertionError(f"{description}: {bytes(output[-1200:])!r}")
            if select.select([master], [], [], 0.05)[0]:
                output.extend(os.read(master, 65536))

    def paste(text):
        os.write(master, b"\x1b[200~" + text.encode() + b"\x1b[201~")

    try:
        wait_for(lambda: b"F2 Create session" in output, "connect to private Rex")
        os.write(master, b"\x1bOQ")
        wait_for(
            lambda: b"rex-paste-private>" in output or b"Waiting for terminal" in output,
            "open session",
        )
        forbidden = root / "sidebar-paste"
        paste(f"touch {shlex.quote(str(forbidden))}\n")
        wait_for(lambda: b"before pasting" in output, "explain why sidebar paste is ignored")
        os.write(master, b"\x1b[17~")
        wait_for(lambda: b"Terminal" in output, "focus the remote shell")
        marker = root / "pasted"
        paste(f"echo PASTE_OK >> {shlex.quote(str(marker))}")
        wait_for(lambda: b"PASTE_OK" in output, "echo the pasted command")
        os.write(master, b"\r")
        wait_for(marker.exists, "execute the pasted command")
        assert marker.read_text() == "PASTE_OK\n"
        assert not forbidden.exists()
        os.write(master, b"\x1b[21~")
        assert app.wait(timeout=3) == 0
        print("Bracketed paste reached only the focused Rex shell, exactly once.")
    finally:
        if app.poll() is None:
            app.kill()
            app.wait()
        os.close(master)


if __name__ == "__main__":
    main()
