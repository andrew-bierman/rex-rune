#!/usr/bin/env python3
"""Install the Windows artifact and exercise Rex through a private HTTPS ConPTY."""

import argparse
import asyncio
from collections import deque
from contextlib import contextmanager
import functools
import hashlib
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import socket
import ssl
import subprocess
import tempfile
import threading
import time
import zipfile


def wait_for(check, label, seconds=20):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if result := check():
            return result
        time.sleep(0.1)
    raise RuntimeError(f"Timed out waiting for {label}")


@contextmanager
def file_mirror(root):
    handler = functools.partial(SimpleHTTPRequestHandler, directory=root)
    with ThreadingHTTPServer(("127.0.0.1", 0), handler) as server:
        worker = threading.Thread(target=server.serve_forever, daemon=True)
        worker.start()
        try:
            yield f"http://127.0.0.1:{server.server_port}"
        finally:
            server.shutdown()
            worker.join()


def install_pair(installer, archive, root, env):
    mirror = root / "mirror"
    mirror.mkdir()
    mirrored = mirror / archive.name
    shutil.copyfile(archive, mirrored)
    destination = root / "installed pair"
    with file_mirror(mirror) as url:
        command = ["pwsh", "-NoLogo", "-NoProfile", "-NonInteractive",
                   "-ExecutionPolicy", "Bypass", "-File", installer]
        install_env = {**env, "REX_TUI_DOWNLOAD_URL": url, "REX_TUI_NO_MODIFY_PATH": "1",
                       "REX_TUI_UNMANAGED_INSTALL": str(destination)}
        subprocess.run(command, env=install_env, check=True, timeout=45)
        with zipfile.ZipFile(archive) as package:
            for name in ("rex-tui.exe", "rex-qt-backend.exe"):
                if (destination / name).read_bytes() != package.read(name):
                    raise RuntimeError(f"Installer changed {name}")
        with mirrored.open("ab") as output:
            output.write(b"tamper")
        rejected = root / "rejected"
        install_env["REX_TUI_UNMANAGED_INSTALL"] = str(rejected)
        result = subprocess.run(command, env=install_env, capture_output=True, text=True, timeout=45)
        if (result.returncode == 0 or "checksum mismatch" not in result.stdout + result.stderr
                or list(rejected.rglob("*.exe"))):
            raise RuntimeError(f"Installer failed the tamper check: {result.stdout}\n{result.stderr}")
    return destination


def test_certificate(root, openssl):
    ca, ca_key = root / "ca.pem", root / "ca.key"
    cert, key, request = root / "fixture.pem", root / "fixture.key", root / "fixture.csr"
    extensions = root / "server.ext"
    extensions.write_text(
        "basicConstraints=critical,CA:FALSE\nsubjectAltName=IP:127.0.0.1\n"
        "extendedKeyUsage=serverAuth\nkeyUsage=critical,digitalSignature,keyEncipherment\n",
        encoding="ascii",
    )

    def certificate(*args):
        subprocess.run([openssl, *args], check=True, timeout=15, capture_output=True)

    certificate("req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
                "-subj", "/CN=rex-tui ephemeral CI CA", "-addext", "basicConstraints=critical,CA:TRUE",
                "-addext", "keyUsage=critical,keyCertSign,cRLSign", "-keyout", ca_key, "-out", ca)
    certificate("req", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=127.0.0.1",
                "-keyout", key, "-out", request)
    certificate("x509", "-req", "-in", request, "-CA", ca, "-CAkey", ca_key,
                "-set_serial", "1", "-days", "1", "-extfile", extensions, "-out", cert)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(cert, key)
    return context, ca


@contextmanager
def rex_server(rex, root, env):
    with socket.socket() as address:
        address.bind(("127.0.0.1", 0))
        port = address.getsockname()[1]
    endpoint = f"http://127.0.0.1:{port}"
    config = root / "blank.lua"
    config.touch()
    log_path = root / "server.log"
    with log_path.open("wb") as log:
        process = subprocess.Popen(
            [rex, "server", "run", "--listen", endpoint, "--allow-anonymous", "--restore=false",
             "--config", config, "--state-dir", root / "rex-state"],
            env=env, stdout=log, stderr=log,
        )
        try:
            def sessions():
                return json.loads(subprocess.check_output(
                    [rex, "--autostart=false", "--server", endpoint, "api", "call", "session.list", "{}"],
                    env=env, text=True, timeout=5,
                ))["sessions"]

            def ready():
                if process.poll() is not None:
                    raise RuntimeError(f"Rex server exited {process.returncode}")
                try:
                    sessions()
                    return True
                except subprocess.CalledProcessError:
                    return False

            wait_for(ready, "disposable Rex")
            yield port, sessions
        finally:
            if process.poll() is None:
                process.terminate()
            process.wait(timeout=10)
            print(log_path.read_text(encoding="utf-8", errors="replace")[-4000:])


