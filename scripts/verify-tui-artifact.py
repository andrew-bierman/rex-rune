#!/usr/bin/env python3
"""Check a cargo-dist manifest, archive checksum, and both packaged executables."""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
GLIBC_BASELINE = (2, 35)


def package_version(manifest_path):
    with manifest_path.open("rb") as manifest:
        return tomllib.load(manifest)["package"]["version"]


def check_glibc(executable):
    output = subprocess.check_output(["readelf", "--version-info", executable], text=True)
    versions = {(int(major), int(minor)) for major, minor in re.findall(r"GLIBC_(\d+)\.(\d+)", output)}
    if not versions:
        raise ValueError(f"{executable.name} has no recorded glibc version")
    required = ".".join(map(str, max(versions)))
    baseline = ".".join(map(str, GLIBC_BASELINE))
    if "GLIBC_PRIVATE" in output or max(versions) > GLIBC_BASELINE:
        raise ValueError(f"{executable.name} requires glibc {required}, outside baseline {baseline}")
    print(f"{executable.name} requires glibc {required} (baseline {baseline})")


def check_cli(directory, frontend, backend, target, version):
    front = directory / frontend
    back = directory / backend
    commands = (
        ([front, "--help"], "--config"),
        ([front, "--version"], f"rex-tui {version}"),
        ([back, "--help"], "--config"),
        ([back, "--version"], f"rex-rune {package_version(ROOT / 'Cargo.toml')}"),
    )
    for command, expected in commands:
        output = subprocess.check_output(command, text=True, timeout=15)
        matched = expected in output if command[1] == "--help" else output.strip() == expected
        if not matched:
            raise ValueError(f"{command[0].name} {command[1]} did not show {expected!r}")
    settings_path = directory / "smoke settings.toml"
    with settings_path.open("w", encoding="utf-8") as settings:
        subprocess.run([front, "--print-default-settings"], stdout=settings, check=True, timeout=15)
    subprocess.run([front, "--check-settings", "--settings", settings_path], check=True, timeout=15)
    if target.endswith("-linux-gnu"):
        check_glibc(front)
        check_glibc(back)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("target")
    parser.add_argument("archive", type=Path)
    parser.add_argument("manifest", type=Path)
    args = parser.parse_args()
    version = package_version(ROOT / "tui" / "Cargo.toml")
    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    if manifest["announcement_tag"] != f"rex-tui-v{version}":
        raise ValueError("Manifest tag does not match the rex-tui Cargo version")
    windows = args.target.endswith("-windows-msvc")
    suffix = ".exe" if windows else ""
    frontend, backend = f"rex-tui{suffix}", f"rex-qt-backend{suffix}"
    asset = manifest["artifacts"][args.archive.name]
    checksum_asset = manifest["artifacts"][asset["checksum"]]
    bins = sorted(entry["path"] for entry in asset["assets"] if entry["kind"] == "executable")
    if asset["target_triples"] != [args.target] or bins != sorted([frontend, backend]):
        raise ValueError(f"Manifest does not describe the matching pair for {args.target}")
    checksum, name = args.archive.with_name(checksum_asset["name"]).read_text().split()
    with args.archive.open("rb") as archive:
        actual = hashlib.file_digest(archive, "sha256").hexdigest()
    if actual != checksum or name.lstrip("*") != args.archive.name:
        raise ValueError(f"Checksum verification failed for {args.archive.name}")
    with tempfile.TemporaryDirectory(prefix="rex-tui-artifact-") as temp:
        root = Path(temp)
        if windows:
            with zipfile.ZipFile(args.archive) as archive:
                archive.extractall(root)
        else:
            with tarfile.open(args.archive, "r:xz") as archive:
                archive.extractall(root, filter="data")
        front, = root.rglob(frontend)
        back, = root.rglob(backend)
        if front.parent != back.parent:
            raise ValueError("Frontend and companion backend must be beside each other")
        check_cli(front.parent, frontend, backend, args.target, version)


if __name__ == "__main__":
    main()
