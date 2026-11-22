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


def exercise(launcher, labwc, root, directional_focus, tab_reorder):
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
        if directional_focus:
            horizontal = splits()[0]
            vertical = horizontal["after"]["split"]
            identities = {
                "left": horizontal["before"]["block_id"],
                "top": vertical["before"]["block_id"],
                "bottom": vertical["after"]["block_id"],
            }
            for name, pane in identities.items():
                api("block.event", session_id=sid, block_id=pane,
                    event={"type": "paste", "payload": {"text": f"export FOCUS_PANE={name}"}})
                api("block.event", session_id=sid, block_id=pane,
                    event={"type": "key", "payload": {"key": "enter", "action": "press", "mods": ""}})

            def proof(label, *expected):
                marker = root / label
                command = f"""printf '%s' "$FOCUS_PANE" > {shlex.quote(str(marker))}"""
                subprocess.run(["wtype", "-s", "150", command, "-k", "Return", "-s", "150"],
                               env=env, check=True, timeout=4)
                result = wait_for(lambda: marker.read_text() if marker.exists() else None,
                                  label, compositor)
                assert result in expected, (label, result, expected)
                return result

            for direction, expected in (("Up", "top"), ("Down", "bottom"), ("Left", "left")):
                key(direction, "ctrl", "alt")
                proof(f"focus-{direction}", expected)
            key("Left", "ctrl", "alt")
            proof("focus-no-wrap", "left")
            key("Right", "ctrl", "alt")
            focused = proof("focus-Right", "top", "bottom")
            key("F7")
            proof("f7-cycles-focus", "bottom" if focused == "top" else "left")
            if focused == "bottom":
                key("Right", "ctrl", "alt")
                proof("restore-right-focus", "top", "bottom")

            settings.write_text('[sidebar]\nwidth = 20\n[keys]\nresize_left = "Alt+r"\nfocus_left = "Alt+l"\n')
            key("F1")
            key("r")
            key("Escape")
            key("l", "alt")
            proof("remapped-focus", "left")
            print("Foot confirmed directional focus, no wrapping, and F7 cycling in nested splits.")
        if tab_reorder:
            def tab_state():
                view = api("session.view", session_id=sid)
                order = tuple(tab["window_id"] for tab in view["windows"])
                return order, view["active_window_id"]

            first = tab_state()[1]
            key("F5")
            second = wait_for(lambda: tab_state()[1] != first and tab_state()[1],
                              "create second tab", compositor)
            key("F5")
            third = wait_for(lambda: len(tab_state()[0]) == 3 and tab_state()[1] != second
                             and tab_state()[1], "create third tab", compositor)
            # Server focus and order can be visible through the observer before
            # rex-tui has received the corresponding backend snapshot.
            def settle(expected):
                wait_for(lambda: tab_state() == (expected, third),
                         "server order and active tab", compositor)
                time.sleep(.5)

            def move(direction, expected):
                key(direction, "alt", "shift")
                settle(expected)

            settle((first, second, third))
            move("Left", (first, third, second))
            move("Left", (third, first, second))
            key("Left", "alt", "shift")
            settle((third, first, second))
            move("Right", (first, third, second))
            move("Right", (first, second, third))
            key("Right", "alt", "shift")
            settle((first, second, third))

            settings.write_text(settings.read_text() + 'move_tab_left = "Alt+m"\n')
            key("F1")
            key("r")
            key("Escape")
            key("m", "alt")
            wait_for(lambda: tab_state() == ((first, third, second), third),
                     "remapped tab move", compositor)
            time.sleep(.5)
            key("F3")
            wait_for(lambda: tab_state()[1] == first, "restore original split tab", compositor)
            print("Foot reordered tabs on the server, retained active ID, and stopped at both edges.")
        key("F10")
        assert compositor.wait(timeout=5) == 0, "Foot session failed to exit"
        assert ratios(.45, .5), "exiting changed the session's layout"
        print("Foot resized both axes in both directions and applied the remapped shortcut.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("launcher", type=Path, help="Path to the packaged rex-tui launcher")
    parser.add_argument("--labwc", default="labwc")
    parser.add_argument("--focus", action="store_true", help="Also verify directional pane focus")
    parser.add_argument("--reorder", action="store_true", help="Also verify keyboard tab reordering")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="rex-resize-") as temp:
        root = Path(temp)
        try:
            exercise(args.launcher.resolve(), args.labwc, root, args.focus, args.reorder)
        except Exception as error:
            # These logs come from the fixture's empty private profile.
            for log in ("server.log", "foot.log"):
                path = root / log
                if path.exists():
                    error.add_note(f"{log}:\n{path.read_text(errors='replace')[-4000:]}")
            raise


if __name__ == "__main__":
    main()