async def tls_relay(port, context):
    async def connect(down_reader, down_writer):
        up_reader, up_writer = await asyncio.open_connection("127.0.0.1", port)

        async def forward(reader, writer):
            while data := await reader.read(65536):
                writer.write(data)
                await writer.drain()

        tasks = [
            asyncio.create_task(forward(down_reader, up_writer)),
            asyncio.create_task(forward(up_reader, down_writer)),
        ]
        try:
            await asyncio.wait(tasks, return_when=asyncio.FIRST_COMPLETED)
        finally:
            for task in tasks:
                task.cancel()
            await asyncio.gather(*tasks, return_exceptions=True)
            up_writer.close()
            down_writer.close()

    return await asyncio.start_server(connect, "127.0.0.1", 0, ssl=context)


def exercise_conpty(frontend, root, profile, sessions, env):
    from winpty import Backend, PtyProcess

    settings = root / "smoke settings.toml"
    settings.write_text('startup_focus = "terminal"\n', encoding="utf-8")
    terminal = PtyProcess.spawn(
        [str(frontend), "--config", str(profile), "--settings", str(settings)],
        env=env, dimensions=(30, 120), backend=Backend.ConPTY,
    )
    chunks = deque(maxlen=1500)

    def collect():
        try:
            while terminal.isalive():
                chunks.append(terminal.read(4096))
        except (EOFError, OSError):
            pass

    output = threading.Thread(target=collect, daemon=True)
    output.start()
    try:
        screen = lambda text: text in "".join(chunks.copy())
        wait_for(lambda: screen("F2 Create session"), "HTTPS connection in rex-tui")
        terminal.write("\x1bOQ")  # F2 creates the disposable session.
        wait_for(sessions, "session created through ConPTY and HTTPS")
        wait_for(lambda: screen("USERPROFILE>"), "disposable PowerShell prompt")
        terminal.write("\x1b[17~")  # F6 focuses the shell.
        wait_for(lambda: screen("Browse"), "terminal input focus")
        marker = root / "command-result.txt"
        escaped_marker = str(marker).replace("'", "''")
        terminal.write(f"[IO.File]::WriteAllText('{escaped_marker}', 'verified')\r")
        def verified_output():
            try:
                return marker.read_text().strip() == "verified"
            except (FileNotFoundError, PermissionError):
                return False

        wait_for(verified_output, "remote shell command")
        terminal.write("\x1b[21~")  # F10 quits the installed frontend.
        wait_for(lambda: not terminal.isalive(), "clean rex-tui exit")
        if terminal.exitstatus != 0 or len(sessions()) != 1:
            raise RuntimeError(f"Unexpected exit status {terminal.exitstatus}")
    except Exception:
        print("".join(chunks.copy())[-7000:].encode("unicode_escape").decode())
        raise
    finally:
        terminal.close(force=True)
        output.join(timeout=3)


async def run_terminal(frontend, root, port, sessions, context, env):
    async with await tls_relay(port, context) as relay:
        https_port = relay.sockets[0].getsockname()[1]
        profile = root / "workspace profile.json"
        profile.write_text(json.dumps({
            "hosts": [{"label": "Private HTTPS", "endpoint": f"https://127.0.0.1:{https_port}"}],
        }), encoding="utf-8")
        await asyncio.to_thread(exercise_conpty, frontend, root, profile, sessions, env)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("installer", type=Path)
    parser.add_argument("archive", type=Path)
    parser.add_argument("rex", type=Path)
    parser.add_argument("--openssl", default="openssl")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="rex-tui-windows-") as temp:
        root = Path(temp)
        env = {key: value for key, value in os.environ.items() if not key.startswith(("REX_", "HERDR_"))}
        for name in ("APPDATA", "LOCALAPPDATA", "USERPROFILE", "HOME", "XDG_CONFIG_HOME",
                     "XDG_STATE_HOME", "XDG_CACHE_HOME"):
            path = root / name
            path.mkdir()
            env[name] = str(path)
        installed = install_pair(args.installer.resolve(), args.archive.resolve(), root, env)
        context, ca = test_certificate(root, args.openssl)
        # rustls-native-certs honors this process-local trust bundle on Windows.
        env.pop("SSL_CERT_DIR", None)
        env["SSL_CERT_FILE"] = str(ca)
        with rex_server(args.rex.resolve(), root, env) as (port, sessions):
            asyncio.run(run_terminal(installed / "rex-tui.exe", root, port, sessions, context, env))
    print("PowerShell installation, package tamper refusal, HTTPS, ConPTY input and clean exit passed.")


if __name__ == "__main__":
    main()
