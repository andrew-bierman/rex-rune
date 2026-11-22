#!/usr/bin/env python3
"""Verify rex-tui renames in Foot on a private labwc and disposable Rex.

Pass the packaged launcher. Requires rex, Foot, wtype, and labwc. For an
extracted compositor, use --labwc and put its libraries on LD_LIBRARY_PATH.
"""

import argparse
from contextlib import ExitStack
import json
import os
from pathlib import Path
import shlex
import signal
import subprocess
import tempfile
import time


def wait_for(check, description, process):
    deadline = time.monotonic() + 10
    while process.poll() is None and time.monotonic() < deadline:
        if result := check():
            return result
        time.sleep(.08)
    raise AssertionError(f"{description} did not complete (process status {process.poll()})")


def stop(process):
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=4)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=4)


def exercise(launcher, labwc, root):
    env = {key: value for key, value in os.environ.items()
           if not key.startswith(("REX_", "HERDR_", "WLR_", "AQ_"))
           and key not in ("DISPLAY", "WAYLAND_DISPLAY", "WAYLAND_SOCKET", "SWAYSOCK",
                           "HYPRLAND_INSTANCE_SIGNATURE", "DBUS_SESSION_BUS_ADDRESS")}
    for name in ("home", "config", "state", "cache", "runtime"):
        (root / name).mkdir(mode=0o700)
    endpoint = f"unix://{root / 'rex.sock'}"
    env.update(HOME=str(root / "home"), XDG_CONFIG_HOME=str(root / "config"),
               XDG_STATE_HOME=str(root / "state"), XDG_CACHE_HOME=str(root / "cache"),
               XDG_RUNTIME_DIR=str(root / "runtime"), SHELL="/bin/sh",
               DBUS_SESSION_BUS_ADDRESS=f"unix:path={root / 'absent-bus'}",
               WLR_BACKENDS="headless", WLR_RENDERER="pixman", WLR_HEADLESS_OUTPUTS="1")
    rex = os.environ.get("REX_BIN", "rex")
    profile = root / "config" / "rex-tui"
    profile.mkdir()
    (profile / "workspace.json").write_text(json.dumps({
        "hosts": [{"label": "Private Rename", "endpoint": endpoint}],
    }))

    def api(method, **payload):
        return json.loads(subprocess.check_output(
            [rex, "--autostart=false", "--server", endpoint, "api", "call", method,
             json.dumps(payload)], env=env, timeout=4))

    with ExitStack() as stack:
        def spawn(command, logfile):
            log = stack.enter_context((root / logfile).open("wb"))
            process = subprocess.Popen(command, env=env, start_new_session=True,
                                       stdout=log, stderr=log)
            stack.callback(stop, process)
            return process

        server = spawn([rex, "server", "run", "--listen", endpoint, "--restore=false",
                        "--config", "/dev/null", "--state-dir", str(root / "server")],
                       "server.log")
        wait_for((root / "rex.sock").exists, "Rex socket", server)
        assert not api("session.list")["sessions"], "private server should start empty"
        command = shlex.join(["foot", "-m", "--window-size-chars=90x28", "--", str(launcher)])
        compositor = spawn([labwc, "-C", str(root / "config"), "-S", command], "foot.log")
        socket = wait_for(lambda: next((root / "runtime").glob("wayland-*.lock"), None),
                          "Wayland socket", compositor)
        env["WAYLAND_DISPLAY"] = socket.stem

        def send(*arguments):
            subprocess.run(["wtype", "-s", "150", *arguments, "-s", "150"],
                           env=env, check=True, timeout=4)

        def key(name, modifier=None):
            chord = ("-M", modifier) if modifier else ()
            send(*chord, "-k", name)

        def edit(label):
            key("u", "ctrl")
            if label:
                send(label)

        def wait(check, description):
            return wait_for(check, description, compositor)

        def shell_proof(name):
            marker = root / name
            send(f"printf passed > {shlex.quote(str(marker))}", "-k", "Return")
            wait(lambda: marker.is_file() and marker.read_text() == "passed",
                 f"terminal input after {name}")

        # Foot needs keyboard focus after the compositor opens its socket.
        time.sleep(1)
        key("F2")
        session, = wait(lambda: api("session.list")["sessions"], "session creation")
        sid = session["session_id"]

        def view():
            return api("session.view", session_id=sid)

        def active_tab():
            return view()["active_window_id"]

        def window(ident):
            return next(window for window in view()["windows"] if window["window_id"] == ident)

        def panes(ident):
            return {block["block_id"]: block.get("label") for layer in window(ident)["layers"]
                    for block in layer["blocks"] if block.get("block_id")}

        first = wait(active_tab, "initial tab")
        key("F6")
        shell_proof("before-rename")
        key("F6", "shift")
        edit("Review workspace")
        key("Return")
        wait(lambda: api("session.list")["sessions"][0]["label"] == "Review workspace",
             "session rename")
        key("Escape")

        key("F5")
        def next_tab():
            current = active_tab()
            return current if current != first else None

        second = wait(next_tab, "second tab")
        leak = root / "unexpected-shell-command"
        new_tab_label = f"touch {shlex.quote(str(leak))}"
        key("F7", "shift")
        edit(new_tab_label)
        key("Return")
        wait(lambda: window(second)["label"] == new_tab_label, "tab rename")
        key("Return")
        key("Escape")
        shell_proof("after-rename")
        assert not leak.exists(), "rename draft or repeated Enter leaked to the shell"

        key("F7", "shift")
        edit("Cancelled label")
        key("Escape")
        shell_proof("after-cancel")
        assert window(second)["label"] == new_tab_label, "Escape applied a rename"

        first_label = window(first).get("label")
        key("F7", "shift")
        edit("Stale label")
        api("session.focus_window", session_id=sid, window_id=first)
        wait(lambda: active_tab() == first, "remote tab switch")
        key("Return")
        key("Escape")
        shell_proof("after-remote-switch")
        assert window(first).get("label") == first_label, "rename followed the remote switch"
        assert window(second)["label"] == new_tab_label, "stale rename changed its old tab"

        original_pane = window(first)["focused_block_id"]
        key("F8")
        wait(lambda: len(panes(first)) == 2, "split creation")
        wait(lambda: window(first).get("focused_block_id") not in (None, original_pane),
             "split focus")
        focused = window(first)["focused_block_id"]
        shell_proof("after-split")
        before = panes(first)
        other, = before.keys() - {focused}
        key("F8", "shift")
        edit("Review shell")
        key("Return")
        try:
            wait(lambda: panes(first)[focused] == "Review shell", "pane rename")
        except AssertionError as error:
            error.add_note(f"Target pane was {focused}. Server labels after the rename: {panes(first)}")
            raise
        key("Escape")
        assert panes(first)[other] == before[other], "rename changed the other pane"
        key("F8", "shift")
        edit("")
        key("Return")
        # A generated title can change when shell output or focus changes.
        wait(lambda: panes(first)[focused] != "Review shell", "pane label reset")
        assert panes(first)[other] == before[other], "reset changed the other pane"
        key("Escape")

        key("F10")
        assert compositor.wait(timeout=5) == 0, "Foot session failed to exit"
        assert len(view()["windows"]) == 2, "client exit changed the session"
        print("Foot verified session, tab and pane renames, cancelled and stale drafts, title reset, and shell isolation.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("launcher", type=Path, help="Path to the packaged rex-tui launcher")
    parser.add_argument("--labwc", default="labwc")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="rex-rename-") as temp:
        root = Path(temp)
        try:
            exercise(args.launcher.resolve(strict=True), args.labwc, root)
        except Exception as error:
            for log in ("server.log", "foot.log"):
                path = root / log
                if path.exists():
                    error.add_note(f"{log}:\n{path.read_text(errors='replace')[-4000:]}")
            raise


if __name__ == "__main__":
    main()
