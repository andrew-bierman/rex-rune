#!/usr/bin/env python3
"""Verify keyboard divider resizing in Foot with a private labwc and Rex.

Pass the packaged rex-tui launcher. Requires rex, foot, wtype and labwc.
Use --labwc for an extracted labwc and its libraries on LD_LIBRARY_PATH.
"""

import argparse
from contextlib import ExitStack
import json
import math
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
        "hosts": [{"label": "Private Resize", "endpoint": endpoint}],
    }))
    settings = profile / "settings.toml"
    settings.write_text('[sidebar]\nwidth = 20\n')

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
        command = shlex.join(["foot", "-m", "--window-size-chars=90x28", "--", str(launcher)])
        compositor = spawn([labwc, "-C", str(root / "config"), "-S", command], "foot.log")
        socket = wait_for(lambda: next((root / "runtime").glob("wayland-*.lock"), None),
                          "Wayland socket", compositor)
        env["WAYLAND_DISPLAY"] = socket.stem

        def key(name, *modifiers):
            chord = [argument for modifier in modifiers for argument in ("-M", modifier)]
            subprocess.run(["wtype", "-s", "150", *chord, "-k", name, "-s", "150"], env=env,
                           check=True, timeout=4)

        # The Wayland socket is created before Foot's first surface has keyboard focus.
        time.sleep(1)
        key("F2")
        session, = wait_for(lambda: api("session.list")["sessions"], "session creation", compositor)
        sid = session["session_id"]

        def active_window():
            view = api("session.view", session_id=sid)
            return next(window for window in view["windows"]
                        if window["window_id"] == view["active_window_id"])

        def splits():
            root_split = active_window()["layers"][0]["layout"].get("split")
            if not root_split:
                return None
            inner = root_split["before"].get("split") or root_split["after"].get("split")
            return root_split, inner

        def ratios(horizontal, vertical):
            pair = splits()
            return (pair and pair[1] and
                    math.isclose(pair[0]["ratio"], horizontal, abs_tol=.0001) and
                    math.isclose(pair[1]["ratio"], vertical, abs_tol=.0001))

        key("F8")
        wait_for(splits, "side-by-side split", compositor)
        key("F9")
        wait_for(lambda: ratios(.5, .5), "nested split at its default ratios", compositor)
        for direction, expected in (("Left", (.45, .5)), ("Right", (.5, .5)),
                                    ("Up", (.5, .45)), ("Down", (.5, .5))):
            key(direction, "ctrl", "shift")
            try:
                wait_for(lambda: ratios(*expected), f"{direction} ratio {expected}", compositor)
            except AssertionError as error:
                error.add_note(f"Server ratios after {direction}: {[split['ratio'] for split in splits()]}")
                raise

        settings.write_text('[sidebar]\nwidth = 20\n[keys]\nresize_left = "Alt+r"\n')
        key("F1")
        time.sleep(.15)
        key("r")
        time.sleep(.15)
        key("Escape")
        time.sleep(.15)
        key("r", "alt")
        wait_for(lambda: ratios(.45, .5), "remapped Left ratio", compositor)
        key("F10")
        assert compositor.wait(timeout=5) == 0, "Foot session failed to exit"
        assert ratios(.45, .5), "exiting changed the session's layout"
        print("Foot resized both axes in both directions and applied the remapped shortcut.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("launcher", type=Path, help="Path to the packaged rex-tui launcher")
    parser.add_argument("--labwc", default="labwc")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="rex-resize-") as temp:
        root = Path(temp)
        try:
            exercise(args.launcher.resolve(), args.labwc, root)
        except Exception as error:
            # These logs come from the fixture's empty private profile.
            for log in ("server.log", "foot.log"):
                path = root / log
                if path.exists():
                    error.add_note(f"{log}:\n{path.read_text(errors='replace')[-4000:]}")
            raise


if __name__ == "__main__":
    main()
